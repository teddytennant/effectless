// The keybindings page that uses most of this is not written yet.
#![allow(dead_code)]

//! Keybindings page logic (spec 06 §11; `KeybindingsSettings.logic.ts`): rows from the
//! effective bindings, groups, sources, conflicts, recording a chord, and the
//! keybindings.json edits (upsert, unassign, remove, reset).

use std::collections::HashMap;

use gpui::{FocusHandle, Keystroke};
use tern_core::settings::{
    DEFAULT_KEYBINDINGS, KeyChord, KeybindingRule, MAX_KEYBINDING_RULES, UNBOUND_COMMANDS,
    command_label, default_keybindings,
};

/// Section titles and the command prefixes they hold, in page order.
pub const GROUPS: &[(&str, &str, &[&str])] = &[
    (
        "navigation",
        "Navigation",
        &[
            "sidebar",
            "rightPanel",
            "commandPalette",
            "filePicker",
            "projectSearch",
            "editor",
        ],
    ),
    ("threads", "Threads", &["thread", "chat", "pullRequest"]),
    ("composer", "Composer", &["composer", "modelPicker"]),
    ("terminal", "Terminal", &["terminal"]),
    ("preview-diff", "Preview & diff", &["preview", "diff"]),
    (
        "appearance",
        "Appearance",
        &["theme", "appearance", "themeEditor"],
    ),
    ("scripts", "Project scripts", &["script"]),
    ("other", "Other", &[]),
];

/// Commands Tern does not ship (preview panel, usage page): not listed.
pub fn is_hidden_command(command: &str) -> bool {
    command.starts_with("preview.") || command.starts_with("usage.")
}

pub fn group_of(command: &str) -> usize {
    let prefix = command.split('.').next().unwrap_or(command);
    GROUPS
        .iter()
        .position(|(_, _, prefixes)| prefixes.contains(&prefix))
        .unwrap_or(GROUPS.len() - 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Default,
    Custom,
    Project,
}

impl Source {
    pub fn badge(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Custom => Some("Custom"),
            Self::Project => Some("Project"),
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Custom => "custom",
            Self::Project => "project",
        }
    }
}

/// One row on the page.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyRow {
    pub command: String,
    /// None for unassigned rows.
    pub key: Option<String>,
    pub when: Option<String>,
    pub source: Source,
    /// The rule this row shows (None for a command with no rule at all).
    pub rule: Option<KeybindingRule>,
}

impl KeyRow {
    /// Stable id for drafts and focus.
    pub fn id(&self) -> String {
        format!(
            "{}|{}|{}",
            self.command,
            self.key.as_deref().unwrap_or(""),
            self.when.as_deref().unwrap_or("")
        )
    }

    pub fn label(&self) -> String {
        command_label(&self.command)
    }

    pub fn has_default(&self) -> bool {
        DEFAULT_KEYBINDINGS
            .iter()
            .any(|(c, _, _)| *c == self.command)
    }
}

/// Canonical key text: modifiers in `mod ctrl alt shift meta` order, then the key.
pub fn normalize_key(key: &str) -> String {
    match KeyChord::parse(key) {
        Some(c) => {
            let mut parts: Vec<String> = Vec::new();
            if c.mod_key {
                parts.push("mod".into());
            }
            if c.ctrl {
                parts.push("ctrl".into());
            }
            if c.alt {
                parts.push("alt".into());
            }
            if c.shift {
                parts.push("shift".into());
            }
            if c.meta {
                parts.push("meta".into());
            }
            parts.push(c.key);
            parts.join("+")
        }
        None => key.trim().to_lowercase(),
    }
}

pub fn normalize_when(when: Option<&str>) -> String {
    when.map(|w| w.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

fn same_rule(a: &KeybindingRule, b: &KeybindingRule) -> bool {
    a.command == b.command
        && normalize_key(&a.key) == normalize_key(&b.key)
        && normalize_when(a.when.as_deref()) == normalize_when(b.when.as_deref())
}

pub fn source_of(rule: &KeybindingRule) -> Source {
    if rule.command.starts_with("script.") {
        return Source::Project;
    }
    let is_default = default_keybindings().iter().any(|d| same_rule(d, rule));
    if is_default {
        Source::Default
    } else {
        Source::Custom
    }
}

fn unassigned(rule: &KeybindingRule) -> bool {
    rule.when.as_deref().map(str::trim) == Some("false")
}

/// Rows for the effective bindings, plus `Unassigned` rows for commands without one,
/// sorted by command then key.
pub fn rows(effective: &[KeybindingRule]) -> Vec<KeyRow> {
    let mut out: Vec<KeyRow> = effective
        .iter()
        .filter(|r| !is_hidden_command(&r.command))
        .map(|r| KeyRow {
            command: r.command.clone(),
            key: (!unassigned(r)).then(|| r.key.clone()),
            when: r.when.clone(),
            source: source_of(r),
            rule: Some(r.clone()),
        })
        .collect();
    for command in UNBOUND_COMMANDS {
        if !effective.iter().any(|r| r.command == *command) {
            out.push(KeyRow {
                command: (*command).to_string(),
                key: None,
                when: None,
                source: Source::Default,
                rule: None,
            });
        }
    }
    out.sort_by(|a, b| {
        a.command.cmp(&b.command).then_with(|| {
            a.key
                .as_deref()
                .unwrap_or("")
                .cmp(b.key.as_deref().unwrap_or(""))
        })
    });
    out
}

/// Search over command id, label, key, when and source word.
pub fn matches(row: &KeyRow, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let hay = format!(
        "{} {} {} {} {}",
        row.command,
        row.label(),
        row.key.as_deref().unwrap_or(""),
        row.when.as_deref().unwrap_or(""),
        row.source.word()
    )
    .to_lowercase();
    q.split_whitespace().all(|w| hay.contains(w))
}

/// A chord as (ctrl, alt, shift, meta, key) after resolving `mod` for the platform.
fn resolved(key: &str, mac: bool) -> Option<(bool, bool, bool, bool, String)> {
    let c = KeyChord::parse(key)?;
    let ctrl = c.ctrl || (c.mod_key && !mac);
    let meta = c.meta || (c.mod_key && mac);
    Some((ctrl, c.alt, c.shift, meta, c.key))
}

/// Two `when`s can both hold: either is empty, or they are the same text.
/// `false` never overlaps.
fn whens_overlap(a: Option<&str>, b: Option<&str>) -> bool {
    let a = normalize_when(a);
    let b = normalize_when(b);
    if a == "false" || b == "false" {
        return false;
    }
    a.is_empty() || b.is_empty() || a == b
}

/// Commands of other rows with the same resolved chord whose `when` can overlap.
pub fn conflicts(row: &KeyRow, all: &[KeyRow], mac: bool) -> Vec<String> {
    let Some(key) = &row.key else {
        return Vec::new();
    };
    let Some(mine) = resolved(key, mac) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for other in all {
        if other.id() == row.id() {
            continue;
        }
        let Some(k) = &other.key else { continue };
        if resolved(k, mac).as_ref() == Some(&mine)
            && whens_overlap(row.when.as_deref(), other.when.as_deref())
        {
            let label = other.label();
            if !out.contains(&label) {
                out.push(label);
            }
        }
    }
    out
}

/// `Conflicts with {a, b, c}, and more. The most recent ...` (KSL:219-246).
pub fn conflict_message(labels: &[String]) -> String {
    let head = match labels.len() {
        0 => return String::new(),
        1..=3 => format!("Conflicts with {}.", labels.join(", ")),
        _ => format!("Conflicts with {}, and more.", labels[..3].join(", ")),
    };
    format!("{head} The most recent matching binding wins when both conditions can apply.")
}

/// A recorded keystroke as key text, or None to keep recording (modifier only) or
/// for keys that cannot be bound. Linux: Ctrl records as `mod`, Super as `meta`.
pub fn record(ks: &Keystroke, mac: bool) -> Option<String> {
    let key = match ks.key.as_str() {
        "up" => "arrowup".to_string(),
        "down" => "arrowdown".to_string(),
        "left" => "arrowleft".to_string(),
        "right" => "arrowright".to_string(),
        "escape" => "esc".to_string(),
        "space" => "space".to_string(),
        k @ ("enter" | "tab" | "backspace" | "delete" | "home" | "end" | "pageup" | "pagedown") => {
            k.to_string()
        }
        k if k.len() > 1
            && k.starts_with('f')
            && k[1..].chars().all(|c| c.is_ascii_digit())
            && k[1..].parse::<u32>().is_ok_and(|n| (1..=99).contains(&n)) =>
        {
            k.to_string()
        }
        k if k.chars().count() == 1 => {
            let ch = k.chars().next()?;
            if ch == '+' {
                "+".into()
            } else {
                ch.to_lowercase().collect()
            }
        }
        _ => return None,
    };
    let m = &ks.modifiers;
    let mut parts: Vec<&str> = Vec::new();
    if mac {
        if m.platform {
            parts.push("mod");
        }
        if m.control {
            parts.push("ctrl");
        }
    } else {
        if m.control {
            parts.push("mod");
        }
        if m.platform {
            parts.push("meta");
        }
    }
    if m.alt {
        parts.push("alt");
    }
    if m.shift {
        parts.push("shift");
    }
    parts.push(&key);
    Some(parts.join("+"))
}

/// Kbd labels for a key (`KeybindingPill`): mod `Ctrl`/`⌘`, shift `⇧`, alt `Alt`/`⌥`,
/// ctrl `⌃`, then the key (single characters uppercased).
pub fn kbd_parts(key: &str, mac: bool) -> Vec<String> {
    let Some(c) = KeyChord::parse(key) else {
        return vec![key.to_string()];
    };
    let mut out = Vec::new();
    if c.mod_key {
        out.push(if mac { "⌘" } else { "Ctrl" }.to_string());
    }
    if c.meta {
        out.push("meta".to_string());
    }
    if c.ctrl {
        out.push("⌃".to_string());
    }
    if c.alt {
        out.push(if mac { "⌥" } else { "Alt" }.to_string());
    }
    if c.shift {
        out.push("⇧".to_string());
    }
    let k = match c.key.as_str() {
        "escape" => "esc".to_string(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.to_string(),
    };
    out.push(k);
    out
}

pub const NO_ROOM: &str = "Not enough room to save this shortcut and its inherited shortcuts.";

/// Writes `rule`, dropping entries equal to it or to `replace`. When this is the
/// command's first rule in the file, its other defaults are written too so they
/// survive (a file rule hides every default of its command).
pub fn upsert(
    file: &[KeybindingRule],
    rule: KeybindingRule,
    replace: Option<&KeybindingRule>,
) -> Result<Vec<KeybindingRule>, String> {
    let mut out: Vec<KeybindingRule> = file
        .iter()
        .filter(|r| !same_rule(r, &rule) && replace.is_none_or(|p| !same_rule(r, p)))
        .cloned()
        .collect();
    let first = !file.iter().any(|r| r.command == rule.command);
    if first {
        for d in default_keybindings() {
            if d.command == rule.command
                && !same_rule(&d, &rule)
                && replace.is_none_or(|p| !same_rule(&d, p))
            {
                out.push(d);
            }
        }
    }
    out.push(rule);
    if out.len() > MAX_KEYBINDING_RULES {
        return Err(NO_ROOM.to_string());
    }
    Ok(out)
}

/// `Remove` on a custom row: drop that rule.
pub fn remove(file: &[KeybindingRule], rule: &KeybindingRule) -> Vec<KeybindingRule> {
    file.iter()
        .filter(|r| !same_rule(r, rule))
        .cloned()
        .collect()
}

/// `Reset to default`: drop every file rule of the command so its defaults return.
pub fn reset_command(file: &[KeybindingRule], command: &str) -> Vec<KeybindingRule> {
    file.iter()
        .filter(|r| r.command != command)
        .cloned()
        .collect()
}

/// `Unassign` on a default row: the same key with `when: "false"`.
pub fn unassign(
    file: &[KeybindingRule],
    rule: &KeybindingRule,
) -> Result<Vec<KeybindingRule>, String> {
    upsert(
        file,
        KeybindingRule::new(&rule.key, &rule.command, Some("false")),
        Some(rule),
    )
}

/// Every command a new binding can target, with its label, sorted by label.
pub fn all_commands() -> Vec<(String, String)> {
    let mut seen: Vec<String> = Vec::new();
    for (c, _, _) in DEFAULT_KEYBINDINGS {
        if !is_hidden_command(c) && !seen.iter().any(|s| s == c) {
            seen.push((*c).to_string());
        }
    }
    for c in UNBOUND_COMMANDS {
        if !seen.iter().any(|s| s == c) {
            seen.push((*c).to_string());
        }
    }
    let mut out: Vec<(String, String)> = seen
        .into_iter()
        .map(|c| {
            let label = command_label(&c);
            (c, label)
        })
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Page state that outlives a render.
#[derive(Default)]
pub struct KeysState {
    pub query: String,
    /// The row id being recorded (or `"new"` for the add row).
    pub recording: Option<String>,
    /// Recorded keys waiting for Save, by row id.
    pub drafts: HashMap<String, String>,
    pub focus: HashMap<String, FocusHandle>,
    /// The add-keybinding row: (command, recorded key).
    pub adding: Option<(Option<String>, Option<String>)>,
    pub when_errors: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Modifiers;

    fn rule(key: &str, command: &str, when: Option<&str>) -> KeybindingRule {
        KeybindingRule::new(key, command, when)
    }

    #[test]
    fn groups_follow_prefixes() {
        assert_eq!(GROUPS[group_of("sidebar.toggle")].1, "Navigation");
        assert_eq!(GROUPS[group_of("chat.new")].1, "Threads");
        assert_eq!(GROUPS[group_of("diff.toggle")].1, "Preview & diff");
        assert_eq!(GROUPS[group_of("script.test.run")].1, "Project scripts");
        assert_eq!(GROUPS[group_of("workspace.focusNext")].1, "Other");
    }

    #[test]
    fn default_rows_include_unassigned_commands_and_skip_hidden() {
        let r = rows(&default_keybindings());
        assert!(r.iter().all(|r| !r.command.starts_with("preview.")));
        let stop = r.iter().find(|r| r.command == "thread.stop").unwrap();
        assert_eq!(stop.key, None);
        assert!(r.iter().all(|r| r.source == Source::Default));
        // sorted by command
        let first = r
            .iter()
            .position(|r| r.command == "appearance.cycle")
            .unwrap();
        assert_eq!(first, 0);
    }

    #[test]
    fn source_labels() {
        assert_eq!(
            source_of(&rule("mod+b", "sidebar.toggle", None)),
            Source::Default
        );
        assert_eq!(
            source_of(&rule("mod+e", "sidebar.toggle", None)),
            Source::Custom
        );
        assert_eq!(
            source_of(&rule("mod+e", "script.test.run", None)),
            Source::Project
        );
    }

    #[test]
    fn conflicts_need_same_chord_and_overlapping_when() {
        let all = rows(&default_keybindings());
        let diff = all
            .iter()
            .find(|r| r.command == "diff.toggle")
            .unwrap()
            .clone();
        // diff.toggle (mod+d, !terminalFocus) vs terminal.split (mod+d, terminalFocus):
        // different whens, no conflict.
        assert!(conflicts(&diff, &all, false).is_empty());
        let mut custom = all.clone();
        custom.push(KeyRow {
            command: "sidebar.toggle".into(),
            key: Some("ctrl+d".into()),
            when: None,
            source: Source::Custom,
            rule: None,
        });
        let c = conflicts(&diff, &custom, false);
        assert_eq!(c, vec!["Sidebar: Toggle".to_string()]);
        // On macOS mod is Cmd, so ctrl+d no longer collides.
        assert!(conflicts(&diff, &custom, true).is_empty());
    }

    #[test]
    fn false_when_never_conflicts() {
        assert!(!whens_overlap(Some("false"), None));
        assert!(whens_overlap(None, Some("x")));
        assert!(whens_overlap(Some("a  && b"), Some("a && b")));
        assert!(!whens_overlap(Some("a"), Some("b")));
    }

    #[test]
    fn conflict_text() {
        let l = |n: usize| (0..n).map(|i| format!("C{i}")).collect::<Vec<_>>();
        assert!(conflict_message(&l(1)).starts_with("Conflicts with C0. The most"));
        assert!(conflict_message(&l(5)).starts_with("Conflicts with C0, C1, C2, and more."));
        assert_eq!(conflict_message(&[]), "");
    }

    #[test]
    fn records_chords() {
        let ks = |key: &str, m: Modifiers| Keystroke {
            modifiers: m,
            key: key.into(),
            key_char: None,
        };
        let ctrl_shift = Modifiers {
            control: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            record(&ks("k", ctrl_shift), false).as_deref(),
            Some("mod+shift+k")
        );
        assert_eq!(
            record(&ks("up", Modifiers::default()), false).as_deref(),
            Some("arrowup")
        );
        assert_eq!(
            record(&ks("f6", Modifiers::default()), false).as_deref(),
            Some("f6")
        );
        assert_eq!(record(&ks("shift", Modifiers::default()), false), None);
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        assert_eq!(record(&ks("1", shift), false).as_deref(), Some("shift+1"));
    }

    #[test]
    fn kbd_labels() {
        assert_eq!(kbd_parts("mod+shift+f", false), vec!["Ctrl", "⇧", "F"]);
        assert_eq!(kbd_parts("mod+alt+b", false), vec!["Ctrl", "Alt", "B"]);
        assert_eq!(kbd_parts("f6", false), vec!["f6"]);
    }

    #[test]
    fn first_custom_rule_keeps_other_defaults() {
        let file: Vec<KeybindingRule> = Vec::new();
        let old = rule("mod+n", "chat.new", Some("!terminalFocus"));
        let new = rule("mod+e", "chat.new", Some("!terminalFocus"));
        let out = upsert(&file, new.clone(), Some(&old)).unwrap();
        // chat.new has two defaults: mod+n (replaced) and mod+shift+o (kept).
        assert_eq!(out.len(), 2);
        assert!(out.iter().any(|r| r.key == "mod+shift+o"));
        assert_eq!(out.last(), Some(&new));
        // Second edit of the same command does not re-add defaults.
        let again = upsert(&out, rule("mod+y", "chat.new", None), Some(&new)).unwrap();
        assert_eq!(again.len(), 2);
        assert!(!again.iter().any(|r| r.key == "mod+e"));
    }

    #[test]
    fn unassign_remove_and_reset() {
        let d = rule("mod+b", "sidebar.toggle", None);
        let out = unassign(&[], &d).unwrap();
        assert_eq!(out, vec![rule("mod+b", "sidebar.toggle", Some("false"))]);
        let merged = tern_core::settings::merge_keybindings(&out);
        let r = rows(&merged);
        let row = r.iter().find(|r| r.command == "sidebar.toggle").unwrap();
        assert_eq!(row.key, None);
        assert!(reset_command(&out, "sidebar.toggle").is_empty());
        assert!(remove(&out, &out[0]).is_empty());
    }

    #[test]
    fn normalizes_modifier_order() {
        assert_eq!(normalize_key("shift+mod+K"), "mod+shift+k");
        assert_eq!(normalize_when(Some(" a  &&  b ")), "a && b");
    }
}

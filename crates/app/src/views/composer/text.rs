//! Prompt text logic (spec 04 §4, §9): trigger detection, the slash command list
//! and its ranking, file-link chips, whole-message commands. Offsets are UTF-8 byte
//! offsets into the prompt string.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerKind {
    Slash,
    Path,
    Skill,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trigger {
    pub kind: TriggerKind,
    pub query: String,
    pub range: Range<usize>,
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\n' | '\t' | '\r')
}

fn is_currency(c: char) -> bool {
    matches!(
        c,
        '$' | '€' | '£' | '¥' | '₩' | '₹' | '₽' | '₺' | '₫' | '₪' | '¢' | '₦' | '₱' | '฿'
    )
}

/// `$5`, `€9.99` and `$10k` are money, not skills.
fn is_currency_amount(query: &str) -> bool {
    let q = query.strip_prefix('.').unwrap_or(query);
    let mut chars = q.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    let rest: String = chars.collect();
    let rest = rest.trim_end_matches(['.', ',', ';', ':', '!', '?', ')']);
    let body =
        rest.trim_start_matches(|c: char| c.is_ascii_digit() || matches!(c, ',' | '.' | '_'));
    body.is_empty()
        || (body.len() == 1 && "kKmMbBtT".contains(body))
        || (body.starts_with(['e', 'E']) && body[1..].chars().all(|c| c.is_ascii_digit()))
}

/// The trigger for the run of non-whitespace before the caret (`composerTrigger.ts`).
pub fn detect_trigger(text: &str, cursor: usize) -> Option<Trigger> {
    let cursor = cursor.min(text.len());
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let before = &text[..cursor];
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| is_ws(*c))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let token = &text[start..cursor];
    let mut chars = token.chars();
    let first = chars.next()?;
    let rest = chars.as_str();
    if first == '/' {
        // A second slash or backslash anywhere in the word makes it a path.
        let word_end = text[cursor..]
            .char_indices()
            .find(|(_, c)| is_ws(*c))
            .map(|(i, _)| cursor + i)
            .unwrap_or(text.len());
        if text[start + 1..word_end].contains(['/', '\\']) {
            return None;
        }
        return Some(Trigger {
            kind: TriggerKind::Slash,
            query: rest.to_string(),
            range: start..cursor,
        });
    }
    if is_currency(first) {
        if is_currency_amount(rest) {
            return None;
        }
        return Some(Trigger {
            kind: TriggerKind::Skill,
            query: rest.to_string(),
            range: start..cursor,
        });
    }
    if first == '@' {
        return Some(Trigger {
            kind: TriggerKind::Path,
            query: rest.to_string(),
            range: start..cursor,
        });
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommandSource {
    BuiltIn,
    Provider,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub source: CommandSource,
    /// Only offered when the token starts the prompt.
    pub prompt_start_only: bool,
}

/// The built-in commands in menu order (`useComposerMenuItems.ts:256-333`).
pub fn builtin_commands(has_thread: bool) -> Vec<SlashCommand> {
    let mut list = vec![
        SlashCommand {
            name: "model",
            description: "Switch response model for this thread",
            source: CommandSource::BuiltIn,
            prompt_start_only: false,
        },
        SlashCommand {
            name: "side",
            description: "Ask a side question in a new side chat",
            source: CommandSource::BuiltIn,
            prompt_start_only: true,
        },
        SlashCommand {
            name: "btw",
            description: "Same as /side",
            source: CommandSource::BuiltIn,
            prompt_start_only: true,
        },
    ];
    if has_thread {
        list.push(SlashCommand {
            name: "clear",
            description: "Start a fresh session; earlier messages stay visible",
            source: CommandSource::BuiltIn,
            prompt_start_only: true,
        });
    }
    list
}

/// Provider commands Tern knows each driver ships (the CLIs do not report them).
pub fn provider_commands(driver: tern_core::Driver) -> Vec<SlashCommand> {
    let cmd = |name, description| SlashCommand {
        name,
        description,
        source: CommandSource::Provider,
        prompt_start_only: true,
    };
    match driver {
        tern_core::Driver::Codex => vec![
            cmd("compact", "Summarize the conversation to free up context"),
            cmd("review", "Review the current changes"),
            cmd("goal", "Set a goal for the agent to work toward"),
            cmd("feedback", "Send feedback about Codex"),
        ],
        tern_core::Driver::Claude => vec![
            cmd(
                "compact",
                "Clear conversation history but keep a summary in context",
            ),
            cmd("review", "Review a pull request"),
            cmd(
                "init",
                "Initialize a new CLAUDE.md file with codebase documentation",
            ),
            cmd("pr-comments", "Get comments from a GitHub pull request"),
            cmd(
                "security-review",
                "Complete a security review of the pending changes",
            ),
        ],
        tern_core::Driver::Acp => vec![],
    }
}

fn name_score(name: &str, query: &str) -> Option<u32> {
    if name == query {
        return Some(0);
    }
    if name.starts_with(query) {
        return Some(2);
    }
    if let Some(i) = name.find(query)
        && i > 0
    {
        let before = name[..i].chars().last();
        return Some(if before.is_some_and(|c| matches!(c, '-' | '_' | '/')) {
            4
        } else {
            6
        });
    }
    if super::model::is_subsequence(query, name) {
        return Some(100 + (name.len().saturating_sub(query.len())) as u32);
    }
    None
}

fn description_score(desc: &str, query: &str) -> Option<u32> {
    let desc = desc.to_lowercase();
    if desc.starts_with(query) {
        return Some(20);
    }
    let mut offset = 0;
    for word in desc.split(|c: char| !c.is_alphanumeric()) {
        if word.starts_with(query) {
            return Some(if offset == 0 { 20 } else { 22 });
        }
        offset += word.len() + 1;
    }
    desc.contains(query).then_some(26)
}

/// Ranks commands for a slash query (`composerSlashCommandSearch.ts`). Empty query
/// keeps list order.
pub fn rank_commands(commands: Vec<SlashCommand>, query: &str) -> Vec<SlashCommand> {
    let query = query.trim_start_matches('/').to_lowercase();
    if query.is_empty() {
        return commands;
    }
    let mut scored: Vec<(u32, SlashCommand)> = commands
        .into_iter()
        .filter_map(|c| {
            let best = [
                name_score(c.name, &query),
                description_score(c.description, &query),
            ]
            .into_iter()
            .flatten()
            .min()?;
            Some((best, c))
        })
        .collect();
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.source.cmp(&b.1.source))
            .then(a.1.name.cmp(b.1.name))
    });
    scored.into_iter().map(|(_, c)| c).collect()
}

/// `/clear` as the whole message arms a fresh session instead of sending.
pub fn is_clear_command(text: &str) -> bool {
    text.trim().eq_ignore_ascii_case("/clear")
}

/// `/side [prompt]` or `/btw [prompt]` as the whole message.
pub fn parse_side_command(text: &str) -> Option<String> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    for cmd in ["/side", "/btw"] {
        if let Some(rest) = lower.strip_prefix(cmd)
            && (rest.is_empty() || rest.starts_with(char::is_whitespace))
        {
            return Some(t[cmd.len()..].trim().to_string());
        }
    }
    None
}

// ---- file links and chips ----

fn escape_label(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    for c in label.chars() {
        if matches!(c, '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn encode_destination(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            _ => out.push(c),
        }
    }
    out
}

fn decode_destination(dest: &str) -> String {
    dest.replace("%20", " ")
        .replace("%28", "(")
        .replace("%29", ")")
        .replace("%3C", "<")
        .replace("%3E", ">")
}

pub fn basename(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

/// `[basename](path)`; a `./` prefix keeps relative paths that start oddly from
/// parsing as something else.
pub fn serialize_file_link(path: &str) -> String {
    let label = escape_label(basename(path));
    let dest = encode_destination(path);
    let needs_dot = !path.starts_with('/')
        && !path.starts_with("./")
        && !path.starts_with("../")
        && (path.starts_with('~') || path.starts_with('#') || path.contains(':'));
    if needs_dot {
        format!("[{label}](./{dest})")
    } else {
        format!("[{label}]({dest})")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChipKind {
    File { label: String, path: String },
    Skill { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChipSpan {
    pub range: Range<usize>,
    pub kind: ChipKind,
}

/// The Markdown file links and `$skill` words the editor shows as chips.
pub fn chip_spans(text: &str, skills: &[String]) -> Vec<ChipSpan> {
    let mut spans = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(span) = parse_link(text, i) {
                i = span.range.end;
                spans.push(span);
                continue;
            }
        } else if bytes[i] == b'$' && (i == 0 || is_ws(text[..i].chars().last().unwrap_or(' '))) {
            let end = text[i + 1..]
                .char_indices()
                .find(|(_, c)| is_ws(*c))
                .map(|(j, _)| i + 1 + j)
                .unwrap_or(text.len());
            let name = &text[i + 1..end];
            if !name.is_empty() && skills.iter().any(|s| s == name) {
                spans.push(ChipSpan {
                    range: i..end,
                    kind: ChipKind::Skill {
                        name: name.to_string(),
                    },
                });
                i = end;
                continue;
            }
        }
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    spans
}

fn parse_link(text: &str, start: usize) -> Option<ChipSpan> {
    let rest = &text[start + 1..];
    let mut label = String::new();
    let mut chars = rest.char_indices();
    let mut label_end = None;
    while let Some((j, c)) = chars.next() {
        match c {
            '\\' => {
                let (_, next) = chars.next()?;
                label.push(next);
            }
            ']' => {
                label_end = Some(start + 1 + j);
                break;
            }
            '\n' | '[' => return None,
            _ => label.push(c),
        }
    }
    let label_end = label_end?;
    let after = &text[label_end + 1..];
    let dest_body = after.strip_prefix('(')?;
    let close = dest_body.find([')', '\n', ' '])?;
    if dest_body.as_bytes()[close] != b')' || close == 0 || label.is_empty() {
        return None;
    }
    let dest = &dest_body[..close];
    let end = label_end + 2 + close + 1;
    let path = decode_destination(dest.strip_prefix("./").unwrap_or(dest));
    Some(ChipSpan {
        range: start..end,
        kind: ChipKind::File { label, path },
    })
}

/// Replaces `range` with `replacement`, absorbing one following space when the
/// replacement already ends in one. Returns the new text and caret.
pub fn replace_range(text: &str, range: Range<usize>, replacement: &str) -> (String, usize) {
    let start = range.start.min(text.len());
    let mut end = range.end.clamp(start, text.len());
    if replacement.ends_with(' ') && text[end..].starts_with(' ') {
        end += 1;
    }
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(&text[..start]);
    out.push_str(replacement);
    out.push_str(&text[end..]);
    (out, start + replacement.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_trigger_at_token_start() {
        let t = detect_trigger("/mo", 3).unwrap();
        assert_eq!(t.kind, TriggerKind::Slash);
        assert_eq!(t.query, "mo");
        assert_eq!(t.range, 0..3);
        // A path is not a command.
        assert_eq!(detect_trigger("/src/main.rs", 4), None);
        assert_eq!(
            detect_trigger("see /usr", 8).map(|t| t.kind),
            Some(TriggerKind::Slash)
        );
    }

    #[test]
    fn mention_and_skill_triggers() {
        let t = detect_trigger("look at @src/ma", 15).unwrap();
        assert_eq!(t.kind, TriggerKind::Path);
        assert_eq!(t.query, "src/ma");
        assert_eq!(t.range, 8..15);
        let s = detect_trigger("use $rev", 8).unwrap();
        assert_eq!(s.kind, TriggerKind::Skill);
        assert_eq!(detect_trigger("costs $5", 8), None);
        assert_eq!(detect_trigger("costs $10k.", 11), None);
        assert_eq!(detect_trigger("plain words", 5), None);
        assert_eq!(detect_trigger("a ", 2), None);
    }

    #[test]
    fn ranking_prefers_name_prefix_and_keeps_order_when_empty() {
        let cmds = builtin_commands(true);
        let names: Vec<_> = rank_commands(cmds.clone(), "")
            .iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["model", "side", "btw", "clear"]);
        let ranked = rank_commands(cmds.clone(), "/cl");
        assert_eq!(ranked[0].name, "clear");
        let ranked = rank_commands(cmds, "fresh");
        assert_eq!(ranked[0].name, "clear");
    }

    #[test]
    fn whole_message_commands() {
        assert!(is_clear_command(" /clear "));
        assert!(!is_clear_command("/clear now"));
        assert_eq!(parse_side_command("/btw why?").as_deref(), Some("why?"));
        assert_eq!(parse_side_command("/side").as_deref(), Some(""));
        assert_eq!(parse_side_command("/sidecar"), None);
    }

    #[test]
    fn file_links_round_trip_into_chips() {
        let link = serialize_file_link("src/my file.rs");
        assert_eq!(link, "[my file.rs](src/my%20file.rs)");
        let text = format!("open {link} and $review it");
        let chips = chip_spans(&text, &["review".to_string()]);
        assert_eq!(chips.len(), 2);
        assert_eq!(
            chips[0].kind,
            ChipKind::File {
                label: "my file.rs".into(),
                path: "src/my file.rs".into()
            }
        );
        assert_eq!(&text[chips[0].range.clone()], link);
        assert_eq!(
            chips[1].kind,
            ChipKind::Skill {
                name: "review".into()
            }
        );
        assert_eq!(serialize_file_link("a:b"), "[a:b](./a:b)");
    }

    #[test]
    fn replace_absorbs_following_space() {
        let (text, caret) = replace_range("hi @no there", 3..6, "[no](no) ");
        assert_eq!(text, "hi [no](no) there");
        assert_eq!(caret, 12);
    }
}

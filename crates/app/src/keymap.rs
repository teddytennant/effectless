//! App-wide commands and keybindings (spec 02 §5).
//!
//! The binding table is T3's: the defaults in `tern_core::settings::DEFAULT_KEYBINDINGS`
//! merged with `~/.tern/userdata/keybindings.json` (a command named in the file drops all of
//! its defaults). The file is re-read when it changes.
//!
//! Resolution follows T3's dispatcher, not GPUI's per-element key contexts: on every
//! keystroke the rules are walked from the end, and the first one whose key matches and
//! whose `when` holds wins. If a handler is registered for its command, the handler runs and
//! the keystroke stops there; otherwise the keystroke goes on to GPUI's own bindings (text
//! inputs, menus, dialogs). `when` identifiers come from [`CommandContext`].
//!
//! # Adding a command handler
//!
//! Register once, from the code that owns the behaviour. For a view, register in its
//! constructor with [`on_command_for`]; the handler stops running when the entity is
//! dropped:
//!
//! ```ignore
//! keymap::on_command_for(&cx.entity(), "terminal.split", |this, window, cx| {
//!     this.split(window, cx);
//! }, cx);
//! ```
//!
//! For app-level behaviour with no entity, [`on_command`]:
//!
//! ```ignore
//! keymap::on_command("usage.open", |window, cx| { … }, cx);
//! ```
//!
//! The latest live handler for a command wins. Run a command from a button or menu with
//! [`run`], and show its shortcut with [`shortcut_label`] (`"Ctrl+Shift+N"`).
//!
//! # Context identifiers
//!
//! Focus identifiers are read from the focused element's key-context stack: a view that
//! owns one adds it to its `key_context`, e.g. the terminal drawer uses
//! `.key_context("Terminal terminalFocus")`. The ones T3 knows: `terminalFocus`,
//! `previewFocus`, `composerFocus`, `modelPickerOpen`, `editableFocus` (any GPUI `Input`
//! counts as editable focus too). State identifiers (`terminalOpen`, `rightPanelOpen`,
//! `draftThreadRoute`, `commandPaletteOpen`, `isDesktop`) come from `AppState`. Anything
//! else (`turnRunning`, `composerRequiresModifier`, `previewOpen`, `usagePageOpen`) is
//! added by a provider:
//!
//! ```ignore
//! keymap::add_context_provider(|_window, cx, ctx| {
//!     if composer_turn_running(cx) { ctx.insert("turnRunning"); }
//! }, cx);
//! ```

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use gpui::{App, Context, Entity, Global, Keystroke, WeakEntity, Window};
use tern_core::settings::{
    self, DEFAULT_KEYBINDINGS, KeyChord, KeybindingRule, UNBOUND_COMMANDS, When,
};

use crate::app_state::{AppState, Route};

/// Focus identifiers read from the focused element's key contexts.
const FOCUS_IDENTIFIERS: [&str; 5] = [
    "terminalFocus",
    "previewFocus",
    "composerFocus",
    "modelPickerOpen",
    "editableFocus",
];

/// The identifiers a `when` clause is evaluated against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandContext {
    set: BTreeSet<String>,
}

impl CommandContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// The context labels are shown for: a desktop window with nothing focused or open.
    pub fn base() -> Self {
        let mut ctx = Self::new();
        ctx.insert("isDesktop");
        ctx
    }

    pub fn insert(&mut self, identifier: impl Into<String>) {
        self.set.insert(identifier.into());
    }

    pub fn remove(&mut self, identifier: &str) {
        self.set.remove(identifier);
    }

    pub fn has(&self, identifier: &str) -> bool {
        self.set.contains(identifier)
    }

    fn eval(&self, when: Option<&When>) -> bool {
        when.is_none_or(|w| w.eval(&|id| self.has(id)))
    }
}

/// One effective binding.
#[derive(Clone, Debug)]
struct Rule {
    command: String,
    chord: KeyChord,
    when: Option<When>,
}

impl Rule {
    fn from_settings(rule: &KeybindingRule) -> Option<Self> {
        let when = match &rule.when {
            Some(text) => Some(When::parse(text)?),
            None => None,
        };
        Some(Self {
            command: rule.command.clone(),
            chord: KeyChord::parse(&rule.key)?,
            when,
        })
    }
}

/// A command as the palette and settings list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandInfo {
    pub id: String,
    pub title: String,
    pub shortcut: Option<String>,
}

type Handler = Rc<dyn Fn(&mut Window, &mut App) -> bool>;
type Provider = Rc<dyn Fn(&Window, &App, &mut CommandContext)>;

/// The keymap global. Use the free functions; this is public for tests and settings.
pub struct Keymap {
    rules: Vec<Rule>,
    handlers: HashMap<String, Vec<Handler>>,
    providers: Vec<Provider>,
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    /// Problems from the last read of `keybindings.json` (settings shows them).
    pub issues: Vec<String>,
}

impl Global for Keymap {}

impl Keymap {
    fn from_rules(rules: &[KeybindingRule]) -> Self {
        Self {
            rules: rules.iter().filter_map(Rule::from_settings).collect(),
            handlers: HashMap::new(),
            providers: Vec::new(),
            path: None,
            modified: None,
            issues: Vec::new(),
        }
    }

    fn defaults() -> Self {
        Self::from_rules(&settings::default_keybindings())
    }

    fn load(&mut self) {
        let Some(path) = &self.path else {
            return;
        };
        self.modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let Some(dir) = path.parent() else {
            return;
        };
        let loaded = settings::load_keybindings(dir);
        self.rules = loaded
            .value
            .iter()
            .filter_map(Rule::from_settings)
            .collect();
        self.issues = loaded.unreadable.into_iter().collect();
        self.issues.extend(loaded.dropped_keys);
    }

    /// The command a keystroke runs in `ctx`, if any: the last matching rule wins.
    pub fn resolve(&self, keystroke: &Keystroke, ctx: &CommandContext) -> Option<&str> {
        self.rules
            .iter()
            .rev()
            .find(|r| chord_matches(&r.chord, keystroke) && ctx.eval(r.when.as_ref()))
            .map(|r| r.command.as_str())
    }

    /// The label of the first binding of `command` that is effective in `ctx` (not
    /// shadowed by a later rule on the same key), e.g. `Ctrl+Shift+N`.
    pub fn label(&self, command: &str, ctx: &CommandContext) -> Option<String> {
        self.rules.iter().enumerate().find_map(|(i, r)| {
            if r.command != command || !ctx.eval(r.when.as_ref()) {
                return None;
            }
            let shadowed = self.rules[i + 1..]
                .iter()
                .any(|later| later.chord == r.chord && ctx.eval(later.when.as_ref()));
            (!shadowed).then(|| r.chord.label(false))
        })
    }

    /// Every known command with its title and shortcut, in table order.
    pub fn commands(&self) -> Vec<CommandInfo> {
        let ctx = CommandContext::base();
        let mut ids: Vec<&str> = Vec::new();
        let known = DEFAULT_KEYBINDINGS
            .iter()
            .map(|(c, _, _)| *c)
            .chain(UNBOUND_COMMANDS.iter().copied())
            .chain(self.rules.iter().map(|r| r.command.as_str()));
        for id in known {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids.into_iter()
            .map(|id| CommandInfo {
                id: id.to_string(),
                title: settings::command_label(id),
                shortcut: self.label(id, &ctx),
            })
            .collect()
    }

    fn handler(&self, command: &str) -> Option<Handler> {
        self.handlers.get(command)?.last().cloned()
    }
}

/// Lowercase key names T3 compares against, from a GPUI key.
fn key_candidates(keystroke: &Keystroke) -> Vec<String> {
    let key = keystroke.key.to_lowercase();
    let mut out = vec![match key.as_str() {
        "up" => "arrowup".to_string(),
        "down" => "arrowdown".to_string(),
        "left" => "arrowleft".to_string(),
        "right" => "arrowright".to_string(),
        _ => key.clone(),
    }];
    // The physical key under a shifted symbol on a US layout, so `mod+shift+[` matches
    // whether the platform reports "[" or "{", and `mod+shift+1` matches "!".
    if let Some(base) = unshifted(&key) {
        out.push(base.to_string());
    }
    if let Some(ch) = keystroke.key_char.as_deref().map(str::to_lowercase)
        && !out.contains(&ch)
    {
        out.push(ch);
    }
    out
}

fn unshifted(key: &str) -> Option<&'static str> {
    Some(match key {
        "{" => "[",
        "}" => "]",
        "!" => "1",
        "@" => "2",
        "#" => "3",
        "$" => "4",
        "%" => "5",
        "^" => "6",
        "&" => "7",
        "*" => "8",
        "(" => "9",
        ")" => "0",
        "_" => "-",
        "+" => "=",
        ":" => ";",
        "\"" => "'",
        "<" => ",",
        ">" => ".",
        "?" => "/",
        "|" => "\\",
        "~" => "`",
        _ => return None,
    })
}

/// Exact modifier match (`mod` is Ctrl on Linux) and one of the key candidates.
fn chord_matches(chord: &KeyChord, keystroke: &Keystroke) -> bool {
    let m = keystroke.modifiers;
    let ctrl = chord.ctrl || chord.mod_key;
    if m.control != ctrl || m.alt != chord.alt || m.shift != chord.shift || m.platform != chord.meta
    {
        return false;
    }
    key_candidates(keystroke).iter().any(|k| *k == chord.key)
}

/// The `keybindings.json` the keymap reads. Read only: the engine writes the defaults
/// when the file is missing.
fn keybindings_path() -> Option<PathBuf> {
    Some(tern_engine::Paths::from_env().keybindings())
}

/// Installs the keymap: reads the bindings, starts watching the file, intercepts keystrokes
/// and registers the shell-level handlers (sidebar, terminal, panels, palette, history).
pub fn init(cx: &mut App) {
    let mut keymap = Keymap::defaults();
    keymap.path = keybindings_path();
    keymap.load();
    cx.set_global(keymap);

    cx.intercept_keystrokes(|event, window, cx| {
        if crate::ui::Dialogs::is_open(cx) {
            return;
        }
        let ctx = context(window, cx);
        let Some(command) = cx
            .global::<Keymap>()
            .resolve(&event.keystroke, &ctx)
            .map(str::to_string)
        else {
            return;
        };
        if run(&command, window, cx) {
            cx.stop_propagation();
        }
    })
    .detach();

    // Live reload: T3 watches the file; a once-a-second mtime check is enough here.
    if cx.global::<Keymap>().path.is_some() {
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let changed = cx.update(|cx| {
                    let keymap = cx.global::<Keymap>();
                    let path = keymap.path.clone()?;
                    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                    Some(modified != keymap.modified)
                });
                if changed == Some(true) {
                    cx.update(|cx| {
                        cx.global_mut::<Keymap>().load();
                        cx.refresh_windows();
                    });
                }
            }
        })
        .detach();
    }

    register_shell_commands(cx);
}

/// Shell-level commands that only touch `AppState`.
fn register_shell_commands(cx: &mut App) {
    fn app(cx: &mut App, f: impl FnOnce(&mut AppState, &mut Context<AppState>)) {
        AppState::global(cx).update(cx, f);
    }
    on_command(
        "sidebar.toggle",
        |window, cx| {
            // Refused while the terminal has focus (tmux prefix), spec 02 §1.6.
            if context(window, cx).has("terminalFocus") {
                return;
            }
            app(cx, |s, cx| s.toggle_sidebar(cx));
        },
        cx,
    );
    on_command(
        "terminal.toggle",
        |_, cx| app(cx, |s, cx| s.toggle_drawer(cx)),
        cx,
    );
    on_command(
        "rightPanel.toggle",
        |_, cx| app(cx, |s, cx| s.toggle_right_panel(cx)),
        cx,
    );
    on_command(
        "threadPanel.toggle",
        |_, cx| app(cx, |s, cx| s.toggle_thread_details(cx)),
        cx,
    );
    on_command(
        "commandPalette.toggle",
        |_, cx| app(cx, |s, cx| s.toggle_palette(cx)),
        cx,
    );
    on_command("navigation.back", |_, cx| app(cx, |s, cx| s.back(cx)), cx);
    on_command(
        "navigation.forward",
        |_, cx| app(cx, |s, cx| s.forward(cx)),
        cx,
    );
}

/// Registers an app-level handler for `command`. The latest registration wins.
pub fn on_command(command: &str, f: impl Fn(&mut Window, &mut App) + 'static, cx: &mut App) {
    let handler: Handler = Rc::new(move |window, cx| {
        f(window, cx);
        true
    });
    push_handler(command, handler, cx);
}

/// Registers a handler that runs on `entity` while it is alive. The latest live
/// registration for a command wins.
pub fn on_command_for<T: 'static>(
    entity: &Entity<T>,
    command: &str,
    f: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
    cx: &mut App,
) {
    let weak: WeakEntity<T> = entity.downgrade();
    let handler: Handler =
        Rc::new(move |window, cx| weak.update(cx, |this, cx| f(this, window, cx)).is_ok());
    push_handler(command, handler, cx);
}

fn push_handler(command: &str, handler: Handler, cx: &mut App) {
    if !cx.has_global::<Keymap>() {
        cx.set_global(Keymap::defaults());
    }
    cx.global_mut::<Keymap>()
        .handlers
        .entry(command.to_string())
        .or_default()
        .push(handler);
}

/// Adds identifiers to every [`CommandContext`] built from now on.
pub fn add_context_provider(
    f: impl Fn(&Window, &App, &mut CommandContext) + 'static,
    cx: &mut App,
) {
    if !cx.has_global::<Keymap>() {
        cx.set_global(Keymap::defaults());
    }
    cx.global_mut::<Keymap>().providers.push(Rc::new(f));
}

/// Runs `command`'s handler. Returns false when nothing handled it.
pub fn run(command: &str, window: &mut Window, cx: &mut App) -> bool {
    if !cx.has_global::<Keymap>() {
        return false;
    }
    // Walk back past handlers whose entity is gone.
    let handlers: Vec<Handler> = cx
        .global::<Keymap>()
        .handlers
        .get(command)
        .cloned()
        .unwrap_or_default();
    handlers.iter().rev().any(|h| h(window, cx))
}

/// Whether anything handles `command` right now.
pub fn has_handler(command: &str, cx: &App) -> bool {
    cx.try_global::<Keymap>()
        .is_some_and(|k| k.handler(command).is_some())
}

/// The `when` context for this window right now.
pub fn context(window: &Window, cx: &App) -> CommandContext {
    let mut ctx = CommandContext::base();
    for key_context in window.context_stack() {
        for id in FOCUS_IDENTIFIERS {
            if key_context.contains(id) {
                ctx.insert(id);
            }
        }
        // gpui-component text fields.
        if key_context.contains("Input") {
            ctx.insert("editableFocus");
        }
    }
    {
        let s = AppState::global(cx).read(cx);
        if s.drawer.open {
            ctx.insert("terminalOpen");
        }
        if s.right_panel.open {
            ctx.insert("rightPanelOpen");
        }
        if s.palette_open {
            ctx.insert("commandPaletteOpen");
        }
        if matches!(s.chat_route(), Route::Draft(_) | Route::Index) {
            ctx.insert("draftThreadRoute");
        }
    }
    if let Some(keymap) = cx.try_global::<Keymap>() {
        for provider in keymap.providers.clone() {
            provider(window, cx, &mut ctx);
        }
    }
    ctx
}

/// The shortcut label T3 shows for `command` (its first effective binding in the base
/// context), e.g. `Ctrl+Shift+N`.
pub fn shortcut_label(command: &str, cx: &App) -> Option<String> {
    cx.try_global::<Keymap>()?
        .label(command, &CommandContext::base())
}

/// Every known command with its title and current shortcut.
pub fn commands(cx: &App) -> Vec<CommandInfo> {
    cx.try_global::<Keymap>()
        .map(Keymap::commands)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Modifiers;

    fn ks(text: &str) -> Keystroke {
        Keystroke::parse(text).unwrap()
    }

    fn keymap(user: &[KeybindingRule]) -> Keymap {
        Keymap::from_rules(&settings::merge_keybindings(user))
    }

    #[test]
    fn resolves_defaults_by_context() {
        let k = keymap(&[]);
        let base = CommandContext::base();
        assert_eq!(
            k.resolve(&ks("ctrl-k"), &base),
            Some("commandPalette.toggle")
        );
        assert_eq!(k.resolve(&ks("ctrl-b"), &base), Some("sidebar.toggle"));
        assert_eq!(k.resolve(&ks("ctrl-n"), &base), Some("chat.new"));
        let mut term = CommandContext::base();
        term.insert("terminalFocus");
        assert_eq!(k.resolve(&ks("ctrl-n"), &term), Some("terminal.new"));
        assert_eq!(k.resolve(&ks("ctrl-k"), &term), None);
        assert_eq!(k.resolve(&ks("ctrl-d"), &base), Some("diff.toggle"));
        assert_eq!(k.resolve(&ks("ctrl-d"), &term), Some("terminal.split"));
    }

    #[test]
    fn modifiers_match_exactly() {
        let k = keymap(&[]);
        let base = CommandContext::base();
        // Ctrl+K does not fire with Shift held.
        assert_eq!(k.resolve(&ks("ctrl-shift-k"), &base), None);
        assert_eq!(k.resolve(&ks("ctrl-shift-n"), &base), Some("chat.newLocal"));
        assert_eq!(k.resolve(&ks("k"), &base), None);
    }

    #[test]
    fn shifted_symbols_match_their_key() {
        let k = keymap(&[]);
        let base = CommandContext::base();
        let brace = Keystroke {
            modifiers: Modifiers {
                control: true,
                shift: true,
                ..Default::default()
            },
            key: "{".into(),
            key_char: None,
        };
        assert_eq!(k.resolve(&brace, &base), Some("thread.previous"));
        assert_eq!(k.resolve(&ks("ctrl-shift-]"), &base), Some("thread.next"));
        let bang = Keystroke {
            key: "!".into(),
            ..ks("ctrl-shift-1")
        };
        let mut usage = CommandContext::base();
        usage.insert("usagePageOpen");
        assert_eq!(k.resolve(&bang, &usage), Some("usage.period.day"));
    }

    #[test]
    fn jump_keys_respect_model_picker() {
        let k = keymap(&[]);
        let base = CommandContext::base();
        assert_eq!(k.resolve(&ks("ctrl-3"), &base), Some("thread.jump.3"));
        let mut picker = CommandContext::base();
        picker.insert("modelPickerOpen");
        assert_eq!(
            k.resolve(&ks("ctrl-3"), &picker),
            Some("modelPicker.jump.3")
        );
    }

    #[test]
    fn user_rules_replace_defaults_and_win() {
        let k = keymap(&[KeybindingRule::new(
            "mod+shift+k",
            "commandPalette.toggle",
            None,
        )]);
        let base = CommandContext::base();
        assert_eq!(k.resolve(&ks("ctrl-k"), &base), None);
        assert_eq!(
            k.resolve(&ks("ctrl-shift-k"), &base),
            Some("commandPalette.toggle")
        );
        assert_eq!(
            k.label("commandPalette.toggle", &base).as_deref(),
            Some("Ctrl+Shift+K")
        );
        // "when": "false" unbinds.
        let k = keymap(&[KeybindingRule::new(
            "mod+b",
            "sidebar.toggle",
            Some("false"),
        )]);
        assert_eq!(k.resolve(&ks("ctrl-b"), &base), None);
        assert_eq!(k.label("sidebar.toggle", &base), None);
    }

    #[test]
    fn labels_skip_shadowed_bindings() {
        let k = keymap(&[]);
        let base = CommandContext::base();
        assert_eq!(
            k.label("chat.newLocal", &base).as_deref(),
            Some("Ctrl+Shift+N")
        );
        assert_eq!(k.label("chat.new", &base).as_deref(), Some("Ctrl+N"));
        assert_eq!(
            k.label("filePicker.toggle", &base).as_deref(),
            Some("Ctrl+P")
        );
        assert_eq!(
            k.label("theme.select", &base).as_deref(),
            Some("Ctrl+Alt+A")
        );
        assert_eq!(
            k.label("themeEditor.toggle", &base).as_deref(),
            Some("Ctrl+Alt+Shift+T")
        );
        assert_eq!(k.label("thread.stop", &base), None);
        // A user rule on Ctrl+K for another command shadows the palette's label.
        let k = keymap(&[KeybindingRule::new("mod+k", "usage.open", None)]);
        assert_eq!(k.label("commandPalette.toggle", &base), None);
        assert_eq!(k.label("usage.open", &base).as_deref(), Some("Ctrl+K"));
    }

    #[test]
    fn command_list_has_titles_and_shortcuts() {
        let k = keymap(&[]);
        let list = k.commands();
        let palette = list
            .iter()
            .find(|c| c.id == "commandPalette.toggle")
            .unwrap();
        assert_eq!(palette.title, "Command Palette: Toggle");
        assert_eq!(palette.shortcut.as_deref(), Some("Ctrl+K"));
        assert!(
            list.iter()
                .any(|c| c.id == "thread.stop" && c.shortcut.is_none())
        );
        let ids: BTreeSet<_> = list.iter().map(|c| &c.id).collect();
        assert_eq!(ids.len(), list.len(), "no duplicates");
    }
}

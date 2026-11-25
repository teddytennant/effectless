//! Input mapping for a focused terminal (spec 05 §3.7-3.8): which keys are app
//! shortcuts, which are terminal clipboard/scroll actions, and what reaches the shell;
//! mouse reports for apps that track the mouse; wheel handling.
//!
//! Key bindings run before a terminal sees a key (GPUI matches the keymap first), so
//! app shortcuts win, as in T3. Tern differs on one key: Ctrl+D sends EOF on Linux and
//! Windows instead of splitting, so `terminal.split` keeps only its macOS chord.

use gpui::{App, KeyBinding, NoAction, actions};
use tern_terminal::TerminalModes;

/// The key context of a focused terminal pane (T3's `terminalFocus`).
pub const CONTEXT: &str = "Terminal";

actions!(
    terminal,
    [
        SplitTerminal,
        SplitTerminalVertical,
        NewTerminal,
        CloseTerminal,
        PreviousTerminal,
        NextTerminal,
        Copy,
        Paste,
        SelectAll,
        ScrollPageUp,
        ScrollPageDown,
        ScrollToTop,
        ScrollToBottom,
    ]
);

pub const MAC: bool = cfg!(target_os = "macos");

/// Default chords (GPUI syntax) per command. `terminal.split` is mac only in Tern.
pub fn default_chord(command: &str) -> Option<&'static str> {
    Some(match (command, MAC) {
        ("terminal.split", true) => "cmd-d",
        ("terminal.split", false) => return None,
        ("terminal.splitVertical", true) => "cmd-shift-d",
        ("terminal.splitVertical", false) => "ctrl-shift-d",
        ("terminal.new", true) => "cmd-n",
        ("terminal.new", false) => "ctrl-n",
        ("terminal.close", true) => "cmd-w",
        ("terminal.close", false) => "ctrl-w",
        ("terminal.previous", true) => "cmd-shift-[",
        ("terminal.previous", false) => "ctrl-alt-[",
        ("terminal.next", true) => "cmd-shift-]",
        ("terminal.next", false) => "ctrl-alt-]",
        ("terminal.toggle", true) => "cmd-j",
        ("terminal.toggle", false) => "ctrl-j",
        _ => return None,
    })
}

/// A chord as T3 labels it: `Ctrl+Shift+D` on Linux, `⇧⌘D` on macOS.
pub fn chord_label(chord: &str) -> String {
    let parts: Vec<&str> = chord.split('-').collect();
    let (mods, key) = parts.split_at(parts.len().saturating_sub(1));
    let key = key.first().copied().unwrap_or("");
    let key_label = match key {
        "up" => "Up".to_string(),
        "down" => "Down".to_string(),
        "left" => "Left".to_string(),
        "right" => "Right".to_string(),
        "escape" => "Esc".to_string(),
        "space" => "Space".to_string(),
        "enter" => "Enter".to_string(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => {
            let mut c = k.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    };
    if MAC {
        let mut out = String::new();
        for (m, sym) in [("ctrl", "⌃"), ("alt", "⌥"), ("shift", "⇧"), ("cmd", "⌘")] {
            if mods.contains(&m) {
                out.push_str(sym);
            }
        }
        out + &key_label
    } else {
        let mut out: Vec<String> = Vec::new();
        for (m, label) in [
            ("ctrl", "Ctrl"),
            ("alt", "Alt"),
            ("shift", "Shift"),
            ("cmd", "Meta"),
        ] {
            if mods.contains(&m) {
                out.push(label.to_string());
            }
        }
        out.push(key_label);
        out.join("+")
    }
}

/// The label for a command's default chord, if it has one.
pub fn shortcut_label(command: &str) -> Option<String> {
    default_chord(command).map(chord_label)
}

/// Converts a T3 key spec (`mod+shift+b`, `alt+arrowup`) to a GPUI chord.
pub fn t3_key_to_gpui(spec: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for part in spec.split('+') {
        let p = part.trim().to_ascii_lowercase();
        let mapped = match p.as_str() {
            "mod" => if MAC { "cmd" } else { "ctrl" }.to_string(),
            "cmd" | "meta" => "cmd".to_string(),
            "ctrl" | "control" => "ctrl".to_string(),
            "alt" | "option" => "alt".to_string(),
            "shift" => "shift".to_string(),
            "arrowup" => "up".to_string(),
            "arrowdown" => "down".to_string(),
            "arrowleft" => "left".to_string(),
            "arrowright" => "right".to_string(),
            "esc" => "escape".to_string(),
            "" => return None,
            other => other.to_string(),
        };
        parts.push(mapped);
    }
    // `mod+shift+=` style keys survive; a trailing `+` means the plus key.
    if spec.ends_with("++") {
        parts.push("+".into());
    }
    Some(parts.join("-"))
}

/// Registers the terminal's key bindings. App shortcuts that T3 limits to
/// `!terminalFocus` are bound to `NoAction` in the terminal context, so the keys reach
/// the shell (Ctrl+[ is Escape, Ctrl+K kills a line).
pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    let mut bindings = Vec::new();
    let chord = default_chord;
    if let Some(c) = chord("terminal.split") {
        bindings.push(KeyBinding::new(c, SplitTerminal, ctx));
    }
    if let Some(c) = chord("terminal.splitVertical") {
        bindings.push(KeyBinding::new(c, SplitTerminalVertical, ctx));
    }
    if let Some(c) = chord("terminal.new") {
        bindings.push(KeyBinding::new(c, NewTerminal, ctx));
    }
    if let Some(c) = chord("terminal.close") {
        bindings.push(KeyBinding::new(c, CloseTerminal, ctx));
    }
    if let Some(c) = chord("terminal.previous") {
        bindings.push(KeyBinding::new(c, PreviousTerminal, ctx));
    }
    if let Some(c) = chord("terminal.next") {
        bindings.push(KeyBinding::new(c, NextTerminal, ctx));
    }
    let mut terminal_chords: Vec<&str> = [
        "terminal.split",
        "terminal.splitVertical",
        "terminal.new",
        "terminal.close",
        "terminal.previous",
        "terminal.next",
        "terminal.toggle",
    ]
    .into_iter()
    .filter_map(chord)
    .collect();
    terminal_chords.extend(if MAC {
        ["cmd-c", "cmd-v", "cmd-a", "cmd-c", "cmd-v"]
    } else {
        ["ctrl-shift-c", "ctrl-shift-v", "ctrl-shift-a", "ctrl-insert", "shift-insert"]
    });
    let mut blocked: Vec<String> = vec!["escape".into()];
    for (command, key, when) in tern_core::settings::DEFAULT_KEYBINDINGS {
        let terminal_off =
            when.is_some_and(|w| w.contains("!terminalFocus")) || *command == "sidebar.toggle";
        if let Some(c) = t3_key_to_gpui(key).filter(|_| terminal_off)
            && !terminal_chords.contains(&c.as_str())
            && !blocked.contains(&c)
        {
            blocked.push(c);
        }
    }
    for c in blocked {
        // Bound once at startup; the keymap wants 'static chords.
        bindings.push(KeyBinding::new(c.leak(), NoAction, ctx));
    }
    if MAC {
        bindings.extend([
            KeyBinding::new("cmd-c", Copy, ctx),
            KeyBinding::new("cmd-v", Paste, ctx),
            KeyBinding::new("cmd-a", SelectAll, ctx),
            KeyBinding::new("shift-cmd-home", ScrollToTop, ctx),
            KeyBinding::new("shift-cmd-end", ScrollToBottom, ctx),
        ]);
    } else {
        bindings.extend([
            KeyBinding::new("ctrl-shift-c", Copy, ctx),
            KeyBinding::new("ctrl-insert", Copy, ctx),
            KeyBinding::new("ctrl-shift-v", Paste, ctx),
            KeyBinding::new("shift-insert", Paste, ctx),
            KeyBinding::new("ctrl-shift-a", SelectAll, ctx),
            KeyBinding::new("shift-ctrl-home", ScrollToTop, ctx),
            KeyBinding::new("shift-ctrl-end", ScrollToBottom, ctx),
        ]);
    }
    bindings.extend([
        KeyBinding::new("shift-pageup", ScrollPageUp, ctx),
        KeyBinding::new("shift-pagedown", ScrollPageDown, ctx),
    ]);
    cx.bind_keys(bindings);
}

/// A key press, as the pane sees it after bindings had their chance.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyPress {
    pub key: String,
    pub key_char: Option<String>,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Cmd on macOS, Super elsewhere.
    pub cmd: bool,
}

/// What the pane does with an unbound key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// Ctrl+C with a selection: copy, then clear the selection so the next Ctrl+C is
    /// SIGINT.
    CopyAndClear,
    /// Typed text (layouts, dead keys): written as is.
    Text(String),
    /// Encoded through `Terminal::send_key`.
    Key,
    /// Bare modifiers and other keys with nothing to send.
    Ignore,
}

const MODIFIER_KEYS: [&str; 8] = [
    "shift", "control", "alt", "platform", "function", "capslock", "super", "cmd",
];

pub fn classify_key(press: &KeyPress, has_selection: bool) -> KeyAction {
    if MODIFIER_KEYS.contains(&press.key.as_str()) {
        return KeyAction::Ignore;
    }
    let copy_mod = if MAC { press.cmd } else { press.ctrl };
    if has_selection && copy_mod && !press.alt && !press.shift && press.key.eq_ignore_ascii_case("c") {
        return KeyAction::CopyAndClear;
    }
    if press.cmd && !MAC {
        // Super chords belong to the desktop.
        return KeyAction::Ignore;
    }
    // Plain typing: prefer the layout's character. Alt keeps the ESC prefix through
    // send_key, and Ctrl needs the control byte.
    if !press.ctrl && !press.alt && !press.cmd
        && let Some(text) = press.key_char.as_ref().filter(|t| !t.is_empty())
        && !is_named_key(&press.key)
    {
        return KeyAction::Text(text.clone());
    }
    KeyAction::Key
}

fn is_named_key(key: &str) -> bool {
    matches!(
        key,
        "enter" | "tab" | "backspace" | "escape" | "space" | "delete" | "insert"
    ) || key.chars().count() > 1 && key.chars().all(|c| c.is_ascii_alphanumeric())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButtonCode {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseKind {
    Press,
    Release,
    /// Movement with a button held (`button` is that button) or none (motion mode).
    Motion,
}

/// Encodes a mouse event for an app that enabled mouse reporting (X10/normal/SGR/UTF-8
/// encodings). `cell` is 1-based. Returns None when the current mode doesn't report
/// this kind of event.
pub fn mouse_report(
    button: Option<MouseButtonCode>,
    kind: MouseKind,
    cell: (u16, u16),
    shift: bool,
    alt: bool,
    ctrl: bool,
    modes: TerminalModes,
) -> Option<Vec<u8>> {
    if !modes.mouse_reporting {
        return None;
    }
    if kind == MouseKind::Motion {
        let allowed = modes.mouse_motion || (modes.mouse_drag && button.is_some());
        if !allowed {
            return None;
        }
    }
    let mut code: u32 = match button {
        Some(MouseButtonCode::Left) => 0,
        Some(MouseButtonCode::Middle) => 1,
        Some(MouseButtonCode::Right) => 2,
        Some(MouseButtonCode::WheelUp) => 64,
        Some(MouseButtonCode::WheelDown) => 65,
        None => 3,
    };
    if kind == MouseKind::Motion {
        code += 32;
    }
    if shift {
        code += 4;
    }
    if alt {
        code += 8;
    }
    if ctrl {
        code += 16;
    }
    let (x, y) = (cell.0.max(1) as u32, cell.1.max(1) as u32);
    if modes.sgr_mouse {
        let end = if kind == MouseKind::Release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes());
    }
    // Normal encoding: releases report button 3 and carry no button identity.
    if kind == MouseKind::Release {
        code = (code & !0b11) | 3;
    }
    let mut out = b"\x1b[M".to_vec();
    let mut push = |v: u32| -> Option<()> {
        let v = v + 32;
        if modes.utf8_mouse {
            let c = char::from_u32(v)?;
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        } else {
            out.push(u8::try_from(v).ok()?);
        }
        Some(())
    };
    push(code)?;
    // Coordinates past 223 can't be encoded in the 1-byte form; clamp like xterm.
    let limit = if modes.utf8_mouse { 2015 } else { 223 };
    push(x.min(limit))?;
    push(y.min(limit))?;
    Some(out)
}

/// What a wheel tick of `lines` (positive = up) does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WheelAction {
    /// Move the viewport through the scrollback.
    Scroll(i32),
    /// Bytes for the app: mouse wheel reports, or arrow keys on the alternate screen.
    Send(Vec<u8>),
    None,
}

pub fn wheel_action(lines: i32, cell: (u16, u16), modes: TerminalModes, shift: bool) -> WheelAction {
    if lines == 0 {
        return WheelAction::None;
    }
    if modes.mouse_reporting && !shift {
        let button = if lines > 0 {
            MouseButtonCode::WheelUp
        } else {
            MouseButtonCode::WheelDown
        };
        let one = mouse_report(Some(button), MouseKind::Press, cell, false, false, false, modes)
            .unwrap_or_default();
        return WheelAction::Send(one.repeat(lines.unsigned_abs() as usize));
    }
    if modes.alt_screen {
        let arrow: &[u8] = match (lines > 0, modes.app_cursor) {
            (true, false) => b"\x1b[A",
            (true, true) => b"\x1bOA",
            (false, false) => b"\x1b[B",
            (false, true) => b"\x1bOB",
        };
        return WheelAction::Send(arrow.repeat(lines.unsigned_abs() as usize));
    }
    WheelAction::Scroll(lines)
}

/// Whether a mouse press goes to the app instead of selecting: reporting is on and
/// no Shift/Ctrl/Meta override is held.
pub fn mouse_goes_to_app(modes: TerminalModes, shift: bool, ctrl: bool, cmd: bool) -> bool {
    modes.mouse_reporting && !shift && !ctrl && !cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: &str, key_char: Option<&str>) -> KeyPress {
        KeyPress {
            key: key.into(),
            key_char: key_char.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn typing_uses_key_char() {
        assert_eq!(
            classify_key(&press("a", Some("a")), false),
            KeyAction::Text("a".into())
        );
        let shifted = KeyPress {
            shift: true,
            ..press("a", Some("A"))
        };
        assert_eq!(classify_key(&shifted, false), KeyAction::Text("A".into()));
        assert_eq!(classify_key(&press("é", Some("é")), false), KeyAction::Text("é".into()));
    }

    #[test]
    fn named_keys_encode() {
        assert_eq!(classify_key(&press("enter", Some("\n")), false), KeyAction::Key);
        assert_eq!(classify_key(&press("tab", Some("\t")), false), KeyAction::Key);
        assert_eq!(classify_key(&press("space", Some(" ")), false), KeyAction::Key);
        assert_eq!(classify_key(&press("up", None), false), KeyAction::Key);
        assert_eq!(classify_key(&press("f5", None), false), KeyAction::Key);
        assert_eq!(classify_key(&press("shift", None), false), KeyAction::Ignore);
    }

    #[test]
    fn ctrl_keys_go_to_the_shell() {
        let ctrl = |k: &str| KeyPress {
            ctrl: true,
            ..press(k, None)
        };
        // Ctrl+D is EOF in Tern (no binding claims it off macOS).
        assert_eq!(classify_key(&ctrl("d"), false), KeyAction::Key);
        assert_eq!(classify_key(&ctrl("c"), false), KeyAction::Key);
        let alt = KeyPress {
            alt: true,
            ..press("b", Some("b"))
        };
        assert_eq!(classify_key(&alt, false), KeyAction::Key);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn ctrl_c_copies_only_with_selection() {
        let ctrl_c = KeyPress {
            ctrl: true,
            ..press("c", None)
        };
        assert_eq!(classify_key(&ctrl_c, true), KeyAction::CopyAndClear);
        assert_eq!(classify_key(&ctrl_c, false), KeyAction::Key);
        let ctrl_shift_c = KeyPress {
            shift: true,
            ..ctrl_c
        };
        assert_eq!(classify_key(&ctrl_shift_c, true), KeyAction::Key);
        let sup = KeyPress {
            cmd: true,
            ..press("x", None)
        };
        assert_eq!(classify_key(&sup, false), KeyAction::Ignore);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn linux_chords_and_labels() {
        assert_eq!(default_chord("terminal.split"), None);
        assert_eq!(shortcut_label("terminal.split"), None);
        assert_eq!(shortcut_label("terminal.splitVertical").as_deref(), Some("Ctrl+Shift+D"));
        assert_eq!(shortcut_label("terminal.new").as_deref(), Some("Ctrl+N"));
        assert_eq!(shortcut_label("terminal.close").as_deref(), Some("Ctrl+W"));
        assert_eq!(shortcut_label("terminal.previous").as_deref(), Some("Ctrl+Alt+["));
        assert_eq!(shortcut_label("terminal.toggle").as_deref(), Some("Ctrl+J"));
        assert_eq!(chord_label("ctrl-alt-shift-cmd-j"), "Ctrl+Alt+Shift+Meta+J");
        assert_eq!(chord_label("shift-f6"), "Shift+F6");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn t3_keys_convert() {
        assert_eq!(t3_key_to_gpui("mod+shift+b").as_deref(), Some("ctrl-shift-b"));
        assert_eq!(t3_key_to_gpui("mod+[").as_deref(), Some("ctrl-["));
        assert_eq!(t3_key_to_gpui("alt+arrowup").as_deref(), Some("alt-up"));
        assert_eq!(t3_key_to_gpui("f6").as_deref(), Some("f6"));
        assert_eq!(t3_key_to_gpui("mod+shift+enter").as_deref(), Some("ctrl-shift-enter"));
    }

    fn modes() -> TerminalModes {
        TerminalModes {
            mouse_reporting: true,
            ..Default::default()
        }
    }

    #[test]
    fn sgr_reports() {
        let m = TerminalModes {
            sgr_mouse: true,
            ..modes()
        };
        let r = |b, k, mods: (bool, bool, bool)| {
            mouse_report(b, k, (10, 5), mods.0, mods.1, mods.2, m).map(|v| String::from_utf8(v).unwrap())
        };
        let none = (false, false, false);
        assert_eq!(r(Some(MouseButtonCode::Left), MouseKind::Press, none).as_deref(), Some("\x1b[<0;10;5M"));
        assert_eq!(r(Some(MouseButtonCode::Left), MouseKind::Release, none).as_deref(), Some("\x1b[<0;10;5m"));
        assert_eq!(r(Some(MouseButtonCode::Right), MouseKind::Press, (false, false, true)).as_deref(), Some("\x1b[<18;10;5M"));
        assert_eq!(r(Some(MouseButtonCode::WheelDown), MouseKind::Press, none).as_deref(), Some("\x1b[<65;10;5M"));
        // Drag without drag mode isn't reported.
        assert_eq!(r(Some(MouseButtonCode::Left), MouseKind::Motion, none), None);
        let drag = TerminalModes { mouse_drag: true, ..m };
        assert_eq!(
            mouse_report(Some(MouseButtonCode::Left), MouseKind::Motion, (1, 1), false, false, false, drag).unwrap(),
            b"\x1b[<32;1;1M"
        );
        assert_eq!(mouse_report(None, MouseKind::Motion, (1, 1), false, false, false, drag), None);
        let any = TerminalModes { mouse_motion: true, ..m };
        assert_eq!(
            mouse_report(None, MouseKind::Motion, (1, 1), false, false, false, any).unwrap(),
            b"\x1b[<35;1;1M"
        );
    }

    #[test]
    fn normal_reports() {
        let m = modes();
        assert_eq!(
            mouse_report(Some(MouseButtonCode::Left), MouseKind::Press, (1, 1), false, false, false, m).unwrap(),
            vec![0x1b, b'[', b'M', 32, 33, 33]
        );
        assert_eq!(
            mouse_report(Some(MouseButtonCode::Right), MouseKind::Release, (2, 3), false, false, false, m).unwrap(),
            vec![0x1b, b'[', b'M', 35, 34, 35]
        );
        // Far cells clamp instead of overflowing a byte.
        let far = mouse_report(Some(MouseButtonCode::Left), MouseKind::Press, (500, 1), false, false, false, m).unwrap();
        assert_eq!(far[4], 255);
        assert_eq!(mouse_report(Some(MouseButtonCode::Left), MouseKind::Press, (1, 1), false, false, false, TerminalModes::default()), None);
    }

    #[test]
    fn wheel() {
        let plain = TerminalModes::default();
        assert_eq!(wheel_action(3, (1, 1), plain, false), WheelAction::Scroll(3));
        assert_eq!(wheel_action(0, (1, 1), plain, false), WheelAction::None);
        let alt = TerminalModes {
            alt_screen: true,
            ..plain
        };
        assert_eq!(wheel_action(2, (1, 1), alt, false), WheelAction::Send(b"\x1b[A\x1b[A".to_vec()));
        let app = TerminalModes {
            app_cursor: true,
            ..alt
        };
        assert_eq!(wheel_action(-1, (1, 1), app, false), WheelAction::Send(b"\x1bOB".to_vec()));
        let sgr = TerminalModes {
            mouse_reporting: true,
            sgr_mouse: true,
            alt_screen: true,
            ..plain
        };
        assert_eq!(wheel_action(1, (4, 2), sgr, false), WheelAction::Send(b"\x1b[<64;4;2M".to_vec()));
        // Shift bypasses reporting.
        assert_eq!(wheel_action(1, (4, 2), sgr, true), WheelAction::Send(b"\x1b[A".to_vec()));
    }

    #[test]
    fn mouse_override() {
        assert!(mouse_goes_to_app(modes(), false, false, false));
        assert!(!mouse_goes_to_app(modes(), true, false, false));
        assert!(!mouse_goes_to_app(TerminalModes::default(), false, false, false));
    }
}

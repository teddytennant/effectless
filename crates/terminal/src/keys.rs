//! Key encoding (xterm sequences) and the drawer's primary-screen overrides
//! (spec/05 §3.8). Key names follow GPUI's `Keystroke::key`: `"up"`, `"pageup"`,
//! `"f5"`, `"enter"`, `"a"`, `";"`.

const ESC: u8 = 0x1b;

/// xterm modifier parameter: 1 + shift + 2·alt + 4·ctrl.
fn modifier_param(ctrl: bool, alt: bool, shift: bool) -> u8 {
    1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8
}

fn csi(body: &str) -> Vec<u8> {
    let mut out = vec![ESC, b'['];
    out.extend_from_slice(body.as_bytes());
    out
}

fn with_alt(alt: bool, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    if alt {
        out.push(ESC);
    }
    out.extend_from_slice(bytes);
    out
}

/// The control byte for Ctrl+`c`, following xterm/VT220 rules.
fn ctrl_byte(c: char) -> Option<u8> {
    let c = c.to_ascii_lowercase();
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        '@' | '2' | ' ' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' | '`' | '~' => 0x1e,
        '_' | '7' | '/' | '-' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

/// Encodes a key press as the bytes an xterm-compatible terminal sends.
///
/// `key` is a GPUI key name. Returns `None` when the key has no encoding (a bare
/// modifier, or Ctrl with a character that has no control byte). For plain text the
/// app should prefer `Keystroke::key_char` (IME, keyboard layouts); single-character
/// keys are still encoded here, with Shift upper-casing ASCII letters and Alt adding an
/// ESC prefix.
pub fn key_to_bytes(
    key: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    let mods = modifier_param(ctrl, alt, shift);
    let key_lower = key.to_ascii_lowercase();

    // Cursor keys and Home/End: SS3 in application cursor mode, CSI otherwise, and
    // `CSI 1 ; m X` once a modifier is held.
    let cursor_final = match key_lower.as_str() {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    };
    if let Some(f) = cursor_final {
        return Some(if mods > 1 {
            csi(&format!("1;{mods}{f}"))
        } else if app_cursor {
            vec![ESC, b'O', f as u8]
        } else {
            vec![ESC, b'[', f as u8]
        });
    }

    let tilde = match key_lower.as_str() {
        "insert" => Some(2),
        "delete" => Some(3),
        "pageup" => Some(5),
        "pagedown" => Some(6),
        "f5" => Some(15),
        "f6" => Some(17),
        "f7" => Some(18),
        "f8" => Some(19),
        "f9" => Some(20),
        "f10" => Some(21),
        "f11" => Some(23),
        "f12" => Some(24),
        "f13" => Some(25),
        "f14" => Some(26),
        "f15" => Some(28),
        "f16" => Some(29),
        "f17" => Some(31),
        "f18" => Some(32),
        "f19" => Some(33),
        "f20" => Some(34),
        _ => None,
    };
    if let Some(n) = tilde {
        return Some(if mods > 1 {
            csi(&format!("{n};{mods}~"))
        } else {
            csi(&format!("{n}~"))
        });
    }

    let ss3 = match key_lower.as_str() {
        "f1" => Some('P'),
        "f2" => Some('Q'),
        "f3" => Some('R'),
        "f4" => Some('S'),
        _ => None,
    };
    if let Some(f) = ss3 {
        return Some(if mods > 1 {
            csi(&format!("1;{mods}{f}"))
        } else {
            vec![ESC, b'O', f as u8]
        });
    }

    match key_lower.as_str() {
        "enter" => return Some(with_alt(alt, b"\r")),
        "tab" => {
            return Some(if shift {
                csi("Z")
            } else {
                with_alt(alt, b"\t")
            });
        }
        "backspace" => {
            return Some(with_alt(alt, if ctrl { &[0x08] } else { &[0x7f] }));
        }
        "escape" => return Some(with_alt(alt, &[ESC])),
        "space" => return Some(with_alt(alt, if ctrl { &[0x00] } else { b" " })),
        _ => {}
    }

    let mut chars = key.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        // A named key we don't encode (e.g. a bare modifier).
        return None;
    }
    if ctrl {
        return ctrl_byte(c).map(|b| with_alt(alt, &[b]));
    }
    let c = if shift { c.to_ascii_uppercase() } else { c };
    let mut buf = [0u8; 4];
    Some(with_alt(alt, c.encode_utf8(&mut buf).as_bytes()))
}

/// The drawer's shell-editing shortcuts, applied only on the primary screen so vim and
/// htop get raw keys (spec/05 §3.8). `cmd` is the macOS Command key (GPUI `platform`).
///
/// | key | sends |
/// |---|---|
/// | Alt+← / Alt+→ | `ESC b` / `ESC f` |
/// | Cmd+← / Cmd+→ (mac) | `^A` / `^E` |
/// | Cmd+Backspace (mac) | `^U` |
/// | Ctrl+L, Cmd+K (mac) | `^L` |
///
/// Shift-modified arrows are left alone.
pub fn primary_screen_override(
    key: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    cmd: bool,
) -> Option<Vec<u8>> {
    let key = key.to_ascii_lowercase();
    let mac = cfg!(target_os = "macos");
    let only = |want_ctrl: bool, want_alt: bool, want_cmd: bool| {
        ctrl == want_ctrl && alt == want_alt && cmd == want_cmd && !shift
    };
    let bytes: &[u8] = match key.as_str() {
        "left" if only(false, true, false) => b"\x1bb",
        "right" if only(false, true, false) => b"\x1bf",
        "left" if mac && only(false, false, true) => &[0x01],
        "right" if mac && only(false, false, true) => &[0x05],
        "backspace" if mac && only(false, false, true) => &[0x15],
        "l" if only(true, false, false) => &[0x0c],
        "k" if mac && only(false, false, true) => &[0x0c],
        _ => return None,
    };
    Some(bytes.to_vec())
}

/// Wraps `text` for a paste: bracketed (`ESC[200~ … ESC[201~`, with any ESC in the text
/// removed so it can't end the paste early) when the app enabled mode 2004, otherwise
/// with newlines turned into carriage returns as a typed Enter would send.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let mut out = b"\x1b[200~".to_vec();
        out.extend(text.bytes().filter(|b| *b != ESC));
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(key: &str, ctrl: bool, alt: bool, shift: bool, app: bool) -> Option<Vec<u8>> {
        key_to_bytes(key, ctrl, alt, shift, app)
    }

    /// key, ctrl, alt, shift, app_cursor, bytes
    type Case = (&'static str, bool, bool, bool, bool, &'static [u8]);

    #[test]
    fn key_table() {
        let cases: &[Case] = &[
            // key, ctrl, alt, shift, app_cursor, bytes
            ("up", false, false, false, false, b"\x1b[A"),
            ("down", false, false, false, false, b"\x1b[B"),
            ("right", false, false, false, false, b"\x1b[C"),
            ("left", false, false, false, false, b"\x1b[D"),
            ("up", false, false, false, true, b"\x1bOA"),
            ("left", false, false, false, true, b"\x1bOD"),
            ("home", false, false, false, false, b"\x1b[H"),
            ("end", false, false, false, false, b"\x1b[F"),
            ("home", false, false, false, true, b"\x1bOH"),
            ("end", false, false, false, true, b"\x1bOF"),
            ("up", false, false, true, false, b"\x1b[1;2A"),
            ("right", true, false, false, false, b"\x1b[1;5C"),
            ("left", false, true, false, true, b"\x1b[1;3D"),
            ("home", true, false, true, false, b"\x1b[1;6H"),
            ("pageup", false, false, false, false, b"\x1b[5~"),
            ("pagedown", false, false, false, false, b"\x1b[6~"),
            ("pageup", false, false, true, false, b"\x1b[5;2~"),
            ("insert", false, false, false, false, b"\x1b[2~"),
            ("delete", false, false, false, false, b"\x1b[3~"),
            ("delete", true, false, false, false, b"\x1b[3;5~"),
            ("f1", false, false, false, false, b"\x1bOP"),
            ("f4", false, false, false, false, b"\x1bOS"),
            ("f1", false, false, true, false, b"\x1b[1;2P"),
            ("f5", false, false, false, false, b"\x1b[15~"),
            ("f10", false, false, false, false, b"\x1b[21~"),
            ("f12", false, false, false, false, b"\x1b[24~"),
            ("f12", true, false, false, false, b"\x1b[24;5~"),
            ("enter", false, false, false, false, b"\r"),
            ("enter", false, true, false, false, b"\x1b\r"),
            ("backspace", false, false, false, false, b"\x7f"),
            ("backspace", true, false, false, false, b"\x08"),
            ("backspace", false, true, false, false, b"\x1b\x7f"),
            ("tab", false, false, false, false, b"\t"),
            ("tab", false, false, true, false, b"\x1b[Z"),
            ("escape", false, false, false, false, b"\x1b"),
            ("space", false, false, false, false, b" "),
            ("space", true, false, false, false, b"\x00"),
            ("a", true, false, false, false, b"\x01"),
            ("c", true, false, false, false, b"\x03"),
            ("z", true, false, false, false, b"\x1a"),
            ("C", true, false, true, false, b"\x03"),
            ("[", true, false, false, false, b"\x1b"),
            ("\\", true, false, false, false, b"\x1c"),
            ("]", true, false, false, false, b"\x1d"),
            ("@", true, false, false, false, b"\x00"),
            ("/", true, false, false, false, b"\x1f"),
            ("?", true, false, false, false, b"\x7f"),
            ("c", true, true, false, false, b"\x1b\x03"),
            ("b", false, true, false, false, b"\x1bb"),
            ("x", false, false, false, false, b"x"),
            ("x", false, false, true, false, b"X"),
            ("é", false, false, false, false, "é".as_bytes()),
        ];
        for (key, ctrl, alt, shift, app, want) in cases {
            assert_eq!(
                k(key, *ctrl, *alt, *shift, *app).as_deref(),
                Some(*want),
                "{key} ctrl={ctrl} alt={alt} shift={shift} app={app}"
            );
        }
    }

    #[test]
    fn unencodable_keys() {
        assert_eq!(k("shift", false, false, true, false), None);
        assert_eq!(k("", false, false, false, false), None);
        assert_eq!(k("1", true, false, false, false), None);
    }

    #[test]
    fn primary_screen_overrides() {
        assert_eq!(
            primary_screen_override("left", false, true, false, false).as_deref(),
            Some(&b"\x1bb"[..])
        );
        assert_eq!(
            primary_screen_override("right", false, true, false, false).as_deref(),
            Some(&b"\x1bf"[..])
        );
        assert_eq!(
            primary_screen_override("l", true, false, false, false).as_deref(),
            Some(&[0x0c][..])
        );
        // Shift-modified arrows are left alone.
        assert_eq!(
            primary_screen_override("left", false, true, true, false),
            None
        );
        assert_eq!(
            primary_screen_override("left", false, false, false, false),
            None
        );
        let mac = primary_screen_override("left", false, false, false, true);
        assert_eq!(mac.is_some(), cfg!(target_os = "macos"));
    }

    #[test]
    fn paste_wrapping() {
        assert_eq!(paste_bytes("a\nb", false), b"a\rb");
        assert_eq!(paste_bytes("a\r\nb", false), b"a\rb");
        assert_eq!(
            paste_bytes("x\x1b[201~y", true),
            b"\x1b[200~x[201~y\x1b[201~"
        );
    }
}

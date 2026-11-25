//! Raw PTY output history, bounded like t3code's `BoundedTerminalHistory`
//! (spec/07 §8.13): the last 5,000 lines and 8 MiB. The grid has its own scrollback;
//! this is the byte stream the app persists and replays into a restarted shell.

use std::collections::VecDeque;

pub const HISTORY_LINE_LIMIT: usize = 5_000;
pub const HISTORY_BYTE_LIMIT: usize = 8 * 1024 * 1024;

/// Pop beyond the kitty keyboard stacks on both screens before clearing flags, then turn
/// off cursor-key, mouse, focus and bracketed-paste modes. A 1049 round trip keeps the
/// main cursor, text and scrollback. Verbatim from `INHERITED_INPUT_RESET`.
pub const INHERITED_INPUT_RESET: &str = "\x1b[?1049h\x1b[<100u\x1b[=0;1u\x1b[?1049l\
\x1b[?1;9;66;67;1000;1002;1003;1004;1005;1006;1015;2004l\
\x1b[?25;1035;1036h\x1b[<100u\x1b[=0;1u\x1b[>4m";

#[derive(Clone, Debug)]
pub struct History {
    buf: VecDeque<u8>,
    lines: usize,
    max_lines: usize,
    max_bytes: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(HISTORY_LINE_LIMIT, HISTORY_BYTE_LIMIT)
    }
}

impl History {
    pub fn new(max_lines: usize, max_bytes: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            lines: 0,
            max_lines,
            max_bytes,
        }
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.lines = 0;
    }

    pub fn push(&mut self, data: &[u8]) {
        if self.max_lines == 0 || self.max_bytes == 0 {
            return;
        }
        self.buf.extend(data);
        self.lines += data.iter().filter(|b| **b == b'\n').count();
        self.trim();
    }

    fn drop_front(&mut self, n: usize) {
        let removed = self.buf.drain(..n).filter(|b| *b == b'\n').count();
        self.lines -= removed;
    }

    fn trim(&mut self) {
        if self.lines > self.max_lines {
            // Keep the last `max_lines` line breaks: cut just after the excess-th one.
            let mut excess = self.lines - self.max_lines;
            let mut cut = 0;
            for (i, b) in self.buf.iter().enumerate() {
                if *b == b'\n' {
                    excess -= 1;
                    if excess == 0 {
                        cut = i + 1;
                        break;
                    }
                }
            }
            self.drop_front(cut);
        }
        if self.buf.len() > self.max_bytes {
            let mut cut = self.buf.len() - self.max_bytes;
            // Prefer a line start; otherwise at least a UTF-8 boundary.
            if let Some(nl) = self.buf.iter().skip(cut).position(|b| *b == b'\n') {
                cut += nl + 1;
            } else {
                while cut < self.buf.len() && (self.buf[cut] & 0xc0) == 0x80 {
                    cut += 1;
                }
            }
            self.drop_front(cut);
        }
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let (a, b) = self.buf.as_slices();
        let mut out = Vec::with_capacity(a.len() + b.len());
        out.extend_from_slice(a);
        out.extend_from_slice(b);
        out
    }
}

/// `neutralizeInheritedHistory`: trims `history` to the limits (reserving room for the
/// reset), then appends a sequence that leaves any alternate screen and turns off
/// mouse/kitty/bracketed modes so a new shell isn't stuck in a dead app's modes.
pub fn neutralize_inherited_history(history: &[u8], max_lines: usize, max_bytes: usize) -> Vec<u8> {
    let has_esc = history.contains(&0x1b);
    let reset_bytes = if has_esc {
        INHERITED_INPUT_RESET.len() + "\x1b7\x1b[?1049l".len()
    } else {
        0
    };
    let lines = if reset_bytes > 0 && history.ends_with(b"\n") {
        max_lines.saturating_sub(1)
    } else {
        max_lines
    };
    let mut bounded = History::new(lines, max_bytes.saturating_sub(reset_bytes));
    bounded.push(history);
    let mut out = bounded.to_vec();
    if !out.contains(&0x1b) {
        return out;
    }
    // An unconditional 1049 reset restores a stale saved cursor on the main screen, so
    // the exit must match the inherited screen's mode.
    let mut alternate: Option<u32> = None;
    let mut i = 0;
    while i < out.len() {
        if out[i] != 0x1b {
            i += 1;
            continue;
        }
        if out.get(i + 1) == Some(&b'c') {
            alternate = None;
            i += 2;
            continue;
        }
        if out.get(i + 1) == Some(&b'[') && out.get(i + 2) == Some(&b'?') {
            let start = i + 3;
            let mut j = start;
            while j < out.len() && (out[j].is_ascii_digit() || out[j] == b';') {
                j += 1;
            }
            if j > start && matches!(out.get(j), Some(b'h' | b'l')) {
                let set = out[j] == b'h';
                for param in out[start..j].split(|b| *b == b';') {
                    let mode: u32 = std::str::from_utf8(param)
                        .ok()
                        .and_then(|p| p.parse().ok())
                        .unwrap_or(0);
                    if matches!(mode, 47 | 1047 | 1049) {
                        alternate = if set { alternate.or(Some(mode)) } else { None };
                    }
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    if let Some(mode) = alternate {
        out.extend_from_slice(format!("\x1b7\x1b[?{mode}l").as_bytes());
    }
    out.extend_from_slice(INHERITED_INPUT_RESET.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_last_lines() {
        let mut h = History::new(3, 1 << 20);
        for i in 0..10 {
            h.push(format!("line {i}\n").as_bytes());
        }
        assert_eq!(h.to_vec(), b"line 7\nline 8\nline 9\n");
    }

    #[test]
    fn keeps_last_bytes_at_line_start() {
        let mut h = History::new(1000, 10);
        h.push(b"aaaa\nbbbb\ncccc\n");
        assert_eq!(h.to_vec(), b"cccc\n");
        let mut h = History::new(1000, 4);
        h.push("xxé".as_bytes());
        h.push("ééé".as_bytes());
        assert!(std::str::from_utf8(&h.to_vec()).is_ok());
        assert!(h.to_vec().len() <= 4);
    }

    #[test]
    fn default_limits() {
        let mut h = History::default();
        for _ in 0..6000 {
            h.push(b"x\n");
        }
        assert_eq!(h.to_vec().len(), HISTORY_LINE_LIMIT * 2);
    }

    #[test]
    fn neutralize_plain_text_untouched() {
        assert_eq!(neutralize_inherited_history(b"hi\n", 10, 100), b"hi\n");
    }

    #[test]
    fn neutralize_leaves_alternate_screen() {
        let out = neutralize_inherited_history(b"$ vim\n\x1b[?1049hjunk", 100, 1 << 20);
        let s = String::from_utf8(out).unwrap();
        assert!(s.ends_with(&format!("\x1b7\x1b[?1049l{INHERITED_INPUT_RESET}")));
        let out = neutralize_inherited_history(b"\x1b[?1049hx\x1b[?1049l", 100, 1 << 20);
        let s = String::from_utf8(out).unwrap();
        assert!(s.ends_with(&format!("\x1b[?1049l{INHERITED_INPUT_RESET}")));
        assert!(!s.contains("\x1b7"));
    }
}

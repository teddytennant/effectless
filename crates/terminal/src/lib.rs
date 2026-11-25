//! PTY terminals for the drawer (spec/05 §3, spec/07 §8.10-8.15).
//! `portable-pty` for the process, `alacritty_terminal` for the grid. The app renders
//! `TerminalSnapshot`s and feeds keys/bytes back through `Terminal::write`.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct TerminalOptions {
    pub cwd: PathBuf,
    pub shell: Option<String>,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CellStyle {
    /// 0-255 indexed, or 0xff000000 | rgb for truecolor. None = default fg/bg.
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub dim: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalLine {
    /// Runs of text sharing a style.
    pub runs: Vec<(String, CellStyle)>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TerminalSnapshot {
    pub lines: Vec<TerminalLine>,
    pub cursor: Option<(u16, u16)>,
    pub cols: u16,
    pub rows: u16,
    pub title: Option<String>,
    pub exited: Option<i32>,
    /// Lines of scrollback above the viewport currently shown.
    pub display_offset: usize,
}

pub struct Terminal {
    _private: (),
}

impl Terminal {
    /// Spawns the shell. `on_change` fires (from a background thread) whenever the
    /// grid changes; the app then calls `snapshot()`.
    pub fn spawn(options: TerminalOptions, on_change: Box<dyn Fn() + Send + Sync>) -> anyhow::Result<Self> {
        let _ = (options, on_change);
        todo!()
    }
    pub fn write(&self, bytes: &[u8]) {
        let _ = bytes;
        todo!()
    }
    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = (cols, rows);
        todo!()
    }
    pub fn scroll(&self, delta_lines: i32) {
        let _ = delta_lines;
        todo!()
    }
    pub fn snapshot(&self) -> TerminalSnapshot {
        todo!()
    }
    /// Selected text for copy, given grid coordinates (col,row) start/end in the viewport.
    pub fn text_in_range(&self, start: (u16, u16), end: (u16, u16)) -> String {
        let _ = (start, end);
        todo!()
    }
    /// SIGHUP, then SIGTERM, then SIGKILL after 1s.
    pub fn close(&self) {
        todo!()
    }
}

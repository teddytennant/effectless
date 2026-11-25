//! PTY terminals for the drawer (spec/05 §3, spec/07 §8.10-8.15).
//! `portable-pty` for the process, `alacritty_terminal` for the grid. The app renders
//! `TerminalSnapshot`s and feeds keys/bytes back through `Terminal::write`.
//!
//! Threads per terminal: a reader (PTY → parser → grid), a writer (input queue → PTY),
//! a waiter (child exit), and a notifier that calls `on_change` and ends stuck
//! synchronized updates.

mod env;
mod history;
mod keys;
mod links;

pub use env::{
    DEFAULT_COLS, DEFAULT_ROWS, MAX_COLS, MAX_ROWS, PROJECT_ROOT_ENV, SHELL_ENV, ShellCandidate,
    THREAD_ID_ENV, WORKTREE_PATH_ENV, launch_env, normalize_shell_command, shell_candidates,
    spawn_env,
};
pub use history::{
    HISTORY_BYTE_LIMIT, HISTORY_LINE_LIMIT, INHERITED_INPUT_RESET, neutralize_inherited_history,
};
pub use keys::{key_to_bytes, paste_bytes, primary_screen_override};
pub use links::{LinkKind, LinkMatch, detect_links, split_path_position};

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Color, CursorShape as VteCursorShape, CursorStyle as VteCursorStyle, NamedColor, Processor,
    Rgb, Timeout,
};
use parking_lot::Mutex;
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use history::History;

/// Grid scrollback, matching the server history's line limit (spec/07 §8.13).
pub const SCROLLBACK_LINES: usize = HISTORY_LINE_LIMIT;
/// Close grace between SIGTERM and SIGKILL.
pub const KILL_GRACE: Duration = Duration::from_millis(1_000);
/// A synchronized update (DEC 2026) is forced to a frame after this long (spec/05 §3.9).
pub const SYNC_UPDATE_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Clone, Debug)]
pub struct TerminalOptions {
    /// Must exist and be a directory.
    pub cwd: PathBuf,
    /// Explicit shell. Falls back through `TERN_TERMINAL_SHELL`, `$SHELL`, `/bin/zsh`,
    /// `/bin/bash`, `/bin/sh`, `zsh`, `bash`, `sh` like T3 does.
    pub shell: Option<String>,
    /// Extra env from the caller (`TERN_PROJECT_ROOT`, script env). Keys must match
    /// `^[A-Za-z_][A-Za-z0-9_]*$`, at most 128 keys, values at most 8192 chars.
    pub env: Vec<(String, String)>,
    /// Grid size; 0 means the default 120×30. Clamped to 1000×500.
    pub cols: u16,
    pub rows: u16,
    /// Run this argv instead of a shell, with no fallback. Mostly for tests and
    /// one-shot commands.
    pub command: Option<Vec<String>>,
    /// Exported as `TERN_THREAD_ID` (T3's port scanner variable).
    pub thread_id: Option<String>,
    /// History inherited from a previous shell in this terminal. It is neutralized
    /// (`neutralize_inherited_history`) and replayed into the grid before the shell
    /// starts; terminal replies to queries in it are dropped.
    pub history: Option<Vec<u8>>,
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_default(),
            shell: None,
            env: Vec::new(),
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            command: None,
            thread_id: None,
            history: None,
        }
    }
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
    pub strikethrough: bool,
}

/// Truecolor values carry this alpha byte, so they never collide with 0-255 indices.
pub const TRUECOLOR_FLAG: u32 = 0xff00_0000;

/// Splits a `CellStyle` color into an index or an rgb value.
pub fn color_rgb(color: u32) -> Result<u8, u32> {
    if color & TRUECOLOR_FLAG == TRUECOLOR_FLAG {
        Err(color & 0x00ff_ffff)
    } else {
        Ok(color as u8)
    }
}

/// An OSC 8 hyperlink over `start..end` columns of a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HyperlinkSpan {
    pub start: u16,
    pub end: u16,
    pub uri: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TerminalLine {
    /// Runs of text sharing a style. Trailing blank default-styled cells are dropped.
    /// A wide (2-column) char appears once; its columns are listed in `wide`.
    pub runs: Vec<(String, CellStyle)>,
    /// Start columns of wide chars, ascending.
    pub wide: Vec<u16>,
    /// The line soft-wraps into the next row.
    pub wrapped: bool,
    pub hyperlinks: Vec<HyperlinkSpan>,
}

impl TerminalLine {
    pub fn text(&self) -> String {
        self.runs.iter().map(|(t, _)| t.as_str()).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Bar,
    HollowBlock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TerminalModes {
    pub app_cursor: bool,
    pub app_keypad: bool,
    pub bracketed_paste: bool,
    pub alt_screen: bool,
    /// Any mouse reporting (click, drag or motion).
    pub mouse_reporting: bool,
    pub mouse_motion: bool,
    pub mouse_drag: bool,
    pub sgr_mouse: bool,
    pub utf8_mouse: bool,
    pub focus_reporting: bool,
    pub alternate_scroll: bool,
}

impl From<TermMode> for TerminalModes {
    fn from(m: TermMode) -> Self {
        Self {
            app_cursor: m.contains(TermMode::APP_CURSOR),
            app_keypad: m.contains(TermMode::APP_KEYPAD),
            bracketed_paste: m.contains(TermMode::BRACKETED_PASTE),
            alt_screen: m.contains(TermMode::ALT_SCREEN),
            mouse_reporting: m.intersects(TermMode::MOUSE_MODE),
            mouse_motion: m.contains(TermMode::MOUSE_MOTION),
            mouse_drag: m.contains(TermMode::MOUSE_DRAG),
            sgr_mouse: m.contains(TermMode::SGR_MOUSE),
            utf8_mouse: m.contains(TermMode::UTF8_MOUSE),
            focus_reporting: m.contains(TermMode::FOCUS_IN_OUT),
            alternate_scroll: m.contains(TermMode::ALTERNATE_SCROLL),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TerminalSnapshot {
    pub lines: Vec<TerminalLine>,
    /// (col, row) in the viewport; None when hidden or scrolled out of view.
    pub cursor: Option<(u16, u16)>,
    pub cols: u16,
    pub rows: u16,
    /// From OSC 0/2.
    pub title: Option<String>,
    /// Exit code once the process is gone (1 when it died from a signal).
    pub exited: Option<i32>,
    /// Lines of scrollback above the viewport currently shown.
    pub display_offset: usize,
    /// Lines of scrollback available.
    pub history_size: usize,
    pub cursor_shape: CursorShape,
    pub cursor_blinking: bool,
    pub modes: TerminalModes,
    /// Signal name when the process was killed by one.
    pub exit_signal: Option<String>,
    /// Palette entries the app changed with OSC 4/10/11/12: (index, 0xff000000|rgb).
    /// 0-255 palette, 256 foreground, 257 background, 258 cursor.
    pub color_overrides: Vec<(u16, u32)>,
}

/// Colors used to answer OSC 4/10/11/12 queries (vim and fish ask for the background).
/// The app sets this from its theme; values are 0xRRGGBB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorScheme {
    pub ansi: [u32; 16],
    pub foreground: u32,
    pub background: u32,
    pub cursor: u32,
}

impl ColorScheme {
    /// spec/05 §3.10, dark.
    pub fn dark() -> Self {
        Self {
            ansi: [
                0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5,
                0x666666, 0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xe5e5e5,
            ],
            foreground: 0xf5f5f5,
            background: 0x000000,
            cursor: 0xb4cbff,
        }
    }

    /// spec/05 §3.10, light.
    pub fn light() -> Self {
        Self {
            ansi: [
                0x000000, 0xcd3131, 0x107c10, 0x949800, 0x0451a5, 0xbc05bc, 0x0598bc, 0x555555,
                0x666666, 0xcd3131, 0x14ce14, 0xb5ba00, 0x0451a5, 0xbc05bc, 0x0598bc, 0xa5a5a5,
            ],
            foreground: 0x18181b,
            background: 0xfcfcfc,
            cursor: 0x26384e,
        }
    }

    /// Resolves a palette index (0-255), or 256 fg / 257 bg / 258 cursor, to 0xRRGGBB.
    /// 16-255 are the xterm cube and gray ramp.
    pub fn resolve(&self, index: usize) -> u32 {
        match index {
            0..=15 => self.ansi[index],
            16..=231 => {
                let i = index - 16;
                let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u32 };
                (level(i / 36) << 16) | (level((i / 6) % 6) << 8) | level(i % 6)
            }
            232..=255 => {
                let v = (8 + (index - 232) * 10) as u32;
                (v << 16) | (v << 8) | v
            }
            257 => self.background,
            258 => self.cursor,
            _ => self.foreground,
        }
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::dark()
    }
}

/// A link in the viewport. Points are (col, row); `end` is exclusive and may sit on a
/// later row when the link crosses a soft wrap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewportLink {
    pub kind: LinkKind,
    /// True for OSC 8 hyperlinks, which win over detected ones.
    pub explicit: bool,
    pub text: String,
    pub start: (u16, u16),
    pub end: (u16, u16),
}

#[derive(Clone, Copy, Debug)]
struct GridSize {
    cols: usize,
    rows: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// Synchronized-update timer with the spec's 500ms instead of vte's 150ms.
#[derive(Default)]
struct SyncTimeout(Option<Instant>);

impl Timeout for SyncTimeout {
    fn set_timeout(&mut self, _duration: Duration) {
        self.0 = Some(Instant::now() + SYNC_UPDATE_TIMEOUT);
    }
    fn clear_timeout(&mut self) {
        self.0 = None;
    }
    fn pending_timeout(&self) -> bool {
        self.0.is_some()
    }
}

type ColorFormatter = Arc<dyn Fn(Rgb) -> String + Send + Sync>;

/// State the parser's event listener touches. It is called with the grid locked, so it
/// only uses its own small locks.
struct ListenerState {
    title: Mutex<Option<String>>,
    input: mpsc::Sender<Vec<u8>>,
    replaying: AtomicBool,
    color_requests: Mutex<Vec<(usize, ColorFormatter)>>,
    clipboard: Mutex<Option<String>>,
    bell: AtomicBool,
    window: Mutex<WindowSize>,
}

#[derive(Clone)]
struct Listener(Arc<ListenerState>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let s = &self.0;
        // Replies to queries replayed from inherited history must not reach the shell.
        let live = !s.replaying.load(Ordering::Relaxed);
        match event {
            Event::Title(title) => *s.title.lock() = Some(title),
            Event::ResetTitle => *s.title.lock() = None,
            Event::PtyWrite(text) if live => {
                let _ = s.input.send(text.into_bytes());
            }
            Event::ColorRequest(index, format) if live => {
                s.color_requests.lock().push((index, format));
            }
            Event::TextAreaSizeRequest(format) if live => {
                let size = *s.window.lock();
                let _ = s.input.send(format(size).into_bytes());
            }
            Event::ClipboardStore(_, text) => *s.clipboard.lock() = Some(text),
            Event::Bell => s.bell.store(true, Ordering::Relaxed),
            _ => {}
        }
    }
}

struct GridState {
    term: Term<Listener>,
    parser: Processor<SyncTimeout>,
    history: History,
}

impl GridState {
    fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }
}

#[derive(Clone, Debug)]
struct ExitInfo {
    code: i32,
    signal: Option<String>,
}

struct Shared {
    grid: Mutex<GridState>,
    listener: Arc<ListenerState>,
    scheme: Mutex<ColorScheme>,
    exit: Mutex<Option<ExitInfo>>,
    /// The grid changed since the notifier last looked.
    dirty: AtomicBool,
    /// `on_change` fired and the app hasn't taken a snapshot since.
    pending: AtomicBool,
    shutdown: AtomicBool,
    notifier: Mutex<Option<thread::Thread>>,
}

impl Shared {
    fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::SeqCst);
        if let Some(t) = self.notifier.lock().as_ref() {
            t.unpark();
        }
    }

    fn exited(&self) -> bool {
        self.exit.lock().is_some()
    }

    /// Answers queued OSC color queries from the app's overrides, else the scheme.
    fn answer_color_requests(&self, grid: &GridState) {
        let requests = std::mem::take(&mut *self.listener.color_requests.lock());
        if requests.is_empty() {
            return;
        }
        let scheme = self.scheme.lock().clone();
        for (index, format) in requests {
            let rgb = grid.term.colors()[index].unwrap_or_else(|| {
                let v = scheme.resolve(index);
                Rgb {
                    r: (v >> 16) as u8,
                    g: (v >> 8) as u8,
                    b: v as u8,
                }
            });
            let _ = self.listener.input.send(format(rgb).into_bytes());
        }
    }
}

pub struct Terminal {
    shared: Arc<Shared>,
    input: mpsc::Sender<Vec<u8>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    pid: Option<u32>,
    #[cfg_attr(unix, allow(dead_code))]
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    closing: Arc<AtomicBool>,
}

fn rgb_u32(rgb: Rgb) -> u32 {
    TRUECOLOR_FLAG | ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32
}

/// Maps a cell color to the snapshot form; the bool is "draw dim".
fn map_color(color: Color) -> (Option<u32>, bool) {
    match color {
        Color::Spec(rgb) => (Some(rgb_u32(rgb)), false),
        Color::Indexed(i) => (Some(i as u32), false),
        Color::Named(named) => {
            let n = named as usize;
            match named {
                _ if n < 16 => (Some(n as u32), false),
                NamedColor::DimBlack
                | NamedColor::DimRed
                | NamedColor::DimGreen
                | NamedColor::DimYellow
                | NamedColor::DimBlue
                | NamedColor::DimMagenta
                | NamedColor::DimCyan
                | NamedColor::DimWhite => (Some((n - NamedColor::DimBlack as usize) as u32), true),
                NamedColor::DimForeground => (None, true),
                _ => (None, false),
            }
        }
    }
}

fn cell_style(cell: &alacritty_terminal::term::cell::Cell) -> CellStyle {
    let (fg, fg_dim) = map_color(cell.fg);
    let (bg, _) = map_color(cell.bg);
    let f = cell.flags;
    CellStyle {
        fg,
        bg,
        bold: f.contains(Flags::BOLD),
        italic: f.contains(Flags::ITALIC),
        underline: f.intersects(Flags::ALL_UNDERLINES),
        inverse: f.contains(Flags::INVERSE),
        dim: f.contains(Flags::DIM) || fg_dim,
        strikethrough: f.contains(Flags::STRIKEOUT),
    }
}

/// A blank cell nothing would draw: dropped from the end of a line.
fn is_blank(cell: &alacritty_terminal::term::cell::Cell) -> bool {
    (cell.c == ' ' || cell.c == '\0')
        && cell.extra.is_none()
        && map_color(cell.bg).0.is_none()
        && !cell
            .flags
            .intersects(Flags::INVERSE | Flags::ALL_UNDERLINES | Flags::STRIKEOUT)
}

fn build_line(term: &Term<Listener>, line: Line) -> TerminalLine {
    let row = &term.grid()[line];
    let cols = term.columns();
    let mut last = cols;
    while last > 0 && is_blank(&row[Column(last - 1)]) {
        last -= 1;
    }
    let mut out = TerminalLine {
        wrapped: cols > 0 && row[Column(cols - 1)].flags.contains(Flags::WRAPLINE),
        ..Default::default()
    };
    let mut link: Option<(u16, String, String)> = None;
    for col in 0..last {
        let cell = &row[Column(col)];
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        if cell.flags.contains(Flags::WIDE_CHAR) {
            out.wide.push(col as u16);
        }
        let style = cell_style(cell);
        let c = if cell.flags.contains(Flags::HIDDEN)
            || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            || cell.c == '\0'
            || cell.c == '\t'
        {
            ' '
        } else {
            cell.c
        };
        match out.runs.last_mut() {
            Some((text, s)) if *s == style => text.push(c),
            _ => out.runs.push((c.to_string(), style)),
        }
        if let Some(zw) = cell.zerowidth()
            && let Some((text, _)) = out.runs.last_mut()
        {
            text.extend(zw.iter());
        }
        let cell_link = cell.hyperlink();
        let same = matches!((&link, &cell_link), (Some((_, id, _)), Some(h)) if id == h.id());
        if !same {
            if let Some((start, _, uri)) = link.take() {
                out.hyperlinks.push(HyperlinkSpan {
                    start,
                    end: col as u16,
                    uri,
                });
            }
            if let Some(h) = cell_link {
                link = Some((col as u16, h.id().to_string(), h.uri().to_string()));
            }
        }
    }
    if let Some((start, _, uri)) = link {
        out.hyperlinks.push(HyperlinkSpan {
            start,
            end: last as u16,
            uri,
        });
    }
    out
}

fn spawn_thread(name: &str, f: impl FnOnce() + Send + 'static) -> anyhow::Result<()> {
    thread::Builder::new()
        .name(name.to_string())
        .spawn(f)
        .map(|_| ())
        .map_err(Into::into)
}

impl Terminal {
    /// Spawns the shell. `on_change` fires (from a background thread) whenever the
    /// grid changes; the app then calls `snapshot()`.
    ///
    /// Notifications are coalesced: after `on_change` fires it won't fire again until
    /// the app calls `snapshot()`, so a flood of output costs one callback per frame the
    /// app actually draws. It also fires once when the process exits.
    pub fn spawn(
        options: TerminalOptions,
        on_change: Box<dyn Fn() + Send + Sync>,
    ) -> anyhow::Result<Self> {
        let cwd = options.cwd.clone();
        if !cwd.exists() {
            anyhow::bail!(
                "Terminal folder does not exist: {}. Restore the folder at this path before retrying.",
                cwd.display()
            );
        }
        if !cwd.is_dir() {
            anyhow::bail!("Terminal cwd is not a directory: {}", cwd.display());
        }
        env::validate_env(&options.env)?;
        let (cols, rows) = env::clamp_size(options.cols, options.rows);

        let pty = native_pty_system();
        let pair = pty.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
        let listener = Arc::new(ListenerState {
            title: Mutex::new(None),
            input: input_tx.clone(),
            replaying: AtomicBool::new(false),
            color_requests: Mutex::new(Vec::new()),
            clipboard: Mutex::new(None),
            bell: AtomicBool::new(false),
            window: Mutex::new(WindowSize {
                num_lines: rows,
                num_cols: cols,
                cell_width: 0,
                cell_height: 0,
            }),
        });
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            default_cursor_style: VteCursorStyle {
                shape: VteCursorShape::Block,
                blinking: true,
            },
            ..Config::default()
        };
        let size = GridSize {
            cols: cols as usize,
            rows: rows as usize,
        };
        let mut grid = GridState {
            term: Term::new(config, &size, Listener(listener.clone())),
            parser: Processor::new(),
            history: History::default(),
        };
        if let Some(inherited) = options.history.as_deref().filter(|h| !h.is_empty()) {
            let replay =
                neutralize_inherited_history(inherited, HISTORY_LINE_LIMIT, HISTORY_BYTE_LIMIT);
            listener.replaying.store(true, Ordering::Relaxed);
            grid.feed(&replay);
            if grid.parser.sync_timeout().pending_timeout() {
                grid.parser.stop_sync(&mut grid.term);
            }
            listener.replaying.store(false, Ordering::Relaxed);
            grid.history.push(&replay);
        }

        // Build the process.
        let home = std::env::var("HOME").unwrap_or_default();
        let base: Vec<(String, String)> = std::env::vars().collect();
        let spawn_env = env::spawn_env(base, &options.env, options.thread_id.as_deref(), &home);
        let candidates: Vec<(String, Vec<String>)> = match &options.command {
            Some(argv) if !argv.is_empty() => vec![(argv[0].clone(), argv[1..].to_vec())],
            Some(_) => anyhow::bail!("Terminal command is empty"),
            None => {
                let configured = options
                    .shell
                    .clone()
                    .or_else(|| {
                        options
                            .env
                            .iter()
                            .find(|(k, _)| k == SHELL_ENV)
                            .map(|(_, v)| v.clone())
                    })
                    .or_else(|| std::env::var(SHELL_ENV).ok());
                let shell_env = std::env::var("SHELL").ok();
                env::shell_candidates(configured.as_deref(), shell_env.as_deref())
                    .into_iter()
                    .map(|c| (c.shell, c.args))
                    .collect()
            }
        };
        let mut child = None;
        let mut last_err = None;
        for (program, args) in &candidates {
            let mut cmd = CommandBuilder::new(program);
            cmd.args(args);
            cmd.env_clear();
            for (k, v) in &spawn_env {
                cmd.env(k, v);
            }
            cmd.cwd(&cwd);
            match pair.slave.spawn_command(cmd) {
                Ok(c) => {
                    child = Some(c);
                    break;
                }
                Err(err) => {
                    tracing::debug!(%program, %err, "terminal shell failed to spawn");
                    last_err = Some(err);
                }
            }
        }
        let Some(mut child) = child else {
            return Err(last_err.unwrap_or_else(|| anyhow::anyhow!("No shell to spawn")));
        };
        // The child holds the slave now; dropping ours lets the reader see EOF on exit.
        drop(pair.slave);
        let pid = child.process_id();
        let killer = child.clone_killer();

        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;

        let shared = Arc::new(Shared {
            grid: Mutex::new(grid),
            listener,
            scheme: Mutex::new(ColorScheme::default()),
            exit: Mutex::new(None),
            dirty: AtomicBool::new(false),
            pending: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            notifier: Mutex::new(None),
        });

        // Writer: input queue → PTY, so `write` never blocks the UI thread.
        spawn_thread("tern-pty-writer", move || {
            for bytes in input_rx {
                if writer
                    .write_all(&bytes)
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        })?;

        // Reader: PTY → parser → grid.
        let (reader_done_tx, reader_done_rx) = mpsc::channel::<()>();
        let reader_shared = shared.clone();
        spawn_thread("tern-pty-reader", move || {
            let _done = reader_done_tx;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    // EIO once every slave fd is closed.
                    Err(_) => break,
                };
                {
                    let mut grid = reader_shared.grid.lock();
                    grid.feed(&buf[..n]);
                    grid.history.push(&buf[..n]);
                    reader_shared.answer_color_requests(&grid);
                }
                reader_shared.mark_dirty();
            }
        })?;

        // Waiter: child exit. Gives the reader a moment to drain output written just
        // before exit, so "exited" never lands ahead of the last output.
        let waiter_shared = shared.clone();
        spawn_thread("tern-pty-waiter", move || {
            let status = child.wait();
            let _ = reader_done_rx.recv_timeout(Duration::from_millis(250));
            let info = match status {
                Ok(s) => {
                    let signal = format!("{s}");
                    ExitInfo {
                        code: s.exit_code() as i32,
                        signal: signal.strip_prefix("Terminated by ").map(str::to_string),
                    }
                }
                Err(_) => ExitInfo {
                    code: -1,
                    signal: None,
                },
            };
            *waiter_shared.exit.lock() = Some(info);
            waiter_shared.mark_dirty();
        })?;

        // Notifier: coalesced `on_change`, and the synchronized-update deadline.
        let notifier_shared = shared.clone();
        let (ready_tx, ready_rx) = mpsc::channel::<()>();
        spawn_thread("tern-pty-notify", move || {
            let s = notifier_shared;
            *s.notifier.lock() = Some(thread::current());
            let _ = ready_tx.send(());
            loop {
                if s.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let deadline = s.grid.lock().parser.sync_timeout().0;
                if deadline.is_some_and(|d| d <= Instant::now()) {
                    let mut grid = s.grid.lock();
                    let GridState { term, parser, .. } = &mut *grid;
                    if parser.sync_timeout().0.is_some_and(|d| d <= Instant::now()) {
                        parser.stop_sync(term);
                    }
                    s.answer_color_requests(&grid);
                    drop(grid);
                    s.dirty.store(true, Ordering::SeqCst);
                }
                if s.dirty.swap(false, Ordering::SeqCst) && !s.pending.swap(true, Ordering::SeqCst)
                {
                    on_change();
                }
                // Bind first: a guard in a `match` scrutinee would be held while parked.
                let deadline = s.grid.lock().parser.sync_timeout().0;
                match deadline {
                    Some(d) => thread::park_timeout(d.saturating_duration_since(Instant::now())),
                    None => thread::park(),
                }
            }
        })?;
        let _ = ready_rx.recv();

        Ok(Self {
            shared,
            input: input_tx,
            master: Mutex::new(pair.master),
            pid,
            killer: Mutex::new(killer),
            closing: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Sends bytes to the process. Writes after exit are ignored.
    pub fn write(&self, bytes: &[u8]) {
        if bytes.is_empty() || self.shared.exited() {
            return;
        }
        let _ = self.input.send(bytes.to_vec());
    }

    /// Feeds bytes to the grid without sending them to the process, for the drawer's
    /// own `\r\n[terminal] Process exited\r\n` style messages.
    pub fn write_local(&self, bytes: &[u8]) {
        self.shared.grid.lock().feed(bytes);
        self.shared.mark_dirty();
    }

    /// Resizes the grid and the PTY. Clamped to 1..=1000 × 1..=500.
    pub fn resize(&self, cols: u16, rows: u16) {
        let cols = cols.clamp(1, MAX_COLS);
        let rows = rows.clamp(1, MAX_ROWS);
        let window = {
            let mut w = self.shared.listener.window.lock();
            w.num_cols = cols;
            w.num_lines = rows;
            *w
        };
        {
            let mut grid = self.shared.grid.lock();
            if grid.term.columns() == cols as usize && grid.term.screen_lines() == rows as usize {
                return;
            }
            grid.term.resize(GridSize {
                cols: cols as usize,
                rows: rows as usize,
            });
        }
        let _ = self.master.lock().resize(PtySize {
            rows,
            cols,
            pixel_width: cols.saturating_mul(window.cell_width),
            pixel_height: rows.saturating_mul(window.cell_height),
        });
        self.shared.mark_dirty();
    }

    /// Cell size in pixels, reported to apps through `CSI 14 t` and the PTY's pixel size.
    pub fn set_cell_size(&self, width: u16, height: u16) {
        let mut w = self.shared.listener.window.lock();
        w.cell_width = width;
        w.cell_height = height;
    }

    /// Scrolls the viewport. Positive deltas move up into the scrollback.
    pub fn scroll(&self, delta_lines: i32) {
        self.scroll_display(Scroll::Delta(delta_lines));
    }

    pub fn scroll_page_up(&self) {
        self.scroll_display(Scroll::PageUp);
    }

    pub fn scroll_page_down(&self) {
        self.scroll_display(Scroll::PageDown);
    }

    pub fn scroll_to_top(&self) {
        self.scroll_display(Scroll::Top);
    }

    pub fn scroll_to_bottom(&self) {
        self.scroll_display(Scroll::Bottom);
    }

    fn scroll_display(&self, scroll: Scroll) {
        let changed = {
            let mut grid = self.shared.grid.lock();
            let before = grid.term.grid().display_offset();
            grid.term.scroll_display(scroll);
            grid.term.grid().display_offset() != before
        };
        if changed {
            self.shared.mark_dirty();
        }
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        // Clear first: any change after this point notifies again.
        self.shared.pending.store(false, Ordering::SeqCst);
        let grid = self.shared.grid.lock();
        let term = &grid.term;
        let rows = term.screen_lines();
        let cols = term.columns();
        let offset = term.grid().display_offset();
        let lines = (0..rows)
            .map(|r| build_line(term, Line(r as i32 - offset as i32)))
            .collect();
        let mode = *term.mode();
        let mut cursor_point = term.grid().cursor.point;
        if term.grid()[cursor_point]
            .flags
            .contains(Flags::WIDE_CHAR_SPACER)
            && cursor_point.column.0 > 0
        {
            cursor_point.column -= 1;
        }
        let cursor_row = cursor_point.line.0 + offset as i32;
        let cursor = (mode.contains(TermMode::SHOW_CURSOR)
            && cursor_row >= 0
            && (cursor_row as usize) < rows)
            .then_some((cursor_point.column.0 as u16, cursor_row as u16));
        let style = term.cursor_style();
        let cursor_shape = match style.shape {
            VteCursorShape::Underline => CursorShape::Underline,
            VteCursorShape::Beam => CursorShape::Bar,
            VteCursorShape::HollowBlock => CursorShape::HollowBlock,
            _ => CursorShape::Block,
        };
        let color_overrides = (0..259)
            .filter_map(|i| term.colors()[i].map(|rgb| (i as u16, rgb_u32(rgb))))
            .collect();
        let history_size = term.grid().history_size();
        drop(grid);
        let exit = self.shared.exit.lock().clone();
        TerminalSnapshot {
            lines,
            cursor,
            cols: cols as u16,
            rows: rows as u16,
            title: self.shared.listener.title.lock().clone(),
            exited: exit.as_ref().map(|e| e.code),
            display_offset: offset,
            history_size,
            cursor_shape,
            cursor_blinking: style.blinking,
            modes: mode.into(),
            exit_signal: exit.and_then(|e| e.signal),
            color_overrides,
        }
    }

    fn viewport_point(term: &Term<Listener>, (col, row): (u16, u16)) -> Point {
        let offset = term.grid().display_offset() as i32;
        let last_row = term.screen_lines() as i32 - 1;
        let row = (row as i32).min(last_row);
        Point::new(
            Line(row - offset),
            Column((col as usize).min(term.columns() - 1)),
        )
    }

    fn to_viewport(term: &Term<Listener>, p: Point) -> (u16, u16) {
        let offset = term.grid().display_offset() as i32;
        let row = (p.line.0 + offset).clamp(0, term.screen_lines() as i32 - 1);
        (p.column.0 as u16, row as u16)
    }

    /// Selected text for copy, given grid coordinates (col,row) start/end in the viewport.
    /// Both ends are inclusive and may come in either order. Soft-wrapped rows join
    /// without a newline; trailing blanks on a row are dropped.
    pub fn text_in_range(&self, start: (u16, u16), end: (u16, u16)) -> String {
        let grid = self.shared.grid.lock();
        let term = &grid.term;
        let mut a = Self::viewport_point(term, start);
        let mut b = Self::viewport_point(term, end);
        if b < a {
            std::mem::swap(&mut a, &mut b);
        }
        term.bounds_to_string(a, b)
    }

    /// The word around a viewport cell (double-click), using alacritty's semantic
    /// separators. Returns inclusive (start, end).
    pub fn word_at(&self, point: (u16, u16)) -> ((u16, u16), (u16, u16)) {
        let grid = self.shared.grid.lock();
        let term = &grid.term;
        let p = Self::viewport_point(term, point);
        let a = term.semantic_search_left(p);
        let b = term.semantic_search_right(p);
        (Self::to_viewport(term, a), Self::to_viewport(term, b))
    }

    /// The logical line (soft-wrapped rows joined) around a viewport row (triple-click).
    /// Returns inclusive (start, end), clamped to the viewport.
    pub fn line_at(&self, row: u16) -> ((u16, u16), (u16, u16)) {
        let grid = self.shared.grid.lock();
        let term = &grid.term;
        let p = Self::viewport_point(term, (0, row));
        let a = term.line_search_left(p);
        let b = term.line_search_right(p);
        (Self::to_viewport(term, a), Self::to_viewport(term, b))
    }

    /// Everything in the scrollback and screen ("Select all" then copy).
    pub fn all_text(&self) -> String {
        let grid = self.shared.grid.lock();
        let term = &grid.term;
        let start = Point::new(term.topmost_line(), Column(0));
        let end = Point::new(term.bottommost_line(), term.last_column());
        let text = term.bounds_to_string(start, end);
        text.trim_end_matches('\n').to_string()
    }

    /// Links in the viewport: OSC 8 hyperlinks, then URLs and paths detected per
    /// logical line (spec/05 §3.11). Path links are not checked against the disk.
    pub fn links_in_viewport(&self) -> Vec<ViewportLink> {
        let grid = self.shared.grid.lock();
        let term = &grid.term;
        let rows = term.screen_lines();
        let cols = term.columns();
        let offset = term.grid().display_offset() as i32;
        let mut out = Vec::new();
        let mut row = 0usize;
        while row < rows {
            // Gather one logical line with each char's cell.
            let mut text = String::new();
            let mut cells: Vec<(u16, u16)> = Vec::new();
            let mut explicit: Vec<(usize, usize, String)> = Vec::new();
            loop {
                let line = Line(row as i32 - offset);
                let r = &term.grid()[line];
                for col in 0..cols {
                    let cell = &r[Column(col)];
                    if cell
                        .flags
                        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                    {
                        continue;
                    }
                    let idx = cells.len();
                    text.push(if cell.c == '\0' { ' ' } else { cell.c });
                    cells.push((col as u16, row as u16));
                    if let Some(h) = cell.hyperlink() {
                        match explicit.last_mut() {
                            Some((_, end, uri)) if *end == idx && uri == h.uri() => *end = idx + 1,
                            _ => explicit.push((idx, idx + 1, h.uri().to_string())),
                        }
                    }
                }
                let wrapped = r[Column(cols - 1)].flags.contains(Flags::WRAPLINE);
                row += 1;
                if !wrapped || row >= rows {
                    break;
                }
            }
            let end_point = |idx: usize| -> (u16, u16) {
                match cells.get(idx) {
                    Some(p) => *p,
                    None => {
                        let (c, r) = cells.last().copied().unwrap_or((0, 0));
                        (c + 1, r)
                    }
                }
            };
            for (s, e, uri) in &explicit {
                out.push(ViewportLink {
                    kind: if uri.starts_with("http://") || uri.starts_with("https://") {
                        LinkKind::Url
                    } else {
                        LinkKind::Path
                    },
                    explicit: true,
                    text: uri.clone(),
                    start: cells[*s],
                    end: end_point(*e),
                });
            }
            for m in detect_links(&text) {
                if explicit.iter().any(|(s, e, _)| m.start < *e && *s < m.end) {
                    continue;
                }
                out.push(ViewportLink {
                    kind: m.kind,
                    explicit: false,
                    text: m.text,
                    start: cells[m.start],
                    end: end_point(m.end),
                });
            }
        }
        out
    }

    /// Clears the grid, its scrollback and the raw history (`terminal.clear`).
    pub fn clear(&self) {
        {
            let mut grid = self.shared.grid.lock();
            grid.feed(b"\x1b[H\x1b[2J");
            grid.term.grid_mut().clear_history();
            grid.history.clear();
        }
        self.shared.mark_dirty();
    }

    /// Raw PTY output, bounded to the last 5,000 lines and 8 MiB, for persisting and
    /// for `TerminalOptions::history` on a restart.
    pub fn history(&self) -> Vec<u8> {
        self.shared.grid.lock().history.to_vec()
    }

    /// DECCKM: arrows and Home/End send `ESC O x`.
    pub fn app_cursor_mode(&self) -> bool {
        self.mode().contains(TermMode::APP_CURSOR)
    }

    /// Mode 2004: pastes are wrapped in `ESC[200~ … ESC[201~`.
    pub fn bracketed_paste(&self) -> bool {
        self.mode().contains(TermMode::BRACKETED_PASTE)
    }

    pub fn alt_screen(&self) -> bool {
        self.mode().contains(TermMode::ALT_SCREEN)
    }

    pub fn modes(&self) -> TerminalModes {
        self.mode().into()
    }

    fn mode(&self) -> TermMode {
        *self.shared.grid.lock().term.mode()
    }

    /// Pastes text, bracketed when the app asked for it. Scrolls to the bottom.
    pub fn paste(&self, text: &str) {
        let bytes = paste_bytes(text, self.bracketed_paste());
        self.scroll_to_bottom();
        self.write(&bytes);
    }

    /// Encodes and sends a key press: the drawer's primary-screen overrides first, then
    /// xterm encoding in the current cursor mode. Scrolls to the bottom when it sends.
    /// Returns false when the key has no encoding. App shortcuts must be checked before
    /// calling this (spec/05 §3.8).
    pub fn send_key(&self, key: &str, ctrl: bool, alt: bool, shift: bool, cmd: bool) -> bool {
        let mode = self.mode();
        let bytes = if mode.contains(TermMode::ALT_SCREEN) {
            None
        } else {
            primary_screen_override(key, ctrl, alt, shift, cmd)
        }
        .or_else(|| {
            if cmd {
                None
            } else {
                key_to_bytes(key, ctrl, alt, shift, mode.contains(TermMode::APP_CURSOR))
            }
        });
        match bytes {
            Some(bytes) => {
                self.scroll_to_bottom();
                self.write(&bytes);
                true
            }
            None => false,
        }
    }

    /// Focus in/out report (`ESC[I` / `ESC[O`) when the app enabled mode 1004.
    pub fn focus_changed(&self, focused: bool) {
        let report = {
            let mut grid = self.shared.grid.lock();
            grid.term.is_focused = focused;
            grid.term.mode().contains(TermMode::FOCUS_IN_OUT)
        };
        if report {
            self.write(if focused { b"\x1b[I" } else { b"\x1b[O" });
        }
    }

    /// Colors used to answer OSC 4/10/11/12 queries; set it from the app theme.
    pub fn set_color_scheme(&self, scheme: ColorScheme) {
        *self.shared.scheme.lock() = scheme;
    }

    /// Text an app copied with OSC 52, if any since the last call.
    pub fn take_clipboard_request(&self) -> Option<String> {
        self.shared.listener.clipboard.lock().take()
    }

    /// Whether the bell rang since the last call.
    pub fn take_bell(&self) -> bool {
        self.shared.listener.bell.swap(false, Ordering::Relaxed)
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn is_running(&self) -> bool {
        !self.shared.exited()
    }

    /// SIGHUP, then SIGTERM, then SIGKILL after 1s. Returns immediately; the escalation
    /// runs on a background thread and stops as soon as the process exits.
    pub fn close(&self) {
        if self.shared.exited() || self.closing.swap(true, Ordering::SeqCst) {
            return;
        }
        #[cfg(unix)]
        {
            let Some(pid) = self.pid else { return };
            let shared = self.shared.clone();
            let _ = spawn_thread("tern-pty-close", move || kill_escalation(pid, &shared));
        }
        // Windows: portable-pty terminates the process.
        #[cfg(not(unix))]
        {
            let _ = self.killer.lock().kill();
        }
    }
}

#[cfg(unix)]
fn kill_escalation(pid: u32, shared: &Shared) {
    let pid = pid as libc::pid_t;
    let signal = |sig| {
        if !shared.exited() {
            // SAFETY: plain kill(2) on our own child; it is reaped only by the waiter,
            // which sets `exit` first, so the pid hasn't been reused while unset.
            unsafe { libc::kill(pid, sig) };
        }
    };
    // Interactive shells ignore SIGTERM but pass SIGHUP to their jobs.
    signal(libc::SIGHUP);
    signal(libc::SIGTERM);
    let deadline = Instant::now() + KILL_GRACE;
    while Instant::now() < deadline {
        if shared.exited() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    signal(libc::SIGKILL);
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.close();
        self.shared.shutdown.store(true, Ordering::SeqCst);
        if let Some(t) = self.shared.notifier.lock().as_ref() {
            t.unpark();
        }
    }
}

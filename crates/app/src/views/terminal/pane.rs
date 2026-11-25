//! One terminal surface: owns the PTY handle and the last snapshot, and handles keys,
//! mouse selection, links, the wheel, the scrollbar, the cursor blink and the context
//! menu (spec 05 §3.8-3.12). The drawer lays panes out and owns their lifecycle.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use gpui::{
    App, ClipboardItem, Context, CursorStyle, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Render, ScrollEvent, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, div, prelude::FluentBuilder as _,
    px,
};
use tern_terminal::{LinkKind, Terminal, TerminalOptions, TerminalSnapshot, ViewportLink};

use super::colors::TermColors;
use super::element::{TerminalElement, TerminalFrame};
use super::input::{self, KeyAction, KeyPress, MouseButtonCode, MouseKind, WheelAction};
use super::layout::{self, GridLayout};
use super::open;
use super::selection::{self, Scroll, SelectMode, Selection};
use crate::theme::{ActiveTheme as _, font};
use crate::ui::menu::{Menu, MenuItem, open_menu};
use crate::ui::popup::PopupAnchor;

/// Cursor blink half-period.
pub const BLINK_INTERVAL: Duration = Duration::from_millis(500);
/// Selections larger than this always clear on new output.
pub const SELECTION_CHECK_LIMIT: usize = 3_000;
/// A press that moves less than this is a click (links open on release).
pub const CLICK_SLOP: f32 = 4.;
/// Notifications are drained at most this often under a flood of output.
const FRAME_PACE: Duration = Duration::from_millis(8);

pub const PROCESS_EXITED: &[u8] = b"\r\n[terminal] Process exited\r\n";

/// What "Add to chat" hands to the composer (spec 05 §3.12).
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalContext {
    pub terminal_id: String,
    pub terminal_label: String,
    pub line_start: u32,
    pub line_end: u32,
    pub text: String,
}

impl TerminalContext {
    pub fn chip_label(&self) -> String {
        selection::chat_chip_label(&self.terminal_label, self.line_start, self.line_end)
    }
}

pub enum PaneEvent {
    /// Pointer down on the pane: make it the active terminal.
    Activated,
    /// The process exited; the drawer closes the terminal without confirmation.
    Exited,
    /// "Add to chat" with the selection (label filled in by the drawer).
    AddToChat(TerminalContext),
}

enum PaneState {
    Running(Arc<Terminal>),
    Failed(String),
}

struct Press {
    x: f32,
    y: f32,
    click_count: usize,
    shift: bool,
    /// The press went to the app (mouse reporting).
    reporting: Option<MouseButtonCode>,
    moved: bool,
}

pub struct TerminalPane {
    pub terminal_id: String,
    state: PaneState,
    snapshot: Arc<TerminalSnapshot>,
    focus: FocusHandle,
    focused: bool,
    selection: Option<Selection>,
    /// Text of the selection when it was made (cleared if output changes it).
    selection_text: Option<String>,
    selecting: bool,
    layout: Rc<Cell<GridLayout>>,
    cursor_on: bool,
    blink_epoch: usize,
    wheel_accum: f32,
    press: Option<Press>,
    hovered_link: Option<ViewportLink>,
    links: Option<(u64, Rc<Vec<ViewportLink>>)>,
    path_exists: HashMap<String, bool>,
    generation: u64,
    scrollbar_hover: bool,
    /// Grab offset inside the thumb while dragging it.
    scrollbar_drag: Option<f32>,
    exited: bool,
    cwd: PathBuf,
    scheme_dark: Option<bool>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl EventEmitter<PaneEvent> for TerminalPane {}

impl Focusable for TerminalPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TerminalPane {
    /// Spawns the shell. A spawn failure is shown in the pane.
    pub fn new(
        terminal_id: String,
        options: TerminalOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cwd = options.cwd.clone();
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<()>();
        let state = match Terminal::spawn(
            options,
            Box::new(move || {
                let _ = tx.unbounded_send(());
            }),
        ) {
            Ok(t) => PaneState::Running(Arc::new(t)),
            Err(err) => PaneState::Failed(format!("{err:#}")),
        };
        let mut tasks = Vec::new();
        if matches!(state, PaneState::Running(_)) {
            tasks.push(cx.spawn(async move |this, cx| {
                while rx.next().await.is_some() {
                    while rx.try_recv().is_ok() {}
                    if this.update(cx, |p, cx| p.refresh(cx)).is_err() {
                        break;
                    }
                    cx.background_executor().timer(FRAME_PACE).await;
                }
            }));
        }
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus, window, |p, _, cx| p.set_focused(true, cx)),
            cx.on_blur(&focus, window, |p, _, cx| p.set_focused(false, cx)),
        ];
        let mut pane = Self {
            terminal_id,
            state,
            snapshot: Arc::new(TerminalSnapshot::default()),
            focus,
            focused: false,
            selection: None,
            selection_text: None,
            selecting: false,
            layout: Rc::new(Cell::new(GridLayout::default())),
            cursor_on: true,
            blink_epoch: 0,
            wheel_accum: 0.,
            press: None,
            hovered_link: None,
            links: None,
            path_exists: HashMap::new(),
            generation: 0,
            scrollbar_hover: false,
            scrollbar_drag: None,
            exited: false,
            cwd,
            scheme_dark: None,
            _tasks: tasks,
            _subscriptions: subscriptions,
        };
        pane.refresh(cx);
        pane
    }

    pub fn terminal(&self) -> Option<&Arc<Terminal>> {
        match &self.state {
            PaneState::Running(t) => Some(t),
            PaneState::Failed(_) => None,
        }
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn has_selection(&self) -> bool {
        self.selection
            .as_ref()
            .is_some_and(|s| s.range(self.snapshot.cols).is_some())
    }

    pub fn cwd(&self) -> &PathBuf {
        &self.cwd
    }

    /// Kills the shell (SIGHUP, SIGTERM, then SIGKILL).
    pub fn close(&self) {
        if let Some(t) = self.terminal() {
            t.close();
        }
    }

    fn scroll_state(&self) -> Scroll {
        Scroll {
            history: self.snapshot.history_size,
            display_offset: self.snapshot.display_offset,
            rows: self.snapshot.rows,
            cols: self.snapshot.cols,
        }
    }

    /// Takes a fresh snapshot after the PTY reported a change.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(term) = self.terminal().cloned() else {
            return;
        };
        let snap = term.snapshot();
        let output = snap.history_size != self.snapshot.history_size
            || snap.cursor != self.snapshot.cursor
            || (snap.display_offset == self.snapshot.display_offset
                && snap.lines != self.snapshot.lines);
        self.snapshot = Arc::new(snap);
        self.generation += 1;
        self.links = None;
        if output {
            self.check_selection();
        }
        if let Some(text) = term.take_clipboard_request() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        let _ = term.take_bell();
        if self.snapshot.exited.is_some() && !self.exited {
            self.exited = true;
            term.write_local(PROCESS_EXITED);
            cx.emit(PaneEvent::Exited);
        }
        cx.notify();
    }

    /// Clears the selection when new output changed the text under it.
    fn check_selection(&mut self) {
        let Some(sel) = &self.selection else {
            return;
        };
        if self.selecting || sel.all {
            return;
        }
        let scroll = self.scroll_state();
        if sel.cell_count(scroll.cols) > SELECTION_CHECK_LIMIT || !sel.fully_visible(scroll) {
            self.clear_selection();
            return;
        }
        let now = self.selected_text();
        if now != self.selection_text {
            self.clear_selection();
        }
    }

    fn clear_selection(&mut self) {
        self.selection = None;
        self.selection_text = None;
        self.selecting = false;
    }

    /// The selection's text (viewport part only; select-all reads everything).
    pub fn selected_text(&self) -> Option<String> {
        let term = self.terminal()?;
        let sel = self.selection.as_ref()?;
        if sel.all {
            return Some(term.all_text());
        }
        let (a, b) = sel.viewport_range(self.scroll_state())?;
        Some(term.text_in_range(a, b))
    }

    fn set_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        if let Some(t) = self.terminal() {
            t.focus_changed(focused);
        }
        self.cursor_on = true;
        if focused {
            self.restart_blink(cx);
        } else {
            self.blink_epoch += 1;
        }
        cx.notify();
    }

    fn restart_blink(&mut self, cx: &mut Context<Self>) {
        self.blink_epoch += 1;
        self.cursor_on = true;
        let epoch = self.blink_epoch;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK_INTERVAL).await;
                let alive = this
                    .update(cx, |p, cx| {
                        if p.blink_epoch != epoch || !p.focused {
                            return false;
                        }
                        if p.snapshot.cursor_blinking {
                            p.cursor_on = !p.cursor_on;
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    // --- keys ----------------------------------------------------------------

    fn key_down(&mut self, e: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(term) = self.terminal().cloned() else {
            return;
        };
        let ks = &e.keystroke;
        let press = KeyPress {
            key: ks.key.clone(),
            key_char: ks.key_char.clone(),
            ctrl: ks.modifiers.control,
            alt: ks.modifiers.alt,
            shift: ks.modifiers.shift,
            cmd: ks.modifiers.platform,
        };
        match input::classify_key(&press, self.has_selection()) {
            KeyAction::CopyAndClear => {
                self.copy(cx);
                self.clear_selection();
            }
            KeyAction::Text(text) => {
                term.scroll_to_bottom();
                term.write(text.as_bytes());
            }
            KeyAction::Key => {
                if !term.send_key(&press.key, press.ctrl, press.alt, press.shift, press.cmd) {
                    return;
                }
            }
            KeyAction::Ignore => return,
        }
        self.cursor_on = true;
        self.restart_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_text().filter(|t| !t.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub fn paste_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(t) = self.terminal() {
            t.paste(text);
        }
        cx.notify();
    }

    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
            self.paste_text(&text, cx);
        }
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selection = Some(Selection::all(self.scroll_state()));
        self.selection_text = None;
        cx.notify();
    }

    pub fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
        if let Some(t) = self.terminal() {
            t.scroll_to_bottom();
        }
        cx.notify();
    }

    // --- mouse ---------------------------------------------------------------

    fn grid(&self) -> GridLayout {
        self.layout.get()
    }

    fn link_at(&mut self, cell: (u16, u16)) -> Option<ViewportLink> {
        let term = self.terminal()?.clone();
        let links = match &self.links {
            Some((generation, links)) if *generation == self.generation => links.clone(),
            _ => {
                let links = Rc::new(term.links_in_viewport());
                self.links = Some((self.generation, links.clone()));
                links
            }
        };
        let key = |p: (u16, u16)| (p.1, p.0);
        let link = links
            .iter()
            .find(|l| key(l.start) <= key(cell) && key(cell) < key(l.end))?
            .clone();
        if link.kind == LinkKind::Path {
            let cwd = self.cwd.clone();
            let exists = *self
                .path_exists
                .entry(link.text.clone())
                .or_insert_with(|| open::resolve_path(&link.text, &cwd).0.exists());
            if !exists {
                return None;
            }
        }
        Some(link)
    }

    fn open_link(&self, link: &ViewportLink) {
        match link.kind {
            LinkKind::Url => {
                open::open_url(&link.text);
            }
            LinkKind::Path => {
                let (path, line, col) = open::resolve_path(&link.text, &self.cwd);
                if !open::open_path(&path, line, col)
                    && let Some(t) = self.terminal()
                {
                    t.write_local(b"\r\n[terminal] Unable to open path\r\n");
                }
            }
        }
    }

    fn report(
        &self,
        button: Option<MouseButtonCode>,
        kind: MouseKind,
        x: f32,
        y: f32,
        m: gpui::Modifiers,
    ) -> bool {
        let Some(t) = self.terminal() else {
            return false;
        };
        let cell = self.grid().report_cell(x, y);
        match input::mouse_report(button, kind, cell, m.shift, m.alt, m.control, self.snapshot.modes) {
            Some(bytes) => {
                t.write(&bytes);
                true
            }
            None => false,
        }
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        cx.emit(PaneEvent::Activated);
        let (x, y) = (f32::from(e.position.x), f32::from(e.position.y));
        let m = e.modifiers;
        let grid = self.grid();
        let modes = self.snapshot.modes;

        if e.button == MouseButton::Left
            && self.snapshot.history_size > 0
            && grid.in_scrollbar(x, y)
        {
            self.begin_scrollbar_drag(y, cx);
            return;
        }

        if input::mouse_goes_to_app(modes, m.shift, m.control, m.platform) {
            let button = match e.button {
                MouseButton::Left => MouseButtonCode::Left,
                MouseButton::Middle => MouseButtonCode::Middle,
                MouseButton::Right => MouseButtonCode::Right,
                _ => return,
            };
            self.report(Some(button), MouseKind::Press, x, y, m);
            self.press = Some(Press {
                x,
                y,
                click_count: e.click_count,
                shift: m.shift,
                reporting: Some(button),
                moved: false,
            });
            cx.stop_propagation();
            return;
        }

        match e.button {
            MouseButton::Left => {
                let (cell, side) = grid.cell_at(x, y);
                let link_mod = if input::MAC { m.platform } else { m.control };
                if link_mod
                    && let Some(link) = self.link_at(cell)
                {
                    self.open_link(&link);
                    cx.stop_propagation();
                    return;
                }
                self.press = Some(Press {
                    x,
                    y,
                    click_count: e.click_count,
                    shift: m.shift,
                    reporting: None,
                    moved: false,
                });
                let scroll = self.scroll_state();
                let abs = scroll.to_abs(cell);
                match e.click_count {
                    2 | 3 => {
                        let Some(t) = self.terminal().cloned() else {
                            return;
                        };
                        let (a, b) = if e.click_count == 2 {
                            t.word_at(cell)
                        } else {
                            t.line_at(cell.1)
                        };
                        let mode = if e.click_count == 2 {
                            SelectMode::Word
                        } else {
                            SelectMode::Line
                        };
                        let mut sel = Selection::new(scroll.to_abs(a), layout::Side::Left, mode);
                        sel.head = scroll.to_abs(b);
                        sel.head_side = layout::Side::Right;
                        self.selection = Some(sel);
                        self.selecting = true;
                    }
                    _ => {
                        if m.shift
                            && let Some(sel) = self.selection.as_mut()
                        {
                            sel.head = abs;
                            sel.head_side = side;
                        } else {
                            self.selection = Some(Selection::new(abs, side, SelectMode::Simple));
                        }
                        self.selecting = true;
                    }
                }
                cx.notify();
            }
            MouseButton::Right => {
                self.open_context_menu(e.position, window, cx);
            }
            MouseButton::Middle => {
                let text = self
                    .selected_text()
                    .filter(|t| !t.is_empty())
                    .or_else(|| cx.read_from_primary().and_then(|c| c.text()));
                if let Some(text) = text {
                    self.paste_text(&text, cx);
                }
            }
            _ => {}
        }
        cx.stop_propagation();
    }

    fn begin_scrollbar_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        let grid = self.grid();
        let track = grid.pane_height - 2. * layout::SCROLLBAR_INSET_Y;
        let Some((top, height)) = layout::scrollbar_thumb(
            track,
            self.snapshot.rows as usize,
            self.snapshot.history_size,
            self.snapshot.display_offset,
        ) else {
            return;
        };
        let local = y - grid.pane_y - layout::SCROLLBAR_INSET_Y;
        let grab = if local >= top && local <= top + height {
            local - top
        } else {
            height / 2.
        };
        self.scrollbar_drag = Some(grab);
        self.drag_scrollbar(y, cx);
    }

    fn drag_scrollbar(&mut self, y: f32, cx: &mut Context<Self>) {
        let (Some(grab), Some(t)) = (self.scrollbar_drag, self.terminal().cloned()) else {
            return;
        };
        let grid = self.grid();
        let track = grid.pane_height - 2. * layout::SCROLLBAR_INSET_Y;
        let Some((_, height)) = layout::scrollbar_thumb(
            track,
            self.snapshot.rows as usize,
            self.snapshot.history_size,
            self.snapshot.display_offset,
        ) else {
            return;
        };
        let top = y - grid.pane_y - layout::SCROLLBAR_INSET_Y - grab;
        let want = layout::offset_for_thumb(track, height, top, self.snapshot.history_size);
        let delta = want as i32 - self.snapshot.display_offset as i32;
        if delta != 0 {
            t.scroll(delta);
        }
        cx.notify();
    }

    /// Mouse moves anywhere in the window while a press that started here is held.
    fn drag_move(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        let (x, y) = (f32::from(e.position.x), f32::from(e.position.y));
        if self.scrollbar_drag.is_some() {
            self.drag_scrollbar(y, cx);
            return;
        }
        let Some(press) = self.press.as_mut() else {
            return;
        };
        if (x - press.x).abs() >= CLICK_SLOP || (y - press.y).abs() >= CLICK_SLOP {
            press.moved = true;
        }
        if let Some(button) = press.reporting {
            self.report(Some(button), MouseKind::Motion, x, y, e.modifiers);
            return;
        }
        if !self.selecting {
            return;
        }
        let grid = self.grid();
        let (cell, side) = grid.cell_at(x, y);
        let scroll = self.scroll_state();
        let mode = self.selection.as_ref().map(|s| s.mode);
        let head = match mode {
            Some(SelectMode::Word) | Some(SelectMode::Line) => {
                let Some(t) = self.terminal().cloned() else {
                    return;
                };
                let anchor = self.selection.as_ref().unwrap().anchor;
                let abs = scroll.to_abs(cell);
                let forward = (abs.1, abs.0) >= (anchor.1, anchor.0);
                let (a, b) = if mode == Some(SelectMode::Word) {
                    t.word_at(cell)
                } else {
                    t.line_at(cell.1)
                };
                (scroll.to_abs(if forward { b } else { a }), layout::Side::Right)
            }
            _ => (scroll.to_abs(cell), side),
        };
        if let Some(sel) = self.selection.as_mut() {
            if sel.head != head.0 || sel.head_side != head.1 {
                sel.head = head.0;
                sel.head_side = head.1;
                cx.notify();
            }
        }
    }

    fn drag_end(&mut self, e: &MouseUpEvent, cx: &mut Context<Self>) {
        let (x, y) = (f32::from(e.position.x), f32::from(e.position.y));
        if self.scrollbar_drag.take().is_some() {
            cx.notify();
            return;
        }
        let Some(press) = self.press.take() else {
            return;
        };
        if let Some(button) = press.reporting {
            self.report(Some(button), MouseKind::Release, x, y, e.modifiers);
            return;
        }
        if self.selecting {
            self.selecting = false;
            if self.has_selection() {
                self.selection_text = self.selected_text();
                if let Some(text) = self.selection_text.clone().filter(|t| !t.is_empty()) {
                    cx.write_to_primary(ClipboardItem::new_string(text));
                }
            } else {
                self.selection = None;
            }
        }
        // A plain click on a link opens it (no modifier needed, spec 05 §3.11).
        if e.button == MouseButton::Left && !press.moved && press.click_count == 1 && !press.shift {
            let (cell, _) = self.grid().cell_at(x, y);
            if let Some(link) = self.link_at(cell) {
                self.open_link(&link);
            }
        }
        cx.notify();
    }

    fn hover_move(&mut self, e: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if e.pressed_button.is_some() {
            return;
        }
        let (x, y) = (f32::from(e.position.x), f32::from(e.position.y));
        let grid = self.grid();
        let over_bar = self.snapshot.history_size > 0 && grid.in_scrollbar(x, y);
        let mut changed = over_bar != self.scrollbar_hover;
        self.scrollbar_hover = over_bar;
        if self.snapshot.modes.mouse_motion && !e.modifiers.shift {
            self.report(None, MouseKind::Motion, x, y, e.modifiers);
        }
        let link = if over_bar {
            None
        } else {
            let (cell, _) = grid.cell_at(x, y);
            self.link_at(cell)
        };
        if link != self.hovered_link {
            self.hovered_link = link;
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    fn scroll_wheel(&mut self, e: &ScrollEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.terminal().cloned() else {
            return;
        };
        let grid = self.grid();
        let line_h = grid.cell.height.max(1.);
        let delta = f32::from(e.delta.pixel_delta(px(line_h)).y);
        let (lines, rest) = layout::wheel_lines(self.wheel_accum, delta, line_h);
        self.wheel_accum = rest;
        let (x, y) = (f32::from(e.position.x), f32::from(e.position.y));
        match input::wheel_action(lines, grid.report_cell(x, y), self.snapshot.modes, e.modifiers.shift) {
            WheelAction::Scroll(n) => t.scroll(n),
            WheelAction::Send(bytes) => t.write(&bytes),
            WheelAction::None => {}
        }
        cx.stop_propagation();
    }

    fn context(&self, label: &str) -> Option<TerminalContext> {
        let sel = self.selection.as_ref()?;
        let (a, b) = sel.viewport_range(self.scroll_state())?;
        let text = selection::chat_text(&self.selected_text()?);
        if text.is_empty() {
            return None;
        }
        Some(TerminalContext {
            terminal_id: self.terminal_id.clone(),
            terminal_label: label.to_string(),
            line_start: a.1 as u32 + 1,
            line_end: b.1 as u32 + 1,
            text,
        })
    }

    fn open_context_menu(&mut self, position: gpui::Point<gpui::Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let has_selection = self.has_selection();
        let focus = self.focus.clone();
        let item = |label: &'static str, enabled: bool, f: fn(&mut TerminalPane, &mut Context<TerminalPane>)| {
            let this = this.clone();
            let focus = focus.clone();
            MenuItem::new(label).disabled(!enabled).on_select(move |window, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |p, cx| f(p, cx));
                }
                focus.focus(window, cx);
            })
        };
        let menu = Menu::new()
            .item(item("Add to chat", has_selection, |p, cx| {
                // Re-check: output may have scrolled the selection away.
                if let Some(ctx) = p.context("") {
                    p.clear_selection();
                    cx.emit(PaneEvent::AddToChat(ctx));
                    cx.notify();
                }
            }))
            .item(item("Copy", has_selection, |p, cx| p.copy(cx)))
            .item(item("Paste", true, |p, cx| p.paste_clipboard(cx)))
            .item(item("Select all", true, |p, cx| p.select_all(cx)))
            .item(item("Jump to latest", true, |p, cx| p.jump_to_latest(cx)));
        open_menu(
            SharedString::from(format!("terminal-menu-{}", self.terminal_id)).into(),
            PopupAnchor::Point {
                position,
                trigger: None,
            },
            menu,
            window,
            cx,
        );
    }

    fn frame(&mut self, cx: &mut Context<Self>) -> Option<TerminalFrame> {
        let term = self.terminal()?.clone();
        let theme = cx.tern_theme().clone();
        let tokens = theme.colors;
        let dark = theme.is_dark();
        let colors = TermColors::new(&tokens, dark);
        if self.scheme_dark != Some(dark) {
            term.set_color_scheme(colors.scheme());
            self.scheme_dark = Some(dark);
        }
        let colors = colors.with_overrides(&self.snapshot);
        let scroll = self.scroll_state();
        let selection = self
            .selection
            .as_ref()
            .and_then(|s| s.viewport_range(scroll));
        let link = self.hovered_link.as_ref().map(|l| (l.start, l.end));
        Some(TerminalFrame {
            terminal: Some(term),
            snapshot: self.snapshot.clone(),
            colors,
            font_family: SharedString::new_static(font::MONO),
            font_size: px(font::CODE_SIZE),
            focused: self.focused,
            cursor_on: self.cursor_on,
            selection,
            link,
            scrollbar_thumb: tokens.app_scrollbar_thumb,
            scrollbar_thumb_hover: tokens.app_scrollbar_thumb_hover,
            scrollbar_hover: self.scrollbar_hover || self.scrollbar_drag.is_some(),
            layout_sink: self.layout.clone(),
        })
    }
}

/// Registers window-wide move/up listeners while a press that started in a pane is
/// held, so dragging past the pane's edge keeps selecting.
struct DragCapture {
    pane: gpui::WeakEntity<TerminalPane>,
}

impl IntoElement for DragCapture {
    type Element = gpui::Canvas<()>;
    fn into_element(self) -> Self::Element {
        let pane = self.pane;
        gpui::canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let move_pane = pane.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Capture {
                        return;
                    }
                    if let Some(p) = move_pane.upgrade() {
                        p.update(cx, |p, cx| p.drag_move(e, cx));
                    }
                });
                let up_pane = pane.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Capture {
                        return;
                    }
                    if let Some(p) = up_pane.upgrade() {
                        p.update(cx, |p, cx| p.drag_end(e, cx));
                    }
                });
            },
        )
        .absolute()
        .size_full()
    }
}

impl Render for TerminalPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        let frame = self.frame(cx);
        let dragging = self.press.is_some() || self.scrollbar_drag.is_some();
        let cursor = if self.hovered_link.is_some() {
            CursorStyle::PointingHand
        } else if self.scrollbar_hover || self.snapshot.modes.mouse_reporting {
            CursorStyle::Arrow
        } else {
            CursorStyle::IBeam
        };
        let bg = frame
            .as_ref()
            .map(|f| f.colors.background)
            .unwrap_or(t.terminal_background);
        div()
            .id(SharedString::from(format!("terminal-pane-{}", self.terminal_id)))
            .key_context(input::CONTEXT)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(bg)
            .cursor(cursor)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(|p, _: &input::Copy, _, cx| p.copy(cx)))
            .on_action(cx.listener(|p, _: &input::Paste, _, cx| p.paste_clipboard(cx)))
            .on_action(cx.listener(|p, _: &input::SelectAll, _, cx| p.select_all(cx)))
            .on_action(cx.listener(|p, _: &input::ScrollPageUp, _, cx| {
                if let Some(t) = p.terminal() {
                    t.scroll_page_up();
                }
                cx.notify();
            }))
            .on_action(cx.listener(|p, _: &input::ScrollPageDown, _, cx| {
                if let Some(t) = p.terminal() {
                    t.scroll_page_down();
                }
                cx.notify();
            }))
            .on_action(cx.listener(|p, _: &input::ScrollToTop, _, cx| {
                if let Some(t) = p.terminal() {
                    t.scroll_to_top();
                }
                cx.notify();
            }))
            .on_action(cx.listener(|p, _: &input::ScrollToBottom, _, cx| p.jump_to_latest(cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::hover_move))
            .on_scroll(cx.listener(Self::scroll_wheel))
            .on_hover(cx.listener(|p, hovered: &bool, _, cx| {
                if !hovered && (p.hovered_link.is_some() || p.scrollbar_hover) {
                    p.hovered_link = None;
                    p.scrollbar_hover = false;
                    cx.notify();
                }
            }))
            .map(|el| match (frame, &self.state) {
                (Some(frame), _) => el.child(TerminalElement::new(frame)),
                (None, PaneState::Failed(message)) => el.child(
                    div()
                        .p(px(layout::PADDING))
                        .font_family(font::MONO)
                        .text_size(px(font::CODE_SIZE))
                        .line_height(px(18.))
                        .text_color(t.terminal_foreground)
                        .child(format!(
                            "{message}. Close and reopen the terminal to retry."
                        )),
                ),
                (None, _) => el,
            })
            .when(dragging, |el| {
                el.child(DragCapture {
                    pane: cx.entity().downgrade(),
                })
            })
    }
}

/// The foreground job's command name when the shell is running one (Linux: the tty's
/// foreground process group from `/proc/<shell>/stat`).
pub fn running_command(shell_pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{shell_pid}/stat")).ok()?;
        let tpgid = parse_tpgid(&stat)?;
        if tpgid <= 0 || tpgid as u32 == shell_pid {
            return None;
        }
        let comm = std::fs::read_to_string(format!("/proc/{tpgid}/comm")).ok()?;
        let comm = comm.trim();
        (!comm.is_empty()).then(|| comm.to_string())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = shell_pid;
        None
    }
}

/// Field 8 of `/proc/<pid>/stat` (the terminal's foreground process group). The
/// command name may contain spaces and parens, so fields count from the last `)`.
pub fn parse_tpgid(stat: &str) -> Option<i32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(5)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tpgid_from_stat() {
        let stat = "1234 (bash) S 1200 1234 1234 34816 5678 4194304 0 0";
        assert_eq!(parse_tpgid(stat), Some(5678));
        let odd = "99 (my (odd) cmd) R 1 99 99 0 -1 0";
        assert_eq!(parse_tpgid(odd), Some(-1));
        assert_eq!(parse_tpgid("garbage"), None);
    }

    #[test]
    fn context_chip() {
        let c = TerminalContext {
            terminal_id: "term-1".into(),
            terminal_label: "Terminal 1".into(),
            line_start: 4,
            line_end: 6,
            text: "x".into(),
        };
        assert_eq!(c.chip_label(), "Terminal 1 lines 4-6");
    }
}

//! The thread sidebar (spec 02 §2): chrome header, search / scope / new-thread row, the
//! inbox list (drafts, Pinned, Active, the Working and Settled shelves), the footer with the
//! undo notice, and the `/` index redirect (§3). The shell owns the container, its width,
//! the rail and collapse.
//!
//! - `model`: sections, sorting, statuses and time labels (pure, tested).
//! - `drafts`: the unsent-draft registry the composer reports into.
//! - `rows`: row rendering. `menu`: the thread context menu and thread actions.
//! - `hero`: `NoProjectsHero`, placed by the shell when there are no projects.
//!
//! Dev flags for screenshots: `TERN_SIDEBAR_HOVER=<thread id>` draws that row hovered with
//! its hover card, `TERN_SIDEBAR_MENU=<thread id>` opens its context menu.

pub mod drafts;
pub mod hero;
mod menu;
pub mod model;
pub(crate) mod rows;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use chrono::{Timelike as _, Utc};
use gpui::{
    AnyElement, App, AppContext as _, Bounds, ClickEvent, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    Window, div, px,
};
use gpui_component::input::{Escape as InputEscape, InputEvent, InputState, MoveDown, MoveUp};
use tern_core::{Command, ProjectId, Snapshot, ThreadId, Ts};

pub use drafts::Drafts;
pub use hero::NoProjectsHero;
use model::{DeriveOptions, Row, Section, Sections};

use crate::app_state::{AppState, Route};
use crate::keymap;
use crate::store::Store;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, layout, radius};
use crate::ui::{Icon, IconName, Toast, Toasts, Tooltip, TooltipExt as _, h_flex, v_flex};
use crate::views::command_palette::{self, PaletteIntent};

/// How long an undo notice stays (`showThreadUndoNotice.ts:163-177`).
const UNDO_TTL: Duration = Duration::from_secs(5);

/// One keyboard-focusable thing in the list, in visible order.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RowKey {
    Draft(ProjectId),
    Thread(ThreadId),
    Shelf(Section),
}

/// The live undo notice in the footer (§2.11).
pub(crate) struct UndoNotice {
    pub label: String,
    pub undo: Vec<Command>,
    pub expires: Instant,
}

pub(crate) struct Rename {
    pub thread_id: ThreadId,
    pub input: Entity<InputState>,
    pub original: String,
    _subs: Vec<Subscription>,
}

/// Dev-only forced states for screenshots.
#[derive(Default)]
struct Forced {
    hover: Option<ThreadId>,
    menu: Option<ThreadId>,
    done: bool,
}

pub struct SidebarView {
    store: Entity<Store>,
    app: Entity<AppState>,
    drafts: Entity<Drafts>,
    focus: FocusHandle,
    search: Entity<InputState>,
    query: String,
    /// Highlighted search result.
    search_highlight: usize,
    /// Projects the list is scoped to; empty = all.
    scope: Vec<ProjectId>,
    working_expanded: bool,
    settled_expanded: bool,
    settled_shown: usize,
    hovered: Option<RowKey>,
    /// Keyboard focus inside the list.
    focused_row: Option<RowKey>,
    selection: Vec<ThreadId>,
    anchor: Option<ThreadId>,
    renaming: Option<Rename>,
    undo: Option<UndoNotice>,
    /// Manual pinned order, written by drag.
    pin_order: HashMap<ThreadId, usize>,
    drop_target: Option<RowKey>,
    scroll: ScrollHandle,
    now: Ts,
    forced: Forced,
    forced_bounds: Rc<Cell<Bounds<Pixels>>>,
    last_route: Route,
    /// Whether the list had keyboard focus at the last render.
    list_focused: bool,
    _tick: Task<()>,
    _subs: Vec<Subscription>,
}

impl SidebarView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = Store::global(cx);
        let app = AppState::global(cx);
        let drafts = Drafts::global(cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let mut subs = vec![
            cx.observe(&store, |this, _, cx| {
                this.resolve_index(cx);
                cx.notify();
            }),
            cx.observe(&app, |this, _, cx| {
                this.on_route_change(cx);
                cx.notify();
            }),
            cx.observe(&drafts, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    this.query = input.read(cx).value().to_string();
                    this.search_highlight = 0;
                    cx.notify();
                }
                if let InputEvent::PressEnter { .. } = event {
                    this.open_search_result(cx);
                }
            }),
        ];
        subs.shrink_to_fit();

        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let alive = this.update(cx, |this, cx| this.tick(cx));
                if alive.is_err() {
                    break;
                }
            }
        });

        let forced = Forced {
            hover: std::env::var("TERN_SIDEBAR_HOVER").ok(),
            menu: std::env::var("TERN_SIDEBAR_MENU").ok(),
            done: false,
        };

        let this = Self {
            last_route: app.read(cx).chat_route().clone(),
            store,
            app,
            drafts,
            focus: cx.focus_handle(),
            search,
            query: String::new(),
            search_highlight: 0,
            scope: Vec::new(),
            working_expanded: false,
            settled_expanded: false,
            settled_shown: model::SETTLED_FIRST_PAGE,
            hovered: forced.hover.clone().map(RowKey::Thread),
            focused_row: None,
            selection: Vec::new(),
            anchor: None,
            renaming: None,
            undo: None,
            pin_order: HashMap::new(),
            drop_target: None,
            scroll: ScrollHandle::new(),
            now: Utc::now(),
            forced,
            forced_bounds: Rc::new(Cell::new(Bounds::default())),
            list_focused: false,
            _tick: tick,
            _subs: subs,
        };
        this.register_commands(cx);
        this
    }

    // --- data ----------------------------------------------------------------

    fn snapshot<'a>(&self, cx: &'a App) -> &'a Snapshot {
        self.store.read(cx).snapshot()
    }

    pub(crate) fn sections(&self, cx: &App) -> Sections {
        model::derive(
            self.snapshot(cx),
            &DeriveOptions {
                scope: &self.scope,
                working_shelf: true,
                pin_order: &self.pin_order,
            },
        )
    }

    fn open_thread_id(&self, cx: &App) -> Option<ThreadId> {
        self.app.read(cx).chat_route().thread_id().cloned()
    }

    fn open_draft_project(&self, cx: &App) -> Option<ProjectId> {
        match self.app.read(cx).chat_route() {
            Route::Draft(p) => Some(p.clone()),
            _ => None,
        }
    }

    /// Every keyboard-reachable row in display order (drafts first).
    pub(crate) fn visible_keys(&self, sections: &Sections, cx: &App) -> Vec<RowKey> {
        let open = self.open_thread_id(cx);
        let mut keys: Vec<RowKey> = self
            .drafts
            .read(cx)
            .project_drafts()
            .into_iter()
            .filter(|(p, _)| self.scope.is_empty() || self.scope.contains(p))
            .map(|(p, _)| RowKey::Draft(p))
            .collect();
        let thread = |r: &Row| RowKey::Thread(r.thread_id.clone());
        keys.extend(sections.pinned.iter().map(thread));
        keys.extend(sections.active.iter().map(thread));
        if !sections.working.is_empty() {
            keys.push(RowKey::Shelf(Section::Working));
            keys.extend(
                sections
                    .working
                    .iter()
                    .filter(|r| self.working_expanded || Some(&r.thread_id) == open.as_ref())
                    .map(thread),
            );
        }
        keys.push(RowKey::Shelf(Section::Settled));
        keys.extend(
            sections
                .settled
                .iter()
                .enumerate()
                .filter(|(i, r)| {
                    (self.settled_expanded && *i < self.settled_shown)
                        || Some(&r.thread_id) == open.as_ref()
                })
                .map(|(_, r)| thread(r)),
        );
        keys
    }

    /// Thread ids in visible order with their sections (navigation after settle/archive).
    fn visible_threads(&self, cx: &App) -> Vec<(ThreadId, Section)> {
        let sections = self.sections(cx);
        let mut out = Vec::new();
        for list in [
            &sections.pinned,
            &sections.active,
            &sections.working,
            &sections.settled,
        ] {
            out.extend(list.iter().map(|r| (r.thread_id.clone(), r.section)));
        }
        out
    }

    // --- routing ---------------------------------------------------------------

    /// `/` resolves to a draft in the most recent project (§3).
    fn resolve_index(&mut self, cx: &mut Context<Self>) {
        if *self.app.read(cx).chat_route() != Route::Index
            || self.app.read(cx).route().is_settings()
        {
            return;
        }
        if let Some(project) = model::index_project(self.snapshot(cx)) {
            self.app
                .update(cx, |s, cx| s.replace(Route::Draft(project), cx));
        }
    }

    fn on_route_change(&mut self, cx: &mut Context<Self>) {
        let route = self.app.read(cx).chat_route().clone();
        if route == self.last_route {
            return;
        }
        self.last_route = route.clone();
        self.resolve_index(cx);
        // Opening a thread marks it visited (clears Done / Failed).
        if let Route::Thread(id) = &route
            && self.snapshot(cx).thread(id).is_some_and(|s| s.is_unread())
        {
            self.store.read(cx).send(Command::MarkVisited {
                thread_id: id.clone(),
            });
        }
    }

    pub(crate) fn navigate(&self, route: Route, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| s.navigate(route, cx));
    }

    pub(crate) fn open_thread(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selection.clear();
        self.anchor = Some(id.to_string());
        self.focused_row = Some(RowKey::Thread(id.to_string()));
        self.navigate(Route::Thread(id.to_string()), cx);
    }

    /// The project new threads go to: the viewed thread's, else the open draft's, else
    /// the most recent one.
    pub(crate) fn contextual_project(&self, cx: &App) -> Option<ProjectId> {
        let snap = self.snapshot(cx);
        if let Some(id) = self.open_thread_id(cx)
            && let Some(shell) = snap.thread(&id)
        {
            return Some(shell.thread.project_id.clone());
        }
        if let Some(p) = self.open_draft_project(cx)
            && snap.project(&p).is_some()
        {
            return Some(p);
        }
        model::index_project(snap)
    }

    /// The header's new-thread button and `chat.new` (`Sidebar.logic.ts:816-851`).
    pub(crate) fn new_thread(&mut self, shift: bool, cx: &mut Context<Self>) {
        let projects = model::ordered_projects(self.snapshot(cx)).len();
        if self.scope.len() == 1 {
            let p = self.scope[0].clone();
            self.navigate(Route::Draft(p), cx);
            return;
        }
        if shift || projects <= 1 {
            match self.contextual_project(cx) {
                Some(p) => self.navigate(Route::Draft(p), cx),
                None => command_palette::open(PaletteIntent::AddProject, cx),
            }
            return;
        }
        command_palette::open(PaletteIntent::NewThreadIn, cx);
    }

    // --- commands --------------------------------------------------------------

    fn register_commands(&self, cx: &mut Context<Self>) {
        let me = cx.entity();
        keymap::on_command_for(
            &me,
            "chat.new",
            |this, _, cx| this.new_thread(false, cx),
            cx,
        );
        keymap::on_command_for(
            &me,
            "chat.newLocal",
            |this, _, cx| this.new_thread(true, cx),
            cx,
        );
        keymap::on_command_for(
            &me,
            "thread.previous",
            |this, _, cx| this.step_thread(-1, cx),
            cx,
        );
        keymap::on_command_for(
            &me,
            "thread.next",
            |this, _, cx| this.step_thread(1, cx),
            cx,
        );
        for n in 1..=9usize {
            keymap::on_command_for(
                &me,
                &format!("thread.jump.{n}"),
                move |this, _, cx| this.jump_to(n, cx),
                cx,
            );
        }
        keymap::on_command_for(
            &me,
            "thread.settle",
            |this, window, cx| {
                let ids = this.command_targets(cx);
                if let Some(first) = ids.first() {
                    let settled = this
                        .snapshot(cx)
                        .thread(first)
                        .is_some_and(|s| s.thread.settled_at.is_some());
                    this.set_settled(ids, !settled, window, cx);
                }
            },
            cx,
        );
        keymap::on_command_for(
            &me,
            "thread.pin",
            |this, window, cx| {
                let ids = this.command_targets(cx);
                if let Some(first) = ids.first() {
                    let pinned = this
                        .snapshot(cx)
                        .thread(first)
                        .is_some_and(|s| s.thread.pinned_at.is_some());
                    this.set_pinned(ids, !pinned, window, cx);
                }
            },
            cx,
        );
        keymap::on_command_for(&me, "thread.undo", |this, _, cx| this.undo(cx), cx);
        keymap::on_command_for(
            &me,
            "thread.regenerateTitle",
            |this, _, cx| {
                for id in this.command_targets(cx) {
                    this.store
                        .read(cx)
                        .send(Command::RegenerateTitle { thread_id: id });
                }
            },
            cx,
        );
        // Ctrl+Z reaches the undo even from a text field while the notice is live.
        let weak = me.downgrade();
        keymap::add_context_provider(
            move |_, cx, ctx| {
                if weak.upgrade().is_some_and(|s| {
                    s.read(cx)
                        .undo
                        .as_ref()
                        .is_some_and(|u| u.expires > Instant::now())
                }) {
                    ctx.remove("editableFocus");
                }
            },
            cx,
        );
    }

    /// Shortcuts act on the selection, else the open thread.
    fn command_targets(&self, cx: &App) -> Vec<ThreadId> {
        if !self.selection.is_empty() {
            return self.selection.clone();
        }
        self.open_thread_id(cx).into_iter().collect()
    }

    /// Shortcuts are ignored while the palette is open.
    fn shortcuts_blocked(&self, cx: &App) -> bool {
        self.app.read(cx).palette_open
    }

    fn openable_keys(&self, cx: &App) -> Vec<RowKey> {
        let sections = self.sections(cx);
        self.visible_keys(&sections, cx)
            .into_iter()
            .filter(|k| !matches!(k, RowKey::Shelf(_)))
            .collect()
    }

    fn open_key(&mut self, key: &RowKey, cx: &mut Context<Self>) {
        match key {
            RowKey::Thread(id) => self.open_thread(&id.clone(), cx),
            RowKey::Draft(p) => self.navigate(Route::Draft(p.clone()), cx),
            RowKey::Shelf(section) => self.toggle_shelf(*section, cx),
        }
    }

    /// `thread.previous` / `thread.next`: the adjacent row in visible order.
    fn step_thread(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.shortcuts_blocked(cx) {
            return;
        }
        let keys = self.openable_keys(cx);
        if keys.is_empty() {
            return;
        }
        let current = match self.app.read(cx).chat_route() {
            Route::Thread(id) => Some(RowKey::Thread(id.clone())),
            Route::Draft(p) => Some(RowKey::Draft(p.clone())),
            _ => None,
        };
        let at = current.and_then(|c| keys.iter().position(|k| *k == c));
        let next = match at {
            Some(i) => (i as isize + delta).clamp(0, keys.len() as isize - 1) as usize,
            None if delta > 0 => 0,
            None => keys.len() - 1,
        };
        let key = keys[next].clone();
        self.open_key(&key, cx);
    }

    fn jump_to(&mut self, n: usize, cx: &mut Context<Self>) {
        if self.shortcuts_blocked(cx) {
            return;
        }
        if let Some(key) = self.openable_keys(cx).get(n - 1).cloned() {
            self.open_key(&key, cx);
        }
    }

    pub(crate) fn toggle_shelf(&mut self, section: Section, cx: &mut Context<Self>) {
        match section {
            Section::Working => self.working_expanded = !self.working_expanded,
            Section::Settled => {
                self.settled_expanded = !self.settled_expanded;
                self.settled_shown = model::SETTLED_FIRST_PAGE;
            }
            _ => {}
        }
        cx.notify();
    }

    // --- undo ------------------------------------------------------------------

    pub(crate) fn show_undo(&mut self, label: String, undo: Vec<Command>, cx: &mut Context<Self>) {
        self.undo = Some(UndoNotice {
            label,
            undo,
            expires: Instant::now() + UNDO_TTL,
        });
        cx.notify();
    }

    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(notice) = self.undo.take() else {
            return;
        };
        if notice.expires < Instant::now() {
            return;
        }
        for command in notice.undo {
            self.store.read(cx).send(command);
        }
        cx.notify();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let now = Utc::now();
        let live = self
            .snapshot(cx)
            .threads
            .iter()
            .any(|s| model::row_status(s).is_live());
        let expired = self
            .undo
            .as_ref()
            .is_some_and(|u| u.expires < Instant::now());
        if expired {
            self.undo = None;
        }
        if live || expired || now.minute() != self.now.minute() {
            self.now = now;
            cx.notify();
        }
    }

    // --- search ------------------------------------------------------------------

    fn search_results(&self, cx: &App) -> Vec<ThreadId> {
        use crate::views::command_palette::search::{ThreadSubject, fold_query, score_thread};
        let query = fold_query(&self.query);
        if query.is_empty() {
            return Vec::new();
        }
        let snap = self.snapshot(cx);
        let now_ms = self.now.timestamp_millis();
        let score = |typos: bool| -> Vec<(ThreadId, f32, f32)> {
            snap.threads
                .iter()
                .filter(|s| {
                    s.thread.archived_at.is_none()
                        && s.thread.deleted_at.is_none()
                        && (self.scope.is_empty() || self.scope.contains(&s.thread.project_id))
                })
                .filter_map(|s| {
                    let project = snap.project(&s.thread.project_id).map(|p| p.title.as_str());
                    let subject = ThreadSubject {
                        title: &s.thread.title,
                        branch: s.thread.branch.as_deref(),
                        project_title: project,
                        pull_request: s
                            .thread
                            .linked_pull_request
                            .as_ref()
                            .map(|pr| (pr.number, pr.title.as_str())),
                        activity_at_ms: Some(model::activity_at(s).timestamp_millis()),
                    };
                    let m = score_thread(&query, &subject, now_ms, typos)?;
                    Some((s.thread.id.clone(), m.quality, m.score))
                })
                .collect()
        };
        let mut hits = score(false);
        if hits.is_empty() {
            hits = score(true);
        }
        hits.sort_by(|a, b| {
            let class = |q: f32| command_palette::search::match_class(q).min(2);
            class(b.1)
                .cmp(&class(a.1))
                .then(b.2.total_cmp(&a.2))
                .then(a.0.cmp(&b.0))
        });
        hits.into_iter().map(|(id, _, _)| id).collect()
    }

    fn open_search_result(&mut self, cx: &mut Context<Self>) {
        let results = self.search_results(cx);
        if let Some(id) = results
            .get(self.search_highlight)
            .or(results.first())
            .cloned()
        {
            self.open_thread(&id, cx);
        }
    }

    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.query.clear();
        self.search_highlight = 0;
        cx.notify();
    }

    // --- keyboard ----------------------------------------------------------------

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming.is_some() || !self.focus.is_focused(window) {
            return;
        }
        let ks = &event.keystroke;
        let m = ks.modifiers;
        if m.control || m.alt || m.platform {
            return;
        }
        let sections = self.sections(cx);
        let keys = self.visible_keys(&sections, cx);
        let at = self
            .focused_row
            .as_ref()
            .and_then(|f| keys.iter().position(|k| k == f));
        let mut handled = true;
        match ks.key.as_str() {
            "up" | "down" | "home" | "end" if !keys.is_empty() => {
                let next = match ks.key.as_str() {
                    "up" => at.map_or(keys.len() - 1, |i| i.saturating_sub(1)),
                    "down" => at.map_or(0, |i| (i + 1).min(keys.len() - 1)),
                    "home" => 0,
                    _ => keys.len() - 1,
                };
                self.focused_row = Some(keys[next].clone());
                cx.notify();
            }
            "enter" | "space" => {
                if let Some(key) = self.focused_row.clone() {
                    self.open_key(&key, cx);
                }
            }
            "f2" => {
                if let Some(RowKey::Thread(id)) = self.focused_row.clone() {
                    self.start_rename(&id, window, cx);
                }
            }
            "f10" if m.shift => self.open_focused_menu(window, cx),
            "menu" => self.open_focused_menu(window, cx),
            "right" | "left" => {
                let expand = ks.key == "right";
                match self.focused_row.clone() {
                    Some(RowKey::Shelf(section)) => {
                        let open = match section {
                            Section::Working => self.working_expanded,
                            _ => self.settled_expanded,
                        };
                        if open != expand {
                            self.toggle_shelf(section, cx);
                        } else if expand
                            && let Some(i) = at
                            && let Some(child) = keys.get(i + 1)
                        {
                            self.focused_row = Some(child.clone());
                        }
                    }
                    Some(RowKey::Thread(id)) if !expand => {
                        // Jump to the shelf header the row lives under.
                        let section = sections
                            .working
                            .iter()
                            .chain(&sections.settled)
                            .find(|r| r.thread_id == id)
                            .map(|r| r.section);
                        if let Some(section) = section {
                            self.focused_row = Some(RowKey::Shelf(section));
                        }
                    }
                    _ => handled = false,
                }
                cx.notify();
            }
            "escape" if !self.selection.is_empty() => {
                self.selection.clear();
                cx.notify();
            }
            _ => handled = false,
        }
        if handled {
            cx.stop_propagation();
        }
    }

    fn open_focused_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(RowKey::Thread(id)) = self.focused_row.clone() {
            let at = window.mouse_position();
            self.open_context_menu(&id, at, window, cx);
        }
    }

    // --- rows ----------------------------------------------------------------------

    pub(crate) fn click_row(
        &mut self,
        id: &str,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus.focus(window, cx);
        let m = event.modifiers();
        if m.control || m.platform {
            if let Some(i) = self.selection.iter().position(|s| s == id) {
                self.selection.remove(i);
            } else {
                if self.selection.is_empty()
                    && let Some(open) = self.open_thread_id(cx)
                    && open != id
                {
                    self.selection.push(open);
                }
                self.selection.push(id.to_string());
            }
            self.anchor = Some(id.to_string());
            cx.notify();
            return;
        }
        if m.shift
            && let Some(anchor) = self.anchor.clone().or_else(|| self.open_thread_id(cx))
        {
            let order: Vec<ThreadId> = self
                .openable_keys(cx)
                .into_iter()
                .filter_map(|k| match k {
                    RowKey::Thread(id) => Some(id),
                    _ => None,
                })
                .collect();
            if let (Some(a), Some(b)) = (
                order.iter().position(|x| *x == anchor),
                order.iter().position(|x| x == id),
            ) {
                let (lo, hi) = (a.min(b), a.max(b));
                self.selection = order[lo..=hi].to_vec();
                cx.notify();
                return;
            }
        }
        if event.click_count() >= 2 {
            // The second click of a double-click renames instead of reopening.
            self.start_rename(id, window, cx);
            return;
        }
        if self.open_thread_id(cx).as_deref() == Some(id) && self.selection.is_empty() {
            self.focused_row = Some(RowKey::Thread(id.to_string()));
            cx.notify();
            return;
        }
        self.open_thread(id, cx);
    }

    pub(crate) fn set_hovered(&mut self, key: RowKey, hovered: bool, cx: &mut Context<Self>) {
        if self.forced.hover.is_some() {
            return;
        }
        if hovered {
            if self.hovered.as_ref() != Some(&key) {
                self.hovered = Some(key);
                cx.notify();
            }
        } else if self.hovered.as_ref() == Some(&key) {
            self.hovered = None;
            cx.notify();
        }
    }

    // --- rename --------------------------------------------------------------------

    pub(crate) fn start_rename(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(shell) = self.snapshot(cx).thread(id) else {
            return;
        };
        let title = shell.thread.title.clone();
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(title.clone(), window, cx);
            state
        });
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        let subs =
            vec![
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::PressEnter { .. } | InputEvent::Blur => {
                            this.commit_rename(window, cx)
                        }
                        _ => {}
                    }
                }),
            ];
        self.renaming = Some(Rename {
            thread_id: id.to_string(),
            input,
            original: title,
            _subs: subs,
        });
        cx.notify();
    }

    pub(crate) fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.renaming.take() else {
            return;
        };
        let title = rename.input.read(cx).value().trim().to_string();
        if title.is_empty() {
            Toasts::push(Toast::warning("Thread title cannot be empty"), cx);
        } else if title != rename.original {
            self.store.read(cx).send(Command::RenameThread {
                thread_id: rename.thread_id.clone(),
                title,
            });
        }
        self.focused_row = Some(RowKey::Thread(rename.thread_id));
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(crate) fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(rename) = self.renaming.take() {
            self.focused_row = Some(RowKey::Thread(rename.thread_id));
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    // --- drag ------------------------------------------------------------------------

    /// Drops `dragged` before `target` in the pinned list (pinning it if needed).
    pub(crate) fn drop_on_pinned(
        &mut self,
        dragged: &str,
        target: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        self.drop_target = None;
        let sections = self.sections(cx);
        let mut order: Vec<ThreadId> = sections
            .pinned
            .iter()
            .map(|r| r.thread_id.clone())
            .filter(|id| id != dragged)
            .collect();
        let at = target
            .and_then(|t| order.iter().position(|id| id == t))
            .unwrap_or(order.len());
        order.insert(at, dragged.to_string());
        self.pin_order = order
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id, i))
            .collect();
        let shell = self.snapshot(cx).thread(dragged).cloned();
        if let Some(shell) = shell {
            if shell.thread.settled_at.is_some() {
                self.store.read(cx).send(Command::SetSettled {
                    thread_id: dragged.to_string(),
                    settled: false,
                });
            }
            if shell.thread.pinned_at.is_none() {
                self.store.read(cx).send(Command::SetPinned {
                    thread_id: dragged.to_string(),
                    pinned: true,
                });
            }
        }
        cx.notify();
    }

    pub(crate) fn drop_on_settled(
        &mut self,
        dragged: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drop_target = None;
        self.set_settled(vec![dragged.to_string()], true, window, cx);
    }

    pub(crate) fn set_drop_target(&mut self, target: Option<RowKey>, cx: &mut Context<Self>) {
        if self.drop_target != target {
            self.drop_target = target;
            cx.notify();
        }
    }

    // --- forced states -----------------------------------------------------------------

    fn apply_forced(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.forced.done {
            return;
        }
        let bounds = self.forced_bounds.get();
        if bounds.size.width <= px(0.) {
            return;
        }
        self.forced.done = true;
        if let Some(id) = self.forced.hover.clone()
            && let Some(card) = rows::hover_card_tooltip(self, &id, cx)
        {
            crate::ui::tooltip::TooltipLayer::show_now(card, bounds, cx);
        }
        if let Some(id) = self.forced.menu.clone() {
            let at = gpui::point(bounds.origin.x + px(112.), bounds.origin.y + px(44.));
            self.open_context_menu(&id, at, window, cx);
        }
    }
}

impl Focusable for SidebarView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SidebarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        self.list_focused = self.focus.is_focused(window);
        if (self.forced.hover.is_some() || self.forced.menu.is_some()) && !self.forced.done {
            cx.on_next_frame(window, |this, window, cx| this.apply_forced(window, cx));
        }
        let has_projects = !self.snapshot(cx).projects.is_empty();
        let searching = !self.query.trim().is_empty();
        let content: AnyElement = if searching {
            rows::search_results(self, cx)
        } else if !has_projects {
            rows::empty_state(self, cx)
        } else {
            rows::list(self, window, cx)
        };
        v_flex()
            .id("sidebar-view")
            .size_full()
            .key_context("Sidebar")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .text_color(t.sidebar_foreground)
            .child(
                // SidebarChromeHeader: h 40, brand at ml 52.
                h_flex()
                    .h(layout::TOPBAR_HEIGHT)
                    .flex_none()
                    .pl(layout::TITLEBAR_CONTENT_LEFT)
                    .pr(px(12.))
                    .child(
                        div()
                            .id("sidebar-brand")
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .rounded(radius::MD)
                            .cursor_pointer()
                            .font_semibold()
                            .text_scale(TextSize::Sm)
                            .text_color(t.sidebar_foreground)
                            .child("Tern")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate(Route::Index, cx);
                                this.resolve_index(cx);
                            })),
                    ),
            )
            .child(rows::thread_header(self, has_projects, window, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(content)
                    .child(rows::scroll_fades(self, cx)),
            )
            .child(rows::footer(self, cx))
    }
}

/// `Tooltip` shorthand for sidebar controls: 150ms open delay.
pub(crate) fn sidebar_tip(text: impl Into<SharedString>) -> Tooltip {
    Tooltip::text(text).delay(crate::theme::motion::TOOLTIP_SIDEBAR_DELAY)
}

/// Icon button 28x28 (SidebarMenuButton size=icon): radius 8, `--sidebar-icon-color`,
/// hover bg `sidebar-row-hover` + `sidebar-foreground`.
pub(crate) fn sidebar_icon_button(
    id: impl Into<gpui::ElementId>,
    icon: IconName,
    size: f32,
    tip: Tooltip,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let t = cx.tokens();
    let id: gpui::ElementId = id.into();
    let group: SharedString = format!("{id:?}").into();
    div()
        .id(id)
        .group(group.clone())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(radius::LG)
        .cursor_pointer()
        .hover(|s| s.bg(t.sidebar_row_hover))
        .child(
            Icon::new(icon)
                .size(px(16.))
                .color(t.sidebar_icon_color)
                .group_hover_color(group, t.sidebar_foreground),
        )
        .t3_tooltip(tip)
}

pub(crate) fn capture_search_keys(el: gpui::Div, cx: &mut Context<SidebarView>) -> gpui::Div {
    el.capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
        let n = this.search_results(cx).len();
        if n > 0 {
            this.search_highlight = (this.search_highlight + 1).min(n - 1);
            cx.notify();
        }
        cx.stop_propagation();
    }))
    .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
        this.search_highlight = this.search_highlight.saturating_sub(1);
        cx.notify();
        cx.stop_propagation();
    }))
    .capture_action(cx.listener(|this, _: &InputEscape, window, cx| {
        this.clear_search(window, cx);
        cx.stop_propagation();
    }))
}

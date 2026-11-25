//! The terminal drawer (spec 05 §3): per-thread terminals that keep running across
//! thread switches, the floating toolbar or the tab sidebar, split groups, the
//! keyboard commands, close confirmation and thread cleanup.
//!
//! The shell places the drawer under the composer and owns its resize handle;
//! `AppState::drawer` holds the current thread's open flag and height. This view
//! keeps every thread's record and swaps it into `AppState` on thread switches.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use tern_core::Snapshot;
use tern_terminal::{TerminalOptions, launch_env};

use super::input::{self, shortcut_label};
use super::model::{GroupKind, SplitDirection, TerminalGroup, ThreadTerminals, next_terminal_id, terminal_label};
use super::pane::{PaneEvent, TerminalContext, TerminalPane, running_command};
use crate::app_state::{AppState, Route};
use crate::store::{Store, StoreEvent};
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, radius};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, ConfirmOptions, Dialogs, Icon, IconName, Tooltip,
    TooltipExt as _, TooltipSide, h_flex, v_flex,
};

/// How often running-command labels are refreshed while the sidebar shows.
const LABEL_POLL: Duration = Duration::from_secs(1);

fn focus_pane(pane: &Entity<TerminalPane>, window: &mut Window, cx: &mut App) {
    let handle = pane.read(cx).focus_handle(cx);
    handle.focus(window, cx);
}

/// Which thread (or draft) a set of terminals belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ScopeKey {
    Thread(String),
    Draft(String),
}

/// The scope for the chat route, if it has a composer.
pub fn scope_for(route: &Route, snapshot: &Snapshot) -> Option<ScopeKey> {
    match route {
        Route::Thread(id) => Some(ScopeKey::Thread(id.clone())),
        Route::Draft(project) => Some(ScopeKey::Draft(project.clone())),
        Route::Index => snapshot
            .projects
            .first()
            .map(|p| ScopeKey::Draft(p.id.clone())),
        _ => None,
    }
}

/// cwd = the thread's worktree, else the project root (spec 05 §3.14).
pub fn launch_paths(key: &ScopeKey, snapshot: &Snapshot) -> Option<(PathBuf, Option<PathBuf>)> {
    match key {
        ScopeKey::Thread(id) => {
            let shell = snapshot.thread(id)?;
            let root = snapshot
                .project(&shell.thread.project_id)
                .map(|p| PathBuf::from(&p.workspace_root))?;
            let worktree = shell.thread.worktree_path.as_ref().map(PathBuf::from);
            Some((root, worktree))
        }
        ScopeKey::Draft(project) => snapshot
            .project(project)
            .map(|p| (PathBuf::from(&p.workspace_root), None)),
    }
}

/// Threads that are gone, archived or deleted lose their terminals.
pub fn scope_alive(key: &ScopeKey, snapshot: &Snapshot) -> bool {
    match key {
        ScopeKey::Thread(id) => snapshot
            .thread(id)
            .is_some_and(|s| s.thread.archived_at.is_none() && s.thread.deleted_at.is_none()),
        ScopeKey::Draft(project) => snapshot.project(project).is_some(),
    }
}

/// The two-line close confirmation (spec 05 §3.7).
pub fn close_message(labels: &[String]) -> String {
    match labels {
        [one] => format!(
            "Close terminal \"{one}\"?\nThis stops the running process and clears its history."
        ),
        many => format!(
            "Close {} terminals?\nThis stops their running processes and clears their histories: {}.",
            many.len(),
            many.iter()
                .map(|l| format!("\"{l}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TerminalDrawerEvent {
    /// "Add to chat" from a terminal's context menu: insert as a composer chip.
    AddToChat(TerminalContext),
}

struct ThreadEntry {
    state: ThreadTerminals,
    panes: HashMap<String, Entity<TerminalPane>>,
    /// Running foreground command per terminal, for labels.
    commands: HashMap<String, String>,
    /// A terminal had focus when the thread was left.
    terminal_had_focus: bool,
    _subscriptions: Vec<Subscription>,
}

impl ThreadEntry {
    fn new(state: ThreadTerminals) -> Self {
        Self {
            state,
            panes: HashMap::new(),
            commands: HashMap::new(),
            terminal_had_focus: false,
            _subscriptions: Vec::new(),
        }
    }

    fn label(&self, id: &str) -> String {
        terminal_label(id, self.commands.get(id).map(String::as_str))
    }
}

pub struct TerminalDrawerView {
    app: Entity<AppState>,
    store: Entity<Store>,
    threads: HashMap<ScopeKey, ThreadEntry>,
    current: Option<ScopeKey>,
    initialized: bool,
    hovered_row: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TerminalDrawerEvent> for TerminalDrawerView {}

impl TerminalDrawerView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = AppState::global(cx);
        let store = Store::global(cx);
        let subscriptions = vec![
            cx.observe_in(&app, window, |this, _, window, cx| this.sync_app(window, cx)),
            cx.subscribe(&store, |this, _, event: &StoreEvent, cx| {
                if matches!(event, StoreEvent::Snapshot) {
                    this.prune(cx);
                }
            }),
        ];
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(LABEL_POLL).await;
                if this.update(cx, |this, cx| this.poll_labels(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut view = Self {
            app,
            store,
            threads: HashMap::new(),
            current: None,
            initialized: false,
            hovered_row: None,
            _subscriptions: subscriptions,
        };
        view.sync_app(window, cx);
        view
    }

    fn entry(&self) -> Option<&ThreadEntry> {
        self.threads.get(self.current.as_ref()?)
    }

    fn entry_mut(&mut self) -> Option<&mut ThreadEntry> {
        self.threads.get_mut(self.current.as_ref()?)
    }

    fn set_app_drawer(&self, open: bool, height: f32, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| {
            if s.drawer.open != open || s.drawer.size != px(height) {
                s.drawer.open = open;
                s.drawer.size = px(height);
                cx.notify();
            }
        });
    }

    /// Mirrors `AppState::drawer` into the current thread, and swaps records when the
    /// chat route moves to another thread.
    fn sync_app(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (route, drawer) = {
            let app = self.app.read(cx);
            (app.chat_route().clone(), app.drawer)
        };
        let key = scope_for(&route, self.store.read(cx).snapshot());
        if key != self.current || !self.initialized {
            let first = !self.initialized;
            self.initialized = true;
            if let Some(old) = self.current.take()
                && let Some(entry) = self.threads.get_mut(&old)
            {
                entry.state.open = drawer.open && !entry.state.is_empty();
                entry.state.height = f32::from(drawer.size);
                entry.terminal_had_focus = entry
                    .panes
                    .values()
                    .any(|p| p.read(cx).is_focused());
            }
            self.current = key.clone();
            let Some(key) = key else {
                return;
            };
            let entry = self
                .threads
                .entry(key)
                .or_insert_with(|| ThreadEntry::new(ThreadTerminals::default()));
            if first {
                // `--layout terminal` and other startup state win over the default record.
                entry.state.open = drawer.open;
                entry.state.height = f32::from(drawer.size);
                if drawer.open && entry.state.is_empty() {
                    entry.state.open = false;
                    self.open_drawer(window, cx);
                }
                return;
            }
            let (open, height, refocus) = (
                entry.state.open,
                entry.state.height,
                entry.terminal_had_focus,
            );
            self.set_app_drawer(open, height, cx);
            if open && refocus {
                self.focus_active(window, cx);
            }
            cx.notify();
            return;
        }
        let Some(entry) = self.entry_mut() else {
            return;
        };
        entry.state.height = f32::from(drawer.size);
        if drawer.open != entry.state.open {
            if drawer.open {
                self.open_drawer(window, cx);
            } else {
                entry.state.open = false;
            }
            cx.notify();
        }
    }

    /// Opens the drawer, launching `term-1` when the thread has no terminals, and
    /// focuses the active terminal.
    fn open_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry_mut() else {
            return;
        };
        entry.state.open = true;
        if entry.state.is_empty() {
            self.new_terminal(window, cx);
        } else {
            let height = entry.state.height;
            self.set_app_drawer(true, height, cx);
            self.focus_active(window, cx);
        }
    }

    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry() else {
            return;
        };
        if let Some(pane) = entry.panes.get(&entry.state.active_terminal_id).cloned() {
            focus_pane(&pane, window, cx);
        }
    }

    fn spawn_pane(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<TerminalPane>> {
        let key = self.current.clone()?;
        let store = self.store.read(cx);
        let snapshot = store.snapshot();
        let (root, worktree) = launch_paths(&key, snapshot).unwrap_or_else(|| {
            (std::env::current_dir().unwrap_or_default(), None)
        });
        let mut cwd = worktree.clone().unwrap_or_else(|| root.clone());
        if !cwd.is_dir() && store.is_demo() {
            // Demo projects live at made-up paths; give the shell somewhere real.
            cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        }
        let options = TerminalOptions {
            cwd,
            env: launch_env(&root, worktree.as_deref().map(Path::new)),
            cols: 0,
            rows: 0,
            thread_id: match &key {
                ScopeKey::Thread(id) => Some(id.clone()),
                ScopeKey::Draft(_) => None,
            },
            ..TerminalOptions::default()
        };
        let terminal_id = id.to_string();
        let pane = cx.new(|cx| TerminalPane::new(terminal_id, options, window, cx));
        let tid = id.to_string();
        let sub = cx.subscribe_in(&pane, window, move |this, _, event: &PaneEvent, window, cx| {
            this.on_pane_event(&tid, event, window, cx);
        });
        let entry = self.threads.get_mut(&key)?;
        entry.panes.insert(id.to_string(), pane.clone());
        entry._subscriptions.push(sub);
        Some(pane)
    }

    fn on_pane_event(&mut self, id: &str, event: &PaneEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            PaneEvent::Activated => {
                if let Some(entry) = self.entry_mut()
                    && entry.state.active_terminal_id != id
                {
                    entry.state.set_active(id);
                    cx.notify();
                }
            }
            PaneEvent::Exited => {
                // On the next tick, close without confirmation (spec 05 §3.7). The
                // terminal may belong to a thread that isn't showing.
                let id = id.to_string();
                cx.defer_in(window, move |this, window, cx| {
                    let key = this
                        .threads
                        .iter()
                        .find(|(_, e)| e.panes.contains_key(&id))
                        .map(|(k, _)| k.clone());
                    if let Some(key) = key {
                        this.close_in(&key, &id, window, cx);
                    }
                });
            }
            PaneEvent::AddToChat(ctx) => {
                let mut ctx = ctx.clone();
                if let Some(entry) = self.entry() {
                    ctx.terminal_label = entry.label(&ctx.terminal_id);
                }
                cx.emit(TerminalDrawerEvent::AddToChat(ctx));
            }
        }
    }

    // --- commands --------------------------------------------------------------

    pub fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry() else {
            return;
        };
        let id = next_terminal_id(entry.state.terminal_ids.iter().map(String::as_str));
        let Some(pane) = self.spawn_pane(&id, window, cx) else {
            return;
        };
        let entry = self.entry_mut().expect("current entry");
        entry.state.new_terminal(&id);
        let height = entry.state.height;
        self.set_app_drawer(true, height, cx);
        focus_pane(&pane, window, cx);
        cx.notify();
    }

    pub fn split(&mut self, direction: SplitDirection, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry() else {
            return;
        };
        if entry.state.is_empty() {
            self.new_terminal(window, cx);
            return;
        }
        if !entry.state.can_split() {
            return;
        }
        let id = next_terminal_id(entry.state.terminal_ids.iter().map(String::as_str));
        let Some(pane) = self.spawn_pane(&id, window, cx) else {
            return;
        };
        let entry = self.entry_mut().expect("current entry");
        entry.state.split(&id, direction);
        let height = entry.state.height;
        self.set_app_drawer(true, height, cx);
        focus_pane(&pane, window, cx);
        cx.notify();
    }

    pub fn activate(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self.entry_mut() {
            entry.state.set_active(id);
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    fn cycle(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry() else {
            return;
        };
        if !entry.state.open {
            return;
        }
        if let Some(id) = entry.state.cycle(delta) {
            self.activate(&id, window, cx);
        }
    }

    /// Asks first (setting `confirmTerminalClose`, default on), then closes.
    pub fn request_close(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry() else {
            return;
        };
        if !entry.state.has(id) {
            return;
        }
        let message = close_message(&[entry.label(id)]);
        let answer = Dialogs::confirm(message, ConfirmOptions::destructive(), window, cx);
        let key = self.current.clone();
        let id = id.to_string();
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.unwrap_or(false) {
                let _ = this.update_in(cx, |this, window, cx| {
                    if let Some(key) = key {
                        this.close_in(&key, &id, window, cx);
                    }
                });
            }
        })
        .detach();
    }

    /// Kills the shell and removes the terminal. The last one closes the drawer.
    fn close_in(&mut self, key: &ScopeKey, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.threads.get_mut(key) else {
            return;
        };
        if let Some(pane) = entry.panes.remove(id) {
            pane.read(cx).close();
        }
        entry.commands.remove(id);
        entry.state.close(id);
        let (empty, height) = (entry.state.is_empty(), entry.state.height);
        if self.current.as_ref() == Some(key) {
            if empty {
                self.set_app_drawer(false, height, cx);
            } else {
                self.focus_active(window, cx);
            }
        }
        cx.notify();
    }

    /// Thread archive or delete kills every shell of that thread.
    fn prune(&mut self, cx: &mut Context<Self>) {
        let dead: Vec<ScopeKey> = {
            let snapshot = self.store.read(cx).snapshot();
            self.threads
                .keys()
                .filter(|k| !scope_alive(k, snapshot))
                .cloned()
                .collect()
        };
        for key in dead {
            if let Some(entry) = self.threads.remove(&key) {
                for pane in entry.panes.values() {
                    pane.read(cx).close();
                }
            }
            if self.current.as_ref() == Some(&key) {
                self.current = None;
                self.initialized = false;
            }
        }
    }

    fn poll_labels(&mut self, cx: &mut Context<Self>) {
        let open = self.app.read(cx).drawer.open;
        let Some(key) = self.current.clone() else {
            return;
        };
        let Some(entry) = self.threads.get(&key) else {
            return;
        };
        if !open || entry.state.len() < 2 {
            return;
        }
        let mut commands = HashMap::new();
        for (id, pane) in &entry.panes {
            if let Some(pid) = pane.read(cx).terminal().and_then(|t| t.pid())
                && let Some(cmd) = running_command(pid)
            {
                commands.insert(id.clone(), cmd);
            }
        }
        let entry = self.threads.get_mut(&key).unwrap();
        if entry.commands != commands {
            entry.commands = commands;
            cx.notify();
        }
    }

    // --- rendering -------------------------------------------------------------

    fn render_group(&self, entry: &ThreadEntry, group: &TerminalGroup, cx: &App) -> AnyElement {
        let t = cx.tokens();
        let side_by_side = group.direction() == SplitDirection::Horizontal;
        let active = &entry.state.active_terminal_id;
        let multi = group.terminal_ids.len() > 1;
        let mut container = div().size_full().flex().overflow_hidden();
        container = if side_by_side {
            container.flex_row()
        } else {
            container.flex_col()
        };
        container
            .children(group.terminal_ids.iter().enumerate().filter_map(|(i, id)| {
                let pane = entry.panes.get(id)?.clone();
                let border = if multi && id == active {
                    t.border
                } else {
                    t.border.opacity(0.7)
                };
                Some(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .overflow_hidden()
                        .border_color(border)
                        .when(i > 0 && side_by_side, |d| d.border_l_1())
                        .when(i > 0 && !side_by_side, |d| d.border_t_1())
                        .child(pane),
                )
            }))
            .into_any_element()
    }

    fn toolbar_button(
        &self,
        id: &'static str,
        icon: IconName,
        tooltip: Tooltip,
        enabled: bool,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        sidebar: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *cx.tokens();
        let hover_bg = if sidebar { t.accent.opacity(0.7) } else { t.accent };
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
            .when(sidebar, |d| d.h_full().px(px(4.)))
            .when(!sidebar, |d| d.p(px(4.)))
            .text_color(t.foreground.opacity(0.9))
            .child(Icon::new(icon).size(px(13.)))
            .when(enabled, |d| {
                d.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
            })
            .when(!enabled, |d| d.opacity(crate::ui::DISABLED_OPACITY).cursor_not_allowed())
            .t3_tooltip(tooltip.side(TooltipSide::Bottom))
            .into_any_element()
    }

    fn toolbar_buttons(&self, sidebar: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let can_split = self.entry().is_none_or(|e| e.state.can_split());
        let tip = |label: &str, command: &str| {
            let tip = Tooltip::text(label.to_string());
            match shortcut_label(command) {
                Some(k) => tip.kbd(k),
                None => tip,
            }
        };
        let split_tip = |label: &str, command: &str| {
            if can_split {
                tip(label, command)
            } else {
                Tooltip::text(format!("{label} (max 4 per group)"))
            }
        };
        vec![
            self.toolbar_button(
                "terminal-split-h",
                IconName::SquareSplitHorizontal,
                split_tip("Split Terminal Horizontally", "terminal.split"),
                can_split,
                |this, window, cx| this.split(SplitDirection::Horizontal, window, cx),
                sidebar,
                cx,
            ),
            self.toolbar_button(
                "terminal-split-v",
                IconName::SquareSplitVertical,
                split_tip("Split Terminal Vertically", "terminal.splitVertical"),
                can_split,
                |this, window, cx| this.split(SplitDirection::Vertical, window, cx),
                sidebar,
                cx,
            ),
            self.toolbar_button(
                "terminal-new",
                IconName::Plus,
                tip("New Terminal", "terminal.new"),
                true,
                |this, window, cx| this.new_terminal(window, cx),
                sidebar,
                cx,
            ),
            self.toolbar_button(
                "terminal-close",
                IconName::Trash2,
                tip("Close Terminal", "terminal.close"),
                true,
                |this, window, cx| {
                    if let Some(id) = this.entry().map(|e| e.state.active_terminal_id.clone()) {
                        this.request_close(&id, window, cx);
                    }
                },
                sidebar,
                cx,
            ),
        ]
    }

    /// Single terminal: the floating pill at the top right.
    fn render_floating_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let shadow = cx.tern_theme().shadows.xs5();
        let divider = || div().h(px(16.)).w(px(1.)).flex_none().bg(t.border.opacity(0.8));
        let mut row = h_flex()
            .absolute()
            .right(px(8.))
            .top(px(8.))
            .rounded(radius::MD)
            .border_1()
            .border_color(t.border.opacity(0.8))
            .bg(t.background)
            .shadow(shadow)
            .overflow_hidden();
        for (i, button) in self.toolbar_buttons(false, cx).into_iter().enumerate() {
            if i > 0 {
                row = row.child(divider());
            }
            row = row.child(button);
        }
        row.into_any_element()
    }

    fn render_sidebar(&self, entry: &ThreadEntry, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let line = t.border.opacity(0.7);
        let mut header = h_flex()
            .h(px(22.))
            .flex_none()
            .justify_end()
            .border_b_1()
            .border_color(line);
        for (i, button) in self.toolbar_buttons(true, cx).into_iter().enumerate() {
            header = header.child(
                div()
                    .h_full()
                    .flex()
                    .when(i > 0, |d| d.border_l_1().border_color(line))
                    .child(button),
            );
        }
        let state = &entry.state;
        let close_shortcut = shortcut_label("terminal.close");
        let groups: Vec<AnyElement> = state
            .sorted_groups()
            .into_iter()
            .map(|group| {
                let active_group = group.id == state.active_group_id;
                let kind = group.kind();
                let icon = match kind {
                    GroupKind::Single => IconName::Square,
                    GroupKind::Stacked => IconName::SquareSplitVertical,
                    GroupKind::SideBySide => IconName::SquareSplitHorizontal,
                };
                let group_id = group.id.clone();
                let header = h_flex()
                    .id(SharedString::from(format!("terminal-group-{}", group.id)))
                    .h(px(22.))
                    .w_full()
                    .gap(px(4.))
                    .rounded(radius::SM)
                    .px(px(6.))
                    .text_scale(TextSize::Xs2)
                    .cursor_pointer()
                    .when(active_group, |d| d.bg(t.accent.opacity(0.5)).text_color(t.foreground))
                    .when(!active_group, |d| {
                        d.text_color(t.muted_foreground)
                            .hover(|s| s.bg(t.accent.opacity(0.4)).text_color(t.foreground))
                    })
                    .child(Icon::new(icon).size(px(12.)))
                    .child(div().flex_1().min_w_0().truncate().child(kind.label()))
                    .child(
                        div()
                            .text_scale(TextSize::Xs3)
                            .text_color(t.muted_foreground.opacity(0.7))
                            .child(group.terminal_ids.len().to_string()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(entry) = this.entry_mut() {
                            entry.state.activate_group(&group_id);
                        }
                        this.focus_active(window, cx);
                        cx.notify();
                    }));
                let rows = group.terminal_ids.iter().map(|id| {
                    let active = *id == state.active_terminal_id;
                    let hovered = self.hovered_row.as_deref() == Some(id.as_str());
                    let label = entry.label(id);
                    let close_tip = match (&close_shortcut, active) {
                        (Some(k), true) => Tooltip::text(format!("Close {label}")).kbd(k.clone()),
                        _ => Tooltip::text(format!("Close {label}")),
                    };
                    let (close_id, activate_id, hover_id) = (id.clone(), id.clone(), id.clone());
                    h_flex()
                        .id(SharedString::from(format!("terminal-tab-{id}")))
                        .h(px(24.))
                        .w_full()
                        .gap(px(2.))
                        .rounded(radius::MD)
                        .pr(px(8.))
                        .pl(px(6.))
                        .text_scale(TextSize::Xs)
                        .when(active, |d| d.bg(t.accent).text_color(t.foreground))
                        .when(!active, |d| {
                            d.text_color(t.muted_foreground)
                                .hover(|s| s.bg(t.accent.opacity(0.6)).text_color(t.foreground))
                        })
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            let next = hovered.then(|| hover_id.clone());
                            if this.hovered_row != next && (*hovered || this.hovered_row.as_deref() == Some(hover_id.as_str())) {
                                this.hovered_row = next;
                                cx.notify();
                            }
                        }))
                        .child(
                            div()
                                .id(SharedString::from(format!("terminal-tab-close-{id}")))
                                .size(px(16.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(radius::SM)
                                .hover(|s| s.bg(t.muted))
                                .child(Icon::new(if hovered {
                                    IconName::X
                                } else {
                                    IconName::SquareTerminal
                                })
                                .size(px(12.)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.request_close(&close_id, window, cx);
                                }))
                                .t3_tooltip(close_tip.side(TooltipSide::Bottom)),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("terminal-tab-label-{id}")))
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .child(div().truncate().child(label))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.activate(&activate_id, window, cx);
                                })),
                        )
                        .into_any_element()
                });
                v_flex()
                    .pb(px(2.))
                    .child(header)
                    .child(v_flex().gap(px(2.)).children(rows))
                    .into_any_element()
            })
            .collect();
        v_flex()
            .w(px(super::layout::SIDEBAR_WIDTH))
            .min_w(px(super::layout::SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .border_1()
            .border_color(line)
            .bg(t.muted.opacity(0.1))
            .child(header)
            .child(
                v_flex()
                    .id("terminal-tab-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(4.))
                    .py(px(4.))
                    .children(groups),
            )
            .into_any_element()
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .px(px(16.))
            .py(px(24.))
            .text_scale(TextSize::Sm)
            .text_color(t.muted_foreground)
            .child("No terminal sessions for this thread yet.")
            .child(
                Button::new("terminal-empty-new")
                    .variant(ButtonVariant::Outline)
                    .size(ButtonSize::Xs)
                    .label("New Terminal")
                    .on_click({
                        let this = cx.entity().downgrade();
                        move |_, window, cx| {
                            if let Some(this) = this.upgrade() {
                                this.update(cx, |this, cx| this.new_terminal(window, cx));
                            }
                        }
                    }),
            )
            .into_any_element()
    }
}

impl Render for TerminalDrawerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        let bg: Hsla = t.terminal_background;
        let root = div()
            .id("terminal-drawer")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(bg)
            .on_action(cx.listener(|this, _: &input::SplitTerminal, window, cx| {
                this.split(SplitDirection::Horizontal, window, cx)
            }))
            .on_action(cx.listener(|this, _: &input::SplitTerminalVertical, window, cx| {
                this.split(SplitDirection::Vertical, window, cx)
            }))
            .on_action(cx.listener(|this, _: &input::NewTerminal, window, cx| {
                this.new_terminal(window, cx)
            }))
            .on_action(cx.listener(|this, _: &input::CloseTerminal, window, cx| {
                if let Some(id) = this.entry().map(|e| e.state.active_terminal_id.clone()) {
                    this.request_close(&id, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &input::PreviousTerminal, window, cx| {
                this.cycle(-1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &input::NextTerminal, window, cx| {
                this.cycle(1, window, cx)
            }))
            // Clicks in the drawer chrome don't fall through to the chat.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        let Some(entry) = self.entry() else {
            return root;
        };
        if entry.state.is_empty() {
            return root.child(self.render_empty(cx));
        }
        let group = entry.state.active_group().cloned();
        let panes = match &group {
            Some(g) => self.render_group(entry, g, cx),
            None => div().into_any_element(),
        };
        if entry.state.len() == 1 {
            let toolbar = self.render_floating_toolbar(cx);
            return root.child(panes).child(toolbar);
        }
        let sidebar = self.render_sidebar(entry, cx);
        root.child(
            h_flex()
                .size_full()
                .items_stretch()
                .gap(px(super::layout::SIDEBAR_GAP))
                .child(div().flex_1().min_w_0().h_full().child(panes))
                .child(sidebar),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_messages() {
        assert_eq!(
            close_message(&["Terminal 1".into()]),
            "Close terminal \"Terminal 1\"?\nThis stops the running process and clears its history."
        );
        assert_eq!(
            close_message(&["a".into(), "b".into()]),
            "Close 2 terminals?\nThis stops their running processes and clears their histories: \"a\", \"b\"."
        );
    }
}

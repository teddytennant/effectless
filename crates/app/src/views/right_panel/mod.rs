//! The right panel (spec 05 §1): a per-thread strip of tabs ("surfaces") over the
//! active surface. Tern keeps Diff (§2), Files and File (the file viewer, ref 19);
//! browser previews, device mirroring and issues are dropped.
//!
//! The shell owns the column (width, resize handle, open/maximized in
//! `AppState::right_panel`) and the layout toggles drawn over the tab bar's right
//! end. This view owns the tabs, the add-surface menu, the empty launcher and the
//! surfaces, and handles `diff.toggle` (Ctrl+D), `rightPanel.close` (Ctrl+W),
//! `rightPanel.new` (Ctrl+T) and `view.reopenClosed` (Ctrl+Shift+T).

pub mod diff;
pub mod file_tree;
pub mod file_view;
pub mod icons;
pub mod model;
pub mod tree_view;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, ClipboardItem, Context, Entity, FocusHandle,
    Global, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window, actions, div,
    prelude::FluentBuilder as _, px,
};
use gpui_base::Align;
use tern_core::ThreadId;

use self::diff::{DiffEvent, DiffView};
use self::file_view::{FileEvent, FileView};
use self::icons::file_icon;
use self::model::{PanelChange, PanelTabs, Surface};
use crate::app_state::AppState;
use crate::store::Store;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, layout};
use crate::ui::menu::ContextMenuExt as _;
use crate::ui::{
    Button, ButtonSize, ButtonVariant, DropdownMenu, Icon, IconName, Kbd, Menu, MenuItem, Tooltip,
    TooltipExt as _, TooltipSide, h_flex, v_flex,
};

actions!(
    right_panel,
    [ToggleDiff, CloseSurface, NewSurface, ReopenClosed]
);

/// Binds the panel's shortcuts (spec 05 §1.8, Linux `mod` = Ctrl) and routes them
/// to the panel from anywhere in the window.
pub fn init(cx: &mut App) {
    use gpui::KeyBinding;
    cx.bind_keys([
        KeyBinding::new("ctrl-d", ToggleDiff, Some("Workspace")),
        KeyBinding::new("ctrl-w", CloseSurface, Some("Workspace")),
        KeyBinding::new("ctrl-t", NewSurface, Some("Workspace")),
        KeyBinding::new("ctrl-shift-t", ReopenClosed, Some("Workspace")),
    ]);
    cx.on_action(|_: &ToggleDiff, cx| with_panel(cx, |p, cx| p.toggle_diff(cx)));
    cx.on_action(|_: &CloseSurface, cx| with_panel(cx, |p, cx| p.close_active(cx)));
    cx.on_action(|_: &ReopenClosed, cx| with_panel(cx, |p, cx| p.reopen_closed(cx)));
    cx.on_action(|_: &NewSurface, cx| {
        with_panel(cx, |p, cx| {
            p.menu_requested = true;
            cx.notify();
        })
    });
}

struct GlobalPanel(gpui::WeakEntity<RightPanelView>);
impl Global for GlobalPanel {}

fn with_panel(cx: &mut App, f: impl FnOnce(&mut RightPanelView, &mut Context<RightPanelView>)) {
    let Some(panel) = cx.try_global::<GlobalPanel>().and_then(|g| g.0.upgrade()) else {
        return;
    };
    panel.update(cx, f);
}

/// The launcher / add-menu entries Tern keeps, in T3's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Terminal,
    Files,
    Diff,
    PullRequest,
}

impl Action {
    const ALL: [Action; 4] = [Self::Terminal, Self::Files, Self::Diff, Self::PullRequest];

    fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Files => "Files",
            Self::Diff => "Diff",
            Self::PullRequest => "Pull request",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Terminal => IconName::SquareTerminal,
            Self::Files => IconName::Files,
            Self::Diff => IconName::FileDiff,
            Self::PullRequest => IconName::GitPullRequestArrow,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Terminal => "T",
            Self::Files => "F",
            Self::Diff => "D",
            Self::PullRequest => "P",
        }
    }

    /// (menu tooltip, launcher hint) when unavailable.
    fn unavailable(self) -> (&'static str, &'static str) {
        match self {
            Self::Terminal => (
                "Terminal surfaces are only available from a project thread.",
                "Available when a project is open.",
            ),
            Self::Files => (
                "Files are only available when a project is open.",
                "Available when a project is open.",
            ),
            Self::Diff => (
                "Diff is only available for server threads in Git repositories.",
                "Available for Git repositories.",
            ),
            Self::PullRequest => (
                "This thread's branch has no pull request yet.",
                "No pull request on this branch yet.",
            ),
        }
    }
}

pub struct RightPanelView {
    app: Entity<AppState>,
    store: Entity<Store>,
    tabs: HashMap<ThreadId, PanelTabs>,
    diffs: HashMap<ThreadId, Entity<DiffView>>,
    files: HashMap<(ThreadId, Option<String>), Entity<FileView>>,
    launcher_focus: FocusHandle,
    launcher_highlight: Option<usize>,
    /// Ctrl+T asked for the add menu; opened on the next render.
    menu_requested: bool,
    /// `TERN_RIGHT_PANEL` (dev/screenshots) still to apply to the first thread.
    preset: Option<String>,
    add_bounds: Rc<Cell<gpui::Bounds<gpui::Pixels>>>,
}

impl RightPanelView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = AppState::global(cx);
        let store = Store::global(cx);
        cx.observe(&app, |_, _, cx| cx.notify()).detach();
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        let weak = cx.entity().downgrade();
        cx.set_global(GlobalPanel(weak));
        Self {
            app,
            store,
            tabs: HashMap::new(),
            diffs: HashMap::new(),
            files: HashMap::new(),
            launcher_focus: cx.focus_handle(),
            launcher_highlight: None,
            menu_requested: false,
            preset: std::env::var("TERN_RIGHT_PANEL").ok(),
            add_bounds: Rc::new(Cell::new(gpui::Bounds::default())),
        }
    }

    fn thread_id(&self, cx: &App) -> Option<ThreadId> {
        self.app.read(cx).chat_route().thread_id().cloned()
    }

    fn panel_open(&self, cx: &App) -> bool {
        self.app.read(cx).right_panel.open
    }

    fn set_open(&self, open: bool, cx: &mut Context<Self>) {
        self.app
            .update(cx, |s, cx| s.set_right_panel_open(open, cx));
    }

    fn apply(&self, change: PanelChange, cx: &mut Context<Self>) {
        match change {
            PanelChange::Open => self.set_open(true, cx),
            PanelChange::Close => self.set_open(false, cx),
            PanelChange::Keep => {}
        }
    }

    fn tabs_mut(&mut self, cx: &App) -> Option<&mut PanelTabs> {
        let id = self.thread_id(cx)?;
        Some(self.tabs.entry(id).or_default())
    }

    fn diff_view(
        &mut self,
        thread: &ThreadId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<DiffView> {
        if let Some(view) = self.diffs.get(thread) {
            return view.clone();
        }
        let id = thread.clone();
        let view = cx.new(|cx| DiffView::new(id, window, cx));
        cx.subscribe(&view, |this, _, event: &DiffEvent, cx| match event {
            DiffEvent::OpenFile(path) => this.open_surface(Surface::File(path.clone()), cx),
        })
        .detach();
        self.diffs.insert(thread.clone(), view.clone());
        view
    }

    fn file_view(
        &mut self,
        thread: &ThreadId,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<FileView> {
        let key = (thread.clone(), path.clone());
        if let Some(view) = self.files.get(&key) {
            return view.clone();
        }
        let id = thread.clone();
        let view = cx.new(|cx| FileView::new(id, path, window, cx));
        cx.subscribe(&view, |this, _, event: &FileEvent, cx| match event {
            FileEvent::OpenFile(path) => this.open_surface(Surface::File(path.clone()), cx),
        })
        .detach();
        self.files.insert(key, view.clone());
        view
    }

    // --- store semantics ---------------------------------------------------------

    pub fn open_surface(&mut self, surface: Surface, cx: &mut Context<Self>) {
        let Some(tabs) = self.tabs_mut(cx) else {
            return;
        };
        tabs.open(surface);
        self.set_open(true, cx);
        cx.notify();
    }

    /// The add menu / launcher: Diff also resets its scope to `branch`.
    fn run(&mut self, action: Action, cx: &mut Context<Self>) {
        match action {
            Action::Diff => {
                if let Some(id) = self.thread_id(cx)
                    && let Some(view) = self.diffs.get(&id)
                {
                    view.update(cx, |v, cx| v.reset_to_branch(cx));
                }
                self.open_surface(Surface::Diff, cx);
            }
            Action::Files => self.open_surface(Surface::Files, cx),
            // Terminal surfaces belong to the terminal work; until they land the
            // entry opens the terminal drawer.
            Action::Terminal => self.app.update(cx, |s, cx| s.set_drawer_open(true, cx)),
            Action::PullRequest => {
                if let Some(url) = self.pull_request_url(cx) {
                    cx.open_url(&url);
                }
            }
        }
    }

    fn pull_request_url(&self, cx: &App) -> Option<String> {
        let id = self.thread_id(cx)?;
        let store = self.store.read(cx);
        let shell = store.snapshot().thread(&id)?;
        shell
            .thread
            .linked_pull_request
            .as_ref()
            .map(|p| p.url.clone())
    }

    fn available(&self, action: Action, cx: &App) -> bool {
        let has_thread = self.thread_id(cx).is_some();
        match action {
            Action::Terminal | Action::Files | Action::Diff => has_thread,
            Action::PullRequest => self.pull_request_url(cx).is_some(),
        }
    }

    /// `diff.toggle`.
    pub fn toggle_diff(&mut self, cx: &mut Context<Self>) {
        let open = self.panel_open(cx);
        let Some(tabs) = self.tabs_mut(cx) else {
            return;
        };
        let change = tabs.toggle(Surface::Diff, open);
        self.apply(change, cx);
        cx.notify();
    }

    /// `rightPanel.close`: close the active surface.
    pub fn close_active(&mut self, cx: &mut Context<Self>) {
        if !self.panel_open(cx) {
            return;
        }
        let Some(tabs) = self.tabs_mut(cx) else {
            return;
        };
        let Some(active) = tabs.active.clone() else {
            return;
        };
        let change = tabs.close(&active);
        self.apply(change, cx);
        cx.notify();
    }

    fn close(&mut self, surface: &Surface, cx: &mut Context<Self>) {
        let Some(tabs) = self.tabs_mut(cx) else {
            return;
        };
        let change = tabs.close(surface);
        self.apply(change, cx);
        cx.notify();
    }

    pub fn reopen_closed(&mut self, cx: &mut Context<Self>) {
        let Some(tabs) = self.tabs_mut(cx) else {
            return;
        };
        if tabs.reopen_closed().is_some() {
            self.set_open(true, cx);
            cx.notify();
        }
    }

    fn activate(&mut self, surface: Surface, cx: &mut Context<Self>) {
        if let Some(tabs) = self.tabs_mut(cx) {
            tabs.active = Some(surface);
            cx.notify();
        }
    }

    /// `TERN_RIGHT_PANEL=diff,file:notes.py,*files,split,tree` opens those surfaces
    /// on the first thread shown (the `*` one active, else the last). Dev aid for
    /// screenshots.
    fn apply_preset(&mut self, cx: &mut Context<Self>) {
        let Some(preset) = self.preset.clone() else {
            return;
        };
        let Some(id) = self.thread_id(cx) else {
            return;
        };
        self.preset = None;
        let tabs = self.tabs.entry(id).or_default();
        let mut active = None;
        for part in preset.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (star, part) = match part.strip_prefix('*') {
                Some(rest) => (true, rest),
                None => (false, part),
            };
            let surface = match part {
                "diff" => Surface::Diff,
                "files" => Surface::Files,
                "split" => {
                    diff::DiffPrefs::update(cx, |p| p.split = true);
                    continue;
                }
                "tree" => {
                    diff::DiffPrefs::update(cx, |p| p.tree_open = true);
                    continue;
                }
                other => match other.strip_prefix("file:") {
                    Some(path) => Surface::File(path.to_string()),
                    None => continue,
                },
            };
            tabs.open(surface.clone());
            if star {
                active = Some(surface);
            }
        }
        if let Some(active) = active {
            tabs.active = Some(active);
        }
    }

    // --- rendering ---------------------------------------------------------------

    fn add_menu(&self, cx: &mut Context<Self>) -> impl Fn(&mut Window, &mut App) -> Menu + 'static {
        let this = cx.entity().downgrade();
        let available: Vec<(Action, bool)> = Action::ALL
            .iter()
            .map(|a| (*a, self.available(*a, cx)))
            .collect();
        move |_, _| {
            let mut menu = Menu::new();
            for (action, ok) in &available {
                let this = this.clone();
                let action = *action;
                let row = MenuItem::new(action.label())
                    .icon(action.icon())
                    .shortcut(action.key())
                    .disabled(!ok);
                menu = menu.item(row.on_select(move |_, cx| {
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| this.run(action, cx));
                    }
                }));
            }
            menu
        }
    }

    fn render_tab(
        &self,
        ix: usize,
        surface: &Surface,
        active: bool,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *cx.tokens();
        let dark = cx.tern_theme().is_dark();
        let title = surface.title();
        let group: SharedString = format!("rp-tab-{ix}").into();
        let (icon, icon_color) = match surface {
            Surface::Diff => (IconName::FileDiff, None),
            Surface::Files => (IconName::Files, None),
            Surface::File(path) => {
                let (icon, color) = file_icon(path, dark);
                (icon, Some(color))
            }
        };
        let this = cx.entity().downgrade();
        let s_close = surface.clone();
        let close_this = this.clone();
        let s_activate = surface.clone();
        let activate_this = this.clone();
        let s_middle = surface.clone();
        let middle_this = this.clone();
        let fg = if active {
            t.foreground
        } else {
            t.muted_foreground
        };
        let s_menu = surface.clone();
        let menu_this = this.clone();
        let cwd = self.thread_cwd(cx);
        let s_drop = surface.clone();
        h_flex()
            .id(SharedString::from(format!("rp-tab-{}", surface.id())))
            .group(group.clone())
            .h(px(24.))
            .max_w(px(144.))
            .flex_none()
            .pl(px(6.))
            .pr(px(8.))
            .gap(px(2.))
            .rounded(px(6.))
            .text_scale(TextSize::Xs)
            .text_color(fg)
            .when(active, |el| el.bg(t.accent))
            .when(!active, |el| {
                el.hover(move |s| s.bg(t.accent.opacity(0.6)).text_color(t.foreground))
            })
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                if let Some(this) = activate_this.upgrade() {
                    let s = s_activate.clone();
                    this.update(cx, |this, cx| this.activate(s, cx));
                }
            })
            .on_mouse_down(MouseButton::Middle, |_, window, _| window.prevent_default())
            .on_mouse_up(MouseButton::Middle, move |_, _, cx| {
                if let Some(this) = middle_this.upgrade() {
                    let s = s_middle.clone();
                    this.update(cx, |this, cx| this.close(&s, cx));
                }
            })
            .on_drag(
                DraggedTab {
                    index: ix,
                    title: title.clone(),
                },
                |tab, _, _, cx| cx.new(|_| tab.clone()),
            )
            .drag_over::<DraggedTab>(move |s, _, _, _| s.bg(t.accent.opacity(0.6)))
            .on_drop(cx.listener(move |this, tab: &DraggedTab, _, cx| {
                let to = this
                    .thread_id(cx)
                    .and_then(|id| this.tabs.get(&id))
                    .and_then(|tabs| tabs.surfaces.iter().position(|s| *s == s_drop));
                if let (Some(to), Some(tabs)) = (to, this.tabs_mut(cx)) {
                    tabs.move_surface(tab.index, to);
                    cx.notify();
                }
            }))
            .t3_context_menu(
                SharedString::from(format!("rp-tab-menu-{ix}")),
                move |_, _| {
                    let action = |f: fn(&mut PanelTabs, &Surface) -> PanelChange| {
                        let this = menu_this.clone();
                        let s = s_menu.clone();
                        move |_: &mut Window, cx: &mut App| {
                            if let Some(this) = this.upgrade() {
                                this.update(cx, |this, cx| {
                                    if let Some(tabs) = this.tabs_mut(cx) {
                                        let change = f(tabs, &s);
                                        this.apply(change, cx);
                                        cx.notify();
                                    }
                                });
                            }
                        }
                    };
                    let mut menu = Menu::new();
                    if let (Surface::File(path), Some(cwd)) = (&s_menu, &cwd) {
                        let abs = cwd.join(path).to_string_lossy().into_owned();
                        menu = menu.item(MenuItem::new("Copy path").on_select(move |_, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(abs.clone()))
                        }));
                    }
                    let is_last = ix + 1 >= count;
                    menu.item(MenuItem::new("Close").on_select(action(|t, s| t.close(s))))
                        .item(
                            MenuItem::new("Close others")
                                .disabled(count <= 1)
                                .on_select(action(|t, s| {
                                    t.close_others(s);
                                    PanelChange::Keep
                                })),
                        )
                        .item(
                            MenuItem::new("Close to the right")
                                .disabled(is_last)
                                .on_select(action(|t, s| {
                                    t.close_to_right(s);
                                    PanelChange::Keep
                                })),
                        )
                        .item(MenuItem::new("Close all").on_select(action(|t, _| t.close_all())))
                },
            )
            .t3_tooltip(Tooltip::text(title.clone()).side(TooltipSide::Bottom))
            .child(
                div()
                    .id(SharedString::from(format!("rp-tab-close-{ix}")))
                    .size(px(16.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .hover(move |s| s.bg(t.muted))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        if let Some(this) = close_this.upgrade() {
                            let s = s_close.clone();
                            this.update(cx, |this, cx| this.close(&s, cx));
                        }
                    })
                    .child(div().group_hover(group.clone(), |s| s.invisible()).child(
                        match icon_color {
                            Some(color) => Icon::new(icon).size(px(12.)).color(color),
                            None => Icon::new(icon).size(px(12.)),
                        },
                    ))
                    .child(
                        div()
                            .absolute()
                            .invisible()
                            .group_hover(group, |s| s.visible())
                            .child(Icon::new(IconName::X).size(px(12.))),
                    )
                    .relative(),
            )
            .child(div().min_w_0().truncate().child(title))
            .into_any_element()
    }

    fn thread_cwd(&self, cx: &App) -> Option<std::path::PathBuf> {
        let id = self.thread_id(cx)?;
        let store = self.store.read(cx);
        let shell = store.snapshot().thread(&id)?;
        if let Some(path) = &shell.thread.worktree_path {
            return Some(path.into());
        }
        let project = store.snapshot().project(&shell.thread.project_id)?;
        Some(project.workspace_root.clone().into())
    }

    fn render_tab_bar(
        &mut self,
        tabs: &PanelTabs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let app = self.app.read(cx);
        let maximized = app.right_panel_maximized;
        let sidebar_open = app.sidebar.open;
        // The shell draws maximize + terminal + panel toggles over our right end.
        let reserve = crate::shell::workspace_controls_right(window) + px(28. * 3. + 4. * 2. + 4.);
        let count = tabs.surfaces.len();
        let tab_elements: Vec<AnyElement> = tabs
            .surfaces
            .iter()
            .enumerate()
            .map(|(i, s)| self.render_tab(i, s, tabs.active.as_ref() == Some(s), count, cx))
            .collect();
        let menu = self.add_menu(cx);
        let requested = std::mem::take(&mut self.menu_requested);
        let add = DropdownMenu::new("rp-add", menu, move |open| {
            Button::new("rp-add-btn")
                .variant(ButtonVariant::GhostMuted)
                .size(ButtonSize::IconXs)
                .icon(IconName::Plus)
                .pressed(open)
                .tooltip(Tooltip::text("Add panel surface").side(TooltipSide::Bottom))
                .into_any_element()
        })
        .align(Align::Start);
        if requested {
            // Ctrl+T: the add menu under the + button.
            let m = self.add_menu(cx)(window, cx);
            crate::ui::menu::open_menu(
                "rp-add".into(),
                crate::ui::popup::PopupAnchor::Trigger {
                    bounds: self.add_bounds.get(),
                    placement: gpui_base::Placement::Bottom,
                    align: Align::Start,
                    offset: px(6.),
                },
                m,
                window,
                cx,
            );
        }
        h_flex()
            .id("rp-tabbar")
            .flex_none()
            .h(layout::TOPBAR_HEIGHT)
            .pl(if maximized && !sidebar_open {
                layout::TITLEBAR_CONTENT_LEFT
            } else {
                px(8.)
            })
            .pr(reserve)
            .gap(px(4.))
            .child(
                h_flex()
                    .id("rp-tabs")
                    .min_w_0()
                    .gap(px(4.))
                    .overflow_x_scroll()
                    .children(tab_elements),
            )
            .child(
                div()
                    .relative()
                    .child(add)
                    .child(crate::ui::bounds_probe(self.add_bounds.clone())),
            )
            .into_any_element()
    }

    fn render_launcher(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let actions: Vec<(Action, bool)> = Action::ALL
            .iter()
            .map(|a| (*a, self.available(*a, cx)))
            .collect();
        let highlight = self.launcher_highlight;
        let this = cx.entity().downgrade();
        let rows = actions.iter().enumerate().map(|(i, (action, ok))| {
            let action = *action;
            let ok = *ok;
            let this = this.clone();
            let row = h_flex()
                .id(SharedString::from(format!("rp-launch-{i}")))
                .h(px(32.))
                .px(px(10.))
                .gap(px(10.))
                .rounded(px(8.))
                .text_scale(TextSize::Sm)
                .text_color(t.foreground)
                .when(highlight == Some(i) && ok, |el| {
                    el.bg(t.accent.opacity(0.6))
                })
                .when(ok, |el| {
                    el.cursor_pointer()
                        .hover(move |s| s.bg(t.accent.opacity(0.6)))
                        .on_click(move |_: &ClickEvent, _, cx| {
                            if let Some(this) = this.upgrade() {
                                this.update(cx, |this, cx| this.run(action, cx));
                            }
                        })
                })
                .when(!ok, |el| {
                    el.opacity(0.5)
                        .t3_tooltip(Tooltip::text(action.unavailable().1).side(TooltipSide::Top))
                })
                .child(
                    Icon::new(action.icon())
                        .size(px(16.))
                        .color(t.muted_foreground),
                )
                .child(div().flex_1().child(action.label()))
                .child(Kbd::new(action.key()));
            row.into_any_element()
        });
        div()
            .id("rp-launcher")
            .track_focus(&self.launcher_focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| this.launcher_key(e, cx)))
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .px(px(24.))
            .pb(layout::TOPBAR_HEIGHT)
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(320.))
                    .py(px(24.))
                    .child(
                        div()
                            .mb(px(12.))
                            .text_center()
                            .text_scale(TextSize::Sm)
                            .font_medium()
                            .text_color(t.foreground)
                            .child("Open a surface"),
                    )
                    .child(v_flex().gap(px(2.)).children(rows)),
            )
            .into_any_element()
    }

    /// Launcher keys: arrows move among available rows (wrapping), Enter runs,
    /// bare letters run directly.
    fn launcher_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        let m = &e.keystroke.modifiers;
        if m.control || m.alt || m.platform {
            return;
        }
        let available: Vec<usize> = Action::ALL
            .iter()
            .enumerate()
            .filter(|(_, a)| self.available(**a, cx))
            .map(|(i, _)| i)
            .collect();
        let key = e.keystroke.key.as_str();
        match key {
            "up" | "left" | "down" | "right" if !available.is_empty() => {
                let pos = self
                    .launcher_highlight
                    .and_then(|h| available.iter().position(|i| *i == h));
                let n = available.len();
                let next = match (key, pos) {
                    ("up" | "left", Some(p)) => (p + n - 1) % n,
                    ("up" | "left", None) => n - 1,
                    (_, Some(p)) => (p + 1) % n,
                    (_, None) => 0,
                };
                self.launcher_highlight = Some(available[next]);
                cx.notify();
            }
            "enter" => {
                if let Some(h) = self.launcher_highlight.filter(|h| available.contains(h)) {
                    self.run(Action::ALL[h], cx);
                }
            }
            _ => {
                if let Some((i, action)) = Action::ALL
                    .iter()
                    .enumerate()
                    .find(|(_, a)| a.key().eq_ignore_ascii_case(key))
                    && available.contains(&i)
                {
                    self.run(*action, cx);
                }
            }
        }
    }
}

/// Drag payload for reordering tabs; also its own drag preview.
#[derive(Clone)]
struct DraggedTab {
    index: usize,
    title: String,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        div()
            .h(px(24.))
            .px(px(8.))
            .rounded(px(6.))
            .bg(t.accent)
            .flex()
            .items_center()
            .text_scale(TextSize::Xs)
            .text_color(t.foreground)
            .child(self.title.clone())
    }
}

impl Render for RightPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        self.apply_preset(cx);
        let thread = self.thread_id(cx);
        let tabs = thread
            .as_ref()
            .and_then(|id| self.tabs.get(id))
            .cloned()
            .unwrap_or_default();
        let tab_bar = self.render_tab_bar(&tabs, window, cx);
        let content: AnyElement = match (&thread, tabs.active.clone()) {
            (Some(id), Some(Surface::Diff)) => self.diff_view(id, window, cx).into_any_element(),
            (Some(id), Some(Surface::Files)) => {
                self.file_view(id, None, window, cx).into_any_element()
            }
            (Some(id), Some(Surface::File(path))) => self
                .file_view(id, Some(path), window, cx)
                .into_any_element(),
            _ => self.render_launcher(cx),
        };
        // Drop views of tabs that are gone.
        let live = &self.tabs;
        self.files.retain(|(thread, path), _| {
            live.get(thread).is_some_and(|tabs| {
                tabs.surfaces.iter().any(|s| match (s, path) {
                    (Surface::Files, None) => true,
                    (Surface::File(p), Some(q)) => p == q,
                    _ => false,
                })
            })
        });
        v_flex()
            .id("right-panel-content")
            .size_full()
            .min_w_0()
            .bg(t.background)
            .text_color(t.foreground)
            .child(tab_bar)
            .child(v_flex().flex_1().min_h_0().child(content))
    }
}

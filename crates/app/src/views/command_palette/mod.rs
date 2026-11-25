//! The command palette (spec 02 §4). The shell draws the backdrop and the `dialog-glass`
//! surface (radius 16, max 576 x 420, top padding 10vh) while `AppState::palette_open` is
//! set; this view renders the input band, the grouped results and the footer.
//!
//! Open it from anywhere with [`open`], optionally on a submenu:
//!
//! ```ignore
//! command_palette::open(PaletteIntent::AddProject, cx);
//! ```
//!
//! Actions owned by other views appear when their keymap command has a handler (Go to file,
//! Search project contents, Find in current thread, Open usage, Focus next area).
//!
//! Dev flags for screenshots: `TERN_PALETTE_QUERY=<text>` types a query on open,
//! `TERN_PALETTE_INTENT=add-project|new-thread-in|appearance` opens on a submenu.

pub mod browse;
pub mod search;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use chrono::Utc;
use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::input::{
    Backspace, Enter, Escape as InputEscape, IndentInline, Input, InputEvent, InputState, MoveDown,
    MoveUp,
};
use tern_core::{Command, ProjectId, Snapshot, ThreadId};

use crate::app_state::{AppState, Route, SettingsPage};
use crate::keymap;
use crate::store::Store;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, ThemeMode, font, radius};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Icon, IconName, Kbd, Toast, Toasts, h_flex, v_flex,
};
use crate::views::sidebar::model;
use crate::views::sidebar::rows::{project_icon, provider_icon};

/// Where the palette opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteIntent {
    Root,
    /// "Add project" (sidebar, hero, scope popup): the Sources view.
    AddProject,
    /// The new-thread button with several projects: "New thread in…".
    NewThreadIn,
    /// Change appearance.
    Appearance,
}

/// Opens the palette (or re-targets it when already open).
pub fn open(intent: PaletteIntent, cx: &mut App) {
    if let Some(handle) = cx.try_global::<PaletteHandle>()
        && let Some(view) = handle.0.upgrade()
    {
        view.update(cx, |v, _| v.pending = Some(intent));
    }
    AppState::global(cx).update(cx, |s, cx| {
        s.palette_open = true;
        cx.notify();
    });
}

struct PaletteHandle(WeakEntity<CommandPaletteView>);
impl Global for PaletteHandle {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    NewThreadIn,
    Appearance,
    AddProject,
    /// The local-folder browser; the query is the path.
    Browse,
}

impl View {
    fn placeholder(self) -> &'static str {
        match self {
            View::Browse => "Enter path (e.g. ~/projects/my-app)",
            _ => "Search...",
        }
    }
}

type Run = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
enum Action {
    /// Close the palette, then run.
    Run(Run),
    Push(View),
    /// Navigate into a folder (browse).
    Enter(String),
}

#[derive(Clone)]
enum ItemIcon {
    Icon(IconName),
    Project(ProjectId),
}

#[derive(Clone)]
struct ThreadInfo {
    project_id: ProjectId,
    branch: Option<String>,
    instance_id: String,
    pr: Option<(u64, String)>,
    activity_ms: i64,
    current: bool,
}

#[derive(Clone)]
#[allow(dead_code)]
struct Item {
    key: SharedString,
    icon: ItemIcon,
    title: SharedString,
    /// A bold suffix ("New thread in **tern**").
    bold: Option<SharedString>,
    keywords: Vec<String>,
    description: Option<SharedString>,
    shortcut: Option<String>,
    trailing: Option<SharedString>,
    submenu: bool,
    thread: Option<ThreadInfo>,
    action: Action,
}

impl Item {
    fn new(
        key: impl Into<SharedString>,
        icon: IconName,
        title: impl Into<SharedString>,
        action: Action,
    ) -> Self {
        Self {
            key: key.into(),
            icon: ItemIcon::Icon(icon),
            title: title.into(),
            bold: None,
            keywords: Vec::new(),
            description: None,
            shortcut: None,
            trailing: None,
            submenu: false,
            thread: None,
            action,
        }
    }

    fn shortcut(mut self, command: &str, cx: &App) -> Self {
        self.shortcut = keymap::shortcut_label(command, cx);
        self
    }

    fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    fn keywords(mut self, words: &[&str]) -> Self {
        self.keywords = words.iter().map(|w| w.to_string()).collect();
        self
    }

    fn full_title(&self) -> String {
        match &self.bold {
            Some(b) => format!("{}{}", self.title, b),
            None => self.title.to_string(),
        }
    }
}

#[derive(Clone)]
struct Group {
    key: &'static str,
    label: SharedString,
    items: Vec<Item>,
}

/// The folder listing behind the browse view.
struct Listing {
    /// The directory the query's head names (as typed, `~` kept).
    head: String,
    dir: PathBuf,
    entries: Result<Vec<String>, String>,
    loading: bool,
    exists: bool,
}

impl Default for Listing {
    fn default() -> Self {
        Self {
            head: String::new(),
            dir: PathBuf::new(),
            entries: Ok(Vec::new()),
            loading: false,
            exists: false,
        }
    }
}

pub struct CommandPaletteView {
    app: Entity<AppState>,
    store: Entity<Store>,
    input: Entity<InputState>,
    query: String,
    /// Submenu stack with each parent's query.
    stack: Vec<(View, String)>,
    highlighted: Option<usize>,
    scroll: ScrollHandle,
    listing: Listing,
    _list_task: Option<Task<()>>,
    pending: Option<PaletteIntent>,
    was_open: bool,
    restore_focus: Option<FocusHandle>,
    /// A path just added: open a draft once its project shows up.
    awaiting_project: Option<String>,
    _subs: Vec<Subscription>,
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

impl CommandPaletteView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = AppState::global(cx);
        let store = Store::global(cx);
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search commands, projects, and threads...")
        });
        let handle = PaletteHandle(cx.entity().downgrade());
        cx.set_global(handle);
        let subs = vec![
            cx.observe_in(&app, window, |this, app, window, cx| {
                let open = app.read(cx).palette_open;
                if open && (!this.was_open || this.pending.is_some()) {
                    let intent = this.pending.take().unwrap_or(PaletteIntent::Root);
                    if !this.was_open {
                        this.restore_focus = window.focused(cx);
                    }
                    this.reset(intent, window, cx);
                } else if !open && this.was_open {
                    this.on_closed(window, cx);
                }
                this.was_open = open;
            }),
            cx.observe(&store, |this, _, cx| this.check_added_project(cx)),
            cx.subscribe_in(
                &input,
                window,
                |this, input, event: &InputEvent, window, cx| {
                    if let InputEvent::Change = event {
                        let value = input.read(cx).value().to_string();
                        this.on_query(value, window, cx);
                    }
                },
            ),
        ];
        let mut this = Self {
            app: app.clone(),
            store,
            input,
            query: String::new(),
            stack: Vec::new(),
            highlighted: Some(0),
            scroll: ScrollHandle::new(),
            listing: Listing::default(),
            _list_task: None,
            pending: None,
            was_open: false,
            restore_focus: None,
            awaiting_project: None,
            _subs: subs,
        };
        if app.read(cx).palette_open {
            this.was_open = true;
            this.reset(PaletteIntent::Root, window, cx);
        }
        this.register_commands(cx);
        this
    }

    fn register_commands(&self, cx: &mut Context<Self>) {
        keymap::on_command(
            "appearance.cycle",
            |window, cx| {
                let app = AppState::global(cx);
                let next = match app.read(cx).theme_mode() {
                    ThemeMode::System => ThemeMode::Light,
                    ThemeMode::Light => ThemeMode::Dark,
                    ThemeMode::Dark => ThemeMode::System,
                };
                app.update(cx, |s, cx| s.set_theme_mode(next, window, cx));
                let label = match next {
                    ThemeMode::System => "System",
                    ThemeMode::Light => "Light",
                    ThemeMode::Dark => "Dark",
                };
                Toasts::push(
                    Toast::message(format!("Appearance: {label}"))
                        .timeout(std::time::Duration::from_millis(1500)),
                    cx,
                );
            },
            cx,
        );
    }

    // --- state -------------------------------------------------------------------

    fn reset(&mut self, intent: PaletteIntent, window: &mut Window, cx: &mut Context<Self>) {
        self.stack.clear();
        self.listing = Listing::default();
        let mut query = String::new();
        match intent {
            PaletteIntent::Root => {}
            PaletteIntent::AddProject => self.stack.push((View::AddProject, String::new())),
            PaletteIntent::NewThreadIn => self.stack.push((View::NewThreadIn, String::new())),
            PaletteIntent::Appearance => self.stack.push((View::Appearance, String::new())),
        }
        if intent == PaletteIntent::Root
            && let Ok(dev) = std::env::var("TERN_PALETTE_INTENT")
        {
            match dev.as_str() {
                "add-project" => self.stack.push((View::AddProject, String::new())),
                "new-thread-in" => self.stack.push((View::NewThreadIn, String::new())),
                "appearance" => self.stack.push((View::Appearance, String::new())),
                _ => {}
            }
            if let Some(path) = dev.strip_prefix("browse:") {
                self.stack.push((View::AddProject, String::new()));
                self.stack.push((View::Browse, String::new()));
                query = path.to_string();
            }
        }
        if let Ok(q) = std::env::var("TERN_PALETTE_QUERY")
            && query.is_empty()
            && intent == PaletteIntent::Root
        {
            query = q;
        }
        self.set_query(query, window, cx);
        self.input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn on_closed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.restore_focus.take() {
            focus.focus(window, cx);
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| s.set_palette_open(false, cx));
    }

    fn view(&self) -> Option<View> {
        self.stack.last().map(|(v, _)| *v)
    }

    fn set_query(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = match self.view() {
            Some(View::Browse) => "Enter path (e.g. ~/projects/my-app)",
            Some(v) => v.placeholder(),
            None if browse::is_path_query(&query) => "Enter project path (e.g. ~/projects/my-app)",
            None => "Search commands, projects, and threads...",
        };
        self.input.update(cx, |s, cx| {
            s.set_placeholder(placeholder, window, cx);
            s.set_value(query.clone(), window, cx);
        });
        self.on_query(query, window, cx);
    }

    fn on_query(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        // Clearing a path-browse query pops the browse view.
        if query.is_empty() && self.view() == Some(View::Browse) && !self.query.is_empty() {
            self.query = query;
            self.pop(window, cx);
            return;
        }
        self.query = query;
        let browsing = self.browsing();
        self.highlighted = if browsing { None } else { Some(0) };
        if browsing {
            self.refresh_listing(cx);
        }
        self.scroll.scroll_to_item(0);
        cx.notify();
    }

    fn push(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        let parent = self.query.clone();
        self.stack.push((view, parent));
        let query = match view {
            View::Browse => {
                let base = String::new();
                if base.is_empty() {
                    "~/".to_string()
                } else {
                    base
                }
            }
            _ => String::new(),
        };
        self.set_query(query, window, cx);
    }

    fn pop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, parent)) = self.stack.pop() {
            self.set_query(parent, window, cx);
        }
    }

    fn browsing(&self) -> bool {
        self.view() == Some(View::Browse)
            || (matches!(self.view(), None | Some(View::AddProject))
                && browse::is_path_query(&self.query))
    }

    // --- folder listing --------------------------------------------------------------

    fn active_project_root(&self, cx: &App) -> Option<PathBuf> {
        let snap = self.store.read(cx).snapshot();
        let project = match self.app.read(cx).chat_route() {
            Route::Thread(id) => snap.thread(id).map(|s| s.thread.project_id.clone()),
            Route::Draft(p) => Some(p.clone()),
            _ => None,
        }?;
        snap.project(&project)
            .map(|p| PathBuf::from(&p.workspace_root))
    }

    fn refresh_listing(&mut self, cx: &mut Context<Self>) {
        let (head, _) = browse::split(&self.query);
        if head == self.listing.head && !head.is_empty() {
            return;
        }
        let head = head.to_string();
        let cwd = self.active_project_root(cx);
        let Some(dir) = browse::expand(&head, &home_dir(), cwd.as_deref()) else {
            self.listing = Listing {
                head,
                entries: Err("Relative paths require an active project.".into()),
                ..Default::default()
            };
            return;
        };
        let dir = browse::normalize(&dir);
        self.listing = Listing {
            head,
            dir: dir.clone(),
            entries: Ok(Vec::new()),
            loading: true,
            exists: true,
        };
        let task = cx.background_spawn(async move { browse::list_dirs(&dir) });
        self._list_task = Some(cx.spawn(async move |this, cx| {
            let entries = task.await;
            let _ = this.update(cx, |this, cx| {
                this.listing.exists = entries.is_ok();
                this.listing.entries = entries;
                this.listing.loading = false;
                cx.notify();
            });
        }));
    }

    /// The full path the query names.
    fn typed_path(&self, cx: &App) -> Option<PathBuf> {
        let cwd = self.active_project_root(cx);
        browse::expand(
            self.query.trim_end_matches('/'),
            &home_dir(),
            cwd.as_deref(),
        )
        .map(|p| browse::normalize(&p))
    }

    fn typed_path_exists(&self, cx: &App) -> bool {
        self.typed_path(cx).is_some_and(|p| p.is_dir())
    }

    /// Submit (`handleAddProjectForEnvironment`, spec 02 §4.5).
    fn submit_path(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        let Some(path) = path.or_else(|| self.typed_path(cx)) else {
            Toasts::push(
                Toast::error("Failed to add project")
                    .description("Relative paths require an active project."),
                cx,
            );
            return;
        };
        let root = path.to_string_lossy().trim_end_matches('/').to_string();
        let root = if root.is_empty() {
            "/".to_string()
        } else {
            root
        };
        let snap = self.store.read(cx).snapshot().clone();
        if let Some(project) = snap
            .projects
            .iter()
            .find(|p| p.workspace_root.trim_end_matches('/') == root)
        {
            let route = latest_unsettled(&snap, &project.id)
                .map(Route::Thread)
                .unwrap_or_else(|| Route::Draft(project.id.clone()));
            self.close(cx);
            self.app.update(cx, |s, cx| s.navigate(route, cx));
            return;
        }
        if !path.exists()
            && let Err(e) = std::fs::create_dir_all(&path)
        {
            Toasts::push(
                Toast::error("Failed to add project").description(e.to_string()),
                cx,
            );
            return;
        }
        self.awaiting_project = Some(root.clone());
        self.store.read(cx).send(Command::AddProject { path: root });
        self.close(cx);
        self.check_added_project(cx);
    }

    fn check_added_project(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.awaiting_project.clone() else {
            return;
        };
        let project = self
            .store
            .read(cx)
            .snapshot()
            .projects
            .iter()
            .find(|p| p.workspace_root.trim_end_matches('/') == root)
            .map(|p| p.id.clone());
        if let Some(id) = project {
            self.awaiting_project = None;
            self.app
                .update(cx, |s, cx| s.navigate(Route::Draft(id), cx));
        }
    }

    fn open_native_picker(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add project".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| this.submit_path(Some(path), cx));
            }
        })
        .detach();
    }

    // --- content -------------------------------------------------------------------

    fn snapshot<'a>(&self, cx: &'a App) -> &'a Snapshot {
        self.store.read(cx).snapshot()
    }

    fn open_thread_id(&self, cx: &App) -> Option<ThreadId> {
        self.app.read(cx).chat_route().thread_id().cloned()
    }

    fn contextual_project(&self, cx: &App) -> Option<ProjectId> {
        let snap = self.snapshot(cx);
        match self.app.read(cx).chat_route() {
            Route::Thread(id) => {
                if let Some(s) = snap.thread(id) {
                    return Some(s.thread.project_id.clone());
                }
            }
            Route::Draft(p) if snap.project(p).is_some() => return Some(p.clone()),
            _ => {}
        }
        model::index_project(snap)
    }

    fn navigate_run(route: Route) -> Action {
        Action::Run(Rc::new(move |_, cx| {
            AppState::global(cx).update(cx, |s, cx| s.navigate(route.clone(), cx))
        }))
    }

    fn keymap_run(command: &'static str) -> Action {
        Action::Run(Rc::new(move |window, cx| {
            keymap::run(command, window, cx);
        }))
    }

    fn send_run(command: Command) -> Action {
        Action::Run(Rc::new(move |_, cx| {
            Store::global(cx).read(cx).send(command.clone())
        }))
    }

    fn root_actions(&self, cx: &App) -> Vec<Item> {
        let snap = self.snapshot(cx);
        let projects = model::ordered_projects(snap);
        let mut items = Vec::new();
        let contextual = self.contextual_project(cx);
        if let Some(p) = contextual.as_ref().and_then(|id| snap.project(id)) {
            let mut item = Item::new(
                "new-thread-here",
                IconName::SquarePen,
                "New thread in ",
                Self::navigate_run(Route::Draft(p.id.clone())),
            )
            .shortcut("chat.newLocal", cx);
            item.bold = Some(p.title.clone().into());
            items.push(item);
        }
        if !projects.is_empty() {
            let mut item = Item::new(
                "new-thread-in",
                IconName::SquarePen,
                "New thread in...",
                Action::Push(View::NewThreadIn),
            );
            item.submenu = true;
            items.push(item);
        }
        let open = self
            .open_thread_id(cx)
            .and_then(|id| snap.thread(&id).cloned());
        if open.is_some() && keymap::has_handler("chat.find", cx) {
            items.push(
                Item::new(
                    "find",
                    IconName::TextSearch,
                    "Find in current thread",
                    Self::keymap_run("chat.find"),
                )
                .shortcut("chat.find", cx),
            );
        }
        if let Some(shell) = &open {
            let (title, value) = match &shell.thread.linked_pull_request {
                Some(pr) => ("Copy pull request link", pr.url.clone()),
                None => ("Copy thread", shell.thread.title.clone()),
            };
            items.push(
                Item::new(
                    "copy-thread",
                    IconName::Link,
                    title,
                    Action::Run(Rc::new(move |_, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(value.clone()))
                    })),
                )
                .shortcut("thread.copyReference", cx),
            );
            let thread_id = shell.thread.id.clone();
            items.push(Item::new(
                "restart-session",
                IconName::RotateCcw,
                "Restart agent session",
                Action::Run(Rc::new(move |_, cx| {
                    Store::global(cx).read(cx).send(Command::ForceStop {
                        thread_id: thread_id.clone(),
                    });
                    Toasts::push(
                        Toast::success("Agent session will restart")
                            .description("Your next message starts a fresh session."),
                        cx,
                    );
                })),
            ));
            items.extend(self.thread_actions(shell, cx));
        }
        for (key, icon, title, command) in [
            (
                "go-to-file",
                IconName::FileSearch,
                "Go to file",
                "filePicker.toggle",
            ),
            (
                "search-contents",
                IconName::TextSearch,
                "Search project contents",
                "projectSearch.toggle",
            ),
        ] {
            if keymap::has_handler(command, cx) {
                items.push(
                    Item::new(key, icon, title, Self::keymap_run(command)).shortcut(command, cx),
                );
            }
        }
        items.push(
            Item::new(
                "add-project",
                IconName::FolderPlus,
                "Add project",
                Action::Push(View::AddProject),
            )
            .keywords(&["folder", "open", "repository"]),
        );
        let mut appearance = Item::new(
            "change-appearance",
            IconName::Monitor,
            "Change appearance",
            Action::Push(View::Appearance),
        )
        .shortcut("appearance.cycle", cx)
        .keywords(&["theme", "dark", "light", "mode"]);
        appearance.submenu = true;
        items.push(appearance);
        if keymap::has_handler("usage.open", cx) {
            items.push(
                Item::new(
                    "usage",
                    IconName::ChartNoAxesColumn,
                    "Open usage",
                    Self::keymap_run("usage.open"),
                )
                .shortcut("usage.open", cx),
            );
        }
        if keymap::has_handler("workspace.focusNext", cx) {
            items.push(
                Item::new(
                    "focus-next",
                    IconName::Monitor,
                    "Focus Next Workspace Area",
                    Self::keymap_run("workspace.focusNext"),
                )
                .shortcut("workspace.focusNext", cx),
            );
            items.push(
                Item::new(
                    "focus-previous",
                    IconName::Monitor,
                    "Focus Previous Workspace Area",
                    Self::keymap_run("workspace.focusPrevious"),
                )
                .shortcut("workspace.focusPrevious", cx),
            );
        }
        items.push(
            Item::new(
                "settings",
                IconName::Settings,
                "Open settings",
                Self::navigate_run(Route::Settings(SettingsPage::General)),
            )
            .keywords(&["preferences", "config"]),
        );
        if let Some(p) = contextual.as_ref().and_then(|id| snap.project(id)) {
            items.push(
                Item::new(
                    "project-settings",
                    IconName::Folder,
                    "Project settings",
                    Self::navigate_run(Route::Settings(SettingsPage::Projects)),
                )
                .description(p.title.clone()),
            );
        }
        items
    }

    /// The open thread's actions (§4.3 row 7): the context menu minus Rename and Project
    /// settings.
    fn thread_actions(&self, shell: &tern_core::ThreadShell, cx: &App) -> Vec<Item> {
        let id = shell.thread.id.clone();
        let pinned = shell.thread.pinned_at.is_some();
        let settled = shell.thread.settled_at.is_some();
        let mut items = vec![
            Item::new(
                "thread-pin",
                if pinned {
                    IconName::PinOff
                } else {
                    IconName::Pin
                },
                if pinned { "Unpin thread" } else { "Pin thread" },
                Self::send_run(Command::SetPinned {
                    thread_id: id.clone(),
                    pinned: !pinned,
                }),
            )
            .shortcut("thread.pin", cx),
            Item::new(
                "thread-settle",
                IconName::CircleCheck,
                if settled {
                    "Un-settle thread"
                } else {
                    "Settle thread"
                },
                Self::keymap_run("thread.settle"),
            )
            .shortcut("thread.settle", cx),
            Item::new(
                "thread-regenerate",
                IconName::RefreshCw,
                "Regenerate title",
                Self::send_run(Command::RegenerateTitle {
                    thread_id: id.clone(),
                }),
            ),
        ];
        if shell.latest_run_completed_at.is_some() && !shell.is_working() {
            items.push(Item::new(
                "thread-unread",
                IconName::MailOpen,
                "Mark unread",
                Self::send_run(Command::MarkUnread {
                    thread_id: id.clone(),
                }),
            ));
        }
        items.push(Item::new(
            "thread-archive",
            IconName::Archive,
            "Archive thread",
            Self::send_run(Command::SetArchived {
                thread_id: id.clone(),
                archived: true,
            }),
        ));
        items
    }

    fn recent_threads(&self, limit: Option<usize>, cx: &App) -> Vec<Item> {
        let snap = self.snapshot(cx);
        let open = self.open_thread_id(cx);
        let now = Utc::now();
        let mut threads: Vec<&tern_core::ThreadShell> = snap
            .threads
            .iter()
            .filter(|s| s.thread.archived_at.is_none() && s.thread.deleted_at.is_none())
            .collect();
        threads.sort_by(|a, b| {
            let current =
                |s: &tern_core::ThreadShell| open.as_deref() == Some(s.thread.id.as_str());
            current(a)
                .cmp(&current(b))
                .then(model::activity_at(b).cmp(&model::activity_at(a)))
        });
        if limit.is_some() {
            threads.retain(|s| open.as_deref() != Some(s.thread.id.as_str()));
        }
        threads
            .into_iter()
            .take(limit.unwrap_or(usize::MAX))
            .map(|s| {
                let at = model::activity_at(s);
                let id = s.thread.id.clone();
                let mut item = Item::new(
                    format!("thread-{id}"),
                    IconName::MessageSquare,
                    s.thread.title.clone(),
                    Self::navigate_run(Route::Thread(id.clone())),
                );
                item.trailing = Some(model::relative_ago(at, now).into());
                item.thread = Some(ThreadInfo {
                    project_id: s.thread.project_id.clone(),
                    branch: s.thread.branch.clone(),
                    instance_id: s.thread.model_selection.instance_id.clone(),
                    pr: s
                        .thread
                        .linked_pull_request
                        .as_ref()
                        .map(|pr| (pr.number, pr.title.clone())),
                    activity_ms: at.timestamp_millis(),
                    current: open.as_deref() == Some(id.as_str()),
                });
                item
            })
            .collect()
    }

    fn project_items(&self, cx: &App) -> Vec<Item> {
        let snap = self.snapshot(cx);
        let current = self.contextual_project(cx);
        model::ordered_projects(snap)
            .into_iter()
            .map(|p| {
                let route = latest_unsettled(snap, &p.id)
                    .map(Route::Thread)
                    .unwrap_or_else(|| Route::Draft(p.id.clone()));
                let mut item = Item::new(
                    format!("project-{}", p.id),
                    IconName::Folder,
                    p.title.clone(),
                    Self::navigate_run(route),
                );
                item.icon = ItemIcon::Project(p.id.clone());
                let desc = if current.as_deref() == Some(p.id.as_str()) {
                    format!("Current · Local · {}", p.workspace_root)
                } else {
                    format!("Local · {}", p.workspace_root)
                };
                item.description = Some(desc.into());
                item
            })
            .collect()
    }

    fn new_thread_in_items(&self, cx: &App) -> Vec<Item> {
        let snap = self.snapshot(cx);
        let projects = model::ordered_projects(snap);
        if projects.is_empty() {
            return vec![Item::new(
                "add-project",
                IconName::FolderPlus,
                "Add project",
                Action::Push(View::AddProject),
            )];
        }
        projects
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                let mut item = Item::new(
                    format!("new-in-{}", p.id),
                    IconName::Folder,
                    p.title.clone(),
                    Self::navigate_run(Route::Draft(p.id.clone())),
                );
                item.icon = ItemIcon::Project(p.id.clone());
                if i < 9 {
                    item.shortcut = Some(format!("Ctrl+{}", i + 1));
                }
                item
            })
            .collect()
    }

    fn appearance_items(&self, cx: &App) -> Vec<Item> {
        let current = self.app.read(cx).theme_mode();
        [
            (ThemeMode::System, "System", IconName::Monitor),
            (ThemeMode::Light, "Light", IconName::Sun),
            (ThemeMode::Dark, "Dark", IconName::Moon),
        ]
        .into_iter()
        .map(|(mode, label, icon)| {
            let mut item = Item::new(
                format!("appearance-{label}"),
                icon,
                label,
                Action::Run(Rc::new(move |window, cx| {
                    AppState::global(cx).update(cx, |s, cx| s.set_theme_mode(mode, window, cx))
                })),
            );
            if mode == current {
                item.trailing = Some("Current".into());
            }
            item
        })
        .collect()
    }

    fn settings_items(&self) -> Vec<Item> {
        SettingsPage::ALL
            .into_iter()
            .map(|page| {
                Item::new(
                    format!("settings-{}", page.slug()),
                    IconName::Settings,
                    page.title(),
                    Self::navigate_run(Route::Settings(page)),
                )
                .description("Settings")
            })
            .collect()
    }

    /// The groups for the current view and query, ranked (§4.3, §4.4).
    fn groups(&self, cx: &App) -> Vec<Group> {
        if self.browsing() {
            return self.browse_groups();
        }
        let query = self.query.clone();
        match self.view() {
            Some(View::AddProject) => {
                let sources = vec![Group {
                    key: "sources",
                    label: "Sources".into(),
                    items: vec![
                        Item::new(
                            "local-folder",
                            IconName::FolderPlus,
                            "Local folder",
                            Action::Push(View::Browse),
                        )
                        .description("Browse a folder on disk"),
                    ],
                }];
                return self.filter_submenu(sources, &query, cx);
            }
            Some(View::NewThreadIn) => {
                let groups = vec![Group {
                    key: "projects",
                    label: "Projects".into(),
                    items: self.new_thread_in_items(cx),
                }];
                return self.filter_submenu(groups, &query, cx);
            }
            Some(View::Appearance) => {
                let groups = vec![Group {
                    key: "appearance",
                    label: "Appearance".into(),
                    items: self.appearance_items(cx),
                }];
                return self.filter_submenu(groups, &query, cx);
            }
            Some(View::Browse) => return self.browse_groups(),
            None => {}
        }
        self.filter_root(&query, cx)
    }

    fn root_groups(&self, cx: &App) -> Vec<Group> {
        let mut groups = vec![Group {
            key: "actions",
            label: "Actions".into(),
            items: self.root_actions(cx),
        }];
        let recent = self.recent_threads(Some(12), cx);
        if !recent.is_empty() {
            groups.push(Group {
                key: "recent-threads",
                label: "Recent Threads".into(),
                items: recent,
            });
        }
        groups
    }

    fn filter_submenu(&self, groups: Vec<Group>, query: &str, cx: &App) -> Vec<Group> {
        let folded = search::fold_query(query);
        if folded.is_empty() {
            return groups;
        }
        let ranked = rank(&groups, &folded, Utc::now().timestamp_millis(), cx, self);
        if ranked.is_empty() {
            return self.filter_root(query, cx);
        }
        ranked
    }

    fn filter_root(&self, query: &str, cx: &App) -> Vec<Group> {
        let actions_only = query.starts_with('>');
        let q = if actions_only { &query[1..] } else { query };
        let folded = search::fold_query(q);
        let mut groups = self.root_groups(cx);
        if folded.is_empty() {
            if actions_only {
                groups.retain(|g| g.key == "actions");
            }
            return groups;
        }
        groups.retain(|g| g.key != "recent-threads");
        if !actions_only {
            // Suggestions: every submenu child flattened.
            let mut nested = Vec::new();
            for (stem, view) in [
                ("New thread in", View::NewThreadIn),
                ("Change appearance:", View::Appearance),
            ] {
                if search::fold_query(stem).starts_with(&folded) {
                    continue;
                }
                let children = match view {
                    View::NewThreadIn => self.new_thread_in_items(cx),
                    _ => self.appearance_items(cx),
                };
                for mut child in children {
                    child.title = format!("{stem} {}", child.title).into();
                    child.shortcut = None;
                    nested.push(child);
                }
            }
            if !nested.is_empty() {
                groups.push(Group {
                    key: "suggestions",
                    label: "Suggestions".into(),
                    items: nested,
                });
            }
            groups.push(Group {
                key: "threads",
                label: "Threads".into(),
                items: self.recent_threads(None, cx),
            });
            groups.push(Group {
                key: "projects",
                label: "Projects".into(),
                items: self.project_items(cx),
            });
            groups.push(Group {
                key: "settings",
                label: "Settings".into(),
                items: self.settings_items(),
            });
        }
        rank(&groups, &folded, Utc::now().timestamp_millis(), cx, self)
    }

    fn browse_groups(&self) -> Vec<Group> {
        let (_, filter) = browse::split(&self.query);
        let Ok(entries) = &self.listing.entries else {
            return Vec::new();
        };
        let mut items = Vec::new();
        if self.listing.dir.parent().is_some() && filter.is_empty() {
            items.push(Item::new(
                "dir-up",
                IconName::CornerLeftUp,
                "..",
                Action::Enter("..".into()),
            ));
        }
        for name in browse::filter_entries(entries, filter) {
            items.push(Item::new(
                format!("dir-{name}"),
                IconName::Folder,
                name.clone(),
                Action::Enter(name),
            ));
        }
        if items.is_empty() {
            return Vec::new();
        }
        vec![Group {
            key: "directories",
            label: "Directories".into(),
            items,
        }]
    }

    fn flat_items(&self, cx: &App) -> Vec<Item> {
        self.groups(cx).into_iter().flat_map(|g| g.items).collect()
    }

    // --- running ---------------------------------------------------------------------

    fn activate(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        match item.action {
            Action::Run(run) => {
                self.close(cx);
                run(window, cx);
            }
            Action::Push(view) => self.push(view, window, cx),
            Action::Enter(name) => {
                let (head, _) = browse::split(&self.query);
                let next = if name == ".." {
                    let dir = browse::normalize(&self.listing.dir);
                    let parent = dir.parent().map(Path::to_path_buf).unwrap_or(dir);
                    let mut s = parent.to_string_lossy().to_string();
                    if !s.ends_with('/') {
                        s.push('/');
                    }
                    s
                } else {
                    format!("{head}{name}/")
                };
                if self.view().is_none() || self.view() == Some(View::AddProject) {
                    self.stack.push((View::Browse, String::new()));
                }
                self.set_query(next, window, cx);
            }
        }
    }

    fn on_enter(&mut self, secondary: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.browsing() && (secondary || self.highlighted.is_none()) {
            self.submit_path(None, cx);
            return;
        }
        let items = self.flat_items(cx);
        let item = self
            .highlighted
            .and_then(|i| items.get(i).cloned())
            .or_else(|| items.first().cloned());
        if let Some(item) = item {
            self.activate(item, window, cx);
        }
    }

    fn move_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.flat_items(cx).len();
        if n == 0 {
            return;
        }
        let next = match self.highlighted {
            None if delta > 0 => 0,
            None => n - 1,
            Some(i) => (i as isize + delta).rem_euclid(n as isize) as usize,
        };
        self.highlighted = Some(next);
        // Children of the list are group headings and rows: skip the headings before it.
        let mut child = 0;
        let mut seen = 0;
        for group in self.groups(cx) {
            child += 1;
            if next < seen + group.items.len() {
                child += next - seen;
                break;
            }
            seen += group.items.len();
            child += group.items.len();
        }
        self.scroll.scroll_to_item(child);
        cx.notify();
    }

    fn on_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.browsing() {
            return;
        }
        let items = self.flat_items(cx);
        if let Some(item) = self.highlighted.and_then(|i| items.get(i).cloned()) {
            self.activate(item, window, cx);
            return;
        }
        let names: Vec<String> = items
            .iter()
            .filter(|i| i.title.as_ref() != "..")
            .map(|i| i.title.to_string())
            .collect();
        let prefix = browse::common_prefix(&names);
        let (head, filter) = browse::split(&self.query);
        if prefix.len() > filter.len() {
            let next = if names.len() == 1 {
                format!("{head}{prefix}/")
            } else {
                format!("{head}{prefix}")
            };
            self.set_query(next, window, cx);
        }
    }

    // --- render pieces -------------------------------------------------------------------

    fn render_item(
        &self,
        index: usize,
        item: &Item,
        highlighted: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *cx.tokens();
        let snap = self.snapshot(cx).clone();
        let icon: AnyElement = match &item.icon {
            ItemIcon::Icon(name) => Icon::new(*name)
                .size(px(16.))
                .color(t.icon_muted)
                .into_any_element(),
            ItemIcon::Project(id) => project_icon(snap.project(id), 16., false, cx),
        };
        let subtitle: Option<AnyElement> = item.thread.as_ref().map(|th| {
            let sep = || {
                div()
                    .text_color(t.muted_foreground.opacity(0.5))
                    .child("·")
                    .into_any_element()
            };
            let mut parts: Vec<AnyElement> = Vec::new();
            if let Some(p) = snap.project(&th.project_id) {
                parts.push(
                    h_flex()
                        .gap(px(4.))
                        .child(project_icon(Some(p), 12., false, cx))
                        .child(p.title.clone())
                        .into_any_element(),
                );
            }
            parts.push(sep());
            parts.push(div().child("Local").into_any_element());
            if let Some(branch) = &th.branch {
                parts.push(sep());
                parts.push(
                    h_flex()
                        .gap(px(4.))
                        .child(Icon::new(IconName::Folder).size(px(12.)))
                        .child(branch.clone())
                        .into_any_element(),
                );
            }
            parts.push(sep());
            parts.push(provider_icon(&th.instance_id, 12., cx));
            if th.current {
                parts.push(sep());
                parts.push(div().child("Current thread").into_any_element());
            }
            h_flex()
                .gap(px(4.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(t.muted_foreground.opacity(0.7))
                .children(parts)
                .into_any_element()
        });
        let item_clone = item.clone();
        div()
            .id(SharedString::from(format!("palette-item-{index}")))
            .flex()
            .flex_row()
            .items_start()
            .min_h(px(28.))
            .px(px(8.))
            .py(px(6.))
            .gap(px(8.))
            .rounded(radius::SM)
            .cursor_pointer()
            .text_scale(TextSize::Sm)
            .text_color(if highlighted {
                t.accent_foreground
            } else {
                t.foreground
            })
            .when(highlighted, |el| el.bg(t.accent))
            .child(div().h(px(20.)).pt(px(2.)).flex_none().child(icon))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        h_flex()
                            .min_w_0()
                            .child(
                                div()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(item.title.clone()),
                            )
                            .when_some(item.bold.clone(), |el, b| {
                                el.child(div().font_semibold().whitespace_nowrap().child(b))
                            }),
                    )
                    .when_some(item.description.clone(), |el, d| {
                        el.child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(t.muted_foreground.opacity(0.7))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(d),
                        )
                    })
                    .children(subtitle),
            )
            .child(
                h_flex()
                    .flex_none()
                    .self_center()
                    .gap(px(4.))
                    .when_some(item.trailing.clone(), |el, text| {
                        el.child(
                            div()
                                .min_w(px(48.))
                                .flex()
                                .justify_end()
                                .text_size(px(12.))
                                .text_color(t.muted_foreground.opacity(0.7))
                                .child(text),
                        )
                    })
                    .when_some(item.shortcut.clone(), |el, s| {
                        el.child(
                            div()
                                .ml(px(16.))
                                .font_medium()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(t.secondary_label)
                                .child(s),
                        )
                    })
                    .when(item.submenu, |el| {
                        el.child(
                            Icon::new(IconName::ChevronRight)
                                .size(px(16.))
                                .color(t.muted_foreground.opacity(0.7)),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.highlighted = Some(index);
                this.activate(item_clone.clone(), window, cx);
            }))
            .into_any_element()
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let s = *cx.states();
        let key = |content: AnyElement| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .h(px(20.))
                .min_w(px(20.))
                .px(px(4.))
                .rounded(radius::SM)
                .bg(s.selected)
                .text_size(px(12.))
                .line_height(px(16.))
                .font_medium()
                .text_color(t.foreground)
                .child(content)
                .into_any_element()
        };
        let icon_key = |name: IconName| key(Icon::new(name).size(px(12.)).into_any_element());
        let text_key = |label: &'static str| key(div().child(label).into_any_element());
        let group = |keys: Vec<AnyElement>, label: &'static str| {
            h_flex()
                .gap(px(4.))
                .child(h_flex().gap(px(4.)).children(keys))
                .child(label)
        };
        let browsing = self.browsing();
        let enter_label = if browsing {
            None
        } else if self.view() == Some(View::AddProject) {
            Some("Select")
        } else {
            Some("Select")
        };
        h_flex()
            .flex_none()
            .px(px(16.))
            .py(px(10.))
            .gap(px(12.))
            .border_t_1()
            .border_color(t.foreground.opacity(0.07))
            .bg(t.foreground.opacity(0.025))
            .text_size(px(14.))
            .line_height(px(20.))
            .font_medium()
            .text_color(t.muted_foreground)
            .child(group(
                vec![icon_key(IconName::ArrowUp), icon_key(IconName::ArrowDown)],
                "Navigate",
            ))
            .when_some(enter_label, |el, label| {
                el.child(group(vec![text_key("Enter")], label))
            })
            .when(!self.stack.is_empty(), |el| {
                el.child(group(vec![text_key("Backspace")], "Back"))
            })
            .child(group(vec![text_key("Esc")], "Close"))
            .when(browsing, |el| {
                el.child(div().flex_1()).child(
                    Button::new("palette-open-files")
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Xs)
                        .label("Open in Files")
                        .on_click(cx.listener(|this, _, _, cx| this.open_native_picker(cx))),
                )
            })
            .into_any_element()
    }
}

fn latest_unsettled(snap: &Snapshot, project_id: &str) -> Option<ThreadId> {
    snap.threads
        .iter()
        .filter(|s| {
            s.thread.project_id == project_id
                && s.thread.archived_at.is_none()
                && s.thread.deleted_at.is_none()
                && s.thread.settled_at.is_none()
        })
        .max_by_key(|s| model::activity_at(s))
        .map(|s| s.thread.id.clone())
}

/// Scores every item and ranks groups (`filterCommandPaletteGroups`).
fn rank(
    groups: &[Group],
    folded: &str,
    now_ms: i64,
    cx: &App,
    view: &CommandPaletteView,
) -> Vec<Group> {
    let snap = view.snapshot(cx);
    let items: Vec<Vec<Item>> = groups.iter().map(|g| g.items.clone()).collect();
    let ranked = search::rank_groups(&items, |_, item, typos| match &item.thread {
        Some(th) => {
            let project = snap.project(&th.project_id).map(|p| p.title.as_str());
            search::score_thread(
                folded,
                &search::ThreadSubject {
                    title: &item.title,
                    branch: th.branch.as_deref(),
                    project_title: project,
                    pull_request: th.pr.as_ref().map(|(n, t)| (*n, t.as_str())),
                    activity_at_ms: Some(th.activity_ms),
                },
                now_ms,
                typos,
            )
        }
        None => {
            let full = item.full_title();
            let mut terms: Vec<&str> = vec![&full];
            terms.extend(item.keywords.iter().map(String::as_str));
            search::score_terms(folded, &terms, typos)
        }
    });
    let threads_match = ranked.iter().any(|(g, _)| groups[*g].key == "threads");
    ranked
        .into_iter()
        .map(|(g, scored)| {
            let group = &groups[g];
            let limit = match group.key {
                "threads" => search::THREAD_LIMIT,
                "suggestions" => search::SECONDARY_LIMIT,
                "actions" | "projects" | "settings" if threads_match => search::SECONDARY_LIMIT,
                _ => usize::MAX,
            };
            Group {
                key: group.key,
                label: group.label.clone(),
                items: scored
                    .into_iter()
                    .take(limit)
                    .map(|(i, _)| group.items[i].clone())
                    .collect(),
            }
        })
        .collect()
}

impl Render for CommandPaletteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        let groups = self.groups(cx);
        let in_submenu = !self.stack.is_empty();
        let browsing = self.browsing();
        let total: usize = groups.iter().map(|g| g.items.len()).sum();
        if let Some(h) = self.highlighted
            && h >= total
        {
            self.highlighted = if total == 0 { None } else { Some(0) };
        }

        // Input band: px 8, py 6, 46 tall; the input text starts at x = 32.
        let leading: AnyElement = if in_submenu {
            div()
                .id("palette-back")
                .flex()
                .items_center()
                .justify_center()
                .size(px(20.))
                .rounded(radius::SM)
                .cursor_pointer()
                .hover(|s| s.bg(t.accent))
                .child(
                    Icon::new(IconName::ArrowLeft)
                        .size(px(16.))
                        .color(t.icon_muted),
                )
                .on_click(cx.listener(|this, _, window, cx| this.pop(window, cx)))
                .into_any_element()
        } else {
            Icon::new(IconName::Search)
                .size(px(16.))
                .color(t.icon_muted.opacity(0.8))
                .into_any_element()
        };
        let submit = browsing.then(|| {
            let exists = self.typed_path_exists(cx);
            let enter = if self.highlighted.is_some() {
                "Ctrl Enter"
            } else {
                "Enter"
            };
            h_flex()
                .id("palette-submit")
                .flex_none()
                .h(px(24.))
                .pl(px(7.))
                .pr(px(4.))
                .gap(px(6.))
                .rounded(radius::LG)
                .border_1()
                .border_color(t.border)
                .bg(cx.states().outline_fill)
                .cursor_pointer()
                .text_size(px(12.))
                .font_medium()
                .text_color(t.foreground)
                .hover(|s| s.bg(cx.states().outline_hover))
                .child(if exists { "Add" } else { "Create & Add" })
                .child(Kbd::new(enter))
                .on_click(cx.listener(|this, _, _, cx| this.submit_path(None, cx)))
        });
        let input_band = h_flex()
            .flex_none()
            .h(px(46.))
            .px(px(8.))
            .py(px(6.))
            .child(
                h_flex()
                    .w(px(32.))
                    .h(px(34.))
                    .pl(px(3.))
                    .flex_none()
                    .child(leading),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                        this.move_highlight(1, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                        this.move_highlight(-1, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this, e: &Enter, window, cx| {
                        this.on_enter(e.secondary, window, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this, _: &InputEscape, _, cx| {
                        this.close(cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                        this.on_tab(window, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this, _: &Backspace, window, cx| {
                        if this.query.is_empty() && !this.stack.is_empty() {
                            this.pop(window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        Input::new(&self.input)
                            .appearance(false)
                            .h(px(34.))
                            .text_size(px(14.))
                            .line_height(px(34.)),
                    ),
            )
            .children(submit.map(|s| div().pr(px(2.)).child(s)));

        // Results.
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut index = 0;
        for (gi, group) in groups.iter().enumerate() {
            rows.push(
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .when(gi > 0, |el| el.pt(px(12.)))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_medium()
                    .text_color(t.muted_foreground)
                    .child(group.label.clone())
                    .into_any_element(),
            );
            for item in &group.items {
                let highlighted = self.highlighted == Some(index);
                rows.push(self.render_item(index, item, highlighted, cx));
                index += 1;
            }
        }
        let status: Option<AnyElement> = browsing
            .then(|| {
                let text = if self.listing.loading {
                    Some("Loading folders...".to_string())
                } else {
                    self.listing.entries.as_ref().err().cloned()
                };
                text.map(|text| {
                    div()
                        .px(px(16.))
                        .py(px(12.))
                        .text_size(px(14.))
                        .text_color(t.muted_foreground)
                        .child(text)
                        .into_any_element()
                })
            })
            .flatten();
        let empty = (total == 0 && !browsing).then(|| {
            div()
                .py(px(40.))
                .flex()
                .justify_center()
                .text_size(px(14.))
                .text_color(t.muted_foreground)
                .child(if self.query.starts_with('>') {
                    "No matching actions."
                } else {
                    "No matching commands, projects, or threads."
                })
                .into_any_element()
        });
        let list = div()
            .id("palette-results")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .when(total > 0, |el| el.p(px(8.)))
            .children(rows)
            .children(status)
            .children(empty);

        let _ = window;
        v_flex()
            .id("command-palette")
            .key_context("CommandPalette")
            .max_h(px(420.))
            .font_family(font::SANS)
            .text_color(t.foreground)
            .child(input_band)
            .child(list)
            .child(self.render_footer(cx))
    }
}

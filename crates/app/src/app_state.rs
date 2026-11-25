//! `AppState`: the window's navigation and layout state (spec 02 §1).
//!
//! One entity per app, reachable from anywhere with [`AppState::global`]. Views
//! observe it (`cx.observe(&app_state, |_, _, cx| cx.notify())`) or read it during
//! render. Mutate through its methods so the clamps and history rules stay in one
//! place:
//!
//! ```ignore
//! let state = AppState::global(cx);
//! state.update(cx, |s, cx| s.navigate(Route::Thread(id), cx));
//! state.update(cx, |s, cx| s.toggle_right_panel(cx));
//! ```

use gpui::{App, AppContext as _, Context, Entity, Global, Pixels, Window};

use crate::theme::{Theme, ThemeMode};
use tern_core::{ProjectId, ThreadId};

/// Settings pages, in nav order (spec 06).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingsPage {
    General,
    Appearance,
    Chat,
    Diffs,
    Keybindings,
    Providers,
    Threads,
    Projects,
    SourceControl,
    Worktrees,
    Maintenance,
    About,
    OpenSourceLicenses,
    Archive,
}

impl SettingsPage {
    pub const ALL: [SettingsPage; 14] = [
        Self::General,
        Self::Appearance,
        Self::Chat,
        Self::Diffs,
        Self::Keybindings,
        Self::Providers,
        Self::Threads,
        Self::Projects,
        Self::SourceControl,
        Self::Worktrees,
        Self::Maintenance,
        Self::About,
        Self::OpenSourceLicenses,
        Self::Archive,
    ];

    /// URL slug, `/settings/<slug>` in T3.
    pub fn slug(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Appearance => "appearance",
            Self::Chat => "chat",
            Self::Diffs => "diffs",
            Self::Keybindings => "keybindings",
            Self::Providers => "providers",
            Self::Threads => "threads",
            Self::Projects => "projects",
            Self::SourceControl => "source-control",
            Self::Worktrees => "worktrees",
            Self::Maintenance => "maintenance",
            Self::About => "about",
            Self::OpenSourceLicenses => "open-source-licenses",
            Self::Archive => "archive",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Chat => "Chat",
            Self::Diffs => "Diffs & pull requests",
            Self::Keybindings => "Keybindings",
            Self::Providers => "Providers",
            Self::Threads => "Threads",
            Self::Projects => "Projects",
            Self::SourceControl => "Source control",
            Self::Worktrees => "Worktrees",
            Self::Maintenance => "Maintenance",
            Self::About => "About",
            Self::OpenSourceLicenses => "Open source licenses",
            Self::Archive => "Archive",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.slug() == slug)
    }
}

/// What the main column shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    /// `/`: resolves to a draft in the most recent project, or the no-projects hero.
    Index,
    /// `/draft/<id>`: a local draft in a project.
    Draft(ProjectId),
    /// `/<thread id>`.
    Thread(ThreadId),
    /// `/settings/<page>`: a modal over the last chat route.
    Settings(SettingsPage),
    /// Developer gallery of every primitive (`--route gallery`). Not in T3.
    Gallery,
}

impl Route {
    /// Parses a `--route` value: `index`, `draft:<project>`, `thread:<id>`,
    /// `settings`, `settings/<page>`, `gallery`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim_start_matches('/');
        match s {
            "" | "index" => return Some(Self::Index),
            "settings" => return Some(Self::Settings(SettingsPage::General)),
            "gallery" => return Some(Self::Gallery),
            _ => {}
        }
        if let Some(page) = s.strip_prefix("settings/") {
            return SettingsPage::from_slug(page).map(Self::Settings);
        }
        if let Some(id) = s
            .strip_prefix("draft:")
            .or_else(|| s.strip_prefix("draft/"))
        {
            return Some(Self::Draft(id.to_string()));
        }
        if let Some(id) = s
            .strip_prefix("thread:")
            .or_else(|| s.strip_prefix("thread/"))
        {
            return Some(Self::Thread(id.to_string()));
        }
        None
    }

    pub fn is_settings(&self) -> bool {
        matches!(self, Self::Settings(_))
    }

    pub fn thread_id(&self) -> Option<&ThreadId> {
        match self {
            Self::Thread(id) => Some(id),
            _ => None,
        }
    }
}

/// Sidebar geometry (spec 02 §1.3, §1.6).
pub mod sidebar {
    use gpui::{Pixels, px};
    pub const DEFAULT_WIDTH: Pixels = px(256.);
    pub const MIN_WIDTH: Pixels = px(208.);
    /// The main column keeps at least this much: max width = window - 640.
    pub const MAIN_MIN_WIDTH: Pixels = px(640.);
    /// Dragging below `min * 0.25` collapses on release.
    pub const COLLAPSE_RATIO: f32 = 0.25;
    /// Rail hit strip, centered on the edge.
    pub const RAIL_WIDTH: Pixels = px(16.);
}

/// Right panel geometry (spec 02 §1.3).
pub mod right_panel {
    use gpui::{Pixels, px};
    pub const DEFAULT_WIDTH: Pixels = px(540.);
    pub const MIN_WIDTH: Pixels = px(360.);
    /// Below this window width the panel overlays instead of taking a column.
    pub const OVERLAY_BELOW: Pixels = px(980.);
}

/// Terminal drawer geometry (spec 02 §1.4).
pub mod drawer {
    use gpui::{Pixels, px};
    pub const DEFAULT_HEIGHT: Pixels = px(280.);
    pub const MIN_HEIGHT: Pixels = px(180.);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SidebarLayout {
    pub width: Pixels,
    pub open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelLayout {
    pub open: bool,
    /// Width for the right panel, height for the drawer.
    pub size: Pixels,
}

pub struct AppState {
    route: Route,
    /// The chat route under the settings modal.
    chat_route: Route,
    back: Vec<Route>,
    forward: Vec<Route>,
    pub sidebar: SidebarLayout,
    pub right_panel: PanelLayout,
    pub right_panel_maximized: bool,
    /// The thread details panel (top right card), toggled from the titlebar.
    pub thread_details_open: bool,
    pub drawer: PanelLayout,
    pub palette_open: bool,
    theme_mode: ThemeMode,
}

struct GlobalAppState(Entity<AppState>);
impl Global for GlobalAppState {}

impl AppState {
    pub fn new(route: Route, theme_mode: ThemeMode) -> Self {
        let chat_route = if route.is_settings() {
            Route::Index
        } else {
            route.clone()
        };
        Self {
            route,
            chat_route,
            back: Vec::new(),
            forward: Vec::new(),
            sidebar: SidebarLayout {
                width: sidebar::DEFAULT_WIDTH,
                open: true,
            },
            right_panel: PanelLayout {
                open: false,
                size: right_panel::DEFAULT_WIDTH,
            },
            right_panel_maximized: false,
            thread_details_open: true,
            drawer: PanelLayout {
                open: false,
                size: drawer::DEFAULT_HEIGHT,
            },
            palette_open: false,
            theme_mode,
        }
    }

    /// Creates the entity and registers it as the global.
    pub fn install(route: Route, theme_mode: ThemeMode, cx: &mut App) -> Entity<Self> {
        let entity = cx.new(|_| Self::new(route, theme_mode));
        cx.set_global(GlobalAppState(entity.clone()));
        entity
    }

    pub fn global(cx: &App) -> Entity<Self> {
        cx.global::<GlobalAppState>().0.clone()
    }

    // --- routing -----------------------------------------------------------

    pub fn route(&self) -> &Route {
        &self.route
    }

    /// The route the main column renders (the chat under settings).
    pub fn chat_route(&self) -> &Route {
        &self.chat_route
    }

    pub fn settings_page(&self) -> Option<SettingsPage> {
        match self.route {
            Route::Settings(page) => Some(page),
            _ => None,
        }
    }

    pub fn navigate(&mut self, route: Route, cx: &mut Context<Self>) {
        if route == self.route {
            return;
        }
        let previous = std::mem::replace(&mut self.route, route.clone());
        self.back.push(previous);
        self.forward.clear();
        if !route.is_settings() {
            self.chat_route = route;
        }
        cx.notify();
    }

    /// Replaces the current route without a history entry (index -> draft).
    pub fn replace(&mut self, route: Route, cx: &mut Context<Self>) {
        if !route.is_settings() {
            self.chat_route = route.clone();
        }
        self.route = route;
        cx.notify();
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(route) = self.back.pop() {
            let current = std::mem::replace(&mut self.route, route.clone());
            self.forward.push(current);
            if !route.is_settings() {
                self.chat_route = route;
            }
            cx.notify();
        }
    }

    pub fn forward(&mut self, cx: &mut Context<Self>) {
        if let Some(route) = self.forward.pop() {
            let current = std::mem::replace(&mut self.route, route.clone());
            self.back.push(current);
            if !route.is_settings() {
                self.chat_route = route;
            }
            cx.notify();
        }
    }

    pub fn open_settings(&mut self, page: SettingsPage, cx: &mut Context<Self>) {
        self.navigate(Route::Settings(page), cx);
    }

    /// Leaves settings for the chat route underneath.
    pub fn close_settings(&mut self, cx: &mut Context<Self>) {
        if self.route.is_settings() {
            let chat = self.chat_route.clone();
            self.navigate(chat, cx);
        }
    }

    // --- sidebar -------------------------------------------------------------

    /// Max sidebar width for a window: window - 640, never below the min.
    pub fn sidebar_max_width(window_width: Pixels) -> Pixels {
        (window_width - sidebar::MAIN_MIN_WIDTH).max(sidebar::MIN_WIDTH)
    }

    /// Applies a drag. Growing is refused when it would leave the main column
    /// under 640; shrinking is always accepted; the result is clamped.
    pub fn resize_sidebar(&mut self, width: Pixels, window_width: Pixels, cx: &mut Context<Self>) {
        let growing = width > self.sidebar.width;
        if growing && window_width - width < sidebar::MAIN_MIN_WIDTH {
            let max = Self::sidebar_max_width(window_width);
            if self.sidebar.width >= max {
                return;
            }
        }
        let next = width.clamp(sidebar::MIN_WIDTH, Self::sidebar_max_width(window_width));
        if next != self.sidebar.width {
            self.sidebar.width = next;
            cx.notify();
        }
    }

    /// Double-click on the rail: back to 256.
    pub fn reset_sidebar_width(&mut self, cx: &mut Context<Self>) {
        self.sidebar.width = sidebar::DEFAULT_WIDTH;
        cx.notify();
    }

    pub fn set_sidebar_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.sidebar.open != open {
            self.sidebar.open = open;
            cx.notify();
        }
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.set_sidebar_open(!self.sidebar.open, cx);
    }

    // --- right panel ---------------------------------------------------------

    /// Max right-panel width: min(70% of the window, row width - 360).
    pub fn right_panel_max_width(window_width: Pixels, row_width: Pixels) -> Pixels {
        (window_width * 0.7)
            .min(row_width - right_panel::MIN_WIDTH)
            .max(right_panel::MIN_WIDTH)
    }

    pub fn set_right_panel_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.right_panel.open != open {
            self.right_panel.open = open;
            if !open {
                self.right_panel_maximized = false;
            }
            cx.notify();
        }
    }

    pub fn toggle_right_panel(&mut self, cx: &mut Context<Self>) {
        self.set_right_panel_open(!self.right_panel.open, cx);
    }

    pub fn toggle_right_panel_maximized(&mut self, cx: &mut Context<Self>) {
        self.right_panel_maximized = !self.right_panel_maximized;
        cx.notify();
    }

    pub fn resize_right_panel(
        &mut self,
        width: Pixels,
        window_width: Pixels,
        row_width: Pixels,
        cx: &mut Context<Self>,
    ) {
        let next = width.clamp(
            right_panel::MIN_WIDTH,
            Self::right_panel_max_width(window_width, row_width),
        );
        if next != self.right_panel.size {
            self.right_panel.size = next;
            cx.notify();
        }
    }

    pub fn toggle_thread_details(&mut self, cx: &mut Context<Self>) {
        self.thread_details_open = !self.thread_details_open;
        cx.notify();
    }

    // --- terminal drawer -----------------------------------------------------

    pub fn set_drawer_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.drawer.open != open {
            self.drawer.open = open;
            cx.notify();
        }
    }

    pub fn toggle_drawer(&mut self, cx: &mut Context<Self>) {
        self.set_drawer_open(!self.drawer.open, cx);
    }

    /// Max drawer height: min(75% of the window, column height - header - composer).
    pub fn resize_drawer(
        &mut self,
        height: Pixels,
        window_height: Pixels,
        available: Pixels,
        cx: &mut Context<Self>,
    ) {
        let max = (window_height * 0.75)
            .min(available)
            .max(drawer::MIN_HEIGHT);
        let next = height.clamp(drawer::MIN_HEIGHT, max);
        if next != self.drawer.size {
            self.drawer.size = next;
            cx.notify();
        }
    }

    // --- palette and theme ---------------------------------------------------

    pub fn set_palette_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.palette_open != open {
            self.palette_open = open;
            cx.notify();
        }
    }

    pub fn toggle_palette(&mut self, cx: &mut Context<Self>) {
        self.set_palette_open(!self.palette_open, cx);
    }

    pub fn theme_mode(&self) -> ThemeMode {
        self.theme_mode
    }

    /// Sets the theme preference and repaints.
    pub fn set_theme_mode(&mut self, mode: ThemeMode, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_mode = mode;
        Theme::set_mode(mode, window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    #[test]
    fn parses_routes() {
        assert_eq!(
            Route::parse("settings"),
            Some(Route::Settings(SettingsPage::General))
        );
        assert_eq!(
            Route::parse("settings/appearance"),
            Some(Route::Settings(SettingsPage::Appearance))
        );
        assert_eq!(
            Route::parse("thread:abc"),
            Some(Route::Thread("abc".into()))
        );
        assert_eq!(Route::parse("/"), Some(Route::Index));
        assert_eq!(Route::parse("nope"), None);
    }

    #[test]
    fn sidebar_max_leaves_main_640() {
        assert_eq!(AppState::sidebar_max_width(px(1100.)), px(460.));
        assert_eq!(AppState::sidebar_max_width(px(700.)), sidebar::MIN_WIDTH);
    }

    #[test]
    fn right_panel_max() {
        assert_eq!(
            AppState::right_panel_max_width(px(1440.), px(1184.)),
            px(824.)
        );
    }
}

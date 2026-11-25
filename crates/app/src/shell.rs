//! The app shell (spec 02 §1): window chrome, the 40px titlebar strip, the
//! resizable/collapsible sidebar, the main column with the chat column, the
//! right-panel and terminal-drawer slots, the settings modal and palette hosts,
//! and the overlay layers (dialogs, toasts, popups, tooltips).
//!
//! The shell owns layout and chrome only. Content views live in `crate::views`.

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, CursorStyle, Decorations, Entity,
    FocusHandle, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, actions, div,
    prelude::FluentBuilder as _, px,
};

use crate::app_state::{AppState, Route, sidebar};
use crate::assets::WindowGlyph;
use crate::gallery::GalleryView;
use crate::store::{Store, StoreEvent};
use crate::theme::{ActiveTheme as _, Scope, TextExt as _, Theme, font, layout, radius, scoped};
use crate::ui::{
    self, Button, ButtonSize, ButtonVariant, Dialogs, Grain, IconName, Toast, Toasts, Toggle,
    ToggleSize, ToggleVariant, Tooltip, TooltipExt as _, TooltipSide, h_flex,
};
use crate::views::{
    ChatView, CommandPaletteView, ComposerView, DiffPanelView, SettingsView, SidebarView,
    TerminalDrawerView,
};

actions!(
    tern,
    [
        ToggleSidebar,
        ToggleTerminal,
        ToggleRightPanel,
        ToggleThreadDetails,
        ToggleCommandPalette,
        GoBack,
        GoForward,
        Dismiss,
    ]
);

/// Default shell key bindings (spec 02 §5.2, Linux).
pub fn bind_keys(cx: &mut App) {
    use gpui::KeyBinding;
    cx.bind_keys([
        KeyBinding::new("ctrl-b", ToggleSidebar, Some("Workspace")),
        KeyBinding::new("ctrl-j", ToggleTerminal, Some("Workspace")),
        KeyBinding::new("ctrl-alt-b", ToggleRightPanel, Some("Workspace")),
        KeyBinding::new("ctrl-shift-b", ToggleThreadDetails, Some("Workspace")),
        KeyBinding::new("ctrl-k", ToggleCommandPalette, Some("Workspace")),
        KeyBinding::new("ctrl-[", GoBack, Some("Workspace")),
        KeyBinding::new("ctrl-]", GoForward, Some("Workspace")),
        KeyBinding::new("escape", Dismiss, Some("Workspace")),
    ]);
    crate::views::right_panel::init(cx);
}

/// Whether Tern draws its own window controls (client-side decorations).
pub fn draws_window_controls(window: &Window) -> bool {
    cfg!(target_os = "linux") && matches!(window.window_decorations(), Decorations::Client { .. })
}

/// `--workspace-controls-right`: the window controls' width + 12 when Tern draws
/// them, else 12. Fixed titlebar content on the right sits this far from the edge.
pub fn workspace_controls_right(window: &Window) -> Pixels {
    if draws_window_controls(window) {
        layout::WINDOW_CONTROLS_WIDTH + px(12.)
    } else {
        px(12.)
    }
}

/// Width the right titlebar controls take from the window's right edge, so headers
/// can keep their content clear of them.
pub fn titlebar_controls_width(window: &Window, right_panel_open: bool) -> Pixels {
    let toggles = if right_panel_open { 4. } else { 3. };
    workspace_controls_right(window) + px(28. * toggles + 4. * (toggles - 1.))
}

#[derive(Clone, Copy, Debug)]
enum Drag {
    Sidebar {
        start_x: Pixels,
        start_width: Pixels,
    },
    Restore {
        start_x: Pixels,
    },
    RightPanel {
        start_x: Pixels,
        start_width: Pixels,
    },
    Drawer {
        start_y: Pixels,
        start_height: Pixels,
    },
}

pub struct Shell {
    app: Entity<AppState>,
    store: Entity<Store>,
    sidebar: Entity<SidebarView>,
    chat: Entity<ChatView>,
    composer: Entity<ComposerView>,
    diff_panel: Entity<DiffPanelView>,
    terminal: Entity<TerminalDrawerView>,
    settings: Entity<SettingsView>,
    palette: Entity<CommandPaletteView>,
    gallery: Entity<GalleryView>,
    dialogs: Entity<Dialogs>,
    toasts: Entity<Toasts>,
    popups: Entity<ui::popup::PopupLayer>,
    tooltips: Entity<ui::tooltip::TooltipLayer>,
    focus: FocusHandle,
    drag: Option<Drag>,
    /// The sidebar is being dragged below the collapse threshold.
    collapsing: bool,
    /// Live width while restoring a collapsed sidebar by dragging the handle.
    restore_preview: Option<Pixels>,
    restore_hover: bool,
    /// Main-column width from the last frame (right panel max).
    row_width: Pixels,
    chat_column_height: Pixels,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = AppState::global(cx);
        let store = Store::global(cx);
        cx.observe(&app, |this, app, cx| {
            let thread = app.read(cx).chat_route().thread_id().cloned();
            this.toasts.update(cx, |t, cx| {
                t.set_current_thread(thread.map(SharedString::from), cx)
            });
            cx.notify();
        })
        .detach();
        cx.subscribe(&store, |_, _, event: &StoreEvent, cx| match event {
            StoreEvent::Error(message) => {
                Toasts::push(Toast::error(message.clone()), cx);
            }
            StoreEvent::Notice(message) => {
                Toasts::push(Toast::info(message.clone()), cx);
            }
            _ => {}
        })
        .detach();
        cx.observe_window_appearance(window, |_, window, cx| {
            Theme::sync_system_appearance(window, cx);
        })
        .detach();
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            sidebar: cx.new(|cx| SidebarView::new(window, cx)),
            chat: cx.new(|cx| ChatView::new(window, cx)),
            composer: cx.new(|cx| ComposerView::new(window, cx)),
            diff_panel: cx.new(|cx| DiffPanelView::new(window, cx)),
            terminal: cx.new(|cx| TerminalDrawerView::new(window, cx)),
            settings: cx.new(|cx| SettingsView::new(window, cx)),
            palette: cx.new(|cx| CommandPaletteView::new(window, cx)),
            gallery: cx.new(|cx| GalleryView::new(window, cx)),
            dialogs: ui::dialog::new_host(cx),
            toasts: ui::toast::new_host(cx),
            popups: ui::popup::new_layer(cx),
            tooltips: ui::tooltip::new_layer(cx),
            app,
            store,
            focus,
            drag: None,
            collapsing: false,
            restore_preview: None,
            restore_hover: false,
            row_width: px(0.),
            chat_column_height: px(0.),
        }
    }

    // --- actions -------------------------------------------------------------

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| s.toggle_sidebar(cx));
    }

    fn toggle_terminal(&mut self, _: &ToggleTerminal, _: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| s.toggle_drawer(cx));
    }

    fn toggle_right_panel(&mut self, _: &ToggleRightPanel, _: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| s.toggle_right_panel(cx));
    }

    fn toggle_details(&mut self, _: &ToggleThreadDetails, _: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| s.toggle_thread_details(cx));
    }

    fn toggle_palette(&mut self, _: &ToggleCommandPalette, _: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| s.toggle_palette(cx));
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| s.back(cx));
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| s.forward(cx));
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        let app = self.app.read(cx);
        if app.palette_open {
            self.app.update(cx, |s, cx| s.set_palette_open(false, cx));
        } else if app.route().is_settings() {
            self.app.update(cx, |s, cx| s.close_settings(cx));
        } else {
            cx.propagate();
        }
    }

    // --- dragging ------------------------------------------------------------

    fn on_mouse_move(&mut self, e: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag else {
            return;
        };
        if e.pressed_button != Some(MouseButton::Left) {
            self.end_drag(window, cx);
            return;
        }
        let viewport = window.viewport_size();
        match drag {
            Drag::Sidebar {
                start_x,
                start_width,
            } => {
                let raw = start_width + (e.position.x - start_x);
                let collapsing = raw < sidebar::MIN_WIDTH * sidebar::COLLAPSE_RATIO;
                if collapsing != self.collapsing {
                    self.collapsing = collapsing;
                    cx.notify();
                }
                if !collapsing {
                    self.app
                        .update(cx, |s, cx| s.resize_sidebar(raw, viewport.width, cx));
                }
            }
            Drag::Restore { start_x } => {
                let dx = e.position.x - start_x;
                self.restore_preview = (dx >= px(48.)).then(|| {
                    e.position.x.clamp(
                        sidebar::MIN_WIDTH,
                        AppState::sidebar_max_width(viewport.width),
                    )
                });
                cx.notify();
            }
            Drag::RightPanel {
                start_x,
                start_width,
            } => {
                let width = start_width - (e.position.x - start_x);
                let row = self.row_width;
                self.app.update(cx, |s, cx| {
                    s.resize_right_panel(width, viewport.width, row, cx)
                });
            }
            Drag::Drawer {
                start_y,
                start_height,
            } => {
                let height = start_height - (e.position.y - start_y);
                // Column height minus header (40) and a minimal composer.
                let available = self.chat_column_height - layout::TOPBAR_HEIGHT - px(130.);
                self.app.update(cx, |s, cx| {
                    s.resize_drawer(height, viewport.height, available, cx)
                });
            }
        }
    }

    fn end_drag(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        match self.drag.take() {
            Some(Drag::Sidebar { .. }) if self.collapsing => {
                self.collapsing = false;
                self.app.update(cx, |s, cx| s.set_sidebar_open(false, cx));
            }
            Some(Drag::Restore { .. }) => {
                let preview = self.restore_preview.take();
                self.app.update(cx, |s, cx| {
                    if let Some(width) = preview {
                        s.sidebar.width = width;
                    }
                    s.set_sidebar_open(true, cx);
                });
            }
            _ => {}
        }
        cx.notify();
    }

    // --- pieces --------------------------------------------------------------

    fn render_sidebar(&mut self, width: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.tern_theme().clone();
        let sc = theme.sidebar_colors;
        let rail_line = sc.sidebar_border;
        div()
            .id("sidebar")
            .relative()
            .flex_none()
            .h_full()
            .w(width)
            .bg(sc.sidebar)
            .border_r_1()
            .border_color(sc.border)
            .text_color(sc.sidebar_foreground)
            .child(Grain::new())
            .child(
                div()
                    .size_full()
                    .overflow_hidden()
                    .child(scoped(Scope::Sidebar, self.sidebar.clone())),
            )
            .child(
                // SidebarRail: a 16px hit strip centered on the right edge.
                div()
                    .id("sidebar-rail")
                    .group("sidebar-rail")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(-8.))
                    .w(sidebar::RAIL_WIDTH)
                    .cursor(CursorStyle::ResizeLeftRight)
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(7.))
                            .w(px(2.))
                            .group_hover("sidebar-rail", move |s| s.bg(rail_line)),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, e: &MouseDownEvent, _, cx| {
                            let start_width = this.app.read(cx).sidebar.width;
                            this.drag = Some(Drag::Sidebar {
                                start_x: e.position.x,
                                start_width,
                            });
                            cx.stop_propagation();
                        }),
                    )
                    .on_click(cx.listener(|this, e: &ClickEvent, _, cx| {
                        if e.click_count() == 2 {
                            this.app.update(cx, |s, cx| s.reset_sidebar_width(cx));
                        }
                    })),
            )
            .into_any_element()
    }

    fn render_restore_handle(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let border = cx.tern_theme().sidebar_colors.sidebar_border;
        let top = (window.viewport_size().height - px(64.)) / 2.;
        let show = self.restore_hover || matches!(self.drag, Some(Drag::Restore { .. }));
        div()
            .id("sidebar-restore")
            .absolute()
            .left(px(8.))
            .top(top)
            .w(px(16.))
            .h(px(64.))
            .cursor_pointer()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.restore_hover = *hovered;
                cx.notify();
            }))
            .when(show, |el| {
                el.child(div().w(px(8.)).h_full().rounded(radius::MD).bg(border))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    this.drag = Some(Drag::Restore {
                        start_x: e.position.x,
                    });
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }

    fn render_chat_column(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let app = self.app.read(cx);
        let drawer = app.drawer;
        let t = *cx.tokens();
        let this = cx.entity().downgrade();
        div()
            .id("chat-column")
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                gpui::canvas(
                    move |bounds, _, cx| {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, _| this.chat_column_height = bounds.size.height);
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(div().flex_1().min_h_0().child(self.chat.clone()))
            .child(div().flex_none().child(self.composer.clone()))
            .when(drawer.open, |el| {
                el.child(
                    div()
                        .relative()
                        .flex_none()
                        .h(drawer.size)
                        .border_t_1()
                        .border_color(t.border)
                        .child(self.terminal.clone())
                        .child(
                            div()
                                .id("drawer-handle")
                                .absolute()
                                .left_0()
                                .right_0()
                                .top(px(-4.))
                                .h(px(8.))
                                .cursor(CursorStyle::ResizeUpDown)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, e: &MouseDownEvent, _, cx| {
                                        let start_height = this.app.read(cx).drawer.size;
                                        this.drag = Some(Drag::Drawer {
                                            start_y: e.position.y,
                                            start_height,
                                        });
                                        cx.stop_propagation();
                                    }),
                                ),
                        ),
                )
            })
            .into_any_element()
    }

    fn render_right_panel(&mut self, overlay: bool, cx: &mut Context<Self>) -> AnyElement {
        let app = self.app.read(cx);
        let width = app.right_panel.size;
        let maximized = app.right_panel_maximized;
        let theme = cx.tern_theme().clone();
        let t = theme.colors;
        div()
            .id("right-panel")
            .relative()
            .flex_none()
            .h_full()
            .map(|el| if maximized { el.flex_1() } else { el.w(width) })
            .bg(t.background)
            .border_l_1()
            .border_color(t.border)
            .when(overlay, |el| {
                el.absolute()
                    .top_0()
                    .right_0()
                    .shadow(theme.shadows.lg(0.05))
            })
            .child(self.diff_panel.clone())
            .when(!maximized, |el| {
                el.child(
                    div()
                        .id("right-panel-handle")
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(-4.))
                        .w(px(8.))
                        .cursor(CursorStyle::ResizeLeftRight)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, e: &MouseDownEvent, _, cx| {
                                let start_width = this.app.read(cx).right_panel.size;
                                this.drag = Some(Drag::RightPanel {
                                    start_x: e.position.x,
                                    start_width,
                                });
                                cx.stop_propagation();
                            }),
                        ),
                )
            })
            .into_any_element()
    }

    /// The fixed top-right controls: panel toggles (`PanelLayoutControls`), then the
    /// window controls.
    fn render_titlebar_right(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let app = self.app.read(cx);
        let (rp_open, details, drawer, maximized) = (
            app.right_panel.open,
            app.thread_details_open,
            app.drawer.open,
            app.right_panel_maximized,
        );
        let has_project = !self.store.read(cx).snapshot().projects.is_empty();
        let panel_width = app.right_panel.size;
        let details_toggle = self.panel_toggle(
            "tb-details",
            IconName::SquareMenu,
            details,
            true,
            "Toggle thread details (Ctrl+Shift+B)",
            |s, cx| s.toggle_thread_details(cx),
        );
        let app_entity = self.app.clone();
        let toggles = h_flex()
            .gap(px(4.))
            .when(rp_open, |el| {
                let app = app_entity.clone();
                el.child(
                    div()
                        .id("tb-maximize")
                        .t3_tooltip(
                            Tooltip::text(if maximized {
                                "Restore panel size"
                            } else {
                                "Maximize panel"
                            })
                            .side(TooltipSide::Bottom),
                        )
                        .child(
                            Toggle::new("tb-maximize-toggle")
                                .variant(ToggleVariant::Ghost)
                                .size(ToggleSize::Sm)
                                .icon(if maximized {
                                    IconName::Minimize2
                                } else {
                                    IconName::Maximize2
                                })
                                .pressed(maximized)
                                .on_click(move |_, _, cx| {
                                    app.update(cx, |s, cx| s.toggle_right_panel_maximized(cx))
                                }),
                        ),
                )
            })
            // With the right panel open, thread details moves to the chat header.
            .when(!rp_open, |el| el.child(details_toggle))
            .child(self.panel_toggle(
                "tb-terminal",
                IconName::PanelBottom,
                drawer,
                has_project,
                "Toggle terminal drawer (Ctrl+J)",
                |s, cx| s.toggle_drawer(cx),
            ))
            .child(self.panel_toggle(
                "tb-right-panel",
                IconName::PanelRight,
                rp_open,
                has_project,
                "Toggle right panel (Ctrl+Alt+B)",
                |s, cx| s.toggle_right_panel(cx),
            ));
        let details_in_header = (rp_open && !maximized).then(|| {
            let details_toggle = self.panel_toggle(
                "tb-details-header",
                IconName::SquareMenu,
                details,
                true,
                "Toggle thread details (Ctrl+Shift+B)",
                |s, cx| s.toggle_thread_details(cx),
            );
            h_flex()
                .absolute()
                .top_0()
                .right(panel_width + px(12.))
                .h(layout::TOPBAR_HEIGHT)
                .child(details_toggle)
        });
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .children(details_in_header)
            .child(
                h_flex()
                    .absolute()
                    .top_0()
                    .right_0()
                    .h(layout::TOPBAR_HEIGHT)
                    .child(toggles)
                    .child(div().w(px(12.)))
                    .when(draws_window_controls(window), |el| {
                        el.child(window_controls(window, cx))
                    }),
            )
            .into_any_element()
    }

    fn panel_toggle(
        &self,
        id: &'static str,
        icon: IconName,
        pressed: bool,
        enabled: bool,
        tip: &'static str,
        act: fn(&mut AppState, &mut Context<AppState>),
    ) -> AnyElement {
        let app = self.app.clone();
        div()
            .id(SharedString::from(format!("{id}-tip")))
            .t3_tooltip(Tooltip::text(tip).side(TooltipSide::Bottom))
            .child(
                Toggle::new(id)
                    .variant(ToggleVariant::Ghost)
                    .size(ToggleSize::Sm)
                    .icon(icon)
                    .pressed(pressed)
                    .disabled(!enabled)
                    .on_click(move |_, _, cx| app.update(cx, act)),
            )
            .into_any_element()
    }

    fn render_settings_modal(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.tern_theme().clone();
        let t = theme.colors;
        let pad = if window.viewport_size().width >= px(1024.) {
            40.
        } else {
            24.
        };
        let app = self.app.clone();
        div()
            .id("settings-modal")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .bg(theme.glass.dialog_backdrop)
            .p(px(pad))
            .flex()
            .items_center()
            .justify_center()
            .on_click(move |_, _, cx| app.update(cx, |s, cx| s.close_settings(cx)))
            .child(
                div()
                    .id("settings-surface")
                    .size_full()
                    .max_w(px(1280.))
                    .max_h(px(960.))
                    .rounded(radius::XL2)
                    .border_1()
                    .border_color(t.border)
                    .bg(t.background)
                    .text_color(t.foreground)
                    .shadow(theme.shadows.xl2(0.4))
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(self.settings.clone()),
            )
            .with_animation_fade("settings-enter")
    }

    fn render_palette(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.tern_theme().clone();
        let top = (window.viewport_size().height * 0.1).max(layout::TOPBAR_HEIGHT + px(8.));
        let app = self.app.clone();
        div()
            .id("palette")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .bg(theme.glass.dialog_backdrop)
            .flex()
            .flex_col()
            .items_center()
            .px(px(16.))
            .pt(top)
            .on_click(move |_, _, cx| app.update(cx, |s, cx| s.set_palette_open(false, cx)))
            .child(
                div()
                    .id("palette-surface")
                    .w_full()
                    .max_w(px(576.))
                    .max_h(px(420.))
                    .rounded(radius::XL2)
                    .border_1()
                    .border_color(theme.glass.dialog_border)
                    .bg(theme.glass.dialog)
                    .shadow(theme.shadows.dialog())
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(self.palette.clone()),
            )
            .with_animation_fade("palette-enter")
    }
}

/// Small helper: the 200ms dialog fade on an overlay root.
trait FadeIn {
    fn with_animation_fade(self, id: &'static str) -> AnyElement;
}

impl FadeIn for gpui::Stateful<gpui::Div> {
    fn with_animation_fade(self, id: &'static str) -> AnyElement {
        use gpui::{Animation, AnimationExt as _};
        self.with_animation(
            id,
            Animation::new(crate::theme::motion::DIALOG)
                .with_easing(crate::theme::motion::ease_standard()),
            |el, t| el.opacity(t),
        )
        .into_any_element()
    }
}

/// Minimize / maximize-restore / close, 46px each, full titlebar height. Glyphs in
/// `titleBarOverlay.symbolColor`; close hovers red.
fn window_controls(window: &Window, cx: &App) -> AnyElement {
    let symbol = cx.tern_theme().extra.window_symbol;
    let maximized = window.is_maximized();
    let button = |id: &'static str, glyph: WindowGlyph, close: bool| {
        let hover_bg = if close {
            crate::theme::color::hex(0xe81123ff)
        } else {
            symbol.opacity(0.1)
        };
        let active_bg = if close {
            crate::theme::color::hex(0xf1707aff)
        } else {
            symbol.opacity(0.2)
        };
        div()
            .id(id)
            .group(id)
            .w(layout::WINDOW_CONTROL_WIDTH)
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .hover(move |s| s.bg(hover_bg))
            .active(move |s| s.bg(active_bg))
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .child(
                gpui::svg()
                    .path(glyph.path())
                    .size(px(10.))
                    .text_color(symbol)
                    .when(close, |s| {
                        s.group_hover(id, |s| s.text_color(gpui::white()))
                    }),
            )
    };
    h_flex()
        .h_full()
        .child(
            button("win-min", WindowGlyph::Minimize, false)
                .on_click(|_, window, _| window.minimize_window()),
        )
        .child(
            button(
                "win-max",
                if maximized {
                    WindowGlyph::Restore
                } else {
                    WindowGlyph::Maximize
                },
                false,
            )
            .on_click(|_, window, _| window.zoom_window()),
        )
        .child(
            button("win-close", WindowGlyph::Close, true)
                .on_click(|_, window, _| window.remove_window()),
        )
        .into_any_element()
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.tern_theme().clone();
        let t = theme.colors;
        let app = self.app.read(cx);
        let route = app.route().clone();
        let chat_route = app.chat_route().clone();
        let sidebar_open = app.sidebar.open;
        let sidebar_width = self.restore_preview.unwrap_or(app.sidebar.width);
        let show_sidebar = (sidebar_open && !self.collapsing) || self.restore_preview.is_some();
        let right_open = app.right_panel.open;
        let maximized = app.right_panel_maximized;
        let palette_open = app.palette_open;
        let viewport = window.viewport_size();
        let overlay_panel = viewport.width < crate::app_state::right_panel::OVERLAY_BELOW;
        let this = cx.entity().downgrade();

        let main: AnyElement = match chat_route {
            Route::Gallery => self.gallery.clone().into_any_element(),
            _ => {
                let chat_column = self.render_chat_column(window, cx);
                h_flex()
                    .size_full()
                    .items_start()
                    .relative()
                    .child(
                        gpui::canvas(
                            move |bounds, _, cx| {
                                if let Some(this) = this.upgrade() {
                                    this.update(cx, |this, _| this.row_width = bounds.size.width);
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .when(!(right_open && maximized), |el| el.child(chat_column))
                    .when(right_open, |el| {
                        el.child(self.render_right_panel(overlay_panel, cx))
                    })
                    .into_any_element()
            }
        };

        let titlebar_right = self.render_titlebar_right(window, cx);
        let app_entity = self.app.clone();
        let mut root = div()
            .id("shell")
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_terminal))
            .on_action(cx.listener(Self::toggle_right_panel))
            .on_action(cx.listener(Self::toggle_details))
            .on_action(cx.listener(Self::toggle_palette))
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::dismiss))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, window, cx| {
                    if this.drag.is_some() {
                        this.end_drag(window, cx);
                    }
                }),
            )
            .when(self.drag.is_some(), |el| {
                el.cursor(match self.drag {
                    Some(Drag::Drawer { .. }) => CursorStyle::ResizeUpDown,
                    _ => CursorStyle::ResizeLeftRight,
                })
            })
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(t.app_chrome_background)
            .text_color(t.foreground)
            .font_family(font::SANS)
            .text_scale(crate::theme::TextSize::Base)
            .child(Grain::new())
            .child(titlebar_drag_strip(cx))
            .child(
                h_flex()
                    .relative()
                    .size_full()
                    .items_start()
                    .when(show_sidebar, |el| {
                        el.child(self.render_sidebar(sidebar_width, cx))
                    })
                    .child(div().relative().flex_1().min_w_0().h_full().child(main)),
            )
            .when(
                !sidebar_open && self.drag.is_none()
                    || matches!(self.drag, Some(Drag::Restore { .. })),
                |el| el.child(self.render_restore_handle(window, cx)),
            )
            // SidebarControl: fixed at x = 13, centered in the 40px strip.
            .child(
                div()
                    .absolute()
                    .top((layout::TOPBAR_HEIGHT - layout::TITLEBAR_CONTROL_SIZE) / 2.)
                    .left(layout::CONTROLS_LEFT + px(1.))
                    .child(
                        Button::new("sidebar-toggle")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::IconSm)
                            .icon(if sidebar_open {
                                IconName::PanelLeftClose
                            } else {
                                IconName::PanelLeft
                            })
                            .pressed(false)
                            .tooltip(
                                Tooltip::text("Toggle main sidebar")
                                    .kbd("Ctrl+B")
                                    .side(TooltipSide::Bottom),
                            )
                            .on_click(move |_, _, cx| {
                                app_entity.update(cx, |s, cx| s.toggle_sidebar(cx))
                            }),
                    ),
            )
            .when(!route.is_settings() && chat_route != Route::Gallery, |el| {
                el.child(titlebar_right)
            });

        if route.is_settings() {
            root = root.child(self.render_settings_modal(window, cx));
        }
        if palette_open {
            root = root.child(self.render_palette(window, cx));
        }
        root.child(self.dialogs.clone())
            .child(self.toasts.clone())
            .child(self.popups.clone())
            .child(self.tooltips.clone())
    }
}

/// The 40px strip along the top that moves the window (and maximizes on
/// double-click). It sits under the content, so buttons and inputs in the
/// titlebar keep their clicks: a control that takes the press calls
/// `window.prevent_default()` and the move does not start.
fn titlebar_drag_strip(cx: &mut Context<Shell>) -> AnyElement {
    let _ = cx;
    div()
        .id("titlebar-drag")
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(layout::TOPBAR_HEIGHT)
        .on_mouse_down(MouseButton::Left, |e: &MouseDownEvent, window, _| {
            if e.click_count == 2 {
                window.zoom_window();
            } else if !window.default_prevented() {
                window.start_window_move();
            }
        })
        .on_mouse_down(MouseButton::Right, |e: &MouseDownEvent, window, _| {
            window.show_window_menu(e.position)
        })
        .into_any_element()
}

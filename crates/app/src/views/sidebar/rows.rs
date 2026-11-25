//! Sidebar rendering (spec 02 §2.3-2.16): the header row, card and slim rows, shelves,
//! drafts, search results, empty states and the footer.

use std::sync::OnceLock;

use gpui::{
    AnyElement, App, AppContext as _, Context, Focusable as _, Hsla, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, img, prelude::FluentBuilder as _,
    px,
};
use gpui_base::Align;
use gpui_component::input::Input;
use tern_core::{Project, ProjectId, Snapshot, ThreadShell, Ts};

use super::drafts::DraftPreview;
use super::model::{self, Row, RowStatus, Section};
use super::{RowKey, SidebarView, capture_search_keys, sidebar_icon_button, sidebar_tip};
use crate::app_state::{Route, SettingsPage};
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, font, motion, radius};
use crate::tokens::Palette;
use crate::ui::scroll::FadeEdge;
use crate::ui::{
    Icon, IconName, Tooltip, TooltipExt as _, TooltipSide, h_flex, scroll_fade, v_flex,
};
use crate::views::command_palette::{self, PaletteIntent};

/// The machine name shown on the meta line (the refs show the local host).
fn hostname() -> &'static str {
    static HOST: OnceLock<String> = OnceLock::new();
    HOST.get_or_init(|| {
        std::fs::read_to_string("/proc/sys/kernel/hostname")
            .or_else(|_| std::fs::read_to_string("/etc/hostname"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "Local".into())
    })
}

fn hex(rgba: u32) -> Hsla {
    crate::tokens::c(rgba)
}

// ---------------------------------------------------------------------------
// Icons

/// Project favicon, else the colored monogram ("DN"): 16px, radius 25%, color/14% fill,
/// mono bold text. `faded`: 40% opacity and gray (settled rows at rest).
pub(crate) fn project_icon(
    project: Option<&Project>,
    size: f32,
    faded: bool,
    cx: &App,
) -> AnyElement {
    let Some(project) = project else {
        return Icon::new(IconName::Folder)
            .size(px(size))
            .color(cx.tokens().sidebar_icon_color)
            .into_any_element();
    };
    if let Some(path) = project.favicon_path.as_deref().filter(|p| !p.is_empty()) {
        return img(std::path::PathBuf::from(path))
            .size(px(size))
            .flex_none()
            .rounded(px(size / 4.))
            .when(faded, |el| el.opacity(0.4))
            .into_any_element();
    }
    let dark = cx.tern_theme().is_dark();
    let (_, light, dark_shade) = model::PROJECT_COLORS[model::project_color_index(&project.title)];
    let mut color = hex(if dark { dark_shade } else { light });
    if faded {
        color = color.grayscale();
    }
    let monogram = model::project_monogram(&project.title);
    let text_size = size * 8.25 / 16.;
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(size / 4.))
        .bg(color.opacity(0.14))
        .text_color(color)
        .font_family(font::MONO)
        .font_weight(gpui::FontWeight::BOLD)
        .text_size(px(text_size))
        .line_height(px(size))
        .when(faded, |el| el.opacity(0.4))
        .child(monogram)
        .into_any_element()
}

/// The provider mark for a model instance: Claude in its orange, OpenAI in the text color.
pub(crate) fn provider_icon(instance_id: &str, size: f32, cx: &App) -> AnyElement {
    let t = cx.tokens();
    let (icon, color) = if instance_id.starts_with("claude") {
        (IconName::ProviderClaude, hex(0xd97757ff))
    } else {
        (IconName::ProviderOpenai, t.foreground)
    };
    Icon::new(icon)
        .size(px(size))
        .color(color)
        .opacity(0.6)
        .into_any_element()
}

fn indigo(cx: &App) -> Hsla {
    hex(if cx.tern_theme().is_dark() {
        0xa5b4fcff
    } else {
        0x4f46e5ff
    })
}

/// (icon, tone) for a status (§2.7).
fn status_look(status: RowStatus, t: &Palette, cx: &App) -> Option<(IconName, Hsla)> {
    Some(match status {
        RowStatus::Approval => (IconName::ShieldQuestion, t.warning_foreground),
        RowStatus::Input => (IconName::MessageCircleQuestion, indigo(cx)),
        RowStatus::Waiting { .. } => (IconName::CircleDotDashed, t.muted_foreground),
        RowStatus::Preparing { .. } | RowStatus::Working { .. } => {
            (IconName::CircleDashed, t.info_foreground)
        }
        RowStatus::Limited => (IconName::CircleAlert, t.warning_foreground),
        RowStatus::Failed => (IconName::CircleAlert, t.error_foreground),
        RowStatus::Done => (IconName::CircleCheck, t.success_foreground),
        RowStatus::Ready => return None,
    })
}

/// The status slot's resting content: the status pill, or the relative time.
fn status_label(status: RowStatus, time_at: Ts, now: Ts, cx: &App) -> AnyElement {
    let t = *cx.tokens();
    match (status.label(), status_look(status, &t, cx)) {
        (Some(label), Some((icon, tone))) => {
            let spinning = matches!(
                status,
                RowStatus::Preparing { .. } | RowStatus::Working { .. }
            );
            let mut icon = Icon::new(icon).size(px(16.)).color(tone);
            if spinning {
                icon = icon.spin(motion::SPIN_PERIOD * 3);
            }
            h_flex()
                .gap(px(4.))
                .font_medium()
                .text_color(tone)
                .child(icon)
                .child(label)
                .when_some(status.since(), |el, since| {
                    el.child(model::working_duration(since, now))
                })
                .into_any_element()
        }
        _ => div()
            .text_color(t.secondary_label)
            .child(model::compact_relative(time_at, now))
            .into_any_element(),
    }
}

// ---------------------------------------------------------------------------
// Header row

pub(crate) fn thread_header(
    view: &mut SidebarView,
    has_projects: bool,
    window: &mut Window,
    cx: &mut Context<SidebarView>,
) -> AnyElement {
    let t = *cx.tokens();
    let focused = view.search.read(cx).focus_handle(cx).is_focused(window);
    let searching = !view.query.is_empty();
    let search = div()
        .id("sidebar-search")
        .group("sidebar-search")
        .flex()
        .flex_row()
        .items_center()
        .flex_1()
        .min_w_0()
        .h(px(32.))
        .px(px(8.))
        .gap(px(8.))
        .rounded(radius::MD)
        .text_scale(TextSize::Sm)
        .font_medium()
        .text_color(t.sidebar_muted_foreground)
        .hover(|s| s.bg(t.sidebar_row_hover).text_color(t.sidebar_foreground))
        .when(focused, |el| {
            el.bg(t.sidebar_row_hover)
                .text_color(t.sidebar_foreground)
                .shadow(vec![gpui::BoxShadow {
                    color: t.ring.opacity(0.7),
                    offset: gpui::point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: true,
                }])
        })
        .child(
            Icon::new(IconName::Search)
                .size(px(16.))
                .color(t.sidebar_icon_color),
        )
        .child(
            capture_search_keys(div().flex_1().min_w_0(), cx).child(
                Input::new(&view.search)
                    .appearance(false)
                    .h(px(20.))
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .font_weight(font::MEDIUM),
            ),
        )
        .when(searching, |el| {
            el.child(
                div()
                    .id("sidebar-search-clear")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(20.))
                    .rounded(radius::SM)
                    .cursor_pointer()
                    .text_color(t.muted_foreground)
                    .hover(|s| s.bg(t.accent).text_color(t.foreground))
                    .child(Icon::new(IconName::X).size(px(12.)))
                    .on_click(cx.listener(|this, _, window, cx| this.clear_search(window, cx))),
            )
        });

    let scope_button = has_projects.then(|| scope_trigger(view, cx));
    let new_tip = {
        let shortcut = crate::keymap::shortcut_label("chat.new", cx);
        let local = crate::keymap::shortcut_label("chat.newLocal", cx);
        let projects = model::ordered_projects(view.snapshot(cx)).len();
        let mut tip = sidebar_tip("New thread");
        if let Some(s) = shortcut {
            tip = tip.kbd(s);
        }
        if projects > 1 && view.scope.len() != 1 {
            tip = tip.secondary(match local {
                Some(l) => format!("New thread in current project: Shift+click ({l})"),
                None => "New thread in current project: Shift+click".into(),
            });
        }
        tip.side(TooltipSide::Bottom)
    };
    let new_button =
        sidebar_icon_button("sidebar-new-thread", IconName::SquarePen, 28., new_tip, cx)
            .when(!has_projects, |el| el.opacity(0.64))
            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                if has_projects {
                    this.new_thread(e.modifiers().shift, cx);
                }
            }));

    h_flex()
        .flex_none()
        .p(px(8.))
        .gap(px(4.))
        .child(search)
        .child(h_flex().children(scope_button).child(new_button))
        .into_any_element()
}

fn scope_trigger(view: &SidebarView, cx: &mut Context<SidebarView>) -> AnyElement {
    let t = *cx.tokens();
    let snap = view.snapshot(cx);
    let names: Vec<String> = view
        .scope
        .iter()
        .filter_map(|id| snap.project(id).map(|p| p.title.clone()))
        .collect();
    let tip = if names.is_empty() {
        "Projects".to_string()
    } else {
        format!("Projects: {}", names.join(", "))
    };
    let icons: AnyElement = match view.scope.as_slice() {
        [] => Icon::new(IconName::Folder)
            .size(px(16.))
            .color(t.sidebar_icon_color)
            .into_any_element(),
        [one] => project_icon(snap.project(one), 16., false, cx),
        [a, b, ..] => h_flex()
            .child(project_icon(snap.project(a), 14., false, cx))
            .child(
                div()
                    .ml(px(-4.))
                    .rounded(px(4.))
                    .border_2()
                    .border_color(t.sidebar)
                    .child(project_icon(snap.project(b), 14., false, cx)),
            )
            .into_any_element(),
    };
    div()
        .id("sidebar-scope")
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded(radius::LG)
        .cursor_pointer()
        .hover(|s| s.bg(t.sidebar_row_hover))
        .child(icons)
        .t3_tooltip(sidebar_tip(tip).side(TooltipSide::Bottom))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, e: &MouseDownEvent, window, cx| {
                window.prevent_default();
                this.open_scope_menu(e.position, window, cx);
                cx.stop_propagation();
            }),
        )
        .into_any_element()
}

impl SidebarView {
    /// The project scope popup (§2.3): All projects, each project (click selects only it,
    /// Shift toggles it into a multi-selection), Add project.
    fn open_scope_menu(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::ui::{Menu, MenuItem};
        let me = cx.entity().downgrade();
        let projects: Vec<(ProjectId, String)> = model::ordered_projects(self.snapshot(cx))
            .into_iter()
            .map(|p| (p.id.clone(), p.title.clone()))
            .collect();
        let mut menu = Menu::new().min_width(px(220.)).item({
            let me = me.clone();
            MenuItem::new("All projects")
                .icon(IconName::Folder)
                .on_select(move |_, cx| {
                    let _ = me.update(cx, |this, cx| {
                        this.scope.clear();
                        cx.notify();
                    });
                })
        });
        for (id, title) in projects {
            let me = me.clone();
            let selected = self.scope.contains(&id);
            menu = menu.item(
                MenuItem::checkbox(title, selected).on_select(move |window, cx| {
                    let shift = window.modifiers().shift;
                    let id = id.clone();
                    let _ = me.update(cx, |this, cx| {
                        if shift {
                            if let Some(i) = this.scope.iter().position(|p| *p == id) {
                                this.scope.remove(i);
                            } else {
                                this.scope.push(id);
                            }
                        } else {
                            this.scope = vec![id];
                        }
                        cx.notify();
                    });
                }),
            );
        }
        menu = menu.separator().item(
            MenuItem::new("Add project")
                .icon(IconName::FolderPlus)
                .on_select(|_, cx| command_palette::open(PaletteIntent::AddProject, cx)),
        );
        crate::ui::menu::open_menu(
            "sidebar-scope-menu".into(),
            crate::ui::popup::PopupAnchor::Point {
                position: at,
                trigger: None,
            },
            menu,
            window,
            cx,
        );
    }
}

// ---------------------------------------------------------------------------
// The list

pub(crate) fn list(
    view: &mut SidebarView,
    _window: &mut Window,
    cx: &mut Context<SidebarView>,
) -> AnyElement {
    let sections = view.sections(cx);
    let snap = view.snapshot(cx).clone();
    let open = view.open_thread_id(cx);
    let drafts: Vec<(ProjectId, DraftPreview)> = view
        .drafts
        .read(cx)
        .project_drafts()
        .into_iter()
        .filter(|(p, _)| view.scope.is_empty() || view.scope.contains(p))
        .collect();

    let mut items: Vec<AnyElement> = Vec::new();
    for (project_id, preview) in &drafts {
        items.push(draft_row(view, project_id, preview, &snap, cx));
    }
    if !drafts.is_empty() {
        let t = cx.tokens();
        items.push(
            div()
                .mx(px(10.))
                .my(px(6.))
                .h(px(1.))
                .bg(t.sidebar_border.opacity(0.6))
                .into_any_element(),
        );
    }
    for row in sections.pinned.iter().chain(&sections.active) {
        if let Some(shell) = snap.thread(&row.thread_id) {
            items.push(card_row(view, row, shell, &snap, cx));
        }
    }
    let active_empty = sections.pinned.is_empty() && sections.active.is_empty();
    if active_empty && drafts.is_empty() {
        items.push(in_list_empty(view, &sections, &snap, cx));
    }
    // The shelves sit at the bottom when the list is short.
    items.push(div().flex_1().min_h(px(0.)).into_any_element());

    if !sections.working.is_empty() {
        items.push(shelf_header(
            view,
            Section::Working,
            sections.working.len(),
            view.working_expanded,
            cx,
        ));
        for row in &sections.working {
            if view.working_expanded || Some(&row.thread_id) == open.as_ref() {
                if let Some(shell) = snap.thread(&row.thread_id) {
                    items.push(card_row(view, row, shell, &snap, cx));
                }
            }
        }
    }
    items.push(shelf_header(
        view,
        Section::Settled,
        sections.settled.len(),
        view.settled_expanded,
        cx,
    ));
    for (i, row) in sections.settled.iter().enumerate() {
        let shown = view.settled_expanded && i < view.settled_shown;
        if shown || Some(&row.thread_id) == open.as_ref() {
            if let Some(shell) = snap.thread(&row.thread_id) {
                items.push(slim_row(view, row, shell, &snap, cx));
            }
        }
    }
    if view.settled_expanded && sections.settled.len() > view.settled_shown {
        let more = (sections.settled.len() - view.settled_shown).min(model::SETTLED_PAGE);
        items.push(show_more(more, cx));
    }

    div()
        .id("sidebar-list")
        .size_full()
        .overflow_y_scroll()
        .track_scroll(&view.scroll)
        .child(v_flex().min_h_full().p(px(8.)).gap(px(1.)).children(items))
        .into_any_element()
}

fn row_surface_colors(
    t: &Palette,
    active: bool,
    selected: bool,
    has_draft: bool,
    hovered: bool,
) -> Hsla {
    if active {
        t.sidebar_row_active
    } else if selected {
        t.sidebar_row_selected
    } else if has_draft {
        t.warning.opacity(if hovered { 0.08 } else { 0.04 })
    } else if hovered {
        t.sidebar_row_hover
    } else {
        gpui::transparent_black()
    }
}

/// The light theme's 1px inset ring on the open / selected row.
fn active_ring(t: &Palette, cx: &App) -> Vec<gpui::BoxShadow> {
    if cx.tern_theme().is_dark() {
        return vec![];
    }
    vec![gpui::BoxShadow {
        color: t.sidebar_border.opacity(0.7),
        offset: gpui::point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(1.),
        inset: true,
    }]
}

/// What a drag carries.
#[derive(Clone)]
pub(crate) struct DraggedThread {
    pub id: String,
    pub title: SharedString,
    pub pinned: bool,
}

struct DragChip {
    title: SharedString,
    verb: &'static str,
    icon: IconName,
}

impl Render for DragChip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.tern_theme().clone();
        let t = theme.sidebar_colors;
        h_flex()
            .w(px(220.))
            .h(px(36.))
            .px(px(10.))
            .gap(px(8.))
            .rounded(radius::MD)
            .bg(t.sidebar_row_active)
            .shadow(theme.shadows.lg(0.1))
            .text_scale(TextSize::Sm)
            .font_medium()
            .text_color(t.sidebar_foreground)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(self.title.clone()),
            )
            .child(
                // Drop verb badge: h20, px6, radius 4, primary/40 border, primary/10 fill.
                h_flex()
                    .h(px(20.))
                    .px(px(6.))
                    .gap(px(4.))
                    .rounded(radius::SM)
                    .border_1()
                    .border_color(t.primary.opacity(0.4))
                    .bg(t.primary.opacity(0.1))
                    .text_size(px(11.))
                    .text_color(t.primary)
                    .child(Icon::new(self.icon).size(px(12.)))
                    .child(self.verb),
            )
    }
}

fn row_drag(
    el: gpui::Stateful<gpui::Div>,
    shell: &ThreadShell,
    row: &Row,
    cx: &mut Context<SidebarView>,
) -> gpui::Stateful<gpui::Div> {
    let dragged = DraggedThread {
        id: shell.thread.id.clone(),
        title: shell.thread.title.clone().into(),
        pinned: row.section == Section::Pinned,
    };
    let target = RowKey::Thread(shell.thread.id.clone());
    let droppable = row.section == Section::Pinned;
    let t = *cx.tokens();
    let el = el.on_drag(dragged, |d, _, _, cx| {
        let (verb, icon) = if d.pinned {
            ("Pin", IconName::Pin)
        } else {
            ("Pin", IconName::Pin)
        };
        cx.new(|_| DragChip {
            title: d.title.clone(),
            verb,
            icon,
        })
    });
    if !droppable {
        return el;
    }
    let target_id = shell.thread.id.clone();
    el.drag_over::<DraggedThread>(move |s, _, _, _| s.border_t_2().border_color(t.primary))
        .on_drop(cx.listener(move |this, d: &DraggedThread, _, cx| {
            this.drop_on_pinned(&d.id, Some(&target_id), cx);
        }))
        .on_drag_move::<DraggedThread>(cx.listener(move |this, _, _, cx| {
            this.set_drop_target(Some(target.clone()), cx);
        }))
}

/// Card row (§2.5): Active, Pinned and Working.
fn card_row(
    view: &SidebarView,
    row: &Row,
    shell: &ThreadShell,
    snap: &Snapshot,
    cx: &mut Context<SidebarView>,
) -> AnyElement {
    let t = *cx.tokens();
    let id = shell.thread.id.clone();
    let key = RowKey::Thread(id.clone());
    let open = view.open_thread_id(cx).as_deref() == Some(id.as_str());
    let selected = view.selection.contains(&id);
    let hovered = view.hovered.as_ref() == Some(&key);
    let focused = view.focused_row.as_ref() == Some(&key) && view.focus_handle_focused(cx);
    let show_actions = hovered || focused;
    let has_draft = view.drafts.read(cx).has_thread_draft(&id);
    let project = snap.project(&shell.thread.project_id);
    let receded = model::is_receded(row.status, row.unread, open, selected);
    let pinned = shell.thread.pinned_at.is_some();
    let renaming = view
        .renaming
        .as_ref()
        .filter(|r| r.thread_id == id)
        .map(|r| r.input.clone());

    let bg = row_surface_colors(&t, open, selected, has_draft && !open, hovered);
    let text = if receded && !hovered {
        t.sidebar_muted_foreground.opacity(0.75)
    } else {
        t.sidebar_foreground
    };

    // Line 1: [draft pen][favicon][pin][project name][status slot]
    let status_slot = {
        let resting = status_label(row.status, row.time_at, view.now, cx);
        let actions = show_actions.then(|| {
            let settle_id = id.clone();
            let discard_id = id.clone();
            h_flex()
                .h(px(20.))
                .text_size(px(12.))
                .text_color(t.muted_foreground)
                .when(has_draft, |el| {
                    el.child(
                        div()
                            .id(SharedString::from(format!("discard-{discard_id}")))
                            .flex()
                            .items_center()
                            .h_full()
                            .px(px(6.))
                            .rounded(radius::MD)
                            .cursor_pointer()
                            .hover(|s| s.text_color(t.foreground))
                            .child(Icon::new(IconName::X).size(px(14.)))
                            .t3_tooltip(Tooltip::text("Discard draft"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.discard_thread_draft(&discard_id, cx);
                                cx.stop_propagation();
                            })),
                    )
                })
                .child(
                    div()
                        .id(SharedString::from(format!("settle-{settle_id}")))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .h_full()
                        .px(px(6.))
                        .mr(px(-4.))
                        .rounded(radius::MD)
                        .cursor_pointer()
                        .hover(|s| s.text_color(t.foreground))
                        .child(Icon::new(IconName::Check).size(px(14.)))
                        .child("Settle")
                        .t3_tooltip(Tooltip::text("Settle thread"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.set_settled(vec![settle_id.clone()], true, window, cx);
                            cx.stop_propagation();
                        })),
                )
        });
        h_flex()
            .flex_none()
            .ml_auto()
            .min_w(px(32.))
            .h(px(20.))
            .justify_end()
            .text_size(px(12.))
            .line_height(px(16.))
            .map(|el| match actions {
                Some(actions) => el.child(actions),
                None => el.child(resting),
            })
    };
    let line1 = h_flex()
        .h(px(20.))
        .min_w_0()
        .gap(px(6.))
        .when(has_draft, |el| {
            el.child(
                Icon::new(IconName::SquarePen)
                    .size(px(12.))
                    .color(t.warning_foreground),
            )
        })
        .child(project_icon(project, 16., false, cx))
        .when(pinned, |el| {
            el.child(
                Icon::new(IconName::Pin)
                    .size(px(12.))
                    .color(t.muted_foreground.opacity(0.65)),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(t.secondary_label)
                .child(project.map(|p| p.title.clone()).unwrap_or_default()),
        )
        .child(status_slot);

    let title: AnyElement = match renaming {
        Some(input) => rename_field(&input, cx),
        None => div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_scale(TextSize::Sm)
            .when(!receded || hovered, |el| el.font_medium())
            .text_color(text)
            .child(shell.thread.title.clone())
            .into_any_element(),
    };
    let line2 = h_flex().mt(px(4.)).min_w_0().gap(px(6.)).child(title);

    let line3 = {
        let diff = shell.diff_totals.filter(|d| d.additions + d.deletions > 0);
        let diff_colors = cx.tern_theme().diff;
        h_flex()
            .mt(px(2.))
            .h(px(16.))
            .gap(px(6.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(t.secondary_label)
            .child(
                h_flex()
                    .gap(px(6.))
                    .min_w_0()
                    .child(
                        Icon::new(IconName::Monitor)
                            .size(px(14.))
                            .color(t.icon_muted),
                    )
                    .child(hostname()),
            )
            .child(div().flex_1())
            .when_some(shell.thread.linked_pull_request.as_ref(), |el, pr| {
                let (icon, tone) = pr_look(&pr.state, cx);
                el.child(
                    h_flex()
                        .gap(px(2.))
                        .text_color(tone)
                        .child(Icon::new(icon).size(px(12.)))
                        .child(format!("{}", pr.number)),
                )
            })
            .when_some(diff, |el, d| {
                el.child(
                    h_flex()
                        .gap(px(4.))
                        .font_family(font::MONO)
                        .child(
                            div()
                                .text_color(diff_colors.diff_addition_foreground)
                                .child(format!("+{}", d.additions)),
                        )
                        .child(
                            div()
                                .text_color(diff_colors.diff_deletion_foreground)
                                .child(format!("−{}", d.deletions)),
                        ),
                )
            })
            .child(provider_icon(
                &shell.thread.model_selection.instance_id,
                12.,
                cx,
            ))
    };

    let row_id = id.clone();
    let menu_id = id.clone();
    let hover_key = key.clone();
    let tip = hover_card_tooltip(view, &id, cx);
    let forced = view.forced.hover.as_deref() == Some(id.as_str())
        || view.forced.menu.as_deref() == Some(id.as_str());
    let probe = view.forced_bounds.clone();

    let inner = div()
        .id(SharedString::from(format!("row-card-{id}")))
        .relative()
        .h(px(78.))
        .px(px(10.))
        .py(px(8.))
        .child(line1)
        .child(line2)
        .child(line3)
        .when_some(tip, |el, tip| el.t3_tooltip(tip))
        .when(forced, |el| el.child(crate::ui::bounds_probe(probe)));

    let surface = div()
        .id(SharedString::from(format!("row-{id}")))
        .relative()
        .w_full()
        .rounded(radius::MD)
        .cursor_pointer()
        .bg(bg)
        .when(open || selected, |el| el.shadow(active_ring(&t, cx)))
        .when(focused && !open, |el| {
            el.shadow(vec![gpui::BoxShadow {
                color: t.ring,
                offset: gpui::point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(2.),
                inset: true,
            }])
        })
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            this.set_hovered(hover_key.clone(), *hovered, cx)
        }))
        .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
            this.click_row(&row_id, e, window, cx)
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                this.open_context_menu(&menu_id, e.position, window, cx);
                cx.stop_propagation();
            }),
        )
        .child(inner);
    let surface = row_drag(surface, shell, row, cx);

    div().py(px(2.)).child(surface).into_any_element()
}

fn pr_look(state: &str, cx: &App) -> (IconName, Hsla) {
    let dark = cx.tern_theme().is_dark();
    let pick = |light: u32, dark_: u32| hex(if dark { dark_ } else { light });
    match state {
        "merged" => (IconName::GitMerge, pick(0x7c3aedff, 0xc4b5fdff)),
        "closed" => (IconName::GitPullRequestClosed, pick(0xdc2626ff, 0xfca5a5ff)),
        "draft" => (IconName::GitPullRequestDraft, pick(0x71717aff, 0xa1a1aaff)),
        _ => (IconName::GitPullRequestArrow, pick(0x059669ff, 0x6ee7b7ff)),
    }
}

/// The inline rename input: 14 medium, `card` fill, 1px inset `input` ring, px 4 (−4
/// margin so the text does not shift), radius 4.
fn rename_field(
    input: &gpui::Entity<gpui_component::input::InputState>,
    cx: &mut Context<SidebarView>,
) -> AnyElement {
    let t = *cx.tokens();
    div()
        .flex_1()
        .min_w_0()
        .mx(px(-4.))
        .px(px(4.))
        .rounded(radius::SM)
        .bg(t.card)
        .text_color(t.card_foreground)
        .shadow(vec![gpui::BoxShadow {
            color: t.foreground,
            offset: gpui::point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(1.),
            inset: true,
        }])
        .capture_action(
            cx.listener(|this, _: &gpui_component::input::Escape, window, cx| {
                this.cancel_rename(window, cx);
                cx.stop_propagation();
            }),
        )
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            Input::new(input)
                .appearance(false)
                .h(px(20.))
                .text_size(px(14.))
                .line_height(px(20.))
                .font_weight(font::MEDIUM),
        )
        .into_any_element()
}

/// Slim row (§2.5): Settled. h36, px10, gap10, one line.
fn slim_row(
    view: &SidebarView,
    row: &Row,
    shell: &ThreadShell,
    snap: &Snapshot,
    cx: &mut Context<SidebarView>,
) -> AnyElement {
    let t = *cx.tokens();
    let id = shell.thread.id.clone();
    let key = RowKey::Thread(id.clone());
    let open = view.open_thread_id(cx).as_deref() == Some(id.as_str());
    let selected = view.selection.contains(&id);
    let hovered = view.hovered.as_ref() == Some(&key);
    let focused = view.focused_row.as_ref() == Some(&key) && view.focus_handle_focused(cx);
    let lit = open || hovered || focused;
    let project = snap.project(&shell.thread.project_id);
    let title_color = if lit {
        t.foreground
    } else if row.unread {
        t.muted_foreground
    } else {
        t.secondary_label.opacity(0.7)
    };
    let renaming = view
        .renaming
        .as_ref()
        .filter(|r| r.thread_id == id)
        .map(|r| r.input.clone());
    let unsettle_id = id.clone();
    let time_slot = h_flex()
        .flex_none()
        .min_w(px(32.))
        .h(px(24.))
        .justify_end()
        .text_size(px(12.))
        .text_color(t.secondary_label)
        .map(|el| {
            if hovered || focused {
                el.child(
                    div()
                        .id(SharedString::from(format!("unsettle-{unsettle_id}")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(24.))
                        .rounded(radius::MD)
                        .cursor_pointer()
                        .text_color(t.muted_foreground)
                        .hover(|s| s.text_color(t.foreground))
                        .child(Icon::new(IconName::Undo2).size(px(14.)))
                        .t3_tooltip(Tooltip::text("Un-settle thread"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.set_settled(vec![unsettle_id.clone()], false, window, cx);
                            cx.stop_propagation();
                        })),
                )
            } else {
                el.child(model::compact_relative(row.time_at, view.now))
            }
        });
    let title: AnyElement = match renaming {
        Some(input) => rename_field(&input, cx),
        None => div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_color(title_color)
            .child(shell.thread.title.clone())
            .into_any_element(),
    };
    let row_id = id.clone();
    let menu_id = id.clone();
    let hover_key = key.clone();
    let tip = hover_card_tooltip(view, &id, cx);
    let surface = div()
        .id(SharedString::from(format!("row-{id}")))
        .flex()
        .flex_row()
        .items_center()
        .h(px(36.))
        .px(px(10.))
        .gap(px(10.))
        .rounded(radius::MD)
        .cursor_pointer()
        .text_scale(TextSize::Sm)
        .bg(row_surface_colors(&t, open, selected, false, hovered))
        .when(open || selected, |el| el.shadow(active_ring(&t, cx)))
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            this.set_hovered(hover_key.clone(), *hovered, cx)
        }))
        .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
            this.click_row(&row_id, e, window, cx)
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                this.open_context_menu(&menu_id, e.position, window, cx);
                cx.stop_propagation();
            }),
        )
        .child(project_icon(project, 16., !lit, cx))
        .when(view.drafts.read(cx).has_thread_draft(&id), |el| {
            el.child(
                Icon::new(IconName::SquarePen)
                    .size(px(12.))
                    .color(t.warning_foreground),
            )
        })
        .child(
            div()
                .id(SharedString::from(format!("row-slim-title-{id}")))
                .flex()
                .flex_1()
                .min_w_0()
                .child(title)
                .when_some(tip, |el, tip| el.t3_tooltip(tip)),
        )
        .when(shell.thread.pinned_at.is_some(), |el| {
            el.child(
                Icon::new(IconName::Pin)
                    .size(px(12.))
                    .color(t.muted_foreground.opacity(0.65)),
            )
        })
        .child(time_slot);
    let surface = row_drag(surface, shell, row, cx);
    surface.into_any_element()
}

/// Shelf header (§2.8): h32, mx2, px8, radius 6, gap 8, text 12 medium.
fn shelf_header(
    view: &SidebarView,
    section: Section,
    count: usize,
    expanded: bool,
    cx: &mut Context<SidebarView>,
) -> AnyElement {
    let t = *cx.tokens();
    let label = match section {
        Section::Working => "Working",
        _ => "Settled",
    };
    let key = RowKey::Shelf(section);
    let focused = view.focused_row.as_ref() == Some(&key) && view.focus_handle_focused(cx);
    let target = view.drop_target.as_ref() == Some(&key);
    let tone = if target {
        t.primary
    } else {
        t.sidebar_muted_foreground
    };
    let mut el = div()
        .id(SharedString::from(format!("shelf-{label}")))
        .flex()
        .flex_row()
        .items_center()
        .flex_none()
        .mx(px(2.))
        .h(px(32.))
        .px(px(8.))
        .gap(px(8.))
        .rounded(radius::MD)
        .cursor_pointer()
        .text_size(px(12.))
        .line_height(px(16.))
        .font_medium()
        .text_color(tone)
        .hover(|s| s.bg(t.sidebar_row_hover))
        .when(focused, |el| {
            el.shadow(vec![gpui::BoxShadow {
                color: t.ring,
                offset: gpui::point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(2.),
                inset: true,
            }])
        })
        .child(label)
        .child(div().flex_1())
        .child(div().font_weight(font::NORMAL).child(count.to_string()))
        .child(
            Icon::new(if expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size(px(12.))
            .color(tone),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.focused_row = Some(RowKey::Shelf(section));
            this.toggle_shelf(section, cx);
        }));
    if section == Section::Settled {
        el = el
            .drag_over::<DraggedThread>(move |s, _, _, _| s.text_color(t.primary))
            .on_drop(cx.listener(|this, d: &DraggedThread, window, cx| {
                this.drop_on_settled(&d.id, window, cx);
            }));
    }
    el.into_any_element()
}

fn show_more(n: usize, cx: &mut Context<SidebarView>) -> AnyElement {
    let t = *cx.tokens();
    div()
        .id("settled-more")
        .flex()
        .flex_row()
        .items_center()
        .h(px(36.))
        .px(px(10.))
        .gap(px(10.))
        .rounded(radius::MD)
        .cursor_pointer()
        .text_scale(TextSize::Sm)
        .text_color(t.sidebar_muted_foreground)
        .hover(|s| s.bg(t.sidebar_row_hover))
        .child(Icon::new(IconName::ChevronDown).size(px(16.)))
        .child(format!("Show {n} more"))
        .on_click(cx.listener(|this, _, _, cx| {
            this.settled_shown += model::SETTLED_PAGE;
            cx.notify();
        }))
        .into_any_element()
}

/// Draft row (§2.9): same 78px card box, warning-tinted.
fn draft_row(
    view: &SidebarView,
    project_id: &str,
    preview: &DraftPreview,
    snap: &Snapshot,
    cx: &mut Context<SidebarView>,
) -> AnyElement {
    let t = *cx.tokens();
    let key = RowKey::Draft(project_id.to_string());
    let open = view.open_draft_project(cx).as_deref() == Some(project_id);
    let hovered = view.hovered.as_ref() == Some(&key);
    let project = snap.project(project_id);
    let bg = if open {
        t.sidebar_row_active
    } else {
        t.warning.opacity(if hovered { 0.08 } else { 0.04 })
    };
    let open_id = project_id.to_string();
    let discard_id = project_id.to_string();
    let hover_key = key.clone();
    div()
        .py(px(2.))
        .child(
            div()
                .id(SharedString::from(format!("draft-{project_id}")))
                .rounded(radius::MD)
                .cursor_pointer()
                .bg(bg)
                .when(open, |el| el.shadow(active_ring(&t, cx)))
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    this.set_hovered(hover_key.clone(), *hovered, cx)
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.focus.focus(window, cx);
                    this.navigate(Route::Draft(open_id.clone()), cx);
                }))
                .child(
                    v_flex()
                        .h(px(78.))
                        .px(px(10.))
                        .py(px(8.))
                        .child(
                            h_flex()
                                .h(px(20.))
                                .gap(px(6.))
                                .child(
                                    Icon::new(IconName::SquarePen)
                                        .size(px(12.))
                                        .color(t.warning_foreground),
                                )
                                .child(project_icon(project, 16., false, cx))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .text_size(px(12.))
                                        .font_medium()
                                        .text_color(t.secondary_label)
                                        .child(
                                            project.map(|p| p.title.clone()).unwrap_or_default(),
                                        ),
                                )
                                .when(hovered, |el| {
                                    el.child(
                                        div()
                                            .id(SharedString::from(format!("draft-x-{discard_id}")))
                                            .flex()
                                            .items_center()
                                            .px(px(6.))
                                            .h_full()
                                            .cursor_pointer()
                                            .text_color(t.muted_foreground)
                                            .hover(|s| s.text_color(t.foreground))
                                            .child(Icon::new(IconName::X).size(px(12.)))
                                            .t3_tooltip(Tooltip::text("Discard draft"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.discard_draft(&discard_id, cx);
                                                cx.stop_propagation();
                                            })),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .mt(px(2.))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_scale(TextSize::Sm)
                                .font_medium()
                                .text_color(t.foreground.opacity(0.9))
                                .child(preview.summary()),
                        ),
                ),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Hover card

/// `ThreadHoverCard` (§2.5): right side, start aligned, offset 4.
pub(crate) fn hover_card_tooltip(view: &SidebarView, id: &str, cx: &App) -> Option<Tooltip> {
    let snap = view.snapshot(cx);
    let shell = snap.thread(id)?.clone();
    let project = snap.project(&shell.thread.project_id).cloned();
    let model_label = snap
        .providers
        .iter()
        .find(|p| p.instance_id == shell.thread.model_selection.instance_id)
        .and_then(|p| {
            p.models
                .iter()
                .find(|m| m.slug == shell.thread.model_selection.model)
                .map(|m| m.name.clone())
        })
        .unwrap_or_else(|| shell.thread.model_selection.model.clone());
    Some(
        Tooltip::custom(move |_, cx| {
            let t = *cx.tokens();
            let line = |icon: AnyElement, text: SharedString| {
                h_flex()
                    .h(px(22.))
                    .gap(px(8.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(12.))
                            .child(icon),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(text),
                    )
            };
            let muted = |name: IconName| {
                Icon::new(name)
                    .size(px(12.))
                    .color(t.muted_foreground)
                    .into_any_element()
            };
            v_flex()
                .w(px(296.))
                .px(px(4.))
                .py(px(6.))
                .child(
                    div()
                        .pb(px(4.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .font_medium()
                        .child(shell.thread.title.clone()),
                )
                .when_some(project.clone(), |el, p| {
                    el.child(line(
                        project_icon(Some(&p), 12., false, cx),
                        p.title.clone().into(),
                    ))
                })
                .child(line(muted(IconName::Monitor), hostname().into()))
                .when_some(shell.thread.branch.clone(), |el, b| {
                    el.child(line(
                        muted(IconName::GitBranch),
                        middle_truncate(&b, 40).into(),
                    ))
                })
                .when_some(shell.thread.worktree_path.clone(), |el, w| {
                    el.child(line(
                        muted(IconName::FolderGit2),
                        middle_truncate(&w, 40).into(),
                    ))
                })
                .child(line(
                    provider_icon(&shell.thread.model_selection.instance_id, 12., cx),
                    model_label.clone().into(),
                ))
                .when_some(shell.thread.linked_pull_request.clone(), |el, pr| {
                    let (icon, tone) = pr_look(&pr.state, cx);
                    el.child(line(
                        Icon::new(icon).size(px(12.)).color(tone).into_any_element(),
                        format!("#{} {}", pr.number, pr.title).into(),
                    ))
                })
                .into_any_element()
        })
        .side(TooltipSide::Right)
        .align(Align::Start)
        .delay(motion::TOOLTIP_SIDEBAR_DELAY),
    )
}

fn middle_truncate(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(1) / 2;
    let head: String = chars[..keep].iter().collect();
    let tail: String = chars[chars.len() - keep..].iter().collect();
    format!("{head}…{tail}")
}

// ---------------------------------------------------------------------------
// Search, empty states, footer

pub(crate) fn search_results(view: &mut SidebarView, cx: &mut Context<SidebarView>) -> AnyElement {
    let t = *cx.tokens();
    let results = view.search_results(cx);
    let snap = view.snapshot(cx).clone();
    let open = view.open_thread_id(cx);
    if results.is_empty() {
        return div()
            .flex()
            .justify_center()
            .py(px(24.))
            .text_size(px(12.))
            .text_color(t.sidebar_muted_foreground)
            .child("No threads found")
            .into_any_element();
    }
    let rows = results.iter().enumerate().filter_map(|(i, id)| {
        let shell = snap.thread(id)?;
        let settled = shell.thread.settled_at.is_some();
        let highlighted = i == view.search_highlight || open.as_deref() == Some(id.as_str());
        let open_id = id.clone();
        Some(
            div()
                .id(SharedString::from(format!("search-{id}")))
                .flex()
                .flex_row()
                .items_center()
                .min_h(px(36.))
                .px(px(10.))
                .py(px(4.))
                .gap(px(10.))
                .rounded(radius::MD)
                .cursor_pointer()
                .text_scale(TextSize::Sm)
                .when(highlighted, |el| el.bg(t.sidebar_row_active))
                .hover(|s| s.bg(t.sidebar_row_hover))
                .child(project_icon(
                    snap.project(&shell.thread.project_id),
                    16.,
                    settled,
                    cx,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .when(!settled, |el| el.font_medium())
                        .text_color(t.sidebar_foreground)
                        .child(shell.thread.title.clone()),
                )
                .child(
                    div()
                        .min_w(px(32.))
                        .flex()
                        .justify_end()
                        .text_size(px(12.))
                        .text_color(t.muted_foreground.opacity(0.55))
                        .child(model::compact_relative(model::activity_at(shell), view.now)),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.open_thread(&open_id, cx)))
                .into_any_element(),
        )
    });
    div()
        .id("sidebar-search-results")
        .size_full()
        .overflow_y_scroll()
        .child(v_flex().p(px(8.)).gap(px(1.)).children(rows))
        .into_any_element()
}

/// The small outlined in-list button ("Add project", "Show all projects").
fn empty_button(
    id: &'static str,
    icon: Option<IconName>,
    label: &'static str,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let t = cx.tokens();
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .rounded(radius::MD)
        .border_1()
        .border_color(t.sidebar_border)
        .px(px(10.))
        .py(px(4.))
        .cursor_pointer()
        .text_size(px(11.))
        .line_height(px(14.))
        .font_medium()
        .text_color(t.sidebar_muted_foreground)
        .hover(|s| s.bg(t.sidebar_row_hover).text_color(t.sidebar_foreground))
        .when_some(icon, |el, icon| el.child(Icon::new(icon).size(px(12.))))
        .child(label)
}

fn empty_column(text: String, button: Option<AnyElement>, cx: &App) -> AnyElement {
    let t = cx.tokens();
    v_flex()
        .items_center()
        .gap(px(8.))
        .px(px(8.))
        .py(px(24.))
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(t.muted_foreground.opacity(0.6))
        .child(text)
        .children(button)
        .into_any_element()
}

/// No projects at all (§2.16).
pub(crate) fn empty_state(_view: &mut SidebarView, cx: &mut Context<SidebarView>) -> AnyElement {
    let button = empty_button(
        "sidebar-add-project",
        Some(IconName::Plus),
        "Add project",
        cx,
    )
    .on_click(|_, _, cx| command_palette::open(PaletteIntent::AddProject, cx))
    .into_any_element();
    div()
        .p(px(8.))
        .child(empty_column("No projects yet".into(), Some(button), cx))
        .into_any_element()
}

fn in_list_empty(
    view: &SidebarView,
    sections: &model::Sections,
    snap: &Snapshot,
    cx: &mut Context<SidebarView>,
) -> AnyElement {
    if view.scope.is_empty() {
        return empty_column("No threads yet".into(), None, cx);
    }
    let any_settled = !sections.settled.is_empty() || !sections.working.is_empty();
    let text = match view.scope.as_slice() {
        [one] => {
            let name = snap
                .project(one)
                .map(|p| p.title.clone())
                .unwrap_or_default();
            if any_settled {
                format!("No active threads in {name}")
            } else {
                format!("No threads in {name} yet")
            }
        }
        _ if any_settled => "No active threads in these projects".into(),
        _ => "No threads in these projects yet".into(),
    };
    let button = empty_button("sidebar-show-all", None, "Show all projects", cx)
        .on_click(cx.listener(|this, _, _, cx| {
            this.scope.clear();
            cx.notify();
        }))
        .into_any_element();
    empty_column(text, Some(button), cx)
}

pub(crate) fn scroll_fades(view: &SidebarView, cx: &App) -> AnyElement {
    let t = cx.tokens();
    let offset = view.scroll.offset().y;
    let max = view.scroll.max_offset().y;
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .when(offset < px(-1.), |el| {
            el.child(scroll_fade(FadeEdge::Top, t.sidebar).size(12.))
        })
        .when(max > px(0.) && -offset < max - px(1.), |el| {
            el.child(scroll_fade(FadeEdge::Bottom, t.sidebar).size(12.))
        })
        .into_any_element()
}

/// Footer (§2.15): settings and usage, then the undo notice.
pub(crate) fn footer(view: &mut SidebarView, cx: &mut Context<SidebarView>) -> AnyElement {
    let t = *cx.tokens();
    let undo = view
        .undo
        .as_ref()
        .filter(|u| u.expires > std::time::Instant::now())
        .map(|u| u.label.clone());
    let undo_key =
        crate::keymap::shortcut_label("thread.undo", cx).unwrap_or_else(|| "Ctrl+Z".into());
    let settings = sidebar_icon_button(
        "sidebar-settings",
        IconName::Settings,
        32.,
        sidebar_tip("Settings").side(TooltipSide::Top),
        cx,
    )
    .on_click(cx.listener(|this, _, _, cx| {
        this.app
            .update(cx, |s, cx| s.open_settings(SettingsPage::General, cx))
    }));
    let usage = crate::keymap::has_handler("usage.open", cx).then(|| {
        let mut tip = sidebar_tip("Usage").side(TooltipSide::Top);
        if let Some(k) = crate::keymap::shortcut_label("usage.open", cx) {
            tip = tip.kbd(k);
        }
        sidebar_icon_button("sidebar-usage", IconName::ChartNoAxesColumn, 32., tip, cx).on_click(
            |_, window, cx| {
                crate::keymap::run("usage.open", window, cx);
            },
        )
    });
    h_flex()
        .flex_none()
        .px(px(8.))
        .pt(px(4.))
        .pb(px(8.))
        .gap(px(8.))
        .child(h_flex().gap(px(4.)).child(settings).children(usage))
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .justify_end()
                .h(px(24.))
                .text_size(px(11.))
                .line_height(px(24.))
                .text_color(t.sidebar_muted_foreground)
                .when_some(undo, |el, label| {
                    el.child(format!("{label},")).child(
                        div()
                            .id("sidebar-undo")
                            .ml(px(4.))
                            .cursor_pointer()
                            .hover(|s| s.text_color(t.sidebar_foreground))
                            .child(format!("{undo_key} to undo"))
                            .on_click(cx.listener(|this, _, _, cx| this.undo_now(cx))),
                    )
                }),
        )
        .into_any_element()
}

impl SidebarView {
    pub(crate) fn focus_handle_focused(&self, _cx: &App) -> bool {
        self.list_focused
    }

    fn undo_now(&mut self, cx: &mut Context<Self>) {
        self.undo(cx);
    }
}

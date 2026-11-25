//! The settings left nav (spec 06 §1.3): search box, grouped page links, search
//! results.

use gpui::{
    AnyElement, Context, Focusable as _, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::Input;

use super::SettingsView;
use super::search;
use crate::app_state::SettingsPage;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, layout, radius};
use crate::ui::{Button, ButtonSize, ButtonVariant, Icon, IconName, Kbd, h_flex, v_flex};

/// (hidden heading?, heading, pages)
const GROUPS: &[(Option<&str>, &[SettingsPage])] = &[
    (
        None,
        &[
            SettingsPage::General,
            SettingsPage::Appearance,
            SettingsPage::Chat,
            SettingsPage::Diffs,
            SettingsPage::Keybindings,
        ],
    ),
    (
        Some("Agents"),
        &[SettingsPage::Providers, SettingsPage::Threads],
    ),
    (
        Some("Code"),
        &[
            SettingsPage::Projects,
            SettingsPage::SourceControl,
            SettingsPage::Worktrees,
        ],
    ),
    (
        None,
        &[
            SettingsPage::Maintenance,
            SettingsPage::About,
            SettingsPage::Archive,
        ],
    ),
];

pub fn icon(page: SettingsPage) -> IconName {
    match page {
        SettingsPage::General => IconName::Settings2,
        SettingsPage::Appearance => IconName::Palette,
        SettingsPage::Chat => IconName::MessageSquareText,
        SettingsPage::Diffs => IconName::FileDiff,
        SettingsPage::Keybindings => IconName::Keyboard,
        SettingsPage::Providers => IconName::Bot,
        SettingsPage::Threads => IconName::MessagesSquare,
        SettingsPage::Projects => IconName::PanelsTopLeft,
        SettingsPage::SourceControl => IconName::GitBranch,
        SettingsPage::Worktrees => IconName::FolderTree,
        SettingsPage::Maintenance => IconName::Wrench,
        SettingsPage::About | SettingsPage::OpenSourceLicenses => IconName::Info,
        SettingsPage::Archive => IconName::Archive,
    }
}

/// Detail pages highlight their parent.
fn nav_parent(page: SettingsPage) -> SettingsPage {
    match page {
        SettingsPage::OpenSourceLicenses => SettingsPage::About,
        p => p,
    }
}

pub fn render(
    view: &mut SettingsView,
    page: SettingsPage,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let t = *cx.tokens();
    let active = nav_parent(page);
    let muted = t.sidebar_muted_foreground;
    let query = view.nav_query.clone();
    let focused = view.nav_search.focus_handle(cx).is_focused(window);
    let search_box = h_flex()
        .id("settings-search")
        .h(px(32.))
        .px(px(8.))
        .gap(px(8.))
        .rounded(radius::MD)
        .font_medium()
        .text_scale(TextSize::Sm)
        .text_color(muted)
        .when(!focused, |el| el.hover(move |s| s.bg(t.sidebar_row_hover)))
        .when(focused, |el| el.bg(t.sidebar_row_hover))
        .on_click(cx.listener(|this, _, window, cx| {
            this.nav_search.update(cx, |s, cx| s.focus(window, cx));
        }))
        .child(
            Icon::new(IconName::Search)
                .size(px(16.))
                .color(muted)
                .opacity(0.8),
        )
        .child(
            div().flex_1().min_w_0().child(
                Input::new(&view.nav_search)
                    .appearance(false)
                    .h(px(20.))
                    .text_size(px(14.))
                    .font_weight(crate::theme::font::MEDIUM),
            ),
        )
        .map(|el| {
            if query.is_empty() {
                el.child(Kbd::new("/"))
            } else {
                el.child(
                    Button::new("settings-search-clear")
                        .variant(ButtonVariant::GhostMuted)
                        .size(ButtonSize::IconMicro)
                        .icon(IconName::X)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.nav_search
                                .update(cx, |s, cx| s.set_value("", window, cx));
                            this.nav_query.clear();
                            cx.notify();
                        })),
                )
            }
        });

    let body =
        if query.trim().is_empty() {
            let mut groups = v_flex().gap(px(16.));
            for (gi, (heading, pages)) in GROUPS.iter().enumerate() {
                let mut group = v_flex().gap(px(4.));
                if let Some(h) = heading {
                    group = group.child(
                        div()
                            .px(px(8.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_medium()
                            .text_color(muted.opacity(muted.a * 0.8))
                            .child(*h),
                    );
                }
                let mut items = v_flex().gap(px(4.));
                for (i, p) in pages.iter().enumerate() {
                    items = items.child(nav_item(view, *p, *p == active, (gi, i), cx));
                }
                groups = groups.child(group.child(items));
            }
            groups.into_any_element()
        } else {
            let hits = search::matches(&query);
            if hits.is_empty() {
                div()
                    .px(px(8.))
                    .py(px(24.))
                    .flex()
                    .justify_center()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(muted)
                    .child("No settings found")
                    .into_any_element()
            } else {
                let highlight = view.nav_highlight.min(hits.len() - 1);
                v_flex()
                    .gap(px(2.))
                    .children(hits.into_iter().enumerate().map(|(i, hit)| {
                        let selected = i == highlight;
                        h_flex()
                            .id(("settings-hit", i))
                            .items_start()
                            .min_h(px(40.))
                            .px(px(8.))
                            .py(px(4.))
                            .gap(px(8.))
                            .rounded(radius::LG)
                            .cursor_pointer()
                            .when(selected, |el| el.bg(t.sidebar_row_hover))
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.nav_highlight != i {
                                    this.nav_highlight = i;
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_result(hit, window, cx)
                            }))
                            .child(
                                div().pt(px(2.)).child(
                                    Icon::new(icon(hit.page))
                                        .size(px(16.))
                                        .color(t.sidebar_icon_color),
                                ),
                            )
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_scale(TextSize::Sm)
                                            .font_medium()
                                            .text_color(t.sidebar_foreground)
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(hit.title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .line_height(px(14.))
                                            .text_color(muted.opacity(muted.a * 0.75))
                                            .child(hit.page.title()),
                                    ),
                            )
                    }))
                    .into_any_element()
            }
        };

    v_flex()
        .id("settings-nav")
        .flex_none()
        .w(px(240.))
        .h_full()
        .border_r_1()
        .border_color(t.border)
        .bg(t.sidebar)
        .text_color(t.sidebar_foreground)
        .p(layout::SIDEBAR_CONTENT_INSET)
        .gap(px(7.))
        .overflow_y_scroll()
        .child(search_box)
        .child(body)
        .into_any_element()
}

fn nav_item(
    view: &SettingsView,
    page: SettingsPage,
    active: bool,
    key: (usize, usize),
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let t = *cx.tokens();
    let muted = t
        .sidebar_muted_foreground
        .opacity(t.sidebar_muted_foreground.a * 0.8);
    let group = gpui::SharedString::from(format!("settings-nav-{}-{}", key.0, key.1));
    let _ = view;
    h_flex()
        .id(("settings-nav", key.0 * 16 + key.1))
        .group(group.clone())
        .h(px(32.))
        .px(px(10.))
        .gap(px(8.))
        .rounded(radius::CONTROL)
        .font_medium()
        .text_scale(TextSize::Sm)
        .cursor_pointer()
        .map(|el| {
            if active {
                el.bg(t.sidebar_row_selected)
                    .text_color(t.sidebar_foreground)
            } else {
                el.text_color(muted)
                    .hover(move |s| s.bg(t.sidebar_row_hover).text_color(t.sidebar_foreground))
                    .active(move |s| s.bg(t.sidebar_row_active))
            }
        })
        .on_click(cx.listener(move |this, _, _, cx| this.go(page, cx)))
        .child(
            Icon::new(icon(page))
                .size(px(16.))
                .color(if active {
                    t.sidebar_foreground
                } else {
                    t.sidebar_icon_color
                })
                .group_hover_color(group, t.sidebar_foreground),
        )
        .child(page.title())
}

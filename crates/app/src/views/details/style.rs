//! The card's row metrics (`threadDetailsPanelStyles.ts`, spec/05 §1.9): 32px rows,
//! radius 8, padding 10, gap 10, 13px medium at foreground/80, a black/3.5% (light)
//! or white/8% (dark) fill on hover, split rows with a 32px chevron half and a
//! hairline that shows only while the pair is in use.

use gpui::{
    App, Div, Hsla, InteractiveElement as _, ParentElement as _, Pixels, SharedString, Stateful,
    Styled as _, div, prelude::FluentBuilder as _, px,
};

use crate::theme::{ActiveTheme as _, TextExt as _, font, radius};
use crate::ui::{DISABLED_OPACITY, Icon, IconName};

pub const CARD_WIDTH: Pixels = px(280.);
pub const CARD_GAP: Pixels = px(12.);
pub const ROW_HEIGHT: Pixels = px(32.);
pub const ICON_SIZE: Pixels = px(16.);

/// The row hover / pressed / popup-open fill.
pub fn hover_fill(cx: &App) -> Hsla {
    if cx.tern_theme().is_dark() {
        gpui::white().opacity(0.08)
    } else {
        gpui::black().opacity(0.035)
    }
}

/// The running action's persistent fill (`bg-black/3.5%`, dark `white/5.5%`).
pub fn running_fill(cx: &App) -> Hsla {
    if cx.tern_theme().is_dark() {
        gpui::white().opacity(0.055)
    } else {
        gpui::black().opacity(0.035)
    }
}

/// `text-foreground/80`.
pub fn row_text(cx: &App) -> Hsla {
    let t = cx.tokens();
    t.foreground.opacity(0.8 * t.foreground.a)
}

fn base(cx: &App) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .min_w_0()
        .h(ROW_HEIGHT)
        .text_size(px(13.))
        .line_height(px(16.))
        .font_weight(font::MEDIUM)
        .text_color(row_text(cx))
        .whitespace_nowrap()
}

/// A full-width row button (`part="row"`).
pub fn row(id: impl Into<gpui::ElementId>, disabled: bool, cx: &App) -> Stateful<Div> {
    let hover = hover_fill(cx);
    base(cx)
        .id(id)
        .w_full()
        .flex_none()
        .px(px(10.))
        .gap(px(10.))
        .rounded(radius::LG)
        .map(|el| {
            if disabled {
                el.opacity(DISABLED_OPACITY)
            } else {
                el.cursor_pointer().hover(move |s| s.bg(hover))
            }
        })
}

/// A plain 32px row with no interaction (`THREAD_DETAILS_PANEL_LOCKED_ROW_CLASS`).
pub fn locked_row(cx: &App) -> Div {
    base(cx)
        .w_full()
        .flex_none()
        .px(px(10.))
        .gap(px(10.))
        .rounded(radius::LG)
}

pub fn icon(name: IconName, cx: &App) -> Icon {
    Icon::new(name)
        .size(ICON_SIZE)
        .color(cx.tokens().muted_foreground)
}

pub fn chevron(cx: &App) -> Icon {
    icon(IconName::ChevronDown, cx)
}

/// A truncating label.
pub fn label(text: impl Into<SharedString>) -> Div {
    div()
        .min_w_0()
        .overflow_hidden()
        .text_ellipsis()
        .child(text.into())
}

/// The split group container: the whole row takes the hover fill, and keeps it while
/// its menu is open.
pub fn split_group(id: &'static str, open: bool, running: bool, cx: &App) -> Stateful<Div> {
    let hover = hover_fill(cx);
    div()
        .id(id)
        .group(id)
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .flex_none()
        .rounded(radius::LG)
        .when(running, |el| el.bg(running_fill(cx)))
        .when(open, |el| el.bg(hover))
        .when(!open && !running, |el| el.hover(move |s| s.bg(hover)))
}

/// The primary half: `flex-1`, square right corners.
pub fn split_primary(id: impl Into<gpui::ElementId>, disabled: bool, cx: &App) -> Stateful<Div> {
    let hover = hover_fill(cx);
    base(cx)
        .id(id)
        .flex_1()
        .px(px(10.))
        .gap(px(10.))
        .rounded_l(radius::LG)
        .map(|el| {
            if disabled {
                el.opacity(DISABLED_OPACITY)
            } else {
                el.cursor_pointer().hover(move |s| s.bg(hover))
            }
        })
}

/// The 1x16 hairline between the halves.
pub fn split_separator(group: &'static str, open: bool, cx: &App) -> Div {
    let line = border65(cx);
    div()
        .w(px(1.))
        .h(px(16.))
        .flex_none()
        .when(open, |el| el.bg(line))
        .group_hover(group, move |s| s.bg(line))
}

/// The 32px chevron half.
pub fn split_secondary(
    id: impl Into<gpui::ElementId>,
    open: bool,
    disabled: bool,
    cx: &App,
) -> Stateful<Div> {
    let hover = hover_fill(cx);
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(32.))
        .min_h(ROW_HEIGHT)
        .self_stretch()
        .flex_none()
        .rounded_r(radius::LG)
        .when(open, |el| el.bg(hover))
        .map(|el| {
            if disabled {
                el.opacity(DISABLED_OPACITY)
            } else {
                el.cursor_pointer().hover(move |s| s.bg(hover))
            }
        })
        .child(chevron(cx))
}

/// `border/65`.
pub fn border65(cx: &App) -> Hsla {
    let b = cx.tokens().border;
    b.opacity(b.a * 0.65)
}

/// A section: padding 8 8 10, with the divider above when `separated`.
pub fn section(separated: bool, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .px(px(8.))
        .pt(px(8.))
        .pb(px(10.))
        .when(separated, |el| el.border_t_1().border_color(border65(cx)))
}

/// The section heading (Lineage): `text-2xs` medium muted, in a 32px row with 10px inset.
#[allow(dead_code)] // for the Lineage section, not wired yet
pub fn section_heading(text: &'static str, cx: &App) -> Div {
    div()
        .flex()
        .items_center()
        .min_h(px(32.))
        .mb(px(4.))
        .px(px(10.))
        .text_scale(crate::theme::TextSize::Xs2)
        .font_medium()
        .text_color(cx.tokens().muted_foreground)
        .child(text)
}

/// The chevron-select row (`part="select"`): label half and a 32px chevron cell, each
/// with its own hover fill over the row's.
pub fn select_row(
    id: &'static str,
    open: bool,
    disabled: bool,
    content: Div,
    cx: &App,
) -> Stateful<Div> {
    let hover = hover_fill(cx);
    let line = border65(cx);
    base(cx)
        .id(id)
        .group(id)
        .w_full()
        .flex_none()
        .rounded(radius::LG)
        .when(open, |el| el.bg(hover))
        .map(|el| {
            if disabled {
                el.opacity(DISABLED_OPACITY)
            } else {
                el.cursor_pointer().hover(move |s| s.bg(hover))
            }
        })
        .child(
            content
                .flex_1()
                .min_w_0()
                .h_full()
                .px(px(10.))
                .gap(px(10.))
                .rounded_l(radius::LG)
                .when(!disabled, |el| el.hover(move |s| s.bg(hover))),
        )
        .child(
            div()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .w(px(32.))
                .h_full()
                .flex_none()
                .rounded_r(radius::LG)
                .when(!disabled, |el| el.hover(move |s| s.bg(hover)))
                .child(
                    div()
                        .absolute()
                        .left(px(-1.))
                        .top(px(8.))
                        .w(px(1.))
                        .h(px(16.))
                        .when(open, |el| el.bg(line))
                        .group_hover(id, move |s| s.bg(line)),
                )
                .child(chevron(cx)),
        )
}

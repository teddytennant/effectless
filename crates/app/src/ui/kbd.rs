//! Kbd and KbdGroup (`kbd.tsx`, spec 01 §4.6).
//!
//! In menus and the command palette the shortcut is plain text instead; use
//! [`shortcut_text`].

use gpui::{
    AnyElement, App, Div, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window,
    div, px,
};

use crate::theme::{ActiveTheme as _, TextExt as _, radius};

#[derive(IntoElement)]
pub struct Kbd {
    label: SharedString,
    outline: bool,
}

impl Kbd {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            outline: false,
        }
    }

    /// `outline`: 1px border with a 2px bottom border, foreground/15, px 6, no fill.
    pub fn outline(mut self) -> Self {
        self.outline = true;
        self
    }
}

impl RenderOnce for Kbd {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.tokens();
        let s = cx.states();
        let el = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .h(px(20.))
            .min_w(px(20.))
            .rounded(radius::SM)
            .font_sans()
            .font_medium()
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(t.muted_foreground)
            .child(self.label);
        if self.outline {
            el.px(px(6.))
                .border_1()
                .border_b_2()
                .border_color(s.kbd_border)
        } else {
            el.px(px(4.)).bg(s.kbd_fill)
        }
    }
}

/// Inline row of keys, gap 4.
#[derive(IntoElement, Default)]
pub struct KbdGroup {
    children: Vec<AnyElement>,
}

impl KbdGroup {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ParentElement for KbdGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for KbdGroup {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .children(self.children)
    }
}

/// The menu / palette shortcut: 12px medium `secondary-label`, right-aligned.
/// (`tracking-widest` is not reproduced: GPUI has no letter-spacing.)
pub fn shortcut_text(label: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .ml_auto()
        .pl(px(16.))
        .flex_none()
        .font_medium()
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(cx.tokens().secondary_label)
        .child(label.into())
}

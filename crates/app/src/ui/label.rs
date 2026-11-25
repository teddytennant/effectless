//! Label (`label.tsx`, spec 01 §4.26): 14px medium foreground, line height 16.

use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, SharedString, StyleRefinement, Styled,
    Window, div, px,
};

use super::Refine as _;
use crate::theme::{ActiveTheme as _, TextExt as _};

#[derive(IntoElement)]
pub struct Label {
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl Label {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            children: vec![text.into().into_any_element()],
            style: StyleRefinement::default(),
        }
    }
}

impl ParentElement for Label {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Label {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Label {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .font_medium()
            .text_size(px(14.))
            .line_height(px(16.))
            .text_color(cx.tokens().foreground)
            .children(self.children)
            .refine(&self.style)
    }
}

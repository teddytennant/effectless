//! Placeholder for the right panel (spec 05 §1-2). The shell places it in the
//! right column with its resize handle and width from `AppState::right_panel`.

use gpui::{Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div, px};

use crate::theme::{ActiveTheme as _, TextExt as _};

pub struct DiffPanelView;

impl DiffPanelView {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for DiffPanelView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        div()
            .size_full()
            .p(px(16.))
            .text_scale(crate::theme::TextSize::Sm)
            .text_color(t.muted_foreground)
            .child("Right panel")
    }
}

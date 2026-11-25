//! Placeholder for the composer (spec 04). The shell places it under `ChatView`
//! in the chat column, inside the 736px content lane.

use gpui::{Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div, px};

use crate::theme::{ActiveTheme as _, TextExt as _, layout, radius};

pub struct ComposerView;

impl ComposerView {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for ComposerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.tern_theme().clone();
        let t = theme.colors;
        div()
            .w_full()
            .flex()
            .justify_center()
            .px(px(24.))
            .pb(px(20.))
            .child(
                div()
                    .w_full()
                    .max_w(layout::CHAT_CONTENT_MAX_WIDTH)
                    .h(px(110.))
                    .rounded(radius::XL2)
                    .border_1()
                    .border_color(t.border)
                    .bg(t.card)
                    .shadow(theme.shadows.composer())
                    .px(px(16.))
                    .py(px(16.))
                    .text_scale(crate::theme::TextSize::Sm)
                    .text_color(t.placeholder)
                    .child("Ask anything. @ files, $ skills, / commands"),
            )
    }
}

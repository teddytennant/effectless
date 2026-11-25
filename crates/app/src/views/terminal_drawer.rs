//! Placeholder for the terminal drawer (spec 05 §3). The shell places it at the
//! bottom of the chat column with its resize handle and height from
//! `AppState::drawer`.

use gpui::{Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div, px};

use crate::theme::{ActiveTheme as _, TextExt as _};

pub struct TerminalDrawerView;

impl TerminalDrawerView {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for TerminalDrawerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.tokens();
        div()
            .size_full()
            .bg(t.terminal_background)
            .p(px(12.))
            .font_mono()
            .text_size(px(crate::theme::font::TERMINAL_SIZE))
            .text_color(t.terminal_foreground)
            .child("$")
    }
}

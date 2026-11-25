//! Placeholder for the command palette (spec 02 §4). The shell opens it when
//! `AppState::palette_open` is set and draws the backdrop and `dialog-glass`
//! surface (radius 16, max 576 x 420, top padding 10vh); this view renders the
//! input row and the list. Close with `AppState::set_palette_open(false)`.

use gpui::{Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div, px};

use crate::theme::{ActiveTheme as _, TextExt as _};
use crate::ui::{Icon, IconName, h_flex};

pub struct CommandPaletteView;

impl CommandPaletteView {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for CommandPaletteView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        h_flex()
            .h(px(48.))
            .px(px(16.))
            .gap(px(12.))
            .text_scale(crate::theme::TextSize::Sm)
            .text_color(t.placeholder)
            .child(Icon::new(IconName::Search).color(t.icon_muted))
            .child(div().child("Search commands, projects and threads…"))
    }
}

//! Placeholder for settings (spec 06). The shell draws the modal (backdrop,
//! padding, the 16px-radius surface) and places this view inside it; this view
//! renders the left nav and the content column. Close with
//! `AppState::close_settings`.

use gpui::{
    Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window, div, px,
};

use crate::app_state::AppState;
use crate::theme::{ActiveTheme as _, TextExt as _};
use crate::ui::{h_flex, v_flex};

pub struct SettingsView {
    app: Entity<AppState>,
}

impl SettingsView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = AppState::global(cx);
        cx.observe(&app, |_, _, cx| cx.notify()).detach();
        Self { app }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        let page = self
            .app
            .read(cx)
            .settings_page()
            .map(|p| p.title())
            .unwrap_or("");
        v_flex().size_full().child(
            h_flex()
                .h(px(48.))
                .flex_none()
                .pl(px(20.))
                .pr(px(8.))
                .gap(px(12.))
                .border_b_1()
                .border_color(t.border)
                .font_medium()
                .text_scale(crate::theme::TextSize::Sm)
                .child(div().text_color(t.muted_foreground).child("Settings"))
                .child(div().text_color(t.icon_muted).child("/"))
                .child(div().text_color(t.foreground).child(page)),
        )
    }
}

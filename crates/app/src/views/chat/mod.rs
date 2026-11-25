//! Placeholder for the chat column (spec 03): the 40px header with the breadcrumb
//! and the timeline. The shell places it for Index, Draft and Thread routes.
//!
//! When the sidebar is collapsed, the header's left padding must become
//! `layout::TITLEBAR_CONTENT_LEFT` (52) so the title clears the fixed toggle; read
//! `AppState::sidebar.open`. The panel toggles at the top right are the shell's
//! (`shell::titlebar`); keep the header content clear of
//! `crate::shell::titlebar_controls_width`.

use gpui::{
    Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window, div, px,
};

use crate::app_state::{AppState, Route};
use crate::store::Store;
use crate::theme::{ActiveTheme as _, TextExt as _, layout};
use crate::ui::{h_flex, v_flex};

pub struct ChatView {
    app: Entity<AppState>,
    store: Entity<Store>,
}

impl ChatView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = AppState::global(cx);
        let store = Store::global(cx);
        cx.observe(&app, |_, _, cx| cx.notify()).detach();
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self { app, store }
    }
}

impl Render for ChatView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        let app = self.app.read(cx);
        let store = self.store.read(cx);
        let sidebar_open = app.sidebar.open;
        let (project, title) = match app.chat_route() {
            Route::Thread(id) => {
                let shell = store.snapshot().thread(id);
                let project = shell
                    .and_then(|s| store.snapshot().project(&s.thread.project_id))
                    .map(|p| p.title.clone());
                (project, shell.map(|s| s.thread.title.clone()))
            }
            Route::Draft(project) => (
                store.snapshot().project(project).map(|p| p.title.clone()),
                Some("New thread".to_string()),
            ),
            _ => (
                store.snapshot().projects.first().map(|p| p.title.clone()),
                Some("New thread".to_string()),
            ),
        };
        let header = h_flex()
            .h(layout::TOPBAR_HEIGHT)
            .flex_none()
            .pl(if sidebar_open {
                layout::WORKSPACE_GUTTER
            } else {
                layout::TITLEBAR_CONTENT_LEFT
            })
            .pr(layout::WORKSPACE_GUTTER)
            .gap(px(12.))
            .font_medium()
            .text_scale(crate::theme::TextSize::Sm)
            .children(project.map(|p| div().text_color(t.muted_foreground).child(p)))
            .child(div().text_color(t.icon_muted).child("/"))
            .children(title.map(|title| div().text_color(t.foreground).child(title)));
        v_flex().size_full().child(header).child(div().flex_1())
    }
}

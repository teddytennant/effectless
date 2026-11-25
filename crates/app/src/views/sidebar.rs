//! Placeholder for the thread sidebar (spec 02 §2). The shell owns the container,
//! its width, the rail and collapse; this view renders the contents, starting
//! with the 40px chrome header (brand at margin-left 52, clear of the toggle).

use gpui::{
    Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window, div, px,
};

use crate::store::Store;
use crate::theme::{ActiveTheme as _, TextExt as _, layout};
use crate::ui::{Icon, IconName, h_flex, v_flex};

pub struct SidebarView {
    store: Entity<Store>,
}

impl SidebarView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = Store::global(cx);
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self { store }
    }
}

impl Render for SidebarView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        let store = self.store.read(cx);
        let rows = store.snapshot().threads.iter().map(|shell| {
            div()
                .px(px(10.))
                .py(px(6.))
                .rounded(px(8.))
                .text_scale(crate::theme::TextSize::Sm)
                .text_color(t.sidebar_foreground)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(shell.thread.title.clone())
        });
        v_flex()
            .size_full()
            .child(
                // SidebarChromeHeader: h 40, brand at ml 52.
                h_flex()
                    .h(layout::TOPBAR_HEIGHT)
                    .flex_none()
                    .pl(layout::TITLEBAR_CONTENT_LEFT)
                    .pr(px(12.))
                    .child(
                        div()
                            .font_semibold()
                            .text_size(px(14.))
                            .text_color(t.sidebar_foreground)
                            .child("Tern"),
                    ),
            )
            .child(
                h_flex()
                    .mx(px(8.))
                    .mt(px(8.))
                    .h(px(32.))
                    .px(px(8.))
                    .gap(px(8.))
                    .text_scale(crate::theme::TextSize::Sm)
                    .font_medium()
                    .text_color(t.sidebar_muted_foreground)
                    .child(Icon::new(IconName::Search).color(t.sidebar_icon_color))
                    .child("Search"),
            )
            .child(v_flex().p(px(8.)).gap(px(1.)).children(rows))
    }
}

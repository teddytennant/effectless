//! `NoProjectsHero` (spec 02 §2.16): the main area when there are no projects.

use gpui::{App, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window, div, px};

use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, layout};
use crate::ui::{Button, ButtonSize, ButtonVariant, IconName, h_flex, v_flex};
use crate::views::command_palette::{self, PaletteIntent};

#[derive(IntoElement, Default)]
pub struct NoProjectsHero;

impl NoProjectsHero {
    pub fn new() -> Self {
        Self
    }
}

impl RenderOnce for NoProjectsHero {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.tokens();
        let wide = window.viewport_size().width >= px(640.);
        v_flex()
            .size_full()
            .bg(t.background)
            // WorkspacePageHeader strip.
            .child(div().h(layout::TOPBAR_HEIGHT).flex_none())
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .p(px(48.))
                    .gap(px(24.))
                    .child(
                        v_flex()
                            .max_w(px(512.))
                            .px(px(32.))
                            .py(px(48.))
                            .items_center()
                            .child(
                                div()
                                    .font_semibold()
                                    .text_color(t.foreground)
                                    .text_scale(if wide { TextSize::Xl3 } else { TextSize::Xl2 })
                                    .child("What should we work on?"),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_scale(TextSize::Sm)
                                    .text_color(t.muted_foreground)
                                    .child("Add a project to start your first thread."),
                            )
                            .child(
                                h_flex().mt(px(24.)).gap(px(8.)).child(
                                    Button::new("hero-add-project")
                                        .size(ButtonSize::Sm)
                                        .variant(ButtonVariant::Default)
                                        .icon(IconName::Plus)
                                        .label("Add project")
                                        .on_click(|_, _, cx| {
                                            command_palette::open(PaletteIntent::AddProject, cx)
                                        }),
                                ),
                            ),
                    ),
            )
    }
}

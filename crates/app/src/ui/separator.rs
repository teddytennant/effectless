//! Separator (`separator.tsx`, spec 01 §4.19): a 1px `border` line.

use gpui::{App, IntoElement, RenderOnce, StyleRefinement, Styled, Window, div, px};

use super::Refine as _;
use crate::theme::ActiveTheme as _;

#[derive(IntoElement)]
pub struct Separator {
    vertical: bool,
    style: StyleRefinement,
}

impl Separator {
    pub fn horizontal() -> Self {
        Self {
            vertical: false,
            style: StyleRefinement::default(),
        }
    }

    /// Stretches to the parent's height.
    pub fn vertical() -> Self {
        Self {
            vertical: true,
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for Separator {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Separator {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let el = div().flex_none().bg(cx.tokens().border);
        let el = if self.vertical {
            el.w(px(1.)).h_full()
        } else {
            el.h(px(1.)).w_full()
        };
        el.refine(&self.style)
    }
}

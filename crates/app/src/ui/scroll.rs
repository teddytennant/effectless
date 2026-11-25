//! Scroll areas (`scroll-area.tsx`, spec 01 §3.7, §4.23).
//!
//! Scrolling and the overlay scrollbars come from gpui-component, which the theme
//! points at T3's thumb colors (`app-scrollbar-thumb`, 6px, fully rounded, shown on
//! hover/scroll):
//!
//! ```ignore
//! use gpui_component::scroll::ScrollableElement as _;
//! div().id("timeline").size_full().overflow_y_scrollbar().child(…)
//! ```
//!
//! This module adds the `scrollFade` edges: 24px fades at edges that have more
//! content. GPUI has no masks, so the fade is painted as a gradient of the surface
//! color over the content.

use gpui::{
    App, Div, Hsla, IntoElement, RenderOnce, Styled, Window, div, linear_color_stop,
    linear_gradient, px,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeEdge {
    Top,
    Bottom,
}

/// A gradient overlay that fades content into `surface` at one edge.
#[derive(IntoElement)]
pub struct ScrollFade {
    edge: FadeEdge,
    surface: Hsla,
    size: f32,
}

/// `scrollFade` at `edge` over a `surface`-colored background, 24px tall.
pub fn scroll_fade(edge: FadeEdge, surface: Hsla) -> ScrollFade {
    ScrollFade {
        edge,
        surface,
        size: 24.,
    }
}

impl ScrollFade {
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for ScrollFade {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let (angle, el): (f32, Div) = match self.edge {
            FadeEdge::Top => (180., div().absolute().top_0()),
            FadeEdge::Bottom => (0., div().absolute().bottom_0()),
        };
        el.left_0().right_0().h(px(self.size)).bg(linear_gradient(
            angle,
            linear_color_stop(self.surface, 0.),
            linear_color_stop(self.surface.opacity(0.), 1.),
        ))
    }
}

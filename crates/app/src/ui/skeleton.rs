//! Skeleton (`skeleton.tsx`, spec 01 §4.20): muted-foreground/15 with the stepped
//! `animate-skeleton` pulse (2.4s: opacity 1 until 42%, 0.55 from 50 to 92%,
//! ramped in 4 steps).

use gpui::{
    Animation, AnimationExt as _, App, ElementId, IntoElement, RenderOnce, StyleRefinement, Styled,
    Window, div,
};

use super::Refine as _;
use crate::theme::{ActiveTheme as _, motion, radius};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SkeletonShape {
    /// radius 4
    #[default]
    Block,
    /// radius 8
    Card,
    Pill,
}

#[derive(IntoElement)]
pub struct Skeleton {
    id: ElementId,
    shape: SkeletonShape,
    style: StyleRefinement,
}

impl Skeleton {
    /// Size it with `.w()` / `.h()`.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            shape: SkeletonShape::Block,
            style: StyleRefinement::default(),
        }
    }

    pub fn shape(mut self, shape: SkeletonShape) -> Self {
        self.shape = shape;
        self
    }
}

impl Styled for Skeleton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

/// Opacity of `animate-skeleton` at `t` in 0..1 of the period.
pub fn skeleton_opacity(t: f32) -> f32 {
    let step = |x: f32| (x * 4.).floor() / 4.;
    if t < 0.42 {
        1.
    } else if t < 0.5 {
        1. - 0.45 * step((t - 0.42) / 0.08)
    } else if t < 0.92 {
        0.55
    } else {
        0.55 + 0.45 * step((t - 0.92) / 0.08)
    }
}

impl RenderOnce for Skeleton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let r = match self.shape {
            SkeletonShape::Block => radius::SM,
            SkeletonShape::Card => radius::LG,
            SkeletonShape::Pill => radius::FULL,
        };
        div()
            .bg(cx.states().skeleton)
            .rounded(r)
            .refine(&self.style)
            .with_animation(
                self.id,
                Animation::new(motion::SKELETON_PERIOD).repeat(),
                |el, t| el.opacity(skeleton_opacity(t)),
            )
    }
}

//! Badge (`badge.tsx`, spec 01 §4.5).

use gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement, RenderOnce, SharedString, StyleRefinement,
    Styled, Window, div, prelude::FluentBuilder as _, px,
};

use super::{Icon, IconName, Refine as _};
use crate::theme::{ActiveTheme as _, TextExt as _, color::mix_srgb, radius};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum BadgeVariant {
    #[default]
    Default,
    Destructive,
    Secondary,
    Outline,
    Error,
    Info,
    Success,
    Warning,
    /// A user label color: bg label @8% (dark 12%), text mix(label 30%, foreground).
    Label(Hsla),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeSize {
    /// 18 high, px 3, text 12.
    #[default]
    Default,
    /// 16 high, px 3, text 10, leading 1.
    Sm,
    /// 22 high, px 5, text 14.
    Lg,
    /// 24 high, px 7, text 12, radius 8.
    Control,
}

#[derive(IntoElement)]
pub struct Badge {
    variant: BadgeVariant,
    size: BadgeSize,
    icon: Option<IconName>,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl Badge {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            variant: BadgeVariant::Default,
            size: BadgeSize::Default,
            icon: None,
            children: vec![label.into().into_any_element()],
            style: StyleRefinement::default(),
        }
    }

    /// A badge with custom content.
    pub fn empty() -> Self {
        Self {
            children: Vec::new(),
            ..Self::new("")
        }
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: BadgeSize) -> Self {
        self.size = size;
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
}

impl ParentElement for Badge {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Badge {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.tokens();
        let s = cx.states();
        let dark = cx.tern_theme().is_dark();
        let transparent = gpui::transparent_black();
        let (bg, fg, border) = match self.variant {
            BadgeVariant::Default => (t.primary, t.primary_foreground, transparent),
            BadgeVariant::Destructive => (t.destructive, gpui::white(), transparent),
            BadgeVariant::Secondary => (t.secondary, t.secondary_foreground, transparent),
            BadgeVariant::Outline => (s.field_fill, t.foreground, t.input),
            BadgeVariant::Error => (s.error_badge_bg, t.destructive_foreground, transparent),
            BadgeVariant::Info => (s.info_surface, t.info_foreground, transparent),
            BadgeVariant::Success => (s.success_surface, t.success_foreground, transparent),
            BadgeVariant::Warning => (s.warning_badge_surface, t.warning_foreground, transparent),
            BadgeVariant::Label(label) => (
                label.opacity(if dark { 0.12 } else { 0.08 }),
                mix_srgb(label, t.foreground, if dark { 0.45 } else { 0.3 }),
                transparent,
            ),
        };
        let (h, pad, text, lh, r) = match self.size {
            BadgeSize::Default => (18., 3., 12., 16., radius::SM),
            BadgeSize::Sm => (16., 3., 10., 10., radius::SM),
            BadgeSize::Lg => (22., 5., 14., 20., radius::SM),
            BadgeSize::Control => (24., 7., 12., 16., radius::LG),
        };
        div()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .h(px(h))
            .min_w(px(h))
            .px(px(pad))
            .rounded(r)
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_color(fg)
            .font_medium()
            .text_size(px(text))
            .line_height(px(lh))
            .whitespace_nowrap()
            .when_some(self.icon, |el, icon| {
                el.child(Icon::new(icon).size(px(12.)).color(fg).opacity(0.8))
            })
            .children(self.children)
            .refine(&self.style)
    }
}

//! Lucide icons as bundled SVGs, stroked at 1.75 (spec 01 §5).
//!
//! ```ignore
//! Icon::new(IconName::PanelLeftClose).size(px(16.)).color(cx.tokens().muted_foreground)
//! ```
//!
//! Without `.color()` the icon takes the inherited text color, which is what CSS
//! `currentColor` does. Default size is 16px.

use std::time::Duration;

use gpui::{
    App, Hsla, InteractiveElement as _, IntoElement, Pixels, RenderOnce, SharedString, Styled,
    TimedTransformation, Transformation, Window, px, radians, svg,
};

use crate::assets::{self, IconName};

#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: Pixels,
    color: Option<Hsla>,
    opacity: f32,
    stroke: Option<f32>,
    spin: Option<Duration>,
    group_hover: Option<(SharedString, Hsla)>,
}

impl Icon {
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: px(16.),
            color: None,
            opacity: 1.,
            stroke: None,
            spin: None,
            group_hover: None,
        }
    }

    /// Square size in px (lucide icons are drawn on a 24-unit grid).
    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = size.into();
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Multiplies the color's alpha (menu icons are 80%).
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// Overrides the 1.75 stroke (toast close 2.25, checkbox mark 3).
    pub fn stroke(mut self, stroke: f32) -> Self {
        self.stroke = Some(stroke);
        self
    }

    /// Color while the named group is hovered (buttons tint their icon on hover).
    pub fn group_hover_color(mut self, group: impl Into<SharedString>, color: Hsla) -> Self {
        self.group_hover = Some((group.into(), color));
        self
    }

    /// Spins one turn per `period` (Spinner uses 1s).
    pub fn spin(mut self, period: Duration) -> Self {
        self.spin = Some(period);
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let color = self
            .color
            .or_else(|| Some(window.text_style().color))
            .unwrap_or_else(gpui::black)
            .opacity(self.opacity);
        let path = match self.stroke {
            Some(stroke) => assets::icon_path_with_stroke(self.name, stroke),
            None => assets::icon_path(self.name),
        };
        let mut el = svg()
            .path(path)
            .size(self.size)
            .flex_none()
            .text_color(color);
        if let Some((group, hover)) = self.group_hover {
            let hover = hover.opacity(self.opacity);
            el = el.group_hover(group, move |s| s.text_color(hover));
        }
        if let Some(period) = self.spin {
            el = el.with_timed_transformation(TimedTransformation::rotation(
                period,
                Transformation::rotate(radians(0.)),
            ));
        }
        el
    }
}

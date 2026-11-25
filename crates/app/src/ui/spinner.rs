//! Spinner (`spinner.tsx`, spec 01 §4.21): lucide `LoaderCircle` turning once a second.

use gpui::{App, Hsla, IntoElement, Pixels, RenderOnce, Window, px};

use super::{Icon, IconName};
use crate::theme::{ActiveTheme as _, motion};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpinnerSize {
    /// 12
    Xs,
    /// 14
    Sm,
    /// 16
    Md,
    /// 20
    Lg,
}

impl SpinnerSize {
    pub fn px(self) -> Pixels {
        px(match self {
            Self::Xs => 12.,
            Self::Sm => 14.,
            Self::Md => 16.,
            Self::Lg => 20.,
        })
    }
}

#[derive(IntoElement)]
pub struct Spinner {
    size: Pixels,
    color: Option<Hsla>,
    muted: bool,
    refresh: bool,
}

impl Spinner {
    pub fn new() -> Self {
        Self {
            size: px(16.),
            color: None,
            muted: false,
            refresh: false,
        }
    }

    /// `RefreshIcon`: the same spin with `RefreshCw`.
    pub fn refresh() -> Self {
        Self {
            refresh: true,
            ..Self::new()
        }
    }

    pub fn size(mut self, size: SpinnerSize) -> Self {
        self.size = size.px();
        self
    }

    pub fn size_px(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Tone `muted`: muted-foreground.
    pub fn muted(mut self) -> Self {
        self.muted = true;
        self
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for Spinner {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let name = if self.refresh {
            IconName::RefreshCw
        } else {
            IconName::LoaderCircle
        };
        let mut icon = Icon::new(name).size(self.size).spin(motion::SPIN_PERIOD);
        if let Some(color) = self.color {
            icon = icon.color(color);
        } else if self.muted {
            icon = icon.color(cx.tokens().muted_foreground);
        }
        icon
    }
}

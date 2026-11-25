//! Switch (`switch.tsx`, spec 01 §4.14).
//!
//! Controlled: pass `checked` and handle `on_change`. Thumb travel is not animated
//! (track color 200ms / thumb 150ms in T3).

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
};

use super::{DISABLED_OPACITY, focus_ring};
use crate::theme::{ActiveTheme as _, radius};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwitchSize {
    /// Track 30x18, thumb 14, travel 12.
    #[default]
    Default,
    /// Track 26x16, thumb 12, travel 10.
    Sm,
}

type OnChange = Rc<dyn Fn(bool, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    checked: bool,
    /// `mixed`: thumb centered at 70% opacity.
    mixed: bool,
    disabled: bool,
    size: SwitchSize,
    on_change: Option<OnChange>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            mixed: false,
            disabled: false,
            size: SwitchSize::Default,
            on_change: None,
        }
    }

    pub fn mixed(mut self, mixed: bool) -> Self {
        self.mixed = mixed;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn size(mut self, size: SwitchSize) -> Self {
        self.size = size;
        self
    }

    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.tokens();
        let theme = cx.tern_theme().clone();
        let thumb_size: f32 = match self.size {
            SwitchSize::Default => 16.,
            SwitchSize::Sm => 14.,
        };
        let track_w = 2. * thumb_size - 2.;
        let track_h = thumb_size + 2.;
        let thumb = thumb_size - 2.;
        let travel = track_w - 4. - thumb;
        let offset = if self.mixed {
            travel / 2.
        } else if self.checked {
            travel
        } else {
            0.
        };
        let thumb_bg = if theme.is_dark() && !self.checked {
            t.muted_foreground
        } else {
            t.background
        };
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let checked = self.checked;
        div()
            .id(self.id)
            .track_focus(&focus)
            .flex_none()
            .w(px(track_w))
            .h(px(track_h))
            .p(px(2.))
            .rounded(radius::FULL)
            .bg(if checked && !self.mixed {
                t.primary
            } else {
                t.input
            })
            .focus_visible(|s| s.shadow(focus_ring(cx)))
            .child(
                div()
                    .size(px(thumb))
                    .ml(px(offset))
                    .rounded(radius::FULL)
                    .bg(thumb_bg)
                    .shadow(theme.shadows.sm(0.05))
                    .when(self.mixed, |el| el.opacity(0.7)),
            )
            .when(self.disabled, |el| el.opacity(DISABLED_OPACITY))
            .when(!self.disabled, |el| {
                el.cursor_pointer().when_some(self.on_change, |el, f| {
                    el.on_click(move |_, window, cx| f(!checked, window, cx))
                })
            })
    }
}

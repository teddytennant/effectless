//! Input and Textarea styling (`input.tsx`, `textarea.tsx`, spec 01 §4.2-4.3).
//!
//! The editing engine is gpui-component's (`InputState` / `TextareaState`); these
//! wrappers draw T3's field around it: radius 8, 1px `input` border, background
//! fill (input/32 in dark), `shadow-xs/5`, edge line, and the field ring on focus.
//!
//! ```ignore
//! let state = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
//! TextField::new(&state).size(InputSize::Sm)
//!
//! let notes = cx.new(|cx| TextareaState::new(window, cx));
//! TextareaField::new(&notes)
//! ```

use gpui::{
    AnyElement, App, Entity, Focusable as _, IntoElement, ParentElement as _, RenderOnce, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputState};

use super::{DISABLED_OPACITY, edge_line, field_ring};
use crate::theme::{ActiveTheme as _, TextExt as _, font, radius};

pub use gpui_component::input::Textarea;
pub use gpui_component::input::TextareaState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputSize {
    /// 32 total, px 11, text 14.
    #[default]
    Default,
    /// 28 total, px 9.
    Sm,
    /// 36 total, px 11.
    Lg,
    /// 30 total, px 9, text 12, radius 6.
    Compact,
}

impl InputSize {
    /// (inner height, padding x, text size, radius)
    fn spec(self) -> (f32, f32, f32, gpui::Pixels) {
        match self {
            Self::Default => (30., 11., 14., radius::LG),
            Self::Sm => (26., 9., 14., radius::LG),
            Self::Lg => (34., 11., 14., radius::LG),
            Self::Compact => (28., 9., 12., radius::MD),
        }
    }
}

/// Paints the field chrome around `child`; `focused` selects the ring look.
fn field(
    child: AnyElement,
    focused: bool,
    invalid: bool,
    disabled: bool,
    r: gpui::Pixels,
    cx: &App,
) -> gpui::Div {
    let t = cx.tokens();
    let s = cx.states();
    let theme = cx.tern_theme();
    let border = match (invalid, focused) {
        (true, true) => s.invalid_focus_border,
        (true, false) => s.invalid_border,
        (false, true) => t.ring,
        (false, false) => t.input,
    };
    let ring = if invalid {
        s.invalid_ring
    } else {
        s.ring_focus
    };
    div()
        .relative()
        .w_full()
        .rounded(r)
        .border_1()
        .border_color(border)
        .bg(s.field_fill)
        .text_color(t.foreground)
        .shadow(if focused {
            field_ring(ring)
        } else {
            theme.shadows.xs5()
        })
        .child(child)
        .when(!focused && !disabled, |el| {
            el.child(edge_line(r - px(1.), cx))
        })
        .when(disabled, |el| el.opacity(DISABLED_OPACITY))
}

/// A single-line input with T3's field chrome.
#[derive(IntoElement)]
pub struct TextField {
    state: Entity<InputState>,
    size: InputSize,
    invalid: bool,
    disabled: bool,
    mono: bool,
}

impl TextField {
    pub fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            size: InputSize::Default,
            invalid: false,
            disabled: false,
            mono: false,
        }
    }

    pub fn size(mut self, size: InputSize) -> Self {
        self.size = size;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// `font="mono"`.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
}

impl RenderOnce for TextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (inner, pad, text, r) = self.size.spec();
        let focused = self.state.focus_handle(cx).is_focused(window);
        let input = Input::new(&self.state)
            .appearance(false)
            .disabled(self.disabled)
            .h(px(inner))
            .px(px(pad))
            .text_size(px(text))
            .line_height(px(inner))
            .when(self.mono, |el| el.font_family(font::MONO))
            .into_any_element();
        field(input, focused, self.invalid, self.disabled, r, cx)
    }
}

/// A multi-line field: padding 5/11, min height 70, grows to 256.
#[derive(IntoElement)]
pub struct TextareaField {
    state: Entity<TextareaState>,
    invalid: bool,
    disabled: bool,
}

impl TextareaField {
    pub fn new(state: &Entity<TextareaState>) -> Self {
        Self {
            state: state.clone(),
            invalid: false,
            disabled: false,
        }
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for TextareaField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focused = self.state.focus_handle(cx).is_focused(window);
        let area = div()
            .min_h(px(68.))
            .max_h(px(254.))
            .px(px(11.))
            .py(px(5.))
            .text_scale(crate::theme::TextSize::Sm)
            .child(
                Textarea::new(&self.state)
                    .appearance(false)
                    .disabled(self.disabled),
            )
            .into_any_element();
        field(area, focused, self.invalid, self.disabled, radius::LG, cx)
    }
}

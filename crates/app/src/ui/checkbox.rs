//! Checkbox (`checkbox.tsx`, spec 01 §4.15).

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
};

use super::{DISABLED_OPACITY, Icon, IconName, edge_line};
use crate::theme::{ActiveTheme as _, radius};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CheckState {
    #[default]
    Unchecked,
    Checked,
    Indeterminate,
}

type OnChange = Rc<dyn Fn(bool, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    state: CheckState,
    disabled: bool,
    invalid: bool,
    contrast: bool,
    on_change: Option<OnChange>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            state: if checked {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            },
            disabled: false,
            invalid: false,
            contrast: false,
            on_change: None,
        }
    }

    pub fn state(mut self, state: CheckState) -> Self {
        self.state = state;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// `contrast` variant: border foreground/55.
    pub fn contrast(mut self) -> Self {
        self.contrast = true;
        self
    }

    /// Called with the new checked value.
    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.tokens();
        let s = *cx.states();
        let theme = cx.tern_theme().clone();
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let checked = self.state == CheckState::Checked;
        let border = if self.invalid {
            s.invalid_border
        } else if self.contrast {
            s.checkbox_contrast_border
        } else {
            t.input
        };
        let ring = t.ring;
        let mut el = div()
            .id(self.id)
            .track_focus(&focus)
            .relative()
            .flex_none()
            .size(px(16.))
            .rounded(radius::SM)
            .border_1()
            .border_color(border)
            .bg(s.field_fill)
            .shadow(theme.shadows.xs5())
            .focus_visible(move |st| st.border_2().border_color(ring));
        el = match self.state {
            CheckState::Unchecked => el.child(edge_line(radius::SM - px(1.), cx)),
            CheckState::Checked => el.child(
                // The indicator is 1px larger on every side and covers the border.
                div()
                    .absolute()
                    .top(px(-1.))
                    .left(px(-1.))
                    .size(px(16.))
                    .rounded(radius::SM)
                    .bg(t.primary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::Check)
                            .size(px(12.))
                            .stroke(3.)
                            .color(t.primary_foreground),
                    ),
            ),
            CheckState::Indeterminate => el.child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::Minus)
                            .size(px(12.))
                            .stroke(3.)
                            .color(t.foreground),
                    ),
            ),
        };
        if self.disabled {
            return el.opacity(DISABLED_OPACITY);
        }
        el.cursor_pointer().when_some(self.on_change, |el, f| {
            el.on_click(move |_, window, cx| f(!checked, window, cx))
        })
    }
}

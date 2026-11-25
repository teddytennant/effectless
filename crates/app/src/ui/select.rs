//! Select (`select.tsx`, spec 01 §4.13).
//!
//! ```ignore
//! Select::new("diff-style", vec![SelectOption::new("unified", "Unified"), SelectOption::new("split", "Split")])
//!     .selected("unified")
//!     .on_change(|value, window, cx| …)
//! ```
//!
//! The list opens with the selected row over the trigger (`alignItemWithTrigger`,
//! macOS style). Items have no checkmark; the selected row is tinted foreground/8.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, ElementId, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels, Render, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div, point,
    prelude::FluentBuilder as _, px,
};

use super::menu::{item_base, popup_surface};
use super::popup::{PopupAnchor, PopupLayer};
use super::{DISABLED_OPACITY, Icon, IconName, bounds_probe, edge_line, field_ring};
use crate::theme::{ActiveTheme as _, TextExt as _, radius};

#[derive(Clone, Debug)]
pub struct SelectOption {
    pub value: SharedString,
    pub label: SharedString,
    pub icon: Option<IconName>,
}

impl SelectOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            icon: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectSize {
    #[default]
    Default,
    Sm,
    Lg,
    Xs,
    Compact,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectVariant {
    /// Full width, min 144, input-wrapper look.
    #[default]
    Default,
    /// Transparent, secondary-label text; hover/open bg accent.
    Ghost,
}

type OnChange = Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;

const ITEM_HEIGHT: f32 = 28.;

pub struct SelectList {
    options: Vec<SelectOption>,
    selected: Option<SharedString>,
    highlighted: Option<usize>,
    on_change: Option<OnChange>,
    min_width: Pixels,
    focus: FocusHandle,
}

impl Focusable for SelectList {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SelectList {
    fn choose(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(option) = self.options.get(ix) else {
            return;
        };
        let value = option.value.clone();
        let on_change = self.on_change.clone();
        PopupLayer::close(window, cx);
        if let Some(f) = on_change {
            f(&value, window, cx);
        }
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.options.len();
        if n == 0 {
            return;
        }
        match e.keystroke.key.as_str() {
            "down" => {
                self.highlighted = Some(self.highlighted.map_or(0, |i| (i + 1) % n));
                cx.notify();
            }
            "up" => {
                self.highlighted = Some(self.highlighted.map_or(n - 1, |i| (i + n - 1) % n));
                cx.notify();
            }
            "enter" | "space" => {
                if let Some(ix) = self.highlighted {
                    self.choose(ix, window, cx);
                }
            }
            "escape" => PopupLayer::close(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for SelectList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        let s = *cx.states();
        let mut list = div().flex().flex_col().p(px(4.));
        for (ix, option) in self.options.iter().enumerate() {
            let highlighted = self.highlighted == Some(ix);
            let selected = self.selected.as_ref() == Some(&option.value);
            list = list.child(
                item_base(highlighted, cx)
                    .id(("select-item", ix))
                    .when(selected && !highlighted, |el| el.bg(s.selected))
                    .when_some(option.icon, |el, icon| {
                        el.child(
                            Icon::new(icon)
                                .size(px(16.))
                                .color(t.icon_muted)
                                .opacity(0.8),
                        )
                    })
                    .child(option.label.clone())
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.highlighted != Some(ix) {
                            this.highlighted = Some(ix);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| this.choose(ix, window, cx))),
            );
        }
        popup_surface(cx)
            .id("select-list")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .min_w(self.min_width)
            .child(list)
    }
}

#[derive(IntoElement)]
pub struct Select {
    id: ElementId,
    options: Vec<SelectOption>,
    selected: Option<SharedString>,
    placeholder: SharedString,
    on_change: Option<OnChange>,
    size: SelectSize,
    variant: SelectVariant,
    disabled: bool,
    invalid: bool,
    width: Option<Pixels>,
}

impl Select {
    pub fn new(id: impl Into<ElementId>, options: Vec<SelectOption>) -> Self {
        Self {
            id: id.into(),
            options,
            selected: None,
            placeholder: "Select…".into(),
            on_change: None,
            size: SelectSize::Default,
            variant: SelectVariant::Default,
            disabled: false,
            invalid: false,
            width: None,
        }
    }

    pub fn selected(mut self, value: impl Into<SharedString>) -> Self {
        self.selected = Some(value.into());
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn on_change(mut self, f: impl Fn(&SharedString, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn size(mut self, size: SelectSize) -> Self {
        self.size = size;
        self
    }

    pub fn variant(mut self, variant: SelectVariant) -> Self {
        self.variant = variant;
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

    /// Fixed width instead of full width.
    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = Some(width.into());
        self
    }
}

impl RenderOnce for Select {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.tokens();
        let s = *cx.states();
        let theme = cx.tern_theme().clone();
        let open = PopupLayer::is_open(&self.id, cx);
        let (h, pad, text, lh, r, icon_px) = match self.size {
            SelectSize::Default => (32., 11., 14., 20., radius::LG, 16.),
            SelectSize::Sm => (28., 9., 14., 20., radius::LG, 16.),
            SelectSize::Lg => (36., 11., 14., 20., radius::LG, 16.),
            SelectSize::Xs => (24., 7., 12., 16., radius::MD, 14.),
            SelectSize::Compact => (28., 7., 12., 16., radius::MD, 14.),
        };
        let current = self
            .selected
            .as_ref()
            .and_then(|v| self.options.iter().find(|o| &o.value == v))
            .cloned();
        let selected_ix = self
            .selected
            .as_ref()
            .and_then(|v| self.options.iter().position(|o| &o.value == v));
        let ghost = self.variant == SelectVariant::Ghost;
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focused = focus.is_focused(window) || open;
        let bounds = Rc::new(std::cell::Cell::new(gpui::Bounds::<Pixels>::default()));
        let click_bounds = bounds.clone();
        let owner = self.id.clone();
        let options = self.options.clone();
        let selected = self.selected.clone();
        let on_change = self.on_change.clone();
        let label_color = if current.is_none() {
            t.placeholder
        } else if ghost && !open {
            t.secondary_label
        } else {
            t.foreground
        };
        let border = if ghost {
            gpui::transparent_black()
        } else if self.invalid && focused {
            s.invalid_focus_border
        } else if self.invalid {
            s.invalid_border
        } else if focused {
            t.ring
        } else {
            t.input
        };
        let mut el = div()
            .id(self.id.clone())
            .track_focus(&focus)
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .min_h(px(h))
            .px(px(pad))
            .rounded(r)
            .border_1()
            .border_color(border)
            .text_size(px(text))
            .line_height(px(lh))
            .font_sans()
            .text_color(label_color)
            .cursor_pointer()
            .child(bounds_probe(bounds))
            .when_some(current.as_ref().and_then(|c| c.icon), |el, icon| {
                el.child(
                    Icon::new(icon)
                        .size(px(icon_px))
                        .color(t.icon_muted)
                        .opacity(0.8),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(
                        current
                            .as_ref()
                            .map(|c| c.label.clone())
                            .unwrap_or(self.placeholder.clone()),
                    ),
            )
            .child(
                div().mr(px(-4.)).child(
                    Icon::new(IconName::ChevronDown)
                        .size(px(14.))
                        .color(label_color)
                        .opacity(0.7),
                ),
            );
        el = if ghost {
            el.when(open, |el| el.bg(t.accent).text_color(t.foreground))
                .hover(move |st| st.bg(t.accent).text_color(t.foreground))
                .focus_visible(|st| st.shadow(super::focus_ring(cx)))
        } else {
            el.bg(s.field_fill)
                .map(|el| match self.width {
                    Some(w) => el.w(w),
                    None => el.w_full().min_w(px(144.)),
                })
                .shadow(if focused {
                    field_ring(if self.invalid {
                        s.invalid_ring
                    } else {
                        s.ring_focus
                    })
                } else {
                    theme.shadows.xs5()
                })
                .when(!focused && !self.disabled, |el| {
                    el.child(edge_line(r - px(1.), cx))
                })
        };
        if self.disabled {
            return el.opacity(DISABLED_OPACITY).cursor_default();
        }
        el.on_mouse_down(MouseButton::Left, move |_, window, cx| {
            window.prevent_default();
            let trigger = click_bounds.get();
            let k = selected_ix.unwrap_or(0) as f32;
            let top = trigger.origin.y + (trigger.size.height - px(ITEM_HEIGHT)) / 2.
                - px(1. + 4. + ITEM_HEIGHT * k);
            let left = trigger.origin.x + px(pad) - px(1. + 4. + 8.);
            let anchor = PopupAnchor::Point {
                position: point(left, top),
                trigger: Some(trigger),
            };
            let options = options.clone();
            let selected = selected.clone();
            let on_change = on_change.clone();
            PopupLayer::toggle(
                owner.clone(),
                anchor,
                move |_, cx| {
                    let view = cx.new(|cx| SelectList {
                        options,
                        highlighted: selected_ix,
                        selected,
                        on_change,
                        min_width: trigger.size.width + px(26. - 2. * pad),
                        focus: cx.focus_handle(),
                    });
                    let focus = view.read(cx).focus.clone();
                    (view.into(), Some(focus))
                },
                window,
                cx,
            );
            cx.stop_propagation();
        })
    }
}

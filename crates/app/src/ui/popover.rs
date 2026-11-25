//! Popover (`popover.tsx`, spec 01 §4.8).
//!
//! ```ignore
//! Popover::new("branch-picker", |open| Button::new("t").label("main").pressed(open).into_any_element())
//!     .width(PopoverWidth::Sm)
//!     .content(|window, cx| div().child("…").into_any_element())
//! ```
//!
//! The content closure re-runs every time the popover renders, so read state from
//! entities inside it. [`close_popups`] closes it from inside.

use std::rc::Rc;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, Context, ElementId,
    FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    ParentElement, Pixels, Render, RenderOnce, SharedString, StatefulInteractiveElement as _,
    Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_base::{Align, Placement};

use super::bounds_probe;
use super::popup::{PopupAnchor, PopupLayer};
use crate::theme::{ActiveTheme as _, TextExt as _, motion, radius};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PopoverWidth {
    /// Fits its content.
    #[default]
    Auto,
    /// 256
    Sm,
    /// 320
    Md,
    /// 384
    Lg,
    Px(u32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PopoverPadding {
    /// 16 all round.
    #[default]
    Default,
    /// 8 vertical, 12 horizontal.
    Compact,
    None,
}

type Content = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

/// Closes whatever popup (menu, popover, select) is open.
pub fn close_popups(window: &mut Window, cx: &mut App) {
    PopupLayer::close(window, cx);
}

pub struct PopoverView {
    content: Content,
    width: PopoverWidth,
    padding: PopoverPadding,
    tooltip_style: bool,
    title: Option<SharedString>,
    focus: FocusHandle,
}

impl Focusable for PopoverView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PopoverView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.tern_theme().clone();
        let t = theme.colors;
        let (r, pad_x, pad_y) = if self.tooltip_style {
            (radius::MD, 8., 4.)
        } else {
            match self.padding {
                PopoverPadding::Default => (radius::LG, 16., 16.),
                PopoverPadding::Compact => (radius::LG, 12., 8.),
                PopoverPadding::None => (radius::LG, 0., 0.),
            }
        };
        let shadow = if self.tooltip_style {
            theme.shadows.md(0.05)
        } else {
            theme.shadows.popup()
        };
        let content = (self.content)(window, cx);
        div()
            .id("popover")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    PopupLayer::close(window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .relative()
            .flex()
            .flex_col()
            .rounded(r)
            .border_1()
            .border_color(theme.glass.dropdown_border)
            .bg(theme.glass.dropdown)
            .shadow(shadow)
            .text_color(t.popover_foreground)
            .font_sans()
            .when(self.tooltip_style, |el| {
                el.text_size(px(12.)).line_height(px(16.))
            })
            .px(px(pad_x))
            .py(px(pad_y))
            .max_w(px(1600.))
            .map(|el| match self.width {
                PopoverWidth::Auto => el,
                PopoverWidth::Sm => el.w(px(256.)),
                PopoverWidth::Md => el.w(px(320.)),
                PopoverWidth::Lg => el.w(px(384.)),
                PopoverWidth::Px(w) => el.w(px(w as f32)),
            })
            .child(super::edge_line(r - px(1.), cx))
            .when_some(self.title.clone(), |el, title| {
                el.child(
                    div()
                        .font_semibold()
                        .text_size(px(14.))
                        .line_height(px(14.))
                        .mb(px(8.))
                        .child(title),
                )
            })
            .child(content)
            .with_animation(
                "popover-enter",
                Animation::new(motion::DEFAULT).with_easing(motion::ease_standard()),
                |el, t| el.opacity(t),
            )
    }
}

#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    trigger: Box<dyn FnOnce(bool) -> AnyElement>,
    content: Option<Content>,
    width: PopoverWidth,
    padding: PopoverPadding,
    align: Align,
    placement: Placement,
    tooltip_style: bool,
    title: Option<SharedString>,
}

impl Popover {
    pub fn new(
        id: impl Into<ElementId>,
        trigger: impl FnOnce(bool) -> AnyElement + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            trigger: Box::new(trigger),
            content: None,
            width: PopoverWidth::Auto,
            padding: PopoverPadding::Default,
            align: Align::Center,
            placement: Placement::Bottom,
            tooltip_style: false,
            title: None,
        }
    }

    pub fn content(
        mut self,
        content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.content = Some(Rc::new(content));
        self
    }

    pub fn width(mut self, width: PopoverWidth) -> Self {
        self.width = width;
        self
    }

    pub fn padding(mut self, padding: PopoverPadding) -> Self {
        self.padding = padding;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    /// `tooltipStyle`: radius 6, text 12, `shadow-md/5`, padding 4/8.
    pub fn tooltip_style(mut self) -> Self {
        self.tooltip_style = true;
        self
    }

    /// Title row: 14px semibold, line-height 1.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }
}

impl RenderOnce for Popover {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let open = PopupLayer::is_open(&self.id, cx);
        let bounds = Rc::new(std::cell::Cell::new(gpui::Bounds::<Pixels>::default()));
        let click_bounds = bounds.clone();
        let owner = self.id.clone();
        let content = self
            .content
            .unwrap_or_else(|| Rc::new(|_, _| div().into_any_element()));
        let (width, padding, align, placement, tooltip_style, title) = (
            self.width,
            self.padding,
            self.align,
            self.placement,
            self.tooltip_style,
            self.title,
        );
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .child((self.trigger)(open))
            .child(bounds_probe(bounds))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.prevent_default();
                let anchor = PopupAnchor::Trigger {
                    bounds: click_bounds.get(),
                    placement,
                    align,
                    offset: px(4.),
                };
                let content = content.clone();
                let title = title.clone();
                PopupLayer::toggle(
                    owner.clone(),
                    anchor,
                    move |_, cx| {
                        let view = cx.new(|cx| PopoverView {
                            content,
                            width,
                            padding,
                            tooltip_style,
                            title,
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

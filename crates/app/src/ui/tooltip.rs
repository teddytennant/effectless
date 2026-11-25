//! Anchored tooltips (`tooltip.tsx`, `shared-tooltip.tsx`, spec 01 §4.9).
//!
//! GPUI's built-in tooltip follows the mouse; T3's sit on a side of the trigger.
//! Attach one to any stateful element:
//!
//! ```ignore
//! div().id("toggle").t3_tooltip(Tooltip::text("Toggle main sidebar").kbd("Ctrl+B").side(TooltipSide::Bottom))
//! ```
//!
//! Timing: opens after 600ms of rest (`.delay()` to change; the sidebar uses 150ms),
//! reopens instantly within 400ms of another tooltip closing, closes on mouse down.
//! The window has one [`TooltipLayer`], mounted by the shell.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, Bounds, Context, Div, Entity,
    Global, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Render,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled, Task, Window, deferred, div,
    prelude::FluentBuilder as _, px,
};
use gpui_base::{Align, Placement, Positioner};

use super::bounds_probe;
use crate::theme::{ActiveTheme as _, TextExt as _, leading, motion, radius};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TooltipSide {
    /// Base UI's default.
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TooltipVariant {
    /// 1px border, popover fill, `shadow-md/5`, edge line.
    #[default]
    Default,
    /// `dropdown-glass` + `shadow-xl` black/25.
    Glass,
    /// Mono 11px, max width 480, left aligned.
    Code,
}

#[derive(Clone)]
enum Content {
    Text(SharedString),
    Custom(Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>),
}

/// What a trigger shows. Cheap to clone.
#[derive(Clone)]
pub struct Tooltip {
    content: Content,
    secondary: Option<SharedString>,
    kbd: Option<SharedString>,
    side: TooltipSide,
    variant: TooltipVariant,
    delay: Duration,
}

impl Tooltip {
    pub fn text(text: impl Into<SharedString>) -> Self {
        Self {
            content: Content::Text(text.into()),
            secondary: None,
            kbd: None,
            side: TooltipSide::Top,
            variant: TooltipVariant::Default,
            delay: motion::TOOLTIP_DELAY,
        }
    }

    pub fn custom(build: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> Self {
        Self {
            content: Content::Custom(Rc::new(build)),
            ..Self::text("")
        }
    }

    /// A second, muted line (the sidebar's "New thread" tooltip).
    pub fn secondary(mut self, text: impl Into<SharedString>) -> Self {
        self.secondary = Some(text.into());
        self
    }

    /// Shortcut label appended in parentheses, T3 style: "Toggle main sidebar (Ctrl+B)".
    pub fn kbd(mut self, label: impl Into<SharedString>) -> Self {
        self.kbd = Some(label.into());
        self
    }

    pub fn side(mut self, side: TooltipSide) -> Self {
        self.side = side;
        self
    }

    pub fn variant(mut self, variant: TooltipVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    fn render_popup(&self, window: &mut Window, cx: &mut App) -> Div {
        let t = *cx.tokens();
        let theme = cx.tern_theme().clone();
        let code = self.variant == TooltipVariant::Code;
        let (text_size, line_height, max_w) = if code {
            (11., 11. * leading::RELAXED, 480.)
        } else {
            (12., 12. * leading::SNUG, 320.)
        };
        let mut el = div()
            .relative()
            .max_w(px(max_w))
            .px(px(8.))
            .py(px(4.))
            .rounded(radius::MD)
            .text_size(px(text_size))
            .line_height(px(line_height))
            .text_color(t.popover_foreground)
            .when(code, |el| el.font_mono())
            .when(!code, |el| el.font_sans());
        el = match self.variant {
            TooltipVariant::Glass => el
                .bg(theme.glass.dropdown)
                .border_1()
                .border_color(theme.glass.dropdown_border)
                .shadow(theme.shadows.xl(0.25)),
            _ => el
                .bg(t.popover)
                .border_1()
                .border_color(t.border)
                .shadow(theme.shadows.md(0.05))
                .child(super::edge_line(radius::MD - px(1.), cx)),
        };
        let main: AnyElement = match &self.content {
            Content::Text(text) => {
                let text = match &self.kbd {
                    Some(kbd) => SharedString::from(format!("{text} ({kbd})")),
                    None => text.clone(),
                };
                div().child(text).into_any_element()
            }
            Content::Custom(build) => build(window, cx),
        };
        el.child(main)
            .when_some(self.secondary.clone(), |el, line| {
                el.child(div().text_color(t.muted_foreground).child(line))
            })
    }
}

struct Shown {
    tooltip: Tooltip,
    anchor: Bounds<Pixels>,
    epoch: usize,
    animate: bool,
}

/// The window's tooltip host. Created by the shell; triggers find it through a global.
pub struct TooltipLayer {
    shown: Option<Shown>,
    pending: Option<(Bounds<Pixels>, Task<()>)>,
    last_hidden: Option<Instant>,
    epoch: usize,
}

struct GlobalTooltipLayer(Entity<TooltipLayer>);
impl Global for GlobalTooltipLayer {}

impl TooltipLayer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let entity = cx.entity();
        cx.set_global(GlobalTooltipLayer(entity));
        Self {
            shown: None,
            pending: None,
            last_hidden: None,
            epoch: 0,
        }
    }

    fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalTooltipLayer>().map(|g| g.0.clone())
    }

    fn hover(
        &mut self,
        tooltip: Tooltip,
        anchor: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let warm = self.shown.is_some()
            || self
                .last_hidden
                .is_some_and(|at| at.elapsed() < motion::TOOLTIP_REOPEN_WINDOW);
        if warm || tooltip.delay.is_zero() {
            self.show(tooltip, anchor, !warm, cx);
            return;
        }
        let delay = tooltip.delay;
        let task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, cx| {
                this.pending = None;
                this.show(tooltip, anchor, true, cx);
            });
        });
        self.pending = Some((anchor, task));
    }

    fn show(
        &mut self,
        tooltip: Tooltip,
        anchor: Bounds<Pixels>,
        animate: bool,
        cx: &mut Context<Self>,
    ) {
        self.epoch += 1;
        self.shown = Some(Shown {
            tooltip,
            anchor,
            epoch: self.epoch,
            animate,
        });
        cx.notify();
    }

    fn unhover(&mut self, anchor: Bounds<Pixels>, cx: &mut Context<Self>) {
        if self.pending.as_ref().is_some_and(|(a, _)| *a == anchor) {
            self.pending = None;
        }
        if self.shown.as_ref().is_some_and(|s| s.anchor == anchor) {
            self.shown = None;
            self.last_hidden = Some(Instant::now());
            cx.notify();
        }
    }

    /// Hides whatever is showing (route changes, drags).
    pub fn hide_all(&mut self, cx: &mut Context<Self>) {
        self.pending = None;
        if self.shown.take().is_some() {
            self.last_hidden = Some(Instant::now());
            cx.notify();
        }
    }
}

impl Render for TooltipLayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shown) = &self.shown else {
            return div().into_any_element();
        };
        let placement = match shown.tooltip.side {
            TooltipSide::Top => Placement::Top,
            TooltipSide::Bottom => Placement::Bottom,
            TooltipSide::Left => Placement::Left,
            TooltipSide::Right => Placement::Right,
        };
        let popup = shown.tooltip.render_popup(window, cx);
        let popup: AnyElement = if shown.animate {
            popup
                .with_animation(
                    ("tooltip-enter", shown.epoch),
                    Animation::new(motion::DEFAULT).with_easing(motion::ease_standard()),
                    |el, t| el.opacity(t),
                )
                .into_any_element()
        } else {
            popup.into_any_element()
        };
        deferred(
            Positioner::side(shown.anchor)
                .placement(placement)
                .align(Align::Center)
                .offset(px(4.))
                .margin(px(5.))
                .child(popup),
        )
        .with_priority(3)
        .into_any_element()
    }
}

/// Attaches an anchored tooltip to a stateful element.
pub trait TooltipExt: Sized {
    fn t3_tooltip(self, tooltip: Tooltip) -> Self;
}

impl TooltipExt for Stateful<Div> {
    fn t3_tooltip(self, tooltip: Tooltip) -> Self {
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let hover_bounds = bounds.clone();
        let down_bounds = bounds.clone();
        self.child(bounds_probe(bounds))
            .on_hover(move |hovered, window, cx| {
                let Some(layer) = TooltipLayer::global(cx) else {
                    return;
                };
                let anchor = hover_bounds.get();
                let tooltip = tooltip.clone();
                layer.update(cx, |layer, cx| {
                    if *hovered {
                        layer.hover(tooltip, anchor, window, cx);
                    } else {
                        layer.unhover(anchor, cx);
                    }
                });
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                if let Some(layer) = TooltipLayer::global(cx) {
                    let anchor = down_bounds.get();
                    layer.update(cx, |layer, cx| layer.unhover(anchor, cx));
                }
            })
    }
}

/// Creates the window's tooltip layer entity (the shell renders it last).
pub fn new_layer(cx: &mut App) -> Entity<TooltipLayer> {
    cx.new(TooltipLayer::new)
}

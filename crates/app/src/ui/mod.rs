//! T3's `components/ui` primitives as GPUI components (spec 01 §4).
//!
//! Every component reads its colors from `cx.tokens()` / `cx.states()` at render
//! time, so it follows the theme, the appearance contrast and the sidebar scope.
//! Sizes are the desktop (`sm:`) values from the spec.
//!
//! Stateless controls (`Button`, `Badge`, `Switch`, `Checkbox`, `Toggle`, …) are
//! `RenderOnce` builders: the caller owns the value and passes an `on_click` /
//! `on_change`. Overlays (`tooltip`, `menu`, `popover`, `select`, `dialog`, `toast`)
//! are hosted by the shell; see each module.

pub mod badge;
pub mod button;
pub mod checkbox;
pub mod dialog;
pub mod grain;
pub mod icon;
pub mod input;
pub mod kbd;
pub mod label;
pub mod menu;
pub mod popover;
pub mod popup;
pub mod scroll;
pub mod select;
pub mod separator;
pub mod skeleton;
pub mod spinner;
pub mod switch;
pub mod toast;
pub mod toggle;
pub mod tooltip;

pub use badge::{Badge, BadgeSize, BadgeVariant};
pub use button::{Button, ButtonSize, ButtonVariant, InlineButton, InlineTone};
pub use checkbox::{CheckState, Checkbox};
pub use dialog::{AlertDialog, ConfirmOptions, ConfirmVariant, Dialog, DialogFooter, Dialogs};
pub use grain::Grain;
pub use icon::Icon;
pub use input::{InputSize, TextField, TextareaField};
pub use kbd::{Kbd, KbdGroup};
pub use label::Label;
pub use menu::{DropdownMenu, Menu, MenuItem};
pub use popover::{Popover, PopoverPadding, PopoverWidth};
pub use scroll::{ScrollFade, scroll_fade};
pub use select::{Select, SelectOption, SelectSize, SelectVariant};
pub use separator::Separator;
pub use skeleton::{Skeleton, SkeletonShape};
pub use spinner::{Spinner, SpinnerSize};
pub use switch::{Switch, SwitchSize};
pub use toast::{Toast, ToastKind, Toasts};
pub use toggle::{Toggle, ToggleGroup, ToggleGroupVariant, ToggleSize, ToggleVariant};
pub use tooltip::{Tooltip, TooltipExt, TooltipSide, TooltipVariant};

pub use crate::assets::IconName;

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Bounds, BoxShadow, Div, Hsla, IntoElement, Pixels, Styled, canvas, div,
    point, px,
};

use crate::theme::ActiveTheme as _;

/// A click handler shared by builders (`on_click`).
pub type ClickHandler = Rc<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut App)>;
/// A callback with window access (menu selections, toast actions).
pub type WindowHandler = Rc<dyn Fn(&mut gpui::Window, &mut App)>;
/// Builds an element on demand (custom menu rows, tooltip content).
pub type ElementBuilder = Rc<dyn Fn(&mut gpui::Window, &mut App) -> gpui::AnyElement>;

/// Every disabled control paints at 64% opacity and takes no pointer events.
pub const DISABLED_OPACITY: f32 = 0.64;

/// The edge line (spec 01 §0): a 1px light over the control's border, on the bottom
/// edge in light mode (black/4%) and the top edge in dark mode (white/6%). Add it as
/// the last child of a `relative()` control with a 1px border; it covers the
/// border box and follows `radius`.
pub fn edge_line(radius: Pixels, cx: &App) -> Div {
    let (color, top) = cx.tern_theme().shadows.edge_line();
    let line = div()
        .absolute()
        .top(px(-1.))
        .left(px(-1.))
        .right(px(-1.))
        .bottom(px(-1.))
        .rounded(radius)
        .border_color(color);
    if top {
        line.border_t_1()
    } else {
        line.border_b_1()
    }
}

/// `focus-visible:ring-2 ring-inset ring-offset-1 ring-offset-background`: inside the
/// border, a 1px band of background, then 2px of ring.
pub fn focus_ring(cx: &App) -> Vec<BoxShadow> {
    let t = cx.tokens();
    vec![inset_band(1., t.background), inset_band(3., t.ring)]
}

/// The field ring (inputs, select triggers): a 3px inset band of ring/24.
pub fn field_ring(color: Hsla) -> Vec<BoxShadow> {
    vec![inset_band(3., color)]
}

fn inset_band(width: f32, color: Hsla) -> BoxShadow {
    BoxShadow {
        color,
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(width),
        inset: true,
    }
}

/// An invisible child that records its parent's bounds every frame. Give it a
/// `relative()` parent. Used by anchored overlays (tooltips, menus, popovers).
pub fn bounds_probe(cell: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(move |bounds, _, _| cell.set(bounds), |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

/// `div().flex().items_center()`.
pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

/// `div().flex().flex_col()`.
pub fn v_flex() -> Div {
    div().flex().flex_col()
}

/// Shorthand for an inline row of children with a gap.
pub fn row(gap: f32) -> Div {
    h_flex().gap(px(gap))
}

/// Applies a caller's `StyleRefinement` on top of a component's own styles.
pub trait Refine: Styled + Sized {
    fn refine(mut self, style: &gpui::StyleRefinement) -> Self {
        gpui::Refineable::refine(self.style(), style);
        self
    }
}

impl<T: Styled> Refine for T {}

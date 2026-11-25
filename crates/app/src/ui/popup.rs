//! The window's popup host: menus, popovers and select lists (z 130 in T3).
//!
//! One popup is open at a time, which is how T3 behaves (opening a menu closes the
//! previous one). Components open their content through [`PopupLayer::open`]; the
//! shell renders the layer. Clicking outside the popup (and outside its trigger)
//! closes it, and the click still reaches what is under the pointer.

use std::time::Instant;

use gpui::{
    Anchor, AnyView, App, AppContext as _, Bounds, Context, ElementId, Entity, FocusHandle, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, Render, Window,
    deferred, div, px,
};
use gpui_base::{Align, Placement, Positioner};

/// Where a popup goes.
#[derive(Clone, Copy, Debug)]
pub enum PopupAnchor {
    /// On a side of the trigger (Base UI `side`/`align`/`sideOffset`), flipping when
    /// it does not fit.
    Trigger {
        bounds: Bounds<Pixels>,
        placement: Placement,
        align: Align,
        offset: Pixels,
    },
    /// Top-left corner at a point (context menus, the select list aligned over its
    /// trigger).
    Point {
        position: Point<Pixels>,
        /// The trigger, which does not count as "outside".
        trigger: Option<Bounds<Pixels>>,
    },
}

impl PopupAnchor {
    pub fn below(bounds: Bounds<Pixels>, align: Align) -> Self {
        Self::Trigger {
            bounds,
            placement: Placement::Bottom,
            align,
            offset: px(4.),
        }
    }

    fn trigger_bounds(&self) -> Option<Bounds<Pixels>> {
        match self {
            Self::Trigger { bounds, .. } => Some(*bounds),
            Self::Point { trigger, .. } => *trigger,
        }
    }
}

struct Open {
    owner: ElementId,
    anchor: PopupAnchor,
    view: AnyView,
    restore_focus: Option<FocusHandle>,
}

pub struct PopupLayer {
    open: Option<Open>,
    closed: Option<(ElementId, Instant)>,
}

struct GlobalPopupLayer(Entity<PopupLayer>);
impl Global for GlobalPopupLayer {}

impl PopupLayer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let entity = cx.entity();
        cx.set_global(GlobalPopupLayer(entity));
        Self {
            open: None,
            closed: None,
        }
    }

    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalPopupLayer>().map(|g| g.0.clone())
    }

    /// Whether the popup opened by `owner` is showing (for a trigger's pressed look).
    pub fn is_open(owner: &ElementId, cx: &App) -> bool {
        Self::global(cx).is_some_and(|layer| {
            layer
                .read(cx)
                .open
                .as_ref()
                .is_some_and(|open| &open.owner == owner)
        })
    }

    /// Opens `view` for `owner`, replacing any open popup. `focus` is focused now and
    /// the previously focused element gets focus back on close.
    pub fn open(
        owner: ElementId,
        anchor: PopupAnchor,
        view: AnyView,
        focus: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(layer) = Self::global(cx) else {
            return;
        };
        let restore_focus = window.focused(cx);
        if let Some(focus) = &focus {
            focus.focus(window, cx);
        }
        layer.update(cx, |layer, cx| {
            layer.open = Some(Open {
                owner,
                anchor,
                view,
                restore_focus,
            });
            cx.notify();
        });
    }

    /// Toggles: closes when `owner`'s popup is open, else opens it. A trigger that
    /// was just closed by its own mouse-down does not reopen on the same click.
    pub fn toggle(
        owner: ElementId,
        anchor: PopupAnchor,
        build: impl FnOnce(&mut Window, &mut App) -> (AnyView, Option<FocusHandle>),
        window: &mut Window,
        cx: &mut App,
    ) {
        if Self::is_open(&owner, cx) {
            Self::close(window, cx);
            return;
        }
        let (view, focus) = build(window, cx);
        Self::open(owner, anchor, view, focus, window, cx);
    }

    pub fn close(window: &mut Window, cx: &mut App) {
        let Some(layer) = Self::global(cx) else {
            return;
        };
        let restore = layer.update(cx, |layer, cx| {
            let open = layer.open.take()?;
            layer.closed = Some((open.owner, Instant::now()));
            cx.notify();
            open.restore_focus
        });
        if let Some(focus) = restore {
            focus.focus(window, cx);
        }
    }
}

impl Render for PopupLayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(open) = &self.open else {
            return div().into_any_element();
        };
        let trigger = open.anchor.trigger_bounds();
        let positioner = match open.anchor {
            PopupAnchor::Trigger {
                bounds,
                placement,
                align,
                offset,
            } => Positioner::side(bounds)
                .placement(placement)
                .align(align)
                .offset(offset),
            PopupAnchor::Point { position, .. } => Positioner::corner(Anchor::TopLeft, position),
        };
        let layer = cx.entity().downgrade();
        deferred(
            positioner.margin(px(8.)).occlude().child(
                div()
                    .on_mouse_down_out(move |event, window, cx| {
                        if trigger.is_some_and(|b| b.contains(&event.position)) {
                            return;
                        }
                        if layer.upgrade().is_some() {
                            PopupLayer::close(window, cx);
                        }
                    })
                    .child(open.view.clone()),
            ),
        )
        .with_priority(1)
        .into_any_element()
    }
}

/// Creates the window's popup layer entity (the shell renders it).
pub fn new_layer(cx: &mut App) -> Entity<PopupLayer> {
    cx.new(PopupLayer::new)
}

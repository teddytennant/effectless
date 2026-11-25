//! Menus (`menu.tsx`, spec 01 §4.7): dropdown and context menus.
//!
//! ```ignore
//! let menu = |_: &mut Window, _: &mut App| {
//!     Menu::new()
//!         .item(MenuItem::new("Rename").icon(IconName::Pencil).shortcut("F2").on_select(|w, cx| …))
//!         .separator()
//!         .item(MenuItem::new("Delete").icon(IconName::Trash2).destructive().on_select(…))
//! };
//! DropdownMenu::new("thread-menu", menu, |open| {
//!     Button::new("more").variant(ButtonVariant::Ghost).size(ButtonSize::IconSm)
//!         .icon(IconName::Ellipsis).pressed(open).into_any_element()
//! })
//! // or on any stateful element:
//! div().id("row").t3_context_menu(menu)
//! ```
//!
//! Menus appear and vanish with no animation, as in T3. Keyboard: Up/Down move,
//! Enter selects, Right/Left open and close submenus, Escape closes.

use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, Div, ElementId, Entity, FocusHandle, Focusable,
    Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels,
    Render, RenderOnce, SharedString, Stateful, StatefulInteractiveElement as _, Styled, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_base::{Align, Placement};

use super::kbd::shortcut_text;
use super::popup::{PopupAnchor, PopupLayer};
use super::{Icon, IconName, bounds_probe};
use crate::theme::{ActiveTheme as _, TextExt as _, radius};

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Kind {
    #[default]
    Action,
    Checkbox(bool),
    Switch(bool),
    Radio(bool),
}

/// One row of a menu.
#[derive(Clone)]
pub enum MenuItem {
    Item(Box<MenuRow>),
    Separator,
    /// Group label: 12px medium muted-foreground, padding 6/8.
    Label {
        text: SharedString,
        inset: bool,
    },
    Submenu {
        row: Box<MenuRow>,
        menu: Rc<Menu>,
    },
    Custom(Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>),
}

#[derive(Clone, Default)]
pub struct MenuRow {
    label: SharedString,
    icon: Option<IconName>,
    shortcut: Option<SharedString>,
    destructive: bool,
    disabled: bool,
    inset: bool,
    kind: Kind,
    handler: Option<Handler>,
}

impl MenuItem {
    pub fn new(label: impl Into<SharedString>) -> MenuRowBuilder {
        MenuRowBuilder(MenuRow {
            label: label.into(),
            ..Default::default()
        })
    }

    /// A checkbox item: mark column 16px, padding-left 8, right 16.
    pub fn checkbox(label: impl Into<SharedString>, checked: bool) -> MenuRowBuilder {
        MenuRowBuilder(MenuRow {
            label: label.into(),
            kind: Kind::Checkbox(checked),
            ..Default::default()
        })
    }

    /// `variant="switch"`: label then a mini switch.
    pub fn switch(label: impl Into<SharedString>, checked: bool) -> MenuRowBuilder {
        MenuRowBuilder(MenuRow {
            label: label.into(),
            kind: Kind::Switch(checked),
            ..Default::default()
        })
    }

    /// A radio item: checked bg foreground/8, check icon 14px at the end.
    pub fn radio(label: impl Into<SharedString>, checked: bool) -> MenuRowBuilder {
        MenuRowBuilder(MenuRow {
            label: label.into(),
            kind: Kind::Radio(checked),
            ..Default::default()
        })
    }

    pub fn separator() -> Self {
        Self::Separator
    }

    pub fn label(text: impl Into<SharedString>) -> Self {
        Self::Label {
            text: text.into(),
            inset: false,
        }
    }

    pub fn submenu(label: impl Into<SharedString>, icon: Option<IconName>, menu: Menu) -> Self {
        Self::Submenu {
            row: Box::new(MenuRow {
                label: label.into(),
                icon,
                ..Default::default()
            }),
            menu: Rc::new(menu),
        }
    }

    pub fn custom(build: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> Self {
        Self::Custom(Rc::new(build))
    }

    fn row(&self) -> Option<&MenuRow> {
        match self {
            Self::Item(row) | Self::Submenu { row, .. } => Some(row),
            _ => None,
        }
    }

    fn selectable(&self) -> bool {
        self.row().is_some_and(|r| !r.disabled)
    }
}

/// Builder for an item row; converts into [`MenuItem`].
pub struct MenuRowBuilder(MenuRow);

impl MenuRowBuilder {
    pub fn icon(mut self, icon: IconName) -> Self {
        self.0.icon = Some(icon);
        self
    }

    /// Right-aligned shortcut text (12px medium `secondary-label`).
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.0.shortcut = Some(shortcut.into());
        self
    }

    /// Text `destructive-foreground`, icon follows it.
    pub fn destructive(mut self) -> Self {
        self.0.destructive = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.0.disabled = disabled;
        self
    }

    /// `inset`: padding-left 32 (aligns with items that have icons).
    pub fn inset(mut self) -> Self {
        self.0.inset = true;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> MenuItem {
        self.0.handler = Some(Rc::new(handler));
        MenuItem::Item(Box::new(self.0))
    }
}

impl From<MenuRowBuilder> for MenuItem {
    fn from(b: MenuRowBuilder) -> Self {
        MenuItem::Item(Box::new(b.0))
    }
}

/// The contents of a menu.
#[derive(Clone)]
pub struct Menu {
    items: Vec<MenuItem>,
    min_width: Pixels,
    fit_content: bool,
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            min_width: px(160.),
            fit_content: false,
        }
    }

    pub fn item(mut self, item: impl Into<MenuItem>) -> Self {
        self.items.push(item.into());
        self
    }

    pub fn items(mut self, items: impl IntoIterator<Item = MenuItem>) -> Self {
        self.items.extend(items);
        self
    }

    pub fn separator(self) -> Self {
        self.item(MenuItem::Separator)
    }

    pub fn label(self, text: impl Into<SharedString>) -> Self {
        self.item(MenuItem::label(text))
    }

    /// `width="content"`: no 160px minimum.
    pub fn fit_content(mut self) -> Self {
        self.fit_content = true;
        self
    }

    pub fn min_width(mut self, width: impl Into<Pixels>) -> Self {
        self.min_width = width.into();
        self
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// An open menu popup.
pub struct MenuView {
    menu: Menu,
    focus: FocusHandle,
    highlighted: Option<usize>,
    submenu: Option<(usize, Entity<MenuView>)>,
    parent: Option<gpui::WeakEntity<MenuView>>,
}

impl MenuView {
    pub fn new(menu: Menu, cx: &mut Context<Self>) -> Self {
        Self {
            menu,
            focus: cx.focus_handle(),
            highlighted: None,
            submenu: None,
            parent: None,
        }
    }

    fn move_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.menu.items.len() as isize;
        if n == 0 {
            return;
        }
        let mut ix = self
            .highlighted
            .map(|i| i as isize)
            .unwrap_or(if delta > 0 { -1 } else { n });
        for _ in 0..n {
            ix = (ix + delta).rem_euclid(n);
            if self.menu.items[ix as usize].selectable() {
                self.highlighted = Some(ix as usize);
                cx.notify();
                return;
            }
        }
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        match self.menu.items.get(ix) {
            Some(MenuItem::Item(row)) if !row.disabled => {
                let handler = row.handler.clone();
                PopupLayer::close(window, cx);
                if let Some(handler) = handler {
                    handler(window, cx);
                }
            }
            Some(MenuItem::Submenu { row, menu }) if !row.disabled => {
                self.open_submenu(ix, menu.clone(), window, cx);
            }
            _ => {}
        }
    }

    fn open_submenu(
        &mut self,
        ix: usize,
        menu: Rc<Menu>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.submenu.as_ref().is_some_and(|(open, _)| *open == ix) {
            return;
        }
        let parent = cx.entity().downgrade();
        let view = cx.new(|cx| {
            let mut v = MenuView::new((*menu).clone(), cx);
            v.parent = Some(parent);
            v
        });
        let _ = window;
        self.submenu = Some((ix, view));
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "down" => self.move_highlight(1, cx),
            "up" => self.move_highlight(-1, cx),
            "enter" | "space" => {
                if let Some(ix) = self.highlighted {
                    self.activate(ix, window, cx);
                    if let Some((_, sub)) = &self.submenu {
                        let focus = sub.read(cx).focus.clone();
                        focus.focus(window, cx);
                        sub.update(cx, |sub, cx| sub.move_highlight(1, cx));
                    }
                }
            }
            "right" => {
                if let Some(ix) = self.highlighted
                    && let Some(MenuItem::Submenu { menu, .. }) = self.menu.items.get(ix)
                {
                    let menu = menu.clone();
                    self.open_submenu(ix, menu, window, cx);
                    if let Some((_, sub)) = &self.submenu {
                        let focus = sub.read(cx).focus.clone();
                        focus.focus(window, cx);
                        sub.update(cx, |sub, cx| sub.move_highlight(1, cx));
                    }
                }
            }
            "left" => {
                if let Some(parent) = self.parent.as_ref().and_then(|p| p.upgrade()) {
                    parent.update(cx, |parent, cx| {
                        parent.submenu = None;
                        let focus = parent.focus.clone();
                        focus.focus(window, cx);
                        cx.notify();
                    });
                }
            }
            "escape" => PopupLayer::close(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Focusable for MenuView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The `dropdown-glass` popup surface shared by menus, selects and comboboxes:
/// radius 8, 1px foreground/10 border, popup shadow.
pub fn popup_surface(cx: &App) -> Div {
    let theme = cx.tern_theme();
    div()
        .flex()
        .flex_col()
        .rounded(radius::LG)
        .border_1()
        .border_color(theme.glass.dropdown_border)
        .bg(theme.glass.dropdown)
        .shadow(theme.shadows.popup())
        .text_color(theme.colors.popover_foreground)
        .font_sans()
}

/// The checkmark path from `menu.tsx` / `checkbox.tsx`, as an icon (lucide `check`
/// is the same shape at this size).
fn check_icon(size: f32, color: Hsla) -> impl IntoElement {
    Icon::new(IconName::Check).size(px(size)).color(color)
}

/// Menu item min height 28, padding 4/8, gap 8, radius 4, text 14.
pub(crate) fn item_base(highlighted: bool, cx: &App) -> Div {
    let t = cx.tokens();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .min_h(px(28.))
        .py(px(4.))
        .px(px(8.))
        .rounded(radius::SM)
        .text_size(px(14.))
        .line_height(px(20.))
        .text_color(if highlighted {
            t.accent_foreground
        } else {
            t.foreground
        })
        .when(highlighted, |el| el.bg(t.accent))
}

/// Mini switch inside a menu item: track 14x22, thumb 12, checked travel 8.
fn mini_switch(checked: bool, cx: &App) -> impl IntoElement {
    let t = cx.tokens();
    div()
        .flex_none()
        .w(px(22.))
        .h(px(14.))
        .p(px(1.))
        .rounded(radius::FULL)
        .bg(if checked { t.primary } else { t.input })
        .child(
            div()
                .size(px(12.))
                .rounded(radius::FULL)
                .bg(t.background)
                .ml(px(if checked { 8. } else { 0. })),
        )
}

impl Render for MenuView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        let s = *cx.states();
        let has_icons = self
            .menu
            .items
            .iter()
            .any(|i| i.row().is_some_and(|r| r.icon.is_some()));
        let mut list = div().flex().flex_col().p(px(4.));
        for (ix, item) in self.menu.items.iter().enumerate() {
            let highlighted = self.highlighted == Some(ix)
                || self.submenu.as_ref().is_some_and(|(open, _)| *open == ix);
            let el: AnyElement = match item {
                MenuItem::Separator => div()
                    .h(px(1.))
                    .my(px(4.))
                    .mx(px(-4.))
                    .bg(t.border)
                    .into_any_element(),
                MenuItem::Label { text, inset } => div()
                    .py(px(6.))
                    .px(px(8.))
                    .when(*inset || has_icons, |el| el.pl(px(32.)))
                    .font_medium()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.muted_foreground)
                    .child(text.clone())
                    .into_any_element(),
                MenuItem::Custom(build) => build(_window, cx),
                MenuItem::Item(row) | MenuItem::Submenu { row, .. } => {
                    let is_submenu = matches!(item, MenuItem::Submenu { .. });
                    let text_color = if row.destructive {
                        t.destructive_foreground
                    } else if highlighted {
                        t.accent_foreground
                    } else {
                        t.foreground
                    };
                    let icon_color = if row.destructive {
                        t.destructive_foreground
                    } else {
                        t.muted_foreground
                    };
                    let mut el = item_base(highlighted, cx)
                        .id(("menu-item", ix))
                        .text_color(text_color)
                        .when(row.inset, |el| el.pl(px(32.)))
                        .when(row.disabled, |el| el.opacity(super::DISABLED_OPACITY));
                    el = match row.kind {
                        Kind::Checkbox(checked) => el.pl(px(8.)).pr(px(16.)).child(
                            div()
                                .w(px(16.))
                                .flex_none()
                                .when(checked, |d| d.child(check_icon(16., t.foreground))),
                        ),
                        Kind::Radio(checked) => el.when(checked, |el| el.bg(s.selected)),
                        _ => el,
                    };
                    el = el
                        .when_some(row.icon, |el, icon| {
                            el.child(
                                Icon::new(icon)
                                    .size(px(16.))
                                    .color(icon_color)
                                    .opacity(if row.destructive { 1. } else { 0.8 }),
                            )
                        })
                        .child(div().flex_1().min_w_0().child(row.label.clone()))
                        .when_some(row.shortcut.clone(), |el, sc| {
                            el.child(shortcut_text(sc, cx))
                        })
                        .when(matches!(row.kind, Kind::Radio(true)), |el| {
                            el.child(check_icon(14., t.foreground))
                        })
                        .when_some(
                            match row.kind {
                                Kind::Switch(c) => Some(c),
                                _ => None,
                            },
                            |el, c| el.child(mini_switch(c, cx)),
                        )
                        .when(is_submenu, |el| {
                            el.child(
                                Icon::new(IconName::ChevronRight)
                                    .size(px(16.))
                                    .color(t.foreground)
                                    .opacity(0.8),
                            )
                        });
                    if !row.disabled {
                        el = el
                            .on_mouse_move(cx.listener(move |this, _, window, cx| {
                                if this.highlighted != Some(ix) {
                                    this.highlighted = Some(ix);
                                    if let Some(MenuItem::Submenu { menu, .. }) =
                                        this.menu.items.get(ix)
                                    {
                                        let menu = menu.clone();
                                        this.open_submenu(ix, menu, window, cx);
                                    } else {
                                        this.submenu = None;
                                    }
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.activate(ix, window, cx);
                            }));
                    }
                    let probe = std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
                    let sub = self
                        .submenu
                        .as_ref()
                        .filter(|(open, _)| *open == ix)
                        .map(|(_, v)| v.clone());
                    let item_bounds = probe.clone();
                    el.relative()
                        .child(bounds_probe(probe))
                        .when_some(sub, move |el, sub| {
                            el.child(SubmenuAnchor {
                                bounds: item_bounds,
                                view: sub,
                            })
                        })
                        .into_any_element()
                }
            };
            list = list.child(el);
        }
        popup_surface(cx)
            .id("menu")
            .track_focus(&self.focus)
            .key_context("Menu")
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(!self.menu.fit_content, |el| el.min_w(self.menu.min_width))
            .max_w(px(480.))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered && this.submenu.is_none() {
                    this.highlighted = None;
                    cx.notify();
                }
            }))
            .child(list)
    }
}

/// Positions an open submenu at the inline end of its item, align-offset -5px.
#[derive(IntoElement)]
struct SubmenuAnchor {
    bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<Pixels>>>,
    view: Entity<MenuView>,
}

impl RenderOnce for SubmenuAnchor {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut b = self.bounds.get();
        b.origin.y -= px(5.);
        gpui::deferred(
            gpui_base::Positioner::side(b)
                .placement(Placement::Right)
                .align(Align::Start)
                .offset(px(0.))
                .margin(px(8.))
                .occlude()
                .child(self.view),
        )
        .with_priority(2)
    }
}

/// Opens `menu` for `owner` at an anchor. Use this from your own click handlers.
pub fn open_menu(
    owner: ElementId,
    anchor: PopupAnchor,
    menu: Menu,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| MenuView::new(menu, cx));
    let focus = view.read(cx).focus.clone();
    PopupLayer::open(owner, anchor, view.into(), Some(focus), window, cx);
}

type MenuBuilder = Rc<dyn Fn(&mut Window, &mut App) -> Menu>;

/// A trigger that opens a menu below itself (Base UI default: side bottom, align
/// center, offset 4; set `.align()` for start/end).
#[derive(IntoElement)]
pub struct DropdownMenu {
    id: ElementId,
    build: MenuBuilder,
    trigger: Box<dyn FnOnce(bool) -> AnyElement>,
    align: Align,
    placement: Placement,
}

impl DropdownMenu {
    pub fn new(
        id: impl Into<ElementId>,
        build: impl Fn(&mut Window, &mut App) -> Menu + 'static,
        trigger: impl FnOnce(bool) -> AnyElement + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            build: Rc::new(build),
            trigger: Box::new(trigger),
            align: Align::Center,
            placement: Placement::Bottom,
        }
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }
}

impl RenderOnce for DropdownMenu {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let open = PopupLayer::is_open(&self.id, cx);
        let bounds = Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
        let click_bounds = bounds.clone();
        let owner = self.id.clone();
        let (align, placement, build) = (self.align, self.placement, self.build);
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
                let build = build.clone();
                PopupLayer::toggle(
                    owner.clone(),
                    anchor,
                    move |window, cx| {
                        let menu = build(window, cx);
                        let view = cx.new(|cx| MenuView::new(menu, cx));
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

/// Right-click menus on any stateful element.
pub trait ContextMenuExt: Sized {
    /// `owner` identifies the menu (any id unique to this element).
    fn t3_context_menu(
        self,
        owner: impl Into<ElementId>,
        build: impl Fn(&mut Window, &mut App) -> Menu + 'static,
    ) -> Self;
}

impl ContextMenuExt for Stateful<Div> {
    fn t3_context_menu(
        self,
        owner: impl Into<ElementId>,
        build: impl Fn(&mut Window, &mut App) -> Menu + 'static,
    ) -> Self {
        let owner: ElementId = owner.into();
        self.on_mouse_down(MouseButton::Right, move |event, window, cx| {
            let menu = build(window, cx);
            if menu.is_empty() {
                return;
            }
            open_menu(
                owner.clone(),
                PopupAnchor::Point {
                    position: event.position,
                    trigger: None,
                },
                menu,
                window,
                cx,
            );
            cx.stop_propagation();
        })
    }
}

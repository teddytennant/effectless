//! The settings layout primitives (spec 06 §1.5-1.8): section, row, reset button,
//! Advanced fold, slider, scope sentence and the small selects and inputs every page
//! uses.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ElementId, Hsla, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled, Window, canvas, div, prelude::FluentBuilder as _, px,
};
use gpui_base::Align;

use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, radius};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Icon, IconName, Select, SelectOption, SelectSize, Tooltip,
    bounds_probe, h_flex, v_flex,
};

pub type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// A 12px muted description, line height 1.5, max width 672 (`text-pretty`).
pub fn description(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    div()
        .max_w(px(672.))
        .text_size(px(12.))
        .line_height(px(18.))
        .text_color(cx.tokens().muted_foreground)
        .child(text.into())
}

/// `SettingsSection` (§1.5).
#[derive(IntoElement)]
pub struct Section {
    id: ElementId,
    title: Option<SharedString>,
    icon: Option<AnyElement>,
    title_action: Option<AnyElement>,
    header_action: Option<AnyElement>,
    plain: bool,
    children: Vec<AnyElement>,
}

impl Section {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: Some(title.into()),
            icon: None,
            title_action: None,
            header_action: None,
            plain: false,
            children: Vec::new(),
        }
    }

    /// A section whose title is hidden (no header row).
    pub fn untitled(id: impl Into<ElementId>) -> Self {
        Self {
            title: None,
            ..Self::new(id, "")
        }
    }

    pub fn icon(mut self, icon: impl IntoElement) -> Self {
        self.icon = Some(icon.into_any_element());
        self
    }

    pub fn title_action(mut self, el: impl IntoElement) -> Self {
        self.title_action = Some(el.into_any_element());
        self
    }

    pub fn header_action(mut self, el: impl IntoElement) -> Self {
        self.header_action = Some(el.into_any_element());
        self
    }

    /// `variant="plain"`: no card, children stacked 4px apart.
    pub fn plain(mut self) -> Self {
        self.plain = true;
        self
    }

    pub fn when_child(mut self, show: bool, f: impl FnOnce() -> AnyElement) -> Self {
        if show {
            self.children.push(f());
        }
        self
    }
}

impl ParentElement for Section {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

/// The grouped card: radius 12, border/60, card/40, shadow-xs/5, rows split by border/50.
pub fn card(children: Vec<AnyElement>, cx: &App) -> gpui::Div {
    let t = cx.tokens();
    let theme = cx.tern_theme();
    let line = t.border.opacity(t.border.a * 0.5);
    let mut card = v_flex()
        .rounded(radius::XL)
        .border_1()
        .border_color(t.border.opacity(t.border.a * 0.6))
        .bg(t.card.opacity(0.4))
        .shadow(theme.shadows.xs5())
        .overflow_hidden();
    for (i, child) in children.into_iter().enumerate() {
        card = card.child(
            div()
                .when(i > 0, |el| el.border_t_1().border_color(line))
                .child(child),
        );
    }
    card
}

impl RenderOnce for Section {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.tokens();
        let header = self.title.map(|title| {
            h_flex()
                .min_h(px(28.))
                .px(px(16.))
                .justify_between()
                .flex_wrap()
                .gap(px(16.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .when_some(self.icon, |el, icon| el.child(icon))
                        .child(
                            div()
                                .text_scale(TextSize::Base)
                                .font_semibold()
                                .text_color(t.foreground)
                                .child(title),
                        )
                        .when_some(self.title_action, |el, a| el.child(a)),
                )
                .when_some(self.header_action, |el, a| {
                    el.child(h_flex().min_h(px(28.)).min_w(px(28.)).child(a))
                })
        });
        let body = if self.plain {
            v_flex().gap(px(4.)).children(self.children)
        } else {
            card(self.children, cx)
        };
        v_flex()
            .id(self.id)
            .gap(px(10.))
            .when_some(header, |el, h| el.child(h))
            .child(body)
    }
}

/// `SettingsRow` (§1.6).
#[derive(IntoElement)]
pub struct Row {
    id: ElementId,
    title: SharedString,
    leading: Option<AnyElement>,
    title_extra: Vec<AnyElement>,
    description: Option<AnyElement>,
    status: Option<AnyElement>,
    controls: Vec<AnyElement>,
    children: Vec<AnyElement>,
    reset: Option<(SharedString, Handler)>,
    disabled: bool,
    highlight: bool,
    probe: Option<Rc<Cell<Bounds<Pixels>>>>,
    bg: Option<Hsla>,
}

impl Row {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            leading: None,
            title_extra: Vec::new(),
            description: None,
            status: None,
            controls: Vec::new(),
            children: Vec::new(),
            reset: None,
            disabled: false,
            highlight: false,
            probe: None,
            bg: None,
        }
    }

    pub fn description(mut self, text: impl Into<SharedString>, cx: &App) -> Self {
        self.description = Some(description(text, cx).into_any_element());
        self
    }

    pub fn description_el(mut self, el: impl IntoElement) -> Self {
        self.description = Some(el.into_any_element());
        self
    }

    pub fn status(mut self, el: impl IntoElement) -> Self {
        self.status = Some(el.into_any_element());
        self
    }

    /// An icon before the title.
    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    /// Badges after the title.
    pub fn title_extra(mut self, el: impl IntoElement) -> Self {
        self.title_extra.push(el.into_any_element());
        self
    }

    pub fn control(mut self, el: impl IntoElement) -> Self {
        self.controls.push(el.into_any_element());
        self
    }

    /// Full-width content below the grid (lists, textareas).
    pub fn below(mut self, el: impl IntoElement) -> Self {
        self.children.push(el.into_any_element());
        self
    }

    /// Shows the reset button when `differs`; `label` is the row title for aria.
    pub fn reset_when(
        mut self,
        differs: bool,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        if differs {
            self.reset = Some((self.title.clone(), Rc::new(f)));
        }
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// The search target pulse (inset 2px primary/70 ring).
    pub fn highlight(mut self, on: bool) -> Self {
        self.highlight = on;
        self
    }

    /// Records the row's window bounds (scroll-to-target).
    pub fn probe(mut self, cell: Option<Rc<Cell<Bounds<Pixels>>>>) -> Self {
        self.probe = cell;
        self
    }

    pub fn bg(mut self, color: Hsla) -> Self {
        self.bg = Some(color);
        self
    }
}

pub fn reset_button(id: impl Into<ElementId>, handler: Handler) -> Button {
    Button::new(id)
        .variant(ButtonVariant::GhostMuted)
        .size(ButtonSize::IconMicro)
        .icon(IconName::Undo2)
        .tooltip(Tooltip::text("Reset to default"))
        .on_click(move |_, window, cx| handler(window, cx))
}

impl RenderOnce for Row {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.tokens();
        let has_children = !self.children.is_empty();
        let title_color = if self.disabled {
            t.muted_foreground
        } else {
            t.foreground
        };
        let reset_id = SharedString::from(format!("{:?}-reset", self.id));
        let left = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(4.))
            .child(
                h_flex()
                    .min_h(px(20.))
                    .gap(px(6.))
                    .when_some(self.leading, |el, l| el.child(l))
                    .child(
                        div()
                            .text_scale(TextSize::Sm)
                            .font_medium()
                            .text_color(title_color)
                            .child(self.title),
                    )
                    .children(self.title_extra)
                    .when_some(self.reset, |el, (_, handler)| {
                        el.child(reset_button(reset_id, handler))
                    }),
            )
            .when_some(self.description, |el, d| el.child(d))
            .when_some(self.status, |el, s| {
                el.child(
                    div()
                        .pt(px(2.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(t.muted_foreground)
                        .child(s),
                )
            });
        let grid = h_flex()
            .gap(px(32.))
            .child(left)
            .when(!self.controls.is_empty(), |el| {
                el.child(
                    h_flex()
                        .flex_none()
                        .gap(px(8.))
                        .justify_end()
                        .children(self.controls),
                )
            });
        v_flex()
            .id(self.id)
            .relative()
            .px(px(16.))
            .pt(px(12.))
            .pb(px(if has_children { 4. } else { 12. }))
            .when_some(self.bg, |el, bg| el.bg(bg))
            .when(self.disabled, |el| el.opacity(0.64))
            .child(grid)
            .children(self.children)
            .when(self.highlight, |el| {
                el.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .rounded(radius::XL)
                        .border_2()
                        .border_color(t.primary.opacity(0.7)),
                )
            })
            .when_some(self.probe, |el, cell| el.child(bounds_probe(cell)))
    }
}

/// The `Advanced` fold trigger (§1.6): min height 40, 12px medium muted, chevron 14.
pub fn advanced_trigger(
    id: impl Into<ElementId>,
    label: &'static str,
    open: bool,
    toggle: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let t = cx.tokens();
    let fg = t.foreground;
    h_flex()
        .id(id.into())
        .min_h(px(40.))
        .px(px(16.))
        .gap(px(6.))
        .text_size(px(12.))
        .line_height(px(16.))
        .font_medium()
        .text_color(t.muted_foreground)
        .cursor_pointer()
        .hover(move |s| s.text_color(fg))
        .on_click(move |_, window, cx| toggle(window, cx))
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size(px(14.)),
        )
        .child(label)
        .into_any_element()
}

/// A settings select: size sm, given width, popup aligned to the trigger's end.
pub fn select(
    id: impl Into<ElementId>,
    options: Vec<(SharedString, SharedString)>,
    selected: impl Into<SharedString>,
    width: f32,
    on_change: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> Select {
    Select::new(
        id,
        options
            .into_iter()
            .map(|(v, l)| SelectOption::new(v, l))
            .collect(),
    )
    .size(SelectSize::Sm)
    .width(px(width))
    .popup_align(Align::End)
    .selected(selected)
    .on_change(move |v, window, cx| on_change(v.as_ref(), window, cx))
}

/// Options from `(stored, label)` string pairs.
pub fn opts(pairs: &[(&'static str, &'static str)]) -> Vec<(SharedString, SharedString)> {
    pairs
        .iter()
        .map(|(v, l)| (SharedString::from(*v), SharedString::from(*l)))
        .collect()
}

/// Slider + readout (§9.2): track and a `{n}%` pill in a 208px row, gap 12.
#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: u32,
    min: u32,
    max: u32,
    step: u32,
    suffix: &'static str,
    readout_width: f32,
    on_change: Option<Rc<dyn Fn(u32, &mut Window, &mut App)>>,
}

impl Slider {
    pub fn new(id: impl Into<ElementId>, value: u32, min: u32, max: u32, step: u32) -> Self {
        Self {
            id: id.into(),
            value,
            min,
            max,
            step,
            suffix: "%",
            readout_width: 48.,
            on_change: None,
        }
    }

    pub fn suffix(mut self, suffix: &'static str, readout_width: f32) -> Self {
        self.suffix = suffix;
        self.readout_width = readout_width;
        self
    }

    pub fn on_change(mut self, f: impl Fn(u32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

/// Snaps a pointer fraction (0..=1) to a stepped value.
pub fn slider_value(fraction: f32, min: u32, max: u32, step: u32) -> u32 {
    let f = fraction.clamp(0., 1.);
    let raw = min as f32 + f * (max - min) as f32;
    let steps = ((raw - min as f32) / step.max(1) as f32).round() as u32;
    (min + steps * step.max(1)).min(max)
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.tokens();
        let theme = cx.tern_theme().clone();
        let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
        let dragging = window.use_keyed_state(
            SharedString::from(format!("{:?}-drag", self.id)),
            cx,
            |_, _| false,
        );
        let (min, max, step) = (self.min, self.max, self.step);
        let fraction = if max > min {
            (self.value.clamp(min, max) - min) as f32 / (max - min) as f32
        } else {
            0.
        };
        let set = {
            let bounds = bounds.clone();
            let on_change = self.on_change.clone();
            let current = self.value;
            move |x: Pixels, window: &mut Window, cx: &mut App| {
                let b = bounds.get();
                if b.size.width <= px(0.) {
                    return;
                }
                let f = (x - b.origin.x) / b.size.width;
                let v = slider_value(f, min, max, step);
                if v != current
                    && let Some(cb) = &on_change
                {
                    cb(v, window, cx);
                }
            }
        };
        let set = Rc::new(set);
        let thumb = 16.;
        let track = div()
            .id(self.id.clone())
            .relative()
            .flex_1()
            .h(px(16.))
            .cursor_pointer()
            .child(bounds_probe(bounds.clone()))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(6.))
                    .h(px(4.))
                    .rounded(radius::FULL)
                    .bg(t.input),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(6.))
                    .h(px(4.))
                    .w(gpui::relative(fraction))
                    .rounded(radius::FULL)
                    .bg(t.primary),
            )
            .child(
                // The thumb's left edge travels 0..(width - thumb).
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right(px(thumb))
                    .h_full()
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left(gpui::relative(fraction))
                            .size(px(thumb))
                            .rounded(radius::FULL)
                            .border_2()
                            .border_color(t.primary)
                            .bg(t.background)
                            .shadow(theme.shadows.sm(0.05)),
                    ),
            )
            .on_mouse_down(MouseButton::Left, {
                let set = set.clone();
                let dragging = dragging.clone();
                move |e: &MouseDownEvent, window, cx| {
                    dragging.update(cx, |d, _| *d = true);
                    set(e.position.x, window, cx);
                    cx.stop_propagation();
                }
            })
            .child({
                let set = set.clone();
                let dragging = dragging.clone();
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let set = set.clone();
                        let d = dragging.clone();
                        window.on_mouse_event(move |e: &MouseMoveEvent, _, window, cx| {
                            if *d.read(cx) {
                                if e.pressed_button == Some(MouseButton::Left) {
                                    set(e.position.x, window, cx);
                                } else {
                                    d.update(cx, |d, _| *d = false);
                                }
                            }
                        });
                        let d = dragging.clone();
                        window.on_mouse_event(move |_: &MouseUpEvent, _, _, cx| {
                            if *d.read(cx) {
                                d.update(cx, |d, _| *d = false);
                            }
                        });
                    },
                )
                .absolute()
                .size_0()
            });
        h_flex()
            .w(px(208.))
            .gap(px(12.))
            .child(
                div()
                    .flex_none()
                    .min_w(px(self.readout_width))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(radius::MD)
                    .bg(t.muted)
                    .font_mono()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.foreground)
                    .flex()
                    .justify_center()
                    .child(format!("{}{}", self.value, self.suffix)),
            )
            .child(track)
    }
}

#[cfg(test)]
mod tests {
    use super::slider_value;

    #[test]
    fn slider_snaps_to_steps() {
        assert_eq!(slider_value(0., 50, 200, 5), 50);
        assert_eq!(slider_value(1., 50, 200, 5), 200);
        assert_eq!(slider_value(0.5, 50, 200, 5), 125);
        assert_eq!(slider_value(0.34, 40, 100, 5), 60);
        assert_eq!(slider_value(-1., 0, 400, 25), 0);
        assert_eq!(slider_value(2., 0, 400, 25), 400);
    }
}

//! The font family picker (spec 06 §9.3): a trigger plus a searchable list of the
//! system's fonts, each row drawn in its own font, the default family first.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels, Render, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_base::{Align, Placement};
use gpui_component::input::{Input, InputEvent, InputState};

use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, radius};
use crate::ui::menu::{item_base, popup_surface};
use crate::ui::popup::{PopupAnchor, PopupLayer};
use crate::ui::{Icon, IconName, bounds_probe, edge_line, h_flex, v_flex};

type OnPick = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// True when `i` and `W` advance the same at 16px.
pub fn is_monospace(family: &str, cx: &App) -> bool {
    let ts = cx.text_system();
    let id = ts.resolve_font(&gpui::font(SharedString::from(family.to_string())));
    let (Ok(i), Ok(w)) = (ts.advance(id, px(16.), 'i'), ts.advance(id, px(16.), 'W')) else {
        return false;
    };
    (i.width - w.width).abs() < px(0.01)
}

/// Families matching `query`, case-insensitive, sorted, deduplicated.
pub fn filter_fonts(all: &[String], query: &str) -> Vec<String> {
    let q = query.trim().to_lowercase();
    let mut out: Vec<String> = all
        .iter()
        .filter(|f| !f.starts_with('.') && (q.is_empty() || f.to_lowercase().contains(&q)))
        .cloned()
        .collect();
    out.sort_by_key(|f| f.to_lowercase());
    out.dedup();
    out
}

pub struct FontList {
    fonts: Vec<String>,
    default_family: SharedString,
    selected: String,
    query: Entity<InputState>,
    highlighted: usize,
    on_pick: OnPick,
    focus: FocusHandle,
    _sub: Subscription,
}

impl Focusable for FontList {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl FontList {
    fn visible(&self, cx: &App) -> Vec<String> {
        let q = self.query.read(cx).value().to_string();
        let mut out = Vec::new();
        if q.trim().is_empty() {
            out.push(String::new());
        }
        out.extend(filter_fonts(&self.fonts, &q));
        out
    }

    fn pick(&mut self, family: String, window: &mut Window, cx: &mut Context<Self>) {
        let f = self.on_pick.clone();
        PopupLayer::close(window, cx);
        f(family, window, cx);
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.visible(cx).len();
        match e.keystroke.key.as_str() {
            "down" if n > 0 => self.highlighted = (self.highlighted + 1) % n,
            "up" if n > 0 => self.highlighted = (self.highlighted + n - 1) % n,
            "enter" => {
                if let Some(f) = self.visible(cx).get(self.highlighted).cloned() {
                    self.pick(f, window, cx);
                }
                return;
            }
            "escape" => {
                PopupLayer::close(window, cx);
                cx.stop_propagation();
                return;
            }
            _ => return,
        }
        cx.notify();
        cx.stop_propagation();
    }
}

impl Render for FontList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        let s = *cx.states();
        let fonts = self.visible(cx);
        let highlighted = self.highlighted.min(fonts.len().saturating_sub(1));
        let rows = fonts.into_iter().enumerate().map(|(ix, family)| {
            let label: SharedString = if family.is_empty() {
                self.default_family.clone()
            } else {
                family.clone().into()
            };
            let selected = family == self.selected;
            let draw_family = label.clone();
            let f = family.clone();
            item_base(ix == highlighted, cx)
                .id(("font-row", ix))
                .when(selected && ix != highlighted, |el| el.bg(s.selected))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .font_family(draw_family)
                        .child(label),
                )
                .when(family.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.muted_foreground)
                            .child("default"),
                    )
                })
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.highlighted != ix {
                        this.highlighted = ix;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.pick(f.clone(), window, cx)))
        });
        let list: Vec<_> = rows.collect();
        let empty = list.is_empty();
        popup_surface(cx)
            .id("font-list")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(288.))
            .child(
                h_flex()
                    .h(px(36.))
                    .px(px(10.))
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(t.border)
                    .child(
                        Icon::new(IconName::Search)
                            .size(px(14.))
                            .color(t.muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&self.query).appearance(false).text_size(px(14.))),
                    ),
            )
            .child(
                v_flex()
                    .id("font-list-scroll")
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .p(px(4.))
                    .children(list)
                    .when(empty, |el| {
                        el.child(
                            div()
                                .py(px(16.))
                                .flex()
                                .justify_center()
                                .text_scale(TextSize::Sm)
                                .text_color(t.muted_foreground)
                                .child("No fonts found."),
                        )
                    }),
            )
    }
}

/// The trigger (min width 176, outline select look) that opens [`FontList`].
#[derive(IntoElement)]
pub struct FontPicker {
    id: ElementId,
    value: String,
    default_family: SharedString,
    monospace_only: bool,
    on_pick: OnPick,
}

impl FontPicker {
    pub fn new(
        id: impl Into<ElementId>,
        value: String,
        default_family: impl Into<SharedString>,
        on_pick: impl Fn(String, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            value,
            default_family: default_family.into(),
            monospace_only: false,
            on_pick: Rc::new(on_pick),
        }
    }

    pub fn monospace_only(mut self) -> Self {
        self.monospace_only = true;
        self
    }
}

impl RenderOnce for FontPicker {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.tokens();
        let s = *cx.states();
        let theme = cx.tern_theme().clone();
        let open = PopupLayer::is_open(&self.id, cx);
        let bounds = Rc::new(std::cell::Cell::new(gpui::Bounds::<Pixels>::default()));
        let click = bounds.clone();
        let owner = self.id.clone();
        let label: SharedString = if self.value.is_empty() {
            self.default_family.clone()
        } else {
            self.value.clone().into()
        };
        let value = self.value.clone();
        let default_family = self.default_family.clone();
        let mono = self.monospace_only;
        let on_pick = self.on_pick.clone();
        h_flex()
            .id(self.id)
            .relative()
            .min_w(px(176.))
            .h(px(28.))
            .px(px(9.))
            .gap(px(6.))
            .rounded(radius::LG)
            .border_1()
            .border_color(if open { t.ring } else { t.input })
            .bg(s.field_fill)
            .shadow(theme.shadows.xs5())
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(t.foreground)
            .cursor_pointer()
            .child(bounds_probe(bounds))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_family(label.clone())
                    .child(label),
            )
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(px(14.))
                    .color(t.foreground)
                    .opacity(0.7),
            )
            .when(!open, |el| el.child(edge_line(radius::LG - px(1.), cx)))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.prevent_default();
                let anchor = PopupAnchor::Trigger {
                    bounds: click.get(),
                    placement: Placement::Bottom,
                    align: Align::End,
                    offset: px(4.),
                };
                let value = value.clone();
                let default_family = default_family.clone();
                let on_pick = on_pick.clone();
                PopupLayer::toggle(
                    owner.clone(),
                    anchor,
                    move |window, cx| {
                        let all = cx.text_system().all_font_names();
                        let fonts: Vec<String> = if mono {
                            all.into_iter().filter(|f| is_monospace(f, cx)).collect()
                        } else {
                            all
                        };
                        let view = cx.new(|cx| {
                            let query = cx
                                .new(|cx| InputState::new(window, cx).placeholder("Search fonts…"));
                            let sub = cx.subscribe(&query, |this: &mut FontList, _, ev, cx| {
                                if matches!(ev, InputEvent::Change) {
                                    this.highlighted = 0;
                                    cx.notify();
                                }
                            });
                            FontList {
                                fonts,
                                default_family,
                                selected: value,
                                query,
                                highlighted: 0,
                                on_pick,
                                focus: cx.focus_handle(),
                                _sub: sub,
                            }
                        });
                        let focus = view.read(cx).query.focus_handle(cx);
                        (view.into(), Some(focus))
                    },
                    window,
                    cx,
                );
                cx.stop_propagation();
            })
    }
}

#[cfg(test)]
mod tests {
    use super::filter_fonts;

    #[test]
    fn filters_case_insensitively_and_hides_dot_fonts() {
        let all = vec![
            "Inter".to_string(),
            ".SystemUI".to_string(),
            "JetBrains Mono".to_string(),
            "inter".to_string(),
        ];
        assert_eq!(
            filter_fonts(&all, "mono"),
            vec!["JetBrains Mono".to_string()]
        );
        assert_eq!(filter_fonts(&all, "").len(), 3);
    }
}

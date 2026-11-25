//! Toggle and ToggleGroup (`toggle.tsx`, `toggle-group.tsx`, spec 01 §4.16-4.17).
//!
//! The segmented ToggleGroup is T3's tab control:
//!
//! ```ignore
//! ToggleGroup::new("diff-scope")
//!     .item(Toggle::new("branch").label("Branch").pressed(scope == Branch).on_click(…))
//!     .item(Toggle::new("uncommitted").label("Uncommitted").pressed(…).on_click(…))
//! ```

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::{DISABLED_OPACITY, Icon, IconName, edge_line, focus_ring};
use crate::theme::{ActiveTheme as _, TextExt as _, radius};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToggleVariant {
    #[default]
    Default,
    Pill,
    Ghost,
    Outline,
    Segmented,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToggleSize {
    #[default]
    Default,
    Sm,
    Xs,
    Lg,
    Compact,
    Segmented,
}

#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<IconName>,
    children: Vec<AnyElement>,
    pressed: bool,
    disabled: bool,
    variant: ToggleVariant,
    size: ToggleSize,
    on_click: Option<super::ClickHandler>,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            children: Vec::new(),
            pressed: false,
            disabled: false,
            variant: ToggleVariant::Default,
            size: ToggleSize::Default,
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn variant(mut self, variant: ToggleVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ToggleSize) -> Self {
        self.size = size;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl ParentElement for Toggle {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *cx.tokens();
        let s = *cx.states();
        let theme = cx.tern_theme().clone();
        let dark = theme.is_dark();
        let (h, pad, text, lh, r, icon_px, min_w) = match self.size {
            ToggleSize::Default => (32., 7., 14., 20., radius::LG, 16., 32.),
            ToggleSize::Sm => (28., 5., 14., 20., radius::LG, 16., 28.),
            ToggleSize::Xs => (24., 3., 14., 20., radius::MD, 16., 24.),
            ToggleSize::Lg => (36., 9., 14., 20., radius::LG, 16., 36.),
            ToggleSize::Compact => (28., 3., 12., 16., radius::MD, 14., 28.),
            ToggleSize::Segmented => (24., 10., 12., 16., radius::MD, 14., 0.),
        };
        let transparent = gpui::transparent_black();
        let pressed = self.pressed;
        // (bg, fg, border) at rest and when hovered.
        let (rest, hover) = match self.variant {
            ToggleVariant::Default => (
                if pressed {
                    (s.toggle_pressed, t.accent_foreground, transparent)
                } else {
                    (transparent, t.foreground, transparent)
                },
                if pressed {
                    (s.toggle_pressed, t.accent_foreground, transparent)
                } else {
                    (t.accent, t.foreground, transparent)
                },
            ),
            ToggleVariant::Pill => {
                let fg = if pressed {
                    t.foreground
                } else {
                    t.muted_foreground
                };
                let bg = if pressed {
                    s.toggle_pressed
                } else {
                    transparent
                };
                (
                    (bg, fg, transparent),
                    (if pressed { bg } else { t.accent }, fg, transparent),
                )
            }
            ToggleVariant::Ghost => {
                let r = if pressed {
                    (t.accent, t.accent_foreground, transparent)
                } else {
                    (transparent, t.foreground, transparent)
                };
                (r, (t.accent, r.1, transparent))
            }
            ToggleVariant::Outline => {
                let bg = if pressed {
                    if dark { t.input } else { s.toggle_pressed }
                } else {
                    s.field_fill
                };
                let hover_bg = if pressed {
                    bg
                } else if dark {
                    s.toggle_pressed
                } else {
                    t.accent
                };
                (
                    (bg, t.foreground, t.input),
                    (hover_bg, t.foreground, t.input),
                )
            }
            ToggleVariant::Segmented => {
                if pressed {
                    let r = (s.segmented_pressed, t.foreground, transparent);
                    (r, r)
                } else {
                    (
                        (transparent, t.muted_foreground, transparent),
                        (s.segmented_hover, t.foreground, transparent),
                    )
                }
            }
        };
        let shadow = match self.variant {
            ToggleVariant::Outline => theme.shadows.xs5(),
            ToggleVariant::Segmented if pressed && !dark => {
                theme.shadows.xs(gpui::black().opacity(0.1))
            }
            _ => vec![],
        };
        let pad = if self.variant == ToggleVariant::Pill {
            12.
        } else {
            pad
        };
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let el = div()
            .id(self.id)
            .track_focus(&focus)
            .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                window.prevent_default()
            })
            .relative()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(if self.size == ToggleSize::Segmented {
                6.
            } else {
                8.
            }))
            .h(px(h))
            .min_w(px(min_w))
            .px(px(pad))
            .rounded(if self.variant == ToggleVariant::Pill {
                radius::FULL
            } else {
                r
            })
            .border_1()
            .border_color(rest.2)
            .bg(rest.0)
            .text_color(rest.1)
            .map(|el| {
                if self.variant == ToggleVariant::Pill {
                    el.font_weight(crate::theme::font::NORMAL)
                } else {
                    el.font_medium()
                }
            })
            .text_size(px(text))
            .line_height(px(lh))
            .whitespace_nowrap()
            .shadow(shadow)
            .when_some(self.icon, |el, icon| {
                el.child(Icon::new(icon).size(px(icon_px)).color(rest.1).opacity(0.8))
            })
            .when_some(self.label, |el, label| el.child(label))
            .children(self.children)
            .when(self.variant == ToggleVariant::Outline && !dark, |el| {
                el.child(edge_line(r - px(1.), cx))
            })
            .focus_visible(|st| st.shadow(focus_ring(cx)));
        if self.disabled {
            return if self.variant == ToggleVariant::Ghost {
                el.text_color(t.muted_foreground)
            } else {
                el.opacity(DISABLED_OPACITY)
            };
        }
        el.cursor_pointer()
            .hover(move |st| st.bg(hover.0).text_color(hover.1).border_color(hover.2))
            .when_some(self.on_click, |el, f| {
                el.on_click(move |e, w, cx| f(e, w, cx))
            })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToggleGroupVariant {
    /// A track (input/40, radius 8, padding 2, gap 2) holding 24px segmented items:
    /// 28px tall overall.
    #[default]
    Segmented,
    /// Gap 2, no track.
    Default,
    /// Items fused, inner radii and borders removed.
    Outline,
}

#[derive(IntoElement)]
pub struct ToggleGroup {
    variant: ToggleGroupVariant,
    items: Vec<Toggle>,
    full_width: bool,
}

impl ToggleGroup {
    pub fn new() -> Self {
        Self {
            variant: ToggleGroupVariant::Segmented,
            items: Vec::new(),
            full_width: false,
        }
    }

    pub fn variant(mut self, variant: ToggleGroupVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Items get the variant matching the group (segmented items for a segmented group).
    pub fn item(mut self, item: Toggle) -> Self {
        self.items.push(item);
        self
    }

    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }
}

impl Default for ToggleGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for ToggleGroup {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let s = *cx.states();
        let full = self.full_width;
        let items = self.items.into_iter().map(|item| match self.variant {
            ToggleGroupVariant::Segmented => item
                .variant(ToggleVariant::Segmented)
                .size(ToggleSize::Segmented),
            ToggleGroupVariant::Outline => item.variant(ToggleVariant::Outline),
            ToggleGroupVariant::Default => item,
        });
        let el = div().flex().flex_row().items_center().gap(px(2.));
        match self.variant {
            ToggleGroupVariant::Segmented => el
                .p(px(2.))
                .rounded(radius::LG)
                .bg(s.segmented_track)
                .when(full, |el| el.w_full())
                .children(items.map(|i| div().when(full, |d| d.flex_1()).child(i))),
            ToggleGroupVariant::Default => el.children(items),
            ToggleGroupVariant::Outline => el.gap(px(0.)).children(items),
        }
    }
}

//! Button and InlineButton (`button.tsx`, spec 01 §4.1).
//!
//! ```ignore
//! Button::new("save").label("Save").on_click(|_, window, cx| …)
//! Button::new("close").variant(ButtonVariant::Ghost).size(ButtonSize::IconSm)
//!     .icon(IconName::X).tooltip(Tooltip::text("Close"))
//! ```
//!
//! Not reproduced: the press `scale(0.97)` (GPUI has no element transforms).

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, FocusHandle, Hsla, InteractiveElement, IntoElement,
    ParentElement, Pixels, RenderOnce, SharedString, StatefulInteractiveElement, StyleRefinement,
    Styled, Window, div, prelude::FluentBuilder as _, px,
};

use super::{
    DISABLED_OPACITY, Icon, IconName, Refine as _, Spinner, Tooltip, TooltipExt as _, edge_line,
    focus_ring,
};
use crate::theme::{ActiveTheme as _, TextExt as _, radius};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    #[default]
    Default,
    Destructive,
    Outline,
    DestructiveOutline,
    Secondary,
    Ghost,
    GhostMuted,
    GhostDestructive,
    Glass,
    Link,
    WarningOutline,
    Overlay,
    MediaClose,
    MediaNavigation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonSize {
    #[default]
    Default,
    Xs,
    Sm,
    Lg,
    Xl,
    Compact,
    Micro,
    Multiline,
    SmMultiline,
    SmPill,
    Icon,
    IconSm,
    IconSmPill,
    IconXs,
    IconLg,
    IconXl,
    IconMicro,
    IconTiny,
}

/// Resolved geometry of a size (desktop values).
#[derive(Clone, Copy, Debug)]
pub struct SizeSpec {
    /// Fixed height, or min height for the multiline sizes.
    pub height: f32,
    /// Square controls set width = height.
    pub square: bool,
    pub padding_x: f32,
    pub padding_y: f32,
    pub gap: f32,
    pub text: f32,
    pub line_height: f32,
    pub icon: f32,
    pub radius: Pixels,
    pub wraps: bool,
}

impl ButtonSize {
    pub fn spec(self) -> SizeSpec {
        let base = SizeSpec {
            height: 32.,
            square: false,
            padding_x: 11.,
            padding_y: 0.,
            gap: 8.,
            text: 14.,
            line_height: 20.,
            icon: 16.,
            radius: radius::CONTROL,
            wraps: false,
        };
        let icon = |h: f32, icon: f32| SizeSpec {
            height: h,
            square: true,
            padding_x: 0.,
            icon,
            ..base
        };
        match self {
            Self::Default => base,
            Self::Xs => SizeSpec {
                height: 24.,
                padding_x: 7.,
                gap: 6.,
                text: 12.,
                line_height: 16.,
                icon: 14.,
                ..base
            },
            Self::Sm => SizeSpec {
                height: 28.,
                padding_x: 9.,
                gap: 6.,
                ..base
            },
            Self::Lg => SizeSpec {
                height: 36.,
                padding_x: 13.,
                ..base
            },
            Self::Xl => SizeSpec {
                height: 40.,
                padding_x: 15.,
                text: 16.,
                line_height: 24.,
                icon: 18.,
                ..base
            },
            Self::Compact => SizeSpec {
                height: 28.,
                padding_x: 7.,
                gap: 6.,
                text: 12.,
                line_height: 16.,
                icon: 14.,
                radius: radius::MD,
                ..base
            },
            Self::Micro => SizeSpec {
                height: 20.,
                padding_x: 5.,
                gap: 6.,
                text: 11.,
                line_height: 16.,
                icon: 12.,
                radius: radius::SM,
                ..base
            },
            Self::Multiline => SizeSpec {
                padding_y: 5.,
                wraps: true,
                ..base
            },
            Self::SmMultiline => SizeSpec {
                height: 28.,
                padding_x: 9.,
                padding_y: 3.,
                gap: 6.,
                wraps: true,
                ..base
            },
            Self::SmPill => SizeSpec {
                height: 28.,
                padding_x: 10.,
                gap: 6.,
                radius: radius::FULL,
                ..base
            },
            Self::Icon => icon(32., 16.),
            Self::IconSm => icon(28., 16.),
            Self::IconSmPill => SizeSpec {
                radius: radius::FULL,
                ..icon(28., 16.)
            },
            Self::IconXs => icon(24., 14.),
            Self::IconLg => icon(36., 16.),
            Self::IconXl => icon(40., 18.),
            Self::IconMicro => SizeSpec {
                radius: radius::SM,
                ..icon(20., 12.)
            },
            Self::IconTiny => icon(16., 12.),
        }
    }
}

/// Colors of one variant in one state.
#[derive(Clone, Copy)]
struct Look {
    bg: Hsla,
    border: Hsla,
    text: Hsla,
    icon: Option<Hsla>,
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    variant: ButtonVariant,
    size: ButtonSize,
    label: Option<SharedString>,
    icon: Option<IconName>,
    icon_end: Option<IconName>,
    icon_size: Option<Pixels>,
    children: Vec<AnyElement>,
    disabled: bool,
    pressed: bool,
    loading: bool,
    full_width: bool,
    on_click: Option<super::ClickHandler>,
    tooltip: Option<Tooltip>,
    focus: Option<FocusHandle>,
    style: StyleRefinement,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            variant: ButtonVariant::Default,
            size: ButtonSize::Default,
            label: None,
            icon: None,
            icon_end: None,
            icon_size: None,
            children: Vec::new(),
            disabled: false,
            pressed: false,
            loading: false,
            full_width: false,
            on_click: None,
            tooltip: None,
            focus: None,
            style: StyleRefinement::default(),
        }
    }

    /// Uses this focus handle instead of an internal one, so the caller can move
    /// focus to the button (dialogs focus their first action).
    pub fn focus_handle(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Leading icon (or the only content for the `Icon*` sizes).
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Trailing icon, e.g. a chevron.
    pub fn icon_end(mut self, icon: IconName) -> Self {
        self.icon_end = Some(icon);
        self
    }

    /// Overrides the size's icon size (`size-3.5` etc. on the svg).
    pub fn icon_size(mut self, size: impl Into<Pixels>) -> Self {
        self.icon_size = Some(size.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// `data-pressed`: the menu or popover this button opens is open. Paints the
    /// hover look.
    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    /// Replaces the leading icon with a Spinner and disables the button.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn tooltip(mut self, tooltip: Tooltip) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    fn looks(&self, cx: &App) -> (Look, Look, Look) {
        let t = cx.tokens();
        let s = cx.states();
        let glass = cx.tern_theme().glass;
        let transparent = gpui::transparent_black();
        let rest = |bg: Hsla, border: Hsla, text: Hsla, icon: Option<Hsla>| Look {
            bg,
            border,
            text,
            icon,
        };
        let muted_icon = Some(t.muted_foreground);
        match self.variant {
            ButtonVariant::Default => {
                let r = rest(t.primary, t.primary, t.primary_foreground, None);
                let h = Look {
                    bg: s.primary_hover,
                    ..r
                };
                (r, h, h)
            }
            ButtonVariant::Destructive => {
                let r = rest(t.destructive, t.destructive, gpui::white(), None);
                let h = Look {
                    bg: s.destructive_hover,
                    ..r
                };
                (r, h, h)
            }
            ButtonVariant::Outline => {
                let r = rest(s.outline_fill, t.input, t.foreground, muted_icon);
                let h = Look {
                    bg: s.outline_hover,
                    ..r
                };
                (r, h, h)
            }
            ButtonVariant::DestructiveOutline => {
                let r = rest(s.outline_fill, t.input, t.destructive_foreground, None);
                let h = Look {
                    bg: s.destructive_outline_hover_bg,
                    border: s.destructive_outline_hover_border,
                    ..r
                };
                (r, h, h)
            }
            ButtonVariant::Secondary => {
                let r = rest(t.secondary, transparent, t.secondary_foreground, None);
                (
                    r,
                    Look {
                        bg: s.secondary_hover,
                        ..r
                    },
                    Look {
                        bg: s.secondary_active,
                        ..r
                    },
                )
            }
            ButtonVariant::Ghost => {
                let r = rest(transparent, transparent, t.foreground, muted_icon);
                let h = Look { bg: t.accent, ..r };
                (r, h, h)
            }
            ButtonVariant::GhostMuted => {
                let r = rest(transparent, transparent, t.muted_foreground, None);
                let h = Look {
                    bg: t.accent,
                    text: t.foreground,
                    ..r
                };
                (r, h, h)
            }
            ButtonVariant::GhostDestructive => {
                let r = rest(transparent, transparent, t.muted_foreground, None);
                let h = Look {
                    bg: t.accent,
                    text: t.destructive,
                    ..r
                };
                (r, h, h)
            }
            ButtonVariant::Glass => {
                let r = rest(glass.surface, s.glass_border, t.foreground, muted_icon);
                let h = Look {
                    border: t.border,
                    ..r
                };
                (r, h, h)
            }
            ButtonVariant::Link => {
                let r = rest(transparent, transparent, t.foreground, None);
                (r, r, r)
            }
            ButtonVariant::WarningOutline => {
                let r = rest(
                    t.warning_surface,
                    s.warning_border,
                    t.warning_foreground,
                    None,
                );
                let h = Look {
                    bg: s.warning_hover,
                    border: s.warning_border_hover,
                    ..r
                };
                (r, h, h)
            }
            ButtonVariant::Overlay => {
                let r = rest(
                    gpui::black().opacity(0.7),
                    transparent,
                    gpui::white().opacity(0.65),
                    None,
                );
                let h = Look {
                    bg: gpui::black().opacity(0.9),
                    ..r
                };
                (r, h, h)
            }
            ButtonVariant::MediaClose => {
                let r = rest(
                    gpui::black().opacity(0.65),
                    transparent,
                    gpui::white(),
                    None,
                );
                let h = Look {
                    bg: gpui::black().opacity(0.8),
                    ..r
                };
                (r, h, h)
            }
            ButtonVariant::MediaNavigation => {
                let r = rest(transparent, transparent, gpui::white().opacity(0.9), None);
                let h = Look {
                    bg: gpui::white().opacity(0.1),
                    text: gpui::white(),
                    ..r
                };
                (r, h, h)
            }
        }
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

const GROUP: &str = "tern-button";

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let spec = self.size.spec();
        let (rest, hover, active) = self.looks(cx);
        let disabled = self.disabled || self.loading;
        let interactive = !disabled;
        let shadows = cx.tern_theme().shadows;
        let s = *cx.states();
        let show_hover = self.pressed;
        let base = if show_hover { hover } else { rest };
        let icon_px = px(self.icon_size.map(f32::from).unwrap_or(spec.icon));
        let focus = match self.focus.clone() {
            Some(focus) => focus,
            None => window
                .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone(),
        };

        // Shadows per variant (rest; hover keeps them; active and pressed drop the
        // drop shadow).
        let (rest_shadow, active_shadow) = match self.variant {
            ButtonVariant::Default | ButtonVariant::Destructive => {
                let color = if self.variant == ButtonVariant::Default {
                    s.primary_shadow
                } else {
                    s.destructive_shadow
                };
                let mut rest = vec![shadows.control_highlight()];
                rest.extend(shadows.xs(color));
                (rest, vec![shadows.control_pressed()])
            }
            ButtonVariant::Outline
            | ButtonVariant::DestructiveOutline
            | ButtonVariant::WarningOutline => (shadows.xs5(), vec![]),
            ButtonVariant::Glass | ButtonVariant::MediaClose => (shadows.sm(0.1), vec![]),
            _ => (vec![], vec![]),
        };
        let rest_shadow = if disabled || self.pressed {
            if self.pressed {
                active_shadow.clone()
            } else {
                vec![]
            }
        } else {
            rest_shadow
        };
        let has_edge_line = matches!(
            self.variant,
            ButtonVariant::Outline | ButtonVariant::DestructiveOutline
        ) && !disabled
            && !self.pressed;

        let icon_color = |look: Look| look.icon.unwrap_or(look.text);
        let make_icon = |name: IconName| {
            Icon::new(name)
                .size(icon_px)
                .color(icon_color(base))
                .group_hover_color(GROUP, icon_color(hover))
        };
        let leading = if self.loading {
            Some(
                Spinner::new()
                    .size_px(icon_px)
                    .color(icon_color(base))
                    .into_any_element(),
            )
        } else {
            self.icon.map(|name| make_icon(name).into_any_element())
        };
        let trailing = self.icon_end.map(|name| make_icon(name).into_any_element());
        let has_text = self.label.is_some() || !self.children.is_empty();
        let icon_margin = if has_text { px(-2.) } else { px(0.) };

        let mut el = div()
            .id(self.id.clone())
            .group(GROUP)
            .relative()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(spec.gap))
            .rounded(spec.radius)
            .border_1()
            .border_color(base.border)
            .bg(base.bg)
            .text_color(base.text)
            .font_medium()
            .text_size(px(spec.text))
            .line_height(px(spec.line_height))
            .px(px(spec.padding_x))
            .shadow(rest_shadow)
            .cursor_pointer()
            .track_focus(&focus)
            .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                window.prevent_default()
            })
            .when(!spec.wraps, |el| el.whitespace_nowrap())
            .map(|el| {
                if spec.square {
                    el.size(px(spec.height))
                } else if spec.wraps {
                    el.min_h(px(spec.height)).py(px(spec.padding_y))
                } else {
                    el.h(px(spec.height))
                }
            })
            .when(self.full_width, |el| el.w_full())
            .when_some(leading, |el, icon| {
                el.child(div().flex().mx(icon_margin).child(icon))
            })
            .when_some(self.label, |el, label| el.child(label))
            .children(self.children)
            .when_some(trailing, |el, icon| {
                el.child(div().flex().mx(icon_margin).child(icon))
            })
            .when(has_edge_line, |el| {
                el.child(edge_line(spec.radius - px(1.), cx))
            });

        if interactive {
            let active_shadow_clone = active_shadow.clone();
            let link = self.variant == ButtonVariant::Link;
            el = el
                .hover(move |style| {
                    let style = style
                        .bg(hover.bg)
                        .border_color(hover.border)
                        .text_color(hover.text);
                    if link { style.underline() } else { style }
                })
                .active(move |style| {
                    style
                        .bg(active.bg)
                        .border_color(active.border)
                        .text_color(active.text)
                        .shadow(active_shadow_clone.clone())
                })
                .focus_visible(|style| style.shadow(focus_ring(cx)));
            if let Some(handler) = self.on_click {
                el = el.on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    handler(event, window, cx)
                });
            }
            if let Some(tooltip) = self.tooltip {
                el = el.t3_tooltip(tooltip);
            }
        } else {
            el = el.opacity(DISABLED_OPACITY).cursor_default();
        }

        el.refine(&self.style)
    }
}

/// Tones of [`InlineButton`] (`button.tsx:103-139`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InlineTone {
    #[default]
    Default,
    Muted,
    Destructive,
    /// Gap 4, underline offset 4, underlined while its popup is open.
    Picker,
}

/// Text-only button: medium weight, gap 2, underline on hover.
#[derive(IntoElement)]
pub struct InlineButton {
    id: ElementId,
    tone: InlineTone,
    label: SharedString,
    icon_end: Option<IconName>,
    open: bool,
    on_click: Option<super::ClickHandler>,
    style: StyleRefinement,
}

impl InlineButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            tone: InlineTone::Default,
            label: label.into(),
            icon_end: None,
            open: false,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn tone(mut self, tone: InlineTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn icon_end(mut self, icon: IconName) -> Self {
        self.icon_end = Some(icon);
        self
    }

    /// For the `Picker` tone: its popup is open.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl Styled for InlineButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for InlineButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.tokens();
        let (rest, hover) = match self.tone {
            InlineTone::Default | InlineTone::Picker => (t.foreground, t.foreground),
            InlineTone::Muted => (t.muted_foreground, t.foreground),
            InlineTone::Destructive => (t.destructive.opacity(0.8), t.destructive),
        };
        let gap = if self.tone == InlineTone::Picker {
            4.
        } else {
            2.
        };
        let underline_now = self.tone == InlineTone::Picker && self.open;
        div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(gap))
            .font_medium()
            .text_color(rest)
            .cursor_pointer()
            .when(underline_now, |el| el.underline())
            .hover(move |s| s.text_color(hover).underline())
            .child(self.label)
            .when_some(self.icon_end, |el, icon| {
                el.child(Icon::new(icon).size(px(14.)).color(rest))
            })
            .when_some(self.on_click, |el, handler| {
                el.on_click(move |e, w, cx| handler(e, w, cx))
            })
            .refine(&self.style)
    }
}

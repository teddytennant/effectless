//! Runtime modes ("Permissions", spec 04 §3.3) and the Permissions popover section
//! shared by the footer chip, the picker footer and the new-thread strip.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use tern_core::RuntimeMode;

use crate::assets::IconName;
use crate::theme::{ActiveTheme as _, TextExt as _, radius};
use crate::ui::popup::PopupLayer;
use crate::ui::{Icon, h_flex, v_flex};

pub const MODES: [RuntimeMode; 4] = [
    RuntimeMode::ApprovalRequired,
    RuntimeMode::AutoAcceptEdits,
    RuntimeMode::Auto,
    RuntimeMode::FullAccess,
];

/// The user's default (`full-access`); the footer chip shows only when a thread differs.
pub const DEFAULT_MODE: RuntimeMode = RuntimeMode::FullAccess;

pub fn label(mode: RuntimeMode) -> &'static str {
    match mode {
        RuntimeMode::ApprovalRequired => "Supervised",
        RuntimeMode::AutoAcceptEdits => "Auto-accept edits",
        RuntimeMode::Auto => "Auto",
        RuntimeMode::FullAccess => "Full access",
    }
}

pub fn description(mode: RuntimeMode) -> &'static str {
    match mode {
        RuntimeMode::ApprovalRequired => "Ask before commands and file changes.",
        RuntimeMode::AutoAcceptEdits => "Auto-approve edits, ask before other actions.",
        RuntimeMode::Auto => "Supported providers approve routine actions; others still ask.",
        RuntimeMode::FullAccess => "Allow commands and edits without prompts.",
    }
}

pub fn icon(mode: RuntimeMode) -> IconName {
    match mode {
        RuntimeMode::ApprovalRequired => IconName::Lock,
        RuntimeMode::AutoAcceptEdits => IconName::PenLine,
        RuntimeMode::Auto => IconName::ShieldCheck,
        RuntimeMode::FullAccess => IconName::ShieldOff,
    }
}

/// Picks a mode; the popup closes afterwards.
pub type OnPick = Rc<dyn Fn(RuntimeMode, &mut Window, &mut App)>;

/// The popover content: heading, segmented radio row, preview (width 288).
pub struct PermissionsView {
    current: RuntimeMode,
    preview: Option<RuntimeMode>,
    on_pick: OnPick,
    focus: FocusHandle,
}

impl PermissionsView {
    pub fn new(current: RuntimeMode, on_pick: OnPick, cx: &mut Context<Self>) -> Self {
        Self {
            current,
            preview: None,
            on_pick,
            focus: cx.focus_handle(),
        }
    }

    /// Opens the section in the window popup layer, anchored above `bounds`.
    pub fn open(
        owner: impl Into<gpui::ElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        current: RuntimeMode,
        on_pick: OnPick,
        window: &mut Window,
        cx: &mut App,
    ) {
        let owner = owner.into();
        let anchor = crate::ui::popup::PopupAnchor::Trigger {
            bounds,
            placement: gpui_base::Placement::Top,
            align: gpui_base::Align::End,
            offset: px(4.),
        };
        PopupLayer::toggle(
            owner,
            anchor,
            move |_, cx| {
                let view = cx.new(|cx| PermissionsView::new(current, on_pick, cx));
                let focus = view.read(cx).focus.clone();
                (view.into(), Some(focus))
            },
            window,
            cx,
        );
    }

    fn pick(&mut self, mode: RuntimeMode, window: &mut Window, cx: &mut Context<Self>) {
        (self.on_pick)(mode, window, cx);
        PopupLayer::close(window, cx);
    }
}

impl Focusable for PermissionsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PermissionsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.tern_theme().clone();
        let t = theme.colors;
        let s = *cx.states();
        let shown = self.preview.unwrap_or(self.current);
        let previewing = shown != self.current;
        let segments = MODES.iter().enumerate().map(|(i, &mode)| {
            let checked = mode == self.current;
            div()
                .id(("perm-seg", i))
                .flex_1()
                .h(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .when(i == 0, |el| el.rounded_l(radius::MD))
                .when(i == MODES.len() - 1, |el| el.rounded_r(radius::MD))
                .when(checked, |el| el.bg(t.input.opacity(0.72)))
                .cursor_pointer()
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    this.preview = if *hovered { Some(mode) } else { None };
                    cx.notify();
                }))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(move |this, _, window, cx| this.pick(mode, window, cx)))
                .child(Icon::new(icon(mode)).size(px(14.)).color(if checked {
                    t.foreground
                } else {
                    t.muted_foreground
                }))
        });
        v_flex()
            .id("permissions-popover")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                let idx = MODES
                    .iter()
                    .position(|m| *m == this.preview.unwrap_or(this.current))
                    .unwrap_or(0);
                match e.keystroke.key.as_str() {
                    "escape" => PopupLayer::close(window, cx),
                    "left" => {
                        this.preview = Some(MODES[idx.saturating_sub(1)]);
                        cx.notify();
                    }
                    "right" => {
                        this.preview = Some(MODES[(idx + 1).min(MODES.len() - 1)]);
                        cx.notify();
                    }
                    "enter" | "space" => {
                        let mode = MODES[idx];
                        this.pick(mode, window, cx);
                    }
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .relative()
            .w(px(288.))
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.glass.dropdown_border)
            .bg(theme.glass.dropdown)
            .shadow(theme.shadows.popup())
            .text_color(t.popover_foreground)
            .font_sans()
            .px(px(8.))
            .py(px(6.))
            .gap(px(6.))
            .child(crate::ui::edge_line(radius::LG - px(1.), cx))
            .child(
                div()
                    .text_scale(crate::theme::TextSize::Xs)
                    .font_medium()
                    .text_color(t.muted_foreground)
                    .px(px(4.))
                    .pt(px(2.))
                    .child("Permissions"),
            )
            .child(
                h_flex()
                    .rounded(radius::LG)
                    .bg(s.segmented_track)
                    .p(px(2.))
                    .children(segments),
            )
            .child(
                v_flex()
                    .px(px(4.))
                    .pb(px(4.))
                    .child(
                        div()
                            .text_scale(crate::theme::TextSize::Sm)
                            .text_color(if previewing {
                                t.muted_foreground
                            } else {
                                t.foreground
                            })
                            .child(SharedString::from(label(shown))),
                    )
                    .child(
                        div()
                            .min_h(px(32.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(t.muted_foreground)
                            .child(SharedString::from(description(shown))),
                    ),
            )
    }
}

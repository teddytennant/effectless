//! Toasts (`toast.tsx`, spec 01 §4.22, spec 02 §6.1).
//!
//! ```ignore
//! Toasts::push(Toast::error("Failed to rename thread").description(err.to_string()), cx);
//! Toasts::push(Toast::success("Project created").description(title)
//!     .action("Open", |window, cx| …), cx);
//! ```
//!
//! Top-right, 72px from the top on Linux, 32 from the right, max 360 wide. Collapsed,
//! older toasts peek 12px below the front one and shrink 10% each; hovering expands
//! the stack into a list with 12px gaps and pauses the 5s timeout.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, ClipboardItem, Context, Entity,
    Global, InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
};

use super::{Button, ButtonSize, ButtonVariant, Icon, IconName, Spinner, Tooltip};
use crate::theme::{ActiveTheme as _, TextExt as _, layout, motion, radius};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastKind {
    #[default]
    Default,
    Error,
    Info,
    Success,
    Warning,
    Loading,
}

type Action = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
struct ToastAction {
    label: SharedString,
    primary: bool,
    handler: Action,
}

#[derive(Clone)]
pub struct Toast {
    kind: ToastKind,
    title: SharedString,
    description: Option<SharedString>,
    actions: Vec<ToastAction>,
    timeout: Option<Duration>,
    thread_id: Option<SharedString>,
}

impl Toast {
    pub fn new(kind: ToastKind, title: impl Into<SharedString>) -> Self {
        Self {
            kind,
            title: title.into(),
            description: None,
            actions: Vec::new(),
            timeout: Some(motion::TOAST_TIMEOUT),
            thread_id: None,
        }
    }

    pub fn message(title: impl Into<SharedString>) -> Self {
        Self::new(ToastKind::Default, title)
    }
    pub fn error(title: impl Into<SharedString>) -> Self {
        Self::new(ToastKind::Error, title)
    }
    pub fn info(title: impl Into<SharedString>) -> Self {
        Self::new(ToastKind::Info, title)
    }
    pub fn success(title: impl Into<SharedString>) -> Self {
        Self::new(ToastKind::Success, title)
    }
    pub fn warning(title: impl Into<SharedString>) -> Self {
        Self::new(ToastKind::Warning, title)
    }
    pub fn loading(title: impl Into<SharedString>) -> Self {
        Self::new(ToastKind::Loading, title).sticky()
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// A secondary (`outline`, `xs`) action.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.actions.push(ToastAction {
            label: label.into(),
            primary: false,
            handler: Rc::new(f),
        });
        self
    }

    /// The primary (`default`, `xs`) action.
    pub fn primary_action(
        mut self,
        label: impl Into<SharedString>,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.actions.push(ToastAction {
            label: label.into(),
            primary: true,
            handler: Rc::new(f),
        });
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// `timeout: 0`: stays until dismissed.
    pub fn sticky(mut self) -> Self {
        self.timeout = None;
        self
    }

    /// Thread-scoped: shown only while that thread is the open route.
    pub fn thread(mut self, thread_id: impl Into<SharedString>) -> Self {
        self.thread_id = Some(thread_id.into());
        self
    }
}

struct Live {
    id: u64,
    toast: Toast,
}

/// The window's toast host.
pub struct Toasts {
    live: Vec<Live>,
    next_id: u64,
    expanded: bool,
    /// The open thread, for thread-scoped toasts (set by the shell).
    current_thread: Option<SharedString>,
}

struct GlobalToasts(Entity<Toasts>);
impl Global for GlobalToasts {}

impl Toasts {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let entity = cx.entity();
        cx.set_global(GlobalToasts(entity));
        Self {
            live: Vec::new(),
            next_id: 0,
            expanded: false,
            current_thread: None,
        }
    }

    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalToasts>().map(|g| g.0.clone())
    }

    /// Shows a toast. Returns its id for [`Toasts::dismiss`].
    pub fn push(toast: Toast, cx: &mut App) -> u64 {
        let Some(host) = Self::global(cx) else {
            return 0;
        };
        host.update(cx, |host, cx| {
            host.next_id += 1;
            let id = host.next_id;
            if let Some(timeout) = toast.timeout {
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(timeout).await;
                    loop {
                        let Ok(expanded) = this.update(cx, |host, _| host.expanded) else {
                            return;
                        };
                        if !expanded {
                            break;
                        }
                        cx.background_executor()
                            .timer(Duration::from_millis(500))
                            .await;
                    }
                    let _ = this.update(cx, |host, cx| host.remove(id, cx));
                })
                .detach();
            }
            host.live.insert(0, Live { id, toast });
            cx.notify();
            id
        })
    }

    pub fn dismiss(id: u64, cx: &mut App) {
        if let Some(host) = Self::global(cx) {
            host.update(cx, |host, cx| host.remove(id, cx));
        }
    }

    pub fn set_current_thread(&mut self, thread: Option<SharedString>, cx: &mut Context<Self>) {
        if self.current_thread != thread {
            self.current_thread = thread;
            cx.notify();
        }
    }

    fn remove(&mut self, id: u64, cx: &mut Context<Self>) {
        let before = self.live.len();
        self.live.retain(|l| l.id != id);
        if self.live.len() != before {
            if self.live.is_empty() {
                self.expanded = false;
            }
            cx.notify();
        }
    }

    fn render_toast(&self, live: &Live, hidden: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.tern_theme().clone();
        let t = theme.colors;
        let s = theme.states;
        let toast = &live.toast;
        let id = live.id;
        let icon: Option<AnyElement> = match toast.kind {
            ToastKind::Default => None,
            ToastKind::Error => Some(
                Icon::new(IconName::CircleAlert)
                    .color(t.destructive)
                    .into_any_element(),
            ),
            ToastKind::Info => Some(Icon::new(IconName::Info).color(t.info).into_any_element()),
            ToastKind::Success => Some(
                Icon::new(IconName::CircleCheck)
                    .color(t.success)
                    .into_any_element(),
            ),
            ToastKind::Warning => Some(
                Icon::new(IconName::TriangleAlert)
                    .color(t.warning)
                    .into_any_element(),
            ),
            ToastKind::Loading => Some(
                Spinner::new()
                    .color(t.popover_foreground.opacity(0.8))
                    .into_any_element(),
            ),
        };
        let copyable = toast.kind == ToastKind::Error;
        let has_trailing = copyable || !toast.actions.is_empty();
        let error_text = toast
            .description
            .clone()
            .unwrap_or_else(|| toast.title.clone());
        let mut actions: Vec<ToastAction> = toast.actions.clone();
        actions.sort_by_key(|a| a.primary);
        let text = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .when_some(icon, |el, icon| {
                        el.child(
                            div()
                                .w(px(16.))
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .child(icon),
                        )
                    })
                    .child(div().flex_1().font_medium().child(toast.title.clone())),
            )
            .when_some(toast.description.clone(), |el, desc| {
                el.child(div().text_color(t.muted_foreground).child(desc))
            });
        let trailing = has_trailing.then(|| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .flex_none()
                .when(copyable, |el| {
                    let text = error_text.clone();
                    el.child(
                        Button::new(("toast-copy", id))
                            .variant(ButtonVariant::GhostMuted)
                            .size(ButtonSize::IconMicro)
                            .icon(IconName::Copy)
                            .tooltip(Tooltip::text("Copy error"))
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()))
                            }),
                    )
                })
                .children(actions.into_iter().enumerate().map(|(i, a)| {
                    let handler = a.handler.clone();
                    Button::new(("toast-action", id * 16 + i as u64))
                        .variant(if a.primary {
                            ButtonVariant::Default
                        } else {
                            ButtonVariant::Outline
                        })
                        .size(ButtonSize::Xs)
                        .label(a.label)
                        .on_click(move |_, window, cx| handler(window, cx))
                }))
        });
        let close_hover_bg = t.popover;
        let close_hover_fg = t.foreground;
        div()
            .id(("toast", id))
            .relative()
            .w_full()
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.glass.dropdown_border)
            .bg(theme.glass.dropdown)
            .shadow(theme.shadows.xl(0.25))
            .text_color(t.popover_foreground)
            .text_scale(crate::theme::TextSize::Sm)
            .font_sans()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .py(px(12.))
                    .pl(px(14.))
                    .pr(px(if has_trailing { 24. } else { 14. }))
                    .when(hidden, |el| el.opacity(0.))
                    .child(text)
                    .children(trailing),
            )
            .child(
                div().absolute().top(px(-6.)).right(px(-6.)).child(
                    div()
                        .id(("toast-close", id))
                        .group("toast-close")
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(radius::FULL)
                        .border_1()
                        .border_color(s.glass_border)
                        .bg(s.toast_close_bg)
                        .text_color(t.muted_foreground)
                        .shadow(theme.shadows.sm(0.1))
                        .cursor_pointer()
                        .hover(move |st| st.bg(close_hover_bg).text_color(close_hover_fg))
                        .child(
                            Icon::new(IconName::X)
                                .size(px(12.))
                                .stroke(2.25)
                                .color(t.muted_foreground)
                                .group_hover_color(
                                    SharedString::from("toast-close"),
                                    close_hover_fg,
                                ),
                        )
                        .on_click(cx.listener(move |host, _, _, cx| host.remove(id, cx))),
                ),
            )
            .into_any_element()
    }
}

impl Render for Toasts {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.current_thread.clone();
        let visible: Vec<&Live> = self
            .live
            .iter()
            .filter(|l| {
                l.toast
                    .thread_id
                    .as_ref()
                    .is_none_or(|t| Some(t) == current.as_ref())
            })
            .take(3)
            .collect();
        if visible.is_empty() {
            return div().into_any_element();
        }
        let inset = 32.;
        let width = (window.viewport_size().width - px(2. * inset)).min(px(360.));
        let expanded = self.expanded;
        let mut stack = div()
            .id("toast-viewport")
            .absolute()
            .top(px(inset) + layout::TOPBAR_HEIGHT)
            .right(px(inset))
            .w(width)
            .on_hover(cx.listener(|host, hovered: &bool, _, cx| {
                host.expanded = *hovered;
                cx.notify();
            }));
        if expanded {
            stack = stack.flex().flex_col().gap(px(12.));
            for live in &visible {
                stack = stack.child(self.render_toast(live, false, cx));
            }
        } else {
            // Behind toasts first so the front one paints on top.
            for (i, live) in visible.iter().enumerate().skip(1).rev() {
                let shrink = 0.1 * i as f32;
                let el = self.render_toast(live, true, cx);
                stack = stack.child(
                    div()
                        .absolute()
                        .top(px(12. * i as f32))
                        .bottom(px(-12. * i as f32))
                        .left(width * (shrink / 2.))
                        .right(width * (shrink / 2.))
                        .overflow_hidden()
                        .child(div().size_full().child(el)),
                );
            }
            let front = visible[0];
            let front_id = front.id;
            stack = stack.child(
                div()
                    .relative()
                    .child(self.render_toast(front, false, cx))
                    .with_animation(
                        ("toast-enter", front_id),
                        Animation::new(motion::TOAST).with_easing(motion::ease_toast()),
                        move |el, t| el.left(width.max(px(0.)) * (1. - t) + px(inset * (1. - t))),
                    ),
            );
        }
        stack.into_any_element()
    }
}

/// Creates the window's toast host entity (the shell renders it).
pub fn new_host(cx: &mut App) -> Entity<Toasts> {
    cx.new(Toasts::new)
}

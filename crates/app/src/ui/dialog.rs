//! Dialog, AlertDialog and the confirm-dialog host (spec 01 §4.10-4.11,
//! spec 02 §6.2).
//!
//! Dialogs are hosted by the window's [`Dialogs`] entity (the shell renders it):
//!
//! ```ignore
//! Dialogs::open(window, cx, |_, cx| {
//!     Dialog::new()
//!         .title("Add project")
//!         .description("Pick a folder.")
//!         .child(…panel content…)
//!         .footer(DialogFooter::new().child(Button::new("ok").label("Add")))
//! });
//!
//! // Confirm (spec 02 §6.2): line 1 ending in `?` is the title, the rest the description.
//! let answer = Dialogs::confirm("Delete thread \"Fix CI\"?\nThis permanently clears …",
//!     ConfirmOptions::destructive(), window, cx);
//! cx.spawn(async move |_, _| if answer.await.unwrap_or(false) { … }).detach();
//! ```
//!
//! Escape closes the top dialog (Cancel for confirms). Clicking the backdrop
//! closes a plain dialog, not an alert or confirm. The 200ms enter is a fade; the
//! scale 0.98 is not reproduced (no transforms in GPUI).

use std::collections::VecDeque;
use std::rc::Rc;

use futures::channel::oneshot;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, Context, Entity, FocusHandle,
    Global, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels,
    Render, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::{Button, ButtonSize, ButtonVariant, IconName};
use crate::theme::{ActiveTheme as _, TextExt as _, leading, motion, radius};

/// Footer variants: `default` has a top border and muted/72 fill; `bare` has none.
#[derive(IntoElement)]
pub struct DialogFooter {
    bare: bool,
    children: Vec<AnyElement>,
}

impl DialogFooter {
    pub fn new() -> Self {
        Self {
            bare: false,
            children: Vec::new(),
        }
    }

    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }
}

impl Default for DialogFooter {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for DialogFooter {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for DialogFooter {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.tokens();
        let s = cx.states();
        div()
            .flex()
            .flex_row()
            .justify_end()
            .items_center()
            .gap(px(8.))
            .px(px(24.))
            .py(px(16.))
            .when(!self.bare, |el| {
                el.border_t_1()
                    .border_color(t.border)
                    .bg(s.dialog_footer)
                    .rounded_b(px(15.))
            })
            .children(self.children)
    }
}

/// A dialog popup. Build it inside the closure given to [`Dialogs::open`].
#[derive(IntoElement)]
pub struct Dialog {
    title: Option<SharedString>,
    description: Option<SharedString>,
    children: Vec<AnyElement>,
    footer: Option<DialogFooter>,
    close_button: bool,
    alert: bool,
    max_width: Pixels,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

impl Dialog {
    pub fn new() -> Self {
        Self {
            title: None,
            description: None,
            children: Vec::new(),
            footer: None,
            close_button: true,
            alert: false,
            max_width: px(512.),
            on_close: None,
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn footer(mut self, footer: DialogFooter) -> Self {
        self.footer = Some(footer);
        self
    }

    /// The X at top 8 / right 8 (on by default for dialogs, never on alerts).
    pub fn close_button(mut self, show: bool) -> Self {
        self.close_button = show;
        self
    }

    pub fn max_width(mut self, width: impl Into<Pixels>) -> Self {
        self.max_width = width.into();
        self
    }

    /// Runs when the close button is pressed (the host closes the dialog first).
    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }
}

impl Default for Dialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for Dialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

/// AlertDialog: same surface, no close button, description scrolls past
/// min(384, 60vh).
pub struct AlertDialog;

impl AlertDialog {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> Dialog {
        Dialog {
            close_button: false,
            alert: true,
            ..Dialog::new()
        }
    }
}

impl RenderOnce for Dialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.tern_theme().clone();
        let t = theme.colors;
        let has_panel = !self.children.is_empty();
        let max_desc = (window.viewport_size().height * 0.6).min(px(384.));
        let header = (self.title.is_some() || self.description.is_some()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .p(px(24.))
                .when(has_panel, |el| el.pb(px(12.)))
                .when_some(self.title, |el, title| {
                    el.child(
                        div()
                            .font_semibold()
                            .text_size(px(20.))
                            .line_height(px(20. * leading::SNUG))
                            .text_color(t.popover_foreground)
                            .child(title),
                    )
                })
                .when_some(self.description, |el, desc| {
                    el.child(
                        div()
                            .id("dialog-description")
                            .text_scale(crate::theme::TextSize::Sm)
                            .text_color(t.muted_foreground)
                            .when(self.alert, |el| el.max_h(max_desc).overflow_y_scroll())
                            .child(desc),
                    )
                })
        });
        let on_close = self.on_close.clone();
        div()
            .id("dialog-popup")
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .max_w(self.max_width)
            .rounded(radius::XL2)
            .border_1()
            .border_color(theme.glass.dialog_border)
            .bg(theme.glass.dialog)
            .shadow(theme.shadows.dialog())
            .text_color(t.popover_foreground)
            .font_sans()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(header)
            .when(has_panel, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .px(px(24.))
                        .pb(px(24.))
                        .pt(px(4.))
                        .children(self.children),
                )
            })
            .children(self.footer)
            .when(self.close_button, |el| {
                el.child(
                    div().absolute().top(px(8.)).right(px(8.)).child(
                        Button::new("dialog-close")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Icon)
                            .icon(IconName::X)
                            .on_click(move |_, window, cx| {
                                Dialogs::close_top(window, cx);
                                if let Some(f) = &on_close {
                                    f(window, cx);
                                }
                            }),
                    ),
                )
            })
    }
}

/// Options for [`Dialogs::confirm`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConfirmVariant {
    #[default]
    Default,
    Destructive,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ConfirmOptions {
    pub variant: ConfirmVariant,
    /// Focus Confirm first (only honored for non-destructive confirms).
    pub focus_confirm: bool,
}

impl ConfirmOptions {
    pub fn destructive() -> Self {
        Self {
            variant: ConfirmVariant::Destructive,
            focus_confirm: false,
        }
    }
}

/// Splits a confirm message into title and description (`ConfirmDialogHost.tsx:166-192`).
pub fn split_confirm_message(message: &str) -> (String, Option<String>) {
    let lines: Vec<&str> = message.split('\n').collect();
    if let Some(ix) = lines
        .iter()
        .position(|l| l.trim_end().ends_with('?') || l.trim_end().ends_with('？'))
    {
        let title = lines[ix].trim().to_string();
        let rest: Vec<&str> = lines
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != ix)
            .map(|(_, l)| *l)
            .collect();
        let desc = rest.join("\n").trim().to_string();
        return (title, (!desc.is_empty()).then_some(desc));
    }
    if let Some(pos) = message.find(['?', '？']) {
        let end = pos + message[pos..].chars().next().map_or(1, char::len_utf8);
        let desc = message[end..].trim().to_string();
        return (
            message[..end].trim().to_string(),
            (!desc.is_empty()).then_some(desc),
        );
    }
    ("Confirm action".to_string(), Some(message.to_string()))
}

type Builder = Rc<dyn Fn(&mut Window, &mut App) -> Dialog>;

struct OpenDialog {
    id: usize,
    build: Builder,
    focus: FocusHandle,
    dismiss_on_backdrop: bool,
}

struct ConfirmRequest {
    message: String,
    options: ConfirmOptions,
    reply: Option<oneshot::Sender<bool>>,
    cancel: FocusHandle,
    confirm: FocusHandle,
    focused_once: bool,
}

/// The window's dialog host.
pub struct Dialogs {
    stack: Vec<OpenDialog>,
    confirms: VecDeque<ConfirmRequest>,
    next_id: usize,
}

struct GlobalDialogs(Entity<Dialogs>);
impl Global for GlobalDialogs {}

impl Dialogs {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let entity = cx.entity();
        cx.set_global(GlobalDialogs(entity));
        Self {
            stack: Vec::new(),
            confirms: VecDeque::new(),
            next_id: 0,
        }
    }

    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalDialogs>().map(|g| g.0.clone())
    }

    /// Whether any dialog or confirm is showing (app shortcuts should stand down).
    pub fn is_open(cx: &App) -> bool {
        Self::global(cx).is_some_and(|d| {
            let d = d.read(cx);
            !d.stack.is_empty() || !d.confirms.is_empty()
        })
    }

    /// Opens a dialog on top of any open ones. Returns its id for [`Dialogs::close`].
    pub fn open(
        window: &mut Window,
        cx: &mut App,
        build: impl Fn(&mut Window, &mut App) -> Dialog + 'static,
    ) -> usize {
        let Some(host) = Self::global(cx) else {
            return 0;
        };
        host.update(cx, |host, cx| {
            host.next_id += 1;
            let focus = cx.focus_handle();
            focus.focus(window, cx);
            host.stack.push(OpenDialog {
                id: host.next_id,
                build: Rc::new(build),
                focus,
                dismiss_on_backdrop: true,
            });
            cx.notify();
            host.next_id
        })
    }

    /// Like [`Dialogs::open`] but the backdrop does not dismiss (alert dialogs).
    pub fn open_alert(
        window: &mut Window,
        cx: &mut App,
        build: impl Fn(&mut Window, &mut App) -> Dialog + 'static,
    ) -> usize {
        let id = Self::open(window, cx, build);
        if let Some(host) = Self::global(cx) {
            host.update(cx, |host, _| {
                if let Some(d) = host.stack.iter_mut().find(|d| d.id == id) {
                    d.dismiss_on_backdrop = false;
                }
            });
        }
        id
    }

    pub fn close(id: usize, _window: &mut Window, cx: &mut App) {
        if let Some(host) = Self::global(cx) {
            host.update(cx, |host, cx| {
                host.stack.retain(|d| d.id != id);
                cx.notify();
            });
        }
    }

    pub fn close_top(_window: &mut Window, cx: &mut App) {
        if let Some(host) = Self::global(cx) {
            host.update(cx, |host, cx| {
                host.stack.pop();
                cx.notify();
            });
        }
    }

    /// Queues a confirm (spec 02 §6.2). Resolves to true on Confirm, false on
    /// Cancel or Escape. Requests show one at a time.
    pub fn confirm(
        message: impl Into<String>,
        options: ConfirmOptions,
        _window: &mut Window,
        cx: &mut App,
    ) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        if let Some(host) = Self::global(cx) {
            host.update(cx, |host, cx| {
                host.confirms.push_back(ConfirmRequest {
                    message: message.into(),
                    options,
                    reply: Some(tx),
                    cancel: cx.focus_handle(),
                    confirm: cx.focus_handle(),
                    focused_once: false,
                });
                cx.notify();
            });
        }
        rx
    }

    fn answer(&mut self, value: bool, cx: &mut Context<Self>) {
        if let Some(mut req) = self.confirms.pop_front() {
            if let Some(tx) = req.reply.take() {
                let _ = tx.send(value);
            }
            cx.notify();
        }
    }

    fn render_confirm(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let req = self.confirms.front_mut()?;
        let (title, description) = split_confirm_message(&req.message);
        let destructive = req.options.variant == ConfirmVariant::Destructive;
        let focus_confirm = !destructive && req.options.focus_confirm;
        if !req.focused_once {
            req.focused_once = true;
            let target = if focus_confirm {
                &req.confirm
            } else {
                &req.cancel
            };
            target.focus(window, cx);
        }
        let (cancel_focus, confirm_focus) = (req.cancel.clone(), req.confirm.clone());
        let this = cx.entity().downgrade();
        let this2 = this.clone();
        let mut dialog = AlertDialog::new().title(title);
        if let Some(desc) = description {
            dialog = dialog.description(desc);
        }
        let dialog = dialog.footer(
            DialogFooter::new()
                .child(
                    Button::new("confirm-cancel")
                        .variant(ButtonVariant::Outline)
                        .label("Cancel")
                        .focus_handle(&cancel_focus)
                        .on_click(move |_, _, cx| {
                            let _ = this.update(cx, |host, cx| host.answer(false, cx));
                        }),
                )
                .child(
                    Button::new("confirm-ok")
                        .variant(if destructive {
                            ButtonVariant::Destructive
                        } else {
                            ButtonVariant::Default
                        })
                        .label("Confirm")
                        .focus_handle(&confirm_focus)
                        .on_click(move |_, _, cx| {
                            let _ = this2.update(cx, |host, cx| host.answer(true, cx));
                        }),
                ),
        );
        Some(self.layer("confirm", dialog.into_any_element(), false, true, cx))
    }

    fn layer(
        &self,
        key: impl Into<SharedString>,
        popup: AnyElement,
        dismiss_on_backdrop: bool,
        top: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.tern_theme().clone();
        let key: SharedString = key.into();
        div()
            .id(gpui::ElementId::Name(key.clone()))
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(theme.glass.dialog_backdrop),
            )
            .child(
                div()
                    .id("dialog-viewport")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .when(dismiss_on_backdrop && top, |el| {
                        el.on_click(cx.listener(|host, _, _, cx| {
                            host.stack.pop();
                            cx.notify();
                        }))
                    })
                    .child(popup),
            )
            .with_animation(
                gpui::ElementId::Name(format!("{key}-enter").into()),
                Animation::new(motion::DIALOG).with_easing(motion::ease_standard()),
                |el, t| el.opacity(t),
            )
            .into_any_element()
    }

    fn on_key(&mut self, e: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if e.keystroke.key != "escape" {
            return;
        }
        if !self.confirms.is_empty() {
            self.answer(false, cx);
        } else if self.stack.pop().is_some() {
            cx.notify();
        } else {
            return;
        }
        cx.stop_propagation();
    }
}

impl Render for Dialogs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.stack.is_empty() && self.confirms.is_empty() {
            return div().into_any_element();
        }
        let mut root = div()
            .id("dialogs")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .key_context("Dialog")
            .on_key_down(cx.listener(Self::on_key));
        let n = self.stack.len();
        let has_confirm = !self.confirms.is_empty();
        let dialogs: Vec<(usize, Builder, FocusHandle, bool)> = self
            .stack
            .iter()
            .map(|d| {
                (
                    d.id,
                    d.build.clone(),
                    d.focus.clone(),
                    d.dismiss_on_backdrop,
                )
            })
            .collect();
        for (i, (id, build, focus, dismiss)) in dialogs.into_iter().enumerate() {
            let depth = n - 1 - i;
            let popup = div()
                .track_focus(&focus)
                .w_full()
                .flex()
                .justify_center()
                .when(depth > 0, |el| el.mt(px(-40. * depth as f32)).opacity(0.9))
                .child(build(window, cx))
                .into_any_element();
            let top = i + 1 == n && !has_confirm;
            root = root.child(self.layer(format!("dialog-{id}"), popup, dismiss, top, cx));
        }
        if let Some(confirm) = self.render_confirm(window, cx) {
            root = root.child(confirm);
        }
        root.into_any_element()
    }
}

/// Creates the window's dialog host entity (the shell renders it).
pub fn new_host(cx: &mut App) -> Entity<Dialogs> {
    cx.new(Dialogs::new)
}

#[cfg(test)]
mod tests {
    use super::split_confirm_message;

    #[test]
    fn title_is_first_question_line() {
        let (t, d) = split_confirm_message(
            "Delete thread \"x\"?\nThis permanently clears conversation history for this thread.",
        );
        assert_eq!(t, "Delete thread \"x\"?");
        assert_eq!(
            d.as_deref(),
            Some("This permanently clears conversation history for this thread.")
        );
    }

    #[test]
    fn question_line_need_not_be_first() {
        let (t, d) = split_confirm_message(
            "This thread is the only one linked to this worktree:\n/tmp/wt\n\nDelete the worktree too?",
        );
        assert_eq!(t, "Delete the worktree too?");
        assert_eq!(
            d.as_deref(),
            Some("This thread is the only one linked to this worktree:\n/tmp/wt")
        );
    }

    #[test]
    fn no_question_mark() {
        let (t, d) = split_confirm_message("Proceed with reset.");
        assert_eq!(t, "Confirm action");
        assert_eq!(d.as_deref(), Some("Proceed with reset."));
    }
}

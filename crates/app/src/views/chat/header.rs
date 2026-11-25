//! The chat header (spec 03 §1): `[project] / [title]` breadcrumb with the title's
//! action menu and inline rename. The panel toggles on the right are the shell's
//! titlebar controls; the header keeps its content clear of them.

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, Hsla, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{InputEvent, InputState};
use tern_core::{Command, NEW_THREAD_TITLE};

use super::ChatView;
use crate::app_state::Route;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, layout, radius};
use crate::ui::{
    ConfirmOptions, Dialogs, DropdownMenu, Icon, IconName, Menu, MenuItem, TextField, Toast,
    Toasts, Tooltip, TooltipExt as _, TooltipSide, h_flex, menu::ContextMenuExt as _,
};

#[derive(Default)]
pub struct HeaderState {
    rename: Option<(Entity<InputState>, Subscription)>,
}

impl HeaderState {
    pub fn cancel_rename(&mut self) {
        self.rename = None;
    }
}

/// Project initials tile: T3 draws a generated favicon (two letters on a tinted square).
pub(crate) fn project_tile(name: &str, size: f32, cx: &App) -> AnyElement {
    let initials: String = name
        .split(['-', '_', ' ', '.'])
        .filter(|p| !p.is_empty())
        .take(2)
        .filter_map(|p| p.chars().next())
        .flat_map(char::to_uppercase)
        .collect();
    let initials = if initials.chars().count() < 2 {
        name.chars().take(2).flat_map(char::to_uppercase).collect()
    } else {
        initials
    };
    let hues = [0.22, 0.58, 0.08, 0.78, 0.45, 0.95, 0.15];
    let hash = name.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let hue = hues[(hash as usize) % hues.len()];
    let dark = cx.tern_theme().is_dark();
    let (bg, fg) = if dark {
        (
            gpui::hsla(hue, 0.55, 0.18, 1.),
            gpui::hsla(hue, 0.85, 0.62, 1.),
        )
    } else {
        (
            gpui::hsla(hue, 0.75, 0.88, 1.),
            gpui::hsla(hue, 0.7, 0.3, 1.),
        )
    };
    div()
        .size(px(size))
        .flex_none()
        .rounded(px(3.))
        .bg(bg)
        .text_color(fg)
        .text_size(px(size * 0.5))
        .line_height(px(size))
        .font_weight(gpui::FontWeight::BOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(initials)
        .into_any_element()
}

impl ChatView {
    fn start_rename(&mut self, title: String, window: &mut Window, cx: &mut Context<Self>) {
        let state = cx.new(|cx| InputState::new(window, cx));
        state.update(cx, |s, cx| {
            s.set_value(title, window, cx);
            s.select_all(window, cx);
            s.focus(window, cx);
        });
        let sub = cx.subscribe_in(&state, window, |this, _, event: &InputEvent, window, cx| {
            match event {
                InputEvent::PressEnter { .. } => this.commit_rename(window, cx),
                InputEvent::Blur => this.commit_rename(window, cx),
                _ => {}
            }
        });
        self.header.rename = Some((state, sub));
        cx.notify();
    }

    fn commit_rename(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some((state, _)) = self.header.rename.take() else {
            return;
        };
        let Some(detail) = self.detail.clone() else {
            return;
        };
        let value = state.read(cx).value().trim().to_string();
        if value.is_empty() {
            Toasts::push(Toast::warning("Thread title cannot be empty"), cx);
        } else if value != detail.thread.title {
            self.store.read(cx).send(Command::RenameThread {
                thread_id: detail.thread.id.clone(),
                title: value,
            });
        }
        cx.notify();
    }

    #[allow(dead_code)]
    pub(crate) fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.header.rename = None;
        cx.notify();
    }

    fn thread_menu(&self, cx: &App) -> impl Fn(&mut Window, &mut App) -> Menu + 'static {
        let view = self.weak.clone();
        let store = self.store.clone();
        let detail = self.detail.clone();
        let workspace = self.workspace_root(cx);
        let working = self.is_working(cx);
        move |_, cx| {
            let Some(detail) = detail.clone() else {
                return Menu::new().item(MenuItem::new("Project settings"));
            };
            let thread = detail.thread.clone();
            let id = thread.id.clone();
            let send = |cmd: Command| {
                let store = store.clone();
                move |_: &mut Window, cx: &mut App| store.read(cx).send(cmd.clone())
            };
            let has_completed = detail
                .runs
                .iter()
                .any(|r| r.status == tern_core::RunStatus::Completed);
            let v_find = view.clone();
            let v_rename = view.clone();
            let title = thread.title.clone();
            let mut copy = Menu::new();
            if let Some(path) = workspace.clone() {
                copy = copy.item(MenuItem::new("Path").icon(IconName::Folder).on_select(move |_, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.clone()))
                }));
            }
            if let Some(branch) = thread.branch.clone() {
                copy = copy.item(MenuItem::new("Branch").icon(IconName::GitBranch).on_select(move |_, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(branch.clone()))
                }));
            }
            let thread_text = title.clone();
            copy = copy
                .item(MenuItem::new("Thread").icon(IconName::MessageSquare).on_select(move |_, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(thread_text.clone()))
                }))
                .item({
                    let id = id.clone();
                    MenuItem::new("Thread ID").icon(IconName::Hash).on_select(move |_, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(id.clone()))
                    })
                });
            let delete_store = store.clone();
            let delete_id = id.clone();
            let delete_title = title.clone();
            let _ = cx;
            Menu::new()
                .item(MenuItem::new("Find in thread").icon(IconName::TextSearch).on_select(
                    move |window, cx| {
                        if let Some(v) = v_find.upgrade() {
                            v.update(cx, |v, cx| v.open_find(window, cx));
                        }
                    },
                ))
                .item(
                    MenuItem::new(if thread.pinned_at.is_some() {
                        "Unpin thread"
                    } else {
                        "Pin thread"
                    })
                    .icon(if thread.pinned_at.is_some() {
                        IconName::PinOff
                    } else {
                        IconName::Pin
                    })
                    .on_select(send(Command::SetPinned {
                        thread_id: id.clone(),
                        pinned: thread.pinned_at.is_none(),
                    })),
                )
                .item(
                    MenuItem::new(if thread.settled_at.is_some() {
                        "Un-settle thread"
                    } else {
                        "Settle thread"
                    })
                    .icon(IconName::CircleCheck)
                    .on_select(send(Command::SetSettled {
                        thread_id: id.clone(),
                        settled: thread.settled_at.is_none(),
                    })),
                )
                .separator()
                .item(MenuItem::new("Rename thread").icon(IconName::Pencil).on_select(
                    move |window, cx| {
                        if let Some(v) = v_rename.upgrade() {
                            let title = title.clone();
                            v.update(cx, |v, cx| v.start_rename(title, window, cx));
                        }
                    },
                ))
                .item(
                    MenuItem::new("Regenerate title")
                        .icon(IconName::RefreshCw)
                        .on_select(send(Command::RegenerateTitle {
                            thread_id: id.clone(),
                        })),
                )
                .item(
                    MenuItem::new("Mark unread")
                        .icon(IconName::MailOpen)
                        .disabled(!has_completed || working)
                        .on_select(send(Command::MarkUnread {
                            thread_id: id.clone(),
                        })),
                )
                .separator()
                .item(MenuItem::submenu("Copy", Some(IconName::Copy), copy))
                .item(MenuItem::new("Project settings").icon(IconName::Settings))
                .separator()
                .item(
                    MenuItem::new("Archive thread")
                        .icon(IconName::Archive)
                        .disabled(working)
                        .on_select(send(Command::SetArchived {
                            thread_id: id.clone(),
                            archived: true,
                        })),
                )
                .item(
                    MenuItem::new("Delete")
                        .icon(IconName::Trash2)
                        .destructive()
                        .on_select(move |window, cx| {
                            let rx = Dialogs::confirm(
                                format!(
                                    "Delete thread \"{delete_title}\"?\nThis permanently clears conversation history for this thread."
                                ),
                                ConfirmOptions::destructive(),
                                window,
                                cx,
                            );
                            let store = delete_store.clone();
                            let id = delete_id.clone();
                            cx.spawn(async move |cx| {
                                if rx.await == Ok(true) {
                                    let _ = store.update(cx, |s, _| {
                                        s.send(Command::DeleteThread { thread_id: id })
                                    });
                                }
                            })
                            .detach();
                        }),
                )
        }
    }

    pub(crate) fn render_header(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let app = self.app.read(cx);
        let sidebar_open = app.sidebar.open;
        let right_open = app.right_panel.open;
        let store = self.store.read(cx);
        let snapshot = store.snapshot();
        let (project, title, is_thread) = match app.chat_route() {
            Route::Thread(id) => {
                let shell = snapshot.thread(id);
                let project = shell
                    .and_then(|s| snapshot.project(&s.thread.project_id))
                    .map(|p| (p.id.clone(), p.title.clone()));
                (
                    project,
                    shell
                        .map(|s| s.thread.title.clone())
                        .unwrap_or_else(|| NEW_THREAD_TITLE.into()),
                    shell.is_some(),
                )
            }
            Route::Draft(project) => (
                snapshot.project(project).map(|p| (p.id.clone(), p.title.clone())),
                NEW_THREAD_TITLE.to_string(),
                false,
            ),
            _ => (
                snapshot.projects.first().map(|p| (p.id.clone(), p.title.clone())),
                NEW_THREAD_TITLE.to_string(),
                false,
            ),
        };
        let controls = crate::shell::titlebar_controls_width(window, right_open);
        let app_entity = self.app.clone();
        let crumb = project.map(|(pid, name)| {
            let tip = format!("New thread in {name}");
            let app = app_entity.clone();
            div()
                .id("crumb-project")
                .flex_none()
                .max_w(px(220.))
                .min_w_0()
                .cursor_pointer()
                .t3_tooltip(Tooltip::text(tip).side(TooltipSide::Top))
                .on_click(move |_, _, cx| {
                    let pid = pid.clone();
                    app.update(cx, |s, cx| s.navigate(Route::Draft(pid), cx));
                })
                .child(
                    h_flex()
                        .gap(px(6.))
                        .text_color(t.muted_foreground)
                        .hover(|s| s.text_color(t.foreground))
                        .child(project_tile(&name, 14., cx))
                        .child(div().min_w_0().truncate().child(name)),
                )
        });
        let separator = crumb.as_ref().map(|_| {
            div()
                .flex_none()
                .text_color(t.icon_muted)
                .child("/")
        });
        let title_el: AnyElement = match &self.header.rename {
            Some((state, _)) => div()
                .flex_1()
                .min_w(px(40.))
                .max_w(px(520.))
                .child(TextField::new(state).size(crate::ui::InputSize::Compact))
                .on_mouse_down_out(cx.listener(|this, _, window, cx| this.commit_rename(window, cx)))
                .into_any_element(),
            None => {
                let menu = self.thread_menu(cx);
                let full_title = title.clone();
                let rename_title = title.clone();
                DropdownMenu::new(
                    "chat-title-menu",
                    menu,
                    move |open| {
                        title_button(full_title, is_thread, open, t.foreground, t.muted_foreground)
                    },
                )
                .align(gpui_base::Align::Start)
                .into_any_element()
                .pipe(|el| {
                    div()
                        .id("chat-title")
                        .min_w(px(40.))
                        .flex_shrink(1.)
                        .min_w_0()
                        .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                            if e.click_count() == 2 && is_thread {
                                crate::ui::popover::close_popups(window, cx);
                                this.start_rename(rename_title.clone(), window, cx);
                            }
                        }))
                        .child(el)
                        .into_any_element()
                })
            }
        };
        let menu = self.thread_menu(cx);
        h_flex()
            .id("chat-header")
            .h(layout::TOPBAR_HEIGHT)
            .flex_none()
            .w_full()
            .pl(if sidebar_open {
                layout::WORKSPACE_GUTTER
            } else {
                layout::TITLEBAR_CONTENT_LEFT
            })
            .pr(controls + px(8.))
            .gap(px(12.))
            .font_medium()
            .text_scale(TextSize::Sm)
            .t3_context_menu("chat-header-menu", menu)
            .on_mouse_down(MouseButton::Middle, |_, _, _| {})
            .children(crumb)
            .children(separator)
            .child(title_el)
            .into_any_element()
    }
}

fn title_button(title: String, chevron: bool, open: bool, fg: Hsla, muted: Hsla) -> AnyElement {
    h_flex()
        .id("chat-title-button")
        .group("chat-title")
        .min_w_0()
        .gap(px(4.))
        .rounded(radius::SM)
        .cursor_pointer()
        .t3_tooltip(Tooltip::text(SharedString::from(title.clone())).side(TooltipSide::Top))
        .child(div().min_w_0().truncate().text_color(fg).child(title))
        .when(chevron, |el| {
            el.child(
                div()
                    .flex_none()
                    .opacity(if open { 1. } else { 0. })
                    .group_hover("chat-title", |s| s.opacity(1.))
                    .child(Icon::new(IconName::ChevronDown).size(px(14.)).color(muted)),
            )
        })
        .into_any_element()
}

trait Pipe: Sized {
    fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}

impl<T> Pipe for T {}

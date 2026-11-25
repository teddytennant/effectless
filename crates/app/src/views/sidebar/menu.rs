//! The thread context menu (spec 02 §2.10) and the thread actions behind it, the row
//! buttons and the shortcuts. Snooze, guest sharing and the auto-settle submenu are left
//! out (§7). With several rows selected the menu acts on all of them.

use gpui::{ClipboardItem, Context, Pixels, Point, Window};
use tern_core::{Command, ThreadId};

use super::{RowKey, SidebarView, model};
use crate::app_state::{Route, SettingsPage};
use crate::ui::menu::open_menu;
use crate::ui::popup::PopupAnchor;
use crate::ui::{ConfirmOptions, Dialogs, IconName, Menu, MenuItem, Toast, Toasts};

/// `confirmThreadDelete` / `confirmThreadArchive` defaults (spec 02 §2.10).
const CONFIRM_DELETE: bool = true;
const CONFIRM_ARCHIVE: bool = false;

impl SidebarView {
    /// The rows a menu opened on `id` acts on: the selection when `id` is in it.
    fn menu_targets(&self, id: &str) -> Vec<ThreadId> {
        if self.selection.len() > 1 && self.selection.iter().any(|s| s == id) {
            self.selection.clone()
        } else {
            vec![id.to_string()]
        }
    }

    pub(crate) fn open_context_menu(
        &mut self,
        id: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.thread_menu(id, cx) else {
            return;
        };
        open_menu(
            gpui::ElementId::Name(format!("thread-menu-{id}").into()),
            PopupAnchor::Point {
                position,
                trigger: None,
            },
            menu,
            window,
            cx,
        );
    }

    fn thread_menu(&self, id: &str, cx: &mut Context<Self>) -> Option<Menu> {
        let snap = self.snapshot(cx);
        let shell = snap.thread(id)?.clone();
        let project = snap.project(&shell.thread.project_id).cloned();
        let targets = self.menu_targets(id);
        let bulk = targets.len() > 1;
        let open = self.open_thread_id(cx).as_deref() == Some(id);
        let running = shell.is_working();
        let pinned = shell.thread.pinned_at.is_some();
        let settled = shell.thread.settled_at.is_some();
        let me = cx.entity().downgrade();
        let act =
            |f: fn(&mut SidebarView, Vec<ThreadId>, &mut Window, &mut Context<SidebarView>)| {
                let me = me.clone();
                let targets = targets.clone();
                move |window: &mut Window, cx: &mut gpui::App| {
                    let targets = targets.clone();
                    let _ = me.update(cx, |this, cx| f(this, targets, window, cx));
                }
            };

        let mut menu = Menu::new();
        if open && !bulk && crate::keymap::has_handler("chat.find", cx) {
            menu = menu.item(
                MenuItem::new("Find in thread")
                    .icon(IconName::TextSearch)
                    .on_select(|window, cx| {
                        crate::keymap::run("chat.find", window, cx);
                    }),
            );
        }
        if !bulk && let Some(branch) = shell.thread.branch.clone() {
            let project_id = shell.thread.project_id.clone();
            let me = me.clone();
            menu = menu.item(
                MenuItem::new(format!("New thread on {branch}"))
                    .icon(IconName::MessageSquarePlus)
                    .on_select(move |_, cx| {
                        let project_id = project_id.clone();
                        let _ =
                            me.update(cx, |this, cx| this.navigate(Route::Draft(project_id), cx));
                    }),
            );
        }
        menu = menu
            .item(
                MenuItem::new(if pinned { "Unpin thread" } else { "Pin thread" })
                    .icon(if pinned {
                        IconName::PinOff
                    } else {
                        IconName::Pin
                    })
                    .shortcut(crate::keymap::shortcut_label("thread.pin", cx).unwrap_or_default())
                    .on_select(act(if pinned {
                        |this, ids, w, cx| this.set_pinned(ids, false, w, cx)
                    } else {
                        |this, ids, w, cx| this.set_pinned(ids, true, w, cx)
                    })),
            )
            .item(
                MenuItem::new(if settled {
                    "Un-settle thread"
                } else {
                    "Settle thread"
                })
                .icon(IconName::CircleCheck)
                .on_select(act(if settled {
                    |this, ids, w, cx| this.set_settled(ids, false, w, cx)
                } else {
                    |this, ids, w, cx| this.set_settled(ids, true, w, cx)
                })),
            )
            .separator();
        if !bulk {
            let me = me.clone();
            let id = id.to_string();
            menu = menu.item(
                MenuItem::new("Rename thread")
                    .icon(IconName::Pencil)
                    .on_select(move |window, cx| {
                        let id = id.clone();
                        let _ = me.update(cx, |this, cx| this.start_rename(&id, window, cx));
                    }),
            );
        }
        let regenerating = shell.thread.title_generation_started_at.is_some();
        menu = menu.item(
            MenuItem::new(if regenerating {
                "Regenerating…"
            } else {
                "Regenerate title"
            })
            .icon(IconName::RefreshCw)
            .disabled(regenerating)
            .on_select(act(|this, ids, _, cx| {
                for thread_id in ids {
                    this.store
                        .read(cx)
                        .send(Command::RegenerateTitle { thread_id });
                }
            })),
        );
        if shell.latest_run_completed_at.is_some() && !running && !bulk {
            menu = menu.item(
                MenuItem::new("Mark unread")
                    .icon(IconName::MailOpen)
                    .on_select(act(|this, ids, _, cx| {
                        for thread_id in ids {
                            this.store.read(cx).send(Command::MarkUnread { thread_id });
                        }
                    })),
            );
        }
        if let Some(project) = &project {
            let scoped = self.scope.len() == 1 && self.scope[0] == project.id;
            let project_id = project.id.clone();
            let me = me.clone();
            menu = menu.item(
                MenuItem::new(if scoped {
                    "Show all projects".to_string()
                } else {
                    format!("Filter by {}", project.title)
                })
                .icon(IconName::FolderTree)
                .on_select(move |_, cx| {
                    let project_id = project_id.clone();
                    let _ = me.update(cx, |this, cx| {
                        this.scope = if scoped { Vec::new() } else { vec![project_id] };
                        cx.notify();
                    });
                }),
            );
        }
        menu = menu.separator();
        if !bulk {
            let path = shell
                .thread
                .worktree_path
                .clone()
                .or_else(|| project.as_ref().map(|p| p.workspace_root.clone()));
            let mut copy =
                Menu::new().item(MenuItem::new("Path").icon(IconName::Folder).on_select(
                    move |_, cx| match &path {
                        Some(path) => {
                            cx.write_to_clipboard(ClipboardItem::new_string(path.clone()))
                        }
                        None => {
                            Toasts::push(
                                Toast::error("Path unavailable").description(
                                    "This thread does not have a workspace path to copy.",
                                ),
                                cx,
                            );
                        }
                    },
                ));
            if let Some(branch) = shell.thread.branch.clone() {
                copy = copy.item(MenuItem::new("Branch").icon(IconName::GitBranch).on_select(
                    move |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(branch.clone())),
                ));
            }
            let title = shell.thread.title.clone();
            let thread_id = shell.thread.id.clone();
            copy = copy
                .item(
                    MenuItem::new("Thread")
                        .icon(IconName::MessageSquare)
                        .on_select(move |_, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(title.clone()))
                        }),
                )
                .item(
                    MenuItem::new("Thread ID")
                        .icon(IconName::Hash)
                        .on_select(move |_, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(thread_id.clone()))
                        }),
                );
            menu = menu.item(MenuItem::submenu("Copy", Some(IconName::Copy), copy));
        }
        {
            let me = me.clone();
            menu = menu.item(
                MenuItem::new("Project settings")
                    .icon(IconName::Settings)
                    .on_select(move |_, cx| {
                        let _ = me.update(cx, |this, cx| {
                            this.app
                                .update(cx, |s, cx| s.open_settings(SettingsPage::Projects, cx))
                        });
                    }),
            );
        }
        menu = menu
            .separator()
            .item(
                MenuItem::new("Archive thread")
                    .icon(IconName::Archive)
                    .disabled(running)
                    .on_select(act(|this, ids, w, cx| this.archive(ids, w, cx))),
            )
            .item(
                MenuItem::new("Delete")
                    .icon(IconName::Trash2)
                    .destructive()
                    .on_select(act(|this, ids, w, cx| this.delete(ids, w, cx))),
            );
        Some(menu)
    }

    // --- actions ---------------------------------------------------------------------

    fn title_of(&self, id: &str, cx: &gpui::App) -> String {
        self.snapshot(cx)
            .thread(id)
            .map(|s| s.thread.title.clone())
            .unwrap_or_default()
    }

    /// Settling or archiving the open thread moves to the next unsettled row below it,
    /// else a draft in its project, else `/`.
    fn leave_open_thread(&mut self, removed: &[ThreadId], cx: &mut Context<Self>) {
        let Some(open) = self.open_thread_id(cx) else {
            return;
        };
        if !removed.contains(&open) {
            return;
        }
        let order: Vec<_> = self
            .visible_threads(cx)
            .into_iter()
            .filter(|(id, _)| *id == open || !removed.contains(id))
            .collect();
        let route = match model::next_after(&order, &open) {
            Some(next) => Route::Thread(next),
            None => match self.snapshot(cx).thread(&open) {
                Some(shell) => Route::Draft(shell.thread.project_id.clone()),
                None => Route::Index,
            },
        };
        self.navigate(route, cx);
    }

    pub(crate) fn set_pinned(
        &mut self,
        ids: Vec<ThreadId>,
        pinned: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for id in &ids {
            self.store.read(cx).send(Command::SetPinned {
                thread_id: id.clone(),
                pinned,
            });
            if !pinned {
                self.pin_order.remove(id);
            }
        }
        if !pinned {
            let undo = ids
                .iter()
                .map(|id| Command::SetPinned {
                    thread_id: id.clone(),
                    pinned: true,
                })
                .collect();
            self.show_undo(count_label("Unpinned", ids.len()), undo, cx);
        }
        self.selection.clear();
    }

    pub(crate) fn set_settled(
        &mut self,
        ids: Vec<ThreadId>,
        settled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if settled {
            self.leave_open_thread(&ids, cx);
        }
        for id in &ids {
            self.store.read(cx).send(Command::SetSettled {
                thread_id: id.clone(),
                settled,
            });
        }
        if settled {
            let undo = ids
                .iter()
                .map(|id| Command::SetSettled {
                    thread_id: id.clone(),
                    settled: false,
                })
                .collect();
            self.show_undo(count_label("Settled", ids.len()), undo, cx);
        }
        self.selection.clear();
        cx.notify();
    }

    pub(crate) fn archive(
        &mut self,
        ids: Vec<ThreadId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if CONFIRM_ARCHIVE && ids.len() == 1 {
            let message = format!("Archive thread \"{}\"?", self.title_of(&ids[0], cx));
            let answer = Dialogs::confirm(message, ConfirmOptions::default(), window, cx);
            cx.spawn(async move |this, cx| {
                if answer.await.unwrap_or(false) {
                    let _ = this.update(cx, |this, cx| this.archive_now(ids, cx));
                }
            })
            .detach();
            return;
        }
        self.archive_now(ids, cx);
    }

    fn archive_now(&mut self, ids: Vec<ThreadId>, cx: &mut Context<Self>) {
        self.leave_open_thread(&ids, cx);
        for id in &ids {
            self.store.read(cx).send(Command::SetArchived {
                thread_id: id.clone(),
                archived: true,
            });
        }
        let undo = ids
            .iter()
            .map(|id| Command::SetArchived {
                thread_id: id.clone(),
                archived: false,
            })
            .collect();
        self.show_undo(count_label("Archived", ids.len()), undo, cx);
        self.selection.clear();
    }

    pub(crate) fn delete(
        &mut self,
        ids: Vec<ThreadId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !CONFIRM_DELETE {
            self.delete_now(ids, cx);
            return;
        }
        let message = if ids.len() == 1 {
            format!(
                "Delete thread \"{}\"?\nThis permanently clears conversation history for this thread.",
                self.title_of(&ids[0], cx)
            )
        } else {
            format!(
                "Delete {} threads?\nThis permanently clears conversation history for these threads.",
                ids.len()
            )
        };
        let answer = Dialogs::confirm(message, ConfirmOptions::destructive(), window, cx);
        cx.spawn(async move |this, cx| {
            if answer.await.unwrap_or(false) {
                let _ = this.update(cx, |this, cx| this.delete_now(ids, cx));
            }
        })
        .detach();
    }

    fn delete_now(&mut self, ids: Vec<ThreadId>, cx: &mut Context<Self>) {
        self.leave_open_thread(&ids, cx);
        for id in &ids {
            self.store.read(cx).send(Command::DeleteThread {
                thread_id: id.clone(),
            });
            self.drafts.update(cx, |d, cx| d.discard_thread(id, cx));
        }
        self.selection.clear();
        cx.notify();
    }

    pub(crate) fn discard_draft(&mut self, project_id: &str, cx: &mut Context<Self>) {
        self.drafts
            .update(cx, |d, cx| d.discard_project(project_id, cx));
        if self.open_draft_project(cx).as_deref() == Some(project_id) {
            self.focused_row = Some(RowKey::Draft(project_id.to_string()));
        }
        cx.notify();
    }

    pub(crate) fn discard_thread_draft(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        self.drafts
            .update(cx, |d, cx| d.discard_thread(thread_id, cx));
    }
}

/// "Settled" / "Settled 3".
fn count_label(verb: &str, n: usize) -> String {
    if n == 1 {
        verb.to_string()
    } else {
        format!("{verb} {n}")
    }
}

//! Dialogs opened from the git rows: the commit dialog (§4b.5), the default-branch
//! confirmation (§4b.6) and "Checkout pull request" (§4b.14).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Task, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::input::{InputEvent, InputState};
use tern_core::ThreadEnvMode;
use tern_git::remote::PullRequestInfo;

use super::git::ActionRequest;
use super::logic::{self, GitAction, GitStatus, StatusFile};
use super::{Ctx, DetailsCard};
use crate::app_state::{DraftWorkspace, Route};
use crate::theme::{ActiveTheme as _, TextExt as _, font, radius};
use crate::ui::input::TextareaState;
use crate::ui::{
    Button, ButtonSize, ButtonVariant, CheckState, Checkbox, Dialog, DialogFooter, Dialogs, Icon,
    IconName, InputSize, Spinner, TextField, TextareaField, Toast, Toasts,
};

/// Re-renders open dialogs after their state entity changed.
pub(super) fn refresh_dialogs(cx: &mut App) {
    if let Some(host) = Dialogs::global(cx) {
        host.update(cx, |_, cx| cx.notify());
    }
}

// ---- commit dialog -----------------------------------------------------------

pub(super) struct CommitDialog {
    message: Entity<TextareaState>,
    excluded: HashSet<String>,
    editing: bool,
    files: Vec<StatusFile>,
    folder: String,
    branch: Option<String>,
    is_default: bool,
    id: usize,
}

impl CommitDialog {
    fn selected(&self) -> Vec<&StatusFile> {
        self.files
            .iter()
            .filter(|f| !self.excluded.contains(&f.path))
            .collect()
    }
}

fn summary_surface(cx: &App) -> (gpui::Hsla, gpui::Hsla) {
    if cx.tern_theme().is_dark() {
        (gpui::white().opacity(0.035), gpui::white().opacity(0.05))
    } else {
        (gpui::hsla(0., 0., 0.988, 1.), gpui::black().opacity(0.05))
    }
}

fn list_surface(cx: &App) -> gpui::Hsla {
    if cx.tern_theme().is_dark() {
        gpui::white().opacity(0.025)
    } else {
        cx.tokens().card
    }
}

/// `+a / -d` in the diff colors.
fn counts(additions: u32, deletions: u32, cx: &App) -> gpui::Div {
    let t = cx.tokens();
    let diff = cx.tern_theme().diff;
    div()
        .flex()
        .flex_none()
        .gap(px(4.))
        .font_family(font::MONO)
        .child(
            div()
                .text_color(diff.diff_addition)
                .child(format!("+{additions}")),
        )
        .child(div().text_color(t.muted_foreground).child("/"))
        .child(
            div()
                .text_color(diff.diff_deletion)
                .child(format!("-{deletions}")),
        )
}

/// The path with the ellipsis on the left.
fn start_truncate(path: &str, max: usize) -> String {
    let n = path.chars().count();
    if n <= max {
        path.to_string()
    } else {
        let tail: String = path.chars().skip(n - (max - 1)).collect();
        format!("…{tail}")
    }
}

impl DetailsCard {
    pub(super) fn open_commit_dialog(
        &mut self,
        ctx: &Ctx,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let status = self
            .git
            .as_ref()
            .and_then(|g| g.status())
            .cloned()
            .unwrap_or_default();
        let message =
            cx.new(|cx| TextareaState::new(window, cx).placeholder("Leave empty to auto-generate"));
        let state = cx.new(|_| CommitDialog {
            message,
            excluded: HashSet::new(),
            editing: false,
            files: status.files.clone(),
            folder: ctx.project.title.clone(),
            branch: status.branch.clone(),
            is_default: status.is_default_ref,
            id: 0,
        });
        let card = cx.entity().downgrade();
        let ctx = ctx.clone();
        let state2 = state.clone();
        let id = Dialogs::open(window, cx, move |_, cx| {
            build_commit_dialog(&state2, &card, &ctx, cx)
        });
        state.update(cx, |s, _| s.id = id);
    }

    pub(super) fn open_default_branch_dialog(
        &mut self,
        ctx: &Ctx,
        action: GitAction,
        status: &GitStatus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let branch = status.branch.clone().unwrap_or_default();
        let includes_commit = action.includes_commit() && status.has_changes;
        let (title, description, continue_label) =
            logic::default_branch_copy(action, includes_commit, &branch);
        let reason = logic::feature_branch_unavailable(action, status.has_changes, &branch);
        let card = cx.entity().downgrade();
        let ctx = ctx.clone();
        let id_cell = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let id_for_build = id_cell.clone();
        let id = Dialogs::open(window, cx, move |_, cx| {
            let t = *cx.tokens();
            let run = |feature: bool| {
                let card = card.clone();
                let ctx = ctx.clone();
                let id = id_for_build.clone();
                move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                    Dialogs::close(id.get(), window, cx);
                    let _ = card.update(cx, |this, cx| {
                        this.run_git_action(
                            &ctx,
                            ActionRequest {
                                action,
                                commit_message: None,
                                feature_branch: feature,
                                file_paths: None,
                            },
                            cx,
                        )
                    });
                }
            };
            let abort_id = id_for_build.clone();
            Dialog::new()
                .title(title.clone())
                .description(description.clone())
                .max_width(px(576.))
                .close_button(false)
                .children(reason.clone().map(|r| {
                    div()
                        .text_scale(crate::theme::TextSize::Xs)
                        .text_color(t.muted_foreground)
                        .child(r)
                }))
                .footer(
                    DialogFooter::new()
                        .bare()
                        .child(
                            div().mr_auto().child(
                                Button::new("default-branch-abort")
                                    .label("Abort")
                                    .variant(ButtonVariant::Outline)
                                    .size(ButtonSize::Sm)
                                    .on_click(move |_, window, cx| {
                                        Dialogs::close(abort_id.get(), window, cx)
                                    }),
                            ),
                        )
                        .child(
                            Button::new("default-branch-continue")
                                .label(continue_label.clone())
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::SmMultiline)
                                .on_click(run(false)),
                        )
                        .child(
                            Button::new("default-branch-feature")
                                .label("Check out feature branch & continue")
                                .size(ButtonSize::SmMultiline)
                                .disabled(reason.is_some())
                                .on_click(run(true)),
                        ),
                )
        });
        id_cell.set(id);
    }
}

fn build_commit_dialog(
    state: &Entity<CommitDialog>,
    card: &gpui::WeakEntity<DetailsCard>,
    ctx: &Ctx,
    cx: &mut App,
) -> Dialog {
    let t = *cx.tokens();
    let s = state.read(cx);
    let (summary_bg, ring) = summary_surface(cx);
    let total = s.files.len();
    let selected = s.selected();
    let n_selected = selected.len();
    let (sel_add, sel_del) = selected
        .iter()
        .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions));
    let editing = s.editing;
    let id = s.id;

    let field_row = |label: &'static str, value: gpui::AnyElement| {
        [
            div()
                .text_color(t.muted_foreground)
                .child(label)
                .into_any_element(),
            value,
        ]
    };
    let branch_value = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .child(
            div()
                .font_medium()
                .truncate()
                .child(s.branch.clone().unwrap_or("(detached HEAD)".into())),
        )
        .when(s.is_default, |el| {
            el.child(
                div()
                    .text_color(t.warning_foreground)
                    .child("Default branch"),
            )
        })
        .into_any_element();
    let grid = div()
        .grid()
        .grid_cols(2)
        .gap_x(px(8.))
        .gap_y(px(4.))
        .children(field_row(
            "Folder",
            div()
                .font_medium()
                .truncate()
                .child(s.folder.clone())
                .into_any_element(),
        ))
        .children(field_row("Branch", branch_value));

    let all_state = if n_selected == total {
        CheckState::Checked
    } else if n_selected == 0 {
        CheckState::Unchecked
    } else {
        CheckState::Indeterminate
    };
    let st = state.clone();
    let toggle_all = move |checked: bool, _: &mut Window, cx: &mut App| {
        st.update(cx, |s, _| {
            if checked {
                s.excluded.clear();
            } else {
                s.excluded = s.files.iter().map(|f| f.path.clone()).collect();
            }
        });
        refresh_dialogs(cx);
    };
    let st = state.clone();
    let files_header = div()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .when(editing && total > 0, |el| {
                    el.child(
                        Checkbox::new("commit-all", all_state == CheckState::Checked)
                            .state(all_state)
                            .on_change(toggle_all),
                    )
                })
                .child(div().text_color(t.muted_foreground).child("Files"))
                .when(n_selected != total && !editing, |el| {
                    el.child(
                        div()
                            .text_color(t.muted_foreground)
                            .child(format!("({n_selected} of {total})")),
                    )
                }),
        )
        .when(total > 0, |el| {
            el.child(
                Button::new("commit-edit-files")
                    .label(if editing { "Done" } else { "Edit" })
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Xs)
                    .on_click(move |_, _, cx| {
                        st.update(cx, |s, _| s.editing = !s.editing);
                        refresh_dialogs(cx);
                    }),
            )
        });
    let files: gpui::AnyElement = if total == 0 {
        div().font_medium().child("none").into_any_element()
    } else {
        let rows = s.files.iter().enumerate().map(|(ix, f)| {
            let excluded = s.excluded.contains(&f.path);
            let st = state.clone();
            let path = f.path.clone();
            let accent = t.accent;
            div()
                .id(("commit-file", ix))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .py(px(4.))
                .rounded(radius::MD)
                .font_family(font::MONO)
                .text_scale(crate::theme::TextSize::Xs)
                .hover(move |s| s.bg(accent.opacity(0.5 * accent.a)))
                .when(editing, |el| {
                    el.child(
                        Checkbox::new(("commit-file-check", ix), !excluded).on_change(
                            move |checked, _, cx| {
                                st.update(cx, |s, _| {
                                    if checked {
                                        s.excluded.remove(&path);
                                    } else {
                                        s.excluded.insert(path.clone());
                                    }
                                });
                                refresh_dialogs(cx);
                            },
                        ),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .when(excluded, |el| el.text_color(t.muted_foreground))
                        .child(start_truncate(&f.path, 52)),
                )
                .map(|el| {
                    if excluded {
                        el.child(div().text_color(t.muted_foreground).child("Excluded"))
                    } else {
                        el.child(counts(f.additions, f.deletions, cx))
                    }
                })
        });
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .id("commit-files")
                    .h(px(176.))
                    .rounded(radius::LG)
                    .bg(list_surface(cx))
                    .border_1()
                    .border_color(ring)
                    .overflow_y_scroll()
                    .p(px(4.))
                    .child(div().flex().flex_col().gap(px(4.)).children(rows)),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .text_scale(crate::theme::TextSize::Xs)
                    .child(counts(sel_add, sel_del, cx)),
            )
            .into_any_element()
    };
    let summary = div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .p(px(12.))
        .rounded(radius::XL)
        .bg(summary_bg)
        .border_1()
        .border_color(ring)
        .text_scale(crate::theme::TextSize::Sm)
        .child(grid)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(files_header)
                .child(files),
        );
    let message = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .text_scale(crate::theme::TextSize::Sm)
                .font_medium()
                .child("Commit message (optional)"),
        )
        .child(TextareaField::new(&s.message));

    let submit = |feature: bool| {
        let st = state.clone();
        let card = card.clone();
        let ctx = ctx.clone();
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            let (message, paths) = {
                let s = st.read(cx);
                let message = s.message.read(cx).value().trim().to_string();
                let paths = (!s.excluded.is_empty())
                    .then(|| s.selected().into_iter().map(|f| f.path.clone()).collect());
                (message, paths)
            };
            Dialogs::close(id, window, cx);
            let _ = card.update(cx, |this, cx| {
                this.run_git_action(
                    &ctx,
                    ActionRequest {
                        action: GitAction::Commit,
                        commit_message: (!message.is_empty()).then_some(message),
                        feature_branch: feature,
                        file_paths: paths,
                    },
                    cx,
                )
            });
        }
    };
    let can_feature = n_selected > 0 && s.branch.is_some();
    Dialog::new()
        .title("Commit changes")
        .description(
            "Review and confirm your commit. Leave the message blank to auto-generate one.",
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(summary)
                .child(message),
        )
        .footer(
            DialogFooter::new()
                .bare()
                .child(
                    Button::new("commit-cancel")
                        .label("Cancel")
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, window, cx| Dialogs::close(id, window, cx)),
                )
                .child(
                    Button::new("commit-new-branch")
                        .label("Commit on new branch")
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Sm)
                        .disabled(!can_feature)
                        .on_click(submit(true)),
                )
                .child(
                    Button::new("commit-submit")
                        .label("Commit")
                        .size(ButtonSize::Sm)
                        .disabled(n_selected == 0)
                        .on_click(submit(false)),
                ),
        )
}

// ---- checkout pull request -----------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Preparing {
    Local,
    Worktree,
}

pub(super) struct CheckoutPrDialog {
    input: Entity<InputState>,
    resolving: bool,
    resolved: Option<Result<PullRequestInfo, String>>,
    preparing: Option<Preparing>,
    error: Option<String>,
    typed: bool,
    cwd: PathBuf,
    generation: u64,
    id: usize,
    _debounce: Option<Task<()>>,
    _subscription: Option<gpui::Subscription>,
}

impl CheckoutPrDialog {
    fn resolve(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).value().trim().to_string();
        self.generation += 1;
        let generation = self.generation;
        self.resolved = None;
        self.error = None;
        if query.is_empty() {
            self.resolving = false;
            if self.typed {
                self.error =
                    Some("Paste a pull request URL, checkout command, or enter 123 / #123.".into());
            }
            return;
        }
        let Some(reference) = logic::parse_pr_reference(&query) else {
            self.resolving = false;
            self.error = Some("Use a pull request URL, checkout command, 123, or #123.".into());
            return;
        };
        self.resolving = true;
        let cwd = self.cwd.clone();
        self._debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(450))
                .await;
            let task = cx.background_spawn(async move {
                if !cwd.is_dir() {
                    return Err(super::git::MISSING_FOLDER.to_string());
                }
                tern_git::remote::resolve_pull_request(&cwd, &reference).map_err(|e| e.to_string())
            });
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.resolving = false;
                    this.resolved = Some(result);
                    cx.notify();
                    refresh_dialogs(cx);
                }
            });
        }));
    }
}

fn pr_state_color(state: &str, cx: &App) -> gpui::Hsla {
    let dark = cx.tern_theme().is_dark();
    match state {
        "open" => {
            if dark {
                gpui::rgba(0x5ee9b5e6).into()
            } else {
                gpui::rgb(0x009966).into()
            }
        }
        "merged" => {
            if dark {
                gpui::rgba(0xc4b4ffe6).into()
            } else {
                gpui::rgb(0x7f22fe).into()
            }
        }
        _ => {
            if dark {
                gpui::rgba(0x9f9fa9cc).into()
            } else {
                gpui::rgb(0x71717b).into()
            }
        }
    }
}

/// `gh pr checkout` (Local) or a fetch of the PR head (Worktree).
fn prepare_pr(cwd: &Path, pr: &PullRequestInfo, mode: Preparing) -> Result<(), String> {
    let run = |program: &str, args: &[&str]| -> Result<(), String> {
        let out = std::process::Command::new(program)
            .args(args)
            .current_dir(cwd)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    };
    let number = pr.number.to_string();
    match mode {
        Preparing::Local => run("gh", &["pr", "checkout", &number]),
        Preparing::Worktree => {
            if tern_git::refs::branch_exists(cwd, &pr.head).unwrap_or(false) {
                return Ok(());
            }
            let remote = tern_git::refs::primary_remote(cwd)
                .ok()
                .flatten()
                .unwrap_or_else(|| "origin".into());
            let spec = format!("pull/{number}/head:{}", pr.head);
            run("git", &["fetch", "--quiet", &remote, &spec])
        }
    }
}

impl DetailsCard {
    pub(super) fn open_checkout_pr_dialog(
        &mut self,
        ctx: &Ctx,
        reference: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("PR URL, checkout command, or #42");
            s.set_value(reference.clone(), window, cx);
            s
        });
        let cwd = PathBuf::from(&ctx.project.workspace_root);
        let state = cx.new(|cx| {
            let mut d = CheckoutPrDialog {
                input: input.clone(),
                resolving: false,
                resolved: None,
                preparing: None,
                error: None,
                typed: false,
                cwd,
                generation: 0,
                id: 0,
                _debounce: None,
                _subscription: None,
            };
            d.resolve(cx);
            d
        });
        let sub = {
            let state = state.clone();
            cx.subscribe_in(
                &input,
                window,
                move |_, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::Change => state.update(cx, |s, cx| {
                            s.typed = true;
                            s.resolve(cx);
                        }),
                        InputEvent::PressEnter { .. } => {
                            let ready = {
                                let s = state.read(cx);
                                !s.resolving && s.preparing.is_none()
                            };
                            if ready {
                                run_checkout(state.clone(), Preparing::Worktree, window, cx);
                            }
                        }
                        _ => {}
                    }
                    refresh_dialogs(cx);
                },
            )
        };
        state.update(cx, |s, _| s._subscription = Some(sub));
        input.update(cx, |i, cx| {
            i.focus(window, cx);
            i.select_all(window, cx);
        });
        let project_id = ctx.project.id.clone();
        let app = self.app.clone();
        let st = state.clone();
        let id = Dialogs::open(window, cx, move |_, cx| {
            build_checkout_pr(&st, &project_id, &app, cx)
        });
        state.update(cx, |s, _| s.id = id);
    }
}

fn run_checkout(
    state: Entity<CheckoutPrDialog>,
    mode: Preparing,
    _window: &mut Window,
    cx: &mut App,
) {
    let Some(Ok(pr)) = state.read(cx).resolved.clone() else {
        return;
    };
    let cwd = state.read(cx).cwd.clone();
    let project = state_project(&state, cx);
    state.update(cx, |s, _| {
        s.preparing = Some(mode);
        s.error = None;
    });
    refresh_dialogs(cx);
    let pr2 = pr.clone();
    let task = cx.background_spawn(async move { prepare_pr(&cwd, &pr2, mode) });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = cx.update(|cx| {
            let id = state.read(cx).id;
            match result {
                Ok(()) => {
                    state.update(cx, |s, _| s.preparing = None);
                    if let Some((project_id, app)) = project {
                        let ws = DraftWorkspace {
                            env_mode: if mode == Preparing::Worktree {
                                ThreadEnvMode::Worktree
                            } else {
                                ThreadEnvMode::Local
                            },
                            branch: Some(pr.head.clone()),
                            worktree_path: None,
                            start_from_origin: false,
                        };
                        app.update(cx, |s, cx| {
                            s.set_draft_workspace(&project_id, ws, cx);
                            s.navigate(Route::Draft(project_id.clone()), cx);
                        });
                    }
                    if let Some(host) = Dialogs::global(cx) {
                        host.update(cx, |_, cx| cx.notify());
                    }
                    close_dialog(id, cx);
                }
                Err(e) => {
                    state.update(cx, |s, _| {
                        s.preparing = None;
                        s.error = Some(if e.is_empty() {
                            "Failed to prepare pull request thread.".into()
                        } else {
                            e
                        });
                    });
                    refresh_dialogs(cx);
                }
            }
        });
    })
    .detach();
}

/// The project the dialog was opened for, kept on the app state entity.
fn state_project(
    _state: &Entity<CheckoutPrDialog>,
    cx: &App,
) -> Option<(String, Entity<crate::app_state::AppState>)> {
    let app = crate::app_state::AppState::global(cx);
    let project = match app.read(cx).chat_route() {
        Route::Draft(p) => Some(p.clone()),
        Route::Thread(id) => crate::store::Store::global(cx)
            .read(cx)
            .snapshot()
            .thread(id)
            .map(|t| t.thread.project_id.clone()),
        _ => crate::store::Store::global(cx)
            .read(cx)
            .snapshot()
            .projects
            .first()
            .map(|p| p.id.clone()),
    }?;
    Some((project, app))
}

fn close_dialog(id: usize, cx: &mut App) {
    if let Some(host) = Dialogs::global(cx) {
        host.update(cx, |host, cx| {
            let _ = host;
            cx.notify();
        });
    }
    // `Dialogs::close` only needs the app; it ignores the window.
    let windows: Vec<_> = cx.windows();
    if let Some(w) = windows.first() {
        let _ = w.update(cx, |_, window, cx| Dialogs::close(id, window, cx));
    }
}

fn build_checkout_pr(
    state: &Entity<CheckoutPrDialog>,
    _project_id: &str,
    _app: &Entity<crate::app_state::AppState>,
    cx: &mut App,
) -> Dialog {
    let t = *cx.tokens();
    let s = state.read(cx);
    let id = s.id;
    let preparing = s.preparing;
    let ready = matches!(s.resolved, Some(Ok(_))) && preparing.is_none() && !s.resolving;
    let error = s.error.clone().or_else(|| match &s.resolved {
        Some(Err(e)) => Some(e.clone()),
        _ => None,
    });
    let card = match &s.resolved {
        Some(Ok(pr)) => Some(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(px(12.))
                .p(px(12.))
                .rounded(radius::XL)
                .border_1()
                .border_color(t.border.opacity(t.border.a * 0.7))
                .bg(t.muted.opacity(0.24))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(
                            div()
                                .text_scale(crate::theme::TextSize::Sm)
                                .font_medium()
                                .truncate()
                                .child(pr.title.clone()),
                        )
                        .child(
                            div()
                                .text_scale(crate::theme::TextSize::Xs)
                                .text_color(t.muted_foreground)
                                .child(format!("#{} · {} to {}", pr.number, pr.head, pr.base)),
                        ),
                )
                .child(
                    div()
                        .text_scale(crate::theme::TextSize::Xs)
                        .text_color(pr_state_color(&pr.state, cx))
                        .child(capitalize(&pr.state)),
                ),
        ),
        _ => None,
    };
    let st_local = state.clone();
    let st_wt = state.clone();
    Dialog::new()
        .title("Checkout pull request")
        .description("Resolve a GitHub pull request, then create the draft thread in the main repo or in a dedicated worktree.")
        .max_width(px(576.))
        .close_button(preparing.is_none())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .text_scale(crate::theme::TextSize::Xs)
                                .font_medium()
                                .child(Icon::new(IconName::Github).size(px(14.)))
                                .child("Pull request"),
                        )
                        .child(TextField::new(&s.input).size(InputSize::Default)),
                )
                .when(s.resolving, |el| {
                    el.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_scale(crate::theme::TextSize::Xs)
                            .text_color(t.muted_foreground)
                            .child(Spinner::new().size_px(px(12.)))
                            .child("Resolving pull request..."),
                    )
                })
                .children(card)
                .children(error.map(|e| {
                    div()
                        .text_scale(crate::theme::TextSize::Xs)
                        .text_color(t.destructive_foreground)
                        .child(e)
                })),
        )
        .footer(
            DialogFooter::new()
                .child(
                    Button::new("pr-cancel")
                        .label("Cancel")
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Sm)
                        .disabled(preparing.is_some())
                        .on_click(move |_, window, cx| Dialogs::close(id, window, cx)),
                )
                .child(
                    Button::new("pr-local")
                        .label(if preparing == Some(Preparing::Local) {
                            "Preparing local..."
                        } else {
                            "Local"
                        })
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Sm)
                        .disabled(!ready)
                        .on_click(move |_, window, cx| {
                            run_checkout(st_local.clone(), Preparing::Local, window, cx)
                        }),
                )
                .child(
                    Button::new("pr-worktree")
                        .label(if preparing == Some(Preparing::Worktree) {
                            "Preparing worktree..."
                        } else {
                            "Worktree"
                        })
                        .size(ButtonSize::Sm)
                        .disabled(!ready)
                        .on_click(move |_, window, cx| {
                            run_checkout(st_wt.clone(), Preparing::Worktree, window, cx)
                        }),
                ),
        )
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[allow(dead_code)]
fn toast_error(title: &str, description: String, cx: &mut App) {
    Toasts::push(Toast::error(title.to_string()).description(description), cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_truncate_from_the_start() {
        assert_eq!(start_truncate("src/a.rs", 20), "src/a.rs");
        assert_eq!(
            start_truncate("crates/app/src/views/details/mod.rs", 12),
            "…ails/mod.rs"
        );
        assert_eq!(capitalize("merged"), "Merged");
    }
}

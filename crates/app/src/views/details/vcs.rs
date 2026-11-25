//! The Version Control section: branch row and picker (§4a.5-4a.8), the git actions
//! split control (§4b.1-4b.3, §4b.7), the Changes row (§4b.4).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_base::Align;
use tern_core::{Command, ThreadEnvMode};
use tern_git::refs::Branch;

use super::branch_picker::{BranchPicker, Pick};
use super::git::{self, ActionRequest};
use super::logic::{self, GitAction, GitStatus, MenuId, NoticeTone, QuickAction, QuickIcon};
use super::workspace::custom_row;
use super::{Ctx, Density, DetailsCard, Running, Success, style};
use crate::app_state::DraftWorkspace;
use crate::theme::{ActiveTheme as _, TextExt as _, font};
use crate::ui::menu::{Menu, MenuItem, open_menu};
use crate::ui::popup::{PopupAnchor, PopupLayer};
use crate::ui::{
    Icon, IconName, Spinner, Toast, Toasts, Tooltip, TooltipExt as _, TooltipSide, bounds_probe,
};

const BRANCH_PICKER: &str = "details-branch-picker";
const GIT_MENU: &str = "details-git-menu";

pub fn quick_icon(icon: QuickIcon) -> IconName {
    match icon {
        QuickIcon::Commit => IconName::GitCommitHorizontal,
        QuickIcon::Push => IconName::CloudUpload,
        QuickIcon::Pull => IconName::CloudDownload,
        QuickIcon::Provider => IconName::Github,
        QuickIcon::Info => IconName::Info,
    }
}

impl DetailsCard {
    fn status(&self) -> Option<&GitStatus> {
        self.git.as_ref().and_then(|g| g.status())
    }

    /// (rows at full density, rows at compact density).
    pub(super) fn vcs_row_count(&self, _ctx: &Ctx, _cx: &App) -> (usize, usize) {
        (3, 2)
    }

    pub(super) fn render_vcs(
        &mut self,
        ctx: &Ctx,
        density: Density,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Vec<AnyElement>> {
        let g = self.git.as_ref()?;
        let is_repo = g.is_repo();
        let mut rows = Vec::new();
        if is_repo {
            rows.push(self.branch_row(ctx, cx));
        }
        let quiet = self.quiet_empty_draft(ctx);
        if let Some(row) = self.git_row(ctx, density, quiet, cx) {
            rows.push(row);
        }
        if is_repo && !quiet {
            rows.push(self.changes_row(ctx, cx));
        }
        (!rows.is_empty()).then_some(rows)
    }

    /// `quietEmptyDraft`: a draft with nothing to do hides its git rows.
    fn quiet_empty_draft(&self, ctx: &Ctx) -> bool {
        let Some(g) = &self.git else { return false };
        let qa = logic::resolve_quick_action(g.status(), self.running.is_some());
        let (_, adds, dels) = logic::changes_totals(g.status());
        ctx.is_draft()
            && self.running.is_none()
            && g.error().is_none()
            && g.read.is_some()
            && qa.hint().is_some()
            && adds == 0
            && dels == 0
    }

    // ---- branch ------------------------------------------------------------

    /// (label, branch name) for the branch row.
    fn branch_label(&self, ctx: &Ctx) -> (String, Option<String>) {
        let status_branch = self.status().and_then(|s| s.branch.clone());
        if ctx.choosing_base() {
            let draft = ctx.draft.clone().unwrap_or_default();
            let branch = draft.branch.clone().or(status_branch);
            let label =
                logic::branch_trigger_label(branch.as_deref(), true, draft.start_from_origin, true);
            return (label, branch);
        }
        let branch = self
            .hold_branch
            .clone()
            .or(status_branch)
            .or_else(|| ctx.thread.as_ref().and_then(|t| t.branch.clone()))
            .or_else(|| ctx.draft.as_ref().and_then(|d| d.branch.clone()));
        (
            logic::branch_trigger_label(branch.as_deref(), false, false, false),
            branch,
        )
    }

    fn branch_row(&mut self, ctx: &Ctx, cx: &mut Context<Self>) -> AnyElement {
        let (label, _) = self.branch_label(ctx);
        let open = PopupLayer::is_open(&BRANCH_PICKER.into(), cx);
        let disabled = self.ref_pending;
        let probe = self.probe("branch");
        let content = div()
            .flex()
            .items_center()
            .child(style::icon(IconName::GitBranch, cx))
            .child(style::label(logic::middle_truncate(&label, 30)));
        let ctx = ctx.clone();
        style::select_row("details-branch", open, disabled, content, cx)
            .relative()
            .child(bounds_probe(probe))
            .when(!disabled, |el| {
                el.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        if PopupLayer::is_open(&BRANCH_PICKER.into(), cx) {
                            PopupLayer::close(window, cx);
                        } else {
                            this.open_branch_picker(&ctx, window, cx);
                        }
                    }),
                )
            })
            .into_any_element()
    }

    pub(super) fn open_branch_picker(
        &mut self,
        ctx: &Ctx,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = self.bounds("branch");
        let (_, current) = self.branch_label(ctx);
        let card = cx.entity().downgrade();
        let ctx2 = ctx.clone();
        let picker = cx.new(|cx| {
            BranchPicker::new(
                ctx.cwd(),
                ctx.project.workspace_root.clone(),
                current,
                ctx.choosing_base(),
                ctx.draft.as_ref().map(|d| d.start_from_origin),
                ctx.running,
                bounds.size.width,
                move |pick, window, cx| {
                    let ctx = ctx2.clone();
                    let _ = card.update(cx, |this, cx| this.apply_pick(&ctx, pick, window, cx));
                },
                window,
                cx,
            )
        });
        let focus = picker.read(cx).focus_handle(cx);
        // Opening it refreshes local status.
        self.refresh_status(false, cx);
        PopupLayer::open(
            BRANCH_PICKER.into(),
            PopupAnchor::below(bounds, Align::Start),
            picker.into(),
            Some(focus),
            window,
            cx,
        );
        cx.notify();
    }

    fn apply_pick(&mut self, ctx: &Ctx, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        match pick {
            Pick::StartFromOrigin(on) => {
                if let Some(draft) = &ctx.draft {
                    let ws = DraftWorkspace {
                        start_from_origin: on,
                        ..draft.clone()
                    };
                    let id = ctx.project.id.clone();
                    self.app
                        .update(cx, |s, cx| s.set_draft_workspace(&id, ws, cx));
                }
            }
            Pick::CheckoutPr(reference) => {
                PopupLayer::close(window, cx);
                self.open_checkout_pr_dialog(ctx, reference, window, cx);
            }
            Pick::Ref(branch) => {
                PopupLayer::close(window, cx);
                self.select_ref(ctx, branch, cx);
            }
            Pick::Create(name) => {
                PopupLayer::close(window, cx);
                self.switch_to(ctx, name, ctx.worktree_path().map(str::to_string), true, cx);
            }
        }
    }

    /// What picking a ref does (§4a.8).
    fn select_ref(&mut self, ctx: &Ctx, branch: Branch, cx: &mut Context<Self>) {
        if ctx.choosing_base() {
            if let Some(draft) = &ctx.draft {
                let ws = DraftWorkspace {
                    branch: Some(branch.name.clone()),
                    ..draft.clone()
                };
                let id = ctx.project.id.clone();
                self.app
                    .update(cx, |s, cx| s.set_draft_workspace(&id, ws, cx));
            }
            return;
        }
        let root = ctx.project.workspace_root.trim_end_matches('/').to_string();
        let current_wt = ctx.worktree_path().map(str::to_string);
        let is_default = self
            .status()
            .and_then(|s| s.default_branch.as_deref())
            .is_some_and(|d| d == branch.name)
            || branch.is_default;
        let next_wt: Option<String> = match &branch.worktree_path {
            Some(p) if p.trim_end_matches('/') == root => None,
            Some(p) => Some(p.clone()),
            None if current_wt.is_some() && is_default => None,
            None => current_wt.clone(),
        };
        if next_wt != current_wt && ctx.running {
            Toasts::push(
                Toast::warning("Stop the current turn to switch worktrees."),
                cx,
            );
            return;
        }
        // Reusing another worktree only changes the thread's metadata.
        if branch.worktree_path.is_some() && next_wt.is_some() && next_wt != current_wt {
            self.record_branch(ctx, Some(branch.name.clone()), next_wt, cx);
            return;
        }
        self.switch_to(ctx, branch.name, next_wt, false, cx);
    }

    /// Checks out (or creates) `name` in the target checkout, then records it.
    fn switch_to(
        &mut self,
        ctx: &Ctx,
        name: String,
        worktree: Option<String>,
        create: bool,
        cx: &mut Context<Self>,
    ) {
        let cwd = PathBuf::from(
            worktree
                .clone()
                .unwrap_or_else(|| ctx.project.workspace_root.clone()),
        );
        let previous_hold = self.hold_branch.replace(name.clone());
        self.ref_pending = true;
        cx.notify();
        let task = cx.background_spawn(async move {
            if create {
                git::create_ref(&cwd, &name)
            } else {
                git::switch_ref(&cwd, &name)
            }
        });
        let ctx = ctx.clone();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.ref_pending = false;
                match result {
                    Ok(now) => {
                        let branch = now.or(this.hold_branch.clone());
                        this.hold_branch = branch.clone();
                        this.record_branch(&ctx, branch, worktree, cx);
                    }
                    Err(e) => {
                        this.hold_branch = previous_hold;
                        let title = if create {
                            "Failed to create and switch ref."
                        } else {
                            "Failed to switch ref."
                        };
                        Toasts::push(Toast::error(title).description(e), cx);
                    }
                }
                this.refresh_status(true, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Writes `{branch, worktreePath}` to the thread, or the draft.
    fn record_branch(
        &mut self,
        ctx: &Ctx,
        branch: Option<String>,
        worktree: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(thread) = &ctx.thread {
            self.store.read(cx).send(Command::SetThreadBranch {
                thread_id: thread.id.clone(),
                branch,
                worktree_path: worktree,
            });
        } else if let Some(draft) = &ctx.draft {
            let env_mode = if worktree.is_some()
                || (draft.env_mode == ThreadEnvMode::Worktree && draft.worktree_path.is_none())
            {
                ThreadEnvMode::Worktree
            } else {
                ThreadEnvMode::Local
            };
            let ws = DraftWorkspace {
                env_mode,
                branch,
                worktree_path: worktree,
                ..draft.clone()
            };
            let id = ctx.project.id.clone();
            self.app
                .update(cx, |s, cx| s.set_draft_workspace(&id, ws, cx));
        }
    }

    // ---- git actions -----------------------------------------------------------

    fn git_row(
        &mut self,
        ctx: &Ctx,
        density: Density,
        quiet: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let g = self.git.as_ref()?;
        if !g.is_repo() {
            let cwd = g.cwd.clone();
            return Some(
                style::row("details-git-init", false, cx)
                    .child(
                        Icon::new(IconName::GitBranchPlus)
                            .size(px(14.))
                            .color(cx.tokens().muted_foreground),
                    )
                    .child(style::label("Initialize Git").ml(px(2.)))
                    .on_click(cx.listener(move |this, _, _, cx| this.init_git(cwd.clone(), cx)))
                    .into_any_element(),
            );
        }
        let success_live = self
            .success
            .as_ref()
            .is_some_and(|s| s.at.elapsed() < Duration::from_secs(10));
        if quiet || (density != Density::Full && self.running.is_none() && !success_live) {
            return None;
        }
        let status = g.status().cloned();
        let error = g.error().map(str::to_string);
        let busy = self.running.is_some();
        let qa = logic::resolve_quick_action(status.as_ref(), busy);
        let menu = logic::git_menu(status.as_ref(), error.as_deref(), busy);
        let open = PopupLayer::is_open(&GIT_MENU.into(), cx);
        let probe = self.probe("git");
        let t = *cx.tokens();

        let mut group = style::split_group("details-git", open, busy, cx)
            .relative()
            .child(bounds_probe(probe));
        if let Some(running) = &self.running {
            let elapsed = logic::format_elapsed(running.started.elapsed().as_secs());
            group = group.child(
                style::split_primary("details-git-primary", false, cx)
                    .h_auto()
                    .min_h(style::ROW_HEIGHT)
                    .py(px(6.))
                    .cursor_default()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .w_full()
                            .h(px(18.))
                            .gap(px(12.))
                            .child(div().mx(px(-2.)).child(Spinner::new().muted()))
                            .child(style::label(running.status.clone()).flex_1())
                            .child(
                                div()
                                    .text_scale(crate::theme::TextSize::Xs2)
                                    .font_weight(font::NORMAL)
                                    .text_color(t.muted_foreground)
                                    .child(elapsed),
                            ),
                    ),
            );
            return Some(group.into_any_element());
        }
        if let Some(success) = self.success.as_ref().filter(|_| success_live) {
            group = group.child(
                style::split_primary("details-git-primary", false, cx)
                    .h_auto()
                    .min_h(style::ROW_HEIGHT)
                    .py(px(6.))
                    .cursor_default()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .w_full()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .h(px(18.))
                                    .child(
                                        Icon::new(IconName::Check).size(px(14.)).color(t.success),
                                    )
                                    .child(style::label(success.title.clone())),
                            )
                            .when_some(success.description.clone(), |el, d| {
                                el.child(
                                    style::label(d)
                                        .pl(px(26.))
                                        .pt(px(2.))
                                        .text_scale(crate::theme::TextSize::Xs2)
                                        .font_weight(font::NORMAL)
                                        .text_color(t.muted_foreground),
                                )
                            }),
                    ),
            );
        } else {
            let label = qa.label();
            let icon = style::icon(quick_icon(qa.icon()), cx);
            let primary = match qa.hint() {
                Some(hint) => style::split_primary("details-git-primary", false, cx)
                    .opacity(crate::ui::DISABLED_OPACITY)
                    .cursor_not_allowed()
                    .child(icon)
                    .child(style::label(label))
                    .t3_tooltip(Tooltip::text(hint.to_string()).side(TooltipSide::Bottom)),
                None => {
                    let ctx = ctx.clone();
                    let qa2 = qa.clone();
                    style::split_primary("details-git-primary", false, cx)
                        .child(icon)
                        .child(style::label(label.clone()))
                        .t3_tooltip(Tooltip::text(label).side(TooltipSide::Top))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.run_quick_action(&ctx, &qa2, window, cx)
                        }))
                }
            };
            group = group.child(primary);
        }
        let usable = menu.usable();
        let ctx2 = ctx.clone();
        group = group
            .child(style::split_separator("details-git", open, cx))
            .child(
                style::split_secondary("details-git-chevron", open, !usable, cx)
                    .t3_tooltip(Tooltip::text("Git action options").side(TooltipSide::Top))
                    .when(!usable, |el| {
                        el.on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                            if *hovered {
                                this.refresh_status(true, cx);
                            }
                        }))
                    })
                    .when(usable, |el| {
                        el.on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                                if PopupLayer::is_open(&GIT_MENU.into(), cx) {
                                    PopupLayer::close(window, cx);
                                } else {
                                    this.open_git_menu(&ctx2, window, cx);
                                }
                            }),
                        )
                    }),
            );
        Some(group.into_any_element())
    }

    pub(super) fn open_git_menu(&mut self, ctx: &Ctx, window: &mut Window, cx: &mut Context<Self>) {
        let Some(g) = &self.git else { return };
        let bounds = self.bounds("git");
        let status = g.status().cloned();
        let menu_model = logic::git_menu(status.as_ref(), g.error(), self.running.is_some());
        self.refresh_status(true, cx);
        let this = cx.entity().downgrade();
        let mut menu = Menu::new().min_width(bounds.size.width);
        for item in menu_model.items {
            let icon = match item.id {
                MenuId::Commit => IconName::GitCommitHorizontal,
                MenuId::Push | MenuId::Publish => IconName::CloudUpload,
                MenuId::Pr => IconName::Github,
            };
            if item.enabled {
                let this = this.clone();
                let ctx = ctx.clone();
                let id = item.id;
                menu = menu.item(MenuItem::new(item.label).icon(icon).on_select(
                    move |window, cx| {
                        let _ =
                            this.update(cx, |this, cx| this.run_menu_item(&ctx, id, window, cx));
                    },
                ));
            } else {
                let reason = item.disabled_reason.unwrap_or_default();
                let label = item.label.clone();
                let ix = item.id as usize;
                menu = menu.item(MenuItem::custom(move |_, cx| {
                    let t = *cx.tokens();
                    custom_row(("details-git-disabled", ix), true, cx)
                        .cursor_not_allowed()
                        .child(
                            Icon::new(icon)
                                .size(px(16.))
                                .color(t.muted_foreground)
                                .opacity(0.8),
                        )
                        .child(div().flex_1().min_w_0().child(label.clone()))
                        .t3_tooltip(Tooltip::text(reason.clone()).side(TooltipSide::Left))
                        .into_any_element()
                }));
            }
        }
        for (tone, text) in menu_model.notices {
            menu = menu.item(MenuItem::custom(move |_, cx| {
                let t = *cx.tokens();
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .text_scale(crate::theme::TextSize::Xs)
                    .text_color(match tone {
                        NoticeTone::Warning => t.warning_foreground,
                        NoticeTone::Destructive => t.destructive_foreground,
                    })
                    .child(text.clone())
                    .into_any_element()
            }));
        }
        open_menu(
            GIT_MENU.into(),
            PopupAnchor::below(bounds, Align::End),
            menu,
            window,
            cx,
        );
        cx.notify();
    }

    fn run_menu_item(
        &mut self,
        ctx: &Ctx,
        id: MenuId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match id {
            MenuId::Commit => self.open_commit_dialog(ctx, window, cx),
            MenuId::Push => self.start_action(ctx, GitAction::Push, window, cx),
            MenuId::Pr => self.start_action(ctx, GitAction::CreatePr, window, cx),
            MenuId::Publish => publish_unavailable(cx),
        }
    }

    fn run_quick_action(
        &mut self,
        ctx: &Ctx,
        qa: &QuickAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match qa {
            QuickAction::Run { action, .. } => self.start_action(ctx, *action, window, cx),
            QuickAction::Pull => self.run_pull(ctx, cx),
            QuickAction::OpenPublish => publish_unavailable(cx),
            QuickAction::Hint { .. } => {}
        }
    }

    /// Runs `action`, stopping at the default-branch dialog when it applies.
    fn start_action(
        &mut self,
        ctx: &Ctx,
        action: GitAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(status) = self.status().cloned()
            && logic::needs_default_branch_confirm(action, &status)
        {
            self.open_default_branch_dialog(ctx, action, &status, window, cx);
            return;
        }
        self.run_git_action(
            ctx,
            ActionRequest {
                action,
                commit_message: None,
                feature_branch: false,
                file_paths: None,
            },
            cx,
        );
    }

    fn start_running(&mut self, status: &str, cx: &mut Context<Self>) {
        self.running = Some(Running {
            status: status.into(),
            started: Instant::now(),
        });
        self.success = None;
        // Tick the elapsed time.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let live = this.update(cx, |this, cx| {
                    cx.notify();
                    this.running.is_some()
                });
                if !matches!(live, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// `runGitActionWithToast` (§4b.7).
    pub(super) fn run_git_action(&mut self, ctx: &Ctx, req: ActionRequest, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        let cwd = ctx.cwd();
        let userdata = self.store.read(cx).engine().map(|e| e.paths().userdata());
        self.start_running("Starting source control action...", cx);
        enum Msg {
            Phase(String),
            Done(Result<git::ActionOutcome, String>),
        }
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Msg>();
        cx.background_spawn(async move {
            let opts = git::textgen_options(&cwd, userdata.as_deref());
            let result = git::run_action(&cwd, &req, &opts, &|phase| {
                let _ = tx.unbounded_send(Msg::Phase(phase.to_string()));
            });
            let _ = tx.unbounded_send(Msg::Done(result));
        })
        .detach();
        let thread = ctx.thread.clone();
        cx.spawn(async move |this, cx| {
            while let Some(msg) = rx.next().await {
                let done = matches!(msg, Msg::Done(_));
                let _ = this.update(cx, |this, cx| {
                    match msg {
                        Msg::Phase(p) => {
                            if let Some(r) = &mut this.running {
                                r.status = p;
                            }
                        }
                        Msg::Done(Ok(outcome)) => {
                            this.running = None;
                            if let (Some(branch), Some(thread)) = (&outcome.created_branch, &thread)
                            {
                                this.store.read(cx).send(Command::SetThreadBranch {
                                    thread_id: thread.id.clone(),
                                    branch: Some(branch.clone()),
                                    worktree_path: thread.worktree_path.clone(),
                                });
                            }
                            this.success = Some(Success {
                                title: outcome.title,
                                description: outcome.description,
                                at: Instant::now(),
                            });
                            this.refresh_status(true, cx);
                        }
                        Msg::Done(Err(e)) => {
                            this.running = None;
                            let message = if e.trim().is_empty() {
                                "An error occurred.".to_string()
                            } else {
                                e
                            };
                            Toasts::push(
                                Toast::error("Action failed").description(message).sticky(),
                                cx,
                            );
                            this.refresh_status(true, cx);
                        }
                    }
                    cx.notify();
                });
                if done {
                    break;
                }
            }
        })
        .detach();
    }

    fn run_pull(&mut self, ctx: &Ctx, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        let cwd = ctx.cwd();
        self.start_running("Pulling latest changes...", cx);
        let task = cx.background_spawn(async move { git::pull(&cwd) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.running = None;
                match result {
                    Ok((title, description)) => {
                        this.success = Some(Success {
                            title,
                            description: Some(description),
                            at: Instant::now(),
                        })
                    }
                    Err(e) => {
                        Toasts::push(Toast::error("Pull failed").description(e).sticky(), cx);
                    }
                }
                this.refresh_status(true, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn init_git(&mut self, cwd: PathBuf, cx: &mut Context<Self>) {
        let task = cx.background_spawn(async move { git::init_repo(&cwd) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = result {
                    Toasts::push(Toast::error("Git initialization failed").description(e), cx);
                }
                this.refresh_status(false, cx);
            });
        })
        .detach();
    }

    // ---- changes -------------------------------------------------------------

    fn changes_row(&mut self, ctx: &Ctx, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let diff = cx.tern_theme().diff;
        let (base, adds, dels) = match (self.status(), ctx.diff_totals) {
            (None, Some(totals)) => (
                Some("Uncommitted".to_string()),
                totals.additions,
                totals.deletions,
            ),
            (status, _) => logic::changes_totals(status),
        };
        // Drafts have no diff panel to open.
        let disabled = ctx.is_draft();
        let app = self.app.clone();
        style::row("details-changes", disabled, cx)
            .child(style::icon(IconName::FileDiff, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_baseline()
                    .gap(px(6.))
                    .child(div().flex_none().child("Changes"))
                    .when_some(base, |el, base| {
                        el.child(
                            style::label(base)
                                .font_weight(font::NORMAL)
                                .text_color(t.muted_foreground),
                        )
                    })
                    .child(
                        div()
                            .ml_auto()
                            .flex()
                            .flex_none()
                            .items_baseline()
                            .gap(px(8.))
                            .pl(px(8.))
                            .font_family(font::MONO)
                            .child(
                                div()
                                    .text_color(if adds > 0 {
                                        diff.diff_addition_foreground
                                    } else {
                                        t.muted_foreground
                                    })
                                    .child(format!("+{}", logic::format_count(adds))),
                            )
                            .child(
                                div()
                                    .text_color(if dels > 0 {
                                        diff.diff_deletion_foreground
                                    } else {
                                        t.muted_foreground
                                    })
                                    .child(format!("-{}", logic::format_count(dels))),
                            ),
                    ),
            )
            .when(!disabled, |el| {
                el.on_click(move |_, _, cx| {
                    app.update(cx, |s, cx| s.set_right_panel_open(true, cx));
                })
            })
            .into_any_element()
    }
}

fn publish_unavailable(cx: &mut App) {
    Toasts::push(
        Toast::info("Publish repository")
            .description("Add a remote with `git remote add origin <url>`, then push."),
        cx,
    );
}

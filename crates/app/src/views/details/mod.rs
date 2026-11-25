//! The thread details card (spec/05 §1.9): the 280px glass card at the top right of
//! the chat canvas, toggled from the titlebar (`AppState::thread_details_open`).
//!
//! Sections, top to bottom:
//! - Workspace: the run context row (machine + checkout, the env mode picker for
//!   drafts, §4a.3), "Open in {editor}" (§5), project scripts (§6).
//! - Version Control: the branch row and picker (§4a.5-4a.8), the git actions split
//!   control with its menu and dialogs (§4b), the Changes row (§4b.4).
//!
//! Mounted by the shell: [`details_card`] returns the canvas-sized overlay for the
//! chat column (below the 40px header, above the terminal drawer). When the card
//! does not fit inline (the chat lane would drop under 640px, or the canvas is under
//! 160px tall), the titlebar toggle opens it as a popover instead.
//!
//! All git work runs in `cx.background_spawn`; failures are toasts.

mod branch_picker;
mod dialogs;
pub mod editors;
pub mod git;
pub mod logic;
mod script_editor;
mod style;
mod vcs;
mod workspace;

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, Size, StatefulInteractiveElement as _,
    Styled as _, Task, Window, canvas, div, prelude::FluentBuilder as _, px,
};
use tern_core::{FileTotals, Project, Thread, ThreadEnvMode};

use crate::app_state::{AppState, DraftWorkspace, Route};
use crate::store::{Store, StoreEvent};
use crate::theme::{ActiveTheme as _, layout, radius};
use crate::ui::popup::PopupLayer;
use git::StatusRead;
use logic::GitStatus;

/// Places the card's canvas in the chat column. The shell calls this from
/// `render_chat_column` with the drawer's height (0 when closed).
pub fn details_card(card: &Entity<DetailsCard>, drawer_height: Pixels) -> AnyElement {
    div()
        .absolute()
        .top(layout::TOPBAR_HEIGHT)
        .left_0()
        .right_0()
        .bottom(drawer_height)
        .child(card.clone())
        .into_any_element()
}

/// T3's chat lane rules (`chatCanvasLayout.ts`): 48px padding, 640px min lane, 32px
/// clearance between the lane and the card.
const LANE_PADDING: f32 = 48.;
const MIN_CHAT_WIDTH: f32 = 640.;
const CARD_CLEARANCE: f32 = 32.;

/// Whether the card fits inline in a canvas of `size` (`resolveThreadDetailsCardLayout`).
pub fn fits_inline(size: Size<Pixels>) -> bool {
    let w = f32::from(size.width);
    let h = f32::from(size.height);
    let room = w
        - f32::from(style::CARD_WIDTH)
        - f32::from(style::CARD_GAP)
        - CARD_CLEARANCE
        - LANE_PADDING;
    room >= MIN_CHAT_WIDTH && h - f32::from(style::CARD_GAP) * 2. >= 160.
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    Full,
    Compact,
    Essential,
}

/// What the card is about: a thread, or a draft in a project.
#[derive(Clone, Debug)]
pub(crate) struct Ctx {
    pub project: Project,
    pub thread: Option<Thread>,
    /// A run is live (switching worktrees is refused).
    pub running: bool,
    pub draft: Option<DraftWorkspace>,
    pub diff_totals: Option<FileTotals>,
}

impl Ctx {
    pub fn is_draft(&self) -> bool {
        self.thread.is_none()
    }

    pub fn worktree_path(&self) -> Option<&str> {
        match &self.thread {
            Some(t) => t.worktree_path.as_deref(),
            None => self.draft.as_ref().and_then(|d| d.worktree_path.as_deref()),
        }
    }

    /// `gitCwd`: the worktree if any, else the project root.
    pub fn cwd(&self) -> PathBuf {
        PathBuf::from(
            self.worktree_path()
                .unwrap_or(self.project.workspace_root.as_str()),
        )
    }

    pub fn env_mode(&self) -> ThreadEnvMode {
        match &self.draft {
            Some(d) => d.env_mode,
            None if self.worktree_path().is_some() => ThreadEnvMode::Worktree,
            None => ThreadEnvMode::Local,
        }
    }

    /// Choosing a base for a new worktree: worktree mode with no worktree yet.
    pub fn choosing_base(&self) -> bool {
        self.is_draft()
            && self.env_mode() == ThreadEnvMode::Worktree
            && self.worktree_path().is_none()
    }

    /// The run context is locked once the thread exists (`envLocked`).
    pub fn env_locked(&self) -> bool {
        !self.is_draft()
    }
}

/// The checkout the git rows describe.
pub(crate) struct GitView {
    pub cwd: PathBuf,
    /// None until the first read lands.
    pub read: Option<Result<StatusRead, String>>,
    pub generation: u64,
}

impl GitView {
    pub fn status(&self) -> Option<&GitStatus> {
        match &self.read {
            Some(Ok(StatusRead::Repo(s))) => Some(s),
            _ => None,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match &self.read {
            Some(Err(e)) => Some(e),
            _ => None,
        }
    }

    /// T3 assumes a repo until status says otherwise.
    pub fn is_repo(&self) -> bool {
        !matches!(self.read, Some(Ok(StatusRead::NotRepo)))
    }
}

/// A git action in flight (§4b.2 "Running").
pub(crate) struct Running {
    pub status: String,
    pub started: Instant,
}

/// The 10s inline success (§4b.7).
pub(crate) struct Success {
    pub title: String,
    pub description: Option<String>,
    pub at: Instant,
}

pub struct DetailsCard {
    pub(crate) app: Entity<AppState>,
    pub(crate) store: Entity<Store>,
    pub(crate) git: Option<GitView>,
    pub(crate) editors: Option<Vec<editors::Available>>,
    pub(crate) running: Option<Running>,
    pub(crate) success: Option<Success>,
    /// A branch switch is in flight (the trigger is disabled).
    pub(crate) ref_pending: bool,
    /// Optimistic branch label after a checkout, until status catches up.
    pub(crate) hold_branch: Option<String>,
    /// `lastInvokedScriptByProjectId`.
    pub(crate) last_script: HashMap<String, String>,
    probes: HashMap<&'static str, Rc<Cell<Bounds<Pixels>>>>,
    size: Size<Pixels>,
    _tasks: Vec<Task<()>>,
}

impl DetailsCard {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = AppState::global(cx);
        let store = Store::global(cx);
        cx.observe(&app, |this, _, cx| {
            this.sync_git(cx);
            cx.notify();
        })
        .detach();
        cx.observe(&store, |this, _, cx| {
            this.sync_git(cx);
            cx.notify();
        })
        .detach();
        cx.subscribe(&store, |this, _, event: &StoreEvent, cx| {
            if let StoreEvent::Thread(id) = event
                && this
                    .context(cx)
                    .and_then(|c| c.thread)
                    .is_some_and(|t| &t.id == id)
            {
                this.refresh_status(false, cx);
            }
        })
        .detach();
        if let Some(layer) = PopupLayer::global(cx) {
            cx.observe(&layer, |_, _, cx| cx.notify()).detach();
        }

        let detect = cx.background_spawn(async move { editors::detect() });
        let editors_task = cx.spawn(async move |this, cx| {
            let found = detect.await;
            let _ = this.update(cx, |this, cx| {
                this.editors = Some(found);
                cx.notify();
            });
        });
        // Local status every 5s while the card shows (T3 streams it).
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                let alive = this.update(cx, |this, cx| {
                    if this.visible(cx) && this.running.is_none() {
                        this.refresh_status(false, cx);
                    }
                    if this
                        .success
                        .as_ref()
                        .is_some_and(|s| s.at.elapsed() >= Duration::from_secs(10))
                    {
                        this.success = None;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        // Dev hook for testing against a real repository without the palette:
        // `TERN_DETAILS_ADD_PROJECT=/path/to/repo`.
        if let Ok(path) = std::env::var("TERN_DETAILS_ADD_PROJECT") {
            let store = store.read(cx);
            if !store
                .snapshot()
                .projects
                .iter()
                .any(|p| p.workspace_root == path)
            {
                store.send(tern_core::Command::AddProject { path });
            }
        }
        let shot = std::env::var("TERN_DETAILS_SHOT").ok();
        let mut tasks = vec![editors_task, poll];
        if let Some(shot) = shot {
            tasks.push(cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(2500))
                    .await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.open_for_screenshot(&shot, window, cx)
                });
            }));
        }
        let mut this = Self {
            app,
            store,
            git: None,
            editors: None,
            running: None,
            success: None,
            ref_pending: false,
            hold_branch: None,
            last_script: HashMap::new(),
            probes: HashMap::new(),
            size: Size::default(),
            _tasks: tasks,
        };
        this.sync_git(cx);
        this
    }

    /// A per-control bounds cell (for anchoring menus to rows).
    pub(crate) fn probe(&mut self, key: &'static str) -> Rc<Cell<Bounds<Pixels>>> {
        self.probes.entry(key).or_default().clone()
    }

    pub(crate) fn bounds(&self, key: &'static str) -> Bounds<Pixels> {
        self.probes.get(key).map(|c| c.get()).unwrap_or_default()
    }

    /// The thread or draft the chat column shows.
    pub(crate) fn context(&self, cx: &App) -> Option<Ctx> {
        let app = self.app.read(cx);
        let store = self.store.read(cx);
        let snapshot = store.snapshot();
        match app.chat_route() {
            Route::Thread(id) => {
                let shell = snapshot.thread(id)?;
                let project = snapshot.project(&shell.thread.project_id)?.clone();
                Some(Ctx {
                    project,
                    thread: Some(shell.thread.clone()),
                    running: shell.is_working(),
                    draft: None,
                    diff_totals: shell.diff_totals,
                })
            }
            Route::Draft(project) => {
                let project = snapshot.project(project)?.clone();
                let draft = app.draft_workspace(&project.id);
                Some(Ctx {
                    project,
                    thread: None,
                    running: false,
                    draft: Some(draft),
                    diff_totals: None,
                })
            }
            Route::Index => {
                let project = snapshot
                    .projects
                    .iter()
                    .filter(|p| p.archived_at.is_none())
                    .max_by_key(|p| p.updated_at)?
                    .clone();
                let draft = app.draft_workspace(&project.id);
                Some(Ctx {
                    project,
                    thread: None,
                    running: false,
                    draft: Some(draft),
                    diff_totals: None,
                })
            }
            _ => None,
        }
    }

    fn visible(&self, cx: &App) -> bool {
        let app = self.app.read(cx);
        if app.thread_details_inline {
            app.thread_details_open
        } else {
            app.thread_details_popover_open
        }
    }

    /// Points the git view at the context's checkout, reading it when it changed.
    fn sync_git(&mut self, cx: &mut Context<Self>) {
        let cwd = self.context(cx).map(|c| c.cwd());
        match (cwd, &self.git) {
            (None, _) => self.git = None,
            (Some(cwd), Some(g)) if g.cwd == cwd => {}
            (Some(cwd), _) => {
                self.git = Some(GitView {
                    cwd,
                    read: None,
                    generation: 0,
                });
                self.hold_branch = None;
                self.refresh_status(false, cx);
            }
        }
    }

    /// Re-reads git status in the background. `force_pr` also re-asks GitHub.
    pub(crate) fn refresh_status(&mut self, force_pr: bool, cx: &mut Context<Self>) {
        let Some(g) = &mut self.git else { return };
        g.generation += 1;
        let (cwd, generation) = (g.cwd.clone(), g.generation);
        let read = {
            let cwd = cwd.clone();
            cx.background_spawn(async move { git::read_status(&cwd, force_pr) })
        };
        cx.spawn(async move |this, cx| {
            let result = read.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(g) = &mut this.git
                    && g.cwd == cwd
                    && g.generation == generation
                {
                    if let (Some(hold), Ok(StatusRead::Repo(s))) = (&this.hold_branch, &result)
                        && s.branch.as_deref() == Some(hold.as_str())
                    {
                        this.hold_branch = None;
                    }
                    g.read = Some(result);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Which rows fit (`resolveThreadDetailsCardDensity`, estimated from row counts).
    fn density(&self, rows_full: usize, rows_compact: usize, height: f32) -> Density {
        let need = |rows: usize| rows as f32 * 32. + 2. * 18. + 1.;
        if need(rows_full) <= height {
            Density::Full
        } else if need(rows_compact) <= height {
            Density::Compact
        } else {
            Density::Essential
        }
    }

    /// `TERN_DETAILS_SHOT=<popup>` opens one popup or dialog after start-up, for
    /// screenshots: env, openin, scripts, branch, git, commit, script-editor, pr,
    /// confirm.
    fn open_for_screenshot(&mut self, which: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ctx) = self.context(cx) else { return };
        match which {
            "env" => self.open_env_menu(&ctx, window, cx),
            "openin" => self.open_editor_menu(&ctx, window, cx),
            "scripts" => self.open_scripts_menu(&ctx, window, cx),
            "branch" => self.open_branch_picker(&ctx, window, cx),
            "git" => self.open_git_menu(&ctx, window, cx),
            "commit" => self.open_commit_dialog(&ctx, window, cx),
            "script-editor" => self.open_script_editor(&ctx, None, window, cx),
            "pr" => self.open_checkout_pr_dialog(&ctx, "#1".into(), window, cx),
            "confirm" => {
                if let Some(status) = self.git.as_ref().and_then(|g| g.status()).cloned() {
                    self.open_default_branch_dialog(
                        &ctx,
                        logic::GitAction::CommitPush,
                        &status,
                        window,
                        cx,
                    );
                }
            }
            _ => {}
        }
    }

    fn render_card(
        &mut self,
        ctx: &Ctx,
        height: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.tern_theme().clone();
        let workspace_rows = self.workspace_rows(ctx, cx);
        let vcs_rows = self.vcs_row_count(ctx, cx);
        let density = self.density(
            workspace_rows.0 + vcs_rows.0,
            workspace_rows.1 + vcs_rows.1,
            height,
        );
        let workspace = self.render_workspace(ctx, density, window, cx);
        let vcs = self.render_vcs(ctx, density, window, cx);
        let has_workspace = workspace.is_some();
        let content = div()
            .flex()
            .flex_col()
            .children(workspace.map(|rows| {
                style::section(false, cx).child(div().flex().flex_col().children(rows))
            }))
            .children(vcs.map(|rows| {
                style::section(has_workspace && density == Density::Full, cx)
                    .child(div().flex().flex_col().children(rows))
            }));
        div()
            .id("thread-details-card")
            .w(style::CARD_WIDTH)
            .max_h(px(height))
            .overflow_y_scroll()
            .rounded(radius::XL2)
            .border_1()
            .border_color(theme.glass.dropdown_border)
            .bg(theme.glass.dropdown)
            .shadow(theme.shadows.popup())
            .text_color(theme.colors.foreground)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(content)
            .into_any_element()
    }
}

impl Render for DetailsCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let app_entity = self.app.clone();
        let probe = canvas(
            move |bounds, _, cx| {
                let inline = fits_inline(bounds.size);
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        if this.size != bounds.size {
                            this.size = bounds.size;
                            cx.notify();
                        }
                    });
                }
                app_entity.update(cx, |s, cx| {
                    if s.thread_details_inline != inline {
                        s.thread_details_inline = inline;
                        if inline {
                            s.thread_details_popover_open = false;
                        }
                        cx.notify();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let root = div().relative().size_full().child(probe);
        let Some(ctx) = self.context(cx) else {
            return root;
        };
        if self.size.width <= px(0.) || !self.visible(cx) {
            return root;
        }
        let inline = self.app.read(cx).thread_details_inline;
        let gap = f32::from(style::CARD_GAP);
        let height = if inline {
            f32::from(self.size.height) - gap * 2.
        } else {
            (f32::from(self.size.height) - 8.).max(160.)
        };
        let card = self.render_card(&ctx, height, window, cx);
        let app = self.app.clone();
        root.child(
            div()
                .absolute()
                .right(if inline { style::CARD_GAP } else { px(8.) })
                .top(if inline { style::CARD_GAP } else { px(4.) })
                .when(!inline, |el| {
                    el.on_mouse_down_out(move |e, _, cx| {
                        // The titlebar toggle closes it itself.
                        if e.position.y < layout::TOPBAR_HEIGHT {
                            return;
                        }
                        app.update(cx, |s, cx| {
                            s.thread_details_popover_open = false;
                            cx.notify();
                        })
                    })
                })
                .child(card),
        )
    }
}

/// The machine label: the host name unless it is generic (§4a.4).
pub(crate) fn machine_label() -> String {
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .or_else(|_| std::fs::read_to_string("/etc/hostname"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if host.is_empty()
        || host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("local")
    {
        "This device".into()
    } else {
        host
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    #[test]
    fn inline_needs_room_for_the_lane() {
        // 1440 window, 256 sidebar
        assert!(fits_inline(size(px(1184.), px(860.))));
        // the default 1100 window leaves 844
        assert!(!fits_inline(size(px(844.), px(740.))));
        assert!(!fits_inline(size(px(1184.), px(150.))));
    }
}

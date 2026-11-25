//! One actor per thread (spec/07 §0, §2). It owns the thread's working state, serializes
//! commands, provider events and effect results, persists each change in one transaction and
//! publishes the result. Slow work (provider session, git, title generation) runs in spawned
//! tasks that report back through the actor's own inbox.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tern_core::*;
use tern_providers::{
    ProviderAdapter, ProviderSessionHandle, RequestResponse, SessionConfig, TurnRequest,
};
use tern_store::{Db, stop_streaming};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::engine::{Inner, Published};
use crate::shell::{active_run, derive_shell, sanitize_title, seed_title, title_context, totals};

pub const USER_STOP_REASON: &str = "Stopped by user";
pub const FORCE_STOP_REASON: &str = "Force stopped by user";
pub const PROVIDER_STOP_REASON: &str = "Stopped by the provider";
pub const DELIVERY_FAILED: &str = "Tern could not confirm that your answer reached the agent. If the run stays stuck, stop it and try again.";
/// Live command output kept per item (not persisted).
const LIVE_OUTPUT_CAP: usize = 1 << 20;

pub(crate) enum ActorMsg {
    Command(Command),
    Launched {
        workspace: WorkspaceStrategy,
        message: Option<OutgoingMessage>,
    },
    Provider {
        session_id: String,
        event: ProviderEvent,
    },
    SessionClosed {
        session_id: String,
    },
    TurnStarted(Box<StartOutcome>),
    WorkspaceReady {
        run_id: Option<String>,
        result: Result<(String, String), String>,
    },
    CheckpointDone {
        run_id: Option<String>,
        checkpoint: Checkpoint,
        cwd: String,
    },
    TitleDone {
        result: Result<String, String>,
        regenerate: bool,
    },
    InterruptTimeout {
        run_id: String,
    },
    IdleCheck {
        generation: u64,
    },
    DeliveryFailed,
    RollbackDone {
        checkpoint_id: String,
        result: Result<(), String>,
    },
    /// Open a session for a run that is already live without starting a turn (demo threads
    /// seeded mid-run).
    ResumeSession {
        run_id: String,
    },
    CaptureMissing {
        run_id: String,
    },
    Shutdown,
}

pub(crate) struct LiveSession {
    pub id: String,
    pub instance_id: String,
    pub driver: Driver,
    pub handle: Arc<dyn ProviderSessionHandle>,
    pub capabilities: Capabilities,
    pub cwd: String,
    pub created_at: Ts,
}

pub(crate) struct StartOutcome {
    run_id: String,
    session: Option<LiveSession>,
    baseline: Option<(Checkpoint, String)>,
    result: Result<(), String>,
    resume_only: bool,
}

#[derive(Default)]
struct Dirty {
    thread: bool,
    binding: bool,
    runs: BTreeSet<String>,
    items: BTreeSet<String>,
    requests: BTreeSet<String>,
    plans: BTreeSet<String>,
    checkpoints: BTreeSet<String>,
    /// Something only the UI sees changed (session, live output).
    view: bool,
}

impl Dirty {
    fn any(&self) -> bool {
        self.thread
            || self.binding
            || self.view
            || !self.runs.is_empty()
            || !self.items.is_empty()
            || !self.requests.is_empty()
            || !self.plans.is_empty()
            || !self.checkpoints.is_empty()
    }
}

struct ThreadActor {
    inner: Arc<Inner>,
    tx: mpsc::UnboundedSender<ActorMsg>,
    d: ThreadDetail,
    binding: Option<ProviderBinding>,
    last_shell: ThreadShell,
    dirty: Dirty,
    checkpoint_cwds: HashMap<String, String>,
    publish_at: Option<Instant>,
    session: Option<LiveSession>,
    opening_session: Option<String>,
    restart_session: bool,
    pending_interrupt: HashSet<String>,
    capturing: Option<String>,
    rolling_back: bool,
    idle_gen: u64,
    gone: bool,
}

pub(crate) fn spawn(inner: Arc<Inner>, p: Published) -> mpsc::UnboundedSender<ActorMsg> {
    let (tx, rx) = mpsc::unbounded_channel();
    let actor = ThreadActor {
        inner: inner.clone(),
        tx: tx.clone(),
        d: (*p.detail).clone(),
        binding: p.binding,
        last_shell: p.shell,
        dirty: Dirty::default(),
        checkpoint_cwds: HashMap::new(),
        publish_at: None,
        session: None,
        opening_session: None,
        restart_session: false,
        pending_interrupt: HashSet::new(),
        capturing: None,
        rolling_back: false,
        idle_gen: 0,
        gone: false,
    };
    inner.handle.spawn(actor.run_loop(rx));
    tx
}

fn failure(class: FailureClass, message: impl Into<String>) -> ProviderFailure {
    ProviderFailure {
        class,
        message: message.into(),
        code: None,
        retryable: None,
        reset_at: None,
    }
}

pub fn checkpoint_id(thread_id: &str, ordinal: u32) -> String {
    format!("{thread_id}:checkpoint:{ordinal}")
}

fn is_streaming(body: &TurnItemBody) -> bool {
    matches!(
        body,
        TurnItemBody::AssistantMessage {
            streaming: true,
            ..
        } | TurnItemBody::Reasoning {
            streaming: true,
            ..
        } | TurnItemBody::ProposedPlan {
            streaming: true,
            ..
        }
    )
}

fn last_line(err: &str) -> String {
    let line = err
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("Title generation failed.");
    if line.chars().count() > 300 {
        format!("{}…", line.chars().take(299).collect::<String>())
    } else {
        line.to_string()
    }
}

impl ThreadActor {
    async fn run_loop(mut self, mut rx: mpsc::UnboundedReceiver<ActorMsg>) {
        loop {
            let msg = match self.publish_at {
                Some(at) => tokio::select! {
                    m = rx.recv() => m,
                    _ = tokio::time::sleep_until(at) => {
                        self.commit();
                        continue;
                    }
                },
                None => rx.recv().await,
            };
            let Some(msg) = msg else { break };
            if matches!(msg, ActorMsg::Shutdown) {
                if let Some(s) = self.session.take() {
                    s.handle.kill();
                }
                self.commit();
                break;
            }
            let streaming = self.handle(msg);
            if self.gone {
                break;
            }
            if streaming {
                if self.publish_at.is_none() {
                    self.publish_at =
                        Some(Instant::now() + self.inner.backends.timing.publish_interval);
                }
            } else {
                self.commit();
            }
        }
    }

    fn id(&self) -> &str {
        &self.d.thread.id
    }

    // ------------------------------------------------------------------------------------
    // persistence + publishing

    fn commit(&mut self) {
        self.publish_at = None;
        if !self.dirty.any() {
            return;
        }
        let dirty = std::mem::take(&mut self.dirty);
        if let Err(e) = self.persist(&dirty) {
            tracing::error!(thread = %self.id(), "persist failed: {e:#}");
            self.inner
                .error(format!("Could not save this thread: {e:#}"));
        }
        self.publish();
    }

    fn persist(&self, dirty: &Dirty) -> Result<()> {
        let mut store = self.inner.store.lock();
        let tx = store.tx()?;
        if dirty.thread {
            tx.upsert_thread(&self.d.thread)?;
        }
        if dirty.binding
            && let Some(b) = &self.binding
        {
            tx.upsert_binding(b)?;
        }
        for r in self.d.runs.iter().filter(|r| dirty.runs.contains(&r.id)) {
            tx.upsert_run(r)?;
        }
        for p in self.d.plans.iter().filter(|p| dirty.plans.contains(&p.id)) {
            tx.upsert_plan(p)?;
        }
        for i in self.d.items.iter().filter(|i| dirty.items.contains(&i.id)) {
            tx.upsert_item(i)?;
        }
        for q in self
            .d
            .requests
            .iter()
            .filter(|q| dirty.requests.contains(&q.id))
        {
            tx.upsert_request(q)?;
        }
        for c in self
            .d
            .checkpoints
            .iter()
            .filter(|c| dirty.checkpoints.contains(&c.id))
        {
            let cwd = self
                .checkpoint_cwds
                .get(&c.id)
                .cloned()
                .or_else(|| self.cwd().map(|p| p.to_string_lossy().into_owned()))
                .unwrap_or_default();
            tx.upsert_checkpoint(c, &cwd)?;
        }
        tx.commit()
    }

    fn publish(&mut self) {
        self.d.revision = self.inner.next_revision();
        let shell = derive_shell(&self.d);
        let shell_changed = shell != self.last_shell;
        let id = self.id().to_string();
        self.inner.state.write().threads.insert(
            id.clone(),
            Published {
                detail: Arc::new(self.d.clone()),
                shell: shell.clone(),
                binding: self.binding.clone(),
            },
        );
        self.last_shell = shell;
        self.inner.emit(Update::Thread(id));
        if shell_changed {
            self.inner.emit(Update::Snapshot);
        }
    }

    // ------------------------------------------------------------------------------------
    // state helpers

    fn touch(&mut self) {
        self.d.thread.updated_at = now();
        self.dirty.thread = true;
    }

    fn cwd(&self) -> Option<PathBuf> {
        if let Some(w) = &self.d.thread.worktree_path {
            return Some(PathBuf::from(w));
        }
        self.inner
            .project(&self.d.thread.project_id)
            .map(|p| PathBuf::from(p.workspace_root))
    }

    fn run(&self, id: &str) -> Option<&Run> {
        self.d.runs.iter().find(|r| r.id == id)
    }

    fn with_run(&mut self, id: &str, f: impl FnOnce(&mut Run)) -> bool {
        match self.d.runs.iter_mut().find(|r| r.id == id) {
            Some(r) => {
                f(r);
                self.dirty.runs.insert(id.to_string());
                true
            }
            None => false,
        }
    }

    fn active_run_id(&self) -> Option<String> {
        active_run(&self.d.runs).map(|r| r.id.clone())
    }

    fn next_ordinal(&self) -> u32 {
        self.d
            .items
            .iter()
            .map(|i| i.ordinal)
            .max()
            .map_or(1, |m| m + 1)
    }

    fn item_idx(&self, id: &str) -> Option<usize> {
        self.d.items.iter().position(|i| i.id == id)
    }

    fn put_item(&mut self, item: TurnItem) {
        self.dirty.items.insert(item.id.clone());
        match self.item_idx(&item.id) {
            Some(i) => {
                let reorder = self.d.items[i].ordinal != item.ordinal;
                self.d.items[i] = item;
                if reorder {
                    self.d.items.sort_by_key(|i| i.ordinal);
                }
            }
            None => {
                let reorder = self
                    .d
                    .items
                    .last()
                    .is_some_and(|l| l.ordinal > item.ordinal);
                self.d.items.push(item);
                if reorder {
                    self.d.items.sort_by_key(|i| i.ordinal);
                }
            }
        }
    }

    fn with_item(&mut self, id: &str, f: impl FnOnce(&mut TurnItem)) -> bool {
        match self.d.items.iter_mut().find(|i| i.id == id) {
            Some(i) => {
                f(i);
                i.updated_at = now();
                self.dirty.items.insert(id.to_string());
                true
            }
            None => false,
        }
    }

    fn add_item(
        &mut self,
        run_id: Option<&str>,
        status: ItemStatus,
        title: Option<&str>,
        body: TurnItemBody,
    ) -> String {
        let t = now();
        let id = new_id();
        let item = TurnItem {
            id: id.clone(),
            thread_id: self.id().to_string(),
            run_id: run_id.map(str::to_string),
            native_item_id: None,
            parent_item_id: None,
            ordinal: self.next_ordinal(),
            status,
            title: title.map(str::to_string),
            started_at: Some(t),
            completed_at: (!status.is_open()).then_some(t),
            updated_at: t,
            body,
        };
        self.put_item(item);
        id
    }

    fn user_message_of(&self, run_id: &str) -> Option<&TurnItem> {
        let run = self.run(run_id)?;
        self.d.items.iter().find(|i| {
            matches!(&i.body, TurnItemBody::UserMessage { message_id, .. } if *message_id == run.user_message_id)
        })
    }

    fn with_request(&mut self, id: &str, f: impl FnOnce(&mut RuntimeRequest)) -> bool {
        match self.d.requests.iter_mut().find(|r| r.id == id) {
            Some(r) => {
                f(r);
                self.dirty.requests.insert(id.to_string());
                true
            }
            None => false,
        }
    }

    fn put_checkpoint(&mut self, cp: Checkpoint, cwd: String) {
        self.d
            .checkpoints
            .retain(|c| c.id != cp.id && c.ordinal != cp.ordinal);
        self.checkpoint_cwds.insert(cp.id.clone(), cwd);
        self.dirty.checkpoints.insert(cp.id.clone());
        self.d.checkpoints.push(cp);
        self.d.checkpoints.sort_by_key(|c| c.ordinal);
    }

    fn set_binding_status(&mut self, status: ProviderThreadStatus) {
        if let Some(b) = &mut self.binding {
            b.status = status;
            b.updated_at = now();
            self.dirty.binding = true;
        }
    }

    fn set_session_status(&mut self, status: SessionStatus) {
        if let Some(s) = &mut self.d.session {
            s.status = status;
            s.updated_at = now();
            self.dirty.view = true;
        }
    }

    // ------------------------------------------------------------------------------------
    // dispatch

    /// Returns true when the change is streaming-only and can be coalesced.
    fn handle(&mut self, msg: ActorMsg) -> bool {
        match msg {
            ActorMsg::Command(c) => self.command(c),
            ActorMsg::Launched { workspace, message } => self.launched(workspace, message),
            ActorMsg::Provider { session_id, event } => {
                let ours = self.session.as_ref().is_some_and(|s| s.id == session_id)
                    || self.opening_session.as_deref() == Some(session_id.as_str());
                if ours {
                    return self.provider_event(event);
                }
            }
            ActorMsg::SessionClosed { session_id } => self.session_closed(&session_id),
            ActorMsg::TurnStarted(o) => self.turn_started(*o),
            ActorMsg::WorkspaceReady { run_id, result } => self.workspace_ready(run_id, result),
            ActorMsg::CheckpointDone {
                run_id,
                checkpoint,
                cwd,
            } => self.checkpoint_done(run_id, checkpoint, cwd),
            ActorMsg::TitleDone { result, regenerate } => self.title_done(result, regenerate),
            ActorMsg::InterruptTimeout { run_id } => self.interrupt_timeout(&run_id),
            ActorMsg::IdleCheck { generation } => {
                if generation == self.idle_gen && self.active_run_id().is_none() {
                    self.release_session();
                }
            }
            ActorMsg::DeliveryFailed => self.delivery_failed(),
            ActorMsg::RollbackDone {
                checkpoint_id,
                result,
            } => self.rollback_done(&checkpoint_id, result),
            ActorMsg::ResumeSession { run_id } => self.resume_session(&run_id),
            ActorMsg::CaptureMissing { run_id } => {
                let settled = self.run(&run_id).is_some_and(|r| {
                    r.status.is_terminal()
                        && r.status != RunStatus::Failed
                        && r.status != RunStatus::RolledBack
                        && r.checkpoint_id.is_none()
                });
                if settled && self.capturing.is_none() {
                    self.start_capture(&run_id);
                }
            }
            ActorMsg::Shutdown => {}
        }
        false
    }

    fn command(&mut self, c: Command) {
        match c {
            Command::SendMessage {
                message,
                model_selection,
                mode,
                ..
            } => self.send_message(message, model_selection, mode),
            Command::Interrupt { run_id, .. } => self.interrupt(&run_id),
            Command::ForceStop { .. } => self.force_stop(),
            Command::RespondToRequest {
                request_id,
                decision,
                feedback,
                answers,
                ..
            } => self.respond(&request_id, decision, feedback, answers),
            Command::DismissUserInput { request_id, .. } => self.dismiss(&request_id),
            Command::RollbackToCheckpoint {
                checkpoint_id,
                restore_files,
                ..
            } => self.rollback(&checkpoint_id, restore_files),
            Command::SetModelSelection {
                model_selection, ..
            } => self.set_model_selection(model_selection),
            Command::SetRuntimeMode { runtime_mode, .. } => {
                if self.d.thread.runtime_mode != runtime_mode {
                    self.d.thread.runtime_mode = runtime_mode;
                    self.touch();
                    if self
                        .session
                        .as_ref()
                        .is_some_and(|s| !s.capabilities.supports_runtime_mode_switch_in_session)
                    {
                        self.restart_session_when_idle();
                    }
                }
            }
            Command::SetInteractionMode {
                interaction_mode, ..
            } => {
                if self.d.thread.interaction_mode != interaction_mode {
                    self.d.thread.interaction_mode = interaction_mode;
                    self.touch();
                }
            }
            Command::RenameThread { title, .. } => {
                let title = title.trim();
                if title.is_empty() {
                    self.inner.error("Thread title cannot be empty");
                    return;
                }
                self.d.thread.title = title.to_string();
                self.d.thread.title_renamed = true;
                self.d.thread.title_generation_started_at = None;
                self.touch();
            }
            Command::RegenerateTitle { .. } => {
                self.d.thread.title_renamed = false;
                self.generate_title(true);
            }
            Command::SetPinned { pinned, .. } => {
                self.d.thread.pinned_at = pinned.then(now);
                self.dirty.thread = true;
            }
            Command::SetSettled { settled, .. } => {
                self.d.thread.settled_at = settled.then(now);
                self.dirty.thread = true;
            }
            Command::SetArchived { archived, .. } => {
                self.d.thread.archived_at = archived.then(now);
                self.dirty.thread = true;
                if archived && self.active_run_id().is_none() {
                    self.release_session();
                }
            }
            Command::DeleteThread { .. } => self.delete(),
            Command::MarkVisited { .. } => {
                self.d.thread.last_visited_at = Some(now());
                self.dirty.thread = true;
            }
            Command::MarkUnread { .. } => {
                let done = self.last_shell.latest_run_completed_at;
                self.d.thread.last_visited_at = done.map(|t| t - chrono::Duration::milliseconds(1));
                self.dirty.thread = true;
            }
            Command::CancelQueuedRun { run_id, .. } => self.cancel_queued(&run_id),
            Command::AddProject { .. }
            | Command::RenameProject { .. }
            | Command::RemoveProject { .. }
            | Command::UpdateProjectScripts { .. }
            | Command::LaunchThread { .. }
            | Command::RefreshProviders => {}
        }
    }

    // ------------------------------------------------------------------------------------
    // sending

    fn launched(&mut self, workspace: WorkspaceStrategy, message: Option<OutgoingMessage>) {
        match workspace {
            WorkspaceStrategy::Worktree {
                base_ref,
                branch,
                start_from_origin,
            } => {
                let run_id = match message {
                    Some(m) => {
                        if !self.validate(&m) {
                            return;
                        }
                        let id =
                            self.create_run(RunStatus::Preparing, &m, InputIntent::TurnStart, None);
                        self.seed_title(&m);
                        Some(id)
                    }
                    None => None,
                };
                self.prepare_worktree(run_id, base_ref, branch, start_from_origin);
            }
            _ => {
                if let Some(m) = message {
                    self.send_message(m, None, DispatchMode::StartImmediately);
                }
            }
        }
    }

    fn validate(&self, m: &OutgoingMessage) -> bool {
        let err = if m.text.trim().is_empty() && m.attachments.is_empty() {
            Some("Write a message first.".to_string())
        } else if m.text.chars().count() > MAX_INPUT_CHARS {
            Some(format!(
                "Messages can be at most {MAX_INPUT_CHARS} characters."
            ))
        } else if m.attachments.len() > MAX_ATTACHMENTS {
            Some(format!("Attach at most {MAX_ATTACHMENTS} files."))
        } else {
            None
        };
        match err {
            Some(e) => {
                self.inner.error(e);
                false
            }
            None => true,
        }
    }

    fn send_message(
        &mut self,
        message: OutgoingMessage,
        model_selection: Option<ModelSelection>,
        mode: DispatchMode,
    ) {
        if !self.validate(&message) {
            return;
        }
        if self.d.thread.archived_at.is_some() {
            self.d.thread.archived_at = None;
        }
        let mut model_switch = None;
        if let Some(ms) = model_selection
            && ms != self.d.thread.model_selection
        {
            if !self.d.runs.is_empty() {
                model_switch = Some(ModelSwitch {
                    from: self.d.thread.model_selection.clone(),
                    to: ms.clone(),
                });
            }
            self.set_model_selection(ms);
        }

        let active = self.active_run_id();
        if let DispatchMode::SteerActive { target_run_id } = &mode
            && active.as_deref() == Some(target_run_id.as_str())
            && self
                .run(target_run_id)
                .is_some_and(|r| r.status == RunStatus::Running)
            && self
                .session
                .as_ref()
                .is_some_and(|s| s.capabilities.supports_active_steering)
        {
            self.steer(target_run_id.clone(), message, model_switch);
            self.touch();
            return;
        }

        let first = !self
            .d
            .items
            .iter()
            .any(|i| matches!(i.body, TurnItemBody::UserMessage { .. }));
        let has_queue = self.d.runs.iter().any(|r| r.status == RunStatus::Queued);
        let start_now =
            active.is_none() && !has_queue && self.capturing.is_none() && !self.rolling_back;
        let (status, intent) = if start_now {
            (RunStatus::Starting, InputIntent::TurnStart)
        } else {
            (RunStatus::Queued, InputIntent::QueuedTurn)
        };
        let run_id = self.create_run(status, &message, intent, model_switch);
        if first {
            self.seed_title(&message);
        }
        self.touch();
        if start_now {
            self.begin_turn(&run_id, false);
        }
    }

    fn steer(
        &mut self,
        run_id: String,
        message: OutgoingMessage,
        model_switch: Option<ModelSwitch>,
    ) {
        self.add_item(
            Some(&run_id),
            ItemStatus::Completed,
            None,
            TurnItemBody::UserMessage {
                message_id: new_id(),
                text: message.text.clone(),
                attachments: message.attachments.clone(),
                input_intent: InputIntent::Steer,
                model_switch,
            },
        );
        if let Some(s) = &self.session {
            let handle = s.handle.clone();
            let tx = self.tx.clone();
            self.inner.handle.spawn(async move {
                if let Err(e) = handle.steer(&run_id, message.text).await {
                    tracing::warn!("steer failed: {e:#}");
                    let _ = tx.send(ActorMsg::DeliveryFailed);
                }
            });
        }
    }

    fn create_run(
        &mut self,
        status: RunStatus,
        message: &OutgoingMessage,
        intent: InputIntent,
        model_switch: Option<ModelSwitch>,
    ) -> String {
        let t = now();
        let ordinal = self
            .d
            .runs
            .iter()
            .filter(|r| r.status != RunStatus::RolledBack)
            .map(|r| r.ordinal)
            .max()
            .unwrap_or(0)
            + 1;
        let queue_position = (status == RunStatus::Queued).then(|| {
            self.d
                .runs
                .iter()
                .filter_map(|r| r.queue_position)
                .max()
                .unwrap_or(0)
                + 1
        });
        let run_id = new_id();
        let message_id = new_id();
        self.d.runs.push(Run {
            id: run_id.clone(),
            thread_id: self.id().to_string(),
            ordinal,
            model_selection: self.d.thread.model_selection.clone(),
            runtime_mode: self.d.thread.runtime_mode,
            interaction_mode: self.d.thread.interaction_mode,
            user_message_id: message_id.clone(),
            status,
            queue_position,
            requested_at: t,
            started_at: None,
            completed_at: None,
            checkpoint_id: None,
            source_plan_id: message.source_plan_id.clone(),
            native_turn_id: None,
            model_reported: None,
            token_usage: None,
            failure: None,
            interrupt_requested_at: None,
            interrupt_unresponsive_at: None,
        });
        self.dirty.runs.insert(run_id.clone());
        self.add_item(
            Some(&run_id),
            ItemStatus::Completed,
            None,
            TurnItemBody::UserMessage {
                message_id,
                text: message.text.clone(),
                attachments: message.attachments.clone(),
                input_intent: intent,
                model_switch,
            },
        );
        if let Some(plan_id) = &message.source_plan_id
            && let Some(p) = self.d.plans.iter_mut().find(|p| p.id == *plan_id)
        {
            p.status = PlanStatus::Completed;
            p.updated_at = t;
            self.dirty.plans.insert(plan_id.clone());
        }
        run_id
    }

    fn seed_title(&mut self, message: &OutgoingMessage) {
        let t = &self.d.thread;
        if t.title_renamed || !(t.title.trim().is_empty() || t.title == NEW_THREAD_TITLE) {
            return;
        }
        self.d.thread.title = seed_title(&message.text, &message.attachments);
        self.dirty.thread = true;
        self.generate_title(false);
    }

    fn session_config(
        &self,
        adapter: &dyn ProviderAdapter,
        cwd: &Path,
        run: &Run,
    ) -> SessionConfig {
        let settings = self.inner.backends.settings.get();
        let cfg = settings.provider_instance(adapter.instance_id());
        let env = cfg
            .environment
            .iter()
            .filter(|v| !v.sensitive && !v.name.is_empty())
            .map(|v| (v.name.clone(), v.value.clone()))
            .collect();
        let resume = self
            .binding
            .as_ref()
            .filter(|b| b.driver == adapter.driver())
            .and_then(|b| b.native_thread_id.clone());
        SessionConfig {
            thread_id: self.id().to_string(),
            cwd: cwd.to_path_buf(),
            resume_native_thread_id: resume,
            model_selection: run.model_selection.clone(),
            runtime_mode: run.runtime_mode,
            binary_path: cfg.binary_path().map(PathBuf::from),
            env,
        }
    }

    /// Start the provider turn for a run in `starting` (spec/07 §2.2 step 3).
    fn begin_turn(&mut self, run_id: &str, resume_only: bool) {
        self.idle_gen += 1;
        let Some(run) = self.run(run_id).cloned() else {
            return;
        };
        let instance = run.model_selection.instance_id.clone();
        let Some(adapter) = self.inner.backends.adapter(&instance) else {
            self.finish_run(
                run_id,
                TerminalStatus::Failed,
                Some(failure(
                    FailureClass::ValidationError,
                    format!("No provider is set up for \"{instance}\"."),
                )),
                None,
                false,
            );
            return;
        };
        let Some(cwd) = self.cwd() else {
            self.finish_run(
                run_id,
                TerminalStatus::Failed,
                Some(failure(
                    FailureClass::ValidationError,
                    "This thread's project no longer exists.",
                )),
                None,
                false,
            );
            return;
        };

        let reuse = self
            .session
            .as_ref()
            .filter(|s| s.instance_id == instance && !self.restart_session)
            .map(|s| s.handle.clone());
        if reuse.is_none() {
            self.release_session();
        }
        self.restart_session = false;
        let open = if reuse.is_none() {
            if self.opening_session.is_some() && resume_only {
                return;
            }
            let sid = new_id();
            self.opening_session = Some(sid.clone());
            Some((
                sid,
                adapter.clone(),
                self.session_config(adapter.as_ref(), &cwd, &run),
            ))
        } else {
            None
        };
        if resume_only && open.is_none() {
            return;
        }

        let user = self.user_message_of(run_id);
        let (text, attachments) = match user.map(|i| &i.body) {
            Some(TurnItemBody::UserMessage {
                text, attachments, ..
            }) => (text.clone(), attachments.clone()),
            _ => (String::new(), vec![]),
        };
        let attachments_dir = self.inner.paths.attachments();
        let turn = TurnRequest {
            run_id: run_id.to_string(),
            text,
            attachments: attachments
                .into_iter()
                .map(|a| {
                    let path = attachments_dir.join(a.id());
                    (a, path)
                })
                .collect(),
            model_selection: run.model_selection.clone(),
            runtime_mode: run.runtime_mode,
            interaction_mode: run.interaction_mode,
            cwd: cwd.clone(),
        };

        let baseline_ordinal = run.ordinal.saturating_sub(1);
        let need_baseline = !resume_only
            && !self
                .d
                .checkpoints
                .iter()
                .any(|c| c.ordinal == baseline_ordinal && c.status != CheckpointStatus::Stale);
        let git = self.inner.backends.git.clone();
        let budget = self.inner.backends.timing.baseline_budget;
        let thread_id = self.id().to_string();
        let tx = self.tx.clone();
        let run_id = run_id.to_string();
        self.inner.handle.spawn(async move {
            let baseline = if need_baseline {
                capture_baseline(git, cwd.clone(), thread_id, baseline_ordinal, budget).await
            } else {
                None
            };
            let (handle, session) = match (reuse, open) {
                (Some(h), _) => (h, None),
                (None, Some((sid, adapter, config))) => {
                    match open_session(adapter, config, sid, cwd, tx.clone()).await {
                        Ok(live) => (live.handle.clone(), Some(live)),
                        Err(e) => {
                            let _ = tx.send(ActorMsg::TurnStarted(Box::new(StartOutcome {
                                run_id,
                                session: None,
                                baseline,
                                result: Err(e),
                                resume_only,
                            })));
                            return;
                        }
                    }
                }
                (None, None) => return,
            };
            let result = if resume_only {
                Ok(())
            } else {
                handle.start_turn(turn).await.map_err(|e| format!("{e:#}"))
            };
            let _ = tx.send(ActorMsg::TurnStarted(Box::new(StartOutcome {
                run_id,
                session,
                baseline,
                result,
                resume_only,
            })));
        });
    }

    fn resume_session(&mut self, run_id: &str) {
        if self.session.is_some() || self.run(run_id).is_none_or(|r| !r.status.is_live()) {
            return;
        }
        self.begin_turn(run_id, true);
    }

    fn attach_session(&mut self, live: LiveSession) {
        let t = now();
        self.d.session = Some(ProviderSession {
            id: live.id.clone(),
            driver: live.driver,
            status: SessionStatus::Ready,
            cwd: live.cwd.clone(),
            model: None,
            capabilities: live.capabilities.clone(),
            startup_phase: None,
            last_error: None,
            created_at: live.created_at,
            updated_at: t,
        });
        self.dirty.view = true;
        match &mut self.binding {
            Some(b) if b.driver == live.driver => {}
            _ => {
                self.binding = Some(ProviderBinding {
                    thread_id: self.d.thread.id.clone(),
                    driver: live.driver,
                    native_thread_id: None,
                    status: ProviderThreadStatus::Idle,
                    context_usage: None,
                    updated_at: t,
                });
                self.dirty.binding = true;
            }
        }
        self.session = Some(live);
    }

    fn turn_started(&mut self, o: StartOutcome) {
        if let Some((cp, cwd)) = o.baseline {
            self.put_checkpoint(cp, cwd);
        }
        match o.session {
            Some(live) if self.opening_session.as_deref() == Some(live.id.as_str()) => {
                self.opening_session = None;
                self.attach_session(live);
            }
            Some(stale) => {
                let h = stale.handle.clone();
                self.inner.handle.spawn(async move { h.shutdown().await });
            }
            None if o.result.is_err() => self.opening_session = None,
            None => {}
        }
        let Some(run) = self.run(&o.run_id).cloned() else {
            return;
        };
        if o.resume_only {
            if let Err(e) = o.result {
                tracing::warn!("resume failed: {e}");
            }
            return;
        }
        match o.result {
            Ok(()) => {
                if run.status.is_terminal() {
                    // stopped while the turn was starting
                    self.send_interrupt(&run.id);
                    return;
                }
                self.with_run(&run.id, |r| {
                    if r.status == RunStatus::Starting {
                        r.status = RunStatus::Running;
                    }
                    r.started_at.get_or_insert_with(now);
                });
                self.set_binding_status(ProviderThreadStatus::Active);
                self.set_session_status(SessionStatus::Running);
                if self.pending_interrupt.remove(&run.id) {
                    self.send_interrupt(&run.id);
                }
            }
            Err(e) => {
                if !run.status.is_terminal() {
                    self.finish_run(
                        &run.id,
                        TerminalStatus::Failed,
                        Some(failure(FailureClass::ProviderError, e)),
                        None,
                        false,
                    );
                }
            }
        }
    }

    fn prepare_worktree(
        &mut self,
        run_id: Option<String>,
        base_ref: String,
        branch: Option<String>,
        start_from_origin: Option<bool>,
    ) {
        let Some(project) = self.inner.project(&self.d.thread.project_id) else {
            return;
        };
        let settings = self.inner.backends.settings.get();
        let git = self.inner.backends.git.clone();
        let root =
            settings.worktrees_root(&self.inner.paths.worktrees(), dirs::home_dir().as_deref());
        let start_from_origin =
            start_from_origin.unwrap_or(settings.new_worktrees_start_from_origin);
        let prefix = settings.branch_name_prefix.clone();
        let tx = self.tx.clone();
        self.inner.handle.spawn_blocking(move || {
            let result = (|| -> Result<(String, String)> {
                let repo = git
                    .repo_root(Path::new(&project.workspace_root))
                    .ok_or_else(|| {
                        anyhow::anyhow!("{} is not a git repository.", project.workspace_root)
                    })?;
                let branch = match branch {
                    Some(b) => b,
                    None => {
                        let id = new_id().replace('-', "");
                        let slug = &id[id.len() - 8..];
                        git.unique_branch_name(&repo, &prefix, slug)?
                    }
                };
                let path =
                    git.create_worktree(&repo, &root, &base_ref, &branch, start_from_origin)?;
                Ok((path.to_string_lossy().into_owned(), branch))
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(ActorMsg::WorkspaceReady { run_id, result });
        });
    }

    fn workspace_ready(
        &mut self,
        run_id: Option<String>,
        result: Result<(String, String), String>,
    ) {
        match result {
            Ok((path, branch)) => {
                self.d.thread.worktree_path = Some(path);
                self.d.thread.branch = Some(branch);
                self.touch();
                if let Some(rid) = run_id
                    && self
                        .run(&rid)
                        .is_some_and(|r| r.status == RunStatus::Preparing)
                {
                    self.with_run(&rid, |r| r.status = RunStatus::Starting);
                    self.begin_turn(&rid, false);
                }
            }
            Err(e) => match run_id {
                Some(rid)
                    if self
                        .run(&rid)
                        .is_some_and(|r| r.status == RunStatus::Preparing) =>
                {
                    let mut f = failure(FailureClass::Unknown, e);
                    f.code = Some("workspace_preparation_failed".into());
                    self.finish_run(&rid, TerminalStatus::Failed, Some(f), None, false);
                }
                Some(_) => {}
                None => self.inner.error(e),
            },
        }
    }

    fn advance_queue(&mut self) {
        if self.active_run_id().is_some() || self.capturing.is_some() || self.rolling_back {
            return;
        }
        let next = self
            .d
            .runs
            .iter()
            .filter(|r| r.status == RunStatus::Queued)
            .min_by_key(|r| (r.queue_position.unwrap_or(u32::MAX), r.ordinal))
            .map(|r| r.id.clone());
        let Some(run_id) = next else {
            return;
        };
        self.with_run(&run_id, |r| {
            r.status = RunStatus::Starting;
            r.queue_position = None;
        });
        // the queued message moves below everything the previous run wrote
        if let Some(item_id) = self.user_message_of(&run_id).map(|i| i.id.clone()) {
            let ordinal = self.next_ordinal();
            if let Some(mut item) = self.d.items.iter().find(|i| i.id == item_id).cloned() {
                item.ordinal = ordinal;
                item.updated_at = now();
                self.put_item(item);
            }
        }
        self.begin_turn(&run_id, false);
    }

    fn cancel_queued(&mut self, run_id: &str) {
        if self
            .run(run_id)
            .is_none_or(|r| r.status != RunStatus::Queued)
        {
            return;
        }
        let t = now();
        self.with_run(run_id, |r| {
            r.status = RunStatus::Cancelled;
            r.queue_position = None;
            r.completed_at = Some(t);
        });
        if let Some(id) = self.user_message_of(run_id).map(|i| i.id.clone()) {
            self.with_item(&id, |i| i.status = ItemStatus::Cancelled);
        }
    }

    // ------------------------------------------------------------------------------------
    // provider events (spec/07 §2.4)

    fn provider_event(&mut self, ev: ProviderEvent) -> bool {
        match ev {
            ProviderEvent::SessionUpdated {
                status,
                model,
                startup_phase,
                last_error,
            } => {
                if let Some(s) = &mut self.d.session {
                    s.status = status;
                    if model.is_some() {
                        s.model = model;
                    }
                    s.startup_phase = startup_phase;
                    s.last_error = last_error;
                    s.updated_at = now();
                    self.dirty.view = true;
                }
            }
            ProviderEvent::BindingUpdated {
                native_thread_id,
                status,
                context_usage,
            } => {
                if let Some(b) = &mut self.binding {
                    if native_thread_id.is_some() {
                        b.native_thread_id = native_thread_id.clone();
                    }
                    b.status = status;
                    if context_usage.is_some() {
                        b.context_usage = context_usage.clone();
                    }
                    b.updated_at = now();
                    self.dirty.binding = true;
                }
                if context_usage.is_some() {
                    self.d.context_usage = context_usage;
                    self.dirty.view = true;
                }
                if native_thread_id.is_some()
                    && self.d.thread.active_provider_thread_id != native_thread_id
                {
                    self.d.thread.active_provider_thread_id = native_thread_id;
                    self.dirty.thread = true;
                }
            }
            ProviderEvent::TurnStarted {
                run_id,
                native_turn_id,
            } => {
                self.with_run(&run_id, |r| {
                    if native_turn_id.is_some() {
                        r.native_turn_id = native_turn_id;
                    }
                    if r.status == RunStatus::Starting {
                        r.status = RunStatus::Running;
                    }
                    r.started_at.get_or_insert_with(now);
                });
            }
            ProviderEvent::TurnUsage {
                run_id,
                usage,
                model,
            } => {
                self.with_run(&run_id, |r| {
                    r.token_usage = Some(usage);
                    if model.is_some() {
                        r.model_reported = model;
                    }
                });
            }
            ProviderEvent::ItemUpserted(item) => return self.upsert_provider_item(item),
            ProviderEvent::CommandOutputDelta { item_id, chunk } => {
                let out = self.d.live_output.entry(item_id).or_default();
                out.push_str(&chunk);
                if out.len() > LIVE_OUTPUT_CAP {
                    let mut cut = out.len() - LIVE_OUTPUT_CAP;
                    while !out.is_char_boundary(cut) {
                        cut += 1;
                    }
                    out.drain(..cut);
                }
                self.dirty.view = true;
                return true;
            }
            ProviderEvent::RequestOpened(mut req) => {
                req.thread_id = self.id().to_string();
                if req.run_id.as_ref().is_none_or(|r| self.run(r).is_none()) {
                    req.run_id = self.active_run_id();
                }
                if req.live_session_id.is_none() {
                    req.live_session_id = self.session.as_ref().map(|s| s.id.clone());
                }
                req.status = RequestStatus::Pending;
                let item_id = req.turn_item_id.clone();
                self.dirty.requests.insert(req.id.clone());
                match self.d.requests.iter_mut().find(|r| r.id == req.id) {
                    Some(r) => *r = req,
                    None => self.d.requests.push(req),
                }
                self.with_item(&item_id, |i| {
                    if i.status.is_open() || i.status == ItemStatus::Completed {
                        i.status = ItemStatus::Waiting;
                    }
                });
                self.set_session_status(SessionStatus::Waiting);
            }
            ProviderEvent::RequestClosed { request_id, status } => {
                let mut item_id = None;
                self.with_request(&request_id, |r| {
                    if r.status == RequestStatus::Pending {
                        r.status = status;
                        r.resolved_at = Some(now());
                        item_id = Some(r.turn_item_id.clone());
                    }
                });
                if let Some(id) = item_id {
                    self.with_item(&id, |i| {
                        if i.status.is_open() {
                            i.status = ItemStatus::Cancelled;
                        }
                    });
                }
            }
            ProviderEvent::PlanUpdated {
                plan_id,
                markdown,
                streaming,
            } => {
                self.plan_updated(&plan_id, markdown, streaming);
                return streaming;
            }
            ProviderEvent::TodoUpdated {
                plan_id,
                steps,
                explanation,
            } => self.todo_updated(&plan_id, steps, explanation),
            ProviderEvent::ModelSelectionChanged { changes } => {
                for (option, _from, to) in changes {
                    if option == "interactionMode" {
                        self.d.thread.interaction_mode = if to == "plan" {
                            InteractionMode::Plan
                        } else {
                            InteractionMode::Default
                        };
                    } else {
                        self.d
                            .thread
                            .model_selection
                            .set_option(&option, OptionValue::Str(to));
                    }
                }
                self.dirty.thread = true;
            }
            ProviderEvent::TurnTerminal {
                run_id,
                status,
                failure,
                reason,
                broken,
            } => self.finish_run(&run_id, status, failure, reason, broken),
        }
        false
    }

    fn upsert_provider_item(&mut self, mut item: TurnItem) -> bool {
        item.thread_id = self.id().to_string();
        if item.run_id.as_ref().is_none_or(|r| self.run(r).is_none()) {
            item.run_id = self.active_run_id();
        }
        // plans and todo lists are keyed by plan id, whatever item id the adapter used
        let plan_key = match &item.body {
            TurnItemBody::ProposedPlan { plan_id, .. } => Some(("p", plan_id.clone())),
            TurnItemBody::TodoList { plan_id, .. } => Some(("t", plan_id.clone())),
            _ => None,
        };
        if let Some((kind, plan_id)) = &plan_key
            && let Some(existing) = self.find_plan_item(kind, plan_id)
        {
            item.id = existing;
        }
        item.ordinal = match self.item_idx(&item.id) {
            Some(i) => self.d.items[i].ordinal,
            None => self.next_ordinal(),
        };
        if !item.status.is_open() && self.d.live_output.remove(&item.id).is_some() {
            self.dirty.view = true;
        }
        if let TurnItemBody::ProposedPlan {
            plan_id,
            markdown,
            streaming,
        } = &item.body
        {
            self.upsert_plan(plan_id, markdown.clone(), *streaming, item.run_id.clone());
        }
        let streaming = is_streaming(&item.body) && item.status.is_open();
        self.put_item(item);
        streaming
    }

    fn find_plan_item(&self, kind: &str, plan_id: &str) -> Option<String> {
        self.d.items.iter().find_map(|i| match (&i.body, kind) {
            (TurnItemBody::ProposedPlan { plan_id: p, .. }, "p") if p == plan_id => {
                Some(i.id.clone())
            }
            (TurnItemBody::TodoList { plan_id: p, .. }, "t") if p == plan_id => Some(i.id.clone()),
            _ => None,
        })
    }

    fn upsert_plan(
        &mut self,
        plan_id: &str,
        markdown: String,
        streaming: bool,
        run_id: Option<String>,
    ) {
        let t = now();
        let status = if streaming {
            PlanStatus::Draft
        } else {
            PlanStatus::Active
        };
        if status == PlanStatus::Active {
            for p in self
                .d
                .plans
                .iter_mut()
                .filter(|p| p.id != plan_id && p.status == PlanStatus::Active)
            {
                p.status = PlanStatus::Superseded;
                p.updated_at = t;
                self.dirty.plans.insert(p.id.clone());
            }
        }
        match self.d.plans.iter_mut().find(|p| p.id == plan_id) {
            Some(p) => {
                p.markdown = markdown;
                if p.status == PlanStatus::Draft || p.status == PlanStatus::Active {
                    p.status = status;
                }
                p.updated_at = t;
            }
            None => self.d.plans.push(ProposedPlan {
                id: plan_id.to_string(),
                thread_id: self.d.thread.id.clone(),
                run_id,
                markdown,
                status,
                created_at: t,
                updated_at: t,
            }),
        }
        self.dirty.plans.insert(plan_id.to_string());
    }

    fn plan_updated(&mut self, plan_id: &str, markdown: String, streaming: bool) {
        let run_id = self.active_run_id();
        self.upsert_plan(plan_id, markdown.clone(), streaming, run_id.clone());
        let body = TurnItemBody::ProposedPlan {
            plan_id: plan_id.to_string(),
            markdown,
            streaming,
        };
        let status = if streaming {
            ItemStatus::Running
        } else {
            ItemStatus::Completed
        };
        match self.find_plan_item("p", plan_id) {
            Some(id) => {
                self.with_item(&id, |i| {
                    i.body = body;
                    i.status = status;
                    if !streaming {
                        i.completed_at = Some(now());
                    }
                });
            }
            None => {
                self.add_item(run_id.as_deref(), status, None, body);
            }
        }
    }

    /// Step durations are stamped on ingest (`ProviderEventIngestor.ts:629-654`).
    fn todo_updated(
        &mut self,
        plan_id: &str,
        mut steps: Vec<PlanStep>,
        explanation: Option<String>,
    ) {
        let t = now();
        let existing = self.find_plan_item("t", plan_id);
        let old: Vec<PlanStep> = existing
            .as_ref()
            .and_then(|id| self.d.items.iter().find(|i| i.id == *id))
            .and_then(|i| match &i.body {
                TurnItemBody::TodoList { steps, .. } => Some(steps.clone()),
                _ => None,
            })
            .unwrap_or_default();
        for s in &mut steps {
            let prev = old.iter().find(|o| o.id == s.id);
            if s.duration_anchor_at.is_none() {
                s.duration_anchor_at = prev.and_then(|p| p.duration_anchor_at.clone());
            }
            if s.duration_ms.is_none() {
                s.duration_ms = prev.and_then(|p| p.duration_ms);
            }
            match s.status {
                PlanStepStatus::Running if s.duration_anchor_at.is_none() => {
                    s.duration_anchor_at = Some(t.to_rfc3339());
                }
                PlanStepStatus::Completed if s.duration_ms.is_none() => {
                    if let Some(anchor) = s
                        .duration_anchor_at
                        .as_deref()
                        .and_then(|a| chrono::DateTime::parse_from_rfc3339(a).ok())
                    {
                        s.duration_ms =
                            Some((t - anchor.to_utc()).num_milliseconds().max(0) as u64);
                    }
                }
                _ => {}
            }
        }
        let done = steps.iter().all(|s| s.status == PlanStepStatus::Completed);
        let body = TurnItemBody::TodoList {
            plan_id: plan_id.to_string(),
            steps,
            explanation,
        };
        let status = if done {
            ItemStatus::Completed
        } else {
            ItemStatus::Running
        };
        match existing {
            Some(id) => {
                self.with_item(&id, |i| {
                    i.body = body;
                    i.status = status;
                });
            }
            None => {
                let run_id = self.active_run_id();
                self.add_item(run_id.as_deref(), status, None, body);
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // terminal + checkpoints (spec/07 §2.2 steps 5-7)

    fn finish_run(
        &mut self,
        run_id: &str,
        status: TerminalStatus,
        fail: Option<ProviderFailure>,
        reason: Option<String>,
        broken: bool,
    ) {
        let Some(run) = self.run(run_id).cloned() else {
            return;
        };
        if run.status.is_terminal() {
            return;
        }
        let t = now();
        let run_status: RunStatus = status.into();
        self.pending_interrupt.remove(run_id);
        self.with_run(run_id, |r| {
            r.status = run_status;
            r.completed_at = Some(t);
            r.queue_position = None;
            if fail.is_some() {
                r.failure = fail.clone();
            }
        });

        // every terminal path settles pending requests and open items (spec/07 §2.5)
        let mut cancelled_items = HashSet::new();
        for q in self.d.requests.iter_mut().filter(|q| {
            q.status == RequestStatus::Pending && q.run_id.as_deref().is_none_or(|r| r == run_id)
        }) {
            q.status = RequestStatus::Cancelled;
            q.resolved_at = Some(t);
            cancelled_items.insert(q.turn_item_id.clone());
            self.dirty.requests.insert(q.id.clone());
        }
        for item in self
            .d
            .items
            .iter_mut()
            .filter(|i| i.run_id.as_deref() == Some(run_id))
        {
            let open = item.status.is_open();
            if !open && !is_streaming(&item.body) {
                continue;
            }
            if open {
                item.status = if cancelled_items.contains(&item.id) {
                    ItemStatus::Cancelled
                } else if run_status == RunStatus::Completed {
                    ItemStatus::Completed
                } else {
                    ItemStatus::Interrupted
                };
                item.completed_at.get_or_insert(t);
            }
            stop_streaming(&mut item.body);
            item.updated_at = t;
            self.d.live_output.remove(&item.id);
            self.dirty.items.insert(item.id.clone());
        }

        self.set_binding_status(if broken {
            ProviderThreadStatus::Error
        } else {
            ProviderThreadStatus::Idle
        });
        if broken {
            if let Some(s) = self.session.take() {
                s.handle.kill();
            }
            self.set_session_status(SessionStatus::Error);
        } else {
            self.set_session_status(SessionStatus::Ready);
        }

        match status {
            TerminalStatus::Interrupted => {
                let message = reason.unwrap_or_else(|| {
                    if run.interrupt_requested_at.is_some() {
                        USER_STOP_REASON.into()
                    } else {
                        PROVIDER_STOP_REASON.into()
                    }
                });
                self.add_item(
                    Some(run_id),
                    ItemStatus::Interrupted,
                    Some("Interrupted"),
                    TurnItemBody::RunInterruptResult { message },
                );
            }
            TerminalStatus::Completed => {
                if let Some(r) = reason.filter(|r| !r.trim().is_empty()) {
                    self.add_item(
                        Some(run_id),
                        ItemStatus::Completed,
                        Some(PROVIDER_STOP_REASON),
                        TurnItemBody::SystemNotice {
                            message: r,
                            level: Some(NoticeLevel::Warning),
                        },
                    );
                }
            }
            TerminalStatus::Failed => {
                let has_error = self.d.items.iter().any(|i| {
                    i.run_id.as_deref() == Some(run_id)
                        && matches!(i.body, TurnItemBody::Error { .. })
                });
                if !has_error {
                    let f = fail.unwrap_or_else(|| {
                        failure(
                            FailureClass::Unknown,
                            reason.unwrap_or_else(|| "The run failed.".into()),
                        )
                    });
                    self.add_item(
                        Some(run_id),
                        ItemStatus::Failed,
                        None,
                        TurnItemBody::Error {
                            failure: f,
                            retry: None,
                        },
                    );
                }
            }
            TerminalStatus::Cancelled => {}
        }

        self.schedule_idle();
        let ran = run.started_at.is_some() || run.status == RunStatus::Running;
        if ran && status != TerminalStatus::Failed {
            self.start_capture(run_id);
        } else {
            self.advance_queue();
        }
    }

    fn start_capture(&mut self, run_id: &str) {
        let Some(run) = self.run(run_id).cloned() else {
            return;
        };
        let Some(cwd) = self.cwd() else {
            self.advance_queue();
            return;
        };
        self.capturing = Some(run_id.to_string());
        let prev = self
            .d
            .checkpoints
            .iter()
            .find(|c| {
                c.ordinal == run.ordinal.saturating_sub(1) && c.status == CheckpointStatus::Ready
            })
            .map(|c| c.git_ref.clone());
        let git = self.inner.backends.git.clone();
        let thread_id = self.id().to_string();
        let ordinal = run.ordinal;
        let tx = self.tx.clone();
        let run_id = run_id.to_string();
        self.inner.handle.spawn_blocking(move || {
            let (git_ref, status, files) = if git.repo_root(&cwd).is_none() {
                (String::new(), CheckpointStatus::Missing, vec![])
            } else {
                match git.capture(&cwd, &thread_id, ordinal) {
                    Ok(r) => {
                        let files = match &prev {
                            Some(p) => git.summarize(&cwd, p, &r).unwrap_or_else(|e| {
                                tracing::warn!("checkpoint summary failed: {e:#}");
                                vec![]
                            }),
                            None => vec![],
                        };
                        (r, CheckpointStatus::Ready, files)
                    }
                    Err(e) => {
                        tracing::warn!("checkpoint capture failed: {e:#}");
                        (String::new(), CheckpointStatus::Error, vec![])
                    }
                }
            };
            let checkpoint = Checkpoint {
                id: checkpoint_id(&thread_id, ordinal),
                thread_id,
                run_id: Some(run_id.clone()),
                ordinal,
                git_ref,
                status,
                files,
                captured_at: now(),
            };
            let _ = tx.send(ActorMsg::CheckpointDone {
                run_id: Some(run_id),
                checkpoint,
                cwd: cwd.to_string_lossy().into_owned(),
            });
        });
    }

    fn checkpoint_done(&mut self, run_id: Option<String>, cp: Checkpoint, cwd: String) {
        let ready = cp.status == CheckpointStatus::Ready;
        let id = cp.id.clone();
        let files = cp.files.clone();
        self.put_checkpoint(cp, cwd);
        if let Some(rid) = &run_id {
            self.with_run(rid, |r| r.checkpoint_id = Some(id.clone()));
            if ready {
                let files_total = Some(totals(&files));
                self.add_item(
                    Some(rid),
                    ItemStatus::Completed,
                    None,
                    TurnItemBody::Checkpoint {
                        checkpoint_id: id,
                        files,
                        files_total,
                    },
                );
            }
        }
        if self.capturing.is_some() && self.capturing == run_id {
            self.capturing = None;
        }
        self.advance_queue();
    }

    // ------------------------------------------------------------------------------------
    // interrupt + force stop (spec/07 §2.5)

    fn interrupt(&mut self, run_id: &str) {
        let Some(run) = self.run(run_id).cloned() else {
            return;
        };
        if run.status == RunStatus::Queued {
            self.cancel_queued(run_id);
            return;
        }
        if !run.status.is_live() || run.interrupt_requested_at.is_some() {
            return;
        }
        self.with_run(run_id, |r| r.interrupt_requested_at = Some(now()));
        self.add_item(
            Some(run_id),
            ItemStatus::Completed,
            Some("Interrupt requested"),
            TurnItemBody::RunInterruptRequest {
                message: USER_STOP_REASON.into(),
                unresponsive_at: None,
            },
        );
        match run.status {
            RunStatus::Preparing => {
                self.finish_run(run_id, TerminalStatus::Interrupted, None, None, false);
                return;
            }
            RunStatus::Starting => {
                self.pending_interrupt.insert(run_id.to_string());
            }
            _ if self.session.is_some() => self.send_interrupt(run_id),
            _ => {
                self.finish_run(run_id, TerminalStatus::Interrupted, None, None, false);
                return;
            }
        }
        let tx = self.tx.clone();
        let after = self.inner.backends.timing.interrupt_timeout;
        let run_id = run_id.to_string();
        self.inner.handle.spawn(async move {
            tokio::time::sleep(after).await;
            let _ = tx.send(ActorMsg::InterruptTimeout { run_id });
        });
    }

    fn send_interrupt(&self, run_id: &str) {
        if let Some(s) = &self.session {
            let h = s.handle.clone();
            let run_id = run_id.to_string();
            self.inner.handle.spawn(async move {
                if let Err(e) = h.interrupt(&run_id).await {
                    tracing::warn!("interrupt failed: {e:#}");
                }
            });
        }
    }

    fn interrupt_timeout(&mut self, run_id: &str) {
        if self.run(run_id).is_none_or(|r| !r.status.is_live()) {
            return;
        }
        let t = now();
        self.with_run(run_id, |r| r.interrupt_unresponsive_at = Some(t));
        let item = self
            .d
            .items
            .iter()
            .rev()
            .find(|i| {
                i.run_id.as_deref() == Some(run_id)
                    && matches!(i.body, TurnItemBody::RunInterruptRequest { .. })
            })
            .map(|i| i.id.clone());
        if let Some(id) = item {
            self.with_item(&id, |i| {
                if let TurnItemBody::RunInterruptRequest {
                    unresponsive_at, ..
                } = &mut i.body
                {
                    *unresponsive_at = Some(t);
                }
            });
        }
    }

    fn force_stop(&mut self) {
        if let Some(s) = self.session.take() {
            s.handle.kill();
        }
        self.opening_session = None;
        self.set_session_status(SessionStatus::Stopped);
        let live: Vec<String> = self
            .d
            .runs
            .iter()
            .filter(|r| r.status.is_live())
            .map(|r| r.id.clone())
            .collect();
        for id in live {
            self.finish_run(
                &id,
                TerminalStatus::Interrupted,
                None,
                Some(FORCE_STOP_REASON.into()),
                false,
            );
        }
    }

    fn session_closed(&mut self, session_id: &str) {
        if self.session.as_ref().is_none_or(|s| s.id != session_id) {
            return;
        }
        self.session = None;
        self.set_session_status(SessionStatus::Stopped);
        let live: Vec<String> = self
            .d
            .runs
            .iter()
            .filter(|r| matches!(r.status, RunStatus::Running | RunStatus::Waiting))
            .map(|r| r.id.clone())
            .collect();
        for id in live {
            self.finish_run(
                &id,
                TerminalStatus::Failed,
                Some(failure(
                    FailureClass::TransportError,
                    "The agent process exited unexpectedly.",
                )),
                None,
                true,
            );
        }
    }

    fn release_session(&mut self) {
        if let Some(s) = self.session.take() {
            let h = s.handle.clone();
            self.inner.handle.spawn(async move { h.shutdown().await });
            self.set_session_status(SessionStatus::Stopped);
            self.set_binding_status(ProviderThreadStatus::NotLoaded);
        }
    }

    fn restart_session_when_idle(&mut self) {
        if self.active_run_id().is_some() {
            self.restart_session = true;
        } else {
            self.release_session();
        }
    }

    fn schedule_idle(&mut self) {
        self.idle_gen += 1;
        let generation = self.idle_gen;
        let timing = &self.inner.backends.timing;
        let after = if self
            .binding
            .as_ref()
            .is_some_and(|b| b.native_thread_id.is_some())
        {
            timing.idle_timeout
        } else {
            timing.idle_timeout_no_resume
        };
        let tx = self.tx.clone();
        self.inner.handle.spawn(async move {
            tokio::time::sleep(after).await;
            let _ = tx.send(ActorMsg::IdleCheck { generation });
        });
    }

    // ------------------------------------------------------------------------------------
    // approvals and questions (spec/07 §2.6)

    fn respond(
        &mut self,
        request_id: &str,
        decision: Option<ApprovalDecision>,
        feedback: Option<String>,
        answers: Option<HashMap<String, serde_json::Value>>,
    ) {
        let Some(req) = self.d.requests.iter().find(|r| r.id == request_id).cloned() else {
            self.inner.error("This request no longer exists.");
            return;
        };
        if req.status != RequestStatus::Pending {
            self.inner.error("This request was already answered.");
            return;
        }
        let t = now();
        self.with_request(request_id, |r| {
            r.status = RequestStatus::Resolved;
            r.resolved_at = Some(t);
            r.decision = decision;
            r.feedback = feedback.clone();
            r.answers = answers.clone();
        });
        let answer_payload = answers.clone().unwrap_or_default();
        self.with_item(&req.turn_item_id, |i| {
            i.status = ItemStatus::Completed;
            i.completed_at = Some(t);
            if let TurnItemBody::UserInputRequest {
                request_id,
                questions,
                question_answer,
            } = &mut i.body
            {
                *question_answer = Some(UserInputAnswerPayload {
                    request_id: request_id.clone(),
                    question_text_by_id: Some(
                        questions
                            .iter()
                            .map(|q| (q.id.clone(), q.question.clone()))
                            .collect(),
                    ),
                    answers: answer_payload,
                    attachments_by_question_id: HashMap::new(),
                });
            }
        });
        if !self
            .d
            .requests
            .iter()
            .any(|r| r.status == RequestStatus::Pending)
        {
            self.set_session_status(SessionStatus::Running);
        }
        let resolved = self.d.requests.iter().find(|r| r.id == request_id).cloned();
        if let Some(resolved) = resolved {
            self.deliver(RequestResponse {
                request: resolved,
                decision,
                feedback,
                answers,
            });
        }
    }

    fn deliver(&mut self, response: RequestResponse) {
        let live = self.session.as_ref().filter(|s| {
            response
                .request
                .live_session_id
                .as_ref()
                .is_none_or(|id| *id == s.id)
        });
        match live {
            Some(s) => {
                let h = s.handle.clone();
                let tx = self.tx.clone();
                self.inner.handle.spawn(async move {
                    if let Err(e) = h.respond(response).await {
                        tracing::warn!("request delivery failed: {e:#}");
                        let _ = tx.send(ActorMsg::DeliveryFailed);
                    }
                });
            }
            None => self.delivery_failed(),
        }
    }

    fn delivery_failed(&mut self) {
        let run_id = self.active_run_id();
        self.add_item(
            run_id.as_deref(),
            ItemStatus::Completed,
            None,
            TurnItemBody::SystemNotice {
                message: DELIVERY_FAILED.into(),
                level: Some(NoticeLevel::Warning),
            },
        );
    }

    fn dismiss(&mut self, request_id: &str) {
        let Some(req) = self.d.requests.iter().find(|r| r.id == request_id).cloned() else {
            return;
        };
        if req.status != RequestStatus::Pending {
            return;
        }
        let t = now();
        self.with_request(request_id, |r| {
            r.status = RequestStatus::Cancelled;
            r.resolved_at = Some(t);
            r.decision = Some(ApprovalDecision::Cancel);
        });
        self.with_item(&req.turn_item_id, |i| {
            i.status = ItemStatus::Cancelled;
            i.completed_at = Some(t);
        });
        if let Some(s) = &self.session {
            let h = s.handle.clone();
            let mut request = req;
            request.status = RequestStatus::Cancelled;
            self.inner.handle.spawn(async move {
                let _ = h
                    .respond(RequestResponse {
                        request,
                        decision: Some(ApprovalDecision::Cancel),
                        feedback: None,
                        answers: None,
                    })
                    .await;
            });
        }
    }

    // ------------------------------------------------------------------------------------
    // revert (spec/07 §7.5)

    fn rollback(&mut self, checkpoint_id: &str, restore_files: bool) {
        if self.active_run_id().is_some() || self.capturing.is_some() || self.rolling_back {
            self.inner.error("Stop the agent before reverting.");
            return;
        }
        let Some(cp) = self
            .d
            .checkpoints
            .iter()
            .find(|c| c.id == checkpoint_id)
            .cloned()
        else {
            self.inner.error("This checkpoint no longer exists.");
            return;
        };
        if !restore_files {
            self.apply_rollback(&cp);
            return;
        }
        if cp.status != CheckpointStatus::Ready {
            self.inner
                .error("This checkpoint can't restore files. Revert the conversation only, or pick another turn.");
            return;
        }
        let Some(cwd) = self
            .checkpoint_cwds
            .get(&cp.id)
            .map(PathBuf::from)
            .or_else(|| self.cwd())
        else {
            return;
        };
        self.rolling_back = true;
        let git = self.inner.backends.git.clone();
        let tx = self.tx.clone();
        let id = cp.id.clone();
        self.inner.handle.spawn_blocking(move || {
            let result = git.restore(&cwd, &cp.git_ref).map_err(|e| format!("{e:#}"));
            let _ = tx.send(ActorMsg::RollbackDone {
                checkpoint_id: id,
                result,
            });
        });
    }

    fn rollback_done(&mut self, checkpoint_id: &str, result: Result<(), String>) {
        self.rolling_back = false;
        match result {
            Ok(()) => {
                if let Some(cp) = self
                    .d
                    .checkpoints
                    .iter()
                    .find(|c| c.id == checkpoint_id)
                    .cloned()
                {
                    self.apply_rollback(&cp);
                }
            }
            Err(e) => self.inner.error(format!("Could not restore files: {e}")),
        }
        self.advance_queue();
    }

    fn apply_rollback(&mut self, cp: &Checkpoint) {
        let k = cp.ordinal;
        let t = now();
        let rolled: Vec<String> = self
            .d
            .runs
            .iter()
            .filter(|r| r.ordinal > k && r.status != RunStatus::RolledBack)
            .map(|r| r.id.clone())
            .collect();
        for id in &rolled {
            self.with_run(id, |r| {
                r.status = RunStatus::RolledBack;
                r.queue_position = None;
                r.completed_at.get_or_insert(t);
            });
        }
        for c in self.d.checkpoints.iter_mut().filter(|c| c.ordinal > k) {
            c.status = CheckpointStatus::Stale;
            self.dirty.checkpoints.insert(c.id.clone());
        }
        for p in self.d.plans.iter_mut().filter(|p| {
            p.run_id.as_ref().is_some_and(|r| rolled.contains(r))
                && matches!(p.status, PlanStatus::Active | PlanStatus::Draft)
        }) {
            p.status = PlanStatus::Superseded;
            p.updated_at = t;
            self.dirty.plans.insert(p.id.clone());
        }
        // The adapter trait has no conversation rewind. Reverting to the start begins a fresh
        // provider conversation; a partial revert keeps the provider's history as is.
        if k == 0 {
            self.release_session();
            if let Some(b) = &mut self.binding {
                b.native_thread_id = None;
                b.updated_at = t;
                self.dirty.binding = true;
            }
            self.d.thread.active_provider_thread_id = None;
        }
        self.touch();
    }

    // ------------------------------------------------------------------------------------
    // model and modes (spec/07 §2.7)

    fn set_model_selection(&mut self, ms: ModelSelection) {
        if ms == self.d.thread.model_selection {
            return;
        }
        let instance_changed = ms.instance_id != self.d.thread.model_selection.instance_id;
        self.d.thread.model_selection = ms.clone();
        self.touch();
        let Some(s) = &self.session else {
            return;
        };
        if instance_changed || !s.capabilities.supports_model_switch_in_session {
            self.restart_session_when_idle();
        } else {
            let h = s.handle.clone();
            self.inner.handle.spawn(async move {
                if let Err(e) = h.set_model(ms).await {
                    tracing::warn!("set_model failed: {e:#}");
                }
            });
        }
    }

    // ------------------------------------------------------------------------------------
    // titles (spec/07 §2.9)

    fn generate_title(&mut self, regenerate: bool) {
        let Some(context) = title_context(&self.d) else {
            if regenerate {
                self.inner
                    .error("This thread has no messages to title yet.");
            }
            return;
        };
        let Some(cwd) = self.cwd() else {
            if regenerate {
                self.inner.error("This thread's project no longer exists.");
            }
            return;
        };
        self.d.thread.title_generation_started_at = Some(now());
        self.dirty.thread = true;
        let settings = self.inner.backends.settings.get();
        let model = settings.text_generation_model_selection.clone();
        let titles = self.inner.backends.titles.clone();
        let timing = self.inner.backends.timing.clone();
        let retries = if regenerate { 0 } else { timing.title_retries };
        let tx = self.tx.clone();
        self.inner.handle.spawn(async move {
            let mut attempt = 0;
            let result = loop {
                match titles
                    .thread_title(model.clone(), context.clone(), cwd.clone())
                    .await
                {
                    Ok(t) => break Ok(t),
                    Err(e) if attempt < retries => {
                        tracing::debug!("title attempt {attempt} failed: {e:#}");
                        tokio::time::sleep(timing.title_retry_base * 2u32.pow(attempt)).await;
                        attempt += 1;
                    }
                    Err(e) => break Err(last_line(&format!("{e:#}"))),
                }
            };
            let _ = tx.send(ActorMsg::TitleDone { result, regenerate });
        });
    }

    fn title_done(&mut self, result: Result<String, String>, regenerate: bool) {
        self.d.thread.title_generation_started_at = None;
        self.dirty.thread = true;
        if self.d.thread.title_renamed {
            return;
        }
        let title = match result {
            Ok(raw) => sanitize_title(&raw)
                .ok_or_else(|| "The model returned the placeholder title \"title\".".to_string()),
            Err(e) => Err(e),
        };
        match title {
            Ok(t) if t != self.d.thread.title => {
                self.d.thread.title = t;
                self.touch();
            }
            Ok(_) => {}
            Err(e) => {
                tracing::info!(thread = %self.id(), "title generation failed: {e}");
                if regenerate {
                    self.inner.error(e);
                }
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // delete

    fn delete(&mut self) {
        if let Some(s) = self.session.take() {
            s.handle.kill();
        }
        let id = self.id().to_string();
        self.d.thread.deleted_at = Some(now());
        let res = {
            let mut store = self.inner.store.lock();
            store.write(|tx| {
                tx.upsert_thread(&self.d.thread)?;
                tx.purge_thread_content(&id)
            })
        };
        if let Err(e) = res {
            self.inner.error(format!("Failed to delete thread: {e:#}"));
            self.d.thread.deleted_at = None;
            return;
        }
        self.dirty = Dirty::default();
        self.inner.state.write().threads.remove(&id);
        self.inner.actors.lock().remove(&id);
        self.inner.next_revision();
        self.inner.emit(Update::Snapshot);
        self.inner.spawn_purge(id);
        self.gone = true;
    }
}

async fn capture_baseline(
    git: Arc<dyn crate::backend::GitBackend>,
    cwd: PathBuf,
    thread_id: String,
    ordinal: u32,
    budget: std::time::Duration,
) -> Option<(Checkpoint, String)> {
    let cwd2 = cwd.clone();
    let tid = thread_id.clone();
    let job = tokio::task::spawn_blocking(move || -> Option<String> {
        git.repo_root(&cwd2)?;
        match git.capture(&cwd2, &tid, ordinal) {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::warn!("baseline capture failed: {e:#}");
                None
            }
        }
    });
    let git_ref = tokio::time::timeout(budget, job).await.ok()?.ok()??;
    Some((
        Checkpoint {
            id: checkpoint_id(&thread_id, ordinal),
            thread_id,
            run_id: None,
            ordinal,
            git_ref,
            status: CheckpointStatus::Ready,
            files: vec![],
            captured_at: now(),
        },
        cwd.to_string_lossy().into_owned(),
    ))
}

async fn open_session(
    adapter: Arc<dyn ProviderAdapter>,
    config: SessionConfig,
    session_id: String,
    cwd: PathBuf,
    actor: mpsc::UnboundedSender<ActorMsg>,
) -> Result<LiveSession, String> {
    let (etx, mut erx) = mpsc::unbounded_channel();
    let handle = adapter
        .open_session(config, etx)
        .await
        .map_err(|e| format!("{e:#}"))?;
    let capabilities = handle.capabilities();
    let sid = session_id.clone();
    tokio::spawn(async move {
        while let Some(event) = erx.recv().await {
            if actor
                .send(ActorMsg::Provider {
                    session_id: sid.clone(),
                    event,
                })
                .is_err()
            {
                return;
            }
        }
        let _ = actor.send(ActorMsg::SessionClosed { session_id: sid });
    });
    Ok(LiveSession {
        id: session_id,
        instance_id: adapter.instance_id().to_string(),
        driver: adapter.driver(),
        handle: Arc::from(handle),
        capabilities,
        cwd: cwd.to_string_lossy().into_owned(),
        created_at: now(),
    })
}

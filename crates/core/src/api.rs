//! The contract between the GPUI app and the engine. The app sends `Command`s and
//! renders from `Snapshot`/`ThreadDetail`; the engine pushes `Update`s when state
//! changes. Nothing in here does IO.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::*;
use crate::provider::ProviderStatus;

pub type ProjectId = String;
pub type ThreadId = String;

/// Sidebar row for a thread, derived from thread + runs + requests
/// (`OrchestrationV2ThreadShell`, orchestrationV2.ts:2410-2496).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadShell {
    pub thread: Thread,
    pub status: ShellThreadStatus,
    pub active_run_id: Option<String>,
    pub latest_run_started_at: Option<Ts>,
    pub latest_run_completed_at: Option<Ts>,
    /// (request id, kind) of the oldest pending request: the "needs approval/input" badge.
    pub pending_request: Option<(String, RequestKind)>,
    /// role ("user" | "assistant") and text of the latest visible message.
    pub latest_message: Option<(String, String)>,
    pub has_actionable_proposed_plan: bool,
    pub last_error: Option<String>,
    pub last_error_class: Option<FailureClass>,
    pub usage_limit_reset_at: Option<String>,
    /// Totals of the latest checkpoint (sidebar diff stat).
    pub diff_totals: Option<FileTotals>,
    /// Latest activity time used for sorting (max of updated/run times).
    pub last_activity_at: Ts,
}

impl ThreadShell {
    /// Unread: there was activity after the user last looked at it.
    pub fn is_unread(&self) -> bool {
        match (self.latest_run_completed_at, self.thread.last_visited_at) {
            (Some(done), Some(seen)) => done > seen,
            (Some(_), None) => true,
            _ => false,
        }
    }

    pub fn is_working(&self) -> bool {
        matches!(self.status, ShellThreadStatus::Run(s) if s.is_live())
    }
}

/// Everything the sidebar, palette and settings need. Cheap to clone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub projects: Vec<Project>,
    pub threads: Vec<ThreadShell>,
    pub providers: Vec<ProviderStatus>,
    /// Draft threads not yet sent (composer state lives in the app).
    pub revision: u64,
}

impl Snapshot {
    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }
    pub fn thread(&self, id: &str) -> Option<&ThreadShell> {
        self.threads.iter().find(|t| t.thread.id == id)
    }
}

/// The full transcript of one thread, for the chat view.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadDetail {
    pub thread: Thread,
    pub runs: Vec<Run>,
    /// Sorted by ordinal.
    pub items: Vec<TurnItem>,
    pub requests: Vec<RuntimeRequest>,
    pub checkpoints: Vec<Checkpoint>,
    pub plans: Vec<ProposedPlan>,
    pub context_usage: Option<ContextUsage>,
    pub session: Option<ProviderSession>,
    /// Live command output by item id (not persisted).
    pub live_output: HashMap<String, String>,
    pub revision: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum WorkspaceStrategy {
    Root {
        branch: Option<String>,
    },
    ExistingWorktree {
        worktree_path: String,
        branch: Option<String>,
    },
    Worktree {
        base_ref: String,
        branch: Option<String>,
        start_from_origin: Option<bool>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum DispatchMode {
    StartImmediately,
    QueueAfterActive,
    SteerActive { target_run_id: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutgoingMessage {
    pub text: String,
    /// Attachment files already copied into `<state>/attachments/<id>`.
    pub attachments: Vec<ChatAttachment>,
    pub source_plan_id: Option<String>,
}

/// Diff scopes the diff panel can ask for (spec/05 §2).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DiffScope {
    /// Branch changes vs the merge base with `base_ref` (None = the default branch).
    /// This is the panel's default scope.
    Branch { base_ref: Option<String> },
    Uncommitted,
    Commit(String),
    /// Changes made by one run (checkpoint n-1 -> n).
    Turn { checkpoint_id: String },
}

#[derive(Clone, Debug)]
pub enum Command {
    AddProject {
        path: String,
    },
    RenameProject {
        project_id: ProjectId,
        title: String,
    },
    RemoveProject {
        project_id: ProjectId,
    },
    UpdateProjectScripts {
        project_id: ProjectId,
        scripts: Vec<ProjectScript>,
    },
    /// Create a thread and, when `message` is set, send it as the first turn.
    LaunchThread {
        thread_id: ThreadId,
        project_id: ProjectId,
        model_selection: ModelSelection,
        runtime_mode: RuntimeMode,
        interaction_mode: InteractionMode,
        workspace: WorkspaceStrategy,
        message: Option<OutgoingMessage>,
    },
    SendMessage {
        thread_id: ThreadId,
        message: OutgoingMessage,
        model_selection: Option<ModelSelection>,
        mode: DispatchMode,
    },
    Interrupt {
        thread_id: ThreadId,
        run_id: String,
    },
    ForceStop {
        thread_id: ThreadId,
    },
    RespondToRequest {
        thread_id: ThreadId,
        request_id: String,
        decision: Option<ApprovalDecision>,
        feedback: Option<String>,
        answers: Option<HashMap<String, serde_json::Value>>,
    },
    DismissUserInput {
        thread_id: ThreadId,
        request_id: String,
    },
    RollbackToCheckpoint {
        thread_id: ThreadId,
        checkpoint_id: String,
        restore_files: bool,
    },
    SetModelSelection {
        thread_id: ThreadId,
        model_selection: ModelSelection,
    },
    SetRuntimeMode {
        thread_id: ThreadId,
        runtime_mode: RuntimeMode,
    },
    SetInteractionMode {
        thread_id: ThreadId,
        interaction_mode: InteractionMode,
    },
    RenameThread {
        thread_id: ThreadId,
        title: String,
    },
    RegenerateTitle {
        thread_id: ThreadId,
    },
    SetPinned {
        thread_id: ThreadId,
        pinned: bool,
    },
    SetSettled {
        thread_id: ThreadId,
        settled: bool,
    },
    SetArchived {
        thread_id: ThreadId,
        archived: bool,
    },
    /// Record the thread's checkout after a branch switch (spec/05 §4a.8).
    SetThreadBranch {
        thread_id: ThreadId,
        branch: Option<String>,
        worktree_path: Option<String>,
    },
    DeleteThread {
        thread_id: ThreadId,
    },
    MarkVisited {
        thread_id: ThreadId,
    },
    MarkUnread {
        thread_id: ThreadId,
    },
    CancelQueuedRun {
        thread_id: ThreadId,
        run_id: String,
    },
    RefreshProviders,
}

/// Pushed by the engine whenever something changed. The app re-reads what it shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    /// Projects, thread shells or provider statuses changed.
    Snapshot,
    /// One thread's transcript changed.
    Thread(ThreadId),
    /// A user-facing error from a command (toast).
    Error { message: String },
    /// A user-facing notice (toast).
    Notice { message: String },
}

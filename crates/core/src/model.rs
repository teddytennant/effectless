//! Domain model. Field names and enum wire values follow T3 Code's orchestration V2
//! contracts (spec/07 §1) so JSON written by Tern reads like T3's.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type Ts = DateTime<Utc>;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeMode {
    ApprovalRequired,
    AutoAcceptEdits,
    Auto,
    #[default]
    FullAccess,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum InteractionMode {
    #[default]
    Default,
    Plan,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum RequestKind {
    Command,
    FileRead,
    FileChange,
    McpElicitation,
    Permission,
    #[serde(rename = "user_input")]
    UserInput,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalDecision {
    Accept,
    AcceptForSession,
    AcceptAlways,
    Decline,
    Cancel,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum FullAccessOverride {
    AskRule,
    ProviderCheck,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Preparing,
    Queued,
    Starting,
    Running,
    Waiting,
    Completed,
    Interrupted,
    Failed,
    Cancelled,
    RolledBack,
}

impl RunStatus {
    /// `activityRunStatus`: the live subset (orchestrationV2.ts:2438-2441).
    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Preparing | Self::Starting | Self::Running | Self::Waiting
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Interrupted | Self::Failed | Self::Cancelled | Self::RolledBack
        )
    }
}

/// Sidebar status: "idle" or the latest run's status.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(untagged)]
pub enum ShellThreadStatus {
    #[default]
    #[serde(rename = "idle")]
    Idle,
    Run(RunStatus),
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Starting,
    Ready,
    Running,
    Waiting,
    Stopped,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum ItemStatus {
    Idle,
    Pending,
    Running,
    Waiting,
    #[default]
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

impl ItemStatus {
    pub fn is_open(self) -> bool {
        matches!(self, Self::Idle | Self::Pending | Self::Running | Self::Waiting)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum RequestStatus {
    Pending,
    Resolved,
    Expired,
    Cancelled,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    UsageLimit,
    ProviderError,
    TransportError,
    PermissionError,
    ValidationError,
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum InputIntent {
    #[default]
    TurnStart,
    QueuedTurn,
    Steer,
    PromotedQueuedToSteer,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThreadEnvMode {
    #[default]
    Local,
    Worktree,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProjectScriptIcon {
    #[default]
    Play,
    Test,
    Lint,
    Configure,
    Build,
    Debug,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Driver {
    Codex,
    Claude,
    Acp,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderThreadStatus {
    #[default]
    NotLoaded,
    Idle,
    Active,
    Archived,
    Closed,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum TerminalStatus {
    Completed,
    Interrupted,
    Cancelled,
    Failed,
}

impl From<TerminalStatus> for RunStatus {
    fn from(s: TerminalStatus) -> Self {
        match s {
            TerminalStatus::Completed => RunStatus::Completed,
            TerminalStatus::Interrupted => RunStatus::Interrupted,
            TerminalStatus::Cancelled => RunStatus::Cancelled,
            TerminalStatus::Failed => RunStatus::Failed,
        }
    }
}

// ---- model selection ----

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum OptionValue {
    Str(String),
    Bool(bool),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct OptionSelection {
    pub id: String,
    pub value: OptionValue,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelection {
    /// "codex" | "claudeAgent" | "acp:<id>"
    pub instance_id: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<OptionSelection>,
}

impl ModelSelection {
    pub fn option(&self, id: &str) -> Option<&OptionValue> {
        self.options.iter().find(|o| o.id == id).map(|o| &o.value)
    }

    pub fn option_str(&self, id: &str) -> Option<&str> {
        match self.option(id) {
            Some(OptionValue::Str(s)) => Some(s),
            _ => None,
        }
    }

    pub fn set_option(&mut self, id: &str, value: OptionValue) {
        match self.options.iter_mut().find(|o| o.id == id) {
            Some(o) => o.value = value,
            None => self.options.push(OptionSelection {
                id: id.to_string(),
                value,
            }),
        }
    }
}

// ---- projects and threads ----

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScript {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub icon: ProjectScriptIcon,
    #[serde(default)]
    pub run_on_worktree_create: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub has_custom_title: bool,
    pub workspace_root: String,
    pub default_model_selection: Option<ModelSelection>,
    pub default_thread_env_mode: Option<ThreadEnvMode>,
    pub favicon_path: Option<String>,
    #[serde(default)]
    pub scripts: Vec<ProjectScript>,
    pub pinned_at: Option<Ts>,
    pub archived_at: Option<Ts>,
    pub created_at: Ts,
    pub updated_at: Ts,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LinkedPullRequest {
    pub number: u64,
    pub url: String,
    pub title: String,
    /// open | closed | merged | draft
    pub state: String,
}

pub const NEW_THREAD_TITLE: &str = "New thread";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub model_selection: ModelSelection,
    pub runtime_mode: RuntimeMode,
    pub interaction_mode: InteractionMode,
    pub branch: Option<String>,
    pub worktree_path: Option<String>,
    pub active_provider_thread_id: Option<String>,
    #[serde(default)]
    pub title_renamed: bool,
    pub title_generation_started_at: Option<Ts>,
    pub created_at: Ts,
    pub updated_at: Ts,
    pub archived_at: Option<Ts>,
    pub pinned_at: Option<Ts>,
    /// Settled threads drop to the Settled section of the sidebar.
    pub settled_at: Option<Ts>,
    pub last_visited_at: Option<Ts>,
    pub linked_pull_request: Option<LinkedPullRequest>,
    pub deleted_at: Option<Ts>,
}

// ---- runs ----

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenCounts {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    #[serde(default)]
    pub has_subagents: bool,
    pub models: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "usageStatus", rename_all = "lowercase")]
pub enum TurnTokenUsage {
    Complete(TokenCounts),
    Partial(TokenCounts),
    Unavailable(TokenCounts),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub used_tokens: u64,
    pub max_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
    pub last_used_tokens: Option<u64>,
    pub auto_compact_threshold: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFailure {
    pub class: FailureClass,
    pub message: String,
    pub code: Option<String>,
    pub retryable: Option<bool>,
    pub reset_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRetry {
    pub attempt: u32,
    pub max_attempts: Option<u32>,
    pub retry_delay_ms: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub thread_id: String,
    pub ordinal: u32,
    pub model_selection: ModelSelection,
    pub runtime_mode: RuntimeMode,
    pub interaction_mode: InteractionMode,
    pub user_message_id: String,
    pub status: RunStatus,
    pub queue_position: Option<u32>,
    pub requested_at: Ts,
    pub started_at: Option<Ts>,
    pub completed_at: Option<Ts>,
    pub checkpoint_id: Option<String>,
    pub source_plan_id: Option<String>,
    pub native_turn_id: Option<String>,
    pub model_reported: Option<String>,
    pub token_usage: Option<TurnTokenUsage>,
    pub failure: Option<ProviderFailure>,
    pub interrupt_requested_at: Option<Ts>,
    pub interrupt_unresponsive_at: Option<Ts>,
}

// ---- turn items ----

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AssistantPhase {
    Commentary,
    PartialAnswer,
    FinalAnswer,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum PlanStepStatus {
    Pending,
    Running,
    Completed,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub id: String,
    pub text: String,
    pub status: PlanStepStatus,
    pub duration_anchor_at: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
    pub value: Option<String>,
    pub preview: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserInputQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    #[serde(default)]
    pub options: Vec<QuestionOption>,
    pub multi_select: Option<bool>,
    pub allow_custom_answer: Option<bool>,
    pub initial_answer: Option<String>,
    pub required: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserInputAnswerPayload {
    pub request_id: String,
    pub question_text_by_id: Option<HashMap<String, String>>,
    /// questionId -> string | string[] | {answers: string[]}
    pub answers: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub attachments_by_question_id: HashMap<String, Vec<ChatAttachment>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalOption {
    pub decision: ApprovalDecision,
    pub label: String,
    pub warning: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeDetail {
    /// add | delete | update | move
    pub operation: String,
    pub path: String,
    pub old_path: Option<String>,
    pub file_type: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileSearchResult {
    pub path: String,
    pub line: Option<u32>,
    pub preview: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchResult {
    pub title: Option<String>,
    pub url: String,
}

/// Attachment files live under `<state>/attachments/<id>`, never in the repo.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum ChatAttachment {
    Image {
        id: String,
        name: String,
        mime_type: String,
        size_bytes: u64,
    },
    File {
        id: String,
        name: String,
        mime_type: String,
        size_bytes: u64,
    },
}

impl ChatAttachment {
    pub fn id(&self) -> &str {
        match self {
            Self::Image { id, .. } | Self::File { id, .. } => id,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Image { name, .. } | Self::File { name, .. } => name,
        }
    }
}

pub const MAX_INPUT_CHARS: usize = 120_000;
pub const MAX_ATTACHMENTS: usize = 100;
pub const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelSwitch {
    pub from: ModelSelection,
    pub to: ModelSelection,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum NoticeLevel {
    Info,
    Warning,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointFileSummary {
    pub path: String,
    /// added | deleted | modified | renamed
    pub kind: String,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileTotals {
    pub count: u32,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum TurnItemBody {
    UserMessage {
        message_id: String,
        text: String,
        #[serde(default)]
        attachments: Vec<ChatAttachment>,
        #[serde(default)]
        input_intent: InputIntent,
        #[serde(default)]
        model_switch: Option<ModelSwitch>,
    },
    AssistantMessage {
        message_id: String,
        text: String,
        streaming: bool,
        #[serde(default)]
        phase: Option<AssistantPhase>,
    },
    Reasoning {
        text: String,
        streaming: bool,
    },
    ProposedPlan {
        plan_id: String,
        markdown: String,
        streaming: bool,
    },
    TodoList {
        plan_id: String,
        steps: Vec<PlanStep>,
        explanation: Option<String>,
    },
    UserInputRequest {
        request_id: String,
        questions: Vec<UserInputQuestion>,
        question_answer: Option<UserInputAnswerPayload>,
    },
    FileChange {
        file_name: String,
        additions: Option<u32>,
        deletions: Option<u32>,
        diff_str: Option<String>,
        old_str: Option<String>,
        new_str: Option<String>,
        changes: Option<Vec<FileChangeDetail>>,
    },
    CommandExecution {
        input: String,
        output: Option<String>,
        exit_code: Option<i32>,
        output_indicates_failure: Option<bool>,
    },
    FileSearch {
        pattern: Option<String>,
        results: Option<Vec<FileSearchResult>>,
    },
    WebSearch {
        patterns: Option<Vec<String>>,
        results: Option<Vec<WebSearchResult>>,
    },
    ApprovalRequest {
        request_id: String,
        request_kind: RequestKind,
        prompt: Option<String>,
        reason: Option<String>,
        full_access_override: Option<FullAccessOverride>,
        options: Option<Vec<ApprovalOption>>,
    },
    Checkpoint {
        checkpoint_id: String,
        files: Vec<CheckpointFileSummary>,
        files_total: Option<FileTotals>,
    },
    RunInterruptRequest {
        message: String,
        unresponsive_at: Option<Ts>,
    },
    RunInterruptResult {
        message: String,
    },
    SystemNotice {
        message: String,
        level: Option<NoticeLevel>,
    },
    Error {
        failure: ProviderFailure,
        retry: Option<ProviderRetry>,
    },
    Compaction {
        summary: Option<String>,
        before_token_count: Option<u32>,
        after_token_count: Option<u32>,
    },
    Subagent {
        subagent_id: String,
        prompt: String,
        progress: Option<String>,
        result: Option<String>,
    },
    DynamicTool {
        tool_name: Option<String>,
        input: serde_json::Value,
        output: Option<serde_json::Value>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnItem {
    pub id: String,
    pub thread_id: String,
    pub run_id: Option<String>,
    pub native_item_id: Option<String>,
    pub parent_item_id: Option<String>,
    /// Sort key within the thread: run.ordinal * 100 + n (RunExecutionService.ts:962).
    pub ordinal: u32,
    pub status: ItemStatus,
    pub title: Option<String>,
    pub started_at: Option<Ts>,
    pub completed_at: Option<Ts>,
    pub updated_at: Ts,
    #[serde(flatten)]
    pub body: TurnItemBody,
}

// ---- plans, requests, checkpoints ----

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum PlanStatus {
    Draft,
    Active,
    Completed,
    Superseded,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProposedPlan {
    pub id: String,
    pub thread_id: String,
    pub run_id: Option<String>,
    pub markdown: String,
    pub status: PlanStatus,
    pub created_at: Ts,
    pub updated_at: Ts,
}

/// "Implement plan" prompt prefix (apps/web/src/proposedPlan.ts:58).
pub const IMPLEMENT_PLAN_PREFIX: &str = "PLEASE IMPLEMENT THIS PLAN:\n";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRequest {
    pub id: String,
    pub thread_id: String,
    pub run_id: Option<String>,
    pub turn_item_id: String,
    pub native_request_id: Option<serde_json::Value>,
    pub kind: RequestKind,
    pub status: RequestStatus,
    pub live_session_id: Option<String>,
    pub created_at: Ts,
    pub resolved_at: Option<Ts>,
    pub decision: Option<ApprovalDecision>,
    pub feedback: Option<String>,
    pub answers: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum CheckpointStatus {
    Ready,
    Missing,
    Error,
    Stale,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub id: String,
    pub thread_id: String,
    /// None = baseline (turn 0)
    pub run_id: Option<String>,
    pub ordinal: u32,
    pub git_ref: String,
    pub status: CheckpointStatus,
    pub files: Vec<CheckpointFileSummary>,
    pub captured_at: Ts,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub supports_interrupt: bool,
    pub supports_active_steering: bool,
    pub emits_proposed_plan: bool,
    pub can_rollback_conversation: bool,
    pub supports_model_switch_in_session: bool,
    pub supports_runtime_mode_switch_in_session: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSession {
    pub id: String,
    pub driver: Driver,
    pub status: SessionStatus,
    pub cwd: String,
    pub model: Option<String>,
    pub capabilities: Capabilities,
    pub startup_phase: Option<String>,
    pub last_error: Option<String>,
    pub created_at: Ts,
    pub updated_at: Ts,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderBinding {
    pub thread_id: String,
    pub driver: Driver,
    pub native_thread_id: Option<String>,
    pub status: ProviderThreadStatus,
    pub context_usage: Option<ContextUsage>,
    pub updated_at: Ts,
}

//! Turn items → timeline entries (`session-logic.ts:372-795`) and the per-entry
//! presentation helpers the rows use (`work-log/presentation.ts`,
//! `TimelineWorkEntryRow.logic.ts`, `toolActivity.ts`).
//!
//! Tern has no run attempts, notifications, forks or MCP apps, so those branches of
//! the T3 logic are absent; everything else keeps T3's names and order of checks.

use std::rc::Rc;

use tern_core::{
    AssistantPhase, ChatAttachment, InputIntent, ItemStatus, NoticeLevel, RequestKind, Ts,
    TurnItem, TurnItemBody,
};

use crate::assets::IconName;

use super::format;

/// `WorkLogEntry.tone`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Thinking,
    Tool,
    Info,
    Error,
}

/// `WorkLogToolLifecycleStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    InProgress,
    Completed,
    Idle,
    Failed,
    Declined,
    Stopped,
}

impl Lifecycle {
    pub fn from_item(status: ItemStatus) -> Self {
        match status {
            ItemStatus::Pending | ItemStatus::Running | ItemStatus::Waiting => Self::InProgress,
            ItemStatus::Completed => Self::Completed,
            ItemStatus::Idle => Self::Idle,
            ItemStatus::Failed => Self::Failed,
            ItemStatus::Cancelled | ItemStatus::Interrupted => Self::Stopped,
        }
    }
}

/// The turn item's `type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemType {
    UserMessage,
    AssistantMessage,
    Reasoning,
    ProposedPlan,
    TodoList,
    UserInputRequest,
    FileChange,
    CommandExecution,
    FileSearch,
    WebSearch,
    ApprovalRequest,
    Checkpoint,
    RunInterruptRequest,
    RunInterruptResult,
    SystemNotice,
    Error,
    Compaction,
    Subagent,
    DynamicTool,
}

impl ItemType {
    pub fn of(body: &TurnItemBody) -> Self {
        match body {
            TurnItemBody::UserMessage { .. } => Self::UserMessage,
            TurnItemBody::AssistantMessage { .. } => Self::AssistantMessage,
            TurnItemBody::Reasoning { .. } => Self::Reasoning,
            TurnItemBody::ProposedPlan { .. } => Self::ProposedPlan,
            TurnItemBody::TodoList { .. } => Self::TodoList,
            TurnItemBody::UserInputRequest { .. } => Self::UserInputRequest,
            TurnItemBody::FileChange { .. } => Self::FileChange,
            TurnItemBody::CommandExecution { .. } => Self::CommandExecution,
            TurnItemBody::FileSearch { .. } => Self::FileSearch,
            TurnItemBody::WebSearch { .. } => Self::WebSearch,
            TurnItemBody::ApprovalRequest { .. } => Self::ApprovalRequest,
            TurnItemBody::Checkpoint { .. } => Self::Checkpoint,
            TurnItemBody::RunInterruptRequest { .. } => Self::RunInterruptRequest,
            TurnItemBody::RunInterruptResult { .. } => Self::RunInterruptResult,
            TurnItemBody::SystemNotice { .. } => Self::SystemNotice,
            TurnItemBody::Error { .. } => Self::Error,
            TurnItemBody::Compaction { .. } => Self::Compaction,
            TurnItemBody::Subagent { .. } => Self::Subagent,
            TurnItemBody::DynamicTool { .. } => Self::DynamicTool,
        }
    }

    /// `HIDEABLE_TOOL_CALL_ITEM_TYPES`.
    pub fn hideable(self) -> bool {
        matches!(
            self,
            Self::CommandExecution
                | Self::FileChange
                | Self::FileSearch
                | Self::WebSearch
                | Self::DynamicTool
        )
    }

    /// `isToolLifecycleItemType` for the types Tern has.
    fn tool_lifecycle(self) -> bool {
        matches!(
            self,
            Self::CommandExecution | Self::FileChange | Self::WebSearch | Self::DynamicTool
        )
    }
}

/// `sourceActivityKind` values the rows look at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    ContextCompaction,
    RuntimeWarning,
    RuntimeError,
}

#[derive(Clone, Debug)]
pub struct WorkEntry {
    pub id: String,
    pub created_at: Ts,
    pub run_id: Option<String>,
    pub label: String,
    pub detail: Option<String>,
    pub command: Option<String>,
    pub changed_files: Vec<String>,
    pub tone: Tone,
    pub item_type: ItemType,
    pub status: Lifecycle,
    pub tool_title: Option<String>,
    pub request_kind: Option<RequestKind>,
    pub source_kind: Option<SourceKind>,
    /// The item answered a question (`questionAnswer`).
    pub question_answer: bool,
    pub item: Rc<TurnItem>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug)]
pub struct MessageEntry {
    pub id: String,
    pub role: Role,
    pub text: String,
    pub run_id: Option<String>,
    pub streaming: bool,
    pub created_at: Ts,
    pub updated_at: Ts,
    pub input_intent: InputIntent,
    pub phase: Option<AssistantPhase>,
    pub attachments: Vec<ChatAttachment>,
    pub item: Rc<TurnItem>,
}

impl MessageEntry {
    pub fn is_steer(&self) -> bool {
        self.role == Role::User
            && matches!(
                self.input_intent,
                InputIntent::Steer | InputIntent::PromotedQueuedToSteer
            )
    }
}

#[derive(Clone, Debug)]
pub struct PlanEntry {
    pub id: String,
    pub run_id: Option<String>,
    pub markdown: String,
    pub streaming: bool,
}

#[derive(Clone, Debug)]
pub enum EntryKind {
    Message(MessageEntry),
    Work(WorkEntry),
    Plan(PlanEntry),
    /// `fork`, `run_interrupt_*`, `subagent`: standalone event cards.
    Event(Rc<TurnItem>),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub created_at: Ts,
    pub kind: EntryKind,
}

impl Entry {
    pub fn work(&self) -> Option<&WorkEntry> {
        match &self.kind {
            EntryKind::Work(w) => Some(w),
            _ => None,
        }
    }

    pub fn message(&self) -> Option<&MessageEntry> {
        match &self.kind {
            EntryKind::Message(m) => Some(m),
            _ => None,
        }
    }

    pub fn event(&self) -> Option<&TurnItem> {
        match &self.kind {
            EntryKind::Event(i) => Some(i),
            _ => None,
        }
    }
}

fn tone_of(item: ItemType) -> Tone {
    match item {
        ItemType::Error => Tone::Info,
        ItemType::Reasoning => Tone::Thinking,
        ItemType::CommandExecution
        | ItemType::FileChange
        | ItemType::FileSearch
        | ItemType::WebSearch
        | ItemType::DynamicTool
        | ItemType::Subagent
        | ItemType::UserInputRequest
        | ItemType::ApprovalRequest => Tone::Tool,
        _ => Tone::Info,
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        one.replace("{count}", &count.to_string())
    } else {
        many.replace("{count}", &count.to_string())
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(path)
}

const SEARCH_QUERY_KEYS: [&str; 6] = ["pattern", "query", "searchTerm", "regex", "grep", "needle"];
const SEARCH_GLOB_KEYS: [&str; 6] = [
    "glob",
    "globPattern",
    "glob_pattern",
    "include",
    "filePattern",
    "file_pattern",
];
const SEARCH_TARGET_KEYS: [&str; 6] = [
    "path",
    "target_directory",
    "targetDirectory",
    "directory",
    "cwd",
    "root",
];

fn first_str(input: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        input
            .get(*k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

/// `formatSearchToolLabel` over a tool input object.
pub fn search_label(input: &serde_json::Value) -> Option<String> {
    let query = first_str(input, &SEARCH_QUERY_KEYS);
    let glob = first_str(input, &SEARCH_GLOB_KEYS);
    let target = first_str(input, &SEARCH_TARGET_KEYS).and_then(|t| {
        t.split(['/', '\\'])
            .rfind(|p| !p.is_empty() && *p != ".")
            .map(str::to_string)
    });
    match (query, glob, target) {
        (Some(q), _, Some(t)) => Some(format!("Searched {q} in {t}")),
        (None, Some(g), Some(t)) => Some(format!("Searched files {g} in {t}")),
        (None, Some(g), None) => Some(format!("Searched files {g}")),
        (Some(q), _, None) => Some(format!("Searched {q}")),
        (None, None, Some(t)) => Some(format!("Searched in {t}")),
        _ => None,
    }
}

/// `collectToolFilePaths` (path-like keys of a tool input).
pub fn tool_file_paths(input: &serde_json::Value) -> Vec<String> {
    ["file_path", "filePath", "path", "file", "filename", "notebook_path"]
        .iter()
        .filter_map(|k| input.get(*k).and_then(|v| v.as_str()))
        .map(str::to_string)
        .take(1)
        .chain(
            input
                .get("paths")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_string)),
        )
        .collect()
}

fn tool_token(name: &str) -> String {
    name.trim()
        .rsplit(['.', ':', '/'])
        .next()
        .unwrap_or(name)
        .chars()
        .filter(|c| !matches!(c, '_' | '-' | ' '))
        .flat_map(char::to_lowercase)
        .collect()
}

/// `classifyToolActivity` for a dynamic tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolClass {
    Command,
    FileChange,
    Search,
    Read,
    Other,
}

pub fn classify_dynamic(tool_name: Option<&str>, input: &serde_json::Value) -> ToolClass {
    let kind = input
        .get("kind")
        .and_then(|v| v.as_str())
        .map(str::to_lowercase);
    let tool = tool_name.map(tool_token).unwrap_or_default();
    match kind.as_deref() {
        Some("execute") => return ToolClass::Command,
        Some("edit" | "move" | "delete" | "write") => return ToolClass::FileChange,
        Some("search") => return ToolClass::Search,
        _ => {}
    }
    if matches!(tool.as_str(), "find" | "grep" | "glob" | "rg" | "ls") {
        return ToolClass::Search;
    }
    if kind.as_deref() == Some("read") {
        return ToolClass::Read;
    }
    match tool.as_str() {
        "terminal" | "bash" | "shell" => ToolClass::Command,
        "read" | "readfile" => ToolClass::Read,
        "edit" | "write" | "multiedit" | "applypatch" => ToolClass::FileChange,
        _ => ToolClass::Other,
    }
}

/// `formatReadToolLabel`.
pub fn read_label(path: &str, more: usize) -> String {
    if path.is_empty() {
        return "Read file".into();
    }
    if more == 0 {
        format!("Read {path}")
    } else {
        format!("Read {path} +{more} more")
    }
}

/// `providerErrorPresentation`.
fn error_presentation(item: &TurnItem) -> (String, String) {
    let TurnItemBody::Error { failure, retry } = &item.body else {
        unreachable!()
    };
    let usage = failure.class == tern_core::FailureClass::UsageLimit;
    let Some(retry) = retry else {
        let label = if usage {
            "Usage limit reached".to_string()
        } else {
            item.title
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .unwrap_or("Provider error")
                .to_string()
        };
        return (label, failure.message.clone());
    };
    let progress = match retry.max_attempts {
        Some(max) => format!("{}/{max}", retry.attempt),
        None => retry.attempt.to_string(),
    };
    let label = match item.status {
        ItemStatus::Running => format!("Retrying provider ({progress})"),
        ItemStatus::Completed => format!("Provider recovered ({progress} retries)"),
        ItemStatus::Failed if usage => format!("Usage limit reached after {progress} retries"),
        ItemStatus::Failed => format!("Provider error after {progress} retries"),
        _ => format!("Provider retry stopped ({progress})"),
    };
    let delay = match retry.retry_delay_ms {
        Some(ms) if item.status == ItemStatus::Running && ms > 0 => {
            if ms < 1_000 {
                format!(" Retrying in {ms}ms.")
            } else {
                let s = format!("{:.1}", ms as f64 / 1_000.);
                format!(" Retrying in {}s.", s.trim_end_matches(".0"))
            }
        }
        _ => String::new(),
    };
    (label, format!("{}{delay}", failure.message))
}

/// `contextCompactionLabel`.
pub fn compaction_label(item: &TurnItem) -> String {
    match item.status {
        ItemStatus::Pending | ItemStatus::Running | ItemStatus::Waiting => {
            "Compacting context".into()
        }
        ItemStatus::Failed => "Context compaction failed".into(),
        ItemStatus::Cancelled | ItemStatus::Interrupted => "Context compaction stopped".into(),
        _ => "Context compacted".into(),
    }
}

/// `projectedWorkEntry`.
fn work_entry(item: &Rc<TurnItem>) -> WorkEntry {
    let item_type = ItemType::of(&item.body);
    let title = item
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    let mut e = WorkEntry {
        id: item.id.clone(),
        created_at: item.started_at.unwrap_or(item.updated_at),
        run_id: item.run_id.clone(),
        label: String::new(),
        detail: None,
        command: None,
        changed_files: Vec::new(),
        tone: tone_of(item_type),
        item_type,
        status: Lifecycle::from_item(item.status),
        tool_title: None,
        request_kind: None,
        source_kind: None,
        question_answer: false,
        item: item.clone(),
    };
    match &item.body {
        TurnItemBody::Compaction { summary, .. } => {
            e.label = compaction_label(item);
            e.source_kind = Some(SourceKind::ContextCompaction);
            e.detail = summary.clone().filter(|s| !s.is_empty());
        }
        TurnItemBody::Reasoning { text, .. } => {
            e.label = title.unwrap_or_else(|| "Thinking".into());
            e.detail = (!text.is_empty()).then(|| text.clone());
        }
        TurnItemBody::CommandExecution { input, .. } => {
            e.label = title.clone().unwrap_or_else(|| "Ran command".into());
            e.command = Some(input.clone());
            e.tool_title = Some(title.unwrap_or_else(|| "Command".into()));
        }
        TurnItemBody::FileChange {
            file_name, changes, ..
        } => {
            let many = changes.as_ref().is_some_and(|c| c.len() > 1);
            e.label = title.clone().unwrap_or_else(|| {
                if many {
                    plural(
                        changes.as_ref().map_or(0, Vec::len),
                        "Changed {count} file",
                        "Changed {count} files",
                    )
                } else {
                    format!("Changed {}", self::file_name(file_name))
                }
            });
            e.changed_files = match changes {
                Some(c) if !c.is_empty() => c.iter().map(|c| c.path.clone()).collect(),
                _ => vec![file_name.clone()],
            };
            e.tool_title = Some(title.unwrap_or_else(|| "File change".into()));
        }
        TurnItemBody::FileSearch { pattern, .. } => {
            let input = serde_json::json!({ "pattern": pattern });
            e.label = title
                .clone()
                .or_else(|| search_label(&input))
                .unwrap_or_else(|| "Searched files".into());
            e.detail = pattern.clone().filter(|p| !p.is_empty());
            e.tool_title = Some(title.unwrap_or_else(|| "File search".into()));
        }
        TurnItemBody::WebSearch { patterns, .. } => {
            e.label = title.clone().unwrap_or_else(|| "Searched the web".into());
            e.detail = patterns
                .as_ref()
                .filter(|p| !p.is_empty())
                .map(|p| p.join(", "));
            e.tool_title = Some(title.unwrap_or_else(|| "Web search".into()));
        }
        TurnItemBody::Checkpoint { files, .. } => {
            e.label = title.unwrap_or_else(|| "Checkpoint captured".into());
            e.changed_files = files.iter().map(|f| f.path.clone()).collect();
        }
        TurnItemBody::SystemNotice { message, level } => {
            e.label = message.clone();
            if *level != Some(NoticeLevel::Info) {
                e.source_kind = Some(SourceKind::RuntimeWarning);
            }
        }
        TurnItemBody::Error { failure, retry } => {
            let (label, detail) = error_presentation(item);
            e.label = label;
            e.detail = Some(detail);
            e.source_kind = if failure.class == tern_core::FailureClass::UsageLimit
                && item.status != ItemStatus::Completed
            {
                Some(SourceKind::RuntimeWarning)
            } else if retry.is_none() {
                Some(SourceKind::RuntimeError)
            } else {
                None
            };
        }
        TurnItemBody::DynamicTool {
            tool_name, input, ..
        } => {
            let class = classify_dynamic(tool_name.as_deref(), input);
            let read_path = tool_file_paths(input).into_iter().next();
            e.label = title.clone().unwrap_or_else(|| match class {
                ToolClass::Read => read_label(read_path.as_deref().unwrap_or(""), 0),
                ToolClass::Search => search_label(input)
                    .or_else(|| tool_name.clone())
                    .unwrap_or_else(|| "Tool call".into()),
                _ => tool_name.clone().unwrap_or_else(|| "Tool call".into()),
            });
            e.tool_title = Some(
                title
                    .or_else(|| tool_name.clone())
                    .unwrap_or_else(|| "Tool".into()),
            );
        }
        TurnItemBody::TodoList { steps, .. } => {
            let done = steps
                .iter()
                .filter(|s| s.status == tern_core::PlanStepStatus::Completed)
                .count();
            let current = steps
                .iter()
                .find(|s| s.status == tern_core::PlanStepStatus::Running)
                .or_else(|| {
                    steps
                        .iter()
                        .find(|s| s.status == tern_core::PlanStepStatus::Pending)
                });
            e.label = match current {
                Some(c) => format!("Tasks {done}/{} · {}", steps.len(), c.text),
                None => format!("Tasks {done}/{}", steps.len()),
            };
        }
        TurnItemBody::ApprovalRequest {
            prompt,
            request_kind,
            ..
        } => {
            e.label = title.unwrap_or_else(|| "Approval requested".into());
            e.detail = Some(
                prompt
                    .clone()
                    .unwrap_or_else(|| request_kind_wire(*request_kind).into()),
            );
        }
        TurnItemBody::UserInputRequest {
            question_answer, ..
        } => {
            e.label = title.unwrap_or_else(|| {
                if question_answer.is_some() {
                    "Answered questions".into()
                } else {
                    "Input requested".into()
                }
            });
            e.question_answer = question_answer.is_some();
        }
        TurnItemBody::Subagent { prompt, .. } => {
            e.label = title.unwrap_or_else(|| prompt.clone());
        }
        _ => {
            e.label = title.unwrap_or_default();
        }
    }
    e
}

fn request_kind_wire(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::Command => "command",
        RequestKind::FileRead => "file-read",
        RequestKind::FileChange => "file-change",
        RequestKind::McpElicitation => "mcp-elicitation",
        RequestKind::Permission => "permission",
        RequestKind::UserInput => "user_input",
    }
}

/// `deriveTimelineEntriesFromVisibleTurnItems` for Tern's items (already ordinal-sorted).
pub fn derive_entries(items: &[Rc<TurnItem>]) -> Vec<Entry> {
    let mut entries = Vec::with_capacity(items.len());
    for item in items {
        let created_at = item.started_at.unwrap_or(item.updated_at);
        let kind = match &item.body {
            TurnItemBody::Checkpoint { .. } => continue,
            TurnItemBody::UserMessage {
                input_intent: InputIntent::QueuedTurn,
                ..
            } if item.status == ItemStatus::Pending => continue,
            TurnItemBody::UserMessage {
                message_id,
                text,
                attachments,
                input_intent,
                ..
            } => EntryKind::Message(MessageEntry {
                id: message_id.clone(),
                role: Role::User,
                text: text.clone(),
                run_id: item.run_id.clone(),
                streaming: false,
                created_at,
                updated_at: item.updated_at,
                input_intent: *input_intent,
                phase: None,
                attachments: attachments.clone(),
                item: item.clone(),
            }),
            TurnItemBody::AssistantMessage {
                message_id,
                text,
                streaming,
                phase,
            } => EntryKind::Message(MessageEntry {
                id: message_id.clone(),
                role: Role::Assistant,
                text: text.clone(),
                run_id: item.run_id.clone(),
                streaming: *streaming,
                created_at,
                updated_at: item.updated_at,
                input_intent: InputIntent::TurnStart,
                phase: *phase,
                attachments: Vec::new(),
                item: item.clone(),
            }),
            TurnItemBody::ProposedPlan {
                markdown,
                streaming,
                ..
            } => EntryKind::Plan(PlanEntry {
                id: item.id.clone(),
                run_id: item.run_id.clone(),
                markdown: markdown.clone(),
                streaming: *streaming,
            }),
            TurnItemBody::RunInterruptRequest { .. }
            | TurnItemBody::RunInterruptResult { .. }
            | TurnItemBody::Subagent { .. } => EntryKind::Event(item.clone()),
            _ => EntryKind::Work(work_entry(item)),
        };
        let id = match &kind {
            EntryKind::Message(m) => m.id.clone(),
            _ => item.id.clone(),
        };
        entries.push(Entry {
            id,
            created_at,
            kind,
        });
    }
    entries
}

// ---------------------------------------------------------------------------
// Presentation of a work entry
// ---------------------------------------------------------------------------

impl WorkEntry {
    /// `workLogEntryIsToolLike` (client-runtime variant, which also counts lifecycle types).
    pub fn is_tool_like(&self) -> bool {
        matches!(self.tone, Tone::Tool | Tone::Thinking | Tone::Error)
            || self.command.as_deref().is_some_and(|c| !c.trim().is_empty())
            || self.request_kind.is_some()
            || self.item_type.tool_lifecycle()
    }

    fn failure_from_output(&self, include_command: bool) -> bool {
        if self.tone == Tone::Error || matches!(self.status, Lifecycle::Failed | Lifecycle::Declined)
        {
            return true;
        }
        if !self.is_tool_like() || self.item_type == ItemType::Reasoning {
            return false;
        }
        if let TurnItemBody::CommandExecution {
            output,
            exit_code,
            output_indicates_failure,
            ..
        } = &self.item.body
        {
            if let Some(code) = exit_code {
                return *code != 0;
            }
            if *output_indicates_failure == Some(true) {
                return true;
            }
            if let Some(out) = output {
                let end = out.floor_char_boundary(32_768);
                if output_indicates_failure_text(&out[..end]) {
                    return true;
                }
            }
        }
        let mut text = self.detail.clone().unwrap_or_default();
        if include_command && let Some(c) = &self.command {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(c);
        }
        !text.is_empty() && output_indicates_failure_text(&text)
    }

    /// `workEntryIndicatesToolFailure`.
    pub fn indicates_failure(&self) -> bool {
        self.failure_from_output(true)
    }

    /// `workEntryDisplayIndicatesToolFailure`.
    pub fn display_failure(&self) -> bool {
        self.failure_from_output(false)
    }

    /// `workEntryIndicatesToolSuccess`.
    pub fn indicates_success(&self) -> bool {
        if !self.is_tool_like()
            || self.indicates_failure()
            || (self.tone == Tone::Thinking && self.item_type != ItemType::Reasoning)
        {
            return false;
        }
        !matches!(
            self.status,
            Lifecycle::Failed
                | Lifecycle::Declined
                | Lifecycle::InProgress
                | Lifecycle::Stopped
                | Lifecycle::Idle
        )
    }

    /// `workEntryIndicatesToolNeutralStatus`.
    pub fn is_neutral(&self) -> bool {
        self.is_tool_like() && !self.indicates_failure() && !self.indicates_success()
    }

    /// `workEntryIsVisibleInGroup`.
    pub fn visible_in_group(&self, expanded_tool_group_entry: bool) -> bool {
        if self.item_type == ItemType::Reasoning {
            return self.detail.as_deref().is_some_and(|d| !d.trim().is_empty());
        }
        (expanded_tool_group_entry && self.status == Lifecycle::InProgress)
            || self.status == Lifecycle::Stopped
            || !self.is_neutral()
    }

    /// `workEntryIsActiveTurnActivity`.
    pub fn is_active_turn_activity(&self) -> bool {
        self.status == Lifecycle::InProgress
    }

    /// `workEntryHiddenByToolCallVisibility`.
    pub fn hidden_by(&self, visibility: ToolCallVisibility) -> bool {
        if visibility == ToolCallVisibility::ShowAll || !self.item_type.hideable() {
            return false;
        }
        if visibility != ToolCallVisibility::HideSuccessful {
            return true;
        }
        self.status == Lifecycle::Completed && !self.display_failure()
    }

    fn dynamic(&self) -> Option<(Option<&str>, &serde_json::Value)> {
        match &self.item.body {
            TurnItemBody::DynamicTool {
                tool_name, input, ..
            } => Some((tool_name.as_deref(), input)),
            _ => None,
        }
    }

    /// `toolGroupAction`.
    pub fn action(&self) -> Action {
        if self.request_kind == Some(RequestKind::FileRead) {
            return Action::Read;
        }
        if matches!(
            self.item_type,
            ItemType::ApprovalRequest | ItemType::UserInputRequest
        ) {
            return if self.is_tool_like() {
                Action::Other
            } else {
                Action::Update
            };
        }
        let class = match self.item_type {
            ItemType::CommandExecution => ToolClass::Command,
            ItemType::FileChange => ToolClass::FileChange,
            ItemType::WebSearch => ToolClass::Search,
            _ => match self.dynamic() {
                Some((name, input)) => classify_dynamic(name, input),
                None => ToolClass::Other,
            },
        };
        let local_search = self.item_type == ItemType::FileSearch
            || (self.item_type == ItemType::WebSearch
                && normalize_compact_label(self.tool_title.as_deref().unwrap_or(&self.label))
                    .to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|w| w == "grep"));
        match class {
            ToolClass::Read => return Action::Read,
            ToolClass::FileChange => return Action::Edit,
            _ if self.item_type == ItemType::FileChange => return Action::Edit,
            ToolClass::Command => return Action::Command,
            _ if self.command.is_some() => return Action::Command,
            ToolClass::Search => {
                return if self.item_type == ItemType::WebSearch && !local_search {
                    Action::Search
                } else {
                    Action::CodeSearch
                };
            }
            _ => {}
        }
        if local_search {
            return Action::CodeSearch;
        }
        if self.item_type == ItemType::WebSearch {
            return Action::Search;
        }
        if !self.changed_files.is_empty() {
            return Action::Edit;
        }
        if self.is_tool_like() {
            Action::Other
        } else {
            Action::Update
        }
    }

    fn read_paths(&self) -> Vec<String> {
        if !self.changed_files.is_empty() {
            return self.changed_files.clone();
        }
        self.dynamic()
            .map(|(_, input)| tool_file_paths(input))
            .unwrap_or_default()
    }

    /// `workEntryDisplayLabel`: the single-line label of a work row.
    pub fn display_label(&self, workspace_root: Option<&str>) -> String {
        if self.item_type == ItemType::SystemNotice {
            return self.label.clone();
        }
        if self.item_type == ItemType::Reasoning || self.tone == Tone::Thinking {
            let thought = collapse_ws(self.detail.as_deref().unwrap_or(""));
            return if thought.is_empty() {
                self.label.clone()
            } else {
                thought
            };
        }
        if let Some(c) = self.command.as_deref().filter(|c| !c.trim().is_empty()) {
            return format::command_display(c);
        }
        if let Some(label) = self.commandless_label(live_status(self.status, false)) {
            return label;
        }
        let action = self.action();
        if matches!(action, Action::CodeSearch | Action::Search) {
            let search = if self.item_type == ItemType::FileSearch {
                Some(self.label.clone())
            } else {
                self.dynamic().and_then(|(_, input)| search_label(input))
            };
            if let Some(s) = search {
                return s;
            }
        }
        if action == Action::Read {
            let paths: Vec<String> = self
                .read_paths()
                .iter()
                .map(|p| relative_path(p, workspace_root))
                .collect();
            if let Some(first) = paths.first() {
                return read_label(first, paths.len() - 1);
            }
        }
        if let Some(title) = self.dynamic_title() {
            return title;
        }
        let retry = matches!(&self.item.body, TurnItemBody::Error { retry: Some(_), .. });
        let detail = self.detail.as_deref().map(str::trim).unwrap_or("");
        let detail_is_search_output = matches!(action, Action::CodeSearch | Action::Search)
            && detail.contains(['\r', '\n']);
        if !detail.is_empty() && !retry && action != Action::Read && !detail_is_search_output {
            return detail.to_string();
        }
        if let Some(first) = self.changed_files.first() {
            let path = relative_path(first, workspace_root);
            return if self.changed_files.len() == 1 {
                path
            } else {
                format!("{path} +{} more", self.changed_files.len() - 1)
            };
        }
        if action == Action::Read {
            return "Read file".into();
        }
        let heading = normalize_compact_label(self.tool_title.as_deref().unwrap_or(&self.label));
        let mut chars = heading.chars();
        match chars.next() {
            Some(c) => c.to_uppercase().chain(chars).collect(),
            None => String::new(),
        }
    }

    /// `dynamicToolTitle`: "Computer use", "Run JavaScript", "Skill: {name}".
    fn dynamic_title(&self) -> Option<String> {
        let (name, input) = self.dynamic()?;
        let token = name.map(tool_token).unwrap_or_default();
        match token.as_str() {
            "computeruse" | "computer" => Some("Computer use".into()),
            "javascript" | "jsrepl" | "repl" => Some("Run JavaScript".into()),
            "skill" => input
                .get("skill")
                .or_else(|| input.get("name"))
                .and_then(|v| v.as_str())
                .map(|s| format!("Skill: {s}")),
            _ => self.item.title.clone().filter(|t| !t.trim().is_empty()),
        }
    }

    /// `commandlessCommandLabel`.
    fn commandless_label(&self, status: Lifecycle) -> Option<String> {
        if self.item_type != ItemType::CommandExecution
            || self.command.as_deref().is_some_and(|c| !c.trim().is_empty())
        {
            return None;
        }
        let title = self.item.title.as_deref().map(str::trim).unwrap_or("");
        let generic = ["Ran command", "Run command", "Command"];
        (title.is_empty() || generic.contains(&normalize_compact_label(title).as_str()))
            .then(|| generic_command_label(status))
    }

    /// `singleToolCallLabel`: the label of a lone tool entry shown without a group.
    pub fn single_label(&self, workspace_root: Option<&str>) -> String {
        if self.item_type == ItemType::Reasoning {
            let t = collapse_ws(self.detail.as_deref().unwrap_or(""));
            return if t.is_empty() { "Thought".into() } else { t };
        }
        if let Some(t) = self.dynamic_title() {
            return t;
        }
        if self.item_type == ItemType::WebSearch {
            return self
                .tool_title
                .clone()
                .unwrap_or_else(|| "Web search".into());
        }
        self.display_label(workspace_root)
    }

    /// `liveWorkEntryLabel`.
    pub fn live_label(&self, workspace_root: Option<&str>, active: bool) -> String {
        let status = live_status(self.status, active);
        if self.item_type == ItemType::Reasoning {
            let t = collapse_ws(self.detail.as_deref().unwrap_or(""));
            if !t.is_empty() {
                return t;
            }
            return if status == Lifecycle::InProgress {
                "Thinking".into()
            } else {
                "Thought".into()
            };
        }
        if let Some(command) = self.command.as_deref().filter(|c| !c.trim().is_empty()) {
            return match format::command_program(command) {
                None => generic_command_label(status),
                Some(p) => match status {
                    Lifecycle::InProgress => format!("Running {p}"),
                    Lifecycle::Failed => format!("Failed {p}"),
                    Lifecycle::Declined => format!("Declined {p}"),
                    Lifecycle::Stopped => format!("Stopped {p}"),
                    _ => format!("Ran {p}"),
                },
            };
        }
        self.commandless_label(status)
            .unwrap_or_else(|| self.display_label(workspace_root))
    }

    /// `workEntryIconName`.
    pub fn icon(&self) -> IconName {
        if self.item_type == ItemType::SystemNotice {
            return match &self.item.body {
                TurnItemBody::SystemNotice {
                    level: Some(NoticeLevel::Info),
                    ..
                } => IconName::Zap,
                _ => IconName::CircleAlert,
            };
        }
        if matches!(
            self.item_type,
            ItemType::UserInputRequest | ItemType::ApprovalRequest
        ) {
            return IconName::MessageCircle;
        }
        if self.item_type == ItemType::TodoList {
            return IconName::ListTodo;
        }
        if self.dynamic_title().as_deref() == Some("Run JavaScript") {
            return IconName::Code;
        }
        let action = self.action();
        if action != Action::Other {
            return SummaryKind::Action(action).icon();
        }
        match self.item_type {
            ItemType::DynamicTool => return IconName::Wrench,
            ItemType::Subagent => return IconName::Bot,
            _ => {}
        }
        match self.tone {
            Tone::Error => IconName::CircleAlert,
            Tone::Thinking => IconName::Brain,
            Tone::Info => IconName::Check,
            Tone::Tool => IconName::Zap,
        }
    }
}

/// `liveActivityToolStatus`.
pub fn live_status(status: Lifecycle, present_tense: bool) -> Lifecycle {
    match status {
        Lifecycle::Failed | Lifecycle::Declined | Lifecycle::Stopped | Lifecycle::Idle => status,
        _ if present_tense || status == Lifecycle::InProgress => Lifecycle::InProgress,
        _ => Lifecycle::Completed,
    }
}

/// `genericCommandStatusLabel`.
pub fn generic_command_label(status: Lifecycle) -> String {
    match status {
        Lifecycle::InProgress => "Running command",
        Lifecycle::Failed => "Failed command",
        Lifecycle::Declined => "Declined command",
        Lifecycle::Stopped => "Stopped command",
        _ => "Ran command",
    }
    .into()
}

/// `normalizeCompactToolLabel`: drops a trailing "complete"/"completed".
pub fn normalize_compact_label(value: &str) -> String {
    let t = value.trim();
    let lower = t.to_lowercase();
    for suffix in [" completed", " complete"] {
        if lower.ends_with(suffix) {
            return t[..t.len() - suffix.len()].trim().to_string();
        }
    }
    t.to_string()
}

pub fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `formatWorkspaceRelativePath`.
pub fn relative_path(path: &str, workspace_root: Option<&str>) -> String {
    let p = path.trim();
    if let Some(root) = workspace_root {
        let root = root.trim_end_matches('/');
        if let Some(rest) = p.strip_prefix(root)
            && let Some(rest) = rest.strip_prefix('/')
        {
            return rest.to_string();
        }
    }
    p.trim_start_matches("./").to_string()
}

/// `toolOutputIndicatesFailure`.
pub fn output_indicates_failure_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    let hint = [
        "enoent",
        "no such",
        "no files found",
        "not found",
        "commandnotfound",
        "not recognized",
        "cannot find path",
        "cannot be found",
        "exit",
    ]
    .iter()
    .any(|h| lower.contains(h));
    if !hint {
        return false;
    }
    if [
        "file not found",
        "no files found",
        "enoent",
        "no such file",
        "commandnotfoundexception",
        "command not found",
        "is not recognized as the name of a cmdlet",
        "a parameter cannot be found that matches parameter name",
    ]
    .iter()
    .any(|n| lower.contains(n))
    {
        return true;
    }
    if lower.contains("cannot find path") && lower.contains("because it does not exist") {
        return true;
    }
    if lower.contains("is not recognized") && lower.contains("the term '") {
        return true;
    }
    exit_code_nonzero(&lower)
}

/// `exit(ed)? with exit code N` / `exit code: N` with N ≥ 1.
fn exit_code_nonzero(lower: &str) -> bool {
    let mut rest = lower;
    while let Some(i) = rest.find("exit code") {
        let after = rest[i + "exit code".len()..].trim_start_matches([' ', ':', '\t']);
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() && !digits.starts_with('0') {
            return true;
        }
        rest = &rest[i + 1..];
    }
    false
}

// ---------------------------------------------------------------------------
// Group actions and summaries
// ---------------------------------------------------------------------------

/// `ToolGroupAction` (Tern subset).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Read,
    Edit,
    Command,
    CodeSearch,
    Search,
    Other,
    Update,
}

/// `ToolGroupSummaryKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SummaryKind {
    Action(Action),
    DynamicTool,
    Reasoning,
    AgentTool,
    ToneTool,
    Mixed,
}

impl SummaryKind {
    /// `toolGroupSummaryIconName`.
    pub fn icon(self) -> IconName {
        match self {
            Self::Action(Action::Read) => IconName::Eye,
            Self::Action(Action::Edit) => IconName::SquarePen,
            Self::Action(Action::Command) => IconName::Terminal,
            Self::Action(Action::Search) => IconName::Globe,
            Self::Action(Action::CodeSearch) => IconName::Search,
            Self::Action(Action::Other) => IconName::Wrench,
            Self::DynamicTool => IconName::Hammer,
            Self::Reasoning => IconName::Brain,
            Self::AgentTool => IconName::Bot,
            Self::ToneTool => IconName::Zap,
            Self::Action(Action::Update) | Self::Mixed => IconName::Hammer,
        }
    }
}

/// `ToolCallVisibility` (settings).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolCallVisibility {
    #[default]
    ShowAll,
    HideSuccessful,
    HideAll,
    Zen,
}

fn action_label(action: Action, count: usize) -> String {
    let (one, many) = match action {
        Action::Read => ("Read {count} file", "Read {count} files"),
        Action::Edit => ("Changed {count} file", "Changed {count} files"),
        Action::Command => ("Ran {count} command", "Ran {count} commands"),
        Action::CodeSearch => ("Searched code {count} time", "Searched code {count} times"),
        Action::Search => (
            "Searched the web {count} time",
            "Searched the web {count} times",
        ),
        Action::Other => ("Used {count} tool", "Used {count} tools"),
        Action::Update => ("Received {count} update", "Received {count} updates"),
    };
    plural(count, one, many)
}

fn action_count(action: Action, entries: &[&WorkEntry]) -> usize {
    if action != Action::Edit {
        return entries.len();
    }
    let mut files = std::collections::HashSet::new();
    let mut without = 0;
    for e in entries {
        if e.changed_files.is_empty() {
            without += 1;
        } else {
            files.extend(e.changed_files.iter().cloned());
        }
    }
    files.len() + without
}

fn priority(action: Action) -> u8 {
    match action {
        Action::Command | Action::Edit => 0,
        Action::Other | Action::Update => 2,
        _ => 1,
    }
}

fn join_clauses(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// `summarizeToolGroup`: "Ran 2 commands and changed 1 file".
pub fn summarize_group(entries: &[WorkEntry]) -> String {
    let tools: Vec<&WorkEntry> = entries
        .iter()
        .filter(|e| e.item_type != ItemType::Reasoning)
        .collect();
    if !entries.is_empty() && tools.is_empty() {
        return if entries.len() == 1 {
            "Thought".into()
        } else {
            format!("Thought (×{})", entries.len())
        };
    }
    let mut groups: Vec<(Action, Vec<&WorkEntry>)> = Vec::new();
    for e in &tools {
        let a = e.action();
        match groups.iter_mut().find(|(g, _)| *g == a) {
            Some((_, list)) => list.push(e),
            None => groups.push((a, vec![e])),
        }
    }
    let mut ranked: Vec<(usize, &(Action, Vec<&WorkEntry>))> = groups.iter().enumerate().collect();
    ranked.sort_by_key(|(i, (a, _))| (priority(*a), *i));
    ranked.truncate(2);
    ranked.sort_by_key(|(i, _)| *i);
    let mut labels: Vec<String> = ranked
        .iter()
        .map(|(_, (a, list))| action_label(*a, action_count(*a, list)))
        .collect();
    let used: usize = ranked.iter().map(|(_, (_, l))| l.len()).sum();
    let remaining = tools.len() - used;
    if remaining > 0 {
        labels.push(plural(
            remaining,
            "Performed {count} other action",
            "Performed {count} other actions",
        ));
    }
    let labels: Vec<String> = labels
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            if i == 0 {
                l
            } else {
                let mut c = l.chars();
                c.next()
                    .map(|f| f.to_lowercase().chain(c).collect())
                    .unwrap_or_default()
            }
        })
        .collect();
    join_clauses(&labels)
}

/// `toolGroupSummaryKind`.
pub fn summary_kind(entries: &[WorkEntry]) -> SummaryKind {
    let tools: Vec<&WorkEntry> = entries
        .iter()
        .filter(|e| e.item_type != ItemType::Reasoning)
        .collect();
    if !entries.is_empty() && tools.is_empty() {
        return SummaryKind::Reasoning;
    }
    let mut actions: Vec<Action> = tools.iter().map(|e| e.action()).collect();
    actions.dedup();
    let first = actions.first().copied();
    if tools.is_empty() || actions.iter().any(|a| Some(*a) != first) {
        return SummaryKind::Mixed;
    }
    let action = first.unwrap();
    if action != Action::Other {
        return SummaryKind::Action(action);
    }
    let kinds: Vec<SummaryKind> = tools
        .iter()
        .map(|e| match (e.item_type, e.tone) {
            (ItemType::DynamicTool, _) => SummaryKind::DynamicTool,
            (ItemType::Subagent, _) | (_, Tone::Thinking) => SummaryKind::AgentTool,
            (_, Tone::Tool) => SummaryKind::ToneTool,
            _ => SummaryKind::Action(Action::Other),
        })
        .collect();
    if kinds.iter().all(|k| *k == kinds[0]) {
        kinds[0]
    } else {
        SummaryKind::Mixed
    }
}

/// `buildToolCallExpandedBody`: the plain `<pre>` body for entries without a
/// structured panel.
pub fn expanded_body(entry: &WorkEntry, workspace_root: Option<&str>, visible: &str) -> Option<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    seen.insert(visible.trim().to_string());
    let mut add = |text: Option<&str>, blocks: &mut Vec<String>| {
        let Some(t) = text.map(str::trim).filter(|t| !t.is_empty()) else {
            return;
        };
        if seen.insert(t.to_string()) {
            blocks.push(t.to_string());
        }
    };
    if let Some((_, input)) = entry.dynamic() {
        let json = serde_json::to_string_pretty(input).unwrap_or_default();
        add(Some(&format!("Tool input\n{json}")), &mut blocks);
    }
    if let Some(c) = entry.command.as_deref() {
        add(Some(c), &mut blocks);
    }
    add(entry.detail.as_deref(), &mut blocks);
    let files: Vec<String> = entry
        .changed_files
        .iter()
        .map(|f| relative_path(f, workspace_root))
        .filter(|f| Some(f.as_str()) != entry.detail.as_deref().map(str::trim))
        .collect();
    if !files.is_empty() {
        add(Some(&files.join("\n")), &mut blocks);
    }
    (!blocks.is_empty()).then(|| blocks.join("\n\n"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};
    use tern_core::{FailureClass, ProviderFailure};

    pub fn at(secs: i64) -> Ts {
        Utc.with_ymd_and_hms(2026, 11, 3, 12, 0, 0).unwrap() + Duration::seconds(secs)
    }

    pub fn item(id: &str, run: Option<&str>, status: ItemStatus, secs: i64, body: TurnItemBody) -> Rc<TurnItem> {
        Rc::new(TurnItem {
            id: id.into(),
            thread_id: "t".into(),
            run_id: run.map(Into::into),
            native_item_id: None,
            parent_item_id: None,
            ordinal: 0,
            status,
            title: None,
            started_at: Some(at(secs)),
            completed_at: None,
            updated_at: at(secs),
            body,
        })
    }

    pub fn cmd(id: &str, run: &str, secs: i64, input: &str, exit: Option<i32>) -> Rc<TurnItem> {
        item(
            id,
            Some(run),
            if exit.is_some() {
                ItemStatus::Completed
            } else {
                ItemStatus::Running
            },
            secs,
            TurnItemBody::CommandExecution {
                input: input.into(),
                output: Some("ok".into()),
                exit_code: exit,
                output_indicates_failure: None,
            },
        )
    }

    pub fn edit(id: &str, run: &str, secs: i64, path: &str) -> Rc<TurnItem> {
        item(
            id,
            Some(run),
            ItemStatus::Completed,
            secs,
            TurnItemBody::FileChange {
                file_name: path.into(),
                additions: Some(1),
                deletions: Some(0),
                diff_str: None,
                old_str: None,
                new_str: None,
                changes: None,
            },
        )
    }

    fn works(items: &[Rc<TurnItem>]) -> Vec<WorkEntry> {
        derive_entries(items)
            .into_iter()
            .filter_map(|e| e.work().cloned())
            .collect()
    }

    #[test]
    fn summary_sentence_orders_by_first_appearance() {
        let entries = works(&[
            cmd("a", "r", 0, "git ls-files", Some(0)),
            edit("b", "r", 1, "notes.py"),
            cmd("c", "r", 2, "head -5 notes.py", Some(0)),
        ]);
        assert_eq!(summarize_group(&entries), "Ran 2 commands and changed 1 file");
        assert_eq!(summary_kind(&entries), SummaryKind::Mixed);
        assert_eq!(summary_kind(&entries).icon(), IconName::Hammer);
    }

    #[test]
    fn summary_keeps_two_clauses_and_counts_the_rest() {
        let search = item(
            "s",
            Some("r"),
            ItemStatus::Completed,
            3,
            TurnItemBody::FileSearch {
                pattern: Some("foo".into()),
                results: None,
            },
        );
        let web = item(
            "w",
            Some("r"),
            ItemStatus::Completed,
            4,
            TurnItemBody::WebSearch {
                patterns: Some(vec!["x".into()]),
                results: None,
            },
        );
        let entries = works(&[
            search,
            cmd("a", "r", 0, "ls", Some(0)),
            web,
            edit("b", "r", 1, "a.rs"),
            edit("c", "r", 2, "a.rs"),
        ]);
        assert_eq!(
            summarize_group(&entries),
            "Ran 1 command, changed 1 file, and performed 2 other actions"
        );
    }

    #[test]
    fn reasoning_only_groups_read_as_thought() {
        let r = |id: &str| {
            item(
                id,
                Some("r"),
                ItemStatus::Completed,
                0,
                TurnItemBody::Reasoning {
                    text: "hm".into(),
                    streaming: false,
                },
            )
        };
        assert_eq!(summarize_group(&works(&[r("a")])), "Thought");
        assert_eq!(summarize_group(&works(&[r("a"), r("b")])), "Thought (×2)");
    }

    #[test]
    fn command_failure_uses_exit_code_first() {
        let ok = &works(&[cmd("a", "r", 0, "grep -r 'not found' .", Some(0))])[0];
        assert!(!ok.indicates_failure());
        assert!(ok.indicates_success());
        let bad = &works(&[cmd("a", "r", 0, "cargo test", Some(101))])[0];
        assert!(bad.display_failure());
        assert!(output_indicates_failure_text("bash: foo: command not found"));
        assert!(output_indicates_failure_text("Process exited with exit code 2"));
        assert!(!output_indicates_failure_text("exit code 0"));
        assert!(!output_indicates_failure_text("all fine"));
    }

    #[test]
    fn labels() {
        let e = &works(&[cmd("a", "r", 0, "bash -lc 'git status'", None)])[0];
        assert_eq!(e.display_label(None), "git status");
        assert_eq!(e.live_label(None, true), "Running git");
        assert_eq!(e.icon(), IconName::Terminal);
        let f = &works(&[edit("b", "r", 1, "/repo/src/app.rs")])[0];
        assert_eq!(f.label, "Changed app.rs");
        assert_eq!(f.display_label(Some("/repo")), "src/app.rs");
        assert_eq!(f.icon(), IconName::SquarePen);
    }

    #[test]
    fn usage_limit_error_labels() {
        let e = item(
            "e",
            Some("r"),
            ItemStatus::Failed,
            0,
            TurnItemBody::Error {
                failure: ProviderFailure {
                    class: FailureClass::UsageLimit,
                    message: "You hit the limit.".into(),
                    code: None,
                    retryable: None,
                    reset_at: None,
                },
                retry: None,
            },
        );
        let w = &works(&[e])[0];
        assert_eq!(w.label, "Usage limit reached");
        assert_eq!(w.source_kind, Some(SourceKind::RuntimeWarning));
    }

    #[test]
    fn search_labels() {
        let input = serde_json::json!({"pattern": "TODO", "path": "/repo/src/"});
        assert_eq!(search_label(&input).as_deref(), Some("Searched TODO in src"));
        let input = serde_json::json!({"glob": "*.rs"});
        assert_eq!(search_label(&input).as_deref(), Some("Searched files *.rs"));
    }
}

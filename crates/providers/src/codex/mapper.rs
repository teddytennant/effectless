//! Codex app-server notifications and server requests -> `ProviderEvent`s (spec/07 §3.4-3.8).
//! Pure state machine: no IO. The session feeds it frames and writes what it returns.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use tern_core::*;

use crate::RequestResponse;
use crate::items::{ItemTracker, emit_items, runtime_request, with_request_id};
use crate::util::{Emitter, answer_strings, diff_counts, failure, str_of, u64_of};

/// How the session should answer a server request.
#[derive(Debug, PartialEq)]
pub(crate) enum ServerReply {
    Result(Value),
    Error(i64, String),
    /// Waiting on the user; `respond` produces the answer later.
    Deferred,
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Breakdown {
    input: u64,
    cached: u64,
    cache_write: Option<u64>,
    output: u64,
    reasoning: u64,
}

impl Breakdown {
    fn from(v: &Value) -> Self {
        Self {
            input: u64_of(v, "inputTokens").unwrap_or(0),
            cached: u64_of(v, "cachedInputTokens").unwrap_or(0),
            cache_write: u64_of(v, "cacheWriteInputTokens"),
            output: u64_of(v, "outputTokens").unwrap_or(0),
            reasoning: u64_of(v, "reasoningOutputTokens").unwrap_or(0),
        }
    }
}

/// prov/CodexTurnTokenUsage.ts: per-turn totals from cumulative thread totals.
#[derive(Default)]
struct UsageState {
    baseline: Option<Breakdown>,
    active_turn: Option<String>,
    acc: Breakdown,
    cache_write_tracked: bool,
    observed: bool,
}

impl UsageState {
    fn begin(&mut self, turn_id: &str) {
        self.active_turn = Some(turn_id.to_string());
        self.acc = Breakdown {
            cache_write: Some(0),
            ..Default::default()
        };
        self.cache_write_tracked = true;
        self.observed = false;
    }

    fn record(&mut self, turn_id: &str, total: Breakdown, last: Breakdown) {
        if self.active_turn.as_deref() != Some(turn_id) {
            self.baseline = Some(total);
            return;
        }
        let delta = match self.baseline {
            Some(b)
                if total.input >= b.input
                    && total.cached >= b.cached
                    && total.output >= b.output
                    && total.reasoning >= b.reasoning =>
            {
                Breakdown {
                    input: total.input - b.input,
                    cached: total.cached - b.cached,
                    cache_write: match (total.cache_write, b.cache_write) {
                        (Some(t), Some(b)) => t.checked_sub(b),
                        _ => None,
                    },
                    output: total.output - b.output,
                    reasoning: total.reasoning - b.reasoning,
                }
            }
            _ => last,
        };
        self.acc.input += delta.input;
        self.acc.cached += delta.cached;
        self.acc.output += delta.output;
        self.acc.reasoning += delta.reasoning;
        match (delta.cache_write, self.acc.cache_write) {
            (Some(d), Some(a)) if self.cache_write_tracked => self.acc.cache_write = Some(a + d),
            _ => {
                self.cache_write_tracked = false;
                self.acc.cache_write = None;
            }
        }
        if delta.input > 0 || delta.cached > 0 || delta.output > 0 || delta.reasoning > 0 {
            self.observed = true;
        }
        self.baseline = Some(total);
    }

    fn finish(
        &mut self,
        completed: bool,
        has_subagents: bool,
        model: Option<String>,
    ) -> TurnTokenUsage {
        self.active_turn = None;
        let models = model.map(|m| vec![m]);
        if !self.observed {
            return TurnTokenUsage::Unavailable(TokenCounts {
                has_subagents,
                models,
                ..Default::default()
            });
        }
        let a = self.acc;
        let counts = TokenCounts {
            input_tokens: Some(a.input),
            output_tokens: Some(a.output),
            cached_input_tokens: Some(a.cached.min(a.input)),
            cache_creation_tokens: a.cache_write.map(|c| c.min(a.input)),
            reasoning_tokens: Some(a.reasoning.min(a.output)),
            has_subagents,
            models,
        };
        if completed {
            TurnTokenUsage::Complete(counts)
        } else {
            TurnTokenUsage::Partial(counts)
        }
    }
}

struct ActiveTurn {
    run_id: String,
    native_turn_id: Option<String>,
    /// turn/start answered or turn/started arrived: the engine has been told.
    accepted: bool,
    native_started: bool,
    interrupting: bool,
    items: ItemTracker,
    last_failure: Option<ProviderFailure>,
    retry_key: Option<String>,
    model: Option<String>,
    has_subagents: bool,
    plan_ids: HashMap<String, String>,
}

struct PendingRequest {
    native_id: Value,
    method: String,
    kind: RequestKind,
    item_key: String,
    params: Value,
    run_id: String,
}

pub(crate) struct CodexMapper {
    thread_id: String,
    live_session_id: String,
    emitter: Emitter,
    pub native_thread_id: Option<String>,
    pub runtime_mode: RuntimeMode,
    model: Option<String>,
    active: Option<ActiveTurn>,
    usage: UsageState,
    allowances: HashSet<RequestKind>,
    /// Tern request id -> what to answer.
    pending: HashMap<String, PendingRequest>,
    context_usage: Option<ContextUsage>,
}

pub(crate) fn item_status(s: Option<&str>) -> ItemStatus {
    match s {
        Some("completed") => ItemStatus::Completed,
        Some("failed") => ItemStatus::Failed,
        Some("declined") => ItemStatus::Cancelled,
        Some("interrupted") => ItemStatus::Interrupted,
        _ => ItemStatus::Running,
    }
}

fn error_info_code(info: &Value) -> Option<String> {
    match info {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => o.keys().next().cloned(),
        _ => None,
    }
}

fn is_usage_limit(code: Option<&str>, info: &Value) -> bool {
    matches!(code, Some("usageLimitExceeded" | "rateLimitExceeded"))
        || info
            .get("responseTooManyFailedAttempts")
            .and_then(|d| d.get("httpStatusCode"))
            .and_then(Value::as_u64)
            == Some(429)
}

/// `codexTerminalFailureClass` (CA:1377-1393).
fn terminal_failure(error: &Value) -> ProviderFailure {
    let info = error.get("codexErrorInfo").cloned().unwrap_or(Value::Null);
    let code = error_info_code(&info);
    let class = if is_usage_limit(code.as_deref(), &info) {
        FailureClass::UsageLimit
    } else {
        let transport = [
            "responseStreamDisconnected",
            "responseStreamConnectionFailed",
            "httpConnectionFailed",
        ]
        .iter()
        .find_map(|k| info.get(*k));
        match transport {
            Some(t) if t.get("httpStatusCode").is_none_or(Value::is_null) => {
                FailureClass::TransportError
            }
            _ => FailureClass::ProviderError,
        }
    };
    let message = str_of(error, "message")
        .unwrap_or("Codex turn failed.")
        .to_string();
    ProviderFailure {
        class,
        message,
        code,
        retryable: None,
        reset_at: None,
    }
}

/// `parseCodexRetryProgress`: "Reconnecting... 2/5".
fn retry_progress(message: &str) -> Option<(u32, u32)> {
    let bytes: Vec<char> = message.chars().collect();
    for (i, c) in bytes.iter().enumerate() {
        if *c != '/' {
            continue;
        }
        let left: String = bytes[..i]
            .iter()
            .rev()
            .skip_while(|c| **c == ' ')
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let right: String = bytes[i + 1..]
            .iter()
            .skip_while(|c| **c == ' ')
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let left: String = left.chars().rev().collect();
        if let (Ok(a), Ok(b)) = (left.parse::<u32>(), right.parse::<u32>())
            && a >= 1
            && b >= 1
        {
            return Some((a, b));
        }
    }
    None
}

fn phase(v: Option<&str>) -> Option<AssistantPhase> {
    match v {
        Some("commentary") => Some(AssistantPhase::Commentary),
        Some("partial_answer") => Some(AssistantPhase::PartialAnswer),
        Some("final_answer") => Some(AssistantPhase::FinalAnswer),
        _ => None,
    }
}

fn file_change_body(item: &Value) -> TurnItemBody {
    let changes = item
        .get("changes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut details = Vec::new();
    let mut diff = String::new();
    let (mut add, mut del) = (0u32, 0u32);
    for c in &changes {
        let path = str_of(c, "path").unwrap_or_default().to_string();
        let kind = c
            .get("kind")
            .and_then(|k| str_of(k, "type"))
            .unwrap_or("update");
        let move_path = c
            .get("kind")
            .and_then(|k| str_of(k, "move_path"))
            .map(str::to_string);
        let d = str_of(c, "diff").unwrap_or_default();
        match kind {
            // add/delete carry the file content, not a diff.
            "add" => add += d.lines().count() as u32,
            "delete" => del += d.lines().count() as u32,
            _ => {
                let (a, r) = diff_counts(d);
                add += a;
                del += r;
            }
        }
        if !diff.is_empty() && !diff.ends_with('\n') {
            diff.push('\n');
        }
        diff.push_str(d);
        details.push(FileChangeDetail {
            operation: if move_path.is_some() {
                "move".into()
            } else {
                kind.to_string()
            },
            path: move_path.clone().unwrap_or_else(|| path.clone()),
            old_path: move_path.map(|_| path),
            file_type: None,
            mime_type: None,
        });
    }
    let file_name = match details.as_slice() {
        [one] => one.path.clone(),
        [] => String::new(),
        many => format!("{} files", many.len()),
    };
    TurnItemBody::FileChange {
        file_name,
        additions: Some(add),
        deletions: Some(del),
        diff_str: (!diff.is_empty()).then_some(diff),
        old_str: None,
        new_str: None,
        changes: Some(details),
    }
}

fn web_search_patterns(item: &Value) -> Option<Vec<String>> {
    let action = item.get("action");
    let mut out: Vec<String> = Vec::new();
    if let Some(a) = action {
        if let Some(qs) = a.get("queries").and_then(Value::as_array) {
            out.extend(qs.iter().filter_map(Value::as_str).map(str::to_string));
        }
        for k in ["query", "url", "pattern"] {
            if let Some(s) = str_of(a, k) {
                out.push(s.to_string());
            }
        }
    }
    if out.is_empty()
        && let Some(q) = str_of(item, "query").filter(|q| !q.is_empty())
    {
        out.push(q.to_string());
    }
    out.dedup();
    (!out.is_empty()).then_some(out)
}

impl CodexMapper {
    pub fn new(
        thread_id: &str,
        live_session_id: &str,
        emitter: Emitter,
        runtime_mode: RuntimeMode,
    ) -> Self {
        Self {
            thread_id: thread_id.to_string(),
            live_session_id: live_session_id.to_string(),
            emitter,
            native_thread_id: None,
            runtime_mode,
            model: None,
            active: None,
            usage: UsageState::default(),
            allowances: HashSet::new(),
            pending: HashMap::new(),
            context_usage: None,
        }
    }

    pub fn emitter(&self) -> &Emitter {
        &self.emitter
    }

    pub fn set_model(&mut self, model: Option<String>) {
        self.model = model;
    }

    pub fn active_run(&self) -> Option<&str> {
        self.active.as_ref().map(|t| t.run_id.as_str())
    }

    /// (native turn id, started) of the active turn if it belongs to `run_id`.
    pub fn turn_of(&self, run_id: &str) -> Option<(Option<String>, bool)> {
        self.active
            .as_ref()
            .filter(|t| t.run_id == run_id)
            .map(|t| (t.native_turn_id.clone(), t.native_started))
    }

    /// A runtime mode change resets session allowances (CA:2412-2421).
    pub fn set_runtime_mode(&mut self, mode: RuntimeMode) {
        if mode != self.runtime_mode {
            self.allowances.clear();
        }
        self.runtime_mode = mode;
    }

    pub fn begin_turn(&mut self, run_id: &str, model: Option<String>) {
        self.active = Some(ActiveTurn {
            run_id: run_id.to_string(),
            native_turn_id: None,
            accepted: false,
            native_started: false,
            interrupting: false,
            items: ItemTracker::new(&self.thread_id, Some(run_id.to_string())),
            last_failure: None,
            retry_key: None,
            model,
            has_subagents: false,
            plan_ids: HashMap::new(),
        });
    }

    /// turn/start was rejected: forget the turn without a terminal.
    pub fn abort_turn(&mut self, run_id: &str) {
        if self
            .active
            .as_ref()
            .is_some_and(|t| t.run_id == run_id && !t.accepted)
        {
            self.active = None;
        }
    }

    pub fn mark_interrupting(&mut self, run_id: &str) {
        if let Some(t) = self.active.as_mut().filter(|t| t.run_id == run_id) {
            t.interrupting = true;
        }
    }

    fn adopt_turn(&mut self, turn: &Value) -> bool {
        let Some(id) = str_of(turn, "id") else {
            return false;
        };
        let started = turn.get("startedAt").is_some_and(|s| !s.is_null());
        let Some(t) = self.active.as_mut() else {
            return false;
        };
        match &t.native_turn_id {
            Some(existing) if existing != id => return false,
            _ => {}
        }
        t.native_turn_id = Some(id.to_string());
        t.native_started |= started;
        let first = !t.accepted;
        t.accepted = true;
        let run_id = t.run_id.clone();
        if first {
            self.usage.begin(id);
            self.emitter.send(ProviderEvent::TurnStarted {
                run_id,
                native_turn_id: Some(id.to_string()),
            });
            self.emitter.send(ProviderEvent::BindingUpdated {
                native_thread_id: self.native_thread_id.clone(),
                status: ProviderThreadStatus::Active,
                context_usage: self.context_usage.clone(),
            });
            self.emitter.send(ProviderEvent::SessionUpdated {
                status: SessionStatus::Running,
                model: self.model.clone(),
                startup_phase: None,
                last_error: None,
            });
        }
        true
    }

    /// The `turn/start` result (`{turn}`); `turn/started` may already have registered it.
    pub fn on_turn_start_response(&mut self, result: &Value) {
        if let Some(turn) = result.get("turn") {
            self.adopt_turn(turn);
        }
    }

    /// Whether a notification belongs to the active turn. Item frames can beat the
    /// `turn/start` response to us; one turn runs per thread, so an unknown turn id while
    /// ours has none yet is ours.
    fn turn_matches(&mut self, params: &Value) -> bool {
        match (self.active.as_ref(), str_of(params, "turnId")) {
            (Some(t), Some(id)) if t.native_turn_id.is_none() => {
                let id = id.to_string();
                self.adopt_turn(&json!({ "id": id }))
            }
            (Some(t), Some(id)) => t.native_turn_id.as_deref() == Some(id),
            (Some(_), None) => true,
            _ => false,
        }
    }

    pub fn flush(&mut self, include_slow: bool) {
        let Some(t) = self.active.as_mut() else {
            return;
        };
        let items = t.items.take_dirty(include_slow);
        for item in &items {
            if let TurnItemBody::ProposedPlan {
                plan_id,
                markdown,
                streaming,
            } = &item.body
            {
                self.emitter.send(ProviderEvent::PlanUpdated {
                    plan_id: plan_id.clone(),
                    markdown: markdown.clone(),
                    streaming: *streaming,
                });
            }
        }
        emit_items(&self.emitter, items);
    }

    /// Any non-command progress clears the "retrying" banner (`completeProviderRetry`).
    fn clear_retry(&mut self) {
        let Some(t) = self.active.as_mut() else {
            return;
        };
        if let Some(key) = t.retry_key.take()
            && let Some(item) = t.items.update(&key, |i| i.status = ItemStatus::Completed)
        {
            self.emitter.send(ProviderEvent::ItemUpserted(item));
        }
    }

    pub fn on_notification(&mut self, method: &str, params: &Value) {
        match method {
            "turn/started" => {
                if let Some(turn) = params.get("turn")
                    && self.active.as_ref().is_some_and(|t| {
                        t.native_turn_id.is_none()
                            || t.native_turn_id.as_deref() == str_of(turn, "id")
                    })
                {
                    self.adopt_turn(turn);
                    if let Some(t) = self.active.as_mut() {
                        t.native_started = true;
                    }
                }
            }
            "turn/completed" => {
                if let Some(turn) = params.get("turn")
                    && self
                        .active
                        .as_ref()
                        .and_then(|t| t.native_turn_id.as_deref())
                        == str_of(turn, "id")
                {
                    self.complete_turn(turn);
                }
            }
            "item/started" | "item/completed" => {
                if !self.turn_matches(params) {
                    return;
                }
                if let Some(item) = params.get("item") {
                    self.flush(true);
                    self.on_item(item, method == "item/completed");
                }
            }
            "item/agentMessage/delta" => self.on_text_delta(params, "agent"),
            "item/reasoning/summaryTextDelta" => self.on_text_delta(params, "summary"),
            "item/reasoning/textDelta" => self.on_text_delta(params, "content"),
            "item/plan/delta" => self.on_text_delta(params, "plan"),
            "item/commandExecution/outputDelta" => {
                let (Some(item_id), Some(delta)) =
                    (str_of(params, "itemId"), str_of(params, "delta"))
                else {
                    return;
                };
                let tern_id = self
                    .active
                    .as_ref()
                    .and_then(|t| t.items.get(item_id))
                    .map(|i| i.id.clone());
                if let Some(id) = tern_id {
                    self.emitter.send(ProviderEvent::CommandOutputDelta {
                        item_id: id,
                        chunk: delta.to_string(),
                    });
                }
            }
            "turn/plan/updated" => self.on_todo(params),
            "thread/tokenUsage/updated" => self.on_token_usage(params),
            "error" => self.on_error(params),
            "model/rerouted" => {
                if self.turn_matches(params)
                    && let (Some(t), Some(to)) = (self.active.as_mut(), str_of(params, "toModel"))
                {
                    t.model = Some(to.to_string());
                }
            }
            "serverRequest/resolved" => {
                let Some(native) = params.get("requestId") else {
                    return;
                };
                let key = self
                    .pending
                    .iter()
                    .find(|(_, p)| &p.native_id == native)
                    .map(|(k, _)| k.clone());
                if let Some(key) = key {
                    self.close_request(&key, RequestStatus::Cancelled);
                }
            }
            "thread/closed" => {
                if let Some(t) = self.active.as_ref().filter(|t| t.accepted) {
                    let status = if t.interrupting {
                        TerminalStatus::Interrupted
                    } else {
                        TerminalStatus::Failed
                    };
                    let fail = (status == TerminalStatus::Failed).then(|| {
                        failure(
                            FailureClass::ProviderError,
                            "Codex closed the thread during the turn.",
                        )
                    });
                    self.finish(status, fail, Some("thread closed".into()), false);
                }
            }
            _ => {}
        }
    }

    fn on_text_delta(&mut self, params: &Value, kind: &str) {
        if !self.turn_matches(params) {
            return;
        }
        let (Some(item_id), Some(delta)) = (str_of(params, "itemId"), str_of(params, "delta"))
        else {
            return;
        };
        if kind != "content" {
            self.clear_retry();
        }
        let Some(t) = self.active.as_mut() else {
            return;
        };
        let (key, slow) = match kind {
            "summary" => (
                format!(
                    "{item_id}:summary:{}",
                    u64_of(params, "summaryIndex").unwrap_or(0)
                ),
                false,
            ),
            "content" => (
                format!(
                    "{item_id}:content:{}",
                    u64_of(params, "contentIndex").unwrap_or(0)
                ),
                false,
            ),
            "plan" => (item_id.to_string(), true),
            _ => (item_id.to_string(), false),
        };
        if !t.items.contains(&key) {
            let body = match kind {
                "agent" => TurnItemBody::AssistantMessage {
                    message_id: item_id.to_string(),
                    text: String::new(),
                    streaming: true,
                    phase: None,
                },
                "plan" => TurnItemBody::ProposedPlan {
                    plan_id: t.plan_ids.entry(key.clone()).or_insert_with(new_id).clone(),
                    markdown: String::new(),
                    streaming: true,
                },
                _ => TurnItemBody::Reasoning {
                    text: String::new(),
                    streaming: true,
                },
            };
            t.items
                .upsert(&key, Some(item_id), body, ItemStatus::Running, None);
        }
        t.items.update_quiet(&key, slow, |i| match &mut i.body {
            TurnItemBody::AssistantMessage { text, .. } | TurnItemBody::Reasoning { text, .. } => {
                text.push_str(delta)
            }
            TurnItemBody::ProposedPlan { markdown, .. } => markdown.push_str(delta),
            _ => {}
        });
    }

    fn on_item(&mut self, item: &Value, completed: bool) {
        let Some(id) = str_of(item, "id") else { return };
        let ty = str_of(item, "type").unwrap_or_default();
        let status = if completed {
            item_status(str_of(item, "status").or(Some("completed")))
        } else {
            ItemStatus::Running
        };
        let emitter = self.emitter.clone();
        let Some(t) = self.active.as_mut() else {
            return;
        };
        let emit = |item: TurnItem| emitter.send(ProviderEvent::ItemUpserted(item));
        match ty {
            "agentMessage" => {
                let text = str_of(item, "text").unwrap_or_default().to_string();
                let body = TurnItemBody::AssistantMessage {
                    message_id: id.to_string(),
                    text,
                    streaming: !completed,
                    phase: phase(str_of(item, "phase")),
                };
                let snap = t.items.upsert(
                    id,
                    Some(id),
                    body,
                    if completed {
                        ItemStatus::Completed
                    } else {
                        ItemStatus::Running
                    },
                    None,
                );
                // Started with no text yet: the first delta emits it.
                if completed {
                    emit(snap);
                }
            }
            "reasoning" => {
                if !completed {
                    return;
                }
                for (part, key) in [("summary", "summary"), ("content", "content")] {
                    let parts = item
                        .get(part)
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    for (i, p) in parts.iter().enumerate() {
                        let Some(text) = p.as_str().filter(|s| !s.is_empty()) else {
                            continue;
                        };
                        let k = format!("{id}:{key}:{i}");
                        let body = TurnItemBody::Reasoning {
                            text: text.to_string(),
                            streaming: false,
                        };
                        emit(
                            t.items
                                .upsert(&k, Some(id), body, ItemStatus::Completed, None),
                        );
                    }
                }
                // Parts that only streamed: close them too.
                let prefix = format!("{id}:");
                let open: Vec<String> = (0..64)
                    .flat_map(|i| {
                        [
                            format!("{prefix}summary:{i}"),
                            format!("{prefix}content:{i}"),
                        ]
                    })
                    .filter(|k| t.items.get(k).is_some_and(|it| it.status.is_open()))
                    .collect();
                for k in open {
                    if let Some(s) = t.items.update(&k, |it| {
                        it.status = ItemStatus::Completed;
                        if let TurnItemBody::Reasoning { streaming, .. } = &mut it.body {
                            *streaming = false;
                        }
                    }) {
                        emit(s);
                    }
                }
            }
            "commandExecution" => {
                let exit_code = item
                    .get("exitCode")
                    .and_then(Value::as_i64)
                    .map(|c| c as i32);
                let body = TurnItemBody::CommandExecution {
                    input: str_of(item, "command").unwrap_or_default().to_string(),
                    output: str_of(item, "aggregatedOutput").map(str::to_string),
                    exit_code,
                    output_indicates_failure: completed
                        .then(|| status == ItemStatus::Failed || exit_code.is_some_and(|c| c != 0)),
                };
                emit(t.items.upsert(id, Some(id), body, status, None));
            }
            "fileChange" => {
                emit(
                    t.items
                        .upsert(id, Some(id), file_change_body(item), status, None),
                )
            }
            "mcpToolCall" => {
                let name = format!(
                    "{}/{}",
                    str_of(item, "server").unwrap_or("mcp"),
                    str_of(item, "tool").unwrap_or("tool")
                );
                let output = item
                    .get("result")
                    .filter(|r| !r.is_null())
                    .cloned()
                    .or_else(|| item.get("error").filter(|e| !e.is_null()).cloned());
                let body = TurnItemBody::DynamicTool {
                    tool_name: Some(name),
                    input: item.get("arguments").cloned().unwrap_or(Value::Null),
                    output,
                };
                emit(t.items.upsert(id, Some(id), body, status, None));
            }
            "dynamicToolCall" => {
                let body = TurnItemBody::DynamicTool {
                    tool_name: str_of(item, "tool").map(str::to_string),
                    input: item.get("arguments").cloned().unwrap_or(Value::Null),
                    output: item.get("contentItems").filter(|c| !c.is_null()).cloned(),
                };
                emit(t.items.upsert(id, Some(id), body, status, None));
            }
            "webSearch" => {
                let body = TurnItemBody::WebSearch {
                    patterns: web_search_patterns(item),
                    results: None,
                };
                emit(t.items.upsert(id, Some(id), body, status, None));
            }
            "plan" => {
                let plan_id = t
                    .plan_ids
                    .entry(id.to_string())
                    .or_insert_with(new_id)
                    .clone();
                if !completed {
                    let body = TurnItemBody::ProposedPlan {
                        plan_id,
                        markdown: String::new(),
                        streaming: true,
                    };
                    t.items
                        .upsert(id, Some(id), body, ItemStatus::Running, None);
                    return;
                }
                let markdown = str_of(item, "text").unwrap_or_default().to_string();
                // An interrupted turn leaves its plan unfinished (CA:4930-4939).
                let st = if t.interrupting {
                    ItemStatus::Interrupted
                } else {
                    ItemStatus::Completed
                };
                let body = TurnItemBody::ProposedPlan {
                    plan_id: plan_id.clone(),
                    markdown: markdown.clone(),
                    streaming: false,
                };
                let snap = t.items.upsert(id, Some(id), body, st, None);
                emitter.send(ProviderEvent::PlanUpdated {
                    plan_id,
                    markdown,
                    streaming: false,
                });
                emit(snap);
            }
            "contextCompaction" => {
                let title = if completed {
                    "Context compacted"
                } else {
                    "Compacting context"
                };
                let body = TurnItemBody::Compaction {
                    summary: None,
                    before_token_count: None,
                    after_token_count: None,
                };
                emit(
                    t.items
                        .upsert(id, Some(id), body, status, Some(title.into())),
                );
            }
            "imageView" if completed => {
                let body = TurnItemBody::DynamicTool {
                    tool_name: Some("view_image".into()),
                    input: json!({ "path": str_of(item, "path") }),
                    output: None,
                };
                emit(
                    t.items
                        .upsert(id, Some(id), body, ItemStatus::Completed, None),
                );
            }
            "imageGeneration" => {
                let body = TurnItemBody::DynamicTool {
                    tool_name: Some("image_generation".into()),
                    input: json!({ "revisedPrompt": item.get("revisedPrompt") }),
                    output: item
                        .get("savedPath")
                        .filter(|p| !p.is_null())
                        .map(|p| json!({ "savedPath": p })),
                };
                let st = if completed {
                    item_status(str_of(item, "status").filter(|s| *s != "generating"))
                } else {
                    ItemStatus::Running
                };
                emit(t.items.upsert(id, Some(id), body, st, None));
            }
            "collabAgentToolCall" => {
                if str_of(item, "tool") == Some("spawnAgent") {
                    t.has_subagents = true;
                }
                let receivers = item.get("receiverThreadIds").and_then(Value::as_array);
                let subagent_id = receivers
                    .and_then(|r| r.first())
                    .and_then(Value::as_str)
                    .unwrap_or(id)
                    .to_string();
                let body = TurnItemBody::Subagent {
                    subagent_id,
                    prompt: str_of(item, "prompt").unwrap_or_default().to_string(),
                    progress: str_of(item, "tool").map(str::to_string),
                    result: None,
                };
                let st = if completed {
                    ItemStatus::Completed
                } else {
                    ItemStatus::Running
                };
                emit(t.items.upsert(id, Some(id), body, st, None));
            }
            "subAgentActivity" => t.has_subagents = true,
            _ => {}
        }
    }

    fn on_todo(&mut self, params: &Value) {
        if !self.turn_matches(params) {
            return;
        }
        let turn_id = str_of(params, "turnId").unwrap_or_default();
        let Some(t) = self.active.as_mut() else {
            return;
        };
        let key = format!("turn-plan:{turn_id}");
        let plan_id = t.plan_ids.entry(key.clone()).or_insert_with(new_id).clone();
        let steps: Vec<PlanStep> = params
            .get("plan")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .enumerate()
                    .map(|(i, s)| PlanStep {
                        id: format!("step-{}", i + 1),
                        text: str_of(s, "step")
                            .filter(|t| !t.is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("Step {}", i + 1)),
                        status: match str_of(s, "status") {
                            Some("completed") => PlanStepStatus::Completed,
                            Some("inProgress") => PlanStepStatus::Running,
                            _ => PlanStepStatus::Pending,
                        },
                        duration_anchor_at: None,
                        duration_ms: None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let explanation = str_of(params, "explanation").map(str::to_string);
        let body = TurnItemBody::TodoList {
            plan_id: plan_id.clone(),
            steps: steps.clone(),
            explanation: explanation.clone(),
        };
        let item = t
            .items
            .upsert(&key, Some(&key), body, ItemStatus::Completed, None);
        self.emitter.send(ProviderEvent::TodoUpdated {
            plan_id,
            steps,
            explanation,
        });
        self.emitter.send(ProviderEvent::ItemUpserted(item));
    }

    fn on_token_usage(&mut self, params: &Value) {
        let Some(u) = params.get("tokenUsage") else {
            return;
        };
        let total = u.get("total").map(Breakdown::from).unwrap_or_default();
        let last_v = u.get("last").cloned().unwrap_or(Value::Null);
        let last = Breakdown::from(&last_v);
        if let Some(turn_id) = str_of(params, "turnId") {
            self.usage.record(turn_id, total, last);
        }
        let ctx = ContextUsage {
            used_tokens: u64_of(&last_v, "totalTokens").unwrap_or(0),
            max_tokens: u64_of(u, "modelContextWindow"),
            input_tokens: Some(last.input),
            cached_input_tokens: Some(last.cached),
            output_tokens: Some(last.output),
            reasoning_output_tokens: Some(last.reasoning),
            last_used_tokens: None,
            auto_compact_threshold: None,
        };
        self.context_usage = Some(ctx.clone());
        let status = if self.active.is_some() {
            ProviderThreadStatus::Active
        } else {
            ProviderThreadStatus::Idle
        };
        self.emitter.send(ProviderEvent::BindingUpdated {
            native_thread_id: self.native_thread_id.clone(),
            status,
            context_usage: Some(ctx),
        });
    }

    fn on_error(&mut self, params: &Value) {
        if !self.turn_matches(params) {
            return;
        }
        let error = params.get("error").cloned().unwrap_or(Value::Null);
        let info = error.get("codexErrorInfo").cloned().unwrap_or(Value::Null);
        let code = error_info_code(&info);
        let message = str_of(&error, "message")
            .unwrap_or("Codex error")
            .to_string();
        let will_retry = params
            .get("willRetry")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let Some(t) = self.active.as_mut() else {
            return;
        };
        if !will_retry {
            let mut f = terminal_failure(&error);
            if let Some(d) = str_of(&error, "additionalDetails").filter(|d| !d.is_empty()) {
                f.message = d.to_string();
            }
            t.last_failure = Some(f);
            return;
        }
        let class = if is_usage_limit(code.as_deref(), &info) {
            FailureClass::UsageLimit
        } else if code
            .as_deref()
            .is_some_and(|c| c.starts_with("http") || c.starts_with("responseStream"))
        {
            FailureClass::TransportError
        } else {
            FailureClass::ProviderError
        };
        let detail = str_of(&error, "additionalDetails")
            .filter(|d| !d.is_empty())
            .map(str::to_string);
        let retry = retry_progress(&message).map(|(attempt, max)| ProviderRetry {
            attempt,
            max_attempts: Some(max),
            retry_delay_ms: None,
        });
        let body = TurnItemBody::Error {
            failure: ProviderFailure {
                class,
                message: detail.unwrap_or(message.clone()),
                code,
                retryable: Some(true),
                reset_at: None,
            },
            retry,
        };
        let key = t
            .retry_key
            .get_or_insert_with(|| format!("retry:{}", new_id()))
            .clone();
        let item = t
            .items
            .upsert(&key, None, body, ItemStatus::Running, Some(message));
        self.emitter.send(ProviderEvent::ItemUpserted(item));
    }

    fn complete_turn(&mut self, turn: &Value) {
        let Some(t) = self.active.as_ref() else {
            return;
        };
        let mut status = match str_of(turn, "status") {
            Some("completed") => TerminalStatus::Completed,
            Some("interrupted") => TerminalStatus::Interrupted,
            Some("failed") => TerminalStatus::Failed,
            _ => TerminalStatus::Completed,
        };
        if t.interrupting && status == TerminalStatus::Completed {
            status = TerminalStatus::Interrupted;
        }
        let fail = if status == TerminalStatus::Failed {
            Some(match turn.get("error").filter(|e| !e.is_null()) {
                Some(e) => terminal_failure(e),
                None => t
                    .last_failure
                    .clone()
                    .unwrap_or_else(|| failure(FailureClass::ProviderError, "Codex turn failed.")),
            })
        } else {
            None
        };
        self.finish(status, fail, None, false);
    }

    /// Closes the active turn: flush, settle items and requests, usage, terminal.
    pub fn finish(
        &mut self,
        status: TerminalStatus,
        fail: Option<ProviderFailure>,
        reason: Option<String>,
        broken: bool,
    ) {
        self.flush(true);
        let Some(mut t) = self.active.take() else {
            return;
        };
        if let Some(key) = t.retry_key.take()
            && let Some(item) = t.items.update(&key, |i| i.status = ItemStatus::Completed)
        {
            self.emitter.send(ProviderEvent::ItemUpserted(item));
        }
        let open_status = match status {
            TerminalStatus::Completed => ItemStatus::Completed,
            TerminalStatus::Failed if broken => ItemStatus::Failed,
            _ => ItemStatus::Interrupted,
        };
        let pending: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, p)| p.run_id == t.run_id)
            .map(|(k, _)| k.clone())
            .collect();
        for key in pending {
            if let Some(p) = self.pending.remove(&key) {
                self.emitter.send(ProviderEvent::RequestClosed {
                    request_id: key,
                    status: RequestStatus::Cancelled,
                });
                t.items
                    .update_quiet(&p.item_key, false, |i| i.status = ItemStatus::Cancelled);
            }
        }
        let mut settled = t.items.take_dirty(true);
        settled.extend(t.items.settle_open(open_status));
        emit_items(&self.emitter, settled);
        if !t.accepted {
            return;
        }
        let usage = self.usage.finish(
            status == TerminalStatus::Completed,
            t.has_subagents,
            t.model.clone().or(self.model.clone()),
        );
        self.emitter.send(ProviderEvent::TurnUsage {
            run_id: t.run_id.clone(),
            usage,
            model: t.model.clone(),
        });
        self.emitter.send(ProviderEvent::TurnTerminal {
            run_id: t.run_id,
            status,
            failure: fail,
            reason,
            broken,
        });
        if !broken {
            self.emitter.send(ProviderEvent::BindingUpdated {
                native_thread_id: self.native_thread_id.clone(),
                status: ProviderThreadStatus::Idle,
                context_usage: self.context_usage.clone(),
            });
            self.emitter.send(ProviderEvent::SessionUpdated {
                status: SessionStatus::Ready,
                model: self.model.clone(),
                startup_phase: None,
                last_error: None,
            });
        }
    }

    /// Process exit, broken pipe or a bad frame: everything open fails and the session is gone.
    pub fn fail_session(&mut self, message: &str) {
        let class = FailureClass::TransportError;
        if self.active.as_ref().is_some_and(|t| t.accepted) {
            let interrupted = self.active.as_ref().is_some_and(|t| t.interrupting);
            if interrupted {
                self.finish(
                    TerminalStatus::Interrupted,
                    None,
                    Some(message.to_string()),
                    true,
                );
            } else {
                self.finish(
                    TerminalStatus::Failed,
                    Some(failure(class, message)),
                    Some(message.to_string()),
                    true,
                );
            }
        } else {
            self.active = None;
        }
        for (key, _) in std::mem::take(&mut self.pending) {
            self.emitter.send(ProviderEvent::RequestClosed {
                request_id: key,
                status: RequestStatus::Cancelled,
            });
        }
        self.emitter.send(ProviderEvent::BindingUpdated {
            native_thread_id: self.native_thread_id.clone(),
            status: ProviderThreadStatus::Error,
            context_usage: self.context_usage.clone(),
        });
        self.emitter.send(ProviderEvent::SessionUpdated {
            status: SessionStatus::Error,
            model: self.model.clone(),
            startup_phase: None,
            last_error: Some(message.to_string()),
        });
    }

    fn close_request(&mut self, key: &str, status: RequestStatus) {
        let Some(p) = self.pending.remove(key) else {
            return;
        };
        self.emitter.send(ProviderEvent::RequestClosed {
            request_id: key.to_string(),
            status,
        });
        if let Some(t) = self.active.as_mut()
            && let Some(item) = t.items.update(&p.item_key, |i| {
                if i.status.is_open() {
                    i.status = ItemStatus::Cancelled;
                }
            })
        {
            self.emitter.send(ProviderEvent::ItemUpserted(item));
        }
    }

    // ---- server requests (§3.5) ----

    fn open_request(
        &mut self,
        native_id: Value,
        method: &str,
        params: &Value,
        kind: RequestKind,
        body: TurnItemBody,
        title: Option<String>,
    ) -> ServerReply {
        let Some(t) = self.active.as_mut() else {
            return ServerReply::Error(-32603, "No active Tern turn for this request.".into());
        };
        let key = format!("request:{native_id}");
        let req_id = new_id();
        let item = t.items.upsert(
            &key,
            None,
            with_request_id(body, &req_id),
            ItemStatus::Waiting,
            title,
        );
        let req = runtime_request(
            req_id,
            &self.thread_id,
            Some(t.run_id.clone()),
            &item.id,
            native_id.clone(),
            kind,
            &self.live_session_id,
        );
        self.pending.insert(
            req.id.clone(),
            PendingRequest {
                native_id,
                method: method.to_string(),
                kind,
                item_key: key,
                params: params.clone(),
                run_id: t.run_id.clone(),
            },
        );
        self.emitter.send(ProviderEvent::ItemUpserted(item));
        self.emitter.send(ProviderEvent::RequestOpened(req));
        self.emitter.send(ProviderEvent::SessionUpdated {
            status: SessionStatus::Waiting,
            model: self.model.clone(),
            startup_phase: None,
            last_error: None,
        });
        ServerReply::Deferred
    }

    fn approval_body(
        kind: RequestKind,
        prompt: Option<String>,
        reason: Option<String>,
    ) -> TurnItemBody {
        TurnItemBody::ApprovalRequest {
            request_id: String::new(),
            request_kind: kind,
            prompt,
            reason,
            full_access_override: None,
            options: None,
        }
    }

    pub fn on_server_request(&mut self, id: Value, method: &str, params: &Value) -> ServerReply {
        self.flush(true);
        let full = self.runtime_mode == RuntimeMode::FullAccess;
        let reason = str_of(params, "reason").map(str::to_string);
        match method {
            "item/commandExecution/requestApproval" | "execCommandApproval" => {
                let legacy = method == "execCommandApproval";
                let network = params
                    .get("networkApprovalContext")
                    .filter(|n| !n.is_null());
                let has_amendments = params
                    .get("proposedNetworkPolicyAmendments")
                    .and_then(Value::as_array)
                    .is_some_and(|a| !a.is_empty());
                if full && network.is_none() && !has_amendments {
                    return ServerReply::Result(decision_value(legacy, ApprovalDecision::Accept));
                }
                if self.allowances.contains(&RequestKind::Command) {
                    return ServerReply::Result(decision_value(
                        legacy,
                        ApprovalDecision::AcceptForSession,
                    ));
                }
                let command = match params.get("command") {
                    Some(Value::String(s)) => Some(s.clone()),
                    Some(Value::Array(a)) => Some(
                        a.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    _ => None,
                };
                let mut lines: Vec<String> = command.into_iter().collect();
                if let Some(cwd) = str_of(params, "cwd") {
                    lines.push(format!("cwd {cwd}"));
                }
                if let Some(n) = network {
                    lines.push(format!(
                        "network {} {}",
                        str_of(n, "protocol").unwrap_or(""),
                        str_of(n, "host").unwrap_or("")
                    ));
                }
                let prompt = (!lines.is_empty()).then(|| lines.join("\n"));
                self.open_request(
                    id,
                    method,
                    params,
                    RequestKind::Command,
                    Self::approval_body(RequestKind::Command, prompt, reason),
                    None,
                )
            }
            "item/fileChange/requestApproval" | "applyPatchApproval" => {
                let legacy = method == "applyPatchApproval";
                if full {
                    return ServerReply::Result(decision_value(legacy, ApprovalDecision::Accept));
                }
                if self.allowances.contains(&RequestKind::FileChange) {
                    return ServerReply::Result(decision_value(
                        legacy,
                        ApprovalDecision::AcceptForSession,
                    ));
                }
                let paths: Vec<String> = if legacy {
                    params
                        .get("fileChanges")
                        .and_then(Value::as_object)
                        .map(|o| o.keys().cloned().collect())
                        .unwrap_or_default()
                } else {
                    let item_id = str_of(params, "itemId").unwrap_or_default();
                    match self
                        .active
                        .as_ref()
                        .and_then(|t| t.items.get(item_id))
                        .map(|i| &i.body)
                    {
                        Some(TurnItemBody::FileChange {
                            changes: Some(c), ..
                        }) => c.iter().map(|d| d.path.clone()).collect(),
                        _ => vec![],
                    }
                };
                let prompt = (!paths.is_empty()).then(|| paths.join("\n"));
                self.open_request(
                    id,
                    method,
                    params,
                    RequestKind::FileChange,
                    Self::approval_body(RequestKind::FileChange, prompt, reason),
                    None,
                )
            }
            "item/permissions/requestApproval" => {
                let perms = params.get("permissions").cloned().unwrap_or(json!({}));
                if full {
                    return ServerReply::Result(json!({ "permissions": perms, "scope": "turn" }));
                }
                let fs = perms.get("fileSystem");
                let has = |k: &str| {
                    fs.and_then(|f| f.get(k))
                        .and_then(Value::as_array)
                        .is_some_and(|a| !a.is_empty())
                        || fs
                            .and_then(|f| f.get("entries"))
                            .and_then(Value::as_array)
                            .is_some_and(|e| e.iter().any(|x| str_of(x, "access") == Some(k)))
                };
                let kind = if has("write") {
                    RequestKind::FileChange
                } else if has("read") {
                    RequestKind::FileRead
                } else {
                    RequestKind::Command
                };
                let prompt = Some(serde_json::to_string_pretty(&perms).unwrap_or_default());
                self.open_request(
                    id,
                    method,
                    params,
                    kind,
                    Self::approval_body(kind, prompt, reason),
                    None,
                )
            }
            "item/tool/requestUserInput" => {
                let questions: Vec<UserInputQuestion> = params
                    .get("questions")
                    .and_then(Value::as_array)
                    .map(|qs| {
                        qs.iter()
                            .map(|q| UserInputQuestion {
                                id: str_of(q, "id").unwrap_or_default().to_string(),
                                header: str_of(q, "header").unwrap_or_default().to_string(),
                                question: str_of(q, "question").unwrap_or_default().to_string(),
                                options: q
                                    .get("options")
                                    .and_then(Value::as_array)
                                    .map(|os| {
                                        os.iter()
                                            .map(|o| QuestionOption {
                                                label: str_of(o, "label")
                                                    .unwrap_or_default()
                                                    .to_string(),
                                                description: str_of(o, "description")
                                                    .map(str::to_string),
                                                value: None,
                                                preview: None,
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                                multi_select: None,
                                allow_custom_answer: q.get("isOther").and_then(Value::as_bool),
                                initial_answer: None,
                                required: None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let body = TurnItemBody::UserInputRequest {
                    request_id: String::new(),
                    questions,
                    question_answer: None,
                };
                self.open_request(id, method, params, RequestKind::UserInput, body, None)
            }
            "mcpServer/elicitation/request" => {
                if str_of(params, "mode") == Some("url") || self.active.is_none() {
                    return ServerReply::Result(json!({ "action": "cancel" }));
                }
                let required = params
                    .get("requestedSchema")
                    .and_then(|s| s.get("required"))
                    .and_then(Value::as_array)
                    .is_some_and(|r| !r.is_empty());
                if full {
                    return ServerReply::Result(if required {
                        json!({ "action": "cancel" })
                    } else {
                        json!({ "action": "accept", "content": {} })
                    });
                }
                let prompt = str_of(params, "message").map(str::to_string);
                let title = str_of(params, "serverName").map(str::to_string);
                self.open_request(
                    id,
                    method,
                    params,
                    RequestKind::McpElicitation,
                    Self::approval_body(RequestKind::McpElicitation, prompt, None),
                    title,
                )
            }
            other => ServerReply::Error(-32601, format!("Method not found: {other}")),
        }
    }

    /// The user's answer for a pending request: `(json-rpc id, result)` to send.
    pub fn respond(&mut self, response: &RequestResponse) -> anyhow::Result<(Value, Value)> {
        let key = response.request.id.clone();
        let p = self
            .pending
            .remove(&key)
            .ok_or_else(|| anyhow::anyhow!("Codex is no longer waiting on request {key}"))?;
        // Answered: the engine owns the item from here; keep settle_open off it.
        if let Some(t) = self.active.as_mut() {
            let _ = t
                .items
                .update(&p.item_key, |i| i.status = ItemStatus::Completed);
        }
        let decision = response
            .decision
            .unwrap_or(if p.kind == RequestKind::UserInput {
                ApprovalDecision::Accept
            } else {
                ApprovalDecision::Decline
            });
        let result = match p.method.as_str() {
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                if matches!(
                    decision,
                    ApprovalDecision::AcceptForSession | ApprovalDecision::AcceptAlways
                ) {
                    self.allowances.insert(p.kind);
                }
                decision_value(false, decision)
            }
            "execCommandApproval" | "applyPatchApproval" => {
                if matches!(
                    decision,
                    ApprovalDecision::AcceptForSession | ApprovalDecision::AcceptAlways
                ) {
                    self.allowances.insert(p.kind);
                }
                decision_value(true, decision)
            }
            "item/permissions/requestApproval" => match decision {
                ApprovalDecision::Accept => {
                    json!({ "permissions": p.params.get("permissions"), "scope": "turn" })
                }
                ApprovalDecision::AcceptForSession => {
                    json!({ "permissions": p.params.get("permissions"), "scope": "session" })
                }
                _ => json!({ "permissions": {}, "scope": "turn" }),
            },
            "item/tool/requestUserInput" => {
                let mut answers = serde_json::Map::new();
                if let Some(a) = &response.answers {
                    for (qid, v) in a {
                        answers.insert(qid.clone(), json!({ "answers": answer_strings(v) }));
                    }
                }
                json!({ "answers": answers })
            }
            "mcpServer/elicitation/request" => match decision {
                ApprovalDecision::Accept
                | ApprovalDecision::AcceptForSession
                | ApprovalDecision::AcceptAlways => {
                    let required = p
                        .params
                        .get("requestedSchema")
                        .and_then(|s| s.get("required"))
                        .and_then(Value::as_array)
                        .is_some_and(|r| !r.is_empty());
                    if required {
                        json!({ "action": "cancel" })
                    } else {
                        let mut r = json!({ "action": "accept", "content": {} });
                        match decision {
                            ApprovalDecision::AcceptForSession => {
                                r["_meta"] = json!({ "persist": "session" })
                            }
                            ApprovalDecision::AcceptAlways => {
                                r["_meta"] = json!({ "persist": "always" })
                            }
                            _ => {}
                        }
                        r
                    }
                }
                ApprovalDecision::Decline => json!({ "action": "decline" }),
                ApprovalDecision::Cancel => json!({ "action": "cancel" }),
            },
            other => anyhow::bail!("unexpected pending method {other}"),
        };
        if self.pending.values().all(|q| q.run_id != p.run_id) && self.active.is_some() {
            self.emitter.send(ProviderEvent::SessionUpdated {
                status: SessionStatus::Running,
                model: self.model.clone(),
                startup_phase: None,
                last_error: None,
            });
        }
        Ok((p.native_id, result))
    }
}

/// v2 decisions verbatim (`acceptAlways` -> `acceptForSession`); legacy v1 `ReviewDecision` (CA:728-742).
fn decision_value(legacy: bool, d: ApprovalDecision) -> Value {
    if legacy {
        return match d {
            ApprovalDecision::Accept => json!({ "decision": "approved" }),
            ApprovalDecision::AcceptForSession | ApprovalDecision::AcceptAlways => {
                json!({ "decision": "approved_for_session" })
            }
            ApprovalDecision::Decline => {
                json!({ "decision": { "denied": { "rejection": "User declined the request." } } })
            }
            ApprovalDecision::Cancel => json!({ "decision": "abort" }),
        };
    }
    let s = match d {
        ApprovalDecision::Accept => "accept",
        ApprovalDecision::AcceptForSession | ApprovalDecision::AcceptAlways => "acceptForSession",
        ApprovalDecision::Decline => "decline",
        ApprovalDecision::Cancel => "cancel",
    };
    json!({ "decision": s })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_progress_parses() {
        assert_eq!(retry_progress("Reconnecting... 2/5"), Some((2, 5)));
        assert_eq!(retry_progress("Reconnecting... 3 / 10"), Some((3, 10)));
        assert_eq!(retry_progress("no numbers"), None);
    }

    #[test]
    fn failure_classes() {
        let f = terminal_failure(&json!({"message": "x", "codexErrorInfo": "usageLimitExceeded"}));
        assert_eq!(f.class, FailureClass::UsageLimit);
        let f = terminal_failure(
            &json!({"message": "x", "codexErrorInfo": {"httpConnectionFailed": {"httpStatusCode": 422}}}),
        );
        assert_eq!(
            (f.class, f.code.as_deref()),
            (FailureClass::ProviderError, Some("httpConnectionFailed"))
        );
        let f = terminal_failure(
            &json!({"message": "x", "codexErrorInfo": {"responseStreamDisconnected": {"httpStatusCode": null}}}),
        );
        assert_eq!(f.class, FailureClass::TransportError);
        let f = terminal_failure(
            &json!({"message": "x", "codexErrorInfo": {"responseTooManyFailedAttempts": {"httpStatusCode": 429}}}),
        );
        assert_eq!(f.class, FailureClass::UsageLimit);
    }

    #[test]
    fn usage_deltas_follow_codex_turn_token_usage() {
        let mut u = UsageState::default();
        let b = |i, o| Breakdown {
            input: i,
            cached: 0,
            cache_write: Some(0),
            output: o,
            reasoning: 0,
        };
        u.record("old", b(100, 10), b(100, 10));
        u.begin("t1");
        u.record("t1", b(150, 15), b(50, 5));
        u.record("t1", b(250, 20), b(100, 5));
        match u.finish(true, false, Some("m".into())) {
            TurnTokenUsage::Complete(c) => {
                assert_eq!((c.input_tokens, c.output_tokens), (Some(150), Some(10)));
                assert_eq!(c.models, Some(vec!["m".into()]));
            }
            other => panic!("{other:?}"),
        }
        u.begin("t2");
        assert!(matches!(
            u.finish(false, false, None),
            TurnTokenUsage::Unavailable(_)
        ));
    }

    #[test]
    fn decisions_map() {
        assert_eq!(
            decision_value(false, ApprovalDecision::AcceptAlways),
            json!({"decision": "acceptForSession"})
        );
        assert_eq!(
            decision_value(true, ApprovalDecision::Cancel),
            json!({"decision": "abort"})
        );
    }
}

//! Claude CLI stream-json stdout frames -> `ProviderEvent`s (spec/07 §4.3-4.7).
//! Pure state machine: no IO. Returns the control responses the session must write.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use tern_core::*;

use crate::RequestResponse;
use crate::items::{ItemTracker, emit_items, runtime_request, with_request_id};
use crate::util::{
    Emitter, answer_strings, epoch_to_iso, failure, replacement_diff, str_of, truncate_chars,
    u64_of,
};

/// The deny T3 sends after capturing a plan from `ExitPlanMode` (A:13013-13030).
pub const PLAN_CAPTURED_MESSAGE: &str = "The client captured your proposed plan. Stop here and wait for the user's feedback or implementation request in a later turn.";
const USAGE_LIMIT_MESSAGE: &str =
    "Claude usage limit reached. Send the message again once the limit resets.";
const CONTEXT_WINDOW_FULL: &str = "Claude stopped: the prompt exceeds the model's context window. Run /compact or start a new thread.";

struct ClaudeTurn {
    run_id: String,
    prompt_uuid: String,
    started_emitted: bool,
    interrupting: bool,
    items: ItemTracker,
    /// Message id of the stream in flight, and its block index -> item key.
    stream_msg: Option<String>,
    blocks: HashMap<u64, String>,
    /// Messages whose text/thinking arrived through `stream_event`.
    streamed_msgs: HashSet<String>,
    snapshot_blocks: u32,
    /// tool_use id -> item key (empty for tools with no work-log row).
    tools: HashMap<String, String>,
    plans_emitted: HashSet<String>,
    rate_limited: bool,
    reset_at: Option<i64>,
    synthetic_error: Option<String>,
    synthetic_text: Option<String>,
    retry_key: Option<String>,
    compaction_key: Option<String>,
    last_assistant_uuid: Option<String>,
    emitted_text: bool,
    /// Steer prompts sent during the turn; a steer can abort the segment in flight.
    steers: u32,
}

struct PendingPermission {
    cli_request_id: Value,
    tool_name: String,
    input: Value,
    tool_use_id: Option<String>,
    suggestions: Option<Value>,
    kind: RequestKind,
    item_key: String,
    run_id: String,
}

pub(crate) struct ClaudeMapper {
    thread_id: String,
    live_session_id: String,
    emitter: Emitter,
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub runtime_mode: RuntimeMode,
    pub interaction_mode: InteractionMode,
    pub context_window: Option<u64>,
    active: Option<ClaudeTurn>,
    pending: HashMap<String, PendingPermission>,
    context_usage: Option<ContextUsage>,
    /// The process saw at least one finished turn, so `--resume` works.
    pub has_turns: bool,
}

fn request_kind(tool: &str) -> RequestKind {
    match tool {
        "Bash" => RequestKind::Command,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => RequestKind::FileChange,
        "Read" | "Grep" | "Glob" | "LS" => RequestKind::FileRead,
        _ => RequestKind::Permission,
    }
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(|b| {
                (str_of(b, "type") == Some("text"))
                    .then(|| str_of(b, "text"))
                    .flatten()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn collect_urls(v: &Value, out: &mut Vec<WebSearchResult>, depth: usize) {
    if depth > 6 || out.len() >= 50 {
        return;
    }
    match v {
        Value::Object(o) => {
            if let Some(url) = o.get("url").and_then(Value::as_str)
                && !out.iter().any(|r| r.url == url)
            {
                out.push(WebSearchResult {
                    title: o.get("title").and_then(Value::as_str).map(str::to_string),
                    url: url.to_string(),
                });
            }
            for x in o.values() {
                collect_urls(x, out, depth + 1);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_urls(x, out, depth + 1)),
        _ => {}
    }
}

/// Grep/Glob output: one path per line, or `path:line:text` in content mode.
fn search_results(text: &str) -> Vec<FileSearchResult> {
    text.lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with("Found ")
                && !l.starts_with("No files")
                && !l.starts_with("No matches")
        })
        .take(200)
        .map(|l| {
            let mut parts = l.splitn(3, ':');
            let path = parts.next().unwrap_or(l);
            match (
                parts.next().and_then(|n| n.parse::<u32>().ok()),
                parts.next(),
            ) {
                (Some(line), preview) => FileSearchResult {
                    path: path.to_string(),
                    line: Some(line),
                    preview: preview.map(str::to_string),
                },
                _ => FileSearchResult {
                    path: l.to_string(),
                    line: None,
                    preview: None,
                },
            }
        })
        .collect()
}

/// `inputTokens + cache` per §4.7, from the last `iterations[]` message entry if any.
fn context_from_usage(usage: &Value, max: Option<u64>) -> Option<ContextUsage> {
    let src = usage
        .get("iterations")
        .and_then(Value::as_array)
        .and_then(|it| {
            it.iter()
                .rev()
                .find(|e| str_of(e, "type") == Some("message"))
        })
        .unwrap_or(usage);
    let input = u64_of(src, "input_tokens")?;
    let cache_create = u64_of(src, "cache_creation_input_tokens").unwrap_or(0);
    let cache_read = u64_of(src, "cache_read_input_tokens").unwrap_or(0);
    let output = u64_of(src, "output_tokens").unwrap_or(0);
    let input_total = input + cache_create + cache_read;
    Some(ContextUsage {
        used_tokens: input_total + output,
        max_tokens: max,
        input_tokens: Some(input_total),
        cached_input_tokens: Some(cache_read),
        output_tokens: Some(output),
        reasoning_output_tokens: None,
        last_used_tokens: None,
        auto_compact_threshold: None,
    })
}

fn terminal_reason_error(reason: Option<&str>, hint: Option<&str>) -> Option<String> {
    Some(
        match reason? {
            "api_error" => {
                return Some(
                    hint.unwrap_or("Claude gave up after repeated API errors.")
                        .to_string(),
                );
            }
            "malformed_tool_use_exhausted" => "Claude gave up after repeated malformed tool calls.",
            "budget_exhausted" => "Claude stopped: the turn's token budget was exhausted.",
            "structured_output_retry_exhausted" => {
                "Claude could not produce the requested structured output."
            }
            "tool_deferred_unavailable" => {
                "Claude could not resume a deferred tool call: the tool is no longer available."
            }
            "turn_setup_failed" => "Claude could not start the turn.",
            "blocking_limit" => return Some(hint.unwrap_or(CONTEXT_WINDOW_FULL).to_string()),
            "rapid_refill_breaker" => {
                "Claude stopped: the context refilled too quickly after compaction."
            }
            "prompt_too_long" => CONTEXT_WINDOW_FULL,
            "image_error" => "Claude stopped: an image in the conversation could not be processed.",
            "model_error" => "Claude stopped: the model returned an error.",
            _ => return None,
        }
        .to_string(),
    )
}

pub(crate) fn control_success(request_id: &Value, response: Value) -> Value {
    json!({ "type": "control_response", "response": { "subtype": "success", "request_id": request_id, "response": response } })
}

pub(crate) fn control_error(request_id: &Value, error: &str) -> Value {
    json!({ "type": "control_response", "response": { "subtype": "error", "request_id": request_id, "error": error } })
}

impl ClaudeMapper {
    pub fn new(thread_id: &str, live_session_id: &str, emitter: Emitter) -> Self {
        Self {
            thread_id: thread_id.to_string(),
            live_session_id: live_session_id.to_string(),
            emitter,
            session_id: None,
            model: None,
            runtime_mode: RuntimeMode::FullAccess,
            interaction_mode: InteractionMode::Default,
            context_window: None,
            active: None,
            pending: HashMap::new(),
            context_usage: None,
            has_turns: false,
        }
    }

    pub fn emitter(&self) -> &Emitter {
        &self.emitter
    }

    pub fn active_run(&self) -> Option<&str> {
        self.active.as_ref().map(|t| t.run_id.as_str())
    }

    pub fn begin_turn(&mut self, run_id: &str, prompt_uuid: &str) {
        self.active = Some(ClaudeTurn {
            run_id: run_id.to_string(),
            prompt_uuid: prompt_uuid.to_string(),
            started_emitted: false,
            interrupting: false,
            items: ItemTracker::new(&self.thread_id, Some(run_id.to_string())),
            stream_msg: None,
            blocks: HashMap::new(),
            streamed_msgs: HashSet::new(),
            snapshot_blocks: 0,
            tools: HashMap::new(),
            plans_emitted: HashSet::new(),
            rate_limited: false,
            reset_at: None,
            synthetic_error: None,
            synthetic_text: None,
            retry_key: None,
            compaction_key: None,
            last_assistant_uuid: None,
            emitted_text: false,
            steers: 0,
        });
    }

    pub fn note_steer(&mut self, run_id: &str) {
        if let Some(t) = self.active.as_mut().filter(|t| t.run_id == run_id) {
            t.steers += 1;
        }
    }

    /// The prompt never reached the CLI: forget the turn without a terminal.
    pub fn abort_turn(&mut self, run_id: &str) {
        if self.active.as_ref().is_some_and(|t| t.run_id == run_id) {
            self.active = None;
        }
    }

    pub fn mark_interrupting(&mut self, run_id: &str) {
        if let Some(t) = self.active.as_mut().filter(|t| t.run_id == run_id) {
            t.interrupting = true;
        }
    }

    pub fn is_interrupting(&self) -> bool {
        self.active.as_ref().is_some_and(|t| t.interrupting)
    }

    fn emit_started(&mut self) {
        let Some(t) = self.active.as_mut() else {
            return;
        };
        if t.started_emitted {
            return;
        }
        t.started_emitted = true;
        let run_id = t.run_id.clone();
        self.emitter.send(ProviderEvent::TurnStarted {
            run_id,
            native_turn_id: None,
        });
        self.emitter.send(ProviderEvent::BindingUpdated {
            native_thread_id: self.session_id.clone(),
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

    fn set_context(&mut self, usage: &Value) {
        if let Some(ctx) = context_from_usage(usage, self.context_window) {
            self.context_usage = Some(ctx.clone());
            self.emitter.send(ProviderEvent::BindingUpdated {
                native_thread_id: self.session_id.clone(),
                status: if self.active.is_some() {
                    ProviderThreadStatus::Active
                } else {
                    ProviderThreadStatus::Idle
                },
                context_usage: Some(ctx),
            });
        }
    }

    pub fn flush(&mut self, include_slow: bool) {
        if let Some(t) = self.active.as_mut() {
            let items = t.items.take_dirty(include_slow);
            emit_items(&self.emitter, items);
        }
    }

    fn clear_retry(&mut self) {
        let Some(t) = self.active.as_mut() else {
            return;
        };
        if let Some(key) = t.retry_key.take()
            && let Some(i) = t.items.update(&key, |i| i.status = ItemStatus::Completed)
        {
            self.emitter.send(ProviderEvent::ItemUpserted(i));
        }
    }

    fn upsert(
        &mut self,
        key: &str,
        native: Option<&str>,
        body: TurnItemBody,
        status: ItemStatus,
        title: Option<String>,
    ) {
        if let Some(t) = self.active.as_mut() {
            let item = t.items.upsert(key, native, body, status, title);
            self.emitter.send(ProviderEvent::ItemUpserted(item));
        }
    }

    /// Handles one stdout frame. Returns control responses to write back.
    pub fn on_frame(&mut self, frame: &Value) -> Vec<Value> {
        let ty = str_of(frame, "type").unwrap_or_default();
        let root = frame.get("parent_tool_use_id").is_none_or(Value::is_null);
        match ty {
            "system" => self.on_system(frame),
            "command_lifecycle" => {
                let ours = self
                    .active
                    .as_ref()
                    .is_some_and(|t| str_of(frame, "command_uuid") == Some(t.prompt_uuid.as_str()));
                if ours && str_of(frame, "state") == Some("started") {
                    self.emit_started();
                }
            }
            "stream_event" if root => self.on_stream(frame),
            "assistant" if root => {
                self.flush(true);
                self.on_assistant(frame);
            }
            "user" if root => {
                self.flush(true);
                self.on_user(frame);
            }
            "result" => self.on_result(frame),
            "rate_limit_event" => {
                let info = frame.get("rate_limit_info").cloned().unwrap_or(Value::Null);
                if let Some(t) = self.active.as_mut() {
                    let overage = info
                        .get("isUsingOverage")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if str_of(&info, "status") == Some("rejected") && !overage {
                        t.rate_limited = true;
                        if let Some(r) = info.get("resetsAt").and_then(Value::as_i64) {
                            t.reset_at = Some(t.reset_at.map_or(r, |x| x.max(r)));
                        }
                    }
                }
            }
            "control_request" => return self.on_control_request(frame),
            "control_cancel_request" => {
                if let Some(id) = frame.get("request_id") {
                    let key = self
                        .pending
                        .iter()
                        .find(|(_, p)| &p.cli_request_id == id)
                        .map(|(k, _)| k.clone());
                    if let Some(k) = key {
                        self.close_request(&k, RequestStatus::Cancelled);
                    }
                }
            }
            _ => {}
        }
        vec![]
    }

    fn on_system(&mut self, frame: &Value) {
        match str_of(frame, "subtype") {
            Some("init") => {
                if let Some(sid) = str_of(frame, "session_id")
                    && self.session_id.as_deref() != Some(sid)
                {
                    // `/clear` or a fresh-session retry: adopt the new id (A:13895-13901).
                    self.session_id = Some(sid.to_string());
                    self.emitter.send(ProviderEvent::BindingUpdated {
                        native_thread_id: Some(sid.to_string()),
                        status: if self.active.is_some() {
                            ProviderThreadStatus::Active
                        } else {
                            ProviderThreadStatus::Idle
                        },
                        context_usage: self.context_usage.clone(),
                    });
                }
                if let Some(m) = str_of(frame, "model") {
                    self.model = Some(m.to_string());
                }
                if self.active.is_some() {
                    self.emit_started();
                }
            }
            Some("status") => {
                let status = frame.get("status").and_then(Value::as_str);
                let result = str_of(frame, "compact_result");
                if status == Some("compacting") {
                    let key = self.active.as_mut().map(|t| {
                        t.compaction_key
                            .get_or_insert_with(|| format!("compact:{}", new_id()))
                            .clone()
                    });
                    if let Some(k) = key {
                        let body = TurnItemBody::Compaction {
                            summary: None,
                            before_token_count: None,
                            after_token_count: None,
                        };
                        self.upsert(
                            &k,
                            None,
                            body,
                            ItemStatus::Running,
                            Some("Compacting context".into()),
                        );
                    }
                } else if let Some(r) = result {
                    let key = self.active.as_mut().and_then(|t| t.compaction_key.take());
                    if let Some(k) = key {
                        let (st, title) = if r == "success" {
                            (ItemStatus::Completed, "Context compacted")
                        } else {
                            (ItemStatus::Failed, "Compaction failed")
                        };
                        let body = TurnItemBody::Compaction {
                            summary: str_of(frame, "compact_error").map(str::to_string),
                            before_token_count: None,
                            after_token_count: None,
                        };
                        self.upsert(&k, None, body, st, Some(title.into()));
                    }
                }
            }
            Some("compact_boundary") => {
                let meta = frame
                    .get("compact_metadata")
                    .cloned()
                    .unwrap_or(Value::Null);
                let pre = u64_of(&meta, "pre_tokens");
                let post = u64_of(&meta, "post_tokens");
                let key = self.active.as_mut().map(|t| {
                    t.compaction_key
                        .take()
                        .unwrap_or_else(|| format!("compact:{}", new_id()))
                });
                if let Some(k) = key {
                    let body = TurnItemBody::Compaction {
                        summary: None,
                        before_token_count: pre.map(|v| v as u32),
                        after_token_count: post.map(|v| v as u32),
                    };
                    self.upsert(
                        &k,
                        None,
                        body,
                        ItemStatus::Completed,
                        Some("Context compacted".into()),
                    );
                }
                if let Some(post) = post {
                    let ctx = ContextUsage {
                        used_tokens: post,
                        max_tokens: self.context_window,
                        ..Default::default()
                    };
                    self.context_usage = Some(ctx.clone());
                    self.emitter.send(ProviderEvent::BindingUpdated {
                        native_thread_id: self.session_id.clone(),
                        status: ProviderThreadStatus::Active,
                        context_usage: Some(ctx),
                    });
                }
            }
            Some("api_retry") => {
                let attempt = u64_of(frame, "attempt").unwrap_or(1) as u32;
                let max = u64_of(frame, "max_retries").map(|m| m as u32);
                let status = u64_of(frame, "error_status");
                let error = str_of(frame, "error").unwrap_or("API error");
                let class = if error == "rate_limit" || status == Some(429) {
                    FailureClass::UsageLimit
                } else {
                    FailureClass::ProviderError
                };
                let message = match status {
                    Some(s) => format!("Claude API error {s} ({error}). Retrying."),
                    None => format!("Claude API error ({error}). Retrying."),
                };
                let key = self.active.as_mut().map(|t| {
                    t.retry_key
                        .get_or_insert_with(|| format!("retry:{}", new_id()))
                        .clone()
                });
                if let Some(k) = key {
                    let body = TurnItemBody::Error {
                        failure: ProviderFailure {
                            class,
                            message: message.clone(),
                            code: status.map(|s| s.to_string()),
                            retryable: Some(true),
                            reset_at: None,
                        },
                        retry: Some(ProviderRetry {
                            attempt,
                            max_attempts: max,
                            retry_delay_ms: u64_of(frame, "retry_delay_ms"),
                        }),
                    };
                    self.upsert(&k, None, body, ItemStatus::Running, Some(message));
                }
            }
            Some("informational") | Some("model_refusal_fallback") => {
                let level = str_of(frame, "level").unwrap_or("warning");
                let content = str_of(frame, "content").unwrap_or_default();
                if level == "info" || content.is_empty() || content.starts_with("Stop says:") {
                    return;
                }
                let level = if level == "error" {
                    NoticeLevel::Error
                } else {
                    NoticeLevel::Warning
                };
                let key = format!("notice:{}", new_id());
                let body = TurnItemBody::SystemNotice {
                    message: content.to_string(),
                    level: Some(level),
                };
                self.upsert(&key, None, body, ItemStatus::Completed, None);
            }
            _ => {}
        }
    }

    fn on_stream(&mut self, frame: &Value) {
        let Some(ev) = frame.get("event") else { return };
        let ety = str_of(ev, "type").unwrap_or_default();
        if ety == "message_start" {
            self.clear_retry();
            self.emit_started();
            if let Some(msg) = ev.get("message") {
                if let Some(u) = msg.get("usage") {
                    self.set_context(u);
                }
                if let Some(t) = self.active.as_mut() {
                    t.stream_msg = str_of(msg, "id").map(str::to_string);
                    t.blocks.clear();
                }
            }
            return;
        }
        if ety == "message_delta" {
            if let Some(u) = ev.get("usage") {
                self.set_context(u);
            }
            return;
        }
        let Some(t) = self.active.as_mut() else {
            return;
        };
        let Some(msg) = t.stream_msg.clone() else {
            return;
        };
        let index = u64_of(ev, "index").unwrap_or(0);
        match ety {
            "content_block_start" => {
                let block = ev.get("content_block").cloned().unwrap_or(Value::Null);
                let (key, body) = match str_of(&block, "type") {
                    Some("text") => (
                        format!("{msg}:text:{index}"),
                        TurnItemBody::AssistantMessage {
                            message_id: msg.clone(),
                            text: String::new(),
                            streaming: true,
                            phase: None,
                        },
                    ),
                    Some("thinking") => (
                        format!("{msg}:thinking:{index}"),
                        TurnItemBody::Reasoning {
                            text: String::new(),
                            streaming: true,
                        },
                    ),
                    _ => return,
                };
                t.streamed_msgs.insert(msg.clone());
                t.items
                    .upsert(&key, Some(&msg), body, ItemStatus::Running, None);
                t.blocks.insert(index, key);
            }
            "content_block_delta" => {
                let Some(key) = t.blocks.get(&index).cloned() else {
                    return;
                };
                let delta = ev.get("delta").cloned().unwrap_or(Value::Null);
                let piece = match str_of(&delta, "type") {
                    Some("text_delta") => str_of(&delta, "text"),
                    Some("thinking_delta") => str_of(&delta, "thinking"),
                    _ => None,
                };
                if let Some(piece) = piece.filter(|p| !p.is_empty()) {
                    let piece = piece.to_string();
                    if key.contains(":text:") {
                        t.emitted_text = true;
                    }
                    t.items.update_quiet(&key, false, |i| match &mut i.body {
                        TurnItemBody::AssistantMessage { text, .. }
                        | TurnItemBody::Reasoning { text, .. } => text.push_str(&piece),
                        _ => {}
                    });
                }
            }
            "content_block_stop" => {
                let Some(key) = t.blocks.remove(&index) else {
                    return;
                };
                let empty = matches!(t.items.get(&key).map(|i| &i.body),
                    Some(TurnItemBody::AssistantMessage { text, .. } | TurnItemBody::Reasoning { text, .. }) if text.is_empty());
                if empty {
                    t.items.discard(&key);
                    return;
                }
                if let Some(item) = t.items.update(&key, |i| {
                    i.status = ItemStatus::Completed;
                    if let TurnItemBody::AssistantMessage { streaming, .. }
                    | TurnItemBody::Reasoning { streaming, .. } = &mut i.body
                    {
                        *streaming = false;
                    }
                }) {
                    self.emitter.send(ProviderEvent::ItemUpserted(item));
                }
            }
            _ => {}
        }
    }

    fn on_assistant(&mut self, frame: &Value) {
        let Some(msg) = frame.get("message") else {
            return;
        };
        if str_of(msg, "model") == Some("<synthetic>") {
            if let Some(t) = self.active.as_mut() {
                t.synthetic_error = str_of(frame, "error")
                    .map(str::to_string)
                    .or(t.synthetic_error.take());
                let text = msg.get("content").map(content_text).unwrap_or_default();
                if !text.is_empty() {
                    t.synthetic_text = Some(text);
                }
            }
            return;
        }
        self.clear_retry();
        self.emit_started();
        let msg_id = str_of(msg, "id").unwrap_or_default().to_string();
        let streamed = self
            .active
            .as_ref()
            .is_some_and(|t| t.streamed_msgs.contains(&msg_id));
        if !streamed && let Some(u) = msg.get("usage") {
            self.set_context(u);
        }
        if let Some(t) = self.active.as_mut() {
            t.last_assistant_uuid = str_of(frame, "uuid").map(str::to_string);
        }
        let blocks = msg
            .get("content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for b in blocks {
            match str_of(&b, "type") {
                // Already delivered by the stream; only older CLIs need the snapshot.
                Some("text") if !streamed => {
                    let text = str_of(&b, "text").unwrap_or_default().to_string();
                    if text.is_empty() {
                        continue;
                    }
                    let Some(t) = self.active.as_mut() else {
                        return;
                    };
                    t.snapshot_blocks += 1;
                    t.emitted_text = true;
                    let key = format!("{msg_id}:snap:{}", t.snapshot_blocks);
                    let body = TurnItemBody::AssistantMessage {
                        message_id: msg_id.clone(),
                        text,
                        streaming: false,
                        phase: None,
                    };
                    self.upsert(&key, Some(&msg_id), body, ItemStatus::Completed, None);
                }
                Some("thinking") if !streamed => {
                    let text = str_of(&b, "thinking").unwrap_or_default().to_string();
                    if text.is_empty() {
                        continue;
                    }
                    let Some(t) = self.active.as_mut() else {
                        return;
                    };
                    t.snapshot_blocks += 1;
                    let key = format!("{msg_id}:snap:{}", t.snapshot_blocks);
                    self.upsert(
                        &key,
                        Some(&msg_id),
                        TurnItemBody::Reasoning {
                            text,
                            streaming: false,
                        },
                        ItemStatus::Completed,
                        None,
                    );
                }
                Some("tool_use") => self.on_tool_use(&b),
                _ => {}
            }
        }
    }

    fn emit_plan(&mut self, tool_use_id: &str, input: &Value) {
        let markdown = str_of(input, "plan").unwrap_or_default().trim().to_string();
        let Some(t) = self.active.as_mut() else {
            return;
        };
        if markdown.is_empty() || !t.plans_emitted.insert(tool_use_id.to_string()) {
            return;
        }
        let key = format!("plan:{tool_use_id}");
        let plan_id = new_id();
        let body = TurnItemBody::ProposedPlan {
            plan_id: plan_id.clone(),
            markdown: markdown.clone(),
            streaming: false,
        };
        let item = t
            .items
            .upsert(&key, Some(tool_use_id), body, ItemStatus::Completed, None);
        self.emitter.send(ProviderEvent::PlanUpdated {
            plan_id,
            markdown,
            streaming: false,
        });
        self.emitter.send(ProviderEvent::ItemUpserted(item));
    }

    fn on_tool_use(&mut self, block: &Value) {
        let Some(id) = str_of(block, "id") else {
            return;
        };
        let name = str_of(block, "name").unwrap_or("tool").to_string();
        let input = block.get("input").cloned().unwrap_or(json!({}));
        let s = |k: &str| str_of(&input, k).unwrap_or_default().to_string();
        let body = match name.as_str() {
            "ExitPlanMode" => {
                self.emit_plan(id, &input);
                return;
            }
            // Shown through can_use_tool as a question card.
            "AskUserQuestion" => return,
            "TodoWrite" => {
                let steps: Vec<PlanStep> = input
                    .get("todos")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .enumerate()
                            .map(|(i, td)| PlanStep {
                                id: format!("step-{}", i + 1),
                                text: str_of(td, "content").unwrap_or_default().to_string(),
                                status: match str_of(td, "status") {
                                    Some("completed") => PlanStepStatus::Completed,
                                    Some("in_progress") => PlanStepStatus::Running,
                                    _ => PlanStepStatus::Pending,
                                },
                                duration_anchor_at: None,
                                duration_ms: None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let Some(t) = self.active.as_mut() else {
                    return;
                };
                let key = "todo".to_string();
                let plan_id = match t.items.get(&key).map(|i| &i.body) {
                    Some(TurnItemBody::TodoList { plan_id, .. }) => plan_id.clone(),
                    _ => new_id(),
                };
                let body = TurnItemBody::TodoList {
                    plan_id: plan_id.clone(),
                    steps: steps.clone(),
                    explanation: None,
                };
                let item = t
                    .items
                    .upsert(&key, Some(id), body, ItemStatus::Completed, None);
                t.tools.insert(id.to_string(), String::new());
                self.emitter.send(ProviderEvent::TodoUpdated {
                    plan_id,
                    steps,
                    explanation: None,
                });
                self.emitter.send(ProviderEvent::ItemUpserted(item));
                return;
            }
            "Bash" => TurnItemBody::CommandExecution {
                input: s("command"),
                output: None,
                exit_code: None,
                output_indicates_failure: None,
            },
            "Edit" => {
                let path = s("file_path");
                let (old, new) = (s("old_string"), s("new_string"));
                let diff = replacement_diff(&path, &old, &new);
                let (a, d) = crate::util::diff_counts(&diff);
                TurnItemBody::FileChange {
                    file_name: path.clone(),
                    additions: Some(a),
                    deletions: Some(d),
                    diff_str: Some(diff),
                    old_str: Some(old),
                    new_str: Some(new),
                    changes: Some(vec![FileChangeDetail {
                        operation: "update".into(),
                        path,
                        old_path: None,
                        file_type: None,
                        mime_type: None,
                    }]),
                }
            }
            "MultiEdit" => {
                let path = s("file_path");
                let mut diff = String::new();
                for e in input
                    .get("edits")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                {
                    let d = replacement_diff(
                        &path,
                        str_of(&e, "old_string").unwrap_or_default(),
                        str_of(&e, "new_string").unwrap_or_default(),
                    );
                    if diff.is_empty() {
                        diff = d;
                    } else {
                        diff.push_str(
                            d.split_once("+++")
                                .and_then(|(_, r)| r.split_once('\n'))
                                .map(|(_, r)| r)
                                .unwrap_or(""),
                        );
                    }
                }
                let (a, d) = crate::util::diff_counts(&diff);
                TurnItemBody::FileChange {
                    file_name: path.clone(),
                    additions: Some(a),
                    deletions: Some(d),
                    diff_str: Some(diff),
                    old_str: None,
                    new_str: None,
                    changes: Some(vec![FileChangeDetail {
                        operation: "update".into(),
                        path,
                        old_path: None,
                        file_type: None,
                        mime_type: None,
                    }]),
                }
            }
            "Write" | "NotebookEdit" => {
                let path = if name == "Write" {
                    s("file_path")
                } else {
                    s("notebook_path")
                };
                let content = if name == "Write" {
                    s("content")
                } else {
                    s("new_source")
                };
                let diff = replacement_diff(&path, "", &content);
                TurnItemBody::FileChange {
                    file_name: path.clone(),
                    additions: Some(content.lines().count() as u32),
                    deletions: Some(0),
                    diff_str: Some(diff),
                    old_str: None,
                    new_str: Some(content),
                    changes: Some(vec![FileChangeDetail {
                        operation: if name == "Write" {
                            "add".into()
                        } else {
                            "update".into()
                        },
                        path,
                        old_path: None,
                        file_type: None,
                        mime_type: None,
                    }]),
                }
            }
            "Grep" | "Glob" => TurnItemBody::FileSearch {
                pattern: str_of(&input, "pattern").map(str::to_string),
                results: None,
            },
            "WebSearch" => TurnItemBody::WebSearch {
                patterns: str_of(&input, "query").map(|q| vec![q.to_string()]),
                results: None,
            },
            "WebFetch" => TurnItemBody::WebSearch {
                patterns: str_of(&input, "url").map(|u| vec![u.to_string()]),
                results: None,
            },
            "Task" | "Agent" => TurnItemBody::Subagent {
                subagent_id: id.to_string(),
                prompt: s("prompt"),
                progress: str_of(&input, "description").map(str::to_string),
                result: None,
            },
            _ => TurnItemBody::DynamicTool {
                tool_name: Some(name.clone()),
                input: input.clone(),
                output: None,
            },
        };
        let title = (name == "Bash")
            .then(|| str_of(&input, "description").map(str::to_string))
            .flatten();
        let key = format!("tool:{id}");
        if let Some(t) = self.active.as_mut() {
            t.tools.insert(id.to_string(), key.clone());
        }
        self.upsert(&key, Some(id), body, ItemStatus::Running, title);
    }

    fn on_user(&mut self, frame: &Value) {
        let content = frame
            .get("message")
            .and_then(|m| m.get("content"))
            .cloned()
            .unwrap_or(Value::Null);
        let Some(blocks) = content.as_array() else {
            return;
        };
        let results: Vec<&Value> = blocks
            .iter()
            .filter(|b| str_of(b, "type") == Some("tool_result"))
            .collect();
        let structured = (results.len() == 1)
            .then(|| frame.get("tool_use_result").cloned())
            .flatten();
        for r in results {
            let Some(id) = str_of(r, "tool_use_id") else {
                continue;
            };
            let is_error = r.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            let text = r.get("content").map(content_text).unwrap_or_default();
            let Some(t) = self.active.as_mut() else {
                return;
            };
            let Some(tool) = t.tools.get(id) else {
                continue;
            };
            if tool.is_empty() {
                continue;
            }
            let key = tool.clone();
            let status = if is_error {
                ItemStatus::Failed
            } else {
                ItemStatus::Completed
            };
            let structured = structured.clone();
            if let Some(item) = t.items.update(&key, |i| {
                i.status = status;
                match &mut i.body {
                    TurnItemBody::CommandExecution {
                        output,
                        output_indicates_failure,
                        ..
                    } => {
                        let out = structured
                            .as_ref()
                            .filter(|s| s.get("stdout").is_some())
                            .map(|s| {
                                let mut o = str_of(s, "stdout").unwrap_or_default().to_string();
                                let err = str_of(s, "stderr").unwrap_or_default();
                                if !err.is_empty() {
                                    if !o.is_empty() && !o.ends_with('\n') {
                                        o.push('\n');
                                    }
                                    o.push_str(err);
                                }
                                o
                            })
                            .unwrap_or_else(|| text.clone());
                        *output = Some(out);
                        *output_indicates_failure = Some(is_error);
                    }
                    TurnItemBody::FileSearch { results, .. } => {
                        *results = Some(search_results(&text))
                    }
                    TurnItemBody::WebSearch { results, .. } => {
                        let mut found = Vec::new();
                        if let Some(s) = &structured {
                            collect_urls(s, &mut found, 0);
                        }
                        *results = Some(found);
                    }
                    TurnItemBody::Subagent { result, .. } => *result = Some(text.clone()),
                    TurnItemBody::DynamicTool { output, .. } => {
                        *output = Some(r.get("content").cloned().unwrap_or(Value::Null));
                    }
                    _ => {}
                }
            }) {
                self.emitter.send(ProviderEvent::ItemUpserted(item));
            }
        }
    }

    // ---- permissions (§4.2.3, §4.4) ----

    fn on_control_request(&mut self, frame: &Value) -> Vec<Value> {
        let id = frame.get("request_id").cloned().unwrap_or(Value::Null);
        let req = frame.get("request").cloned().unwrap_or(Value::Null);
        match str_of(&req, "subtype") {
            Some("can_use_tool") => self.on_can_use_tool(id, &req).into_iter().collect(),
            // No hooks are registered, so any callback is a no-op.
            Some("hook_callback") => vec![control_success(&id, json!({}))],
            Some(other) => vec![control_error(&id, &format!("Tern does not handle {other}"))],
            None => vec![control_error(&id, "missing subtype")],
        }
    }

    fn open_request(
        &mut self,
        cli_id: Value,
        req: &Value,
        kind: RequestKind,
        body: TurnItemBody,
    ) -> Option<Value> {
        let tool_name = str_of(req, "tool_name").unwrap_or_default().to_string();
        let input = req.get("input").cloned().unwrap_or(json!({}));
        let tool_use_id = str_of(req, "tool_use_id").map(str::to_string);
        let Some(t) = self.active.as_mut() else {
            return Some(control_success(
                &cli_id,
                json!({ "behavior": "deny", "message": "User cancelled tool execution.", "interrupt": true }),
            ));
        };
        let key = format!("request:{cli_id}");
        let req_id = new_id();
        let item = t.items.upsert(
            &key,
            tool_use_id.as_deref(),
            with_request_id(body, &req_id),
            ItemStatus::Waiting,
            None,
        );
        let rr = runtime_request(
            req_id,
            &self.thread_id,
            Some(t.run_id.clone()),
            &item.id,
            cli_id.clone(),
            kind,
            &self.live_session_id,
        );
        self.pending.insert(
            rr.id.clone(),
            PendingPermission {
                cli_request_id: cli_id,
                tool_name,
                input,
                tool_use_id,
                suggestions: req.get("permission_suggestions").cloned(),
                kind,
                item_key: key,
                run_id: t.run_id.clone(),
            },
        );
        self.emitter.send(ProviderEvent::ItemUpserted(item));
        self.emitter.send(ProviderEvent::RequestOpened(rr));
        self.emitter.send(ProviderEvent::SessionUpdated {
            status: SessionStatus::Waiting,
            model: self.model.clone(),
            startup_phase: None,
            last_error: None,
        });
        None
    }

    fn on_can_use_tool(&mut self, id: Value, req: &Value) -> Option<Value> {
        self.flush(true);
        let tool = str_of(req, "tool_name").unwrap_or_default();
        let input = req.get("input").cloned().unwrap_or(json!({}));
        let tool_use_id = str_of(req, "tool_use_id").map(str::to_string);
        match tool {
            "ExitPlanMode" => {
                self.emit_plan(tool_use_id.as_deref().unwrap_or("plan"), &input);
                let mut r = json!({ "behavior": "deny", "message": PLAN_CAPTURED_MESSAGE });
                if let Some(t) = &tool_use_id {
                    r["toolUseID"] = json!(t);
                }
                Some(control_success(&id, r))
            }
            "AskUserQuestion" => {
                let questions: Vec<UserInputQuestion> = input
                    .get("questions")
                    .and_then(Value::as_array)
                    .map(|qs| {
                        qs.iter()
                            .enumerate()
                            .map(|(i, q)| {
                                let text = str_of(q, "question").unwrap_or_default().to_string();
                                UserInputQuestion {
                                    id: text.clone(),
                                    header: str_of(q, "header")
                                        .filter(|h| !h.is_empty())
                                        .map(str::to_string)
                                        .unwrap_or_else(|| format!("Question {}", i + 1)),
                                    question: text,
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
                                                    preview: str_of(o, "preview")
                                                        .map(str::to_string),
                                                })
                                                .collect()
                                        })
                                        .unwrap_or_default(),
                                    multi_select: q.get("multiSelect").and_then(Value::as_bool),
                                    allow_custom_answer: Some(true),
                                    initial_answer: None,
                                    required: None,
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let body = TurnItemBody::UserInputRequest {
                    request_id: String::new(),
                    questions,
                    question_answer: None,
                };
                self.open_request(id, req, RequestKind::UserInput, body)
            }
            _ => {
                // `auto` installs no callback in T3: the CLI's own classifier decides (A:3706-3816).
                if self.runtime_mode == RuntimeMode::Auto
                    && self.interaction_mode == InteractionMode::Default
                {
                    let mut r = json!({ "behavior": "allow", "updatedInput": input });
                    if let Some(t) = &tool_use_id {
                        r["toolUseID"] = json!(t);
                    }
                    return Some(control_success(&id, r));
                }
                let kind = request_kind(tool);
                let prompt = match tool {
                    "Bash" => str_of(&input, "command").map(str::to_string),
                    "Edit" | "MultiEdit" | "Write" | "Read" => {
                        str_of(&input, "file_path").map(str::to_string)
                    }
                    "NotebookEdit" => str_of(&input, "notebook_path").map(str::to_string),
                    "WebFetch" => str_of(&input, "url").map(str::to_string),
                    _ => str_of(req, "title")
                        .or_else(|| str_of(req, "display_name"))
                        .map(str::to_string)
                        .or_else(|| {
                            Some(format!(
                                "{tool} {}",
                                truncate_chars(&input.to_string(), 400)
                            ))
                        }),
                };
                let reason = str_of(req, "decision_reason")
                    .or_else(|| str_of(req, "description"))
                    .or_else(|| str_of(&input, "description"))
                    .map(str::to_string);
                let body = TurnItemBody::ApprovalRequest {
                    request_id: String::new(),
                    request_kind: kind,
                    prompt,
                    reason,
                    full_access_override: (self.runtime_mode == RuntimeMode::FullAccess)
                        .then_some(FullAccessOverride::ProviderCheck),
                    options: None,
                };
                self.open_request(id, req, kind, body)
            }
        }
    }

    /// Answer for a pending permission: the `control_response` frame to write.
    pub fn respond(&mut self, response: &RequestResponse) -> anyhow::Result<Value> {
        let key = response.request.id.clone();
        let p = self
            .pending
            .remove(&key)
            .ok_or_else(|| anyhow::anyhow!("Claude is no longer waiting on request {key}"))?;
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
        let mut r = if p.tool_name == "AskUserQuestion" {
            match decision {
                ApprovalDecision::Decline | ApprovalDecision::Cancel => {
                    json!({ "behavior": "deny", "message": "User cancelled tool execution." })
                }
                _ => {
                    let mut answers = serde_json::Map::new();
                    for (qid, v) in response.answers.clone().unwrap_or_default() {
                        answers.insert(qid, json!(answer_strings(&v).join(", ")));
                    }
                    let mut updated = p.input.clone();
                    if !updated.is_object() {
                        updated = json!({});
                    }
                    updated["answers"] = Value::Object(answers);
                    json!({ "behavior": "allow", "updatedInput": updated })
                }
            }
        } else {
            match decision {
                ApprovalDecision::Accept => {
                    json!({ "behavior": "allow", "updatedInput": p.input, "decisionClassification": "user_temporary" })
                }
                ApprovalDecision::AcceptForSession | ApprovalDecision::AcceptAlways => {
                    let suggestions: Vec<Value> = p
                        .suggestions
                        .as_ref()
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .map(|s| {
                                    let mut s = s.clone();
                                    s["destination"] = json!("session");
                                    s
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let updated_permissions = if suggestions.is_empty() {
                        vec![
                            json!({ "type": "addRules", "rules": [{ "toolName": p.tool_name }], "behavior": "allow", "destination": "session" }),
                        ]
                    } else {
                        suggestions
                    };
                    json!({
                        "behavior": "allow",
                        "updatedInput": p.input,
                        "decisionClassification": "user_permanent",
                        "updatedPermissions": updated_permissions,
                    })
                }
                ApprovalDecision::Decline => {
                    let message = match response
                        .feedback
                        .as_deref()
                        .filter(|f| !f.trim().is_empty())
                    {
                        Some(f) => format!("The user declined this tool call and said:\n{f}"),
                        None => "User declined tool execution.".to_string(),
                    };
                    json!({ "behavior": "deny", "message": message, "decisionClassification": "user_reject" })
                }
                ApprovalDecision::Cancel => json!({
                    "behavior": "deny",
                    "message": "User cancelled tool execution.",
                    "decisionClassification": "user_reject",
                    "interrupt": true,
                }),
            }
        };
        if let Some(t) = &p.tool_use_id {
            r["toolUseID"] = json!(t);
        }
        if self.pending.values().all(|q| q.run_id != p.run_id) && self.active.is_some() {
            self.emitter.send(ProviderEvent::SessionUpdated {
                status: SessionStatus::Running,
                model: self.model.clone(),
                startup_phase: None,
                last_error: None,
            });
        }
        Ok(control_success(&p.cli_request_id, r))
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

    /// Requests still waiting on the user, so an interrupt can cancel them.
    pub fn pending_cli_ids(&self) -> Vec<(String, Value)> {
        self.pending
            .iter()
            .map(|(k, p)| (k.clone(), p.cli_request_id.clone()))
            .collect()
    }

    // ---- turn end (§4.3.5, §4.7) ----

    fn on_result(&mut self, frame: &Value) {
        let reason = str_of(frame, "terminal_reason");
        // A steer aborts the segment in flight and the CLI goes on with the steered
        // prompt in a new segment of the same turn (`isClaudeActiveSteeringAbortResult`).
        if let Some(t) = self.active.as_mut()
            && !t.interrupting
            && t.steers > 0
            && matches!(reason, Some("aborted_streaming" | "aborted_tools"))
        {
            t.steers -= 1;
            return;
        }
        let Some(t) = self.active.as_ref() else {
            return;
        };
        let subtype = str_of(frame, "subtype").unwrap_or("success");
        let api_status = frame.get("api_error_status").and_then(Value::as_u64);
        let is_error = frame
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let errors: Vec<String> = frame
            .get("errors")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let result_text = str_of(frame, "result").unwrap_or_default().to_string();
        let usage_hit = t.rate_limited || t.synthetic_error.as_deref() == Some("rate_limit");
        let hint: Option<String> = if usage_hit && matches!(api_status, None | Some(429)) {
            Some(USAGE_LIMIT_MESSAGE.into())
        } else if t.synthetic_error.as_deref() == Some("authentication_failed")
            || api_status == Some(401)
        {
            Some(if result_text.is_empty() {
                "Claude Code is not authenticated. Run `claude auth login` and try again.".into()
            } else {
                truncate_chars(&result_text, 500)
            })
        } else {
            None
        };

        let mut status =
            if t.interrupting || matches!(reason, Some("aborted_tools" | "aborted_streaming")) {
                TerminalStatus::Interrupted
            } else if subtype == "success" {
                if matches!(api_status, Some(529 | 429 | 403))
                    || terminal_reason_error(reason, hint.as_deref()).is_some()
                    || (is_error && (hint.is_some() || api_status.is_some()))
                {
                    TerminalStatus::Failed
                } else {
                    TerminalStatus::Completed
                }
            } else {
                let text = errors.join("\n").to_ascii_lowercase();
                if text.contains("interrupt") {
                    TerminalStatus::Interrupted
                } else if text.contains("cancel") {
                    TerminalStatus::Cancelled
                } else {
                    TerminalStatus::Failed
                }
            };
        if t.interrupting {
            status = TerminalStatus::Interrupted;
        }

        let fail = (status == TerminalStatus::Failed).then(|| {
            let user_error = errors
                .iter()
                .find(|e| !e.starts_with("[ede_diagnostic]"))
                .cloned();
            if usage_hit
                && matches!(api_status, None | Some(429))
                && matches!(
                    reason,
                    None | Some("api_error" | "blocking_limit" | "completed")
                )
            {
                ProviderFailure {
                    class: FailureClass::UsageLimit,
                    message: USAGE_LIMIT_MESSAGE.into(),
                    code: Some("usage_limit".into()),
                    retryable: Some(true),
                    reset_at: t.reset_at.and_then(epoch_to_iso),
                }
            } else if api_status == Some(429) {
                ProviderFailure {
                    code: Some("429".into()),
                    ..failure(
                        FailureClass::UsageLimit,
                        "Claude API rate limit reached. Try again later.",
                    )
                }
            } else if api_status == Some(529) {
                ProviderFailure {
                    code: Some("529".into()),
                    ..failure(
                        FailureClass::ProviderError,
                        "Claude API is overloaded (529). Try again shortly.",
                    )
                }
            } else if let Some(h) = hint.clone().filter(|_| {
                api_status == Some(401)
                    || t.synthetic_error.as_deref() == Some("authentication_failed")
            }) {
                ProviderFailure {
                    code: Some("authentication_failed".into()),
                    ..failure(FailureClass::ProviderError, h)
                }
            } else {
                let message = user_error
                    .or_else(|| terminal_reason_error(reason, hint.as_deref()))
                    .or_else(|| {
                        (result_text.starts_with("API Error:") || api_status.is_some())
                            .then(|| truncate_chars(&result_text, 500))
                    })
                    .or_else(|| t.synthetic_text.clone())
                    .unwrap_or_else(|| "Claude turn failed.".to_string());
                ProviderFailure {
                    code: reason.map(str::to_string),
                    ..failure(FailureClass::ProviderError, message)
                }
            }
        });

        // Context window the CLI reports wins over the manifest (A:13916-13922).
        let model_usage = frame
            .get("modelUsage")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let models: Vec<String> = model_usage.keys().cloned().collect();
        if let Some(w) = model_usage
            .values()
            .filter_map(|m| u64_of(m, "contextWindow"))
            .max()
        {
            self.context_window = Some(w);
        }
        let usage = frame.get("usage").filter(|u| u.is_object());
        let turn_usage = match usage {
            None => TurnTokenUsage::Unavailable(TokenCounts::default()),
            Some(u) => {
                let input = u64_of(u, "input_tokens").unwrap_or(0);
                let read = u64_of(u, "cache_read_input_tokens").unwrap_or(0);
                let create = u64_of(u, "cache_creation_input_tokens").unwrap_or(0);
                let output = u64_of(u, "output_tokens").unwrap_or(0);
                let thinking = u
                    .get("output_tokens_details")
                    .and_then(|d| u64_of(d, "thinking_tokens"));
                let counts = TokenCounts {
                    input_tokens: Some(input + read + create),
                    output_tokens: Some(output),
                    cached_input_tokens: Some(read),
                    cache_creation_tokens: Some(create),
                    reasoning_tokens: thinking.map(|t| t.min(output)),
                    has_subagents: false,
                    models: (!models.is_empty()).then(|| models.clone()),
                };
                if status == TerminalStatus::Completed && subtype == "success" {
                    TurnTokenUsage::Complete(counts)
                } else {
                    TurnTokenUsage::Partial(counts)
                }
            }
        };

        // `result.result` is the reply only when nothing streamed (A:12476-12499).
        let fallback =
            (!t.emitted_text && status == TerminalStatus::Completed && !result_text.is_empty())
                .then(|| result_text.clone());
        let run_id = t.run_id.clone();
        if let Some(text) = fallback {
            let key = format!("result:{}", new_id());
            let body = TurnItemBody::AssistantMessage {
                message_id: key.clone(),
                text,
                streaming: false,
                phase: Some(AssistantPhase::FinalAnswer),
            };
            self.upsert(&key, None, body, ItemStatus::Completed, None);
        }
        let model = models.first().cloned().or(self.model.clone());
        self.emitter.send(ProviderEvent::TurnUsage {
            run_id,
            usage: turn_usage,
            model,
        });
        if let Some(u) = usage {
            self.set_context(u);
        }
        self.has_turns = true;
        self.finish(status, fail, None, false);
    }

    /// Closes the active turn: flush, settle open items and requests, terminal.
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
        if let Some(k) = t.retry_key.take() {
            t.items
                .update_quiet(&k, false, |i| i.status = ItemStatus::Completed);
        }
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
        let open_status = match status {
            TerminalStatus::Completed => ItemStatus::Completed,
            TerminalStatus::Failed if broken => ItemStatus::Failed,
            _ => ItemStatus::Interrupted,
        };
        let mut settled = t.items.take_dirty(true);
        settled.extend(t.items.settle_open(open_status));
        emit_items(&self.emitter, settled);
        self.emitter.send(ProviderEvent::TurnTerminal {
            run_id: t.run_id,
            status,
            failure: fail,
            reason,
            broken,
        });
        if !broken {
            self.emitter.send(ProviderEvent::BindingUpdated {
                native_thread_id: self.session_id.clone(),
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

    /// The process went away with a turn open (§4.3.8: interrupted if we stopped it).
    pub fn process_gone(&mut self, message: &str, expected: bool) {
        if self.active.is_some() {
            if self.is_interrupting() {
                self.finish(
                    TerminalStatus::Interrupted,
                    None,
                    Some(message.to_string()),
                    !expected,
                );
            } else {
                self.finish(
                    TerminalStatus::Failed,
                    Some(failure(FailureClass::TransportError, message)),
                    Some(message.to_string()),
                    true,
                );
            }
        }
        for (key, _) in std::mem::take(&mut self.pending) {
            self.emitter.send(ProviderEvent::RequestClosed {
                request_id: key,
                status: RequestStatus::Cancelled,
            });
        }
        if !expected {
            self.emitter.send(ProviderEvent::SessionUpdated {
                status: SessionStatus::Error,
                model: self.model.clone(),
                startup_phase: None,
                last_error: Some(message.to_string()),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_usage_uses_last_iteration() {
        let u = json!({"input_tokens": 3, "cache_creation_input_tokens": 10, "cache_read_input_tokens": 100, "output_tokens": 5,
                        "iterations": [{"type": "message", "input_tokens": 2, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 50, "output_tokens": 4}]});
        let c = context_from_usage(&u, Some(1000)).unwrap();
        assert_eq!(
            (c.used_tokens, c.input_tokens, c.cached_input_tokens),
            (56, Some(52), Some(50))
        );
    }

    #[test]
    fn grep_output_parses() {
        let r = search_results("Found 2 files\nsrc/a.rs\nsrc/b.rs:12:fn main()");
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].line, Some(12));
    }

    #[test]
    fn terminal_reasons_have_messages() {
        assert!(terminal_reason_error(Some("completed"), None).is_none());
        assert_eq!(
            terminal_reason_error(Some("prompt_too_long"), None).as_deref(),
            Some(CONTEXT_WINDOW_FULL)
        );
    }
}

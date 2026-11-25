//! `codex app-server` JSON-RPC client (spec/07 §3).
//!
//! One app-server process per Tern thread. Frames are JSON-RPC 2.0 without the
//! `"jsonrpc"` field, one per line. A reader task routes responses to waiting requests,
//! notifications and server requests to [`mapper::CodexMapper`], and flushes coalesced
//! deltas every 50 ms. Any transport failure tears the session down and settles the turn.

pub mod mapper;
pub mod models;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tern_core::*;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Notify, mpsc, oneshot};

use self::mapper::{CodexMapper, ServerReply};
use crate::discovery::{VersionProbe, find_binary, parse_version, probe_version};
use crate::process::{Frame, Proc, SpawnSpec, read_frame};
use crate::util::{Emitter, is_native_image, mime_of, read_base64, text_with_attachment_paths};
use crate::{ProviderAdapter, ProviderSessionHandle, RequestResponse, SessionConfig, TurnRequest};

pub const CLIENT_NAME: &str = "Tern";
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
/// `CODEX_APP_SERVER_REQUEST_DEADLINE` (CA:625).
const REQUEST_DEADLINE: Duration = Duration::from_secs(120);
const SHORT_TIMEOUT: Duration = Duration::from_secs(10);
/// `CODEX_STOP_CONFIRM_TIMEOUT_MS` (CA:175).
const STOP_CONFIRM_TIMEOUT: Duration = Duration::from_secs(10);
const FORCE_KILL_AFTER: Duration = Duration::from_secs(2);
const FLUSH_INTERVAL: Duration = Duration::from_millis(50);

pub fn capabilities() -> Capabilities {
    Capabilities {
        supports_interrupt: true,
        supports_active_steering: true,
        emits_proposed_plan: true,
        can_rollback_conversation: false,
        supports_model_switch_in_session: true,
        supports_runtime_mode_switch_in_session: true,
    }
}

#[derive(Debug)]
pub enum RpcError {
    Rpc { code: i64, message: String },
    Transport(String),
    Timeout(String),
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::Rpc { code, message } => write!(f, "Codex error {code}: {message}"),
            RpcError::Transport(m) => write!(f, "Codex app-server is gone: {m}"),
            RpcError::Timeout(m) => write!(f, "Codex did not answer {m} in time"),
        }
    }
}

impl std::error::Error for RpcError {}

type Pending = Mutex<HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>>;

struct Inner {
    thread_id: String,
    cwd: PathBuf,
    out: mpsc::UnboundedSender<String>,
    next_id: AtomicI64,
    pending: Pending,
    mapper: Mutex<CodexMapper>,
    proc: Option<Arc<Proc>>,
    dead: AtomicBool,
    closing: AtomicBool,
    turn_progress: Notify,
    /// Tells the writer to close stdin (used when there is no process to kill).
    writer_close: Arc<Notify>,
    version: Mutex<Option<String>>,
}

impl Inner {
    fn send_frame(&self, frame: &Value) -> Result<(), RpcError> {
        if self.dead.load(Ordering::SeqCst) {
            return Err(RpcError::Transport("the session already ended".into()));
        }
        self.out.send(frame.to_string()).map_err(|_| RpcError::Transport("stdin closed".into()))
    }

    async fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let frame = if params.is_null() {
            json!({ "id": id, "method": method })
        } else {
            json!({ "id": id, "method": method, "params": params })
        };
        if let Err(e) = self.send_frame(&frame) {
            self.pending.lock().unwrap().remove(&id);
            return Err(e);
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(RpcError::Transport("the session ended".into())),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(RpcError::Timeout(method.to_string()))
            }
        }
    }

    fn notify(&self, method: &str) -> Result<(), RpcError> {
        self.send_frame(&json!({ "method": method }))
    }

    /// Process exit, EOF, write error or a bad frame. Runs once.
    async fn teardown(self: &Arc<Self>, reason: String) {
        if self.dead.swap(true, Ordering::SeqCst) {
            return;
        }
        let message = match &self.proc {
            Some(p) => {
                let status = p.wait_exit(Duration::from_millis(250)).await;
                format!("Codex app-server {} ({reason})", p.exit_message(status))
            }
            None => format!("Codex app-server stopped ({reason})"),
        };
        for (_, tx) in self.pending.lock().unwrap().drain() {
            let _ = tx.send(Err(RpcError::Transport(message.clone())));
        }
        {
            let mut m = self.mapper.lock().unwrap();
            if self.closing.load(Ordering::SeqCst) {
                m.emitter().send(ProviderEvent::SessionUpdated {
                    status: SessionStatus::Stopped,
                    model: None,
                    startup_phase: None,
                    last_error: None,
                });
            } else {
                tracing::warn!(thread = %self.thread_id, "{message}");
                m.fail_session(&message);
            }
        }
        self.turn_progress.notify_waiters();
        self.writer_close.notify_one();
        if let Some(p) = &self.proc {
            p.kill();
        }
    }

    fn handle_line(self: &Arc<Self>, line: &str) -> Result<(), String> {
        if line.trim().is_empty() {
            return Ok(());
        }
        let msg: Value = serde_json::from_str(line).map_err(|e| format!("malformed frame from Codex: {e}"))?;
        let method = msg.get("method").and_then(Value::as_str);
        let id = msg.get("id").filter(|i| i.is_number() || i.is_string());
        match (method, id) {
            (Some(method), Some(id)) => {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                let reply = self.mapper.lock().unwrap().on_server_request(id.clone(), method, &params);
                let frame = match reply {
                    ServerReply::Result(r) => json!({ "id": id, "result": r }),
                    ServerReply::Error(code, message) => json!({ "id": id, "error": { "code": code, "message": message } }),
                    ServerReply::Deferred => return Ok(()),
                };
                let _ = self.send_frame(&frame);
            }
            (Some(method), None) if msg.get("id").is_none() => {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                self.mapper.lock().unwrap().on_notification(method, &params);
                self.turn_progress.notify_waiters();
            }
            (None, Some(_)) if msg.get("result").is_some() || msg.get("error").is_some() => {
                let id = msg["id"].as_i64().or_else(|| msg["id"].as_str().and_then(|s| s.parse().ok()));
                let tx = id.and_then(|id| self.pending.lock().unwrap().remove(&id));
                if let Some(tx) = tx {
                    let r = match msg.get("error").filter(|e| !e.is_null()) {
                        Some(e) => Err(RpcError::Rpc {
                            code: e.get("code").and_then(Value::as_i64).unwrap_or(0),
                            message: e.get("message").and_then(Value::as_str).unwrap_or_default().to_string(),
                        }),
                        None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    let _ = tx.send(r);
                }
            }
            _ => return Err(format!("unroutable frame from Codex: {}", crate::util::truncate_chars(line, 200))),
        }
        Ok(())
    }
}

/// A live `codex app-server` bound to one Tern thread.
pub struct CodexSession {
    inner: Arc<Inner>,
    config: SessionConfig,
}

impl CodexSession {
    /// Wires the JSON-RPC plumbing over any byte streams. Tests use in-memory pipes;
    /// [`CodexAdapter::open_session`] passes the child's stdio.
    pub fn over_io<R, W>(reader: R, writer: W, config: SessionConfig, events: mpsc::UnboundedSender<ProviderEvent>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::wire(reader, writer, None, config, events)
    }

    fn wire<R, W>(
        reader: R,
        mut writer: W,
        proc: Option<Arc<Proc>>,
        config: SessionConfig,
        events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let live_session_id = new_id();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        let mut mapper = CodexMapper::new(&config.thread_id, &live_session_id, Emitter(events), config.runtime_mode);
        mapper.set_model(Some(config.model_selection.model.clone()));
        let inner = Arc::new(Inner {
            thread_id: config.thread_id.clone(),
            cwd: config.cwd.clone(),
            out: out_tx,
            next_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
            mapper: Mutex::new(mapper),
            proc,
            dead: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            turn_progress: Notify::new(),
            writer_close: Arc::new(Notify::new()),
            version: Mutex::new(None),
        });

        // Writer: one frame per line. A failed write (EPIPE) ends the session.
        let w_inner = Arc::downgrade(&inner);
        let w_close = inner.writer_close.clone();
        tokio::spawn(async move {
            loop {
                let line = tokio::select! {
                    l = out_rx.recv() => l,
                    _ = w_close.notified() => None,
                };
                let Some(line) = line else {
                    let _ = writer.shutdown().await;
                    return;
                };
                let res = async {
                    writer.write_all(line.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await
                }
                .await;
                if let Err(e) = res {
                    if let Some(inner) = w_inner.upgrade() {
                        inner.teardown(format!("write failed: {e}")).await;
                    }
                    return;
                }
            }
        });

        // Line reader feeding a channel, so the routing loop can select on it safely.
        let (line_tx, mut line_rx) = mpsc::channel::<Frame>(256);
        tokio::spawn(async move {
            let mut reader = BufReader::with_capacity(64 * 1024, reader);
            let mut buf = Vec::new();
            loop {
                let f = read_frame(&mut reader, &mut buf).await;
                let end = !matches!(f, Frame::Line(_));
                if line_tx.send(f).await.is_err() || end {
                    return;
                }
            }
        });

        let r_inner = inner.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(FLUSH_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut n: u64 = 0;
            loop {
                tokio::select! {
                    frame = line_rx.recv() => {
                        let reason = match frame {
                            Some(Frame::Line(line)) => match r_inner.handle_line(&line) {
                                Ok(()) => continue,
                                Err(e) => e,
                            },
                            Some(Frame::TooLong) => "a frame exceeded 128 MiB".to_string(),
                            Some(Frame::Error(e)) => format!("read failed: {e}"),
                            Some(Frame::Eof) | None => "stdout closed".to_string(),
                        };
                        r_inner.teardown(reason).await;
                        return;
                    }
                    _ = tick.tick() => {
                        n += 1;
                        r_inner.mapper.lock().unwrap().flush(n % 8 == 0);
                    }
                }
            }
        });

        CodexSession { inner, config }
    }

    /// `initialize` -> `initialized` (§3.2).
    pub async fn handshake(&self, full: bool) -> anyhow::Result<Value> {
        let mut caps = json!({ "experimentalApi": true });
        if full {
            caps["optOutNotificationMethods"] = json!(["turn/diff/updated"]);
        }
        let params = json!({
            "clientInfo": { "name": CLIENT_NAME, "title": CLIENT_NAME, "version": env!("CARGO_PKG_VERSION") },
            "capabilities": caps,
        });
        let result = match self.inner.request("initialize", params, INITIALIZE_TIMEOUT).await {
            Ok(r) => r,
            Err(RpcError::Rpc { code: -32600, message }) if message.contains("Already initialized") => Value::Null,
            Err(e) => {
                self.retire();
                return Err(e.into());
            }
        };
        if let Some(ua) = result.get("userAgent").and_then(Value::as_str) {
            *self.inner.version.lock().unwrap() = parse_version(ua);
        }
        self.inner.notify("initialized")?;
        Ok(result)
    }

    pub fn version(&self) -> Option<String> {
        self.inner.version.lock().unwrap().clone()
    }

    fn thread_config(&self) -> Value {
        json!({
            "tools.update_plan.enabled": true,
            "mcp_optional_startup_grace_ms": 10000,
            "features.concurrent_reasoning_summaries": true,
        })
    }

    fn emitter(&self) -> Emitter {
        self.inner.mapper.lock().unwrap().emitter().clone()
    }

    /// `thread/resume` the bound thread, or `thread/start` a new one (§3.3.2-3.3.3).
    pub async fn open_thread(&self) -> anyhow::Result<String> {
        let cwd = self.inner.cwd.display().to_string();
        let model = self.config.model_selection.model.clone();
        let mut native = None;
        if let Some(id) = &self.config.resume_native_thread_id {
            let params = json!({ "threadId": id, "excludeTurns": true, "cwd": cwd, "model": model, "config": self.thread_config() });
            let mut attempt = 0;
            loop {
                attempt += 1;
                match self.inner.request("thread/resume", params.clone(), REQUEST_DEADLINE).await {
                    Ok(r) => {
                        native = r.get("thread").and_then(|t| t.get("id")).and_then(Value::as_str).map(str::to_string);
                        break;
                    }
                    Err(RpcError::Rpc { message, .. }) if attempt < 3 => {
                        let lower = message.to_ascii_lowercase();
                        if lower.contains("is archived") || lower.contains("codex unarchive") {
                            let _ = self.inner.request("thread/unarchive", json!({ "threadId": id }), SHORT_TIMEOUT).await;
                        } else if lower.contains("is closing") {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        } else if lower.contains("already has an active writer") {
                            anyhow::bail!("This Codex thread is open in another Codex client. Close it there and try again.");
                        } else {
                            tracing::warn!("thread/resume failed, starting a new Codex thread: {message}");
                            break;
                        }
                    }
                    Err(e @ RpcError::Timeout(_)) => {
                        self.retire();
                        return Err(e.into());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
        let native = match native {
            Some(n) => n,
            None => {
                let params = json!({ "cwd": cwd, "model": model, "config": self.thread_config() });
                let r = match self.inner.request("thread/start", params, REQUEST_DEADLINE).await {
                    Ok(r) => r,
                    Err(e) => {
                        if matches!(e, RpcError::Timeout(_)) {
                            self.retire();
                        }
                        return Err(e.into());
                    }
                };
                r.get("thread")
                    .and_then(|t| t.get("id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("thread/start returned no thread id"))?
                    .to_string()
            }
        };
        self.inner.mapper.lock().unwrap().native_thread_id = Some(native.clone());
        let em = self.emitter();
        em.send(ProviderEvent::BindingUpdated {
            native_thread_id: Some(native.clone()),
            status: ProviderThreadStatus::Idle,
            context_usage: None,
        });
        em.send(ProviderEvent::SessionUpdated {
            status: SessionStatus::Ready,
            model: Some(model),
            startup_phase: None,
            last_error: None,
        });
        Ok(native)
    }

    /// Kills the app-server; the reader then settles everything as broken.
    fn retire(&self) {
        match &self.inner.proc {
            Some(p) => p.kill(),
            None => {
                let inner = self.inner.clone();
                tokio::spawn(async move { inner.teardown("deadline exceeded".into()).await });
            }
        }
    }

    fn native_thread(&self) -> anyhow::Result<String> {
        self.inner
            .mapper
            .lock()
            .unwrap()
            .native_thread_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("The Codex thread is not open"))
    }

    fn build_input(turn: &TurnRequest) -> anyhow::Result<Vec<Value>> {
        let mut input = Vec::new();
        let text = text_with_attachment_paths(&turn.text, &turn.attachments);
        if !text.is_empty() {
            input.push(json!({ "type": "text", "text": text }));
        }
        for (att, path) in &turn.attachments {
            if is_native_image(att) {
                input.push(json!({ "type": "image", "url": format!("data:{};base64,{}", mime_of(att), read_base64(path)?) }));
            }
        }
        anyhow::ensure!(!input.is_empty(), "Turn requires non-empty text or attachments.");
        Ok(input)
    }

    /// `buildCodexTurnStartParams` (CA:913-1027) with the §3.6.2 policy table.
    pub fn turn_start_params(native_thread: &str, turn: &TurnRequest, input: Vec<Value>, summary: &str) -> Value {
        let (approval, reviewer, sandbox) = match turn.runtime_mode {
            RuntimeMode::ApprovalRequired => (json!("untrusted"), "user", json!({ "type": "readOnly" })),
            RuntimeMode::AutoAcceptEdits => (json!("on-request"), "user", json!({ "type": "workspaceWrite" })),
            RuntimeMode::Auto => (json!("on-request"), "auto_review", json!({ "type": "workspaceWrite" })),
            RuntimeMode::FullAccess => (
                json!({ "granular": { "mcp_elicitations": true, "request_permissions": false, "rules": false, "sandbox_approval": false, "skill_approval": false } }),
                "user",
                json!({ "type": "dangerFullAccess" }),
            ),
        };
        let sel = &turn.model_selection;
        let model = sel.model.clone();
        let effort = sel
            .option_str("reasoningEffort")
            .map(str::to_string)
            .or_else(|| (model == "gpt-6-astra").then(|| "medium".to_string()));
        let mode = match turn.interaction_mode {
            InteractionMode::Plan => "plan",
            InteractionMode::Default => "default",
        };
        let mut settings = json!({ "model": model });
        if let Some(e) = &effort {
            settings["reasoning_effort"] = json!(e);
        }
        let mut p = json!({
            "threadId": native_thread,
            "input": input,
            "cwd": turn.cwd.display().to_string(),
            "model": model,
            "summary": summary,
            "approvalsReviewer": reviewer,
            "approvalPolicy": approval,
            "sandboxPolicy": sandbox,
            "collaborationMode": { "mode": mode, "settings": settings },
        });
        if let Some(e) = effort {
            p["effort"] = json!(e);
        }
        if let Some(tier) = sel.option_str("serviceTier").filter(|t| *t != "default") {
            p["serviceTier"] = json!(tier);
        }
        if let Some(program) = sel.option_str("cyberAccessProgram") {
            p["cyberAccessProgram"] = json!(program);
        }
        p
    }

    async fn wait_started(&self, run_id: &str, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.inner.turn_progress.notified();
            {
                let m = self.inner.mapper.lock().unwrap();
                match m.turn_of(run_id) {
                    Some((_, true)) => return true,
                    None => return false,
                    _ => {}
                }
            }
            if self.inner.dead.load(Ordering::SeqCst) {
                return false;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return false;
            }
        }
    }
}

impl Drop for CodexSession {
    fn drop(&mut self) {
        if let Some(p) = &self.inner.proc {
            p.kill();
        }
    }
}

#[async_trait]
impl ProviderSessionHandle for CodexSession {
    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    async fn start_turn(&self, turn: TurnRequest) -> anyhow::Result<()> {
        let native = self.native_thread()?;
        anyhow::ensure!(
            self.inner.mapper.lock().unwrap().active_run().is_none(),
            "A Codex turn is already running on this thread"
        );
        let input = Self::build_input(&turn)?;
        // config/read: pass the user's model_reasoning_summary through, else "auto" (CA:8028-8036).
        let summary = match self
            .inner
            .request("config/read", json!({ "cwd": turn.cwd.display().to_string(), "includeLayers": false }), REQUEST_DEADLINE)
            .await
        {
            Ok(r) => r
                .get("config")
                .and_then(|c| c.get("model_reasoning_summary"))
                .and_then(Value::as_str)
                .unwrap_or("auto")
                .to_string(),
            Err(e @ RpcError::Transport(_)) => return Err(e.into()),
            Err(_) => "auto".to_string(),
        };
        let params = Self::turn_start_params(&native, &turn, input, &summary);
        {
            let mut m = self.inner.mapper.lock().unwrap();
            m.set_runtime_mode(turn.runtime_mode);
            m.set_model(Some(turn.model_selection.model.clone()));
            m.begin_turn(&turn.run_id, Some(turn.model_selection.model.clone()));
        }
        match self.inner.request("turn/start", params, REQUEST_DEADLINE).await {
            Ok(r) => {
                self.inner.mapper.lock().unwrap().on_turn_start_response(&r);
                self.inner.turn_progress.notify_waiters();
                Ok(())
            }
            Err(e) => {
                let accepted = {
                    let mut m = self.inner.mapper.lock().unwrap();
                    m.abort_turn(&turn.run_id);
                    m.turn_of(&turn.run_id).is_some()
                };
                if matches!(e, RpcError::Timeout(_)) {
                    self.retire();
                }
                // turn/started already arrived: the turn runs and will end with a terminal.
                if accepted { Ok(()) } else { Err(e.into()) }
            }
        }
    }

    async fn steer(&self, run_id: &str, text: String) -> anyhow::Result<()> {
        let native = self.native_thread()?;
        let turn_id = {
            let m = self.inner.mapper.lock().unwrap();
            m.turn_of(run_id).and_then(|(id, _)| id)
        };
        let turn_id = turn_id.ok_or_else(|| anyhow::anyhow!("No running Codex turn to steer"))?;
        let params = json!({ "threadId": native, "expectedTurnId": turn_id, "input": [{ "type": "text", "text": text }] });
        self.inner.request("turn/steer", params, REQUEST_DEADLINE).await?;
        Ok(())
    }

    async fn interrupt(&self, run_id: &str) -> anyhow::Result<()> {
        let native = self.native_thread()?;
        if self.inner.mapper.lock().unwrap().turn_of(run_id).is_none() {
            return Ok(());
        }
        self.inner.mapper.lock().unwrap().mark_interrupting(run_id);
        // A queued turn can only be interrupted once Codex started it (CA:9162-9172).
        if !self.wait_started(run_id, SHORT_TIMEOUT).await {
            if self.inner.mapper.lock().unwrap().turn_of(run_id).is_none() {
                return Ok(());
            }
            anyhow::bail!("Codex did not start the queued turn within 10 seconds");
        }
        let turn_id = self.inner.mapper.lock().unwrap().turn_of(run_id).and_then(|(id, _)| id);
        let Some(turn_id) = turn_id else { return Ok(()) };
        self.inner
            .request("turn/interrupt", json!({ "threadId": native, "turnId": turn_id }), SHORT_TIMEOUT)
            .await?;
        // No turn/completed within 10 s: finish it as interrupted ourselves (CA:9212-9218).
        let inner = self.inner.clone();
        let run = run_id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(STOP_CONFIRM_TIMEOUT).await;
            let mut m = inner.mapper.lock().unwrap();
            if m.turn_of(&run).is_some() {
                m.finish(TerminalStatus::Interrupted, None, Some("Codex did not confirm the stop".into()), false);
            }
        });
        Ok(())
    }

    async fn respond(&self, response: RequestResponse) -> anyhow::Result<()> {
        let (id, result) = self.inner.mapper.lock().unwrap().respond(&response)?;
        self.inner.send_frame(&json!({ "id": id, "result": result }))?;
        Ok(())
    }

    async fn set_model(&self, selection: ModelSelection) -> anyhow::Result<()> {
        // Codex takes model and effort per turn/start; nothing to send now.
        self.inner.mapper.lock().unwrap().set_model(Some(selection.model));
        Ok(())
    }

    async fn shutdown(&self) {
        self.inner.closing.store(true, Ordering::SeqCst);
        {
            let mut m = self.inner.mapper.lock().unwrap();
            if m.active_run().is_some() {
                m.finish(TerminalStatus::Interrupted, None, Some("session shut down".into()), false);
            }
        }
        match &self.inner.proc {
            Some(p) => p.terminate(FORCE_KILL_AFTER).await,
            None => self.inner.teardown("shut down".into()).await,
        }
    }

    fn kill(&self) {
        {
            let mut m = self.inner.mapper.lock().unwrap();
            if let Some(run) = m.active_run().map(str::to_string) {
                m.mark_interrupting(&run);
            }
        }
        match &self.inner.proc {
            Some(p) => p.kill(),
            None => {
                let inner = self.inner.clone();
                tokio::spawn(async move { inner.teardown("killed".into()).await });
            }
        }
    }
}

pub struct CodexAdapter;

impl CodexAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn spawn_app_server(bin: PathBuf, cwd: Option<PathBuf>, env: &HashMap<String, String>) -> anyhow::Result<(Arc<Proc>, tokio::process::ChildStdin, tokio::process::ChildStdout)> {
    // The originator must be set at spawn: app-server refreshes the model catalog
    // before initialize (CP:606).
    let mut env_map: HashMap<String, String> = HashMap::new();
    env_map.insert("CODEX_INTERNAL_ORIGINATOR_OVERRIDE".into(), CLIENT_NAME.into());
    env_map.extend(env.clone());
    let spec = SpawnSpec {
        program: bin,
        args: vec!["app-server".into(), "-c".into(), "thread_unload_delay_secs=0".into()],
        cwd,
        env: env_map,
        env_remove: vec![],
    };
    Ok(Proc::spawn(&spec)?)
}

fn not_found_message() -> String {
    "Codex CLI (`codex`) was not found on PATH.".into()
}

#[async_trait]
impl ProviderAdapter for CodexAdapter {
    fn driver(&self) -> Driver {
        Driver::Codex
    }

    fn instance_id(&self) -> &str {
        "codex"
    }

    async fn status(&self, binary_path: Option<PathBuf>) -> ProviderStatus {
        let mut status = ProviderStatus {
            instance_id: "codex".into(),
            driver: Driver::Codex,
            display_name: "Codex".into(),
            enabled: true,
            binary_path: None,
            version: None,
            auth: ProviderAuthState::Unknown,
            account_label: None,
            models: vec![],
            capabilities: capabilities(),
            message: None,
        };
        let Some(bin) = find_binary(binary_path.as_deref(), "codex") else {
            status.auth = ProviderAuthState::NotInstalled;
            status.message = Some(not_found_message());
            return status;
        };
        status.binary_path = Some(bin.display().to_string());
        match probe_version(&bin, Duration::from_secs(15)).await {
            VersionProbe::Ok(v) => status.version = Some(v),
            VersionProbe::Failed(e) => {
                status.message = Some(format!("Codex CLI is installed but failed to run. {e}"));
                return status;
            }
        }
        match tokio::time::timeout(Duration::from_secs(60), probe_app_server(bin)).await {
            Ok(Ok((account, models))) => {
                status.auth = account.auth;
                status.account_label = account.label;
                status.message = account.message;
                status.models = models;
            }
            Ok(Err(e)) => status.message = Some(format!("Codex app-server probe failed: {e}")),
            Err(_) => status.message = Some("Timed out while checking Codex account status.".into()),
        }
        status
    }

    async fn open_session(
        &self,
        config: SessionConfig,
        events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> anyhow::Result<Box<dyn ProviderSessionHandle>> {
        let bin = find_binary(config.binary_path.as_deref(), "codex").ok_or_else(|| anyhow::anyhow!(not_found_message()))?;
        let _ = events.send(ProviderEvent::SessionUpdated {
            status: SessionStatus::Starting,
            model: Some(config.model_selection.model.clone()),
            startup_phase: Some("Starting Codex".into()),
            last_error: None,
        });
        let (proc, stdin, stdout) = spawn_app_server(bin, Some(config.cwd.clone()), &config.env)?;
        let session = CodexSession::wire(stdout, stdin, Some(proc), config, events);
        session.handshake(true).await?;
        session.open_thread().await?;
        Ok(Box::new(session))
    }
}

/// Short-lived status process: initialize, account/read, then every model/list page.
async fn probe_app_server(bin: PathBuf) -> anyhow::Result<(models::AccountStatus, Vec<ProviderModel>)> {
    let (proc, stdin, stdout) = spawn_app_server(bin, None, &HashMap::new())?;
    let (tx, _rx) = mpsc::unbounded_channel();
    let config = SessionConfig {
        thread_id: String::new(),
        cwd: PathBuf::from("."),
        resume_native_thread_id: None,
        model_selection: ModelSelection { instance_id: "codex".into(), model: String::new(), options: vec![] },
        runtime_mode: RuntimeMode::FullAccess,
        binary_path: None,
        env: HashMap::new(),
    };
    let session = CodexSession::wire(stdout, stdin, Some(proc.clone()), config, tx);
    let result = async {
        session.handshake(false).await?;
        let account = session.inner.request("account/read", json!({}), SHORT_TIMEOUT * 3).await?;
        let account = models::map_account(&account);
        let mut models = vec![];
        if account.auth != ProviderAuthState::SignedOut {
            let mut pages = vec![];
            let mut cursor: Option<String> = None;
            for _ in 0..20 {
                let params = match &cursor {
                    Some(c) => json!({ "cursor": c }),
                    None => json!({}),
                };
                let page = session.inner.request("model/list", params, SHORT_TIMEOUT * 3).await?;
                cursor = page.get("nextCursor").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_string);
                pages.push(page);
                if cursor.is_none() {
                    break;
                }
            }
            models = models::map_models(&pages);
        }
        anyhow::Ok((account, models))
    }
    .await;
    session.inner.closing.store(true, Ordering::SeqCst);
    proc.terminate(FORCE_KILL_AFTER).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(mode: RuntimeMode, interaction: InteractionMode, options: Vec<OptionSelection>) -> TurnRequest {
        TurnRequest {
            run_id: "r".into(),
            text: "hi".into(),
            attachments: vec![],
            model_selection: ModelSelection { instance_id: "codex".into(), model: "gpt-6-luna".into(), options },
            runtime_mode: mode,
            interaction_mode: interaction,
            cwd: "/w".into(),
        }
    }

    #[test]
    fn runtime_modes_map_to_policies() {
        let input = vec![json!({"type": "text", "text": "hi"})];
        let p = CodexSession::turn_start_params("t", &turn(RuntimeMode::ApprovalRequired, InteractionMode::Default, vec![]), input.clone(), "auto");
        assert_eq!(p["approvalPolicy"], json!("untrusted"));
        assert_eq!(p["sandboxPolicy"], json!({"type": "readOnly"}));
        assert_eq!(p["approvalsReviewer"], json!("user"));
        let p = CodexSession::turn_start_params("t", &turn(RuntimeMode::Auto, InteractionMode::Plan, vec![]), input.clone(), "auto");
        assert_eq!(p["approvalsReviewer"], json!("auto_review"));
        assert_eq!(p["sandboxPolicy"], json!({"type": "workspaceWrite"}));
        assert_eq!(p["collaborationMode"]["mode"], json!("plan"));
        assert!(p.get("effort").is_none());
        let opts = vec![
            OptionSelection { id: "reasoningEffort".into(), value: OptionValue::Str("low".into()) },
            OptionSelection { id: "serviceTier".into(), value: OptionValue::Str("default".into()) },
        ];
        let p = CodexSession::turn_start_params("t", &turn(RuntimeMode::FullAccess, InteractionMode::Default, opts), input, "concise");
        assert_eq!(p["approvalPolicy"]["granular"]["mcp_elicitations"], json!(true));
        assert_eq!(p["sandboxPolicy"], json!({"type": "dangerFullAccess"}));
        assert_eq!(p["effort"], json!("low"));
        assert_eq!(p["collaborationMode"]["settings"]["reasoning_effort"], json!("low"));
        assert_eq!(p["summary"], json!("concise"));
        assert!(p.get("serviceTier").is_none());
    }
}

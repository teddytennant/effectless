//! `claude` CLI stream-json client (spec/07 §4).
//!
//! Tern drives the CLI directly instead of through the Agent SDK:
//! `claude -p --input-format stream-json --output-format stream-json --verbose
//! --include-partial-messages --permission-prompt-tool stdio ...`. Prompts and control
//! requests go in on stdin, one JSON object per line; stdout carries SDK messages plus
//! `control_request` (permission prompts) and `control_response` frames.
//!
//! One CLI process serves a thread while its policy (permission mode, thinking) stays
//! the same. A model or effort change is applied live with `set_model` and
//! `apply_flag_settings`; anything else respawns the CLI with `--resume <session id>`.

pub mod mapper;
pub mod models;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tern_core::*;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Notify, mpsc, oneshot};

use self::mapper::ClaudeMapper;
use self::models::CompiledSelection;
use crate::discovery::{VersionProbe, find_binary, probe_version};
use crate::process::{Frame, Proc, SpawnSpec, read_frame};
use crate::util::{
    Emitter, is_native_image, mime_of, read_base64, str_of, text_with_attachment_paths,
};
use crate::{ProviderAdapter, ProviderSessionHandle, RequestResponse, SessionConfig, TurnRequest};

/// `initialize` deadline (A:387).
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(300);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(10);
const INTERRUPT_CONFIRM: Duration = Duration::from_secs(10);
const EXIT_GRACE: Duration = Duration::from_secs(5);
const FLUSH_INTERVAL: Duration = Duration::from_millis(50);

/// Vars that would make the child join a parent Claude session (ClaudeHome.ts:50-63).
pub const SCRUBBED_ENV: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_HOST_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_DESKTOP_APP_VERSION",
    "CLAUDE_EFFORT",
    "CLAUDE_PID",
    "CLAUDE_AGENT_SDK_VERSION",
    "ELECTRON_RUN_AS_NODE",
];

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

/// §4.4.1: Tern runtime/interaction mode -> `--permission-mode`.
pub fn permission_mode(runtime: RuntimeMode, interaction: InteractionMode) -> &'static str {
    if interaction == InteractionMode::Plan {
        return "plan";
    }
    match runtime {
        RuntimeMode::ApprovalRequired => "default",
        RuntimeMode::AutoAcceptEdits => "acceptEdits",
        RuntimeMode::Auto => "auto",
        RuntimeMode::FullAccess => "bypassPermissions",
    }
}

/// Everything that is fixed for one CLI process.
#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    pub permission_mode: &'static str,
    pub model: String,
    pub effort: Option<String>,
    pub settings: Map<String, Value>,
}

impl Policy {
    pub fn new(
        sel: &CompiledSelection,
        runtime: RuntimeMode,
        interaction: InteractionMode,
    ) -> Self {
        let mut settings = Map::new();
        settings.insert("showThinkingSummaries".into(), json!(true));
        settings.extend(sel.settings.clone());
        Self {
            permission_mode: permission_mode(runtime, interaction),
            model: sel.api_model.clone(),
            effort: sel.effort.clone(),
            settings,
        }
    }

    /// Model, effort, fast mode and ultracode switch live; the rest needs a respawn (A:2358-2378).
    fn live_switchable_from(&self, old: &Policy) -> bool {
        let thinking = |p: &Policy| p.settings.get("alwaysThinkingEnabled").cloned();
        self.permission_mode == old.permission_mode
            && thinking(self) == thinking(old)
            && (self.effort.is_some() || old.effort.is_none())
    }
}

/// `--session-id` for a new conversation, `--resume` for one with history.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionArg {
    New(String),
    Resume(String),
}

/// The CLI argv (§4.1.5), checked against claude 2.1.332's `--help`.
pub fn build_args(policy: &Policy, session: &SessionArg) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--input-format",
        "stream-json",
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
        "--setting-sources",
        "user,project,local",
        "--thinking-display",
        "summarized",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.extend([
        "--model".into(),
        policy.model.clone(),
        "--permission-mode".into(),
        policy.permission_mode.into(),
    ]);
    if policy.permission_mode == "bypassPermissions" {
        a.push("--allow-dangerously-skip-permissions".into());
    }
    match session {
        SessionArg::New(id) => a.extend(["--session-id".into(), id.clone()]),
        SessionArg::Resume(id) => a.extend(["--resume".into(), id.clone()]),
    }
    if let Some(e) = &policy.effort {
        a.extend(["--effort".into(), e.clone()]);
    }
    a.extend([
        "--settings".into(),
        Value::Object(policy.settings.clone()).to_string(),
    ]);
    a
}

/// The stdin line for a prompt (§4.2.1).
pub fn user_message(
    text: &str,
    attachments: &[(ChatAttachment, PathBuf)],
    prefix: Option<&str>,
    uuid: &str,
    steer: bool,
) -> anyhow::Result<Value> {
    let mut text = text_with_attachment_paths(text, attachments);
    if let Some(p) = prefix
        && !text.starts_with('/')
    {
        text = format!("{p}{text}");
    }
    let images: Vec<&(ChatAttachment, PathBuf)> = attachments
        .iter()
        .filter(|(a, _)| is_native_image(a))
        .collect();
    let content = if images.is_empty() {
        json!(text)
    } else {
        let mut blocks = Vec::new();
        for (att, path) in images {
            blocks.push(json!({ "type": "image", "source": { "type": "base64", "media_type": mime_of(att), "data": read_base64(path)? } }));
        }
        blocks.push(json!({ "type": "text", "text": text }));
        Value::Array(blocks)
    };
    let mut msg = json!({ "type": "user", "message": { "role": "user", "content": content }, "parent_tool_use_id": null, "uuid": uuid });
    if steer {
        msg["priority"] = json!("next");
    }
    Ok(msg)
}

type Io = (
    Box<dyn AsyncRead + Unpin + Send>,
    Box<dyn AsyncWrite + Unpin + Send>,
);

struct Conn {
    generation: u64,
    out: mpsc::UnboundedSender<String>,
    proc: Option<Arc<Proc>>,
    policy: Policy,
    alive: Arc<AtomicBool>,
    expected_exit: Arc<AtomicBool>,
}

struct Inner {
    config: SessionConfig,
    bin: Option<PathBuf>,
    test_io: Mutex<Option<Io>>,
    mapper: Mutex<ClaudeMapper>,
    conn: tokio::sync::Mutex<Option<Conn>>,
    /// Sender of the live connection, for writes that must not wait on `conn`.
    out: Mutex<Option<(u64, mpsc::UnboundedSender<String>)>>,
    pending_ctrl: Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
    next_req: AtomicU64,
    generation: AtomicU64,
    /// A stored session to `--resume` (else `--session-id` until a turn finished).
    resumable: AtomicBool,
    turn_done: Notify,
    selection: Mutex<ModelSelection>,
}

impl Inner {
    fn write(&self, frame: &Value) -> anyhow::Result<()> {
        let out = self.out.lock().unwrap().clone();
        let (_, tx) = out.ok_or_else(|| anyhow::anyhow!("Claude is not running"))?;
        tx.send(frame.to_string())
            .map_err(|_| anyhow::anyhow!("Claude's stdin is closed"))
    }

    /// Drops the shared stdin sender so the writer can close stdin.
    fn release_out(&self, generation: u64) {
        let mut out = self.out.lock().unwrap();
        if out.as_ref().is_some_and(|(g, _)| *g == generation) {
            *out = None;
        }
    }

    async fn control(&self, request: Value, timeout: Duration) -> anyhow::Result<Value> {
        let id = format!("req_{}", self.next_req.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = oneshot::channel();
        self.pending_ctrl.lock().unwrap().insert(id.clone(), tx);
        let frame = json!({ "type": "control_request", "request_id": id, "request": request });
        if let Err(e) = self.write(&frame) {
            self.pending_ctrl.lock().unwrap().remove(&id);
            return Err(e);
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(e))) => Err(anyhow::anyhow!(
                "Claude rejected {}: {e}",
                request["subtype"]
            )),
            Ok(Err(_)) => Err(anyhow::anyhow!(
                "Claude exited before answering {}",
                request["subtype"]
            )),
            Err(_) => {
                self.pending_ctrl.lock().unwrap().remove(&id);
                Err(anyhow::anyhow!(
                    "Claude did not answer {} in time",
                    request["subtype"]
                ))
            }
        }
    }

    fn fail_controls(&self, message: &str) {
        for (_, tx) in self.pending_ctrl.lock().unwrap().drain() {
            let _ = tx.send(Err(message.to_string()));
        }
    }

    fn handle_line(
        &self,
        line: &str,
        out: &mpsc::WeakUnboundedSender<String>,
    ) -> Result<(), String> {
        if line.trim().is_empty() {
            return Ok(());
        }
        let frame: Value =
            serde_json::from_str(line).map_err(|e| format!("malformed line from Claude: {e}"))?;
        if !frame.is_object() {
            return Err("malformed line from Claude: not an object".into());
        }
        if str_of(&frame, "type") == Some("control_response") {
            let resp = frame.get("response").cloned().unwrap_or(Value::Null);
            if let Some(id) = str_of(&resp, "request_id")
                && let Some(tx) = self.pending_ctrl.lock().unwrap().remove(id)
            {
                let r = if str_of(&resp, "subtype") == Some("error") {
                    Err(str_of(&resp, "error").unwrap_or("error").to_string())
                } else {
                    Ok(resp.get("response").cloned().unwrap_or(Value::Null))
                };
                let _ = tx.send(r);
            }
            return Ok(());
        }
        let (replies, done) = {
            let mut m = self.mapper.lock().unwrap();
            let had = m.active_run().is_some();
            let replies = m.on_frame(&frame);
            (replies, had && m.active_run().is_none())
        };
        if let Some(out) = out.upgrade() {
            for r in replies {
                let _ = out.send(r.to_string());
            }
        }
        if done {
            self.turn_done.notify_waiters();
        }
        Ok(())
    }

    /// Wires reader and writer tasks for one CLI process.
    fn attach(
        self: &Arc<Self>,
        reader: Box<dyn AsyncRead + Unpin + Send>,
        mut writer: Box<dyn AsyncWrite + Unpin + Send>,
        proc: Option<Arc<Proc>>,
        policy: Policy,
    ) -> Conn {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        let alive = Arc::new(AtomicBool::new(true));
        let expected_exit = Arc::new(AtomicBool::new(false));
        *self.out.lock().unwrap() = Some((generation, out_tx.clone()));

        let w_alive = alive.clone();
        let w_proc = proc.clone();
        tokio::spawn(async move {
            while let Some(line) = out_rx.recv().await {
                let res = async {
                    writer.write_all(line.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await
                }
                .await;
                if res.is_err() {
                    // EPIPE: the reader sees EOF next and settles the turn.
                    w_alive.store(false, Ordering::SeqCst);
                    if let Some(p) = &w_proc {
                        p.kill();
                    }
                    return;
                }
            }
            // All senders gone: closing stdin tells the CLI to exit (§4.1.6).
            let _ = writer.shutdown().await;
        });

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

        let inner = self.clone();
        let r_alive = alive.clone();
        let r_expected = expected_exit.clone();
        let r_proc = proc.clone();
        let r_out = out_tx.downgrade();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(FLUSH_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let reason = loop {
                tokio::select! {
                    frame = line_rx.recv() => match frame {
                        Some(Frame::Line(line)) => {
                            if let Err(e) = inner.handle_line(&line, &r_out) {
                                break e;
                            }
                        }
                        Some(Frame::TooLong) => break "a line from Claude exceeded 128 MiB".to_string(),
                        Some(Frame::Error(e)) => break format!("read failed: {e}"),
                        Some(Frame::Eof) | None => break "stdout closed".to_string(),
                    },
                    _ = tick.tick() => inner.mapper.lock().unwrap().flush(true),
                }
            };
            r_alive.store(false, Ordering::SeqCst);
            let message = match &r_proc {
                Some(p) => {
                    p.kill();
                    let status = p.wait_exit(Duration::from_millis(250)).await;
                    format!("Claude {} ({reason})", p.exit_message(status))
                }
                None => format!("Claude stopped ({reason})"),
            };
            let current = inner.generation.load(Ordering::SeqCst) == generation;
            if current {
                *inner.out.lock().unwrap() = None;
                inner.fail_controls(&message);
            }
            // A replaced process must not touch the turn its successor is running.
            if current {
                let expected = r_expected.load(Ordering::SeqCst);
                if !expected {
                    tracing::warn!(thread = %inner.config.thread_id, "{message}");
                }
                inner
                    .mapper
                    .lock()
                    .unwrap()
                    .process_gone(&message, expected);
            }
            inner.turn_done.notify_waiters();
        });

        Conn {
            generation,
            out: out_tx,
            proc,
            policy,
            alive,
            expected_exit,
        }
    }

    fn spawn_env(&self, resuming: bool) -> (HashMap<String, String>, Vec<String>) {
        let mut env = HashMap::new();
        env.insert("CLAUDE_CODE_ENTRYPOINT".to_string(), "tern".to_string());
        if resuming && std::env::var_os("MCP_CONNECTION_NONBLOCKING").is_none() {
            env.insert("MCP_CONNECTION_NONBLOCKING".into(), "0".into());
        }
        env.extend(self.config.env.clone());
        (env, SCRUBBED_ENV.iter().map(|s| s.to_string()).collect())
    }

    /// Starts a CLI for `policy` and runs the `initialize` handshake.
    async fn connect(self: &Arc<Self>, policy: &Policy) -> anyhow::Result<Conn> {
        for attempt in 0..2 {
            let (sid, has_turns) = {
                let m = self.mapper.lock().unwrap();
                (m.session_id.clone().unwrap_or_default(), m.has_turns)
            };
            let resuming = self.resumable.load(Ordering::SeqCst) || has_turns;
            let session = if resuming {
                SessionArg::Resume(sid.clone())
            } else {
                SessionArg::New(sid.clone())
            };
            let conn = if let Some((r, w)) = self.test_io.lock().unwrap().take() {
                self.attach(r, w, None, policy.clone())
            } else {
                let bin = self.bin.clone().ok_or_else(|| {
                    anyhow::anyhow!("Claude Code CLI (`claude`) was not found on PATH.")
                })?;
                let (env, env_remove) = self.spawn_env(resuming);
                let spec = SpawnSpec {
                    program: bin,
                    args: build_args(policy, &session),
                    cwd: Some(self.config.cwd.clone()),
                    env,
                    env_remove,
                };
                let (proc, stdin, stdout) = Proc::spawn(&spec)?;
                self.attach(
                    Box::new(stdout),
                    Box::new(stdin),
                    Some(proc),
                    policy.clone(),
                )
            };
            match self
                .control(json!({ "subtype": "initialize" }), INITIALIZE_TIMEOUT)
                .await
            {
                Ok(init) => {
                    if let Some(m) = init.get("models").and_then(Value::as_array) {
                        tracing::debug!(count = m.len(), "claude initialize");
                    }
                    return Ok(conn);
                }
                Err(e) => {
                    let stderr = conn
                        .proc
                        .as_ref()
                        .map(|p| p.stderr_tail())
                        .unwrap_or_default();
                    conn.expected_exit.store(true, Ordering::SeqCst);
                    if let Some(p) = &conn.proc {
                        p.kill();
                    }
                    // The stored session is gone: start a fresh one (A:1945-1953).
                    if attempt == 0
                        && resuming
                        && stderr.contains("No conversation found with session ID")
                    {
                        let fresh = uuid::Uuid::new_v4().to_string();
                        self.resumable.store(false, Ordering::SeqCst);
                        let mut m = self.mapper.lock().unwrap();
                        m.session_id = Some(fresh.clone());
                        m.has_turns = false;
                        m.emitter().send(ProviderEvent::BindingUpdated {
                            native_thread_id: Some(fresh),
                            status: ProviderThreadStatus::Idle,
                            context_usage: None,
                        });
                        continue;
                    }
                    let excerpt = crate::process::stderr_excerpt(&stderr);
                    return Err(match excerpt {
                        Some(x) => anyhow::anyhow!("{e}: {x}"),
                        None => e,
                    });
                }
            }
        }
        unreachable!("connect loop returns")
    }

    async fn ensure(self: &Arc<Self>, policy: Policy) -> anyhow::Result<()> {
        let mut guard = self.conn.lock().await;
        if let Some(c) = guard.as_ref().filter(|c| c.alive.load(Ordering::SeqCst)) {
            if c.policy == policy {
                return Ok(());
            }
            if policy.live_switchable_from(&c.policy) {
                let live = async {
                    if policy.model != c.policy.model {
                        self.control(json!({ "subtype": "set_model", "model": policy.model }), CONTROL_TIMEOUT).await?;
                    }
                    let mut flags = json!({
                        "fastMode": policy.settings.get("fastMode").cloned().unwrap_or(Value::Null),
                        "ultracode": policy.settings.get("ultracode").cloned().unwrap_or(Value::Null),
                    });
                    if let Some(e) = &policy.effort {
                        flags["effortLevel"] = json!(e);
                    }
                    self.control(json!({ "subtype": "apply_flag_settings", "settings": flags }), CONTROL_TIMEOUT).await
                }
                .await;
                if live.is_ok() {
                    if let Some(c) = guard.as_mut() {
                        c.policy = policy;
                    }
                    return Ok(());
                }
            }
        }
        if let Some(old) = guard.take() {
            old.expected_exit.store(true, Ordering::SeqCst);
            self.release_out(old.generation);
            drop(old.out);
            if let Some(p) = &old.proc
                && p.wait_exit(EXIT_GRACE).await.is_none()
            {
                p.terminate(Duration::from_secs(2)).await;
            }
        }
        let conn = self.connect(&policy).await?;
        *guard = Some(conn);
        Ok(())
    }
}

pub struct ClaudeSession {
    inner: Arc<Inner>,
}

impl ClaudeSession {
    fn build(
        config: SessionConfig,
        bin: Option<PathBuf>,
        test_io: Option<Io>,
        events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> Self {
        let live_session_id = new_id();
        let mut mapper = ClaudeMapper::new(&config.thread_id, &live_session_id, Emitter(events));
        let (sid, resumable) = match &config.resume_native_thread_id {
            Some(id) => (id.clone(), true),
            None => (uuid::Uuid::new_v4().to_string(), false),
        };
        mapper.session_id = Some(sid);
        mapper.runtime_mode = config.runtime_mode;
        mapper.model = Some(config.model_selection.model.clone());
        let selection = config.model_selection.clone();
        Self {
            inner: Arc::new(Inner {
                config,
                bin,
                test_io: Mutex::new(test_io),
                mapper: Mutex::new(mapper),
                conn: tokio::sync::Mutex::new(None),
                out: Mutex::new(None),
                pending_ctrl: Mutex::new(HashMap::new()),
                next_req: AtomicU64::new(1),
                generation: AtomicU64::new(0),
                resumable: AtomicBool::new(resumable),
                turn_done: Notify::new(),
                selection: Mutex::new(selection),
            }),
        }
    }

    /// A session over one pre-connected transport instead of a spawned CLI. Replay tests
    /// use it; a respawn (policy change, interrupt fallback) fails.
    pub fn over_io<R, W>(
        reader: R,
        writer: W,
        config: SessionConfig,
        events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::build(
            config,
            None,
            Some((Box::new(reader), Box::new(writer))),
            events,
        )
    }

    /// Spawns the CLI now so startup errors surface at open (§4.1.6).
    pub async fn start(&self) -> anyhow::Result<()> {
        let sel = models::compile(&self.inner.config.model_selection);
        let policy = Policy::new(
            &sel,
            self.inner.config.runtime_mode,
            InteractionMode::Default,
        );
        self.inner.ensure(policy).await?;
        let (sid, model) = {
            let m = self.inner.mapper.lock().unwrap();
            (m.session_id.clone(), m.model.clone())
        };
        let em = self.inner.mapper.lock().unwrap().emitter().clone();
        em.send(ProviderEvent::BindingUpdated {
            native_thread_id: sid,
            status: ProviderThreadStatus::Idle,
            context_usage: None,
        });
        em.send(ProviderEvent::SessionUpdated {
            status: SessionStatus::Ready,
            model,
            startup_phase: None,
            last_error: None,
        });
        Ok(())
    }

    /// The native session id: allocated here, or adopted from `system/init`.
    pub fn session_id(&self) -> String {
        self.inner
            .mapper
            .lock()
            .unwrap()
            .session_id
            .clone()
            .unwrap_or_default()
    }
}

impl Drop for ClaudeSession {
    fn drop(&mut self) {
        if let Ok(guard) = self.inner.conn.try_lock()
            && let Some(p) = guard.as_ref().and_then(|c| c.proc.as_ref())
        {
            p.kill();
        }
    }
}

#[async_trait]
impl ProviderSessionHandle for ClaudeSession {
    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    async fn start_turn(&self, turn: TurnRequest) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.inner.mapper.lock().unwrap().active_run().is_none(),
            "A Claude turn is already running on this thread"
        );
        let compiled = models::compile(&turn.model_selection);
        let policy = Policy::new(&compiled, turn.runtime_mode, turn.interaction_mode);
        {
            let mut m = self.inner.mapper.lock().unwrap();
            m.runtime_mode = turn.runtime_mode;
            m.interaction_mode = turn.interaction_mode;
            if compiled.context_window.is_some() {
                m.context_window = compiled.context_window;
            }
        }
        *self.inner.selection.lock().unwrap() = turn.model_selection.clone();
        self.inner.ensure(policy).await?;
        let prompt_uuid = uuid::Uuid::new_v4().to_string();
        let msg = user_message(
            &turn.text,
            &turn.attachments,
            compiled.prompt_prefix,
            &prompt_uuid,
            false,
        )?;
        self.inner
            .mapper
            .lock()
            .unwrap()
            .begin_turn(&turn.run_id, &prompt_uuid);
        if let Err(e) = self.inner.write(&msg) {
            // Never reached Claude: no terminal follows an Err.
            self.inner.mapper.lock().unwrap().abort_turn(&turn.run_id);
            return Err(e);
        }
        Ok(())
    }

    async fn steer(&self, run_id: &str, text: String) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.inner.mapper.lock().unwrap().active_run() == Some(run_id),
            "No running Claude turn to steer"
        );
        let msg = user_message(&text, &[], None, &uuid::Uuid::new_v4().to_string(), true)?;
        self.inner.write(&msg)?;
        self.inner.mapper.lock().unwrap().note_steer(run_id);
        Ok(())
    }

    async fn interrupt(&self, run_id: &str) -> anyhow::Result<()> {
        let pending = {
            let mut m = self.inner.mapper.lock().unwrap();
            if m.active_run() != Some(run_id) {
                return Ok(());
            }
            m.mark_interrupting(run_id);
            m.pending_cli_ids()
        };
        // Questions and approvals still open are answered "cancel" so the CLI unblocks.
        for (tern_id, _) in pending {
            let req = RuntimeRequest {
                id: tern_id,
                thread_id: self.inner.config.thread_id.clone(),
                run_id: Some(run_id.to_string()),
                turn_item_id: String::new(),
                native_request_id: None,
                kind: RequestKind::Permission,
                status: RequestStatus::Pending,
                live_session_id: None,
                created_at: now(),
                resolved_at: None,
                decision: None,
                feedback: None,
                answers: None,
            };
            let resp = RequestResponse {
                request: req,
                decision: Some(ApprovalDecision::Cancel),
                feedback: None,
                answers: None,
            };
            let mut m = self.inner.mapper.lock().unwrap();
            if let Ok(frame) = m.respond(&resp) {
                let _ = self.inner.write(&frame);
                m.emitter().send(ProviderEvent::RequestClosed {
                    request_id: resp.request.id.clone(),
                    status: RequestStatus::Cancelled,
                });
            }
        }
        let sent = self
            .inner
            .control(json!({ "subtype": "interrupt" }), CONTROL_TIMEOUT)
            .await;
        // Keep the process warm when the CLI ends the turn itself; otherwise kill it and
        // let the next turn resume (A:14476-14502).
        let inner = self.inner.clone();
        let run = run_id.to_string();
        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now()
                + if sent.is_ok() {
                    INTERRUPT_CONFIRM
                } else {
                    Duration::ZERO
                };
            loop {
                let notified = inner.turn_done.notified();
                if inner.mapper.lock().unwrap().active_run() != Some(run.as_str()) {
                    return;
                }
                if tokio::time::timeout_at(deadline, notified).await.is_err() {
                    break;
                }
            }
            let guard = inner.conn.lock().await;
            if let Some(c) = guard.as_ref() {
                c.expected_exit.store(true, Ordering::SeqCst);
                match &c.proc {
                    Some(p) => p.kill(),
                    None => {
                        drop(guard);
                        inner
                            .mapper
                            .lock()
                            .unwrap()
                            .process_gone("Claude did not confirm the stop", true);
                    }
                }
            }
        });
        Ok(())
    }

    async fn respond(&self, response: RequestResponse) -> anyhow::Result<()> {
        let frame = self.inner.mapper.lock().unwrap().respond(&response)?;
        self.inner.write(&frame)
    }

    async fn set_model(&self, selection: ModelSelection) -> anyhow::Result<()> {
        *self.inner.selection.lock().unwrap() = selection.clone();
        let idle = self.inner.mapper.lock().unwrap().active_run().is_none();
        let current = self
            .inner
            .conn
            .lock()
            .await
            .as_ref()
            .filter(|c| c.alive.load(Ordering::SeqCst))
            .map(|c| c.policy.clone());
        // Applied live when the process is idle and the change allows it; otherwise the
        // next turn's policy check respawns.
        if let (true, Some(old)) = (idle, current) {
            let compiled = models::compile(&selection);
            let mut policy = Policy::new(
                &compiled,
                self.inner.config.runtime_mode,
                InteractionMode::Default,
            );
            policy.permission_mode = old.permission_mode;
            if policy.live_switchable_from(&old) {
                self.inner.ensure(policy).await?;
            }
        }
        Ok(())
    }

    async fn shutdown(&self) {
        {
            let mut m = self.inner.mapper.lock().unwrap();
            if m.active_run().is_some() {
                m.finish(
                    TerminalStatus::Interrupted,
                    None,
                    Some("session shut down".into()),
                    false,
                );
            }
        }
        let conn = self.inner.conn.lock().await.take();
        *self.inner.out.lock().unwrap() = None;
        if let Some(c) = conn {
            c.expected_exit.store(true, Ordering::SeqCst);
            drop(c.out);
            if let Some(p) = &c.proc
                && p.wait_exit(EXIT_GRACE).await.is_none()
            {
                p.terminate(Duration::from_secs(2)).await;
            }
        }
        let em = self.inner.mapper.lock().unwrap().emitter().clone();
        em.send(ProviderEvent::SessionUpdated {
            status: SessionStatus::Stopped,
            model: None,
            startup_phase: None,
            last_error: None,
        });
    }

    fn kill(&self) {
        {
            let mut m = self.inner.mapper.lock().unwrap();
            if let Some(run) = m.active_run().map(str::to_string) {
                m.mark_interrupting(&run);
            }
        }
        if let Ok(guard) = self.inner.conn.try_lock()
            && let Some(p) = guard.as_ref().and_then(|c| c.proc.as_ref())
        {
            p.kill();
            return;
        }
        // Spawning or test transport: settle synchronously.
        self.inner
            .mapper
            .lock()
            .unwrap()
            .process_gone("Claude was force stopped", false);
    }
}

pub struct ClaudeAdapter;

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn not_found() -> String {
    "Claude Code CLI (`claude`) was not found on PATH.".into()
}

fn title_case(s: &str) -> String {
    s.split([' ', '_', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `formatClaudeSubscriptionAuthLabel` (ClaudeProvider.ts:114-179).
pub fn subscription_label(subscription: &str) -> String {
    let norm: String = subscription
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .collect();
    let base = match norm.as_str() {
        "claudemaxsubscription" | "claudemax5xsubscription" | "max" | "maxplan" | "max5" => {
            "Max".to_string()
        }
        "claudemax20xsubscription" | "max20" => "Ultra".to_string(),
        "claudeenterprisesubscription" | "enterprise" => "Enterprise".to_string(),
        "claudeteamsubscription" | "team" => "Team".to_string(),
        "claudeprosubscription" | "pro" => "Pro".to_string(),
        "claudefreesubscription" | "free" => "Free".to_string(),
        _ => title_case(subscription),
    };
    let n: String = base
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .collect();
    match (n.starts_with("claude"), n.ends_with("subscription")) {
        (true, true) => base,
        (true, false) => format!("{base} Subscription"),
        (false, true) => format!("Claude {base}"),
        (false, false) => format!("Claude {base} Subscription"),
    }
}

/// `claudeAuthStatus` + `claudeAuthMetadata` from the `initialize` reply's `account`.
pub fn map_account(account: &Value) -> (ProviderAuthState, Option<String>) {
    let s = |k: &str| str_of(account, k).filter(|v| !v.is_empty());
    let api_key = s("apiKeySource").is_some_and(|k| k != "none")
        || matches!(
            s("tokenSource")
                .map(|t| t.to_lowercase().replace([' ', '_', '-'], ""))
                .as_deref(),
            Some("apikey" | "anthropicapikey" | "anthropicauthtoken")
        );
    let provider = s("apiProvider");
    let authed = match provider {
        Some(p) if p != "firstParty" => true,
        _ => {
            s("tokenSource") != Some("none")
                || api_key
                || s("email").is_some()
                || s("subscriptionType").is_some()
        }
    };
    if !authed {
        return (ProviderAuthState::SignedOut, None);
    }
    let label = if provider == Some("bedrock") {
        Some("Amazon Bedrock".to_string())
    } else if api_key {
        Some("Claude API Key".to_string())
    } else {
        s("subscriptionType").map(subscription_label)
    };
    (ProviderAuthState::SignedIn, label)
}

/// Starts a throwaway CLI, sends `initialize`, reads `account` (ClaudeProvider.ts:271-306).
async fn probe_account(bin: PathBuf) -> anyhow::Result<Value> {
    let args: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--input-format",
        "stream-json",
        "--setting-sources",
        "user,project,local",
        "--settings",
        r#"{"disableAllHooks":true}"#,
        "--strict-mcp-config",
        "--no-session-persistence",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut env = HashMap::new();
    env.insert("CLAUDE_CODE_ENTRYPOINT".to_string(), "tern".to_string());
    env.insert(
        "ENABLE_CLAUDEAI_MCP_SERVERS".to_string(),
        "false".to_string(),
    );
    let spec = SpawnSpec {
        program: bin,
        args,
        cwd: std::env::temp_dir().into(),
        env,
        env_remove: SCRUBBED_ENV.iter().map(|s| s.to_string()).collect(),
    };
    let (proc, mut stdin, stdout) = Proc::spawn(&spec)?;
    let line = json!({ "type": "control_request", "request_id": "probe_1", "request": { "subtype": "initialize" } });
    stdin.write_all(format!("{line}\n").as_bytes()).await?;
    stdin.flush().await?;
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::new();
    let result = loop {
        match read_frame(&mut reader, &mut buf).await {
            Frame::Line(l) => {
                let Ok(v) = serde_json::from_str::<Value>(&l) else {
                    continue;
                };
                if str_of(&v, "type") == Some("control_response") {
                    let r = &v["response"];
                    if str_of(r, "subtype") == Some("error") {
                        break Err(anyhow::anyhow!(
                            str_of(r, "error")
                                .unwrap_or("initialize failed")
                                .to_string()
                        ));
                    }
                    break Ok(r.get("response").cloned().unwrap_or(Value::Null));
                }
            }
            _ => {
                let status = proc.wait_exit(Duration::from_millis(250)).await;
                break Err(anyhow::anyhow!("Claude {}", proc.exit_message(status)));
            }
        }
    };
    drop(stdin);
    if proc.wait_exit(Duration::from_secs(3)).await.is_none() {
        proc.terminate(Duration::from_secs(2)).await;
    }
    result
}

#[async_trait]
impl ProviderAdapter for ClaudeAdapter {
    fn driver(&self) -> Driver {
        Driver::Claude
    }

    fn instance_id(&self) -> &str {
        "claudeAgent"
    }

    async fn status(&self, binary_path: Option<PathBuf>) -> ProviderStatus {
        let mut status = ProviderStatus {
            instance_id: "claudeAgent".into(),
            driver: Driver::Claude,
            display_name: "Claude".into(),
            enabled: true,
            binary_path: None,
            version: None,
            auth: ProviderAuthState::Unknown,
            account_label: None,
            models: models::models(None),
            capabilities: capabilities(),
            message: None,
        };
        let Some(bin) = find_binary(binary_path.as_deref(), "claude") else {
            status.auth = ProviderAuthState::NotInstalled;
            status.message = Some(not_found());
            return status;
        };
        status.binary_path = Some(bin.display().to_string());
        match probe_version(&bin, Duration::from_secs(15)).await {
            VersionProbe::Ok(v) => {
                status.models = models::models(Some(&v));
                status.version = Some(v);
            }
            VersionProbe::Failed(e) => {
                status.message = Some(format!(
                    "Claude Code CLI is installed but failed to run. {e}"
                ));
                return status;
            }
        }
        match tokio::time::timeout(Duration::from_secs(60), probe_account(bin)).await {
            Ok(Ok(init)) => {
                let account = init.get("account").cloned().unwrap_or(Value::Null);
                let (auth, label) = map_account(&account);
                status.auth = auth;
                status.account_label = label;
                if auth == ProviderAuthState::SignedOut {
                    status.message = Some(
                        "Claude Code is not authenticated. Run `claude auth login` and try again."
                            .into(),
                    );
                }
            }
            Ok(Err(e)) => {
                status.message = Some(format!("Could not verify Claude authentication: {e}"))
            }
            Err(_) => {
                status.message =
                    Some("Timed out after 60s while checking Claude account status.".into())
            }
        }
        status
    }

    async fn open_session(
        &self,
        config: SessionConfig,
        events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> anyhow::Result<Box<dyn ProviderSessionHandle>> {
        let bin = find_binary(config.binary_path.as_deref(), "claude")
            .ok_or_else(|| anyhow::anyhow!(not_found()))?;
        let _ = events.send(ProviderEvent::SessionUpdated {
            status: SessionStatus::Starting,
            model: Some(config.model_selection.model.clone()),
            startup_phase: Some("Starting Claude".into()),
            last_error: None,
        });
        let session = ClaudeSession::build(config, Some(bin), None, events);
        session.start().await?;
        Ok(Box::new(session))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_matches_cli_flags() {
        let sel = models::compile(&ModelSelection {
            instance_id: "claudeAgent".into(),
            model: "claude-haiku-5-5".into(),
            options: vec![OptionSelection {
                id: "effort".into(),
                value: OptionValue::Str("low".into()),
            }],
        });
        let p = Policy::new(&sel, RuntimeMode::FullAccess, InteractionMode::Default);
        let a = build_args(&p, &SessionArg::New("s".into()));
        let s = a.join(" ");
        assert!(s.starts_with("-p --output-format stream-json --verbose --input-format stream-json --include-partial-messages --permission-prompt-tool stdio"));
        assert!(s.contains("--model claude-haiku-5-5 --permission-mode bypassPermissions --allow-dangerously-skip-permissions --session-id s --effort low"));
        assert!(s.contains("--setting-sources user,project,local"));
        let p = Policy::new(&sel, RuntimeMode::FullAccess, InteractionMode::Plan);
        assert!(
            build_args(&p, &SessionArg::Resume("s".into()))
                .join(" ")
                .contains("--permission-mode plan --resume s")
        );
    }

    #[test]
    fn modes_map() {
        assert_eq!(
            permission_mode(RuntimeMode::ApprovalRequired, InteractionMode::Default),
            "default"
        );
        assert_eq!(
            permission_mode(RuntimeMode::AutoAcceptEdits, InteractionMode::Default),
            "acceptEdits"
        );
        assert_eq!(
            permission_mode(RuntimeMode::Auto, InteractionMode::Default),
            "auto"
        );
        assert_eq!(
            permission_mode(RuntimeMode::FullAccess, InteractionMode::Plan),
            "plan"
        );
    }

    #[test]
    fn user_lines() {
        let m = user_message("hi", &[], Some("Ultrathink:\n"), "u", false).unwrap();
        assert_eq!(m["message"]["content"], json!("Ultrathink:\nhi"));
        assert_eq!(m["uuid"], json!("u"));
        assert!(m.get("priority").is_none());
        let m = user_message("/compact", &[], Some("Ultrathink:\n"), "u", true).unwrap();
        assert_eq!(m["message"]["content"], json!("/compact"));
        assert_eq!(m["priority"], json!("next"));
    }

    #[test]
    fn accounts() {
        let (a, l) = map_account(
            &json!({"email": "x@y.z", "subscriptionType": "Claude Ultra", "apiProvider": "firstParty"}),
        );
        assert_eq!(
            (a, l.as_deref()),
            (
                ProviderAuthState::SignedIn,
                Some("Claude Ultra Subscription")
            )
        );
        let (_, l) = map_account(&json!({"subscriptionType": "max", "tokenSource": "claude.ai"}));
        assert_eq!(l.as_deref(), Some("Claude Max Subscription"));
        assert_eq!(
            map_account(&json!({"tokenSource": "none"})).0,
            ProviderAuthState::SignedOut
        );
        assert_eq!(
            map_account(&json!({"tokenSource": "none", "apiKeySource": "ANTHROPIC_API_KEY"}))
                .1
                .as_deref(),
            Some("Claude API Key")
        );
    }

    #[test]
    fn live_switch_rules() {
        let c = |effort: Option<&str>| CompiledSelection {
            api_model: "m".into(),
            effort: effort.map(str::to_string),
            ..Default::default()
        };
        let a = Policy::new(
            &c(Some("low")),
            RuntimeMode::FullAccess,
            InteractionMode::Default,
        );
        let b = Policy::new(
            &c(Some("high")),
            RuntimeMode::FullAccess,
            InteractionMode::Default,
        );
        assert!(b.live_switchable_from(&a));
        let none = Policy::new(&c(None), RuntimeMode::FullAccess, InteractionMode::Default);
        assert!(!none.live_switchable_from(&a));
        let plan = Policy::new(
            &c(Some("low")),
            RuntimeMode::FullAccess,
            InteractionMode::Plan,
        );
        assert!(!plan.live_switchable_from(&a));
    }
}

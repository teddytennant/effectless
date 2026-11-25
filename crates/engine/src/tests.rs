//! Engine tests. Everything runs against fakes: no git, no provider CLI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use parking_lot::Mutex;
use tern_core::settings::Settings;
use tern_core::*;
use tern_providers::{
    ProviderAdapter, ProviderSessionHandle, RequestResponse, SessionConfig, TurnRequest,
};
use tern_store::{Db, ITEM_KINDS, Store, item_kind};
use tokio::sync::mpsc;

use crate::backend::{Backends, BoxFuture, GitBackend, SettingsSource, Timing, TitleGenerator};
use crate::*;

// ---------------------------------------------------------------------------------------------
// fakes

#[derive(Default)]
struct FakeState {
    events: Option<mpsc::UnboundedSender<ProviderEvent>>,
    calls: Vec<String>,
    opens: usize,
    configs: Vec<SessionConfig>,
    turns: Vec<TurnRequest>,
    responses: Vec<RequestResponse>,
    fail_start: bool,
    caps: Capabilities,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<FakeState>>);

impl Fake {
    fn emit(&self, ev: ProviderEvent) {
        let tx = self.0.lock().events.clone().expect("no session open");
        tx.send(ev).unwrap();
    }
    fn calls(&self) -> Vec<String> {
        self.0.lock().calls.clone()
    }
    fn called(&self, prefix: &str) -> bool {
        self.calls().iter().any(|c| c.starts_with(prefix))
    }
}

struct FakeAdapter(Fake);
struct FakeSession(Fake);

#[async_trait]
impl ProviderAdapter for FakeAdapter {
    fn driver(&self) -> Driver {
        Driver::Codex
    }
    fn instance_id(&self) -> &str {
        "codex"
    }
    async fn status(&self, _binary_path: Option<PathBuf>) -> ProviderStatus {
        ProviderStatus {
            instance_id: "codex".into(),
            driver: Driver::Codex,
            display_name: "Codex".into(),
            enabled: true,
            binary_path: None,
            version: Some("0.0.0".into()),
            auth: ProviderAuthState::SignedIn,
            account_label: None,
            models: vec![],
            capabilities: Capabilities::default(),
            message: None,
        }
    }
    async fn open_session(
        &self,
        config: SessionConfig,
        events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> anyhow::Result<Box<dyn ProviderSessionHandle>> {
        let mut st = self.0.0.lock();
        st.opens += 1;
        st.events = Some(events);
        st.calls.push("open".into());
        st.configs.push(config);
        Ok(Box::new(FakeSession(self.0.clone())))
    }
}

#[async_trait]
impl ProviderSessionHandle for FakeSession {
    fn capabilities(&self) -> Capabilities {
        self.0.0.lock().caps.clone()
    }
    async fn start_turn(&self, turn: TurnRequest) -> anyhow::Result<()> {
        let mut st = self.0.0.lock();
        st.calls.push(format!("start:{}", turn.run_id));
        st.turns.push(turn);
        if st.fail_start {
            anyhow::bail!("codex app-server exited during turn/start");
        }
        Ok(())
    }
    async fn steer(&self, run_id: &str, text: String) -> anyhow::Result<()> {
        self.0.0.lock().calls.push(format!("steer:{run_id}:{text}"));
        Ok(())
    }
    async fn interrupt(&self, run_id: &str) -> anyhow::Result<()> {
        self.0.0.lock().calls.push(format!("interrupt:{run_id}"));
        Ok(())
    }
    async fn respond(&self, response: RequestResponse) -> anyhow::Result<()> {
        let mut st = self.0.0.lock();
        st.calls.push(format!("respond:{}", response.request.id));
        st.responses.push(response);
        Ok(())
    }
    async fn set_model(&self, selection: ModelSelection) -> anyhow::Result<()> {
        self.0
            .0
            .lock()
            .calls
            .push(format!("set_model:{}", selection.model));
        Ok(())
    }
    async fn shutdown(&self) {
        self.0.0.lock().calls.push("shutdown".into());
    }
    fn kill(&self) {
        self.0.0.lock().calls.push("kill".into());
    }
}

#[derive(Default)]
struct FakeGit {
    calls: Mutex<Vec<String>>,
}

impl GitBackend for FakeGit {
    fn repo_root(&self, path: &Path) -> Option<PathBuf> {
        Some(path.to_path_buf())
    }
    fn capture(&self, _cwd: &Path, _thread_id: &str, ordinal: u32) -> Result<String> {
        self.calls.lock().push(format!("capture:{ordinal}"));
        Ok(format!("ref-{ordinal}"))
    }
    fn summarize(&self, _cwd: &Path, from: &str, to: &str) -> Result<Vec<CheckpointFileSummary>> {
        self.calls.lock().push(format!("summarize:{from}:{to}"));
        Ok(vec![CheckpointFileSummary {
            path: "src/lib.rs".into(),
            kind: "modified".into(),
            additions: 5,
            deletions: 2,
        }])
    }
    fn restore(&self, _cwd: &Path, git_ref: &str) -> Result<()> {
        self.calls.lock().push(format!("restore:{git_ref}"));
        Ok(())
    }
    fn delete_all(&self, _cwd: &Path, thread_id: &str) -> Result<()> {
        self.calls.lock().push(format!("delete_all:{thread_id}"));
        Ok(())
    }
    fn create_worktree(
        &self,
        _repo: &Path,
        root: &Path,
        base: &str,
        branch: &str,
        _o: bool,
    ) -> Result<PathBuf> {
        self.calls.lock().push(format!("worktree:{base}:{branch}"));
        Ok(root.join(branch.replace('/', "-")))
    }
    fn unique_branch_name(&self, _repo: &Path, prefix: &str, slug: &str) -> Result<String> {
        Ok(format!("{prefix}/{slug}"))
    }
}

struct FakeTitles(Mutex<Option<String>>);

impl TitleGenerator for FakeTitles {
    fn thread_title(
        &self,
        _m: ModelSelection,
        _c: String,
        _cwd: PathBuf,
    ) -> BoxFuture<Result<String>> {
        let t = self.0.lock().clone();
        Box::pin(async move { t.ok_or_else(|| anyhow::anyhow!("codex exec failed\nrate limited")) })
    }
}

// ---------------------------------------------------------------------------------------------
// harness

struct H {
    engine: EngineHandle,
    fake: Fake,
    git: Arc<FakeGit>,
    updates: Arc<Mutex<Vec<Update>>>,
    project_id: String,
}

fn fast_timing() -> Timing {
    Timing {
        interrupt_timeout: Duration::from_millis(150),
        idle_timeout: Duration::from_secs(60),
        idle_timeout_no_resume: Duration::from_secs(60),
        publish_interval: Duration::from_millis(10),
        baseline_budget: Duration::from_secs(2),
        title_retries: 0,
        title_retry_base: Duration::from_millis(5),
    }
}

fn harness_with(store: Store, title: Option<&str>) -> H {
    let fake = Fake::default();
    let git = Arc::new(FakeGit::default());
    let backends = Backends {
        adapters: vec![Arc::new(FakeAdapter(fake.clone()))],
        git: git.clone(),
        titles: Arc::new(FakeTitles(Mutex::new(title.map(Into::into)))),
        settings: SettingsSource::Fixed(Arc::new(Settings::default())),
        timing: fast_timing(),
    };
    let updates = Arc::new(Mutex::new(Vec::new()));
    let u = updates.clone();
    let paths = Paths {
        home: std::env::temp_dir().join(format!("tern-engine-test-{}", new_id())),
    };
    let engine = EngineHandle::start_with(
        paths,
        store,
        backends,
        Box::new(move |up| u.lock().push(up)),
    )
    .unwrap();
    let mut h = H {
        engine,
        fake,
        git,
        updates,
        project_id: String::new(),
    };
    if h.engine.snapshot().projects.is_empty() {
        h.engine.send(Command::AddProject {
            path: "/work/proj/".into(),
        });
    }
    h.project_id = h.engine.snapshot().projects[0].id.clone();
    h
}

fn harness() -> H {
    harness_with(Store::open_in_memory().unwrap(), Some("Fake title"))
}

fn msg(text: &str) -> OutgoingMessage {
    OutgoingMessage {
        text: text.into(),
        attachments: vec![],
        source_plan_id: None,
    }
}

fn codex() -> ModelSelection {
    ModelSelection {
        instance_id: "codex".into(),
        model: "gpt-6-astra".into(),
        options: vec![],
    }
}

impl H {
    fn wait<T>(&self, what: &str, f: impl Fn(&EngineHandle) -> Option<T>) -> T {
        let start = Instant::now();
        loop {
            if let Some(v) = f(&self.engine) {
                return v;
            }
            if start.elapsed() > Duration::from_secs(5) {
                panic!("timed out waiting for {what}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn launch(&self, text: Option<&str>) -> String {
        let id = new_id();
        self.engine.send(Command::LaunchThread {
            thread_id: id.clone(),
            project_id: self.project_id.clone(),
            model_selection: codex(),
            runtime_mode: RuntimeMode::FullAccess,
            interaction_mode: InteractionMode::Default,
            workspace: WorkspaceStrategy::Root { branch: None },
            message: text.map(msg),
        });
        id
    }

    fn detail(&self, id: &str) -> ThreadDetail {
        self.engine.thread(id).unwrap()
    }

    fn shell(&self, id: &str) -> ThreadShell {
        self.engine.snapshot().thread(id).unwrap().clone()
    }

    /// Launch with a message and wait until the provider accepted the turn.
    fn running(&self, text: &str) -> (String, String) {
        let id = self.launch(Some(text));
        let run = self.wait("run running", |e| {
            let d = e.thread(&id)?;
            let r = d.runs.last()?;
            (r.status == RunStatus::Running).then(|| r.id.clone())
        });
        (id, run)
    }

    fn run_status(&self, thread: &str, run: &str) -> RunStatus {
        self.detail(thread)
            .runs
            .iter()
            .find(|r| r.id == run)
            .unwrap()
            .status
    }

    fn wait_run(&self, thread: &str, run: &str, status: RunStatus) {
        self.wait(&format!("run {status:?}"), |e| {
            let d = e.thread(thread)?;
            (d.runs.iter().find(|r| r.id == run)?.status == status).then_some(())
        });
    }

    fn item(
        &self,
        thread: &str,
        run: &str,
        id: &str,
        status: ItemStatus,
        body: TurnItemBody,
    ) -> TurnItem {
        let t = now();
        TurnItem {
            id: id.into(),
            thread_id: thread.into(),
            run_id: Some(run.into()),
            native_item_id: Some(format!("n-{id}")),
            parent_item_id: None,
            ordinal: 0,
            status,
            title: None,
            started_at: Some(t),
            completed_at: None,
            updated_at: t,
            body,
        }
    }

    fn terminal(&self, run: &str, status: TerminalStatus, failure: Option<ProviderFailure>) {
        self.fake.emit(ProviderEvent::TurnTerminal {
            run_id: run.into(),
            status,
            failure,
            reason: None,
            broken: false,
        });
    }

    fn errors(&self) -> Vec<String> {
        self.updates
            .lock()
            .iter()
            .filter_map(|u| match u {
                Update::Error { message } => Some(message.clone()),
                _ => None,
            })
            .collect()
    }
}

fn assistant(text: &str, streaming: bool) -> TurnItemBody {
    TurnItemBody::AssistantMessage {
        message_id: "m1".into(),
        text: text.into(),
        streaming,
        phase: None,
    }
}

// ---------------------------------------------------------------------------------------------
// state machine

#[test]
fn send_stream_complete_and_checkpoint() {
    let h = harness();
    let (id, run) = h.running("Fix the login redirect loop");
    let d = h.detail(&id);
    assert_eq!(d.runs[0].ordinal, 1);
    assert!(matches!(
        d.items[0].body,
        TurnItemBody::UserMessage {
            input_intent: InputIntent::TurnStart,
            ..
        }
    ));
    assert!(h.fake.called(&format!("start:{run}")));
    assert_eq!(h.fake.0.lock().turns[0].text, "Fix the login redirect loop");
    assert_eq!(h.fake.0.lock().configs[0].cwd, PathBuf::from("/work/proj"));

    // baseline before run 1
    h.wait("baseline", |e| {
        e.thread(&id)?
            .checkpoints
            .iter()
            .any(|c| c.ordinal == 0)
            .then_some(())
    });

    // title: seed first, then the generated one
    h.wait("title", |e| {
        (e.thread(&id)?.thread.title == "Fake title").then_some(())
    });
    assert!(h.detail(&id).thread.title_generation_started_at.is_none());

    h.fake.emit(ProviderEvent::TurnStarted {
        run_id: run.clone(),
        native_turn_id: Some("turn-1".into()),
    });
    h.fake.emit(ProviderEvent::ItemUpserted(h.item(
        &id,
        &run,
        "a1",
        ItemStatus::Running,
        assistant("Look", true),
    )));
    h.fake.emit(ProviderEvent::ItemUpserted(h.item(
        &id,
        &run,
        "a1",
        ItemStatus::Running,
        assistant("Looking at", true),
    )));
    h.fake.emit(ProviderEvent::ItemUpserted(h.item(
        &id,
        &run,
        "c1",
        ItemStatus::Running,
        TurnItemBody::CommandExecution {
            input: "cargo test".into(),
            output: None,
            exit_code: None,
            output_indicates_failure: None,
        },
    )));
    h.fake.emit(ProviderEvent::CommandOutputDelta {
        item_id: "c1".into(),
        chunk: "running 3 tests\n".into(),
    });
    h.wait("live output", |e| {
        (e.thread(&id)?.live_output.get("c1")? == "running 3 tests\n").then_some(())
    });
    let shell = h.shell(&id);
    assert!(shell.is_working());
    assert_eq!(shell.active_run_id.as_deref(), Some(run.as_str()));

    h.fake.emit(ProviderEvent::ItemUpserted(h.item(
        &id,
        &run,
        "c1",
        ItemStatus::Completed,
        TurnItemBody::CommandExecution {
            input: "cargo test".into(),
            output: Some("ok".into()),
            exit_code: Some(0),
            output_indicates_failure: Some(false),
        },
    )));
    h.fake.emit(ProviderEvent::ItemUpserted(h.item(
        &id,
        &run,
        "a1",
        ItemStatus::Completed,
        assistant("Looking at it. Done.", false),
    )));
    h.terminal(&run, TerminalStatus::Completed, None);

    h.wait_run(&id, &run, RunStatus::Completed);
    let d = h.wait("checkpoint item", |e| {
        let d = e.thread(&id)?;
        d.items
            .iter()
            .any(|i| matches!(i.body, TurnItemBody::Checkpoint { .. }))
            .then_some(d)
    });
    assert!(d.live_output.is_empty());
    let a1 = d.items.iter().find(|i| i.id == "a1").unwrap();
    assert_eq!(a1.ordinal, 2, "streamed item keeps its first ordinal");
    let ords: Vec<u32> = d.items.iter().map(|i| i.ordinal).collect();
    assert!(ords.windows(2).all(|w| w[0] < w[1]), "{ords:?}");
    let run_row = d.runs.iter().find(|r| r.id == run).unwrap();
    assert_eq!(run_row.native_turn_id.as_deref(), Some("turn-1"));
    assert_eq!(
        run_row.checkpoint_id.as_deref(),
        Some(checkpoint_id(&id, 1).as_str())
    );
    assert!(
        h.git
            .calls
            .lock()
            .contains(&"summarize:ref-0:ref-1".to_string())
    );

    let shell = h.shell(&id);
    assert_eq!(shell.status, ShellThreadStatus::Run(RunStatus::Completed));
    assert_eq!(
        shell.latest_message,
        Some(("assistant".into(), "Looking at it. Done.".into()))
    );
    assert_eq!(
        shell.diff_totals,
        Some(FileTotals {
            count: 1,
            additions: 5,
            deletions: 2
        })
    );
    assert!(!shell.is_working());

    // persisted
    let inner = &h.engine.inner;
    let store = inner.store.lock();
    let rows = store.thread_rows(&id).unwrap();
    assert_eq!(rows.runs[0].status, RunStatus::Completed);
    assert_eq!(rows.items.len(), d.items.len());
    assert_eq!(rows.checkpoints.len(), 2);
    assert_eq!(store.thread(&id).unwrap().unwrap().title, "Fake title");
}

#[test]
fn approval_round_trip() {
    let h = harness();
    let (id, run) = h.running("Delete the build dir");
    h.fake.emit(ProviderEvent::ItemUpserted(h.item(
        &id,
        &run,
        "ap",
        ItemStatus::Waiting,
        TurnItemBody::ApprovalRequest {
            request_id: "req-1".into(),
            request_kind: RequestKind::Command,
            prompt: Some("rm -rf build".into()),
            reason: None,
            full_access_override: None,
            options: None,
        },
    )));
    h.fake.emit(ProviderEvent::RequestOpened(RuntimeRequest {
        id: "req-1".into(),
        thread_id: String::new(),
        run_id: Some(run.clone()),
        turn_item_id: "ap".into(),
        native_request_id: Some(serde_json::json!(3)),
        kind: RequestKind::Command,
        status: RequestStatus::Pending,
        live_session_id: None,
        created_at: now(),
        resolved_at: None,
        decision: None,
        feedback: None,
        answers: None,
    }));
    h.wait("badge", |e| {
        let s = e.snapshot().thread(&id)?.clone();
        (s.pending_request == Some(("req-1".into(), RequestKind::Command))).then_some(())
    });
    assert_eq!(h.run_status(&id, &run), RunStatus::Running);

    h.engine.send(Command::RespondToRequest {
        thread_id: id.clone(),
        request_id: "req-1".into(),
        decision: Some(ApprovalDecision::Accept),
        feedback: None,
        answers: None,
    });
    h.wait("delivered", |_| {
        h.fake.called("respond:req-1").then_some(())
    });
    let d = h.detail(&id);
    let req = &d.requests[0];
    assert_eq!(req.status, RequestStatus::Resolved);
    assert_eq!(req.decision, Some(ApprovalDecision::Accept));
    assert!(req.live_session_id.is_some());
    assert_eq!(
        d.items.iter().find(|i| i.id == "ap").unwrap().status,
        ItemStatus::Completed
    );
    assert_eq!(
        h.fake.0.lock().responses[0].decision,
        Some(ApprovalDecision::Accept)
    );
    assert!(h.shell(&id).pending_request.is_none());

    // a second answer is refused
    h.engine.send(Command::RespondToRequest {
        thread_id: id.clone(),
        request_id: "req-1".into(),
        decision: Some(ApprovalDecision::Decline),
        feedback: None,
        answers: None,
    });
    h.wait("refusal", |_| (!h.errors().is_empty()).then_some(()));
}

#[test]
fn user_input_answer_is_recorded_on_the_item() {
    let h = harness();
    let (id, run) = h.running("Pick a store");
    h.fake.emit(ProviderEvent::ItemUpserted(h.item(
        &id,
        &run,
        "q",
        ItemStatus::Waiting,
        TurnItemBody::UserInputRequest {
            request_id: "uq".into(),
            questions: vec![UserInputQuestion {
                id: "store".into(),
                header: "Store".into(),
                question: "Where?".into(),
                options: vec![],
                multi_select: None,
                allow_custom_answer: None,
                initial_answer: None,
                required: None,
            }],
            question_answer: None,
        },
    )));
    h.fake.emit(ProviderEvent::RequestOpened(RuntimeRequest {
        id: "uq".into(),
        thread_id: String::new(),
        run_id: None,
        turn_item_id: "q".into(),
        native_request_id: None,
        kind: RequestKind::UserInput,
        status: RequestStatus::Pending,
        live_session_id: None,
        created_at: now(),
        resolved_at: None,
        decision: None,
        feedback: None,
        answers: None,
    }));
    h.wait("badge", |e| {
        e.snapshot().thread(&id)?.pending_request.clone()
    });
    let mut answers = HashMap::new();
    answers.insert("store".to_string(), serde_json::json!("Redis"));
    h.engine.send(Command::RespondToRequest {
        thread_id: id.clone(),
        request_id: "uq".into(),
        decision: None,
        feedback: None,
        answers: Some(answers),
    });
    h.wait("delivered", |_| h.fake.called("respond:uq").then_some(()));
    let d = h.detail(&id);
    let q = d.items.iter().find(|i| i.id == "q").unwrap();
    match &q.body {
        TurnItemBody::UserInputRequest {
            question_answer: Some(a),
            ..
        } => {
            assert_eq!(a.answers["store"], "Redis");
            assert_eq!(a.question_text_by_id.as_ref().unwrap()["store"], "Where?");
        }
        other => panic!("{other:?}"),
    }
    // the request was bound to the active run
    assert_eq!(d.requests[0].run_id.as_deref(), Some(run.as_str()));
}

#[test]
fn interrupt_then_terminal() {
    let h = harness();
    let (id, run) = h.running("Long task");
    h.engine.send(Command::Interrupt {
        thread_id: id.clone(),
        run_id: run.clone(),
    });
    h.wait("interrupt sent", |_| {
        h.fake.called(&format!("interrupt:{run}")).then_some(())
    });
    let d = h.detail(&id);
    assert!(d.runs[0].interrupt_requested_at.is_some());
    assert!(d.items.iter().any(|i| matches!(
        &i.body,
        TurnItemBody::RunInterruptRequest { message, .. } if message == USER_STOP_REASON
    )));
    h.terminal(&run, TerminalStatus::Interrupted, None);
    h.wait_run(&id, &run, RunStatus::Interrupted);
    let d = h.detail(&id);
    assert!(d.items.iter().any(|i| matches!(
        &i.body,
        TurnItemBody::RunInterruptResult { message } if message == USER_STOP_REASON
    )));
    // interrupted runs still get their end checkpoint
    h.wait("checkpoint", |e| {
        e.thread(&id)?.runs[0].checkpoint_id.clone()
    });
}

#[test]
fn unresponsive_interrupt_then_force_stop() {
    let h = harness();
    let (id, run) = h.running("Hang");
    h.fake.emit(ProviderEvent::ItemUpserted(h.item(
        &id,
        &run,
        "c",
        ItemStatus::Running,
        TurnItemBody::CommandExecution {
            input: "sleep 1000".into(),
            output: None,
            exit_code: None,
            output_indicates_failure: None,
        },
    )));
    h.engine.send(Command::Interrupt {
        thread_id: id.clone(),
        run_id: run.clone(),
    });
    let d = h.wait("unresponsive", |e| {
        let d = e.thread(&id)?;
        d.runs[0].interrupt_unresponsive_at.is_some().then_some(d)
    });
    assert!(d.items.iter().any(|i| matches!(
        i.body,
        TurnItemBody::RunInterruptRequest {
            unresponsive_at: Some(_),
            ..
        }
    )));
    h.engine.send(Command::ForceStop {
        thread_id: id.clone(),
    });
    h.wait_run(&id, &run, RunStatus::Interrupted);
    assert!(h.fake.called("kill"));
    let d = h.detail(&id);
    assert_eq!(
        d.items.iter().find(|i| i.id == "c").unwrap().status,
        ItemStatus::Interrupted
    );
    assert!(d.items.iter().any(|i| matches!(
        &i.body,
        TurnItemBody::RunInterruptResult { message } if message == FORCE_STOP_REASON
    )));
    // the next message opens a fresh session
    h.engine.send(Command::SendMessage {
        thread_id: id.clone(),
        message: msg("again"),
        model_selection: None,
        mode: DispatchMode::StartImmediately,
    });
    h.wait("reopen", |_| (h.fake.0.lock().opens == 2).then_some(()));
}

#[test]
fn start_failure_fails_the_run() {
    let h = harness();
    h.fake.0.lock().fail_start = true;
    let id = h.launch(Some("hello"));
    let d = h.wait("failed", |e| {
        let d = e.thread(&id)?;
        (d.runs.first()?.status == RunStatus::Failed).then_some(d)
    });
    let err = d
        .items
        .iter()
        .find_map(|i| match &i.body {
            TurnItemBody::Error { failure, .. } => Some(failure.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(err.class, FailureClass::ProviderError);
    assert!(err.message.contains("turn/start"));
    let shell = h.shell(&id);
    assert_eq!(shell.status, ShellThreadStatus::Run(RunStatus::Failed));
    assert_eq!(shell.last_error_class, Some(FailureClass::ProviderError));
    // failed runs get no end checkpoint
    assert!(d.runs[0].checkpoint_id.is_none());
}

#[test]
fn usage_limit_failure_shows_reset_time() {
    let h = harness();
    let (id, run) = h.running("big job");
    h.terminal(
        &run,
        TerminalStatus::Failed,
        Some(ProviderFailure {
            class: FailureClass::UsageLimit,
            message: "You've hit your usage limit.".into(),
            code: Some("usage_limit_reached".into()),
            retryable: Some(false),
            reset_at: Some("2026-11-25T21:12:00Z".into()),
        }),
    );
    h.wait_run(&id, &run, RunStatus::Failed);
    let shell = h.shell(&id);
    assert_eq!(shell.last_error_class, Some(FailureClass::UsageLimit));
    assert_eq!(
        shell.usage_limit_reset_at.as_deref(),
        Some("2026-11-25T21:12:00Z")
    );
    assert_eq!(
        shell.last_error.as_deref(),
        Some("You've hit your usage limit.")
    );
}

#[test]
fn queued_follow_up_starts_after_the_active_run() {
    let h = harness();
    let (id, run1) = h.running("first");
    h.fake.emit(ProviderEvent::ItemUpserted(h.item(
        &id,
        &run1,
        "a",
        ItemStatus::Running,
        assistant("working", true),
    )));
    h.engine.send(Command::SendMessage {
        thread_id: id.clone(),
        message: msg("second"),
        model_selection: None,
        mode: DispatchMode::QueueAfterActive,
    });
    let run2 = h.wait("queued", |e| {
        let d = e.thread(&id)?;
        let r = d.runs.iter().find(|r| r.status == RunStatus::Queued)?;
        Some(r.id.clone())
    });
    let d = h.detail(&id);
    let r2 = d.runs.iter().find(|r| r.id == run2).unwrap();
    assert_eq!((r2.ordinal, r2.queue_position), (2, Some(1)));
    assert_eq!(
        h.shell(&id).status,
        ShellThreadStatus::Run(RunStatus::Running)
    );
    // a third one queues behind it, then gets cancelled
    h.engine.send(Command::SendMessage {
        thread_id: id.clone(),
        message: msg("third"),
        model_selection: None,
        mode: DispatchMode::QueueAfterActive,
    });
    let run3 = h.wait("queued 3", |e| {
        let d = e.thread(&id)?;
        let r = d.runs.iter().find(|r| r.queue_position == Some(2))?;
        Some(r.id.clone())
    });
    h.engine.send(Command::CancelQueuedRun {
        thread_id: id.clone(),
        run_id: run3.clone(),
    });
    h.wait_run(&id, &run3, RunStatus::Cancelled);

    h.fake.emit(ProviderEvent::ItemUpserted(h.item(
        &id,
        &run1,
        "a",
        ItemStatus::Completed,
        assistant("working done", false),
    )));
    h.terminal(&run1, TerminalStatus::Completed, None);
    h.wait("run2 started", |_| {
        h.fake.called(&format!("start:{run2}")).then_some(())
    });
    let d = h.detail(&id);
    assert!(matches!(
        d.runs.iter().find(|r| r.id == run2).unwrap().status,
        RunStatus::Starting | RunStatus::Running
    ));
    assert_eq!(h.fake.0.lock().opens, 1, "session reused");
    // the queued message now sits after everything run 1 wrote
    let pos = |pred: &dyn Fn(&TurnItem) -> bool| d.items.iter().position(pred).unwrap();
    let second =
        pos(&|i| matches!(&i.body, TurnItemBody::UserMessage { text, .. } if text == "second"));
    let checkpoint = pos(&|i| matches!(i.body, TurnItemBody::Checkpoint { .. }));
    assert!(second > checkpoint, "{:#?}", d.items);
    // the cancelled one stays hidden
    let shell = h.shell(&id);
    assert_eq!(shell.latest_message, Some(("user".into(), "second".into())));
}

#[test]
fn steer_goes_to_the_live_turn() {
    let h = harness();
    h.fake.0.lock().caps.supports_active_steering = true;
    let (id, run) = h.running("start");
    h.engine.send(Command::SendMessage {
        thread_id: id.clone(),
        message: msg("use tabs"),
        model_selection: None,
        mode: DispatchMode::SteerActive {
            target_run_id: run.clone(),
        },
    });
    h.wait("steer", |_| {
        h.fake
            .called(&format!("steer:{run}:use tabs"))
            .then_some(())
    });
    let d = h.detail(&id);
    assert_eq!(d.runs.len(), 1);
    assert!(d.items.iter().any(|i| matches!(
        &i.body,
        TurnItemBody::UserMessage {
            input_intent: InputIntent::Steer,
            ..
        }
    )));
}

#[test]
fn steer_without_support_queues() {
    let h = harness();
    let (id, run) = h.running("start");
    h.engine.send(Command::SendMessage {
        thread_id: id.clone(),
        message: msg("use tabs"),
        model_selection: None,
        mode: DispatchMode::SteerActive { target_run_id: run },
    });
    h.wait("queued", |e| {
        e.thread(&id)?
            .runs
            .iter()
            .any(|r| r.status == RunStatus::Queued)
            .then_some(())
    });
}

#[test]
fn session_exit_fails_the_live_run() {
    let h = harness();
    let (id, run) = h.running("start");
    // dropping the adapter's sender is what a crashed child process looks like
    h.fake.0.lock().events = None;
    h.wait_run(&id, &run, RunStatus::Failed);
    let shell = h.shell(&id);
    assert_eq!(shell.last_error_class, Some(FailureClass::TransportError));
}

#[test]
fn plan_and_todo_updates() {
    let h = harness();
    let (id, run) = h.running("plan it");
    h.fake.emit(ProviderEvent::PlanUpdated {
        plan_id: "p1".into(),
        markdown: "# Plan\n1.".into(),
        streaming: true,
    });
    h.fake.emit(ProviderEvent::PlanUpdated {
        plan_id: "p1".into(),
        markdown: "# Plan\n1. Do it".into(),
        streaming: false,
    });
    let step = |s: PlanStepStatus| PlanStep {
        id: "s1".into(),
        text: "Do it".into(),
        status: s,
        duration_anchor_at: None,
        duration_ms: None,
    };
    h.fake.emit(ProviderEvent::TodoUpdated {
        plan_id: "t1".into(),
        steps: vec![step(PlanStepStatus::Running)],
        explanation: None,
    });
    h.fake.emit(ProviderEvent::TodoUpdated {
        plan_id: "t1".into(),
        steps: vec![step(PlanStepStatus::Completed)],
        explanation: None,
    });
    let d = h.wait("todo done", |e| {
        let d = e.thread(&id)?;
        d.items
            .iter()
            .any(|i| {
                i.status == ItemStatus::Completed && matches!(i.body, TurnItemBody::TodoList { .. })
            })
            .then_some(d)
    });
    assert_eq!(d.plans.len(), 1);
    assert_eq!(d.plans[0].status, PlanStatus::Active);
    let plans: Vec<_> = d
        .items
        .iter()
        .filter(|i| matches!(i.body, TurnItemBody::ProposedPlan { .. }))
        .collect();
    assert_eq!(plans.len(), 1);
    match &d
        .items
        .iter()
        .find(|i| matches!(i.body, TurnItemBody::TodoList { .. }))
        .unwrap()
        .body
    {
        TurnItemBody::TodoList { steps, .. } => {
            assert!(steps[0].duration_anchor_at.is_some());
            assert!(steps[0].duration_ms.is_some());
        }
        _ => unreachable!(),
    }
    assert!(h.shell(&id).has_actionable_proposed_plan);

    // implementing the plan completes it
    h.terminal(&run, TerminalStatus::Completed, None);
    h.wait_run(&id, &run, RunStatus::Completed);
    h.wait("capture", |e| e.thread(&id)?.runs[0].checkpoint_id.clone());
    h.engine.send(Command::SendMessage {
        thread_id: id.clone(),
        message: OutgoingMessage {
            text: format!("{IMPLEMENT_PLAN_PREFIX}# Plan\n1. Do it"),
            attachments: vec![],
            source_plan_id: Some("p1".into()),
        },
        model_selection: None,
        mode: DispatchMode::StartImmediately,
    });
    h.wait("plan completed", |e| {
        (e.thread(&id)?.plans[0].status == PlanStatus::Completed).then_some(())
    });
    assert!(!h.shell(&id).has_actionable_proposed_plan);
}

#[test]
fn rollback_marks_later_runs_and_restores_files() {
    let h = harness();
    let (id, run1) = h.running("one");
    h.terminal(&run1, TerminalStatus::Completed, None);
    h.wait("cp1", |e| e.thread(&id)?.runs[0].checkpoint_id.clone());
    h.engine.send(Command::SendMessage {
        thread_id: id.clone(),
        message: msg("two"),
        model_selection: None,
        mode: DispatchMode::StartImmediately,
    });
    let run2 = h.wait("run2", |e| {
        let d = e.thread(&id)?;
        let r = d.runs.get(1)?;
        (r.status == RunStatus::Running).then(|| r.id.clone())
    });
    h.terminal(&run2, TerminalStatus::Completed, None);
    h.wait("cp2", |e| e.thread(&id)?.runs.get(1)?.checkpoint_id.clone());

    h.engine.send(Command::RollbackToCheckpoint {
        thread_id: id.clone(),
        checkpoint_id: checkpoint_id(&id, 1),
        restore_files: true,
    });
    h.wait_run(&id, &run2, RunStatus::RolledBack);
    assert!(h.git.calls.lock().contains(&"restore:ref-1".to_string()));
    let d = h.detail(&id);
    let cp2 = d.checkpoints.iter().find(|c| c.ordinal == 2).unwrap();
    assert_eq!(cp2.status, CheckpointStatus::Stale);
    assert_eq!(h.run_status(&id, &run1), RunStatus::Completed);
    let shell = h.shell(&id);
    assert_eq!(shell.latest_message, Some(("user".into(), "one".into())));

    // the next run reuses ordinal 2 and recaptures it
    h.engine.send(Command::SendMessage {
        thread_id: id.clone(),
        message: msg("two again"),
        model_selection: None,
        mode: DispatchMode::StartImmediately,
    });
    let run3 = h.wait("run3", |e| {
        let d = e.thread(&id)?;
        let r = d.runs.get(2)?;
        (r.status == RunStatus::Running).then(|| r.id.clone())
    });
    assert_eq!(h.detail(&id).runs[2].ordinal, 2);
    h.terminal(&run3, TerminalStatus::Completed, None);
    h.wait("cp2 again", |e| {
        let d = e.thread(&id)?;
        (d.checkpoints.iter().find(|c| c.ordinal == 2)?.status == CheckpointStatus::Ready)
            .then_some(())
    });
}

#[test]
fn rollback_refused_while_running() {
    let h = harness();
    let (id, _run) = h.running("one");
    h.engine.send(Command::RollbackToCheckpoint {
        thread_id: id,
        checkpoint_id: "x".into(),
        restore_files: false,
    });
    h.wait("error", |_| (!h.errors().is_empty()).then_some(()));
}

#[test]
fn model_change_restarts_a_session_without_in_session_switch() {
    let h = harness();
    let (id, run) = h.running("one");
    h.terminal(&run, TerminalStatus::Completed, None);
    h.wait_run(&id, &run, RunStatus::Completed);
    let mut ms = codex();
    ms.model = "gpt-5.6-sol".into();
    h.engine.send(Command::SetModelSelection {
        thread_id: id.clone(),
        model_selection: ms,
    });
    h.wait("shutdown", |_| h.fake.called("shutdown").then_some(()));
    assert_eq!(h.detail(&id).thread.model_selection.model, "gpt-5.6-sol");

    // with in-session switching the live session is kept
    h.fake.0.lock().caps.supports_model_switch_in_session = true;
    h.engine.send(Command::SendMessage {
        thread_id: id.clone(),
        message: msg("two"),
        model_selection: None,
        mode: DispatchMode::StartImmediately,
    });
    h.wait("run 2 running", |e| {
        let d = e.thread(&id)?;
        (d.runs.get(1)?.status == RunStatus::Running).then_some(())
    });
    let mut ms = codex();
    ms.model = "gpt-6-luna".into();
    h.engine.send(Command::SetModelSelection {
        thread_id: id.clone(),
        model_selection: ms,
    });
    h.wait("set_model", |_| {
        h.fake.called("set_model:gpt-6-luna").then_some(())
    });
}

#[test]
fn title_failure_keeps_the_seed() {
    let h = harness_with(Store::open_in_memory().unwrap(), None);
    let long = "Investigate why the nightly export job writes duplicate rows for accounts with multiple currencies";
    let id = h.launch(Some(long));
    h.wait("title settled", |e| {
        let d = e.thread(&id)?;
        (d.thread.title_generation_started_at.is_none() && d.thread.title != NEW_THREAD_TITLE)
            .then_some(())
    });
    let title = h.detail(&id).thread.title;
    assert_eq!(
        title,
        format!("{}...", long.chars().take(50).collect::<String>())
    );
}

#[test]
fn renamed_thread_keeps_its_title() {
    let h = harness();
    let id = h.launch(None);
    h.engine.send(Command::RenameThread {
        thread_id: id.clone(),
        title: "My title".into(),
    });
    h.wait("renamed", |e| {
        e.thread(&id)?.thread.title_renamed.then_some(())
    });
    h.engine.send(Command::SendMessage {
        thread_id: id.clone(),
        message: msg("first message"),
        model_selection: None,
        mode: DispatchMode::StartImmediately,
    });
    h.wait("run", |e| (!e.thread(&id)?.runs.is_empty()).then_some(()));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(h.detail(&id).thread.title, "My title");
}

#[test]
fn set_thread_branch_records_checkout() {
    let h = harness();
    let id = h.launch(None);
    h.engine.send(Command::SetThreadBranch {
        thread_id: id.clone(),
        branch: Some("feature/x".into()),
        worktree_path: Some("/tmp/wt".into()),
    });
    h.wait("branch", |e| {
        (e.thread(&id)?.thread.branch.as_deref() == Some("feature/x")).then_some(())
    });
    assert_eq!(
        h.detail(&id).thread.worktree_path.as_deref(),
        Some("/tmp/wt")
    );
}

#[test]
fn sidebar_flags_and_unread() {
    let h = harness();
    let (id, run) = h.running("one");
    h.terminal(&run, TerminalStatus::Completed, None);
    h.wait_run(&id, &run, RunStatus::Completed);
    // launched threads count as visited at creation, so a finished run is unread
    assert!(h.shell(&id).is_unread());
    h.engine.send(Command::MarkVisited {
        thread_id: id.clone(),
    });
    h.wait("visited", |e| {
        (!e.snapshot().thread(&id)?.is_unread()).then_some(())
    });
    h.engine.send(Command::MarkUnread {
        thread_id: id.clone(),
    });
    h.wait("unread", |e| {
        e.snapshot().thread(&id)?.is_unread().then_some(())
    });
    for c in [
        Command::SetPinned {
            thread_id: id.clone(),
            pinned: true,
        },
        Command::SetSettled {
            thread_id: id.clone(),
            settled: true,
        },
        Command::SetArchived {
            thread_id: id.clone(),
            archived: true,
        },
    ] {
        h.engine.send(c);
    }
    let t = h.wait("flags", |e| {
        let t = e.thread(&id)?.thread;
        t.archived_at.is_some().then_some(t)
    });
    assert!(t.pinned_at.is_some() && t.settled_at.is_some());
}

#[test]
fn delete_thread_purges_it() {
    let mut h = harness();
    // the ref sweep only runs in folders that still exist
    let dir = std::env::temp_dir().join(format!("tern-del-{}", new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    h.engine.send(Command::AddProject {
        path: dir.to_string_lossy().into_owned(),
    });
    h.project_id = h
        .engine
        .snapshot()
        .projects
        .iter()
        .find(|p| p.workspace_root == dir.to_string_lossy())
        .unwrap()
        .id
        .clone();
    let (id, run) = h.running("one");
    h.terminal(&run, TerminalStatus::Completed, None);
    h.wait("cp", |e| e.thread(&id)?.runs[0].checkpoint_id.clone());
    h.engine.send(Command::DeleteThread {
        thread_id: id.clone(),
    });
    h.wait("gone", |e| e.snapshot().thread(&id).is_none().then_some(()));
    assert!(h.engine.thread(&id).is_none());
    h.wait("refs swept + row deleted", |e| {
        e.inner
            .store
            .lock()
            .thread(&id)
            .unwrap()
            .is_none()
            .then_some(())
    });
    assert!(h.git.calls.lock().contains(&format!("delete_all:{id}")));
    // commands for it now fail
    h.engine.send(Command::MarkVisited { thread_id: id });
    h.wait("error", |_| (!h.errors().is_empty()).then_some(()));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn worktree_launch_prepares_then_starts() {
    let h = harness();
    let id = new_id();
    h.engine.send(Command::LaunchThread {
        thread_id: id.clone(),
        project_id: h.project_id.clone(),
        model_selection: codex(),
        runtime_mode: RuntimeMode::FullAccess,
        interaction_mode: InteractionMode::Default,
        workspace: WorkspaceStrategy::Worktree {
            base_ref: "main".into(),
            branch: None,
            start_from_origin: Some(false),
        },
        message: Some(msg("in a worktree")),
    });
    let d = h.wait("started", |e| {
        let d = e.thread(&id)?;
        (d.runs.first()?.status == RunStatus::Running).then_some(d)
    });
    let branch = d.thread.branch.clone().unwrap();
    assert!(branch.starts_with("t3/") && branch.len() == 11, "{branch}");
    let wt = d.thread.worktree_path.clone().unwrap();
    assert!(wt.ends_with(&branch.replace('/', "-")));
    assert_eq!(h.fake.0.lock().turns[0].cwd, PathBuf::from(&wt));
}

#[test]
fn validation_errors_do_not_create_runs() {
    let h = harness();
    let id = h.launch(None);
    h.engine.send(Command::SendMessage {
        thread_id: id.clone(),
        message: msg("   "),
        model_selection: None,
        mode: DispatchMode::StartImmediately,
    });
    h.wait("error", |_| (!h.errors().is_empty()).then_some(()));
    assert!(h.detail(&id).runs.is_empty());
}

#[test]
fn unknown_provider_fails_the_run() {
    let h = harness();
    let id = new_id();
    h.engine.send(Command::LaunchThread {
        thread_id: id.clone(),
        project_id: h.project_id.clone(),
        model_selection: ModelSelection {
            instance_id: "acp:gemini".into(),
            model: "x".into(),
            options: vec![],
        },
        runtime_mode: RuntimeMode::FullAccess,
        interaction_mode: InteractionMode::Default,
        workspace: WorkspaceStrategy::Root { branch: None },
        message: Some(msg("hi")),
    });
    h.wait("failed", |e| {
        (e.thread(&id)?.runs.first()?.status == RunStatus::Failed).then_some(())
    });
}

#[test]
fn projects_add_rename_remove() {
    let h = harness();
    assert_eq!(h.engine.snapshot().projects[0].workspace_root, "/work/proj");
    assert_eq!(h.engine.snapshot().projects[0].title, "proj");
    h.engine.send(Command::AddProject {
        path: "/work/proj".into(),
    });
    h.wait("dup error", |_| (!h.errors().is_empty()).then_some(()));
    h.engine.send(Command::RenameProject {
        project_id: h.project_id.clone(),
        title: "Project".into(),
    });
    assert!(h.engine.snapshot().projects[0].has_custom_title);
    let id = h.launch(None);
    h.engine.send(Command::RemoveProject {
        project_id: h.project_id.clone(),
    });
    assert!(h.engine.snapshot().projects.is_empty());
    h.wait("thread deleted", |e| {
        e.snapshot().thread(&id).is_none().then_some(())
    });
}

#[test]
fn startup_recovers_live_runs_and_pending_requests() {
    let dir = std::env::temp_dir().join(format!("tern-recover-{}", new_id()));
    let db = dir.join("state.sqlite");
    let (thread_id, run_id) = {
        let h = harness_with(Store::open(&db).unwrap(), Some("t"));
        let (id, run) = h.running("survive a crash");
        h.fake.emit(ProviderEvent::ItemUpserted(h.item(
            &id,
            &run,
            "ap",
            ItemStatus::Waiting,
            TurnItemBody::ApprovalRequest {
                request_id: "r".into(),
                request_kind: RequestKind::Command,
                prompt: None,
                reason: None,
                full_access_override: None,
                options: None,
            },
        )));
        h.fake.emit(ProviderEvent::RequestOpened(RuntimeRequest {
            id: "r".into(),
            thread_id: String::new(),
            run_id: Some(run.clone()),
            turn_item_id: "ap".into(),
            native_request_id: None,
            kind: RequestKind::Command,
            status: RequestStatus::Pending,
            live_session_id: None,
            created_at: now(),
            resolved_at: None,
            decision: None,
            feedback: None,
            answers: None,
        }));
        h.wait("pending", |e| {
            e.snapshot().thread(&id)?.pending_request.clone()
        });
        (id, run)
    };
    let h = harness_with(Store::open(&db).unwrap(), Some("t"));
    let d = h.detail(&thread_id);
    assert_eq!(d.runs[0].id, run_id);
    assert_eq!(d.runs[0].status, RunStatus::Interrupted);
    assert_eq!(d.requests[0].status, RequestStatus::Cancelled);
    assert_eq!(
        d.items.iter().find(|i| i.id == "ap").unwrap().status,
        ItemStatus::Cancelled
    );
    assert!(h.shell(&thread_id).pending_request.is_none());
    // the interrupted run gets its missing checkpoint on startup
    h.wait("sweep capture", |e| {
        e.thread(&thread_id)?.runs[0].checkpoint_id.clone()
    });
    drop(h);
    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------------------------------------
// shell derivation

fn bare_detail() -> ThreadDetail {
    let t = now();
    ThreadDetail {
        thread: Thread {
            id: "t".into(),
            project_id: "p".into(),
            title: "x".into(),
            model_selection: codex(),
            runtime_mode: RuntimeMode::FullAccess,
            interaction_mode: InteractionMode::Default,
            branch: None,
            worktree_path: None,
            active_provider_thread_id: None,
            title_renamed: false,
            title_generation_started_at: None,
            created_at: t,
            updated_at: t,
            archived_at: None,
            pinned_at: None,
            settled_at: None,
            last_visited_at: None,
            linked_pull_request: None,
            deleted_at: None,
        },
        runs: vec![],
        items: vec![],
        requests: vec![],
        checkpoints: vec![],
        plans: vec![],
        context_usage: None,
        session: None,
        live_output: HashMap::new(),
        revision: 0,
    }
}

fn req(id: &str, kind: RequestKind, secs_ago: i64) -> RuntimeRequest {
    RuntimeRequest {
        id: id.into(),
        thread_id: "t".into(),
        run_id: None,
        turn_item_id: "i".into(),
        native_request_id: None,
        kind,
        status: RequestStatus::Pending,
        live_session_id: None,
        created_at: now() - chrono::Duration::seconds(secs_ago),
        resolved_at: None,
        decision: None,
        feedback: None,
        answers: None,
    }
}

#[test]
fn shell_idle_and_request_priority() {
    let mut d = bare_detail();
    let s = shell::derive_shell(&d);
    assert_eq!(s.status, ShellThreadStatus::Idle);
    assert!(!s.is_unread() && s.diff_totals.is_none() && s.latest_message.is_none());

    d.requests = vec![
        req("q-new", RequestKind::UserInput, 1),
        req("a-old", RequestKind::Command, 30),
        req("a-new", RequestKind::FileChange, 5),
    ];
    let s = shell::derive_shell(&d);
    assert_eq!(
        s.pending_request,
        Some(("a-new".into(), RequestKind::FileChange))
    );
    d.requests.retain(|r| r.kind == RequestKind::UserInput);
    assert_eq!(
        shell::derive_shell(&d).pending_request,
        Some(("q-new".into(), RequestKind::UserInput))
    );
}

#[test]
fn shell_session_error_hides_failure_class() {
    let mut d = bare_detail();
    d.runs.push(Run {
        id: "r".into(),
        thread_id: "t".into(),
        ordinal: 1,
        model_selection: codex(),
        runtime_mode: RuntimeMode::FullAccess,
        interaction_mode: InteractionMode::Default,
        user_message_id: "m".into(),
        status: RunStatus::Failed,
        queue_position: None,
        requested_at: now(),
        started_at: Some(now()),
        completed_at: Some(now()),
        checkpoint_id: None,
        source_plan_id: None,
        native_turn_id: None,
        model_reported: None,
        token_usage: None,
        failure: Some(ProviderFailure {
            class: FailureClass::UsageLimit,
            message: "limit".into(),
            code: None,
            retryable: None,
            reset_at: Some("soon".into()),
        }),
        interrupt_requested_at: None,
        interrupt_unresponsive_at: None,
    });
    let s = shell::derive_shell(&d);
    assert_eq!(s.last_error_class, Some(FailureClass::UsageLimit));
    assert!(s.is_unread());
    d.session = Some(ProviderSession {
        id: "s".into(),
        driver: Driver::Codex,
        status: SessionStatus::Error,
        cwd: "/".into(),
        model: None,
        capabilities: Capabilities::default(),
        startup_phase: None,
        last_error: Some("codex crashed".into()),
        created_at: now(),
        updated_at: now(),
    });
    let s = shell::derive_shell(&d);
    assert_eq!(s.last_error.as_deref(), Some("codex crashed"));
    assert_eq!(s.last_error_class, None);
    assert_eq!(s.usage_limit_reset_at, None);
}

// ---------------------------------------------------------------------------------------------
// demo

fn demo() -> (EngineHandle, Arc<Mutex<Vec<Update>>>) {
    let updates = Arc::new(Mutex::new(Vec::new()));
    let u = updates.clone();
    let e = EngineHandle::demo(Box::new(move |up| u.lock().push(up)));
    (e, updates)
}

fn wait_demo<T>(
    e: &EngineHandle,
    what: &str,
    secs: u64,
    f: impl Fn(&EngineHandle) -> Option<T>,
) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f(e) {
            return v;
        }
        if start.elapsed() > Duration::from_secs(secs) {
            panic!("timed out waiting for {what}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn demo_seeds_every_item_kind_and_state() {
    let (e, _) = demo();
    let snap = e.snapshot();
    assert_eq!(snap.projects.len(), 3);
    assert_eq!(snap.threads.len(), 14);
    assert_eq!(snap.providers.len(), 2);

    let mut kinds = std::collections::BTreeSet::new();
    for s in &snap.threads {
        let d = e.thread(&s.thread.id).unwrap();
        let ords: Vec<u32> = d.items.iter().map(|i| i.ordinal).collect();
        assert!(ords.windows(2).all(|w| w[0] < w[1]), "{}", s.thread.title);
        for i in &d.items {
            kinds.insert(item_kind(&i.body));
        }
    }
    let missing: Vec<_> = ITEM_KINDS.iter().filter(|k| !kinds.contains(*k)).collect();
    assert!(missing.is_empty(), "demo is missing {missing:?}");

    let status = |s: &ThreadShell| s.status;
    let has = |st: RunStatus| {
        snap.threads
            .iter()
            .any(|s| status(s) == ShellThreadStatus::Run(st))
    };
    for st in [
        RunStatus::Running,
        RunStatus::Completed,
        RunStatus::Failed,
        RunStatus::Interrupted,
    ] {
        assert!(has(st), "no {st:?} thread");
    }
    let pending: Vec<_> = snap
        .threads
        .iter()
        .filter_map(|s| s.pending_request.clone())
        .collect();
    assert!(pending.iter().any(|(_, k)| *k == RequestKind::Command));
    assert!(pending.iter().any(|(_, k)| *k == RequestKind::UserInput));
    assert!(snap.threads.iter().any(|s| s.thread.pinned_at.is_some()));
    assert!(snap.threads.iter().any(|s| s.thread.settled_at.is_some()));
    assert!(snap.threads.iter().any(|s| s.thread.archived_at.is_some()));
    assert!(snap.threads.iter().any(|s| s.is_unread()));
    assert!(snap.threads.iter().any(|s| s.has_actionable_proposed_plan));
    assert!(
        snap.threads
            .iter()
            .any(|s| s.usage_limit_reset_at.is_some())
    );
    assert!(snap.threads.iter().any(|s| s.diff_totals.is_some()));
    assert!(snap.threads.iter().any(|s| {
        e.thread(&s.thread.id)
            .unwrap()
            .runs
            .iter()
            .any(|r| r.status == RunStatus::Queued)
    }));
    let unread = snap.threads.iter().filter(|s| s.is_unread()).count();
    assert!(unread <= 2, "{unread} unread threads");
    // the big transcript has markdown with every block kind
    let big = snap
        .threads
        .iter()
        .find(|s| s.thread.title == "Port the diff parser to Rust")
        .unwrap();
    let d = e.thread(&big.thread.id).unwrap();
    let text: String = d
        .items
        .iter()
        .filter_map(|i| match &i.body {
            TurnItemBody::AssistantMessage { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    for needle in [
        "## ",
        "```rust",
        "```ts",
        "```bash",
        "|---|",
        "](https://",
        "- **",
        ".rs:",
    ] {
        assert!(text.contains(needle), "missing {needle}");
    }
}

#[test]
fn demo_running_thread_keeps_streaming() {
    let (e, _) = demo();
    let id = e
        .snapshot()
        .threads
        .iter()
        .find(|s| s.thread.title == "Stream markdown in the chat timeline")
        .unwrap()
        .thread
        .id
        .clone();
    let len = |e: &EngineHandle| {
        e.thread(&id)
            .unwrap()
            .items
            .iter()
            .rev()
            .find_map(|i| match &i.body {
                TurnItemBody::AssistantMessage {
                    text,
                    streaming: true,
                    ..
                } => Some(text.len()),
                _ => None,
            })
            .unwrap_or(0)
    };
    let before = len(&e);
    wait_demo(&e, "more text", 5, |e| (len(e) > before).then_some(()));
    wait_demo(&e, "live output", 5, |e| {
        (!e.thread(&id)?.live_output.is_empty()).then_some(())
    });
}

#[test]
fn demo_send_streams_a_reply_end_to_end() {
    let (e, _) = demo();
    let snap = e.snapshot();
    let id = snap
        .threads
        .iter()
        .find(|s| s.thread.title == "Sidebar keyboard navigation")
        .unwrap()
        .thread
        .id
        .clone();
    let runs_before = e.thread(&id).unwrap().runs.len();
    e.send(Command::SendMessage {
        thread_id: id.clone(),
        message: msg("Also support Home and End."),
        model_selection: None,
        mode: DispatchMode::StartImmediately,
    });
    let d = wait_demo(&e, "demo reply", 15, |e| {
        let d = e.thread(&id)?;
        let r = d.runs.last()?;
        (d.runs.len() > runs_before
            && r.status == RunStatus::Completed
            && r.checkpoint_id.is_some())
        .then_some(d)
    });
    let run = d.runs.last().unwrap();
    let items: Vec<_> = d
        .items
        .iter()
        .filter(|i| i.run_id.as_deref() == Some(run.id.as_str()))
        .collect();
    assert!(items.iter().any(|i| matches!(
        i.body,
        TurnItemBody::CommandExecution {
            exit_code: Some(0),
            ..
        }
    )));
    assert!(
        items
            .iter()
            .any(|i| matches!(i.body, TurnItemBody::Reasoning { .. }))
    );
    assert!(
        items
            .iter()
            .any(|i| matches!(i.body, TurnItemBody::Checkpoint { .. }))
    );
    assert!(
        items
            .iter()
            .filter(|i| matches!(
                i.body,
                TurnItemBody::AssistantMessage {
                    streaming: false,
                    ..
                }
            ))
            .count()
            >= 2
    );
}

#[test]
fn demo_approval_completes_the_run() {
    let (e, _) = demo();
    let shell = e
        .snapshot()
        .threads
        .into_iter()
        .find(|s| matches!(s.pending_request, Some((_, RequestKind::Command))))
        .unwrap();
    let id = shell.thread.id.clone();
    let (request_id, _) = shell.pending_request.unwrap();
    // the session attaches right after start
    wait_demo(&e, "session", 5, |e| e.thread(&id)?.session.clone());
    e.send(Command::RespondToRequest {
        thread_id: id.clone(),
        request_id,
        decision: Some(ApprovalDecision::Accept),
        feedback: None,
        answers: None,
    });
    wait_demo(&e, "approved run completes", 15, |e| {
        (e.thread(&id)?.runs.last()?.status == RunStatus::Completed).then_some(())
    });
    assert!(e.snapshot().thread(&id).unwrap().pending_request.is_none());
}

#[test]
fn demo_question_answer_produces_a_plan() {
    let (e, _) = demo();
    let shell = e
        .snapshot()
        .threads
        .into_iter()
        .find(|s| matches!(s.pending_request, Some((_, RequestKind::UserInput))))
        .unwrap();
    let id = shell.thread.id.clone();
    let (request_id, _) = shell.pending_request.unwrap();
    wait_demo(&e, "session", 5, |e| e.thread(&id)?.session.clone());
    let mut answers = HashMap::new();
    answers.insert("store".to_string(), serde_json::json!("Postgres"));
    answers.insert(
        "algorithm".to_string(),
        serde_json::json!(["Sliding window"]),
    );
    e.send(Command::RespondToRequest {
        thread_id: id.clone(),
        request_id,
        decision: None,
        feedback: None,
        answers: Some(answers),
    });
    let d = wait_demo(&e, "plan", 15, |e| {
        let d = e.thread(&id)?;
        (d.runs.last()?.status == RunStatus::Completed).then_some(d)
    });
    assert!(
        d.plans
            .iter()
            .any(|p| p.status == PlanStatus::Active && p.markdown.contains("Postgres"))
    );
}

#[test]
fn demo_interrupt_stops_the_streaming_thread() {
    let (e, _) = demo();
    let shell = e
        .snapshot()
        .threads
        .into_iter()
        .find(|s| s.thread.title == "Stream markdown in the chat timeline")
        .unwrap();
    let id = shell.thread.id.clone();
    let run = shell.active_run_id.unwrap();
    wait_demo(&e, "session", 5, |e| e.thread(&id)?.session.clone());
    e.send(Command::Interrupt {
        thread_id: id.clone(),
        run_id: run.clone(),
    });
    wait_demo(&e, "interrupted", 5, |e| {
        (e.thread(&id)?.runs.iter().find(|r| r.id == run)?.status == RunStatus::Interrupted)
            .then_some(())
    });
}

#[test]
fn dropping_the_engine_kills_live_sessions() {
    let h = harness();
    let (_id, _run) = h.running("long");
    let fake = h.fake.clone();
    drop(h);
    assert!(fake.called("kill"));
}

//! Real child processes (shell stand-ins in tests/fixtures/bin) that crash, print
//! garbage or hang. Every case must end the turn with a broken terminal, not hang.

mod common;

use std::collections::HashMap;
use std::time::Duration;

use common::*;
use tern_core::*;
use tern_providers::claude::ClaudeAdapter;
use tern_providers::codex::CodexAdapter;
use tern_providers::*;
use tokio::sync::mpsc;

fn session_config(
    bin: &str,
    instance: &str,
    model: &str,
    mode: &str,
    resume: Option<&str>,
) -> SessionConfig {
    let mut env = HashMap::new();
    env.insert("FAKE_MODE".to_string(), mode.to_string());
    SessionConfig {
        thread_id: "thread-1".into(),
        cwd: std::env::temp_dir(),
        resume_native_thread_id: resume.map(str::to_string),
        model_selection: ModelSelection {
            instance_id: instance.into(),
            model: model.into(),
            options: vec![],
        },
        runtime_mode: RuntimeMode::FullAccess,
        binary_path: Some(fixture(&format!("bin/{bin}"))),
        env,
    }
}

async fn until_terminal(rx: &mut mpsc::UnboundedReceiver<ProviderEvent>) -> Vec<ProviderEvent> {
    let mut out = vec![];
    loop {
        match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
            Ok(Some(e)) => {
                let done = matches!(e, ProviderEvent::TurnTerminal { .. });
                out.push(e);
                if done {
                    return out;
                }
            }
            _ => panic!("no terminal: {out:#?}"),
        }
    }
}

async fn codex_turn(mode: &str) -> Vec<ProviderEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = CodexAdapter::new()
        .open_session(
            session_config("fake-codex", "codex", "gpt-6-luna", mode, None),
            tx,
        )
        .await
        .unwrap();
    session
        .start_turn(turn(
            "run-1",
            "hi",
            "gpt-6-luna",
            "codex",
            RuntimeMode::FullAccess,
            InteractionMode::Default,
        ))
        .await
        .unwrap();
    let mut events = until_terminal(&mut rx).await;
    // The session-level error follows the terminal.
    while let Ok(Some(e)) = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await {
        events.push(e);
    }
    session.shutdown().await;
    events
}

#[tokio::test]
async fn codex_crash_mid_turn_is_a_broken_failure() {
    let events = codex_turn("crash").await;
    let (status, failure, broken) = terminal(&events);
    assert_eq!((status, broken), (TerminalStatus::Failed, true));
    let f = failure.unwrap();
    assert_eq!(f.class, FailureClass::TransportError);
    assert!(f.message.contains("exited with code 3"), "{}", f.message);
    assert!(
        f.message.contains("error: fake codex crashed"),
        "{}",
        f.message
    );
    let items = final_items(&events);
    assert_settled(&items);
    assert!(
        items
            .iter()
            .any(|i| matches!(i.body, TurnItemBody::CommandExecution { .. })
                && i.status == ItemStatus::Failed)
    );
    assert!(events.iter().any(|e| matches!(
        e,
        ProviderEvent::SessionUpdated {
            status: SessionStatus::Error,
            ..
        }
    )));
}

#[tokio::test]
async fn codex_garbage_line_is_a_broken_failure() {
    let events = codex_turn("garbage").await;
    let (status, failure, broken) = terminal(&events);
    assert_eq!((status, broken), (TerminalStatus::Failed, true));
    assert!(failure.unwrap().message.contains("malformed frame"));
    assert_settled(&final_items(&events));
}

#[tokio::test]
async fn codex_status_reads_version_account_and_models() {
    let st = CodexAdapter::new()
        .status(Some(fixture("bin/fake-codex")))
        .await;
    assert_eq!(st.version.as_deref(), Some("0.160.1"));
    assert_eq!(st.auth, ProviderAuthState::SignedIn);
    assert_eq!(st.account_label.as_deref(), Some("OpenAI API Key"));
    assert_eq!(st.models.len(), 1);
    assert_eq!(st.models[0].name, "GPT-6 Luna");
    let missing = CodexAdapter::new()
        .status(Some("/nonexistent/codex".into()))
        .await;
    assert_eq!(missing.auth, ProviderAuthState::NotInstalled);
}

#[tokio::test]
async fn claude_crash_mid_stream_settles_partial_text() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = ClaudeAdapter::new()
        .open_session(
            session_config(
                "fake-claude",
                "claudeAgent",
                "claude-haiku-5-5",
                "crash",
                None,
            ),
            tx,
        )
        .await
        .unwrap();
    session
        .start_turn(turn(
            "run-1",
            "hi",
            "claude-haiku-5-5",
            "claudeAgent",
            RuntimeMode::FullAccess,
            InteractionMode::Default,
        ))
        .await
        .unwrap();
    let events = until_terminal(&mut rx).await;
    let (status, failure, broken) = terminal(&events);
    assert_eq!((status, broken), (TerminalStatus::Failed, true));
    assert!(
        failure
            .unwrap()
            .message
            .contains("error: fake claude crashed")
    );
    let items = final_items(&events);
    assert_eq!(assistant_texts(&items), ["partial"]);
    assert_settled(&items);
    session.shutdown().await;
}

#[tokio::test]
async fn claude_garbage_line_is_a_broken_failure() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = ClaudeAdapter::new()
        .open_session(
            session_config(
                "fake-claude",
                "claudeAgent",
                "claude-haiku-5-5",
                "garbage",
                None,
            ),
            tx,
        )
        .await
        .unwrap();
    session
        .start_turn(turn(
            "run-1",
            "hi",
            "claude-haiku-5-5",
            "claudeAgent",
            RuntimeMode::FullAccess,
            InteractionMode::Default,
        ))
        .await
        .unwrap();
    let events = until_terminal(&mut rx).await;
    let (status, failure, broken) = terminal(&events);
    assert_eq!((status, broken), (TerminalStatus::Failed, true));
    assert!(failure.unwrap().message.contains("malformed line"));
    session.shutdown().await;
}

#[tokio::test]
async fn claude_force_stop_of_a_hung_turn() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = ClaudeAdapter::new()
        .open_session(
            session_config(
                "fake-claude",
                "claudeAgent",
                "claude-haiku-5-5",
                "hang",
                None,
            ),
            tx,
        )
        .await
        .unwrap();
    session
        .start_turn(turn(
            "run-1",
            "hi",
            "claude-haiku-5-5",
            "claudeAgent",
            RuntimeMode::FullAccess,
            InteractionMode::Default,
        ))
        .await
        .unwrap();
    // Wait until the partial text arrived, then pull the plug.
    loop {
        let e = tokio::time::timeout(Duration::from_secs(10), rx.recv())
            .await
            .unwrap()
            .unwrap();
        if matches!(e, ProviderEvent::TurnStarted { .. }) {
            break;
        }
    }
    session.kill();
    let events = until_terminal(&mut rx).await;
    let (status, _, broken) = terminal(&events);
    assert_eq!((status, broken), (TerminalStatus::Interrupted, true));
    session.shutdown().await;
}

#[tokio::test]
async fn claude_policy_change_respawns_with_resume() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = ClaudeAdapter::new()
        .open_session(
            session_config("fake-claude", "claudeAgent", "claude-haiku-5-5", "ok", None),
            tx,
        )
        .await
        .unwrap();
    for run in ["run-1", "run-2"] {
        session
            .start_turn(turn(
                run,
                "hi",
                "claude-haiku-5-5",
                "claudeAgent",
                RuntimeMode::FullAccess,
                InteractionMode::Default,
            ))
            .await
            .unwrap();
        let events = until_terminal(&mut rx).await;
        assert_eq!(terminal(&events).0, TerminalStatus::Completed);
    }
    // A runtime-mode change needs a new process; it resumes the same session.
    session
        .start_turn(turn(
            "run-3",
            "hi",
            "claude-haiku-5-5",
            "claudeAgent",
            RuntimeMode::ApprovalRequired,
            InteractionMode::Default,
        ))
        .await
        .unwrap();
    let events = until_terminal(&mut rx).await;
    assert_eq!(terminal(&events).0, TerminalStatus::Completed);
    // The respawned CLI was given --resume <same id>: no new session was adopted.
    assert!(events.iter().all(|e| !matches!(e, ProviderEvent::BindingUpdated { native_thread_id: Some(id), .. } if id.is_empty())));
    session.shutdown().await;
}

#[tokio::test]
async fn claude_missing_conversation_starts_fresh() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = ClaudeAdapter::new()
        .open_session(
            session_config(
                "fake-claude",
                "claudeAgent",
                "claude-haiku-5-5",
                "resume_missing",
                Some("gone"),
            ),
            tx,
        )
        .await
        .unwrap();
    let mut fresh = None;
    while let Ok(Some(e)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if let ProviderEvent::BindingUpdated {
            native_thread_id: Some(id),
            ..
        } = e
        {
            fresh = Some(id);
        }
    }
    let fresh = fresh.expect("binding updated");
    assert_ne!(fresh, "gone");
    assert_eq!(fresh.len(), 36);
    session.shutdown().await;
}

#[tokio::test]
async fn claude_status_reads_account_from_initialize() {
    let st = ClaudeAdapter::new()
        .status(Some(fixture("bin/fake-claude")))
        .await;
    assert_eq!(st.version.as_deref(), Some("2.1.332"));
    assert_eq!(st.auth, ProviderAuthState::SignedIn);
    assert_eq!(st.account_label.as_deref(), Some("Claude Max Subscription"));
    assert!(st.models.iter().any(|m| m.slug == "claude-fable-5-2"));
    let missing = ClaudeAdapter::new()
        .status(Some("/nonexistent/claude".into()))
        .await;
    assert_eq!(missing.auth, ProviderAuthState::NotInstalled);
}

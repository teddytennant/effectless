//! Against the installed CLIs. Ignored by default: the status probes are free, the turns
//! spend real tokens. `cargo test -p tern-providers --test live -- --ignored <name>`.
//! TERN_LIVE_CODEX_MODEL / TERN_LIVE_CLAUDE_MODEL pick the models.

mod common;

use std::time::Duration;

use common::*;
use tern_core::*;
use tern_providers::claude::ClaudeAdapter;
use tern_providers::codex::CodexAdapter;
use tern_providers::textgen::{TextGenOptions, ThreadTitleInput};
use tern_providers::*;
use tokio::sync::mpsc;

fn git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let ok = std::process::Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(ok.success());
    dir
}

async fn live_turn(
    adapter: &dyn ProviderAdapter,
    instance: &str,
    model: &str,
    effort: (&str, &str),
) -> Vec<ProviderEvent> {
    let dir = git_repo();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut cfg = config(model, instance, RuntimeMode::ApprovalRequired);
    cfg.cwd = dir.path().to_path_buf();
    let session = adapter.open_session(cfg, tx).await.unwrap();
    let mut t = turn(
        "run-1",
        "reply with the word ok",
        model,
        instance,
        RuntimeMode::ApprovalRequired,
        InteractionMode::Default,
    );
    t.cwd = dir.path().to_path_buf();
    t.model_selection.options.push(OptionSelection {
        id: effort.0.into(),
        value: OptionValue::Str(effort.1.into()),
    });
    session.start_turn(t).await.unwrap();
    let mut events = vec![];
    loop {
        let e = tokio::time::timeout(Duration::from_secs(180), rx.recv())
            .await
            .expect("turn timed out")
            .expect("channel");
        let done = matches!(e, ProviderEvent::TurnTerminal { .. });
        events.push(e);
        if done {
            break;
        }
    }
    session.shutdown().await;
    events
}

#[tokio::test]
#[ignore]
async fn codex_status() {
    let st = CodexAdapter::new().status(None).await;
    eprintln!("{st:#?}");
    assert!(st.version.is_some(), "{st:?}");
    assert!(!st.models.is_empty() || st.auth == ProviderAuthState::SignedOut);
}

#[tokio::test]
#[ignore]
async fn claude_status() {
    let st = ClaudeAdapter::new().status(None).await;
    eprintln!(
        "{:?} {:?} {:?} {:?}",
        st.version, st.auth, st.account_label, st.message
    );
    assert!(st.version.is_some(), "{st:?}");
    assert_ne!(st.auth, ProviderAuthState::Unknown, "{st:?}");
}

#[tokio::test]
#[ignore]
async fn codex_turn() {
    let model = std::env::var("TERN_LIVE_CODEX_MODEL").unwrap_or_else(|_| "gpt-6-luna".into());
    let events = live_turn(
        &CodexAdapter::new(),
        "codex",
        &model,
        ("reasoningEffort", "low"),
    )
    .await;
    eprintln!("{:#?}", terminal(&events));
    assert_settled(&final_items(&events));
}

#[tokio::test]
#[ignore]
async fn claude_turn() {
    let model =
        std::env::var("TERN_LIVE_CLAUDE_MODEL").unwrap_or_else(|_| "claude-haiku-5-5".into());
    let events = live_turn(
        &ClaudeAdapter::new(),
        "claudeAgent",
        &model,
        ("effort", "low"),
    )
    .await;
    assert_eq!(terminal(&events).0, TerminalStatus::Completed);
    let texts = assistant_texts(&final_items(&events));
    assert!(
        texts.iter().any(|t| t.to_lowercase().contains("ok")),
        "{texts:?}"
    );
}

#[tokio::test]
#[ignore]
async fn claude_thread_title() {
    let dir = tempfile::tempdir().unwrap();
    let mut opts = TextGenOptions::default_for(dir.path().to_path_buf());
    opts.model = ModelSelection {
        instance_id: "claudeAgent".into(),
        model: "claude-haiku-5-5".into(),
        options: vec![],
    };
    let title = textgen::thread_title(
        &opts,
        &ThreadTitleInput {
            message: "the login page throws a 500 when the password has a quote in it".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    eprintln!("{title}");
    assert!(!title.is_empty() && title != NEW_THREAD_TITLE);
}

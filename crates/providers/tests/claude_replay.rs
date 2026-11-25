//! Replays Claude CLI stream-json traffic through the real session code.
//! `live_simple` is a capture from claude 2.1.332; the rest are T3's Agent SDK
//! recordings rewritten to the CLI's stdio frames (permission prompts become
//! `can_use_tool` control requests, field names checked against the 2.1.332 binary).

mod common;

use common::*;
use serde_json::{Value, json};
use tern_core::*;
use tern_providers::claude::ClaudeSession;
use tern_providers::*;
use tokio::sync::mpsc;

const MODEL: &str = "claude-sonnet-4-6";

struct Run {
    events: Vec<ProviderEvent>,
    client: Vec<Value>,
    entries: Vec<Value>,
    session_id: String,
}

fn prompts(entries: &[Value]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| e["type"] == "expect_outbound" && e["frame"]["type"] == "user")
        .map(|e| {
            e["frame"]["message"]["content"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

async fn replay_entries(
    entries: Vec<Value>,
    mode: RuntimeMode,
    interaction: InteractionMode,
    answer: impl FnMut(&RuntimeRequest) -> Option<RequestResponse>,
    on_started: impl FnMut() -> Option<Action>,
) -> Run {
    let text = prompts(&entries)[0].clone();
    let (r, w, fake) = spawn_fake(Proto::Claude, entries.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = ClaudeSession::over_io(r, w, config(MODEL, "claudeAgent", mode), tx);
    session.start().await.unwrap();
    session
        .start_turn(turn(
            "run-1",
            &text,
            MODEL,
            "claudeAgent",
            mode,
            interaction,
        ))
        .await
        .unwrap();
    let events = drive(&mut rx, &session, answer, on_started).await;
    let session_id = session.session_id();
    session.shutdown().await;
    drop(session);
    let client = fake.await.unwrap().client_frames;
    Run {
        events,
        client,
        entries,
        session_id,
    }
}

async fn replay(
    name: &str,
    mode: RuntimeMode,
    answer: impl FnMut(&RuntimeRequest) -> Option<RequestResponse>,
    on_started: impl FnMut() -> Option<Action>,
) -> Run {
    replay_entries(
        load(&format!("claude/{name}.ndjson")),
        mode,
        InteractionMode::Default,
        answer,
        on_started,
    )
    .await
}

#[tokio::test]
async fn live_2_1_332_simple_turn() {
    let run = replay(
        "live_simple",
        RuntimeMode::ApprovalRequired,
        |_| None,
        || None,
    )
    .await;
    let items = final_items(&run.events);
    assert_eq!(assistant_texts(&items), ["ok"]);
    assert_settled(&items);
    assert_eq!(
        terminal(&run.events),
        (TerminalStatus::Completed, None, false)
    );
    match run.events.iter().find_map(|e| match e {
        ProviderEvent::TurnUsage { usage, model, .. } => Some((usage.clone(), model.clone())),
        _ => None,
    }) {
        Some((TurnTokenUsage::Complete(c), Some(model))) => {
            assert_eq!(c.input_tokens, Some(2 + 26516 + 14318));
            assert_eq!(c.output_tokens, Some(4));
            assert_eq!(c.cached_input_tokens, Some(26516));
            assert_eq!(c.reasoning_tokens, Some(0));
            assert_eq!(model, "claude-haiku-5-5");
        }
        other => panic!("{other:?}"),
    }
    // The init frame names the session; Tern adopts it.
    assert_eq!(run.session_id, "78fd1a7d-399a-4c16-8ce3-957b42a2a3be");
    assert!(run.events.iter().any(|e| matches!(e, ProviderEvent::BindingUpdated { native_thread_id: Some(id), .. } if id == "78fd1a7d-399a-4c16-8ce3-957b42a2a3be")));
    assert!(run.events.iter().any(|e| matches!(e, ProviderEvent::BindingUpdated { context_usage: Some(c), .. } if c.max_tokens == Some(1_000_000))));
    // Wire: initialize first, then one user line with a fresh uuid.
    assert_eq!(run.client[0]["type"], "control_request");
    assert_eq!(run.client[0]["request"]["subtype"], "initialize");
    let user = run.client.iter().find(|f| f["type"] == "user").unwrap();
    assert_eq!(user["message"]["content"], "reply with the word ok");
    assert_eq!(user["uuid"].as_str().unwrap().len(), 36);
    assert!(user.get("priority").is_none());
}

#[tokio::test]
async fn snapshot_text_without_stream_is_published() {
    let run = replay("simple", RuntimeMode::FullAccess, |_| None, || None).await;
    assert_eq!(
        assistant_texts(&final_items(&run.events)),
        ["fixture simple ok"]
    );
    assert_eq!(terminal(&run.events).0, TerminalStatus::Completed);
}

#[tokio::test]
async fn streamed_thinking_text_and_bash() {
    let run = replay(
        "command_output_streaming",
        RuntimeMode::FullAccess,
        |_| None,
        || None,
    )
    .await;
    let items = final_items(&run.events);
    assert!(
        matches!(&items[0].body, TurnItemBody::Reasoning { text, streaming: false } if text.starts_with("The user wants me to run"))
    );
    let texts = assistant_texts(&items);
    assert_eq!(
        texts[0], "Running the specified command now.",
        "streamed once, snapshot deduped"
    );
    let cmd = items
        .iter()
        .find(|i| matches!(i.body, TurnItemBody::CommandExecution { .. }))
        .unwrap();
    match &cmd.body {
        TurnItemBody::CommandExecution { input, output, .. } => {
            assert!(input.starts_with("for i in 1 2 3 4"));
            assert!(output.as_deref().unwrap().contains("stream line 4"));
        }
        _ => unreachable!(),
    }
    assert_eq!(cmd.status, ItemStatus::Completed);
    assert_settled(&items);
    // Stream items coalesce: far fewer snapshots than deltas.
    let reasoning_snaps = run
        .events
        .iter()
        .filter(|e| matches!(e, ProviderEvent::ItemUpserted(i) if i.id == items[0].id))
        .count();
    assert!(reasoning_snaps < 9, "{reasoning_snaps}");
}

#[tokio::test]
async fn bash_approval_accept_matches_t3_reply() {
    let mut opened = None;
    let run = replay(
        "tool_call_restricted_granular",
        RuntimeMode::ApprovalRequired,
        |r| {
            opened = Some(r.clone());
            Some(respond_with(r, ApprovalDecision::Accept))
        },
        || None,
    )
    .await;
    let req = opened.unwrap();
    assert_eq!(req.kind, RequestKind::Command);
    let items = final_items(&run.events);
    let approval = items.iter().find(|i| i.id == req.turn_item_id).unwrap();
    assert!(
        matches!(&approval.body, TurnItemBody::ApprovalRequest { prompt: Some(p), .. } if p.starts_with("printf 'codex app-server approval fixture'"))
    );
    let ours = run
        .client
        .iter()
        .find(|f| f["type"] == "control_response")
        .unwrap();
    let theirs = expected(&run.entries, |f| f["type"] == "control_response").unwrap();
    assert_eq!(ours, theirs);
    assert_eq!(terminal(&run.events).0, TerminalStatus::Completed);
    assert!(items.iter().any(|i| matches!(&i.body, TurnItemBody::CommandExecution { output: Some(o), .. } if o.is_empty())));
}

#[tokio::test]
async fn write_decline_matches_t3_reply_and_fails_the_tool() {
    let run = replay(
        "tool_call_denied_write",
        RuntimeMode::ApprovalRequired,
        |r| Some(respond_with(r, ApprovalDecision::Decline)),
        || None,
    )
    .await;
    let ours = run
        .client
        .iter()
        .find(|f| f["type"] == "control_response")
        .unwrap();
    let theirs = expected(&run.entries, |f| f["type"] == "control_response").unwrap();
    assert_eq!(ours, theirs);
    let items = final_items(&run.events);
    let change = items
        .iter()
        .find(|i| matches!(i.body, TurnItemBody::FileChange { .. }))
        .unwrap();
    assert_eq!(change.status, ItemStatus::Failed);
    assert!(
        matches!(&change.body, TurnItemBody::FileChange { new_str: Some(s), additions: Some(1), .. } if s == "approval fixture")
    );
    assert_eq!(
        assistant_texts(&items).last().unwrap(),
        "write permission denied"
    );
}

#[tokio::test]
async fn read_tools_are_dynamic_tools_with_output() {
    let run = replay(
        "tool_call_read_only",
        RuntimeMode::FullAccess,
        |_| None,
        || None,
    )
    .await;
    let reads: Vec<TurnItem> = final_items(&run.events)
        .into_iter()
        .filter(|i| matches!(&i.body, TurnItemBody::DynamicTool { tool_name: Some(n), .. } if n == "Read"))
        .collect();
    assert_eq!(reads.len(), 2);
    assert!(reads.iter().all(|r| r.status == ItemStatus::Completed
        && matches!(
            &r.body,
            TurnItemBody::DynamicTool {
                output: Some(_),
                ..
            }
        )));
}

#[tokio::test]
async fn web_search_collects_result_links() {
    let run = replay("web_search", RuntimeMode::FullAccess, |_| None, || None).await;
    let ws = final_items(&run.events)
        .into_iter()
        .find(|i| matches!(i.body, TurnItemBody::WebSearch { .. }))
        .unwrap();
    match &ws.body {
        TurnItemBody::WebSearch { patterns, results } => {
            assert_eq!(
                patterns.as_deref(),
                Some(&["FIFA World Cup 2026 ticket pricing".to_string()][..])
            );
            assert!(
                results
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|r| r.url.contains("fifa.com"))
            );
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn usage_limit_is_classified() {
    let run = replay("usage_limit", RuntimeMode::FullAccess, |_| None, || None).await;
    let (status, failure, broken) = terminal(&run.events);
    assert_eq!((status, broken), (TerminalStatus::Failed, false));
    let f = failure.unwrap();
    assert_eq!(f.class, FailureClass::UsageLimit);
    assert_eq!(
        f.message,
        "Claude usage limit reached. Send the message again once the limit resets."
    );
    let items = final_items(&run.events);
    let retry = items
        .iter()
        .find(|i| matches!(i.body, TurnItemBody::Error { .. }))
        .unwrap();
    assert!(
        matches!(&retry.body, TurnItemBody::Error { retry: Some(r), .. } if r.attempt == 10 && r.max_attempts == Some(10))
    );
    assert_settled(&items);
    assert!(
        assistant_texts(&items).is_empty(),
        "synthetic API error text is not a reply"
    );
}

#[tokio::test]
async fn auth_error_fails_then_next_turn_recovers() {
    let entries = load("claude/claude_result_is_error.ndjson");
    let ps = prompts(&entries);
    let (r, w, fake) = spawn_fake(Proto::Claude, entries);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = ClaudeSession::over_io(
        r,
        w,
        config(MODEL, "claudeAgent", RuntimeMode::FullAccess),
        tx,
    );
    session.start().await.unwrap();
    session
        .start_turn(turn(
            "run-1",
            &ps[0],
            MODEL,
            "claudeAgent",
            RuntimeMode::FullAccess,
            InteractionMode::Default,
        ))
        .await
        .unwrap();
    let first = drive(&mut rx, &session, |_| None, || None).await;
    let (status, failure, _) = terminal(&first);
    assert_eq!(status, TerminalStatus::Failed);
    assert!(failure.unwrap().message.contains("Failed to authenticate"));
    session
        .start_turn(turn(
            "run-2",
            &ps[1],
            MODEL,
            "claudeAgent",
            RuntimeMode::FullAccess,
            InteractionMode::Default,
        ))
        .await
        .unwrap();
    let second = drive(&mut rx, &session, |_| None, || None).await;
    assert_eq!(terminal(&second).0, TerminalStatus::Completed);
    assert_eq!(
        assistant_texts(&final_items(&second)),
        ["claude result is_error fixture recovered"]
    );
    session.shutdown().await;
    drop(session);
    fake.await.unwrap();
}

#[tokio::test]
async fn interrupt_mid_tool_sends_control_request_and_settles() {
    let mut entries = load("claude/turn_interrupt_mid_tool.ndjson");
    // The recording stops at the interrupt; the CLI then exits.
    entries.push(json!({ "type": "server_exit" }));
    let run = replay_entries(
        entries,
        RuntimeMode::FullAccess,
        InteractionMode::Default,
        |_| None,
        || Some(Action::Interrupt),
    )
    .await;
    assert!(
        run.client
            .iter()
            .any(|f| f["type"] == "control_request" && f["request"]["subtype"] == "interrupt")
    );
    let (status, _, _) = terminal(&run.events);
    assert_eq!(status, TerminalStatus::Interrupted);
    let items = final_items(&run.events);
    let cmd = items
        .iter()
        .find(|i| matches!(i.body, TurnItemBody::CommandExecution { .. }))
        .unwrap();
    assert_eq!(cmd.status, ItemStatus::Interrupted);
    assert_settled(&items);
}

#[tokio::test]
async fn two_turns_share_one_process() {
    let entries = load("claude/multi_turn.ndjson");
    let ps = prompts(&entries);
    let (r, w, fake) = spawn_fake(Proto::Claude, entries);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = ClaudeSession::over_io(
        r,
        w,
        config(MODEL, "claudeAgent", RuntimeMode::FullAccess),
        tx,
    );
    session.start().await.unwrap();
    for (i, p) in ps.iter().enumerate() {
        session
            .start_turn(turn(
                &format!("run-{i}"),
                p,
                MODEL,
                "claudeAgent",
                RuntimeMode::FullAccess,
                InteractionMode::Default,
            ))
            .await
            .unwrap();
        let ev = drive(&mut rx, &session, |_| None, || None).await;
        assert_eq!(terminal(&ev).0, TerminalStatus::Completed);
    }
    session.shutdown().await;
    drop(session);
    let client = fake.await.unwrap().client_frames;
    assert_eq!(
        client
            .iter()
            .filter(|f| f["request"]["subtype"] == "initialize")
            .count(),
        1
    );
}

#[tokio::test]
async fn steer_aborted_segment_keeps_the_turn_open() {
    let entries = load("claude/message_steering.ndjson");
    let ps = prompts(&entries);
    let (r, w, fake) = spawn_fake(Proto::Claude, entries);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = ClaudeSession::over_io(
        r,
        w,
        config(MODEL, "claudeAgent", RuntimeMode::FullAccess),
        tx,
    );
    session.start().await.unwrap();
    session
        .start_turn(turn(
            "run-1",
            &ps[0],
            MODEL,
            "claudeAgent",
            RuntimeMode::FullAccess,
            InteractionMode::Default,
        ))
        .await
        .unwrap();
    session.steer("run-1", ps[1].clone()).await.unwrap();
    let ev = drive(&mut rx, &session, |_| None, || None).await;
    assert_eq!(terminal(&ev).0, TerminalStatus::Completed);
    assert_eq!(
        assistant_texts(&final_items(&ev)),
        ["steering fixture observed"]
    );
    session.shutdown().await;
    drop(session);
    let client = fake.await.unwrap().client_frames;
    let steer = client
        .iter()
        .filter(|f| f["type"] == "user")
        .nth(1)
        .unwrap();
    assert_eq!(steer["priority"], "next");
}

#[tokio::test]
async fn tool_kinds_questions_plans_and_cancelled_prompts() {
    let mut seen = vec![];
    let run = replay(
        "synthetic_tools",
        RuntimeMode::ApprovalRequired,
        |r| {
            seen.push(r.kind);
            match r.kind {
                RequestKind::FileChange => {
                    Some(respond_with(r, ApprovalDecision::AcceptForSession))
                }
                RequestKind::UserInput => {
                    let mut answers = std::collections::HashMap::new();
                    answers.insert("Which?".to_string(), json!(["A"]));
                    Some(RequestResponse {
                        request: r.clone(),
                        decision: None,
                        feedback: None,
                        answers: Some(answers),
                    })
                }
                // The Bash prompt is withdrawn by the CLI before anyone answers.
                _ => None,
            }
        },
        || None,
    )
    .await;
    assert_eq!(
        seen,
        [
            RequestKind::FileChange,
            RequestKind::UserInput,
            RequestKind::Command
        ]
    );
    let replies: Vec<&Value> = run
        .client
        .iter()
        .filter(|f| f["type"] == "control_response")
        .collect();
    let edit = &replies[0]["response"]["response"];
    assert_eq!(edit["behavior"], "allow");
    assert_eq!(edit["decisionClassification"], "user_permanent");
    assert_eq!(
        edit["updatedPermissions"],
        json!([{"type": "setMode", "mode": "acceptEdits", "destination": "session"}])
    );
    assert_eq!(edit["toolUseID"], "t-edit");
    let ask = &replies[1]["response"]["response"];
    assert_eq!(ask["updatedInput"]["answers"], json!({"Which?": "A"}));
    assert_eq!(ask["updatedInput"]["questions"][0]["header"], "Pick");
    let plan = &replies[2]["response"]["response"];
    assert_eq!(plan["behavior"], "deny");
    assert!(
        plan["message"]
            .as_str()
            .unwrap()
            .starts_with("The client captured your proposed plan.")
    );

    assert!(run.events.iter().any(|e| matches!(
        e,
        ProviderEvent::RequestClosed {
            status: RequestStatus::Cancelled,
            ..
        }
    )));
    assert!(run.events.iter().any(|e| matches!(e, ProviderEvent::TodoUpdated { steps, .. } if steps.len() == 2 && steps[0].status == PlanStepStatus::Running)));
    assert!(run.events.iter().any(|e| matches!(e, ProviderEvent::PlanUpdated { markdown, streaming: false, .. } if markdown == "# Plan\n- step one")));

    let items = final_items(&run.events);
    assert_settled(&items);
    let kinds: Vec<&str> = items
        .iter()
        .map(|i| match &i.body {
            TurnItemBody::TodoList { .. } => "todo",
            TurnItemBody::FileChange { .. } => "file_change",
            TurnItemBody::ApprovalRequest { .. } => "approval",
            TurnItemBody::FileSearch { .. } => "file_search",
            TurnItemBody::Subagent { .. } => "subagent",
            TurnItemBody::UserInputRequest { .. } => "question",
            TurnItemBody::CommandExecution { .. } => "command",
            TurnItemBody::ProposedPlan { .. } => "plan",
            TurnItemBody::AssistantMessage { .. } => "text",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "todo",
            "file_change",
            "approval",
            "file_search",
            "subagent",
            "question",
            "command",
            "approval",
            "plan",
            "text"
        ]
    );
    match &items[1].body {
        TurnItemBody::FileChange {
            file_name,
            diff_str: Some(d),
            additions,
            deletions,
            ..
        } => {
            assert_eq!(file_name, "/w/a.rs");
            assert!(d.contains("-b\n+c"));
            assert_eq!((*additions, *deletions), (Some(1), Some(1)));
        }
        other => panic!("{other:?}"),
    }
    assert!(
        matches!(&items[3].body, TurnItemBody::FileSearch { pattern: Some(p), results: Some(r) } if p == "fn main" && r[0].path == "src/main.rs" && r[0].line == Some(1))
    );
    assert!(
        matches!(&items[4].body, TurnItemBody::Subagent { prompt, result: Some(r), .. } if prompt == "look around" && r == "found it")
    );
    assert_eq!(
        items[6].status,
        ItemStatus::Failed,
        "declined Bash tool_result is an error"
    );
    assert_eq!(items[7].status, ItemStatus::Cancelled, "withdrawn prompt");
    assert!(run.events.iter().any(|e| matches!(e, ProviderEvent::BindingUpdated { context_usage: Some(c), .. } if c.max_tokens == Some(200_000))));
}

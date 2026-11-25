//! Replays recorded `codex app-server` traffic through the real session code.
//! Fixtures: T3's testkit recordings (codex 0.156.1) plus one capture from 0.160.1.

mod common;

use common::*;
use serde_json::{Value, json};
use tern_core::*;
use tern_providers::codex::CodexSession;
use tern_providers::*;
use tokio::sync::mpsc;

struct Run {
    events: Vec<ProviderEvent>,
    client: Vec<Value>,
    entries: Vec<Value>,
}

fn turn_text(entries: &[Value], nth: usize) -> (String, String) {
    let starts: Vec<&Value> = entries
        .iter()
        .filter(|e| e["type"] == "expect_outbound" && e["frame"]["method"] == "turn/start")
        .map(|e| &e["frame"])
        .collect();
    let f = starts[nth];
    (f["params"]["input"][0]["text"].as_str().unwrap().to_string(), f["params"]["model"].as_str().unwrap().to_string())
}

async fn replay(
    name: &str,
    mode: RuntimeMode,
    interaction: InteractionMode,
    answer: impl FnMut(&RuntimeRequest) -> Option<RequestResponse>,
    on_started: impl FnMut() -> Option<Action>,
) -> Run {
    let entries = load(&format!("codex/{name}.ndjson"));
    let (text, model) = turn_text(&entries, 0);
    let (r, w, fake) = spawn_fake(Proto::Codex, entries.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = CodexSession::over_io(r, w, config(&model, "codex", mode), tx);
    session.handshake(true).await.unwrap();
    session.open_thread().await.unwrap();
    session.start_turn(turn("run-1", &text, &model, "codex", mode, interaction)).await.unwrap();
    let events = drive(&mut rx, &session, answer, on_started).await;
    session.shutdown().await;
    drop(session);
    let client = fake.await.unwrap().client_frames;
    Run { events, client, entries }
}

fn sent<'a>(client: &'a [Value], method: &str) -> Vec<&'a Value> {
    client.iter().filter(|f| f["method"] == method).collect()
}

#[tokio::test]
async fn simple_turn_streams_and_completes() {
    let run = replay("simple", RuntimeMode::FullAccess, InteractionMode::Default, |_| None, || None).await;
    let items = final_items(&run.events);
    assert_eq!(assistant_texts(&items), ["fixture simple ok"]);
    assert!(matches!(items[0].body, TurnItemBody::AssistantMessage { phase: Some(AssistantPhase::FinalAnswer), streaming: false, .. }));
    assert_eq!(items[0].ordinal, 1);
    assert_settled(&items);
    let (status, failure, broken) = terminal(&run.events);
    assert_eq!((status, failure, broken), (TerminalStatus::Completed, None, false));

    // TurnStarted carries the native turn id; usage is the per-turn delta.
    assert!(run.events.iter().any(|e| matches!(e, ProviderEvent::TurnStarted { native_turn_id: Some(id), .. } if id == "01a0d5eb-f176-71d0-8e1c-a459700443de")));
    let usage = run.events.iter().find_map(|e| match e {
        ProviderEvent::TurnUsage { usage: TurnTokenUsage::Complete(c), .. } => Some(c.clone()),
        _ => None,
    });
    let usage = usage.expect("complete usage");
    assert_eq!((usage.input_tokens, usage.output_tokens, usage.cached_input_tokens), (Some(22730), Some(7), Some(6912)));
    assert!(run.events.iter().any(|e| matches!(e, ProviderEvent::BindingUpdated { context_usage: Some(c), .. } if c.used_tokens == 22737)));
    assert!(run.events.iter().any(|e| matches!(e, ProviderEvent::BindingUpdated { native_thread_id: Some(id), status: ProviderThreadStatus::Idle, .. } if id == "01a0d5eb-f0c0-7481-a417-5e8871805cbc")));

    // Three deltas in the same millisecond arrive as fewer snapshots than deltas.
    let snapshots = run.events.iter().filter(|e| matches!(e, ProviderEvent::ItemUpserted(i) if i.id == items[0].id)).count();
    assert!(snapshots <= 3, "{snapshots} snapshots");

    // What we put on the wire.
    let init = sent(&run.client, "initialize")[0];
    assert_eq!(init["params"]["clientInfo"]["name"], "Tern");
    assert_eq!(init["params"]["capabilities"]["optOutNotificationMethods"], json!(["turn/diff/updated"]));
    assert_eq!(sent(&run.client, "initialized").len(), 1);
    let start = sent(&run.client, "thread/start")[0];
    assert_eq!(start["params"]["config"]["tools.update_plan.enabled"], true);
    let ts = sent(&run.client, "turn/start")[0];
    assert_eq!(ts["params"]["threadId"], "01a0d5eb-f0c0-7481-a417-5e8871805cbc");
    assert_eq!(ts["params"]["sandboxPolicy"], json!({"type": "dangerFullAccess"}));
    assert_eq!(ts["params"]["approvalPolicy"]["granular"]["sandbox_approval"], false);
    assert_eq!(ts["params"]["collaborationMode"]["mode"], "default");
    assert_eq!(ts["params"]["summary"], "auto");
}

#[tokio::test]
async fn file_change_approval_round_trips() {
    let mut opened = None;
    let run = replay(
        "tool_call_restricted_granular",
        RuntimeMode::ApprovalRequired,
        InteractionMode::Default,
        |r| {
            opened = Some(r.clone());
            Some(respond_with(r, ApprovalDecision::Accept))
        },
        || None,
    )
    .await;
    let req = opened.expect("approval opened");
    assert_eq!(req.kind, RequestKind::FileChange);
    assert_eq!(req.native_request_id, Some(json!(0)));
    let items = final_items(&run.events);
    let approval = items.iter().find(|i| i.id == req.turn_item_id).unwrap();
    match &approval.body {
        TurnItemBody::ApprovalRequest { request_id, prompt, .. } => {
            assert_eq!(request_id, &req.id);
            assert!(prompt.as_deref().unwrap().ends_with(".codex-probe-write-action.txt"));
        }
        other => panic!("{other:?}"),
    }
    let change = items.iter().find(|i| matches!(i.body, TurnItemBody::FileChange { .. })).unwrap();
    assert_eq!(change.status, ItemStatus::Completed);
    assert!(matches!(&change.body, TurnItemBody::FileChange { additions: Some(1), deletions: Some(0), .. }));
    // Our answer is exactly what T3 sent.
    let expected_answer = expected(&run.entries, |f| f.get("result").is_some() && f["id"] == 0).unwrap();
    let ours = run.client.iter().find(|f| f["id"] == 0 && f.get("result").is_some()).unwrap();
    assert_eq!(ours, expected_answer);
    assert!(items.iter().any(|i| matches!(&i.body, TurnItemBody::Reasoning { text, .. } if text.contains("Planning patch"))));
    assert_eq!(terminal(&run.events).0, TerminalStatus::Completed);
}

#[tokio::test]
async fn user_input_answers_use_codex_question_ids() {
    let mut questions = vec![];
    let run = replay(
        "plan_questions",
        RuntimeMode::FullAccess,
        InteractionMode::Default,
        |r| {
            assert_eq!(r.kind, RequestKind::UserInput);
            let mut answers = std::collections::HashMap::new();
            answers.insert("schema_preference".to_string(), json!("Strict schemas (Recommended)"));
            Some(RequestResponse { request: r.clone(), decision: None, feedback: None, answers: Some(answers) })
        },
        || None,
    )
    .await;
    for i in final_items(&run.events) {
        if let TurnItemBody::UserInputRequest { questions: q, .. } = i.body {
            questions = q;
        }
    }
    assert_eq!(questions[0].id, "schema_preference");
    assert!(!questions[0].options.is_empty());
    let expected_answer = expected(&run.entries, |f| f.get("result").is_some() && f["id"] == 0).unwrap();
    let ours = run.client.iter().find(|f| f["id"] == 0 && f.get("result").is_some()).unwrap();
    assert_eq!(ours, expected_answer);
    assert_eq!(terminal(&run.events).0, TerminalStatus::Completed);
}

#[tokio::test]
async fn interrupt_sends_turn_interrupt() {
    let run = replay("turn_interrupt", RuntimeMode::FullAccess, InteractionMode::Default, |_| None, || Some(Action::Interrupt)).await;
    let int = sent(&run.client, "turn/interrupt")[0];
    assert_eq!(int["params"], json!({"threadId": "01a0d5ed-3804-72d0-9a3f-929885a11f32", "turnId": "01a0d5ed-38b3-7a11-9203-bd9650a02764"}));
    assert_eq!(terminal(&run.events), (TerminalStatus::Interrupted, None, false));
}

#[tokio::test]
async fn steer_targets_the_running_turn() {
    let mut once = true;
    let run = replay(
        "message_steering",
        RuntimeMode::FullAccess,
        InteractionMode::Default,
        |_| None,
        || std::mem::take(&mut once).then(|| Action::Steer("Actually, respond with exactly: steering fixture observed".into())),
    )
    .await;
    let steer = sent(&run.client, "turn/steer")[0];
    assert_eq!(steer["params"]["expectedTurnId"], "01a0d5ed-670e-7032-9a87-707c736491a6");
    assert_eq!(steer["params"]["input"][0]["text"], "Actually, respond with exactly: steering fixture observed");
    let texts = assistant_texts(&final_items(&run.events));
    assert_eq!(texts, ["steering fixture initial response", "steering fixture observed"]);
}

#[tokio::test]
async fn plan_mode_streams_a_proposed_plan() {
    let run = replay("proposed_plan", RuntimeMode::FullAccess, InteractionMode::Plan, |_| None, || None).await;
    let ts = sent(&run.client, "turn/start")[0];
    assert_eq!(ts["params"]["collaborationMode"]["mode"], "plan");
    let updates: Vec<(String, bool)> = run
        .events
        .iter()
        .filter_map(|e| match e {
            ProviderEvent::PlanUpdated { markdown, streaming, .. } => Some((markdown.clone(), *streaming)),
            _ => None,
        })
        .collect();
    let (last, streaming) = updates.last().unwrap();
    assert!(!streaming);
    assert!(last.starts_with("1. **Find the replay boundary.**"));
    let plan = final_items(&run.events).into_iter().find(|i| matches!(i.body, TurnItemBody::ProposedPlan { .. })).unwrap();
    assert_eq!(plan.status, ItemStatus::Completed);
}

#[tokio::test]
async fn todo_updates_replace_the_list() {
    let run = replay("todo_list", RuntimeMode::FullAccess, InteractionMode::Default, |_| None, || None).await;
    let todos: Vec<Vec<PlanStep>> = run
        .events
        .iter()
        .filter_map(|e| match e {
            ProviderEvent::TodoUpdated { steps, .. } => Some(steps.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(todos.len(), 2);
    assert_eq!(todos[0][0].status, PlanStepStatus::Running);
    assert_eq!(todos[0][0].text, "Inspect package.json");
    assert!(todos[1].iter().all(|s| s.status == PlanStepStatus::Completed));
    let lists: Vec<TurnItem> = final_items(&run.events).into_iter().filter(|i| matches!(i.body, TurnItemBody::TodoList { .. })).collect();
    assert_eq!(lists.len(), 1, "one todo item, updated in place");
}

#[tokio::test]
async fn command_output_streams_live() {
    let run = replay("command_output_streaming", RuntimeMode::FullAccess, InteractionMode::Default, |_| None, || None).await;
    let items = final_items(&run.events);
    let cmd = items.iter().find(|i| matches!(i.body, TurnItemBody::CommandExecution { .. })).unwrap();
    let chunks: String = run
        .events
        .iter()
        .filter_map(|e| match e {
            ProviderEvent::CommandOutputDelta { item_id, chunk } if item_id == &cmd.id => Some(chunk.clone()),
            _ => None,
        })
        .collect();
    assert!(chunks.contains("stream line 4"));
    match &cmd.body {
        TurnItemBody::CommandExecution { exit_code, output, output_indicates_failure, .. } => {
            assert_eq!(*exit_code, Some(0));
            assert!(output.as_deref().unwrap().contains("progress 100%"));
            assert_eq!(*output_indicates_failure, Some(false));
        }
        _ => unreachable!(),
    }
    assert_settled(&items);
}

#[tokio::test]
async fn web_search_patterns_come_from_the_action() {
    let run = replay("web_search", RuntimeMode::FullAccess, InteractionMode::Default, |_| None, || None).await;
    let ws = final_items(&run.events).into_iter().find(|i| matches!(i.body, TurnItemBody::WebSearch { .. })).unwrap();
    assert!(matches!(&ws.body, TurnItemBody::WebSearch { patterns: Some(p), .. } if p[0] == "FIFA World Cup 2026 ticket prices official"));
}

#[tokio::test]
async fn rerouted_model_is_reported_and_second_turn_reuses_thread() {
    let entries = load("codex/codex_model_reroute.ndjson");
    let (r, w, fake) = spawn_fake(Proto::Codex, entries.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (text, model) = turn_text(&entries, 0);
    let session = CodexSession::over_io(r, w, config(&model, "codex", RuntimeMode::FullAccess), tx);
    session.handshake(true).await.unwrap();
    session.open_thread().await.unwrap();
    session.start_turn(turn("run-1", &text, &model, "codex", RuntimeMode::FullAccess, InteractionMode::Default)).await.unwrap();
    let first = drive(&mut rx, &session, |_| None, || None).await;
    assert!(first.iter().any(|e| matches!(e, ProviderEvent::TurnUsage { model: Some(m), .. } if m == "gpt-6-astra")));
    let (text2, model2) = turn_text(&entries, 1);
    session.start_turn(turn("run-2", &text2, &model2, "codex", RuntimeMode::FullAccess, InteractionMode::Default)).await.unwrap();
    let second = drive(&mut rx, &session, |_| None, || None).await;
    assert!(matches!(terminal(&second), (TerminalStatus::Completed, None, false)));
    assert!(second.iter().any(|e| matches!(e, ProviderEvent::TurnTerminal { run_id, .. } if run_id == "run-2")));
    session.shutdown().await;
    drop(session);
    let client = fake.await.unwrap().client_frames;
    assert_eq!(client.iter().filter(|f| f["method"] == "thread/start").count(), 1);
}

/// The one real turn captured from codex-cli 0.160.1: the provider rejected the request,
/// Codex retried five times and failed the turn.
#[tokio::test]
async fn live_0_160_failed_turn_reports_retries_and_failure() {
    let run = replay("live_failed_turn", RuntimeMode::ApprovalRequired, InteractionMode::Default, |_| None, || None).await;
    let items = final_items(&run.events);
    let retry = items.iter().find(|i| matches!(i.body, TurnItemBody::Error { .. })).expect("retry banner");
    match &retry.body {
        TurnItemBody::Error { retry: Some(r), failure } => {
            assert_eq!((r.attempt, r.max_attempts), (5, Some(5)));
            assert_eq!(failure.class, FailureClass::TransportError);
        }
        other => panic!("{other:?}"),
    }
    assert_settled(&items);
    let (status, failure, broken) = terminal(&run.events);
    assert_eq!((status, broken), (TerminalStatus::Failed, false));
    let failure = failure.unwrap();
    assert_eq!(failure.class, FailureClass::ProviderError);
    assert_eq!(failure.code.as_deref(), Some("httpConnectionFailed"));
    assert!(failure.message.contains("422 Unprocessable Entity"));
    let ts = sent(&run.client, "turn/start")[0];
    assert_eq!(ts["params"]["approvalPolicy"], "untrusted");
    assert_eq!(ts["params"]["sandboxPolicy"], json!({"type": "readOnly"}));
    assert!(matches!(run.events.iter().find(|e| matches!(e, ProviderEvent::TurnUsage { .. })), Some(ProviderEvent::TurnUsage { usage: TurnTokenUsage::Unavailable(_), .. })));
}

#[tokio::test]
async fn model_list_from_0_160_maps_to_picker_models() {
    let page: Value = serde_json::from_str(&std::fs::read_to_string(fixture("codex/model_list_0_160_1.json")).unwrap()).unwrap();
    let models = tern_providers::codex::models::map_models(&[page]);
    let astra = models.iter().find(|m| m.slug == "gpt-6-astra").unwrap();
    assert_eq!(astra.name, "GPT-6 Astra");
    assert_eq!(astra.short_name.as_deref(), Some("6 Astra"));
    assert!(astra.is_default, "astra beats Codex's own default");
    assert!(!models.iter().find(|m| m.slug == "gpt-6.1-sol").unwrap().is_default);
    let effort = &astra.options[0];
    assert_eq!(effort.id, "reasoningEffort");
    let labels: Vec<&str> = effort.choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["Low", "Medium", "High", "Extra High", "Max", "Ultra"]);
    assert!(effort.choices.iter().find(|c| c.value == "medium").unwrap().is_default, "astra forced to medium");
    let tier = astra.options.iter().find(|o| o.id == "serviceTier").unwrap();
    assert_eq!(tier.choices[0].label, "Standard");
    assert_eq!(tier.choices[1].label, "Fast");
    assert!(models.iter().find(|m| m.slug == "gpt-5.5").unwrap().is_legacy);
}

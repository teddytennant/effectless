//! Fake CLI peer that replays a recorded transcript over in-memory pipes.
//!
//! Transcripts use T3's testkit format: `expect_outbound` frames are what the client
//! should write, `emit_inbound` frames are what the CLI wrote. The fake walks the file in
//! order. It matches client frames loosely (method / type / subtype), rewrites response
//! ids to the ones the client actually used, answers client requests the recording does
//! not contain with an empty result, and skips ahead when the client omits a request
//! T3 made (e.g. `thread/name/set`). `server_exit` closes the pipe.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};
use tern_core::*;
use tern_providers::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};
use tokio::sync::mpsc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Proto {
    Codex,
    Claude,
}

pub fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(rel)
}

pub fn load(rel: &str) -> Vec<Value> {
    std::fs::read_to_string(fixture(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// The first expected client frame matching `pred`.
pub fn expected<'a>(entries: &'a [Value], pred: impl Fn(&Value) -> bool) -> Option<&'a Value> {
    entries
        .iter()
        .filter(|e| e["type"] == "expect_outbound")
        .map(|e| &e["frame"])
        .find(|f| pred(f))
}

fn key(proto: Proto, f: &Value) -> String {
    match proto {
        Proto::Codex => match (f.get("method").and_then(Value::as_str), f.get("id")) {
            (Some(m), Some(_)) => format!("req:{m}"),
            (Some(m), None) => format!("note:{m}"),
            (None, Some(id)) => format!("resp:{id}"),
            _ => "?".into(),
        },
        Proto::Claude => match f["type"].as_str() {
            Some("control_request") => format!("ctrl:{}", f["request"]["subtype"].as_str().unwrap_or("")),
            Some("control_response") => format!("ctrlresp:{}", f["response"]["request_id"].as_str().unwrap_or("")),
            Some(t) => t.to_string(),
            None => "?".into(),
        },
    }
}

pub struct Fake {
    /// Every frame the client wrote.
    pub client_frames: Vec<Value>,
}

/// Starts the fake peer; returns the client's ends of the pipe.
pub fn spawn_fake(proto: Proto, entries: Vec<Value>) -> (ReadHalf<DuplexStream>, WriteHalf<DuplexStream>, tokio::task::JoinHandle<Fake>) {
    let (client, server) = tokio::io::duplex(1 << 20);
    let (cr, cw) = tokio::io::split(client);
    let (sr, mut sw) = tokio::io::split(server);
    let handle = tokio::spawn(async move {
        let mut lines = BufReader::new(sr).lines();
        let mut log = Vec::new();
        let mut ids: HashMap<String, Value> = HashMap::new();
        let mut last_ctrl: Option<Value> = None;
        let mut i = 0;
        let script: Vec<(String, Value)> = entries
            .iter()
            .filter_map(|e| {
                let t = e["type"].as_str()?;
                matches!(t, "expect_outbound" | "emit_inbound" | "server_exit").then(|| (t.to_string(), e.get("frame").cloned().unwrap_or(Value::Null)))
            })
            .collect();
        macro_rules! write_frame {
            ($f:expr) => {{
                let s = format!("{}\n", $f);
                if sw.write_all(s.as_bytes()).await.is_err() {
                    return Fake { client_frames: log };
                }
            }};
        }
        while i < script.len() {
            let (ty, frame) = &script[i];
            match ty.as_str() {
                "server_exit" => {
                    let _ = sw.shutdown().await;
                    return Fake { client_frames: log };
                }
                "emit_inbound" => {
                    let mut f = frame.clone();
                    match proto {
                        Proto::Codex => {
                            if f.get("method").is_none() {
                                if let Some(mapped) = f.get("id").and_then(|id| ids.get(&id.to_string())) {
                                    f["id"] = mapped.clone();
                                }
                            }
                        }
                        Proto::Claude => {
                            if f["type"] == "control_response" {
                                let rid = f["response"]["request_id"].clone();
                                if let Some(m) = ids.get(&rid.to_string()).cloned().or_else(|| (rid == "*").then(|| last_ctrl.clone()).flatten()) {
                                    f["response"]["request_id"] = m;
                                }
                            }
                        }
                    }
                    write_frame!(f);
                    i += 1;
                }
                _ => {
                    let want = key(proto, frame);
                    let line = match tokio::time::timeout(Duration::from_secs(20), lines.next_line()).await {
                        Ok(Ok(Some(l))) => l,
                        _ => return Fake { client_frames: log },
                    };
                    let got: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("client wrote bad json {line}: {e}"));
                    log.push(got.clone());
                    let got_key = key(proto, &got);
                    if got_key == want {
                        match proto {
                            Proto::Codex => {
                                if let (Some(e), Some(g)) = (frame.get("id"), got.get("id")) {
                                    ids.insert(e.to_string(), g.clone());
                                }
                            }
                            Proto::Claude => {
                                if got["type"] == "control_request" {
                                    last_ctrl = Some(got["request_id"].clone());
                                    ids.insert(frame["request_id"].to_string(), got["request_id"].clone());
                                }
                            }
                        }
                        i += 1;
                        continue;
                    }
                    // The client skipped something T3 sent: jump to where this frame is expected.
                    let later = script[i..].iter().position(|(t, f)| t == "expect_outbound" && key(proto, f) == got_key);
                    if let Some(off) = later {
                        let (_, f) = &script[i + off];
                        match proto {
                            Proto::Codex => {
                                if let (Some(e), Some(g)) = (f.get("id"), got.get("id")) {
                                    ids.insert(e.to_string(), g.clone());
                                }
                            }
                            Proto::Claude => {
                                if got["type"] == "control_request" {
                                    last_ctrl = Some(got["request_id"].clone());
                                    ids.insert(f["request_id"].to_string(), got["request_id"].clone());
                                }
                            }
                        }
                        i += off + 1;
                        continue;
                    }
                    // Unrecorded request: answer it blandly.
                    match proto {
                        Proto::Codex if got.get("method").is_some() && got.get("id").is_some() => {
                            write_frame!(json!({ "id": got["id"], "result": {} }));
                        }
                        Proto::Claude if got["type"] == "control_request" => {
                            write_frame!(json!({ "type": "control_response", "response": { "subtype": "success", "request_id": got["request_id"], "response": {} } }));
                        }
                        _ => {}
                    }
                }
            }
        }
        // Script done: keep answering until the client hangs up.
        while let Ok(Ok(Some(line))) = tokio::time::timeout(Duration::from_secs(30), lines.next_line()).await {
            let Ok(got) = serde_json::from_str::<Value>(&line) else { continue };
            log.push(got.clone());
            if proto == Proto::Codex && got.get("method").is_some() && got.get("id").is_some() {
                write_frame!(json!({ "id": got["id"], "result": {} }));
            }
            if proto == Proto::Claude && got["type"] == "control_request" {
                write_frame!(json!({ "type": "control_response", "response": { "subtype": "success", "request_id": got["request_id"], "response": {} } }));
            }
        }
        Fake { client_frames: log }
    });
    (cr, cw, handle)
}

pub fn config(model: &str, instance: &str, mode: RuntimeMode) -> SessionConfig {
    SessionConfig {
        thread_id: "thread-1".into(),
        cwd: "/tmp".into(),
        resume_native_thread_id: None,
        model_selection: ModelSelection { instance_id: instance.into(), model: model.into(), options: vec![] },
        runtime_mode: mode,
        binary_path: None,
        env: HashMap::new(),
    }
}

pub fn turn(run_id: &str, text: &str, model: &str, instance: &str, mode: RuntimeMode, interaction: InteractionMode) -> TurnRequest {
    TurnRequest {
        run_id: run_id.into(),
        text: text.into(),
        attachments: vec![],
        model_selection: ModelSelection { instance_id: instance.into(), model: model.into(), options: vec![] },
        runtime_mode: mode,
        interaction_mode: interaction,
        cwd: "/tmp".into(),
    }
}

/// Collects events until `TurnTerminal` (or a timeout), answering requests with `answer`.
pub async fn drive(
    rx: &mut mpsc::UnboundedReceiver<ProviderEvent>,
    session: &dyn ProviderSessionHandle,
    mut answer: impl FnMut(&RuntimeRequest) -> Option<RequestResponse>,
    mut on_started: impl FnMut() -> Option<Action>,
) -> Vec<ProviderEvent> {
    let mut out = Vec::new();
    loop {
        let ev = match tokio::time::timeout(Duration::from_secs(20), rx.recv()).await {
            Ok(Some(ev)) => ev,
            _ => panic!("no TurnTerminal; got {:#?}", out),
        };
        out.push(ev.clone());
        match &ev {
            ProviderEvent::RequestOpened(r) => {
                if let Some(resp) = answer(r) {
                    session.respond(resp).await.unwrap();
                }
            }
            ProviderEvent::TurnStarted { run_id, .. } => match on_started() {
                Some(Action::Interrupt) => session.interrupt(run_id).await.unwrap(),
                Some(Action::Steer(t)) => session.steer(run_id, t).await.unwrap(),
                None => {}
            },
            ProviderEvent::TurnTerminal { .. } => return out,
            _ => {}
        }
    }
}

pub enum Action {
    Interrupt,
    Steer(String),
}

pub fn respond_with(r: &RuntimeRequest, decision: ApprovalDecision) -> RequestResponse {
    RequestResponse { request: r.clone(), decision: Some(decision), feedback: None, answers: None }
}

/// Latest snapshot of each item, in ordinal order.
pub fn final_items(events: &[ProviderEvent]) -> Vec<TurnItem> {
    let mut by_id: Vec<TurnItem> = Vec::new();
    for e in events {
        if let ProviderEvent::ItemUpserted(i) = e {
            match by_id.iter_mut().find(|x| x.id == i.id) {
                Some(x) => *x = i.clone(),
                None => by_id.push(i.clone()),
            }
        }
    }
    by_id.sort_by_key(|i| i.ordinal);
    by_id
}

pub fn terminal(events: &[ProviderEvent]) -> (TerminalStatus, Option<ProviderFailure>, bool) {
    events
        .iter()
        .find_map(|e| match e {
            ProviderEvent::TurnTerminal { status, failure, broken, .. } => Some((*status, failure.clone(), *broken)),
            _ => None,
        })
        .expect("terminal")
}

pub fn assistant_texts(items: &[TurnItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match &i.body {
            TurnItemBody::AssistantMessage { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// No item is left open or streaming once the turn ended.
pub fn assert_settled(items: &[TurnItem]) {
    for i in items {
        assert!(!i.status.is_open(), "item left open: {i:#?}");
        if let TurnItemBody::AssistantMessage { streaming, .. } | TurnItemBody::Reasoning { streaming, .. } | TurnItemBody::ProposedPlan { streaming, .. } = &i.body {
            assert!(!streaming, "item still streaming: {i:#?}");
        }
    }
}

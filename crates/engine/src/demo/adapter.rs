//! A fake provider for demo mode. New turns stream a short canned reply (reasoning, about two
//! seconds of tokens, a command, then completion). Threads seeded mid-run get a script that
//! continues them: keep streaming, finish after an approval, or answer a question.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use tern_core::*;
use tern_providers::{
    ProviderAdapter, ProviderSessionHandle, RequestResponse, SessionConfig, TurnRequest,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// What a seeded live thread does once its session opens.
#[derive(Clone, Debug)]
pub(crate) enum Script {
    /// Keep appending `text` to the streaming assistant item, feed live output into the
    /// running command, and finish after `complete_after` (never, if None).
    Stream {
        run_id: String,
        message_item: String,
        text: String,
        shown: usize,
        command: Option<(String, String)>,
        complete_after: Option<Duration>,
    },
    /// A pending command approval: accept runs the command, decline skips it.
    Approval {
        run_id: String,
        request_id: String,
        command: String,
    },
    /// A pending question: the answer turns into a proposed plan.
    Question { run_id: String, request_id: String },
}

pub(crate) type Scripts = Arc<Mutex<HashMap<String, Script>>>;

pub(crate) struct DemoAdapter {
    pub instance: &'static str,
    pub driver: Driver,
    pub scripts: Scripts,
}

fn capabilities() -> Capabilities {
    Capabilities {
        supports_interrupt: true,
        supports_active_steering: true,
        emits_proposed_plan: true,
        can_rollback_conversation: true,
        supports_model_switch_in_session: true,
        supports_runtime_mode_switch_in_session: true,
    }
}

fn choice(value: &str, label: &str, is_default: bool) -> ModelOptionChoice {
    ModelOptionChoice {
        value: value.into(),
        label: label.into(),
        is_default,
        description: None,
        prompt_injected: false,
    }
}

fn model(
    slug: &str,
    name: &str,
    short: &str,
    desc: &str,
    is_default: bool,
    options: Vec<ModelOptionDescriptor>,
) -> ProviderModel {
    ProviderModel {
        slug: slug.into(),
        name: name.into(),
        short_name: Some(short.into()),
        description: Some(desc.into()),
        is_default,
        is_custom: false,
        is_legacy: false,
        options,
    }
}

pub(crate) fn provider_status(instance: &str) -> ProviderStatus {
    if instance == "codex" {
        let effort = ModelOptionDescriptor {
            id: "reasoningEffort".into(),
            label: "Reasoning".into(),
            choices: vec![
                choice("low", "Low", false),
                choice("medium", "Medium", true),
                choice("high", "High", false),
                choice("xhigh", "Extra High", false),
            ],
            boolean: false,
        };
        let fast = ModelOptionDescriptor {
            id: "fastMode".into(),
            label: "Fast mode".into(),
            choices: vec![],
            boolean: true,
        };
        ProviderStatus {
            instance_id: "codex".into(),
            driver: Driver::Codex,
            display_name: "Codex".into(),
            enabled: true,
            binary_path: Some("/home/ada/.local/bin/codex".into()),
            version: Some("0.171.0".into()),
            auth: ProviderAuthState::SignedIn,
            account_label: Some("ChatGPT Pro Subscription".into()),
            models: vec![
                model(
                    "gpt-6-astra",
                    "GPT-6 Astra",
                    "Astra",
                    "Most capable coding model",
                    true,
                    vec![effort.clone(), fast],
                ),
                model(
                    "gpt-5.6-sol",
                    "GPT-5.6 Sol",
                    "Sol",
                    "Fast and steady for everyday edits",
                    false,
                    vec![effort.clone()],
                ),
                model(
                    "gpt-6-luna",
                    "GPT-6 Luna",
                    "Luna",
                    "Small and quick",
                    false,
                    vec![effort],
                ),
            ],
            capabilities: capabilities(),
            message: None,
        }
    } else {
        let effort = ModelOptionDescriptor {
            id: "effort".into(),
            label: "Effort".into(),
            choices: vec![
                choice("low", "Low", false),
                choice("medium", "Medium", false),
                choice("high", "High", true),
                choice("max", "Max", false),
            ],
            boolean: false,
        };
        let thinking = ModelOptionDescriptor {
            id: "thinking".into(),
            label: "Thinking".into(),
            choices: vec![],
            boolean: true,
        };
        ProviderStatus {
            instance_id: "claudeAgent".into(),
            driver: Driver::Claude,
            display_name: "Claude".into(),
            enabled: true,
            binary_path: Some("/home/ada/.local/bin/claude".into()),
            version: Some("2.3.14".into()),
            auth: ProviderAuthState::SignedIn,
            account_label: Some("Claude Max Subscription".into()),
            models: vec![
                model(
                    "claude-opus-5-5",
                    "Claude Opus 5.5",
                    "Opus 5.5",
                    "Deepest reasoning for hard problems",
                    true,
                    vec![effort.clone(), thinking.clone()],
                ),
                model(
                    "claude-sonnet-5",
                    "Claude Sonnet 5",
                    "Sonnet 5",
                    "Balanced speed and quality",
                    false,
                    vec![effort, thinking],
                ),
                model(
                    "claude-haiku-5-5",
                    "Claude Haiku 5.5",
                    "Haiku 5.5",
                    "Fastest responses",
                    false,
                    vec![],
                ),
            ],
            capabilities: capabilities(),
            message: None,
        }
    }
}

#[async_trait]
impl ProviderAdapter for DemoAdapter {
    fn driver(&self) -> Driver {
        self.driver
    }
    fn instance_id(&self) -> &str {
        self.instance
    }
    async fn status(&self, _binary_path: Option<PathBuf>) -> ProviderStatus {
        provider_status(self.instance)
    }
    async fn open_session(
        &self,
        config: SessionConfig,
        events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> anyhow::Result<Box<dyn ProviderSessionHandle>> {
        let script = self.scripts.lock().remove(&config.thread_id);
        let session = DemoSession {
            thread_id: config.thread_id.clone(),
            events: Emitter {
                tx: events,
                thread_id: config.thread_id,
            },
            task: Mutex::new(None),
            script: Mutex::new(None),
        };
        match script {
            Some(Script::Stream {
                run_id,
                message_item,
                text,
                shown,
                command,
                complete_after,
            }) => {
                let ev = session.events.clone();
                session.set_task(tokio::spawn(continue_stream(
                    ev,
                    run_id,
                    message_item,
                    text,
                    shown,
                    command,
                    complete_after,
                )));
            }
            other => *session.script.lock() = other,
        }
        Ok(Box::new(session))
    }
}

#[derive(Clone)]
struct Emitter {
    tx: mpsc::UnboundedSender<ProviderEvent>,
    thread_id: String,
}

impl Emitter {
    fn send(&self, ev: ProviderEvent) {
        let _ = self.tx.send(ev);
    }

    fn item(
        &self,
        run_id: &str,
        id: &str,
        status: ItemStatus,
        title: Option<&str>,
        body: TurnItemBody,
    ) {
        let t = now();
        self.send(ProviderEvent::ItemUpserted(TurnItem {
            id: id.into(),
            thread_id: self.thread_id.clone(),
            run_id: Some(run_id.into()),
            native_item_id: Some(format!("native-{id}")),
            parent_item_id: None,
            ordinal: 0,
            status,
            title: title.map(Into::into),
            started_at: Some(t),
            completed_at: (!status.is_open()).then_some(t),
            updated_at: t,
            body,
        }));
    }

    fn message(&self, run_id: &str, id: &str, text: &str, streaming: bool, phase: AssistantPhase) {
        self.item(
            run_id,
            id,
            if streaming {
                ItemStatus::Running
            } else {
                ItemStatus::Completed
            },
            None,
            TurnItemBody::AssistantMessage {
                message_id: id.into(),
                text: text.into(),
                streaming,
                phase: Some(phase),
            },
        );
    }

    /// Stream `text` word by word over roughly `total`.
    async fn stream_message(
        &self,
        run_id: &str,
        id: &str,
        text: &str,
        total: Duration,
        phase: AssistantPhase,
    ) {
        let words: Vec<&str> = text.split_inclusive(' ').collect();
        let step = total / words.len().max(1) as u32;
        let mut acc = String::new();
        for (i, w) in words.iter().enumerate() {
            acc.push_str(w);
            if i % 2 == 1 || i + 1 == words.len() {
                self.message(run_id, id, &acc, true, phase);
            }
            tokio::time::sleep(step).await;
        }
        self.message(run_id, id, text, false, phase);
    }

    async fn command(&self, run_id: &str, input: &str, lines: &[&str], exit_code: i32) {
        let id = new_id();
        let running = TurnItemBody::CommandExecution {
            input: input.into(),
            output: None,
            exit_code: None,
            output_indicates_failure: None,
        };
        self.item(
            run_id,
            &id,
            ItemStatus::Running,
            Some("Ran command"),
            running,
        );
        let mut out = String::new();
        for line in lines {
            tokio::time::sleep(Duration::from_millis(220)).await;
            let chunk = format!("{line}\n");
            out.push_str(&chunk);
            self.send(ProviderEvent::CommandOutputDelta {
                item_id: id.clone(),
                chunk,
            });
        }
        self.item(
            run_id,
            &id,
            if exit_code == 0 {
                ItemStatus::Completed
            } else {
                ItemStatus::Failed
            },
            Some("Ran command"),
            TurnItemBody::CommandExecution {
                input: input.into(),
                output: Some(out),
                exit_code: Some(exit_code),
                output_indicates_failure: Some(exit_code != 0),
            },
        );
    }

    fn usage(&self, run_id: &str, input: u64, output: u64) {
        self.send(ProviderEvent::TurnUsage {
            run_id: run_id.into(),
            usage: TurnTokenUsage::Complete(TokenCounts {
                input_tokens: Some(input),
                output_tokens: Some(output),
                cached_input_tokens: Some(input / 2),
                ..Default::default()
            }),
            model: None,
        });
        self.send(ProviderEvent::BindingUpdated {
            native_thread_id: Some(format!("demo-{}", self.thread_id)),
            status: ProviderThreadStatus::Active,
            context_usage: Some(ContextUsage {
                used_tokens: input + output,
                max_tokens: Some(400_000),
                input_tokens: Some(input),
                output_tokens: Some(output),
                ..Default::default()
            }),
        });
    }

    fn done(&self, run_id: &str) {
        self.send(ProviderEvent::TurnTerminal {
            run_id: run_id.into(),
            status: TerminalStatus::Completed,
            failure: None,
            reason: None,
            broken: false,
        });
    }
}

struct DemoSession {
    thread_id: String,
    events: Emitter,
    task: Mutex<Option<JoinHandle<()>>>,
    script: Mutex<Option<Script>>,
}

impl DemoSession {
    fn set_task(&self, task: JoinHandle<()>) {
        if let Some(old) = self.task.lock().replace(task) {
            old.abort();
        }
    }

    fn stop(&self) {
        if let Some(t) = self.task.lock().take() {
            t.abort();
        }
    }
}

#[async_trait]
impl ProviderSessionHandle for DemoSession {
    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    async fn start_turn(&self, turn: TurnRequest) -> anyhow::Result<()> {
        let ev = self.events.clone();
        self.set_task(tokio::spawn(generic_turn(ev, turn)));
        Ok(())
    }

    async fn steer(&self, run_id: &str, text: String) -> anyhow::Result<()> {
        let ev = self.events.clone();
        let run_id = run_id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let short: String = text.chars().take(60).collect();
            ev.message(
                &run_id,
                &new_id(),
                &format!("Got it, folding that in: *{short}*."),
                false,
                AssistantPhase::Commentary,
            );
        });
        Ok(())
    }

    async fn interrupt(&self, run_id: &str) -> anyhow::Result<()> {
        self.stop();
        self.events.send(ProviderEvent::TurnTerminal {
            run_id: run_id.into(),
            status: TerminalStatus::Interrupted,
            failure: None,
            reason: None,
            broken: false,
        });
        Ok(())
    }

    async fn respond(&self, response: RequestResponse) -> anyhow::Result<()> {
        let script = self.script.lock().take();
        let ev = self.events.clone();
        match script {
            Some(Script::Approval {
                run_id,
                request_id,
                command,
            }) if request_id == response.request.id => {
                self.set_task(tokio::spawn(after_approval(ev, run_id, command, response)));
            }
            Some(Script::Question { run_id, request_id }) if request_id == response.request.id => {
                self.set_task(tokio::spawn(after_question(ev, run_id, response)));
            }
            other => *self.script.lock() = other,
        }
        let _ = &self.thread_id;
        Ok(())
    }

    async fn set_model(&self, _selection: ModelSelection) -> anyhow::Result<()> {
        Ok(())
    }

    async fn shutdown(&self) {
        self.stop();
    }

    fn kill(&self) {
        self.stop();
    }
}

const REPLY: &str = "Sure. I read through the code involved before touching anything.\n\n\
## What I found\n\n\
- The entry point is `src/main.rs:42`, which hands off to `run()` in `src/app.rs`.\n\
- Nothing else depends on the behavior you want to change, so the edit stays local.\n\
- There's already a test module next to it I can extend.\n\n\
I'll run the tests first so we have a baseline.";

const FINAL: &str = "Tests pass. Here's the change in short:\n\n\
```rust\n\
pub fn run(config: Config) -> anyhow::Result<()> {\n\
    let app = App::new(config)?;\n\
    app.start()\n\
}\n\
```\n\n\
| Check | Result |\n\
|---|---|\n\
| `cargo test` | 38 passed |\n\
| `cargo clippy` | clean |\n\n\
Let me know if you want me to keep going.";

async fn generic_turn(ev: Emitter, turn: TurnRequest) {
    let run_id = turn.run_id.clone();
    ev.send(ProviderEvent::TurnStarted {
        run_id: run_id.clone(),
        native_turn_id: Some(format!("turn-{}", &new_id()[..8])),
    });
    tokio::time::sleep(Duration::from_millis(250)).await;

    let reasoning = new_id();
    let thought = "Reading the request and the files it points at before deciding what to change.";
    ev.item(
        &run_id,
        &reasoning,
        ItemStatus::Running,
        Some("Thinking"),
        TurnItemBody::Reasoning {
            text: thought.into(),
            streaming: true,
        },
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    ev.item(
        &run_id,
        &reasoning,
        ItemStatus::Completed,
        Some("Thinking"),
        TurnItemBody::Reasoning {
            text: thought.into(),
            streaming: false,
        },
    );

    if turn.interaction_mode == InteractionMode::Plan {
        let plan_id = new_id();
        let plan = "# Plan\n\n1. Read the current implementation and its tests.\n2. Make the change behind the existing interface.\n3. Extend the tests to cover the new path.\n4. Run `cargo test` and `cargo clippy`.\n";
        let mut acc = String::new();
        for line in plan.split_inclusive('\n') {
            acc.push_str(line);
            ev.send(ProviderEvent::PlanUpdated {
                plan_id: plan_id.clone(),
                markdown: acc.clone(),
                streaming: true,
            });
            tokio::time::sleep(Duration::from_millis(180)).await;
        }
        ev.send(ProviderEvent::PlanUpdated {
            plan_id,
            markdown: plan.into(),
            streaming: false,
        });
        ev.usage(&run_id, 18_400, 620);
        ev.done(&run_id);
        return;
    }

    ev.stream_message(
        &run_id,
        &new_id(),
        REPLY,
        Duration::from_millis(2000),
        AssistantPhase::Commentary,
    )
    .await;
    ev.command(
        &run_id,
        "cargo test --quiet",
        &[
            "running 38 tests",
            "......................................",
            "test result: ok. 38 passed; 0 failed; 0 ignored; finished in 0.84s",
        ],
        0,
    )
    .await;
    ev.stream_message(
        &run_id,
        &new_id(),
        FINAL,
        Duration::from_millis(900),
        AssistantPhase::FinalAnswer,
    )
    .await;
    ev.usage(&run_id, 24_310, 1_184);
    ev.done(&run_id);
}

async fn continue_stream(
    ev: Emitter,
    run_id: String,
    message_item: String,
    text: String,
    shown: usize,
    command: Option<(String, String)>,
    complete_after: Option<Duration>,
) {
    let started = tokio::time::Instant::now();
    if let Some((id, input)) = command {
        let lines = [
            "   Compiling tern v0.1.0 (/home/ada/code/tern/crates/app)",
            "    Finished `test` profile [unoptimized + debuginfo] target(s) in 6.42s",
            "     Running unittests src/main.rs (target/debug/deps/tern-5f1e2b7c9a0d3e41)",
            "",
            "running 9 tests",
            "test chat::markdown::tests::splits_finished_blocks ... ok",
            "test chat::markdown::tests::keeps_open_fence_in_tail ... ok",
            "test chat::markdown::tests::reuses_cached_blocks ... ok",
            "test chat::markdown::tests::tables_close_on_blank_line ... ok",
            "test chat::markdown::tests::list_continuation ... ok",
            "test chat::markdown::tests::nested_code_in_list ... ok",
            "test chat::markdown::tests::heading_after_paragraph ... ok",
            "test chat::markdown::tests::html_comment_is_text ... ok",
            "test chat::markdown::tests::streaming_tail_reparses_only_tail ... ok",
            "",
            "test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s",
        ];
        let ev2 = ev.clone();
        let rid = run_id.clone();
        tokio::spawn(async move {
            let mut out = String::new();
            for line in lines {
                tokio::time::sleep(Duration::from_millis(450)).await;
                let chunk = format!("{line}\n");
                out.push_str(&chunk);
                ev2.send(ProviderEvent::CommandOutputDelta {
                    item_id: id.clone(),
                    chunk,
                });
            }
            ev2.item(
                &rid,
                &id,
                ItemStatus::Completed,
                Some("Ran command"),
                TurnItemBody::CommandExecution {
                    input,
                    output: Some(out),
                    exit_code: Some(0),
                    output_indicates_failure: Some(false),
                },
            );
        });
    }
    let chars: Vec<char> = text.chars().collect();
    let mut i = shown.min(chars.len());
    while i < chars.len() {
        // a few words at a time, like a real token stream
        let mut j = i;
        let mut spaces = 0;
        while j < chars.len() && spaces < 3 {
            if chars[j] == ' ' || chars[j] == '\n' {
                spaces += 1;
            }
            j += 1;
        }
        i = j;
        let acc: String = chars[..i].iter().collect();
        ev.message(
            &run_id,
            &message_item,
            &acc,
            true,
            AssistantPhase::PartialAnswer,
        );
        tokio::time::sleep(Duration::from_millis(140)).await;
    }
    let Some(after) = complete_after else {
        // keep the run live (and the message streaming) until someone stops it
        std::future::pending::<()>().await;
        return;
    };
    tokio::time::sleep_until(started + after).await;
    ev.message(
        &run_id,
        &message_item,
        &text,
        false,
        AssistantPhase::FinalAnswer,
    );
    ev.usage(&run_id, 41_200, 2_310);
    ev.done(&run_id);
}

async fn after_approval(ev: Emitter, run_id: String, command: String, response: RequestResponse) {
    tokio::time::sleep(Duration::from_millis(300)).await;
    match response.decision {
        Some(
            ApprovalDecision::Accept
            | ApprovalDecision::AcceptForSession
            | ApprovalDecision::AcceptAlways,
        ) => {
            ev.command(
                &run_id,
                &command,
                &[
                    "Removed worktree /home/ada/.tern/worktrees/tern/t3-4f2a9c1e-8d1c0b6a7e2f4a19",
                    "Removed worktree /home/ada/.tern/worktrees/tern/t3-b81d07aa-3c9e2f1d0a4b5c6e",
                    "Removed worktree /home/ada/.tern/worktrees/tern/sidebar-nav-77e0a1c2d3b4f5a6",
                    "Deleted branch t3/4f2a9c1e (was 9c1e2f3).",
                    "Deleted branch t3/b81d07aa (was 4ab0d17).",
                    "Deleted branch sidebar-nav (was e02c6d1).",
                ],
                0,
            )
            .await;
            ev.stream_message(
                &run_id,
                &new_id(),
                "Done. I removed the three clean worktrees and their merged branches:\n\n\
                 - `t3/4f2a9c1e`\n- `t3/b81d07aa`\n- `sidebar-nav`\n\n\
                 `t3/diff-panel` still has uncommitted changes in `src/diff/view.rs`, so I left it alone.",
                Duration::from_millis(1500),
                AssistantPhase::FinalAnswer,
            )
            .await;
        }
        _ => {
            let extra = response
                .feedback
                .map(|f| format!(" You said: *{f}*."))
                .unwrap_or_default();
            ev.stream_message(
                &run_id,
                &new_id(),
                &format!("Okay, I didn't remove anything.{extra} The worktrees are still listed above if you want to clean them up by hand."),
                Duration::from_millis(900),
                AssistantPhase::FinalAnswer,
            )
            .await;
        }
    }
    ev.usage(&run_id, 12_900, 410);
    ev.done(&run_id);
}

fn answer_text(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        Some(serde_json::Value::Object(o)) => answer_text(o.get("answers")),
        _ => String::new(),
    }
}

async fn after_question(ev: Emitter, run_id: String, response: RequestResponse) {
    tokio::time::sleep(Duration::from_millis(300)).await;
    if response.decision == Some(ApprovalDecision::Cancel) {
        ev.stream_message(
            &run_id,
            &new_id(),
            "No problem. I'll default to Redis with a token bucket; tell me if you'd rather pick.",
            Duration::from_millis(900),
            AssistantPhase::FinalAnswer,
        )
        .await;
        ev.done(&run_id);
        return;
    }
    let answers = response.answers.unwrap_or_default();
    let store = answer_text(answers.get("store"));
    let algo = answer_text(answers.get("algorithm"));
    let store = if store.is_empty() {
        "Redis".into()
    } else {
        store
    };
    let algo = if algo.is_empty() {
        "Token bucket".into()
    } else {
        algo
    };
    let plan_id = new_id();
    let plan = format!(
        "# Rate limiting for the public API\n\n\
         **Store:** {store} · **Algorithm:** {algo}\n\n\
         ## Steps\n\n\
         1. Add `src/middleware/rateLimit.ts` with a `{algo}` limiter keyed by API key, falling back to client IP.\n\
         2. Back it with {store}; reuse the connection from `src/db/redis.ts` if that's the store.\n\
         3. Mount it in `src/middleware/index.ts` before auth so rejected requests stay cheap.\n\
         4. Return `429` with `Retry-After` and `X-RateLimit-Remaining` headers.\n\
         5. Limits: 600 req/min per key, 60 req/min per anonymous IP, configurable in `config/limits.ts`.\n\
         6. Tests: burst at the limit, refill over time, header values, anonymous fallback.\n\n\
         ## Out of scope\n\n\
         - Per-endpoint limits (we can add a table later)\n\
         - Billing-tier based limits\n"
    );
    let mut acc = String::new();
    for line in plan.split_inclusive('\n') {
        acc.push_str(line);
        ev.send(ProviderEvent::PlanUpdated {
            plan_id: plan_id.clone(),
            markdown: acc.clone(),
            streaming: true,
        });
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
    ev.send(ProviderEvent::PlanUpdated {
        plan_id,
        markdown: plan,
        streaming: false,
    });
    ev.usage(&run_id, 33_050, 1_402);
    ev.done(&run_id);
}

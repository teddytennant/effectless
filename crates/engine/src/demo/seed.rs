//! Demo projects and threads. Every `TurnItemBody` variant appears somewhere, and the threads
//! cover each sidebar state: running, needs approval, needs input, completed, failed on a usage
//! limit, interrupted, queued follow-up, pinned, settled, archived, unread, plan ready.

use std::collections::HashMap;

use chrono::Duration;
use serde_json::json;
use tern_core::*;
use tern_store::ThreadRows;

use super::adapter::Script;

pub(crate) struct SeedThread {
    pub thread: Thread,
    pub rows: ThreadRows,
    /// Checkpoint id -> cwd.
    pub cwd: String,
}

pub(crate) struct Seeded {
    pub projects: Vec<Project>,
    pub threads: Vec<SeedThread>,
    pub scripts: HashMap<String, Script>,
    /// Threads whose live run should get a session as soon as the engine starts.
    pub resume: Vec<(String, String)>,
}

fn sel(instance: &str, model: &str, options: &[(&str, &str)]) -> ModelSelection {
    ModelSelection {
        instance_id: instance.into(),
        model: model.into(),
        options: options
            .iter()
            .map(|(id, v)| OptionSelection {
                id: (*id).into(),
                value: OptionValue::Str((*v).into()),
            })
            .collect(),
    }
}

fn codex() -> ModelSelection {
    sel("codex", "gpt-6-astra", &[("reasoningEffort", "high")])
}

fn claude() -> ModelSelection {
    sel("claudeAgent", "claude-opus-5-5", &[("effort", "high")])
}

fn sonnet() -> ModelSelection {
    sel("claudeAgent", "claude-sonnet-5", &[("effort", "medium")])
}

fn diff_counts(diff: &str) -> (u32, u32) {
    let mut a = 0;
    let mut d = 0;
    for l in diff.lines() {
        if l.starts_with('+') && !l.starts_with("+++") {
            a += 1;
        } else if l.starts_with('-') && !l.starts_with("---") {
            d += 1;
        }
    }
    (a, d)
}

fn file(path: &str, kind: &str, additions: u32, deletions: u32) -> CheckpointFileSummary {
    CheckpointFileSummary {
        path: path.into(),
        kind: kind.into(),
        additions,
        deletions,
    }
}

/// Builds one thread with a moving clock.
struct B {
    thread: Thread,
    rows: ThreadRows,
    clock: Ts,
    run: Option<String>,
    cwd: String,
}

impl B {
    fn new(project: &Project, title: &str, model: ModelSelection, minutes_ago: i64) -> Self {
        let start = now() - Duration::minutes(minutes_ago);
        let id = new_id();
        Self {
            thread: Thread {
                id,
                project_id: project.id.clone(),
                title: title.into(),
                model_selection: model,
                runtime_mode: RuntimeMode::FullAccess,
                interaction_mode: InteractionMode::Default,
                branch: Some("main".into()),
                worktree_path: None,
                active_provider_thread_id: None,
                title_renamed: false,
                title_generation_started_at: None,
                created_at: start,
                updated_at: start,
                archived_at: None,
                pinned_at: None,
                settled_at: None,
                last_visited_at: None,
                linked_pull_request: None,
                deleted_at: None,
            },
            rows: ThreadRows::default(),
            clock: start,
            run: None,
            cwd: project.workspace_root.clone(),
        }
    }

    fn tick(&mut self, secs: i64) -> Ts {
        self.clock += Duration::seconds(secs);
        self.clock
    }

    fn next_ordinal(&self) -> u32 {
        self.rows.items.last().map_or(1, |i| i.ordinal + 1)
    }

    fn run_id(&self) -> String {
        self.run.clone().expect("no run")
    }

    fn push(&mut self, status: ItemStatus, title: Option<&str>, body: TurnItemBody) -> String {
        let t = self.tick(4);
        let id = new_id();
        self.rows.items.push(TurnItem {
            id: id.clone(),
            thread_id: self.thread.id.clone(),
            run_id: self.run.clone(),
            native_item_id: None,
            parent_item_id: None,
            ordinal: self.next_ordinal(),
            status,
            title: title.map(Into::into),
            started_at: Some(t),
            completed_at: (!status.is_open()).then_some(t),
            updated_at: t,
            body,
        });
        id
    }

    /// Start a run with a user message.
    fn user(&mut self, text: &str) -> String {
        self.user_with(text, vec![], None, RunStatus::Running)
    }

    fn user_with(
        &mut self,
        text: &str,
        attachments: Vec<ChatAttachment>,
        model_switch: Option<ModelSwitch>,
        status: RunStatus,
    ) -> String {
        let t = self.tick(20);
        let run_id = new_id();
        let message_id = new_id();
        let ordinal = self.rows.runs.iter().map(|r| r.ordinal).max().unwrap_or(0) + 1;
        let queued = status == RunStatus::Queued;
        self.rows.runs.push(Run {
            id: run_id.clone(),
            thread_id: self.thread.id.clone(),
            ordinal,
            model_selection: self.thread.model_selection.clone(),
            runtime_mode: self.thread.runtime_mode,
            interaction_mode: self.thread.interaction_mode,
            user_message_id: message_id.clone(),
            status,
            queue_position: queued.then_some(1),
            requested_at: t,
            started_at: (!queued).then_some(t),
            completed_at: None,
            checkpoint_id: None,
            source_plan_id: None,
            native_turn_id: (!queued).then(|| format!("turn-{ordinal}")),
            model_reported: None,
            token_usage: None,
            failure: None,
            interrupt_requested_at: None,
            interrupt_unresponsive_at: None,
        });
        let prev = self.run.replace(run_id.clone());
        self.push(
            ItemStatus::Completed,
            None,
            TurnItemBody::UserMessage {
                message_id,
                text: text.into(),
                attachments,
                input_intent: if queued {
                    InputIntent::QueuedTurn
                } else {
                    InputIntent::TurnStart
                },
                model_switch,
            },
        );
        if queued {
            self.run = prev;
        }
        run_id
    }

    fn reasoning(&mut self, text: &str) {
        self.push(
            ItemStatus::Completed,
            Some("Thinking"),
            TurnItemBody::Reasoning {
                text: text.into(),
                streaming: false,
            },
        );
    }

    fn say(&mut self, text: &str, phase: AssistantPhase) -> String {
        let id = new_id();
        self.push(
            ItemStatus::Completed,
            None,
            TurnItemBody::AssistantMessage {
                message_id: id,
                text: text.into(),
                streaming: false,
                phase: Some(phase),
            },
        )
    }

    fn cmd(&mut self, input: &str, output: &str, exit: i32) -> String {
        self.push(
            if exit == 0 {
                ItemStatus::Completed
            } else {
                ItemStatus::Failed
            },
            Some("Ran command"),
            TurnItemBody::CommandExecution {
                input: input.into(),
                output: Some(output.into()),
                exit_code: Some(exit),
                output_indicates_failure: Some(exit != 0),
            },
        )
    }

    fn edit(&mut self, path: &str, diff: &str, op: &str) {
        let (a, d) = diff_counts(diff);
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        self.push(
            ItemStatus::Completed,
            Some(if op == "add" {
                "Created file"
            } else {
                "Edited file"
            }),
            TurnItemBody::FileChange {
                file_name: name,
                additions: Some(a),
                deletions: Some(d),
                diff_str: Some(diff.into()),
                old_str: None,
                new_str: None,
                changes: Some(vec![FileChangeDetail {
                    operation: op.into(),
                    path: path.into(),
                    old_path: None,
                    file_type: Some("text".into()),
                    mime_type: None,
                }]),
            },
        );
    }

    fn search(&mut self, pattern: &str, results: &[(&str, u32, &str)]) {
        self.push(
            ItemStatus::Completed,
            Some("Searched files"),
            TurnItemBody::FileSearch {
                pattern: Some(pattern.into()),
                results: Some(
                    results
                        .iter()
                        .map(|(p, l, prev)| FileSearchResult {
                            path: (*p).into(),
                            line: Some(*l),
                            preview: Some((*prev).into()),
                        })
                        .collect(),
                ),
            },
        );
    }

    fn web(&mut self, patterns: &[&str], results: &[(&str, &str)]) {
        self.push(
            ItemStatus::Completed,
            Some("Searched the web"),
            TurnItemBody::WebSearch {
                patterns: Some(patterns.iter().map(|p| (*p).into()).collect()),
                results: Some(
                    results
                        .iter()
                        .map(|(title, url)| WebSearchResult {
                            title: Some((*title).into()),
                            url: (*url).into(),
                        })
                        .collect(),
                ),
            },
        );
    }

    fn end(&mut self, status: RunStatus, input: u64, output: u64) {
        let t = self.tick(6);
        let run_id = self.run_id();
        let r = self.rows.runs.iter_mut().find(|r| r.id == run_id).unwrap();
        r.status = status;
        r.completed_at = Some(t);
        r.token_usage = Some(TurnTokenUsage::Complete(TokenCounts {
            input_tokens: Some(input),
            output_tokens: Some(output),
            cached_input_tokens: Some(input * 3 / 5),
            ..Default::default()
        }));
        r.model_reported = Some(self.thread.model_selection.model.clone());
        self.thread.updated_at = t;
    }

    /// Checkpoint after the current run, plus the "N files changed" card.
    fn checkpoint(&mut self, files: Vec<CheckpointFileSummary>) {
        let run_id = self.run_id();
        let ordinal = self
            .rows
            .runs
            .iter()
            .find(|r| r.id == run_id)
            .unwrap()
            .ordinal;
        if !self
            .rows
            .checkpoints
            .iter()
            .any(|c| c.ordinal == ordinal - 1)
        {
            self.rows.checkpoints.push(Checkpoint {
                id: crate::checkpoint_id(&self.thread.id, ordinal - 1),
                thread_id: self.thread.id.clone(),
                run_id: None,
                ordinal: ordinal - 1,
                git_ref: format!(
                    "refs/tern/checkpoints/{}/ordinal/{}",
                    &self.thread.id[..8],
                    ordinal - 1
                ),
                status: CheckpointStatus::Ready,
                files: vec![],
                captured_at: self.clock,
            });
        }
        let id = crate::checkpoint_id(&self.thread.id, ordinal);
        let t = self.tick(1);
        self.rows.checkpoints.push(Checkpoint {
            id: id.clone(),
            thread_id: self.thread.id.clone(),
            run_id: Some(run_id.clone()),
            ordinal,
            git_ref: format!(
                "refs/tern/checkpoints/{}/ordinal/{ordinal}",
                &self.thread.id[..8]
            ),
            status: CheckpointStatus::Ready,
            files: files.clone(),
            captured_at: t,
        });
        self.rows
            .runs
            .iter_mut()
            .find(|r| r.id == run_id)
            .unwrap()
            .checkpoint_id = Some(id.clone());
        let total = crate::shell::totals(&files);
        self.push(
            ItemStatus::Completed,
            None,
            TurnItemBody::Checkpoint {
                checkpoint_id: id,
                files,
                files_total: Some(total),
            },
        );
    }

    fn visited(&mut self) {
        self.thread.last_visited_at = Some(self.clock + Duration::minutes(1));
    }

    fn finish(self) -> SeedThread {
        SeedThread {
            thread: self.thread,
            rows: self.rows,
            cwd: self.cwd,
        }
    }
}

fn project(title: &str, root: &str, days_ago: i64, scripts: Vec<ProjectScript>) -> Project {
    let t = now() - Duration::days(days_ago);
    Project {
        id: new_id(),
        title: title.into(),
        has_custom_title: false,
        workspace_root: root.into(),
        default_model_selection: None,
        default_thread_env_mode: None,
        favicon_path: None,
        scripts,
        pinned_at: None,
        archived_at: None,
        created_at: t,
        updated_at: t,
    }
}

fn script(id: &str, name: &str, command: &str, icon: ProjectScriptIcon) -> ProjectScript {
    ProjectScript {
        id: id.into(),
        name: name.into(),
        command: command.into(),
        icon,
        run_on_worktree_create: false,
    }
}

pub(crate) fn seed() -> Seeded {
    let tern = project(
        "tern",
        "/home/ada/code/tern",
        21,
        vec![
            script(
                "test",
                "Test",
                "cargo test --workspace",
                ProjectScriptIcon::Test,
            ),
            script(
                "run",
                "Run",
                "cargo run -p tern -- --demo",
                ProjectScriptIcon::Play,
            ),
            script(
                "lint",
                "Lint",
                "cargo clippy --workspace -- -D warnings",
                ProjectScriptIcon::Lint,
            ),
        ],
    );
    let harbor = project(
        "harbor-api",
        "/home/ada/code/harbor-api",
        60,
        vec![
            script("dev", "Dev", "pnpm dev", ProjectScriptIcon::Play),
            script("test", "Test", "pnpm vitest run", ProjectScriptIcon::Test),
        ],
    );
    let dotfiles = project("dotfiles", "/home/ada/dotfiles", 200, vec![]);

    let mut scripts = HashMap::new();
    let mut resume = Vec::new();
    let mut threads = Vec::new();

    // 1. running, streaming
    {
        let mut b = B::new(&tern, "Stream markdown in the chat timeline", claude(), 3);
        b.thread.branch = Some("t3/stream-md".into());
        b.thread.worktree_path =
            Some("/home/ada/.tern/worktrees/tern/t3-stream-md-2c41f0a9d8e7b6c5".into());
        b.cwd = b.thread.worktree_path.clone().unwrap();
        let run = b.user(
            "The chat timeline re-renders the whole assistant message on every token. Make streaming append-only: parse finished blocks once and only re-layout the tail block.",
        );
        b.reasoning(
            "The renderer calls `parse_markdown(&message.text)` inside `render()`, so every delta reparses the full string. Finished blocks never change once a blank line or a closing fence follows them. I can split at the last safe boundary, cache the parsed prefix keyed by its byte length, and only reparse the tail.",
        );
        b.push(
            ItemStatus::Running,
            None,
            TurnItemBody::TodoList {
                plan_id: new_id(),
                steps: vec![
                    PlanStep {
                        id: "1".into(),
                        text: "Find where markdown is parsed".into(),
                        status: PlanStepStatus::Completed,
                        duration_anchor_at: None,
                        duration_ms: Some(8_200),
                    },
                    PlanStep {
                        id: "2".into(),
                        text: "Split finished blocks from the open tail".into(),
                        status: PlanStepStatus::Completed,
                        duration_anchor_at: None,
                        duration_ms: Some(41_500),
                    },
                    PlanStep {
                        id: "3".into(),
                        text: "Cache parsed blocks per message".into(),
                        status: PlanStepStatus::Running,
                        duration_anchor_at: Some((now() - Duration::seconds(50)).to_rfc3339()),
                        duration_ms: None,
                    },
                    PlanStep {
                        id: "4".into(),
                        text: "Benchmark with a 2,000-line answer".into(),
                        status: PlanStepStatus::Pending,
                        duration_anchor_at: None,
                        duration_ms: None,
                    },
                ],
                explanation: Some(
                    "Keep the public render API the same; only the internals change.".into(),
                ),
            },
        );
        b.search(
            "parse_markdown",
            &[
                (
                    "crates/app/src/chat/markdown.rs",
                    31,
                    "pub fn parse_markdown(text: &str) -> Vec<Block> {",
                ),
                (
                    "crates/app/src/chat/message.rs",
                    118,
                    "let blocks = parse_markdown(&self.text);",
                ),
                (
                    "crates/app/src/chat/plan_card.rs",
                    54,
                    "let blocks = parse_markdown(&plan.markdown);",
                ),
            ],
        );
        b.cmd(
            "cargo bench -p tern --bench markdown -- --save-baseline before",
            "    Finished `bench` profile [optimized] target(s) in 41.18s\n     Running benches/markdown.rs (target/release/deps/markdown-a1f3c9e2)\nstream_2000_lines       time:   [184.21 ms 186.07 ms 188.11 ms]\nstream_200_lines        time:   [2.0412 ms 2.0533 ms 2.0671 ms]\n",
            0,
        );
        b.edit(
            "crates/app/src/chat/markdown.rs",
            "diff --git a/crates/app/src/chat/markdown.rs b/crates/app/src/chat/markdown.rs\nindex 61c0a4e..d2f9b17 100644\n--- a/crates/app/src/chat/markdown.rs\n+++ b/crates/app/src/chat/markdown.rs\n@@ -28,6 +28,32 @@ pub enum Block {\n     Table(Table),\n }\n \n+/// Parsed blocks of a message that is still streaming. Everything before\n+/// `stable_len` is final; only the tail is reparsed on each delta.\n+#[derive(Default)]\n+pub struct StreamingBlocks {\n+    stable: Vec<Block>,\n+    stable_len: usize,\n+}\n+\n+impl StreamingBlocks {\n+    pub fn update(&mut self, text: &str) -> impl Iterator<Item = &Block> {\n+        let boundary = last_safe_boundary(text);\n+        if boundary > self.stable_len {\n+            self.stable.extend(parse_markdown(&text[self.stable_len..boundary]));\n+            self.stable_len = boundary;\n+        }\n+        self.stable.iter()\n+    }\n+}\n+\n+/// Byte offset after the last blank line that is not inside a code fence.\n+fn last_safe_boundary(text: &str) -> usize {\n+    let mut in_fence = false;\n+    let mut boundary = 0;\n+    for (i, line) in text.split_inclusive('\\n').scan(0, |o, l| { let s = *o; *o += l.len(); Some((s, l)) }) {\n+        if line.trim_start().starts_with(\"```\") { in_fence = !in_fence; }\n+        if !in_fence && line.trim().is_empty() { boundary = i + line.len(); }\n+    }\n+    boundary\n+}\n+\n pub fn parse_markdown(text: &str) -> Vec<Block> {\n",
            "update",
        );
        let cmd_id = b.push(
            ItemStatus::Running,
            Some("Running command"),
            TurnItemBody::CommandExecution {
                input: "cargo test -p tern chat::markdown".into(),
                output: None,
                exit_code: None,
                output_indicates_failure: None,
            },
        );
        let full = STREAMING_ANSWER.to_string();
        let shown = full.find("## Numbers").unwrap_or(full.len() / 3);
        let partial: String = full[..shown].to_string();
        let msg_id = new_id();
        b.rows.items.push(TurnItem {
            id: msg_id.clone(),
            thread_id: b.thread.id.clone(),
            run_id: Some(run.clone()),
            native_item_id: None,
            parent_item_id: None,
            ordinal: b.next_ordinal(),
            status: ItemStatus::Running,
            title: None,
            started_at: Some(now() - Duration::seconds(20)),
            completed_at: None,
            updated_at: now(),
            body: TurnItemBody::AssistantMessage {
                message_id: msg_id.clone(),
                text: partial,
                streaming: true,
                phase: Some(AssistantPhase::PartialAnswer),
            },
        });
        b.rows.binding = Some(ProviderBinding {
            thread_id: b.thread.id.clone(),
            driver: Driver::Claude,
            native_thread_id: Some("0b7e5a51-9d1f-4c6a-8f0e-2d3c4b5a6978".into()),
            status: ProviderThreadStatus::Active,
            context_usage: Some(ContextUsage {
                used_tokens: 61_840,
                max_tokens: Some(400_000),
                input_tokens: Some(58_200),
                cached_input_tokens: Some(41_000),
                output_tokens: Some(3_640),
                reasoning_output_tokens: Some(1_120),
                last_used_tokens: Some(61_840),
                auto_compact_threshold: Some(320_000),
            }),
            updated_at: now(),
        });
        b.thread.last_visited_at = Some(now() - Duration::seconds(30));
        scripts.insert(
            b.thread.id.clone(),
            Script::Stream {
                run_id: run.clone(),
                message_item: msg_id,
                text: full,
                shown,
                command: Some((cmd_id, "cargo test -p tern chat::markdown".into())),
                complete_after: None,
            },
        );
        resume.push((b.thread.id.clone(), run));
        threads.push(b.finish());
    }

    // 2. waiting on a command approval
    {
        let mut b = B::new(
            &tern,
            "Clean up stale worktrees",
            sel("codex", "gpt-5.6-sol", &[("reasoningEffort", "medium")]),
            9,
        );
        b.thread.runtime_mode = RuntimeMode::ApprovalRequired;
        let run = b.user("Clean up the stale worktrees under ~/.tern/worktrees/tern. Keep anything with uncommitted changes.");
        b.reasoning("List the worktrees, check each for local changes and whether its branch is merged into main, then remove only the clean, merged ones.");
        b.cmd(
            "git worktree list --porcelain",
            "worktree /home/ada/code/tern\nHEAD 3f0e9a1c7b2d4e6f8a9b0c1d2e3f4a5b6c7d8e9f\nbranch refs/heads/main\n\nworktree /home/ada/.tern/worktrees/tern/t3-4f2a9c1e-8d1c0b6a7e2f4a19\nHEAD 9c1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e\nbranch refs/heads/t3/4f2a9c1e\n\nworktree /home/ada/.tern/worktrees/tern/t3-b81d07aa-3c9e2f1d0a4b5c6e\nHEAD 4ab0d17e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c\nbranch refs/heads/t3/b81d07aa\n\nworktree /home/ada/.tern/worktrees/tern/sidebar-nav-77e0a1c2d3b4f5a6\nHEAD e02c6d1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d\nbranch refs/heads/sidebar-nav\n\nworktree /home/ada/.tern/worktrees/tern/t3-diff-panel-0a9b8c7d6e5f4a3b\nHEAD 71d2c3b4a5f6e7d8c9b0a1f2e3d4c5b6a7f8e9d0\nbranch refs/heads/t3/diff-panel\n",
            0,
        );
        b.cmd(
            "for w in ~/.tern/worktrees/tern/*; do printf '%s ' \"$(basename $w)\"; git -C \"$w\" status --porcelain | wc -l; done; git branch --merged main",
            "sidebar-nav-77e0a1c2d3b4f5a6 0\nt3-4f2a9c1e-8d1c0b6a7e2f4a19 0\nt3-b81d07aa-3c9e2f1d0a4b5c6e 0\nt3-diff-panel-0a9b8c7d6e5f4a3b 2\n  sidebar-nav\n* main\n  t3/4f2a9c1e\n  t3/b81d07aa\n",
            0,
        );
        b.say(
            "Three worktrees are clean and their branches are merged into `main`. `t3/diff-panel` has 2 uncommitted files, so I'll keep it. Removing the other three needs your approval.",
            AssistantPhase::Commentary,
        );
        let request_id = new_id();
        let command = "git worktree remove ~/.tern/worktrees/tern/t3-4f2a9c1e-8d1c0b6a7e2f4a19 && git worktree remove ~/.tern/worktrees/tern/t3-b81d07aa-3c9e2f1d0a4b5c6e && git worktree remove ~/.tern/worktrees/tern/sidebar-nav-77e0a1c2d3b4f5a6 && git branch -d t3/4f2a9c1e t3/b81d07aa sidebar-nav";
        let item = b.push(
            ItemStatus::Waiting,
            Some("Run command?"),
            TurnItemBody::ApprovalRequest {
                request_id: request_id.clone(),
                request_kind: RequestKind::Command,
                prompt: Some(command.into()),
                reason: Some("Removes 3 clean worktrees and deletes their merged branches.".into()),
                full_access_override: None,
                options: Some(vec![
                    ApprovalOption {
                        decision: ApprovalDecision::Accept,
                        label: "Yes".into(),
                        warning: None,
                    },
                    ApprovalOption {
                        decision: ApprovalDecision::AcceptForSession,
                        label: "Yes, and don't ask again this session".into(),
                        warning: None,
                    },
                    ApprovalOption {
                        decision: ApprovalDecision::Decline,
                        label: "No, and tell Codex what to do differently".into(),
                        warning: None,
                    },
                ]),
            },
        );
        b.rows.requests.push(RuntimeRequest {
            id: request_id.clone(),
            thread_id: b.thread.id.clone(),
            run_id: Some(run.clone()),
            turn_item_id: item,
            native_request_id: Some(json!(17)),
            kind: RequestKind::Command,
            status: RequestStatus::Pending,
            live_session_id: None,
            created_at: b.clock,
            resolved_at: None,
            decision: None,
            feedback: None,
            answers: None,
        });
        b.thread.last_visited_at = Some(b.clock - Duration::minutes(2));
        scripts.insert(
            b.thread.id.clone(),
            Script::Approval {
                run_id: run.clone(),
                request_id,
                command: command.into(),
            },
        );
        resume.push((b.thread.id.clone(), run));
        threads.push(b.finish());
    }

    // 3. waiting on a question with two parts
    {
        let mut b = B::new(
            &harbor,
            "Pick a rate limiter for the public API",
            claude(),
            14,
        );
        b.thread.interaction_mode = InteractionMode::Plan;
        let run = b.user("We need rate limiting on the public API before launch. Look at how middleware is set up and propose an approach.");
        b.reasoning("Middleware is composed in src/middleware/index.ts. Sessions already use Redis via src/db/redis.ts, which matters for where counters live. Two choices drive the design: the store and the algorithm. Both are product calls, so ask.");
        b.search(
            "app.use(",
            &[
                ("src/middleware/index.ts", 12, "app.use(requestId());"),
                (
                    "src/middleware/index.ts",
                    13,
                    "app.use(auth({ optional: true }));",
                ),
                ("src/server.ts", 41, "app.use(\"/v1\", v1Router);"),
            ],
        );
        b.cmd(
            "sed -n '1,40p' src/middleware/index.ts",
            "import type { Hono } from \"hono\";\nimport { auth } from \"./auth\";\nimport { cors } from \"./cors\";\nimport { requestId } from \"./requestId\";\nimport { logger } from \"./logger\";\n\nexport function mountMiddleware(app: Hono) {\n  app.use(logger());\n  app.use(cors());\n\n  // order matters: auth needs the request id for audit logs\n  app.use(requestId());\n  app.use(auth({ optional: true }));\n}\n",
            0,
        );
        b.say(
            "Two decisions shape this, so I'll ask before writing the plan.",
            AssistantPhase::Commentary,
        );
        let request_id = new_id();
        let item = b.push(
            ItemStatus::Waiting,
            Some("Questions"),
            TurnItemBody::UserInputRequest {
                request_id: request_id.clone(),
                questions: vec![
                    UserInputQuestion {
                        id: "store".into(),
                        header: "Store".into(),
                        question: "Where should rate limit counters live?".into(),
                        options: vec![
                            QuestionOption {
                                label: "Redis".into(),
                                description: Some(
                                    "Shared across instances. We already run Redis for sessions."
                                        .into(),
                                ),
                                value: None,
                                preview: None,
                            },
                            QuestionOption {
                                label: "In-memory".into(),
                                description: Some(
                                    "Simplest, but each instance counts on its own.".into(),
                                ),
                                value: None,
                                preview: None,
                            },
                            QuestionOption {
                                label: "Postgres".into(),
                                description: Some(
                                    "No new infrastructure, adds a write per request.".into(),
                                ),
                                value: None,
                                preview: None,
                            },
                        ],
                        multi_select: Some(false),
                        allow_custom_answer: Some(true),
                        initial_answer: None,
                        required: Some(true),
                    },
                    UserInputQuestion {
                        id: "algorithm".into(),
                        header: "Algorithm".into(),
                        question: "Which limiting algorithm should we use?".into(),
                        options: vec![
                            QuestionOption {
                                label: "Token bucket".into(),
                                description: Some(
                                    "Allows short bursts, then a steady rate.".into(),
                                ),
                                value: None,
                                preview: None,
                            },
                            QuestionOption {
                                label: "Sliding window".into(),
                                description: Some("Smoother limits, slightly more storage.".into()),
                                value: None,
                                preview: None,
                            },
                            QuestionOption {
                                label: "Fixed window".into(),
                                description: Some("Cheapest, but bursty at window edges.".into()),
                                value: None,
                                preview: None,
                            },
                        ],
                        multi_select: Some(false),
                        allow_custom_answer: Some(true),
                        initial_answer: None,
                        required: Some(true),
                    },
                ],
                question_answer: None,
            },
        );
        b.rows.requests.push(RuntimeRequest {
            id: request_id.clone(),
            thread_id: b.thread.id.clone(),
            run_id: Some(run.clone()),
            turn_item_id: item,
            native_request_id: Some(json!("toolu_01HkV2mN8qR4tY6uW8xZ0aB2")),
            kind: RequestKind::UserInput,
            status: RequestStatus::Pending,
            live_session_id: None,
            created_at: b.clock,
            resolved_at: None,
            decision: None,
            feedback: None,
            answers: None,
        });
        b.thread.last_visited_at = Some(b.clock - Duration::minutes(1));
        scripts.insert(
            b.thread.id.clone(),
            Script::Question {
                run_id: run.clone(),
                request_id,
            },
        );
        resume.push((b.thread.id.clone(), run));
        threads.push(b.finish());
    }

    // 4. completed, big transcript
    threads.push(diff_parser_thread(&tern));

    // 5. failed on a usage limit
    {
        let mut b = B::new(&harbor, "Nightly billing export job", codex(), 38);
        b.thread.branch = Some("t3/billing-export".into());
        b.user("Write the nightly billing export job: dump yesterday's invoices to S3 as parquet, partitioned by day. Reuse the S3 client in src/lib/s3.ts.");
        b.reasoning("Need the invoice query, a parquet writer and a cron entry. Check what's already in the repo.");
        b.cmd(
            "ls src/jobs && rg -n \"parquet\" package.json",
            "cleanupSessions.ts\nindex.ts\nsendDunningEmails.ts\n",
            0,
        );
        b.edit(
            "src/jobs/exportInvoices.ts",
            "diff --git a/src/jobs/exportInvoices.ts b/src/jobs/exportInvoices.ts\nnew file mode 100644\nindex 0000000..5be1a2c\n--- /dev/null\n+++ b/src/jobs/exportInvoices.ts\n@@ -0,0 +1,14 @@\n+import { s3 } from \"../lib/s3\";\n+import { db } from \"../db\";\n+\n+export async function exportInvoices(day: Date) {\n+  const rows = await db.invoice.findMany({\n+    where: { issuedAt: { gte: startOfDay(day), lt: endOfDay(day) } },\n+  });\n+  const key = `exports/invoices/dt=${isoDate(day)}/part-0000.parquet`;\n+  await s3.putObject({\n+    Bucket: process.env.EXPORT_BUCKET!,\n+    Key: key,\n+    Body: await toParquet(rows),\n+  });\n+}\n",
            "add",
        );
        let reset = (now() + Duration::minutes(137)).to_rfc3339();
        let fail = ProviderFailure {
            class: FailureClass::UsageLimit,
            message: "You've hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/codex/settings/usage to purchase more credits or try again at 4:12 PM.".into(),
            code: Some("usage_limit_reached".into()),
            retryable: Some(false),
            reset_at: Some(reset),
        };
        b.push(
            ItemStatus::Failed,
            None,
            TurnItemBody::Error {
                failure: fail.clone(),
                retry: None,
            },
        );
        b.end(RunStatus::Failed, 88_200, 2_950);
        let rid = b.run_id();
        b.rows
            .runs
            .iter_mut()
            .find(|r| r.id == rid)
            .unwrap()
            .failure = Some(fail);
        b.visited();
        threads.push(b.finish());
    }

    // 6. interrupted
    {
        let mut b = B::new(&dotfiles, "Speed up fish startup", sonnet(), 64);
        b.user("fish takes about 900ms to start. Find out why and fix it.");
        b.cmd(
            "fish --profile-startup /tmp/fish.prof -i -c exit; sort -nk2 /tmp/fish.prof | tail -6",
            "Time\tSum\tCommand\n1204\t18311\t> source /home/ada/.config/fish/conf.d/direnv.fish\n2381\t40292\t> starship init fish | source\n3012\t61120\t> zoxide init fish | source\n9877\t104562\t> source /home/ada/.config/fish/conf.d/nix-index.fish\n512330\t612904\t> nvm use default --silent\n",
            0,
        );
        b.reasoning("`nvm use default` accounts for ~610ms of the ~900ms. nix-index adds another 100ms because it builds the command-not-found handler eagerly. Both can be lazy.");
        b.say(
            "Most of the time is `nvm use default` in `conf.d/nvm.fish` (about 610ms). The rest is mostly `nix-index` building its handler at startup. Let me check how often you actually need node in a fresh shell before changing it.",
            AssistantPhase::Commentary,
        );
        b.push(
            ItemStatus::Interrupted,
            Some("Ran command"),
            TurnItemBody::CommandExecution {
                input: "rg -c 'node|npm|pnpm' ~/.local/share/fish/fish_history".into(),
                output: Some("".into()),
                exit_code: None,
                output_indicates_failure: None,
            },
        );
        b.push(
            ItemStatus::Completed,
            Some("Interrupt requested"),
            TurnItemBody::RunInterruptRequest {
                message: crate::USER_STOP_REASON.into(),
                unresponsive_at: None,
            },
        );
        b.push(
            ItemStatus::Interrupted,
            Some("Interrupted"),
            TurnItemBody::RunInterruptResult {
                message: crate::USER_STOP_REASON.into(),
            },
        );
        let rid = b.run_id();
        b.end(RunStatus::Interrupted, 21_400, 860);
        b.rows
            .runs
            .iter_mut()
            .find(|r| r.id == rid)
            .unwrap()
            .interrupt_requested_at = Some(b.clock);
        b.checkpoint(vec![]);
        b.visited();
        threads.push(b.finish());
    }

    // 7. running with a queued follow-up
    {
        let mut b = B::new(&harbor, "Add cursor pagination to /v1/invoices", codex(), 2);
        b.thread.branch = Some("t3/invoice-pagination".into());
        let run = b.user("Add cursor pagination to GET /v1/invoices. Opaque cursor, default page size 50, max 200.");
        b.reasoning("Order by (issued_at, id) so the cursor is stable under ties. Encode both in a base64url JSON cursor.");
        b.edit(
            "src/routes/v1/invoices.ts",
            "diff --git a/src/routes/v1/invoices.ts b/src/routes/v1/invoices.ts\nindex 0c2d9e1..8a4f7b3 100644\n--- a/src/routes/v1/invoices.ts\n+++ b/src/routes/v1/invoices.ts\n@@ -9,11 +9,24 @@ const router = new Hono();\n \n router.get(\"/\", async (c) => {\n-  const invoices = await db.invoice.findMany({\n-    where: { accountId: c.var.accountId },\n-    orderBy: { issuedAt: \"desc\" },\n-  });\n-  return c.json({ data: invoices });\n+  const limit = clamp(Number(c.req.query(\"limit\") ?? 50), 1, 200);\n+  const cursor = decodeCursor(c.req.query(\"cursor\"));\n+  const rows = await db.invoice.findMany({\n+    where: {\n+      accountId: c.var.accountId,\n+      ...(cursor && { OR: [\n+        { issuedAt: { lt: cursor.issuedAt } },\n+        { issuedAt: cursor.issuedAt, id: { lt: cursor.id } },\n+      ] }),\n+    },\n+    orderBy: [{ issuedAt: \"desc\" }, { id: \"desc\" }],\n+    take: limit + 1,\n+  });\n+  const page = rows.slice(0, limit);\n+  const next = rows.length > limit ? encodeCursor(page[page.length - 1]) : null;\n+  return c.json({ data: page, nextCursor: next });\n });\n",
            "update",
        );
        let msg_id = new_id();
        let text = PAGINATION_ANSWER.to_string();
        let shown = text.len() / 4;
        let shown = (0..=shown)
            .rev()
            .find(|i| text.is_char_boundary(*i))
            .unwrap_or(0);
        b.rows.items.push(TurnItem {
            id: msg_id.clone(),
            thread_id: b.thread.id.clone(),
            run_id: Some(run.clone()),
            native_item_id: None,
            parent_item_id: None,
            ordinal: b.next_ordinal(),
            status: ItemStatus::Running,
            title: None,
            started_at: Some(now() - Duration::seconds(5)),
            completed_at: None,
            updated_at: now(),
            body: TurnItemBody::AssistantMessage {
                message_id: msg_id.clone(),
                text: text[..shown].to_string(),
                streaming: true,
                phase: Some(AssistantPhase::PartialAnswer),
            },
        });
        b.user_with(
            "Also update the OpenAPI spec and regenerate the TypeScript client.",
            vec![],
            None,
            RunStatus::Queued,
        );
        b.thread.last_visited_at = Some(now() - Duration::seconds(10));
        scripts.insert(
            b.thread.id.clone(),
            Script::Stream {
                run_id: run.clone(),
                message_item: msg_id,
                text,
                shown,
                command: None,
                complete_after: Some(std::time::Duration::from_secs(25)),
            },
        );
        resume.push((b.thread.id.clone(), run));
        threads.push(b.finish());
    }

    // 8. pinned
    {
        let mut b = B::new(&tern, "Sidebar keyboard navigation", claude(), 26 * 60);
        b.user("Add keyboard navigation to the sidebar: up/down moves between threads, enter opens, F2 renames.");
        b.search(
            "on_key_down",
            &[(
                "crates/app/src/sidebar/mod.rs",
                212,
                ".on_key_down(cx.listener(Self::on_key))",
            )],
        );
        b.edit(
            "crates/app/src/sidebar/mod.rs",
            "diff --git a/crates/app/src/sidebar/mod.rs b/crates/app/src/sidebar/mod.rs\nindex 2b9e0f1..c41d8a2 100644\n--- a/crates/app/src/sidebar/mod.rs\n+++ b/crates/app/src/sidebar/mod.rs\n@@ -210,6 +210,19 @@ impl Sidebar {\n     fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {\n-        let _ = (ev, window, cx);\n+        match ev.keystroke.key.as_str() {\n+            \"up\" => self.move_selection(-1, cx),\n+            \"down\" => self.move_selection(1, cx),\n+            \"enter\" => self.open_selected(window, cx),\n+            \"f2\" => self.rename_selected(window, cx),\n+            _ => return,\n+        }\n+        cx.stop_propagation();\n     }\n",
            "update",
        );
        b.cmd("cargo test -p tern sidebar", "running 4 tests\ntest sidebar::tests::wraps_at_end ... ok\ntest sidebar::tests::skips_collapsed_groups ... ok\ntest sidebar::tests::enter_opens ... ok\ntest sidebar::tests::f2_starts_rename ... ok\n\ntest result: ok. 4 passed; 0 failed; 0 ignored\n", 0);
        b.say(
            "Up/Down move through visible rows (collapsed groups are skipped), Enter opens the thread and F2 starts an inline rename. Selection wraps at both ends. The handler lives in `crates/app/src/sidebar/mod.rs:212`.",
            AssistantPhase::FinalAnswer,
        );
        b.end(RunStatus::Completed, 30_120, 1_204);
        b.checkpoint(vec![
            file("crates/app/src/sidebar/mod.rs", "modified", 9, 1),
            file("crates/app/src/sidebar/tests.rs", "modified", 38, 0),
        ]);
        b.thread.pinned_at = Some(b.clock);
        b.visited();
        threads.push(b.finish());
    }

    // 9. settled
    {
        let mut b = B::new(
            &dotfiles,
            "Move to home-manager flakes",
            codex(),
            3 * 24 * 60,
        );
        b.user("Convert my home-manager setup from channels to a flake. Keep the same modules.");
        b.edit(
            "flake.nix",
            "diff --git a/flake.nix b/flake.nix\nnew file mode 100644\nindex 0000000..a3c41e2\n--- /dev/null\n+++ b/flake.nix\n@@ -0,0 +1,16 @@\n+{\n+  inputs = {\n+    nixpkgs.url = \"github:NixOS/nixpkgs/nixos-unstable\";\n+    home-manager.url = \"github:nix-community/home-manager\";\n+    home-manager.inputs.nixpkgs.follows = \"nixpkgs\";\n+  };\n+\n+  outputs = { nixpkgs, home-manager, ... }: {\n+    homeConfigurations.ada = home-manager.lib.homeManagerConfiguration {\n+      pkgs = nixpkgs.legacyPackages.x86_64-linux;\n+      modules = [ ./home.nix ];\n+    };\n+  };\n+}\n",
            "add",
        );
        b.cmd("home-manager switch --flake .#ada", "Starting Home Manager activation\nActivating checkFilesChanged\nActivating checkLinkTargets\nActivating writeBoundary\nActivating linkGeneration\nCreating home file links in /home/ada\nActivating onFilesChange\n", 0);
        b.say("Done: `flake.nix` pins nixpkgs and home-manager, and `home-manager switch --flake .#ada` activates cleanly. You can drop the channels with `nix-channel --remove home-manager`.", AssistantPhase::FinalAnswer);
        b.end(RunStatus::Completed, 19_002, 640);
        b.checkpoint(vec![
            file("flake.nix", "added", 14, 0),
            file("flake.lock", "added", 48, 0),
        ]);
        b.visited();
        b.thread.settled_at = Some(b.clock + Duration::hours(2));
        threads.push(b.finish());
    }

    // 10. archived
    {
        let mut b = B::new(&tern, "Spike: webview renderer", sonnet(), 9 * 24 * 60);
        b.user("Quick spike: could we render the chat in a webview instead of native text? Just measure, don't build it.");
        b.say("A webview would cost about 80MB RSS per window and 300ms first paint. Native text layout is already fast enough after caching shaped lines. Not worth it.", AssistantPhase::FinalAnswer);
        b.end(RunStatus::Completed, 8_100, 410);
        b.visited();
        b.thread.archived_at = Some(b.clock + Duration::days(1));
        threads.push(b.finish());
    }

    // 11. unread
    {
        let mut b = B::new(&harbor, "Fix flaky webhook retry test", sonnet(), 7);
        b.user(
            "tests/webhooks/retry.test.ts fails about 1 in 10 runs on CI. Find out why and fix it.",
        );
        b.cmd(
            "pnpm vitest run tests/webhooks/retry.test.ts --repeat 20",
            " ❯ tests/webhooks/retry.test.ts (60 tests | 3 failed)\n   × retries with exponential backoff (repeat 7)\n     → expected 3 calls, received 2\n   × retries with exponential backoff (repeat 13)\n     → expected 3 calls, received 2\n   × retries with exponential backoff (repeat 18)\n     → expected 3 calls, received 2\n\n Test Files  1 failed (1)\n      Tests  3 failed | 57 passed (60)\n",
            1,
        );
        b.reasoning("The test advances fake timers by exactly the backoff sum, but the retry adds jitter with Math.random(). When jitter rounds up, the third attempt lands after the advanced time.");
        b.edit(
            "tests/webhooks/retry.test.ts",
            "diff --git a/tests/webhooks/retry.test.ts b/tests/webhooks/retry.test.ts\nindex 7d1e3a0..5c2b9f4 100644\n--- a/tests/webhooks/retry.test.ts\n+++ b/tests/webhooks/retry.test.ts\n@@ -18,7 +18,9 @@ describe(\"webhook retries\", () => {\n   it(\"retries with exponential backoff\", async () => {\n+    vi.spyOn(Math, \"random\").mockReturnValue(0.5);\n     const send = vi.fn().mockRejectedValueOnce(err).mockRejectedValueOnce(err).mockResolvedValue(ok);\n     const p = deliver(hook, { send });\n-    await vi.advanceTimersByTimeAsync(1_000 + 2_000);\n+    // base delays plus the fixed 50% jitter\n+    await vi.advanceTimersByTimeAsync(1_500 + 3_000);\n     await p;\n     expect(send).toHaveBeenCalledTimes(3);\n",
            "update",
        );
        b.cmd("pnpm vitest run tests/webhooks/retry.test.ts --repeat 50", " ✓ tests/webhooks/retry.test.ts (150 tests) 412ms\n\n Test Files  1 passed (1)\n      Tests  150 passed (150)\n", 0);
        b.say(
            "The flake came from jitter: `deliver()` multiplies each backoff by `1 + Math.random()`, but the test advanced fake timers by the base delays only. When jitter pushed the third attempt past that window, the test saw 2 calls.\n\nI pinned `Math.random` to `0.5` in the test and advance by the jittered total. 150 repeats pass now.",
            AssistantPhase::FinalAnswer,
        );
        b.end(RunStatus::Completed, 26_880, 930);
        b.checkpoint(vec![file("tests/webhooks/retry.test.ts", "modified", 3, 1)]);
        b.thread.last_visited_at = Some(b.thread.created_at);
        threads.push(b.finish());
    }

    // 12. plan ready to implement
    {
        let mut b = B::new(&tern, "Plan the settings page", claude(), 27);
        b.thread.interaction_mode = InteractionMode::Plan;
        b.user("Plan the settings page: modal with a left nav, General and Keybindings first. Don't write code yet.");
        b.search(
            "settings",
            &[
                ("spec/06-settings.md", 21, "### 1.1 Modal"),
                (
                    "crates/core/src/settings.rs",
                    1,
                    "//! Settings, client settings and keybindings",
                ),
                (
                    "crates/app/src/main.rs",
                    14,
                    "gpui_platform::application().run(|cx: &mut App| {",
                ),
            ],
        );
        b.reasoning("The spec fixes the modal layout. Settings types already exist in tern-core with load/save, so the page only needs views and an update path through the engine.");
        let plan_id = new_id();
        b.push(
            ItemStatus::Completed,
            None,
            TurnItemBody::ProposedPlan {
                plan_id: plan_id.clone(),
                markdown: SETTINGS_PLAN.into(),
                streaming: false,
            },
        );
        b.end(RunStatus::Completed, 44_300, 1_870);
        b.rows.plans.push(ProposedPlan {
            id: plan_id,
            thread_id: b.thread.id.clone(),
            run_id: Some(b.run_id()),
            markdown: SETTINGS_PLAN.into(),
            status: PlanStatus::Active,
            created_at: b.clock,
            updated_at: b.clock,
        });
        b.visited();
        threads.push(b.finish());
    }

    // 13. long investigation with a compaction and a provider stop
    {
        let mut b = B::new(&harbor, "Investigate slow p99 on /search", sonnet(), 5 * 60);
        b.user(
            "p99 on GET /v1/search jumped from 180ms to 1.4s after Tuesday's deploy. Find out why.",
        );
        b.cmd(
            "psql \"$DATABASE_URL\" -c \"EXPLAIN (ANALYZE, BUFFERS) SELECT id, title FROM documents WHERE account_id = 42 AND tsv @@ plainto_tsquery('english', 'quarterly report') ORDER BY updated_at DESC LIMIT 20;\"",
            "                                                     QUERY PLAN\n--------------------------------------------------------------------------------------------------------------------\n Limit  (cost=0.43..1893.22 rows=20 width=52) (actual time=1312.402..1312.441 rows=20 loops=1)\n   Buffers: shared hit=81231 read=40211\n   ->  Index Scan Backward using documents_updated_at_idx on documents  (cost=0.43..412933.10 rows=4363 width=52) (actual time=1312.400..1312.437 rows=20 loops=1)\n         Filter: ((account_id = 42) AND (tsv @@ '''quarter'' & ''report'''::tsquery))\n         Rows Removed by Filter: 918442\n Planning Time: 0.412 ms\n Execution Time: 1312.502 ms\n(7 rows)\n",
            0,
        );
        b.web(
            &["postgres planner chooses order by index over gin index limit", "postgres combine gin tsvector with btree account_id"],
            &[
                ("PostgreSQL: Documentation: 18: 12.9. Preferred Index Types for Text Search", "https://www.postgresql.org/docs/current/textsearch-indexes.html"),
                ("Why is Postgres not using my GIN index with ORDER BY LIMIT?", "https://dba.stackexchange.com/questions/239372/postgres-order-by-limit-gin-index"),
                ("btree_gin: GIN operator classes with B-tree behavior", "https://www.postgresql.org/docs/current/btree-gin.html"),
            ],
        );
        b.say(
            "The planner switched to walking `documents_updated_at_idx` backwards and filtering 918k rows, because Tuesday's migration dropped `documents_account_tsv_idx`. With `LIMIT 20` it bets on finding matches early, which fails for rare terms.",
            AssistantPhase::Commentary,
        );
        b.push(
            ItemStatus::Completed,
            Some(crate::PROVIDER_STOP_REASON),
            TurnItemBody::SystemNotice {
                message: "Reached the 50 tool call limit for one turn.".into(),
                level: Some(NoticeLevel::Warning),
            },
        );
        b.end(RunStatus::Completed, 182_400, 4_120);
        b.checkpoint(vec![]);
        let from = b.thread.model_selection.clone();
        b.thread.model_selection = claude();
        b.user_with(
            "Keep going. Write the migration to restore the index and check the plan again.",
            vec![],
            Some(ModelSwitch { from, to: claude() }),
            RunStatus::Running,
        );
        b.push(
            ItemStatus::Completed,
            Some("Context compacted"),
            TurnItemBody::Compaction {
                summary: Some("Investigated /v1/search p99 regression. Root cause: migration 0142 dropped documents_account_tsv_idx; the planner now scans documents_updated_at_idx backwards and filters ~918k rows for rare terms. Next: restore a composite GIN index (btree_gin) on (account_id, tsv) and re-run EXPLAIN.".into()),
                before_token_count: Some(182_400),
                after_token_count: Some(31_200),
            },
        );
        b.edit(
            "migrations/0147_restore_search_index.sql",
            "diff --git a/migrations/0147_restore_search_index.sql b/migrations/0147_restore_search_index.sql\nnew file mode 100644\nindex 0000000..e81f0c2\n--- /dev/null\n+++ b/migrations/0147_restore_search_index.sql\n@@ -0,0 +1,4 @@\n+CREATE EXTENSION IF NOT EXISTS btree_gin;\n+\n+CREATE INDEX CONCURRENTLY IF NOT EXISTS documents_account_tsv_idx\n+  ON documents USING gin (account_id, tsv);\n",
            "add",
        );
        b.cmd(
            "pnpm db:migrate && psql \"$DATABASE_URL\" -c \"EXPLAIN ANALYZE SELECT id FROM documents WHERE account_id = 42 AND tsv @@ plainto_tsquery('english', 'quarterly report') ORDER BY updated_at DESC LIMIT 20;\" | tail -3",
            "Applied 0147_restore_search_index.sql (8.4s)\n Planning Time: 0.388 ms\n Execution Time: 3.912 ms\n(9 rows)\n",
            0,
        );
        b.say(SEARCH_ANSWER, AssistantPhase::FinalAnswer);
        b.end(RunStatus::Completed, 64_900, 2_310);
        b.checkpoint(vec![file(
            "migrations/0147_restore_search_index.sql",
            "added",
            4,
            0,
        )]);
        b.visited();
        threads.push(b.finish());
    }

    // 14. an answered question and a declined approval
    {
        let mut b = B::new(
            &dotfiles,
            "Choose a bootstrap script",
            sonnet(),
            2 * 24 * 60,
        );
        b.thread.runtime_mode = RuntimeMode::AutoAcceptEdits;
        let run = b.user_with(
            "Write a bootstrap script for a fresh machine. Here's what my old one looked like.",
            vec![ChatAttachment::File {
                id: new_id(),
                name: "bootstrap-old.sh".into(),
                mime_type: "text/x-shellscript".into(),
                size_bytes: 2_431,
            }],
            None,
            RunStatus::Running,
        );
        let qid = new_id();
        let mut answers = HashMap::new();
        answers.insert("shell".to_string(), json!("bash"));
        let item = b.push(
            ItemStatus::Completed,
            Some("Questions"),
            TurnItemBody::UserInputRequest {
                request_id: qid.clone(),
                questions: vec![UserInputQuestion {
                    id: "shell".into(),
                    header: "Shell".into(),
                    question: "Which shell should the script target?".into(),
                    options: vec![
                        QuestionOption {
                            label: "bash".into(),
                            description: Some("Present on every machine you listed".into()),
                            value: Some("bash".into()),
                            preview: None,
                        },
                        QuestionOption {
                            label: "fish".into(),
                            description: Some(
                                "Matches your login shell, but needs installing first".into(),
                            ),
                            value: Some("fish".into()),
                            preview: None,
                        },
                    ],
                    multi_select: Some(false),
                    allow_custom_answer: Some(false),
                    initial_answer: None,
                    required: Some(true),
                }],
                question_answer: Some(UserInputAnswerPayload {
                    request_id: qid.clone(),
                    question_text_by_id: Some(HashMap::from([(
                        "shell".to_string(),
                        "Which shell should the script target?".to_string(),
                    )])),
                    answers: answers.clone(),
                    attachments_by_question_id: HashMap::new(),
                }),
            },
        );
        b.rows.requests.push(RuntimeRequest {
            id: qid,
            thread_id: b.thread.id.clone(),
            run_id: Some(run.clone()),
            turn_item_id: item,
            native_request_id: None,
            kind: RequestKind::UserInput,
            status: RequestStatus::Resolved,
            live_session_id: None,
            created_at: b.clock,
            resolved_at: Some(b.clock + Duration::seconds(40)),
            decision: None,
            feedback: None,
            answers: Some(answers),
        });
        b.edit(
            "bootstrap.sh",
            "diff --git a/bootstrap.sh b/bootstrap.sh\nnew file mode 100755\nindex 0000000..f19a2b3\n--- /dev/null\n+++ b/bootstrap.sh\n@@ -0,0 +1,9 @@\n+#!/usr/bin/env bash\n+set -euo pipefail\n+\n+command -v nix >/dev/null || sh <(curl -L https://nixos.org/nix/install) --daemon\n+git clone https://github.com/ada/dotfiles ~/dotfiles 2>/dev/null || git -C ~/dotfiles pull\n+cd ~/dotfiles\n+nix run home-manager/master -- switch --flake .#ada\n+chsh -s \"$(command -v fish)\"\n+echo \"done, open a new terminal\"\n",
            "add",
        );
        let aid = new_id();
        let approval = b.push(
            ItemStatus::Completed,
            Some("Run command?"),
            TurnItemBody::ApprovalRequest {
                request_id: aid.clone(),
                request_kind: RequestKind::Command,
                prompt: Some("bash ./bootstrap.sh".into()),
                reason: Some("Test the script on this machine".into()),
                full_access_override: None,
                options: None,
            },
        );
        b.rows.requests.push(RuntimeRequest {
            id: aid,
            thread_id: b.thread.id.clone(),
            run_id: Some(run),
            turn_item_id: approval,
            native_request_id: None,
            kind: RequestKind::Command,
            status: RequestStatus::Resolved,
            live_session_id: None,
            created_at: b.clock,
            resolved_at: Some(b.clock + Duration::seconds(12)),
            decision: Some(ApprovalDecision::Decline),
            feedback: Some("Don't run it here, this machine is already set up.".into()),
            answers: None,
        });
        b.say("Fair, I won't run it here. `bootstrap.sh` is idempotent: it skips the Nix install when `nix` exists and pulls instead of cloning when `~/dotfiles` is there, so it's safe to re-run on a new machine.", AssistantPhase::FinalAnswer);
        b.end(RunStatus::Completed, 15_230, 702);
        b.checkpoint(vec![file("bootstrap.sh", "added", 9, 0)]);
        b.visited();
        threads.push(b.finish());
    }

    Seeded {
        projects: vec![tern, harbor, dotfiles],
        threads,
        scripts,
        resume,
    }
}

fn diff_parser_thread(tern: &Project) -> SeedThread {
    let mut b = B::new(tern, "Port the diff parser to Rust", codex(), 130);
    b.thread.branch = Some("t3/diff-parser".into());
    b.user("Port `parseUnifiedDiff` from apps/web/src/lib/diff.ts to Rust in crates/git/src/diff.rs. Keep the hunk header parsing identical and add tests for renames and binary files.");
    b.reasoning("The TypeScript parser is a line-oriented state machine: `diff --git` starts a file, `---`/`+++` set paths, `@@` starts a hunk and resets both line counters, and `+`, `-`, ` ` lines advance one or both counters. Renames come from `rename from`/`rename to`; binary files from the `Binary files ... differ` line. Port it one to one, then compare on the same fixtures.");
    b.search(
        "parseUnifiedDiff",
        &[
            (
                "apps/web/src/lib/diff.ts",
                12,
                "export function parseUnifiedDiff(text: string): FileDiff[] {",
            ),
            (
                "apps/web/src/lib/diff.test.ts",
                3,
                "import { parseUnifiedDiff } from \"./diff\";",
            ),
            (
                "apps/web/src/components/DiffPanel.tsx",
                88,
                "const files = useMemo(() => parseUnifiedDiff(patch), [patch]);",
            ),
        ],
    );
    b.push(
        ItemStatus::Completed,
        None,
        TurnItemBody::TodoList {
            plan_id: new_id(),
            steps: vec![
                PlanStep {
                    id: "1".into(),
                    text: "Read the TypeScript parser and its fixtures".into(),
                    status: PlanStepStatus::Completed,
                    duration_anchor_at: None,
                    duration_ms: Some(12_400),
                },
                PlanStep {
                    id: "2".into(),
                    text: "Port the hunk state machine".into(),
                    status: PlanStepStatus::Completed,
                    duration_anchor_at: None,
                    duration_ms: Some(96_000),
                },
                PlanStep {
                    id: "3".into(),
                    text: "Handle renames and binary files".into(),
                    status: PlanStepStatus::Completed,
                    duration_anchor_at: None,
                    duration_ms: Some(38_700),
                },
                PlanStep {
                    id: "4".into(),
                    text: "Add tests and run clippy".into(),
                    status: PlanStepStatus::Completed,
                    duration_anchor_at: None,
                    duration_ms: Some(51_200),
                },
            ],
            explanation: None,
        },
    );
    b.edit("crates/git/src/diff.rs", PARSER_DIFF, "update");
    b.cmd(
        "cargo test -p tern-git diff",
        "   Compiling tern-git v0.1.0 (/home/ada/code/tern/crates/git)\n    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.81s\n     Running unittests src/lib.rs (target/debug/deps/tern_git-7c2f1a9e0b3d4e5f)\n\nrunning 14 tests\ntest diff::tests::parses_single_hunk ... ok\ntest diff::tests::parses_multiple_files ... ok\ntest diff::tests::counts_additions_and_deletions ... ok\ntest diff::tests::no_newline_marker_is_skipped ... ok\ntest diff::tests::binary_file ... ok\ntest diff::tests::pure_rename ... FAILED\ntest diff::tests::rename_with_edits ... FAILED\ntest diff::tests::mode_change_only ... ok\n\nfailures:\n\n---- diff::tests::pure_rename stdout ----\nthread 'diff::tests::pure_rename' panicked at crates/git/src/diff.rs:212:9:\nassertion `left == right` failed\n  left: \"b/new_name.rs\"\n right: \"new_name.rs\"\n\nfailures:\n    diff::tests::pure_rename\n    diff::tests::rename_with_edits\n\ntest result: FAILED. 12 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n\nerror: test failed, to rerun pass `-p tern-git --lib`\n",
        101,
    );
    b.say(
        "Two rename tests fail: `rename to` lines carry the bare path, but I was stripping the `b/` prefix only for `+++` lines and reusing that helper. Fixing the prefix handling.",
        AssistantPhase::Commentary,
    );
    b.edit(
        "crates/git/src/diff.rs",
        "diff --git a/crates/git/src/diff.rs b/crates/git/src/diff.rs\nindex 9b7e014..1d4c8f2 100644\n--- a/crates/git/src/diff.rs\n+++ b/crates/git/src/diff.rs\n@@ -96,10 +96,10 @@ pub fn parse_unified(text: &str) -> Vec<FileDiff> {\n         } else if let Some(rest) = line.strip_prefix(\"rename from \") {\n-            cur.old_path = Some(strip_side(rest).to_string());\n+            cur.old_path = Some(rest.to_string());\n             cur.kind = DiffFile::Renamed;\n         } else if let Some(rest) = line.strip_prefix(\"rename to \") {\n-            cur.path = strip_side(rest).to_string();\n+            cur.path = rest.to_string();\n             cur.kind = DiffFile::Renamed;\n",
        "update",
    );
    let req = new_id();
    let approval = b.push(
        ItemStatus::Completed,
        Some("Run command?"),
        TurnItemBody::ApprovalRequest {
            request_id: req.clone(),
            request_kind: RequestKind::Command,
            prompt: Some("cargo fmt --all".into()),
            reason: Some("Format the workspace before running clippy".into()),
            full_access_override: Some(FullAccessOverride::AskRule),
            options: Some(vec![
                ApprovalOption {
                    decision: ApprovalDecision::Accept,
                    label: "Yes".into(),
                    warning: None,
                },
                ApprovalOption {
                    decision: ApprovalDecision::AcceptAlways,
                    label: "Always allow `cargo fmt`".into(),
                    warning: Some("Adds a rule to your Codex config".into()),
                },
                ApprovalOption {
                    decision: ApprovalDecision::Decline,
                    label: "No".into(),
                    warning: None,
                },
            ]),
        },
    );
    let run = b.run_id();
    b.rows.requests.push(RuntimeRequest {
        id: req,
        thread_id: b.thread.id.clone(),
        run_id: Some(run),
        turn_item_id: approval,
        native_request_id: Some(json!(9)),
        kind: RequestKind::Command,
        status: RequestStatus::Resolved,
        live_session_id: None,
        created_at: b.clock,
        resolved_at: Some(b.clock + Duration::seconds(5)),
        decision: Some(ApprovalDecision::Accept),
        feedback: None,
        answers: None,
    });
    b.cmd(
        "cargo fmt --all && cargo test -p tern-git diff && cargo clippy -p tern-git -- -D warnings",
        "    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.12s\n     Running unittests src/lib.rs (target/debug/deps/tern_git-7c2f1a9e0b3d4e5f)\n\nrunning 14 tests\ntest diff::tests::binary_file ... ok\ntest diff::tests::counts_additions_and_deletions ... ok\ntest diff::tests::crlf_is_kept ... ok\ntest diff::tests::empty_context_line ... ok\ntest diff::tests::hunk_header_context_text ... ok\ntest diff::tests::line_numbers_advance_per_side ... ok\ntest diff::tests::mode_change_only ... ok\ntest diff::tests::no_newline_marker_is_skipped ... ok\ntest diff::tests::parses_multiple_files ... ok\ntest diff::tests::parses_single_hunk ... ok\ntest diff::tests::pure_rename ... ok\ntest diff::tests::rename_with_edits ... ok\ntest diff::tests::single_line_hunk_without_count ... ok\ntest diff::tests::unicode_paths ... ok\n\ntest result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n\n    Checking tern-git v0.1.0 (/home/ada/code/tern/crates/git)\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.94s\n",
        0,
    );
    b.web(
        &["git diff rename from rename to header format similarity index"],
        &[
            (
                "Git - git-diff Documentation: Generating patch text with -p",
                "https://git-scm.com/docs/git-diff#_generating_patch_text_with_p",
            ),
            (
                "Git - diff-format Documentation",
                "https://git-scm.com/docs/diff-format",
            ),
        ],
    );
    let parent = b.push(
        ItemStatus::Completed,
        Some("Subagent"),
        TurnItemBody::Subagent {
            subagent_id: "agent-diff-callers".into(),
            prompt: "Find every caller of the TypeScript parseUnifiedDiff and list which fields of FileDiff each one reads, so the Rust struct keeps all of them.".into(),
            progress: Some("Checked 3 of 3 call sites".into()),
            result: Some("All three callers read `path`, `oldPath`, `additions`, `deletions` and `hunks`. Only `DiffPanel.tsx` reads `binary`. Nothing reads `isNew`/`isDeleted` directly; they derive it from `kind`.".into()),
        },
    );
    let id = b.push(
        ItemStatus::Completed,
        Some("github.search_code"),
        TurnItemBody::DynamicTool {
            tool_name: Some("github.search_code".into()),
            input: json!({"query": "\"rename from\" \"similarity index\" language:rust parse", "per_page": 5}),
            output: Some(json!({
                "total_count": 2,
                "items": [
                    {"repository": "rust-lang/cargo", "path": "crates/cargo-test-support/src/diff.rs"},
                    {"repository": "helix-editor/helix", "path": "helix-vcs/src/diff.rs"}
                ]
            })),
        },
    );
    if let Some(item) = b.rows.items.iter_mut().find(|i| i.id == id) {
        item.parent_item_id = Some(parent);
    }
    b.say(DIFF_ANSWER, AssistantPhase::FinalAnswer);
    b.end(RunStatus::Completed, 142_300, 6_480);
    b.checkpoint(vec![
        file("crates/git/src/diff.rs", "modified", 168, 4),
        file("crates/git/src/diff/tests.rs", "added", 212, 0),
        file("crates/git/Cargo.toml", "modified", 1, 0),
    ]);

    b.user("Nice. Can you also add a `FileDiff::is_pure_rename()` helper?");
    b.edit(
        "crates/git/src/diff.rs",
        "diff --git a/crates/git/src/diff.rs b/crates/git/src/diff.rs\nindex 1d4c8f2..a07b3e9 100644\n--- a/crates/git/src/diff.rs\n+++ b/crates/git/src/diff.rs\n@@ -42,6 +42,13 @@ pub struct FileDiff {\n     pub hunks: Vec<DiffHunk>,\n }\n \n+impl FileDiff {\n+    /// A rename with no content changes.\n+    pub fn is_pure_rename(&self) -> bool {\n+        self.kind == DiffFile::Renamed && self.hunks.is_empty() && !self.binary\n+    }\n+}\n+\n",
        "update",
    );
    b.cmd("cargo test -p tern-git diff -q", "running 15 tests\n...............\ntest result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n", 0);
    b.say(
        "Added it, with a test:\n\n```rust\nlet d = parse_unified(PURE_RENAME).remove(0);\nassert!(d.is_pure_rename());\n```\n\nA binary rename is not a pure rename, since git can't tell whether the content changed.",
        AssistantPhase::FinalAnswer,
    );
    b.end(RunStatus::Completed, 151_000, 612);
    b.checkpoint(vec![
        file("crates/git/src/diff.rs", "modified", 7, 0),
        file("crates/git/src/diff/tests.rs", "modified", 6, 0),
    ]);
    b.visited();
    b.finish()
}

const PARSER_DIFF: &str = "diff --git a/crates/git/src/diff.rs b/crates/git/src/diff.rs
index 3f1c2aa..9b7e014 100644
--- a/crates/git/src/diff.rs
+++ b/crates/git/src/diff.rs
@@ -44,5 +44,74 @@ pub struct FileDiff {
     pub hunks: Vec<DiffHunk>,
 }

-pub fn parse_unified(text: &str) -> Vec<FileDiff> {
-    let _ = text;
-    todo!()
-}
+pub fn parse_unified(text: &str) -> Vec<FileDiff> {
+    let mut files = Vec::new();
+    let mut cur: Option<FileDiff> = None;
+    let (mut old_no, mut new_no) = (0u32, 0u32);
+
+    for line in text.lines() {
+        if let Some(rest) = line.strip_prefix(\"diff --git \") {
+            files.extend(cur.take());
+            cur = Some(FileDiff::from_header(rest));
+            continue;
+        }
+        let Some(file) = cur.as_mut() else { continue };
+        if let Some(header) = line.strip_prefix(\"@@ \") {
+            let (old_start, new_start, rest) = parse_hunk_header(header);
+            old_no = old_start;
+            new_no = new_start;
+            file.hunks.push(DiffHunk {
+                header: rest.to_string(),
+                old_start,
+                new_start,
+                lines: Vec::new(),
+            });
+        } else if line.starts_with(\"Binary files \") {
+            file.binary = true;
+        } else if let Some(rest) = line.strip_prefix(\"rename from \") {
+            file.old_path = Some(strip_side(rest).to_string());
+            file.kind = DiffFile::Renamed;
+        } else if let Some(rest) = line.strip_prefix(\"rename to \") {
+            file.path = strip_side(rest).to_string();
+            file.kind = DiffFile::Renamed;
+        } else if let Some(hunk) = file.hunks.last_mut() {
+            match line.as_bytes().first() {
+                Some(b'+') => {
+                    hunk.lines.push(DiffLine::added(new_no, &line[1..]));
+                    new_no += 1;
+                    file.additions += 1;
+                }
+                Some(b'-') => {
+                    hunk.lines.push(DiffLine::deleted(old_no, &line[1..]));
+                    old_no += 1;
+                    file.deletions += 1;
+                }
+                Some(b' ') | None => {
+                    hunk.lines.push(DiffLine::context(old_no, new_no, line.get(1..).unwrap_or(\"\")));
+                    old_no += 1;
+                    new_no += 1;
+                }
+                _ => {} // \\ No newline at end of file
+            }
+        }
+    }
+    files.extend(cur);
+    files
+}
+
+/// `-12,7 +12,9 @@ fn main` -> (12, 12, \"fn main\")
+fn parse_hunk_header(header: &str) -> (u32, u32, &str) {
+    let mut parts = header.splitn(3, ' ');
+    let old = parts.next().unwrap_or(\"-0\");
+    let new = parts.next().unwrap_or(\"+0\");
+    let rest = parts.next().unwrap_or(\"\").trim_start_matches(\"@@\").trim();
+    (start_of(old), start_of(new), rest)
+}
+
+fn start_of(range: &str) -> u32 {
+    range[1..].split(',').next().and_then(|s| s.parse().ok()).unwrap_or(0)
+}
";

const DIFF_ANSWER: &str = r##"## What changed

I ported `parseUnifiedDiff` to `parse_unified` in [crates/git/src/diff.rs](crates/git/src/diff.rs) and kept the hunk header handling identical to the TypeScript version.

- **Hunk headers** are parsed by hand instead of with a regex. `@@ -12,7 +12,9 @@ fn main` gives `old_start = 12`, `new_start = 12`, and keeps `fn main` as the header text.
- **Renames** come from the `rename from` / `rename to` lines, so a pure rename produces a `FileDiff` with no hunks.
- **Binary files** (`Binary files a/x and b/x differ`) set `binary: true` and skip line counting.
- Line numbers start at the header values and only advance on the side a line belongs to (see `crates/git/src/diff.rs:88`).

### The core loop

```rust
match line.as_bytes().first() {
    Some(b'+') => {
        hunk.lines.push(DiffLine::added(new_no, &line[1..]));
        new_no += 1;
        file.additions += 1;
    }
    Some(b'-') => {
        hunk.lines.push(DiffLine::deleted(old_no, &line[1..]));
        old_no += 1;
        file.deletions += 1;
    }
    _ => { /* context, or the no-newline marker */ }
}
```

### Parity with the TypeScript version

| Case | TypeScript | Rust |
|---|---|---|
| `\ No newline at end of file` | skipped | skipped |
| CRLF line endings | kept in `text` | kept in `text` |
| Empty context line | context | context |
| Mode-only change | no hunks | no hunks |
| Rename with edits | `kind: "renamed"` + hunks | `DiffFile::Renamed` + hunks |

For reference, the web app calls it like this:

```ts
const files = parseUnifiedDiff(await git.diff(["--no-color", "-M"]));
```

### Check it yourself

```bash
cargo test -p tern-git diff
cargo clippy -p tern-git -- -D warnings
```

All 14 tests pass. I left `scope_diff` alone; it still needs the checkpoint ref lookup at `crates/git/src/lib.rs:31`. The rename header format is described in the [git diff docs](https://git-scm.com/docs/git-diff#_generating_patch_text_with_p).
"##;

const STREAMING_ANSWER: &str = r##"Blocks are now cached per message, and only the open tail is reparsed on each delta.

## How it works

1. `StreamingBlocks::update` finds the last *safe boundary*: a blank line that is not inside a code fence.
2. Everything before it is parsed once and appended to `stable`.
3. The tail after the boundary is reparsed on every token, which is cheap because it's at most one block.

The message view keeps one `StreamingBlocks` per message id in `crates/app/src/chat/message.rs:118`:

```rust
let blocks = self.cache.entry(msg.id.clone()).or_default();
for block in blocks.update(&msg.text) {
    render_block(block, window, cx);
}
render_tail(&msg.text[blocks.stable_len()..], window, cx);
```

## Numbers

| Benchmark | Before | After |
|---|---|---|
| `stream_2000_lines` | 186.1 ms | 4.3 ms |
| `stream_200_lines` | 2.05 ms | 0.31 ms |

The 2,000-line case drops from quadratic to linear because each token now costs one tail parse instead of a full reparse.

## Edge cases I checked

- A fence that opens but hasn't closed yet stays in the tail, so a half-streamed code block renders as code, not as paragraphs.
- Tables only become stable after the blank line that ends them.
- Nested lists keep their indentation because the boundary is always at column 0.

I'm running the markdown tests now; after that I'll benchmark the 2,000-line answer with `cargo bench -p tern --bench markdown -- --baseline before` and update the table.
"##;

const PAGINATION_ANSWER: &str = r##"Pagination is in. `GET /v1/invoices` now takes `limit` (default 50, max 200) and an opaque `cursor`, and returns `nextCursor` when there's another page.

### Cursor format

The cursor is base64url JSON of the last row's sort key:

```ts
encodeCursor({ issuedAt: "2026-11-24T18:02:11.000Z", id: "inv_9f2c" })
// => "eyJpc3N1ZWRBdCI6IjIwMjYtMTEtMjRUMTg6MDI6MTEuMDAwWiIsImlkIjoiaW52XzlmMmMifQ"
```

Sorting by `(issuedAt, id)` keeps pages stable when two invoices share a timestamp. A bad cursor returns `400` with `{"error": "invalid_cursor"}`.

### Tests

- first page returns 50 and a cursor
- `limit=500` is clamped to 200
- walking every page yields each invoice exactly once
- ties on `issuedAt` are not skipped or duplicated
"##;

const SETTINGS_PLAN: &str = r##"# Settings page

## Shape

A modal over the main window (spec/06 §1.1): left nav at 220px, content scrolls on the right. Escape closes it unless a key recorder is active.

## Steps

1. **Engine API.** Add `Command::UpdateSettings { patch }` that deep-merges into `Settings` / `ClientSettings`, persists through `tern_core::settings`, and emits `Update::Snapshot`. Refuse writes when `settings.json` was unreadable.
2. **Row primitives.** `SettingsSection`, `SettingsRow` (title, description, control slot, reset button when the value differs from the default).
3. **General page.** Notifications, sidebar, confirmations, time format. Every row maps 1:1 to a `ClientSettings` field.
4. **Keybindings page.** Grouped list from `merge_keybindings`, search, the key recorder, conflict detection with `When::parse`.
5. **Nav + routing.** Page index from spec/06 in nav order; unbuilt pages show a placeholder.

## Risks

- The key recorder must swallow Escape without closing the modal.
- `client-settings.json` is written whole, so a write must start from the latest file contents.

## Out of scope for this pass

Providers, Projects, Source control and Appearance pages.
"##;

const SEARCH_ANSWER: &str = r##"## Root cause

Migration `0142_drop_unused_indexes.sql` (Tuesday) dropped `documents_account_tsv_idx` because pg_stat showed few scans on it. Without it, Postgres picks `documents_updated_at_idx` for `ORDER BY updated_at DESC LIMIT 20` and filters rows until it finds 20 matches. For rare terms that means reading most of the table.

## Fix

`migrations/0147_restore_search_index.sql` recreates it as a composite GIN index on `(account_id, tsv)` using `btree_gin`, built `CONCURRENTLY` so it doesn't lock writes.

| Query | Before | After |
|---|---|---|
| rare term, big account | 1312 ms | 3.9 ms |
| common term, big account | 41 ms | 2.7 ms |
| any term, small account | 6 ms | 1.1 ms |

## Follow-ups

- Mark the index in `0142` as *do not drop*; low scan counts were from a quiet week.
- Add the p99 query to `scripts/explain-regressions.sh` so CI catches plan changes.
"##;

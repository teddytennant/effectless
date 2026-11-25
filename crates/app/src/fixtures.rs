//! A small local fixture so the shell renders something until the engine's demo
//! mode lands. Served through the same [`crate::store::Store`] interface.

use std::collections::HashMap;

use chrono::{Duration, Utc};
use tern_core::*;

fn model() -> ModelSelection {
    ModelSelection {
        instance_id: "codex".into(),
        model: "gpt-5.5".into(),
        options: Vec::new(),
    }
}

fn project(id: &str, title: &str, root: &str, minutes_ago: i64) -> Project {
    let at = Utc::now() - Duration::minutes(minutes_ago);
    Project {
        id: id.into(),
        title: title.into(),
        has_custom_title: false,
        workspace_root: root.into(),
        default_model_selection: Some(model()),
        default_thread_env_mode: None,
        favicon_path: None,
        scripts: Vec::new(),
        pinned_at: None,
        archived_at: None,
        created_at: at,
        updated_at: at,
    }
}

struct Spec {
    id: &'static str,
    project: &'static str,
    title: &'static str,
    minutes_ago: i64,
    status: ShellThreadStatus,
    pinned: bool,
    settled: bool,
    unread: bool,
    diff: Option<(u32, u32, u32)>,
    branch: Option<&'static str>,
}

fn shell(spec: &Spec) -> ThreadShell {
    let at = Utc::now() - Duration::minutes(spec.minutes_ago);
    let thread = Thread {
        id: spec.id.into(),
        project_id: spec.project.into(),
        title: spec.title.into(),
        model_selection: model(),
        runtime_mode: RuntimeMode::FullAccess,
        interaction_mode: InteractionMode::Default,
        branch: spec.branch.map(Into::into),
        worktree_path: None,
        active_provider_thread_id: None,
        title_renamed: false,
        title_generation_started_at: None,
        created_at: at - Duration::minutes(30),
        updated_at: at,
        archived_at: None,
        pinned_at: spec.pinned.then_some(at),
        settled_at: spec.settled.then_some(at),
        last_visited_at: (!spec.unread).then_some(at + Duration::minutes(1)),
        linked_pull_request: None,
        deleted_at: None,
    };
    ThreadShell {
        thread,
        status: spec.status,
        active_run_id: matches!(spec.status, ShellThreadStatus::Run(s) if s.is_live())
            .then(|| format!("{}-run", spec.id)),
        latest_run_started_at: Some(at - Duration::minutes(2)),
        latest_run_completed_at: (!matches!(spec.status, ShellThreadStatus::Run(s) if s.is_live()))
            .then_some(at),
        pending_request: None,
        latest_message: Some(("assistant".into(), "Done. The tests pass.".into())),
        has_actionable_proposed_plan: false,
        last_error: None,
        last_error_class: None,
        usage_limit_reset_at: None,
        diff_totals: spec.diff.map(|(count, additions, deletions)| FileTotals {
            count,
            additions,
            deletions,
        }),
        last_activity_at: at,
    }
}

/// The fixture snapshot: two projects, a handful of threads in different states.
pub fn snapshot() -> Snapshot {
    let specs = [
        Spec {
            id: "t-ci",
            project: "p-tern",
            title: "Fix flaky CI on the store tests",
            minutes_ago: 3,
            status: ShellThreadStatus::Run(RunStatus::Running),
            pinned: false,
            settled: false,
            unread: false,
            diff: Some((3, 42, 7)),
            branch: Some("fix/store-ci"),
        },
        Spec {
            id: "t-sidebar",
            project: "p-tern",
            title: "Port the sidebar resize rail",
            minutes_ago: 18,
            status: ShellThreadStatus::Run(RunStatus::Completed),
            pinned: true,
            settled: false,
            unread: true,
            diff: Some((5, 230, 41)),
            branch: Some("sidebar-rail"),
        },
        Spec {
            id: "t-tokens",
            project: "p-tern",
            title: "Generate theme tokens from the spec",
            minutes_ago: 64,
            status: ShellThreadStatus::Run(RunStatus::Completed),
            pinned: false,
            settled: false,
            unread: false,
            diff: None,
            branch: None,
        },
        Spec {
            id: "t-api",
            project: "p-site",
            title: "Add pagination to the posts API",
            minutes_ago: 140,
            status: ShellThreadStatus::Run(RunStatus::Failed),
            pinned: false,
            settled: false,
            unread: false,
            diff: Some((2, 18, 3)),
            branch: Some("posts-pagination"),
        },
        Spec {
            id: "t-readme",
            project: "p-site",
            title: "Tighten the README install steps",
            minutes_ago: 60 * 26,
            status: ShellThreadStatus::Run(RunStatus::Completed),
            pinned: false,
            settled: true,
            unread: false,
            diff: None,
            branch: None,
        },
    ];
    Snapshot {
        projects: vec![
            project("p-tern", "tern", "/home/user/code/tern", 3),
            project("p-site", "site", "/home/user/code/site", 140),
        ],
        threads: specs.iter().map(shell).collect(),
        providers: Vec::new(),
        revision: 1,
    }
}

fn item(thread: &str, ordinal: u32, minutes_ago: i64, body: TurnItemBody) -> TurnItem {
    let at = Utc::now() - Duration::minutes(minutes_ago);
    TurnItem {
        id: format!("{thread}-item-{ordinal}"),
        thread_id: thread.into(),
        run_id: Some(format!("{thread}-run")),
        native_item_id: None,
        parent_item_id: None,
        ordinal,
        status: ItemStatus::Completed,
        title: None,
        started_at: Some(at),
        completed_at: Some(at),
        updated_at: at,
        body,
    }
}

/// A short transcript for any fixture thread.
pub fn thread(snapshot: &Snapshot, id: &str) -> Option<ThreadDetail> {
    let shell = snapshot.thread(id)?;
    let items = vec![
        item(
            id,
            0,
            5,
            TurnItemBody::UserMessage {
                message_id: format!("{id}-m0"),
                text: format!("{}. Keep the change small.", shell.thread.title),
                attachments: Vec::new(),
                input_intent: InputIntent::default(),
                model_switch: None,
            },
        ),
        item(
            id,
            1,
            4,
            TurnItemBody::AssistantMessage {
                message_id: format!("{id}-m1"),
                text: "I traced it to the shared temp directory. Each test now gets its own, \
                       and the suite passes ten runs in a row."
                    .into(),
                streaming: false,
                phase: None,
            },
        ),
    ];
    Some(ThreadDetail {
        thread: shell.thread.clone(),
        runs: Vec::new(),
        items,
        requests: Vec::new(),
        checkpoints: Vec::new(),
        plans: Vec::new(),
        context_usage: None,
        session: None,
        live_output: HashMap::new(),
        revision: 1,
    })
}

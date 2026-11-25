use super::*;

fn project(id: &str, root: &str) -> Project {
    let t = now();
    Project {
        id: id.into(),
        title: "tern".into(),
        has_custom_title: false,
        workspace_root: root.into(),
        default_model_selection: None,
        default_thread_env_mode: None,
        favicon_path: None,
        scripts: vec![],
        pinned_at: None,
        archived_at: None,
        created_at: t,
        updated_at: t,
    }
}

fn thread(id: &str, project_id: &str) -> Thread {
    let t = now();
    Thread {
        id: id.into(),
        project_id: project_id.into(),
        title: NEW_THREAD_TITLE.into(),
        model_selection: ModelSelection {
            instance_id: "codex".into(),
            model: "gpt-5".into(),
            options: vec![],
        },
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
    }
}

fn run(id: &str, thread_id: &str, ordinal: u32, status: RunStatus) -> Run {
    Run {
        id: id.into(),
        thread_id: thread_id.into(),
        ordinal,
        model_selection: thread(thread_id, "p").model_selection,
        runtime_mode: RuntimeMode::FullAccess,
        interaction_mode: InteractionMode::Default,
        user_message_id: format!("{id}-msg"),
        status,
        queue_position: None,
        requested_at: now(),
        started_at: None,
        completed_at: None,
        checkpoint_id: None,
        source_plan_id: None,
        native_turn_id: None,
        model_reported: None,
        token_usage: None,
        failure: None,
        interrupt_requested_at: None,
        interrupt_unresponsive_at: None,
    }
}

fn item(id: &str, thread_id: &str, run_id: &str, ordinal: u32, body: TurnItemBody) -> TurnItem {
    TurnItem {
        id: id.into(),
        thread_id: thread_id.into(),
        run_id: Some(run_id.into()),
        native_item_id: None,
        parent_item_id: None,
        ordinal,
        status: ItemStatus::Completed,
        title: None,
        started_at: None,
        completed_at: None,
        updated_at: now(),
        body,
    }
}

fn seeded() -> Store {
    let store = Store::open_in_memory().unwrap();
    store.upsert_project(&project("p", "/src/tern")).unwrap();
    store.upsert_thread(&thread("t", "p")).unwrap();
    store
}

#[test]
fn migrations_set_user_version() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.schema_version().unwrap() as usize, MIGRATIONS.len());
}

#[test]
fn file_db_reopens_without_rerunning_migrations() {
    let dir = tempdir();
    let path = dir.join("userdata/state.sqlite");
    {
        let store = Store::open(&path).unwrap();
        store.upsert_project(&project("p", "/x")).unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.projects().unwrap().len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tern-store-{}", new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn project_crud() {
    let store = Store::open_in_memory().unwrap();
    let mut p = project("p", "/src/tern");
    store.upsert_project(&p).unwrap();
    assert_eq!(store.project("p").unwrap(), Some(p.clone()));
    p.title = "Tern".into();
    p.scripts.push(ProjectScript {
        id: "test".into(),
        name: "Test".into(),
        command: "cargo test".into(),
        icon: ProjectScriptIcon::Test,
        ..Default::default()
    });
    store.upsert_project(&p).unwrap();
    assert_eq!(store.project_by_root("/src/tern").unwrap(), Some(p.clone()));
    assert_eq!(store.projects().unwrap(), vec![p]);

    // same root twice is refused while the first is live
    assert!(store.upsert_project(&project("q", "/src/tern")).is_err());
    store.delete_project("p").unwrap();
    assert!(store.project("p").unwrap().is_none());
    store.upsert_project(&project("q", "/src/tern")).unwrap();
}

#[test]
fn thread_crud_and_soft_delete() {
    let store = seeded();
    let mut t = store.thread("t").unwrap().unwrap();
    t.title = "Fix the flaky test".into();
    t.pinned_at = Some(now());
    store.upsert_thread(&t).unwrap();
    assert_eq!(store.threads().unwrap(), vec![t.clone()]);

    t.deleted_at = Some(now());
    store.upsert_thread(&t).unwrap();
    assert!(store.threads().unwrap().is_empty());
    assert_eq!(store.deleted_threads().unwrap().len(), 1);
    store.hard_delete_thread("t").unwrap();
    assert!(store.thread("t").unwrap().is_none());
}

#[test]
fn run_item_request_roundtrip() {
    let mut store = seeded();
    store
        .write(|tx| {
            tx.upsert_run(&run("r1", "t", 1, RunStatus::Running))?;
            tx.upsert_item(&item(
                "i1",
                "t",
                "r1",
                1,
                TurnItemBody::UserMessage {
                    message_id: "m1".into(),
                    text: "hello".into(),
                    attachments: vec![],
                    input_intent: InputIntent::TurnStart,
                    model_switch: None,
                },
            ))?;
            tx.upsert_item(&item(
                "i2",
                "t",
                "r1",
                2,
                TurnItemBody::CommandExecution {
                    input: "cargo test".into(),
                    output: Some("ok".into()),
                    exit_code: Some(0),
                    output_indicates_failure: None,
                },
            ))?;
            tx.upsert_request(&RuntimeRequest {
                id: "q1".into(),
                thread_id: "t".into(),
                run_id: Some("r1".into()),
                turn_item_id: "i2".into(),
                native_request_id: Some(serde_json::json!(7)),
                kind: RequestKind::Command,
                status: RequestStatus::Pending,
                live_session_id: Some("s".into()),
                created_at: now(),
                resolved_at: None,
                decision: None,
                feedback: None,
                answers: None,
            })?;
            Ok(())
        })
        .unwrap();

    let rows = store.thread_rows("t").unwrap();
    assert_eq!(rows.runs.len(), 1);
    assert_eq!(
        rows.items.iter().map(|i| i.ordinal).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        rows.requests[0].native_request_id,
        Some(serde_json::json!(7))
    );
    assert_eq!(store.max_item_ordinal("t").unwrap(), Some(2));
    assert_eq!(
        store.items_of_kind("t", "command_execution").unwrap().len(),
        1
    );

    // updating a run must not cascade-delete its items (no INSERT OR REPLACE)
    let mut r = store.run("r1").unwrap().unwrap();
    r.status = RunStatus::Completed;
    store.upsert_run(&r).unwrap();
    assert_eq!(store.items("t").unwrap().len(), 2);

    // two items can't share an ordinal
    assert!(
        store
            .upsert_item(&item(
                "i3",
                "t",
                "r1",
                2,
                TurnItemBody::SystemNotice {
                    message: "x".into(),
                    level: None
                }
            ))
            .is_err()
    );
}

#[test]
fn rollback_on_error_leaves_nothing() {
    let mut store = seeded();
    let res: Result<()> = store.write(|tx| {
        tx.upsert_run(&run("r1", "t", 1, RunStatus::Running))?;
        anyhow::bail!("boom")
    });
    assert!(res.is_err());
    assert!(store.runs("t").unwrap().is_empty());
}

#[test]
fn checkpoint_recapture_replaces_same_ordinal() {
    let store = seeded();
    let mk = |id: &str, status| Checkpoint {
        id: id.into(),
        thread_id: "t".into(),
        run_id: Some("r1".into()),
        ordinal: 1,
        git_ref: "refs/tern/x/1".into(),
        status,
        files: vec![CheckpointFileSummary {
            path: "src/main.rs".into(),
            kind: "modified".into(),
            additions: 3,
            deletions: 1,
        }],
        captured_at: now(),
    };
    store
        .upsert_checkpoint(&mk("c1", CheckpointStatus::Stale), "/src/tern")
        .unwrap();
    store
        .upsert_checkpoint(&mk("c2", CheckpointStatus::Ready), "/src/tern")
        .unwrap();
    let cps = store.checkpoints("t").unwrap();
    assert_eq!(cps.len(), 1);
    assert_eq!(cps[0].id, "c2");
    assert_eq!(
        store.checkpoint_cwds("t").unwrap(),
        vec!["/src/tern".to_string()]
    );
}

#[test]
fn plans_and_bindings() {
    let store = seeded();
    let mut plan = ProposedPlan {
        id: "pl".into(),
        thread_id: "t".into(),
        run_id: None,
        markdown: "# Plan".into(),
        status: PlanStatus::Draft,
        created_at: now(),
        updated_at: now(),
    };
    store.upsert_plan(&plan).unwrap();
    plan.status = PlanStatus::Active;
    store.upsert_plan(&plan).unwrap();
    assert_eq!(store.plans("t").unwrap(), vec![plan]);

    let b = ProviderBinding {
        thread_id: "t".into(),
        driver: Driver::Claude,
        native_thread_id: Some("sess".into()),
        status: ProviderThreadStatus::Idle,
        context_usage: Some(ContextUsage {
            used_tokens: 1200,
            max_tokens: Some(200_000),
            ..Default::default()
        }),
        updated_at: now(),
    };
    store.upsert_binding(&b).unwrap();
    assert_eq!(store.binding("t").unwrap(), Some(b));
}

#[test]
fn purge_keeps_checkpoints_then_hard_delete_cascades() {
    let store = seeded();
    store
        .upsert_run(&run("r1", "t", 1, RunStatus::Completed))
        .unwrap();
    store
        .upsert_checkpoint(
            &Checkpoint {
                id: "c".into(),
                thread_id: "t".into(),
                run_id: Some("r1".into()),
                ordinal: 1,
                git_ref: "r".into(),
                status: CheckpointStatus::Ready,
                files: vec![],
                captured_at: now(),
            },
            "/x",
        )
        .unwrap();
    store.purge_thread_content("t").unwrap();
    assert!(store.runs("t").unwrap().is_empty());
    assert_eq!(store.checkpoints("t").unwrap().len(), 1);
    store.hard_delete_thread("t").unwrap();
    assert!(store.checkpoints("t").unwrap().is_empty());
}

#[test]
fn recover_interrupts_live_runs_and_cancels_requests() {
    let mut store = seeded();
    store
        .upsert_run(&run("r1", "t", 1, RunStatus::Completed))
        .unwrap();
    store
        .upsert_run(&run("r2", "t", 2, RunStatus::Running))
        .unwrap();
    store
        .upsert_run(&run("r3", "t", 3, RunStatus::Queued))
        .unwrap();
    let mut approval = item(
        "a",
        "t",
        "r2",
        5,
        TurnItemBody::ApprovalRequest {
            request_id: "q".into(),
            request_kind: RequestKind::Command,
            prompt: Some("rm -rf target".into()),
            reason: None,
            full_access_override: None,
            options: None,
        },
    );
    approval.status = ItemStatus::Waiting;
    store.upsert_item(&approval).unwrap();
    let mut msg = item(
        "m",
        "t",
        "r2",
        4,
        TurnItemBody::AssistantMessage {
            message_id: "m".into(),
            text: "Working".into(),
            streaming: true,
            phase: None,
        },
    );
    msg.status = ItemStatus::Running;
    store.upsert_item(&msg).unwrap();
    store
        .upsert_request(&RuntimeRequest {
            id: "q".into(),
            thread_id: "t".into(),
            run_id: Some("r2".into()),
            turn_item_id: "a".into(),
            native_request_id: None,
            kind: RequestKind::Command,
            status: RequestStatus::Pending,
            live_session_id: None,
            created_at: now(),
            resolved_at: None,
            decision: None,
            feedback: None,
            answers: None,
        })
        .unwrap();

    let touched = store.recover().unwrap();
    assert_eq!(touched, vec!["t".to_string()]);
    let rows = store.thread_rows("t").unwrap();
    let status = |id: &str| rows.runs.iter().find(|r| r.id == id).unwrap().status;
    assert_eq!(status("r1"), RunStatus::Completed);
    assert_eq!(status("r2"), RunStatus::Interrupted);
    assert_eq!(status("r3"), RunStatus::Queued);
    assert_eq!(rows.requests[0].status, RequestStatus::Cancelled);
    let a = rows.items.iter().find(|i| i.id == "a").unwrap();
    assert_eq!(a.status, ItemStatus::Cancelled);
    let m = rows.items.iter().find(|i| i.id == "m").unwrap();
    assert_eq!(m.status, ItemStatus::Interrupted);
    assert!(matches!(
        m.body,
        TurnItemBody::AssistantMessage {
            streaming: false,
            ..
        }
    ));
    assert!(store.recover().unwrap().is_empty());
}

#[test]
fn item_kind_matches_serde_tag() {
    let body = TurnItemBody::RunInterruptResult {
        message: "x".into(),
    };
    let v = serde_json::to_value(&body).unwrap();
    assert_eq!(v["type"], item_kind(&body));
    assert_eq!(ITEM_KINDS.len(), 19);
}

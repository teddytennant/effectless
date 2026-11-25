//! SQLite persistence (spec/07 §6). Projection rows written directly, no event log.
//!
//! Every table is a handful of indexed scalar columns plus a JSON column holding the full
//! `tern_core` object, the same split T3's V2 projections use (spec/07 §6.1): the columns are
//! query keys, the JSON is what callers read back.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::SecondsFormat;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tern_core::*;

/// Migrations, applied in order and tracked with `PRAGMA user_version`.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE projects (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  workspace_root TEXT NOT NULL,
  pinned_at TEXT,
  archived_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  data_json TEXT NOT NULL
);
CREATE UNIQUE INDEX projects_root ON projects(workspace_root) WHERE deleted_at IS NULL;

CREATE TABLE threads (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id),
  title TEXT NOT NULL,
  instance_id TEXT NOT NULL,
  model TEXT NOT NULL,
  runtime_mode TEXT NOT NULL,
  interaction_mode TEXT NOT NULL DEFAULT 'default',
  branch TEXT,
  worktree_path TEXT,
  pinned_at TEXT,
  archived_at TEXT,
  settled_at TEXT,
  last_visited_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  data_json TEXT NOT NULL
);
CREATE INDEX threads_project ON threads(project_id, updated_at) WHERE deleted_at IS NULL;

CREATE TABLE provider_bindings (
  thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
  driver TEXT NOT NULL,
  native_thread_id TEXT,
  status TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  data_json TEXT NOT NULL
);

CREATE TABLE runs (
  id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('preparing','queued','starting','running','waiting',
    'completed','interrupted','failed','cancelled','rolled_back')),
  queue_position INTEGER,
  user_message_id TEXT NOT NULL,
  checkpoint_id TEXT,
  requested_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  data_json TEXT NOT NULL
);
CREATE INDEX runs_thread ON runs(thread_id, ordinal);
CREATE INDEX runs_active ON runs(thread_id)
  WHERE status IN ('preparing','queued','starting','running','waiting');

CREATE TABLE turn_items (
  id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  run_id TEXT REFERENCES runs(id) ON DELETE CASCADE,
  parent_item_id TEXT,
  ordinal INTEGER NOT NULL,
  kind TEXT NOT NULL,
  status TEXT NOT NULL,
  native_item_id TEXT,
  started_at TEXT,
  completed_at TEXT,
  updated_at TEXT NOT NULL,
  body_json TEXT NOT NULL,
  UNIQUE (thread_id, ordinal)
);
CREATE INDEX turn_items_run ON turn_items(run_id, ordinal);
CREATE INDEX turn_items_native ON turn_items(thread_id, native_item_id)
  WHERE native_item_id IS NOT NULL;

CREATE TABLE runtime_requests (
  id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  run_id TEXT REFERENCES runs(id) ON DELETE CASCADE,
  turn_item_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending','resolved','expired','cancelled')),
  created_at TEXT NOT NULL,
  resolved_at TEXT,
  data_json TEXT NOT NULL
);
CREATE INDEX runtime_requests_pending ON runtime_requests(thread_id) WHERE status = 'pending';

CREATE TABLE checkpoints (
  id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  run_id TEXT,
  ordinal INTEGER NOT NULL,
  cwd TEXT NOT NULL,
  git_ref TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('ready','missing','error','stale')),
  captured_at TEXT NOT NULL,
  data_json TEXT NOT NULL,
  UNIQUE (thread_id, ordinal)
);

CREATE TABLE plans (
  id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  run_id TEXT,
  status TEXT NOT NULL CHECK (status IN ('draft','active','completed','superseded')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  data_json TEXT NOT NULL
);
CREATE INDEX plans_thread ON plans(thread_id, created_at);
"#];

pub struct Store {
    conn: Connection,
}

/// A write transaction. Dropping it without `commit` rolls back.
pub struct Tx<'a> {
    tx: Transaction<'a>,
}

impl Tx<'_> {
    pub fn commit(self) -> Result<()> {
        self.tx.commit().context("commit")
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let fresh = !path.exists();
        let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        if fresh {
            conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
        }
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "journal_size_limit", 134_217_728i64)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "busy_timeout", 5000i64)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))?)
    }

    fn migrate(&mut self) -> Result<()> {
        let current = self.schema_version()? as usize;
        if current > MIGRATIONS.len() {
            anyhow::bail!(
                "database schema {current} is newer than this build ({})",
                MIGRATIONS.len()
            );
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)
                .with_context(|| format!("migration {}", i + 1))?;
            tx.pragma_update(None, "user_version", (i + 1) as u32)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn tx(&mut self) -> Result<Tx<'_>> {
        Ok(Tx {
            tx: self.conn.transaction()?,
        })
    }

    /// Run `f` in one transaction and commit it.
    pub fn write<R>(&mut self, f: impl FnOnce(&Tx<'_>) -> Result<R>) -> Result<R> {
        let tx = self.tx()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    /// Startup recovery (spec/07 §0): live runs become `interrupted`, pending requests become
    /// `cancelled`, items still open are settled. Returns the ids of threads that changed.
    pub fn recover(&mut self) -> Result<Vec<String>> {
        let now = now();
        self.write(|tx| {
            let mut touched = BTreeSet::new();
            for mut run in tx.live_runs()? {
                run.status = RunStatus::Interrupted;
                run.completed_at.get_or_insert(now);
                touched.insert(run.thread_id.clone());
                tx.upsert_run(&run)?;
            }
            let mut cancelled_items = BTreeSet::new();
            for mut req in tx.pending_requests()? {
                req.status = RequestStatus::Cancelled;
                req.resolved_at = Some(now);
                cancelled_items.insert(req.turn_item_id.clone());
                touched.insert(req.thread_id.clone());
                tx.upsert_request(&req)?;
            }
            for mut item in tx.open_items()? {
                item.status = if cancelled_items.contains(&item.id) {
                    ItemStatus::Cancelled
                } else {
                    ItemStatus::Interrupted
                };
                stop_streaming(&mut item.body);
                item.completed_at.get_or_insert(now);
                item.updated_at = now;
                touched.insert(item.thread_id.clone());
                tx.upsert_item(&item)?;
            }
            Ok(touched.into_iter().collect())
        })
    }
}

/// Clear the `streaming` flag of a message-like body.
pub fn stop_streaming(body: &mut TurnItemBody) {
    match body {
        TurnItemBody::AssistantMessage { streaming, .. }
        | TurnItemBody::Reasoning { streaming, .. }
        | TurnItemBody::ProposedPlan { streaming, .. } => *streaming = false,
        _ => {}
    }
}

/// Serde tag of a turn item body (`user_message`, `command_execution`, ...).
pub fn item_kind(body: &TurnItemBody) -> &'static str {
    match body {
        TurnItemBody::UserMessage { .. } => "user_message",
        TurnItemBody::AssistantMessage { .. } => "assistant_message",
        TurnItemBody::Reasoning { .. } => "reasoning",
        TurnItemBody::ProposedPlan { .. } => "proposed_plan",
        TurnItemBody::TodoList { .. } => "todo_list",
        TurnItemBody::UserInputRequest { .. } => "user_input_request",
        TurnItemBody::FileChange { .. } => "file_change",
        TurnItemBody::CommandExecution { .. } => "command_execution",
        TurnItemBody::FileSearch { .. } => "file_search",
        TurnItemBody::WebSearch { .. } => "web_search",
        TurnItemBody::ApprovalRequest { .. } => "approval_request",
        TurnItemBody::Checkpoint { .. } => "checkpoint",
        TurnItemBody::RunInterruptRequest { .. } => "run_interrupt_request",
        TurnItemBody::RunInterruptResult { .. } => "run_interrupt_result",
        TurnItemBody::SystemNotice { .. } => "system_notice",
        TurnItemBody::Error { .. } => "error",
        TurnItemBody::Compaction { .. } => "compaction",
        TurnItemBody::Subagent { .. } => "subagent",
        TurnItemBody::DynamicTool { .. } => "dynamic_tool",
    }
}

/// Every turn item kind, in declaration order.
pub const ITEM_KINDS: &[&str] = &[
    "user_message",
    "assistant_message",
    "reasoning",
    "proposed_plan",
    "todo_list",
    "user_input_request",
    "file_change",
    "command_execution",
    "file_search",
    "web_search",
    "approval_request",
    "checkpoint",
    "run_interrupt_request",
    "run_interrupt_result",
    "system_notice",
    "error",
    "compaction",
    "subagent",
    "dynamic_tool",
];

fn ts(t: &Ts) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn ots(t: &Option<Ts>) -> Option<String> {
    t.as_ref().map(ts)
}

fn json<T: Serialize>(v: &T) -> Result<String> {
    Ok(serde_json::to_string(v)?)
}

/// Wire value of a unit enum (`"full-access"`, `"running"`, ...).
fn wire<T: Serialize>(v: &T) -> Result<String> {
    match serde_json::to_value(v)? {
        serde_json::Value::String(s) => Ok(s),
        other => Ok(other.to_string()),
    }
}

fn decode<T: DeserializeOwned>(s: String) -> rusqlite::Result<T> {
    serde_json::from_str(&s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn query_json<T: DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    args: impl rusqlite::Params,
) -> Result<Vec<T>> {
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map(args, |r| decode::<T>(r.get(0)?))?;
    Ok(rows.collect::<rusqlite::Result<Vec<T>>>()?)
}

fn query_one<T: DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    args: impl rusqlite::Params,
) -> Result<Option<T>> {
    let mut stmt = conn.prepare_cached(sql)?;
    Ok(stmt
        .query_row(args, |r| decode::<T>(r.get(0)?))
        .optional()?)
}

/// Everything persisted for one thread, as loaded at startup.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThreadRows {
    pub runs: Vec<Run>,
    pub items: Vec<TurnItem>,
    pub requests: Vec<RuntimeRequest>,
    pub checkpoints: Vec<Checkpoint>,
    pub plans: Vec<ProposedPlan>,
    pub binding: Option<ProviderBinding>,
}

/// Typed CRUD, available on both `Store` (autocommit) and `Tx`.
pub trait Db {
    fn conn(&self) -> &Connection;

    // ---- projects ----

    fn upsert_project(&self, p: &Project) -> Result<()> {
        self.conn().execute(
            "INSERT INTO projects (id, title, workspace_root, pinned_at, archived_at, created_at,
               updated_at, deleted_at, data_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8)
             ON CONFLICT(id) DO UPDATE SET title = excluded.title,
               workspace_root = excluded.workspace_root, pinned_at = excluded.pinned_at,
               archived_at = excluded.archived_at, updated_at = excluded.updated_at,
               data_json = excluded.data_json",
            params![
                p.id,
                p.title,
                p.workspace_root,
                ots(&p.pinned_at),
                ots(&p.archived_at),
                ts(&p.created_at),
                ts(&p.updated_at),
                json(p)?
            ],
        )?;
        Ok(())
    }

    fn project(&self, id: &str) -> Result<Option<Project>> {
        query_one(
            self.conn(),
            "SELECT data_json FROM projects WHERE id = ?1 AND deleted_at IS NULL",
            [id],
        )
    }

    fn project_by_root(&self, root: &str) -> Result<Option<Project>> {
        query_one(
            self.conn(),
            "SELECT data_json FROM projects WHERE workspace_root = ?1 AND deleted_at IS NULL",
            [root],
        )
    }

    fn projects(&self) -> Result<Vec<Project>> {
        query_json(
            self.conn(),
            "SELECT data_json FROM projects WHERE deleted_at IS NULL ORDER BY created_at, id",
            [],
        )
    }

    /// Soft delete. The project row stays for threads that still point at it.
    fn delete_project(&self, id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE projects SET deleted_at = ?2 WHERE id = ?1",
            params![id, ts(&now())],
        )?;
        Ok(())
    }

    // ---- threads ----

    fn upsert_thread(&self, t: &Thread) -> Result<()> {
        self.conn().execute(
            "INSERT INTO threads (id, project_id, title, instance_id, model, runtime_mode,
               interaction_mode, branch, worktree_path, pinned_at, archived_at, settled_at,
               last_visited_at, created_at, updated_at, deleted_at, data_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
             ON CONFLICT(id) DO UPDATE SET project_id = excluded.project_id,
               title = excluded.title, instance_id = excluded.instance_id,
               model = excluded.model, runtime_mode = excluded.runtime_mode,
               interaction_mode = excluded.interaction_mode, branch = excluded.branch,
               worktree_path = excluded.worktree_path, pinned_at = excluded.pinned_at,
               archived_at = excluded.archived_at, settled_at = excluded.settled_at,
               last_visited_at = excluded.last_visited_at, updated_at = excluded.updated_at,
               deleted_at = excluded.deleted_at, data_json = excluded.data_json",
            params![
                t.id,
                t.project_id,
                t.title,
                t.model_selection.instance_id,
                t.model_selection.model,
                wire(&t.runtime_mode)?,
                wire(&t.interaction_mode)?,
                t.branch,
                t.worktree_path,
                ots(&t.pinned_at),
                ots(&t.archived_at),
                ots(&t.settled_at),
                ots(&t.last_visited_at),
                ts(&t.created_at),
                ts(&t.updated_at),
                ots(&t.deleted_at),
                json(t)?
            ],
        )?;
        Ok(())
    }

    fn thread(&self, id: &str) -> Result<Option<Thread>> {
        query_one(
            self.conn(),
            "SELECT data_json FROM threads WHERE id = ?1",
            [id],
        )
    }

    /// Threads that are not deleted, oldest first.
    fn threads(&self) -> Result<Vec<Thread>> {
        query_json(
            self.conn(),
            "SELECT data_json FROM threads WHERE deleted_at IS NULL ORDER BY created_at, id",
            [],
        )
    }

    /// Soft-deleted threads whose refs still need sweeping.
    fn deleted_threads(&self) -> Result<Vec<Thread>> {
        query_json(
            self.conn(),
            "SELECT data_json FROM threads WHERE deleted_at IS NOT NULL ORDER BY id",
            [],
        )
    }

    /// Drop the transcript of a soft-deleted thread. Checkpoint rows stay so the ref sweep can
    /// find their cwd (spec/07 §7.7).
    fn purge_thread_content(&self, id: &str) -> Result<()> {
        let c = self.conn();
        c.execute("DELETE FROM runtime_requests WHERE thread_id = ?1", [id])?;
        c.execute("DELETE FROM turn_items WHERE thread_id = ?1", [id])?;
        c.execute("DELETE FROM plans WHERE thread_id = ?1", [id])?;
        c.execute("DELETE FROM runs WHERE thread_id = ?1", [id])?;
        c.execute("DELETE FROM provider_bindings WHERE thread_id = ?1", [id])?;
        Ok(())
    }

    /// Hard delete; cascades to every child table.
    fn hard_delete_thread(&self, id: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM threads WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- runs ----

    fn upsert_run(&self, r: &Run) -> Result<()> {
        self.conn().execute(
            "INSERT INTO runs (id, thread_id, ordinal, status, queue_position, user_message_id,
               checkpoint_id, requested_at, started_at, completed_at, data_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET ordinal = excluded.ordinal, status = excluded.status,
               queue_position = excluded.queue_position, checkpoint_id = excluded.checkpoint_id,
               started_at = excluded.started_at, completed_at = excluded.completed_at,
               data_json = excluded.data_json",
            params![
                r.id,
                r.thread_id,
                r.ordinal,
                wire(&r.status)?,
                r.queue_position,
                r.user_message_id,
                r.checkpoint_id,
                ts(&r.requested_at),
                ots(&r.started_at),
                ots(&r.completed_at),
                json(r)?
            ],
        )?;
        Ok(())
    }

    fn run(&self, id: &str) -> Result<Option<Run>> {
        query_one(
            self.conn(),
            "SELECT data_json FROM runs WHERE id = ?1",
            [id],
        )
    }

    fn runs(&self, thread_id: &str) -> Result<Vec<Run>> {
        query_json(
            self.conn(),
            "SELECT data_json FROM runs WHERE thread_id = ?1 ORDER BY ordinal, requested_at",
            [thread_id],
        )
    }

    /// Runs in preparing/starting/running/waiting, across all threads.
    fn live_runs(&self) -> Result<Vec<Run>> {
        query_json(
            self.conn(),
            "SELECT data_json FROM runs
             WHERE status IN ('preparing','starting','running','waiting')",
            [],
        )
    }

    // ---- turn items ----

    fn upsert_item(&self, i: &TurnItem) -> Result<()> {
        self.conn().execute(
            "INSERT INTO turn_items (id, thread_id, run_id, parent_item_id, ordinal, kind, status,
               native_item_id, started_at, completed_at, updated_at, body_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO UPDATE SET run_id = excluded.run_id,
               parent_item_id = excluded.parent_item_id, ordinal = excluded.ordinal,
               kind = excluded.kind, status = excluded.status,
               native_item_id = excluded.native_item_id, started_at = excluded.started_at,
               completed_at = excluded.completed_at, updated_at = excluded.updated_at,
               body_json = excluded.body_json",
            params![
                i.id,
                i.thread_id,
                i.run_id,
                i.parent_item_id,
                i.ordinal,
                item_kind(&i.body),
                wire(&i.status)?,
                i.native_item_id,
                ots(&i.started_at),
                ots(&i.completed_at),
                ts(&i.updated_at),
                json(i)?
            ],
        )?;
        Ok(())
    }

    fn item(&self, id: &str) -> Result<Option<TurnItem>> {
        query_one(
            self.conn(),
            "SELECT body_json FROM turn_items WHERE id = ?1",
            [id],
        )
    }

    fn items(&self, thread_id: &str) -> Result<Vec<TurnItem>> {
        query_json(
            self.conn(),
            "SELECT body_json FROM turn_items WHERE thread_id = ?1 ORDER BY ordinal",
            [thread_id],
        )
    }

    fn items_of_kind(&self, thread_id: &str, kind: &str) -> Result<Vec<TurnItem>> {
        query_json(
            self.conn(),
            "SELECT body_json FROM turn_items WHERE thread_id = ?1 AND kind = ?2 ORDER BY ordinal",
            params![thread_id, kind],
        )
    }

    fn open_items(&self) -> Result<Vec<TurnItem>> {
        query_json(
            self.conn(),
            "SELECT body_json FROM turn_items WHERE status IN ('idle','pending','running','waiting')",
            [],
        )
    }

    fn max_item_ordinal(&self, thread_id: &str) -> Result<Option<u32>> {
        Ok(self.conn().query_row(
            "SELECT MAX(ordinal) FROM turn_items WHERE thread_id = ?1",
            [thread_id],
            |r| r.get::<_, Option<u32>>(0),
        )?)
    }

    fn delete_item(&self, id: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM turn_items WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- runtime requests ----

    fn upsert_request(&self, r: &RuntimeRequest) -> Result<()> {
        self.conn().execute(
            "INSERT INTO runtime_requests (id, thread_id, run_id, turn_item_id, kind, status,
               created_at, resolved_at, data_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET run_id = excluded.run_id,
               turn_item_id = excluded.turn_item_id, kind = excluded.kind,
               status = excluded.status, resolved_at = excluded.resolved_at,
               data_json = excluded.data_json",
            params![
                r.id,
                r.thread_id,
                r.run_id,
                r.turn_item_id,
                wire(&r.kind)?,
                wire(&r.status)?,
                ts(&r.created_at),
                ots(&r.resolved_at),
                json(r)?
            ],
        )?;
        Ok(())
    }

    fn request(&self, id: &str) -> Result<Option<RuntimeRequest>> {
        query_one(
            self.conn(),
            "SELECT data_json FROM runtime_requests WHERE id = ?1",
            [id],
        )
    }

    fn requests(&self, thread_id: &str) -> Result<Vec<RuntimeRequest>> {
        query_json(
            self.conn(),
            "SELECT data_json FROM runtime_requests WHERE thread_id = ?1 ORDER BY created_at, id",
            [thread_id],
        )
    }

    fn pending_requests(&self) -> Result<Vec<RuntimeRequest>> {
        query_json(
            self.conn(),
            "SELECT data_json FROM runtime_requests WHERE status = 'pending'",
            [],
        )
    }

    // ---- checkpoints ----

    /// One row per (thread, ordinal): a recapture after a rollback replaces the stale row.
    fn upsert_checkpoint(&self, c: &Checkpoint, cwd: &str) -> Result<()> {
        self.conn().execute(
            "DELETE FROM checkpoints WHERE thread_id = ?1 AND ordinal = ?2 AND id <> ?3",
            params![c.thread_id, c.ordinal, c.id],
        )?;
        self.conn().execute(
            "INSERT INTO checkpoints (id, thread_id, run_id, ordinal, cwd, git_ref, status,
               captured_at, data_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET run_id = excluded.run_id, ordinal = excluded.ordinal,
               cwd = excluded.cwd, git_ref = excluded.git_ref, status = excluded.status,
               captured_at = excluded.captured_at, data_json = excluded.data_json",
            params![
                c.id,
                c.thread_id,
                c.run_id,
                c.ordinal,
                cwd,
                c.git_ref,
                wire(&c.status)?,
                ts(&c.captured_at),
                json(c)?
            ],
        )?;
        Ok(())
    }

    fn checkpoints(&self, thread_id: &str) -> Result<Vec<Checkpoint>> {
        query_json(
            self.conn(),
            "SELECT data_json FROM checkpoints WHERE thread_id = ?1 ORDER BY ordinal",
            [thread_id],
        )
    }

    /// Distinct working directories checkpoints of this thread were taken in.
    fn checkpoint_cwds(&self, thread_id: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn()
            .prepare_cached("SELECT DISTINCT cwd FROM checkpoints WHERE thread_id = ?1")?;
        let rows = stmt.query_map([thread_id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- plans ----

    fn upsert_plan(&self, p: &ProposedPlan) -> Result<()> {
        self.conn().execute(
            "INSERT INTO plans (id, thread_id, run_id, status, created_at, updated_at, data_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET run_id = excluded.run_id, status = excluded.status,
               updated_at = excluded.updated_at, data_json = excluded.data_json",
            params![
                p.id,
                p.thread_id,
                p.run_id,
                wire(&p.status)?,
                ts(&p.created_at),
                ts(&p.updated_at),
                json(p)?
            ],
        )?;
        Ok(())
    }

    fn plans(&self, thread_id: &str) -> Result<Vec<ProposedPlan>> {
        query_json(
            self.conn(),
            "SELECT data_json FROM plans WHERE thread_id = ?1 ORDER BY created_at, id",
            [thread_id],
        )
    }

    // ---- provider bindings ----

    fn upsert_binding(&self, b: &ProviderBinding) -> Result<()> {
        self.conn().execute(
            "INSERT INTO provider_bindings (thread_id, driver, native_thread_id, status,
               updated_at, data_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(thread_id) DO UPDATE SET driver = excluded.driver,
               native_thread_id = excluded.native_thread_id, status = excluded.status,
               updated_at = excluded.updated_at, data_json = excluded.data_json",
            params![
                b.thread_id,
                wire(&b.driver)?,
                b.native_thread_id,
                wire(&b.status)?,
                ts(&b.updated_at),
                json(b)?
            ],
        )?;
        Ok(())
    }

    fn binding(&self, thread_id: &str) -> Result<Option<ProviderBinding>> {
        query_one(
            self.conn(),
            "SELECT data_json FROM provider_bindings WHERE thread_id = ?1",
            [thread_id],
        )
    }

    /// Everything for one thread.
    fn thread_rows(&self, thread_id: &str) -> Result<ThreadRows> {
        Ok(ThreadRows {
            runs: self.runs(thread_id)?,
            items: self.items(thread_id)?,
            requests: self.requests(thread_id)?,
            checkpoints: self.checkpoints(thread_id)?,
            plans: self.plans(thread_id)?,
            binding: self.binding(thread_id)?,
        })
    }
}

impl Db for Store {
    fn conn(&self) -> &Connection {
        &self.conn
    }
}

impl Db for Tx<'_> {
    fn conn(&self) -> &Connection {
        &self.tx
    }
}

#[cfg(test)]
mod tests;

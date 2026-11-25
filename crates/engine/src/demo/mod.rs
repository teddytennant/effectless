//! Demo mode: an in-memory engine seeded with sample projects and threads. Commands mutate
//! in-memory state through the normal actors; providers, git and title generation are fakes,
//! so nothing touches disk, a CLI or a repository.

mod adapter;
mod seed;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use parking_lot::Mutex;
use tern_core::settings::Settings;
use tern_core::*;
use tern_store::{Db, Store};

use crate::Paths;
use crate::actor::ActorMsg;
use crate::backend::{Backends, BoxFuture, GitBackend, SettingsSource, Timing, TitleGenerator};
use crate::engine::{Inner, UpdateFn};

pub(crate) use adapter::DemoAdapter;

/// Pretends every folder is a repo and every capture changed one file.
pub(crate) struct DemoGit;

impl GitBackend for DemoGit {
    fn repo_root(&self, path: &Path) -> Option<PathBuf> {
        Some(path.to_path_buf())
    }
    fn capture(&self, _cwd: &Path, thread_id: &str, ordinal: u32) -> Result<String> {
        Ok(format!(
            "refs/tern/checkpoints/{thread_id}/ordinal/{ordinal}"
        ))
    }
    fn summarize(&self, _cwd: &Path, _from: &str, _to: &str) -> Result<Vec<CheckpointFileSummary>> {
        Ok(vec![CheckpointFileSummary {
            path: "src/app.rs".into(),
            kind: "modified".into(),
            additions: 12,
            deletions: 3,
        }])
    }
    fn restore(&self, _cwd: &Path, _git_ref: &str) -> Result<()> {
        Ok(())
    }
    fn delete_all(&self, _cwd: &Path, _thread_id: &str) -> Result<()> {
        Ok(())
    }
    fn create_worktree(
        &self,
        _repo: &Path,
        worktrees_root: &Path,
        _base_ref: &str,
        branch: &str,
        _start_from_origin: bool,
    ) -> Result<PathBuf> {
        Ok(worktrees_root.join(branch.replace('/', "-")))
    }
    fn unique_branch_name(&self, _repo: &Path, prefix: &str, slug: &str) -> Result<String> {
        Ok(if prefix.is_empty() {
            slug.to_string()
        } else {
            format!("{prefix}/{slug}")
        })
    }
}

/// Titles from the first few words of the message, after a short delay.
pub(crate) struct DemoTitles;

impl TitleGenerator for DemoTitles {
    fn thread_title(
        &self,
        _model: ModelSelection,
        context: String,
        _cwd: PathBuf,
    ) -> BoxFuture<Result<String>> {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(700)).await;
            let words: Vec<String> = context
                .split_whitespace()
                .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
                .filter(|w| !w.is_empty())
                .take(6)
                .collect();
            let mut title = words.join(" ");
            if let Some(first) = title.get(..1) {
                title = first.to_uppercase() + &title[1..];
            }
            Ok(title)
        })
    }
}

pub(crate) fn demo_backends(scripts: adapter::Scripts) -> Backends {
    Backends {
        adapters: vec![
            Arc::new(DemoAdapter {
                instance: "codex",
                driver: Driver::Codex,
                scripts: scripts.clone(),
            }),
            Arc::new(DemoAdapter {
                instance: "claudeAgent",
                driver: Driver::Claude,
                scripts,
            }),
        ],
        git: Arc::new(DemoGit),
        titles: Arc::new(DemoTitles),
        settings: SettingsSource::Fixed(Arc::new(Settings::default())),
        timing: Timing::default(),
    }
}

pub(crate) fn start(on_update: UpdateFn) -> Arc<Inner> {
    let seeded = seed::seed();
    let scripts = Arc::new(Mutex::new(seeded.scripts));
    let store = Store::open_in_memory().expect("in-memory sqlite");
    let inner = Inner::new(Paths::from_env(), store, demo_backends(scripts), on_update)
        .expect("demo engine runtime");

    {
        let mut store = inner.store.lock();
        store
            .write(|tx| {
                for p in &seeded.projects {
                    tx.upsert_project(p)?;
                }
                for t in &seeded.threads {
                    tx.upsert_thread(&t.thread)?;
                    if let Some(b) = &t.rows.binding {
                        tx.upsert_binding(b)?;
                    }
                    for r in &t.rows.runs {
                        tx.upsert_run(r)?;
                    }
                    for p in &t.rows.plans {
                        tx.upsert_plan(p)?;
                    }
                    for i in &t.rows.items {
                        tx.upsert_item(i)?;
                    }
                    for q in &t.rows.requests {
                        tx.upsert_request(q)?;
                    }
                    for c in &t.rows.checkpoints {
                        tx.upsert_checkpoint(c, &t.cwd)?;
                    }
                }
                Ok(())
            })
            .expect("seed demo data");
    }
    inner.state.write().projects = seeded.projects;
    inner.state.write().providers = vec![
        adapter::provider_status("codex"),
        adapter::provider_status("claudeAgent"),
    ];
    for t in seeded.threads {
        inner.install_thread(t.thread, t.rows);
    }
    for (thread_id, run_id) in seeded.resume {
        inner.to_actor(&thread_id, ActorMsg::ResumeSession { run_id });
    }
    inner
}

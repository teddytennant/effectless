//! Shared engine state, command routing, project commands and startup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use parking_lot::{Mutex, RwLock};
use tern_core::*;
use tern_store::{Db, Store, ThreadRows};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::mpsc;

use crate::Paths;
use crate::actor::{self, ActorMsg};
use crate::backend::Backends;
use crate::shell::derive_shell;

pub(crate) type UpdateFn = Box<dyn Fn(Update) + Send + Sync>;

/// One thread as the UI sees it. Replaced wholesale by the thread's actor on every publish.
#[derive(Clone)]
pub(crate) struct Published {
    pub detail: Arc<ThreadDetail>,
    pub shell: ThreadShell,
    pub binding: Option<ProviderBinding>,
}

#[derive(Default)]
pub(crate) struct State {
    pub projects: Vec<Project>,
    pub threads: HashMap<String, Published>,
    pub providers: Vec<ProviderStatus>,
}

pub(crate) struct Inner {
    pub paths: Paths,
    pub rt: Mutex<Option<Runtime>>,
    pub handle: Handle,
    pub store: Mutex<Store>,
    pub state: RwLock<State>,
    pub actors: Mutex<HashMap<String, mpsc::UnboundedSender<ActorMsg>>>,
    pub backends: Backends,
    pub on_update: UpdateFn,
    pub revision: AtomicU64,
}

impl Inner {
    pub fn new(
        paths: Paths,
        store: Store,
        backends: Backends,
        on_update: UpdateFn,
    ) -> Result<Arc<Self>> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("tern-engine")
            .enable_all()
            .build()
            .context("start tokio runtime")?;
        let handle = rt.handle().clone();
        Ok(Arc::new(Self {
            paths,
            rt: Mutex::new(Some(rt)),
            handle,
            store: Mutex::new(store),
            state: RwLock::new(State::default()),
            actors: Mutex::new(HashMap::new()),
            backends,
            on_update,
            revision: AtomicU64::new(1),
        }))
    }

    pub fn emit(&self, update: Update) {
        (self.on_update)(update);
    }

    pub fn error(&self, message: impl Into<String>) {
        self.emit(Update::Error {
            message: message.into(),
        });
    }

    pub fn next_revision(&self) -> u64 {
        self.revision.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn project(&self, id: &str) -> Option<Project> {
        self.state
            .read()
            .projects
            .iter()
            .find(|p| p.id == id)
            .cloned()
    }

    // ---- reads ----

    pub fn snapshot(&self) -> Snapshot {
        let state = self.state.read();
        let mut threads: Vec<ThreadShell> =
            state.threads.values().map(|p| p.shell.clone()).collect();
        threads.sort_by(|a, b| {
            b.last_activity_at
                .cmp(&a.last_activity_at)
                .then_with(|| a.thread.id.cmp(&b.thread.id))
        });
        Snapshot {
            projects: state.projects.clone(),
            threads,
            providers: state.providers.clone(),
            revision: self.revision.load(Ordering::Relaxed),
        }
    }

    pub fn thread(&self, id: &str) -> Option<ThreadDetail> {
        self.state
            .read()
            .threads
            .get(id)
            .map(|p| (*p.detail).clone())
    }

    // ---- loading ----

    /// Put a thread into the published map (startup and demo seeding).
    pub fn install_thread(&self, thread: Thread, rows: ThreadRows) {
        let detail = ThreadDetail {
            context_usage: rows.binding.as_ref().and_then(|b| b.context_usage.clone()),
            thread,
            runs: rows.runs,
            items: rows.items,
            requests: rows.requests,
            checkpoints: rows.checkpoints,
            plans: rows.plans,
            session: None,
            live_output: HashMap::new(),
            revision: self.next_revision(),
        };
        let shell = derive_shell(&detail);
        let id = detail.thread.id.clone();
        self.state.write().threads.insert(
            id,
            Published {
                detail: Arc::new(detail),
                shell,
                binding: rows.binding,
            },
        );
    }

    /// Load everything from the store after recovery.
    pub fn load(self: &Arc<Self>) -> Result<()> {
        let touched = self.store.lock().recover()?;
        if !touched.is_empty() {
            tracing::info!(threads = touched.len(), "recovered interrupted runs");
        }
        let (projects, threads) = {
            let store = self.store.lock();
            let projects = store.projects()?;
            let mut threads = Vec::new();
            for t in store.threads()? {
                let rows = store.thread_rows(&t.id)?;
                threads.push((t, rows));
            }
            (projects, threads)
        };
        self.state.write().projects = projects;
        for (t, rows) in threads {
            self.install_thread(t, rows);
        }
        Ok(())
    }

    /// Startup sweeps: finish deleting soft-deleted threads; capture checkpoints for settled
    /// runs that never got one (spec/07 §6.3).
    pub fn startup_sweeps(self: &Arc<Self>) {
        let deleted = self.store.lock().deleted_threads().unwrap_or_default();
        for t in deleted {
            self.spawn_purge(t.id);
        }
        let missing: Vec<(String, String)> = {
            let state = self.state.read();
            state
                .threads
                .values()
                .filter_map(|p| {
                    let run = crate::shell::latest_run(&p.detail.runs)?;
                    let settled = matches!(
                        run.status,
                        RunStatus::Completed | RunStatus::Interrupted | RunStatus::Cancelled
                    );
                    (settled && run.started_at.is_some() && run.checkpoint_id.is_none())
                        .then(|| (p.detail.thread.id.clone(), run.id.clone()))
                })
                .collect()
        };
        for (thread_id, run_id) in missing {
            self.to_actor(&thread_id, ActorMsg::CaptureMissing { run_id });
        }
    }

    /// Delete a soft-deleted thread's checkpoint refs, then its rows.
    pub fn spawn_purge(self: &Arc<Self>, thread_id: String) {
        let inner = self.clone();
        self.handle.spawn_blocking(move || {
            let cwds = inner
                .store
                .lock()
                .checkpoint_cwds(&thread_id)
                .unwrap_or_default();
            for cwd in cwds {
                let path = Path::new(&cwd);
                if path.exists()
                    && let Err(e) = inner.backends.git.delete_all(path, &thread_id)
                {
                    tracing::warn!(%thread_id, %cwd, "checkpoint ref sweep failed: {e:#}");
                    return;
                }
            }
            if let Err(e) = inner.store.lock().hard_delete_thread(&thread_id) {
                tracing::warn!(%thread_id, "hard delete failed: {e:#}");
            }
        });
    }

    // ---- routing ----

    /// The actor for a thread, started on first use. None if the thread is unknown.
    pub fn actor(self: &Arc<Self>, thread_id: &str) -> Option<mpsc::UnboundedSender<ActorMsg>> {
        let mut actors = self.actors.lock();
        if let Some(tx) = actors.get(thread_id)
            && !tx.is_closed()
        {
            return Some(tx.clone());
        }
        let published = self.state.read().threads.get(thread_id).cloned()?;
        let tx = actor::spawn(self.clone(), published);
        actors.insert(thread_id.to_string(), tx.clone());
        Some(tx)
    }

    pub fn to_actor(self: &Arc<Self>, thread_id: &str, msg: ActorMsg) -> bool {
        match self.actor(thread_id) {
            Some(tx) => tx.send(msg).is_ok(),
            None => false,
        }
    }

    pub fn send(self: &Arc<Self>, command: Command) {
        let result = match command {
            Command::AddProject { path } => self.add_project(&path),
            Command::RenameProject { project_id, title } => {
                self.rename_project(&project_id, &title)
            }
            Command::RemoveProject { project_id } => self.remove_project(&project_id),
            Command::UpdateProjectScripts {
                project_id,
                scripts,
            } => self.update_project(&project_id, |p| p.scripts = scripts),
            Command::RefreshProviders => {
                self.refresh_providers();
                Ok(())
            }
            Command::LaunchThread {
                thread_id,
                project_id,
                model_selection,
                runtime_mode,
                interaction_mode,
                workspace,
                message,
            } => self.launch_thread(
                thread_id,
                project_id,
                model_selection,
                runtime_mode,
                interaction_mode,
                workspace,
                message,
            ),
            other => {
                let Some(thread_id) = thread_of(&other) else {
                    return;
                };
                let thread_id = thread_id.to_string();
                if self.to_actor(&thread_id, ActorMsg::Command(other)) {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("This thread no longer exists."))
                }
            }
        };
        if let Err(e) = result {
            self.error(format!("{e:#}"));
        }
    }

    fn publish_projects(&self) {
        self.next_revision();
        self.emit(Update::Snapshot);
    }

    fn add_project(self: &Arc<Self>, path: &str) -> Result<()> {
        let root = normalize_root(path);
        if root.is_empty() {
            anyhow::bail!("Choose a folder to add.");
        }
        if self
            .state
            .read()
            .projects
            .iter()
            .any(|p| p.workspace_root == root)
        {
            anyhow::bail!("{root} is already a project.");
        }
        let t = now();
        let title = Path::new(&root)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&root)
            .to_string();
        let project = Project {
            id: new_id(),
            title,
            has_custom_title: false,
            workspace_root: root,
            default_model_selection: None,
            default_thread_env_mode: None,
            favicon_path: None,
            scripts: vec![],
            pinned_at: None,
            archived_at: None,
            created_at: t,
            updated_at: t,
        };
        self.store.lock().upsert_project(&project)?;
        self.state.write().projects.push(project);
        self.publish_projects();
        Ok(())
    }

    fn update_project(&self, id: &str, f: impl FnOnce(&mut Project)) -> Result<()> {
        let mut project = self
            .project(id)
            .ok_or_else(|| anyhow::anyhow!("This project no longer exists."))?;
        f(&mut project);
        project.updated_at = now();
        self.store.lock().upsert_project(&project)?;
        if let Some(p) = self.state.write().projects.iter_mut().find(|p| p.id == id) {
            *p = project;
        }
        self.publish_projects();
        Ok(())
    }

    fn rename_project(&self, id: &str, title: &str) -> Result<()> {
        let title = title.trim();
        if title.is_empty() {
            anyhow::bail!("Project title cannot be empty");
        }
        self.update_project(id, |p| {
            p.title = title.to_string();
            p.has_custom_title = true;
        })
    }

    fn remove_project(self: &Arc<Self>, id: &str) -> Result<()> {
        if self.project(id).is_none() {
            anyhow::bail!("This project no longer exists.");
        }
        let threads: Vec<String> = self
            .state
            .read()
            .threads
            .values()
            .filter(|p| p.detail.thread.project_id == id)
            .map(|p| p.detail.thread.id.clone())
            .collect();
        for t in threads {
            self.to_actor(
                &t,
                ActorMsg::Command(Command::DeleteThread {
                    thread_id: t.clone(),
                }),
            );
        }
        self.store.lock().delete_project(id)?;
        self.state.write().projects.retain(|p| p.id != id);
        self.publish_projects();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn launch_thread(
        self: &Arc<Self>,
        thread_id: String,
        project_id: String,
        model_selection: ModelSelection,
        runtime_mode: RuntimeMode,
        interaction_mode: InteractionMode,
        workspace: WorkspaceStrategy,
        message: Option<OutgoingMessage>,
    ) -> Result<()> {
        if self.project(&project_id).is_none() {
            anyhow::bail!("This thread's project no longer exists.");
        }
        if self.state.read().threads.contains_key(&thread_id) {
            anyhow::bail!("A thread with this id already exists.");
        }
        let t = now();
        let (branch, worktree_path) = match &workspace {
            WorkspaceStrategy::Root { branch } => (branch.clone(), None),
            WorkspaceStrategy::ExistingWorktree {
                worktree_path,
                branch,
            } => (branch.clone(), Some(worktree_path.clone())),
            WorkspaceStrategy::Worktree { .. } => (None, None),
        };
        let thread = Thread {
            id: thread_id.clone(),
            project_id,
            title: NEW_THREAD_TITLE.into(),
            model_selection,
            runtime_mode,
            interaction_mode,
            branch,
            worktree_path,
            active_provider_thread_id: None,
            title_renamed: false,
            title_generation_started_at: None,
            created_at: t,
            updated_at: t,
            archived_at: None,
            pinned_at: None,
            settled_at: None,
            last_visited_at: Some(t),
            linked_pull_request: None,
            deleted_at: None,
        };
        self.store.lock().upsert_thread(&thread)?;
        self.install_thread(thread, ThreadRows::default());
        self.next_revision();
        self.emit(Update::Snapshot);
        self.to_actor(&thread_id, ActorMsg::Launched { workspace, message });
        Ok(())
    }

    pub fn refresh_providers(self: &Arc<Self>) {
        let inner = self.clone();
        self.handle.spawn(async move {
            let settings = inner.backends.settings.get();
            let mut out = Vec::new();
            for adapter in &inner.backends.adapters {
                let cfg = settings.provider_instance(adapter.instance_id());
                let mut status = adapter.status(cfg.binary_path().map(PathBuf::from)).await;
                status.enabled = status.enabled && cfg.is_enabled();
                if let Some(name) = cfg.display_name.clone() {
                    status.display_name = name;
                }
                out.push(status);
            }
            inner.state.write().providers = out;
            inner.next_revision();
            inner.emit(Update::Snapshot);
        });
    }

    /// Ask every actor to kill its provider session and flush, wait up to a second for
    /// them, then drop the runtime.
    pub fn shutdown(&self) {
        let actors: Vec<_> = self.actors.lock().drain().map(|(_, tx)| tx).collect();
        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        let mut expected = 0;
        for tx in actors {
            if tx.send(ActorMsg::Shutdown(ack_tx.clone())).is_ok() {
                expected += 1;
            }
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        for _ in 0..expected {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if ack_rx.recv_timeout(left).is_err() {
                break;
            }
        }
        if let Some(rt) = self.rt.lock().take() {
            rt.shutdown_background();
        }
    }
}

fn normalize_root(path: &str) -> String {
    let p = path.trim();
    let p = if p.len() > 1 {
        p.trim_end_matches('/')
    } else {
        p
    };
    p.to_string()
}

/// The thread a thread-scoped command targets.
pub(crate) fn thread_of(command: &Command) -> Option<&str> {
    Some(match command {
        Command::SendMessage { thread_id, .. }
        | Command::Interrupt { thread_id, .. }
        | Command::ForceStop { thread_id }
        | Command::RespondToRequest { thread_id, .. }
        | Command::DismissUserInput { thread_id, .. }
        | Command::RollbackToCheckpoint { thread_id, .. }
        | Command::SetModelSelection { thread_id, .. }
        | Command::SetRuntimeMode { thread_id, .. }
        | Command::SetInteractionMode { thread_id, .. }
        | Command::RenameThread { thread_id, .. }
        | Command::RegenerateTitle { thread_id }
        | Command::SetPinned { thread_id, .. }
        | Command::SetSettled { thread_id, .. }
        | Command::SetArchived { thread_id, .. }
        | Command::DeleteThread { thread_id }
        | Command::MarkVisited { thread_id }
        | Command::MarkUnread { thread_id }
        | Command::CancelQueuedRun { thread_id, .. } => thread_id,
        Command::AddProject { .. }
        | Command::RenameProject { .. }
        | Command::RemoveProject { .. }
        | Command::UpdateProjectScripts { .. }
        | Command::LaunchThread { .. }
        | Command::RefreshProviders => return None,
    })
}

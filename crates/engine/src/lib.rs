//! Orchestration (spec/07 §2): one actor per thread, provider sessions, checkpoints,
//! title generation, persistence through `tern-store`. The app holds an `EngineHandle`.

mod actor;
pub mod backend;
mod demo;
mod engine;
pub mod shell;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use tern_core::*;
use tern_store::Store;

pub use actor::{
    DELIVERY_FAILED, FORCE_STOP_REASON, PROVIDER_STOP_REASON, USER_STOP_REASON, checkpoint_id,
};
pub use backend::{Backends, GitBackend, SettingsSource, Timing, TitleGenerator};
use engine::Inner;
pub use tern_core::settings;

/// Where Tern keeps its state. Default `~/.tern` (override `TERN_HOME`).
#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
}

impl Paths {
    pub fn from_env() -> Self {
        let home = std::env::var_os("TERN_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".tern"));
        Self { home }
    }
    pub fn userdata(&self) -> PathBuf {
        self.home.join("userdata")
    }
    pub fn db(&self) -> PathBuf {
        self.userdata().join("state.sqlite")
    }
    pub fn attachments(&self) -> PathBuf {
        self.userdata().join("attachments")
    }
    pub fn worktrees(&self) -> PathBuf {
        self.home.join("worktrees")
    }
    pub fn settings(&self) -> PathBuf {
        self.userdata().join("settings.json")
    }
    pub fn client_settings(&self) -> PathBuf {
        self.userdata().join("client-settings.json")
    }
    pub fn keybindings(&self) -> PathBuf {
        self.userdata().join("keybindings.json")
    }
    pub fn logs(&self) -> PathBuf {
        self.userdata().join("logs")
    }
}

/// Cheap to clone; safe to use from any thread. The engine shuts down when the last clone
/// is dropped.
#[derive(Clone)]
pub struct EngineHandle {
    inner: Arc<Inner>,
    _guard: Arc<Guard>,
}

struct Guard(Arc<Inner>);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

/// `$T3CODE_HOME/userdata` or `~/.t3/userdata`, for the one-time settings import.
fn t3_userdata() -> Option<PathBuf> {
    let home = std::env::var_os("T3CODE_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".t3")))?;
    Some(home.join("userdata"))
}

impl EngineHandle {
    /// Opens the store, recovers interrupted runs, starts the tokio runtime.
    /// `on_update` is called from engine threads; the app forwards to its executor.
    pub fn start(
        paths: Paths,
        on_update: Box<dyn Fn(Update) + Send + Sync>,
    ) -> anyhow::Result<Self> {
        let userdata = settings::load_userdata(&paths.userdata(), t3_userdata().as_deref());
        if let Some(e) = &userdata.settings.unreadable {
            tracing::warn!("settings.json is unreadable, running on defaults: {e}");
        }
        let store = Store::open(&paths.db())?;
        let backends = Backends::real(&paths);
        Self::start_with(paths, store, backends, on_update)
    }

    /// Start with an explicit store and backends (tests inject fakes here).
    pub fn start_with(
        paths: Paths,
        store: Store,
        backends: Backends,
        on_update: Box<dyn Fn(Update) + Send + Sync>,
    ) -> anyhow::Result<Self> {
        let inner = Inner::new(paths, store, backends, on_update)?;
        inner.load()?;
        inner.startup_sweeps();
        inner.refresh_providers();
        Ok(Self::wrap(inner))
    }

    fn wrap(inner: Arc<Inner>) -> Self {
        Self {
            _guard: Arc::new(Guard(inner.clone())),
            inner,
        }
    }

    /// In-memory engine seeded with sample projects and threads, no providers.
    /// Used by the app's `--demo` flag and UI screenshots. Never touches disk, git or a CLI.
    pub fn demo(on_update: Box<dyn Fn(Update) + Send + Sync>) -> Self {
        Self::wrap(demo::start(on_update))
    }

    pub fn snapshot(&self) -> Snapshot {
        self.inner.snapshot()
    }

    pub fn thread(&self, thread_id: &str) -> Option<ThreadDetail> {
        self.inner.thread(thread_id)
    }

    /// Fire and forget; failures come back as `Update::Error`.
    pub fn send(&self, command: Command) {
        self.inner.send(command);
    }

    pub fn paths(&self) -> Paths {
        self.inner.paths.clone()
    }
}

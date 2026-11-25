//! Orchestration (spec/07 §2): one actor per thread, provider sessions, checkpoints,
//! title generation, persistence through `tern-store`. The app holds an `EngineHandle`.

use std::path::PathBuf;
use std::sync::Arc;

use tern_core::*;

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

/// Cheap to clone; safe to use from any thread.
#[derive(Clone)]
pub struct EngineHandle {
    _inner: Arc<()>,
}

impl EngineHandle {
    /// Opens the store, recovers interrupted runs, starts the tokio runtime.
    /// `on_update` is called from engine threads; the app forwards to its executor.
    pub fn start(paths: Paths, on_update: Box<dyn Fn(Update) + Send + Sync>) -> anyhow::Result<Self> {
        let _ = (paths, on_update);
        todo!()
    }

    /// In-memory engine seeded with sample projects and threads, no providers.
    /// Used by the app's `--demo` flag and UI screenshots.
    pub fn demo(on_update: Box<dyn Fn(Update) + Send + Sync>) -> Self {
        let _ = on_update;
        todo!()
    }

    pub fn snapshot(&self) -> Snapshot {
        todo!()
    }

    pub fn thread(&self, thread_id: &str) -> Option<ThreadDetail> {
        let _ = thread_id;
        todo!()
    }

    /// Fire and forget; failures come back as `Update::Error`.
    pub fn send(&self, command: Command) {
        let _ = command;
        todo!()
    }

    pub fn paths(&self) -> Paths {
        todo!()
    }
}

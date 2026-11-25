//! Agent drivers. Each adapter runs a CLI as a child process and turns its protocol
//! into `tern_core::ProviderEvent`s. spec/07 §3 (Codex), §4 (Claude), §9 (text generation).
//!
//! Contract notes for the engine:
//! - `TurnItem.ordinal` on emitted items is the item's position inside its turn
//!   (1, 2, 3, ...). The engine adds `run.ordinal * 100` (RunExecutionService.ts:962);
//!   the user message the engine writes itself takes position 0.
//! - Every turn that `start_turn` accepted ends with exactly one `TurnTerminal`, including
//!   when the process dies, a pipe breaks or a line can't be parsed (`broken: true`).
//!   Open tool items are settled (`interrupted`) and pending requests closed
//!   (`cancelled`) before it.
//! - Text deltas are coalesced in the adapter (50 ms, plans 400 ms) and arrive as full
//!   snapshots in `ItemUpserted`, idempotent by `TurnItem.id`.

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use tern_core::*;
use tokio::sync::mpsc;

pub mod claude;
pub mod codex;
pub mod textgen;

mod discovery;
mod items;
mod process;
mod util;

pub use discovery::{expand_tilde, find_binary};

/// What a session needs to start a turn.
#[derive(Clone, Debug)]
pub struct TurnRequest {
    pub run_id: String,
    pub text: String,
    /// Absolute paths of attachment files on disk, with their metadata.
    pub attachments: Vec<(ChatAttachment, PathBuf)>,
    pub model_selection: ModelSelection,
    pub runtime_mode: RuntimeMode,
    pub interaction_mode: InteractionMode,
    pub cwd: PathBuf,
}

#[derive(Clone, Debug)]
pub struct SessionConfig {
    /// Tern thread id; adapters stamp it on emitted TurnItems.
    pub thread_id: String,
    pub cwd: PathBuf,
    /// Native thread/session id to resume, if the binding has one.
    pub resume_native_thread_id: Option<String>,
    pub model_selection: ModelSelection,
    pub runtime_mode: RuntimeMode,
    /// Override for the CLI binary (settings); None = discover on PATH.
    pub binary_path: Option<PathBuf>,
    pub env: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct RequestResponse {
    pub request: RuntimeRequest,
    pub decision: Option<ApprovalDecision>,
    pub feedback: Option<String>,
    pub answers: Option<HashMap<String, serde_json::Value>>,
}

/// A live provider session for one Tern thread.
#[async_trait]
pub trait ProviderSessionHandle: Send + Sync {
    fn capabilities(&self) -> Capabilities;
    /// Starts a turn. Returns once the provider accepted it; events for it arrive on the
    /// session's event channel. An `Err` means the turn never started and no
    /// `TurnTerminal` will follow.
    async fn start_turn(&self, turn: TurnRequest) -> anyhow::Result<()>;
    async fn steer(&self, run_id: &str, text: String) -> anyhow::Result<()>;
    async fn interrupt(&self, run_id: &str) -> anyhow::Result<()>;
    async fn respond(&self, response: RequestResponse) -> anyhow::Result<()>;
    async fn set_model(&self, selection: ModelSelection) -> anyhow::Result<()>;
    /// Graceful stop; kills the process if it doesn't exit.
    async fn shutdown(&self);
    /// Hard kill of the process group (Force stop).
    fn kill(&self);
}

#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn driver(&self) -> Driver;
    fn instance_id(&self) -> &str;
    /// Installed? signed in? version and models. Never opens a session.
    async fn status(&self, binary_path: Option<PathBuf>) -> ProviderStatus;
    async fn open_session(
        &self,
        config: SessionConfig,
        events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> anyhow::Result<Box<dyn ProviderSessionHandle>>;
}

pub fn builtin_adapters() -> Vec<Box<dyn ProviderAdapter>> {
    vec![
        Box::new(codex::CodexAdapter::new()),
        Box::new(claude::ClaudeAdapter::new()),
    ]
}

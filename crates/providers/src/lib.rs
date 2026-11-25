//! Agent drivers. Each adapter runs a CLI as a child process and turns its protocol
//! into `tern_core::ProviderEvent`s. spec/07 §3 (Codex), §4 (Claude), §9 (text generation).

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use tern_core::*;
use tokio::sync::mpsc;

pub mod claude;
pub mod codex;
pub mod textgen;

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
    /// Starts a turn. Events for it arrive on the session's event channel.
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
    vec![Box::new(codex::CodexAdapter::new()), Box::new(claude::ClaudeAdapter::new())]
}

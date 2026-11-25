//! `codex app-server` JSON-RPC client (spec/07 §3).
use super::*;

pub struct CodexAdapter;

impl CodexAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProviderAdapter for CodexAdapter {
    fn driver(&self) -> Driver {
        Driver::Codex
    }
    fn instance_id(&self) -> &str {
        "codex"
    }
    async fn status(&self, _binary_path: Option<PathBuf>) -> ProviderStatus {
        todo!()
    }
    async fn open_session(
        &self,
        _config: SessionConfig,
        _events: mpsc::UnboundedSender<ProviderEvent>,
    ) -> anyhow::Result<Box<dyn ProviderSessionHandle>> {
        todo!()
    }
}

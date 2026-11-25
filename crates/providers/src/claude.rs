//! `claude` CLI stream-json client (spec/07 §4).
use super::*;

pub struct ClaudeAdapter;

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProviderAdapter for ClaudeAdapter {
    fn driver(&self) -> Driver {
        Driver::Claude
    }
    fn instance_id(&self) -> &str {
        "claudeAgent"
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

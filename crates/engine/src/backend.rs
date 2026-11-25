//! The seams between the engine and the outside world: git, provider adapters, title
//! generation and settings. `Backends::real` wires the real crates; tests and demo mode pass
//! fakes so nothing touches git, a provider CLI or the disk.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tern_core::settings::Settings;
use tern_core::{CheckpointFileSummary, ModelSelection};
use tern_providers::ProviderAdapter;

use crate::Paths;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Git operations the engine needs (spec/07 §7-8). Blocking; the engine calls them from
/// `spawn_blocking`.
pub trait GitBackend: Send + Sync {
    fn repo_root(&self, path: &Path) -> Option<PathBuf>;
    fn capture(&self, cwd: &Path, thread_id: &str, ordinal: u32) -> Result<String>;
    fn summarize(
        &self,
        cwd: &Path,
        from_ref: &str,
        to_ref: &str,
    ) -> Result<Vec<CheckpointFileSummary>>;
    fn restore(&self, cwd: &Path, git_ref: &str) -> Result<()>;
    fn delete_all(&self, cwd: &Path, thread_id: &str) -> Result<()>;
    fn create_worktree(
        &self,
        repo: &Path,
        worktrees_root: &Path,
        base_ref: &str,
        branch: &str,
        start_from_origin: bool,
    ) -> Result<PathBuf>;
    fn unique_branch_name(&self, repo: &Path, prefix: &str, slug: &str) -> Result<String>;
}

/// `tern-git`.
pub struct RealGit;

impl GitBackend for RealGit {
    fn repo_root(&self, path: &Path) -> Option<PathBuf> {
        tern_git::repo_root(path)
    }
    fn capture(&self, cwd: &Path, thread_id: &str, ordinal: u32) -> Result<String> {
        tern_git::checkpoint::capture(cwd, thread_id, ordinal)
    }
    fn summarize(
        &self,
        cwd: &Path,
        from_ref: &str,
        to_ref: &str,
    ) -> Result<Vec<CheckpointFileSummary>> {
        tern_git::checkpoint::summarize(cwd, from_ref, to_ref)
    }
    fn restore(&self, cwd: &Path, git_ref: &str) -> Result<()> {
        tern_git::checkpoint::restore(cwd, git_ref)
    }
    fn delete_all(&self, cwd: &Path, thread_id: &str) -> Result<()> {
        tern_git::checkpoint::delete_all(cwd, thread_id)
    }
    fn create_worktree(
        &self,
        repo: &Path,
        worktrees_root: &Path,
        base_ref: &str,
        branch: &str,
        start_from_origin: bool,
    ) -> Result<PathBuf> {
        tern_git::worktree::create(repo, worktrees_root, base_ref, branch, start_from_origin)
    }
    fn unique_branch_name(&self, repo: &Path, prefix: &str, slug: &str) -> Result<String> {
        tern_git::worktree::unique_branch_name(repo, prefix, slug)
    }
}

/// One-shot thread title generation (spec/07 §9.8).
pub trait TitleGenerator: Send + Sync {
    fn thread_title(
        &self,
        model: ModelSelection,
        context: String,
        cwd: PathBuf,
    ) -> BoxFuture<Result<String>>;
}

/// `tern_providers::textgen`.
pub struct RealTitles;

impl TitleGenerator for RealTitles {
    fn thread_title(
        &self,
        model: ModelSelection,
        context: String,
        cwd: PathBuf,
    ) -> BoxFuture<Result<String>> {
        Box::pin(async move {
            use tern_providers::textgen::{TextGenOptions, ThreadTitleInput, thread_title};
            let opts = TextGenOptions {
                model,
                ..TextGenOptions::default_for(cwd)
            };
            let input = ThreadTitleInput {
                message: context,
                project_name: None,
                instructions: None,
                attachments: Vec::new(),
                previous_title: None,
                linked_context: None,
                fork: None,
            };
            thread_title(&opts, &input).await
        })
    }
}

/// Where the engine reads settings from.
#[derive(Clone)]
pub enum SettingsSource {
    /// Re-read `<userdata>/settings.json` on use, so edits apply without a restart.
    Disk(PathBuf),
    Fixed(Arc<Settings>),
}

impl SettingsSource {
    pub fn get(&self) -> Arc<Settings> {
        match self {
            Self::Disk(dir) => Arc::new(Settings::load(dir).value),
            Self::Fixed(s) => s.clone(),
        }
    }
}

/// Timeouts from T3 (spec/07 §2.4-2.8), shortened in tests.
#[derive(Clone, Debug)]
pub struct Timing {
    /// `PROVIDER_INTERRUPT_TIMEOUT_MS`: after this, the UI offers Force stop.
    pub interrupt_timeout: Duration,
    /// Idle session release when the provider can resume natively.
    pub idle_timeout: Duration,
    /// Idle session release when it cannot.
    pub idle_timeout_no_resume: Duration,
    /// UI push coalescing while streaming (`ThreadLiveEventCoalescer.ts:18`).
    pub publish_interval: Duration,
    /// Baseline capture budget before a turn (`BASELINE_CAPTURE_BUDGET`).
    pub baseline_budget: Duration,
    /// Initial title generation retries, exponential from `title_retry_base`.
    pub title_retries: u32,
    pub title_retry_base: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            interrupt_timeout: Duration::from_millis(15_000),
            idle_timeout: Duration::from_secs(120),
            idle_timeout_no_resume: Duration::from_secs(30 * 60),
            publish_interval: Duration::from_millis(50),
            baseline_budget: Duration::from_secs(5),
            title_retries: 2,
            title_retry_base: Duration::from_secs(2),
        }
    }
}

/// Everything the engine talks to. Swap pieces to test or demo.
pub struct Backends {
    pub adapters: Vec<Arc<dyn ProviderAdapter>>,
    pub git: Arc<dyn GitBackend>,
    pub titles: Arc<dyn TitleGenerator>,
    pub settings: SettingsSource,
    pub timing: Timing,
}

impl Backends {
    /// Real git, real provider CLIs, settings from `paths.userdata()`.
    pub fn real(paths: &Paths) -> Self {
        Self {
            adapters: tern_providers::builtin_adapters()
                .into_iter()
                .map(Arc::from)
                .collect(),
            git: Arc::new(RealGit),
            titles: Arc::new(RealTitles),
            settings: SettingsSource::Disk(paths.userdata()),
            timing: Timing::default(),
        }
    }

    pub fn adapter(&self, instance_id: &str) -> Option<Arc<dyn ProviderAdapter>> {
        self.adapters
            .iter()
            .find(|a| a.instance_id() == instance_id)
            .cloned()
    }
}

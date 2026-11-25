//! Worktrees under `~/.tern/worktrees/<project>/<branch>` (spec/05 §4a.15, §4a.17).
use std::path::{Path, PathBuf};

pub fn create(repo: &Path, worktrees_root: &Path, base_ref: &str, branch: &str, start_from_origin: bool) -> anyhow::Result<PathBuf> {
    let _ = (repo, worktrees_root, base_ref, branch, start_from_origin);
    todo!()
}
pub fn remove(repo: &Path, worktree: &Path, force: bool) -> anyhow::Result<()> {
    let _ = (repo, worktree, force);
    todo!()
}
/// `tern/<slug>` branch name with a unique suffix if taken.
pub fn unique_branch_name(repo: &Path, prefix: &str, slug: &str) -> anyhow::Result<String> {
    let _ = (repo, prefix, slug);
    todo!()
}

//! Git for Tern: per-turn checkpoints, review diffs, refs, worktrees, commit/push/PR.
//! All functions shell out to `git` (and `gh` for GitHub). spec/05 §2.13-2.14, §4a, §4b;
//! spec/07 §7-8. Blocking: call from a background thread/task.

use std::path::{Path, PathBuf};

use tern_core::{CheckpointFileSummary, DiffScope, FileTotals};

pub mod checkpoint;
pub mod diff;
pub mod refs;
pub mod worktree;
pub mod remote;

pub use diff::{DiffFile, DiffHunk, DiffLine, DiffLineKind, FileDiff};

/// Repository root containing `path`, or None if not a git repo.
pub fn repo_root(path: &Path) -> Option<PathBuf> {
    let _ = path;
    todo!()
}

pub fn totals(files: &[CheckpointFileSummary]) -> FileTotals {
    FileTotals {
        count: files.len() as u32,
        additions: files.iter().map(|f| f.additions).sum(),
        deletions: files.iter().map(|f| f.deletions).sum(),
    }
}

/// Parsed diff for a review scope (the diff panel's data source).
pub fn scope_diff(cwd: &Path, scope: &DiffScope, checkpoint_refs: &dyn Fn(&str) -> Option<(String, String)>) -> anyhow::Result<Vec<FileDiff>> {
    let _ = (cwd, scope, checkpoint_refs);
    todo!()
}

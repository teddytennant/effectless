//! Hidden-ref workspace snapshots (spec/05 §2.13, spec/07 §7):
//! scratch index, `git add -A`, `write-tree`, `commit-tree`, `update-ref`
//! under `refs/tern/checkpoints/<thread-hash>/ordinal/<n>`.
use std::path::Path;

use tern_core::CheckpointFileSummary;

/// Snapshot the working tree of `cwd` into the ref for (thread, ordinal). Returns the ref.
pub fn capture(cwd: &Path, thread_id: &str, ordinal: u32) -> anyhow::Result<String> {
    let _ = (cwd, thread_id, ordinal);
    todo!()
}

/// Per-file +/- between two checkpoint refs.
pub fn summarize(cwd: &Path, from_ref: &str, to_ref: &str) -> anyhow::Result<Vec<CheckpointFileSummary>> {
    let _ = (cwd, from_ref, to_ref);
    todo!()
}

/// Restore the working tree to a checkpoint ref (revert).
pub fn restore(cwd: &Path, git_ref: &str) -> anyhow::Result<()> {
    let _ = (cwd, git_ref);
    todo!()
}

/// Delete every checkpoint ref of a thread.
pub fn delete_all(cwd: &Path, thread_id: &str) -> anyhow::Result<()> {
    let _ = (cwd, thread_id);
    todo!()
}

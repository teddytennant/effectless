//! One-shot text generation through `codex exec` / `claude -p` (spec/07 §9):
//! thread titles, commit messages, PR title/body, branch names.
use std::path::Path;

use tern_core::ModelSelection;

pub async fn thread_title(_model: &ModelSelection, _first_message: &str, _cwd: &Path) -> anyhow::Result<String> {
    todo!()
}
pub async fn commit_message(_model: &ModelSelection, _diff: &str, _cwd: &Path) -> anyhow::Result<String> {
    todo!()
}
/// Returns (title, body).
pub async fn pull_request(_model: &ModelSelection, _commits: &str, _diff: &str, _cwd: &Path) -> anyhow::Result<(String, String)> {
    todo!()
}
pub async fn branch_name(_model: &ModelSelection, _message: &str, _cwd: &Path) -> anyhow::Result<String> {
    todo!()
}

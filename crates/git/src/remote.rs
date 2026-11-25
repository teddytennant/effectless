//! Commit, push, pull request through `gh` (spec/05 §4b, spec/07 §8).
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct PullRequestInfo {
    pub number: u64,
    pub url: String,
    pub title: String,
    pub state: String,
    pub base: String,
    pub head: String,
}

/// Stage everything and commit. Returns the new commit sha.
pub fn commit_all(cwd: &Path, message: &str) -> anyhow::Result<String> {
    let _ = (cwd, message);
    todo!()
}
pub fn push(cwd: &Path, set_upstream: bool) -> anyhow::Result<()> {
    let _ = (cwd, set_upstream);
    todo!()
}
pub fn pull(cwd: &Path) -> anyhow::Result<()> {
    let _ = cwd;
    todo!()
}
pub fn open_pull_request(cwd: &Path, branch: &str) -> anyhow::Result<Option<PullRequestInfo>> {
    let _ = (cwd, branch);
    todo!()
}
pub fn create_pull_request(cwd: &Path, base: &str, title: &str, body: &str, draft: bool) -> anyhow::Result<PullRequestInfo> {
    let _ = (cwd, base, title, body, draft);
    todo!()
}

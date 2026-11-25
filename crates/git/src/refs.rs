//! Branches, status, checkout (spec/05 §4a.12-4a.14).
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Branch {
    pub name: String,
    pub is_remote: bool,
    pub is_current: bool,
    pub is_default: bool,
    /// Path of the worktree that has this branch checked out, if any.
    pub worktree_path: Option<String>,
    pub last_commit_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct RepoStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub has_changes: bool,
    pub changed_files: u32,
    pub additions: u32,
    pub deletions: u32,
    pub default_branch: Option<String>,
    pub has_origin: bool,
}

pub fn list_branches(cwd: &Path) -> anyhow::Result<Vec<Branch>> {
    let _ = cwd;
    todo!()
}
pub fn status(cwd: &Path) -> anyhow::Result<RepoStatus> {
    let _ = cwd;
    todo!()
}
pub fn checkout(cwd: &Path, branch: &str) -> anyhow::Result<()> {
    let _ = (cwd, branch);
    todo!()
}
pub fn create_branch(cwd: &Path, name: &str, base: Option<&str>) -> anyhow::Result<()> {
    let _ = (cwd, name, base);
    todo!()
}

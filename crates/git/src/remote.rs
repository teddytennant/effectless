//! Commit, push, pull, pull requests through `gh` (spec/05 §4b.8-4b.11, spec/07 §8.7).
//!
//! T3 talks to the GitHub REST/GraphQL API with a token it gets from `gh`; Tern lets `gh` do
//! the calls (`gh pr create`, `gh pr list/view --json`). Title, body and base mean the same.
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use serde_json::Value;

use crate::cmd::{Cmd, gh, git, split_nul};
use crate::refs::{
    branch_base_config_key, branch_base_ref_config_key, config_get, config_set, current_branch,
    head_commit, primary_remote, read_branch_base, remote_default_branch, remote_error, remotes,
    rev_count,
};
use crate::util::truncate_utf8;

const COMMIT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const PR_FIELDS: &str = "number,url,title,state,baseRefName,headRefName,isDraft";
const CLIP_MARKER: &str = "\n\n[truncated]";

#[derive(Clone, Debug, PartialEq)]
pub struct PullRequestInfo {
    pub number: u64,
    pub url: String,
    pub title: String,
    /// open | closed | merged
    pub state: String,
    pub base: String,
    pub head: String,
}

/// `parseCustomCommitMessage`: first line is the subject, the rest (trimmed) the body.
pub fn split_commit_message(message: &str) -> (String, String) {
    let message = message.trim();
    match message.split_once('\n') {
        Some((subject, body)) => (subject.trim().to_string(), body.trim().to_string()),
        None => (message.to_string(), String::new()),
    }
}

fn in_progress_operation(cwd: &Path) -> anyhow::Result<Option<&'static str>> {
    for (head, name) in [
        ("MERGE_HEAD", "merge"),
        ("CHERRY_PICK_HEAD", "cherry-pick"),
        ("REVERT_HEAD", "revert"),
    ] {
        if git(cwd).args(["rev-parse", "-q", "--verify", head]).ok()? {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

fn literal_pathspec_stdin(paths: &[String]) -> String {
    paths
        .iter()
        .map(|p| format!(":(top,literal){p}\0"))
        .collect()
}

fn has_staged_changes(cwd: &Path) -> anyhow::Result<bool> {
    let out = git(cwd)
        .args(["diff", "--cached", "--quiet", "--no-ext-diff"])
        .output()?;
    // On an unborn branch `diff --cached` compares against the empty tree, which is what we want.
    Ok(out.code == Some(1))
}

/// Stage (everything, or just `paths`) and commit. `Ok(None)` when staging found nothing to
/// commit (T3's `skipped_no_changes`). Hooks and the user's identity apply.
pub fn commit(
    cwd: &Path,
    message: &str,
    paths: Option<&[String]>,
) -> anyhow::Result<Option<String>> {
    let (subject, body) = split_commit_message(message);
    if subject.is_empty() {
        bail!("Commit message is empty.");
    }
    match paths {
        None => {
            let conflicted = split_nul(
                &git(cwd)
                    .args(["diff", "--name-only", "--diff-filter=U", "-z"])
                    .output()?
                    .stdout,
            );
            let root = crate::refs::show_toplevel_path(cwd).unwrap_or_else(|| cwd.to_path_buf());
            let marked: Vec<String> = conflicted
                .into_iter()
                .filter(|p| {
                    std::fs::read(root.join(p)).is_ok_and(|bytes| {
                        String::from_utf8_lossy(&bytes)
                            .lines()
                            .any(|l| l.starts_with("<<<<<<<") || l.starts_with(">>>>>>>"))
                    })
                })
                .collect();
            if !marked.is_empty() {
                bail!(
                    "Cannot commit with unresolved merge conflicts in {}. Resolve them, then commit.",
                    marked.join(", ")
                );
            }
            git(cwd).args(["add", "-A"]).run()?;
        }
        Some(paths) => {
            if let Some(op) = in_progress_operation(cwd)? {
                bail!(
                    "Cannot commit selected files during a {op}. Commit all files to finish the {op}."
                );
            }
            if head_commit(cwd)?.is_some() {
                git(cwd).args(["reset", "--quiet"]).run()?;
            }
            git(cwd)
                .args(["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"])
                .stdin(literal_pathspec_stdin(paths))
                .run()?;
        }
    }
    let merging = in_progress_operation(cwd)? == Some("merge");
    if !has_staged_changes(cwd)? && !merging {
        return Ok(None);
    }
    let mut cmd = git(cwd).args(["commit", "-m", &subject]);
    if !body.is_empty() {
        cmd = cmd.args(["-m", &body]);
    }
    if let Some(paths) = paths {
        cmd = cmd
            .args(["--only", "--pathspec-from-file=-", "--pathspec-file-nul"])
            .stdin(literal_pathspec_stdin(paths));
    }
    cmd.timeout(COMMIT_TIMEOUT).run()?;
    if let Some(paths) = paths {
        // `--only` leaves the index as it was for those paths; sync it with the new HEAD.
        let _ = git(cwd)
            .args([
                "reset",
                "--quiet",
                "HEAD",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ])
            .stdin(literal_pathspec_stdin(paths))
            .output();
    }
    let sha = git(cwd).args(["rev-parse", "HEAD"]).run()?;
    Ok(Some(sha.trim().to_string()))
}

/// Stage everything and commit. Returns the new commit sha.
pub fn commit_all(cwd: &Path, message: &str) -> anyhow::Result<String> {
    commit(cwd, message, None)?.context("Staging found no changes.")
}

#[derive(Clone, Debug, PartialEq)]
pub enum PushOutcome {
    Pushed {
        branch: String,
        remote: String,
        /// The branch name on the remote.
        upstream_branch: String,
        /// This push published the branch and wrote its upstream config.
        set_upstream: bool,
    },
    UpToDate,
}

fn is_alias_of_upstream_head(branch: &str, upstream_branch: &str, upstream_ref: &str) -> bool {
    branch == upstream_branch
        || (branch.ends_with(&format!("/{upstream_branch}"))
            && upstream_ref.ends_with(&format!("/{branch}")))
}

fn network(cmd: Cmd) -> Cmd {
    cmd.env("GCM_INTERACTIVE", "never")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env("SSH_ASKPASS_REQUIRE", "never")
}

/// The branch's upstream as (remote, branch on the remote), from `branch.<b>.remote/merge`.
fn upstream_of(cwd: &Path, branch: &str) -> anyhow::Result<Option<(String, String)>> {
    let remote = config_get(cwd, &format!("branch.{branch}.remote"))?;
    let merge = config_get(cwd, &format!("branch.{branch}.merge"))?;
    let resolves = git(cwd)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("{branch}@{{upstream}}"))
        .ok()?;
    Ok(match (remote, merge) {
        (Some(r), Some(m)) if resolves && r != "." => m
            .strip_prefix("refs/heads/")
            .map(|b| (r.clone(), b.to_string())),
        _ => None,
    })
}

fn push_remote_for(cwd: &Path, branch: &str) -> anyhow::Result<Option<String>> {
    if let Some(r) = config_get(cwd, &format!("branch.{branch}.pushRemote"))? {
        return Ok(Some(r));
    }
    if let Some(r) = config_get(cwd, "remote.pushDefault")? {
        return Ok(Some(r));
    }
    Ok(None)
}

/// Push the current branch (spec/05 §4b.9 "Push"). Never forces, never `-u`: when the branch
/// is published, its upstream is written by hand (if `record_upstream`).
pub fn push_branch(cwd: &Path, record_upstream: bool) -> anyhow::Result<PushOutcome> {
    let Some(branch) = current_branch(cwd)? else {
        bail!("Cannot push from detached HEAD.");
    };
    if remotes(cwd)?.is_empty() {
        bail!("Cannot push because no git remote is configured for this repository.");
    }
    let upstream = upstream_of(cwd, &branch)?;
    let explicit_push_remote = push_remote_for(cwd, &branch)?;

    if let Some((up_remote, up_branch)) = &upstream {
        let up_ref = format!("refs/remotes/{up_remote}/{up_branch}");
        if is_alias_of_upstream_head(&branch, up_branch, &up_ref) {
            let remote = explicit_push_remote.unwrap_or_else(|| up_remote.clone());
            if &remote == up_remote {
                let counts = git(cwd)
                    .args(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
                    .output()?;
                let ab: Vec<u32> = counts
                    .line()
                    .split_whitespace()
                    .filter_map(|x| x.parse().ok())
                    .collect();
                if counts.success() && ab.first() == Some(&0) && ab.get(1) == Some(&0) {
                    return Ok(PushOutcome::UpToDate);
                }
            }
            let out = network(git(cwd))
                .args(["push", "--", &remote])
                .arg(format!("HEAD:refs/heads/{up_branch}"))
                .output()?;
            if !out.success() {
                return Err(remote_error(&out.stderr));
            }
            return Ok(PushOutcome::Pushed {
                branch: branch.clone(),
                remote,
                upstream_branch: up_branch.clone(),
                set_upstream: false,
            });
        }
    }

    // Publish under the local name. A differently named upstream is a base, not a target.
    let remote = match explicit_push_remote {
        Some(r) => r,
        None => primary_remote(cwd)?
            .context("Cannot push because no git remote is configured for this repository.")?,
    };
    let publish = branch
        .strip_prefix(&format!("{remote}/"))
        .unwrap_or(&branch)
        .to_string();
    if let Some((up_remote, up_branch)) = &upstream
        && config_get(cwd, &branch_base_ref_config_key(&branch))?.is_none()
    {
        config_set(cwd, &branch_base_config_key(&branch), up_branch)?;
        config_set(
            cwd,
            &branch_base_ref_config_key(&branch),
            &format!("refs/remotes/{up_remote}/{up_branch}"),
        )?;
    }
    let out = network(git(cwd))
        .args(["push", "--", &remote])
        .arg(format!("HEAD:refs/heads/{publish}"))
        .output()?;
    if !out.success() {
        return Err(remote_error(&out.stderr));
    }
    if record_upstream {
        config_set(cwd, &format!("branch.{branch}.remote"), &remote)?;
        config_set(
            cwd,
            &format!("branch.{branch}.merge"),
            &format!("refs/heads/{publish}"),
        )?;
        let auto_rebase = config_get(cwd, "branch.autoSetupRebase")?;
        if matches!(auto_rebase.as_deref(), Some("always") | Some("remote")) {
            config_set(cwd, &format!("branch.{branch}.rebase"), "true")?;
        }
    }
    Ok(PushOutcome::Pushed {
        branch,
        remote,
        upstream_branch: publish,
        set_upstream: record_upstream,
    })
}

pub fn push(cwd: &Path, set_upstream: bool) -> anyhow::Result<()> {
    push_branch(cwd, set_upstream).map(|_| ())
}

/// `git pull --ff-only`.
pub fn pull(cwd: &Path) -> anyhow::Result<()> {
    let Some(branch) = current_branch(cwd)? else {
        bail!("Cannot pull from detached HEAD.");
    };
    if upstream_of(cwd, &branch)?.is_none() {
        bail!("Current branch has no upstream configured. Push with upstream first.");
    }
    let out = network(git(cwd)).args(["pull", "--ff-only"]).output()?;
    if !out.success() {
        return Err(remote_error(&out.stderr));
    }
    Ok(())
}

/// Is the GitHub CLI installed?
pub fn gh_available() -> bool {
    gh(Path::new("."))
        .arg("--version")
        .timeout(Duration::from_secs(10))
        .ok()
        .unwrap_or(false)
}

fn parse_pr(v: &Value) -> Option<PullRequestInfo> {
    let s = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Some(PullRequestInfo {
        number: v.get("number")?.as_u64()?,
        url: s("url"),
        title: s("title"),
        state: s("state").to_lowercase(),
        base: s("baseRefName"),
        head: s("headRefName"),
    })
}

fn gh_json(cmd: Cmd) -> anyhow::Result<Value> {
    let out = cmd.checked()?;
    serde_json::from_slice(&out.stdout).context("gh returned invalid JSON")
}

fn pr_list(cwd: &Path, head: &str, state: &str) -> anyhow::Result<Option<PullRequestInfo>> {
    let v = gh_json(gh(cwd).args([
        "pr", "list", "--head", head, "--state", state, "--limit", "1", "--json", PR_FIELDS,
    ]))?;
    Ok(v.as_array().and_then(|a| a.first()).and_then(parse_pr))
}

/// The open PR whose head is `branch`.
pub fn open_pull_request(cwd: &Path, branch: &str) -> anyhow::Result<Option<PullRequestInfo>> {
    pr_list(cwd, branch, "open")
}

/// Like `gh pr view <branch>`: the open PR, else the newest PR of any state.
pub fn pull_request_for_branch(
    cwd: &Path,
    branch: &str,
) -> anyhow::Result<Option<PullRequestInfo>> {
    if let Some(pr) = pr_list(cwd, branch, "open")? {
        return Ok(Some(pr));
    }
    pr_list(cwd, branch, "all")
}

/// A PR by number, URL or branch (`gh pr view <reference>`).
pub fn resolve_pull_request(cwd: &Path, reference: &str) -> anyhow::Result<PullRequestInfo> {
    let reference = reference.trim().trim_start_matches('#');
    let v = gh_json(gh(cwd).args(["pr", "view", reference, "--json", PR_FIELDS]))?;
    parse_pr(&v).context("gh returned no pull request")
}

fn strip_remote_prefix(cwd: &Path, name: &str) -> anyhow::Result<String> {
    let name = name
        .trim_start_matches("refs/remotes/")
        .trim_start_matches("refs/heads/");
    let mut all = remotes(cwd)?;
    all.sort_by_key(|r| std::cmp::Reverse(r.len()));
    for r in all {
        if let Some(rest) = name.strip_prefix(&format!("{r}/")) {
            return Ok(rest.to_string());
        }
    }
    Ok(name.to_string())
}

/// PR base for `branch` (spec/05 §4b.10 step 5): the recorded base ref, the recorded base, the
/// upstream branch when it is a different branch of the same repo, GitHub's default branch,
/// `refs/remotes/<primary>/HEAD`, then `main`.
pub fn resolve_pr_base(cwd: &Path, branch: &str) -> anyhow::Result<String> {
    if let Some(r) = config_get(cwd, &branch_base_ref_config_key(branch))? {
        return strip_remote_prefix(cwd, &r);
    }
    if let Some(b) = read_branch_base(cwd, branch)? {
        return strip_remote_prefix(cwd, &b);
    }
    let primary = primary_remote(cwd)?;
    if let Some((remote, up)) = upstream_of(cwd, branch)?
        && Some(&remote) == primary.as_ref()
        && up != branch
    {
        return Ok(up);
    }
    if let Ok(v) = gh_json(gh(cwd).args(["repo", "view", "--json", "defaultBranchRef"]))
        && let Some(name) = v
            .get("defaultBranchRef")
            .and_then(|d| d.get("name"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    {
        return Ok(name.to_string());
    }
    if let Some(r) = &primary
        && let Some(d) = remote_default_branch(cwd, r)?
    {
        return Ok(d);
    }
    Ok("main".into())
}

/// Open a PR for the current branch through `gh pr create` (spec/05 §4b.10). `base` empty =
/// [`resolve_pr_base`]. An already-open PR for the branch is returned as is; check
/// [`open_pull_request`] first to tell the two apart.
pub fn create_pull_request(
    cwd: &Path,
    base: &str,
    title: &str,
    body: &str,
    draft: bool,
) -> anyhow::Result<PullRequestInfo> {
    let Some(branch) = current_branch(cwd)? else {
        bail!("Cannot create a pull request from detached HEAD.");
    };
    let Some((up_remote, head)) = upstream_of(cwd, &branch)? else {
        bail!("Current branch has not been pushed. Push before creating a PR.");
    };
    if let Some(pr) = open_pull_request(cwd, &head)? {
        return Ok(pr);
    }
    let separate_push_remote = push_remote_for(cwd, &branch)?.is_some_and(|r| r != up_remote);
    if !separate_push_remote {
        let ahead = rev_count(cwd, "@{upstream}..HEAD")?;
        if ahead > 0 {
            let s = if ahead == 1 { "" } else { "s" };
            bail!("The branch has {ahead} unpushed commit{s}. Push before opening the PR.");
        }
    }
    let base = if base.trim().is_empty() {
        resolve_pr_base(cwd, &branch)?
    } else {
        base.trim().to_string()
    };
    if base == head {
        bail!("{base} is the base branch. Check out a feature branch to create a PR.");
    }

    let body_file = BodyFile::write(body)?;
    let title = {
        let t = title.lines().next().unwrap_or_default().trim();
        if t.is_empty() {
            "Update project changes"
        } else {
            t
        }
    };
    let mut cmd = gh(cwd)
        .args([
            "pr", "create", "--base", &base, "--head", &head, "--title", title,
        ])
        .arg("--body-file")
        .arg(&body_file.0);
    if draft {
        cmd = cmd.arg("--draft");
    }
    let out = cmd.checked()?;
    let url = out
        .text()
        .lines()
        .rev()
        .find(|l| l.starts_with("http"))
        .unwrap_or_default()
        .trim()
        .to_string();
    if let Ok(pr) = resolve_pull_request(cwd, if url.is_empty() { &head } else { &url }) {
        return Ok(pr);
    }
    let number = url
        .rsplit('/')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    Ok(PullRequestInfo {
        number,
        url,
        title: title.to_string(),
        state: "open".into(),
        base,
        head,
    })
}

struct BodyFile(PathBuf);
impl BodyFile {
    fn write(body: &str) -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "tern-pr-body-{}-{}.md",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, body.trim())?;
        Ok(BodyFile(path))
    }
}
impl Drop for BodyFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut out = truncate_utf8(s, max).to_string();
    out.push_str(CLIP_MARKER);
    out
}

/// What the commit-message prompt sees (spec/05 §4b.11 "Commit context").
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CommitContext {
    /// `git diff --cached --name-status`, clipped to 8,000.
    pub staged_summary: String,
    /// `git diff --cached --patch --minimal`, clipped to 50,000.
    pub staged_patch: String,
}

/// Stage into a throwaway copy of the index and describe what a commit would contain, without
/// touching the user's staging. `None` when there is nothing to commit.
pub fn commit_context(
    cwd: &Path,
    paths: Option<&[String]>,
) -> anyhow::Result<Option<CommitContext>> {
    let real = git(cwd)
        .args(["rev-parse", "--path-format=absolute", "--git-path", "index"])
        .run()?;
    let real = PathBuf::from(real.trim_end());
    let dir = tempfile_dir()?;
    let index = dir.0.join("index");
    if real.exists() {
        std::fs::copy(&real, &index)?;
    }
    let with_index = |c: Cmd| c.env("GIT_INDEX_FILE", &index);
    match paths {
        None => {
            with_index(git(cwd)).args(["add", "-A"]).run()?;
        }
        Some(paths) => {
            if head_commit(cwd)?.is_some() {
                with_index(git(cwd)).args(["reset", "--quiet"]).run()?;
            }
            with_index(git(cwd))
                .args(["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"])
                .stdin(literal_pathspec_stdin(paths))
                .run()?;
        }
    }
    let summary = with_index(git(cwd))
        .args([
            "-c",
            "diff.relative=false",
            "diff",
            "--cached",
            "--name-status",
        ])
        .run()?;
    if summary.trim().is_empty() && in_progress_operation(cwd)? != Some("merge") {
        return Ok(None);
    }
    let patch = with_index(git(cwd))
        .args([
            "-c",
            "diff.relative=false",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--cached",
            "--patch",
            "--minimal",
        ])
        .run()?;
    Ok(Some(CommitContext {
        staged_summary: clip(&summary, 8_000),
        staged_patch: clip(&patch, 50_000),
    }))
}

struct TempDir(PathBuf);
fn tempfile_dir() -> anyhow::Result<TempDir> {
    let p = std::env::temp_dir().join(format!(
        "tern-commit-index-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&p)?;
    Ok(TempDir(p))
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What the PR prompt sees (spec/05 §4b.11 "PR context").
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RangeContext {
    pub commit_summary: String,
    pub diff_summary: String,
    pub diff_patch: String,
}

/// `<base>..HEAD` commits, `<base>...HEAD` stat and patch. `base` should be the base's
/// remote-tracking ref or commit.
pub fn range_context(cwd: &Path, base: &str) -> anyhow::Result<RangeContext> {
    let commits = git(cwd)
        .args([
            "log",
            "--oneline",
            "--no-color",
            "--no-decorate",
            "--no-show-signature",
            "--end-of-options",
        ])
        .arg(format!("{base}..HEAD"))
        .run()?;
    let stat = git(cwd)
        .args([
            "-c",
            "diff.relative=false",
            "diff",
            "--stat",
            "--no-color",
            "--end-of-options",
        ])
        .arg(format!("{base}...HEAD"))
        .run()?;
    let patch = git(cwd)
        .args([
            "-c",
            "diff.relative=false",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--patch",
            "--minimal",
            "--end-of-options",
        ])
        .arg(format!("{base}...HEAD"))
        .run()?;
    Ok(RangeContext {
        commit_summary: clip(&commits, 20_000),
        diff_summary: clip(&stat, 20_000),
        diff_patch: clip(&patch, 60_000),
    })
}

/// The last 20 non-merge commit subjects (the `repo_conventions` writing style examples).
pub fn recent_commit_subjects(cwd: &Path) -> anyhow::Result<Vec<String>> {
    if head_commit(cwd)?.is_none() {
        return Ok(Vec::new());
    }
    Ok(git(cwd)
        .args([
            "log",
            "-n",
            "20",
            "--no-merges",
            "--no-show-signature",
            "--pretty=format:%s",
        ])
        .run()?
        .lines()
        .map(str::to_string)
        .collect())
}

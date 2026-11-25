//! Branches, status, checkout, fetch (spec/05 §4a.12-4a.14, §4b.9).
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};

use crate::cmd::{Cmd, git, split_nul};
use crate::diff::parse_numstat;

/// Git's empty tree, for diffs against an unborn HEAD.
pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
pub const DEFAULT_BASE_BRANCH_CANDIDATES: &[&str] = &["main", "master"];
const FETCH_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const RENAME_TIMEOUT: Duration = Duration::from_secs(10);

pub const AUTH_ERROR: &str = "Git could not authenticate with the remote. Check Git credentials or SSH access on the server, then retry.";
pub const NETWORK_ERROR: &str = "Git could not reach the remote. Check the server's network connection and remote host, then retry.";

/// `branch.<b>.tern-merge-base`: the branch, named as on the host, a branch was cut from.
pub fn branch_base_config_key(branch: &str) -> String {
    format!("branch.{branch}.tern-merge-base")
}
/// `branch.<b>.tern-merge-base-ref`: the full remote-tracking ref of that base.
pub fn branch_base_ref_config_key(branch: &str) -> String {
    format!("branch.{branch}.tern-merge-base-ref")
}
/// The GitHub CLI's key, still read for branches cut before Tern had its own.
fn legacy_branch_base_config_key(branch: &str) -> String {
    format!("branch.{branch}.gh-merge-base")
}

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

pub(crate) fn config_get(cwd: &Path, key: &str) -> anyhow::Result<Option<String>> {
    let out = git(cwd).args(["config", "--get", key]).output()?;
    Ok(out.success().then(|| out.line()).filter(|s| !s.is_empty()))
}

pub(crate) fn config_set(cwd: &Path, key: &str, value: &str) -> anyhow::Result<()> {
    git(cwd).args(["config", key, value]).run()?;
    Ok(())
}

/// Current branch, None when detached or not a repo.
pub fn current_branch(cwd: &Path) -> anyhow::Result<Option<String>> {
    let out = git(cwd).args(["branch", "--show-current"]).output()?;
    Ok(out.success().then(|| out.line()).filter(|s| !s.is_empty()))
}

/// Remote names in `git remote` order.
pub fn remotes(cwd: &Path) -> anyhow::Result<Vec<String>> {
    Ok(git(cwd)
        .arg("remote")
        .run()?
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// `origin` if it exists, else the first remote.
pub fn primary_remote(cwd: &Path) -> anyhow::Result<Option<String>> {
    let all = remotes(cwd)?;
    if all.iter().any(|r| r == "origin") {
        return Ok(Some("origin".into()));
    }
    Ok(all.into_iter().next())
}

/// The default branch of `remote` from `refs/remotes/<remote>/HEAD`.
pub fn remote_default_branch(cwd: &Path, remote: &str) -> anyhow::Result<Option<String>> {
    let out = git(cwd)
        .args(["rev-parse", "--verify", "--quiet", "--symbolic-full-name"])
        .arg(format!("refs/remotes/{remote}/HEAD"))
        .output()?;
    let prefix = format!("refs/remotes/{remote}/");
    Ok(out
        .success()
        .then(|| out.line())
        .and_then(|s| s.strip_prefix(&prefix).map(str::to_string))
        .filter(|s| !s.is_empty() && s != "HEAD"))
}

/// The primary remote's default branch name (`main`), if known.
pub fn default_branch(cwd: &Path) -> anyhow::Result<Option<String>> {
    match primary_remote(cwd)? {
        Some(remote) => remote_default_branch(cwd, &remote),
        None => Ok(None),
    }
}

/// `branch` is the default branch, or none is known and it is `main`/`master`.
pub fn is_default_branch(cwd: &Path, branch: &str) -> anyhow::Result<bool> {
    Ok(match default_branch(cwd)? {
        Some(d) => d == branch,
        None => DEFAULT_BASE_BRANCH_CANDIDATES.contains(&branch),
    })
}

pub fn branch_exists(cwd: &Path, branch: &str) -> anyhow::Result<bool> {
    git(cwd)
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .ok()
}

pub fn remote_branch_exists(cwd: &Path, remote: &str, branch: &str) -> anyhow::Result<bool> {
    git(cwd)
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/remotes/{remote}/{branch}"))
        .ok()
}

pub(crate) fn head_commit(cwd: &Path) -> anyhow::Result<Option<String>> {
    let out = git(cwd)
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .output()?;
    Ok(out.success().then(|| out.line()).filter(|s| !s.is_empty()))
}

pub(crate) fn git_common_dir(cwd: &Path) -> anyhow::Result<PathBuf> {
    Ok(PathBuf::from(
        git(cwd)
            .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .run()?
            .trim_end(),
    ))
}

pub(crate) fn show_toplevel(cwd: &Path) -> anyhow::Result<Option<String>> {
    let out = git(cwd).args(["rev-parse", "--show-toplevel"]).output()?;
    Ok(out.success().then(|| out.line()).filter(|s| !s.is_empty()))
}

/// `git branch --list --no-column --format=%(refname:lstrip=2)`.
pub fn local_branch_names(cwd: &Path) -> anyhow::Result<Vec<String>> {
    Ok(git(cwd)
        .args([
            "branch",
            "--list",
            "--no-column",
            "--format=%(refname:lstrip=2)",
        ])
        .run()?
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// branch -> worktree path, from `git worktree list --porcelain -z`, prunable entries skipped.
pub fn worktree_map(cwd: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let out = git(cwd)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()?;
    if !out.success() {
        // Fall back to the line format.
        let text = git(cwd)
            .args(["worktree", "list", "--porcelain"])
            .run()?
            .replace('\n', "\0");
        return Ok(parse_worktrees(text.as_bytes()));
    }
    Ok(parse_worktrees(&out.stdout))
}

fn parse_worktrees(bytes: &[u8]) -> Vec<(String, String)> {
    let mut map = Vec::new();
    let mut path: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut prunable = false;
    let mut flush =
        |path: &mut Option<String>, branch: &mut Option<String>, prunable: &mut bool| {
            if let (Some(p), Some(b)) = (path.take(), branch.take())
                && !*prunable
            {
                map.push((b, p));
            }
            *path = None;
            *branch = None;
            *prunable = false;
        };
    for field in bytes.split(|b| *b == 0) {
        let field = String::from_utf8_lossy(field);
        if field.is_empty() {
            flush(&mut path, &mut branch, &mut prunable);
        } else if let Some(p) = field.strip_prefix("worktree ") {
            flush(&mut path, &mut branch, &mut prunable);
            path = Some(p.to_string());
        } else if let Some(b) = field.strip_prefix("branch refs/heads/") {
            branch = Some(b.to_string());
        } else if field.starts_with("prunable") {
            prunable = true;
        }
    }
    flush(&mut path, &mut branch, &mut prunable);
    map
}

/// Every local and remote branch, ordered the way the branch picker shows them: current first,
/// then the default, then locals by recency, then remotes by recency. `origin/x` is dropped
/// when a local `x` exists (spec/05 §4a.12).
pub fn list_branches(cwd: &Path) -> anyhow::Result<Vec<Branch>> {
    list_branches_with(cwd, false)
}

/// [`list_branches`], optionally keeping `origin/x` refs that have a local match.
pub fn list_branches_with(
    cwd: &Path,
    include_matching_remote_refs: bool,
) -> anyhow::Result<Vec<Branch>> {
    let snapshot = git(cwd)
        .args([
            "for-each-ref",
            "--format=%(refname)%09%(committerdate:unix)%09%(symref)%09%(upstream)",
            "refs/heads",
            "refs/remotes",
        ])
        .run()?;
    let mut remote_names = remotes(cwd)?;
    remote_names.sort_by_key(|r| std::cmp::Reverse(r.len()));
    let primary = if remote_names.iter().any(|r| r == "origin") {
        Some("origin".to_string())
    } else {
        remotes(cwd)?.into_iter().next()
    };
    let default = match &primary {
        Some(r) => remote_default_branch(cwd, r)?,
        None => None,
    };
    let worktrees = worktree_map(cwd)?;

    let mut locals = Vec::new();
    let mut remote_refs: Vec<(Branch, Option<String>)> = Vec::new();
    for line in snapshot.lines() {
        let mut parts = line.split('\t');
        let (Some(refname), Some(date), Some(symref)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if !symref.is_empty() {
            continue;
        }
        let last_commit_at = date.parse::<i64>().ok();
        if let Some(name) = refname.strip_prefix("refs/heads/") {
            locals.push(Branch {
                name: name.to_string(),
                is_remote: false,
                is_current: false,
                is_default: default.as_deref() == Some(name),
                worktree_path: worktrees
                    .iter()
                    .find(|(b, _)| b == name)
                    .map(|(_, p)| p.clone()),
                last_commit_at,
            });
        } else if let Some(name) = refname.strip_prefix("refs/remotes/") {
            let remote = remote_names
                .iter()
                .find(|r| name.starts_with(&format!("{r}/")))
                .cloned();
            let short = remote
                .as_ref()
                .map(|r| &name[r.len() + 1..])
                .unwrap_or(name);
            let is_default =
                remote.is_some() && remote == primary && default.as_deref() == Some(short);
            remote_refs.push((
                Branch {
                    name: name.to_string(),
                    is_remote: true,
                    is_current: false,
                    is_default,
                    worktree_path: None,
                    last_commit_at,
                },
                remote,
            ));
        }
    }
    let by_recency = |a: &Branch, b: &Branch| {
        b.last_commit_at
            .cmp(&a.last_commit_at)
            .then_with(|| a.name.cmp(&b.name))
    };
    locals.sort_by(by_recency);
    remote_refs.sort_by(|a, b| by_recency(&a.0, &b.0));

    let current = current_branch(cwd)?;
    let root = show_toplevel(cwd)?;
    if let Some(cur) = &current
        && !locals.iter().any(|b| &b.name == cur)
        && branch_exists(cwd, cur)?
    {
        locals.insert(
            0,
            Branch {
                name: cur.clone(),
                is_remote: false,
                is_current: true,
                is_default: default.as_deref() == Some(cur),
                worktree_path: root.clone(),
                last_commit_at: None,
            },
        );
    }
    for b in &mut locals {
        b.is_current = current.as_deref() == Some(b.name.as_str());
        if let Some(root) = &root {
            if b.is_current {
                b.worktree_path = Some(root.clone());
            } else if b.worktree_path.as_ref() == Some(root) {
                b.worktree_path = None;
            }
        }
    }

    let local_names: HashSet<String> = locals.iter().map(|b| b.name.clone()).collect();
    let mut all = locals;
    for (b, remote) in remote_refs {
        if !include_matching_remote_refs
            && remote.as_deref() == Some("origin")
            && local_names.contains(&b.name["origin/".len()..])
        {
            continue;
        }
        all.push(b);
    }
    // Stable: current, default, then snapshot order.
    all.sort_by_key(|b| {
        if b.is_current {
            0
        } else if b.is_default {
            1
        } else {
            2
        }
    });
    Ok(all)
}

/// Local working-tree status plus upstream counts (spec/05 §4b.9 "Local status").
pub fn status(cwd: &Path) -> anyhow::Result<RepoStatus> {
    let facts = git(cwd)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "index",
            "--git-common-dir",
            "--show-toplevel",
        ])
        .run()
        .context("not a git repository")?;
    let index = facts.lines().next().unwrap_or_default();
    if Path::new(&format!("{index}.lock")).exists() {
        bail!("Git index is locked. Status will resume when the index lock is removed.");
    }

    let porcelain = git(cwd)
        .args([
            "status",
            "--porcelain=2",
            "--branch",
            "-z",
            "--untracked-files=normal",
        ])
        .checked()?;
    let head = head_commit(cwd)?;
    let numstat = git(cwd)
        .args(["diff-index", "-M", "--numstat", "-z"])
        .arg(head.as_deref().unwrap_or(EMPTY_TREE))
        .arg("--")
        .checked()?;

    let mut st = RepoStatus::default();
    let mut saw_ab = false;
    let mut dirty: HashSet<String> = HashSet::new();
    let records = split_nul(&porcelain.stdout);
    let mut i = 0;
    while i < records.len() {
        let rec = &records[i];
        i += 1;
        if let Some(h) = rec.strip_prefix("# branch.head ") {
            if h != "(detached)" && h != "(unknown)" {
                st.branch = Some(h.to_string());
            }
        } else if let Some(u) = rec.strip_prefix("# branch.upstream ") {
            st.upstream = Some(u.to_string());
        } else if let Some(ab) = rec.strip_prefix("# branch.ab ") {
            saw_ab = true;
            for part in ab.split(' ') {
                if let Some(a) = part.strip_prefix('+') {
                    st.ahead = a.parse().unwrap_or(0);
                } else if let Some(b) = part.strip_prefix('-') {
                    st.behind = b.parse().unwrap_or(0);
                }
            }
        } else if rec.starts_with('#') {
        } else if rec.starts_with("2 ") {
            // Rename/copy: path is the record's last field, the original path comes next.
            dirty.insert(field_path(rec, 9));
            if let Some(orig) = records.get(i) {
                dirty.insert(orig.clone());
            }
            i += 1;
        } else if rec.starts_with("1 ") {
            dirty.insert(field_path(rec, 8));
        } else if rec.starts_with("u ") {
            dirty.insert(field_path(rec, 10));
        } else if let Some(p) = rec.strip_prefix("? ").or_else(|| rec.strip_prefix("! ")) {
            dirty.insert(p.to_string());
        }
    }
    // `# branch.ab` missing means the upstream is gone.
    if st.upstream.is_some() && !saw_ab {
        st.upstream = None;
    }
    for f in parse_numstat(&numstat.stdout) {
        st.additions += f.additions;
        st.deletions += f.deletions;
    }
    st.changed_files = dirty.len() as u32;
    st.has_changes = !dirty.is_empty();

    let primary = primary_remote(cwd)?;
    st.has_origin = primary.is_some();
    st.default_branch = match &primary {
        Some(r) => remote_default_branch(cwd, r)?,
        None => None,
    };

    if st.upstream.is_none()
        && head.is_some()
        && let Some(branch) = &st.branch
    {
        st.behind = 0;
        st.ahead = match resolve_base_branch(cwd, branch, false)? {
            Some(base) => rev_count(cwd, &format!("{}..HEAD", base.comparison_ref))?,
            None => 0,
        };
    }
    Ok(st)
}

/// The path of a porcelain v2 record: everything after the first `n` space-separated fields.
fn field_path(rec: &str, n: usize) -> String {
    rec.splitn(n + 1, ' ')
        .nth(n)
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn rev_count(cwd: &Path, range: &str) -> anyhow::Result<u32> {
    let out = git(cwd)
        .args(["rev-list", "--count", "--end-of-options", range])
        .output()?;
    Ok(if out.success() {
        out.line().parse().unwrap_or(0)
    } else {
        0
    })
}

/// A base branch: `base_ref` for display (`origin/main`), `comparison_ref` to diff against
/// (`refs/remotes/origin/main`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseRef {
    pub base_ref: String,
    pub comparison_ref: String,
}

/// Tern's recorded base for `branch` (`tern-merge-base`, then the legacy `gh-merge-base`).
pub fn read_branch_base(cwd: &Path, branch: &str) -> anyhow::Result<Option<String>> {
    match config_get(cwd, &branch_base_config_key(branch))? {
        Some(v) => Ok(Some(v)),
        None => config_get(cwd, &legacy_branch_base_config_key(branch)),
    }
}

fn comparison_base_ref(cwd: &Path, base_ref: &str) -> anyhow::Result<Option<BaseRef>> {
    let comparison = if base_ref.starts_with("refs/heads/") || base_ref.starts_with("refs/remotes/")
    {
        base_ref.to_string()
    } else if base_ref.contains('/') && !base_ref.starts_with("refs/") {
        format!("refs/remotes/{base_ref}")
    } else {
        return Ok(None);
    };
    let exists = git(cwd)
        .args(["show-ref", "--verify", "--quiet", &comparison])
        .ok()?;
    Ok(exists.then(|| BaseRef {
        base_ref: base_ref.to_string(),
        comparison_ref: comparison,
    }))
}

fn tracked_remote_base(cwd: &Path, branch: &str) -> anyhow::Result<Option<BaseRef>> {
    let out = git(cwd)
        .args(["rev-parse", "--symbolic-full-name"])
        .arg(format!("{branch}@{{upstream}}"))
        .output()?;
    let full = out.line();
    Ok(match full.strip_prefix("refs/remotes/") {
        Some(short) if out.success() => Some(BaseRef {
            base_ref: short.to_string(),
            comparison_ref: full.clone(),
        }),
        _ => None,
    })
}

/// Which branch `branch` was cut from (`findBaseBranchForNoUpstream`, spec/05 §4b.9):
/// `tern-merge-base-ref`, then the recorded base, then the primary remote's default branch,
/// then `main`, `master`; each tried as a remote-tracking ref first, then a local branch.
/// With `allow_remote_of_current`, a candidate equal to `branch` itself resolves to its
/// upstream (so `main` diffs against `origin/main`).
pub fn resolve_base_branch(
    cwd: &Path,
    branch: &str,
    allow_remote_of_current: bool,
) -> anyhow::Result<Option<BaseRef>> {
    if let Some(cfg) = config_get(cwd, &branch_base_ref_config_key(branch))?
        && (cfg.starts_with("refs/heads/") || cfg.starts_with("refs/remotes/"))
        && let Some(base) = comparison_base_ref(cwd, &cfg)?
    {
        let short = cfg.trim_start_matches("refs/remotes/");
        let base_ref = if branch_exists(cwd, short)? {
            cfg.clone()
        } else {
            short.to_string()
        };
        return Ok(Some(BaseRef { base_ref, ..base }));
    }

    let configured = read_branch_base(cwd, branch)?.unwrap_or_default();
    let configured_base = comparison_base_ref(cwd, &configured)?;
    if configured_base.is_some()
        && configured.contains('/')
        && !configured.starts_with("refs/")
        && branch_exists(cwd, &configured)?
    {
        let r = format!("refs/heads/{configured}");
        return Ok(Some(BaseRef {
            base_ref: r.clone(),
            comparison_ref: r,
        }));
    }
    if configured_base.is_some() {
        return Ok(configured_base);
    }
    if !configured.is_empty()
        && configured != branch
        && let Some(tracked) = tracked_remote_base(cwd, &configured)?
    {
        return Ok(Some(tracked));
    }

    let primary = primary_remote(cwd)?;
    let mut candidates = vec![configured];
    if let Some(r) = &primary {
        candidates.push(remote_default_branch(cwd, r)?.unwrap_or_default());
    }
    candidates.extend(DEFAULT_BASE_BRANCH_CANDIDATES.iter().map(|s| s.to_string()));

    let remote_prefix = primary
        .as_ref()
        .filter(|r| *r != "origin")
        .map(|r| format!("{r}/"));
    for candidate in candidates {
        if candidate.is_empty() {
            continue;
        }
        let normalized = if let Some(s) = candidate.strip_prefix("origin/") {
            s.to_string()
        } else if let Some(s) = remote_prefix
            .as_ref()
            .and_then(|p| candidate.strip_prefix(p.as_str()))
        {
            s.to_string()
        } else {
            candidate.clone()
        };
        if normalized.is_empty() {
            continue;
        }
        let remote_hit = |name: &str| -> anyhow::Result<Option<BaseRef>> {
            Ok(match &primary {
                Some(r) if remote_branch_exists(cwd, r, name)? => Some(BaseRef {
                    base_ref: format!("{r}/{name}"),
                    comparison_ref: format!("refs/remotes/{r}/{name}"),
                }),
                _ => None,
            })
        };
        if normalized == branch {
            if allow_remote_of_current {
                if let Some(tracked) = tracked_remote_base(cwd, branch)? {
                    return Ok(Some(tracked));
                }
                if let Some(hit) = remote_hit(&normalized)? {
                    return Ok(Some(hit));
                }
            }
            continue;
        }
        if let Some(hit) = remote_hit(&normalized)? {
            return Ok(Some(hit));
        }
        if branch_exists(cwd, &normalized)? {
            return Ok(Some(BaseRef {
                base_ref: normalized.clone(),
                comparison_ref: format!("refs/heads/{normalized}"),
            }));
        }
    }
    Ok(None)
}

fn refuse_dash(name: &str) -> anyhow::Result<()> {
    if name.starts_with('-') {
        bail!("Invalid ref name \"{name}\".");
    }
    Ok(())
}

/// Longest known remote that prefixes `remote_ref` (`origin/feature/x` -> origin).
fn remote_of(cwd: &Path, remote_ref: &str) -> anyhow::Result<Option<String>> {
    let mut all = remotes(cwd)?;
    all.sort_by_key(|r| std::cmp::Reverse(r.len()));
    Ok(all
        .into_iter()
        .find(|r| remote_ref.starts_with(&format!("{r}/"))))
}

/// Switch to a local or remote branch (`switchRef`, spec/05 §4a.14). A remote-only ref gets a
/// local tracking branch. Returns the branch checked out afterwards.
pub fn checkout(cwd: &Path, branch: &str) -> anyhow::Result<()> {
    switch_ref(cwd, branch).map(|_| ())
}

/// [`checkout`] returning `git branch --show-current` afterwards.
pub fn switch_ref(cwd: &Path, refname: &str) -> anyhow::Result<Option<String>> {
    refuse_dash(refname)?;
    let local = branch_exists(cwd, refname)?;
    let remote = !local
        && git(cwd)
            .args(["show-ref", "--verify", "--quiet"])
            .arg(format!("refs/remotes/{refname}"))
            .ok()?;

    let args: Vec<String> = if local || !remote {
        vec!["checkout".into(), refname.into()]
    } else {
        let remote_full = format!("refs/remotes/{refname}");
        let trackers = git(cwd)
            .args([
                "for-each-ref",
                "--format=%(refname:lstrip=2)%09%(upstream)",
                "refs/heads",
            ])
            .run()?;
        let tracker = trackers.lines().find_map(|l| {
            let (name, up) = l.split_once('\t')?;
            (up == remote_full).then(|| name.to_string())
        });
        match tracker {
            Some(t) => vec!["checkout".into(), t],
            None => {
                let local_name = match remote_of(cwd, refname)? {
                    Some(r) => refname[r.len() + 1..].to_string(),
                    None => refname
                        .split_once('/')
                        .map(|(_, b)| b.to_string())
                        .unwrap_or_else(|| refname.to_string()),
                };
                refuse_dash(&local_name)?;
                if branch_exists(cwd, &local_name)? {
                    vec![
                        "checkout".into(),
                        "--track".into(),
                        "-b".into(),
                        local_name,
                        remote_full,
                    ]
                } else {
                    git(cwd)
                        .args(["branch", "--no-track", "--", &local_name, &remote_full])
                        .run()?;
                    let set = git(cwd)
                        .arg("branch")
                        .arg(format!("--set-upstream-to={remote_full}"))
                        .args(["--", &local_name])
                        .output()?;
                    if !set.success() {
                        let _ = git(cwd)
                            .args([
                                "update-ref",
                                "-d",
                                &format!("refs/heads/{local_name}"),
                                &remote_full,
                            ])
                            .output();
                        bail!("Could not track {refname}: {}", set.stderr.trim());
                    }
                    vec!["checkout".into(), local_name]
                }
            }
        }
    };
    // Always end with `--` so a stale name can't become a path checkout.
    git(cwd).args(&args).arg("--").run()?;
    current_branch(cwd)
}

/// Create a branch. With `base`, `createBranchAt` (`--no-track`, upstream set when the start
/// is a remote-tracking ref); without, `git branch -- <name>` at HEAD. Doesn't switch.
pub fn create_branch(cwd: &Path, name: &str, base: Option<&str>) -> anyhow::Result<()> {
    refuse_dash(name)?;
    match base {
        None => {
            git(cwd)
                .args(["branch", "--", name])
                .timeout(Duration::from_secs(10))
                .run()?;
        }
        Some(start) => create_branch_at(cwd, name, start)?,
    }
    Ok(())
}

pub(crate) fn create_branch_at(cwd: &Path, name: &str, start: &str) -> anyhow::Result<()> {
    let start = if start == "-" { "@{-1}" } else { start };
    git(cwd)
        .args(["branch", "--no-track", "--", name, start])
        .run()?;
    let full = git(cwd)
        .args(["rev-parse", "--symbolic-full-name", start])
        .output()?;
    let full = full.line();
    if full.starts_with("refs/remotes/") {
        let set = git(cwd)
            .arg("branch")
            .arg(format!("--set-upstream-to={full}"))
            .args(["--", name])
            .output()?;
        if !set.success() {
            tracing::warn!(branch = name, upstream = %full, "could not set upstream");
        }
    }
    Ok(())
}

/// `git branch -m -- <old> <new>`.
pub fn rename_branch(cwd: &Path, old: &str, new: &str) -> anyhow::Result<()> {
    refuse_dash(new)?;
    git(cwd)
        .args(["branch", "-m", "--", old, new])
        .timeout(RENAME_TIMEOUT)
        .run()?;
    Ok(())
}

fn network_env(cmd: Cmd) -> Cmd {
    cmd.env("GCM_INTERACTIVE", "never")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .timeout(FETCH_TIMEOUT)
}

/// Map git's remote failure text onto T3's fixed error details.
pub(crate) fn remote_error(stderr: &str) -> anyhow::Error {
    let s = stderr.to_ascii_lowercase();
    let auth = [
        "authentication failed",
        "permission denied",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "invalid username or password",
        "403",
    ];
    let network = [
        "could not resolve host",
        "unable to access",
        "connection refused",
        "connection timed out",
        "network is unreachable",
        "operation timed out",
        "could not read from remote repository",
    ];
    if auth.iter().any(|p| s.contains(p)) {
        anyhow::anyhow!(AUTH_ERROR)
    } else if network.iter().any(|p| s.contains(p)) {
        anyhow::anyhow!(NETWORK_ERROR)
    } else {
        anyhow::anyhow!("{}", stderr.trim())
    }
}

/// Targeted fetch of one branch into its remote-tracking ref. Ok(false) when the remote has no
/// such branch. Retried once on a ref-lock failure.
pub fn fetch_branch(cwd: &Path, remote: &str, branch: &str) -> anyhow::Result<bool> {
    refuse_dash(remote)?;
    refuse_dash(branch)?;
    let refspec = format!("+refs/heads/{branch}:refs/remotes/{remote}/{branch}");
    let run = || {
        network_env(git(cwd))
            .args(["fetch", "--quiet", "--no-tags", "--", remote, &refspec])
            .output()
    };
    let mut out = run()?;
    if !out.success() && out.stderr.contains("cannot lock ref") {
        out = run()?;
    }
    if out.success() {
        return Ok(true);
    }
    if out
        .stderr
        .contains(&format!("couldn't find remote ref refs/heads/{branch}"))
    {
        return Ok(false);
    }
    Err(remote_error(&out.stderr))
}

/// Background upstream fetch: `fetch --quiet --no-tags --no-write-fetch-head -- <remote>`.
pub fn fetch_remote(cwd: &Path, remote: &str) -> anyhow::Result<()> {
    refuse_dash(remote)?;
    let out = network_env(git(cwd))
        .args([
            "fetch",
            "--quiet",
            "--no-tags",
            "--no-write-fetch-head",
            "--",
            remote,
        ])
        .output()?;
    if out.success() {
        Ok(())
    } else {
        Err(remote_error(&out.stderr))
    }
}

pub(crate) fn show_toplevel_path(cwd: &Path) -> Option<PathBuf> {
    show_toplevel(cwd).ok().flatten().map(PathBuf::from)
}

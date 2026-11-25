//! Worktrees under `<root>/<repo>/<branch-prefix>-<hash16>` (spec/05 §4a.15, §4a.17,
//! spec/07 §8.2-8.4). The app passes `~/.tern/worktrees` as the root.
//!
//! Not ported: sparse patterns from a project file, the macOS APFS checkout cache, checkout
//! progress parsing, and setup scripts (those run in a terminal, elsewhere).
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};

use crate::cmd::git;
use crate::naming::{
    BranchNamingMode, format_generated_branch_name, resolve_available_branch_name,
};
use crate::refs::{
    branch_base_config_key, branch_base_ref_config_key, branch_exists, config_get, config_set,
    create_branch_at, current_branch, fetch_branch, git_common_dir, primary_remote,
    remote_default_branch, remotes, show_toplevel,
};
use crate::util::sha256_hex;

/// What [`provision`] made.
#[derive(Clone, Debug, PartialEq)]
pub struct CreatedWorktree {
    pub path: PathBuf,
    pub branch: String,
    /// The base actually used. Differs from the request when it didn't resolve and a fallback
    /// (the remote's default branch, the current branch, HEAD) was taken.
    pub base_ref: String,
    /// Commit or ref the branch was created at.
    pub start_point: String,
    /// `git submodule update` failed. Not fatal.
    pub submodule_warning: Option<String>,
}

pub fn create(
    repo: &Path,
    worktrees_root: &Path,
    base_ref: &str,
    branch: &str,
    start_from_origin: bool,
) -> anyhow::Result<PathBuf> {
    provision(repo, worktrees_root, base_ref, branch, start_from_origin).map(|w| w.path)
}

fn check_root(root: &Path) -> anyhow::Result<()> {
    let is_fs_root = root.parent().is_none();
    if !root.is_absolute() || is_fs_root {
        bail!(
            "The worktree location \"{}\" must be an absolute folder on this machine, not a drive root. Change it in Settings → Worktrees.",
            root.display()
        );
    }
    Ok(())
}

/// Revision syntax and names git would read as options are not a branch to fetch.
fn plain_branch_name(name: &str) -> bool {
    !name.is_empty()
        && name != "HEAD"
        && !name.starts_with('-')
        && !name.contains("..")
        && !name.contains("@{")
        && !name
            .chars()
            .any(|c| matches!(c, '^' | '~' | ':' | '?' | '*' | '[' | '\\' | ' ') || c.is_control())
}

/// Which remote branch a base names: its upstream, else `<remote>/<b>`, else `origin/<base>`.
fn remote_target(repo: &Path, base: &str) -> anyhow::Result<Option<(String, String)>> {
    let known = remotes(repo)?;
    let upstream = git(repo)
        .args([
            "for-each-ref",
            "--format=%(refname)%09%(upstream:remotename)%09%(upstream:remoteref)",
        ])
        .arg(format!("refs/heads/{base}"))
        .run()?;
    let target = upstream
        .lines()
        .find_map(|l| {
            let mut p = l.split('\t');
            let (_, remote, rref) = (p.next()?, p.next()?, p.next()?);
            let b = rref.strip_prefix("refs/heads/")?;
            known
                .iter()
                .any(|r| r == remote)
                .then(|| (remote.to_string(), b.to_string()))
        })
        .or_else(|| {
            let mut sorted = known.clone();
            sorted.sort_by_key(|r| std::cmp::Reverse(r.len()));
            sorted.iter().find_map(|r| {
                base.strip_prefix(&format!("{r}/"))
                    .map(|b| (r.clone(), b.to_string()))
            })
        })
        .unwrap_or_else(|| ("origin".to_string(), base.to_string()));
    if !known.contains(&target.0) || !plain_branch_name(&target.1) {
        return Ok(None);
    }
    Ok(Some(target))
}

fn resolves_to_commit(repo: &Path, rev: &str) -> anyhow::Result<Option<String>> {
    let rev = if rev == "-" { "@{-1}" } else { rev };
    let out = git(repo)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("{rev}^{{commit}}"))
        .output()?;
    Ok(out.success().then(|| out.line()).filter(|s| !s.is_empty()))
}

/// `<root>/<repo basename>/<branch, / -> -, <= 23 bytes>-<sha256(common dir \0 branch)[..16]>`,
/// with `-2`, `-3`, ... when taken.
pub fn worktree_path(repo: &Path, worktrees_root: &Path, branch: &str) -> anyhow::Result<PathBuf> {
    check_root(worktrees_root)?;
    let abs = if repo.is_absolute() {
        repo.to_path_buf()
    } else {
        std::env::current_dir()?.join(repo)
    };
    let repo_name = abs
        .file_name()
        .context("repository path has no folder name")?
        .to_string_lossy()
        .into_owned();
    let common = git_common_dir(repo)?;
    let flat = branch.replace('/', "-");
    let mut end = flat.len().min(23);
    while !flat.is_char_boundary(end) {
        end -= 1;
    }
    let identity = format!("{}\0{branch}", common.display());
    let hash = sha256_hex(identity.as_bytes());
    let base = worktrees_root
        .join(&repo_name)
        .join(format!("{}-{}", &flat[..end], &hash[..16]));
    if !base.exists() {
        return Ok(base);
    }
    for n in 2.. {
        let candidate = PathBuf::from(format!("{}-{n}", base.display()));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    unreachable!()
}

/// Create `branch` at `base_ref` (fetched from its remote first when `start_from_origin`) and
/// check it out in a new worktree (spec/05 §4a.15 steps 3-12).
pub fn provision(
    repo: &Path,
    worktrees_root: &Path,
    base_ref: &str,
    branch: &str,
    start_from_origin: bool,
) -> anyhow::Result<CreatedWorktree> {
    check_root(worktrees_root)?;
    if show_toplevel(repo)?.is_none() {
        bail!("{} is not a Git repository.", repo.display());
    }
    if branch.starts_with('-')
        || !git(repo)
            .args(["check-ref-format", "--branch", branch])
            .ok()?
    {
        bail!("\"{branch}\" is not a valid branch name.");
    }
    if branch_exists(repo, branch)? {
        bail!("Branch \"{branch}\" already exists.");
    }
    let lower = branch.to_lowercase();
    for existing in crate::refs::local_branch_names(repo)? {
        let e = existing.to_lowercase();
        if lower.starts_with(&format!("{e}/")) || e.starts_with(&format!("{lower}/")) {
            bail!("Branch \"{branch}\" cannot be created: branch \"{existing}\" exists.");
        }
    }

    // Fetch, then fall back (step 3-4).
    let mut start: Option<String> = None;
    let mut used_base = base_ref.to_string();
    let mut record_base: Option<String> = None;
    let mut record_base_ref: Option<String> = None;
    if start_from_origin && let Some((remote, rb)) = remote_target(repo, base_ref)? {
        match fetch_branch(repo, &remote, &rb) {
            Ok(true) => {
                let tracking = format!("refs/remotes/{remote}/{rb}");
                if let Some(sha) = resolves_to_commit(repo, &tracking)? {
                    start = Some(sha);
                    record_base = Some(rb);
                    record_base_ref = Some(tracking);
                }
            }
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, "worktree base fetch failed"),
        }
    }
    if start.is_none() && resolves_to_commit(repo, base_ref)?.is_some() {
        start = Some(if base_ref == "-" {
            "@{-1}".into()
        } else {
            base_ref.to_string()
        });
        record_base = Some(base_ref.to_string());
    }
    if start.is_none()
        && let Some(remote) = primary_remote(repo)?
        && let Some(default) = remote_default_branch(repo, &remote)?
    {
        let tracking = format!("refs/remotes/{remote}/{default}");
        if start_from_origin {
            let _ = fetch_branch(repo, &remote, &default);
        }
        if let Some(sha) = resolves_to_commit(repo, &tracking)? {
            start = Some(sha);
            used_base = format!("{remote}/{default}");
            record_base = Some(default);
            record_base_ref = Some(tracking);
        }
    }
    if start.is_none()
        && let Some(cur) = current_branch(repo)?
        && resolves_to_commit(repo, &cur)?.is_some()
    {
        used_base = cur.clone();
        record_base = Some(cur.clone());
        start = Some(cur);
    }
    if start.is_none() && resolves_to_commit(repo, "HEAD")?.is_some() {
        used_base = "HEAD".into();
        start = Some("HEAD".into());
    }
    let Some(start) = start else {
        bail!("Base ref \"{base_ref}\" does not resolve to a commit.");
    };

    let path = worktree_path(repo, worktrees_root, branch)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // `--reason` arrived in git 2.17; probe instead of parsing versions.
    let help = git(repo).args(["worktree", "add", "-h"]).output()?;
    let supports_reason = help.text().contains("--reason") || help.stderr.contains("--reason");

    create_branch_at(repo, branch, &start)?;
    let workers = config_get(repo, "checkout.workers")?.unwrap_or_else(|| "0".into());
    let mut add = git(repo)
        .arg("-c")
        .arg(format!("checkout.workers={workers}"))
        .args(["worktree", "add"]);
    if supports_reason {
        add = add.args([
            "--lock",
            "--reason",
            &format!("tern-create-{}", uuid::Uuid::new_v4()),
        ]);
    }
    let added = add
        .arg("--")
        .arg(&path)
        .arg(branch)
        .env("GIT_PROGRESS_DELAY", "0")
        .output()?;
    if !added.success() {
        let _ = git(repo).args(["worktree", "prune"]).output();
        let _ = git(repo).args(["branch", "-D", "--", branch]).output();
        bail!("git worktree add failed: {}", added.stderr.trim());
    }

    let finish = || -> anyhow::Result<Option<String>> {
        let mut warning = None;
        if git(&path)
            .args(["cat-file", "-e", "HEAD:.gitmodules"])
            .ok()?
        {
            let sub = git(&path)
                .args(["submodule", "update", "--init", "--recursive"])
                .env("LANGUAGE", "C")
                .output()?;
            if !sub.success() {
                warning = Some(sub.stderr.trim().to_string());
            }
        }
        if let Some(b) = &record_base {
            config_set(repo, &branch_base_config_key(branch), b)?;
        }
        if let Some(r) = &record_base_ref {
            config_set(repo, &branch_base_ref_config_key(branch), r)?;
        }
        Ok(warning)
    };
    let result = finish();
    if supports_reason {
        let _ = git(repo)
            .args(["worktree", "unlock", "--"])
            .arg(&path)
            .output();
    }
    let submodule_warning = match result {
        Ok(w) => w,
        Err(e) => {
            let _ = remove(repo, &path, true);
            let _ = git(repo).args(["branch", "-D", "--", branch]).output();
            return Err(e);
        }
    };
    Ok(CreatedWorktree {
        path,
        branch: branch.to_string(),
        base_ref: used_base,
        start_point: start,
        submodule_warning,
    })
}

/// `git worktree remove [--force]`. A worktree whose folder is already gone is pruned and
/// counts as removed. The branch is kept.
pub fn remove(repo: &Path, worktree: &Path, force: bool) -> anyhow::Result<()> {
    let mut cmd = git(repo).args(["worktree", "remove"]);
    if force {
        cmd = cmd.arg("--force");
    }
    let out = cmd
        .arg(worktree)
        .timeout(Duration::from_secs(5 * 60))
        .output()?;
    if out.success() {
        return Ok(());
    }
    if !worktree.exists() {
        git(repo)
            .args(["worktree", "prune"])
            .timeout(Duration::from_secs(15))
            .run()?;
        return Ok(());
    }
    let detail = out.stderr.trim();
    if detail.is_empty() {
        bail!("git worktree remove failed");
    }
    bail!("git worktree remove failed: {detail}")
}

/// `<prefix>/<slug>` (slug sanitized), made unique against existing branches with `-1`..`-100`,
/// or flattened when a parent path is a branch.
pub fn unique_branch_name(repo: &Path, prefix: &str, slug: &str) -> anyhow::Result<String> {
    let wanted = format_generated_branch_name(slug, BranchNamingMode::Static, prefix);
    resolve_available_branch_name(repo, &wanted)
}

/// A temporary `<prefix>/<8 hex>` branch name for a new worktree.
pub fn temporary_branch_name(repo: &Path, prefix: &str) -> anyhow::Result<String> {
    crate::naming::creatable_temporary_branch_name(repo, prefix)
}

/// Rename a worktree's temporary branch to a generated name, deduplicated. `custom` names are
/// used as given. Returns the final name.
pub fn rename_generated_branch(
    repo: &Path,
    old: &str,
    generated: &str,
    custom: bool,
) -> anyhow::Result<String> {
    let new = if custom {
        generated.to_string()
    } else {
        resolve_available_branch_name(repo, generated)?
    };
    crate::refs::rename_branch(repo, old, &new)?;
    // Carry the recorded base over; git moves branch.<old>.* sections itself, this is a guard
    // for configs it couldn't rewrite.
    if config_get(repo, &branch_base_config_key(&new))?.is_none()
        && let Some(b) = config_get(repo, &branch_base_config_key(old))?
    {
        let _ = config_set(repo, &branch_base_config_key(&new), &b);
    }
    Ok(new)
}

/// Is `path` exactly `<root>/<repo>/<leaf>` under one of the managed roots?
pub fn is_managed_path(path: &Path, managed_roots: &[&Path]) -> bool {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return false;
    }
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    managed_roots.iter().any(|root| {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        real.strip_prefix(&root)
            .is_ok_and(|rest| rest.components().count() == 2)
    })
}

/// Recreate a managed worktree whose folder went missing (spec/05 §4a.17 "Restore").
pub fn restore_missing(
    repo: &Path,
    managed_roots: &[&Path],
    path: &Path,
    branch: &str,
) -> anyhow::Result<()> {
    if !is_managed_path(path, managed_roots) {
        bail!("{} is not a managed worktree path.", path.display());
    }
    if path.exists() {
        let leftovers = std::fs::read_dir(path)?
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        if leftovers
            .iter()
            .all(|n| n == "node_modules" || n == ".next")
        {
            std::fs::remove_dir_all(path)?;
        } else {
            return Ok(());
        }
    }
    let fail = || {
        anyhow::anyhow!(
            "Could not recreate the worktree for branch '{branch}' at {}.",
            path.display()
        )
    };
    git(repo).args(["worktree", "prune"]).output()?;
    if !branch_exists(repo, branch)? {
        let primary = primary_remote(repo)?;
        let remote_ref = primary
            .as_ref()
            .map(|p| format!("refs/remotes/{p}/{branch}"));
        let start = match &remote_ref {
            Some(r) if resolves_to_commit(repo, r)?.is_some() => Some(r.clone()),
            _ => {
                let cur = current_branch(repo)?.filter(|c| c != branch);
                let remote_head = primary
                    .as_ref()
                    .map(|p| format!("refs/remotes/{p}/HEAD"))
                    .and_then(|r| resolves_to_commit(repo, &r).ok().flatten());
                match cur {
                    Some(c) if resolves_to_commit(repo, &c)?.is_some() => Some(c),
                    _ => remote_head.or(resolves_to_commit(repo, "HEAD")?),
                }
            }
        };
        let Some(start) = start else {
            bail!(
                "Could not recreate branch '{branch}' at {}: the project workspace has no commit to start it from.",
                path.display()
            );
        };
        create_branch_at(repo, branch, &start).map_err(|_| fail())?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    git(repo)
        .args(["worktree", "add", "--"])
        .arg(path)
        .arg(branch)
        .run()
        .map_err(|_| fail())?;
    Ok(())
}

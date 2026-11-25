//! Hidden-ref workspace snapshots (spec/05 §2.13, spec/07 §7):
//! scratch index, `git add -A`, `write-tree`, `commit-tree`, `update-ref`
//! under `refs/tern/checkpoints/<thread-hash>/ordinal/<n>`.
//!
//! The port is spec/07 §7.3's "minimal port": no kept capture indexes, private object dirs,
//! LFS redirection, sparse handling, branch-switch replay or submodule recursion.
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use tern_core::CheckpointFileSummary;

use crate::cmd::git;
use crate::diff::parse_numstat;
use crate::util::{base64url, sha256_hex, truncate_utf8};

pub const CHECKPOINT_REFS_PREFIX: &str = "refs/tern/checkpoints";
pub const IDENTITY_NAME: &str = "Tern";
pub const IDENTITY_EMAIL: &str = "tern@users.noreply.github.com";
/// `CHECKPOINT_DIFF_MAX_OUTPUT_BYTES`.
pub const TURN_DIFF_MAX_BYTES: usize = 10_000_000;
/// Appended to a patch cut at the byte cap; the diff panel shows its truncation banner on it.
pub const TRUNCATED_MARKER: &str = "\n\n[truncated]";

/// `-c` flags every index-touching checkpoint call carries.
const INDEX_CONFIG: &[&str] = &[
    "-c",
    "core.fsmonitor=false",
    "-c",
    "sparse.expectFilesOutsideOfPatterns=false",
    "-c",
    "core.splitIndex=false",
    "-c",
    "splitIndex.sharedIndexExpire=never",
    "-c",
    "core.ignorestat=false",
];
const DURABLE_WRITE: &[&str] = &[
    "-c",
    "core.fsync=objects,reference",
    "-c",
    "core.fsyncMethod=fsync",
];

/// Diff args shared by the checkpoint numstat and patch (spec/07 §7.4).
const CHECKPOINT_DIFF_ARGS: &[&str] = &[
    "-c",
    "diff.relative=false",
    "-c",
    "diff.suppressBlankEmpty=false",
    "diff",
];
const CHECKPOINT_DIFF_TAIL: &[&str] = &[
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--submodule=short",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

/// `base64url(hex(sha256(thread_id))[0..32])`, the per-thread ref segment.
pub fn thread_key(thread_id: &str) -> String {
    let hex = sha256_hex(thread_id.as_bytes());
    base64url(&hex.as_bytes()[..32])
}

/// `refs/tern/checkpoints/<thread-key>/ordinal/<n>`. Ordinal 0 is the thread-start baseline.
pub fn checkpoint_ref(thread_id: &str, ordinal: u32) -> String {
    format!(
        "{CHECKPOINT_REFS_PREFIX}/{}/ordinal/{ordinal}",
        thread_key(thread_id)
    )
}

/// Safety snapshot taken before a rewind, so a failed restore can be put back.
pub fn pre_rewind_ref(thread_id: &str) -> String {
    format!(
        "{CHECKPOINT_REFS_PREFIX}/{}/pre-rewind",
        thread_key(thread_id)
    )
}

/// Does `git_ref` resolve to a commit?
pub fn has_ref(cwd: &Path, git_ref: &str) -> anyhow::Result<bool> {
    Ok(resolve_commit(cwd, git_ref)?.is_some())
}

fn resolve_commit(cwd: &Path, git_ref: &str) -> anyhow::Result<Option<String>> {
    let out = git(cwd)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("{git_ref}^{{commit}}"))
        .env("LC_ALL", "C")
        .output()?;
    Ok(out.success().then(|| out.line()).filter(|s| !s.is_empty()))
}

/// Snapshot the working tree of `cwd` into the ref for (thread, ordinal). Returns the ref.
pub fn capture(cwd: &Path, thread_id: &str, ordinal: u32) -> anyhow::Result<String> {
    let git_ref = checkpoint_ref(thread_id, ordinal);
    capture_ref(cwd, &git_ref)?;
    Ok(git_ref)
}

/// Snapshot the working tree of `cwd` (tracked and untracked, `.gitignore` respected, scoped
/// to `cwd`) into an arbitrary ref. Returns the commit sha.
pub fn capture_ref(cwd: &Path, git_ref: &str) -> anyhow::Result<String> {
    let facts = git(cwd)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
            "--git-path",
            "index",
        ])
        .run()
        .context("not a git repository")?;
    let mut lines = facts.lines();
    let common_dir = PathBuf::from(lines.next().context("rev-parse gave no common dir")?);
    let real_index = PathBuf::from(lines.next().context("rev-parse gave no index path")?);
    let head = resolve_commit(cwd, "HEAD")?;

    let temp_index = common_dir.join(format!(
        "tern-checkpoint-index-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let _guard = RemoveOnDrop(vec![temp_index.clone(), temp_index.with_extension("lock")]);
    let index_env = temp_index.as_os_str().to_owned();

    if let Some(head) = &head {
        // Copying the real index keeps its stat cache, so `add` doesn't rehash every file.
        let seeded = copy_index(&real_index, &temp_index).is_ok()
            && git(cwd)
                .args(INDEX_CONFIG)
                .args(["read-tree", "--reset", head])
                .env("GIT_INDEX_FILE", &index_env)
                .ok()?;
        if !seeded {
            let _ = std::fs::remove_file(&temp_index);
            git(cwd)
                .args(INDEX_CONFIG)
                .args(["read-tree", head])
                .env("GIT_INDEX_FILE", &index_env)
                .run()?;
        }
    }

    let add = |ignore_errors: bool| {
        let mut cmd = git(cwd).args(INDEX_CONFIG).args(DURABLE_WRITE).args([
            "-c",
            "core.safecrlf=false",
            "add",
        ]);
        if ignore_errors {
            cmd = cmd.arg("--ignore-errors");
        }
        cmd.args(["-A", "--", "."])
            .env("GIT_INDEX_FILE", &index_env)
            .output()
    };
    let first = add(false)?;
    if !first.success() {
        // Unreadable files keep their indexed version.
        let second = add(true)?;
        if !second.success() {
            tracing::warn!(stderr = %second.stderr.trim(), "checkpoint add reported errors");
        }
    }

    let tree = git(cwd)
        .args(INDEX_CONFIG)
        .args(DURABLE_WRITE)
        .arg("write-tree")
        .env("GIT_INDEX_FILE", &index_env)
        .run()?
        .trim()
        .to_string();

    let mut commit = git(cwd).args(DURABLE_WRITE).args(["commit-tree", &tree]);
    if let Some(head) = &head {
        // HEAD is always the first parent so later queries can see how HEAD moved.
        commit = commit.args(["-p", head]);
    }
    let commit = commit
        .args(["-m", &format!("tern checkpoint ref={git_ref}")])
        .env("GIT_AUTHOR_NAME", IDENTITY_NAME)
        .env("GIT_AUTHOR_EMAIL", IDENTITY_EMAIL)
        .env("GIT_COMMITTER_NAME", IDENTITY_NAME)
        .env("GIT_COMMITTER_EMAIL", IDENTITY_EMAIL)
        .run()?
        .trim()
        .to_string();

    git(cwd)
        .args(DURABLE_WRITE)
        .args(["update-ref", git_ref, &commit])
        .run()?;
    Ok(commit)
}

fn copy_index(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::metadata(from)?;
    std::fs::copy(from, to)?;
    // Floor to whole seconds so files written in the index's own second stay racy (rechecked).
    if let Ok(mtime) = meta.modified() {
        let secs = mtime
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let floored = UNIX_EPOCH + Duration::from_secs(secs);
        let f = std::fs::OpenOptions::new().write(true).open(to)?;
        f.set_modified(floored.min(SystemTime::now()))?;
    }
    Ok(())
}

struct RemoveOnDrop(Vec<PathBuf>);
impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Per-file +/- between two checkpoint refs.
pub fn summarize(
    cwd: &Path,
    from_ref: &str,
    to_ref: &str,
) -> anyhow::Result<Vec<CheckpointFileSummary>> {
    let out = git(cwd)
        .args(CHECKPOINT_DIFF_ARGS)
        .args(["--raw", "--numstat", "-z"])
        .args(CHECKPOINT_DIFF_TAIL)
        .arg(format!("{from_ref}^{{commit}}"))
        .arg(format!("{to_ref}^{{commit}}"))
        .checked()?;
    Ok(parse_numstat(&out.stdout)
        .into_iter()
        .map(|f| CheckpointFileSummary {
            path: f.path,
            kind: f.kind.as_str().to_string(),
            additions: f.additions,
            deletions: f.deletions,
        })
        .collect())
}

/// A patch as text, cut at a byte cap.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Patch {
    pub text: String,
    /// The text was cut and ends with [`TRUNCATED_MARKER`].
    pub truncated: bool,
}

impl Patch {
    pub(crate) fn capped(raw: Vec<u8>, max: usize) -> Patch {
        let text = String::from_utf8_lossy(&raw).into_owned();
        if text.len() <= max {
            return Patch {
                text,
                truncated: false,
            };
        }
        let mut cut = truncate_utf8(&text, max).to_string();
        cut.push_str(TRUNCATED_MARKER);
        Patch {
            text: cut,
            truncated: true,
        }
    }
}

/// The turn diff between two checkpoint refs (spec/05 §2.13 "Turn diff"). Turn diffs ignore
/// whitespace by default in T3. `from == to` is empty.
pub fn turn_patch(
    cwd: &Path,
    from_ref: &str,
    to_ref: &str,
    ignore_whitespace: bool,
) -> anyhow::Result<Patch> {
    if from_ref == to_ref {
        return Ok(Patch::default());
    }
    let mut cmd = git(cwd)
        .args(CHECKPOINT_DIFF_ARGS)
        .arg("--patch")
        .args(CHECKPOINT_DIFF_TAIL);
    if ignore_whitespace {
        cmd = cmd.arg("--ignore-all-space");
    }
    let out = cmd
        .arg(format!("{from_ref}^{{commit}}"))
        .arg(format!("{to_ref}^{{commit}}"))
        .checked()?;
    Ok(Patch::capped(out.stdout, TURN_DIFF_MAX_BYTES))
}

/// Restore the working tree to a checkpoint ref (revert). HEAD and the branch don't move,
/// ignored files survive, untracked files not in the snapshot are removed, and the index is
/// reset to HEAD for the scope (spec/07 §7.5).
pub fn restore(cwd: &Path, git_ref: &str) -> anyhow::Result<()> {
    let Some(oid) = resolve_commit(cwd, git_ref)? else {
        bail!("Checkpoint {git_ref} does not exist.");
    };
    let reset_index = || -> anyhow::Result<()> {
        std::fs::create_dir_all(cwd)?;
        git(cwd).args(["reset", "--quiet", "--", "."]).run()?;
        Ok(())
    };

    let tracked = git(cwd)
        .args(["ls-files", "--cached"])
        .arg(format!("--with-tree={oid}"))
        .args(["-z", "--", "."])
        .checked()?;
    // An empty index and checkpoint have nothing for restore's pathspec to match.
    if !tracked.stdout.is_empty() {
        let sparse = git(cwd)
            .args(["config", "--bool", "core.sparseCheckout"])
            .output()?
            .line()
            == "true";
        let mut cmd = git(cwd).args(["restore", "--source", &oid, "--worktree", "--staged"]);
        if !sparse {
            cmd = cmd.arg("--ignore-skip-worktree-bits");
        }
        if let Err(e) = cmd.args(["--", "."]).run() {
            let _ = reset_index();
            return Err(e);
        }
    }

    std::fs::create_dir_all(cwd)?;
    let clean = || {
        git(cwd)
            .args(["clean", "-fd", "--", "."])
            .env("LC_ALL", "C")
            .output()
    };
    let removed = |text: &str| text.lines().any(|l| l.starts_with("Removing "));
    let mut cleaned = clean()?;
    // Removing a new .gitignore exposes the files it hid; clean again until nothing is left.
    let mut pass = 1;
    while pass < 4 && cleaned.success() && removed(&cleaned.text()) {
        std::fs::create_dir_all(cwd)?;
        cleaned = clean()?;
        pass += 1;
    }
    if cleaned.success() && removed(&cleaned.text()) {
        std::fs::create_dir_all(cwd)?;
        let remaining = git(cwd)
            .args(["clean", "-dn", "--", "."])
            .env("LC_ALL", "C")
            .run()?;
        if remaining.lines().any(|l| l.starts_with("Would remove ")) {
            reset_index()?;
            bail!("Could not clean the checkpoint workspace: untracked files remain.");
        }
    }
    if !cleaned.success() {
        // Git can remove every child, then fail trying to remove './' itself.
        let emptied = cleaned.code == Some(1)
            && cleaned
                .stderr
                .trim()
                .starts_with("warning: failed to remove ./:")
            && std::fs::read_dir(cwd).is_ok_and(|mut d| d.next().is_none());
        if !emptied {
            reset_index()?;
            let detail = cleaned.stderr.trim();
            if detail.is_empty() {
                bail!("Could not clean the checkpoint workspace.");
            }
            bail!("{detail}");
        }
    }
    reset_index()
}

/// Delete refs with `update-ref --no-deref -z --stdin`.
pub fn delete_refs(cwd: &Path, refs: &[String]) -> anyhow::Result<()> {
    if refs.is_empty() {
        return Ok(());
    }
    let mut input = String::new();
    for r in refs {
        input.push_str("delete ");
        input.push_str(r);
        input.push_str("\0\0");
    }
    git(cwd)
        .args(["update-ref", "--no-deref", "-z", "--stdin"])
        .stdin(input)
        .run()?;
    Ok(())
}

/// Every checkpoint ref of a thread (ordinals and the pre-rewind snapshot).
pub fn list_refs(cwd: &Path, thread_id: &str) -> anyhow::Result<Vec<String>> {
    let prefix = format!("{CHECKPOINT_REFS_PREFIX}/{}/", thread_key(thread_id));
    let out = git(cwd)
        .args(["for-each-ref", "--format=%(refname)", &prefix])
        .run()?;
    Ok(out
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// Delete every checkpoint ref of a thread.
pub fn delete_all(cwd: &Path, thread_id: &str) -> anyhow::Result<()> {
    delete_refs(cwd, &list_refs(cwd, thread_id)?)
}

/// Checkpoint refs of a thread with an ordinal above `ordinal` (rolled back turns).
pub fn refs_after(cwd: &Path, thread_id: &str, ordinal: u32) -> anyhow::Result<Vec<String>> {
    Ok(list_refs(cwd, thread_id)?
        .into_iter()
        .filter(|r| {
            r.rsplit_once("/ordinal/")
                .and_then(|(_, n)| n.parse::<u32>().ok())
                .is_some_and(|n| n > ordinal)
        })
        .collect())
}

//! Review scopes for the diff panel (spec/05 §2.2, §2.14): branch changes vs the merge base,
//! uncommitted changes, one commit, and one turn between two checkpoint refs.
//!
//! Working-tree scopes include untracked files: they're added `--intent-to-add` to a throwaway
//! copy of the index, passed to `git diff` through `GIT_INDEX_FILE`, so they show up as new
//! files without touching the user's staging.
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::bail;
use tern_core::DiffScope;

use crate::checkpoint::{Patch, TURN_DIFF_MAX_BYTES};
use crate::cmd::{Cmd, git, split_nul};
use crate::diff::{FileStat, parse_numstat};
use crate::refs::{EMPTY_TREE, current_branch, head_commit, resolve_base_branch};

/// Whole-preview patch cap for git scopes.
pub const REVIEW_PATCH_MAX_BYTES: usize = 120_000;
/// Per-file patch cap when files load lazily.
pub const REVIEW_FILE_PATCH_MAX_BYTES: usize = 1024 * 1024;
pub const REVIEW_UNTRACKED_MAX_FILES: usize = 5_000;
pub const REVIEW_UNTRACKED_MAX_BYTES: u64 = 256 * 1024 * 1024;
pub const REVIEW_BRANCH_COMMIT_LIMIT: usize = 100;
pub const REVIEW_BRANCH_SUBJECT_LIMIT: usize = 300;

const REVIEW_DIFF_ARGS: &[&str] = &[
    "-c",
    "diff.suppressBlankEmpty=false",
    "diff",
    "--find-renames",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--submodule=short",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

#[derive(Clone, Debug, PartialEq)]
pub struct DiffOptions {
    /// `--ignore-all-space`. The panel's setting defaults to true.
    pub ignore_whitespace: bool,
    /// Limit to these repo-relative paths (lazy per-file loading).
    pub paths: Vec<String>,
    /// Patch byte cap. None = the turn-diff cap (10 MB).
    pub max_bytes: Option<usize>,
    /// Explicit base for the branch scope; None = the branch's recorded base or default branch.
    pub base_ref: Option<String>,
}

impl Default for DiffOptions {
    fn default() -> Self {
        DiffOptions {
            ignore_whitespace: true,
            paths: Vec::new(),
            max_bytes: None,
            base_ref: None,
        }
    }
}

/// What a scope resolved to.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ScopeSource {
    /// "Changes", "Changes vs {base}", "Uncommitted", the commit subject or "Commit {sha7}",
    /// or "Turn".
    pub title: String,
    /// The base branch (branch scope) or parent commit (commit scope).
    pub base_ref: Option<String>,
    /// The revision diffed from.
    pub from: String,
    /// The revision diffed to; None = the working tree.
    pub to: Option<String>,
    /// Some untracked files were left out (too many or too large).
    pub incomplete: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ScopePatch {
    pub source: ScopeSource,
    pub patch: Patch,
}

/// One commit of the branch scope's commit list.
#[derive(Clone, Debug, PartialEq)]
pub struct CommitSummary {
    pub sha: String,
    pub subject: String,
}

type RefsFn<'a> = &'a dyn Fn(&str) -> Option<(String, String)>;

fn pathspecs(paths: &[String]) -> Vec<String> {
    paths.iter().map(|p| format!(":(top,literal){p}")).collect()
}

struct Resolved {
    source: ScopeSource,
    index: Option<TempIndex>,
    checkpoint: bool,
}

fn resolve(
    cwd: &Path,
    scope: &DiffScope,
    refs: RefsFn,
    opts: &DiffOptions,
) -> anyhow::Result<Resolved> {
    match scope {
        DiffScope::Turn { checkpoint_id } => {
            let Some((from, to)) = refs(checkpoint_id) else {
                bail!("This turn's checkpoint is not available.");
            };
            Ok(Resolved {
                source: ScopeSource {
                    title: "Turn".into(),
                    base_ref: None,
                    from,
                    to: Some(to),
                    incomplete: false,
                },
                index: None,
                checkpoint: true,
            })
        }
        DiffScope::Commit(sha) => {
            let (sha, parent) = resolve_commit(cwd, sha)?;
            let subject = commit_subject(cwd, &sha)?;
            Ok(Resolved {
                source: ScopeSource {
                    title: if subject.is_empty() {
                        format!("Commit {}", &sha[..7])
                    } else {
                        subject
                    },
                    base_ref: Some(parent.clone()),
                    from: parent,
                    to: Some(sha),
                    incomplete: false,
                },
                index: None,
                checkpoint: false,
            })
        }
        DiffScope::Uncommitted | DiffScope::Branch => {
            let head = head_commit(cwd)?;
            let (title, base_ref, from) = if matches!(scope, DiffScope::Uncommitted) {
                (
                    "Uncommitted".to_string(),
                    None,
                    head.unwrap_or_else(|| EMPTY_TREE.to_string()),
                )
            } else {
                let (base, merge_base) = merge_base(cwd, opts.base_ref.as_deref())?;
                let title = match &base {
                    Some(b) if opts.base_ref.is_some() => format!("Changes vs {b}"),
                    _ => "Changes".to_string(),
                };
                let from = match merge_base {
                    Some(m) => m,
                    None => head.unwrap_or_else(|| EMPTY_TREE.to_string()),
                };
                (title, base, from)
            };
            let index = prepare_untracked(cwd, &opts.paths)?;
            Ok(Resolved {
                source: ScopeSource {
                    title,
                    base_ref,
                    from,
                    to: None,
                    incomplete: index.incomplete,
                },
                index: Some(index),
                checkpoint: false,
            })
        }
    }
}

/// Base branch and merge base for the branch scope. (None, None) = no base, diff vs HEAD.
fn merge_base(
    cwd: &Path,
    explicit: Option<&str>,
) -> anyhow::Result<(Option<String>, Option<String>)> {
    let base = match explicit {
        Some(b) => {
            let comparison = if !b.starts_with("refs/") && crate::refs::branch_exists(cwd, b)? {
                format!("refs/heads/{b}")
            } else if !b.starts_with("refs/")
                && b.contains('/')
                && git(cwd)
                    .args(["show-ref", "--verify", "--quiet"])
                    .arg(format!("refs/remotes/{b}"))
                    .ok()?
            {
                format!("refs/remotes/{b}")
            } else {
                b.to_string()
            };
            Some((b.to_string(), comparison))
        }
        None => match current_branch(cwd)? {
            Some(branch) => {
                resolve_base_branch(cwd, &branch, true)?.map(|r| (r.base_ref, r.comparison_ref))
            }
            None => None,
        },
    };
    let Some((base_ref, comparison)) = base else {
        return Ok((None, None));
    };
    let out = git(cwd)
        .args(["merge-base", "--end-of-options", &comparison, "HEAD"])
        .output()?;
    let mb = out.line();
    if !out.success() || mb.is_empty() {
        if explicit.is_none() && head_commit(cwd)?.is_none() {
            return Ok((None, None));
        }
        bail!("Could not find a common commit between '{base_ref}' and HEAD.");
    }
    Ok((Some(base_ref), Some(mb)))
}

fn resolve_commit(cwd: &Path, sha: &str) -> anyhow::Result<(String, String)> {
    let valid = (sha.len() == 40 || sha.len() == 64) && sha.bytes().all(|b| b.is_ascii_hexdigit());
    let invalid = "Selected commit is invalid or no longer available.";
    if !valid {
        bail!(invalid);
    }
    let present = |rev: &str| {
        git(cwd)
            .args(["cat-file", "-e"])
            .arg(format!("{rev}^{{commit}}"))
            .env("GIT_NO_LAZY_FETCH", "1")
            .ok()
    };
    if !present(sha)? {
        bail!(invalid);
    }
    // The raw object keeps parents a shallow clone grafts away, so a boundary isn't a root.
    let object = git(cwd).args(["cat-file", "commit", sha]).run()?;
    let parent = object
        .lines()
        .nth(1)
        .and_then(|l| l.strip_prefix("parent "))
        .map(str::to_string);
    let parent = match parent {
        Some(p) => {
            if !present(&p)? {
                bail!("Selected commit's parent is not available locally.");
            }
            p
        }
        None => EMPTY_TREE.to_string(),
    };
    Ok((sha.to_lowercase(), parent))
}

fn commit_subject(cwd: &Path, sha: &str) -> anyhow::Result<String> {
    let body = git(cwd)
        .args([
            "show",
            "-s",
            "--no-notes",
            "--no-show-signature",
            "--format=%B",
            sha,
        ])
        .run()?;
    Ok(body.lines().next().unwrap_or_default().trim().to_string())
}

fn diff_cmd(cwd: &Path, r: &Resolved, opts: &DiffOptions) -> Cmd {
    let mut cmd = if r.checkpoint {
        git(cwd).args([
            "-c",
            "diff.relative=false",
            "-c",
            "diff.suppressBlankEmpty=false",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--submodule=short",
            "--src-prefix=a/",
            "--dst-prefix=b/",
        ])
    } else {
        git(cwd).args(REVIEW_DIFF_ARGS)
    };
    if opts.ignore_whitespace {
        cmd = cmd.arg("--ignore-all-space");
    }
    if let Some(TempIndex {
        path: Some(path), ..
    }) = &r.index
    {
        cmd = cmd.env("GIT_INDEX_FILE", path);
    }
    cmd
}

fn revisions(r: &Resolved) -> Vec<String> {
    let s = &r.source;
    match (&s.to, r.checkpoint) {
        (Some(to), true) => vec![format!("{}^{{commit}}", s.from), format!("{to}^{{commit}}")],
        (Some(to), false) => vec![s.from.clone(), to.clone()],
        (None, _) => vec![s.from.clone()],
    }
}

/// The patch for a scope, cut at `opts.max_bytes`.
pub fn scope_patch(
    cwd: &Path,
    scope: &DiffScope,
    checkpoint_refs: RefsFn,
    opts: &DiffOptions,
) -> anyhow::Result<ScopePatch> {
    let r = resolve(cwd, scope, checkpoint_refs, opts)?;
    if r.checkpoint && r.source.to.as_deref() == Some(r.source.from.as_str()) {
        return Ok(ScopePatch {
            source: r.source,
            patch: Patch::default(),
        });
    }
    let mut cmd = diff_cmd(cwd, &r, opts).arg("--patch").args(revisions(&r));
    if !opts.paths.is_empty() {
        cmd = cmd.arg("--").args(pathspecs(&opts.paths));
    }
    let out = cmd.checked()?;
    let patch = Patch::capped(out.stdout, opts.max_bytes.unwrap_or(TURN_DIFF_MAX_BYTES));
    Ok(ScopePatch {
        source: r.source,
        patch,
    })
}

/// File list with +/- for a scope (`--raw --numstat -z`), no patches.
pub fn scope_files(
    cwd: &Path,
    scope: &DiffScope,
    checkpoint_refs: RefsFn,
    opts: &DiffOptions,
) -> anyhow::Result<(ScopeSource, Vec<FileStat>)> {
    let r = resolve(cwd, scope, checkpoint_refs, opts)?;
    let mut cmd = diff_cmd(cwd, &r, opts)
        .args(["--raw", "--numstat", "-z"])
        .args(revisions(&r));
    if !opts.paths.is_empty() {
        cmd = cmd.arg("--").args(pathspecs(&opts.paths));
    }
    let out = cmd.checked()?;
    Ok((r.source, parse_numstat(&out.stdout)))
}

/// Commits on the current branch since its merge base (newest first, at most 100).
pub fn branch_commits(cwd: &Path, base_ref: Option<&str>) -> anyhow::Result<Vec<CommitSummary>> {
    if head_commit(cwd)?.is_none() {
        return Ok(Vec::new());
    }
    let (_, merge_base) = merge_base(cwd, base_ref)?;
    let mut cmd = git(cwd).args([
        "log",
        "--no-show-signature",
        "--no-color",
        "--format=%H%x00%s%x00",
        "-n",
        &REVIEW_BRANCH_COMMIT_LIMIT.to_string(),
    ]);
    cmd = match merge_base {
        Some(mb) => cmd.arg(format!("{mb}..HEAD")),
        None => cmd.arg("HEAD"),
    };
    let out = cmd.checked()?;
    let fields = split_nul(&out.stdout);
    Ok(fields
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| CommitSummary {
            sha: c[0].trim().to_string(),
            subject: c[1].chars().take(REVIEW_BRANCH_SUBJECT_LIMIT).collect(),
        })
        .collect())
}

/// Removes the temporary index on drop.
pub(crate) struct TempIndex {
    pub path: Option<PathBuf>,
    pub incomplete: bool,
}

impl Drop for TempIndex {
    fn drop(&mut self) {
        if let Some(p) = &self.path {
            let _ = std::fs::remove_file(p);
            let _ = std::fs::remove_file(p.with_extension("lock"));
        }
    }
}

fn prepare_untracked(cwd: &Path, paths: &[String]) -> anyhow::Result<TempIndex> {
    let none = |incomplete| TempIndex {
        path: None,
        incomplete,
    };
    let mut ls = git(cwd).args(["ls-files", "--others", "--exclude-standard", "-z"]);
    if !paths.is_empty() {
        ls = ls.arg("--").args(pathspecs(paths));
    }
    let untracked = split_nul(&ls.checked()?.stdout);
    if untracked.is_empty() {
        return Ok(none(false));
    }
    if untracked.len() > REVIEW_UNTRACKED_MAX_FILES {
        return Ok(none(true));
    }

    let staged_deletions: Vec<String> = if head_commit(cwd)?.is_some() {
        split_nul(
            &git(cwd)
                .args([
                    "diff",
                    "--cached",
                    "--no-renames",
                    "--name-only",
                    "--diff-filter=D",
                    "-z",
                    "HEAD",
                    "--",
                ])
                .output()?
                .stdout,
        )
    } else {
        Vec::new()
    };
    let mut sized: Vec<(String, u64)> = Vec::new();
    for p in untracked {
        if staged_deletions.contains(&p) {
            continue;
        }
        // A nested repository with no commits can't be indexed.
        if p.ends_with('/') && head_commit(&cwd.join(&p))?.is_none() {
            continue;
        }
        let size = std::fs::symlink_metadata(cwd.join(&p))
            .map(|m| if m.is_file() { m.len() } else { 0 })
            .unwrap_or(0);
        sized.push((p, size));
    }
    let candidates = sized.len();
    // Smallest first, so which files fit doesn't depend on how their names sort.
    sized.sort_by_key(|(_, s)| *s);
    let mut total = 0u64;
    let mut add: Vec<String> = Vec::new();
    for (p, s) in sized {
        total += s;
        if total > REVIEW_UNTRACKED_MAX_BYTES {
            break;
        }
        add.push(p);
    }
    let omitted = add.len() < candidates;
    if add.is_empty() {
        return Ok(none(omitted));
    }

    let temp = std::env::temp_dir().join(format!(
        "tern-review-index-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    let index = TempIndex {
        path: Some(temp.clone()),
        incomplete: omitted,
    };
    let real = git(cwd)
        .args(["rev-parse", "--path-format=absolute", "--git-path", "index"])
        .run()?;
    let real = PathBuf::from(real.trim_end());
    let split_cfg = [
        "-c",
        "core.splitIndex=false",
        "-c",
        "splitIndex.sharedIndexExpire=never",
    ];
    if real.exists() {
        let mtime = std::fs::metadata(&real)?.modified().ok();
        std::fs::copy(&real, &temp)?;
        if let Some(m) = mtime {
            let secs = m
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let floored = (UNIX_EPOCH + Duration::from_secs(secs)).min(SystemTime::now());
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .open(&temp)
                .and_then(|f| f.set_modified(floored));
        }
    } else {
        git(cwd)
            .args(["read-tree", "--empty"])
            .env("GIT_INDEX_FILE", &temp)
            .run()?;
    }
    git(cwd)
        .args(split_cfg)
        .args(["update-index", "--no-split-index"])
        .env("GIT_INDEX_FILE", &temp)
        .run()?;
    let mut stdin = add.join("\0");
    stdin.push('\0');
    git(cwd)
        .args(split_cfg)
        .args([
            "-c",
            "core.preloadIndex=false",
            "-c",
            "core.sparseCheckout=false",
            "--literal-pathspecs",
            "add",
            "--intent-to-add",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
        ])
        .env("GIT_INDEX_FILE", &temp)
        .stdin(stdin)
        .run()?;
    Ok(index)
}

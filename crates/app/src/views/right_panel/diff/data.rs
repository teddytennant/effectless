//! Where diffs come from (spec 05 §2.2, §2.13-2.14). Everything here blocks: run it
//! in `cx.background_spawn`.
//!
//! Git scopes call `tern_git::review` against the thread folder. Turn scopes
//! resolve checkpoint refs from `ThreadDetail::checkpoints`. Tool calls use the
//! patch the provider recorded. `--demo` projects have no folder on disk, so when
//! the cwd does not exist every scope falls back to the thread's recorded
//! `FileChange` patches (a dev fallback so screenshots have content).

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::HighlightStyle;
use tern_core::{CheckpointStatus, DiffScope, ThreadDetail, TurnItemBody};
use tern_git::checkpoint::TRUNCATED_MARKER;
use tern_git::diff::parse_unified;
use tern_git::review::DiffOptions;
use tern_git::{DiffLineKind, FileDiff};

use super::rows::sort_files;
use crate::syntax;

/// `DiffPanelSelection` (spec 05 §2.2).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Selection {
    /// "Changes": working tree vs the merge base with the base ref. The default.
    Branch {
        base_ref: Option<String>,
    },
    /// "Uncommitted": working tree vs HEAD.
    Uncommitted,
    Commit {
        sha: String,
        subject: String,
    },
    /// One turn: checkpoint N-1 -> N.
    Turn {
        checkpoint_id: String,
    },
    /// The patch recorded for one file-change item.
    ToolCall {
        item_id: String,
    },
}

impl Default for Selection {
    fn default() -> Self {
        Self::Branch { base_ref: None }
    }
}

impl Selection {
    pub fn is_git(&self) -> bool {
        matches!(
            self,
            Self::Branch { .. } | Self::Uncommitted | Self::Commit { .. }
        )
    }

    /// Section id for fold and scroll memory (`DiffPanel.tsx:412-418`).
    pub fn section_id(&self, thread_id: &str) -> String {
        match self {
            Self::Branch { .. } => "branch".into(),
            Self::Uncommitted => "unstaged".into(),
            Self::Commit { sha, .. } => format!("commit:{sha}"),
            Self::Turn { checkpoint_id } => format!("turn:{checkpoint_id}"),
            Self::ToolCall { item_id } => format!("tool-call:{thread_id}:{item_id}"),
        }
    }

    pub fn loading_label(&self) -> &'static str {
        match self {
            Self::ToolCall { .. } => "Loading tool call diff...",
            Self::Turn { .. } => "Loading checkpoint diff...",
            Self::Uncommitted => "Loading uncommitted changes...",
            Self::Commit { .. } => "Loading commit...",
            Self::Branch { .. } => "Loading changes...",
        }
    }
}

/// One completed turn for the scope menu, newest first.
#[derive(Clone, Debug, PartialEq)]
pub struct TurnEntry {
    pub checkpoint_id: String,
    /// The run's ordinal: "Turn {n}".
    pub ordinal: u32,
    pub completed_at: chrono::DateTime<chrono::Utc>,
}

/// Completed turns with a ready checkpoint, newest first (spec 05 §2.2).
pub fn turns(detail: &ThreadDetail) -> Vec<TurnEntry> {
    let mut out: Vec<TurnEntry> = detail
        .checkpoints
        .iter()
        .filter(|c| c.run_id.is_some())
        .filter(|c| {
            matches!(
                c.status,
                CheckpointStatus::Ready | CheckpointStatus::Missing
            )
        })
        .map(|c| TurnEntry {
            checkpoint_id: c.id.clone(),
            ordinal: c.ordinal,
            completed_at: c.captured_at,
        })
        .collect();
    out.sort_by(|a, b| {
        b.ordinal
            .cmp(&a.ordinal)
            .then(b.completed_at.cmp(&a.completed_at))
    });
    out
}

/// (from ref, to ref) for a turn's checkpoint: the previous ordinal's ref, then its own.
pub fn checkpoint_refs(detail: &ThreadDetail, checkpoint_id: &str) -> Option<(String, String)> {
    let to = detail.checkpoints.iter().find(|c| c.id == checkpoint_id)?;
    let from = detail
        .checkpoints
        .iter()
        .filter(|c| c.ordinal < to.ordinal)
        .max_by_key(|c| c.ordinal)?;
    Some((from.git_ref.clone(), to.git_ref.clone()))
}

/// A recorded file-change patch.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedPatch {
    pub item_id: String,
    pub run_id: Option<String>,
    pub diff: String,
}

/// Every `FileChange` item with a patch, in timeline order.
pub fn recorded_patches(detail: &ThreadDetail) -> Vec<RecordedPatch> {
    detail
        .items
        .iter()
        .filter_map(|item| match &item.body {
            TurnItemBody::FileChange {
                diff_str: Some(diff),
                ..
            } => Some(RecordedPatch {
                item_id: item.id.clone(),
                run_id: item.run_id.clone(),
                diff: diff.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// Everything a load needs, captured on the foreground.
#[derive(Clone, Debug)]
pub struct Request {
    pub cwd: PathBuf,
    pub thread_id: String,
    pub selection: Selection,
    pub ignore_whitespace: bool,
    /// Checkpoint refs for a turn selection.
    pub turn_refs: Option<(String, String)>,
    /// The thread's recorded patches (tool calls and the demo fallback).
    pub patches: Vec<RecordedPatch>,
    /// Run id of a turn selection (demo fallback filter).
    pub turn_run_id: Option<String>,
    pub dark: bool,
}

/// Per-line syntax spans, `[hunk][line]`, ranges relative to the line.
pub type LineSpans = Vec<Vec<Vec<(Range<usize>, HighlightStyle)>>>;

#[derive(Clone, Debug, Default)]
pub struct Loaded {
    pub files: Vec<FileDiff>,
    pub spans: Vec<LineSpans>,
    /// The patch hit the size cap.
    pub truncated: bool,
    /// Untracked files were left out.
    pub incomplete: bool,
    /// The patch could not be parsed; shown raw.
    pub raw: Option<String>,
    pub error: Option<String>,
    /// Resolved base ref (branch scope).
    pub base_ref: Option<String>,
    /// HEAD's branch, for the "{head} → {base}" compare.
    pub head_ref: Option<String>,
    /// Data came from recorded patches because the folder does not exist.
    pub demo: bool,
}

/// Loads and highlights one selection.
pub fn load(req: &Request) -> Loaded {
    let mut loaded = if req.cwd.is_dir() {
        match &req.selection {
            Selection::ToolCall { item_id } => from_patches(req, |p| &p.item_id == item_id),
            selection => load_git(req, selection),
        }
    } else {
        let mut l = match &req.selection {
            Selection::ToolCall { item_id } => from_patches(req, |p| &p.item_id == item_id),
            Selection::Turn { .. } => {
                from_patches(req, |p| p.run_id.is_some() && p.run_id == req.turn_run_id)
            }
            _ => from_patches(req, |_| true),
        };
        l.demo = true;
        if matches!(req.selection, Selection::Branch { .. }) {
            l.head_ref = Some("main".into());
        }
        l
    };
    sort_files(&mut loaded.files);
    loaded.spans = loaded
        .files
        .iter()
        .map(|f| highlight_file(f, req.dark))
        .collect();
    loaded
}

fn load_git(req: &Request, selection: &Selection) -> Loaded {
    let scope = match selection {
        Selection::Branch { base_ref } => DiffScope::Branch {
            base_ref: base_ref.clone(),
        },
        Selection::Uncommitted => DiffScope::Uncommitted,
        Selection::Commit { sha, .. } => DiffScope::Commit(sha.clone()),
        Selection::Turn { checkpoint_id } => DiffScope::Turn {
            checkpoint_id: checkpoint_id.clone(),
        },
        Selection::ToolCall { .. } => unreachable!(),
    };
    let refs = req.turn_refs.clone();
    let lookup = move |_: &str| refs.clone();
    // One request at the turn-diff cap (10 MB) instead of T3's 120 KB preview plus
    // per-file lazy patches: the list is virtualized, so size only costs parse time.
    let opts = DiffOptions {
        ignore_whitespace: req.ignore_whitespace,
        max_bytes: None,
        ..Default::default()
    };
    let head_ref = tern_git::refs::current_branch(&req.cwd).ok().flatten();
    match tern_git::scope_patch(&req.cwd, &scope, &lookup, &opts) {
        Ok(patch) => {
            let text = patch.patch.text;
            let truncated = text.ends_with(TRUNCATED_MARKER) || patch.patch.truncated;
            let files = parse_unified(&text);
            let raw = (files.is_empty() && !text.trim().is_empty()).then(|| text.clone());
            Loaded {
                files,
                truncated,
                incomplete: patch.source.incomplete,
                raw,
                base_ref: patch.source.base_ref,
                head_ref,
                ..Default::default()
            }
        }
        Err(err) => Loaded {
            error: Some(err.to_string()),
            head_ref,
            ..Default::default()
        },
    }
}

/// Parses the recorded patches that pass `keep`, merging repeated edits of one
/// file into a single entry.
fn from_patches(req: &Request, keep: impl Fn(&RecordedPatch) -> bool) -> Loaded {
    let mut files: Vec<FileDiff> = Vec::new();
    for patch in req.patches.iter().filter(|p| keep(p)) {
        for file in parse_unified(&patch.diff) {
            match files.iter_mut().find(|f| f.path == file.path) {
                Some(existing) => merge_into(existing, file),
                None => files.push(file),
            }
        }
    }
    Loaded {
        files,
        ..Default::default()
    }
}

fn merge_into(existing: &mut FileDiff, file: FileDiff) {
    existing.additions += file.additions;
    existing.deletions += file.deletions;
    for hunk in file.hunks {
        if !existing.hunks.contains(&hunk) {
            existing.hunks.push(hunk);
        }
    }
    existing.hunks.sort_by_key(|h| h.new_start);
}

/// Highlights both sides of a file's hunks: the new side (context + additions)
/// and the old side (context + deletions) are each highlighted as one document so
/// multi-line constructs keep their colors, then mapped back to diff lines.
pub fn highlight_file(file: &FileDiff, dark: bool) -> LineSpans {
    let language = syntax::language_for_path(&file.path);
    let mut out: LineSpans = file
        .hunks
        .iter()
        .map(|h| vec![Vec::new(); h.lines.len()])
        .collect();
    if language == "text" || file.binary {
        return out;
    }
    for side in [DiffLineKind::Addition, DiffLineKind::Deletion] {
        let mut text = String::new();
        let mut index: Vec<(usize, usize)> = Vec::new();
        for (hi, hunk) in file.hunks.iter().enumerate() {
            for (li, line) in hunk.lines.iter().enumerate() {
                if line.kind == DiffLineKind::Context || line.kind == side {
                    if !index.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&line.text);
                    index.push((hi, li));
                }
            }
        }
        let lines = syntax::highlight_lines(&text, language, dark);
        for ((hi, li), spans) in index.into_iter().zip(lines) {
            let kind = file.hunks[hi].lines[li].kind;
            // Context lines take the new side's spans.
            if kind == side || (kind == DiffLineKind::Context && side == DiffLineKind::Addition) {
                out[hi][li] = spans;
            }
        }
    }
    out
}

/// A file's full text for the viewer: from disk when the folder exists, else
/// rebuilt from the recorded patches (new side of every hunk).
pub fn read_file(cwd: &Path, rel: &str, patches: &[RecordedPatch]) -> Result<String, String> {
    if cwd.is_dir() {
        let path = cwd.join(rel);
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        if bytes.len() > 1024 * 1024 {
            return Ok(String::from_utf8_lossy(&bytes[..1024 * 1024]).into_owned());
        }
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    let mut lines: Vec<(u32, String)> = Vec::new();
    for patch in patches {
        for file in parse_unified(&patch.diff) {
            if file.path != rel {
                continue;
            }
            for hunk in &file.hunks {
                for line in &hunk.lines {
                    if let Some(n) = line.new_lineno {
                        match lines.iter_mut().find(|(k, _)| *k == n) {
                            Some(existing) => existing.1 = line.text.clone(),
                            None => lines.push((n, line.text.clone())),
                        }
                    }
                }
            }
        }
    }
    if lines.is_empty() {
        return Err("The file could not be loaded.".into());
    }
    lines.sort_by_key(|(n, _)| *n);
    Ok(lines
        .into_iter()
        .map(|(_, t)| t)
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Workspace files for the file tree: `git ls-files` (tracked + untracked, not
/// ignored), else a bounded directory walk, else the recorded patches' paths.
pub fn list_files(cwd: &Path, patches: &[RecordedPatch]) -> Vec<String> {
    if cwd.is_dir() {
        if let Ok(out) = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args([
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
            ])
            .output()
            && out.status.success()
        {
            let mut files: Vec<String> = out
                .stdout
                .split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect();
            files.sort();
            files.dedup();
            return files;
        }
        let mut files = Vec::new();
        walk(cwd, cwd, &mut files);
        files.sort();
        return files;
    }
    let mut files: Vec<String> = patches
        .iter()
        .flat_map(|p| parse_unified(&p.diff))
        .filter(|f| f.kind != tern_git::DiffFile::Deleted)
        .map(|f| f.path)
        .collect();
    files.sort();
    files.dedup();
    files
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    if out.len() >= 10_000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && name != ".gitignore" {
            continue;
        }
        if matches!(name.as_ref(), "node_modules" | "target") {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().into_owned());
        }
    }
}

/// Branch names for the base-ref picker: (name, has_local, has_remote), head
/// excluded, at most 100.
pub fn list_refs(cwd: &Path) -> Vec<(String, bool, bool)> {
    let Ok(branches) = tern_git::refs::list_branches(cwd) else {
        return Vec::new();
    };
    let mut out: Vec<(String, bool, bool)> = Vec::new();
    for b in branches {
        if b.is_current {
            continue;
        }
        let name = if b.is_remote {
            b.name
                .split_once('/')
                .map(|(_, n)| n.to_string())
                .unwrap_or(b.name.clone())
        } else {
            b.name.clone()
        };
        if name == "HEAD" {
            continue;
        }
        match out.iter_mut().find(|(n, _, _)| *n == name) {
            Some(entry) => {
                if b.is_remote {
                    entry.2 = true;
                } else {
                    entry.1 = true;
                }
            }
            None => out.push((name, !b.is_remote, b.is_remote)),
        }
        if out.len() >= 100 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(item: &str, run: &str, diff: &str) -> RecordedPatch {
        RecordedPatch {
            item_id: item.into(),
            run_id: Some(run.into()),
            diff: diff.into(),
        }
    }

    fn request(selection: Selection, patches: Vec<RecordedPatch>) -> Request {
        Request {
            cwd: PathBuf::from("/nonexistent/tern-demo"),
            thread_id: "t".into(),
            selection,
            ignore_whitespace: true,
            turn_refs: None,
            patches,
            turn_run_id: Some("r2".into()),
            dark: true,
        }
    }

    const A1: &str = "--- a/b.py\n+++ b/b.py\n@@ -1,2 +1,3 @@\n+x = 1\n a\n b\n";
    const A2: &str = "--- a/b.py\n+++ b/b.py\n@@ -10,2 +11,2 @@\n-c\n+C\n d\n";
    const B: &str = "--- a/a.py\n+++ b/a.py\n@@ -1 +1 @@\n-q\n+Q\n";

    #[test]
    fn demo_fallback_merges_files_and_sorts() {
        let req = request(
            Selection::default(),
            vec![
                patch("i1", "r1", A1),
                patch("i2", "r2", A2),
                patch("i3", "r2", B),
            ],
        );
        let loaded = load(&req);
        assert!(loaded.demo);
        assert_eq!(loaded.files.len(), 2);
        assert_eq!(loaded.files[0].path, "a.py");
        assert_eq!(loaded.files[1].hunks.len(), 2);
        assert_eq!(loaded.files[1].additions, 2);
        assert_eq!(loaded.spans.len(), 2);
        // `x = 1` got syntax spans on the new side.
        assert!(!loaded.spans[1][0][0].is_empty());
    }

    #[test]
    fn demo_turn_and_tool_call_filter() {
        let patches = vec![
            patch("i1", "r1", A1),
            patch("i2", "r2", A2),
            patch("i3", "r2", B),
        ];
        let turn = load(&request(
            Selection::Turn {
                checkpoint_id: "c".into(),
            },
            patches.clone(),
        ));
        assert_eq!(turn.files.len(), 2);
        assert_eq!(turn.files[1].hunks.len(), 1);
        let tool = load(&request(
            Selection::ToolCall {
                item_id: "i1".into(),
            },
            patches,
        ));
        assert_eq!(tool.files.len(), 1);
    }

    #[test]
    fn rebuilds_file_from_patches() {
        let text = read_file(
            Path::new("/nonexistent"),
            "b.py",
            &[patch("i1", "r1", A1), patch("i2", "r2", A2)],
        )
        .unwrap();
        assert_eq!(text, "x = 1\na\nb\nC\nd");
    }

    #[test]
    fn loads_a_real_repo() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(root.join("notes.py"), "import json\nimport sys\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "init"]);
        std::fs::write(
            root.join("notes.py"),
            "\"\"\"doc\"\"\"\nimport json\nimport sys\n",
        )
        .unwrap();
        std::fs::write(root.join("new.txt"), "hello\n").unwrap();
        let mut req = request(Selection::Uncommitted, Vec::new());
        req.cwd = root.to_path_buf();
        let loaded = load(&req);
        assert!(loaded.error.is_none(), "{:?}", loaded.error);
        assert!(!loaded.demo);
        let paths: Vec<_> = loaded.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["new.txt", "notes.py"]);
        assert_eq!(loaded.files[1].additions, 1);
        assert_eq!(loaded.head_ref.as_deref(), Some("main"));
        let mut req = request(Selection::default(), Vec::new());
        req.cwd = root.to_path_buf();
        let branch = load(&req);
        assert!(branch.error.is_none(), "{:?}", branch.error);
        assert_eq!(branch.files.len(), 2);
        let files = list_files(root, &[]);
        assert_eq!(files, vec!["new.txt", "notes.py"]);
    }
}

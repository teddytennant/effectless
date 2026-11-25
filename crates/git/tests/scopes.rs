mod common;

use common::{empty_repo, repo};
use tern_core::DiffScope;
use tern_git::{DiffFile, DiffOptions, checkpoint, scope_diff, scope_files, scope_patch};

fn no_refs(_: &str) -> Option<(String, String)> {
    None
}

fn paths(files: &[tern_git::FileDiff]) -> Vec<(String, DiffFile)> {
    files.iter().map(|f| (f.path.clone(), f.kind)).collect()
}

#[test]
fn uncommitted_includes_staged_unstaged_and_untracked() {
    let r = repo();
    r.write("staged.txt", "s\n");
    r.commit("s");
    r.write("staged.txt", "s2\n");
    r.git(&["add", "staged.txt"]);
    r.write("README.md", "hello\nworld\n");
    r.write("dir/untracked file.txt", "u\n");
    r.write(".gitignore", "*.tmp\n");
    r.write("x.tmp", "ignored\n");
    let status_before = r.git(&["status", "--porcelain"]);

    let files = scope_diff(&r.path, &DiffScope::Uncommitted, &no_refs).unwrap();
    assert_eq!(
        paths(&files),
        vec![
            (".gitignore".into(), DiffFile::Added),
            ("README.md".into(), DiffFile::Modified),
            ("dir/untracked file.txt".into(), DiffFile::Added),
            ("staged.txt".into(), DiffFile::Modified),
        ]
    );
    let untracked = &files[2];
    assert_eq!((untracked.additions, untracked.deletions), (1, 0));
    // The review index is a copy: the user's staging is unchanged.
    assert_eq!(r.git(&["status", "--porcelain"]), status_before);

    let (source, stats) = scope_files(
        &r.path,
        &DiffScope::Uncommitted,
        &no_refs,
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(source.title, "Uncommitted");
    assert!(source.to.is_none());
    assert_eq!(stats.len(), 4);
    assert_eq!(stats[2].kind, DiffFile::Added);

    // One file only, as lazy loading asks for it.
    let one = scope_patch(
        &r.path,
        &DiffScope::Uncommitted,
        &no_refs,
        &DiffOptions {
            paths: vec!["dir/untracked file.txt".into()],
            ..Default::default()
        },
    )
    .unwrap();
    let parsed = tern_git::diff::parse_unified(&one.patch.text);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].path, "dir/untracked file.txt");
}

#[test]
fn uncommitted_on_unborn_branch() {
    let r = empty_repo();
    r.write("a.txt", "a\n");
    let files = scope_diff(&r.path, &DiffScope::Uncommitted, &no_refs).unwrap();
    assert_eq!(paths(&files), vec![("a.txt".into(), DiffFile::Added)]);
}

#[test]
fn ignore_whitespace_option() {
    let r = repo();
    r.write("README.md", "hello  \n");
    let ignored = scope_diff(&r.path, &DiffScope::Uncommitted, &no_refs).unwrap();
    assert!(ignored.iter().all(|f| f.hunks.is_empty()));
    let shown = scope_patch(
        &r.path,
        &DiffScope::Uncommitted,
        &no_refs,
        &DiffOptions {
            ignore_whitespace: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(shown.patch.text.contains("+hello  "));
}

#[test]
fn branch_scope_diffs_against_merge_base_with_default_branch() {
    let r = repo();
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("feature.txt", "f\n");
    r.commit("feature work");
    r.git(&["checkout", "-q", "main"]);
    r.write("main-only.txt", "m\n");
    r.commit("main moves on");
    r.git(&["checkout", "-q", "feature"]);
    r.write("README.md", "hello\nedited\n");
    r.write("wip.txt", "w\n");

    let files = scope_diff(&r.path, &DiffScope::Branch { base_ref: None }, &no_refs).unwrap();
    // Committed + uncommitted + untracked, but not main's later commit.
    assert_eq!(
        paths(&files),
        vec![
            ("README.md".into(), DiffFile::Modified),
            ("feature.txt".into(), DiffFile::Added),
            ("wip.txt".into(), DiffFile::Added),
        ]
    );
    let (source, _) = scope_files(
        &r.path,
        &DiffScope::Branch { base_ref: None },
        &no_refs,
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(source.title, "Changes");
    assert_eq!(source.base_ref.as_deref(), Some("main"));

    let commits = tern_git::review::branch_commits(&r.path, None).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].subject, "feature work");

    // Explicit base.
    let p = scope_patch(
        &r.path,
        &DiffScope::Branch { base_ref: None },
        &no_refs,
        &DiffOptions {
            base_ref: Some("main".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(p.source.title, "Changes vs main");
}

#[test]
fn branch_scope_on_default_branch_uses_its_upstream() {
    let r = repo();
    r.add_origin();
    // On main with no remote ahead: only uncommitted work.
    r.write("README.md", "hello\nlocal\n");
    let files = scope_diff(&r.path, &DiffScope::Branch { base_ref: None }, &no_refs).unwrap();
    assert_eq!(
        paths(&files),
        vec![("README.md".into(), DiffFile::Modified)]
    );
    // An unpushed commit on main shows up against origin/main.
    r.write("unpushed.txt", "u\n");
    r.commit("unpushed");
    let files = scope_diff(&r.path, &DiffScope::Branch { base_ref: None }, &no_refs).unwrap();
    assert_eq!(
        paths(&files),
        vec![
            ("README.md".into(), DiffFile::Modified),
            ("unpushed.txt".into(), DiffFile::Added),
        ]
    );
}

#[test]
fn branch_scope_without_any_base_is_uncommitted() {
    let r = repo();
    r.git(&["branch", "-m", "trunk"]);
    r.write("README.md", "changed\n");
    let files = scope_diff(&r.path, &DiffScope::Branch { base_ref: None }, &no_refs).unwrap();
    assert_eq!(
        paths(&files),
        vec![("README.md".into(), DiffFile::Modified)]
    );
}

#[test]
fn commit_scope() {
    let r = repo();
    r.write("keep.txt", "1\n2\n3\n4\n5\n6\n");
    let root_parent = r.commit("keep");
    r.git(&["mv", "keep.txt", "moved.txt"]);
    r.write("moved.txt", "1\n2\n3\n4\n5\n6\n7\n");
    let sha = r.commit("move it");

    let files = scope_diff(&r.path, &DiffScope::Commit(sha.clone()), &no_refs).unwrap();
    assert_eq!(paths(&files), vec![("moved.txt".into(), DiffFile::Renamed)]);
    assert_eq!(files[0].old_path.as_deref(), Some("keep.txt"));
    assert_eq!((files[0].additions, files[0].deletions), (1, 0));

    let p = scope_patch(
        &r.path,
        &DiffScope::Commit(sha.clone()),
        &no_refs,
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(p.source.title, "move it");
    assert_eq!(p.source.base_ref.as_deref(), Some(root_parent.as_str()));

    // The root commit diffs against the empty tree.
    let root = r.git(&["rev-list", "--max-parents=0", "HEAD"]);
    let files = scope_diff(&r.path, &DiffScope::Commit(root.trim().into()), &no_refs).unwrap();
    assert_eq!(paths(&files), vec![("README.md".into(), DiffFile::Added)]);

    assert!(scope_diff(&r.path, &DiffScope::Commit("HEAD".into()), &no_refs).is_err());
    assert!(scope_diff(&r.path, &DiffScope::Commit("0".repeat(40)), &no_refs).is_err());
}

#[test]
fn turn_scope_uses_checkpoint_refs() {
    let r = repo();
    let ref0 = checkpoint::capture(&r.path, "thread", 0).unwrap();
    r.write("README.md", "hello\nagent\n");
    r.write("agent.rs", "fn main() {}\n");
    let ref1 = checkpoint::capture(&r.path, "thread", 1).unwrap();
    let refs = |id: &str| (id == "cp-1").then(|| (ref0.clone(), ref1.clone()));

    let files = scope_diff(
        &r.path,
        &DiffScope::Turn {
            checkpoint_id: "cp-1".into(),
        },
        &refs,
    )
    .unwrap();
    assert_eq!(
        paths(&files),
        vec![
            ("README.md".into(), DiffFile::Modified),
            ("agent.rs".into(), DiffFile::Added),
        ]
    );
    // Work after the checkpoint isn't part of the turn.
    r.write("later.txt", "x\n");
    let again = scope_diff(
        &r.path,
        &DiffScope::Turn {
            checkpoint_id: "cp-1".into(),
        },
        &refs,
    )
    .unwrap();
    assert_eq!(again.len(), 2);

    assert!(
        scope_diff(
            &r.path,
            &DiffScope::Turn {
                checkpoint_id: "missing".into()
            },
            &refs
        )
        .is_err()
    );
}

#[test]
fn patch_cap_truncates_with_marker() {
    let r = repo();
    r.write("big.txt", &"line\n".repeat(1000));
    let p = scope_patch(
        &r.path,
        &DiffScope::Uncommitted,
        &no_refs,
        &DiffOptions {
            max_bytes: Some(200),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(p.patch.truncated);
    assert!(p.patch.text.ends_with("\n\n[truncated]"));
    // The partial patch still parses.
    let files = tern_git::diff::parse_unified(&p.patch.text);
    assert_eq!(files.len(), 1);
}

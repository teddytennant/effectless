mod common;

use common::{empty_repo, repo};
use tern_git::DiffFile;
use tern_git::checkpoint;
use tern_git::diff::parse_unified;

fn kinds(files: &[tern_core::CheckpointFileSummary]) -> Vec<(String, String, u32, u32)> {
    files
        .iter()
        .map(|f| (f.path.clone(), f.kind.clone(), f.additions, f.deletions))
        .collect()
}

#[test]
fn ref_names_use_thread_hash() {
    let r = checkpoint::checkpoint_ref("thread-1", 3);
    let key = checkpoint::thread_key("thread-1");
    assert_eq!(r, format!("refs/tern/checkpoints/{key}/ordinal/3"));
    // base64url of 32 hex chars, no padding.
    assert_eq!(key.len(), 43);
    assert!(
        key.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
    assert_ne!(key, checkpoint::thread_key("thread-2"));
}

#[test]
fn capture_summarize_restore_round_trip() {
    let r = repo();
    r.write(".gitignore", "*.log\n");
    r.write("a.txt", "one\ntwo\nthree\n");
    r.write("b.txt", "bee\n");
    let head = r.commit("files");

    let ref0 = checkpoint::capture(&r.path, "t", 0).unwrap();
    assert!(checkpoint::has_ref(&r.path, &ref0).unwrap());

    r.write("a.txt", "one\nTWO\nthree\nfour\n");
    std::fs::remove_file(r.path.join("b.txt")).unwrap();
    r.write("new file.txt", "fresh\n");
    r.write("debug.log", "ignored\n");
    r.git(&["add", "a.txt"]);
    let status_before = r.git(&["status", "--porcelain"]);
    let staged_before = r.git(&["diff", "--cached", "--name-only"]);

    let ref1 = checkpoint::capture(&r.path, "t", 1).unwrap();

    // Capturing doesn't touch HEAD, the index or the working tree.
    assert_eq!(r.git(&["rev-parse", "HEAD"]).trim(), head);
    assert_eq!(r.git(&["status", "--porcelain"]), status_before);
    assert_eq!(r.git(&["diff", "--cached", "--name-only"]), staged_before);

    // The checkpoint commit has HEAD as its first parent and the Tern identity.
    let parents = r.git(&["log", "-1", "--format=%P %an", &ref1]);
    assert_eq!(parents.trim(), format!("{head} Tern"));

    let files = checkpoint::summarize(&r.path, &ref0, &ref1).unwrap();
    assert_eq!(
        kinds(&files),
        vec![
            ("a.txt".into(), "modified".into(), 2, 1),
            ("b.txt".into(), "deleted".into(), 0, 1),
            ("new file.txt".into(), "added".into(), 1, 0),
        ]
    );
    let totals = tern_git::totals(&files);
    assert_eq!(
        (totals.count, totals.additions, totals.deletions),
        (3, 3, 2)
    );

    let patch = checkpoint::turn_patch(&r.path, &ref0, &ref1, true).unwrap();
    assert!(!patch.truncated);
    let parsed = parse_unified(&patch.text);
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[2].path, "new file.txt");
    assert_eq!(parsed[2].kind, DiffFile::Added);
    assert!(
        checkpoint::turn_patch(&r.path, &ref1, &ref1, true)
            .unwrap()
            .text
            .is_empty()
    );

    // Back to the baseline: b.txt returns, the untracked file goes, ignored files stay,
    // HEAD stays, nothing is left staged.
    checkpoint::restore(&r.path, &ref0).unwrap();
    assert_eq!(r.read("a.txt"), "one\ntwo\nthree\n");
    assert_eq!(r.read("b.txt"), "bee\n");
    assert!(!r.path.join("new file.txt").exists());
    assert!(r.path.join("debug.log").exists());
    assert_eq!(r.git(&["rev-parse", "HEAD"]).trim(), head);
    assert_eq!(r.git(&["status", "--porcelain"]), "");

    // And forward again; the untracked file comes back untracked.
    checkpoint::restore(&r.path, &ref1).unwrap();
    assert_eq!(r.read("a.txt"), "one\nTWO\nthree\nfour\n");
    assert!(!r.path.join("b.txt").exists());
    assert_eq!(r.read("new file.txt"), "fresh\n");
    let status = r.git(&["status", "--porcelain"]);
    assert!(status.contains("?? \"new file.txt\""), "{status}");
    assert!(status.contains(" M a.txt"), "{status}");
    assert!(r.git(&["diff", "--cached", "--name-only"]).is_empty());

    assert_eq!(checkpoint::list_refs(&r.path, "t").unwrap().len(), 2);
    assert_eq!(
        checkpoint::refs_after(&r.path, "t", 0).unwrap(),
        vec![ref1.clone()]
    );
    checkpoint::delete_all(&r.path, "t").unwrap();
    assert!(checkpoint::list_refs(&r.path, "t").unwrap().is_empty());
    assert!(!checkpoint::has_ref(&r.path, &ref0).unwrap());
    assert!(checkpoint::restore(&r.path, &ref0).is_err());
}

#[test]
fn restore_removes_files_hidden_by_a_new_gitignore() {
    let r = repo();
    let ref0 = checkpoint::capture(&r.path, "t", 0).unwrap();
    // The turn adds a .gitignore and a file it hides.
    r.write(".gitignore", "secret.txt\n");
    r.write("secret.txt", "s\n");
    checkpoint::capture(&r.path, "t", 1).unwrap();
    checkpoint::restore(&r.path, &ref0).unwrap();
    assert!(!r.path.join(".gitignore").exists());
    // Removing .gitignore exposed secret.txt, which isn't in the baseline: cleaned too.
    assert!(!r.path.join("secret.txt").exists());
}

#[test]
fn capture_on_unborn_branch() {
    let r = empty_repo();
    let ref0 = checkpoint::capture(&r.path, "t", 0).unwrap();
    r.write("first.txt", "1\n");
    let ref1 = checkpoint::capture(&r.path, "t", 1).unwrap();
    let files = checkpoint::summarize(&r.path, &ref0, &ref1).unwrap();
    assert_eq!(
        kinds(&files),
        vec![("first.txt".into(), "added".into(), 1, 0)]
    );
    // A parentless commit.
    assert_eq!(r.git(&["log", "-1", "--format=%P", &ref1]).trim(), "");
    checkpoint::restore(&r.path, &ref0).unwrap();
    assert!(!r.path.join("first.txt").exists());
}

#[test]
fn capture_is_scoped_to_cwd() {
    let r = repo();
    r.write("sub/x.txt", "x\n");
    r.commit("sub");
    r.write("README.md", "changed outside\n");
    r.write("sub/x.txt", "changed inside\n");
    let sub = r.path.join("sub");
    let git_ref = checkpoint::capture(&sub, "t", 1).unwrap();
    assert_eq!(r.git(&["show", &format!("{git_ref}:README.md")]), "hello\n");
    assert_eq!(
        r.git(&["show", &format!("{git_ref}:sub/x.txt")]),
        "changed inside\n"
    );
}

#[test]
fn summary_reports_renames() {
    let r = repo();
    r.write("old name.txt", "a\nb\nc\nd\ne\n");
    r.commit("old");
    let ref0 = checkpoint::capture(&r.path, "t", 0).unwrap();
    std::fs::rename(r.path.join("old name.txt"), r.path.join("new name.txt")).unwrap();
    let ref1 = checkpoint::capture(&r.path, "t", 1).unwrap();
    let files = checkpoint::summarize(&r.path, &ref0, &ref1).unwrap();
    assert_eq!(
        kinds(&files),
        vec![("new name.txt".into(), "renamed".into(), 0, 0)]
    );
}

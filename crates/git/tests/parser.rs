mod common;

use common::repo;
use tern_core::DiffScope;
use tern_git::diff::{parse_numstat, parse_unified};
use tern_git::{DiffFile, DiffLineKind, scope_diff};

#[test]
fn modified_file_line_numbers() {
    let text = "\
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,4 +10,5 @@ fn header()
 ctx
-old
+new
+added

 tail
";
    let files = parse_unified(text);
    assert_eq!(files.len(), 1);
    let f = &files[0];
    assert_eq!(f.path, "src/lib.rs");
    assert_eq!(f.kind, DiffFile::Modified);
    assert_eq!((f.additions, f.deletions), (2, 1));
    let h = &f.hunks[0];
    assert_eq!(h.header, "@@ -10,4 +10,5 @@ fn header()");
    assert_eq!((h.old_start, h.new_start), (10, 10));
    let got: Vec<_> = h
        .lines
        .iter()
        .map(|l| (l.kind, l.old_lineno, l.new_lineno, l.text.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (DiffLineKind::Context, Some(10), Some(10), "ctx"),
            (DiffLineKind::Deletion, Some(11), None, "old"),
            (DiffLineKind::Addition, None, Some(11), "new"),
            (DiffLineKind::Addition, None, Some(12), "added"),
            // A blank context line printed as "" (diff.suppressBlankEmpty).
            (DiffLineKind::Context, Some(12), Some(13), ""),
            (DiffLineKind::Context, Some(13), Some(14), "tail"),
        ]
    );
}

#[test]
fn removed_lines_that_look_like_headers() {
    // Deleting "-- x" and adding "++ y" prints "--- x" / "+++ y" inside the hunk.
    let text = "\
diff --git a/notes.md b/notes.md
--- a/notes.md
+++ b/notes.md
@@ -1,2 +1,2 @@
--- x
+++ y
 keep
diff --git a/other b/other
--- a/other
+++ b/other
@@ -1 +1 @@
-a
+b
";
    let files = parse_unified(text);
    assert_eq!(files.len(), 2);
    let lines = &files[0].hunks[0].lines;
    assert_eq!(lines[0].kind, DiffLineKind::Deletion);
    assert_eq!(lines[0].text, "-- x");
    assert_eq!(lines[1].kind, DiffLineKind::Addition);
    assert_eq!(lines[1].text, "++ y");
    assert_eq!(files[0].path, "notes.md");
    assert_eq!(files[1].path, "other");
}

#[test]
fn new_deleted_binary_rename_mode_and_no_newline() {
    let text = "\
diff --git a/bin.dat b/bin.dat
index bdc955b..8835708 100644
Binary files a/bin.dat and b/bin.dat differ
diff --git a/img.png b/img.png
new file mode 100644
index 0000000..1a9d148
Binary files /dev/null and b/img.png differ
diff --git a/nonl.txt b/nonl.txt
new file mode 100644
index 0000000..1a9d148
--- /dev/null
+++ b/nonl.txt
@@ -0,0 +1 @@
+nonl
\\ No newline at end of file
diff --git a/ren.txt b/ren new.txt
similarity index 100%
rename from ren.txt
rename to ren new.txt
diff --git a/edit old.txt b/edit new.txt
similarity index 80%
rename from edit old.txt
rename to edit new.txt
index 1..2 100644
--- a/edit old.txt\t
+++ b/edit new.txt\t
@@ -1,2 +1,2 @@
 same
-before
\\ No newline at end of file
+after
\\ No newline at end of file
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index 975fbec..0000000
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-y
diff --git a/run.sh b/run.sh
old mode 100644
new mode 100755
";
    let files = parse_unified(text);
    let summary: Vec<_> = files
        .iter()
        .map(|f| {
            (
                f.path.as_str(),
                f.old_path.as_deref(),
                f.kind,
                f.binary,
                f.additions,
                f.deletions,
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("bin.dat", None, DiffFile::Modified, true, 0, 0),
            ("img.png", None, DiffFile::Added, true, 0, 0),
            ("nonl.txt", None, DiffFile::Added, false, 1, 0),
            (
                "ren new.txt",
                Some("ren.txt"),
                DiffFile::Renamed,
                false,
                0,
                0
            ),
            (
                "edit new.txt",
                Some("edit old.txt"),
                DiffFile::Renamed,
                false,
                1,
                1
            ),
            ("gone.txt", None, DiffFile::Deleted, false, 0, 1),
            ("run.sh", None, DiffFile::Modified, false, 0, 0),
        ]
    );
    // The no-newline markers are not lines.
    assert_eq!(files[2].hunks[0].lines.len(), 1);
    assert_eq!(files[4].hunks[0].lines.len(), 3);
    assert_eq!(files[4].hunks[0].lines[2].new_lineno, Some(2));
}

#[test]
fn quoted_paths() {
    let text = r#"diff --git "a/q\"uote.txt" "b/q\"uote.txt"
index 587be6b..2794641 100644
--- "a/q\"uote.txt"
+++ "b/q\"uote.txt"
@@ -1 +1,2 @@
 x
+c
diff --git "a/tab\tx.txt" "b/tab\tx.txt"
deleted file mode 100644
--- "a/tab\tx.txt"
+++ /dev/null
@@ -1 +0,0 @@
-y
diff --git "a/\303\274n\303\257.txt" "b/\303\274n\303\257.txt"
--- "a/\303\274n\303\257.txt"
+++ "b/\303\274n\303\257.txt"
@@ -1 +1 @@
-z
+w
diff --git a/plain "b/back\\slash"
similarity index 100%
rename from plain
rename to "back\\slash"
"#;
    let files = parse_unified(text);
    let names: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        names,
        vec!["q\"uote.txt", "tab\tx.txt", "ünï.txt", "back\\slash"]
    );
    assert_eq!(files[1].kind, DiffFile::Deleted);
    assert_eq!(files[3].old_path.as_deref(), Some("plain"));
    assert_eq!(files[3].kind, DiffFile::Renamed);
}

#[test]
fn plain_unified_diff_without_git_headers() {
    let text = "\
--- a/one.txt\t2024-01-01 00:00:00
+++ b/one.txt\t2024-01-02 00:00:00
@@ -1 +1 @@
-a
+b
--- /dev/null
+++ b/two.txt
@@ -0,0 +1,2 @@
+x
+y
--- a/three.txt
+++ /dev/null
@@ -1 +0,0 @@
-z
";
    let files = parse_unified(text);
    let summary: Vec<_> = files
        .iter()
        .map(|f| (f.path.as_str(), f.kind, f.additions, f.deletions))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("one.txt", DiffFile::Modified, 1, 1),
            ("two.txt", DiffFile::Added, 2, 0),
            ("three.txt", DiffFile::Deleted, 0, 1),
        ]
    );
}

#[test]
fn empty_and_junk_input() {
    assert!(parse_unified("").is_empty());
    assert!(parse_unified("\n\n[truncated]").is_empty());
}

#[test]
fn numstat_with_raw_records() {
    let raw: Vec<u8> = [
        ":100644 100644 bdc955b 8835708 M",
        "bin.dat",
        ":000000 100644 0000000 1a9d148 A",
        "new.txt",
        ":100644 100644 bd6bb36 bd6bb36 R100",
        "old.txt",
        "new name.txt",
        ":100644 000000 975fbec 0000000 D",
        "tab\tx.txt",
        "-\t-\tbin.dat",
        "2\t0\tnew.txt",
        "0\t0\t",
        "old.txt",
        "new name.txt",
        "0\t1\ttab\tx.txt",
    ]
    .iter()
    .flat_map(|s| s.bytes().chain([0]))
    .collect();
    let stats = parse_numstat(&raw);
    let got: Vec<_> = stats
        .iter()
        .map(|s| {
            (
                s.path.as_str(),
                s.old_path.as_deref(),
                s.kind,
                s.binary,
                s.additions,
                s.deletions,
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            ("bin.dat", None, DiffFile::Modified, true, 0, 0),
            (
                "new name.txt",
                Some("old.txt"),
                DiffFile::Renamed,
                false,
                0,
                0
            ),
            ("new.txt", None, DiffFile::Added, false, 2, 0),
            ("tab\tx.txt", None, DiffFile::Deleted, false, 0, 1),
        ]
    );
}

/// The same edge cases, produced by real git through `scope_diff`.
#[test]
fn real_git_output_round_trip() {
    let r = repo();
    r.write("with space.txt", "a\n");
    r.write("q\"uote.txt", "x\n");
    r.write("tab\tx.txt", "y\n");
    r.write("ünï.txt", "z\n");
    std::fs::write(r.path.join("bin.dat"), [0u8, 1, 2]).unwrap();
    r.write("ren.txt", "keep\nme\nplease\nnow\n");
    r.write("script.sh", "echo\n");
    r.commit("all");

    r.write("with space.txt", "b\n");
    r.git(&["mv", "ren.txt", "ren new.txt"]);
    r.write("q\"uote.txt", "x\nc\n");
    std::fs::write(r.path.join("bin.dat"), [0u8, 9, 9]).unwrap();
    r.write("nonl.txt", "nonl");
    std::fs::remove_file(r.path.join("tab\tx.txt")).unwrap();
    r.write("ünï.txt", "z\nw");
    {
        use std::os::unix::fs::PermissionsExt;
        let p = r.path.join("script.sh");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let files = scope_diff(&r.path, &DiffScope::Uncommitted, &|_| None).unwrap();
    let summary: Vec<_> = files
        .iter()
        .map(|f| (f.path.as_str(), f.old_path.as_deref(), f.kind, f.binary))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("bin.dat", None, DiffFile::Modified, true),
            ("nonl.txt", None, DiffFile::Added, false),
            ("q\"uote.txt", None, DiffFile::Modified, false),
            ("ren new.txt", Some("ren.txt"), DiffFile::Renamed, false),
            ("script.sh", None, DiffFile::Modified, false),
            ("tab\tx.txt", None, DiffFile::Deleted, false),
            ("with space.txt", None, DiffFile::Modified, false),
            ("ünï.txt", None, DiffFile::Modified, false),
        ]
    );
    let unicode = files.last().unwrap();
    assert_eq!((unicode.additions, unicode.deletions), (1, 0));
}

//! Row derivation for the diff code view (spec 05 §2.6): one flat list of rows
//! (file headers, hunk separators, code lines) that the virtualized list renders
//! one row per item, plus the small helpers the header needs (file order, stats,
//! compact numbers, test-file detection).

use std::cmp::Ordering;

use tern_git::{DiffFile, DiffHunk, DiffLineKind, FileDiff};

/// One row of the code view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// The 32px file header.
    Header { file: usize },
    /// A 24px separator for unchanged lines before hunk `before` (`hunks.len()` =
    /// after the last hunk).
    Gap {
        file: usize,
        before: usize,
        lines: GapLines,
    },
    /// Unified view: one diff line.
    Line {
        file: usize,
        hunk: usize,
        line: usize,
    },
    /// Split view: old line on the left, new line on the right; `None` is the
    /// striped buffer.
    Pair {
        file: usize,
        hunk: usize,
        left: Option<usize>,
        right: Option<usize>,
    },
    /// 8px bottom padding after an expanded file.
    Footer { file: usize },
}

impl Row {
    pub fn file(&self) -> usize {
        match *self {
            Row::Header { file }
            | Row::Gap { file, .. }
            | Row::Line { file, .. }
            | Row::Pair { file, .. }
            | Row::Footer { file } => file,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapLines {
    /// "{n} unmodified line(s)".
    Known(u32),
    /// "More unchanged context may be available": the end of a file whose length
    /// the patch does not tell.
    Unknown,
}

impl GapLines {
    pub fn label(self) -> String {
        match self {
            GapLines::Known(1) => "1 unmodified line".into(),
            GapLines::Known(n) => format!("{n} unmodified lines"),
            GapLines::Unknown => "More unchanged context may be available".into(),
        }
    }
}

/// Git's default context. A last hunk with fewer trailing context lines reached
/// the end of the file.
const DEFAULT_CONTEXT: usize = 3;

fn new_count(hunk: &DiffHunk) -> u32 {
    hunk.lines
        .iter()
        .filter(|l| l.kind != DiffLineKind::Deletion)
        .count() as u32
}

fn trailing_context(hunk: &DiffHunk) -> usize {
    hunk.lines
        .iter()
        .rev()
        .take_while(|l| l.kind == DiffLineKind::Context)
        .count()
}

/// The separators of one file: (before hunk index, lines).
pub fn gaps(file: &FileDiff) -> Vec<(usize, GapLines)> {
    let mut out = Vec::new();
    if file.hunks.is_empty() || matches!(file.kind, DiffFile::Added | DiffFile::Deleted) {
        return out;
    }
    let first = &file.hunks[0];
    let start = first.new_start.max(first.old_start);
    if start > 1 {
        out.push((0, GapLines::Known(start - 1)));
    }
    for i in 1..file.hunks.len() {
        let prev = &file.hunks[i - 1];
        let cur = &file.hunks[i];
        let prev_end = prev.new_start + new_count(prev);
        if cur.new_start > prev_end {
            out.push((i, GapLines::Known(cur.new_start - prev_end)));
        }
    }
    let last = file.hunks.last().unwrap();
    if trailing_context(last) >= DEFAULT_CONTEXT {
        out.push((file.hunks.len(), GapLines::Unknown));
    }
    out
}

/// Pairs one hunk's lines for the split view: context lines sit on both sides, a
/// run of changes puts its deletions left and additions right, row by row.
pub fn split_pairs(hunk: &DiffHunk) -> Vec<(Option<usize>, Option<usize>)> {
    let mut out = Vec::new();
    let lines = &hunk.lines;
    let mut i = 0;
    while i < lines.len() {
        if lines[i].kind == DiffLineKind::Context {
            out.push((Some(i), Some(i)));
            i += 1;
            continue;
        }
        let mut dels = Vec::new();
        let mut adds = Vec::new();
        while i < lines.len() && lines[i].kind != DiffLineKind::Context {
            match lines[i].kind {
                DiffLineKind::Deletion => dels.push(i),
                _ => adds.push(i),
            }
            i += 1;
        }
        for k in 0..dels.len().max(adds.len()) {
            out.push((dels.get(k).copied(), adds.get(k).copied()));
        }
    }
    out
}

/// Builds the rows for `files`. `collapsed(i)` hides a file's body; `visible(i)`
/// leaves a file out entirely (hidden test files).
pub fn build_rows(
    files: &[FileDiff],
    split: bool,
    collapsed: impl Fn(usize) -> bool,
    visible: impl Fn(usize) -> bool,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for (fi, file) in files.iter().enumerate() {
        if !visible(fi) {
            continue;
        }
        rows.push(Row::Header { file: fi });
        if collapsed(fi) {
            continue;
        }
        let gaps = gaps(file);
        let gap_before = |h: usize| gaps.iter().find(|(b, _)| *b == h).map(|(_, l)| *l);
        for (hi, hunk) in file.hunks.iter().enumerate() {
            if let Some(lines) = gap_before(hi) {
                rows.push(Row::Gap {
                    file: fi,
                    before: hi,
                    lines,
                });
            }
            if split {
                for (left, right) in split_pairs(hunk) {
                    rows.push(Row::Pair {
                        file: fi,
                        hunk: hi,
                        left,
                        right,
                    });
                }
            } else {
                for li in 0..hunk.lines.len() {
                    rows.push(Row::Line {
                        file: fi,
                        hunk: hi,
                        line: li,
                    });
                }
            }
        }
        if let Some(lines) = gap_before(file.hunks.len()) {
            rows.push(Row::Gap {
                file: fi,
                before: file.hunks.len(),
                lines,
            });
        }
        rows.push(Row::Footer { file: fi });
    }
    rows
}

/// `Intl.Collator(undefined, {numeric: true, sensitivity: "base"})`, roughly:
/// case-insensitive, digit runs compared by value.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    bi.next();
                }
                let ta = na.trim_start_matches('0');
                let tb = nb.trim_start_matches('0');
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// Sorts files by path with the natural collator (not review order).
pub fn sort_files(files: &mut [FileDiff]) {
    files.sort_by(|a, b| natural_cmp(&a.path, &b.path));
}

/// (additions, deletions) over `files`.
pub fn totals<'a>(files: impl IntoIterator<Item = &'a FileDiff>) -> (u32, u32) {
    files
        .into_iter()
        .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions))
}

/// DiffStatLabel's compact numbers: <1000 as is, then `1.2k`, `12k`, `1.2m`, `1.2b`
/// (one decimal under 10, trailing `.0` dropped).
pub fn compact_number(n: u64) -> String {
    if n < 1000 {
        return n.to_string();
    }
    let (value, suffix) = if n < 999_500 {
        (n as f64 / 1e3, "k")
    } else if n < 999_500_000 {
        (n as f64 / 1e6, "m")
    } else {
        (n as f64 / 1e9, "b")
    };
    if value < 10. {
        let s = format!("{:.1}", (value * 10.).round() / 10.);
        let s = s.strip_suffix(".0").map(str::to_string).unwrap_or(s);
        if s == "10" {
            return format!("10{suffix}");
        }
        format!("{s}{suffix}")
    } else {
        format!("{}{suffix}", value.round() as u64)
    }
}

/// Test files for the "Hide test files" toggle.
pub fn is_test_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    let in_test_dir = lower.split('/').rev().skip(1).any(|segment| {
        matches!(
            segment,
            "test" | "tests" | "__tests__" | "spec" | "specs" | "__mocks__"
        )
    });
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    in_test_dir
        || name.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_tests")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || stem.ends_with("_spec")
}

/// Collapse identity: `prevPath\0path\0kind`, `\0{n}` on duplicates.
pub fn file_keys(files: &[FileDiff]) -> Vec<String> {
    let mut seen: std::collections::HashMap<String, usize> = Default::default();
    files
        .iter()
        .map(|f| {
            let base = format!(
                "{}\0{}\0{}",
                f.old_path.as_deref().unwrap_or(""),
                f.path,
                f.kind.as_str()
            );
            let n = seen.entry(base.clone()).or_insert(0);
            *n += 1;
            if *n == 1 {
                base
            } else {
                format!("{base}\0{}", *n - 1)
            }
        })
        .collect()
}

/// Gutter width in characters: 2ch left padding, the widest number, 1ch right.
pub fn gutter_chars(file: &FileDiff) -> usize {
    let max = file
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .map(|l| l.new_lineno.unwrap_or(0).max(l.old_lineno.unwrap_or(0)))
        .max()
        .unwrap_or(1);
    2 + max.max(1).to_string().len() + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use tern_git::diff::parse_unified;

    const TWO_HUNKS: &str = "diff --git a/x.py b/x.py\n--- a/x.py\n+++ b/x.py\n@@ -5,4 +5,5 @@\n a\n b\n+c\n d\n e\n@@ -20,3 +21,3 @@ def f():\n x\n-y\n+Y\n z\n";

    #[test]
    fn gaps_before_between_and_after() {
        let files = parse_unified(TWO_HUNKS);
        let g = gaps(&files[0]);
        assert_eq!(g[0], (0, GapLines::Known(4)));
        // hunk 1 ends at new line 5+5=10, next starts at 21.
        assert_eq!(g[1], (1, GapLines::Known(11)));
        // last hunk has 1 trailing context line: it reached the end of the file.
        assert_eq!(g.len(), 2);
        assert_eq!(GapLines::Known(1).label(), "1 unmodified line");
        assert_eq!(GapLines::Known(11).label(), "11 unmodified lines");
    }

    #[test]
    fn unknown_end_after_full_context() {
        let d = "--- a/n.py\n+++ b/n.py\n@@ -1,3 +1,5 @@\n+\"\"\"doc\"\"\"\n+\n import json\n import sys\n from x import y\n";
        let files = parse_unified(d);
        assert_eq!(gaps(&files[0]), vec![(1, GapLines::Unknown)]);
    }

    #[test]
    fn new_files_have_no_gaps() {
        let d = "diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+a\n+b\n";
        let files = parse_unified(d);
        assert!(gaps(&files[0]).is_empty());
    }

    #[test]
    fn rows_unified_and_collapsed() {
        let files = parse_unified(TWO_HUNKS);
        let rows = build_rows(&files, false, |_| false, |_| true);
        assert_eq!(rows[0], Row::Header { file: 0 });
        assert!(matches!(rows[1], Row::Gap { before: 0, .. }));
        assert_eq!(
            rows[2],
            Row::Line {
                file: 0,
                hunk: 0,
                line: 0
            }
        );
        assert_eq!(*rows.last().unwrap(), Row::Footer { file: 0 });
        // 1 header + 2 gaps + 5 + 4 lines + footer
        assert_eq!(rows.len(), 1 + 2 + 5 + 4 + 1);
        let rows = build_rows(&files, false, |_| true, |_| true);
        assert_eq!(rows, vec![Row::Header { file: 0 }]);
        assert!(build_rows(&files, false, |_| false, |_| false).is_empty());
    }

    #[test]
    fn split_pairs_changes_side_by_side() {
        let d = "--- a/x\n+++ b/x\n@@ -1,5 +1,4 @@\n a\n-b\n-c\n-d\n+B\n+C\n e\n";
        let files = parse_unified(d);
        let pairs = split_pairs(&files[0].hunks[0]);
        assert_eq!(
            pairs,
            vec![
                (Some(0), Some(0)),
                (Some(1), Some(4)),
                (Some(2), Some(5)),
                (Some(3), None),
                (Some(6), Some(6)),
            ]
        );
        let d = "--- a/x\n+++ b/x\n@@ -1,1 +1,3 @@\n a\n+b\n+c\n";
        let pairs = split_pairs(&parse_unified(d)[0].hunks[0]);
        assert_eq!(pairs[1], (None, Some(1)));
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["file10.rs", "File2.rs", "file1.rs", "a/b.rs", "B.rs"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            v,
            vec!["a/b.rs", "B.rs", "file1.rs", "File2.rs", "file10.rs"]
        );
    }

    #[test]
    fn compact_numbers() {
        assert_eq!(compact_number(999), "999");
        assert_eq!(compact_number(1000), "1k");
        assert_eq!(compact_number(1234), "1.2k");
        assert_eq!(compact_number(12_345), "12k");
        assert_eq!(compact_number(999_499), "999k");
        assert_eq!(compact_number(999_500), "1m");
        assert_eq!(compact_number(1_250_000), "1.3m");
        assert_eq!(compact_number(2_000_000_000), "2b");
    }

    #[test]
    fn test_files() {
        assert!(is_test_file("test_notes.py"));
        assert!(is_test_file("src/foo.test.ts"));
        assert!(is_test_file("src/foo.spec.tsx"));
        assert!(is_test_file("crates/x/tests/it.rs"));
        assert!(is_test_file("pkg/foo_test.go"));
        assert!(!is_test_file("src/testing.rs"));
        assert!(!is_test_file("notes.py"));
    }

    #[test]
    fn keys_disambiguate_duplicates() {
        let d = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n--- a/x\n+++ b/x\n@@ -9 +9 @@\n-a\n+b\n";
        let files = parse_unified(d);
        let keys = file_keys(&files);
        assert_eq!(keys.len(), 2);
        assert_ne!(keys[0], keys[1]);
        assert_eq!(gutter_chars(&files[0]), 4);
    }
}

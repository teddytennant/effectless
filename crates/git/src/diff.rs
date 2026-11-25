//! Unified diff model + parser (`git diff --no-color -M` output), and the `--raw --numstat -z`
//! parser the file lists use.
//!
//! The patch parser takes git's `diff --git` output with `a/`/`b/` prefixes (every call in this
//! crate forces them), and also plain `---`/`+++` unified diffs such as the patches providers
//! record for a tool call. Quoted paths (`"a/q\"uote"`, octal escapes for non-ASCII), paths with
//! spaces (git adds a trailing tab to their `---`/`+++` lines), renames, copies, binary files,
//! mode-only changes, new and deleted files and `\ No newline at end of file` markers are handled.
//! Anything it does not understand outside a hunk (the `[truncated]` marker, `index` lines) is
//! skipped.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiffHunk {
    pub header: String,
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffFile {
    Added,
    Deleted,
    Modified,
    Renamed,
}

impl DiffFile {
    /// The `CheckpointFileSummary::kind` string.
    pub fn as_str(self) -> &'static str {
        match self {
            DiffFile::Added => "added",
            DiffFile::Deleted => "deleted",
            DiffFile::Modified => "modified",
            DiffFile::Renamed => "renamed",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: DiffFile,
    pub binary: bool,
    pub additions: u32,
    pub deletions: u32,
    pub hunks: Vec<DiffHunk>,
}

impl FileDiff {
    fn new(path: String) -> Self {
        FileDiff {
            path,
            old_path: None,
            kind: DiffFile::Modified,
            binary: false,
            additions: 0,
            deletions: 0,
            hunks: Vec::new(),
        }
    }
}

struct Hunk {
    old_left: u32,
    new_left: u32,
    old_no: u32,
    new_no: u32,
}

pub fn parse_unified(text: &str) -> Vec<FileDiff> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut files: Vec<FileDiff> = Vec::new();
    let mut cur: Option<FileDiff> = None;
    // Set once a `---` line was seen for `cur`, so a second one starts a new plain-diff file.
    let mut saw_minus = false;
    let mut hunk: Option<Hunk> = None;

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;

        if let (Some(h), Some(file)) = (hunk.as_mut(), cur.as_mut())
            && (h.old_left > 0 || h.new_left > 0)
        {
            let (tag, body) = match line.as_bytes().first() {
                Some(b' ') => (Some(DiffLineKind::Context), &line[1..]),
                Some(b'+') => (Some(DiffLineKind::Addition), &line[1..]),
                Some(b'-') => (Some(DiffLineKind::Deletion), &line[1..]),
                Some(b'\\') => (None, ""),
                // diff.suppressBlankEmpty prints a blank context line as "".
                None if i < lines.len() => (Some(DiffLineKind::Context), ""),
                _ => {
                    hunk = None;
                    i -= 1;
                    continue;
                }
            };
            let Some(kind) = tag else { continue };
            let (old_lineno, new_lineno) = match kind {
                DiffLineKind::Context => {
                    let r = (Some(h.old_no), Some(h.new_no));
                    h.old_no += 1;
                    h.new_no += 1;
                    h.old_left = h.old_left.saturating_sub(1);
                    h.new_left = h.new_left.saturating_sub(1);
                    r
                }
                DiffLineKind::Addition => {
                    let r = (None, Some(h.new_no));
                    h.new_no += 1;
                    h.new_left = h.new_left.saturating_sub(1);
                    file.additions += 1;
                    r
                }
                DiffLineKind::Deletion => {
                    let r = (Some(h.old_no), None);
                    h.old_no += 1;
                    h.old_left = h.old_left.saturating_sub(1);
                    file.deletions += 1;
                    r
                }
            };
            if let Some(last) = file.hunks.last_mut() {
                last.lines.push(DiffLine {
                    kind,
                    old_lineno,
                    new_lineno,
                    text: body.to_string(),
                });
            }
            continue;
        }
        hunk = None;

        if line.starts_with('\\') {
            // "\ No newline at end of file" after the hunk's last line.
            continue;
        }

        if let Some(rest) = line.strip_prefix("diff --git ") {
            files.extend(cur.take());
            saw_minus = false;
            let (a, b) = split_git_header(rest);
            let mut f = FileDiff::new(b.clone().unwrap_or_else(|| a.clone().unwrap_or_default()));
            if let (Some(a), Some(b)) = (&a, &b)
                && a != b
            {
                f.old_path = Some(a.clone());
            }
            cur = Some(f);
            continue;
        }

        if let Some(rest) = line.strip_prefix("--- ") {
            let next_is_plus = lines.get(i).is_some_and(|l| l.starts_with("+++ "));
            let starts_plain = cur
                .as_ref()
                .is_none_or(|f| saw_minus || !f.hunks.is_empty());
            if next_is_plus && starts_plain {
                files.extend(cur.take());
                cur = Some(FileDiff::new(String::new()));
            }
            if let Some(f) = cur.as_mut() {
                saw_minus = true;
                match marker_path(rest, "a/") {
                    None => f.kind = DiffFile::Added,
                    Some(p) => {
                        if f.path.is_empty() {
                            f.path = p;
                        } else if f.kind == DiffFile::Modified && p != f.path {
                            f.old_path = Some(p);
                        }
                    }
                }
            }
            continue;
        }

        let Some(f) = cur.as_mut() else { continue };

        if let Some(rest) = line.strip_prefix("+++ ") {
            match marker_path(rest, "b/") {
                None => {
                    f.kind = DiffFile::Deleted;
                    if let Some(old) = f.old_path.take() {
                        f.path = old;
                    }
                }
                Some(p) => {
                    if f.path != p {
                        if f.old_path.is_none()
                            && !f.path.is_empty()
                            && f.kind == DiffFile::Modified
                        {
                            f.old_path = Some(f.path.clone());
                        }
                        f.path = p;
                    }
                }
            }
            if f.kind == DiffFile::Modified && f.old_path.is_some() {
                f.kind = DiffFile::Renamed;
            }
        } else if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some((old_start, old_len, new_start, new_len)) = parse_hunk_header(rest) {
                if f.kind == DiffFile::Modified && f.old_path.is_some() {
                    f.kind = DiffFile::Renamed;
                }
                f.hunks.push(DiffHunk {
                    header: line.to_string(),
                    old_start,
                    new_start,
                    lines: Vec::new(),
                });
                hunk = Some(Hunk {
                    old_left: old_len,
                    new_left: new_len,
                    old_no: old_start,
                    new_no: new_start,
                });
            }
        } else if line.starts_with("new file mode") {
            f.kind = DiffFile::Added;
            f.old_path = None;
        } else if line.starts_with("deleted file mode") {
            f.kind = DiffFile::Deleted;
            f.old_path = None;
        } else if let Some(p) = line
            .strip_prefix("rename from ")
            .or_else(|| line.strip_prefix("copy from "))
        {
            f.old_path = Some(unquote(p));
            f.kind = if line.starts_with("copy") {
                DiffFile::Added
            } else {
                DiffFile::Renamed
            };
        } else if let Some(p) = line
            .strip_prefix("rename to ")
            .or_else(|| line.strip_prefix("copy to "))
        {
            f.path = unquote(p);
        } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
            f.binary = true;
            if line.starts_with("Binary files /dev/null ") {
                f.kind = DiffFile::Added;
            } else if line.ends_with(" and /dev/null differ") {
                f.kind = DiffFile::Deleted;
            }
        }
    }
    files.extend(cur);
    files
}

/// `@@ -a[,b] +c[,d] @@ ...` -> (a, b, c, d).
fn parse_hunk_header(rest: &str) -> Option<(u32, u32, u32, u32)> {
    let mut parts = rest.split(' ');
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let range = |s: &str| -> Option<(u32, u32)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (a, b) = range(old)?;
    let (c, d) = range(new)?;
    Some((a, b, c, d))
}

/// Path from a `---`/`+++` line. None for /dev/null.
fn marker_path(rest: &str, prefix: &str) -> Option<String> {
    let raw = if rest.starts_with('"') {
        let (p, _) = take_quoted(rest);
        p
    } else {
        // git appends "\t" when the name has a space; plain diffs put a timestamp after a tab.
        rest.split('\t').next().unwrap_or(rest).to_string()
    };
    if raw == "/dev/null" {
        return None;
    }
    Some(raw.strip_prefix(prefix).map(str::to_string).unwrap_or(raw))
}

/// The two paths of `diff --git a/X b/Y`, prefixes stripped.
fn split_git_header(rest: &str) -> (Option<String>, Option<String>) {
    let strip = |s: String, p: &str| s.strip_prefix(p).map(str::to_string).unwrap_or(s);
    if rest.starts_with('"') {
        let (a, tail) = take_quoted(rest);
        let tail = tail.trim_start_matches(' ');
        let b = if tail.starts_with('"') {
            take_quoted(tail).0
        } else {
            tail.to_string()
        };
        return (Some(strip(a, "a/")), Some(strip(b, "b/")));
    }
    if let Some(pos) = rest.find(" \"b/") {
        let (b, _) = take_quoted(&rest[pos + 1..]);
        return (
            Some(strip(rest[..pos].to_string(), "a/")),
            Some(strip(b, "b/")),
        );
    }
    // Unquoted with spaces: when both names are equal the split is the middle.
    let n = rest.len();
    if n % 2 == 1 {
        let half = n / 2;
        if rest.is_char_boundary(half)
            && rest.as_bytes()[half] == b' '
            && rest.starts_with("a/")
            && rest[half + 1..].starts_with("b/")
            && rest[2..half] == rest[half + 3..]
        {
            let p = rest[2..half].to_string();
            return (Some(p.clone()), Some(p));
        }
    }
    match rest.find(" b/") {
        Some(pos) => (
            Some(strip(rest[..pos].to_string(), "a/")),
            Some(rest[pos + 3..].to_string()),
        ),
        None => (None, None),
    }
}

/// Unquote a path git may have C-quoted.
fn unquote(s: &str) -> String {
    if s.starts_with('"') {
        take_quoted(s).0
    } else {
        s.to_string()
    }
}

/// Parse a C-quoted string at the start of `s`. Returns the value and the rest after the quote.
fn take_quoted(s: &str) -> (String, &str) {
    let bytes = s.as_bytes();
    debug_assert_eq!(bytes.first(), Some(&b'"'));
    let mut out: Vec<u8> = Vec::new();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                return (String::from_utf8_lossy(&out).into_owned(), &s[i + 1..]);
            }
            b'\\' if i + 1 < bytes.len() => {
                let c = bytes[i + 1];
                i += 2;
                match c {
                    b'a' => out.push(7),
                    b'b' => out.push(8),
                    b't' => out.push(b'\t'),
                    b'n' => out.push(b'\n'),
                    b'v' => out.push(11),
                    b'f' => out.push(12),
                    b'r' => out.push(b'\r'),
                    b'0'..=b'7' => {
                        let mut v = (c - b'0') as u32;
                        let mut k = 0;
                        while k < 2 && i < bytes.len() && (b'0'..=b'7').contains(&bytes[i]) {
                            v = v * 8 + (bytes[i] - b'0') as u32;
                            i += 1;
                            k += 1;
                        }
                        out.push(v as u8);
                    }
                    other => out.push(other),
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    (String::from_utf8_lossy(&out).into_owned(), "")
}

/// One entry of `git diff --raw --numstat -z` (or plain `--numstat -z`).
#[derive(Clone, Debug, PartialEq)]
pub struct FileStat {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: DiffFile,
    pub binary: bool,
    pub additions: u32,
    pub deletions: u32,
}

/// Parse `--raw --numstat -z` output. Raw records give the status, numstat the counts. Plain
/// `--numstat -z` works too (everything is `Modified` unless it is a rename). Binary counts
/// (`-`) are 0. Sorted by path.
pub fn parse_numstat(bytes: &[u8]) -> Vec<FileStat> {
    let tokens: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let mut status: Vec<(String, char)> = Vec::new();
    let mut out: Vec<FileStat> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        i += 1;
        if tok.is_empty() {
            continue;
        }
        if tok[0] == b':' {
            let meta = s(tok);
            let letter = meta
                .rsplit(' ')
                .next()
                .and_then(|x| x.chars().next())
                .unwrap_or('M');
            if matches!(letter, 'R' | 'C') {
                let new = tokens.get(i + 1).map(|t| s(t)).unwrap_or_default();
                status.push((new, letter));
                i += 2;
            } else {
                let path = tokens.get(i).map(|t| s(t)).unwrap_or_default();
                status.push((path, letter));
                i += 1;
            }
            continue;
        }
        let text = s(tok);
        let mut parts = text.splitn(3, '\t');
        let (Some(a), Some(d), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let binary = a == "-" || d == "-";
        let additions = a.parse().unwrap_or(0);
        let deletions = d.parse().unwrap_or(0);
        let (path, old_path) = if path.is_empty() {
            let old = tokens.get(i).map(|t| s(t)).unwrap_or_default();
            let new = tokens.get(i + 1).map(|t| s(t)).unwrap_or_default();
            i += 2;
            (new, Some(old))
        } else {
            (path.to_string(), None)
        };
        out.push(FileStat {
            path,
            old_path,
            kind: DiffFile::Modified,
            binary,
            additions,
            deletions,
        });
    }
    for f in &mut out {
        let letter = status.iter().find(|(p, _)| *p == f.path).map(|(_, l)| *l);
        f.kind = match letter {
            Some('A') | Some('C') => DiffFile::Added,
            Some('D') => DiffFile::Deleted,
            Some('R') => DiffFile::Renamed,
            Some(_) => DiffFile::Modified,
            None if f.old_path.is_some() => DiffFile::Renamed,
            None => DiffFile::Modified,
        };
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_octal_and_escapes() {
        let (v, rest) = take_quoted(r#""a/\303\274n\"x\ty" tail"#);
        assert_eq!(v, "a/ün\"x\ty");
        assert_eq!(rest, " tail");
    }

    #[test]
    fn header_split_with_spaces() {
        assert_eq!(
            split_git_header("a/x y.txt b/x y.txt"),
            (Some("x y.txt".into()), Some("x y.txt".into()))
        );
        assert_eq!(
            split_git_header("a/old b/new name"),
            (Some("old".into()), Some("new name".into()))
        );
        assert_eq!(
            split_git_header(r#""a/q\"t" "b/q\"t""#),
            (Some("q\"t".into()), Some("q\"t".into()))
        );
    }

    #[test]
    fn hunk_header_defaults() {
        assert_eq!(parse_hunk_header("-1 +1 @@"), Some((1, 1, 1, 1)));
        assert_eq!(parse_hunk_header("-0,0 +1,3 @@ fn x"), Some((0, 0, 1, 3)));
    }
}

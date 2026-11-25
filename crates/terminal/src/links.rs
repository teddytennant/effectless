//! Link detection in a logical terminal line (spec/05 §3.11), ported from t3code
//! `apps/web/src/terminal-links.ts`. The JS regexes use lookbehind, which the `regex`
//! crate lacks, so each pattern is a small hand-written matcher with the same
//! leftmost, global-scan semantics.
//!
//! Offsets are `char` indices into the line, end exclusive.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Url,
    Path,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkMatch {
    pub kind: LinkKind,
    /// The link target. For paths this keeps any `:line[:col]` suffix; a TypeScript
    /// `file(12,3): error TS…` location is rewritten to `file:12:3`.
    pub text: String,
    pub start: usize,
    pub end: usize,
}

type Range = (usize, usize);

fn overlaps(a: Range, b: Range) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// `[\p{L}\p{M}\p{N}]`, approximated: alphanumerics plus combining marks.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || is_mark(c)
}

fn is_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f)
}

/// `[\p{L}\p{M}\p{N}._$+@-]`
fn is_seg(c: char) -> bool {
    is_word(c) || matches!(c, '.' | '_' | '$' | '+' | '@' | '-')
}

/// `[\p{L}\p{M}\p{N}._-]`
fn is_bracket_seg(c: char) -> bool {
    is_word(c) || matches!(c, '.' | '_' | '-')
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// `[^\s"'`<>]`
fn is_link_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, '"' | '\'' | '`' | '<' | '>')
}

/// JS `\w` (ASCII only).
fn is_js_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn starts_with_ci(s: &[char], at: usize, prefix: &str) -> bool {
    let mut i = at;
    for p in prefix.chars() {
        match s.get(i) {
            Some(c) if c.eq_ignore_ascii_case(&p) => i += 1,
            _ => return false,
        }
    }
    true
}

fn starts_with(s: &[char], at: usize, prefix: &str) -> bool {
    (at..)
        .zip(prefix.chars())
        .all(|(i, p)| s.get(i) == Some(&p))
}

fn run(s: &[char], mut i: usize, pred: impl Fn(char) -> bool) -> usize {
    while i < s.len() && pred(s[i]) {
        i += 1;
    }
    i
}

fn digits(s: &[char], i: usize) -> usize {
    run(s, i, |c| c.is_ascii_digit())
}

/// Global scan: calls `at` at each position from the left; on a match, resumes at its
/// end (or one past an empty match).
fn scan(s: &[char], mut at: impl FnMut(usize) -> Option<usize>) -> Vec<Range> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        match at(i) {
            Some(end) => {
                out.push((i, end));
                i = end.max(i + 1);
            }
            None => i += 1,
        }
    }
    out
}

/// `https?:\/\/[^\s"'`<>]+`, case-insensitive.
fn match_url(s: &[char], i: usize) -> Option<usize> {
    let scheme = if starts_with_ci(s, i, "https://") {
        8
    } else if starts_with_ci(s, i, "http://") {
        7
    } else {
        return None;
    };
    let end = run(s, i + scheme, is_link_char);
    (end > i + scheme).then_some(end)
}

/// `(?<![A-Za-z\d+.-])[+-]?[A-Za-z][A-Za-z\d+.-]+:\/\/[^\s"'`<>]+`
fn match_scheme_url(s: &[char], i: usize) -> Option<usize> {
    let scheme_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-');
    if i > 0 && scheme_char(s[i - 1]) {
        return None;
    }
    let mut j = i;
    if matches!(s[j], '+' | '-') {
        j += 1;
    }
    if !s.get(j)?.is_ascii_alphabetic() {
        return None;
    }
    let scheme_end = run(s, j + 1, scheme_char);
    if scheme_end < j + 2 || !starts_with_ci(s, scheme_end, "://") {
        return None;
    }
    let end = run(s, scheme_end + 3, is_link_char);
    (end > scheme_end + 3).then_some(end)
}

/// `(?<![\w.@-])[\w.-]+@[\w.-]+:[^\s"'`<>]+`; returns the tail after the colon.
fn scp_remote_tails(s: &[char]) -> Vec<Range> {
    let host_char = |c: char| is_js_word(c) || c == '.' || c == '-';
    let mut tails = Vec::new();
    for (start, end) in scan(s, |i| {
        if i > 0 && (host_char(s[i - 1]) || s[i - 1] == '@') {
            return None;
        }
        let user_end = run(s, i, host_char);
        if user_end == i || s.get(user_end) != Some(&'@') {
            return None;
        }
        let host_end = run(s, user_end + 1, host_char);
        if host_end == user_end + 1 || s.get(host_end) != Some(&':') {
            return None;
        }
        let end = run(s, host_end + 1, is_link_char);
        (end > host_end + 1).then_some(end)
    }) {
        let colon = (start..end).find(|&k| s[k] == ':').unwrap_or(start);
        tails.push((colon + 1, end));
    }
    tails
}

/// `(?:^|[\s"'`])(?:GET|…|proxying)\s+["'`]?(?:\/|[A-Za-z0-9_-]+\/)[^\s?]*\?\S*`
fn request_routes(s: &[char]) -> Vec<Range> {
    const METHODS: &[&str] = &[
        "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "CONNECT", "TRACE", "proxying",
    ];
    let delim = |c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`');
    scan(s, |i| {
        let word_start = if i == 0 && !delim(s[0]) {
            0
        } else if delim(s[i]) {
            i + 1
        } else {
            return None;
        };
        for method in METHODS {
            let m: Vec<char> = method.chars().collect();
            if s.len() < word_start + m.len() || s[word_start..word_start + m.len()] != m[..] {
                continue;
            }
            let mut j = word_start + m.len();
            let ws_end = run(s, j, char::is_whitespace);
            if ws_end == j {
                continue;
            }
            j = ws_end;
            if matches!(s.get(j), Some('"' | '\'' | '`')) {
                j += 1;
            }
            if s.get(j) == Some(&'/') {
                j += 1;
            } else {
                let w = run(s, j, |c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
                if w == j || s.get(w) != Some(&'/') {
                    continue;
                }
                j = w + 1;
            }
            j = run(s, j, |c| !c.is_whitespace() && c != '?');
            if s.get(j) != Some(&'?') {
                continue;
            }
            return Some(run(s, j + 1, |c| !c.is_whitespace()));
        }
        None
    })
}

/// `^PS ((?:[A-Za-z]:[\\/]|\\\\|\/)[^>\r\n]*)>`
fn powershell_prompt(s: &[char]) -> Option<LinkMatch> {
    if !starts_with(s, 0, "PS ") {
        return None;
    }
    let p = 3;
    let ok = (s.get(p).is_some_and(|c| c.is_ascii_alphabetic())
        && s.get(p + 1) == Some(&':')
        && s.get(p + 2).is_some_and(|c| is_sep(*c)))
        || (s.get(p) == Some(&'\\') && s.get(p + 1) == Some(&'\\'))
        || s.get(p) == Some(&'/');
    if !ok {
        return None;
    }
    let end = run(s, p, |c| !matches!(c, '>' | '\r' | '\n'));
    if s.get(end) != Some(&'>') {
        return None;
    }
    Some(LinkMatch {
        kind: LinkKind::Path,
        text: s[p..end].iter().collect(),
        start: p,
        end,
    })
}

/// Rooted path prefixes: `~/`, `./`, `../`, `/`, `C:\`, `\\` (either slash).
fn rooted_prefix(s: &[char], i: usize) -> Option<usize> {
    let c = *s.get(i)?;
    let next = s.get(i + 1).copied();
    if c == '~' && next.is_some_and(is_sep) {
        return Some(i + 2);
    }
    if c == '.' {
        if next.is_some_and(is_sep) {
            return Some(i + 2);
        }
        if next == Some('.') && s.get(i + 2).is_some_and(|c| is_sep(*c)) {
            return Some(i + 3);
        }
    }
    if c == '/' {
        return Some(i + 1);
    }
    if c.is_ascii_alphabetic() && next == Some(':') && s.get(i + 2).is_some_and(|c| is_sep(*c)) {
        return Some(i + 3);
    }
    if c == '\\' && next == Some('\\') {
        return Some(i + 2);
    }
    None
}

/// `:\s+(?:error|warning) TS\d+:` at `i`.
fn ts_error_follows(s: &[char], i: usize) -> bool {
    if s.get(i) != Some(&':') {
        return false;
    }
    let j = run(s, i + 1, char::is_whitespace);
    if j == i + 1 {
        return false;
    }
    let k = if starts_with(s, j, "error") {
        j + 5
    } else if starts_with(s, j, "warning") {
        j + 7
    } else {
        return false;
    };
    if !starts_with(s, k, " TS") {
        return false;
    }
    let d = digits(s, k + 3);
    d > k + 3 && s.get(d) == Some(&':')
}

/// `\(\d+,\d+\)` at `i`; returns its end.
fn ts_position(s: &[char], i: usize) -> Option<usize> {
    if s.get(i) != Some(&'(') {
        return None;
    }
    let a = digits(s, i + 1);
    if a == i + 1 || s.get(a) != Some(&',') {
        return None;
    }
    let b = digits(s, a + 1);
    if b == a + 1 || s.get(b) != Some(&')') {
        return None;
    }
    Some(b + 1)
}

/// One element of a path segment after a separator: a segment char, `[x]`/`[[x]]`, or
/// `(x)` followed by a separator.
fn seg_element(s: &[char], i: usize) -> Option<usize> {
    let c = *s.get(i)?;
    if is_seg(c) {
        return Some(i + 1);
    }
    if c == '[' {
        let mut j = i + 1;
        if s.get(j) == Some(&'[') {
            j += 1;
        }
        let body = run(s, j, is_bracket_seg);
        if body == j || s.get(body) != Some(&']') {
            return None;
        }
        let mut end = body + 1;
        if s.get(end) == Some(&']') {
            end += 1;
        }
        return Some(end);
    }
    if c == '(' {
        let body = run(s, i + 1, is_bracket_seg);
        if body == i + 1
            || s.get(body) != Some(&')')
            || !s.get(body + 1).is_some_and(|c| is_sep(*c))
        {
            return None;
        }
        return Some(body + 1);
    }
    None
}

/// `FILE_PATH_PATTERN` at `i`.
fn match_file_path(s: &[char], i: usize) -> Option<usize> {
    if let Some(after) = rooted_prefix(s, i) {
        let end = run(s, after, is_link_char);
        if end > after {
            return Some(end);
        }
    }
    if !is_seg(s[i]) || (i > 0 && is_seg(s[i - 1])) {
        return None;
    }
    let head = run(s, i, is_seg);
    // Branch 1: one or more `[\\/]` + segment elements.
    let mut end = head;
    loop {
        if !s.get(end).is_some_and(|c| is_sep(*c)) {
            break;
        }
        let mut j = end + 1;
        let mut any = false;
        while let Some(next) = seg_element(s, j) {
            j = next;
            any = true;
        }
        if !any {
            break;
        }
        end = j;
    }
    if end == head {
        // Branch 2: bare `name.ext` followed by `:line:col` or a TS location.
        let mut found = None;
        for dot in (i + 1..head).rev() {
            if s[dot] != '.' || !s.get(dot + 1).is_some_and(|c| c.is_alphabetic()) {
                continue;
            }
            let ext_end = run(s, dot + 2, |c| is_word(c) || c == '_' || c == '-');
            let line_col = s.get(ext_end) == Some(&':')
                && digits(s, ext_end + 1) > ext_end + 1
                && s.get(digits(s, ext_end + 1)) == Some(&':')
                && s.get(digits(s, ext_end + 1) + 1)
                    .is_some_and(|c| c.is_ascii_digit());
            let ts = ts_position(s, ext_end).is_some_and(|p| ts_error_follows(s, p));
            if line_col || ts {
                found = Some(ext_end);
                break;
            }
        }
        end = found?;
    }
    // Optional `(:\d+){1,2}` or a TS `(l,c)` before `: error TS…:`.
    let mut j = end;
    for _ in 0..2 {
        if s.get(j) == Some(&':') {
            let d = digits(s, j + 1);
            if d > j + 1 {
                j = d;
                continue;
            }
        }
        break;
    }
    if j == end
        && let Some(p) = ts_position(s, end).filter(|p| ts_error_follows(s, *p))
    {
        j = p;
    }
    Some(j)
}

/// `QUOTED_FILE_PATH_PATTERN` at `i`: returns (path range, full match end, has position).
fn match_quoted_path(s: &[char], i: usize) -> Option<(Range, usize)> {
    let q = s[i];
    if !matches!(q, '"' | '\'' | '`') {
        return None;
    }
    let p = i + 1;
    let quoted_word = |c: char| is_word(c) || matches!(c, '.' | '_' | ' ' | '-');
    // Alt (a): rooted, or `words/` — then lazily up to the first closing quote.
    let prefix_end = rooted_prefix(s, p).or_else(|| {
        let w = run(s, p, quoted_word);
        (w > p && s.get(w).is_some_and(|c| is_sep(*c))).then_some(w + 1)
    });
    let mut path_end = None;
    if let Some(prefix_end) = prefix_end {
        path_end = (prefix_end..s.len())
            .take_while(|&k| s[k] != '\r' && s[k] != '\n')
            .find(|&k| s[k] == q);
    }
    if path_end.is_none() {
        // Alt (b): `name.ext(:\d+){0,2}` filling the quotes exactly.
        let close = (p..s.len()).find(|&k| s[k] == q)?;
        let mut body_end = close;
        for _ in 0..2 {
            let d = (p..body_end)
                .rev()
                .take_while(|&k| s[k].is_ascii_digit())
                .count();
            if d > 0 && body_end - d > p && s[body_end - d - 1] == ':' {
                body_end -= d + 1;
            } else {
                break;
            }
        }
        let dot = (p..body_end).rev().find(|&k| s[k] == '.')?;
        let name_ok = dot > p && s[p..dot].iter().all(|c| quoted_word(*c));
        let ext_ok = dot + 1 < body_end
            && s[dot + 1..body_end]
                .iter()
                .all(|c| is_word(*c) || matches!(c, '_' | '-'));
        if !(name_ok && ext_ok) {
            return None;
        }
        path_end = Some(close);
    }
    let close = path_end?;
    if close == p {
        return None;
    }
    let mut end = close + 1;
    for _ in 0..2 {
        if s.get(end) == Some(&':') {
            let d = digits(s, end + 1);
            if d > end + 1 {
                end = d;
                continue;
            }
        }
        break;
    }
    Some(((p, close), end))
}

fn trim_closing_delimiters(value: &[char], kind: LinkKind) -> usize {
    let trailing = |c: char| match kind {
        LinkKind::Path => matches!(c, '.' | ',' | ';' | ':' | '!' | '?'),
        LinkKind::Url => matches!(c, '.' | ',' | ';' | '!' | '?'),
    };
    let mut len = value.len();
    while len > 0 && trailing(value[len - 1]) {
        len -= 1;
    }
    let output = &value[..len];
    let mut end = len;
    for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
        if end == 0 || output[end - 1] != close {
            continue;
        }
        let count = |c: char| output.iter().filter(|x| **x == c).count() as isize;
        let mut excess = count(close) - count(open);
        while excess > 0 && end > 0 && output[end - 1] == close {
            end -= 1;
            excess -= 1;
        }
    }
    end
}

fn is_terminal_url(text: &str) -> bool {
    let lower = text.get(..8).unwrap_or(text).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Finds URL and file-path links in one logical line (soft-wrapped rows joined).
pub fn detect_links(line: &str) -> Vec<LinkMatch> {
    let s: Vec<char> = line.chars().collect();
    if s.is_empty() {
        return Vec::new();
    }
    let prompt: Vec<LinkMatch> = powershell_prompt(&s).into_iter().collect();
    let mut non_path = request_routes(&s);
    non_path.extend(scan(&s, |i| match_scheme_url(&s, i)));
    let remote_tails = scp_remote_tails(&s);

    let collides = |r: Range, lists: &[&[LinkMatch]]| {
        lists
            .iter()
            .any(|list| list.iter().any(|m| overlaps(r, (m.start, m.end))))
    };

    let mut urls: Vec<LinkMatch> = Vec::new();
    for (start, end) in scan(&s, |i| match_url(&s, i)) {
        let len = trim_closing_delimiters(&s[start..end], LinkKind::Url);
        if len == 0 {
            continue;
        }
        let r = (start, start + len);
        if collides(r, &[&prompt, &urls]) {
            continue;
        }
        urls.push(LinkMatch {
            kind: LinkKind::Url,
            text: s[start..start + len].iter().collect(),
            start,
            end: start + len,
        });
    }

    let mut quoted: Vec<LinkMatch> = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let Some(((ps, pe), end)) = match_quoted_path(&s, i) else {
            i += 1;
            continue;
        };
        i = end;
        let position: String = s[pe + 1..end].iter().collect();
        let text: String = s[ps..pe].iter().collect::<String>() + &position;
        if is_terminal_url(&text) {
            continue;
        }
        let len = text.chars().count();
        // A position outside quotes spans the closing quote in the buffer.
        let r = (ps, ps + len + usize::from(!position.is_empty()));
        if collides(r, &[&prompt, &urls, &quoted]) {
            continue;
        }
        quoted.push(LinkMatch {
            kind: LinkKind::Path,
            text,
            start: r.0,
            end: r.1,
        });
    }

    let mut paths: Vec<LinkMatch> = Vec::new();
    for (start, end) in scan(&s, |i| match_file_path(&s, i)) {
        let len = trim_closing_delimiters(&s[start..end], LinkKind::Path);
        if len == 0 {
            continue;
        }
        let mut text: String = s[start..start + len].iter().collect();
        if is_terminal_url(&text) {
            continue;
        }
        let r = (start, start + len);
        if collides(r, &[&prompt, &urls, &quoted, &paths]) {
            continue;
        }
        if ts_error_follows(&s, start + len)
            && text.ends_with(')')
            && let Some(open) = text.rfind('(')
        {
            let inner = text[open + 1..text.len() - 1].replace(',', ":");
            text = format!("{}:{inner}", &text[..open]);
        }
        paths.push(LinkMatch {
            kind: LinkKind::Path,
            text,
            start: r.0,
            end: r.1,
        });
    }

    let mut all: Vec<LinkMatch> = prompt
        .into_iter()
        .chain(urls)
        .chain(quoted)
        .chain(paths)
        .filter(|m| {
            m.kind == LinkKind::Url
                || (!non_path.iter().any(|r| overlaps((m.start, m.end), *r))
                    && !remote_tails.iter().any(|t| m.start >= t.0 && m.end <= t.1))
        })
        .collect();
    all.sort_by_key(|m| m.start);
    all
}

/// Splits a path link's `:line[:col]` suffix.
pub fn split_path_position(text: &str) -> (&str, Option<u32>, Option<u32>) {
    let parse_tail = |s: &str| -> Option<(usize, u32)> {
        let colon = s.rfind(':')?;
        let tail = &s[colon + 1..];
        if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()) || colon == 0 {
            return None;
        }
        Some((colon, tail.parse().ok()?))
    };
    let Some((c1, last)) = parse_tail(text) else {
        return (text, None, None);
    };
    match parse_tail(&text[..c1]) {
        Some((c0, line)) => (&text[..c0], Some(line), Some(last)),
        None => (&text[..c1], Some(last), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(line: &str) -> Vec<(LinkKind, String)> {
        detect_links(line)
            .into_iter()
            .map(|m| (m.kind, m.text))
            .collect()
    }

    fn url(s: &str) -> (LinkKind, String) {
        (LinkKind::Url, s.to_string())
    }

    fn path(s: &str) -> (LinkKind, String) {
        (LinkKind::Path, s.to_string())
    }

    #[test]
    fn urls() {
        assert_eq!(
            links("see https://example.com/a?b=1."),
            [url("https://example.com/a?b=1")]
        );
        assert_eq!(links("(http://x.dev/foo)"), [url("http://x.dev/foo")]);
        assert_eq!(
            links("https://en.wikipedia.org/wiki/Foo_(bar) ok"),
            [url("https://en.wikipedia.org/wiki/Foo_(bar)")]
        );
        assert_eq!(links("<https://a.b/c>"), [url("https://a.b/c")]);
        assert_eq!(links("HTTPS://A.B"), [url("HTTPS://A.B")]);
        let m = &detect_links("go: https://a.b/c, then")[0];
        assert_eq!((m.start, m.end), (4, 17));
    }

    #[test]
    fn paths_with_positions() {
        assert_eq!(links("src/main.rs:12:3: error"), [path("src/main.rs:12:3")]);
        assert_eq!(links("at ./foo/bar.ts:7"), [path("./foo/bar.ts:7")]);
        assert_eq!(links("open ~/notes.md now"), [path("~/notes.md")]);
        assert_eq!(links("cd /etc/nixos."), [path("/etc/nixos")]);
        assert_eq!(links("../up/x"), [path("../up/x")]);
        assert_eq!(
            links("C:\\Users\\me\\a.txt"),
            [path("C:\\Users\\me\\a.txt")]
        );
        assert_eq!(
            links("app/[id]/page.tsx:3:1"),
            [path("app/[id]/page.tsx:3:1")]
        );
        assert_eq!(links("(see src/lib.rs)"), [path("src/lib.rs")]);
    }

    #[test]
    fn bare_names_need_a_position() {
        assert_eq!(links("main.rs:10:2"), [path("main.rs:10:2")]);
        assert_eq!(links("edit main.rs please"), []);
        assert_eq!(links("localhost:3000"), []);
        assert_eq!(links("version 1.2.3"), []);
        assert_eq!(
            links("index.ts(4,9): error TS2304: Cannot find name"),
            [path("index.ts:4:9")]
        );
    }

    #[test]
    fn quoted_paths_may_contain_spaces() {
        assert_eq!(
            links("open \"My Docs/report one.txt\" now"),
            [path("My Docs/report one.txt")]
        );
        assert_eq!(links("'notes file.md':4"), [path("notes file.md:4")]);
        let m = &detect_links("'notes file.md':4")[0];
        assert_eq!((m.start, m.end), (1, 17));
    }

    #[test]
    fn exclusions() {
        assert_eq!(links("redis://host/0"), []);
        assert_eq!(links("load file:///a.mjs"), []);
        assert_eq!(links("GET /api/x?y=1 200"), []);
        assert_eq!(links("git@github.com:acme/widgets.git"), []);
        assert_eq!(
            links("src/contact@support.vue:12:3"),
            [path("src/contact@support.vue:12:3")]
        );
        // A URL is never also a path.
        assert_eq!(links("https://a.b/c/d"), [url("https://a.b/c/d")]);
    }

    #[test]
    fn powershell_prompt_path() {
        assert_eq!(links("PS C:\\Users\\me> dir"), [path("C:\\Users\\me")]);
    }

    #[test]
    fn mixed_and_sorted() {
        let found = links("x src/a.rs:1 https://h.io/p y ./b");
        assert_eq!(
            found,
            [path("src/a.rs:1"), url("https://h.io/p"), path("./b")]
        );
    }

    #[test]
    fn char_offsets_not_bytes() {
        let m = &detect_links("é → src/a.rs")[0];
        assert_eq!((m.start, m.end), (4, 12));
    }

    #[test]
    fn splits_positions() {
        assert_eq!(
            split_path_position("a/b.rs:12:3"),
            ("a/b.rs", Some(12), Some(3))
        );
        assert_eq!(split_path_position("a/b.rs:12"), ("a/b.rs", Some(12), None));
        assert_eq!(split_path_position("a/b.rs"), ("a/b.rs", None, None));
        assert_eq!(split_path_position("C:\\x"), ("C:\\x", None, None));
    }
}

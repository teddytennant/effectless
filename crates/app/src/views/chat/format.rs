//! Text formatting shared by the timeline rows: durations
//! (`orchestrationTimingCopy.ts`), the "Working for" clock, message timestamps and
//! the command labels (`commandLabel.ts`, reduced to the cases agents produce).

use chrono::{DateTime, Local, Utc};

/// `formatDuration`: "640ms", "4.2s", "38s", "1m 3s", "1h 2m 3s", "1h 5s", "2h".
pub fn duration(ms: i64) -> String {
    if ms < 0 {
        return "0ms".into();
    }
    if ms < 1_000 {
        return format!("{}ms", ms.max(1));
    }
    if ms < 10_000 {
        let tenths = (ms as f64 / 100.).round() / 10.;
        return if tenths.fract() == 0. {
            format!("{}s", tenths as i64)
        } else {
            format!("{tenths:.1}s")
        };
    }
    let total = (ms as f64 / 1_000.).round() as i64;
    if total < 60 {
        return format!("{total}s");
    }
    let (h, m, s) = (total / 3_600, (total % 3_600) / 60, total % 60);
    match (h, m, s) {
        (0, m, s) => format!("{m}m {s}s"),
        (h, m, s) if m > 0 => format!("{h}h {m}m {s}s"),
        (h, 0, s) if s > 0 => format!("{h}h {s}s"),
        (h, _, _) => format!("{h}h"),
    }
}

/// The ticking "Working for {t}" clock: whole seconds, "1m 5s", "1h 2m 3s".
pub fn working_elapsed(secs: i64) -> String {
    let secs = secs.max(0);
    if secs < 60 {
        return format!("{secs}s");
    }
    let (h, m, s) = (secs / 3_600, (secs % 3_600) / 60, secs % 60);
    if h == 0 {
        format!("{m}m {s}s")
    } else {
        format!("{h}h {m}m {s}s")
    }
}

/// Message timestamps: "7:25 AM" today, "yesterday at 7:25 AM", else "11/3/2026, 7:25 AM".
pub fn timestamp(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let local = at.with_timezone(&Local);
    let today = now.with_timezone(&Local).date_naive();
    let time = local.format("%-I:%M %p").to_string();
    let day = local.date_naive();
    if day == today {
        time
    } else if today.pred_opt() == Some(day) {
        format!("yesterday at {time}")
    } else {
        format!("{}, {time}", local.format("%-m/%-d/%Y"))
    }
}

/// Tooltip form: "7:25 AM, 3rd November 2026".
pub fn timestamp_long(at: DateTime<Utc>) -> String {
    let local = at.with_timezone(&Local);
    let day: u32 = local.format("%-d").to_string().parse().unwrap_or(1);
    let suffix = match (day % 10, day % 100) {
        (1, n) if n != 11 => "st",
        (2, n) if n != 12 => "nd",
        (3, n) if n != 13 => "rd",
        _ => "th",
    };
    format!(
        "{}, {day}{suffix} {}",
        local.format("%-I:%M %p"),
        local.format("%B %Y")
    )
}

const SHELLS: [&str; 7] = ["sh", "bash", "zsh", "dash", "ash", "ksh", "fish"];

fn basename(token: &str) -> &str {
    token.rsplit('/').next().unwrap_or(token)
}

/// Strips one quoting layer if the whole string is a single quoted word.
fn unquote(s: &str) -> Option<String> {
    let s = s.trim();
    let q = s.chars().next()?;
    if (q != '\'' && q != '"') || s.len() < 2 || !s.ends_with(q) {
        return None;
    }
    let inner = &s[1..s.len() - 1];
    if q == '\'' {
        // `'…'\''…'` sequences close and reopen the quote around an escaped one.
        if inner.contains('\'') && !inner.contains("'\\''") {
            return None;
        }
        return Some(inner.replace("'\\''", "'"));
    }
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(&n @ ('"' | '\\' | '$' | '`')) => {
                    out.push(n);
                    chars.next();
                }
                _ => out.push(c),
            }
        } else if c == '"' {
            return None;
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// `bash -lc '…'` / `/bin/zsh -c "…"` → the inner script; anything else unchanged.
pub fn without_shell_wrapper(command: &str) -> String {
    let trimmed = command.trim();
    let mut rest = trimmed;
    let Some((program, after)) = split_token(rest) else {
        return trimmed.to_string();
    };
    if !SHELLS.contains(&basename(program)) {
        return trimmed.to_string();
    }
    rest = after;
    // Options up to and including the one carrying `c`.
    loop {
        let Some((tok, after)) = split_token(rest) else {
            return trimmed.to_string();
        };
        if !tok.starts_with('-') {
            return trimmed.to_string();
        }
        rest = after;
        if tok.starts_with("--") {
            continue;
        }
        if tok.contains('c') {
            break;
        }
    }
    match unquote(rest) {
        Some(script) => script,
        None if !rest.trim().starts_with(['\'', '"']) && !rest.trim().is_empty() => {
            rest.trim().to_string()
        }
        None => trimmed.to_string(),
    }
}

fn split_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    Some((&s[..end], &s[end..]))
}

/// Control characters drawn as Unicode control pictures (ESC → ␛); tab and newlines kept.
pub fn visible_control_chars(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{7f}' => '␡',
            '\t' | '\n' | '\r' => c,
            c if (c as u32) < 0x20 => char::from_u32(0x2400 + c as u32).unwrap_or(c),
            c => c,
        })
        .collect()
}

/// `commandDisplayText`.
pub fn command_display(command: &str) -> String {
    visible_control_chars(&without_shell_wrapper(command))
}

const NON_DESCRIPTIVE: [&str; 8] = ["cd", "pushd", "popd", "export", "set", "source", ".", "true"];
const WRAPPERS: [&str; 8] = [
    "sudo",
    "env",
    "command",
    "builtin",
    "exec",
    "time",
    "nocorrect",
    "noglob",
];

/// `commandProgramName`: the first descriptive program of a command line (`git`, `pnpm`),
/// looking through shell wrappers, `cd x &&` prefixes, env assignments and `sudo`/`env`.
pub fn command_program(command: &str) -> Option<String> {
    let script = without_shell_wrapper(command);
    for segment in split_segments(&script) {
        let mut tokens = segment.split_whitespace().peekable();
        let mut program = None;
        while let Some(tok) = tokens.next() {
            let tok = tok.trim_matches(['(', ')', '{', '}']);
            if tok.is_empty() {
                continue;
            }
            let is_assignment = tok
                .split_once('=')
                .is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || c == '_'));
            if is_assignment || WRAPPERS.contains(&tok) || (tok.starts_with('-') && program.is_none()) {
                continue;
            }
            if tok.starts_with(['>', '<', '&', '|', '$', '"', '\'']) || tok.ends_with('=') {
                break;
            }
            program = Some(basename(tok).to_string());
            break;
        }
        match program {
            Some(p) if NON_DESCRIPTIVE.contains(&p.as_str()) => continue,
            Some(p) if SHELLS.contains(&p.as_str()) => {
                let inner = without_shell_wrapper(segment);
                if inner != segment.trim() {
                    return command_program(&inner);
                }
                return Some(p);
            }
            Some(p) => return Some(p),
            None => continue,
        }
    }
    None
}

/// Splits on `&&`, `||`, `;`, `|` and newlines outside quotes.
fn split_segments(script: &str) -> Vec<&str> {
    let bytes = script.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' && q == b'"' {
                    i += 1;
                } else if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'\'' | b'"' => quote = Some(b),
                b'\\' => i += 1,
                b';' | b'\n' | b'|' | b'&' => {
                    let double = i + 1 < bytes.len() && bytes[i + 1] == b && b != b';';
                    if b == b'&' && !double {
                        // `2>&1`, `&>`: part of a redirection.
                        i += 1;
                        continue;
                    }
                    out.push(&script[start..i]);
                    i += if double { 2 } else { 1 };
                    start = i;
                    continue;
                }
                _ => {}
            },
        }
        i += 1;
    }
    out.push(&script[start..]);
    out.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// "1.2k", "12k", "1.5m": compact counts (`DiffStatLabel`).
pub fn compact_count(n: u64) -> String {
    fn one(v: f64, unit: &str) -> String {
        if v < 10. && (v * 10.).round() % 10. != 0. {
            format!("{:.1}{unit}", (v * 10.).floor() / 10.)
        } else {
            format!("{}{unit}", v.floor() as u64)
        }
    }
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => one(n as f64 / 1e3, "k"),
        1_000_000..=999_999_999 => one(n as f64 / 1e6, "m"),
        _ => one(n as f64 / 1e9, "b"),
    }
}

/// Token counts in the compaction divider: "182k".
pub fn tokens(n: u32) -> String {
    compact_count(n as u64)
}

/// `liveThoughtLine`: a leading `**heading**`, else the text up to the first sentence end,
/// with inline markdown flattened.
pub fn live_thought_line(markdown: &str) -> String {
    let trimmed = markdown.trim_start();
    let heading = trimmed.strip_prefix("**").and_then(|rest| {
        let end = rest.find("**")?;
        let head = &rest[..end];
        let after = rest[end + 2..].trim_start_matches([' ', '\t']);
        (!head.contains(['\n', '*']) && (after.is_empty() || after.starts_with(['\n', '\r'])))
            .then_some(head)
    });
    let text = flatten_inline(heading.unwrap_or(markdown));
    if heading.is_some() {
        return text;
    }
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if matches!(c, '。' | '！' | '？') {
            return chars[..=i].iter().collect();
        }
        if matches!(c, '.' | '?' | '!') {
            let mut end = i + 1;
            if chars.get(end).is_some_and(|c| "\"'”’)".contains(*c)) {
                end += 1;
            }
            if chars.get(end).is_some_and(|c| c.is_whitespace()) {
                return chars[..end].iter().collect();
            }
        }
    }
    text
}

/// Drops links/emphasis/code markers and list/heading markers, collapsing whitespace.
pub fn flatten_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let l = line.trim_start();
        let l = l
            .strip_prefix(|c| c == '#')
            .map(|r| r.trim_start_matches('#'))
            .filter(|r| r.starts_with([' ', '\t']))
            .unwrap_or(l);
        let l = ["- ", "* ", "+ "]
            .iter()
            .find_map(|m| l.strip_prefix(m))
            .unwrap_or(l);
        let l = match l.find(". ") {
            Some(i) if i > 0 && l[..i].chars().all(|c| c.is_ascii_digit()) => &l[i + 2..],
            _ => l,
        };
        out.push_str(l);
        out.push(' ');
    }
    // [text](url) → text
    let mut flat = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(open) = rest.find('[') {
        let start = if open > 0 && rest.as_bytes()[open - 1] == b'!' {
            open - 1
        } else {
            open
        };
        let Some(close) = rest[open..].find("](").map(|i| open + i) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|i| close + i) else {
            break;
        };
        flat.push_str(&rest[..start]);
        flat.push_str(&rest[open + 1..close]);
        rest = &rest[end + 1..];
    }
    flat.push_str(rest);
    let flat = flat.replace("**", "").replace("~~", "").replace('`', "");
    let mut result = String::with_capacity(flat.len());
    let chars: Vec<char> = flat.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '*' || c == '_' {
            let prev_word = i > 0 && chars[i - 1].is_alphanumeric();
            let next_word = chars.get(i + 1).is_some_and(|n| n.is_alphanumeric());
            if prev_word && next_word {
                result.push(c);
            }
            continue;
        }
        result.push(c);
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_follow_t3() {
        assert_eq!(duration(0), "1ms");
        assert_eq!(duration(640), "640ms");
        assert_eq!(duration(8_800), "8.8s");
        assert_eq!(duration(4_000), "4s");
        assert_eq!(duration(38_400), "38s");
        assert_eq!(duration(59_600), "1m 0s");
        assert_eq!(duration(63_000), "1m 3s");
        assert_eq!(duration(3_723_000), "1h 2m 3s");
        assert_eq!(duration(3_605_000), "1h 5s");
        assert_eq!(duration(7_200_000), "2h");
    }

    #[test]
    fn working_clock() {
        assert_eq!(working_elapsed(5), "5s");
        assert_eq!(working_elapsed(65), "1m 5s");
        assert_eq!(working_elapsed(482), "8m 2s");
        assert_eq!(working_elapsed(3_723), "1h 2m 3s");
    }

    #[test]
    fn shell_wrappers_unwrap() {
        assert_eq!(without_shell_wrapper("bash -lc 'ls -la'"), "ls -la");
        assert_eq!(
            without_shell_wrapper("/bin/zsh -c \"echo \\\"hi\\\"\""),
            "echo \"hi\""
        );
        assert_eq!(without_shell_wrapper("git status"), "git status");
        assert_eq!(without_shell_wrapper("bash script.sh"), "bash script.sh");
    }

    #[test]
    fn program_names() {
        assert_eq!(command_program("head -15 README.md && echo ---").as_deref(), Some("head"));
        assert_eq!(command_program("cd crates/app && cargo test").as_deref(), Some("cargo"));
        assert_eq!(command_program("FOO=1 sudo pnpm i").as_deref(), Some("pnpm"));
        assert_eq!(command_program("bash -lc 'git status'").as_deref(), Some("git"));
        assert_eq!(command_program("/usr/bin/rg -n foo 2>&1").as_deref(), Some("rg"));
        assert_eq!(command_program("touch TODO.md && ls").as_deref(), Some("touch"));
        assert_eq!(command_program("").as_deref(), None);
    }

    #[test]
    fn control_pictures() {
        assert_eq!(visible_control_chars("a\u{1b}[0m\tb"), "a␛[0m\tb");
    }

    #[test]
    fn compact_counts() {
        assert_eq!(compact_count(999), "999");
        assert_eq!(compact_count(1_234), "1.2k");
        assert_eq!(compact_count(12_345), "12k");
        assert_eq!(compact_count(1_500_000), "1.5m");
        assert_eq!(compact_count(2_000_000_000), "2b");
    }

    #[test]
    fn thought_lines() {
        assert_eq!(
            live_thought_line("**Planning the fix**\nThen more."),
            "Planning the fix"
        );
        assert_eq!(
            live_thought_line("I'll read `main.rs` first. Then the tests."),
            "I'll read main.rs first."
        );
        assert_eq!(live_thought_line("No end here"), "No end here");
        assert_eq!(
            live_thought_line("See [the docs](http://x) for *more*."),
            "See the docs for more."
        );
    }

    #[test]
    fn ordinal_suffixes() {
        let at = chrono::DateTime::parse_from_rfc3339("2026-11-03T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(timestamp_long(at).contains("November 2026"));
    }
}

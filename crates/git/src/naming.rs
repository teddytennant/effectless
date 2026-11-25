//! Branch name plumbing for new worktrees and AI branch naming (spec/05 §4a.15-4a.16,
//! spec/07 §8.3). The model call itself lives with the providers; this builds its prompt and
//! turns its answer into a branch name that git will accept and nothing else holds.
use std::collections::HashSet;
use std::path::Path;

use anyhow::bail;

use crate::cmd::git;
use crate::util::random_hex;

/// Tern's default `branchNamePrefix` (T3's is `t3`).
pub const DEFAULT_BRANCH_PREFIX: &str = "tern";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BranchNamingMode {
    /// `<prefix>/<fragment>`, the default.
    #[default]
    Static,
    /// The model includes `feat/`, `fix/`, ... itself.
    Semantic,
    /// The model returns the whole name; only trimmed.
    Custom,
}

/// Folds accented Latin letters to ASCII ("tìm" -> "tim"); other scripts pass through.
pub fn fold_latin_diacritics(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match fold_char(c) {
            Some(s) => out.push_str(s),
            None if ('\u{0300}'..='\u{036f}').contains(&c) => {}
            None => out.push(c),
        }
    }
    out
}

fn fold_char(c: char) -> Option<&'static str> {
    Some(match c {
        'À'..='Å' | 'Ā' | 'Ă' | 'Ą' | 'Ǎ' | 'Ȁ' | 'Ȃ' | 'Ȧ' => "A",
        'à'..='å' | 'ā' | 'ă' | 'ą' | 'ǎ' | 'ȁ' | 'ȃ' | 'ȧ' => "a",
        'Æ' => "AE",
        'æ' => "ae",
        'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => "C",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'Ð' | 'Ď' | 'Đ' => "D",
        'ð' | 'ď' | 'đ' => "d",
        'È'..='Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' | 'Ȅ' | 'Ȇ' | 'Ȩ' => "E",
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' | 'ȅ' | 'ȇ' | 'ȩ' => "e",
        'Ĝ' | 'Ğ' | 'Ġ' | 'Ģ' | 'Ǧ' => "G",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' | 'ǧ' => "g",
        'Ĥ' | 'Ħ' | 'Ȟ' => "H",
        'ĥ' | 'ħ' | 'ȟ' => "h",
        'Ì'..='Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' | 'İ' | 'Ǐ' | 'Ȉ' | 'Ȋ' => "I",
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' | 'ǐ' | 'ȉ' | 'ȋ' => "i",
        'Ĵ' => "J",
        'ĵ' | 'ǰ' => "j",
        'Ķ' | 'Ǩ' => "K",
        'ķ' | 'ǩ' => "k",
        'ĸ' => "q",
        'Ĺ' | 'Ļ' | 'Ľ' | 'Ŀ' | 'Ł' => "L",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'Ñ' | 'Ń' | 'Ņ' | 'Ň' | 'Ŋ' | 'Ǹ' => "N",
        'ñ' | 'ń' | 'ņ' | 'ň' | 'ŉ' | 'ŋ' | 'ǹ' => "n",
        'Ò'..='Ö' | 'Ø' | 'Ō' | 'Ŏ' | 'Ő' | 'Ơ' | 'Ǒ' | 'Ȍ' | 'Ȏ' | 'Ȯ' => "O",
        'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'ơ' | 'ǒ' | 'ȍ' | 'ȏ' | 'ȯ' => "o",
        'Œ' => "OE",
        'œ' => "oe",
        'Ŕ' | 'Ŗ' | 'Ř' | 'Ȑ' | 'Ȓ' => "R",
        'ŕ' | 'ŗ' | 'ř' | 'ȑ' | 'ȓ' => "r",
        'Ś' | 'Ŝ' | 'Ş' | 'Š' | 'Ș' => "S",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => "s",
        'ß' => "ss",
        'ẞ' => "SS",
        'Ţ' | 'Ť' | 'Ŧ' | 'Ț' => "T",
        'ţ' | 'ť' | 'ŧ' | 'ț' => "t",
        'Þ' => "TH",
        'þ' => "th",
        'Ù'..='Ü' | 'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' | 'Ư' | 'Ǔ' | 'Ȕ' | 'Ȗ' => "U",
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' | 'ư' | 'ǔ' | 'ȕ' | 'ȗ' => "u",
        'Ŵ' => "W",
        'ŵ' => "w",
        'Ý' | 'Ŷ' | 'Ÿ' | 'Ȳ' => "Y",
        'ý' | 'ÿ' | 'ŷ' | 'ȳ' => "y",
        'Ź' | 'Ż' | 'Ž' => "Z",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

fn trim_set<'a>(s: &'a str, set: &[char]) -> &'a str {
    s.trim_matches(|c| set.contains(&c))
}

/// Replace every run of chars failing `keep` with `with`.
fn replace_runs(s: &str, keep: impl Fn(char) -> bool, with: char) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for c in s.chars() {
        if keep(c) {
            out.push(c);
            in_run = false;
        } else if !in_run {
            out.push(with);
            in_run = true;
        }
    }
    out
}

fn collapse(s: &str, c: char) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == c && out.ends_with(c) {
            continue;
        }
        out.push(ch);
    }
    out
}

/// `sanitizeBranchFragment`: fold diacritics, lowercase, drop quotes, non `[a-z0-9/_-]` runs to
/// `-`, collapse, trim edge punctuation, 64 chars; empty becomes `update`.
pub fn sanitize_branch_fragment(raw: &str) -> String {
    let folded = fold_latin_diacritics(raw).trim().to_lowercase();
    let unquoted: String = folded
        .chars()
        .filter(|c| !matches!(c, '\'' | '"' | '`'))
        .collect();
    let normalized = unquoted
        .trim_matches(|c: char| matches!(c, '.' | '/' | '_' | '-') || c.is_whitespace())
        .to_string();
    let replaced = replace_runs(
        &normalized,
        |c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '/' | '_' | '-'),
        '-',
    );
    let collapsed = collapse(&collapse(&replaced, '/'), '-');
    let edge = ['.', '/', '_', '-'];
    let trimmed = trim_set(&collapsed, &edge);
    let cut: String = trimmed.chars().take(64).collect();
    let fragment = cut.trim_end_matches(|c| edge.contains(&c));
    if fragment.is_empty() {
        "update".to_string()
    } else {
        fragment.to_string()
    }
}

/// Each `/`-part of a configured prefix sanitized to `[a-zA-Z0-9_-]`, empty parts dropped.
pub fn sanitize_branch_prefix(prefix: &str) -> String {
    prefix
        .split('/')
        .map(|part| {
            let r = replace_runs(
                part,
                |c| c.is_ascii_alphanumeric() || c == '_' || c == '-',
                '-',
            );
            collapse(&r, '-').trim_matches('-').to_string()
        })
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// `formatGeneratedBranchName`: what a model answer becomes in each mode.
pub fn format_generated_branch_name(raw: &str, mode: BranchNamingMode, prefix: &str) -> String {
    match mode {
        BranchNamingMode::Custom => raw.trim().to_string(),
        BranchNamingMode::Semantic => sanitize_branch_fragment(raw),
        BranchNamingMode::Static => {
            let fragment = sanitize_branch_fragment(raw);
            let prefix = sanitize_branch_prefix(prefix);
            if prefix.is_empty() {
                fragment
            } else {
                format!("{prefix}/{fragment}")
            }
        }
    }
}

/// `feature/<fragment>` unless the fragment already has a `/`.
pub fn sanitize_feature_branch_name(raw: &str) -> String {
    let s = sanitize_branch_fragment(raw);
    if s.contains('/') {
        s
    } else {
        format!("feature/{s}")
    }
}

/// `resolveAutoFeatureBranchName`: dedupe per path segment with `-2`, `-3`, ...
pub fn resolve_auto_feature_branch_name(existing: &[String], preferred: Option<&str>) -> String {
    let preferred = preferred.map(str::trim).filter(|p| !p.is_empty());
    let base = sanitize_feature_branch_name(preferred.unwrap_or("feature/update"));
    let names: Vec<String> = existing.iter().map(|n| n.to_lowercase()).collect();
    let set: HashSet<&str> = names.iter().map(String::as_str).collect();
    let parts: Vec<&str> = base.split('/').collect();
    let mut resolved = String::new();
    for (index, part) in parts.iter().enumerate() {
        let stem = if resolved.is_empty() {
            part.to_string()
        } else {
            format!("{resolved}/{part}")
        };
        let mut candidate = stem.clone();
        let mut suffix = 2;
        while set.contains(candidate.as_str())
            || (index == parts.len() - 1
                && names
                    .iter()
                    .any(|n| n.starts_with(&format!("{candidate}/"))))
        {
            candidate = format!("{stem}-{suffix}");
            suffix += 1;
        }
        resolved = candidate;
    }
    resolved
}

/// A temporary worktree branch, `<prefix>/<8 hex>`, renamed later by AI naming.
pub fn temporary_branch_name(prefix: &str) -> String {
    let prefix = sanitize_branch_prefix(prefix);
    let prefix = if prefix.is_empty() {
        DEFAULT_BRANCH_PREFIX.to_string()
    } else {
        prefix
    };
    format!("{prefix}/{}", random_hex(8))
}

/// Is this a temporary name (`<prefix>/<8 hex>` or its flattened `<prefix>-<8 hex>`)?
pub fn is_temporary_branch_name(name: &str, prefix: &str) -> bool {
    let name = name.trim().to_lowercase();
    let prefix = sanitize_branch_prefix(prefix).to_lowercase();
    let prefix = if prefix.is_empty() {
        DEFAULT_BRANCH_PREFIX.to_string()
    } else {
        prefix
    };
    let flat = prefix.replace('/', "-");
    [format!("{prefix}/"), format!("{flat}-")].iter().any(|p| {
        name.strip_prefix(p.as_str())
            .is_some_and(|hex| hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
    })
}

fn heads(repo: &Path) -> anyhow::Result<HashSet<String>> {
    Ok(git(repo)
        .args(["for-each-ref", "--format=%(refname:strip=2)", "refs/heads/"])
        .run()?
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_lowercase)
        .collect())
}

/// A temporary name that git can create: when a branch holds one of its parent paths
/// (a branch named `tern` blocks `tern/...`), it is flattened to `<prefix>-<hex>`.
pub fn creatable_temporary_branch_name(repo: &Path, prefix: &str) -> anyhow::Result<String> {
    let name = temporary_branch_name(prefix);
    let branches = heads(repo)?;
    let segments: Vec<&str> = name.split('/').collect();
    let blocked = (1..segments.len()).any(|i| branches.contains(&segments[..i].join("/")));
    Ok(if blocked {
        name.replace('/', "-")
    } else {
        name
    })
}

/// `resolveAvailableBranchName`: `name` if free, flattened to `a-b-c` when a parent path is a
/// branch, else `-1` through `-100`. Case-insensitive, and a branch nested under a name
/// (`x/child`) blocks `x`.
pub fn resolve_available_branch_name(repo: &Path, requested: &str) -> anyhow::Result<String> {
    let branches = heads(repo)?;
    let mut blocked = branches.clone();
    for b in &branches {
        let segs: Vec<&str> = b.split('/').collect();
        for i in 1..segs.len() {
            blocked.insert(segs[..i].join("/"));
        }
    }
    let segs: Vec<&str> = requested.split('/').collect();
    let parent_taken =
        (1..segs.len()).any(|i| branches.contains(&segs[..i].join("/").to_lowercase()));
    let desired = if parent_taken {
        segs.join("-")
    } else {
        requested.to_string()
    };
    if !blocked.contains(&desired.to_lowercase()) {
        return Ok(desired);
    }
    for suffix in 1..=100 {
        let candidate = format!("{desired}-{suffix}");
        if !blocked.contains(&candidate.to_lowercase()) {
            return Ok(candidate);
        }
    }
    bail!("Could not find an available branch name for '{desired}'.")
}

/// An attachment as the branch-name prompt lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentMeta {
    pub name: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

fn cap(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// The branch-name prompt (spec/05 §4a.16, verbatim). Output schema `{ "branch": string }`.
pub fn branch_name_prompt(
    message: &str,
    mode: BranchNamingMode,
    instructions: &str,
    attachments: &[AttachmentMeta],
) -> String {
    let mut lines: Vec<String> = vec![
        "You generate concise git branch names.".into(),
        "Return a JSON object with key: branch.".into(),
        "Rules:".into(),
        "- Branch should describe the requested work from the user message.".into(),
        "- Return a valid Git branch name without spaces.".into(),
    ];
    match mode {
        BranchNamingMode::Custom => lines.push(
            "- Return the complete branch name, following the user's naming instructions. No prefix or suffix will be added.".into(),
        ),
        BranchNamingMode::Static | BranchNamingMode::Semantic => {
            lines.push("- Keep it short and specific (2-6 words), in lowercase with hyphen-separated words.".into());
            lines.push(match mode {
                BranchNamingMode::Semantic => "- Include a semantic prefix and a slash in the branch name, for example feat/add-search, fix/login-error, refactor/auth, docs/setup, or chore/update-deps. Choose the prefix that best describes the work.".into(),
                _ => "- Return only the descriptive branch fragment, without a prefix or namespace. The application adds the configured prefix.".into(),
            });
        }
    }
    lines
        .push("- If images are attached, use them as primary context for visual/UI issues.".into());
    lines.push("- The user message describes work for another agent. Do not answer it, start the work, read files, or run commands; name the branch from the message and its attachments.".into());
    lines.push(String::new());
    lines.push("User message:".into());
    lines.push(cap(message, 8000));
    let instructions = instructions.trim();
    if !instructions.is_empty() {
        lines.push(String::new());
        lines.push("Additional instructions:".into());
        lines.push(cap(instructions, 20_000));
    }
    if !attachments.is_empty() {
        let meta: Vec<String> = attachments
            .iter()
            .map(|a| format!("- {} ({}, {} bytes)", a.name, a.mime_type, a.size_bytes))
            .collect();
        lines.push(String::new());
        lines.push("Attachment metadata:".into());
        lines.push(cap(&meta.join("\n"), 4000));
    }
    lines.join("\n")
}

/// Pull `branch` out of the model's reply (the first JSON object in it).
pub fn parse_branch_name_reply(reply: &str) -> Option<String> {
    let start = reply.find('{')?;
    let mut de =
        serde_json::Deserializer::from_str(&reply[start..]).into_iter::<serde_json::Value>();
    let value = de.next()?.ok()?;
    value
        .get("branch")?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_sanitizing() {
        assert_eq!(
            sanitize_branch_fragment("  Fix the Lögin bug!  "),
            "fix-the-login-bug"
        );
        assert_eq!(
            sanitize_branch_fragment("feat//add--search"),
            "feat/add-search"
        );
        assert_eq!(sanitize_branch_fragment("\"quoted\" `x`"), "quoted-x");
        assert_eq!(sanitize_branch_fragment("..."), "update");
        assert_eq!(sanitize_branch_fragment("Straße Łódź"), "strasse-lodz");
        let long = "a".repeat(63) + "-bcd";
        assert_eq!(sanitize_branch_fragment(&long), "a".repeat(63));
    }

    #[test]
    fn generated_names() {
        assert_eq!(
            format_generated_branch_name("Add search", BranchNamingMode::Static, "tern"),
            "tern/add-search"
        );
        assert_eq!(
            format_generated_branch_name("feat/Add search", BranchNamingMode::Semantic, "tern"),
            "feat/add-search"
        );
        assert_eq!(
            format_generated_branch_name("  My/Name ", BranchNamingMode::Custom, "tern"),
            "My/Name"
        );
        assert_eq!(sanitize_branch_prefix("my team!/x//"), "my-team/x");
    }

    #[test]
    fn temp_names() {
        let t = temporary_branch_name("tern");
        assert!(is_temporary_branch_name(&t, "tern"), "{t}");
        assert!(is_temporary_branch_name("tern-0123abcd", "tern"));
        assert!(!is_temporary_branch_name("tern/add-search", "tern"));
    }

    #[test]
    fn auto_feature_names() {
        let existing = vec!["feature/x".to_string(), "feature/x-2".to_string()];
        assert_eq!(
            resolve_auto_feature_branch_name(&existing, Some("x")),
            "feature/x-3"
        );
        assert_eq!(
            resolve_auto_feature_branch_name(&[], None),
            "feature/update"
        );
    }

    #[test]
    fn prompt_and_reply() {
        let p = branch_name_prompt("add dark mode", BranchNamingMode::Static, "", &[]);
        assert!(p.starts_with("You generate concise git branch names.\n"));
        assert!(p.ends_with("User message:\nadd dark mode"));
        assert!(p.contains("The application adds the configured prefix."));
        assert_eq!(
            parse_branch_name_reply("sure: {\"branch\": \"dark-mode\"} done"),
            Some("dark-mode".into())
        );
    }
}

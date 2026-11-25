//! The add-project folder browser's path logic (spec 02 §4.5): when a query is a path,
//! which directory to list, how the typed tail filters it, and Tab completion.

use std::path::{Path, PathBuf};

/// Browse mode is on whenever the query starts like a path.
pub fn is_path_query(query: &str) -> bool {
    query.starts_with('/')
        || query.starts_with("~/")
        || query == "~"
        || query.starts_with("./")
        || query.starts_with("../")
}

/// Expands `~` and resolves `./`, `../` against `cwd`. None for a relative path with no
/// `cwd` ("Relative paths require an active project.").
pub fn expand(query: &str, home: &Path, cwd: Option<&Path>) -> Option<PathBuf> {
    if query == "~" {
        return Some(home.to_path_buf());
    }
    if let Some(rest) = query.strip_prefix("~/") {
        return Some(home.join(rest));
    }
    if query.starts_with("./") || query.starts_with("../") {
        return cwd.map(|c| c.join(query));
    }
    Some(PathBuf::from(query))
}

/// Splits a path query into the directory to list (everything up to the last `/`) and the
/// filter (the text after it).
pub fn split(query: &str) -> (&str, &str) {
    match query.rfind('/') {
        Some(i) => (&query[..=i], &query[i + 1..]),
        None => ("", query),
    }
}

/// Normalizes `a/b/../c` lexically, keeping a leading `/`.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Entries matching `filter`: exact, then prefix, then word boundary, then substring,
/// then fuzzy (letters in order). Dot-folders only when the filter starts with ".".
pub fn filter_entries(entries: &[String], filter: &str) -> Vec<String> {
    let f = filter.to_lowercase();
    let show_hidden = f.starts_with('.');
    let mut ranked: Vec<(u8, &String)> = entries
        .iter()
        .filter(|e| show_hidden || !e.starts_with('.'))
        .filter_map(|e| {
            if f.is_empty() {
                return Some((0, e));
            }
            let name = e.to_lowercase();
            let rank = if name == f {
                0
            } else if name.starts_with(&f) {
                1
            } else if name
                .match_indices(&f)
                .any(|(i, _)| i > 0 && !name.as_bytes()[i - 1].is_ascii_alphanumeric())
            {
                2
            } else if name.contains(&f) {
                3
            } else if fuzzy(&name, &f) {
                4
            } else {
                return None;
            };
            Some((rank, e))
        })
        .collect();
    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });
    ranked.into_iter().map(|(_, e)| e.clone()).collect()
}

fn fuzzy(name: &str, filter: &str) -> bool {
    let mut chars = name.chars();
    filter.chars().all(|c| chars.any(|n| n == c))
}

/// The longest common prefix of `names` (Tab completion).
pub fn common_prefix(names: &[String]) -> String {
    let Some(first) = names.first() else {
        return String::new();
    };
    let mut len = first.len();
    for name in &names[1..] {
        len = first
            .char_indices()
            .zip(name.chars())
            .take_while(|((_, a), b)| a == b)
            .map(|((i, a), _)| i + a.len_utf8())
            .last()
            .unwrap_or(0)
            .min(len);
    }
    first[..len].to_string()
}

/// Lists the directories in `dir`, sorted by name. Runs on a background thread.
pub fn list_dirs(dir: &Path) -> Result<Vec<String>, String> {
    let read = std::fs::read_dir(dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("{} does not exist", dir.display()),
        std::io::ErrorKind::PermissionDenied => format!("Permission denied: {}", dir.display()),
        _ => e.to_string(),
    })?;
    let mut names: Vec<String> = read
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_queries() {
        assert!(is_path_query("/tmp"));
        assert!(is_path_query("~/code"));
        assert!(is_path_query("./x"));
        assert!(is_path_query("../x"));
        assert!(!is_path_query("theme"));
        assert!(!is_path_query("~code"));
    }

    #[test]
    fn expansion_and_split() {
        let home = Path::new("/home/ada");
        assert_eq!(
            expand("~/code", home, None),
            Some(PathBuf::from("/home/ada/code"))
        );
        assert_eq!(expand("./x", home, None), None);
        assert_eq!(
            expand("../x", home, Some(Path::new("/p/repo"))),
            Some(PathBuf::from("/p/repo/../x"))
        );
        assert_eq!(
            normalize(Path::new("/p/repo/../x/.")),
            PathBuf::from("/p/x")
        );
        assert_eq!(split("/tmp/t3ref/re"), ("/tmp/t3ref/", "re"));
        assert_eq!(split("/tmp/"), ("/tmp/", ""));
    }

    #[test]
    fn filtering_ranks_matches() {
        let entries: Vec<String> = [
            "notes",
            "demo-notes",
            "Notebook",
            ".git",
            "annotations",
            "n-o-t",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(filter_entries(&entries, "notes"), ["notes", "demo-notes"]);
        assert_eq!(
            filter_entries(&entries, "not"),
            ["Notebook", "notes", "demo-notes", "annotations", "n-o-t"]
        );
        assert!(!filter_entries(&entries, "").contains(&".git".to_string()));
        assert_eq!(filter_entries(&entries, ".g"), [".git"]);
    }

    #[test]
    fn completion_prefix() {
        let names = vec!["demo-notes".to_string(), "demo-app".to_string()];
        assert_eq!(common_prefix(&names), "demo-");
        assert_eq!(common_prefix(&["x".to_string()]), "x");
        assert_eq!(common_prefix(&[]), "");
    }
}

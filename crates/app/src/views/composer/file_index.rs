//! A background file index of a project for `@` mentions (spec 04 §4.4). Walks the
//! workspace with the `ignore` crate (honours .gitignore), keeps relative paths of
//! files and folders, and ranks them fuzzily against a query.

use std::path::Path;

/// Entries past this are dropped; huge trees still index their first files.
const MAX_ENTRIES: usize = 50_000;
/// `projects.searchEntries` limit.
pub const RESULT_LIMIT: usize = 80;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Relative to the workspace root, `/`-separated.
    pub path: String,
    pub is_dir: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FileIndex {
    pub entries: Vec<Entry>,
}

impl FileIndex {
    /// Walks `root`. Run it on a background thread.
    pub fn build(root: &Path) -> Self {
        let mut entries = Vec::new();
        let walker = ignore::WalkBuilder::new(root)
            .hidden(true)
            .git_ignore(true)
            .git_exclude(true)
            .parents(true)
            .max_depth(Some(24))
            .build();
        for entry in walker.flatten() {
            if entries.len() >= MAX_ENTRIES {
                break;
            }
            let Ok(rel) = entry.path().strip_prefix(root) else {
                continue;
            };
            if rel.as_os_str().is_empty() {
                continue;
            }
            let path = rel.to_string_lossy().replace('\\', "/");
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            entries.push(Entry { path, is_dir });
        }
        Self { entries }
    }

    /// Best matches for `query`: basename matches before path matches, shorter and
    /// shallower paths first. An empty query lists top-level entries first.
    pub fn search(&self, query: &str) -> Vec<Entry> {
        let query = query.trim().to_lowercase();
        let mut scored: Vec<(u32, usize, &Entry)> = self
            .entries
            .iter()
            .filter_map(|e| score(e, &query).map(|s| (s, e.path.len(), e)))
            .collect();
        scored.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.cmp(&b.1))
                .then(a.2.path.cmp(&b.2.path))
        });
        scored
            .into_iter()
            .take(RESULT_LIMIT)
            .map(|(_, _, e)| e.clone())
            .collect()
    }
}

fn score(entry: &Entry, query: &str) -> Option<u32> {
    let path = entry.path.to_lowercase();
    let depth = path.matches('/').count() as u32;
    if query.is_empty() {
        return Some(depth * 10 + u32::from(!entry.is_dir));
    }
    let base = super::text::basename(&path);
    let s = if base == query {
        0
    } else if base.starts_with(query) {
        10
    } else if base.contains(query) {
        20
    } else if path.starts_with(query) {
        30
    } else if path.contains(query) {
        40
    } else if super::model::is_subsequence(query, base) {
        60
    } else if super::model::is_subsequence(query, &path) {
        80
    } else {
        return None;
    };
    Some(s * 10 + depth.min(9))
}

/// Parent directory shown as the row description (empty for top level).
pub fn parent_dir(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit_once('/')
        .map_or("", |(dir, _)| dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(paths: &[(&str, bool)]) -> FileIndex {
        FileIndex {
            entries: paths
                .iter()
                .map(|(p, d)| Entry {
                    path: p.to_string(),
                    is_dir: *d,
                })
                .collect(),
        }
    }

    #[test]
    fn basename_beats_path_and_fuzzy() {
        let idx = index(&[
            ("src/main.rs", false),
            ("docs/maintenance.md", false),
            ("src/domain/main_loop.rs", false),
            ("src", true),
        ]);
        let hits: Vec<_> = idx.search("main").into_iter().map(|e| e.path).collect();
        assert_eq!(hits[0], "src/main.rs");
        assert!(hits.contains(&"docs/maintenance.md".to_string()));
        let fuzzy: Vec<_> = idx.search("mnrs").into_iter().map(|e| e.path).collect();
        assert_eq!(fuzzy[0], "src/main.rs");
        assert!(idx.search("zzz").is_empty());
    }

    #[test]
    fn empty_query_lists_shallow_first() {
        let idx = index(&[("a/b/c.rs", false), ("README.md", false), ("src", true)]);
        let hits: Vec<_> = idx.search("").into_iter().map(|e| e.path).collect();
        assert_eq!(hits, ["src", "README.md", "a/b/c.rs"]);
    }

    #[test]
    fn walks_a_real_tree_and_honours_gitignore() {
        let dir = std::env::temp_dir().join(format!("tern-index-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".gitignore"), "target\n").unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();
        std::fs::write(dir.join("target/out.o"), "").unwrap();
        let idx = FileIndex::build(&dir);
        let paths: Vec<_> = idx.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"src/lib.rs"));
        assert!(paths.contains(&"src"));
        assert!(!paths.iter().any(|p| p.starts_with("target")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parent_dirs() {
        assert_eq!(parent_dir("src/a/b.rs"), "src/a");
        assert_eq!(parent_dir("b.rs"), "");
    }
}

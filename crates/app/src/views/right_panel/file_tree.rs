//! File trees for the diff panel (spec 05 §2.8) and the file viewer's explorer:
//! a path list folded into directories, single-child directory chains flattened
//! into one row (`a/b/c`), then flattened again into the visible rows.

use std::collections::HashSet;

use tern_git::DiffFile;

use super::diff::rows::natural_cmp;

#[derive(Clone, Debug, PartialEq)]
struct Node {
    /// Row label: one segment, or several joined with `/` for a flattened chain.
    label: String,
    /// Full path from the root.
    path: String,
    children: Vec<Node>,
    /// Some for files.
    file: Option<Option<DiffFile>>,
    /// Position of the first file under this node in the input list.
    first: usize,
}

/// How siblings are ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeOrder {
    /// The diff tree: by first appearance in the (already sorted) file list.
    Input,
    /// The explorer: directories first, then names, case-insensitive and numeric.
    Explorer,
}

/// One visible row.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow {
    pub depth: usize,
    pub label: String,
    pub path: String,
    pub is_dir: bool,
    pub expanded: bool,
    pub status: Option<DiffFile>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileTree {
    roots: Vec<Node>,
}

impl FileTree {
    /// `files`: (path, git status) in display order.
    pub fn build(files: &[(String, Option<DiffFile>)], order: TreeOrder) -> Self {
        let mut roots: Vec<Node> = Vec::new();
        for (ix, (path, status)) in files.iter().enumerate() {
            let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            insert(&mut roots, &segments, ix, *status);
        }
        for root in &mut roots {
            flatten(root);
        }
        sort(&mut roots, order);
        fix_paths(&mut roots, "");
        Self { roots }
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    pub fn has_dirs(&self) -> bool {
        self.roots.iter().any(|n| n.file.is_none())
    }

    /// Every directory path (for "Collapse all folders").
    pub fn dirs(&self) -> Vec<String> {
        let mut out = Vec::new();
        fn go(nodes: &[Node], out: &mut Vec<String>) {
            for n in nodes {
                if n.file.is_none() {
                    out.push(n.path.clone());
                    go(&n.children, out);
                }
            }
        }
        go(&self.roots, &mut out);
        out
    }

    /// Rows with every directory open except those in `collapsed`.
    pub fn rows(&self, collapsed: &HashSet<String>) -> Vec<TreeRow> {
        let mut out = Vec::new();
        fn go(nodes: &[Node], depth: usize, collapsed: &HashSet<String>, out: &mut Vec<TreeRow>) {
            for n in nodes {
                let is_dir = n.file.is_none();
                let expanded = is_dir && !collapsed.contains(&n.path);
                out.push(TreeRow {
                    depth,
                    label: n.label.clone(),
                    path: n.path.clone(),
                    is_dir,
                    expanded,
                    status: n.file.flatten(),
                });
                if expanded {
                    go(&n.children, depth + 1, collapsed, out);
                }
            }
        }
        go(&self.roots, 0, collapsed, &mut out);
        out
    }
}

fn insert(nodes: &mut Vec<Node>, segments: &[&str], ix: usize, status: Option<DiffFile>) {
    let Some((head, rest)) = segments.split_first() else {
        return;
    };
    let is_file = rest.is_empty();
    // A file whose path is also a directory stays a separate node.
    let existing = nodes
        .iter_mut()
        .position(|n| n.label == *head && n.file.is_none() != is_file);
    let node = match existing {
        Some(i) => &mut nodes[i],
        None => {
            nodes.push(Node {
                label: head.to_string(),
                path: String::new(),
                children: Vec::new(),
                file: is_file.then_some(status),
                first: ix,
            });
            nodes.last_mut().unwrap()
        }
    };
    node.first = node.first.min(ix);
    if !is_file {
        insert(&mut node.children, rest, ix, status);
    }
}

/// Sets each node's full path from its (possibly flattened) labels.
fn fix_paths(nodes: &mut [Node], prefix: &str) {
    for n in nodes {
        n.path = if prefix.is_empty() {
            n.label.clone()
        } else {
            format!("{prefix}/{}", n.label)
        };
        let p = n.path.clone();
        fix_paths(&mut n.children, &p);
    }
}

/// Merges a directory with its only child directory (`a` + `b` -> `a/b`).
fn flatten(node: &mut Node) {
    while node.file.is_none() && node.children.len() == 1 && node.children[0].file.is_none() {
        let child = node.children.remove(0);
        node.label = format!("{}/{}", node.label, child.label);
        node.children = child.children;
    }
    for child in &mut node.children {
        flatten(child);
    }
}

fn sort(nodes: &mut [Node], order: TreeOrder) {
    match order {
        TreeOrder::Input => nodes.sort_by(|a, b| {
            a.first
                .cmp(&b.first)
                .then_with(|| natural_cmp(&a.label, &b.label))
        }),
        TreeOrder::Explorer => nodes.sort_by(|a, b| {
            b.file
                .is_none()
                .cmp(&a.file.is_none())
                .then_with(|| natural_cmp(&a.label, &b.label))
        }),
    }
    for n in nodes.iter_mut() {
        sort(&mut n.children, order);
    }
}

/// Keeps the files whose path contains `query` (case-insensitive).
pub fn filter_paths(paths: &[String], query: &str) -> Vec<String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return paths.to_vec();
    }
    paths
        .iter()
        .filter(|p| p.to_lowercase().contains(&q))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> Vec<(String, Option<DiffFile>)> {
        paths.iter().map(|p| (p.to_string(), None)).collect()
    }

    fn labels(rows: &[TreeRow]) -> Vec<(usize, &str)> {
        rows.iter().map(|r| (r.depth, r.label.as_str())).collect()
    }

    #[test]
    fn flattens_single_child_dirs() {
        let tree = FileTree::build(
            &files(&[
                "crates/app/src/main.rs",
                "crates/app/src/lib.rs",
                "README.md",
            ]),
            TreeOrder::Input,
        );
        let rows = tree.rows(&HashSet::new());
        assert_eq!(
            labels(&rows),
            vec![
                (0, "crates/app/src"),
                (1, "main.rs"),
                (1, "lib.rs"),
                (0, "README.md")
            ]
        );
        assert_eq!(rows[0].path, "crates/app/src");
        assert_eq!(rows[1].path, "crates/app/src/main.rs");
        assert!(tree.has_dirs());
    }

    #[test]
    fn explorer_order_dirs_first_case_insensitive() {
        let tree = FileTree::build(
            &files(&[
                "test_notes.py",
                "README.md",
                "src/a.rs",
                "pyproject.toml",
                ".gitignore",
                "notes.py",
            ]),
            TreeOrder::Explorer,
        );
        let rows = tree.rows(&HashSet::new());
        assert_eq!(
            labels(&rows),
            vec![
                (0, "src"),
                (1, "a.rs"),
                (0, ".gitignore"),
                (0, "notes.py"),
                (0, "pyproject.toml"),
                (0, "README.md"),
                (0, "test_notes.py"),
            ]
        );
    }

    #[test]
    fn collapsing_hides_children() {
        let tree = FileTree::build(&files(&["a/x.rs", "a/y.rs", "b.rs"]), TreeOrder::Input);
        let mut collapsed = HashSet::new();
        collapsed.insert("a".to_string());
        let rows = tree.rows(&collapsed);
        assert_eq!(labels(&rows), vec![(0, "a"), (0, "b.rs")]);
        assert!(!rows[0].expanded);
        assert_eq!(tree.dirs(), vec!["a".to_string()]);
    }

    #[test]
    fn keeps_status() {
        let tree = FileTree::build(
            &[("a.rs".to_string(), Some(DiffFile::Added))],
            TreeOrder::Input,
        );
        assert_eq!(tree.rows(&HashSet::new())[0].status, Some(DiffFile::Added));
    }

    #[test]
    fn filters() {
        let paths = vec!["src/Main.rs".to_string(), "README.md".to_string()];
        assert_eq!(
            filter_paths(&paths, "main"),
            vec!["src/Main.rs".to_string()]
        );
        assert_eq!(filter_paths(&paths, " ").len(), 2);
    }
}

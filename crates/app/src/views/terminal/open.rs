//! Opening terminal links (spec 05 §3.11): URLs in the system browser, paths in the
//! user's editor at the line and column.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tern_terminal::split_path_position;

/// Resolves a path link against the terminal's cwd: `~/` expands, relative paths join
/// the cwd, `file://` URIs drop their scheme. Returns the path and its line/column.
pub fn resolve_path(text: &str, cwd: &Path) -> (PathBuf, Option<u32>, Option<u32>) {
    let text = text
        .strip_prefix("file://localhost")
        .or_else(|| text.strip_prefix("file://"))
        .unwrap_or(text);
    let unquoted = text.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    let (path, line, col) = split_path_position(unquoted);
    let path = if let Some(rest) = path.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(rest)
    } else {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        }
    };
    (path, line, col)
}

fn spawn_detached(program: &str, args: &[String]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// Opens a URL in the system browser.
pub fn open_url(url: &str) -> bool {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    spawn_detached(opener, &[url.to_string()])
}

/// The editor command lines to try for a file at a position, most specific first.
pub fn editor_commands(path: &Path, line: Option<u32>, col: Option<u32>) -> Vec<(String, Vec<String>)> {
    let p = path.display().to_string();
    let at = match (line, col) {
        (Some(l), Some(c)) => format!("{p}:{l}:{c}"),
        (Some(l), None) => format!("{p}:{l}"),
        _ => p.clone(),
    };
    let mut out = Vec::new();
    if let Ok(cmd) = std::env::var("TERN_EDITOR")
        && !cmd.trim().is_empty()
    {
        let mut parts = cmd.split_whitespace().map(str::to_string);
        if let Some(prog) = parts.next() {
            let mut args: Vec<String> = parts.collect();
            args.push(at.clone());
            out.push((prog, args));
        }
    }
    for editor in ["cursor", "code"] {
        out.push((editor.to_string(), vec!["-g".into(), at.clone()]));
    }
    out.push(("zed".to_string(), vec![at.clone()]));
    out
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())
    })
}

/// Opens a file or folder in the preferred editor, falling back to the system opener.
/// Returns false when nothing could be started.
pub fn open_path(path: &Path, line: Option<u32>, col: Option<u32>) -> bool {
    if path.is_file() {
        for (program, args) in editor_commands(path, line, col) {
            if (program.contains('/') || on_path(&program)) && spawn_detached(&program, &args) {
                return true;
            }
        }
    }
    open_url(&path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_and_positions() {
        let cwd = Path::new("/work/repo");
        assert_eq!(
            resolve_path("src/main.rs:12:3", cwd),
            (PathBuf::from("/work/repo/src/main.rs"), Some(12), Some(3))
        );
        assert_eq!(
            resolve_path("/etc/hosts", cwd),
            (PathBuf::from("/etc/hosts"), None, None)
        );
        assert_eq!(
            resolve_path("file:///tmp/x.txt", cwd),
            (PathBuf::from("/tmp/x.txt"), None, None)
        );
        assert_eq!(resolve_path("../a.rs:4", cwd).1, Some(4));
    }

    #[test]
    fn editor_args_carry_position() {
        let cmds = editor_commands(Path::new("/a/b.rs"), Some(3), Some(9));
        assert!(cmds.iter().any(|(p, a)| p == "code" && a == &["-g", "/a/b.rs:3:9"]));
        let cmds = editor_commands(Path::new("/a/b.rs"), None, None);
        assert!(cmds.iter().any(|(p, a)| p == "zed" && a == &["/a/b.rs"]));
    }
}

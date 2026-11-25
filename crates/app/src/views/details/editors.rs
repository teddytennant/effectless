//! The open-in picker's editors (spec/05 §5.3-5.5): detection on Linux and launch
//! arguments. Detection shells out, so it runs on a background thread.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchStyle {
    /// VS Code family: `--new-window <dir>`, `--goto <file>:<line>`.
    Goto,
    /// Zed, the file manager: the path as is.
    DirectPath,
    /// JetBrains: `--line <n> <file>`.
    LineColumn,
    /// A terminal editor, run inside a terminal emulator.
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Editor {
    pub id: &'static str,
    pub label: &'static str,
    pub commands: &'static [&'static str],
    pub base_args: &'static [&'static str],
    pub style: LaunchStyle,
}

const fn ed(
    id: &'static str,
    label: &'static str,
    commands: &'static [&'static str],
    base_args: &'static [&'static str],
    style: LaunchStyle,
) -> Editor {
    Editor {
        id,
        label,
        commands,
        base_args,
        style,
    }
}

pub const FILE_MANAGER: &str = "file-manager";

/// Picker order. Neovim is Tern's own addition (it runs in a terminal emulator).
pub const EDITORS: &[Editor] = &[
    ed(
        "cursor",
        "Cursor",
        &["cursor"],
        &["--classic"],
        LaunchStyle::Goto,
    ),
    ed("trae", "Trae", &["trae"], &[], LaunchStyle::Goto),
    ed("kiro", "Kiro", &["kiro"], &["ide"], LaunchStyle::Goto),
    ed("vscode", "VS Code", &["code"], &[], LaunchStyle::Goto),
    ed(
        "vscode-insiders",
        "VS Code Insiders",
        &["code-insiders"],
        &[],
        LaunchStyle::Goto,
    ),
    ed("vscodium", "VSCodium", &["codium"], &[], LaunchStyle::Goto),
    ed(
        "zed",
        "Zed",
        &["zeditor", "zed"],
        &[],
        LaunchStyle::DirectPath,
    ),
    ed(
        "antigravity",
        "Antigravity",
        &["antigravity-ide", "agy-ide"],
        &[],
        LaunchStyle::Goto,
    ),
    ed(
        "idea",
        "IntelliJ IDEA",
        &["idea"],
        &[],
        LaunchStyle::LineColumn,
    ),
    ed("aqua", "Aqua", &["aqua"], &[], LaunchStyle::LineColumn),
    ed("clion", "CLion", &["clion"], &[], LaunchStyle::LineColumn),
    ed(
        "datagrip",
        "DataGrip",
        &["datagrip"],
        &[],
        LaunchStyle::LineColumn,
    ),
    ed(
        "dataspell",
        "DataSpell",
        &["dataspell"],
        &[],
        LaunchStyle::LineColumn,
    ),
    ed(
        "goland",
        "GoLand",
        &["goland"],
        &[],
        LaunchStyle::LineColumn,
    ),
    ed(
        "phpstorm",
        "PhpStorm",
        &["phpstorm"],
        &[],
        LaunchStyle::LineColumn,
    ),
    ed(
        "pycharm",
        "PyCharm",
        &["pycharm"],
        &[],
        LaunchStyle::LineColumn,
    ),
    ed("rider", "Rider", &["rider"], &[], LaunchStyle::LineColumn),
    ed(
        "rubymine",
        "RubyMine",
        &["rubymine"],
        &[],
        LaunchStyle::LineColumn,
    ),
    ed(
        "rustrover",
        "RustRover",
        &["rustrover"],
        &[],
        LaunchStyle::LineColumn,
    ),
    ed(
        "webstorm",
        "WebStorm",
        &["webstorm"],
        &[],
        LaunchStyle::LineColumn,
    ),
    ed("nvim", "Neovim", &["nvim"], &[], LaunchStyle::Terminal),
    ed(
        FILE_MANAGER,
        "Files",
        &["xdg-open"],
        &[],
        LaunchStyle::DirectPath,
    ),
];

pub fn editor(id: &str) -> Option<&'static Editor> {
    EDITORS.iter().find(|e| e.id == id)
}

/// An editor found on this machine, with the binary to run.
#[derive(Clone, Debug, PartialEq)]
pub struct Available {
    pub id: &'static str,
    pub label: &'static str,
    pub program: PathBuf,
}

fn fallback_dirs() -> Vec<PathBuf> {
    let home = dirs_home();
    let mut dirs = vec![
        home.join(".local/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/snap/bin"),
    ];
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    dirs.push(data.join("JetBrains/Toolbox/scripts"));
    dirs
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// The first `name` on `PATH`, then in the fallback install dirs.
pub fn which(name: &str, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let from_path = path_var
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .map(|d| d.join(name));
    from_path
        .chain(fallback_dirs().into_iter().map(|d| d.join(name)))
        .find(|p| is_executable(p))
}

/// Runs `program args` with a 2s budget. `Some(stdout)` on exit 0; a timeout counts as
/// a pass with empty output (Cursor's check).
fn probe(program: &Path, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut s) = child.stdout.take() {
                    use std::io::Read as _;
                    let _ = s.read_to_string(&mut out);
                }
                return status.success().then_some(out);
            }
            Ok(None) if start.elapsed() > Duration::from_secs(2) => {
                let _ = child.kill();
                return Some(String::new());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    }
}

fn graphical_session() -> bool {
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn file_manager_available(xdg_open: &Path) -> bool {
    if !graphical_session() || !is_executable(xdg_open) {
        return false;
    }
    let Some(xdg_mime) = which("xdg-mime", std::env::var_os("PATH").as_deref()) else {
        return false;
    };
    probe(&xdg_mime, &["query", "default", "inode/directory"])
        .is_some_and(|out| !out.trim().is_empty())
}

/// `buildAvailableEditors` for Linux, in picker order.
pub fn detect() -> Vec<Available> {
    let path = std::env::var_os("PATH");
    let mut found = Vec::new();
    for e in EDITORS {
        let program = if e.id == FILE_MANAGER {
            which("xdg-open", path.as_deref()).filter(|p| file_manager_available(p))
        } else if e.id == "zed" {
            e.commands.iter().find_map(|c| {
                let p = which(c, path.as_deref())?;
                probe(&p, &["--version"])
                    .filter(|out| out.starts_with("Zed"))
                    .map(|_| p)
            })
        } else if e.id == "cursor" {
            which("cursor", path.as_deref()).filter(|p| probe(p, &["--version"]).is_some())
        } else if e.style == LaunchStyle::Terminal {
            which(e.commands[0], path.as_deref()).filter(|_| terminal_emulator().is_some())
        } else {
            e.commands.iter().find_map(|c| which(c, path.as_deref()))
        };
        if let Some(program) = program {
            found.push(Available {
                id: e.id,
                label: e.label,
                program,
            });
        }
    }
    found
}

/// A terminal emulator to host a terminal editor: `$TERMINAL`, then common ones.
pub fn terminal_emulator() -> Option<PathBuf> {
    let path = std::env::var_os("PATH");
    if let Some(t) = std::env::var_os("TERMINAL")
        && let Some(p) = which(&t.to_string_lossy(), path.as_deref())
    {
        return Some(p);
    }
    [
        "x-terminal-emulator",
        "foot",
        "kitty",
        "alacritty",
        "wezterm",
        "ghostty",
        "gnome-terminal",
        "konsole",
        "xterm",
    ]
    .into_iter()
    .find_map(|t| which(t, path.as_deref()))
}

/// Arguments for opening `target` (a folder or file) with `editor` (§5.5 table).
pub fn launch_args(
    editor: &Editor,
    target: &Path,
    position: Option<(u32, Option<u32>)>,
) -> Vec<String> {
    let mut args: Vec<String> = editor.base_args.iter().map(|s| s.to_string()).collect();
    let target_s = target.to_string_lossy().to_string();
    let is_dir = target.is_dir();
    match (editor.style, position) {
        (LaunchStyle::Goto, _) if is_dir => {
            args.push("--new-window".into());
            args.push(target_s);
        }
        (LaunchStyle::Goto, Some((line, col))) => {
            args.push("--goto".into());
            args.push(match col {
                Some(c) => format!("{target_s}:{line}:{c}"),
                None => format!("{target_s}:{line}"),
            });
        }
        (LaunchStyle::LineColumn, Some((line, col))) if !is_dir => {
            args.push("--line".into());
            args.push(line.to_string());
            if let Some(c) = col {
                args.push("--column".into());
                args.push(c.saturating_sub(1).to_string());
            }
            args.push(target_s);
        }
        (LaunchStyle::DirectPath, Some((line, col))) if !is_dir && editor.id != FILE_MANAGER => {
            args.push(match col {
                Some(c) => format!("{target_s}:{line}:{c}"),
                None => format!("{target_s}:{line}"),
            });
        }
        _ => args.push(target_s),
    }
    args
}

/// Spawns detached, stdio ignored, not awaited.
pub fn launch(available: &Available, target: &Path) -> Result<(), String> {
    let editor = editor(available.id).ok_or("Unknown editor")?;
    let args = launch_args(editor, target, None);
    let mut cmd = if editor.style == LaunchStyle::Terminal {
        let term = terminal_emulator().ok_or("No terminal emulator found to run Neovim.")?;
        let mut c = Command::new(term);
        c.arg("-e").arg(&available.program).args(&args);
        c
    } else {
        let mut c = Command::new(&available.program);
        c.args(&args);
        c
    };
    if target.is_dir() {
        cmd.current_dir(target);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|mut child| {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        })
        .map_err(|e| e.to_string())
}

/// The preferred editor (`t3code:last-editor`), kept for the session.
static PREFERRED: Mutex<Option<&'static str>> = Mutex::new(None);

pub fn preferred() -> Option<&'static str> {
    PREFERRED.lock().ok().and_then(|p| *p)
}

pub fn set_preferred(id: &'static str) {
    if let Ok(mut p) = PREFERRED.lock() {
        *p = Some(id);
    }
}

/// The preferred editor if installed, else the first installed one.
pub fn resolve_preferred<'a>(
    available: &'a [Available],
    preferred: Option<&str>,
) -> Option<&'a Available> {
    preferred
        .and_then(|id| available.iter().find(|a| a.id == id))
        .or_else(|| available.first())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_argument_styles() {
        let dir = std::env::temp_dir();
        let code = editor("vscode").unwrap();
        assert_eq!(
            launch_args(code, &dir, None),
            vec![
                "--new-window".to_string(),
                dir.to_string_lossy().to_string()
            ]
        );
        let file = Path::new("/nonexistent/src/main.rs");
        assert_eq!(
            launch_args(code, file, Some((12, Some(3)))),
            vec!["--goto", "/nonexistent/src/main.rs:12:3"]
        );
        let cursor = editor("cursor").unwrap();
        assert_eq!(launch_args(cursor, file, None)[0], "--classic");
        let idea = editor("idea").unwrap();
        assert_eq!(
            launch_args(idea, file, Some((4, Some(2)))),
            vec!["--line", "4", "--column", "1", "/nonexistent/src/main.rs"]
        );
        let zed = editor("zed").unwrap();
        assert_eq!(
            launch_args(zed, file, Some((4, None))),
            vec!["/nonexistent/src/main.rs:4"]
        );
        let files = editor(FILE_MANAGER).unwrap();
        assert_eq!(
            launch_args(files, &dir, None),
            vec![dir.to_string_lossy().to_string()]
        );
    }

    #[test]
    fn preferred_falls_back_to_first() {
        let a = vec![
            Available {
                id: "zed",
                label: "Zed",
                program: "/bin/zed".into(),
            },
            Available {
                id: FILE_MANAGER,
                label: "Files",
                program: "/bin/xdg-open".into(),
            },
        ];
        assert_eq!(resolve_preferred(&a, None).unwrap().id, "zed");
        assert_eq!(
            resolve_preferred(&a, Some(FILE_MANAGER)).unwrap().id,
            FILE_MANAGER
        );
        assert_eq!(resolve_preferred(&a, Some("cursor")).unwrap().id, "zed");
        assert!(resolve_preferred(&[], Some("zed")).is_none());
    }

    #[test]
    fn which_finds_executables_only() {
        let dir = std::env::temp_dir().join(format!("tern-which-{}", tern_core::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("fake-editor");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        assert_eq!(which("fake-editor", Some(dir.as_os_str())), None);
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(which("fake-editor", Some(dir.as_os_str())), Some(bin));
        let _ = std::fs::remove_dir_all(dir);
    }
}

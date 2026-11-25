//! Shell candidates and spawn environment (spec/07 §8.11-8.12, spec/05 §3.15).
//! Ported from t3code `apps/server/src/terminal/Manager.ts`, with the `T3CODE_` prefix
//! renamed to `TERN_`.

use std::path::Path;

/// Default grid size for a new terminal (`DEFAULT_OPEN_COLS/ROWS`).
pub const DEFAULT_COLS: u16 = 120;
pub const DEFAULT_ROWS: u16 = 30;
/// Schema limits from `packages/contracts/src/terminal.ts`.
pub const MAX_COLS: u16 = 1000;
pub const MAX_ROWS: u16 = 500;
/// Limits on the caller's env (`TerminalOpenInput.env`).
pub const MAX_ENV_KEYS: usize = 128;
pub const MAX_ENV_VALUE_LEN: usize = 8192;

pub const TERM_NAME: &str = "xterm-256color";
pub const TERM_PROGRAM: &str = "tern";
pub const SHELL_ENV: &str = "TERN_TERMINAL_SHELL";
pub const THREAD_ID_ENV: &str = "TERN_THREAD_ID";
pub const PROJECT_ROOT_ENV: &str = "TERN_PROJECT_ROOT";
pub const WORKTREE_PATH_ENV: &str = "TERN_WORKTREE_PATH";

/// The tmux/screen/Zellij keys name the multiplexer pane that launched the app, so
/// inheriting them points multiplexer-aware tools at an unrelated pane.
const ENV_BLOCKLIST: &[&str] = &[
    "PORT",
    "ELECTRON_RENDERER_PORT",
    "ELECTRON_RUN_AS_NODE",
    "TMUX",
    "TMUX_PANE",
    "STY",
    "WINDOW",
    "ZELLIJ",
    "ZELLIJ_SESSION_NAME",
    "ZELLIJ_PANE_ID",
];

const HOME_PATH_VARIABLES: &[&str] = &["CODEX_HOME", "CLAUDE_CONFIG_DIR", "GROK_HOME"];
const APPIMAGE_RUNTIME_ENV_KEYS: &[&str] = &["APPIMAGE", "APPDIR", "ARGV0", "OWD"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellCandidate {
    pub shell: String,
    pub args: Vec<String>,
}

/// The env a drawer terminal is opened with (spec/05 §3.14):
/// `TERN_PROJECT_ROOT`, plus `TERN_WORKTREE_PATH` when the thread has a worktree.
pub fn launch_env(project_root: &Path, worktree_path: Option<&Path>) -> Vec<(String, String)> {
    let mut env = vec![(
        PROJECT_ROOT_ENV.to_string(),
        project_root.to_string_lossy().into_owned(),
    )];
    if let Some(worktree) = worktree_path {
        env.push((
            WORKTREE_PATH_ENV.to_string(),
            worktree.to_string_lossy().into_owned(),
        ));
    }
    env
}

fn excluded_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.starts_with("TERN_")
        || upper.starts_with("VITE_")
        || ENV_BLOCKLIST.contains(&upper.as_str())
}

fn valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Checks the caller's env against the open schema.
pub fn validate_env(env: &[(String, String)]) -> anyhow::Result<()> {
    if env.len() > MAX_ENV_KEYS {
        anyhow::bail!("Terminal env has more than {MAX_ENV_KEYS} keys");
    }
    for (key, value) in env {
        if !valid_env_key(key) {
            anyhow::bail!("Invalid terminal env key: {key}");
        }
        if value.encode_utf16().count() > MAX_ENV_VALUE_LEN {
            anyhow::bail!("Terminal env value for {key} is longer than {MAX_ENV_VALUE_LEN}");
        }
    }
    Ok(())
}

fn expand_home_variable(name: &str, value: &str, home: &str) -> String {
    if !HOME_PATH_VARIABLES.contains(&name) || value.is_empty() {
        return value.to_string();
    }
    if value == "~" {
        return home.to_string();
    }
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return Path::new(home).join(rest).to_string_lossy().into_owned();
    }
    value.to_string()
}

fn set(env: &mut Vec<(String, String)>, key: &str, value: String) {
    match env.iter_mut().find(|(k, _)| k == key) {
        Some(entry) => entry.1 = value,
        None => env.push((key.to_string(), value)),
    }
}

fn get<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
    env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// AppImage mount paths would make child commands resolve against the mount.
fn strip_appimage_runtime_env(env: &mut Vec<(String, String)>) {
    if get(env, "APPIMAGE").is_none() && get(env, "APPDIR").is_none() {
        return;
    }
    let app_dir = get(env, "APPDIR").map(|d| d.trim_end_matches('/').to_string());
    env.retain(|(k, _)| !APPIMAGE_RUNTIME_ENV_KEYS.contains(&k.as_str()));
    let Some(app_dir) = app_dir.filter(|d| !d.is_empty()) else {
        return;
    };
    let under = |segment: &str| segment == app_dir || segment.starts_with(&format!("{app_dir}/"));
    env.retain_mut(|(key, value)| {
        let separators: &[char] = match key.as_str() {
            "PATH" | "XDG_DATA_DIRS" | "GSETTINGS_SCHEMA_DIR" => &[':'],
            "LD_LIBRARY_PATH" => &[':', ';'],
            _ => return true,
        };
        let segments: Vec<&str> = value.split(separators).collect();
        let kept: Vec<&str> = segments.iter().copied().filter(|s| !under(s)).collect();
        if kept.len() == segments.len() {
            return true;
        }
        if kept.is_empty() {
            return false;
        }
        // Empty entries mean the current directory; a lone one must stay ":".
        *value = if kept.len() == 1 && kept[0].is_empty() {
            ":".to_string()
        } else {
            kept.join(":")
        };
        true
    });
}

/// `createTerminalSpawnEnv`: inherit `base` minus T3/Tern-private and multiplexer keys,
/// then layer the terminal identity, the caller's env, the thread id and the defaults.
pub fn spawn_env(
    base: impl IntoIterator<Item = (String, String)>,
    runtime_env: &[(String, String)],
    thread_id: Option<&str>,
    home: &str,
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> =
        base.into_iter().filter(|(k, _)| !excluded_key(k)).collect();
    // The app inherits its launcher's identity (e.g. Apple_Terminal); tools inside the
    // integrated terminal should see Tern instead.
    set(&mut env, "TERM_PROGRAM", TERM_PROGRAM.to_string());
    set(
        &mut env,
        "TERM_PROGRAM_VERSION",
        env!("CARGO_PKG_VERSION").to_string(),
    );
    for (key, value) in runtime_env {
        set(&mut env, key, expand_home_variable(key, value, home));
    }
    if let Some(thread_id) = thread_id {
        set(&mut env, THREAD_ID_ENV, thread_id.to_string());
    }
    let colorterm_set = get(&env, "COLORTERM").is_some_and(|v| !v.is_empty());
    if !colorterm_set && !runtime_env.iter().any(|(k, _)| k == "COLORTERM") {
        set(&mut env, "COLORTERM", "truecolor".to_string());
    }
    // fish 4.1+ waits 10s at startup for a DA1 reply. Skip the query unless the user
    // already chose `query-term` or `no-query-term`.
    match get(&env, "fish_features").map(str::to_string) {
        Some(features) if features.contains("query-term") => {}
        Some(features) if !features.is_empty() => set(
            &mut env,
            "fish_features",
            format!("{features},no-query-term"),
        ),
        _ => set(&mut env, "fish_features", "no-query-term".to_string()),
    }
    strip_appimage_runtime_env(&mut env);
    // node-pty's `name` becomes TERM on Unix.
    set(&mut env, "TERM", TERM_NAME.to_string());
    env
}

/// `normalizeShellCommand`: turns a `$SHELL`-style value into an executable path.
pub fn normalize_shell_command(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        return None;
    }
    if cfg!(windows) {
        return Some(trimmed.to_string());
    }
    // ^(['"])(.*?)\1(?:\s|$)
    if let Some(quote) = trimmed.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let rest = &trimmed[1..];
        let mut search = 0;
        while let Some(pos) = rest[search..].find(quote) {
            let end = search + pos;
            let after = &rest[end + 1..];
            if after.is_empty() || after.starts_with(char::is_whitespace) {
                let inner = &rest[..end];
                return (!inner.is_empty()).then(|| inner.to_string());
            }
            search = end + 1;
        }
    }
    if trimmed.starts_with('/') && trimmed.contains(char::is_whitespace) {
        // Unquoted paths are ambiguous; prefer the longest existing file over flags.
        let mut ends = vec![trimmed.len()];
        let mut in_ws = false;
        let mut starts = Vec::new();
        for (i, c) in trimmed.char_indices() {
            if c.is_whitespace() {
                if !in_ws {
                    starts.push(i);
                }
                in_ws = true;
            } else {
                in_ws = false;
            }
        }
        ends.extend(starts.into_iter().rev());
        for end in ends {
            let candidate = &trimmed[..end];
            if Path::new(candidate).is_file() {
                return Some(candidate.to_string());
            }
        }
    }
    // split(/\s+(?=-)/)[0]
    let mut first = trimmed;
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if (bytes[i] as char).is_ascii_whitespace() {
            let mut j = i;
            while j < bytes.len() && (bytes[j] as char).is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'-' {
                first = &trimmed[..i];
                break;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    let first = first.strip_prefix(['"', '\'']).unwrap_or(first);
    let first = first.strip_suffix(['"', '\'']).unwrap_or(first);
    (!first.is_empty()).then(|| first.to_string())
}

/// `shellCandidateFromCommand`: per-shell args. zsh gets `-o nopromptsp`; bash is a
/// login shell only on macOS; everything else runs with no args.
pub fn shell_candidate(command: &str) -> ShellCandidate {
    let name = command
        .rsplit(['/', '\\'])
        .find(|p| !p.is_empty())
        .unwrap_or(command)
        .to_ascii_lowercase();
    let args: &[&str] = if cfg!(windows) {
        match name.as_str() {
            "pwsh.exe" | "powershell.exe" => &[
                "-NoLogo",
                "-NoExit",
                "-Command",
                "[Console]::OutputEncoding = $OutputEncoding = [System.Text.UTF8Encoding]::new()",
            ],
            "bash.exe" | "bash" => &["--login", "-i"],
            _ => &[],
        }
    } else if name == "zsh" {
        &["-o", "nopromptsp"]
    } else if cfg!(target_os = "macos") && name == "bash" {
        &["-l"]
    } else {
        &[]
    };
    ShellCandidate {
        shell: command.to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
    }
}

/// `resolveShellCandidates`, in spawn order and deduped. `configured` is the explicit
/// shell (`TerminalOptions::shell`, else `TERN_TERMINAL_SHELL` from the open's env or
/// the app's env); `shell_env` is the app's `$SHELL`.
pub fn shell_candidates(configured: Option<&str>, shell_env: Option<&str>) -> Vec<ShellCandidate> {
    let configured = configured
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(shell_candidate);
    let mut list: Vec<Option<ShellCandidate>> = vec![configured];
    if cfg!(windows) {
        list.push(normalize_shell_command(Some("pwsh.exe")).map(|s| shell_candidate(&s)));
        for c in ["powershell.exe", "cmd.exe"] {
            list.push(Some(shell_candidate(c)));
        }
    } else {
        // The resolver is `env.SHELL ?? "bash"`, then `$SHELL` again (deduped).
        let requested = normalize_shell_command(Some(shell_env.unwrap_or("bash")));
        list.push(requested.map(|s| shell_candidate(&s)));
        list.push(normalize_shell_command(shell_env).map(|s| shell_candidate(&s)));
        for c in ["/bin/zsh", "/bin/bash", "/bin/sh", "zsh", "bash", "sh"] {
            list.push(Some(shell_candidate(c)));
        }
    }
    let mut out: Vec<ShellCandidate> = Vec::new();
    for candidate in list.into_iter().flatten() {
        if !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}

/// Clamps a requested grid to the schema limits; 0 means the default.
pub fn clamp_size(cols: u16, rows: u16) -> (u16, u16) {
    let cols = if cols == 0 {
        DEFAULT_COLS
    } else {
        cols.min(MAX_COLS)
    };
    let rows = if rows == 0 {
        DEFAULT_ROWS
    } else {
        rows.min(MAX_ROWS)
    };
    (cols, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn env_strips_private_and_multiplexer_keys() {
        let base = kv(&[
            ("PATH", "/usr/bin"),
            ("TERN_SECRET", "x"),
            ("tern_lower", "x"),
            ("VITE_PORT", "1"),
            ("TMUX", "/tmp/tmux"),
            ("ZELLIJ_PANE_ID", "3"),
            ("PORT", "8080"),
            ("TERM", "screen"),
        ]);
        let env = spawn_env(base, &[], Some("thread-1"), "/home/u");
        let get = |k: &str| get(&env, k).map(str::to_string);
        assert_eq!(get("PATH").as_deref(), Some("/usr/bin"));
        for k in [
            "TERN_SECRET",
            "tern_lower",
            "VITE_PORT",
            "TMUX",
            "ZELLIJ_PANE_ID",
            "PORT",
        ] {
            assert_eq!(get(k), None, "{k}");
        }
        assert_eq!(get("TERM").as_deref(), Some("xterm-256color"));
        assert_eq!(get("TERM_PROGRAM").as_deref(), Some("tern"));
        assert_eq!(get("COLORTERM").as_deref(), Some("truecolor"));
        assert_eq!(get("TERN_THREAD_ID").as_deref(), Some("thread-1"));
        assert_eq!(get("fish_features").as_deref(), Some("no-query-term"));
    }

    #[test]
    fn env_runtime_values_win_and_expand_home() {
        let base = kv(&[("COLORTERM", "24bit"), ("fish_features", "foo")]);
        let runtime = kv(&[
            ("TERN_PROJECT_ROOT", "/p"),
            ("CODEX_HOME", "~/.codex"),
            ("OTHER", "~/x"),
        ]);
        let env = spawn_env(base, &runtime, None, "/home/u");
        assert_eq!(get(&env, "TERN_PROJECT_ROOT"), Some("/p"));
        assert_eq!(get(&env, "CODEX_HOME"), Some("/home/u/.codex"));
        assert_eq!(get(&env, "OTHER"), Some("~/x"));
        assert_eq!(get(&env, "COLORTERM"), Some("24bit"));
        assert_eq!(get(&env, "fish_features"), Some("foo,no-query-term"));
        assert_eq!(get(&env, "TERN_THREAD_ID"), None);
    }

    #[test]
    fn env_colorterm_left_alone_when_runtime_sets_it_empty() {
        let env = spawn_env(Vec::new(), &kv(&[("COLORTERM", "")]), None, "/h");
        assert_eq!(get(&env, "COLORTERM"), Some(""));
    }

    #[test]
    fn env_strips_appimage() {
        let base = kv(&[
            ("APPIMAGE", "/a.AppImage"),
            ("APPDIR", "/tmp/.mount_x/"),
            ("PATH", "/tmp/.mount_x/usr/bin:/usr/bin"),
            ("LD_LIBRARY_PATH", "/tmp/.mount_x/lib"),
        ]);
        let env = spawn_env(base, &[], None, "/h");
        assert_eq!(get(&env, "APPIMAGE"), None);
        assert_eq!(get(&env, "APPDIR"), None);
        assert_eq!(get(&env, "PATH"), Some("/usr/bin"));
        assert_eq!(get(&env, "LD_LIBRARY_PATH"), None);
    }

    #[test]
    fn launch_env_has_root_and_worktree() {
        let env = launch_env(Path::new("/repo"), Some(Path::new("/wt")));
        assert_eq!(get(&env, "TERN_PROJECT_ROOT"), Some("/repo"));
        assert_eq!(get(&env, "TERN_WORKTREE_PATH"), Some("/wt"));
        assert_eq!(launch_env(Path::new("/repo"), None).len(), 1);
    }

    #[test]
    fn env_validation() {
        assert!(validate_env(&kv(&[("A_1", "x")])).is_ok());
        assert!(validate_env(&kv(&[("1A", "x")])).is_err());
        assert!(validate_env(&kv(&[("A-B", "x")])).is_err());
        assert!(validate_env(&kv(&[("A", &"x".repeat(8193))])).is_err());
    }

    #[test]
    fn normalizes_shell_values() {
        assert_eq!(normalize_shell_command(Some("  ")), None);
        assert_eq!(
            normalize_shell_command(Some("/bin/zsh")).as_deref(),
            Some("/bin/zsh")
        );
        assert_eq!(
            normalize_shell_command(Some("\"/opt/my shell/bin/fish\" -l")).as_deref(),
            Some("/opt/my shell/bin/fish")
        );
        assert_eq!(
            normalize_shell_command(Some("fish --login")).as_deref(),
            Some("fish")
        );
        // Not an existing file: falls back to the text before the first flag.
        assert_eq!(
            normalize_shell_command(Some("/nope/zsh -l")).as_deref(),
            Some("/nope/zsh")
        );
        assert_eq!(
            normalize_shell_command(Some("/bin/sh -l")).as_deref(),
            Some("/bin/sh")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn candidates_follow_t3_order() {
        let list = shell_candidates(None, Some("/usr/bin/fish"));
        let shells: Vec<&str> = list.iter().map(|c| c.shell.as_str()).collect();
        assert_eq!(
            shells,
            [
                "/usr/bin/fish",
                "/bin/zsh",
                "/bin/bash",
                "/bin/sh",
                "zsh",
                "bash",
                "sh"
            ]
        );
        assert!(list[0].args.is_empty());
        assert_eq!(list[1].args, ["-o", "nopromptsp"]);
        // bash is not a login shell on Linux.
        assert!(list[2].args.is_empty());

        let list = shell_candidates(Some("/bin/sh"), None);
        assert_eq!(list[0].shell, "/bin/sh");
        assert_eq!(list[1].shell, "bash");
    }

    #[test]
    fn clamps_sizes() {
        assert_eq!(clamp_size(0, 0), (120, 30));
        assert_eq!(clamp_size(5000, 900), (1000, 500));
        assert_eq!(clamp_size(80, 24), (80, 24));
    }
}

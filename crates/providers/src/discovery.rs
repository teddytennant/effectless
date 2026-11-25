//! Finding a provider CLI and asking its version (settings override first, then PATH).

use std::path::{Path, PathBuf};
use std::time::Duration;

/// `~` and `~/...` expand to `$HOME` (CA:2068, ClaudeDriver.ts:133).
pub fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if (s == "~" || s.starts_with("~/"))
        && let Some(home) = std::env::var_os("HOME")
    {
        let mut p = PathBuf::from(home);
        if s.len() > 2 {
            p.push(&s[2..]);
        }
        return p;
    }
    path.to_path_buf()
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Resolves the CLI to run. A settings override that names a bare command (no `/`) is
/// looked up on PATH like the default; a path must exist and be executable.
pub fn find_binary(override_path: Option<&Path>, default_name: &str) -> Option<PathBuf> {
    let wanted = match override_path {
        Some(p) if !p.as_os_str().is_empty() => expand_tilde(p),
        _ => PathBuf::from(default_name),
    };
    if wanted.components().count() > 1 || wanted.is_absolute() {
        return is_executable(&wanted).then_some(wanted);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(&wanted))
        .find(|p| is_executable(p))
}

/// First `N.N.N` in the text: `codex-cli 0.160.1`, `2.1.332 (Claude Code)`.
pub(crate) fn parse_version(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || c == '/' || c == '(' || c == ')')
        .map(|w| w.trim_start_matches('v'))
        .find(|w| {
            let parts: Vec<&str> = w.split('.').collect();
            parts.len() >= 3
                && parts[..3]
                    .iter()
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        })
        .map(str::to_string)
}

/// Compares dotted numeric versions; non-numeric parts count as 0.
pub(crate) fn version_at_least(version: &str, min: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    let (a, b) = (parse(version), parse(min));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    true
}

pub(crate) enum VersionProbe {
    Ok(String),
    Failed(String),
}

/// Runs `<bin> --version` with a timeout.
pub(crate) async fn probe_version(bin: &Path, timeout: Duration) -> VersionProbe {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("--version")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    match tokio::time::timeout(timeout, cmd.output()).await {
        Err(_) => VersionProbe::Failed("Timed out while running command.".into()),
        Ok(Err(e)) => VersionProbe::Failed(e.to_string()),
        Ok(Ok(out)) if !out.status.success() => VersionProbe::Failed(format!(
            "exited with {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Ok(Ok(out)) => {
            let text = String::from_utf8_lossy(&out.stdout);
            match parse_version(&text) {
                Some(v) => VersionProbe::Ok(v),
                None => {
                    VersionProbe::Failed(format!("unrecognized version output: {}", text.trim()))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_versions() {
        assert_eq!(
            parse_version("codex-cli 0.160.1\n").as_deref(),
            Some("0.160.1")
        );
        assert_eq!(
            parse_version("2.1.332 (Claude Code)").as_deref(),
            Some("2.1.332")
        );
        assert_eq!(
            parse_version("Tern/0.160.1 (Linux; x86_64)").as_deref(),
            Some("0.160.1")
        );
        assert_eq!(parse_version("nothing"), None);
    }

    #[test]
    fn compares_versions() {
        assert!(version_at_least("2.1.332", "2.1.321"));
        assert!(!version_at_least("2.1.300", "2.1.321"));
        assert!(version_at_least("0.160.1", "0.160.1"));
    }

    #[test]
    fn finds_sh_on_path_and_rejects_missing_paths() {
        assert!(find_binary(None, "sh").is_some());
        assert!(find_binary(Some(Path::new("/nonexistent/tern-cli")), "sh").is_none());
        assert!(find_binary(None, "tern-definitely-not-installed").is_none());
    }
}

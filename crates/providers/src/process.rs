//! Child processes: own process group, stderr tail, signals, and a bounded line reader.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt};
use tokio::process::{ChildStdin, ChildStdout};

/// Max incoming line (proto:22): 128 MiB.
pub(crate) const MAX_LINE_BYTES: usize = 128 * 1024 * 1024;
const STDERR_TAIL_CHARS: usize = 8192;

pub(crate) struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: HashMap<String, String>,
    pub env_remove: Vec<String>,
}

pub(crate) struct Proc {
    pid: i32,
    child: tokio::sync::Mutex<tokio::process::Child>,
    exit: Mutex<Option<ExitStatus>>,
    stderr_tail: Arc<Mutex<String>>,
}

impl Proc {
    /// Spawns in a new process group so the whole agent tree can be signalled at once.
    pub fn spawn(spec: &SpawnSpec) -> std::io::Result<(Arc<Proc>, ChildStdin, ChildStdout)> {
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for k in &spec.env_remove {
            cmd.env_remove(k);
        }
        cmd.envs(&spec.env);
        let mut child = cmd.spawn()?;
        let pid = child.id().map(|p| p as i32).unwrap_or(0);
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let tail = Arc::new(Mutex::new(String::new()));
        tokio::spawn(drain_stderr(stderr, tail.clone()));
        let proc = Arc::new(Proc {
            pid,
            child: tokio::sync::Mutex::new(child),
            exit: Mutex::new(None),
            stderr_tail: tail,
        });
        Ok((proc, stdin, stdout))
    }

    pub fn signal(&self, sig: i32) {
        if self.pid > 0 && self.exit.lock().unwrap().is_none() {
            // SAFETY: plain syscall; a stale group id at worst gets ESRCH.
            unsafe {
                libc::killpg(self.pid, sig);
            }
        }
    }

    pub fn kill(&self) {
        self.signal(libc::SIGKILL);
    }

    /// Waits up to `timeout` for the exit status.
    pub async fn wait_exit(&self, timeout: Duration) -> Option<ExitStatus> {
        if let Some(s) = *self.exit.lock().unwrap() {
            return Some(s);
        }
        let mut child = self.child.lock().await;
        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(Ok(status)) => {
                *self.exit.lock().unwrap() = Some(status);
                Some(status)
            }
            _ => None,
        }
    }

    /// SIGTERM to the group, then SIGKILL after `grace` (CODEX_APP_SERVER_FORCE_KILL_AFTER = 2 s).
    pub async fn terminate(&self, grace: Duration) {
        self.signal(libc::SIGTERM);
        if self.wait_exit(grace).await.is_none() {
            self.kill();
            let _ = self.wait_exit(Duration::from_secs(2)).await;
        }
    }

    pub fn stderr_tail(&self) -> String {
        self.stderr_tail.lock().unwrap().clone()
    }

    /// Human summary of why the process went away, with the useful stderr line.
    pub fn exit_message(&self, status: Option<ExitStatus>) -> String {
        let what = match status {
            Some(s) => match s.code() {
                Some(code) => format!("exited with code {code}"),
                None => format!("was killed ({s})"),
            },
            None => "closed its output".to_string(),
        };
        match stderr_excerpt(&self.stderr_tail()) {
            Some(ex) => format!("{what}: {ex}"),
            None => what,
        }
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        self.kill();
    }
}

async fn drain_stderr(mut stderr: impl AsyncRead + Unpin, tail: Arc<Mutex<String>>) {
    let mut buf = [0u8; 4096];
    loop {
        match stderr.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                let mut t = tail.lock().unwrap();
                t.push_str(&String::from_utf8_lossy(&buf[..n]));
                let len = t.chars().count();
                if len > STDERR_TAIL_CHARS {
                    let cut = t.char_indices().nth(len - STDERR_TAIL_CHARS).map(|(i, _)| i).unwrap_or(0);
                    t.drain(..cut);
                }
            }
        }
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// stdio:57-91: the last `error:`/`fatal:` line, or the last line; an exit summary
/// brings the two lines before it. At most 512 chars.
pub(crate) fn stderr_excerpt(tail: &str) -> Option<String> {
    let clean = strip_ansi(tail);
    let lines: Vec<&str> = clean.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let last = *lines.last()?;
    let is_error_line = |l: &str| {
        let lower = l.to_ascii_lowercase();
        let head: String = lower.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        let rest = &lower[head.len()..];
        let rest = rest.trim_start_matches(' ');
        let rest = if rest.starts_with('[') { rest.split_once(']').map(|x| x.1).unwrap_or(rest) } else { rest };
        ((head.ends_with("error")) || head == "fatal") && rest.starts_with(':') && rest[1..].trim().len() > 0
    };
    let pick = if let Some(l) = lines.iter().rev().find(|l| is_error_line(l)) {
        l.to_string()
    } else {
        let lower = last.to_ascii_lowercase();
        if lower.contains("exit status") || lower.contains("exit code") || lower.contains("exited with code") {
            let start = lines.len().saturating_sub(3);
            lines[start..].join(" | ")
        } else {
            last.to_string()
        }
    };
    Some(crate::util::truncate_chars(&pick, 512))
}

pub(crate) enum Frame {
    Line(String),
    Eof,
    TooLong,
    Error(String),
}

/// Reads one `\n`-terminated line (trailing `\r` stripped). A final unterminated line
/// at EOF is still returned (proto:487-493).
pub(crate) async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R, buf: &mut Vec<u8>) -> Frame {
    buf.clear();
    loop {
        let available = match reader.fill_buf().await {
            Ok(b) => b,
            Err(e) => return Frame::Error(e.to_string()),
        };
        if available.is_empty() {
            if buf.is_empty() {
                return Frame::Eof;
            }
            break;
        }
        if let Some(pos) = available.iter().position(|b| *b == b'\n') {
            buf.extend_from_slice(&available[..pos]);
            reader.consume(pos + 1);
            break;
        }
        let n = available.len();
        buf.extend_from_slice(available);
        reader.consume(n);
        if buf.len() > MAX_LINE_BYTES {
            return Frame::TooLong;
        }
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    if buf.len() > MAX_LINE_BYTES {
        return Frame::TooLong;
    }
    Frame::Line(String::from_utf8_lossy(buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_prefers_error_lines() {
        let tail = "starting\nERROR codex: boom\nmore noise\n";
        assert_eq!(stderr_excerpt(tail).as_deref(), Some("more noise"));
        let tail = "starting\nerror: boom happened\nmore noise\n";
        assert_eq!(stderr_excerpt(tail).as_deref(), Some("error: boom happened"));
        let tail = "a\nb\nc\nprocess exited with code 1\n";
        assert_eq!(stderr_excerpt(tail).as_deref(), Some("b | c | process exited with code 1"));
        assert_eq!(stderr_excerpt("\n\n"), None);
    }

    #[tokio::test]
    async fn frames_split_on_newlines() {
        let data: &[u8] = b"{\"a\":1}\r\n\nlast";
        let mut r = tokio::io::BufReader::new(data);
        let mut buf = Vec::new();
        assert!(matches!(read_frame(&mut r, &mut buf).await, Frame::Line(l) if l == "{\"a\":1}"));
        assert!(matches!(read_frame(&mut r, &mut buf).await, Frame::Line(l) if l.is_empty()));
        assert!(matches!(read_frame(&mut r, &mut buf).await, Frame::Line(l) if l == "last"));
        assert!(matches!(read_frame(&mut r, &mut buf).await, Frame::Eof));
    }
}

//! One place that spawns `git` and `gh`.
//!
//! Every git call drops the repo-scoped variables a parent process may have set, turns off
//! optional locks and terminal prompts, and injects the same server config T3 does
//! (spec/07 §7: `safe.bareRepository=explicit`, `maintenance.auto=false`, `gc.auto=0`).
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};

const SCRUBBED_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_PREFIX",
];

const INJECTED_CONFIG: &[(&str, &str)] = &[
    ("safe.bareRepository", "explicit"),
    ("maintenance.auto", "false"),
    ("gc.auto", "0"),
];

pub(crate) struct Out {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Out {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    /// stdout with one trailing newline removed.
    pub fn line(&self) -> String {
        let mut s = self.text();
        while s.ends_with('\n') || s.ends_with('\r') {
            s.pop();
        }
        s
    }
}

pub(crate) struct Cmd {
    cmd: Command,
    program: &'static str,
    shown: Vec<String>,
    stdin: Option<Vec<u8>>,
    timeout: Option<Duration>,
}

pub(crate) fn git(cwd: &Path) -> Cmd {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd);
    for key in SCRUBBED_ENV {
        cmd.env_remove(key);
    }
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("GIT_CONFIG_COUNT", INJECTED_CONFIG.len().to_string());
    for (i, (k, v)) in INJECTED_CONFIG.iter().enumerate() {
        cmd.env(format!("GIT_CONFIG_KEY_{i}"), k);
        cmd.env(format!("GIT_CONFIG_VALUE_{i}"), v);
    }
    Cmd {
        cmd,
        program: "git",
        shown: Vec::new(),
        stdin: None,
        timeout: None,
    }
}

pub(crate) fn gh(cwd: &Path) -> Cmd {
    let mut cmd = Command::new("gh");
    cmd.current_dir(cwd);
    for key in SCRUBBED_ENV {
        cmd.env_remove(key);
    }
    cmd.env("GH_PROMPT_DISABLED", "1");
    cmd.env("GH_NO_UPDATE_NOTIFIER", "1");
    cmd.env("NO_COLOR", "1");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    Cmd {
        cmd,
        program: "gh",
        shown: Vec::new(),
        stdin: None,
        timeout: Some(Duration::from_secs(60)),
    }
}

impl Cmd {
    pub fn arg(mut self, a: impl AsRef<OsStr>) -> Self {
        self.shown.push(a.as_ref().to_string_lossy().into_owned());
        self.cmd.arg(a);
        self
    }
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for a in args {
            self = self.arg(a);
        }
        self
    }
    pub fn env(mut self, k: impl AsRef<OsStr>, v: impl AsRef<OsStr>) -> Self {
        self.cmd.env(k, v);
        self
    }
    pub fn stdin(mut self, input: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(input.into());
        self
    }
    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    fn describe(&self) -> String {
        let mut s = String::from(self.program);
        for a in &self.shown {
            s.push(' ');
            s.push_str(a);
        }
        s
    }

    /// Run to completion. Only spawn failures and timeouts are errors; check `code` yourself.
    pub fn output(mut self) -> anyhow::Result<Out> {
        let desc = self.describe();
        self.cmd
            .stdin(if self.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = self
            .cmd
            .spawn()
            .with_context(|| format!("could not start {}", self.program))?;
        let writer = match (self.stdin.take(), child.stdin.take()) {
            (Some(input), Some(mut pipe)) => Some(thread::spawn(move || {
                let _ = pipe.write_all(&input);
            })),
            _ => None,
        };
        let mut out_pipe = child.stdout.take().expect("piped stdout");
        let mut err_pipe = child.stderr.take().expect("piped stderr");
        let out_reader = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = out_pipe.read_to_end(&mut buf);
            buf
        });
        let err_reader = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = err_pipe.read_to_end(&mut buf);
            buf
        });
        let status = match self.timeout {
            None => child.wait()?,
            Some(limit) => {
                let start = Instant::now();
                let mut nap = Duration::from_millis(1);
                loop {
                    if let Some(status) = child.try_wait()? {
                        break status;
                    }
                    if start.elapsed() >= limit {
                        let _ = child.kill();
                        let _ = child.wait();
                        bail!("{desc} timed out after {}s", limit.as_secs());
                    }
                    thread::sleep(nap);
                    nap = (nap * 2).min(Duration::from_millis(25));
                }
            }
        };
        if let Some(w) = writer {
            let _ = w.join();
        }
        let stdout = out_reader.join().unwrap_or_default();
        let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();
        tracing::trace!(command = %desc, code = ?status.code(), "ran");
        Ok(Out {
            code: status.code(),
            stdout,
            stderr,
        })
    }

    /// Run and require exit 0. Returns stdout.
    pub fn run(self) -> anyhow::Result<String> {
        Ok(self.checked()?.text())
    }

    /// Run and require exit 0. Returns the whole output.
    pub fn checked(self) -> anyhow::Result<Out> {
        let desc = self.describe();
        let out = self.output()?;
        if !out.success() {
            let detail = out.stderr.trim();
            if detail.is_empty() {
                bail!("{desc} failed (exit {:?})", out.code);
            }
            bail!("{desc} failed: {detail}");
        }
        Ok(out)
    }

    /// Exit 0?
    pub fn ok(self) -> anyhow::Result<bool> {
        Ok(self.output()?.success())
    }
}

/// Split NUL-separated output, dropping the empty tail.
pub(crate) fn split_nul(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

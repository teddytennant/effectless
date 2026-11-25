#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .args(args)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn write(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, content).unwrap();
}

pub fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

pub struct Repo {
    pub tmp: TempDir,
    pub path: PathBuf,
}

impl Repo {
    pub fn git(&self, args: &[&str]) -> String {
        git(&self.path, args)
    }
    pub fn write(&self, rel: &str, content: &str) {
        write(&self.path, rel, content)
    }
    pub fn read(&self, rel: &str) -> String {
        read(&self.path, rel)
    }
    pub fn commit(&self, msg: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }
    /// A bare `origin` with main pushed and origin/HEAD set.
    pub fn add_origin(&self) -> PathBuf {
        let bare = self.tmp.path().join("origin.git");
        git(
            self.tmp.path(),
            &["init", "-q", "--bare", "-b", "main", "origin.git"],
        );
        self.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
        self.git(&["push", "-q", "-u", "origin", "main"]);
        self.git(&["remote", "set-head", "origin", "main"]);
        bare
    }
}

/// A repo named `project` with one commit on main.
pub fn repo() -> Repo {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("project");
    std::fs::create_dir_all(&path).unwrap();
    init(&path);
    let r = Repo { tmp, path };
    r.write("README.md", "hello\n");
    r.commit("init");
    r
}

/// An empty repo (unborn main).
pub fn empty_repo() -> Repo {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("project");
    std::fs::create_dir_all(&path).unwrap();
    init(&path);
    Repo { tmp, path }
}

fn init(path: &Path) {
    git(path, &["init", "-q", "-b", "main"]);
    git(path, &["config", "user.name", "Test"]);
    git(path, &["config", "user.email", "test@example.com"]);
    git(path, &["config", "commit.gpgsign", "false"]);
    git(path, &["config", "core.hooksPath", "/dev/null"]);
}

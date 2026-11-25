//! Blocking git work for the details card. Everything here runs on a background
//! thread (`cx.background_spawn`); the card shows progress and errors as toasts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tern_core::{DiffScope, ModelSelection};
use tern_git::refs::{self, Branch};
use tern_git::{DiffOptions, naming, remote, review};
use tern_providers::textgen::{self, CommitMessageInput, PullRequestInput, TextGenOptions};

use super::logic::{BranchChanges, GitAction, GitStatus, StatusFile, truncate_description};

/// What a status read found.
#[derive(Clone, Debug, PartialEq)]
pub enum StatusRead {
    /// A git checkout.
    Repo(GitStatus),
    /// The folder exists but is not in a repository ("Initialize Git").
    NotRepo,
}

/// The folder is gone, which is how the `--demo` projects look.
pub const MISSING_FOLDER: &str = "Git status is unavailable.";

/// Open PR lookups go through `gh` and the network: cached 60s per branch, 5 min when
/// no PR is open (§4b open questions).
fn pr_cache() -> &'static Mutex<HashMap<(PathBuf, String), (Instant, bool)>> {
    static CACHE: OnceLock<Mutex<HashMap<(PathBuf, String), (Instant, bool)>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn has_open_pr(cwd: &Path, branch: &str, force: bool) -> bool {
    let key = (cwd.to_path_buf(), branch.to_string());
    if !force && let Some((at, open)) = pr_cache().lock().ok().and_then(|c| c.get(&key).copied()) {
        let ttl = if open { 60 } else { 300 };
        if at.elapsed() < Duration::from_secs(ttl) {
            return open;
        }
    }
    let open = remote::gh_available()
        && remote::open_pull_request(cwd, branch)
            .ok()
            .flatten()
            .is_some();
    if let Ok(mut c) = pr_cache().lock() {
        c.insert(key, (Instant::now(), open));
    }
    open
}

/// Reads everything the git rows need. `force_pr` skips the open-PR cache.
pub fn read_status(cwd: &Path, force_pr: bool) -> Result<StatusRead, String> {
    if !cwd.is_dir() {
        return Err(MISSING_FOLDER.into());
    }
    if tern_git::repo_root(cwd).is_none() {
        return Ok(StatusRead::NotRepo);
    }
    let st = refs::status(cwd).map_err(|e| e.to_string())?;
    let opts = DiffOptions {
        ignore_whitespace: false,
        ..Default::default()
    };
    let files = tern_git::scope_files(cwd, &DiffScope::Uncommitted, &|_| None, &opts)
        .map(|(_, files)| {
            files
                .into_iter()
                .map(|f| StatusFile {
                    path: f.path,
                    additions: f.additions,
                    deletions: f.deletions,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let is_default_ref = match (&st.branch, &st.default_branch) {
        (Some(b), Some(d)) => b == d,
        (Some(b), None) => b == "main" || b == "master",
        _ => false,
    };
    let mut status = GitStatus {
        branch: st.branch.clone(),
        upstream: st.upstream.clone(),
        has_changes: st.has_changes,
        ahead: st.ahead,
        behind: st.behind,
        ahead_of_default: None,
        has_open_pr: false,
        is_default_ref,
        has_primary_remote: st.has_origin,
        default_branch: st.default_branch.clone(),
        additions: st.additions,
        deletions: st.deletions,
        files,
        branch_changes: None,
    };
    if let Some(branch) = &st.branch
        && !is_default_ref
    {
        status.ahead_of_default = review::branch_commits(cwd, None)
            .ok()
            .map(|c| c.len() as u32);
        if let Ok((source, files)) =
            tern_git::scope_files(cwd, &DiffScope::Branch { base_ref: None }, &|_| None, &opts)
            && !files.is_empty()
        {
            status.branch_changes = Some(BranchChanges {
                base: source
                    .base_ref
                    .unwrap_or_else(|| st.default_branch.clone().unwrap_or("main".into())),
                additions: files.iter().map(|f| f.additions).sum(),
                deletions: files.iter().map(|f| f.deletions).sum(),
            });
        }
        if st.has_origin {
            status.has_open_pr = has_open_pr(cwd, branch, force_pr);
        }
    }
    Ok(StatusRead::Repo(status))
}

pub fn list_refs(cwd: &Path) -> Result<Vec<Branch>, String> {
    if !cwd.is_dir() {
        return Err(MISSING_FOLDER.into());
    }
    refs::list_branches(cwd).map_err(|e| e.to_string())
}

/// `vcs.switchRef` with T3's error wording (§4a.8).
pub fn switch_ref(cwd: &Path, name: &str) -> Result<Option<String>, String> {
    refs::switch_ref(cwd, name).map_err(|e| checkout_error(&e.to_string()))
}

/// `vcs.createRef {switchRef: true}`.
pub fn create_ref(cwd: &Path, name: &str) -> Result<Option<String>, String> {
    refs::create_branch(cwd, name, None).map_err(|e| e.to_string())?;
    switch_ref(cwd, name)
}

fn checkout_error(raw: &str) -> String {
    if raw.contains("would be overwritten by checkout") {
        "Your local changes would be overwritten by switching branches. Commit, stash, or discard them, then switch again.".into()
    } else if raw.trim().is_empty() {
        "git checkout failed".into()
    } else {
        raw.trim().to_string()
    }
}

pub fn init_repo(cwd: &Path) -> Result<(), String> {
    let out = std::process::Command::new("git")
        .arg("init")
        .current_dir(cwd)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// `vcsPull`: (title, description) for the inline success.
pub fn pull(cwd: &Path) -> Result<(String, String), String> {
    let before = refs::status(cwd).map_err(|e| e.to_string())?;
    let Some(branch) = before.branch.clone() else {
        return Err("Cannot pull from detached HEAD.".into());
    };
    let Some(upstream) = before.upstream.clone() else {
        return Err("Current branch has no upstream configured. Push with upstream first.".into());
    };
    if before.behind == 0 {
        return Ok((
            "Already up to date".into(),
            format!("{branch} is already synchronized."),
        ));
    }
    remote::pull(cwd).map_err(|e| e.to_string())?;
    Ok(("Pulled".into(), format!("Updated {branch} from {upstream}")))
}

/// A stacked action request (`gitRunStackedAction`).
#[derive(Clone, Debug)]
pub struct ActionRequest {
    pub action: GitAction,
    pub commit_message: Option<String>,
    pub feature_branch: bool,
    pub file_paths: Option<Vec<String>>,
}

/// What the action produced, for the inline success.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionOutcome {
    pub title: String,
    pub description: Option<String>,
    /// A feature branch was created and checked out.
    pub created_branch: Option<String>,
}

/// The model for commit messages and PR text: the writer model if set, else the text
/// generation model.
pub fn textgen_options(cwd: &Path, userdata: Option<&Path>) -> TextGenOptions {
    let mut opts = TextGenOptions::default_for(cwd.to_path_buf());
    if let Some(dir) = userdata {
        let settings = tern_core::settings::Settings::load(dir).value;
        let model: ModelSelection = settings
            .source_control_writer_model_selection
            .clone()
            .unwrap_or(settings.text_generation_model_selection.clone());
        opts.model = model;
    }
    opts
}

fn block_on<T>(f: impl std::future::Future<Output = anyhow::Result<T>>) -> Result<T, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(f).map_err(|e| e.to_string())
}

fn style_instructions(cwd: &Path, pr: bool) -> Option<String> {
    let subjects = remote::recent_commit_subjects(cwd).unwrap_or_default();
    let lead = if pr {
        "Follow the repository's established change request title and body style when examples are available."
    } else {
        "Follow the repository's established commit message style when examples are available."
    };
    if subjects.is_empty() {
        return Some(lead.into());
    }
    Some(format!(
        "{lead}\n\nRecent commit subjects from this repository:\n{}",
        subjects.join("\n")
    ))
}

/// `runStackedAction` (§4b.8). `progress` receives the phase labels.
pub fn run_action(
    cwd: &Path,
    req: &ActionRequest,
    textgen: &TextGenOptions,
    progress: &dyn Fn(&str),
) -> Result<ActionOutcome, String> {
    let status = match read_status(cwd, true)? {
        StatusRead::Repo(s) => s,
        StatusRead::NotRepo => return Err("Not a git repository.".into()),
    };
    let action = req.action;
    let wants_commit = action.includes_commit();
    let wants_push = matches!(
        action,
        GitAction::Push | GitAction::CommitPush | GitAction::CommitPushPr
    ) || (action == GitAction::CreatePr
        && (!status.has_upstream() || status.ahead > 0));
    let wants_pr = matches!(action, GitAction::CreatePr | GitAction::CommitPushPr);
    if req.feature_branch && !wants_commit {
        return Err("Feature-branch checkout is only supported for commit actions.".into());
    }
    if !req.feature_branch && status.branch.is_none() {
        if wants_push {
            return Err("Cannot push from detached HEAD.".into());
        }
        if wants_pr {
            return Err("Cannot create a pull request from detached HEAD.".into());
        }
    }
    let paths = req.file_paths.as_deref();
    let custom = req
        .commit_message
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string);
    let mut outcome = ActionOutcome::default();
    let mut message = custom.clone();

    if req.feature_branch {
        progress("Preparing feature branch...");
        let ctx = remote::commit_context(cwd, paths)
            .map_err(|e| e.to_string())?
            .ok_or("Cannot create a feature branch because there are no changes to commit.")?;
        let suggestion = block_on(textgen::commit_message(
            textgen,
            &CommitMessageInput {
                branch: status.branch.clone(),
                staged_summary: ctx.staged_summary,
                staged_patch: ctx.staged_patch,
                include_branch: true,
                instructions: style_instructions(cwd, false),
            },
        ))?;
        let preferred = suggestion
            .branch
            .clone()
            .unwrap_or_else(|| naming::sanitize_feature_branch_name(&suggestion.subject));
        let existing = refs::local_branch_names(cwd).map_err(|e| e.to_string())?;
        let name = naming::resolve_auto_feature_branch_name(&existing, Some(&preferred));
        refs::create_branch(cwd, &name, None).map_err(|e| e.to_string())?;
        switch_ref(cwd, &name)?;
        outcome.created_branch = Some(name);
        if message.is_none() {
            message = Some(if suggestion.body.is_empty() {
                suggestion.subject
            } else {
                format!("{}\n\n{}", suggestion.subject, suggestion.body)
            });
        }
    }

    let branch = refs::current_branch(cwd).map_err(|e| e.to_string())?;
    if wants_pr
        && !req.feature_branch
        && let Some(b) = &branch
    {
        let base = remote::resolve_pr_base(cwd, b).map_err(|e| e.to_string())?;
        if &base == b && !status.has_open_pr {
            return Err(format!(
                "{base} is the base branch. Check out a feature branch to create a PR."
            ));
        }
    }

    let mut commit: Option<(String, String)> = None;
    if wants_commit {
        let msg = match message {
            Some(m) => m,
            None => {
                progress("Generating commit message...");
                match remote::commit_context(cwd, paths).map_err(|e| e.to_string())? {
                    None => String::new(),
                    Some(ctx) => {
                        let s = block_on(textgen::commit_message(
                            textgen,
                            &CommitMessageInput {
                                branch: branch.clone(),
                                staged_summary: ctx.staged_summary,
                                staged_patch: ctx.staged_patch,
                                include_branch: false,
                                instructions: style_instructions(cwd, false),
                            },
                        ))?;
                        if s.body.is_empty() {
                            s.subject
                        } else {
                            format!("{}\n\n{}", s.subject, s.body)
                        }
                    }
                }
            }
        };
        if !msg.is_empty() {
            progress("Committing...");
            if let Some(sha) = remote::commit(cwd, &msg, paths).map_err(|e| e.to_string())? {
                let (subject, _) = remote::split_commit_message(&msg);
                commit = Some((sha, subject));
            }
        }
    }

    let mut pushed: Option<String> = None;
    if wants_push {
        progress("Pushing...");
        match remote::push_branch(cwd, true).map_err(|e| e.to_string())? {
            remote::PushOutcome::Pushed {
                remote,
                upstream_branch,
                ..
            } => pushed = Some(format!("{remote}/{upstream_branch}")),
            remote::PushOutcome::UpToDate => {}
        }
    }

    if wants_pr {
        progress("Preparing PR...");
        let head = refs::current_branch(cwd)
            .map_err(|e| e.to_string())?
            .ok_or("Cannot create a pull request from detached HEAD.")?;
        if let Some(pr) = remote::open_pull_request(cwd, &head).map_err(|e| e.to_string())? {
            outcome.title = format!("Opened PR #{}", pr.number);
            outcome.description = Some(truncate_description(&pr.title));
            return Ok(outcome);
        }
        let base = remote::resolve_pr_base(cwd, &head).map_err(|e| e.to_string())?;
        progress("Generating PR content...");
        let range_base = refs::primary_remote(cwd)
            .ok()
            .flatten()
            .filter(|r| refs::remote_branch_exists(cwd, r, &base).unwrap_or(false))
            .map(|r| format!("{r}/{base}"))
            .unwrap_or_else(|| base.clone());
        let range = remote::range_context(cwd, &range_base)
            .or_else(|_| remote::range_context(cwd, &base))
            .map_err(|e| e.to_string())?;
        let (title, body) = block_on(textgen::pull_request(
            textgen,
            &PullRequestInput {
                base_branch: base.clone(),
                head_branch: head.clone(),
                commit_summary: range.commit_summary,
                diff_summary: range.diff_summary,
                diff_patch: range.diff_patch,
                instructions: style_instructions(cwd, true),
                ..Default::default()
            },
        ))?;
        progress("Creating pull request...");
        let pr = remote::create_pull_request(cwd, &base, &title, &body, false)
            .map_err(|e| e.to_string())?;
        // A created PR shows "Created PR" with no description.
        outcome.title = "Created PR".into();
        let _ = pr;
        return Ok(outcome);
    }

    let subject = commit.as_ref().map(|(_, s)| truncate_description(s));
    outcome.title = match (&pushed, &commit) {
        (Some(up), Some((sha, _))) => format!("Pushed {} to {up}", &sha[..sha.len().min(7)]),
        (Some(up), None) => format!("Pushed to {up}"),
        (None, Some((sha, _))) => format!("Committed {}", &sha[..sha.len().min(7)]),
        (None, None) => "Done".into(),
    };
    outcome.description = subject;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn sh(cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn repo() -> (tempdir::Dir, PathBuf) {
        let dir = tempdir::Dir::new();
        let root = dir.0.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        sh(&root, &["init", "-q", "-b", "main"]);
        sh(&root, &["config", "user.name", "t"]);
        sh(&root, &["config", "user.email", "t@t"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        sh(&root, &["add", "."]);
        sh(&root, &["commit", "-qm", "init"]);
        (dir, root)
    }

    mod tempdir {
        pub struct Dir(pub std::path::PathBuf);
        impl Dir {
            pub fn new() -> Self {
                let p = std::env::temp_dir().join(format!("tern-details-{}", tern_core::new_id()));
                std::fs::create_dir_all(&p).unwrap();
                Dir(p)
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn missing_folder_is_unavailable() {
        assert_eq!(
            read_status(Path::new("/nonexistent/tern/demo"), false),
            Err(MISSING_FOLDER.into())
        );
    }

    #[test]
    fn plain_folder_is_not_a_repo() {
        let dir = tempdir::Dir::new();
        assert_eq!(read_status(&dir.0, false), Ok(StatusRead::NotRepo));
        init_repo(&dir.0).unwrap();
        assert!(matches!(
            read_status(&dir.0, false),
            Ok(StatusRead::Repo(_))
        ));
    }

    #[test]
    fn status_counts_and_commit_push() {
        let (dir, root) = repo();
        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        let StatusRead::Repo(s) = read_status(&root, false).unwrap() else {
            panic!()
        };
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert!(s.has_changes && s.is_default_ref);
        assert_eq!((s.additions, s.deletions), (2, 0));
        assert_eq!(s.files.len(), 1);

        // a bare remote
        let bare = dir.0.join("remote.git");
        sh(&dir.0, &["init", "-q", "--bare", "remote.git"]);
        sh(&root, &["remote", "add", "origin", bare.to_str().unwrap()]);
        let req = ActionRequest {
            action: GitAction::CommitPush,
            commit_message: Some("add lines".into()),
            feature_branch: false,
            file_paths: None,
        };
        let opts = TextGenOptions::default_for(root.clone());
        let phases = std::cell::RefCell::new(Vec::new());
        let out = run_action(&root, &req, &opts, &|p| {
            phases.borrow_mut().push(p.to_string())
        })
        .unwrap();
        assert!(out.title.starts_with("Pushed "), "{}", out.title);
        assert!(out.title.ends_with(" to origin/main"), "{}", out.title);
        assert_eq!(out.description.as_deref(), Some("add lines"));
        assert_eq!(*phases.borrow(), ["Committing...", "Pushing..."]);
        let StatusRead::Repo(s) = read_status(&root, false).unwrap() else {
            panic!()
        };
        assert!(!s.has_changes);
        assert_eq!(s.ahead, 0);
    }

    #[test]
    fn switch_and_create_refs() {
        let (_dir, root) = repo();
        assert_eq!(
            create_ref(&root, "feature/x").unwrap().as_deref(),
            Some("feature/x")
        );
        assert_eq!(switch_ref(&root, "main").unwrap().as_deref(), Some("main"));
        let names: Vec<_> = list_refs(&root)
            .unwrap()
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert!(names.contains(&"feature/x".to_string()));
        // dirty conflicting checkout
        sh(&root, &["switch", "-q", "feature/x"]);
        std::fs::write(root.join("a.txt"), "feature\n").unwrap();
        sh(&root, &["commit", "-qam", "f"]);
        sh(&root, &["switch", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "dirty\n").unwrap();
        let err = switch_ref(&root, "feature/x").unwrap_err();
        assert!(
            err.starts_with("Your local changes would be overwritten"),
            "{err}"
        );
    }

    #[test]
    fn commit_selected_files_only() {
        let (_dir, root) = repo();
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        std::fs::write(root.join("b.txt"), "new\n").unwrap();
        let req = ActionRequest {
            action: GitAction::Commit,
            commit_message: Some("only b".into()),
            feature_branch: false,
            file_paths: Some(vec!["b.txt".into()]),
        };
        let opts = TextGenOptions::default_for(root.clone());
        let out = run_action(&root, &req, &opts, &|_| {}).unwrap();
        assert!(out.title.starts_with("Committed "));
        let StatusRead::Repo(s) = read_status(&root, false).unwrap() else {
            panic!()
        };
        assert_eq!(
            s.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["a.txt"]
        );
    }

    #[test]
    fn pull_reports_up_to_date() {
        let (dir, root) = repo();
        sh(&dir.0, &["init", "-q", "--bare", "remote.git"]);
        let bare = dir.0.join("remote.git");
        sh(&root, &["remote", "add", "origin", bare.to_str().unwrap()]);
        sh(&root, &["push", "-q", "-u", "origin", "main"]);
        assert_eq!(pull(&root).unwrap().0, "Already up to date");
    }
}

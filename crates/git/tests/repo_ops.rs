mod common;

use common::{git, repo};
use tern_git::remote::{self, PushOutcome};
use tern_git::{naming, refs, worktree};

#[test]
fn repo_root_detection() {
    let r = repo();
    r.write("deep/er/file.txt", "x\n");
    let root = tern_git::repo_root(&r.path.join("deep/er")).unwrap();
    assert_eq!(root.canonicalize().unwrap(), r.path.canonicalize().unwrap());
    assert!(tern_git::repo_root(&r.path.join("deep/er/file.txt")).is_some());
    let outside = tempfile::tempdir().unwrap();
    assert!(tern_git::repo_root(outside.path()).is_none());
}

#[test]
fn list_branches_orders_and_dedupes() {
    let r = repo();
    r.add_origin();
    r.git(&["branch", "older"]);
    // Make `newer` the most recent commit.
    r.git(&["checkout", "-q", "-b", "newer"]);
    r.write("n.txt", "n\n");
    git(&r.path, &["-c", "core.hooksPath=/dev/null", "add", "-A"]);
    std::process::Command::new("git")
        .current_dir(&r.path)
        .args(["commit", "-q", "-m", "newer"])
        .env("GIT_COMMITTER_DATE", "2100-01-01T00:00:00Z")
        .status()
        .unwrap();
    r.git(&["push", "-q", "origin", "newer"]);
    r.git(&["checkout", "-q", "older"]);

    let branches = refs::list_branches(&r.path).unwrap();
    let names: Vec<_> = branches.iter().map(|b| b.name.as_str()).collect();
    // current, default, then locals by recency; origin/main and origin/newer have local twins.
    assert_eq!(names, vec!["older", "main", "newer"]);
    assert!(branches[0].is_current);
    assert!(branches[1].is_default);
    assert_eq!(
        std::path::Path::new(branches[0].worktree_path.as_deref().unwrap())
            .canonicalize()
            .unwrap(),
        r.path.canonicalize().unwrap()
    );
    assert!(branches[1].worktree_path.is_none());
    assert!(branches.iter().all(|b| b.last_commit_at.is_some()));

    let all = refs::list_branches_with(&r.path, true).unwrap();
    let remote: Vec<_> = all
        .iter()
        .filter(|b| b.is_remote)
        .map(|b| (b.name.as_str(), b.is_default))
        .collect();
    assert_eq!(remote, vec![("origin/main", true), ("origin/newer", false)]);

    assert_eq!(
        refs::default_branch(&r.path).unwrap().as_deref(),
        Some("main")
    );
    assert_eq!(
        refs::primary_remote(&r.path).unwrap().as_deref(),
        Some("origin")
    );
}

#[test]
fn status_counts_and_upstream() {
    let r = repo();
    r.add_origin();
    let st = refs::status(&r.path).unwrap();
    assert_eq!(st.branch.as_deref(), Some("main"));
    assert_eq!(st.upstream.as_deref(), Some("origin/main"));
    assert_eq!((st.ahead, st.behind), (0, 0));
    assert!(!st.has_changes);
    assert!(st.has_origin);
    assert_eq!(st.default_branch.as_deref(), Some("main"));

    r.write("README.md", "hello\nmore\nlines\n");
    r.write("new.txt", "n\n");
    r.git(&["add", "new.txt"]);
    r.write("untracked.txt", "u\n");
    let st = refs::status(&r.path).unwrap();
    assert!(st.has_changes);
    assert_eq!(st.changed_files, 3);
    // Untracked files have no numstat: README +2, new.txt +1.
    assert_eq!((st.additions, st.deletions), (3, 0));

    r.commit("ahead");
    let st = refs::status(&r.path).unwrap();
    assert_eq!((st.ahead, st.behind), (1, 0));

    // A branch with no upstream counts commits ahead of its base.
    r.git(&["checkout", "-q", "-b", "topic"]);
    r.write("t.txt", "t\n");
    r.commit("t1");
    let st = refs::status(&r.path).unwrap();
    assert!(st.upstream.is_none());
    assert_eq!(st.ahead, 2);

    // Detached.
    r.git(&["checkout", "-q", "--detach"]);
    assert_eq!(refs::status(&r.path).unwrap().branch, None);

    // Index lock.
    std::fs::write(r.path.join(".git/index.lock"), "").unwrap();
    let err = refs::status(&r.path).unwrap_err().to_string();
    assert!(err.contains("Git index is locked"), "{err}");
}

#[test]
fn checkout_local_remote_and_create() {
    let r = repo();
    r.add_origin();
    // A branch that only exists on the remote.
    r.git(&["checkout", "-q", "-b", "remote-only"]);
    r.write("r.txt", "r\n");
    r.commit("r");
    r.git(&["push", "-q", "origin", "remote-only"]);
    r.git(&["checkout", "-q", "main"]);
    r.git(&["branch", "-D", "remote-only"]);

    let now = refs::switch_ref(&r.path, "origin/remote-only").unwrap();
    assert_eq!(now.as_deref(), Some("remote-only"));
    assert_eq!(
        r.git(&["rev-parse", "--abbrev-ref", "remote-only@{upstream}"])
            .trim(),
        "origin/remote-only"
    );

    refs::checkout(&r.path, "main").unwrap();
    assert_eq!(
        refs::current_branch(&r.path).unwrap().as_deref(),
        Some("main")
    );
    // The existing tracker is reused.
    refs::checkout(&r.path, "origin/remote-only").unwrap();
    assert_eq!(
        refs::current_branch(&r.path).unwrap().as_deref(),
        Some("remote-only")
    );

    refs::create_branch(&r.path, "plain", None).unwrap();
    assert!(refs::branch_exists(&r.path, "plain").unwrap());
    refs::create_branch(&r.path, "tracked", Some("origin/main")).unwrap();
    assert_eq!(
        r.git(&["rev-parse", "--abbrev-ref", "tracked@{upstream}"])
            .trim(),
        "origin/main"
    );
    assert!(refs::checkout(&r.path, "-f").is_err());
    assert!(refs::create_branch(&r.path, "-x", None).is_err());

    refs::rename_branch(&r.path, "plain", "renamed").unwrap();
    assert!(refs::branch_exists(&r.path, "renamed").unwrap());
}

#[test]
fn fetch_branch_reports_missing() {
    let r = repo();
    let bare = r.add_origin();
    // Someone else pushes a branch.
    let other = r.tmp.path().join("other");
    git(
        r.tmp.path(),
        &["clone", "-q", bare.to_str().unwrap(), "other"],
    );
    git(&other, &["checkout", "-q", "-b", "theirs"]);
    git(
        &other,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=O",
            "-c",
            "user.email=o@x",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "o",
        ],
    );
    git(&other, &["push", "-q", "origin", "theirs"]);

    assert!(refs::fetch_branch(&r.path, "origin", "theirs").unwrap());
    assert!(refs::remote_branch_exists(&r.path, "origin", "theirs").unwrap());
    assert!(!refs::fetch_branch(&r.path, "origin", "nope").unwrap());
    refs::fetch_remote(&r.path, "origin").unwrap();
}

#[test]
fn worktree_create_and_remove() {
    let r = repo();
    let root = r.tmp.path().join("worktrees");
    let made = worktree::provision(&r.path, &root, "main", "tern/feature-x", true).unwrap();

    // <root>/<repo>/<branch, / -> -, <= 23 bytes>-<16 hex>
    let rel = made.path.strip_prefix(&root).unwrap();
    let parts: Vec<_> = rel
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    assert_eq!(parts[0], "project");
    let leaf = &parts[1];
    assert!(leaf.starts_with("tern-feature-x-"), "{leaf}");
    assert_eq!(leaf.len(), "tern-feature-x-".len() + 16);
    assert_eq!(made.base_ref, "main");
    assert!(made.submodule_warning.is_none());

    assert_eq!(common::read(&made.path, "README.md"), "hello\n");
    assert_eq!(
        refs::current_branch(&made.path).unwrap().as_deref(),
        Some("tern/feature-x")
    );
    assert_eq!(
        r.git(&["config", "branch.tern/feature-x.tern-merge-base"])
            .trim(),
        "main"
    );
    // Not left locked.
    assert!(
        !r.git(&["worktree", "list", "--porcelain"])
            .contains("locked")
    );
    // The branch shows its worktree.
    let b = refs::list_branches(&r.path).unwrap();
    let wt = b.iter().find(|b| b.name == "tern/feature-x").unwrap();
    assert!(wt.worktree_path.is_some());
    // Its branch scope diffs against main.
    let base = refs::resolve_base_branch(&made.path, "tern/feature-x", true)
        .unwrap()
        .unwrap();
    assert_eq!(base.comparison_ref, "refs/heads/main");

    // Same branch again: refused.
    let err = worktree::create(&r.path, &root, "main", "tern/feature-x", false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("already exists"), "{err}");
    // A nested name collides.
    let err = worktree::create(&r.path, &root, "main", "tern/feature-x/deeper", false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot be created"), "{err}");
    // A base that doesn't resolve falls back to the current branch.
    let fallback = worktree::provision(&r.path, &root, "no-such-base", "tern/fb", false).unwrap();
    assert_eq!(fallback.base_ref, "main");
    // Relative roots are refused.
    assert!(worktree::create(&r.path, std::path::Path::new("rel"), "main", "x", false).is_err());

    // Dirty worktrees need force.
    common::write(&made.path, "dirty.txt", "d\n");
    assert!(worktree::remove(&r.path, &made.path, false).is_err());
    worktree::remove(&r.path, &made.path, true).unwrap();
    assert!(!made.path.exists());
    // The branch is kept.
    assert!(refs::branch_exists(&r.path, "tern/feature-x").unwrap());

    // A worktree whose folder vanished is pruned.
    std::fs::remove_dir_all(&fallback.path).unwrap();
    worktree::remove(&r.path, &fallback.path, false).unwrap();
    assert!(!r.git(&["worktree", "list"]).contains("tern/fb"));
}

#[test]
fn worktree_from_origin_records_remote_base() {
    let r = repo();
    let bare = r.add_origin();
    // origin/main moves ahead of local main.
    let other = r.tmp.path().join("other");
    git(
        r.tmp.path(),
        &["clone", "-q", bare.to_str().unwrap(), "other"],
    );
    common::write(&other, "upstream.txt", "u\n");
    git(&other, &["add", "-A"]);
    git(
        &other,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "upstream",
        ],
    );
    git(&other, &["push", "-q", "origin", "main"]);

    let root = r.tmp.path().join("wt");
    let made = worktree::provision(&r.path, &root, "main", "tern/from-origin", true).unwrap();
    assert!(made.path.join("upstream.txt").exists());
    assert_eq!(
        r.git(&["config", "branch.tern/from-origin.tern-merge-base-ref"])
            .trim(),
        "refs/remotes/origin/main"
    );
    // Without start_from_origin, local main is the start.
    let local = worktree::provision(&r.path, &root, "main", "tern/local", false).unwrap();
    assert!(!local.path.join("upstream.txt").exists());

    // Restoring a deleted managed worktree.
    std::fs::remove_dir_all(&made.path).unwrap();
    worktree::restore_missing(&r.path, &[root.as_path()], &made.path, "tern/from-origin").unwrap();
    assert!(made.path.join("upstream.txt").exists());
    assert!(worktree::restore_missing(&r.path, &[root.as_path()], &r.path, "main").is_err());
}

#[test]
fn unique_and_temporary_branch_names() {
    let r = repo();
    assert_eq!(
        worktree::unique_branch_name(&r.path, "tern", "Add Search").unwrap(),
        "tern/add-search"
    );
    r.git(&["branch", "tern/add-search"]);
    assert_eq!(
        worktree::unique_branch_name(&r.path, "tern", "add search").unwrap(),
        "tern/add-search-1"
    );
    r.git(&["branch", "TERN/add-search-1"]);
    assert_eq!(
        worktree::unique_branch_name(&r.path, "tern", "add search").unwrap(),
        "tern/add-search-2"
    );
    // A branch nested under the name blocks it.
    r.git(&["branch", "tern/fix/child"]);
    assert_eq!(
        worktree::unique_branch_name(&r.path, "tern", "fix").unwrap(),
        "tern/fix-1"
    );

    // A branch named like a parent path flattens.
    let r2 = repo();
    r2.git(&["branch", "tern"]);
    assert_eq!(
        worktree::unique_branch_name(&r2.path, "tern", "x").unwrap(),
        "tern-x"
    );
    let temp = worktree::temporary_branch_name(&r2.path, "tern").unwrap();
    assert!(temp.starts_with("tern-"), "{temp}");
    assert!(naming::is_temporary_branch_name(&temp, "tern"));
    let temp = worktree::temporary_branch_name(&r.path, "tern").unwrap();
    assert!(temp.starts_with("tern/"), "{temp}");

    // Rename after AI naming.
    let root = r.tmp.path().join("wt");
    let made = worktree::provision(&r.path, &root, "main", &temp, false).unwrap();
    let new = worktree::rename_generated_branch(&r.path, &temp, "tern/add-search", false).unwrap();
    assert_eq!(new, "tern/add-search-2");
    assert_eq!(
        refs::current_branch(&made.path).unwrap().as_deref(),
        Some("tern/add-search-2")
    );
    assert_eq!(
        r.git(&["config", "branch.tern/add-search-2.tern-merge-base"])
            .trim(),
        "main"
    );
}

#[test]
fn commit_push_pull() {
    let r = repo();
    let bare = r.add_origin();

    // Nothing to commit.
    assert_eq!(remote::commit(&r.path, "nothing", None).unwrap(), None);
    assert!(remote::commit_all(&r.path, "nothing").is_err());

    r.write("a.txt", "a\n");
    r.write("b.txt", "b\n");
    let ctx = remote::commit_context(&r.path, None).unwrap().unwrap();
    assert!(ctx.staged_summary.contains("A\ta.txt"));
    assert!(ctx.staged_patch.contains("+a"));
    // The context didn't stage anything for real.
    assert!(r.git(&["diff", "--cached", "--name-only"]).is_empty());

    // Selected files only.
    let sha = remote::commit(&r.path, "add a\n\nbody text", Some(&["a.txt".to_string()]))
        .unwrap()
        .unwrap();
    assert_eq!(r.git(&["rev-parse", "HEAD"]).trim(), sha);
    assert_eq!(
        r.git(&["log", "-1", "--format=%s|%b"]).trim(),
        "add a|body text"
    );
    assert_eq!(r.git(&["status", "--porcelain"]), "?? b.txt\n");

    // Push with an upstream.
    assert_eq!(
        remote::push_branch(&r.path, true).unwrap(),
        PushOutcome::Pushed {
            branch: "main".into(),
            remote: "origin".into(),
            upstream_branch: "main".into(),
            set_upstream: false,
        }
    );
    assert_eq!(
        remote::push_branch(&r.path, true).unwrap(),
        PushOutcome::UpToDate
    );

    // Publish a new branch: upstream written by hand.
    r.git(&["checkout", "-q", "-b", "feature/pub"]);
    let sha = remote::commit_all(&r.path, "add b").unwrap();
    let out = remote::push_branch(&r.path, true).unwrap();
    assert_eq!(
        out,
        PushOutcome::Pushed {
            branch: "feature/pub".into(),
            remote: "origin".into(),
            upstream_branch: "feature/pub".into(),
            set_upstream: true,
        }
    );
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/feature/pub"]).trim(),
        sha
    );
    assert_eq!(
        r.git(&["rev-parse", "--abbrev-ref", "feature/pub@{upstream}"])
            .trim(),
        "origin/feature/pub"
    );
    assert_eq!(
        remote::resolve_pr_base(&r.path, "feature/pub").unwrap(),
        "main"
    );

    let range = remote::range_context(&r.path, "origin/main").unwrap();
    assert!(range.commit_summary.contains("add b"));
    assert!(range.diff_summary.contains("b.txt"));
    assert_eq!(remote::recent_commit_subjects(&r.path).unwrap()[0], "add b");

    // Pull: someone else advances main.
    r.git(&["checkout", "-q", "main"]);
    let other = r.tmp.path().join("other");
    git(
        r.tmp.path(),
        &["clone", "-q", bare.to_str().unwrap(), "other"],
    );
    common::write(&other, "c.txt", "c\n");
    git(&other, &["add", "-A"]);
    git(
        &other,
        &["-c", "commit.gpgsign=false", "commit", "-q", "-m", "c"],
    );
    git(&other, &["push", "-q", "origin", "main"]);
    remote::pull(&r.path).unwrap();
    assert!(r.path.join("c.txt").exists());

    r.git(&["checkout", "-q", "--detach"]);
    assert!(remote::pull(&r.path).is_err());
    assert!(remote::push(&r.path, true).is_err());
}

#[test]
fn commit_refuses_conflict_markers() {
    let r = repo();
    r.git(&["checkout", "-q", "-b", "side"]);
    r.write("README.md", "side\n");
    r.commit("side");
    r.git(&["checkout", "-q", "main"]);
    r.write("README.md", "main\n");
    r.commit("main");
    let merge = std::process::Command::new("git")
        .current_dir(&r.path)
        .args(["merge", "-q", "side"])
        .output()
        .unwrap();
    assert!(!merge.status.success());
    let err = remote::commit_all(&r.path, "merge")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("unresolved merge conflicts in README.md"),
        "{err}"
    );
    let err = remote::commit(&r.path, "x", Some(&["README.md".into()]))
        .unwrap_err()
        .to_string();
    assert!(err.contains("during a merge"), "{err}");
}

#[test]
fn push_without_remote() {
    let r = repo();
    let err = remote::push(&r.path, true).unwrap_err().to_string();
    assert_eq!(
        err,
        "Cannot push because no git remote is configured for this repository."
    );
}

/// Needs `gh auth` and a GitHub repo you can push to: set TERN_GH_TEST_REPO to its checkout.
#[test]
#[ignore]
fn pull_request_through_gh() {
    let dir = std::env::var("TERN_GH_TEST_REPO").expect("TERN_GH_TEST_REPO");
    let cwd = std::path::Path::new(&dir);
    assert!(remote::gh_available());
    let branch = format!("tern-gh-test-{}", std::process::id());
    refs::create_branch(cwd, &branch, None).unwrap();
    refs::checkout(cwd, &branch).unwrap();
    common::write(cwd, &format!("{branch}.txt"), "x\n");
    remote::commit_all(cwd, "tern gh test").unwrap();
    remote::push(cwd, true).unwrap();
    let pr = remote::create_pull_request(cwd, "", "Tern gh test", "body", true).unwrap();
    assert_eq!(pr.head, branch);
    assert_eq!(pr.state, "open");
    let found = remote::open_pull_request(cwd, &branch).unwrap().unwrap();
    assert_eq!(found.number, pr.number);
    // Same PR again, not a second one.
    assert_eq!(
        remote::create_pull_request(cwd, "", "again", "", true)
            .unwrap()
            .number,
        pr.number
    );
}

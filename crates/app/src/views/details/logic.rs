//! Pure rules behind the thread details card: the git quick action state machine and
//! its menu (spec/05 §4b.2-4b.6), the Changes totals (§4b.4), the branch picker's
//! labels and filtering (§4a.5, §4a.7) and project script helpers (§6.1). No gpui,
//! no IO, so all of it is unit tested.

use tern_core::ProjectScript;

/// GitHub's short term for a change request (§4b.12).
pub const PR: &str = "PR";

/// What the card knows about a checkout, built from `git status` and friends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GitStatus {
    /// `refName`; None when HEAD is detached.
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub has_changes: bool,
    pub ahead: u32,
    pub behind: u32,
    /// Commits ahead of the default branch (None = same as `ahead`).
    pub ahead_of_default: Option<u32>,
    pub has_open_pr: bool,
    pub is_default_ref: bool,
    /// Any remote exists.
    pub has_primary_remote: bool,
    pub default_branch: Option<String>,
    /// Working tree +/- against HEAD.
    pub additions: u32,
    pub deletions: u32,
    pub files: Vec<StatusFile>,
    /// The branch-range preview the diff panel would show (`vs {ref}`).
    pub branch_changes: Option<BranchChanges>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusFile {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BranchChanges {
    pub base: String,
    pub additions: u32,
    pub deletions: u32,
}

impl GitStatus {
    pub fn has_upstream(&self) -> bool {
        self.upstream.is_some()
    }

    fn ahead_of_default(&self) -> u32 {
        self.ahead_of_default.unwrap_or(self.ahead)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GitAction {
    Commit,
    Push,
    CreatePr,
    CommitPush,
    CommitPushPr,
}

impl GitAction {
    pub fn includes_commit(self) -> bool {
        matches!(self, Self::Commit | Self::CommitPush | Self::CommitPushPr)
    }

    /// Actions that ask before running on the default branch (§4b.6).
    pub fn confirmable(self) -> bool {
        !matches!(self, Self::Commit)
    }
}

/// The primary button of the split control (§4b.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuickAction {
    Run { label: String, action: GitAction },
    Pull,
    OpenPublish,
    Hint { label: String, hint: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickIcon {
    Commit,
    Push,
    Pull,
    Provider,
    Info,
}

impl QuickAction {
    pub fn label(&self) -> String {
        match self {
            Self::Run { label, .. } | Self::Hint { label, .. } => label.clone(),
            Self::Pull => "Pull".into(),
            Self::OpenPublish => "Publish repository".into(),
        }
    }

    pub fn hint(&self) -> Option<&str> {
        match self {
            Self::Hint { hint, .. } => Some(hint),
            _ => None,
        }
    }

    pub fn icon(&self) -> QuickIcon {
        match self {
            Self::OpenPublish => QuickIcon::Push,
            Self::Pull => QuickIcon::Pull,
            Self::Run { action, .. } => match action {
                GitAction::Commit => QuickIcon::Commit,
                GitAction::Push | GitAction::CommitPush => QuickIcon::Push,
                GitAction::CreatePr | GitAction::CommitPushPr => QuickIcon::Provider,
            },
            Self::Hint { label, .. } => match label.as_str() {
                "Commit" => QuickIcon::Commit,
                "Push" => QuickIcon::Push,
                _ => QuickIcon::Info,
            },
        }
    }
}

fn hint(label: &str, hint: &str) -> QuickAction {
    QuickAction::Hint {
        label: label.into(),
        hint: hint.into(),
    }
}

fn run(label: String, action: GitAction) -> QuickAction {
    QuickAction::Run { label, action }
}

const UP_TO_DATE: &str = "Branch is up to date. No action needed.";

/// `resolveQuickAction`: first match wins.
pub fn resolve_quick_action(status: Option<&GitStatus>, busy: bool) -> QuickAction {
    if busy {
        return hint("Commit", "Git action in progress.");
    }
    let Some(s) = status else {
        return hint("Commit", "Git status is unavailable.");
    };
    if s.branch.is_none() {
        return hint(
            "Commit",
            "Create and checkout a ref before pushing or opening a pull request.",
        );
    }
    let dirty = s.has_changes;
    let no_remote = !s.has_upstream() && !s.has_primary_remote;
    if dirty {
        if s.behind > 0 || no_remote {
            return run("Commit".into(), GitAction::Commit);
        }
        if s.has_open_pr || s.is_default_ref {
            return run("Commit & push".into(), GitAction::CommitPush);
        }
        return run(format!("Commit, push & {PR}"), GitAction::CommitPushPr);
    }
    if !s.has_upstream() {
        if !s.has_primary_remote {
            return QuickAction::OpenPublish;
        }
        if s.ahead == 0 {
            return if s.has_open_pr {
                hint("Commit", UP_TO_DATE)
            } else {
                hint("Push", "No local commits to push.")
            };
        }
        if s.has_open_pr || s.is_default_ref {
            return run("Push".into(), GitAction::Push);
        }
        return run(format!("Push & create {PR}"), GitAction::CreatePr);
    }
    if s.ahead > 0 && s.behind > 0 {
        return hint(
            "Sync ref",
            "Branch has diverged from upstream. Rebase/merge first.",
        );
    }
    if s.behind > 0 {
        return QuickAction::Pull;
    }
    if s.ahead > 0 {
        if s.has_open_pr || s.is_default_ref {
            return run("Push".into(), GitAction::Push);
        }
        return run(format!("Push & create {PR}"), GitAction::CreatePr);
    }
    if s.has_open_pr {
        return hint("Commit", UP_TO_DATE);
    }
    if s.ahead_of_default() > 0 && !s.is_default_ref {
        return run(format!("Create {PR}"), GitAction::CreatePr);
    }
    hint("Commit", UP_TO_DATE)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuId {
    Commit,
    Push,
    Pr,
    Publish,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitMenuItem {
    pub id: MenuId,
    pub label: String,
    pub enabled: bool,
    /// Shown in a hover popover when disabled.
    pub disabled_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeTone {
    Warning,
    Destructive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitMenu {
    pub items: Vec<GitMenuItem>,
    pub notices: Vec<(NoticeTone, String)>,
}

impl GitMenu {
    /// `hasUsableGitMenuContent`.
    pub fn usable(&self) -> bool {
        !self.notices.is_empty()
            || self
                .items
                .iter()
                .any(|i| i.enabled || i.id == MenuId::Publish)
    }
}

/// The chevron menu (§4b.3).
pub fn git_menu(status: Option<&GitStatus>, status_error: Option<&str>, busy: bool) -> GitMenu {
    let mut menu = GitMenu {
        items: Vec::new(),
        notices: Vec::new(),
    };
    if let Some(s) = status {
        let branch = s.branch.is_some();
        let remote_ok = s.has_upstream() || s.has_primary_remote;
        let mut push = |id, label: String, shown: bool, enabled: bool| {
            if shown {
                menu.items.push(GitMenuItem {
                    id,
                    disabled_reason: (!enabled).then(|| menu_disabled_reason(id, s, busy)),
                    label,
                    enabled,
                });
            }
        };
        push(
            MenuId::Commit,
            "Commit".into(),
            true,
            !busy && s.has_changes,
        );
        push(
            MenuId::Push,
            "Push".into(),
            s.has_primary_remote,
            !busy && branch && s.behind == 0 && s.ahead > 0 && remote_ok,
        );
        push(
            MenuId::Pr,
            format!("Create {PR}"),
            s.has_primary_remote && !s.has_open_pr,
            !busy
                && branch
                && !s.has_open_pr
                && s.ahead_of_default() > 0
                && s.behind == 0
                && remote_ok,
        );
        push(
            MenuId::Publish,
            "Publish repository...".into(),
            !s.has_primary_remote,
            !busy,
        );
        if !branch {
            menu.notices.push((
                NoticeTone::Warning,
                "Detached HEAD: create and check out a branch to enable push and pull request actions."
                    .into(),
            ));
        } else if !s.has_changes && s.behind > 0 && s.ahead == 0 {
            menu.notices.push((
                NoticeTone::Warning,
                "Behind upstream. Pull/rebase first.".into(),
            ));
        }
    }
    if let Some(err) = status_error {
        menu.notices
            .push((NoticeTone::Destructive, err.to_string()));
    }
    menu
}

/// `getMenuActionDisabledReason`.
pub fn menu_disabled_reason(id: MenuId, s: &GitStatus, busy: bool) -> String {
    if busy {
        return "Git action in progress.".into();
    }
    let detached = s.branch.is_none();
    let no_remote = !s.has_upstream() && !s.has_primary_remote;
    let r = match id {
        MenuId::Commit if !s.has_changes => "Worktree is clean. Make changes before committing.",
        MenuId::Commit => "Commit is currently unavailable.",
        MenuId::Push if detached => "Detached HEAD: check out a branch before pushing.",
        MenuId::Push if s.behind > 0 => "Branch is behind upstream. Pull/rebase before pushing.",
        MenuId::Push if no_remote => "Add an \"origin\" remote before pushing.",
        MenuId::Push if s.ahead == 0 => "No local commits to push.",
        MenuId::Push => "Push is currently unavailable.",
        MenuId::Pr if detached => {
            "Detached HEAD: check out a branch before creating a pull request."
        }
        MenuId::Pr if no_remote => "Add an \"origin\" remote before creating a pull request.",
        MenuId::Pr if s.ahead_of_default() == 0 => "No local commits to include in a pull request.",
        MenuId::Pr if s.behind > 0 => {
            "Branch is behind upstream. Pull/rebase before creating a pull request."
        }
        MenuId::Pr => "Create pull request is currently unavailable.",
        MenuId::Publish => "Publish is currently unavailable.",
    };
    r.into()
}

/// Copy for the default-branch confirmation (§4b.6): (title, description, continue label).
pub fn default_branch_copy(
    action: GitAction,
    includes_commit: bool,
    branch: &str,
) -> (String, String, String) {
    let tail =
        "You can continue on this ref or create a feature ref and run the same action there.";
    match (action, includes_commit) {
        (GitAction::Push | GitAction::CommitPush, true) => (
            "Commit & push to default ref?".into(),
            format!("This action will commit and push changes on \"{branch}\". {tail}"),
            format!("Commit & push to {branch}"),
        ),
        (GitAction::Push | GitAction::CommitPush, false) => (
            "Push to default ref?".into(),
            format!("This action will push local commits on \"{branch}\". {tail}"),
            format!("Push to {branch}"),
        ),
        (GitAction::CreatePr | GitAction::CommitPushPr, true) => (
            "Commit, push & create PR from default ref?".into(),
            format!(
                "This action will commit, push, and create a pull request on \"{branch}\". {tail}"
            ),
            "Commit, push & create PR".into(),
        ),
        (GitAction::CreatePr | GitAction::CommitPushPr, false) => (
            "Push & create PR from default ref?".into(),
            format!(
                "This action will push local commits and create a pull request on \"{branch}\". {tail}"
            ),
            "Push & create PR".into(),
        ),
        (GitAction::Commit, _) => (
            "Run action on default branch?".into(),
            String::new(),
            "Continue".into(),
        ),
    }
}

/// Why "Check out feature branch & continue" is unavailable, if it is.
pub fn feature_branch_unavailable(action: GitAction, dirty: bool, branch: &str) -> Option<String> {
    if matches!(action, GitAction::Push | GitAction::CreatePr) {
        return Some(format!(
            "This action doesn't commit your changes, so a feature branch can't be named. Create a branch yourself or continue on {branch}."
        ));
    }
    (!dirty).then(|| {
        format!(
            "There are no changes to commit, so a feature branch can't be named. Create a branch yourself or continue on {branch}."
        )
    })
}

/// Whether a run should stop at the default-branch dialog first.
pub fn needs_default_branch_confirm(action: GitAction, status: &GitStatus) -> bool {
    action.confirmable() && status.is_default_ref && status.branch.is_some()
}

/// Totals for the Changes row (§4b.4): (base label, additions, deletions).
pub fn changes_totals(status: Option<&GitStatus>) -> (Option<String>, u32, u32) {
    let Some(s) = status else {
        return (Some("Uncommitted".into()), 0, 0);
    };
    if let Some(b) = &s.branch_changes
        && (b.additions > 0 || b.deletions > 0)
    {
        return (Some(format!("vs {}", b.base)), b.additions, b.deletions);
    }
    (Some("Uncommitted".into()), s.additions, s.deletions)
}

/// `Intl.NumberFormat` for en-US: thousands separators.
pub fn format_count(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `formatGitActionElapsed`.
pub fn format_elapsed(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}

/// Server toast descriptions are cut to 72 chars with "...".
pub fn truncate_description(text: &str) -> String {
    if text.chars().count() <= 72 {
        text.to_string()
    } else {
        let cut: String = text.chars().take(69).collect();
        format!("{}...", cut.trim_end())
    }
}

// ---- branch picker ---------------------------------------------------------

/// `sanitizeNewRefName`: trimmed, runs of ASCII whitespace become `-`.
pub fn sanitize_new_ref_name(query: &str) -> String {
    let mut out = String::new();
    let mut in_space = false;
    for c in query.trim().chars() {
        if c.is_ascii_whitespace() {
            if !in_space {
                out.push('-');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// Client-side ref filter: keeps a ref whose lowercase name contains the query (or the
/// sanitized query when that differs).
pub fn ref_matches(name: &str, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let name = name.to_lowercase();
    let sanitized = sanitize_new_ref_name(&q);
    name.contains(&q) || (sanitized != q && name.contains(&sanitized))
}

/// The badge on a ref row, first that applies.
pub fn ref_badge(
    current: bool,
    worktree_path: Option<&str>,
    project_root: &str,
    remote: bool,
    default: bool,
) -> Option<&'static str> {
    if current {
        Some("current")
    } else if worktree_path
        .is_some_and(|p| p.trim_end_matches('/') != project_root.trim_end_matches('/'))
    {
        Some("worktree")
    } else if remote {
        Some("remote")
    } else if default {
        Some("default")
    } else {
        None
    }
}

/// `resolveBranchTriggerLabel`.
pub fn branch_trigger_label(
    branch: Option<&str>,
    choosing_base: bool,
    start_from_origin: bool,
    known_local: bool,
) -> String {
    match branch {
        None => "Select ref".into(),
        Some(b) if choosing_base && start_from_origin && known_local => {
            format!("From {b} (remote)")
        }
        Some(b) if choosing_base => format!("From {b}"),
        Some(b) => b.to_string(),
    }
}

/// A query that parses as a pull request reference: `#123`, `123`, a PR URL or a
/// `gh pr checkout` command. Returns the normalized reference.
pub fn parse_pr_reference(query: &str) -> Option<String> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let digits = q.strip_prefix('#').unwrap_or(q);
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!("#{digits}"));
    }
    if let Some(rest) = q.strip_prefix("gh pr checkout ") {
        let r = rest.trim().trim_start_matches('#');
        if !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("#{r}"));
        }
    }
    if (q.starts_with("https://") || q.starts_with("http://"))
        && let Some((_, tail)) = q.split_once("/pull/")
    {
        let n: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !n.is_empty() {
            return Some(q.to_string());
        }
    }
    None
}

/// Middle truncation that keeps the tail visible (`ui/middle-truncate.tsx`).
pub fn middle_truncate(text: &str, max_chars: usize) -> String {
    let n = text.chars().count();
    if n <= max_chars || max_chars < 5 {
        return text.to_string();
    }
    let keep = max_chars - 1;
    let head = keep / 2;
    let tail = keep - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(n - tail).collect();
    format!("{start}…{end}")
}

// ---- project scripts -------------------------------------------------------

/// `nextProjectScriptId`: slug of the name, max 24, `script` when empty, `-2`, `-3`, ...
/// on collision.
pub fn next_script_id(name: &str, existing: &[String]) -> String {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(24).collect();
    let base = slug.trim_end_matches('-');
    let base = if base.is_empty() { "script" } else { base };
    if !existing.iter().any(|e| e == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !existing.iter().any(|e| e == id))
        .unwrap_or_else(|| base.to_string())
}

/// The script the split row runs: the last one run, else the first that is neither a
/// setup nor an on-settle script, else the first that is not on-settle.
pub fn primary_script<'a>(
    scripts: &'a [ProjectScript],
    last_invoked: Option<&str>,
) -> Option<&'a ProjectScript> {
    if let Some(id) = last_invoked
        && let Some(s) = scripts.iter().find(|s| s.id == id)
    {
        return Some(s);
    }
    scripts
        .iter()
        .find(|s| !s.run_on_worktree_create && !s.run_on_settle)
        .or_else(|| scripts.iter().find(|s| !s.run_on_settle))
}

/// "{name} (setup)", "{name} (on settle)", "{name} (setup, on settle)".
pub fn script_menu_label(script: &ProjectScript) -> String {
    match (script.run_on_worktree_create, script.run_on_settle) {
        (true, true) => format!("{} (setup, on settle)", script.name),
        (true, false) => format!("{} (setup)", script.name),
        (false, true) => format!("{} (on settle)", script.name),
        (false, false) => script.name.clone(),
    }
}

/// Saving a script that claims the setup or on-settle role releases it elsewhere.
pub fn release_claimed_roles(scripts: &mut [ProjectScript], keep_id: &str) {
    let (setup, settle) = scripts
        .iter()
        .find(|s| s.id == keep_id)
        .map(|s| (s.run_on_worktree_create, s.run_on_settle))
        .unwrap_or_default();
    for s in scripts.iter_mut().filter(|s| s.id != keep_id) {
        if setup {
            s.run_on_worktree_create = false;
            s.blocking_setup = false;
        }
        if settle {
            s.run_on_settle = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st() -> GitStatus {
        GitStatus {
            branch: Some("feature/x".into()),
            upstream: Some("origin/feature/x".into()),
            has_primary_remote: true,
            ..Default::default()
        }
    }

    fn label(s: &GitStatus) -> String {
        resolve_quick_action(Some(s), false).label()
    }

    #[test]
    fn quick_action_rows() {
        assert_eq!(resolve_quick_action(None, false).label(), "Commit");
        assert_eq!(
            resolve_quick_action(None, false).hint(),
            Some("Git status is unavailable.")
        );
        assert_eq!(
            resolve_quick_action(Some(&st()), true).hint(),
            Some("Git action in progress.")
        );
        let mut s = st();
        s.branch = None;
        assert!(
            resolve_quick_action(Some(&s), false)
                .hint()
                .unwrap()
                .starts_with("Create and checkout")
        );

        let mut s = st();
        s.has_changes = true;
        assert_eq!(label(&s), "Commit, push & PR");
        s.is_default_ref = true;
        assert_eq!(label(&s), "Commit & push");
        s.behind = 1;
        assert_eq!(label(&s), "Commit");

        // dirty, no upstream, no remote
        let s = GitStatus {
            branch: Some("main".into()),
            has_changes: true,
            ..Default::default()
        };
        assert_eq!(label(&s), "Commit");
        let s = GitStatus {
            branch: Some("main".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_quick_action(Some(&s), false),
            QuickAction::OpenPublish
        );
    }

    #[test]
    fn quick_action_clean_rows() {
        let mut s = st();
        s.upstream = None;
        assert_eq!(label(&s), "Push");
        assert!(resolve_quick_action(Some(&s), false).hint().is_some());
        s.ahead = 2;
        assert_eq!(label(&s), "Push & create PR");
        s.is_default_ref = true;
        assert_eq!(
            resolve_quick_action(Some(&s), false),
            QuickAction::Run {
                label: "Push".into(),
                action: GitAction::Push
            }
        );

        let mut s = st();
        s.ahead = 1;
        s.behind = 1;
        assert_eq!(label(&s), "Sync ref");
        assert_eq!(
            resolve_quick_action(Some(&s), false).icon(),
            QuickIcon::Info
        );
        s.ahead = 0;
        assert_eq!(resolve_quick_action(Some(&s), false), QuickAction::Pull);
        s.behind = 0;
        s.ahead = 3;
        assert_eq!(label(&s), "Push & create PR");
        s.ahead = 0;
        s.ahead_of_default = Some(2);
        assert_eq!(label(&s), "Create PR");
        s.has_open_pr = true;
        assert_eq!(
            resolve_quick_action(Some(&s), false).hint(),
            Some(UP_TO_DATE)
        );
    }

    #[test]
    fn menu_items_and_reasons() {
        let mut s = st();
        let m = git_menu(Some(&s), None, false);
        assert_eq!(
            m.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            ["Commit", "Push", "Create PR"]
        );
        assert!(m.items.iter().all(|i| !i.enabled));
        assert_eq!(
            m.items[0].disabled_reason.as_deref(),
            Some("Worktree is clean. Make changes before committing.")
        );
        assert_eq!(
            m.items[1].disabled_reason.as_deref(),
            Some("No local commits to push.")
        );
        assert!(!m.usable());
        s.has_changes = true;
        s.ahead = 1;
        let m = git_menu(Some(&s), None, false);
        assert!(m.items.iter().all(|i| i.enabled));
        s.has_primary_remote = false;
        s.upstream = None;
        let m = git_menu(Some(&s), None, false);
        assert_eq!(m.items.last().unwrap().label, "Publish repository...");
        assert!(m.usable());
        let m = git_menu(None, Some("boom"), false);
        assert!(m.items.is_empty());
        assert_eq!(m.notices, vec![(NoticeTone::Destructive, "boom".into())]);
        let mut s = st();
        s.behind = 2;
        let m = git_menu(Some(&s), None, false);
        assert_eq!(m.notices[0].1, "Behind upstream. Pull/rebase first.");
    }

    #[test]
    fn default_branch_dialog_copy() {
        let (t, d, c) = default_branch_copy(GitAction::CommitPush, true, "main");
        assert_eq!(t, "Commit & push to default ref?");
        assert!(d.starts_with("This action will commit and push changes on \"main\"."));
        assert_eq!(c, "Commit & push to main");
        let (t, _, c) = default_branch_copy(GitAction::CreatePr, false, "main");
        assert_eq!(t, "Push & create PR from default ref?");
        assert_eq!(c, "Push & create PR");
        assert!(feature_branch_unavailable(GitAction::Push, true, "main").is_some());
        assert!(feature_branch_unavailable(GitAction::CommitPush, true, "main").is_none());
        assert!(feature_branch_unavailable(GitAction::CommitPush, false, "main").is_some());
    }

    #[test]
    fn changes_prefers_branch_range() {
        let mut s = st();
        s.additions = 2;
        assert_eq!(changes_totals(Some(&s)), (Some("Uncommitted".into()), 2, 0));
        s.branch_changes = Some(BranchChanges {
            base: "main".into(),
            additions: 10,
            deletions: 4,
        });
        assert_eq!(changes_totals(Some(&s)), (Some("vs main".into()), 10, 4));
        assert_eq!(format_count(1234567), "1,234,567");
        assert_eq!(format_count(12), "12");
    }

    #[test]
    fn elapsed_and_truncation() {
        assert_eq!(format_elapsed(9), "9s");
        assert_eq!(format_elapsed(75), "1m 15s");
        let long = "x".repeat(100);
        assert_eq!(truncate_description(&long).chars().count(), 72);
    }

    #[test]
    fn ref_names_and_filter() {
        assert_eq!(sanitize_new_ref_name("  fix  the bug "), "fix-the-bug");
        assert!(ref_matches("feature/fix-the-bug", "fix the"));
        assert!(ref_matches("Main", "mai"));
        assert!(!ref_matches("main", "dev"));
        assert!(ref_matches("anything", "  "));
        assert_eq!(ref_badge(true, None, "/r", false, true), Some("current"));
        assert_eq!(
            ref_badge(false, Some("/r"), "/r", false, true),
            Some("default")
        );
        assert_eq!(
            ref_badge(false, Some("/w"), "/r", true, false),
            Some("worktree")
        );
        assert_eq!(ref_badge(false, None, "/r", true, false), Some("remote"));
        assert_eq!(
            branch_trigger_label(None, false, false, false),
            "Select ref"
        );
        assert_eq!(
            branch_trigger_label(Some("main"), true, true, true),
            "From main (remote)"
        );
        assert_eq!(
            branch_trigger_label(Some("main"), true, false, true),
            "From main"
        );
        assert_eq!(
            branch_trigger_label(Some("main"), false, true, true),
            "main"
        );
    }

    #[test]
    fn pr_references() {
        assert_eq!(parse_pr_reference("#42").as_deref(), Some("#42"));
        assert_eq!(parse_pr_reference("42").as_deref(), Some("#42"));
        assert_eq!(
            parse_pr_reference("gh pr checkout 7").as_deref(),
            Some("#7")
        );
        assert!(parse_pr_reference("https://github.com/a/b/pull/9/files").is_some());
        assert_eq!(parse_pr_reference("main"), None);
        assert_eq!(
            middle_truncate("feature/very-long-branch-name", 11),
            "featu…-name"
        );
    }

    fn script(id: &str, setup: bool, settle: bool) -> ProjectScript {
        ProjectScript {
            id: id.into(),
            name: id.to_uppercase(),
            command: "x".into(),
            run_on_worktree_create: setup,
            run_on_settle: settle,
            ..Default::default()
        }
    }

    #[test]
    fn script_helpers() {
        assert_eq!(next_script_id("Run Tests!", &[]), "run-tests");
        assert_eq!(next_script_id("", &[]), "script");
        assert_eq!(
            next_script_id("test", &["test".into(), "test-2".into()]),
            "test-3"
        );
        assert_eq!(
            next_script_id("a very long script name that keeps going", &[]),
            "a-very-long-script-name"
        );
        let scripts = vec![script("setup", true, false), script("run", false, false)];
        assert_eq!(primary_script(&scripts, None).unwrap().id, "run");
        assert_eq!(primary_script(&scripts, Some("setup")).unwrap().id, "setup");
        let only = vec![script("s", true, false), script("t", false, true)];
        assert_eq!(primary_script(&only, None).unwrap().id, "s");
        assert_eq!(
            script_menu_label(&script("a", true, true)),
            "A (setup, on settle)"
        );
        let mut many = vec![script("a", true, false), script("b", true, true)];
        release_claimed_roles(&mut many, "b");
        assert!(!many[0].run_on_worktree_create);
        assert!(many[1].run_on_worktree_create);
    }
}

//! Row derivation for the sidebar (spec 02 §2.4, §2.7): which section a thread lives in,
//! the order inside each section, the status label, and the time labels. No GPUI here.

use std::cmp::Ordering;
use std::collections::HashMap;

use tern_core::{
    FailureClass, Project, ProjectId, RequestKind, RunStatus, ShellThreadStatus, Snapshot,
    ThreadId, ThreadShell, Ts,
};

/// Settled shows 10 rows, then pages of 25 ("Show N more").
pub const SETTLED_FIRST_PAGE: usize = 10;
pub const SETTLED_PAGE: usize = 25;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Pinned,
    Active,
    Working,
    Settled,
}

/// What the status slot shows (§2.7), first match wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowStatus {
    Approval,
    Input,
    /// Blocked or idle between steps; duration since the last run completed.
    Waiting {
        since: Option<Ts>,
    },
    /// "Setting up" plus a live duration.
    Preparing {
        since: Option<Ts>,
    },
    /// "Working" plus a live duration.
    Working {
        since: Option<Ts>,
    },
    Limited,
    Failed,
    /// Ready, with a completed run the user has not seen.
    Done,
    /// Nothing to report: the slot shows the relative time.
    Ready,
}

impl RowStatus {
    pub fn label(self) -> Option<&'static str> {
        Some(match self {
            Self::Approval => "Approval",
            Self::Input => "Input",
            Self::Waiting { .. } => "Waiting",
            Self::Preparing { .. } => "Setting up",
            Self::Working { .. } => "Working",
            Self::Limited => "Limited",
            Self::Failed => "Failed",
            Self::Done => "Done",
            Self::Ready => return None,
        })
    }

    /// The start of the live duration shown after the label, if any.
    pub fn since(self) -> Option<Ts> {
        match self {
            Self::Waiting { since } | Self::Preparing { since } | Self::Working { since } => since,
            _ => None,
        }
    }

    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Waiting { .. } | Self::Preparing { .. } | Self::Working { .. }
        )
    }
}

/// `Sidebar.logic.ts:964-1005`.
pub fn row_status(shell: &ThreadShell) -> RowStatus {
    if let Some((_, kind)) = &shell.pending_request {
        return if *kind == RequestKind::UserInput {
            RowStatus::Input
        } else {
            RowStatus::Approval
        };
    }
    let started = shell.latest_run_started_at;
    match shell.status {
        ShellThreadStatus::Run(RunStatus::Waiting) => RowStatus::Waiting {
            since: shell.latest_run_completed_at.or(started),
        },
        ShellThreadStatus::Run(RunStatus::Preparing) => RowStatus::Preparing { since: started },
        ShellThreadStatus::Run(RunStatus::Queued | RunStatus::Starting | RunStatus::Running) => {
            RowStatus::Working { since: started }
        }
        ShellThreadStatus::Run(RunStatus::Failed)
            if shell.last_error_class == Some(FailureClass::UsageLimit) =>
        {
            RowStatus::Limited
        }
        // A failed run the user already opened reverts to ready.
        ShellThreadStatus::Run(RunStatus::Failed) if shell.is_unread() => RowStatus::Failed,
        ShellThreadStatus::Run(RunStatus::Completed) if shell.is_unread() => RowStatus::Done,
        _ => RowStatus::Ready,
    }
}

/// `isThreadWorking`: no pending request and the runtime is busy.
pub fn is_working(shell: &ThreadShell) -> bool {
    shell.pending_request.is_none()
        && matches!(
            shell.status,
            ShellThreadStatus::Run(
                RunStatus::Preparing
                    | RunStatus::Queued
                    | RunStatus::Starting
                    | RunStatus::Running
                    | RunStatus::Waiting
            )
        )
}

/// Receded rows (`Sidebar.logic.ts:930-962`) draw muted until hovered.
pub fn is_receded(status: RowStatus, unread: bool, open: bool, selected: bool) -> bool {
    if open || selected {
        return false;
    }
    match status {
        RowStatus::Input => false,
        RowStatus::Preparing { .. } | RowStatus::Working { .. } | RowStatus::Waiting { .. } => true,
        RowStatus::Ready | RowStatus::Approval => !unread,
        _ => false,
    }
}

/// The time a row's relative label counts from (`threadSort.ts:41-58`).
pub fn activity_at(shell: &ThreadShell) -> Ts {
    shell
        .latest_run_started_at
        .into_iter()
        .chain(shell.latest_run_completed_at)
        .max()
        .unwrap_or(shell.thread.updated_at)
}

/// The Active section's sort key with the Working shelf on: the newest of creation and
/// the latest run's request/completion.
fn active_sort_at(shell: &ThreadShell) -> Ts {
    [
        Some(shell.thread.created_at),
        shell.latest_run_started_at,
        shell.latest_run_completed_at,
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(shell.thread.created_at)
}

fn working_sort_at(shell: &ThreadShell) -> Ts {
    shell
        .latest_run_started_at
        .map_or(shell.thread.created_at, |t| t.max(shell.thread.created_at))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub thread_id: ThreadId,
    pub project_id: ProjectId,
    pub section: Section,
    pub status: RowStatus,
    pub unread: bool,
    /// Relative-time anchor: settle time for settled rows, else activity.
    pub time_at: Ts,
}

/// What the list shows, section by section, in display order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sections {
    pub pinned: Vec<Row>,
    pub active: Vec<Row>,
    pub working: Vec<Row>,
    pub settled: Vec<Row>,
}

impl Sections {
    pub fn is_empty(&self) -> bool {
        self.pinned.is_empty()
            && self.active.is_empty()
            && self.working.is_empty()
            && self.settled.is_empty()
    }
}

pub struct DeriveOptions<'a> {
    /// Projects in scope; empty means all.
    pub scope: &'a [ProjectId],
    pub working_shelf: bool,
    /// Manual pinned order (position by thread id), written by drag.
    pub pin_order: &'a HashMap<ThreadId, usize>,
}

impl Default for DeriveOptions<'_> {
    fn default() -> Self {
        static EMPTY: std::sync::LazyLock<HashMap<ThreadId, usize>> =
            std::sync::LazyLock::new(HashMap::new);
        Self {
            scope: &[],
            working_shelf: true,
            pin_order: &EMPTY,
        }
    }
}

/// Not archived, not deleted, inside the scope.
pub fn is_visible(shell: &ThreadShell, scope: &[ProjectId]) -> bool {
    shell.thread.archived_at.is_none()
        && shell.thread.deleted_at.is_none()
        && (scope.is_empty() || scope.contains(&shell.thread.project_id))
}

pub fn section_of(shell: &ThreadShell, working_shelf: bool) -> Section {
    if shell.thread.settled_at.is_some() {
        Section::Settled
    } else if shell.thread.pinned_at.is_some() {
        Section::Pinned
    } else if working_shelf && is_working(shell) {
        Section::Working
    } else {
        Section::Active
    }
}

fn row(shell: &ThreadShell, section: Section) -> Row {
    Row {
        thread_id: shell.thread.id.clone(),
        project_id: shell.thread.project_id.clone(),
        section,
        status: row_status(shell),
        unread: shell.is_unread(),
        time_at: match section {
            Section::Settled => shell
                .thread
                .settled_at
                .unwrap_or_else(|| activity_at(shell)),
            _ => activity_at(shell),
        },
    }
}

/// Splits the snapshot's threads into sections and sorts each one.
pub fn derive(snapshot: &Snapshot, opts: &DeriveOptions) -> Sections {
    let mut by_section: HashMap<Section, Vec<&ThreadShell>> = HashMap::new();
    for shell in &snapshot.threads {
        if is_visible(shell, opts.scope) {
            by_section
                .entry(section_of(shell, opts.working_shelf))
                .or_default()
                .push(shell);
        }
    }
    let tie = |a: &ThreadShell, b: &ThreadShell| a.thread.id.cmp(&b.thread.id);
    fn take<'a>(
        s: Section,
        by_section: &mut HashMap<Section, Vec<&'a ThreadShell>>,
    ) -> Vec<&'a ThreadShell> {
        by_section.remove(&s).unwrap_or_default()
    }

    let mut pinned = take(Section::Pinned, &mut by_section);
    pinned.sort_by(|a, b| {
        let ka = opts.pin_order.get(&a.thread.id);
        let kb = opts.pin_order.get(&b.thread.id);
        match (ka, kb) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => b.thread.created_at.cmp(&a.thread.created_at),
        }
        .then_with(|| tie(a, b))
    });

    let mut active = take(Section::Active, &mut by_section);
    active.sort_by(|a, b| {
        active_sort_at(b)
            .cmp(&active_sort_at(a))
            .then_with(|| tie(a, b))
    });

    let mut working = take(Section::Working, &mut by_section);
    working.sort_by(|a, b| {
        working_sort_at(b)
            .cmp(&working_sort_at(a))
            .then_with(|| tie(a, b))
    });

    let mut settled = take(Section::Settled, &mut by_section);
    settled.sort_by(|a, b| {
        let at = |s: &ThreadShell| s.thread.settled_at.unwrap_or_else(|| activity_at(s));
        at(b).cmp(&at(a)).then_with(|| tie(a, b))
    });

    Sections {
        pinned: pinned
            .into_iter()
            .map(|s| row(s, Section::Pinned))
            .collect(),
        active: active
            .into_iter()
            .map(|s| row(s, Section::Active))
            .collect(),
        working: working
            .into_iter()
            .map(|s| row(s, Section::Working))
            .collect(),
        settled: settled
            .into_iter()
            .map(|s| row(s, Section::Settled))
            .collect(),
    }
}

/// Project order for `updated_at` (`Sidebar.logic.ts:1378-1505`): the newest thread activity
/// in the project, else the project's own update time; ties by title, then id. Archived
/// projects are left out.
pub fn ordered_projects(snapshot: &Snapshot) -> Vec<&Project> {
    let mut latest: HashMap<&str, Ts> = HashMap::new();
    for shell in &snapshot.threads {
        if shell.thread.deleted_at.is_some() {
            continue;
        }
        let at = activity_at(shell);
        latest
            .entry(shell.thread.project_id.as_str())
            .and_modify(|t| *t = (*t).max(at))
            .or_insert(at);
    }
    let mut projects: Vec<&Project> = snapshot
        .projects
        .iter()
        .filter(|p| p.archived_at.is_none())
        .collect();
    let key = |p: &Project| latest.get(p.id.as_str()).copied().unwrap_or(p.updated_at);
    projects.sort_by(|a, b| {
        key(b)
            .cmp(&key(a))
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.id.cmp(&b.id))
    });
    projects
}

/// Where `/` goes: a draft in the most recent project.
pub fn index_project(snapshot: &Snapshot) -> Option<ProjectId> {
    ordered_projects(snapshot).first().map(|p| p.id.clone())
}

/// After settling or archiving the open thread: the next row below it that is not
/// settled (wrapping), in visible order (`Sidebar.tsx:1645-1670`).
pub fn next_after(order: &[(ThreadId, Section)], current: &str) -> Option<ThreadId> {
    let at = order.iter().position(|(id, _)| id == current)?;
    let n = order.len();
    (1..n)
        .map(|step| &order[(at + step) % n])
        .find(|(_, section)| *section != Section::Settled)
        .map(|(id, _)| id.clone())
}

// ---------------------------------------------------------------------------
// Time labels

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// The sidebar's compact relative time (`compactSidebarTimeLabel`): "now" under a
/// minute, else "5m", "3h", "2d".
pub fn compact_relative(at: Ts, now: Ts) -> String {
    let elapsed = (now - at).num_milliseconds().max(0);
    if elapsed < MINUTE {
        return "now".into();
    }
    if elapsed < HOUR {
        return format!("{}m", elapsed / MINUTE);
    }
    if elapsed < DAY {
        return format!("{}h", elapsed / HOUR);
    }
    format!("{}d", elapsed / DAY)
}

/// The palette's relative time (`formatRelativeTimeLabel`): "just now", "5m ago", …
pub fn relative_ago(at: Ts, now: Ts) -> String {
    let elapsed = (now - at).num_milliseconds().max(0);
    if elapsed < MINUTE {
        return "just now".into();
    }
    if elapsed < HOUR {
        return format!("{}m ago", elapsed / MINUTE);
    }
    if elapsed < DAY {
        return format!("{}h ago", elapsed / HOUR);
    }
    format!("{}d ago", elapsed / DAY)
}

/// A live run duration (`formatWorkingDurationLabel`): "5s", "12m", "1h 4m".
pub fn working_duration(since: Ts, now: Ts) -> String {
    let seconds = (now - since).num_seconds().max(0);
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    format!("{}h {}m", minutes / 60, minutes % 60)
}

// ---------------------------------------------------------------------------
// Project identity (`projectIdentity.ts`, `projectMonogram.ts`)

/// The two-letter mark shown when a project has no favicon: "demo-notes" -> "DN".
pub fn project_monogram(name: &str) -> String {
    let words: Vec<Vec<char>> = name
        .trim()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.chars().collect())
        .collect();
    let Some(first_word) = words.first() else {
        return "PR".into();
    };
    let first = first_word[0];
    if is_single_glyph_script(first) {
        return first.to_string();
    }
    let second = first_word[1..]
        .iter()
        .copied()
        .find(|c| c.is_numeric())
        .or_else(|| {
            if words.len() > 1 {
                words.last().and_then(|w| w.first().copied())
            } else {
                first_word.last().copied()
            }
        })
        .unwrap_or(first);
    format!("{first}{second}")
        .to_uppercase()
        .chars()
        .take(2)
        .collect()
}

fn is_single_glyph_script(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff | 0xac00..=0xd7af)
}

/// `PROJECT_ICON_COLORS` in order: (name, 600 shade, 400 shade). Light mode uses the 600,
/// dark the 400.
pub const PROJECT_COLORS: [(&str, u32, u32); 18] = [
    ("gray", 0x4b5563ff, 0x9ca3afff),
    ("red", 0xdc2626ff, 0xf87171ff),
    ("orange", 0xea580cff, 0xfb923cff),
    ("amber", 0xd97706ff, 0xfbbf24ff),
    ("yellow", 0xca8a04ff, 0xfacc15ff),
    ("lime", 0x65a30dff, 0xa3e635ff),
    ("green", 0x16a34aff, 0x4ade80ff),
    ("emerald", 0x059669ff, 0x34d399ff),
    ("teal", 0x0d9488ff, 0x2dd4bfff),
    ("cyan", 0x0891b2ff, 0x22d3eeff),
    ("sky", 0x0284c7ff, 0x38bdf8ff),
    ("blue", 0x2563ebff, 0x60a5faff),
    ("indigo", 0x4f46e5ff, 0x818cf8ff),
    ("violet", 0x7c3aedff, 0xa78bfaff),
    ("purple", 0x9333eaff, 0xc084fcff),
    ("fuchsia", 0xc026d3ff, 0xe879f9ff),
    ("pink", 0xdb2777ff, 0xf472b6ff),
    ("rose", 0xe11d48ff, 0xfb7185ff),
];

/// Index into [`PROJECT_COLORS`]: a hash of the lowercased name.
pub fn project_color_index(name: &str) -> usize {
    let seed = name.trim().to_lowercase();
    let seed = if seed.is_empty() {
        "project".to_string()
    } else {
        seed
    };
    let mut index: u64 = 0;
    for c in seed.chars() {
        index = (index * 31 + c as u64) % PROJECT_COLORS.len() as u64;
    }
    index as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone as _, Utc};
    use tern_core::{InteractionMode, ModelSelection, RuntimeMode, Thread};

    fn t0() -> Ts {
        Utc.with_ymd_and_hms(2026, 11, 25, 12, 0, 0).unwrap()
    }

    fn shell(id: &str, project: &str, created_min: i64) -> ThreadShell {
        let created = t0() + Duration::minutes(created_min);
        ThreadShell {
            thread: Thread {
                id: id.into(),
                project_id: project.into(),
                title: id.into(),
                model_selection: ModelSelection {
                    instance_id: "codex".into(),
                    model: "gpt".into(),
                    options: vec![],
                },
                runtime_mode: RuntimeMode::FullAccess,
                interaction_mode: InteractionMode::Default,
                branch: None,
                worktree_path: None,
                active_provider_thread_id: None,
                title_renamed: false,
                title_generation_started_at: None,
                created_at: created,
                updated_at: created,
                archived_at: None,
                pinned_at: None,
                settled_at: None,
                last_visited_at: None,
                linked_pull_request: None,
                deleted_at: None,
            },
            status: ShellThreadStatus::Idle,
            active_run_id: None,
            latest_run_started_at: None,
            latest_run_completed_at: None,
            pending_request: None,
            latest_message: None,
            has_actionable_proposed_plan: false,
            last_error: None,
            last_error_class: None,
            usage_limit_reset_at: None,
            diff_totals: None,
            last_activity_at: created,
        }
    }

    fn ids(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.thread_id.as_str()).collect()
    }

    #[test]
    fn relative_time_matches_t3() {
        let now = t0();
        assert_eq!(compact_relative(now - Duration::seconds(59), now), "now");
        assert_eq!(compact_relative(now + Duration::seconds(5), now), "now");
        assert_eq!(compact_relative(now - Duration::minutes(9), now), "9m");
        assert_eq!(compact_relative(now - Duration::minutes(59), now), "59m");
        assert_eq!(compact_relative(now - Duration::minutes(60), now), "1h");
        assert_eq!(compact_relative(now - Duration::hours(23), now), "23h");
        assert_eq!(compact_relative(now - Duration::hours(49), now), "2d");
        assert_eq!(relative_ago(now - Duration::seconds(3), now), "just now");
        assert_eq!(relative_ago(now - Duration::minutes(11), now), "11m ago");
        assert_eq!(relative_ago(now - Duration::hours(5), now), "5h ago");
        assert_eq!(relative_ago(now - Duration::days(3), now), "3d ago");
    }

    #[test]
    fn working_duration_matches_t3() {
        let now = t0();
        assert_eq!(working_duration(now - Duration::seconds(5), now), "5s");
        assert_eq!(working_duration(now - Duration::seconds(75), now), "1m");
        assert_eq!(working_duration(now - Duration::minutes(64), now), "1h 4m");
        assert_eq!(working_duration(now + Duration::seconds(3), now), "0s");
    }

    #[test]
    fn monograms_and_colors() {
        assert_eq!(project_monogram("demo-notes"), "DN");
        assert_eq!(project_monogram("tern"), "TN");
        assert_eq!(project_monogram("t3code"), "T3");
        assert_eq!(project_monogram("harbor-api"), "HA");
        assert_eq!(project_monogram("  "), "PR");
        assert_eq!(project_monogram("日本"), "日");
        // The refs show demo-notes in lime.
        assert_eq!(PROJECT_COLORS[project_color_index("demo-notes")].0, "lime");
    }

    #[test]
    fn sections_and_sorting() {
        let mut snap = Snapshot::default();
        let mut pinned_old = shell("pin-old", "p", 0);
        pinned_old.thread.pinned_at = Some(t0());
        let mut pinned_new = shell("pin-new", "p", 10);
        pinned_new.thread.pinned_at = Some(t0());
        let mut settled_a = shell("settled-a", "p", 0);
        settled_a.thread.settled_at = Some(t0() + Duration::minutes(5));
        let mut settled_b = shell("settled-b", "p", 0);
        settled_b.thread.settled_at = Some(t0() + Duration::minutes(50));
        let mut running = shell("running", "p", 1);
        running.status = ShellThreadStatus::Run(RunStatus::Running);
        running.latest_run_started_at = Some(t0() + Duration::minutes(30));
        let mut approval = shell("approval", "p", 2);
        approval.status = ShellThreadStatus::Run(RunStatus::Waiting);
        approval.pending_request = Some(("r".into(), RequestKind::Command));
        let mut archived = shell("archived", "p", 3);
        archived.thread.archived_at = Some(t0());
        let quiet = shell("quiet", "p", 20);
        let other = shell("other-project", "q", 40);
        snap.threads = vec![
            pinned_old, pinned_new, settled_a, settled_b, running, approval, archived, quiet, other,
        ];

        let s = derive(&snap, &DeriveOptions::default());
        assert_eq!(ids(&s.pinned), ["pin-new", "pin-old"]);
        assert_eq!(ids(&s.active), ["other-project", "quiet", "approval"]);
        assert_eq!(ids(&s.working), ["running"]);
        assert_eq!(ids(&s.settled), ["settled-b", "settled-a"]);
        assert_eq!(s.active[2].status, RowStatus::Approval);

        // Shelf off: working threads stay in Active.
        let s = derive(
            &snap,
            &DeriveOptions {
                working_shelf: false,
                ..Default::default()
            },
        );
        assert!(s.working.is_empty());
        assert_eq!(ids(&s.active)[0], "running");

        // Manual pin order wins over creation time; scope filters projects.
        let order = HashMap::from([("pin-old".to_string(), 0), ("pin-new".to_string(), 1)]);
        let scope = vec!["q".to_string()];
        let s = derive(
            &snap,
            &DeriveOptions {
                pin_order: &order,
                ..Default::default()
            },
        );
        assert_eq!(ids(&s.pinned), ["pin-old", "pin-new"]);
        let s = derive(
            &snap,
            &DeriveOptions {
                scope: &scope,
                ..Default::default()
            },
        );
        assert_eq!(ids(&s.active), ["other-project"]);
        assert!(s.pinned.is_empty());
    }

    #[test]
    fn statuses() {
        let mut s = shell("a", "p", 0);
        assert_eq!(row_status(&s), RowStatus::Ready);
        s.status = ShellThreadStatus::Run(RunStatus::Completed);
        s.latest_run_completed_at = Some(t0() + Duration::minutes(1));
        assert_eq!(row_status(&s), RowStatus::Done);
        s.thread.last_visited_at = Some(t0() + Duration::minutes(2));
        assert_eq!(row_status(&s), RowStatus::Ready);
        s.status = ShellThreadStatus::Run(RunStatus::Failed);
        assert_eq!(row_status(&s), RowStatus::Ready, "seen failures revert");
        s.last_error_class = Some(FailureClass::UsageLimit);
        assert_eq!(row_status(&s), RowStatus::Limited);
        s.pending_request = Some(("r".into(), RequestKind::UserInput));
        assert_eq!(row_status(&s), RowStatus::Input);
        s.pending_request = None;
        s.status = ShellThreadStatus::Run(RunStatus::Preparing);
        assert!(matches!(row_status(&s), RowStatus::Preparing { .. }));
        assert_eq!(row_status(&s).label(), Some("Setting up"));
    }

    #[test]
    fn receded_rules() {
        assert!(is_receded(RowStatus::Ready, false, false, false));
        assert!(!is_receded(RowStatus::Ready, true, false, false));
        assert!(!is_receded(RowStatus::Ready, false, true, false));
        assert!(is_receded(
            RowStatus::Working { since: None },
            true,
            false,
            false
        ));
        assert!(!is_receded(RowStatus::Input, false, false, false));
        assert!(!is_receded(RowStatus::Done, false, false, false));
    }

    #[test]
    fn project_order_and_index() {
        let mut snap = Snapshot::default();
        let project = |id: &str, title: &str, updated: i64| Project {
            id: id.into(),
            title: title.into(),
            has_custom_title: false,
            workspace_root: format!("/{id}"),
            default_model_selection: None,
            default_thread_env_mode: None,
            favicon_path: None,
            scripts: vec![],
            pinned_at: None,
            archived_at: None,
            created_at: t0(),
            updated_at: t0() + Duration::minutes(updated),
        };
        snap.projects = vec![project("a", "alpha", 5), project("b", "beta", 1)];
        assert_eq!(index_project(&snap).as_deref(), Some("a"));
        snap.threads = vec![shell("t", "b", 30)];
        assert_eq!(index_project(&snap).as_deref(), Some("b"));
        snap.projects[1].archived_at = Some(t0());
        assert_eq!(index_project(&snap).as_deref(), Some("a"));
        snap.projects.clear();
        assert_eq!(index_project(&snap), None);
    }

    #[test]
    fn next_row_wraps_and_skips_settled() {
        let order = vec![
            ("a".to_string(), Section::Active),
            ("b".to_string(), Section::Active),
            ("c".to_string(), Section::Settled),
        ];
        assert_eq!(next_after(&order, "b").as_deref(), Some("a"));
        assert_eq!(next_after(&order, "a").as_deref(), Some("b"));
        assert_eq!(next_after(&[("a".to_string(), Section::Active)], "a"), None);
    }
}

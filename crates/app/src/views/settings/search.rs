//! The settings search index (`settingsSearch.ts`, spec 06 §1.3). Each entry names
//! a row id (the scroll target), its title and its page.

use crate::app_state::SettingsPage as P;

pub struct SearchItem {
    pub id: &'static str,
    pub title: &'static str,
    pub page: P,
    /// Extra words that match but are not shown.
    pub keywords: &'static str,
}

const fn item(id: &'static str, title: &'static str, page: P) -> SearchItem {
    SearchItem {
        id,
        title,
        page,
        keywords: "",
    }
}

const fn kw(id: &'static str, title: &'static str, page: P, keywords: &'static str) -> SearchItem {
    SearchItem {
        id,
        title,
        page,
        keywords,
    }
}

pub const ITEMS: &[SearchItem] = &[
    // General
    kw(
        "thread-notifications",
        "Thread notifications",
        P::General,
        "sound alerts",
    ),
    item("in-app-notifications", "In-app notifications", P::General),
    item("project-grouping", "Project grouping", P::General),
    item("project-order", "Project order", P::General),
    item("working-section", "Working section", P::General),
    item("unpin-confirmation", "Unpin confirmation", P::General),
    item("archive-confirmation", "Archive confirmation", P::General),
    item("delete-confirmation", "Delete confirmation", P::General),
    item(
        "terminal-close-confirmation",
        "Terminal close confirmation",
        P::General,
    ),
    kw("quit-shortcut", "Quit shortcut", P::General, "ctrl q"),
    kw(
        "time-format",
        "Time format",
        P::General,
        "clock 24-hour 12-hour",
    ),
    // Appearance
    kw(
        "color-scheme",
        "Color scheme",
        P::Appearance,
        "theme dark light system",
    ),
    item("themes", "Themes", P::Appearance),
    item("contrast", "Contrast", P::Appearance),
    item("glass-opacity", "Glass opacity", P::Appearance),
    item("chat-width", "Chat width", P::Appearance),
    item("thread-details", "Thread details", P::Appearance),
    item("effort-display", "Effort display", P::Appearance),
    kw("render-math", "Render math", P::Appearance, "latex"),
    item("panel-animations", "Panel animations", P::Appearance),
    item("composer-context", "Composer context", P::Appearance),
    kw(
        "interface-font",
        "Interface font",
        P::Appearance,
        "typography size",
    ),
    item("prompt-font", "Prompt font", P::Appearance),
    kw("code-font", "Code font", P::Appearance, "monospace"),
    item("terminal-font", "Terminal font", P::Appearance),
    item("monospace-font", "Monospace font", P::Appearance),
    item("word-wrap", "Word wrap", P::Appearance),
    item("separate-fonts", "Separate fonts", P::Appearance),
    // Chat
    kw(
        "rich-text-composer",
        "Rich text composer",
        P::Chat,
        "markdown",
    ),
    kw("send-shortcut", "Send shortcut", P::Chat, "enter"),
    item("show-skills", "Show skills in slash menu", P::Chat),
    kw(
        "follow-up-behavior",
        "Follow-up behavior",
        P::Chat,
        "queue steer",
    ),
    item(
        "side-thread-target",
        "Open subagents and side chats in",
        P::Chat,
    ),
    item("tool-calls", "Tool calls in chat", P::Chat),
    item("screen-reader", "Screen reader announcements", P::Chat),
    item("screen-reader-title", "Screen reader window title", P::Chat),
    // Diffs
    item("diff-layout", "Diff layout", P::Diffs),
    item("diff-whitespace", "Hide whitespace changes", P::Diffs),
    item("diff-file-state", "Default diff file state", P::Diffs),
    item("diff-colors", "Diff colors", P::Diffs),
    item(
        "proactive-panels",
        "Open pull requests and changes panels proactively",
        P::Diffs,
    ),
    item("pr-links", "Open pull requests in Tern", P::Diffs),
    // Keybindings
    kw(
        "keybindings",
        "Keybindings",
        P::Keybindings,
        "shortcuts keys",
    ),
    // Providers
    kw("providers", "Providers", P::Providers, "codex claude"),
    item(
        "health-check-interval",
        "Health check interval",
        P::Providers,
    ),
    item("compact-idle-threads", "Compact idle threads", P::Providers),
    // Threads
    item("add-project-base", "Add project starts in", P::Threads),
    item("ghq-layout", "Clone into host folders", P::Threads),
    kw("default-model", "Model", P::Threads, "default model"),
    item("fallback-model", "Fallback model", P::Threads),
    item("permissions", "Permissions", P::Threads),
    item("workspace", "Workspace", P::Threads),
    item("snooze-options", "Snooze options", P::Threads),
    item("auto-resume", "Resume after usage limits", P::Threads),
    item("snooze-limited", "Snooze until limits reset", P::Threads),
    item(
        "auto-settle-merged",
        "Auto-settle merged threads",
        P::Threads,
    ),
    item(
        "auto-settle-inactive",
        "Auto-settle inactive threads",
        P::Threads,
    ),
    item("text-generation-model", "Text generation model", P::Threads),
    item(
        "thread-title-instructions",
        "Thread title instructions",
        P::Threads,
    ),
    item("response-streaming", "Response streaming", P::Threads),
    // Projects
    item("projects", "Projects", P::Projects),
    // Source control
    item("auto-pull", "Automatically pull", P::SourceControl),
    item(
        "remove-credits",
        "Remove agent credits when merging",
        P::SourceControl,
    ),
    item("merge-method", "Default merge method", P::SourceControl),
    item("git", "Git", P::SourceControl),
    kw("github", "GitHub", P::SourceControl, "gh token"),
    item(
        "writing-style",
        "Commit and pull request writing",
        P::SourceControl,
    ),
    item("review-guides", "Review guides", P::SourceControl),
    item(
        "pr-templates",
        "Follow pull request templates",
        P::SourceControl,
    ),
    item(
        "writer-model",
        "Source control writer model",
        P::SourceControl,
    ),
    // Worktrees
    item("worktree-location", "Worktree location", P::Worktrees),
    item("start-from-remote", "Start from remote", P::Worktrees),
    item("submodules", "Submodules", P::Worktrees),
    item("branch-naming", "Worktree branch naming", P::Worktrees),
    item("branch-prefix", "Branch prefix", P::Worktrees),
    item(
        "worktree-on-delete",
        "Delete worktrees with deleted threads",
        P::Worktrees,
    ),
    item("worktree-on-merge", "Delete merged worktrees", P::Worktrees),
    item(
        "worktree-after-days",
        "Delete inactive worktrees",
        P::Worktrees,
    ),
    item("worktree-settled", "Delete settled worktrees", P::Worktrees),
    item(
        "worktree-unchanged",
        "Delete unchanged worktrees",
        P::Worktrees,
    ),
    item(
        "worktree-keep-when",
        "Keep worktrees with local changes",
        P::Worktrees,
    ),
    // Maintenance
    item(
        "provider-update-checks",
        "Provider update checks",
        P::Maintenance,
    ),
    item(
        "continue-threads",
        "Continue threads after restarts",
        P::Maintenance,
    ),
    item("background-activity", "Background activity", P::Maintenance),
    kw(
        "keep-awake",
        "Keep awake while agents work",
        P::Maintenance,
        "sleep",
    ),
    item("logs-after-days", "Delete old rotated logs", P::Maintenance),
    // About
    item("version", "Version", P::About),
    kw("licenses", "Open source licenses", P::About, "notices"),
    // Archive
    item("archived-threads", "Archived threads", P::Archive),
];

/// Items whose title, page label or keywords contain every word of `query`.
pub fn matches(query: &str) -> Vec<&'static SearchItem> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    if words.is_empty() {
        return Vec::new();
    }
    ITEMS
        .iter()
        .filter(|item| {
            let hay =
                format!("{} {} {}", item.title, item.page.title(), item.keywords).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_by_title_and_keyword() {
        let hits = matches("font");
        assert!(hits.iter().any(|i| i.id == "code-font"));
        assert!(matches("latex").iter().any(|i| i.id == "render-math"));
        assert!(matches("").is_empty());
        assert!(matches("zzzz").is_empty());
    }

    #[test]
    fn every_word_must_match() {
        let hits = matches("diff colors");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "diff-colors");
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = ITEMS.iter().map(|i| i.id).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(n, ids.len());
    }
}

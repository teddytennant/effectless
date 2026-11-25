//! Pure derivations: the sidebar row of a thread (spec/07 §1.4, T3 `ProjectionStore.ts:1880-2060`)
//! and title seeding/sanitizing (spec/07 §9.8-9.9).

use tern_core::*;

const LATEST_MESSAGE_CHARS: usize = 240;

/// The run a thread is working on: the newest live run.
pub fn active_run(runs: &[Run]) -> Option<&Run> {
    runs.iter()
        .filter(|r| r.status.is_live())
        .max_by_key(|r| r.ordinal)
}

/// The newest run that counts for the sidebar: rolled-back runs and queued runs cancelled
/// before they started are skipped.
pub fn latest_run(runs: &[Run]) -> Option<&Run> {
    runs.iter()
        .filter(|r| r.status != RunStatus::RolledBack)
        .filter(|r| !(r.status == RunStatus::Cancelled && r.started_at.is_none()))
        .max_by_key(|r| r.ordinal)
}

fn run_by_id<'a>(runs: &'a [Run], id: &str) -> Option<&'a Run> {
    runs.iter().find(|r| r.id == id)
}

/// Is this item part of the visible conversation (not a rolled-back run, not a queued
/// message that hasn't started)?
pub fn item_visible(runs: &[Run], item: &TurnItem) -> bool {
    match item.run_id.as_deref().and_then(|id| run_by_id(runs, id)) {
        Some(r) => match r.status {
            RunStatus::RolledBack | RunStatus::Queued => false,
            RunStatus::Cancelled => r.started_at.is_some(),
            _ => true,
        },
        None => true,
    }
}

/// Derive the sidebar row.
pub fn derive_shell(d: &ThreadDetail) -> ThreadShell {
    let active = active_run(&d.runs);
    let latest = latest_run(&d.runs);
    let shown = active.or(latest);

    let status = match shown {
        Some(r) => ShellThreadStatus::Run(r.status),
        None => ShellThreadStatus::Idle,
    };

    // approvals before questions, then newest first (ProjectionStore.ts:1900-1907)
    let pending_request = d
        .requests
        .iter()
        .filter(|r| r.status == RequestStatus::Pending)
        .min_by(|a, b| {
            let qa = a.kind == RequestKind::UserInput;
            let qb = b.kind == RequestKind::UserInput;
            qa.cmp(&qb).then(b.created_at.cmp(&a.created_at))
        })
        .map(|r| (r.id.clone(), r.kind));

    let latest_message = d
        .items
        .iter()
        .rev()
        .filter(|i| item_visible(&d.runs, i))
        .find_map(|i| match &i.body {
            TurnItemBody::UserMessage { text, .. } => Some(("user", text)),
            TurnItemBody::AssistantMessage { text, .. } if !text.trim().is_empty() => {
                Some(("assistant", text))
            }
            _ => None,
        })
        .map(|(role, text)| (role.to_string(), preview(text)));

    let failure = shown
        .filter(|r| r.status == RunStatus::Failed)
        .and_then(|r| {
            d.items
                .iter()
                .rev()
                .filter(|i| i.run_id.as_deref() == Some(&r.id) && i.status == ItemStatus::Failed)
                .find_map(|i| match &i.body {
                    TurnItemBody::Error { failure, .. } => Some(failure.clone()),
                    _ => None,
                })
                .or_else(|| r.failure.clone())
        });
    let session_error = d.session.as_ref().and_then(|s| s.last_error.clone());
    // threadErrorSummary: a session error that differs from the failure hides its class
    let current = match (&session_error, &failure) {
        (Some(se), Some(f)) if *se != f.message => None,
        _ => failure.clone(),
    };
    let last_error = session_error.or_else(|| failure.map(|f| f.message));
    let last_error_class = current.as_ref().map(|f| f.class);
    let usage_limit_reset_at = current
        .as_ref()
        .filter(|f| f.class == FailureClass::UsageLimit)
        .and_then(|f| f.reset_at.clone());

    let diff_totals = d
        .checkpoints
        .iter()
        .filter(|c| c.status == CheckpointStatus::Ready && c.run_id.is_some())
        .filter(|c| {
            c.run_id
                .as_deref()
                .and_then(|id| run_by_id(&d.runs, id))
                .is_none_or(|r| r.status != RunStatus::RolledBack)
        })
        .max_by_key(|c| c.ordinal)
        .map(|c| totals(&c.files))
        .filter(|t| t.count > 0);

    let mut last_activity_at = d.thread.updated_at;
    if let Some(r) = shown {
        for t in [Some(r.requested_at), r.started_at, r.completed_at]
            .into_iter()
            .flatten()
        {
            last_activity_at = last_activity_at.max(t);
        }
    }

    ThreadShell {
        thread: d.thread.clone(),
        status,
        active_run_id: active
            .filter(|r| r.status != RunStatus::Waiting)
            .map(|r| r.id.clone()),
        latest_run_started_at: shown.and_then(|r| r.started_at),
        latest_run_completed_at: shown.and_then(|r| r.completed_at),
        pending_request,
        latest_message,
        has_actionable_proposed_plan: d.plans.iter().any(|p| p.status == PlanStatus::Active),
        last_error,
        last_error_class,
        usage_limit_reset_at,
        diff_totals,
        last_activity_at,
    }
}

pub fn totals(files: &[CheckpointFileSummary]) -> FileTotals {
    FileTotals {
        count: files.len() as u32,
        additions: files.iter().map(|f| f.additions).sum(),
        deletions: files.iter().map(|f| f.deletions).sum(),
    }
}

fn preview(text: &str) -> String {
    let collapsed = collapse_ws(text);
    if collapsed.chars().count() <= LATEST_MESSAGE_CHARS {
        collapsed
    } else {
        let cut: String = collapsed.chars().take(LATEST_MESSAGE_CHARS - 1).collect();
        format!("{}…", cut.trim_end())
    }
}

fn collapse_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `deriveThreadTitleSeed` + `truncate` (spec/07 §9.9): first 50 code points + `...`.
pub fn seed_title(text: &str, attachments: &[ChatAttachment]) -> String {
    let collapsed = collapse_ws(text);
    if !collapsed.is_empty() {
        let n = collapsed.chars().count();
        if n <= 50 {
            return collapsed;
        }
        let head: String = collapsed.chars().take(50).collect();
        return format!("{head}...");
    }
    let images = attachments
        .iter()
        .filter(|a| matches!(a, ChatAttachment::Image { .. }))
        .count();
    match images {
        1 => "Attached image".into(),
        n if n > 1 => "Attached images".into(),
        _ => match attachments.first() {
            Some(a) => format!("File: {}", a.name()),
            None => NEW_THREAD_TITLE.into(),
        },
    }
}

/// `sanitizeThreadTitle` (spec/07 §9.8). Returns None for the literal placeholder "title".
pub fn sanitize_title(raw: &str) -> Option<String> {
    let mut text = raw.trim().to_string();
    if text.starts_with('{')
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(t) = v.get("title").and_then(|t| t.as_str())
    {
        text = t.to_string();
    }
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let mut line = first.to_string();
    for q in ['"', '\'', '`'] {
        if line.len() >= 2 && line.starts_with(q) && line.ends_with(q) {
            line = line[1..line.len() - 1].to_string();
            break;
        }
    }
    let line = collapse_ws(&line);
    if line.eq_ignore_ascii_case("title") {
        return None;
    }
    if line.is_empty() {
        return Some(NEW_THREAD_TITLE.into());
    }
    if line.chars().count() > 120 {
        let cut: String = line.chars().take(117).collect();
        return Some(format!("{cut}..."));
    }
    Some(line)
}

/// Text sent to the title generator: the first user message, head and tail kept around a
/// truncation marker when long (`limitTitleMessage`).
pub fn title_context(d: &ThreadDetail) -> Option<String> {
    let text = d.items.iter().find_map(|i| match &i.body {
        TurnItemBody::UserMessage {
            text, attachments, ..
        } => {
            let mut t = text.clone();
            if !attachments.is_empty() {
                let names: Vec<_> = attachments.iter().map(|a| a.name()).collect();
                t.push_str(&format!("\n[Attachments: {}]", names.join(", ")));
            }
            Some(t)
        }
        _ => None,
    })?;
    const BUDGET: usize = 2_000;
    let n = text.chars().count();
    if n <= BUDGET {
        return Some(text);
    }
    let half = BUDGET / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = text.chars().skip(n - half).collect();
    Some(format!("{head}\n[Content truncated]\n{tail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_titles() {
        assert_eq!(seed_title("  fix   the\nbuild ", &[]), "fix the build");
        let long = "a".repeat(60);
        assert_eq!(seed_title(&long, &[]), format!("{}...", "a".repeat(50)));
        let img = ChatAttachment::Image {
            id: "1".into(),
            name: "shot.png".into(),
            mime_type: "image/png".into(),
            size_bytes: 1,
        };
        let file = ChatAttachment::File {
            id: "2".into(),
            name: "log.txt".into(),
            mime_type: "text/plain".into(),
            size_bytes: 1,
        };
        assert_eq!(seed_title("", std::slice::from_ref(&img)), "Attached image");
        assert_eq!(seed_title("", &[img.clone(), img]), "Attached images");
        assert_eq!(seed_title("", &[file]), "File: log.txt");
        assert_eq!(seed_title("", &[]), NEW_THREAD_TITLE);
    }

    #[test]
    fn sanitize_titles() {
        assert_eq!(
            sanitize_title("\"Fix login\"\nmore").as_deref(),
            Some("Fix login")
        );
        assert_eq!(
            sanitize_title(r#"{"title": "Rate limiter choice", "needsRefinement": false}"#)
                .as_deref(),
            Some("Rate limiter choice")
        );
        assert_eq!(sanitize_title("   ").as_deref(), Some(NEW_THREAD_TITLE));
        assert_eq!(sanitize_title("Title"), None);
        let long = "x".repeat(130);
        assert_eq!(sanitize_title(&long).unwrap().chars().count(), 120);
    }
}

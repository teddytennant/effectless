//! Timeline entries → display rows: a port of `deriveMessagesTimelineRows`
//! (`MessagesTimeline.logic.ts:1619-2448`) and its helpers (turn folds, the live
//! activity tail, the working header, the thinking placeholder, assistant meta
//! placement and the grouping post-pass). Attempts, html renders, MCP apps,
//! worktree setup and `/compact` turns do not exist in Tern and are left out.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use tern_core::{
    FailureClass, ItemStatus, RunStatus, Ts, TurnItem, TurnItemBody,
};

use super::entries::{
    Entry, EntryKind, ItemType, Lifecycle, MessageEntry, PlanEntry, Role, SourceKind,
    SummaryKind, Tone, ToolCallVisibility, WorkEntry, summarize_group, summary_kind,
};
use super::format;

pub const LIVE_ACTIVITY_ROW_ID: &str = "live-activity-row";
pub const WORKING_ROW_ID: &str = "working-indicator-row";

/// `TimelineLatestRun`.
#[derive(Clone, Debug)]
pub struct LatestRun {
    pub run_id: String,
    pub status: RunStatus,
    pub started_at: Option<Ts>,
    pub completed_at: Option<Ts>,
}

#[derive(Clone, Debug)]
pub enum RowKind {
    Message {
        message: MessageEntry,
        show_meta: bool,
        /// The reply is still streaming or its run is still answering (hides copy).
        copy_streaming: bool,
        grouped_with_next: bool,
        grouped_with_previous: bool,
        continues_steer: bool,
    },
    AssistantMeta {
        message: MessageEntry,
    },
    Work {
        entries: Vec<WorkEntry>,
        expanded_group: bool,
        display_label: Option<String>,
    },
    WorkLive {
        entry: WorkEntry,
        entries: Vec<WorkEntry>,
        group_id: String,
        expanded: bool,
        active: bool,
        thought: Option<WorkEntry>,
    },
    Working {
        compacting: bool,
    },
    Thinking {
        label: Option<String>,
        group_id: Option<String>,
        expanded: bool,
    },
    WorkToggle {
        run_id: Option<String>,
        group_id: String,
        count: usize,
        expanded: bool,
        summary: String,
        kind: SummaryKind,
        has_failure: bool,
    },
    TurnFold {
        run_id: String,
        label: String,
        expanded: bool,
    },
    Compaction {
        label: String,
        tokens: Option<(u32, u32)>,
        active: bool,
        failed: bool,
        failure: Option<String>,
    },
    Plan(PlanEntry),
    Event {
        item: Rc<TurnItem>,
        subagents: Vec<Rc<TurnItem>>,
    },
}

#[derive(Clone, Debug)]
pub struct Row {
    pub id: String,
    pub created_at: Option<Ts>,
    pub kind: RowKind,
    pub continues_work_log: bool,
}

impl Row {
    /// Work log rows share the tighter spacing (`timelineRowIsWorkLog`).
    pub fn is_work_log(&self) -> bool {
        matches!(
            self.kind,
            RowKind::Work { .. }
                | RowKind::WorkLive { .. }
                | RowKind::WorkToggle { .. }
                | RowKind::Thinking { .. }
        ) || matches!(&self.kind, RowKind::Event { item, .. } if matches!(item.body, TurnItemBody::Subagent { .. }))
    }

    /// A cheap content hash: rows whose fingerprint is unchanged keep their measured
    /// height in the list.
    pub fn fingerprint(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.id.hash(&mut h);
        self.continues_work_log.hash(&mut h);
        fn item(h: &mut impl Hasher, i: &TurnItem) {
            i.id.hash(h);
            i.updated_at.hash(h);
            (i.status as u8).hash(h);
        }
        fn work(h: &mut impl Hasher, w: &WorkEntry) {
            item(h, &w.item);
            w.label.hash(h);
            w.detail.as_ref().map(String::len).hash(h);
        }
        match &self.kind {
            RowKind::Message {
                message,
                show_meta,
                copy_streaming,
                grouped_with_next,
                grouped_with_previous,
                continues_steer,
            } => {
                0u8.hash(&mut h);
                item(&mut h, &message.item);
                message.text.len().hash(&mut h);
                message.streaming.hash(&mut h);
                (show_meta, copy_streaming, grouped_with_next).hash(&mut h);
                (grouped_with_previous, continues_steer).hash(&mut h);
            }
            RowKind::AssistantMeta { message } => {
                1u8.hash(&mut h);
                item(&mut h, &message.item);
            }
            RowKind::Work {
                entries,
                expanded_group,
                display_label,
            } => {
                2u8.hash(&mut h);
                entries.iter().for_each(|e| work(&mut h, e));
                (expanded_group, display_label).hash(&mut h);
            }
            RowKind::WorkLive {
                entry,
                entries,
                expanded,
                active,
                thought,
                ..
            } => {
                3u8.hash(&mut h);
                work(&mut h, entry);
                entries.iter().for_each(|e| work(&mut h, e));
                (expanded, active).hash(&mut h);
                thought.as_ref().map(|t| work(&mut h, t));
            }
            RowKind::Working { compacting } => (4u8, compacting, self.created_at).hash(&mut h),
            RowKind::Thinking {
                label, expanded, ..
            } => (5u8, label, expanded).hash(&mut h),
            RowKind::WorkToggle {
                count,
                expanded,
                summary,
                has_failure,
                ..
            } => (6u8, count, expanded, summary, has_failure).hash(&mut h),
            RowKind::TurnFold {
                label, expanded, ..
            } => (7u8, label, expanded).hash(&mut h),
            RowKind::Compaction {
                label,
                tokens,
                active,
                failed,
                failure,
            } => (8u8, label, tokens, active, failed, failure).hash(&mut h),
            RowKind::Plan(p) => (9u8, p.markdown.len(), p.streaming).hash(&mut h),
            RowKind::Event { item: i, subagents } => {
                10u8.hash(&mut h);
                item(&mut h, i);
                subagents.iter().for_each(|s| item(&mut h, s));
            }
        }
        h.finish()
    }
}

pub struct RowsInput<'a> {
    pub entries: &'a [Entry],
    /// Request ids of approvals still waiting on the user.
    pub pending_approvals: &'a HashSet<String>,
    pub latest_run: Option<&'a LatestRun>,
    pub running_run_id: Option<&'a str>,
    pub expanded_runs: &'a HashSet<String>,
    pub expanded_groups: &'a HashSet<String>,
    pub is_working: bool,
    pub active_turn_started_at: Option<Ts>,
    pub visibility: ToolCallVisibility,
}

fn work_group_id(entry_id: &str) -> String {
    format!("work-group:{entry_id}")
}

fn expanded_group_row(group_id: &str, created_at: Ts, entries: Vec<WorkEntry>) -> Row {
    Row {
        id: format!("{group_id}:details"),
        created_at: Some(created_at),
        kind: RowKind::Work {
            entries,
            expanded_group: true,
            display_label: None,
        },
        continues_work_log: false,
    }
}

/// `timelineEntryRunId`.
fn entry_run_id(e: &Entry) -> Option<&str> {
    match &e.kind {
        EntryKind::Message(m) if m.role == Role::Assistant => m.run_id.as_deref(),
        EntryKind::Message(_) => None,
        EntryKind::Plan(p) => p.run_id.as_deref(),
        EntryKind::Work(w) => w.run_id.as_deref(),
        EntryKind::Event(_) => None,
    }
}

/// `timelineEntryStartsResponse`.
fn starts_response(e: &Entry) -> bool {
    matches!(&e.kind, EntryKind::Message(m) if m.role == Role::User && !m.is_steer())
}

fn is_steer(e: &Entry) -> bool {
    matches!(&e.kind, EntryKind::Message(m) if m.is_steer())
}

fn is_compaction(e: &Entry) -> bool {
    e.work()
        .is_some_and(|w| w.source_kind == Some(SourceKind::ContextCompaction))
}

fn entry_end(e: &Entry) -> Ts {
    match &e.kind {
        EntryKind::Message(m) => m.updated_at,
        _ => e.created_at,
    }
}

/// `responseBoundaryRunId`.
fn boundary_run_id(e: &Entry) -> Option<&str> {
    match &e.kind {
        EntryKind::Message(m) => m.run_id.as_deref(),
        EntryKind::Work(w) => w.run_id.as_deref(),
        _ => None,
    }
}

fn last_response_boundary(entries: &[Entry]) -> Option<usize> {
    entries.iter().rposition(starts_response)
}

/// `deriveUnsettledRunId`.
fn unsettled_run_id<'a>(latest: Option<&'a LatestRun>, running: Option<&'a str>) -> Option<&'a str> {
    if running.is_some() {
        return running;
    }
    let latest = latest?;
    let settled = latest.completed_at.is_some()
        && !matches!(
            latest.status,
            RunStatus::Running | RunStatus::Starting | RunStatus::Waiting
        );
    (!settled).then_some(latest.run_id.as_str())
}

/// `deriveTerminalAssistantMessageIds`.
fn terminal_assistant_ids(entries: &[Entry]) -> HashSet<String> {
    let mut by_key: HashMap<String, String> = HashMap::new();
    let mut null_turn = 0usize;
    for e in entries {
        let Some(m) = e.message() else { continue };
        if m.role == Role::User {
            null_turn += 1;
            continue;
        }
        let key = match &m.run_id {
            Some(r) => format!("turn:{r}"),
            None => format!("unkeyed:{null_turn}"),
        };
        by_key.insert(key, m.id.clone());
    }
    by_key.into_values().collect()
}

/// `failedTimelineRunIds`.
fn failed_item(e: &Entry) -> Option<&TurnItem> {
    let item = match &e.kind {
        EntryKind::Event(i) => i.as_ref(),
        EntryKind::Work(w) => w.item.as_ref(),
        _ => return None,
    };
    (matches!(item.body, TurnItemBody::Error { .. })
        && item.status == ItemStatus::Failed
        && item.parent_item_id.is_none())
    .then_some(item)
}

fn failed_run_ids(entries: &[Entry], latest: Option<&LatestRun>) -> HashSet<String> {
    let mut failed = HashSet::new();
    if let Some(l) = latest
        && l.status == RunStatus::Failed
    {
        failed.insert(l.run_id.clone());
    }
    for e in entries {
        if let Some(r) = failed_item(e).and_then(|i| i.run_id.clone()) {
            failed.insert(r);
        }
    }
    failed
}

/// `itemizedFailedRunIds`: usage-limit pauses keep their groups.
fn itemized_run_ids(entries: &[Entry], failed: &HashSet<String>) -> HashSet<String> {
    let mut usage = HashSet::new();
    let mut other = HashSet::new();
    for e in entries {
        if let Some(item) = failed_item(e)
            && let Some(run) = &item.run_id
        {
            let usage_limit = matches!(&item.body, TurnItemBody::Error { failure, .. } if failure.class == FailureClass::UsageLimit);
            if usage_limit {
                usage.insert(run.clone());
            } else {
                other.insert(run.clone());
            }
        }
    }
    failed
        .iter()
        .filter(|r| !usage.contains(*r) || other.contains(*r))
        .cloned()
        .collect()
}

/// `deriveActiveVisualResponseRunIds`.
fn active_visual_runs(entries: &[Entry], unsettled: Option<&str>, is_working: bool) -> HashSet<String> {
    let mut runs = HashSet::new();
    let Some(u) = unsettled else { return runs };
    runs.insert(u.to_string());
    if !is_working {
        return runs;
    }
    let from = last_response_boundary(entries).map_or(0, |i| i + 1);
    for e in &entries[from..] {
        if let Some(r) = entry_run_id(e) {
            runs.insert(r.to_string());
        }
    }
    runs
}

/// `liveSubagentCardEntryIds`: subagent cards whose batch still has a live child.
fn live_subagent_ids(entries: &[Entry]) -> HashSet<String> {
    let mut visible = HashSet::new();
    let mut batch: Vec<&Entry> = Vec::new();
    let flush = |batch: &mut Vec<&Entry>, visible: &mut HashSet<String>| {
        if batch
            .iter()
            .any(|e| e.event().is_some_and(|i| i.status.is_open()))
        {
            visible.extend(batch.iter().map(|e| e.id.clone()));
        }
        batch.clear();
    };
    for e in entries {
        let Some(item) = e.event().filter(|i| matches!(i.body, TurnItemBody::Subagent { .. }))
        else {
            flush(&mut batch, &mut visible);
            continue;
        };
        if let Some(prev) = batch.last().and_then(|p| p.event())
            && prev.run_id != item.run_id
        {
            flush(&mut batch, &mut visible);
        }
        batch.push(e);
    }
    flush(&mut batch, &mut visible);
    visible
}

/// `retainedAssistantReplyEntryIds` without attempts: explicit final answers.
fn retained_reply_ids(entries: &[Entry]) -> HashSet<String> {
    entries
        .iter()
        .filter(|e| {
            e.message()
                .is_some_and(|m| m.phase == Some(tern_core::AssistantPhase::FinalAnswer))
        })
        .map(|e| e.id.clone())
        .collect()
}

/// `timelineEntryFoldRunId`.
fn fold_run_id(e: &Entry, runless: Option<&str>) -> Option<String> {
    match &e.kind {
        EntryKind::Work(w) if w.item_type == ItemType::SystemNotice => None,
        EntryKind::Message(m) if m.role == Role::Assistant => {
            m.run_id.clone().or(runless.map(str::to_string))
        }
        EntryKind::Work(w) => w.run_id.clone().or(runless.map(str::to_string)),
        EntryKind::Event(i) if matches!(i.body, TurnItemBody::Subagent { .. }) => {
            i.run_id.clone().or(runless.map(str::to_string))
        }
        _ => None,
    }
}

struct TurnFold {
    run_id: String,
    anchor: String,
    created_at: Ts,
    hidden: HashSet<String>,
    label: String,
}

struct FoldInput<'a> {
    entries: &'a [Entry],
    terminal: &'a HashSet<String>,
    latest: Option<&'a LatestRun>,
    unfolded: &'a HashSet<String>,
    live_subagents: &'a HashSet<String>,
    retained: &'a HashSet<String>,
}

fn max_ts(a: Option<Ts>, b: Ts) -> Ts {
    a.map_or(b, |a| a.max(b))
}

/// `deriveTurnFolds`.
fn derive_turn_folds(input: &FoldInput) -> HashMap<String, TurnFold> {
    let entries = input.entries;
    let mut interrupted: HashSet<String> = HashSet::new();
    let mut prompt_index: HashMap<String, usize> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        if starts_response(e)
            && let Some(r) = boundary_run_id(e)
        {
            prompt_index.entry(r.to_string()).or_insert(i);
        }
        if let Some(item) = e.event()
            && matches!(
                item.body,
                TurnItemBody::RunInterruptRequest { .. } | TurnItemBody::RunInterruptResult { .. }
            )
            && let Some(r) = &item.run_id
        {
            interrupted.insert(r.clone());
        }
    }

    struct Group<'e> {
        entries: Vec<&'e Entry>,
        terminal: Option<&'e Entry>,
        streaming: bool,
        start: Option<Ts>,
        anchor: String,
        steers: Vec<(usize, &'e Entry)>,
    }
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut runless_failed: HashSet<String> = HashSet::new();
    let mut runless_key: Option<String> = None;
    let mut response_key: Option<String> = None;
    let mut pending: Option<(Ts, String)> = None;

    for (i, e) in entries.iter().enumerate() {
        if starts_response(e) {
            pending = entries.get(i + 1).map(|n| (e.created_at, n.id.clone()));
            let boundary = boundary_run_id(e);
            runless_key = boundary.is_none().then(|| format!("runless:{}", e.id));
            response_key = boundary.map(str::to_string).or(runless_key.clone());
            continue;
        }
        if is_steer(e) {
            let steer_run = e
                .message()
                .and_then(|m| m.run_id.clone())
                .or(response_key.clone());
            if let Some(g) = steer_run.and_then(|r| groups.get_mut(&r))
                && !g.entries.is_empty()
            {
                if g.steers.last().map(|s| s.0) == Some(g.entries.len()) {
                    g.steers.pop();
                }
                g.steers.push((g.entries.len(), e));
            }
            continue;
        }
        let Some(own) = fold_run_id(e, runless_key.as_deref()) else {
            continue;
        };
        let run = match &response_key {
            Some(rk) if prompt_index.get(&own).copied().unwrap_or(0) > i
                && prompt_index.contains_key(&own) =>
            {
                rk.clone()
            }
            _ => own,
        };
        if Some(&run) == runless_key.as_ref() && failed_item(e).is_some() {
            runless_failed.insert(run.clone());
        }
        let g = groups.entry(run.clone()).or_insert_with(|| {
            order.push(run.clone());
            let (start, anchor) = match pending.take() {
                Some((t, a)) => (Some(t), a),
                None => (None, e.id.clone()),
            };
            Group {
                entries: Vec::new(),
                terminal: None,
                streaming: false,
                start,
                anchor,
                steers: Vec::new(),
            }
        });
        g.entries.push(e);
        if let Some(m) = e.message() {
            if input.terminal.contains(&m.id) {
                g.terminal = Some(e);
            }
            if m.streaming {
                g.streaming = true;
            }
        }
    }

    let mut folds = HashMap::new();
    for run_id in order {
        let g = &groups[&run_id];
        if input.unfolded.contains(&run_id)
            || interrupted.contains(&run_id)
            || runless_failed.contains(&run_id)
            || g.streaming
        {
            continue;
        }
        let terminal_index = g
            .terminal
            .and_then(|t| g.entries.iter().position(|e| e.id == t.id))
            .unwrap_or(g.entries.len());
        let visible_compaction = g.entries.iter().rev().find(|e| is_compaction(e)).map(|e| &e.id);
        let mut hidden = HashSet::new();
        for (i, e) in g.entries.iter().enumerate() {
            if Some(e.id.as_str()) == g.terminal.map(|t| t.id.as_str()) {
                continue;
            }
            let compaction = is_compaction(e);
            let foldable_trailing = e
                .work()
                .is_some_and(|w| w.status != Lifecycle::InProgress && !w.display_failure());
            if !compaction && i > terminal_index && !foldable_trailing {
                continue;
            }
            if Some(&e.id) == visible_compaction
                || input.live_subagents.contains(&e.id)
                || input.retained.contains(&e.id)
            {
                continue;
            }
            if e.work().is_some_and(|w| w.question_answer) {
                continue;
            }
            hidden.insert(e.id.clone());
        }
        let hides_work = |list: &[&Entry]| {
            list.iter()
                .any(|e| hidden.contains(&e.id) && !is_compaction(e))
        };
        if hidden.is_empty() || !hides_work(&g.entries) {
            continue;
        }
        let first = g.entries[0];
        let last = *g.entries.last().unwrap();
        let elapsed_ms = match input.latest {
            Some(l) if l.run_id == run_id && l.started_at.is_some() && l.completed_at.is_some() => {
                Some((l.completed_at.unwrap() - l.started_at.unwrap()).num_milliseconds())
            }
            _ => {
                let end = max_ts(g.terminal.and_then(|t| t.message().map(|m| m.updated_at)), entry_end(last));
                Some((end - g.start.unwrap_or(first.created_at)).num_milliseconds())
            }
        };
        let duration = elapsed_ms.map(|ms| format::duration(ms.max(0)));
        let latest_interrupted = input
            .latest
            .is_some_and(|l| l.run_id == run_id && l.status == RunStatus::Interrupted);
        let label = match (latest_interrupted, &duration) {
            (true, Some(d)) => format!("You stopped after {d}"),
            (true, None) => "You stopped this response".into(),
            (false, Some(d)) => format!("Worked for {d}"),
            (false, None) => "Worked".into(),
        };
        let first_end = g.steers.first().map_or(g.entries.len(), |s| s.0);
        let segment_hidden = |list: &[&Entry]| -> HashSet<String> {
            list.iter()
                .filter(|e| hidden.contains(&e.id))
                .map(|e| e.id.clone())
                .collect()
        };
        folds.insert(
            g.anchor.clone(),
            TurnFold {
                run_id: run_id.clone(),
                anchor: g.anchor.clone(),
                created_at: g.start.unwrap_or(first.created_at),
                hidden: segment_hidden(&g.entries[..first_end]),
                label,
            },
        );
        for (si, (start, steer)) in g.steers.iter().enumerate() {
            let end = g.steers.get(si + 1).map_or(g.entries.len(), |s| s.0);
            let seg = &g.entries[*start..end];
            let Some(anchor) = seg.first() else { continue };
            if !hides_work(seg) {
                continue;
            }
            let seg_end = seg.iter().map(|e| entry_end(e)).max().unwrap_or(steer.created_at);
            let ms = (seg_end - steer.created_at).num_milliseconds();
            folds.insert(
                anchor.id.clone(),
                TurnFold {
                    run_id: format!("{run_id}:steer:{}", steer.id),
                    anchor: anchor.id.clone(),
                    created_at: steer.created_at,
                    hidden: segment_hidden(seg),
                    label: format!("Continued for {}", format::duration(ms.max(0))),
                },
            );
        }
    }
    folds
}

/// `attachTrailingToolGroupsToAssistant`.
fn attach_meta(rows: Vec<Row>) -> Vec<Row> {
    let mut without_meta: HashSet<usize> = HashSet::new();
    let mut meta_after: HashMap<usize, Row> = HashMap::new();
    for (mi, row) in rows.iter().enumerate() {
        let RowKind::Message {
            message, show_meta, ..
        } = &row.kind
        else {
            continue;
        };
        let Some(run) = message.run_id.as_deref().filter(|_| *show_meta && message.role == Role::Assistant)
        else {
            continue;
        };
        let mut last = None;
        let mut has_group = false;
        for (i, c) in rows.iter().enumerate().skip(mi + 1) {
            match &c.kind {
                RowKind::Message { .. } => break,
                RowKind::WorkToggle { run_id, .. } if run_id.as_deref() == Some(run) => {
                    has_group = true;
                    last = Some(i);
                }
                RowKind::Work {
                    entries,
                    expanded_group,
                    ..
                } if entries.iter().any(|e| e.run_id.as_deref() == Some(run)) => {
                    if !expanded_group
                        && entries.iter().any(|e| {
                            e.is_tool_like()
                                || (e.item_type == ItemType::Error && e.status == Lifecycle::Failed)
                        })
                    {
                        has_group = true;
                    }
                    if has_group {
                        last = Some(i);
                    }
                }
                _ => {}
            }
        }
        if let Some(li) = last {
            without_meta.insert(mi);
            meta_after.insert(
                li,
                Row {
                    id: format!("assistant-meta:{}", message.id),
                    created_at: rows[li].created_at.or(Some(message.updated_at)),
                    kind: RowKind::AssistantMeta {
                        message: message.clone(),
                    },
                    continues_work_log: false,
                },
            );
        }
    }
    let mut out = Vec::with_capacity(rows.len() + meta_after.len());
    for (i, mut row) in rows.into_iter().enumerate() {
        if without_meta.contains(&i)
            && let RowKind::Message { show_meta, .. } = &mut row.kind
        {
            *show_meta = false;
        }
        out.push(row);
        if let Some(meta) = meta_after.remove(&i) {
            out.push(meta);
        }
    }
    out
}

/// `settleSupersededReasoning`: only the last entry can still be thinking.
fn settle_reasoning(entries: &[Entry]) -> Vec<Entry> {
    let n = entries.len();
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let mut e = e.clone();
            if i + 1 < n
                && let EntryKind::Work(w) = &mut e.kind
                && w.item_type == ItemType::Reasoning
                && w.status == Lifecycle::InProgress
            {
                w.status = Lifecycle::Completed;
            }
            e
        })
        .collect()
}

/// `timelineRowEntries`: visibility filtering.
fn row_entries(input: &RowsInput) -> Vec<Entry> {
    let entries = input.entries;
    let shown: Vec<Entry> = match input.visibility {
        ToolCallVisibility::ShowAll => entries.to_vec(),
        ToolCallVisibility::Zen => {
            let answering_run = if input.is_working {
                unsettled_run_id(input.latest_run, input.running_run_id)
            } else {
                None
            };
            let answering_from = if input.is_working {
                last_response_boundary(entries).map_or(0, |i| i + 1)
            } else {
                entries.len()
            };
            let terminal = terminal_assistant_ids(entries);
            let retained = retained_reply_ids(entries);
            entries
                .iter()
                .enumerate()
                .filter(|(i, e)| match &e.kind {
                    EntryKind::Message(m) => {
                        m.role != Role::Assistant
                            || retained.contains(&e.id)
                            || (terminal.contains(&m.id)
                                && *i < answering_from
                                && (answering_run.is_none()
                                    || m.run_id.as_deref() != answering_run))
                    }
                    EntryKind::Event(item) => !matches!(item.body, TurnItemBody::Subagent { .. }),
                    EntryKind::Plan(_) => true,
                    EntryKind::Work(w) => {
                        let pending = matches!(&w.item.body, TurnItemBody::ApprovalRequest { request_id, .. } if input.pending_approvals.contains(request_id));
                        pending
                            || w.question_answer
                            || is_compaction(e)
                            || w.tone == Tone::Error
                            || w.source_kind == Some(SourceKind::RuntimeError)
                            || (w.item_type == ItemType::Error && w.status != Lifecycle::Completed)
                    }
                })
                .map(|(_, e)| e.clone())
                .collect()
        }
        v => entries
            .iter()
            .filter(|e| e.work().is_none_or(|w| !w.hidden_by(v)))
            .cloned()
            .collect(),
    };
    settle_reasoning(&shown)
}

/// `deriveMessagesTimelineRows`.
pub fn derive_rows(input: &RowsInput) -> Vec<Row> {
    let entries = row_entries(input);
    let entries = entries.as_slice();
    let is_pending_approval = |w: &WorkEntry| {
        matches!(&w.item.body, TurnItemBody::ApprovalRequest { request_id, .. } if input.pending_approvals.contains(request_id))
    };
    let unsettled = unsettled_run_id(input.latest_run, input.running_run_id);
    let terminal = terminal_assistant_ids(entries);
    let failed = failed_run_ids(entries, input.latest_run);
    let itemized = itemized_run_ids(entries, &failed);
    let live_subagents = live_subagent_ids(entries);
    let retained = retained_reply_ids(entries);
    let active_visual = active_visual_runs(entries, unsettled, input.is_working);
    let folds = if input.visibility == ToolCallVisibility::Zen {
        HashMap::new()
    } else {
        let unfolded: HashSet<String> = active_visual.union(&failed).cloned().collect();
        derive_turn_folds(&FoldInput {
            entries,
            terminal: &terminal,
            latest: input.latest_run,
            unfolded: &unfolded,
            live_subagents: &live_subagents,
            retained: &retained,
        })
    };
    let mut collapsed: HashSet<String> = HashSet::new();
    for fold in folds.values() {
        if !input.expanded_runs.contains(&fold.run_id) {
            collapsed.extend(fold.hidden.iter().cloned());
        }
    }
    for e in entries {
        if e.work().is_some_and(is_pending_approval) {
            collapsed.remove(&e.id);
        }
    }
    let run_is_active =
        |run: Option<&str>| run.is_some_and(|r| active_visual.contains(r));
    let in_active_run = |w: &WorkEntry| {
        input.is_working
            && w.status == Lifecycle::InProgress
            && w.run_id.is_some()
            && w.run_id.as_deref() == unsettled
    };

    let header_index = if input.is_working {
        last_response_boundary(entries).map_or(0, |i| i + 1)
    } else {
        entries.len()
    };

    // The live activity tail of the running turn.
    let mut active_tool: Vec<&Entry> = Vec::new();
    if input.is_working && unsettled.is_some() {
        for e in entries[header_index.min(entries.len())..].iter().rev() {
            let Some(w) = e.work() else { break };
            if w.tone == Tone::Error
                || w.source_kind == Some(SourceKind::RuntimeError)
                || matches!(w.item_type, ItemType::SystemNotice | ItemType::TodoList)
                || w.question_answer
                || is_pending_approval(w)
                || !run_is_active(w.run_id.as_deref())
                || w.source_kind == Some(SourceKind::ContextCompaction)
                || collapsed.contains(&e.id)
                || folds.contains_key(&e.id)
            {
                break;
            }
            active_tool.push(e);
        }
    }
    active_tool.reverse();
    let visible_active: Vec<&Entry> = active_tool
        .iter()
        .copied()
        .filter(|e| e.work().unwrap().visible_in_group(true))
        .collect();
    let anchor = active_tool.first().copied();
    let latest_visible = visible_active.last().copied();
    let latest_running = visible_active
        .iter()
        .rev()
        .find(|e| e.work().unwrap().is_active_turn_activity())
        .copied();
    let keeps_live = latest_running.is_some()
        || (latest_visible.is_some_and(|e| e.work().unwrap().indicates_success())
            && input.pending_approvals.is_empty());
    let latest_failed = latest_running.is_none()
        && latest_visible.is_some_and(|e| {
            let w = e.work().unwrap();
            w.status != Lifecycle::Declined && w.display_failure()
        });
    let latest_thought = visible_active
        .iter()
        .rev()
        .find(|e| {
            let w = e.work().unwrap();
            w.item_type == ItemType::Reasoning
                && w.detail.as_deref().is_some_and(|d| !d.trim().is_empty())
        })
        .map(|e| e.work().unwrap().clone());
    let placement_id = latest_visible.map(|e| e.id.clone());
    let active_row: Option<Row> = match (anchor, latest_visible) {
        (Some(anchor), Some(latest)) if !latest_failed => {
            let group_id = work_group_id(&anchor.id);
            Some(Row {
                id: if keeps_live {
                    LIVE_ACTIVITY_ROW_ID.into()
                } else {
                    format!("work-live:{}", anchor.id)
                },
                created_at: Some(anchor.created_at),
                kind: RowKind::WorkLive {
                    entry: latest_running.unwrap_or(latest).work().unwrap().clone(),
                    entries: visible_active.iter().map(|e| e.work().unwrap().clone()).collect(),
                    expanded: input.expanded_groups.contains(&group_id),
                    group_id,
                    active: keeps_live,
                    thought: latest_thought,
                },
                continues_work_log: false,
            })
        }
        _ => None,
    };
    let active_ids: HashSet<String> = if active_row.is_some() || latest_failed {
        active_tool.iter().map(|e| e.id.clone()).collect()
    } else {
        HashSet::new()
    };
    let active_compaction = entries
        .iter()
        .any(|e| is_compaction(e) && e.work().is_some_and(in_active_run));

    let mut rows: Vec<Row> = Vec::new();
    let mut has_activity_row = false;
    let working_row = |rows: &mut Vec<Row>| {
        rows.push(Row {
            id: WORKING_ROW_ID.into(),
            created_at: input.active_turn_started_at,
            kind: RowKind::Working { compacting: false },
            continues_work_log: false,
        });
    };

    let mut index = 0;
    while index < entries.len() {
        let e = &entries[index];
        if input.is_working && index == header_index {
            working_row(&mut rows);
        }
        if Some(&e.id) == placement_id.as_ref()
            && let Some(row) = &active_row
        {
            rows.push(row.clone());
            if let RowKind::WorkLive {
                expanded: true,
                group_id,
                entries: list,
                ..
            } = &row.kind
            {
                rows.push(expanded_group_row(group_id, e.created_at, list.clone()));
            }
            has_activity_row |= matches!(row.kind, RowKind::WorkLive { active: true, .. });
        }
        if e.event()
            .is_some_and(|i| matches!(i.body, TurnItemBody::RunInterruptRequest { .. }))
        {
            index += 1;
            continue;
        }
        if let Some(fold) = folds.get(&e.id) {
            rows.push(Row {
                id: format!("turn-fold:{}", fold.run_id),
                created_at: Some(fold.created_at),
                kind: RowKind::TurnFold {
                    run_id: fold.run_id.clone(),
                    label: fold.label.clone(),
                    expanded: input.expanded_runs.contains(&fold.run_id),
                },
                continues_work_log: false,
            });
            debug_assert_eq!(fold.anchor, e.id);
        }
        if collapsed.contains(&e.id) || active_ids.contains(&e.id) {
            index += 1;
            continue;
        }
        match &e.kind {
            EntryKind::Work(w) if w.source_kind == Some(SourceKind::ContextCompaction) => {
                let active = in_active_run(w);
                let (tokens, failure) = match &w.item.body {
                    TurnItemBody::Compaction {
                        before_token_count,
                        after_token_count,
                        summary,
                    } => (
                        before_token_count.zip(*after_token_count),
                        summary.clone().filter(|s| !s.trim().is_empty()),
                    ),
                    _ => (None, None),
                };
                let failed = w.item.status == ItemStatus::Failed;
                rows.push(Row {
                    id: e.id.clone(),
                    created_at: Some(e.created_at),
                    kind: RowKind::Compaction {
                        label: w.label.clone(),
                        tokens,
                        active,
                        failed,
                        failure: failure.filter(|_| failed),
                    },
                    continues_work_log: false,
                });
            }
            EntryKind::Work(w) => {
                let alone = is_pending_approval(w)
                    || w.run_id.as_ref().is_some_and(|r| itemized.contains(r))
                    || (w.item_type == ItemType::Error && w.status == Lifecycle::Failed)
                    || w.tone == Tone::Error
                    || w.source_kind == Some(SourceKind::RuntimeError)
                    || matches!(w.item_type, ItemType::SystemNotice | ItemType::TodoList)
                    || w.question_answer;
                if alone {
                    rows.push(Row {
                        id: e.id.clone(),
                        created_at: Some(e.created_at),
                        kind: RowKind::Work {
                            entries: vec![w.clone()],
                            expanded_group: false,
                            display_label: None,
                        },
                        continues_work_log: false,
                    });
                    index += 1;
                    continue;
                }
                let mut grouped = vec![w.clone()];
                let mut cursor = index + 1;
                while let Some(next) = entries.get(cursor) {
                    let Some(nw) = next.work() else { break };
                    if nw.source_kind == Some(SourceKind::ContextCompaction)
                        || (nw.item_type == ItemType::Error && nw.status == Lifecycle::Failed)
                        || nw.tone == Tone::Error
                        || nw.source_kind == Some(SourceKind::RuntimeError)
                        || matches!(nw.item_type, ItemType::SystemNotice | ItemType::TodoList)
                        || nw.question_answer
                        || is_pending_approval(nw)
                        || active_ids.contains(&next.id)
                        || collapsed.contains(&next.id)
                        || folds.contains_key(&next.id)
                        || nw.run_id != w.run_id
                    {
                        break;
                    }
                    grouped.push(nw.clone());
                    cursor += 1;
                }
                let visible: Vec<WorkEntry> = grouped
                    .into_iter()
                    .filter(|g| g.visible_in_group(in_active_run(g)))
                    .collect();
                if !visible.is_empty() {
                    let group_id = work_group_id(&e.id);
                    let expanded = input.expanded_groups.contains(&group_id);
                    let in_progress: Vec<&WorkEntry> =
                        visible.iter().filter(|g| in_active_run(g)).collect();
                    if let Some(latest) = in_progress.last() {
                        rows.push(Row {
                            id: format!("work-live:{}", e.id),
                            created_at: Some(e.created_at),
                            kind: RowKind::WorkLive {
                                entry: (*latest).clone(),
                                entries: visible.clone(),
                                group_id: group_id.clone(),
                                expanded,
                                active: true,
                                thought: None,
                            },
                            continues_work_log: false,
                        });
                        has_activity_row = true;
                        if expanded {
                            rows.push(expanded_group_row(&group_id, e.created_at, visible));
                        }
                    } else if visible.len() == 1 && visible[0].is_tool_like() {
                        let single = &visible[0];
                        let label = if single.action() == super::entries::Action::Edit {
                            summarize_group(&visible)
                        } else {
                            single.single_label(None)
                        };
                        rows.push(Row {
                            id: e.id.clone(),
                            created_at: Some(e.created_at),
                            kind: RowKind::Work {
                                entries: visible,
                                expanded_group: false,
                                display_label: Some(label),
                            },
                            continues_work_log: false,
                        });
                    } else {
                        let latest_tool = visible.iter().rev().find(|g| g.is_tool_like());
                        let summary = if visible.len() == 1 && !visible[0].is_tool_like() {
                            visible[0].label.clone()
                        } else {
                            summarize_group(&visible)
                        };
                        rows.push(Row {
                            id: format!("work-toggle:{}", e.id),
                            created_at: Some(e.created_at),
                            kind: RowKind::WorkToggle {
                                run_id: w.run_id.clone(),
                                group_id: group_id.clone(),
                                count: visible.len(),
                                expanded,
                                summary,
                                kind: summary_kind(&visible),
                                has_failure: latest_tool.is_some_and(|t| t.display_failure()),
                            },
                            continues_work_log: false,
                        });
                        if expanded {
                            rows.push(expanded_group_row(&group_id, e.created_at, visible));
                        }
                    }
                }
                index = cursor;
                continue;
            }
            EntryKind::Plan(p) => rows.push(Row {
                id: e.id.clone(),
                created_at: Some(e.created_at),
                kind: RowKind::Plan(p.clone()),
                continues_work_log: false,
            }),
            EntryKind::Event(item) => {
                if let TurnItemBody::Subagent { .. } = item.body
                    && let Some(Row {
                        kind:
                            RowKind::Event {
                                item: prev,
                                subagents,
                            },
                        ..
                    }) = rows.last_mut()
                    && matches!(prev.body, TurnItemBody::Subagent { .. })
                    && prev.run_id == item.run_id
                {
                    if subagents.is_empty() {
                        subagents.push(prev.clone());
                    }
                    subagents.push(item.clone());
                    index += 1;
                    continue;
                }
                rows.push(Row {
                    id: e.id.clone(),
                    created_at: Some(e.created_at),
                    kind: RowKind::Event {
                        item: item.clone(),
                        subagents: Vec::new(),
                    },
                    continues_work_log: false,
                });
            }
            EntryKind::Message(m) => {
                let still_answering =
                    m.role == Role::Assistant && run_is_active(m.run_id.as_deref());
                let show_meta =
                    m.role == Role::Assistant && terminal.contains(&m.id) && !still_answering;
                rows.push(Row {
                    id: e.id.clone(),
                    created_at: Some(e.created_at),
                    kind: RowKind::Message {
                        message: m.clone(),
                        show_meta,
                        copy_streaming: m.streaming || still_answering,
                        grouped_with_next: false,
                        grouped_with_previous: false,
                        continues_steer: false,
                    },
                    continues_work_log: false,
                });
            }
        }
        index += 1;
    }

    let reply_streaming = entries
        .last()
        .and_then(Entry::message)
        .is_some_and(|m| m.role == Role::Assistant && m.streaming);
    let has_working = rows.iter().any(|r| matches!(r.kind, RowKind::Working { .. }));
    if input.is_working && !has_working && header_index >= entries.len() {
        working_row(&mut rows);
    }
    let turn_ended = input
        .latest_run
        .is_some_and(|l| l.status.is_terminal());
    if input.is_working
        && !turn_ended
        && !reply_streaming
        && !active_compaction
        && (latest_failed || (!has_activity_row && input.pending_approvals.is_empty()))
    {
        match anchor.filter(|_| latest_failed) {
            Some(a) => {
                let group_id = work_group_id(&a.id);
                let expanded = input.expanded_groups.contains(&group_id);
                rows.push(Row {
                    id: LIVE_ACTIVITY_ROW_ID.into(),
                    created_at: input.active_turn_started_at,
                    kind: RowKind::Thinking {
                        label: None,
                        group_id: Some(group_id.clone()),
                        expanded,
                    },
                    continues_work_log: false,
                });
                if expanded {
                    let list = visible_active.iter().map(|e| e.work().unwrap().clone()).collect();
                    rows.push(expanded_group_row(&group_id, a.created_at, list));
                }
            }
            None => {
                let waiting = unsettled.map_or(0, |r| running_subagents_at_tail(input.entries, r));
                rows.push(Row {
                    id: LIVE_ACTIVITY_ROW_ID.into(),
                    created_at: input.active_turn_started_at,
                    kind: RowKind::Thinking {
                        label: match waiting {
                            0 => None,
                            1 => Some("Waiting on a subagent".into()),
                            n => Some(format!("Waiting on {n} subagents")),
                        },
                        group_id: None,
                        expanded: false,
                    },
                    continues_work_log: false,
                });
            }
        }
    }

    let mut rows = attach_meta(rows);
    // Post-pass: work log continuation and grouped own messages.
    let n = rows.len();
    for i in 0..n {
        if rows[i].is_work_log() && rows.get(i + 1).is_some_and(Row::is_work_log) {
            rows[i].continues_work_log = true;
        }
    }
    let own_user = |r: Option<&Row>| {
        matches!(r, Some(Row { kind: RowKind::Message { message, .. }, .. }) if message.role == Role::User)
    };
    let steer_of = |r: &Row| matches!(&r.kind, RowKind::Message { message, .. } if message.is_steer());
    for i in 0..n {
        if !own_user(rows.get(i)) {
            continue;
        }
        let next = own_user(rows.get(i + 1));
        let prev = i > 0 && own_user(rows.get(i - 1));
        let steer = prev && steer_of(&rows[i]) && steer_of(&rows[i - 1]);
        if let RowKind::Message {
            grouped_with_next,
            grouped_with_previous,
            continues_steer,
            ..
        } = &mut rows[i].kind
        {
            *grouped_with_next = next;
            *grouped_with_previous = prev;
            *continues_steer = steer;
        }
    }
    rows
}

/// `runningSubagentsAtRunTail`.
fn running_subagents_at_tail(entries: &[Entry], run: &str) -> usize {
    let mut running = 0;
    for e in entries.iter().rev() {
        if let Some(m) = e.message()
            && m.role == Role::Assistant
            && !m.streaming
            && m.run_id.as_deref() == Some(run)
        {
            continue;
        }
        let Some(item) = e.event() else { break };
        if !matches!(item.body, TurnItemBody::Subagent { .. }) || item.run_id.as_deref() != Some(run) {
            break;
        }
        if item.status.is_open() {
            running += 1;
        }
    }
    running
}

/// Spacing below a row (`MessagesTimeline.tsx:1667-1700`, spec §2.4), in px.
pub fn row_padding_bottom(row: &Row) -> f32 {
    match &row.kind {
        RowKind::Work { expanded_group: true, .. } => 4.,
        _ if row.is_work_log() => {
            if row.continues_work_log {
                0.
            } else {
                8.
            }
        }
        RowKind::TurnFold { .. } | RowKind::Working { .. } => 6.,
        RowKind::Message {
            message,
            grouped_with_next,
            show_meta,
            ..
        } => match message.role {
            Role::User if *grouped_with_next => 4.,
            Role::User => 16.,
            Role::Assistant if *show_meta => 16.,
            Role::Assistant => 8.,
        },
        RowKind::Event { .. } => 8.,
        _ => 16.,
    }
}

#[cfg(test)]
mod tests {
    use super::super::entries::derive_entries;
    use super::super::entries::tests::{at, cmd, edit, item};
    use super::*;
    use tern_core::{AssistantPhase, InputIntent};

    fn user(id: &str, run: &str, secs: i64, text: &str) -> Rc<TurnItem> {
        item(
            id,
            Some(run),
            ItemStatus::Completed,
            secs,
            TurnItemBody::UserMessage {
                message_id: format!("m-{id}"),
                text: text.into(),
                attachments: vec![],
                input_intent: InputIntent::TurnStart,
                model_switch: None,
            },
        )
    }

    fn say(id: &str, run: &str, secs: i64, text: &str, streaming: bool) -> Rc<TurnItem> {
        item(
            id,
            Some(run),
            if streaming {
                ItemStatus::Running
            } else {
                ItemStatus::Completed
            },
            secs,
            TurnItemBody::AssistantMessage {
                message_id: format!("m-{id}"),
                text: text.into(),
                streaming,
                phase: Some(AssistantPhase::FinalAnswer),
            },
        )
    }

    fn reasoning(id: &str, run: &str, secs: i64, status: ItemStatus) -> Rc<TurnItem> {
        item(
            id,
            Some(run),
            status,
            secs,
            TurnItemBody::Reasoning {
                text: "I'm adding the docstring now.".into(),
                streaming: false,
            },
        )
    }

    struct Fixture {
        entries: Vec<Entry>,
        pending: HashSet<String>,
        runs: HashSet<String>,
        groups: HashSet<String>,
        latest: Option<LatestRun>,
        working: bool,
    }

    impl Fixture {
        fn new(items: &[Rc<TurnItem>]) -> Self {
            Self {
                entries: derive_entries(items),
                pending: HashSet::new(),
                runs: HashSet::new(),
                groups: HashSet::new(),
                latest: None,
                working: false,
            }
        }

        fn finished(mut self, run: &str, start: i64, end: i64) -> Self {
            self.latest = Some(LatestRun {
                run_id: run.into(),
                status: RunStatus::Completed,
                started_at: Some(at(start)),
                completed_at: Some(at(end)),
            });
            self
        }

        fn running(mut self, run: &str, start: i64) -> Self {
            self.latest = Some(LatestRun {
                run_id: run.into(),
                status: RunStatus::Running,
                started_at: Some(at(start)),
                completed_at: None,
            });
            self.working = true;
            self
        }

        fn rows(&self) -> Vec<Row> {
            derive_rows(&RowsInput {
                entries: &self.entries,
                pending_approvals: &self.pending,
                latest_run: self.latest.as_ref(),
                running_run_id: if self.working {
                    self.latest.as_ref().map(|l| l.run_id.as_str())
                } else {
                    None
                },
                expanded_runs: &self.runs,
                expanded_groups: &self.groups,
                is_working: self.working,
                active_turn_started_at: self.latest.as_ref().and_then(|l| l.started_at),
                visibility: ToolCallVisibility::ShowAll,
            })
        }
    }

    fn kinds(rows: &[Row]) -> Vec<&'static str> {
        rows.iter()
            .map(|r| match &r.kind {
                RowKind::Message { message, .. } if message.role == Role::User => "user",
                RowKind::Message { .. } => "assistant",
                RowKind::AssistantMeta { .. } => "meta",
                RowKind::Work { expanded_group: true, .. } => "work-details",
                RowKind::Work { .. } => "work",
                RowKind::WorkLive { .. } => "work-live",
                RowKind::Working { .. } => "working",
                RowKind::Thinking { .. } => "thinking",
                RowKind::WorkToggle { .. } => "work-toggle",
                RowKind::TurnFold { .. } => "turn-fold",
                RowKind::Compaction { .. } => "compaction",
                RowKind::Plan(_) => "plan",
                RowKind::Event { .. } => "event",
            })
            .collect()
    }

    fn complete_thread() -> Vec<Rc<TurnItem>> {
        vec![
            user("u", "r", 0, "List the files"),
            cmd("c1", "r", 1, "git ls-files", Some(0)),
            reasoning("t", "r", 2, ItemStatus::Completed),
            cmd("c2", "r", 3, "head -15 README.md", Some(0)),
            edit("e", "r", 4, "notes.py"),
            say("a", "r", 8, "Tracked files in the repo:", false),
        ]
    }

    #[test]
    fn finished_turn_folds_its_work() {
        let f = Fixture::new(&complete_thread()).finished("r", 0, 8);
        let rows = f.rows();
        assert_eq!(kinds(&rows), ["user", "turn-fold", "assistant"]);
        let RowKind::TurnFold { label, expanded, .. } = &rows[1].kind else {
            panic!()
        };
        assert_eq!(label, "Worked for 8s");
        assert!(!expanded);
        let RowKind::Message { show_meta, .. } = &rows[2].kind else {
            panic!()
        };
        assert!(show_meta);
    }

    #[test]
    fn opening_the_fold_reveals_the_summary_and_its_group() {
        let mut f = Fixture::new(&complete_thread()).finished("r", 0, 8);
        f.runs.insert("r".into());
        let rows = f.rows();
        assert_eq!(kinds(&rows), ["user", "turn-fold", "work-toggle", "assistant"]);
        let RowKind::WorkToggle {
            summary, group_id, ..
        } = &rows[2].kind
        else {
            panic!()
        };
        assert_eq!(summary, "Ran 2 commands and changed 1 file");
        f.groups.insert(group_id.clone());
        let rows = f.rows();
        assert_eq!(
            kinds(&rows),
            ["user", "turn-fold", "work-toggle", "work-details", "assistant"]
        );
        let RowKind::Work { entries, .. } = &rows[3].kind else {
            panic!()
        };
        assert_eq!(entries.len(), 4);
        assert!(rows[2].continues_work_log);
    }

    #[test]
    fn running_turn_shows_working_header_and_live_row() {
        let items = vec![
            user("u", "r", 0, "List the files"),
            reasoning("t", "r", 2, ItemStatus::Completed),
            cmd("c", "r", 3, "head -15 README.md", None),
        ];
        let f = Fixture::new(&items).running("r", 0);
        let rows = f.rows();
        assert_eq!(kinds(&rows), ["user", "working", "work-live"]);
        let RowKind::WorkLive {
            entry,
            active,
            thought,
            ..
        } = &rows[2].kind
        else {
            panic!()
        };
        assert!(active);
        assert!(thought.is_some());
        assert_eq!(entry.live_label(None, true), "Running head");
        assert_eq!(rows[2].id, LIVE_ACTIVITY_ROW_ID);
    }

    #[test]
    fn running_turn_without_activity_shows_thinking() {
        let f = Fixture::new(&[user("u", "r", 0, "hi")]).running("r", 0);
        assert_eq!(kinds(&f.rows()), ["user", "working", "thinking"]);
    }

    #[test]
    fn streaming_reply_has_no_thinking_row_and_no_fold() {
        let items = vec![
            user("u", "r", 0, "hi"),
            cmd("c", "r", 1, "ls", Some(0)),
            say("a", "r", 2, "Here", true),
        ];
        let f = Fixture::new(&items).running("r", 0);
        let rows = f.rows();
        assert_eq!(kinds(&rows), ["user", "working", "work", "assistant"]);
        let RowKind::Message {
            show_meta,
            copy_streaming,
            ..
        } = &rows[3].kind
        else {
            panic!()
        };
        assert!(!show_meta);
        assert!(copy_streaming);
    }

    #[test]
    fn pending_approval_stands_alone_and_suppresses_thinking() {
        let approval = item(
            "ap",
            Some("r"),
            ItemStatus::Waiting,
            2,
            TurnItemBody::ApprovalRequest {
                request_id: "req".into(),
                request_kind: tern_core::RequestKind::Command,
                prompt: Some("touch TODO.md && ls".into()),
                reason: None,
                full_access_override: None,
                options: None,
            },
        );
        let mut f = Fixture::new(&[user("u", "r", 0, "touch it"), approval]).running("r", 0);
        f.pending.insert("req".into());
        let rows = f.rows();
        assert_eq!(kinds(&rows), ["user", "working", "work"]);
    }

    #[test]
    fn failed_command_after_answer_keeps_meta_after_tools() {
        let items = vec![
            user("u", "r", 0, "do it"),
            say("a", "r", 2, "Done.", false),
            cmd("c", "r", 3, "cargo test", Some(101)),
        ];
        let f = Fixture::new(&items).finished("r", 0, 4);
        let rows = f.rows();
        // The failed trailing command stays visible; the meta line moves below it.
        assert_eq!(kinds(&rows), ["user", "assistant", "work", "meta"]);
    }

    #[test]
    fn consecutive_user_messages_group() {
        let items = vec![
            user("u1", "r1", 0, "one"),
            user("u2", "r2", 1, "two"),
        ];
        let rows = Fixture::new(&items).rows();
        let RowKind::Message {
            grouped_with_next, ..
        } = &rows[0].kind
        else {
            panic!()
        };
        assert!(grouped_with_next);
        assert_eq!(row_padding_bottom(&rows[0]), 4.);
        assert_eq!(row_padding_bottom(&rows[1]), 16.);
    }

    #[test]
    fn interrupted_turn_does_not_fold() {
        let items = vec![
            user("u", "r", 0, "go"),
            cmd("c", "r", 1, "sleep 40", Some(0)),
            item(
                "ir",
                Some("r"),
                ItemStatus::Completed,
                5,
                TurnItemBody::RunInterruptResult {
                    message: "Stopped".into(),
                },
            ),
        ];
        let mut f = Fixture::new(&items);
        f.latest = Some(LatestRun {
            run_id: "r".into(),
            status: RunStatus::Interrupted,
            started_at: Some(at(0)),
            completed_at: Some(at(5)),
        });
        assert_eq!(kinds(&f.rows()), ["user", "work", "event"]);
    }

    #[test]
    fn fingerprints_change_with_content() {
        let f = Fixture::new(&complete_thread()).finished("r", 0, 8);
        let a = f.rows();
        let mut g = Fixture::new(&complete_thread()).finished("r", 0, 8);
        g.runs.insert("r".into());
        let b = g.rows();
        assert_eq!(a[0].fingerprint(), b[0].fingerprint());
        assert_ne!(a[1].fingerprint(), b[1].fingerprint());
    }
}

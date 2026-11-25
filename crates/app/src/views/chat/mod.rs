//! The chat column (spec 03): the header with the breadcrumb, and the timeline of a
//! thread as a virtualized list of rows derived from the thread's turn items.
//!
//! - `entries`: turn items → timeline entries and their presentation (labels, icons).
//! - `rows`: entries → display rows (turn folds, groups, live activity, working header).
//! - `format`: durations, timestamps, command labels.
//! - `header`, `row_view`, `work_view`, `find`: the rendering.
//!
//! Rows are keyed by id; a row whose fingerprint did not change keeps its measured
//! height. Markdown is parsed once per message text and only the tail reparses while
//! a reply streams. Disclosures (open folds, groups, entries) and the scroll position
//! are remembered per thread for the session.

pub mod entries;
pub mod find;
pub mod format;
pub mod header;
pub mod row_view;
pub mod rows;
pub mod work_view;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ListAlignment, ListOffset, ListState,
    ParentElement as _, Pixels, Render, SharedString, Styled as _, Task, Window, actions, canvas,
    div, list, prelude::FluentBuilder as _, px,
};
use tern_core::{RequestKind, RequestStatus, Snapshot, ThreadDetail, ThreadId, TurnItem};

use crate::app_state::{AppState, Route};
use crate::markdown;
use crate::store::{Store, StoreEvent};
use crate::theme::{ActiveTheme as _, TextExt as _, layout};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Icon, IconName, ScrollFade, h_flex, scroll::FadeEdge,
    scroll_fade, v_flex,
};

use entries::{Entry, ToolCallVisibility};
use rows::{LatestRun, Row, RowsInput};

actions!(chat, [FindInThread, FindNext, FindPrevious, CloseFind]);

/// Binds Ctrl+F (spec 03 §6.1). Call once at startup; [`ChatView::new`] does it.
fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-f", FindInThread, Some("Workspace")),
        KeyBinding::new("enter", FindNext, Some("ThreadFind")),
        KeyBinding::new("shift-enter", FindPrevious, Some("ThreadFind")),
        KeyBinding::new("escape", CloseFind, Some("ThreadFind")),
    ]);
}

/// Open folds, groups and work entries of one thread.
#[derive(Clone, Debug, Default)]
pub struct Disclosures {
    pub runs: HashSet<String>,
    pub groups: HashSet<String>,
    /// Work entries whose details are open.
    pub entries: HashSet<String>,
}

/// Where a thread was left (`timelineScrollAnchoring.ts:115-151`).
#[derive(Clone, Copy, Debug)]
struct SavedPosition {
    at_end: bool,
    offset: ListOffset,
}

const POSITION_MEMORY: usize = 100;

pub struct ChatView {
    weak: gpui::WeakEntity<ChatView>,
    app: Entity<AppState>,
    store: Entity<Store>,
    focus: FocusHandle,
    thread: Option<ThreadId>,
    detail: Option<Rc<ThreadDetail>>,
    detail_revision: Option<u64>,
    entries: Vec<Entry>,
    rows: Rc<Vec<Row>>,
    fingerprints: Vec<u64>,
    list: ListState,
    md: markdown::Cache,
    disclosures: HashMap<ThreadId, Disclosures>,
    positions: Vec<(ThreadId, SavedPosition)>,
    /// Row id whose copy button shows the check.
    copied: Option<String>,
    find: find::FindState,
    header: header::HeaderState,
    tick: Option<Task<()>>,
    list_width: Rc<Cell<Pixels>>,
    visibility: ToolCallVisibility,
}

impl ChatView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        bind_keys(cx);
        let app = AppState::global(cx);
        let store = Store::global(cx);
        cx.observe(&app, |this, _, cx| {
            this.sync(cx);
            cx.notify();
        })
        .detach();
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&store, |this, _, event: &StoreEvent, cx| {
            let changed = match event {
                StoreEvent::Thread(id) => this.thread.as_ref() == Some(id),
                StoreEvent::Snapshot => true,
                _ => false,
            };
            if changed {
                this.sync(cx);
            }
        })
        .detach();
        let list = ListState::new(0, ListAlignment::Top, px(600.));
        list.set_follow_mode(gpui::FollowMode::Tail);
        let this = cx.entity().downgrade();
        list.set_scroll_handler(move |_, _, cx| {
            if let Some(this) = this.upgrade() {
                this.update(cx, |_, cx| cx.notify());
            }
        });
        let mut view = Self {
            weak: cx.entity().downgrade(),
            app,
            store,
            focus: cx.focus_handle(),
            thread: None,
            detail: None,
            detail_revision: None,
            entries: Vec::new(),
            rows: Rc::new(Vec::new()),
            fingerprints: Vec::new(),
            list,
            md: markdown::Cache::default(),
            disclosures: HashMap::new(),
            positions: Vec::new(),
            copied: None,
            find: find::FindState::new(window, cx),
            header: header::HeaderState::default(),
            tick: None,
            list_width: Rc::new(Cell::new(px(1000.))),
            visibility: ToolCallVisibility::ShowAll,
        };
        view.sync(cx);
        view
    }

    fn route_thread(&self, cx: &App) -> Option<ThreadId> {
        match self.app.read(cx).chat_route() {
            Route::Thread(id) => Some(id.clone()),
            _ => None,
        }
    }

    fn disclosures(&self) -> Disclosures {
        self.thread
            .as_ref()
            .and_then(|t| self.disclosures.get(t))
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn update_disclosures(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Disclosures),
    ) {
        let Some(thread) = self.thread.clone() else {
            return;
        };
        f(self.disclosures.entry(thread).or_default());
        // A disclosure is the reader's own navigation; keep the toggled row in place.
        if self.list.is_following_tail() && !self.is_working(cx) {
            self.list.pause_following_tail();
        }
        self.rebuild(cx);
        cx.notify();
    }

    fn save_position(&mut self) {
        let Some(thread) = self.thread.clone() else {
            return;
        };
        let pos = SavedPosition {
            at_end: self.list.is_following_tail(),
            offset: self.list.logical_scroll_top(),
        };
        self.positions.retain(|(t, _)| *t != thread);
        self.positions.push((thread, pos));
        if self.positions.len() > POSITION_MEMORY {
            self.positions.remove(0);
        }
    }

    pub(crate) fn is_working(&self, cx: &App) -> bool {
        self.thread
            .as_ref()
            .and_then(|t| self.store.read(cx).snapshot().thread(t).map(|s| s.is_working()))
            .unwrap_or(false)
    }

    /// Re-reads the route and the thread; rebuilds rows when anything changed.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let route_thread = self.route_thread(cx);
        if route_thread != self.thread {
            self.save_position();
            self.thread = route_thread.clone();
            self.detail = None;
            self.detail_revision = None;
            self.header.cancel_rename();
            self.find.close();
            self.rows = Rc::new(Vec::new());
            self.fingerprints.clear();
            self.list.reset(0);
            let saved = route_thread
                .as_ref()
                .and_then(|t| self.positions.iter().find(|(id, _)| id == t))
                .map(|(_, p)| *p);
            self.rebuild(cx);
            match saved {
                Some(p) if !p.at_end => {
                    self.list.set_follow_mode(gpui::FollowMode::Tail);
                    self.list.scroll_to(p.offset);
                }
                _ => self.list.set_follow_mode(gpui::FollowMode::Tail),
            }
        } else {
            self.rebuild(cx);
        }
        self.update_tick(cx);
    }

    /// Ticks once a second while the thread works, for "Working for 12s".
    fn update_tick(&mut self, cx: &mut Context<Self>) {
        let working = self.is_working(cx);
        if working && self.tick.is_none() {
            self.tick = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
        } else if !working {
            self.tick = None;
        }
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let Some(thread) = self.thread.clone() else {
            if !self.rows.is_empty() {
                self.rows = Rc::new(Vec::new());
                self.fingerprints.clear();
                self.list.reset(0);
            }
            return;
        };
        let store = self.store.read(cx);
        let detail = store.thread(&thread);
        let shell = store.snapshot().thread(&thread).cloned();
        let Some(detail) = detail else {
            return;
        };
        if self.detail_revision != Some(detail.revision)
            || !self
                .detail
                .as_ref()
                .is_some_and(|d| Rc::ptr_eq(d, &detail))
        {
            let items: Vec<Rc<TurnItem>> = detail.items.iter().cloned().map(Rc::new).collect();
            self.entries = entries::derive_entries(&items);
            self.detail_revision = Some(detail.revision);
            self.detail = Some(detail.clone());
        }
        let pending: HashSet<String> = detail
            .requests
            .iter()
            .filter(|r| r.status == RequestStatus::Pending && r.kind != RequestKind::UserInput)
            .map(|r| r.id.clone())
            .collect();
        let latest = detail.runs.iter().max_by_key(|r| r.ordinal).map(|r| LatestRun {
            run_id: r.id.clone(),
            status: r.status,
            started_at: r.started_at,
            completed_at: r.completed_at,
        });
        let working = shell.as_ref().is_some_and(|s| s.is_working());
        let running = shell.as_ref().and_then(|s| s.active_run_id.clone());
        let disclosures = self.disclosures();
        let rows = rows::derive_rows(&RowsInput {
            entries: &self.entries,
            pending_approvals: &pending,
            latest_run: latest.as_ref(),
            running_run_id: running.as_deref(),
            expanded_runs: &disclosures.runs,
            expanded_groups: &disclosures.groups,
            is_working: working,
            active_turn_started_at: latest.as_ref().and_then(|l| l.started_at),
            visibility: self.visibility,
        });
        self.apply_rows(rows);
        self.find.refresh(&self.rows, &mut self.md);
    }

    /// Splices the list where rows changed so unchanged rows keep their heights.
    fn apply_rows(&mut self, rows: Vec<Row>) {
        let fps: Vec<u64> = rows.iter().map(Row::fingerprint).collect();
        let old = &self.fingerprints;
        let prefix = old.iter().zip(&fps).take_while(|(a, b)| a == b).count();
        let max_suffix = old.len().min(fps.len()) - prefix;
        let suffix = old
            .iter()
            .rev()
            .zip(fps.iter().rev())
            .take(max_suffix)
            .take_while(|(a, b)| a == b)
            .count();
        if prefix != old.len() || prefix != fps.len() {
            self.list
                .splice(prefix..old.len() - suffix, fps.len() - suffix - prefix);
        }
        self.fingerprints = fps;
        self.rows = Rc::new(rows);
    }

    pub(crate) fn workspace_root(&self, cx: &App) -> Option<String> {
        let detail = self.detail.as_ref()?;
        if let Some(w) = &detail.thread.worktree_path {
            return Some(w.clone());
        }
        let snapshot: &Snapshot = self.store.read(cx).snapshot();
        snapshot
            .project(&detail.thread.project_id)
            .map(|p| p.workspace_root.clone())
    }

    pub(crate) fn scroll_to_end(&mut self, cx: &mut Context<Self>) {
        self.list.set_follow_mode(gpui::FollowMode::Tail);
        cx.notify();
    }

    pub(crate) fn mark_copied(&mut self, row_id: String, cx: &mut Context<Self>) {
        self.copied = Some(row_id.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1000)).await;
            let _ = this.update(cx, |this, cx| {
                if this.copied.as_deref() == Some(row_id.as_str()) {
                    this.copied = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Lane gutter: 12px below 640px of column width, 48px from there up.
    fn gutter(&self) -> Pixels {
        if self.list_width.get() < px(640.) {
            px(12.)
        } else {
            layout::CHAT_LANE_PADDING
        }
    }

    fn render_list_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.rows.clone();
        let Some(row) = rows.get(ix) else {
            return div().into_any_element();
        };
        let content = self.render_row(row, ix, window, cx);
        let pad = rows::row_padding_bottom(row);
        let highlight = self.find.row_is_active(ix);
        h_flex()
            .w_full()
            .justify_center()
            .px(self.gutter())
            .child(
                div()
                    .w_full()
                    .max_w(layout::CHAT_CONTENT_MAX_WIDTH)
                    .min_w_0()
                    .pb(px(pad))
                    .when(highlight, |el| el.rounded(px(6.)))
                    .child(content),
            )
            .into_any_element()
    }

    fn render_timeline(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        if self.thread.is_none() || (self.rows.is_empty() && self.detail.is_some()) {
            let text = if self.thread.is_some() {
                "Send a message to start the conversation."
            } else {
                ""
            };
            return div()
                .flex_1()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_scale(crate::theme::TextSize::Sm)
                .text_color(t.muted_foreground)
                .child(text)
                .into_any_element();
        }
        let width = self.list_width.clone();
        let following = self.list.is_following_tail();
        let show_pill = !following && !self.find.is_open() && !self.rows.is_empty();
        let summary = self.scroll_pill_summary(cx);
        div()
            .relative()
            .flex_1()
            .size_full()
            .min_h_0()
            .child(
                canvas(move |bounds, _, _| width.set(bounds.size.width), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(
                list(self.list.clone(), cx.processor(Self::render_list_row))
                    .size_full()
                    .pt(px(24.))
                    .pb(px(16.)),
            )
            // Top fade under the header (`topbar-scroll-fade`, 25px).
            .child(scroll_fade_band(t.background))
            .when(show_pill, |el| {
                el.child(
                    h_flex()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(10.))
                        .justify_center()
                        .child(
                            Button::new("scroll-to-end")
                                .variant(ButtonVariant::Glass)
                                .size(ButtonSize::Xs)
                                .icon(IconName::ChevronDown)
                                .icon_size(px(14.))
                                .label(match summary {
                                    Some(s) => SharedString::from(format!("Scroll to end · {s}")),
                                    None => "Scroll to end".into(),
                                })
                                .on_click(cx.listener(|this, _, _, cx| this.scroll_to_end(cx))),
                        ),
                )
            })
            .into_any_element()
    }

    fn scroll_pill_summary(&self, cx: &App) -> Option<String> {
        let detail = self.detail.as_ref()?;
        let queued = detail
            .runs
            .iter()
            .filter(|r| r.status == tern_core::RunStatus::Queued)
            .count();
        let _ = cx;
        (queued > 0).then(|| format!("{queued} queued"))
    }
}

fn scroll_fade_band(bg: gpui::Hsla) -> ScrollFade {
    scroll_fade(FadeEdge::Top, bg).size(25.)
}

impl Focusable for ChatView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        // Cheap when nothing changed: the store hands back the same detail.
        if self.thread.is_some()
            && let Some(d) = self.thread.as_ref().and_then(|id| self.store.read(cx).thread(id))
            && self.detail_revision != Some(d.revision)
        {
            self.rebuild(cx);
        }
        let header = self.render_header(window, cx);
        let timeline = self.render_timeline(window, cx);
        let find = self.find.is_open().then(|| self.render_find(window, cx));
        v_flex()
            .id("chat-view")
            .track_focus(&self.focus)
            .size_full()
            .bg(t.background)
            .child(header)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(timeline)
                    .children(find),
            )
            .on_action(cx.listener(|this, _: &FindInThread, window, cx| this.open_find(window, cx)))
            .on_action(cx.listener(|this, _: &CloseFind, window, cx| this.close_find(window, cx)))
            .on_action(cx.listener(|this, _: &FindNext, _, cx| this.find_step(1, cx)))
            .on_action(cx.listener(|this, _: &FindPrevious, _, cx| this.find_step(-1, cx)))
    }
}

/// Icon helper shared by the row renderers.
pub(crate) fn icon(name: IconName, size: f32, color: gpui::Hsla) -> Icon {
    Icon::new(name).size(px(size)).color(color)
}

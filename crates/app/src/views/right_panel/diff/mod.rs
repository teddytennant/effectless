//! The diff panel (spec 05 §2): one selection at a time (branch changes,
//! uncommitted, a commit, a turn or a tool call) as a virtualized list of per-file
//! diffs, with an optional file tree on the right.
//!
//! One `DiffView` per thread, kept alive while the thread's Diff tab exists so its
//! selection, folds and scroll survive tab switches.

pub mod code;
pub mod data;
pub mod rows;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, ClipboardItem, Context, Entity, EventEmitter,
    Global, InteractiveElement as _, IntoElement, ListAlignment, ListOffset, ListState,
    MouseButton, ParentElement as _, Pixels, Render, ScrollEvent, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, canvas, div, list,
    prelude::FluentBuilder as _, px,
};
use gpui_base::Align;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::scroll::Scrollbar;
use tern_core::{ThreadDetail, ThreadId};
use tern_git::DiffFile;

use self::code::{CodeColors, CodeMetrics};
use self::data::{Loaded, Request, Selection, TurnEntry};
use self::rows::{Row, build_rows, file_keys, is_test_file};
use super::file_tree::{FileTree, TreeOrder, TreeRow};
use super::tree_view;
use crate::store::{Store, StoreEvent};
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, font};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, DropdownMenu, Icon, IconName, Menu, MenuItem, Popover,
    PopoverPadding, PopoverWidth, Skeleton, Toggle, ToggleGroup, ToggleSize, ToggleVariant,
    Tooltip, TooltipExt as _, TooltipSide, h_flex, v_flex,
};

/// Diff settings shared by every thread (spec 05 §2.5): layout, whitespace, file
/// tree, hidden tests. Line wrap is per panel instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffPrefs {
    pub split: bool,
    pub ignore_whitespace: bool,
    pub tree_open: bool,
    pub hide_tests: bool,
    pub tree_width: Option<Pixels>,
}

impl Default for DiffPrefs {
    fn default() -> Self {
        Self {
            split: false,
            ignore_whitespace: true,
            tree_open: false,
            hide_tests: false,
            tree_width: None,
        }
    }
}

impl Global for DiffPrefs {}

impl DiffPrefs {
    pub fn get(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }

    pub fn update(cx: &mut App, f: impl FnOnce(&mut Self)) {
        let mut prefs = Self::get(cx);
        f(&mut prefs);
        cx.set_global(prefs);
    }
}

/// `LIVE_REFRESH_MIN_INTERVAL_MS`.
const LIVE_REFRESH_MIN: Duration = Duration::from_secs(10);
/// File tree width bounds (spec 05 §2.8).
const TREE_MIN: f32 = 160.;
const TREE_MAX: f32 = 560.;

pub enum DiffEvent {
    /// A file title was clicked: open it in the file viewer.
    OpenFile(String),
}

impl EventEmitter<DiffEvent> for DiffView {}

#[derive(Clone, Debug, PartialEq)]
struct LoadKey {
    selection: Selection,
    ignore_whitespace: bool,
    dark: bool,
    cwd: PathBuf,
}

pub struct DiffView {
    thread_id: ThreadId,
    store: Entity<Store>,
    selection: Selection,
    /// Remembered base ref while away from the branch scope.
    remembered_base: Option<String>,
    loaded: Option<Rc<Loaded>>,
    loaded_key: Option<LoadKey>,
    loading: Option<(LoadKey, Task<()>)>,
    last_load: Option<Instant>,
    /// Collapsed file keys per section id.
    collapsed: HashMap<String, HashSet<String>>,
    rows: Rc<Vec<Row>>,
    rows_key: Option<(usize, bool, Vec<bool>)>,
    list: ListState,
    word_wrap: bool,
    scroll_x: Pixels,
    commits: Rc<Vec<tern_git::review::CommitSummary>>,
    refs: Rc<Vec<(String, bool, bool)>>,
    ref_query: Entity<InputState>,
    tree_collapsed: HashSet<String>,
    tree_selected: Option<String>,
    tree_drag: Option<(Pixels, Pixels)>,
    body_width: Pixels,
    refs_width: Pixels,
    _subscriptions: Vec<gpui::Subscription>,
}

impl DiffView {
    pub fn new(thread_id: ThreadId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = Store::global(cx);
        let mut subscriptions = vec![
            cx.observe_global::<DiffPrefs>(|_, cx| cx.notify()),
            cx.subscribe(&store, |this: &mut Self, _, event: &StoreEvent, cx| {
                if let StoreEvent::Thread(id) = event
                    && *id == this.thread_id
                {
                    this.on_thread_changed(cx);
                }
            }),
        ];
        let ref_query = cx.new(|cx| InputState::new(window, cx).placeholder("Search refs..."));
        subscriptions.push(cx.subscribe(&ref_query, |_, _, _: &InputEvent, cx| cx.notify()));
        let list = ListState::new(0, ListAlignment::Top, px(600.));
        let this = cx.entity().downgrade();
        list.set_scroll_handler(move |_, _, cx| {
            if let Some(this) = this.upgrade() {
                this.update(cx, |_, cx| cx.notify());
            }
        });
        Self {
            thread_id,
            store,
            selection: Selection::default(),
            remembered_base: None,
            loaded: None,
            loaded_key: None,
            loading: None,
            last_load: None,
            collapsed: HashMap::new(),
            rows: Rc::new(Vec::new()),
            rows_key: None,
            list,
            word_wrap: true,
            scroll_x: px(0.),
            commits: Rc::new(Vec::new()),
            refs: Rc::new(Vec::new()),
            ref_query,
            tree_collapsed: HashSet::new(),
            tree_selected: None,
            tree_drag: None,
            body_width: px(0.),
            refs_width: px(1000.),
            _subscriptions: subscriptions,
        }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    fn detail(&self, cx: &App) -> Option<Rc<ThreadDetail>> {
        self.store.read(cx).thread(&self.thread_id)
    }

    /// The thread folder: its worktree, else the project root.
    fn cwd(&self, cx: &App) -> Option<PathBuf> {
        let store = self.store.read(cx);
        let shell = store.snapshot().thread(&self.thread_id)?;
        if let Some(path) = &shell.thread.worktree_path {
            return Some(PathBuf::from(path));
        }
        let project = store.snapshot().project(&shell.thread.project_id)?;
        Some(PathBuf::from(&project.workspace_root))
    }

    fn turns(&self, cx: &App) -> Vec<TurnEntry> {
        self.detail(cx).map(|d| data::turns(&d)).unwrap_or_default()
    }

    // --- selection -----------------------------------------------------------

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn select(&mut self, selection: Selection, cx: &mut Context<Self>) {
        if selection == self.selection {
            return;
        }
        // Leaving branch saves its base ref; returning restores it.
        if let Selection::Branch { base_ref } = &self.selection {
            self.remembered_base = base_ref.clone();
        }
        let selection = match selection {
            Selection::Branch { base_ref: None } => Selection::Branch {
                base_ref: self.remembered_base.clone(),
            },
            other => other,
        };
        self.selection = selection;
        self.list.scroll_to(ListOffset::default());
        self.scroll_x = px(0.);
        cx.notify();
    }

    /// "Diff" from the add menu / launcher resets to the branch scope.
    pub fn reset_to_branch(&mut self, cx: &mut Context<Self>) {
        self.select(Selection::Branch { base_ref: None }, cx);
    }

    fn set_base_ref(&mut self, base: Option<String>, cx: &mut Context<Self>) {
        self.remembered_base = base.clone();
        self.selection = Selection::Branch { base_ref: base };
        cx.notify();
    }

    fn on_thread_changed(&mut self, cx: &mut Context<Self>) {
        // A rolled-back turn falls back to the latest one.
        if let Selection::Turn { checkpoint_id } = &self.selection {
            let turns = self.turns(cx);
            if !turns.iter().any(|t| &t.checkpoint_id == checkpoint_id)
                && let Some(latest) = turns.first()
            {
                self.selection = Selection::Turn {
                    checkpoint_id: latest.checkpoint_id.clone(),
                };
            }
        }
        // Git scopes follow workspace changes, at most every 10s.
        if self.selection.is_git()
            && self
                .last_load
                .is_none_or(|t| t.elapsed() >= LIVE_REFRESH_MIN)
        {
            self.loaded_key = None;
        }
        cx.notify();
    }

    /// "Refresh diff".
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.loaded_key = None;
        self.loading = None;
        cx.notify();
    }

    // --- loading -------------------------------------------------------------

    fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        let Some(cwd) = self.cwd(cx) else {
            return;
        };
        let key = LoadKey {
            selection: self.selection.clone(),
            ignore_whitespace: DiffPrefs::get(cx).ignore_whitespace,
            dark: cx.tern_theme().is_dark(),
            cwd: cwd.clone(),
        };
        if self.loaded_key.as_ref() == Some(&key)
            || self.loading.as_ref().is_some_and(|(k, _)| *k == key)
        {
            return;
        }
        let detail = self.detail(cx);
        let (turn_refs, turn_run_id) = match (&self.selection, &detail) {
            (Selection::Turn { checkpoint_id }, Some(d)) => (
                data::checkpoint_refs(d, checkpoint_id),
                d.checkpoints
                    .iter()
                    .find(|c| &c.id == checkpoint_id)
                    .and_then(|c| c.run_id.clone()),
            ),
            _ => (None, None),
        };
        let request = Request {
            cwd: cwd.clone(),
            thread_id: self.thread_id.clone(),
            selection: self.selection.clone(),
            ignore_whitespace: key.ignore_whitespace,
            turn_refs,
            patches: detail
                .as_ref()
                .map(|d| data::recorded_patches(d))
                .unwrap_or_default(),
            turn_run_id,
            dark: key.dark,
        };
        let want_lists = self.selection.is_git() && cwd.is_dir();
        let base = match &self.selection {
            Selection::Branch { base_ref } => base_ref.clone(),
            _ => None,
        };
        let job = cx.background_spawn(async move {
            let loaded = data::load(&request);
            let lists = want_lists.then(|| {
                (
                    tern_git::review::branch_commits(&request.cwd, base.as_deref())
                        .unwrap_or_default(),
                    data::list_refs(&request.cwd),
                )
            });
            (loaded, lists)
        });
        let task_key = key.clone();
        let task = cx.spawn(async move |this, cx| {
            let (loaded, lists) = job.await;
            let _ = this.update(cx, |this, cx| {
                this.loaded = Some(Rc::new(loaded));
                this.loaded_key = Some(task_key);
                this.loading = None;
                this.last_load = Some(Instant::now());
                if let Some((commits, refs)) = lists {
                    this.commits = Rc::new(commits);
                    this.refs = Rc::new(refs);
                }
                this.rows_key = None;
                cx.notify();
            });
        });
        self.loading = Some((key, task));
    }

    fn section_id(&self) -> String {
        self.selection.section_id(&self.thread_id)
    }

    fn visible_files(&self, loaded: &Loaded, cx: &App) -> Vec<bool> {
        let hide = DiffPrefs::get(cx).hide_tests;
        loaded
            .files
            .iter()
            .map(|f| !(hide && is_test_file(&f.path)))
            .collect()
    }

    fn collapsed_flags(&self, loaded: &Loaded) -> Vec<bool> {
        let keys = file_keys(&loaded.files);
        let set = self.collapsed.get(&self.section_id());
        keys.iter()
            .map(|k| set.is_some_and(|s| s.contains(k)))
            .collect()
    }

    fn sync_rows(&mut self, cx: &App) {
        let Some(loaded) = self.loaded.clone() else {
            return;
        };
        let split = DiffPrefs::get(cx).split;
        let visible = self.visible_files(&loaded, cx);
        let collapsed = self.collapsed_flags(&loaded);
        let flags: Vec<bool> = visible
            .iter()
            .zip(&collapsed)
            .flat_map(|(v, c)| [*v, *c])
            .collect();
        let key = (Rc::as_ptr(&loaded) as usize, split, flags);
        if self.rows_key.as_ref() == Some(&key) {
            return;
        }
        let rows = build_rows(&loaded.files, split, |i| collapsed[i], |i| visible[i]);
        let old_top = self.list.logical_scroll_top();
        self.list.reset(rows.len());
        if self.rows_key.as_ref().is_some_and(|k| k.0 == key.0) {
            self.list.scroll_to(ListOffset {
                item_ix: old_top.item_ix.min(rows.len().saturating_sub(1)),
                offset_in_item: px(0.),
            });
        }
        self.rows = Rc::new(rows);
        self.rows_key = Some(key);
    }

    fn toggle_file(&mut self, file: usize, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded.clone() else {
            return;
        };
        let keys = file_keys(&loaded.files);
        let set = self.collapsed.entry(self.section_id()).or_default();
        if !set.remove(&keys[file]) {
            set.insert(keys[file].clone());
        }
        cx.notify();
    }

    fn all_collapsed(&self, cx: &App) -> bool {
        let Some(loaded) = &self.loaded else {
            return false;
        };
        let visible = self.visible_files(loaded, cx);
        let collapsed = self.collapsed_flags(loaded);
        let mut any = false;
        for (v, c) in visible.iter().zip(collapsed) {
            if *v {
                any = true;
                if !c {
                    return false;
                }
            }
        }
        any
    }

    /// "Collapse all files" / "Expand all files": visible files only.
    fn toggle_all(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded.clone() else {
            return;
        };
        let collapse = !self.all_collapsed(cx);
        let visible = self.visible_files(&loaded, cx);
        let keys = file_keys(&loaded.files);
        let set = self.collapsed.entry(self.section_id()).or_default();
        for (key, v) in keys.iter().zip(visible) {
            if v {
                if collapse {
                    set.insert(key.clone());
                } else {
                    set.remove(key);
                }
            }
        }
        cx.notify();
    }

    /// Reveal (spec 05 §2.9): un-collapse the file and scroll its header to the top.
    pub fn reveal(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded.clone() else {
            return;
        };
        let Some(file) = loaded.files.iter().position(|f| f.path == path) else {
            return;
        };
        let keys = file_keys(&loaded.files);
        if let Some(set) = self.collapsed.get_mut(&self.section_id()) {
            set.remove(&keys[file]);
        }
        self.tree_selected = Some(path.to_string());
        self.sync_rows(cx);
        if let Some(ix) = self.rows.iter().position(|r| *r == Row::Header { file }) {
            self.list.scroll_to(ListOffset {
                item_ix: ix,
                offset_in_item: px(0.),
            });
        }
        cx.notify();
    }

    fn copy_path(path: &str, cx: &mut App) {
        cx.write_to_clipboard(ClipboardItem::new_string(path.to_string()));
    }

    // --- header --------------------------------------------------------------

    fn scope_label(&self, cx: &App) -> String {
        match &self.selection {
            Selection::Branch { .. } => "Changes".into(),
            Selection::Uncommitted => "Uncommitted".into(),
            Selection::Commit { sha, subject } => {
                if subject.is_empty() {
                    format!("Commit {}", &sha[..sha.len().min(7)])
                } else {
                    subject.clone()
                }
            }
            Selection::ToolCall { .. } => "Tool call".into(),
            Selection::Turn { checkpoint_id } => {
                let turns = self.turns(cx);
                match turns.iter().position(|t| &t.checkpoint_id == checkpoint_id) {
                    Some(0) => "Latest turn".into(),
                    Some(i) => format!("Turn {}", turns[i].ordinal),
                    None => "Turn".into(),
                }
            }
        }
    }

    fn scope_menu(
        &self,
        cx: &mut Context<Self>,
    ) -> impl Fn(&mut Window, &mut App) -> Menu + 'static {
        let this = cx.entity().downgrade();
        let selection = self.selection.clone();
        let turns = self.turns(cx);
        let commits = self.commits.clone();
        let commits_loaded = self.loaded.is_some();
        move |_, _| {
            let pick = |sel: Selection| {
                let this = this.clone();
                move |_: &mut Window, cx: &mut App| {
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| this.select(sel.clone(), cx));
                    }
                }
            };
            let mut menu = Menu::new()
                .item(
                    MenuItem::radio("Changes", matches!(selection, Selection::Branch { .. }))
                        .on_select(pick(Selection::Branch { base_ref: None })),
                )
                .item(
                    MenuItem::radio("Uncommitted", selection == Selection::Uncommitted)
                        .on_select(pick(Selection::Uncommitted)),
                );
            let latest = turns.first().map(|t| t.checkpoint_id.clone());
            let latest_selected = matches!(&selection, Selection::Turn { checkpoint_id } if Some(checkpoint_id) == latest.as_ref());
            menu = menu.item(
                MenuItem::radio("Latest turn", latest_selected)
                    .disabled(latest.is_none())
                    .on_select(pick(Selection::Turn {
                        checkpoint_id: latest.clone().unwrap_or_default(),
                    })),
            );
            if !turns.is_empty() {
                let mut sub = Menu::new();
                for turn in &turns {
                    let checked = matches!(&selection, Selection::Turn { checkpoint_id } if *checkpoint_id == turn.checkpoint_id);
                    let time = turn
                        .completed_at
                        .with_timezone(&chrono::Local)
                        .format("%-I:%M %p")
                        .to_string();
                    sub = sub.item(
                        MenuItem::radio(format!("Turn {}", turn.ordinal), checked)
                            .shortcut(time)
                            .on_select(pick(Selection::Turn {
                                checkpoint_id: turn.checkpoint_id.clone(),
                            })),
                    );
                }
                menu = menu.item(MenuItem::submenu("Turn", None, sub));
            }
            let mut sub = Menu::new().min_width(px(240.));
            for commit in commits.iter() {
                let checked =
                    matches!(&selection, Selection::Commit { sha, .. } if *sha == commit.sha);
                let target = if checked {
                    Selection::Branch { base_ref: None }
                } else {
                    Selection::Commit {
                        sha: commit.sha.clone(),
                        subject: commit.subject.clone(),
                    }
                };
                sub = sub.item(
                    MenuItem::radio(commit.subject.clone(), checked)
                        .shortcut(commit.sha[..7.min(commit.sha.len())].to_string())
                        .on_select(pick(target)),
                );
            }
            let footer = if !commits_loaded {
                Some("Loading commits...".to_string())
            } else if commits.is_empty() {
                Some("No commits on this branch.".to_string())
            } else if commits.len() >= tern_git::review::REVIEW_BRANCH_COMMIT_LIMIT {
                Some(format!("Showing latest {} commits", commits.len()))
            } else {
                None
            };
            if let Some(footer) = footer {
                sub = sub.item(MenuItem::new(footer).disabled(true).on_select(|_, _| {}));
            }
            menu.item(MenuItem::submenu("Commits", None, sub))
        }
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let diff = cx.tern_theme().diff;
        let prefs = DiffPrefs::get(cx);
        let loaded = self.loaded.clone();
        let files = loaded.as_ref().map(|l| l.files.len()).unwrap_or(0);
        let is_git = self.selection.is_git();
        let label = self.scope_label(cx);
        let menu = self.scope_menu(cx);
        let this = cx.entity().downgrade();

        let scope = DropdownMenu::new("diff-scope", menu, move |open| {
            Button::new("diff-scope-trigger")
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Xs)
                .label(label)
                .icon_end(IconName::ChevronDown)
                .pressed(open)
                .into_any_element()
        })
        .align(Align::Start);

        let mut left = h_flex()
            .id("diff-refs")
            .flex_1()
            .min_w_0()
            .gap(px(12.))
            .child(scope);
        if let Selection::Commit { sha, .. } = &self.selection {
            left = left.child(
                div()
                    .font_family(font::MONO)
                    .text_scale(TextSize::Xs)
                    .text_color(t.muted_foreground)
                    .underline()
                    .child(sha[..sha.len().min(7)].to_string()),
            );
        }
        if matches!(self.selection, Selection::Branch { .. }) {
            let base_label = loaded
                .as_ref()
                .and_then(|l| l.base_ref.clone())
                .filter(|_| matches!(&self.selection, Selection::Branch { base_ref: Some(_) }))
                .unwrap_or_else(|| "Automatic".to_string());
            let head = loaded.as_ref().and_then(|l| l.head_ref.clone());
            let narrow = self.refs_width < px(320.);
            let compare: AnyElement = match head {
                Some(head) if !narrow => h_flex()
                    .id("diff-head")
                    .gap(px(8.))
                    .min_w_0()
                    .t3_tooltip(
                        Tooltip::text(format!("{head} → {base_label}")).side(TooltipSide::Top),
                    )
                    .child(div().max_w(px(192.)).truncate().child(head))
                    .child(Icon::new(IconName::ArrowRight).size(px(14.)).opacity(0.7))
                    .into_any_element(),
                _ => div().flex_none().child("vs").into_any_element(),
            };
            let refs = self.refs.clone();
            let query = self.ref_query.clone();
            let current = match &self.selection {
                Selection::Branch { base_ref } => base_ref.clone(),
                _ => None,
            };
            let this2 = this.clone();
            let picker = Popover::new("diff-base-ref", {
                let base_label = base_label.clone();
                move |open| {
                    Button::new("diff-base-ref-trigger")
                        .variant(ButtonVariant::GhostMuted)
                        .size(ButtonSize::Xs)
                        .label(base_label)
                        .icon_end(IconName::ChevronDown)
                        .pressed(open)
                        .into_any_element()
                }
            })
            .width(PopoverWidth::Px(288))
            .padding(PopoverPadding::None)
            .align(Align::Start)
            .content(move |window, cx| {
                base_ref_popup(&refs, &query, current.clone(), this2.clone(), window, cx)
            });
            left = left.child(
                h_flex()
                    .min_w_0()
                    .gap(px(8.))
                    .overflow_hidden()
                    .text_scale(TextSize::Xs)
                    .text_color(t.muted_foreground)
                    .child(compare)
                    .child(picker),
            );
        }
        // Measure the left group for the 320px "vs" breakpoint.
        let weak = this.clone();
        left = left.child(
            canvas(
                move |bounds, _, cx| {
                    if let Some(this) = weak.upgrade() {
                        this.update(cx, |this, cx| {
                            let was = this.refs_width < px(320.);
                            this.refs_width = bounds.size.width;
                            if was != (this.refs_width < px(320.)) {
                                cx.notify();
                            }
                        });
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        );
        let left = left.relative();

        let (additions, deletions) = loaded
            .as_ref()
            .map(|l| rows::totals(l.files.iter()))
            .unwrap_or((0, 0));
        let test_count = loaded
            .as_ref()
            .map(|l| l.files.iter().filter(|f| is_test_file(&f.path)).count())
            .unwrap_or(0);
        let all_collapsed = self.all_collapsed(cx);
        let refreshing = self.loading.is_some() && self.loaded.is_some();

        let icon14 = |icon| Icon::new(icon).size(px(14.)).opacity(0.8);
        let tip = |id: &'static str, text: String, child: AnyElement| {
            div()
                .id(id)
                .t3_tooltip(Tooltip::text(text).side(TooltipSide::Top))
                .child(child)
        };
        let weak = this.clone();
        let on = move |f: fn(&mut DiffView, &mut Context<DiffView>)| {
            let weak = weak.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                if let Some(this) = weak.upgrade() {
                    this.update(cx, f);
                }
            }
        };
        let right =
            h_flex()
                .flex_none()
                .gap(px(4.))
                .when(files > 0, |el| {
                    el.child(
                        div()
                            .id("diff-stat")
                            .mr(px(4.))
                            .t3_tooltip(Tooltip::text(format!(
                                "{additions} additions, {deletions} deletions"
                            )))
                            .child(
                                h_flex()
                                    .gap(px(4.))
                                    .font_family(font::MONO)
                                    .text_size(px(11.))
                                    .line_height(px(16.))
                                    .child(div().text_color(diff.diff_addition_foreground).child(
                                        format!("+{}", rows::compact_number(additions as u64)),
                                    ))
                                    .child(div().text_color(diff.diff_deletion_foreground).child(
                                        format!("-{}", rows::compact_number(deletions as u64)),
                                    )),
                            ),
                    )
                })
                .when(is_git, |el| {
                    el.child(tip(
                        "diff-refresh",
                        if refreshing {
                            "Refreshing diff…".into()
                        } else {
                            "Refresh diff".into()
                        },
                        Button::new("diff-refresh-btn")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::IconSm)
                            .icon(IconName::RefreshCw)
                            .icon_size(px(16.))
                            .on_click(on(|this, cx| this.refresh(cx)))
                            .into_any_element(),
                    ))
                })
                .when(files > 0, |el| {
                    el.child(tip(
                        "diff-collapse",
                        if all_collapsed {
                            "Expand all files".into()
                        } else {
                            "Collapse all files".into()
                        },
                        Button::new("diff-collapse-btn")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::IconSm)
                            .icon(if all_collapsed {
                                IconName::ChevronsUpDown
                            } else {
                                IconName::ChevronsDownUp
                            })
                            .icon_size(px(14.))
                            .on_click(on(|this, cx| this.toggle_all(cx)))
                            .into_any_element(),
                    ))
                })
                .child(
                    ToggleGroup::new()
                        .item(
                            Toggle::new("diff-stacked")
                                .pressed(!prefs.split)
                                .child(icon14(IconName::Rows3))
                                .on_click(|_, _, cx| DiffPrefs::update(cx, |p| p.split = false)),
                        )
                        .item(
                            Toggle::new("diff-split")
                                .pressed(prefs.split)
                                .child(icon14(IconName::Columns2))
                                .on_click(|_, _, cx| DiffPrefs::update(cx, |p| p.split = true)),
                        ),
                )
                .child(tip(
                    "diff-wrap",
                    if self.word_wrap {
                        "Disable line wrapping".into()
                    } else {
                        "Enable line wrapping".into()
                    },
                    Toggle::new("diff-wrap-toggle")
                        .variant(ToggleVariant::Ghost)
                        .size(ToggleSize::Sm)
                        .pressed(self.word_wrap)
                        .child(icon14(IconName::TextWrap))
                        .on_click(on(|this, cx| {
                            this.word_wrap = !this.word_wrap;
                            this.scroll_x = px(0.);
                            this.list.remeasure();
                            cx.notify();
                        }))
                        .into_any_element(),
                ))
                .child(tip(
                    "diff-ws",
                    if prefs.ignore_whitespace {
                        "Show whitespace changes".into()
                    } else {
                        "Hide whitespace changes".into()
                    },
                    Toggle::new("diff-ws-toggle")
                        .variant(ToggleVariant::Ghost)
                        .size(ToggleSize::Sm)
                        .pressed(prefs.ignore_whitespace)
                        .child(icon14(IconName::Pilcrow))
                        .on_click(|_, _, cx| {
                            DiffPrefs::update(cx, |p| p.ignore_whitespace = !p.ignore_whitespace)
                        })
                        .into_any_element(),
                ))
                .when(test_count > 0, |el| {
                    el.child(tip(
                        "diff-tests",
                        if prefs.hide_tests {
                            if test_count == 1 {
                                "Show 1 hidden test file".into()
                            } else {
                                format!("Show {test_count} hidden test files")
                            }
                        } else {
                            "Hide test files".into()
                        },
                        Toggle::new("diff-tests-toggle")
                            .variant(ToggleVariant::Ghost)
                            .size(ToggleSize::Sm)
                            .pressed(prefs.hide_tests)
                            .child(icon14(IconName::FlaskConicalOff))
                            .on_click(|_, _, cx| {
                                DiffPrefs::update(cx, |p| p.hide_tests = !p.hide_tests)
                            })
                            .into_any_element(),
                    ))
                })
                .when(files > 0, |el| {
                    el.child(tip(
                        "diff-tree",
                        if prefs.tree_open {
                            "Hide file tree".into()
                        } else {
                            "Show file tree".into()
                        },
                        Toggle::new("diff-tree-toggle")
                            .variant(ToggleVariant::Ghost)
                            .size(ToggleSize::Sm)
                            .pressed(prefs.tree_open)
                            .child(icon14(IconName::FolderTree))
                            .on_click(|_, _, cx| {
                                DiffPrefs::update(cx, |p| p.tree_open = !p.tree_open)
                            })
                            .into_any_element(),
                    ))
                });

        // Inline-panel variant of the header: 28px tall, 12px bottom margin, no border.
        h_flex()
            .flex_none()
            .h(px(28.))
            .mb(px(12.))
            .px(px(8.))
            .gap(px(8.))
            .justify_between()
            .child(left)
            .child(right)
            .into_any_element()
    }

    // --- body ----------------------------------------------------------------

    fn render_state(text: impl Into<SharedString>, cx: &App) -> AnyElement {
        let t = cx.tokens();
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .px(px(20.))
            .text_center()
            .text_scale(TextSize::Xs)
            .text_color(t.muted_foreground.opacity(0.7))
            .child(text.into())
            .into_any_element()
    }

    fn render_loading(&self, cx: &App) -> AnyElement {
        let t = cx.tokens();
        let header = |id: &'static str, width: f32| {
            h_flex()
                .h(px(32.))
                .px(px(8.))
                .pr(px(12.))
                .gap(px(8.))
                .border_b_1()
                .border_color(t.border.opacity(0.4))
                .child(
                    div()
                        .size(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Skeleton::new(SharedString::from(format!("{id}-a")))
                                .w(px(10.))
                                .h(px(10.)),
                        ),
                )
                .child(Skeleton::new(SharedString::from(format!("{id}-b"))).size(px(20.)))
                .child(
                    Skeleton::new(SharedString::from(format!("{id}-c")))
                        .h(px(12.))
                        .w(px(width)),
                )
                .child(div().flex_1())
                .child(
                    Skeleton::new(SharedString::from(format!("{id}-d")))
                        .h(px(12.))
                        .w(px(20.)),
                )
                .child(
                    Skeleton::new(SharedString::from(format!("{id}-e")))
                        .h(px(12.))
                        .w(px(20.)),
                )
        };
        let line = |id: &'static str, w: f32| {
            h_flex()
                .gap(px(8.))
                .child(
                    Skeleton::new(SharedString::from(format!("{id}-n")))
                        .h(px(10.))
                        .w(px(20.)),
                )
                .child(
                    Skeleton::new(SharedString::from(format!("{id}-t")))
                        .h(px(10.))
                        .w(gpui::relative(w)),
                )
        };
        v_flex()
            .id("diff-loading")
            .flex_1()
            .child(header("lh1", 220.))
            .child(
                h_flex()
                    .h(px(24.))
                    .px(px(12.))
                    .gap(px(8.))
                    .child(div().flex_1().h(px(1.)).bg(t.border.opacity(0.4)))
                    .child(Skeleton::new("lsep").h(px(10.)).w(px(96.)))
                    .child(div().flex_1().h(px(1.)).bg(t.border.opacity(0.4))),
            )
            .child(
                v_flex()
                    .px(px(12.))
                    .py(px(8.))
                    .gap(px(8.))
                    .child(line("l1", 0.6))
                    .child(line("l2", 0.67))
                    .child(line("l3", 0.8)),
            )
            .child(header("lh2", 160.))
            .child(header("lh3", 280.))
            .child(
                div()
                    .p(px(16.))
                    .text_scale(TextSize::Xs)
                    .text_color(t.muted_foreground)
                    .child(self.selection.loading_label()),
            )
            .into_any_element()
    }

    fn render_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(ix).copied() else {
            return div().into_any_element();
        };
        let Some(loaded) = self.loaded.clone() else {
            return div().into_any_element();
        };
        let theme = cx.tern_theme().clone();
        let colors = CodeColors::new(&theme, theme.colors.background);
        let metrics = CodeMetrics {
            ch: code::char_width(window),
            wrap: self.word_wrap,
            scroll_x: self.scroll_x,
        };
        let file = &loaded.files[row.file()];
        let gutter_w = metrics.ch * rows::gutter_chars(file) as f32;
        let el = match row {
            Row::Header { file: fi } => self.render_file_header(fi, colors, cx),
            Row::Gap {
                file,
                before,
                lines,
            } => code::separator(
                SharedString::from(format!("gap-{file}-{before}")),
                lines.label(),
                colors,
            ),
            Row::Line {
                file: fi,
                hunk,
                line,
            } => {
                let l = &file.hunks[hunk].lines[line];
                let spans = &loaded.spans[fi][hunk][line];
                code::unified_line(
                    SharedString::from(format!("l-{ix}")),
                    l,
                    spans,
                    gutter_w,
                    colors,
                    metrics,
                )
            }
            Row::Pair {
                file: fi,
                hunk,
                left,
                right,
            } => {
                let h = &file.hunks[hunk];
                let side = |i: Option<usize>| {
                    i.map(|i| (&h.lines[i], loaded.spans[fi][hunk][i].as_slice()))
                };
                code::split_line(side(left), side(right), gutter_w, colors, metrics)
            }
            Row::Footer { .. } => div().h(code::FOOTER_HEIGHT).into_any_element(),
        };
        div()
            .w_full()
            .font_family(font::MONO)
            .text_size(code::CODE_FONT_SIZE)
            .child(el)
            .into_any_element()
    }

    fn render_file_header(
        &self,
        fi: usize,
        colors: CodeColors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(loaded) = self.loaded.clone() else {
            return div().into_any_element();
        };
        let file = &loaded.files[fi];
        let collapsed = self.collapsed_flags(&loaded)[fi];
        let path = file.path.clone();
        let this = cx.entity().downgrade();
        let toggle = {
            let this = this.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| this.toggle_file(fi, cx));
                }
            }
        };
        let open = {
            let this = this.clone();
            let path = path.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                cx.stop_propagation();
                if let Some(this) = this.upgrade() {
                    this.update(cx, |_, cx| cx.emit(DiffEvent::OpenFile(path.clone())));
                }
            }
        };
        let copy_path = path.clone();
        let group: SharedString = format!("diff-header-{fi}").into();
        let title = div()
            .id(SharedString::from(format!("diff-title-{fi}")))
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .cursor_pointer()
            .text_color(colors.fg)
            .hover(move |s| s.text_color(colors.title_hover))
            .on_click(open)
            .child(truncate_start(&file.path, 64));
        h_flex()
            .id(SharedString::from(format!("diff-header-row-{fi}")))
            .group(group.clone())
            .relative()
            .w_full()
            .h(code::HEADER_HEIGHT)
            .pl(px(8.))
            .pr(px(12.))
            .bg(colors.bg)
            .font_family(font::SANS)
            .text_size(px(12.))
            .line_height(px(16.))
            .cursor_pointer()
            .on_click(toggle.clone())
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(px(3.))
                    .invisible()
                    .group_hover(group.clone(), |s| s.visible())
                    .bg(colors.header_hover_bar),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(8.))
                    .child(
                        div()
                            .id(SharedString::from(format!("diff-chevron-{fi}")))
                            .t3_tooltip(
                                Tooltip::text(if collapsed {
                                    "Expand diff"
                                } else {
                                    "Collapse diff"
                                })
                                .side(TooltipSide::Top),
                            )
                            .ml(px(-2.))
                            .child(
                                Button::new(SharedString::from(format!("diff-chevron-btn-{fi}")))
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::IconMicro)
                                    .icon(if collapsed {
                                        IconName::ChevronRight
                                    } else {
                                        IconName::ChevronDown
                                    })
                                    .icon_size(px(16.))
                                    .on_click(toggle),
                            ),
                    )
                    .child(code::change_icon(file, colors))
                    .when_some(
                        file.old_path
                            .clone()
                            .filter(|_| file.kind == DiffFile::Renamed),
                        |el, old| {
                            el.child(
                                div()
                                    .opacity(0.7)
                                    .whitespace_nowrap()
                                    .text_color(colors.fg)
                                    .child(old),
                            )
                            .child(
                                Icon::new(IconName::DiffRenameArrow)
                                    .size(px(16.))
                                    .color(colors.muted),
                            )
                        },
                    )
                    .child(title)
                    .child(
                        div()
                            .id(SharedString::from(format!("diff-copy-{fi}")))
                            .invisible()
                            .group_hover(group, |s| s.visible())
                            .t3_tooltip(Tooltip::text("Copy path").side(TooltipSide::Top))
                            .child(
                                Button::new(SharedString::from(format!("diff-copy-btn-{fi}")))
                                    .variant(ButtonVariant::GhostMuted)
                                    .size(ButtonSize::IconMicro)
                                    .icon(IconName::Copy)
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        Self::copy_path(&copy_path, cx);
                                    }),
                            ),
                    ),
            )
            .child(code::header_counts(
                file.additions,
                file.deletions,
                colors,
                cx,
            ))
            .into_any_element()
    }

    /// The sticky copy of the header for the file at the top of the viewport.
    fn render_sticky_header(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let top = self.list.logical_scroll_top();
        let row = self.rows.get(top.item_ix)?;
        if matches!(row, Row::Header { .. }) && top.offset_in_item <= px(0.) {
            return None;
        }
        let theme = cx.tern_theme().clone();
        let colors = CodeColors::new(&theme, theme.colors.background);
        let file = row.file();
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .child(self.render_file_header(file, colors, cx))
                .into_any_element(),
        )
    }

    fn render_code(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let sticky = self.render_sticky_header(cx);
        let wrap = self.word_wrap;
        let list = list(
            self.list.clone(),
            cx.processor(|this, ix, window, cx| this.render_row(ix, window, cx)),
        )
        .size_full();
        div()
            .id("diff-code")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .when(!wrap, |el| {
                el.on_scroll(cx.listener(|this, e: &ScrollEvent, window, cx| {
                    let delta = e.delta.pixel_delta(window.line_height());
                    let dx = if e.modifiers.shift && delta.x == px(0.) {
                        delta.y
                    } else {
                        delta.x
                    };
                    if dx != px(0.) {
                        this.scroll_x = (this.scroll_x - dx).max(px(0.));
                        cx.notify();
                    }
                }))
            })
            .child(list)
            .children(sticky)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .bottom_0()
                    .w(px(12.))
                    .child(Scrollbar::vertical(&self.list)),
            )
            .into_any_element()
    }

    fn render_tree(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let loaded = self.loaded.clone()?;
        let t = *cx.tokens();
        let dark = cx.tern_theme().is_dark();
        let visible = self.visible_files(&loaded, cx);
        let entries: Vec<(String, Option<DiffFile>)> = loaded
            .files
            .iter()
            .zip(&visible)
            .filter(|(_, v)| **v)
            .map(|(f, _)| (f.path.clone(), Some(f.kind)))
            .collect();
        let tree = FileTree::build(&entries, TreeOrder::Input);
        let rows: Vec<TreeRow> = tree.rows(&self.tree_collapsed);
        let has_dirs = tree.has_dirs();
        let any_open = tree.dirs().iter().any(|d| !self.tree_collapsed.contains(d));
        let width = DiffPrefs::get(cx)
            .tree_width
            .unwrap_or_else(|| px(256f32.min(self.body_width.as_f32() * 0.4).max(TREE_MIN)));
        let max = px(TREE_MAX.min(self.body_width.as_f32() - 256.).max(TREE_MIN));
        let width = width.clamp(px(TREE_MIN), max);
        let this = cx.entity().downgrade();
        let selected = self.tree_selected.clone();
        let count = entries.len();
        let collapse_all = {
            let this = this.clone();
            let dirs = tree.dirs();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        if any_open {
                            this.tree_collapsed.extend(dirs.iter().cloned());
                        } else {
                            this.tree_collapsed.clear();
                        }
                        cx.notify();
                    });
                }
            }
        };
        let list = rows.into_iter().enumerate().map(|(i, row)| {
            let this = this.clone();
            let path = row.path.clone();
            let is_dir = row.is_dir;
            tree_view::row(
                SharedString::from(format!("diff-tree-{i}")),
                &row,
                selected.as_deref() == Some(row.path.as_str()),
                dark,
                cx,
            )
            .on_click(move |_, _, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        if is_dir {
                            if !this.tree_collapsed.remove(&path) {
                                this.tree_collapsed.insert(path.clone());
                            }
                            cx.notify();
                        } else {
                            this.reveal(&path, cx);
                        }
                    });
                }
            })
            .into_any_element()
        });
        let handle = div()
            .id("diff-tree-resize")
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(-2.))
            .w(px(4.))
            .cursor_col_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                    this.tree_drag = Some((e.position.x, width));
                    cx.stop_propagation();
                }),
            );
        Some(
            v_flex()
                .id("diff-tree")
                .relative()
                .flex_none()
                .w(width)
                .h_full()
                .border_l_1()
                .border_color(t.border.opacity(0.6))
                .bg(t.background)
                .child(
                    h_flex()
                        .flex_none()
                        .h(px(40.))
                        .px(px(8.))
                        .gap(px(4.))
                        .border_b_1()
                        .border_color(t.border.opacity(0.6))
                        .text_scale(TextSize::Xs)
                        .text_color(t.muted_foreground)
                        .child(
                            div()
                                .px(px(4.))
                                .font_medium()
                                .text_color(t.foreground)
                                .child("Files"),
                        )
                        .child(div().flex_1())
                        .child(count.to_string())
                        .when(has_dirs, |el| {
                            el.child(
                                Button::new("diff-tree-collapse")
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::IconXs)
                                    .icon(if any_open {
                                        IconName::ChevronsDownUp
                                    } else {
                                        IconName::ChevronsUpDown
                                    })
                                    .tooltip(Tooltip::text(if any_open {
                                        "Collapse all folders"
                                    } else {
                                        "Expand all folders"
                                    }))
                                    .on_click(collapse_all),
                            )
                        }),
                )
                .child(
                    v_flex()
                        .id("diff-tree-list")
                        .flex_1()
                        .p(px(8.))
                        .gap(px(0.))
                        .overflow_y_scroll()
                        .children(list),
                )
                .child(handle)
                .into_any_element(),
        )
    }
}

/// The base-ref combobox (spec 05 §2.3): search, "Automatic", then one row per
/// branch with a local/remote switch column.
fn base_ref_popup(
    refs: &[(String, bool, bool)],
    query: &Entity<InputState>,
    current: Option<String>,
    this: gpui::WeakEntity<DiffView>,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let t = *cx.tokens();
    let q = query.read(cx).value().to_lowercase();
    let pick = |value: Option<String>| {
        let this = this.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            crate::ui::popover::close_popups(window, cx);
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| this.set_base_ref(value.clone(), cx));
            }
        }
    };
    let row = |id: SharedString, label: String, selected: bool, right: Option<AnyElement>| {
        h_flex()
            .id(id)
            .h(px(28.))
            .px(px(8.))
            .gap(px(8.))
            .rounded(px(4.))
            .text_scale(TextSize::Sm)
            .cursor_pointer()
            .hover(move |s| s.bg(t.accent))
            .child(
                div()
                    .w(px(16.))
                    .flex_none()
                    .children(selected.then(|| Icon::new(IconName::Check).size(px(14.)))),
            )
            .child(div().flex_1().min_w_0().truncate().child(label))
            .child(div().w(px(32.)).flex().justify_center().children(right))
    };
    let mut rows: Vec<AnyElement> = Vec::new();
    if q.is_empty() {
        rows.push(
            row(
                "ref-auto".into(),
                "Automatic".into(),
                current.is_none(),
                None,
            )
            .on_click(pick(None))
            .into_any_element(),
        );
    }
    for (i, (name, local, remote)) in refs.iter().enumerate() {
        if !q.is_empty() && !name.to_lowercase().contains(&q) {
            continue;
        }
        let selected = current.as_deref() == Some(name.as_str())
            || current.as_deref() == Some(&format!("origin/{name}"));
        let right = match (local, remote) {
            (true, true) => Some(
                crate::ui::Switch::new(
                    SharedString::from(format!("ref-sw-{i}")),
                    current.as_deref() == Some(&format!("origin/{name}")),
                )
                .size(crate::ui::SwitchSize::Sm)
                .on_change({
                    let this = this.clone();
                    let name = name.clone();
                    move |on: bool, _: &mut Window, cx: &mut App| {
                        if let Some(this) = this.upgrade() {
                            let value = if on {
                                format!("origin/{name}")
                            } else {
                                name.clone()
                            };
                            this.update(cx, |this, cx| this.set_base_ref(Some(value), cx));
                        }
                    }
                })
                .into_any_element(),
            ),
            (false, true) => Some(
                div()
                    .id(SharedString::from(format!("ref-remote-{i}")))
                    .t3_tooltip(Tooltip::text("Remote only"))
                    .child(Icon::new(IconName::Check).size(px(12.)))
                    .into_any_element(),
            ),
            _ => None,
        };
        let value = if *local {
            name.clone()
        } else {
            format!("origin/{name}")
        };
        rows.push(
            row(
                SharedString::from(format!("ref-{i}")),
                name.clone(),
                selected,
                right,
            )
            .on_click(pick(Some(value)))
            .into_any_element(),
        );
    }
    let empty = rows.is_empty();
    v_flex()
        .w_full()
        .child(
            div()
                .p(px(4.))
                .child(crate::ui::TextField::new(query).size(crate::ui::InputSize::Sm)),
        )
        .child(
            h_flex()
                .border_b_1()
                .border_color(t.border.opacity(0.7))
                .pl(px(12.))
                .pr(px(26.))
                .pt(px(8.))
                .pb(px(6.))
                .text_size(px(10.))
                .line_height(px(14.))
                .font_medium()
                .text_color(t.muted_foreground)
                .child(div().w(px(16.)))
                .child(div().flex_1().child("BRANCH"))
                .child(div().w(px(32.)).flex().justify_center().child("REMOTE")),
        )
        .child(
            v_flex()
                .id("ref-list")
                .max_h(px(256.))
                .overflow_y_scroll()
                .p(px(4.))
                .children(rows)
                .when(empty, |el| {
                    el.child(
                        div()
                            .py(px(16.))
                            .text_center()
                            .text_scale(TextSize::Xs)
                            .text_color(t.muted_foreground)
                            .child("No matching refs."),
                    )
                }),
        )
        .into_any_element()
}

/// Keeps the end of a long path ("…/src/main.rs"), as Pierre's `direction: rtl`
/// ellipsis does.
pub fn truncate_start(path: &str, max_chars: usize) -> String {
    let count = path.chars().count();
    if count <= max_chars {
        return path.to_string();
    }
    let tail: String = path.chars().skip(count - (max_chars - 1)).collect();
    format!("…{tail}")
}

impl Render for DiffView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        self.ensure_loaded(cx);
        self.sync_rows(cx);
        let header = self.render_header(cx);
        let detail = self.detail(cx);
        let loaded = self.loaded.clone();
        let mut banners: Vec<AnyElement> = Vec::new();
        let banner = |text: String| {
            div()
                .flex_none()
                .border_b_1()
                .border_color(t.border.opacity(0.7))
                .bg(t.muted.opacity(0.4))
                .px(px(12.))
                .py(px(6.))
                .text_scale(TextSize::Xs2)
                .text_color(t.muted_foreground)
                .child(text)
                .into_any_element()
        };
        if let Some(l) = &loaded {
            if l.incomplete {
                banners.push(banner(
                    "Showing tracked changes. Untracked changes could not be loaded. Refresh to retry."
                        .into(),
                ));
            }
            if l.truncated {
                banners.push(banner(
                    "This preview exceeds the size limit. Changes shown are incomplete.".into(),
                ));
            }
        }
        let no_turns = matches!(self.selection, Selection::Turn { .. })
            && detail.as_ref().is_some_and(|d| data::turns(d).is_empty());
        let body: AnyElement = if detail.is_none() {
            Self::render_state("Select a thread to inspect turn diffs.", cx)
        } else if no_turns {
            Self::render_state("No completed turns yet.", cx)
        } else if let Some(l) = loaded.as_ref().filter(|_| {
            self.loaded_key
                .as_ref()
                .is_some_and(|k| k.selection == self.selection)
                || self.loading.is_none()
        }) {
            if let Some(err) = l.error.as_ref().filter(|_| l.files.is_empty()) {
                let not_repo = err.contains("not a git repository");
                if not_repo {
                    Self::render_state(
                        "Turn diffs are unavailable because this project is not a git repository.",
                        cx,
                    )
                } else {
                    div()
                        .px(px(12.))
                        .pt(px(8.))
                        .text_scale(TextSize::Xs2)
                        .text_color(t.error.opacity(0.8))
                        .child(err.clone())
                        .into_any_element()
                }
            } else if let Some(raw) = &l.raw {
                v_flex()
                    .id("diff-raw")
                    .p(px(8.))
                    .gap(px(8.))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .text_scale(TextSize::Xs2)
                            .text_color(t.muted_foreground.opacity(0.75))
                            .child("Failed to parse patch. Showing raw patch."),
                    )
                    .child(
                        div()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(t.border.opacity(0.7))
                            .bg(t.background.opacity(0.7))
                            .p(px(12.))
                            .font_family(font::MONO)
                            .text_size(px(11.))
                            .text_color(t.muted_foreground.opacity(0.9))
                            .child(raw.clone()),
                    )
                    .into_any_element()
            } else if l.files.is_empty() {
                Self::render_state("No net changes in this selection.", cx)
            } else {
                let tree = DiffPrefs::get(cx)
                    .tree_open
                    .then(|| self.render_tree(cx))
                    .flatten();
                let weak = cx.entity().downgrade();
                h_flex()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                if let Some(this) = weak.upgrade() {
                                    this.update(cx, |this, _| this.body_width = bounds.size.width);
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(self.render_code(cx))
                    .children(tree)
                    .into_any_element()
            }
        } else {
            self.render_loading(cx)
        };
        let _ = window;
        v_flex()
            .id("diff-panel")
            .size_full()
            .min_w_0()
            .bg(t.background)
            .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, cx| {
                if let Some((start_x, start_w)) = this.tree_drag {
                    if !e.dragging() {
                        this.tree_drag = None;
                        return;
                    }
                    let w = (start_w + (start_x - e.position.x)).max(px(TREE_MIN));
                    DiffPrefs::update(cx, |p| p.tree_width = Some(w));
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.tree_drag = None),
            )
            .child(header)
            .children(banners)
            .child(v_flex().flex_1().min_h_0().child(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_from_the_start() {
        assert_eq!(truncate_start("a/b.rs", 10), "a/b.rs");
        assert_eq!(truncate_start("crates/app/src/main.rs", 10), "…c/main.rs");
    }

    #[test]
    fn prefs_defaults_match_t3() {
        let p = DiffPrefs::default();
        assert!(!p.split && p.ignore_whitespace && !p.tree_open && !p.hide_tests);
    }
}

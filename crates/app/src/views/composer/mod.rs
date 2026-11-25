//! The composer (spec 04): the prompt card under the chat timeline, and the
//! centered card on the new-thread screen (`views::draft`). One entity for the
//! window; switching threads swaps the draft in place.
//!
//! - `model`, `permissions`, `picker`: the model button, effort, runtime mode.
//! - `text`, `list`: triggers, slash commands, chips, list continuation.
//! - `file_index`: the `@` mention index. `drafts`: per-thread drafts on disk.
//! - `pending`: approvals, questions, queued runs. `render`, `attach`: the card.

pub mod attach;
pub mod drafts;
pub mod file_index;
pub mod list;
pub mod model;
pub mod pending;
pub mod permissions;
pub mod picker;
mod render;
pub mod text;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, Focusable as _, KeyBinding, Pixels, Task,
    Window, actions,
};
use gpui_component::input::{InlineToken, InputContent, InputEvent};
use tern_core::{
    Command, DispatchMode, InteractionMode, ModelSelection, OutgoingMessage, RuntimeMode,
    ThreadEnvMode, WorkspaceStrategy,
};

use crate::app_state::{AppState, Route};
use crate::store::{Store, StoreEvent};
use crate::ui::input::TextareaState;
use crate::ui::popup::{PopupAnchor, PopupLayer};
use drafts::{Draft, DraftAttachment, DraftKey, DraftStore};
use file_index::FileIndex;
use pending::Answer;
use text::{ChipKind, SlashCommand, Trigger, TriggerKind};

actions!(
    composer,
    [ToggleModelPicker, OpenEffortPicker, OpenPermissions,]
);

/// Composer shortcuts (spec 04 §6), in the shell's `Workspace` context so they
/// work wherever focus is.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-shift-m", ToggleModelPicker, Some("Workspace")),
        KeyBinding::new("ctrl-shift-e", OpenEffortPicker, Some("Workspace")),
        KeyBinding::new("ctrl-shift-a", OpenPermissions, Some("Workspace")),
    ]);
}

/// Something to open on the next render (keyboard shortcuts arrive without a window).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingOpen {
    Picker { effort: bool },
    Permissions,
}

/// One row of the slash / mention menu.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem {
    Command(SlashCommand),
    File(file_index::Entry),
}

#[derive(Clone, Debug)]
pub struct MenuState {
    pub trigger: Trigger,
    pub items: Vec<MenuItem>,
    pub highlight: usize,
    /// The index for this project is still being built.
    pub loading: bool,
}

/// ↑ history browsing.
#[derive(Clone, Debug)]
struct HistoryBrowse {
    entries: Vec<String>,
    /// Index into `entries` of the shown entry.
    pos: usize,
}

enum IndexState {
    Building,
    Ready(Arc<FileIndex>),
}

/// The pending question panel's state.
#[derive(Clone, Debug, Default)]
pub struct QuestionState {
    pub request_id: String,
    pub page: usize,
    pub answers: HashMap<String, Answer>,
    pub collapsed: bool,
}

pub struct ComposerView {
    app: Entity<AppState>,
    store: Entity<Store>,
    pub(crate) editor: Entity<TextareaState>,
    pub(crate) drafts: DraftStore,
    key: Option<DraftKey>,
    attachments: Vec<DraftAttachment>,
    menu: Option<MenuState>,
    /// A trigger dismissed with Escape stays closed until a new one is typed.
    dismissed_trigger: Option<usize>,
    indexes: HashMap<String, IndexState>,
    history: Option<HistoryBrowse>,
    stopping: Option<Instant>,
    question: QuestionState,
    queue_expanded: bool,
    error: Option<String>,
    /// Programmatic edits in flight (ignore their change events for history).
    applying: bool,
    pending_open: Option<PendingOpen>,
    save_task: Option<Task<()>>,
    pub(crate) card_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) model_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) permission_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl ComposerView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        bind_keys(cx);
        let app = AppState::global(cx);
        let store = Store::global(cx);
        let editor = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 9)
                .placeholder("")
        });
        cx.subscribe_in(&editor, window, Self::on_editor_event)
            .detach();
        cx.observe_in(&app, window, |this, _, window, cx| {
            this.sync_target(window, cx);
            cx.notify();
        })
        .detach();
        cx.subscribe_in(&store, window, |this, _, event: &StoreEvent, window, cx| {
            match event {
                StoreEvent::Thread(id)
                    if this.key.as_ref().and_then(|k| k.thread_id()) == Some(id) =>
                {
                    this.on_thread_changed(cx);
                }
                StoreEvent::Snapshot => this.sync_target(window, cx),
                _ => {}
            }
            cx.notify();
        })
        .detach();
        let weak = cx.entity().downgrade();
        App::on_action(cx, move |action: &ToggleModelPicker, cx: &mut App| {
            let _ = action;
            weak.update(cx, |this, cx| {
                this.pending_open = Some(PendingOpen::Picker { effort: false });
                cx.notify();
            })
            .ok();
        });
        let weak = cx.entity().downgrade();
        App::on_action(cx, move |_: &OpenEffortPicker, cx: &mut App| {
            weak.update(cx, |this, cx| {
                this.pending_open = Some(PendingOpen::Picker { effort: true });
                cx.notify();
            })
            .ok();
        });
        let weak = cx.entity().downgrade();
        App::on_action(cx, move |_: &OpenPermissions, cx: &mut App| {
            weak.update(cx, |this, cx| {
                this.pending_open = Some(PendingOpen::Permissions);
                cx.notify();
            })
            .ok();
        });

        let demo_or_fixture =
            store.read(cx).is_fixture() || std::env::args().any(|a| a == "--demo");
        let drafts = if demo_or_fixture {
            DraftStore::ephemeral()
        } else {
            DraftStore::load(&tern_engine::Paths::from_env().userdata())
        };
        let mut this = Self {
            app,
            store,
            editor,
            drafts,
            key: None,
            attachments: Vec::new(),
            menu: None,
            dismissed_trigger: None,
            indexes: HashMap::new(),
            history: None,
            stopping: None,
            question: QuestionState::default(),
            queue_expanded: false,
            error: None,
            applying: false,
            pending_open: None,
            save_task: None,
            card_bounds: Rc::default(),
            model_bounds: Rc::default(),
            permission_bounds: Rc::default(),
        };
        this.sync_target(window, cx);
        this.editor.read(cx).focus_handle(cx).focus(window, cx);
        this
    }

    // --- target --------------------------------------------------------------

    /// The draft the current route shows.
    fn route_key(&self, cx: &App) -> Option<DraftKey> {
        let app = self.app.read(cx);
        let store = self.store.read(cx);
        match app.chat_route() {
            Route::Thread(id) => Some(DraftKey::Thread(id.clone())),
            Route::Draft(project) => Some(DraftKey::New(project.clone())),
            Route::Index => most_recent_project(store.snapshot()).map(DraftKey::New),
            _ => None,
        }
    }

    /// Loads the route's draft into the editor when the route changed.
    fn sync_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.route_key(cx);
        if key == self.key {
            return;
        }
        self.store_editor_into_draft(cx);
        self.key = key.clone();
        self.menu = None;
        self.history = None;
        self.stopping = None;
        self.error = None;
        self.question = QuestionState::default();
        let draft = key.as_ref().map(|k| self.drafts.get(k)).unwrap_or_default();
        self.attachments = draft.attachments.clone();
        self.set_editor_text(&draft.prompt, draft.prompt.len(), window, cx);
        self.on_thread_changed(cx);
        if let Some(DraftKey::New(project)) = &key {
            self.ensure_index(project.clone(), cx);
        } else if let Some(DraftKey::Thread(id)) = &key
            && let Some(p) = self
                .store
                .read(cx)
                .snapshot()
                .thread(id)
                .map(|t| t.thread.project_id.clone())
        {
            self.ensure_index(p, cx);
        }
        if key.is_some() {
            self.editor.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    fn store_editor_into_draft(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.key.clone() else {
            return;
        };
        let prompt = self.editor.read(cx).value().to_string();
        let attachments = self.attachments.clone();
        self.drafts.update(&key, |d| {
            d.prompt = prompt;
            d.attachments = attachments;
        });
        self.schedule_save(cx);
    }

    fn on_thread_changed(&mut self, cx: &mut Context<Self>) {
        let Some(detail) = self.thread_detail(cx) else {
            return;
        };
        match pending::pending_question(&detail) {
            Some(q) if q.request_id != self.question.request_id => {
                let mut answers = HashMap::new();
                for question in &q.questions {
                    if let Some(initial) = &question.initial_answer
                        && let Some(i) = question.options.iter().position(|o| &o.label == initial)
                    {
                        answers.insert(
                            question.id.clone(),
                            Answer {
                                selected: vec![i],
                                custom: String::new(),
                            },
                        );
                    }
                }
                self.question = QuestionState {
                    request_id: q.request_id,
                    page: 0,
                    answers,
                    collapsed: false,
                };
            }
            None if !self.question.request_id.is_empty() => {
                self.question = QuestionState::default();
            }
            _ => {}
        }
        if let Some(at) = self.stopping
            && (pending::live_run(&detail).is_none() || at.elapsed() > Duration::from_secs(10))
        {
            self.stopping = None;
        }
    }

    pub(crate) fn thread_id(&self) -> Option<&str> {
        self.key.as_ref().and_then(|k| k.thread_id())
    }

    pub(crate) fn thread_detail(&self, cx: &App) -> Option<Rc<tern_core::ThreadDetail>> {
        self.store.read(cx).thread(self.thread_id()?)
    }

    /// The project a new-thread draft targets.
    pub fn draft_project(&self) -> Option<&str> {
        match &self.key {
            Some(DraftKey::New(p)) => Some(p),
            _ => None,
        }
    }

    pub(crate) fn current_draft(&self) -> Draft {
        self.key
            .as_ref()
            .map(|k| self.drafts.get(k))
            .unwrap_or_default()
    }

    // --- model, mode -----------------------------------------------------------

    /// The selection the next send uses.
    pub fn selection(&self, cx: &App) -> Option<ModelSelection> {
        let store = self.store.read(cx);
        let snapshot = store.snapshot();
        let draft = self.current_draft();
        let preferred = match &self.key {
            Some(DraftKey::Thread(id)) => draft.model_selection.or_else(|| {
                snapshot
                    .thread(id)
                    .map(|t| t.thread.model_selection.clone())
            }),
            Some(DraftKey::New(project)) => draft
                .model_selection
                .or_else(|| self.drafts.sticky().cloned())
                .or_else(|| {
                    snapshot
                        .project(project)
                        .and_then(|p| p.default_model_selection.clone())
                }),
            None => self.drafts.sticky().cloned(),
        };
        if let Some(DraftKey::Thread(_)) = &self.key
            && let Some(sel) = &preferred
        {
            // An existing thread keeps its model even when the catalog lacks it.
            if model::find_model(&snapshot.providers, sel).is_none() {
                return Some(sel.clone());
            }
        }
        model::resolve_selection(&snapshot.providers, preferred.as_ref())
    }

    /// An existing thread can only switch models within its provider.
    pub fn locked_instance(&self, cx: &App) -> Option<String> {
        let id = self.thread_id()?;
        self.store
            .read(cx)
            .snapshot()
            .thread(id)
            .map(|t| t.thread.model_selection.instance_id.clone())
    }

    pub fn set_selection(&mut self, sel: ModelSelection, cx: &mut Context<Self>) {
        self.drafts.remember_pick(&sel);
        if let Some(key) = self.key.clone() {
            self.drafts.update(&key, |d| d.model_selection = Some(sel));
        }
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn toggle_favorite(&mut self, instance: &str, slug: &str, cx: &mut Context<Self>) {
        self.drafts.toggle_favorite(instance, slug);
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn runtime_mode(&self, cx: &App) -> RuntimeMode {
        match &self.key {
            Some(DraftKey::Thread(id)) => self
                .store
                .read(cx)
                .snapshot()
                .thread(id)
                .map(|t| t.thread.runtime_mode)
                .unwrap_or(permissions::DEFAULT_MODE),
            Some(key) => self
                .drafts
                .get(key)
                .runtime_mode
                .unwrap_or(permissions::DEFAULT_MODE),
            None => permissions::DEFAULT_MODE,
        }
    }

    pub fn set_runtime_mode(&mut self, mode: RuntimeMode, cx: &mut Context<Self>) {
        match self.key.clone() {
            Some(DraftKey::Thread(id)) => {
                self.store.read(cx).send(Command::SetRuntimeMode {
                    thread_id: id,
                    runtime_mode: mode,
                });
            }
            Some(key) => {
                self.drafts.update(&key, |d| d.runtime_mode = Some(mode));
                self.schedule_save(cx);
            }
            None => {}
        }
        cx.notify();
    }

    pub fn env_mode(&self) -> ThreadEnvMode {
        self.current_draft().env_mode.unwrap_or_default()
    }

    pub fn set_env_mode(&mut self, mode: ThreadEnvMode, cx: &mut Context<Self>) {
        if let Some(key) = self.key.clone() {
            self.drafts.update(&key, |d| d.env_mode = Some(mode));
            self.schedule_save(cx);
        }
        cx.notify();
    }

    pub fn branch(&self) -> Option<String> {
        self.current_draft().branch
    }

    pub fn set_branch(&mut self, branch: String, cx: &mut Context<Self>) {
        if let Some(key) = self.key.clone() {
            self.drafts.update(&key, |d| d.branch = Some(branch));
            self.schedule_save(cx);
        }
        cx.notify();
    }

    // --- persistence -------------------------------------------------------------

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        if !self.drafts.persists() {
            return;
        }
        self.save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let Ok(Some((path, bytes))) = this.update(cx, |this, _| this.drafts.snapshot()) else {
                return;
            };
            cx.background_spawn(async move {
                if let Err(e) = drafts::write_file(&path, &bytes) {
                    eprintln!("tern: saving composer drafts failed: {e}");
                }
            })
            .await;
        }));
    }

    // --- editor ------------------------------------------------------------------

    pub(crate) fn text(&self, cx: &App) -> String {
        self.editor.read(cx).value().to_string()
    }

    /// Replaces the whole prompt (no undo entry) and puts the caret at `caret`.
    fn set_editor_text(
        &mut self,
        text: &str,
        caret: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.applying = true;
        let content = self.content_with_chips(text);
        self.editor.update(cx, |state, cx| {
            state.set_value(content, window, cx);
            let caret = caret.min(text.len());
            state.set_selected_range(caret..caret, cx);
        });
        self.applying = false;
    }

    /// Replaces `range` with `replacement` as one undoable edit.
    fn replace_in_editor(
        &mut self,
        range: std::ops::Range<usize>,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |state, cx| {
            state.set_selected_range(range, cx);
            state.replace(replacement.to_string(), window, cx);
        });
    }

    fn content_with_chips(&self, text: &str) -> InputContent {
        let mut content = InputContent::new(text.to_string());
        for span in text::chip_spans(text, &[]) {
            let label = match &span.kind {
                ChipKind::File { label, .. } => label.clone(),
                ChipKind::Skill { name } => name.clone(),
            };
            let id = match &span.kind {
                ChipKind::File { path, .. } => format!("file:{path}"),
                ChipKind::Skill { name } => format!("skill:{name}"),
            };
            let token =
                InlineToken::new(id, text[span.range.clone()].to_string()).with_label(label);
            match content.clone().with_token(span.range, token) {
                Ok(next) => content = next,
                Err(_) => continue,
            }
        }
        content
    }

    fn refresh_chips(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let content = self.content_with_chips(&text);
        self.editor.update(cx, |state, cx| {
            let _ = state.set_tokens(content, cx);
        });
    }

    fn on_editor_event(
        &mut self,
        _: &Entity<TextareaState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                if !self.applying {
                    self.history = None;
                }
                self.error = None;
                self.refresh_chips(cx);
                self.update_menu(cx);
                let text = self.text(cx);
                if let Some(key) = self.key.clone() {
                    self.drafts.update(&key, |d| d.prompt = text.clone());
                    self.schedule_save(cx);
                }
                // A question with free text keeps the custom answer in the editor.
                if !self.question.request_id.is_empty()
                    && let Some(q) = self.current_question(cx)
                {
                    self.question
                        .answers
                        .entry(q.id.clone())
                        .or_default()
                        .custom = text;
                }
                cx.notify();
            }
            InputEvent::Focus | InputEvent::Blur => cx.notify(),
            InputEvent::PressEnter { .. } => {
                let _ = window;
            }
        }
    }

    // --- menu ----------------------------------------------------------------------

    fn update_menu(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let cursor = self.editor.read(cx).cursor();
        let Some(trigger) = text::detect_trigger(&text, cursor) else {
            self.menu = None;
            self.dismissed_trigger = None;
            return;
        };
        if self.dismissed_trigger == Some(trigger.range.start) {
            self.menu = None;
            return;
        }
        self.dismissed_trigger = None;
        let (items, loading) = match trigger.kind {
            TriggerKind::Slash => {
                let at_start = text[..trigger.range.start].trim().is_empty();
                let mut commands = text::builtin_commands(false)
                    .into_iter()
                    // Side chats and fresh sessions need engine support Tern lacks.
                    .filter(|c| c.name == "model")
                    .collect::<Vec<_>>();
                if let Some(driver) = self.selection(cx).and_then(|sel| {
                    self.store
                        .read(cx)
                        .snapshot()
                        .providers
                        .iter()
                        .find(|p| p.instance_id == sel.instance_id)
                        .map(|p| p.driver)
                }) {
                    commands.extend(text::provider_commands(driver));
                }
                let commands = commands
                    .into_iter()
                    .filter(|c| at_start || !c.prompt_start_only)
                    .collect();
                (
                    text::rank_commands(commands, &trigger.query)
                        .into_iter()
                        .map(MenuItem::Command)
                        .collect(),
                    false,
                )
            }
            TriggerKind::Path => {
                let project = self.project_id(cx);
                match project.as_ref().and_then(|p| self.indexes.get(p)) {
                    Some(IndexState::Ready(index)) => (
                        index
                            .search(&trigger.query)
                            .into_iter()
                            .map(MenuItem::File)
                            .collect(),
                        false,
                    ),
                    Some(IndexState::Building) => (Vec::new(), true),
                    None => {
                        if let Some(p) = project {
                            self.ensure_index(p, cx);
                        }
                        (Vec::new(), true)
                    }
                }
            }
            TriggerKind::Skill => (Vec::new(), false),
        };
        let highlight = self
            .menu
            .as_ref()
            .filter(|m| m.trigger.range.start == trigger.range.start)
            .map(|m| m.highlight.min(items.len().saturating_sub(1)))
            .unwrap_or(0);
        self.menu = Some(MenuState {
            trigger,
            items,
            highlight,
            loading,
        });
    }

    fn project_id(&self, cx: &App) -> Option<String> {
        match &self.key {
            Some(DraftKey::New(p)) => Some(p.clone()),
            Some(DraftKey::Thread(id)) => self
                .store
                .read(cx)
                .snapshot()
                .thread(id)
                .map(|t| t.thread.project_id.clone()),
            None => None,
        }
    }

    /// Starts indexing a project's workspace in the background (once).
    fn ensure_index(&mut self, project: String, cx: &mut Context<Self>) {
        if self.indexes.contains_key(&project) {
            return;
        }
        let Some(root) = self
            .store
            .read(cx)
            .snapshot()
            .project(&project)
            .map(|p| std::path::PathBuf::from(&p.workspace_root))
        else {
            return;
        };
        self.indexes.insert(project.clone(), IndexState::Building);
        let task = cx.background_spawn(async move { FileIndex::build(&root) });
        cx.spawn(async move |this, cx| {
            let index = task.await;
            this.update(cx, |this, cx| {
                this.indexes
                    .insert(project, IndexState::Ready(Arc::new(index)));
                if this
                    .menu
                    .as_ref()
                    .is_some_and(|m| m.trigger.kind == TriggerKind::Path)
                {
                    this.update_menu(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn pick_menu_item(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.clone() else {
            return;
        };
        let Some(item) = menu.items.get(index).cloned() else {
            return;
        };
        self.menu = None;
        match item {
            MenuItem::Command(cmd) if cmd.name == "model" => {
                self.replace_in_editor(menu.trigger.range.clone(), "", window, cx);
                self.menu = None;
                self.pending_open = Some(PendingOpen::Picker { effort: false });
            }
            MenuItem::Command(cmd) => {
                let replacement = format!("/{} ", cmd.name);
                self.replace_in_editor(menu.trigger.range.clone(), &replacement, window, cx);
            }
            MenuItem::File(entry) => {
                let link = format!("{} ", text::serialize_file_link(&entry.path));
                let text = self.text(cx);
                let mut range = menu.trigger.range.clone();
                if text[range.end..].starts_with(' ') {
                    range.end += 1;
                }
                self.replace_in_editor(range, &link, window, cx);
            }
        }
        self.menu = None;
        self.dismissed_trigger = None;
        cx.notify();
    }

    // --- sending -------------------------------------------------------------------

    /// Whether the composer holds something to send.
    pub(crate) fn has_content(&self, cx: &App) -> bool {
        !self.text(cx).trim().is_empty() || !self.attachments.is_empty()
    }

    pub(crate) fn is_running(&self, cx: &App) -> bool {
        self.thread_detail(cx)
            .is_some_and(|d| pending::live_run(&d).is_some())
    }

    fn supports_steering(&self, cx: &App) -> bool {
        let Some(sel) = self.selection(cx) else {
            return false;
        };
        self.store
            .read(cx)
            .snapshot()
            .providers
            .iter()
            .find(|p| p.instance_id == sel.instance_id)
            .is_some_and(|p| p.capabilities.supports_active_steering)
    }

    /// Follow-up behavior while a run is live: steer by default, queue when the
    /// turn cannot take steering; `flip` (Ctrl) swaps them.
    pub(crate) fn follow_up_steers(&self, flip: bool, cx: &App) -> bool {
        self.supports_steering(cx) && !flip
    }

    pub(crate) fn submit(&mut self, flip: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.question.request_id.is_empty() {
            self.submit_question(window, cx);
            return;
        }
        let text = self.text(cx);
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() && self.attachments.is_empty() {
            return;
        }
        if trimmed.chars().count() > tern_core::MAX_INPUT_CHARS {
            self.error = Some(format!(
                "Prompt is too long ({} characters, the limit is {}).",
                trimmed.chars().count(),
                tern_core::MAX_INPUT_CHARS
            ));
            cx.notify();
            return;
        }
        let store = self.store.read(cx);
        let Some(sel) = self.selection(cx) else {
            self.error = Some("Enable a provider in Settings to send a message".into());
            cx.notify();
            return;
        };
        let message = OutgoingMessage {
            text: trimmed,
            attachments: self
                .attachments
                .iter()
                .map(|a| a.attachment.clone())
                .collect(),
            source_plan_id: None,
        };
        match self.key.clone() {
            Some(DraftKey::Thread(thread_id)) => {
                let detail = store.thread(&thread_id);
                let approvals = detail
                    .as_ref()
                    .map(|d| pending::pending_approvals(d))
                    .unwrap_or_default();
                if !approvals.is_empty() {
                    // Sending declines the waiting approvals with the text as feedback.
                    for request_id in approvals {
                        store.send(Command::RespondToRequest {
                            thread_id: thread_id.clone(),
                            request_id,
                            decision: Some(tern_core::ApprovalDecision::Decline),
                            feedback: Some(message.text.clone()),
                            answers: None,
                        });
                    }
                } else {
                    let live = detail.as_ref().and_then(|d| pending::live_run(d).cloned());
                    let mode = match live {
                        Some(run) if self.follow_up_steers(flip, cx) => DispatchMode::SteerActive {
                            target_run_id: run.id,
                        },
                        Some(_) => DispatchMode::QueueAfterActive,
                        None => DispatchMode::StartImmediately,
                    };
                    let explicit = self.current_draft().model_selection.filter(|s| {
                        Some(s)
                            != store
                                .snapshot()
                                .thread(&thread_id)
                                .map(|t| &t.thread.model_selection)
                    });
                    store.send(Command::SendMessage {
                        thread_id: thread_id.clone(),
                        message,
                        model_selection: explicit.or(Some(sel)),
                        mode,
                    });
                }
            }
            Some(DraftKey::New(project_id)) => {
                let draft = self.current_draft();
                let branch = draft.branch.clone();
                let workspace = match draft.env_mode.unwrap_or_default() {
                    ThreadEnvMode::Local => WorkspaceStrategy::Root { branch },
                    ThreadEnvMode::Worktree => WorkspaceStrategy::Worktree {
                        base_ref: branch.unwrap_or_else(|| "HEAD".into()),
                        branch: None,
                        start_from_origin: None,
                    },
                };
                let thread_id = tern_core::new_id();
                store.send(Command::LaunchThread {
                    thread_id: thread_id.clone(),
                    project_id,
                    model_selection: sel,
                    runtime_mode: draft.runtime_mode.unwrap_or(permissions::DEFAULT_MODE),
                    interaction_mode: InteractionMode::Default,
                    workspace,
                    message: Some(message),
                });
                self.clear_after_send(window, cx);
                let key = self.key.clone();
                if let Some(key) = key {
                    self.drafts.put(&key, Draft::default());
                }
                self.app
                    .update(cx, |app, cx| app.navigate(Route::Thread(thread_id), cx));
                return;
            }
            None => return,
        }
        self.clear_after_send(window, cx);
    }

    fn clear_after_send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.attachments.clear();
        self.menu = None;
        self.history = None;
        self.set_editor_text("", 0, window, cx);
        if let Some(key) = self.key.clone() {
            self.drafts.update(&key, |d| {
                d.prompt.clear();
                d.attachments.clear();
            });
        }
        self.schedule_save(cx);
        cx.notify();
    }

    pub(crate) fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.thread_id().map(str::to_string) else {
            return;
        };
        let Some(run) = self
            .thread_detail(cx)
            .and_then(|d| pending::live_run(&d).cloned())
        else {
            return;
        };
        self.stopping = Some(Instant::now());
        self.store.read(cx).send(Command::Interrupt {
            thread_id,
            run_id: run.id,
        });
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(10))
                .await;
            entity
                .update(cx, |this, cx| {
                    this.stopping = None;
                    cx.notify();
                })
                .ok();
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn cancel_queued(&mut self, run_id: String, cx: &mut Context<Self>) {
        if let Some(thread_id) = self.thread_id().map(str::to_string) {
            self.store
                .read(cx)
                .send(Command::CancelQueuedRun { thread_id, run_id });
        }
    }

    /// "Send now": pull a queued message out of the queue and steer it into the
    /// running turn.
    pub(crate) fn steer_queued(&mut self, run: pending::QueuedRun, cx: &mut Context<Self>) {
        let Some(thread_id) = self.thread_id().map(str::to_string) else {
            return;
        };
        let live = self
            .thread_detail(cx)
            .and_then(|d| pending::live_run(&d).cloned());
        let store = self.store.read(cx);
        store.send(Command::CancelQueuedRun {
            thread_id: thread_id.clone(),
            run_id: run.run_id,
        });
        store.send(Command::SendMessage {
            thread_id,
            message: OutgoingMessage {
                text: run.text,
                attachments: Vec::new(),
                source_plan_id: None,
            },
            model_selection: None,
            mode: match live {
                Some(r) => DispatchMode::SteerActive {
                    target_run_id: r.id,
                },
                None => DispatchMode::StartImmediately,
            },
        });
    }

    /// Edit: take the queued message back into the composer.
    pub(crate) fn edit_queued(
        &mut self,
        run: pending::QueuedRun,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_queued(run.run_id.clone(), cx);
        let text = run.text;
        self.set_editor_text(&text, text.len(), window, cx);
        if let Some(key) = self.key.clone() {
            self.drafts.update(&key, |d| d.prompt = text.clone());
        }
        self.editor.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    // --- questions -----------------------------------------------------------------

    pub(crate) fn current_question(&self, cx: &App) -> Option<tern_core::UserInputQuestion> {
        let detail = self.thread_detail(cx)?;
        let q = pending::pending_question(&detail)?;
        q.questions.get(self.question.page).cloned()
    }

    pub(crate) fn question_count(&self, cx: &App) -> usize {
        self.thread_detail(cx)
            .and_then(|d| pending::pending_question(&d))
            .map_or(0, |q| q.questions.len())
    }

    pub(crate) fn select_option(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(q) = self.current_question(cx) else {
            return;
        };
        let multi = q.multi_select.unwrap_or(false);
        let answer = self.question.answers.entry(q.id.clone()).or_default();
        if multi {
            if let Some(pos) = answer.selected.iter().position(|i| *i == index) {
                answer.selected.remove(pos);
            } else {
                answer.selected.push(index);
            }
        } else {
            answer.selected = vec![index];
            answer.custom.clear();
            if !self.text(cx).is_empty() {
                self.set_editor_text("", 0, window, cx);
            }
            // Single-select auto-advances after 200ms.
            let count = self.question_count(cx);
            if self.question.page + 1 < count {
                let page = self.question.page;
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(200))
                        .await;
                    this.update_in(cx, |this, window, cx| {
                        if this.question.page == page {
                            this.go_to_question(page + 1, window, cx);
                        }
                    })
                    .ok();
                })
                .detach();
            }
        }
        cx.notify();
    }

    pub(crate) fn go_to_question(
        &mut self,
        page: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.question.page = page;
        let custom = self
            .current_question(cx)
            .and_then(|q| self.question.answers.get(&q.id).map(|a| a.custom.clone()))
            .unwrap_or_default();
        self.set_editor_text(&custom, custom.len(), window, cx);
        cx.notify();
    }

    fn submit_question(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.question_count(cx);
        if self.question.page + 1 < count {
            self.go_to_question(self.question.page + 1, window, cx);
            return;
        }
        let (Some(thread_id), Some(detail)) =
            (self.thread_id().map(str::to_string), self.thread_detail(cx))
        else {
            return;
        };
        let Some(q) = pending::pending_question(&detail) else {
            return;
        };
        let answers = pending::answers_payload(&q.questions, &self.question.answers);
        self.store.read(cx).send(Command::RespondToRequest {
            thread_id,
            request_id: q.request_id,
            decision: None,
            feedback: None,
            answers: Some(answers),
        });
        self.question = QuestionState::default();
        self.clear_after_send(window, cx);
    }

    pub(crate) fn dismiss_question(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.thread_id().map(str::to_string) else {
            return;
        };
        if self.question.request_id.is_empty() {
            return;
        }
        self.store.read(cx).send(Command::DismissUserInput {
            thread_id,
            request_id: self.question.request_id.clone(),
        });
    }

    // --- keyboard --------------------------------------------------------------------

    /// Enter / Shift+Enter / Ctrl+Enter from the editor. Returns true when handled.
    fn on_enter(
        &mut self,
        shift: bool,
        secondary: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(menu) = &self.menu
            && !shift
            && !secondary
        {
            if !menu.items.is_empty() {
                let i = menu.highlight;
                self.pick_menu_item(i, window, cx);
            }
            return true;
        }
        if shift {
            // Newline with list continuation.
            let text = self.text(cx);
            let cursor = self.editor.read(cx).cursor();
            if let Some(edit) = list::continuation_for_enter(&text, cursor) {
                self.replace_in_editor(edit.start..edit.end, &edit.replacement, window, cx);
                let c = edit.cursor_after;
                self.editor
                    .update(cx, |s, cx| s.set_selected_range(c..c, cx));
                return true;
            }
            return false;
        }
        self.submit(secondary, window, cx);
        true
    }

    fn on_tab(&mut self, outdent: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(menu) = &self.menu
            && !outdent
        {
            if !menu.items.is_empty() {
                let i = menu.highlight;
                self.pick_menu_item(i, window, cx);
            }
            return true;
        }
        let text = self.text(cx);
        let cursor = self.editor.read(cx).cursor();
        let edit = if outdent {
            list::outdent_for_tab(&text, cursor)
        } else {
            list::indent_for_tab(&text, cursor)
        };
        if let Some(edit) = edit {
            self.replace_in_editor(edit.start..edit.end, &edit.replacement, window, cx);
            let c = edit.cursor_after;
            self.editor
                .update(cx, |s, cx| s.set_selected_range(c..c, cx));
            return true;
        }
        false
    }

    fn on_arrow(&mut self, up: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(menu) = &mut self.menu {
            let len = menu.items.len();
            if len > 0 {
                menu.highlight = if up {
                    (menu.highlight + len - 1) % len
                } else {
                    (menu.highlight + 1) % len
                };
            }
            cx.notify();
            return true;
        }
        if !self.question.request_id.is_empty() || !self.attachments.is_empty() {
            return false;
        }
        let text = self.text(cx);
        let cursor = self.editor.read(cx).cursor();
        if up {
            let on_first_line = !text[..cursor.min(text.len())].contains('\n');
            let browsing = self.history.is_some();
            if !on_first_line || (!text.is_empty() && !browsing) {
                return false;
            }
            let entries = self
                .thread_detail(cx)
                .map(|d| pending::prompt_history(&d))
                .unwrap_or_default();
            if entries.is_empty() {
                return false;
            }
            let pos = match &self.history {
                Some(h) if h.pos == 0 => return true,
                Some(h) => h.pos - 1,
                None => entries.len() - 1,
            };
            let entry = entries[pos].clone();
            self.history = Some(HistoryBrowse { entries, pos });
            self.set_editor_text(&entry, entry.len(), window, cx);
            cx.notify();
            true
        } else {
            let Some(h) = self.history.clone() else {
                return false;
            };
            let on_last_line = !text[cursor.min(text.len())..].contains('\n');
            if !on_last_line {
                return false;
            }
            if h.pos + 1 >= h.entries.len() {
                self.history = None;
                self.set_editor_text("", 0, window, cx);
            } else {
                let entry = h.entries[h.pos + 1].clone();
                self.history = Some(HistoryBrowse {
                    entries: h.entries,
                    pos: h.pos + 1,
                });
                self.set_editor_text(&entry, entry.len(), window, cx);
            }
            cx.notify();
            true
        }
    }

    fn on_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(menu) = self.menu.take() {
            self.dismissed_trigger = Some(menu.trigger.range.start);
            cx.notify();
            return true;
        }
        false
    }

    // --- popups -------------------------------------------------------------------

    pub(crate) fn open_model_picker(
        &mut self,
        effort: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection(cx).is_none() {
            return;
        }
        self.store.read(cx).send(Command::RefreshProviders);
        let composer = cx.entity().downgrade();
        let anchor = PopupAnchor::Trigger {
            bounds: self.model_bounds.get(),
            placement: gpui_base::Placement::Top,
            align: gpui_base::Align::End,
            offset: gpui::px(4.),
        };
        PopupLayer::toggle(
            "composer-model-picker".into(),
            anchor,
            move |window, cx| {
                let view = cx.new(|cx| picker::ModelPickerView::new(composer, effort, window, cx));
                let focus = view.read(cx).initial_focus(cx);
                (view.into(), Some(focus))
            },
            window,
            cx,
        );
    }

    pub(crate) fn open_permissions(
        &mut self,
        owner: &'static str,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = self.runtime_mode(cx);
        let composer = cx.entity().downgrade();
        permissions::PermissionsView::open(
            owner,
            bounds,
            mode,
            Rc::new(move |mode, _, cx| {
                if let Some(c) = composer.upgrade() {
                    c.update(cx, |c, cx| c.set_runtime_mode(mode, cx));
                }
            }),
            window,
            cx,
        );
    }

    fn take_pending_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.pending_open.take() {
            Some(PendingOpen::Picker { effort }) => {
                if !PopupLayer::is_open(&"composer-model-picker".into(), cx) {
                    self.open_model_picker(effort, window, cx);
                }
            }
            Some(PendingOpen::Permissions) => {
                if self.draft_project().is_some() {
                    self.permission_bounds.get();
                }
                let bounds = self.permission_bounds.get();
                if bounds.size.width > gpui::px(0.) {
                    self.open_permissions("composer-permissions", bounds, window, cx);
                } else {
                    self.open_model_picker(false, window, cx);
                }
            }
            None => {}
        }
    }
}

/// The most recently updated non-archived project (the index route's target).
pub fn most_recent_project(snapshot: &tern_core::Snapshot) -> Option<String> {
    snapshot
        .projects
        .iter()
        .filter(|p| p.archived_at.is_none())
        .max_by_key(|p| p.updated_at)
        .map(|p| p.id.clone())
}

//! Settings (spec 06). The shell draws the modal (backdrop, padding, the 16px-radius
//! surface) and places this view inside it; this view renders the left nav, the
//! content header and the page. Close with `AppState::close_settings`.
//!
//! Values live in [`SettingsModel`]; pages are functions in [`pages`] that build their
//! sections from the model and write back through it.

// Helpers for the pages still stubbed out (keybindings, providers, threads, ...).
#![allow(dead_code)]
mod font_picker;
mod keymap;
mod licenses;
pub mod model;
mod nav;
mod pages;
mod search;
pub mod themes;
mod widgets;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    div, point, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{InputEvent, InputState};

use crate::app_state::{AppState, SettingsPage};
use crate::store::Store;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize};
use crate::ui::input::TextareaState;
use crate::ui::{
    Button, ButtonSize, ButtonVariant, ConfirmOptions, Dialogs, IconName, Tooltip, TooltipSide,
    h_flex, v_flex,
};

pub use model::SettingsModel;

/// Commit callback for a text field: the field's value on blur or Enter.
type Commit = Rc<dyn Fn(&mut SettingsView, String, &mut Window, &mut Context<SettingsView>)>;

pub struct SettingsView {
    app: Entity<AppState>,
    store: Entity<Store>,
    model: Entity<SettingsModel>,
    focus: FocusHandle,
    nav_search: Entity<InputState>,
    nav_query: String,
    nav_highlight: usize,
    scroll: ScrollHandle,
    inputs: HashMap<SharedString, Entity<InputState>>,
    textareas: HashMap<SharedString, Entity<TextareaState>>,
    input_subs: Vec<Subscription>,
    open_folds: HashSet<SharedString>,
    /// The scope sentence's project (None = All projects).
    scope_project: Option<String>,
    /// A search result to scroll to and pulse.
    target: Option<SharedString>,
    target_bounds: Rc<Cell<Bounds<Pixels>>>,
    pulse: Option<SharedString>,
    epoch: u64,
    page: Option<SettingsPage>,
    keys: keymap::KeysState,
    providers: pages::providers::ProvidersState,
    licenses: licenses::LicensesState,
    _subs: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = AppState::global(cx);
        let store = Store::global(cx);
        let model = SettingsModel::global(cx);
        model.update(cx, |m, cx| m.apply_startup(window, cx));
        let subs = vec![
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.observe(&model, |this, model, cx| {
                let epoch = model.read(cx).epoch();
                if epoch != this.epoch {
                    this.epoch = epoch;
                    this.reset_drafts();
                }
                cx.notify();
            }),
        ];
        let nav_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let search_sub = cx.subscribe_in(
            &nav_search,
            window,
            |this, state, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => {
                    this.nav_query = state.read(cx).value().to_string();
                    this.nav_highlight = 0;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    let hits = search::matches(&this.nav_query);
                    if let Some(hit) =
                        hits.get(this.nav_highlight.min(hits.len().saturating_sub(1)))
                    {
                        this.open_result(hit, window, cx);
                    }
                }
                _ => {}
            },
        );
        let epoch = model.read(cx).epoch();
        let mut subs = subs;
        subs.push(search_sub);
        Self {
            app,
            store,
            model,
            focus: cx.focus_handle(),
            nav_search,
            nav_query: String::new(),
            nav_highlight: 0,
            scroll: ScrollHandle::new(),
            inputs: HashMap::new(),
            textareas: HashMap::new(),
            input_subs: Vec::new(),
            open_folds: HashSet::new(),
            scope_project: None,
            target: None,
            target_bounds: Rc::new(Cell::new(Bounds::default())),
            pulse: None,
            epoch,
            page: None,
            keys: keymap::KeysState::default(),
            providers: pages::providers::ProvidersState::default(),
            licenses: licenses::LicensesState::default(),
            _subs: subs,
        }
    }

    fn reset_drafts(&mut self) {
        self.inputs.clear();
        self.textareas.clear();
        self.input_subs.clear();
        self.keys = keymap::KeysState::default();
    }

    pub(crate) fn model(&self) -> &Entity<SettingsModel> {
        &self.model
    }

    // --- fields --------------------------------------------------------------

    /// A single-line field keyed by `key`, created with `initial` and committed on
    /// blur or Enter. Changing the key (a new project or provider) makes a new field.
    pub(crate) fn input(
        &mut self,
        key: impl Into<SharedString>,
        initial: impl FnOnce() -> String,
        placeholder: &str,
        commit: impl Fn(&mut SettingsView, String, &mut Window, &mut Context<SettingsView>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let key = key.into();
        if let Some(state) = self.inputs.get(&key) {
            return state.clone();
        }
        let value = initial();
        let placeholder = placeholder.to_string();
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(value)
        });
        let commit: Commit = Rc::new(commit);
        let sub = cx.subscribe_in(
            &state,
            window,
            move |this, state, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let value = state.read(cx).value().to_string();
                    commit(this, value, window, cx);
                }
            },
        );
        self.input_subs.push(sub);
        self.inputs.insert(key, state.clone());
        state
    }

    /// A live field (search boxes): `on_change` runs on every edit.
    pub(crate) fn live_input(
        &mut self,
        key: impl Into<SharedString>,
        placeholder: &str,
        on_change: impl Fn(&mut SettingsView, String, &mut Context<SettingsView>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let key = key.into();
        if let Some(state) = self.inputs.get(&key) {
            return state.clone();
        }
        let placeholder = placeholder.to_string();
        let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let sub = cx.subscribe_in(
            &state,
            window,
            move |this, state, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change) {
                    let value = state.read(cx).value().to_string();
                    on_change(this, value, cx);
                    cx.notify();
                }
            },
        );
        self.input_subs.push(sub);
        self.inputs.insert(key, state.clone());
        state
    }

    /// A multi-line field committed on blur.
    pub(crate) fn textarea(
        &mut self,
        key: impl Into<SharedString>,
        initial: impl FnOnce() -> String,
        placeholder: &str,
        commit: impl Fn(&mut SettingsView, String, &mut Window, &mut Context<SettingsView>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextareaState> {
        let key = key.into();
        if let Some(state) = self.textareas.get(&key) {
            return state.clone();
        }
        let value = initial();
        let placeholder = placeholder.to_string();
        let state = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(placeholder)
                .default_value(value)
        });
        let sub = cx.subscribe_in(
            &state,
            window,
            move |this, state, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::Blur) {
                    let value = state.read(cx).value().to_string();
                    commit(this, value, window, cx);
                }
            },
        );
        self.input_subs.push(sub);
        self.textareas.insert(key, state.clone());
        state
    }

    pub(crate) fn fold_open(&self, key: &str) -> bool {
        self.open_folds.contains(key)
    }

    pub(crate) fn toggle_fold(&mut self, key: impl Into<SharedString>, cx: &mut Context<Self>) {
        let key = key.into();
        if !self.open_folds.remove(&key) {
            self.open_folds.insert(key);
        }
        cx.notify();
    }

    /// Whether the row with this id is the search target (pulse ring + scroll probe).
    pub(crate) fn is_pulsing(&self, id: &str) -> bool {
        self.pulse.as_deref() == Some(id)
    }

    pub(crate) fn probe_for(&self, id: &str) -> Option<Rc<Cell<Bounds<Pixels>>>> {
        (self.target.as_deref() == Some(id)).then(|| self.target_bounds.clone())
    }

    // --- navigation ------------------------------------------------------------

    fn go(&mut self, page: SettingsPage, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| {
            s.replace(crate::app_state::Route::Settings(page), cx)
        });
    }

    fn open_result(
        &mut self,
        hit: &search::SearchItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = SharedString::from(hit.id);
        self.go(hit.page, cx);
        self.target = Some(id.clone());
        self.pulse = Some(id.clone());
        // Folds holding the target open first.
        if let Some(fold) = pages::fold_for(hit.id) {
            self.open_folds.insert(fold.into());
        }
        self.nav_search
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.nav_query.clear();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1300))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.pulse.as_ref() == Some(&id) {
                    this.pulse = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// After a frame with the target laid out, scroll it to the middle.
    fn scroll_to_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.target.is_none() {
            return;
        }
        let bounds = self.target_bounds.clone();
        let scroll = self.scroll.clone();
        cx.on_next_frame(window, move |this, _, cx| {
            let b = bounds.get();
            let viewport = scroll.bounds();
            if b.size.height > px(0.) && viewport.size.height > px(0.) {
                let center = viewport.origin.y + viewport.size.height / 2.;
                let row_center = b.origin.y + b.size.height / 2.;
                let offset = scroll.offset();
                let max = scroll.max_offset().y;
                let y = (offset.y - (row_center - center)).clamp(-max, px(0.));
                scroll.set_offset(point(offset.x, y));
                this.target = None;
            }
            cx.notify();
        });
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &e.keystroke;
        let typing = self.nav_search.focus_handle(cx).is_focused(window)
            || self
                .inputs
                .values()
                .any(|s| s.focus_handle(cx).is_focused(window))
            || self
                .textareas
                .values()
                .any(|s| s.focus_handle(cx).is_focused(window))
            || self.keys.recording.is_some();
        let nav_focused = self.nav_search.focus_handle(cx).is_focused(window);
        if nav_focused && !self.nav_query.is_empty() {
            let n = search::matches(&self.nav_query).len();
            match ks.key.as_str() {
                "down" if n > 0 => {
                    self.nav_highlight = (self.nav_highlight + 1) % n;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                "up" if n > 0 => {
                    self.nav_highlight = (self.nav_highlight + n - 1) % n;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                "escape" => {
                    self.nav_search
                        .update(cx, |s, cx| s.set_value("", window, cx));
                    self.nav_query.clear();
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }
        let m = &ks.modifiers;
        let slash = ks.key == "/" && !m.control && !m.alt && !m.platform && !typing;
        let find = ks.key == "f" && m.control && !m.alt && !m.shift;
        if slash || find {
            self.nav_search.update(cx, |s, cx| {
                s.focus(window, cx);
                s.select_all(window, cx);
            });
            cx.stop_propagation();
        }
    }

    fn render_header(
        &mut self,
        page: SettingsPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *cx.tokens();
        let model = self.model.clone();
        let restore = (page == SettingsPage::General).then(|| {
            let labels = model.read(cx).changed_device_labels();
            let narrow = window.viewport_size().width < px(640.);
            let mut b = Button::new("restore-defaults")
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Xs)
                .icon(IconName::RotateCcw)
                .disabled(labels.is_empty());
            if !narrow {
                b = b.label("Restore device defaults");
            }
            b.on_click(move |_, window, cx| {
                let labels = model.read(cx).changed_device_labels();
                if labels.is_empty() {
                    return;
                }
                let message = format!(
                    "Restore default settings?\nThis will reset: {}.",
                    labels.join(", ")
                );
                let rx = Dialogs::confirm(message, ConfirmOptions::destructive(), window, cx);
                let model = model.clone();
                window
                    .spawn(cx, async move |cx| {
                        if rx.await.unwrap_or(false) {
                            let _ = cx.update(|window, cx| {
                                model.update(cx, |m, cx| m.restore_device_defaults(window, cx))
                            });
                        }
                    })
                    .detach();
            })
        });
        let app = self.app.clone();
        h_flex()
            .h(px(48.))
            .flex_none()
            .pl(px(20.))
            .pr(px(8.))
            .gap(px(12.))
            .border_b_1()
            .border_color(t.border)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(12.))
                    .font_medium()
                    .text_scale(TextSize::Sm)
                    .child(div().text_color(t.muted_foreground).child("Settings"))
                    .child(div().text_color(t.icon_muted).child("/"))
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_color(t.foreground)
                            .child(page.title()),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(4.))
                    .when_some(restore, |el, b| el.child(b))
                    .child(
                        Button::new("settings-close")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::IconSm)
                            .icon(IconName::X)
                            .tooltip(Tooltip::text("Close (Esc)").side(TooltipSide::Bottom))
                            .on_click(move |_, window, cx| {
                                window.blur(cx);
                                app.update(cx, |s, cx| s.close_settings(cx));
                            }),
                    ),
            )
            .into_any_element()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        let page = self
            .app
            .read(cx)
            .settings_page()
            .unwrap_or(SettingsPage::General);
        if self.page != Some(page) {
            self.page = Some(page);
            if self.target.is_none() {
                self.scroll.set_offset(point(px(0.), px(0.)));
            }
        }
        let header = self.render_header(page, window, cx);
        let nav = nav::render(self, page, window, cx);
        let sections = pages::render(self, page, window, cx);
        self.scroll_to_target(window, cx);
        h_flex()
            .id("settings-view")
            .key_context("Settings")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .items_start()
            .child(nav)
            .child(
                v_flex().flex_1().min_w_0().h_full().child(header).child(
                    div()
                        .id("settings-scroll")
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .bg(t.background)
                        .child(
                            v_flex()
                                .w_full()
                                .max_w(px(page.column_width()))
                                .mx_auto()
                                .px(px(24.))
                                .pt(px(24.))
                                .pb(px(48.))
                                .gap(px(32.))
                                .children(sections),
                        ),
                ),
            )
    }
}

/// Column widths (`readable` 896 by default).
trait ColumnWidth {
    fn column_width(self) -> f32;
}

impl ColumnWidth for SettingsPage {
    fn column_width(self) -> f32 {
        896.
    }
}

/// Width of the settings content column (the modal minus the nav), for layout math.
pub(crate) fn content_width(window: &Window) -> Pixels {
    let vw = window.viewport_size().width;
    let pad = if vw >= px(1024.) {
        px(80.)
    } else if vw >= px(640.) {
        px(48.)
    } else {
        px(0.)
    };
    let modal = (vw - pad).min(px(1280.));
    (modal - px(242.)).min(px(896.))
}

/// Helper for pages: the model's client settings, read.
pub(crate) fn client(cx: &App) -> &tern_core::settings::ClientSettings {
    SettingsModel::try_global(cx)
        .map(|m| m.read(cx).client())
        .expect("settings model installed")
}

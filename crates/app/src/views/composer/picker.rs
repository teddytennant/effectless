//! The composer's model picker popup (spec 04 §2.2-2.3): provider rail, searchable
//! model list, effort column and the footer with speed and Permissions.

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement as _, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{
    Enter, Escape, Input, InputEvent, InputState, MoveDown, MoveLeft, MoveRight, MoveUp,
};
use tern_core::provider::{ModelOptionDescriptor, ProviderModel, ProviderStatus};
use tern_core::{ModelSelection, OptionValue};

use super::ComposerView;
use super::model::{self, bool_value, reasoning_descriptor, selected_choice, short_name};
use super::permissions;
use crate::assets::IconName;
use crate::store::Store;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, radius};
use crate::ui::popup::PopupLayer;
use crate::ui::{Icon, Switch, SwitchSize, Tooltip, TooltipExt as _, TooltipSide, h_flex, v_flex};

const BODY_HEIGHT: f32 = 346.;
const SIDEBAR_WIDTH: f32 = 44.;
const LIST_WIDTH: f32 = 316.;
const EFFORT_WIDTH: f32 = 192.;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tab {
    Favorites,
    Provider(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    List,
    Effort,
}

/// One row of the model list.
#[derive(Clone, Debug, PartialEq)]
enum Row {
    Model { instance: String, slug: String },
    ShowMore,
}

pub struct ModelPickerView {
    composer: WeakEntity<ComposerView>,
    store: Entity<Store>,
    search: Entity<InputState>,
    query: String,
    tab: Tab,
    highlight: Option<usize>,
    effort_highlight: Option<usize>,
    focus_area: Focus,
    show_legacy: bool,
    hovered_row: Option<usize>,
    list_scroll: ScrollHandle,
    focus: FocusHandle,
}

impl ModelPickerView {
    pub fn new(
        composer: WeakEntity<ComposerView>,
        focus_effort: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = Store::global(cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search models..."));
        cx.subscribe(
            &search,
            |this: &mut Self, search, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    this.query = search.read(cx).value().to_string();
                    this.highlight = Some(0);
                    cx.notify();
                }
            },
        )
        .detach();
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        let (selection, locked, has_favorite) = composer
            .upgrade()
            .map(|c| {
                let c = c.read(cx);
                (
                    c.selection(cx),
                    c.locked_instance(cx),
                    !c.drafts.file.favorites.is_empty(),
                )
            })
            .unwrap_or((None, None, false));
        let tab = match (&locked, has_favorite, &selection) {
            (Some(instance), _, _) => Tab::Provider(instance.clone()),
            (None, true, _) => Tab::Favorites,
            (None, false, Some(sel)) => Tab::Provider(sel.instance_id.clone()),
            _ => store
                .read(cx)
                .snapshot()
                .providers
                .first()
                .map(|p| Tab::Provider(p.instance_id.clone()))
                .unwrap_or(Tab::Favorites),
        };
        let show_legacy = selection.as_ref().is_some_and(|sel| {
            model::find_model(&store.read(cx).snapshot().providers, sel)
                .is_some_and(|(_, m)| m.is_legacy)
        });
        let focus = cx.focus_handle();
        let mut this = Self {
            composer,
            store,
            search,
            query: String::new(),
            tab,
            highlight: None,
            effort_highlight: None,
            focus_area: if focus_effort {
                Focus::Effort
            } else {
                Focus::List
            },
            show_legacy,
            hovered_row: None,
            list_scroll: ScrollHandle::new(),
            focus,
        };
        this.highlight = this.selected_row_index(cx).or(Some(0));
        if focus_effort {
            this.effort_highlight = this.selected_effort_index(cx);
        }
        this
    }

    /// The element to focus when the popup opens (the search field).
    pub fn initial_focus(&self, cx: &App) -> FocusHandle {
        self.search.read(cx).focus_handle(cx)
    }

    fn selection(&self, cx: &App) -> Option<ModelSelection> {
        self.composer.upgrade()?.read(cx).selection(cx)
    }

    fn locked(&self, cx: &App) -> Option<String> {
        self.composer.upgrade()?.read(cx).locked_instance(cx)
    }

    fn is_favorite(&self, instance: &str, slug: &str, cx: &App) -> bool {
        self.composer
            .upgrade()
            .is_some_and(|c| c.read(cx).drafts.is_favorite(instance, slug))
    }

    fn rows(&self, cx: &App) -> Vec<Row> {
        let store = self.store.read(cx);
        let providers = &store.snapshot().providers;
        let locked = self.locked(cx);
        if !self.query.trim().is_empty() {
            return model::search_models(providers, &self.query)
                .into_iter()
                .filter(|(instance, _)| locked.as_ref().is_none_or(|l| l == instance))
                .map(|(instance, slug)| Row::Model { instance, slug })
                .collect();
        }
        match &self.tab {
            Tab::Favorites => {
                let favs = self
                    .composer
                    .upgrade()
                    .map(|c| c.read(cx).drafts.file.favorites.clone())
                    .unwrap_or_default();
                let mut rows = Vec::new();
                for p in model::usable(providers) {
                    for m in &p.models {
                        if favs.contains(&format!("{}:{}", p.instance_id, m.slug)) {
                            rows.push(Row::Model {
                                instance: p.instance_id.clone(),
                                slug: m.slug.clone(),
                            });
                        }
                    }
                }
                rows
            }
            Tab::Provider(instance) => {
                let Some(p) = providers.iter().find(|p| &p.instance_id == instance) else {
                    return Vec::new();
                };
                let fav = |m: &ProviderModel| self.is_favorite(&p.instance_id, &m.slug, cx);
                let mut rows: Vec<Row> = p
                    .models
                    .iter()
                    .filter(|m| fav(m))
                    .chain(p.models.iter().filter(|m| !fav(m) && !m.is_legacy))
                    .map(|m| Row::Model {
                        instance: p.instance_id.clone(),
                        slug: m.slug.clone(),
                    })
                    .collect();
                let legacy: Vec<&ProviderModel> =
                    p.models.iter().filter(|m| !fav(m) && m.is_legacy).collect();
                if !legacy.is_empty() {
                    if self.show_legacy {
                        rows.extend(legacy.iter().map(|m| Row::Model {
                            instance: p.instance_id.clone(),
                            slug: m.slug.clone(),
                        }));
                    }
                    rows.push(Row::ShowMore);
                }
                rows
            }
        }
    }

    fn selected_row_index(&self, cx: &App) -> Option<usize> {
        let sel = self.selection(cx)?;
        self.rows(cx).iter().position(
            |r| matches!(r, Row::Model { instance, slug } if *instance == sel.instance_id && *slug == sel.model),
        )
    }

    fn current_model<'a>(
        &self,
        providers: &'a [ProviderStatus],
        cx: &App,
    ) -> Option<(&'a ProviderStatus, &'a ProviderModel, ModelSelection)> {
        let sel = self.selection(cx)?;
        let (p, m) = model::find_model(providers, &sel)?;
        Some((p, m, sel))
    }

    fn effort_choices(&self, cx: &App) -> Vec<String> {
        let store = self.store.read(cx);
        let Some((_, m, _)) = self.current_model(&store.snapshot().providers, cx) else {
            return Vec::new();
        };
        match reasoning_descriptor(m) {
            Some(d) if !d.boolean => d.choices.iter().map(|c| c.value.clone()).collect(),
            _ => Vec::new(),
        }
    }

    fn selected_effort_index(&self, cx: &App) -> Option<usize> {
        let store = self.store.read(cx);
        let (_, m, sel) = self.current_model(&store.snapshot().providers, cx)?;
        let d = reasoning_descriptor(m).filter(|d| !d.boolean)?;
        let value = selected_choice(d, &sel)?;
        d.choices.iter().position(|c| c.value == value)
    }

    fn pick_model(
        &mut self,
        instance: &str,
        slug: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(composer) = self.composer.upgrade() else {
            return;
        };
        let current = self.selection(cx);
        let keep_options = current
            .as_ref()
            .filter(|c| c.instance_id == instance && c.model == slug)
            .map(|c| c.options.clone());
        let sticky = composer
            .read(cx)
            .drafts
            .file
            .sticky_model_selection_by_provider
            .get(instance)
            .filter(|s| s.model == slug)
            .map(|s| s.options.clone());
        let sel = ModelSelection {
            instance_id: instance.to_string(),
            model: slug.to_string(),
            options: keep_options.or(sticky).unwrap_or_default(),
        };
        composer.update(cx, |c, cx| c.set_selection(sel, cx));
        // Docked behavior: stay open, move to the effort column.
        self.focus_area = Focus::Effort;
        self.effort_highlight = self.selected_effort_index(cx);
        let _ = window;
        cx.notify();
    }

    fn set_option(&mut self, id: &str, value: OptionValue, cx: &mut Context<Self>) {
        let Some(mut sel) = self.selection(cx) else {
            return;
        };
        sel.set_option(id, value);
        if let Some(composer) = self.composer.upgrade() {
            composer.update(cx, |c, cx| c.set_selection(sel, cx));
        }
        cx.notify();
    }

    fn move_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        match self.focus_area {
            Focus::List => {
                let len = self.rows(cx).len();
                if len == 0 {
                    return;
                }
                let next = match self.highlight {
                    None => 0,
                    Some(i) => (i as isize + delta).rem_euclid(len as isize) as usize,
                };
                self.highlight = Some(next);
                self.list_scroll.scroll_to_item(next);
            }
            Focus::Effort => {
                let len = self.effort_choices(cx).len();
                if len == 0 {
                    return;
                }
                let next = match self.effort_highlight {
                    None => 0,
                    Some(i) => (i as isize + delta).rem_euclid(len as isize) as usize,
                };
                self.effort_highlight = Some(next);
            }
        }
        cx.notify();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.focus_area {
            Focus::List => {
                let rows = self.rows(cx);
                match self.highlight.and_then(|i| rows.get(i)).cloned() {
                    Some(Row::Model { instance, slug }) => {
                        self.pick_model(&instance, &slug, window, cx)
                    }
                    Some(Row::ShowMore) => {
                        self.show_legacy = !self.show_legacy;
                        cx.notify();
                    }
                    None => {}
                }
            }
            Focus::Effort => {
                let choices = self.effort_choices(cx);
                let store = self.store.read(cx);
                let id = self
                    .current_model(&store.snapshot().providers, cx)
                    .and_then(|(_, m, _)| reasoning_descriptor(m).map(|d| d.id.clone()));
                if let (Some(id), Some(value)) =
                    (id, self.effort_highlight.and_then(|i| choices.get(i)))
                {
                    self.set_option(&id, OptionValue::Str(value.clone()), cx);
                }
            }
        }
    }

    fn cycle_tab(&mut self, delta: isize, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        let locked = self.locked(cx);
        let mut tabs = vec![Tab::Favorites];
        tabs.extend(
            model::usable(&store.snapshot().providers)
                .filter(|p| locked.as_ref().is_none_or(|l| l == &p.instance_id))
                .map(|p| Tab::Provider(p.instance_id.clone())),
        );
        let i = tabs.iter().position(|t| *t == self.tab).unwrap_or(0) as isize;
        self.tab = tabs[(i + delta).rem_euclid(tabs.len() as isize) as usize].clone();
        self.highlight = Some(0);
        cx.notify();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let store = self.store.read(cx);
        let locked = self.locked(cx);
        let item = |id: SharedString, selected: bool, disabled: bool| {
            div()
                .id(id)
                .relative()
                .size(px(36.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(radius::MD)
                .when(!disabled, |el| {
                    el.cursor_pointer()
                        .hover(|s| s.bg(t.foreground.opacity(0.1)))
                })
                .when(disabled, |el| el.opacity(0.5).cursor_not_allowed())
                .when(selected, |el| {
                    el.child(
                        div()
                            .absolute()
                            .right(px(-4.))
                            .top(px(8.))
                            .w(px(3.))
                            .h(px(20.))
                            .rounded(radius::FULL)
                            .bg(t.primary),
                    )
                })
        };
        let favorites = item(
            "picker-tab-favorites".into(),
            self.tab == Tab::Favorites,
            false,
        )
        .t3_tooltip(
            Tooltip::text("Favorites")
                .side(TooltipSide::Left)
                .delay(std::time::Duration::from_millis(300)),
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.tab = Tab::Favorites;
            this.highlight = Some(0);
            cx.notify();
        }))
        .child(svg_star(20., true, t.foreground));
        let providers: Vec<AnyElement> = store
            .snapshot()
            .providers
            .iter()
            .filter(|p| p.enabled || p.models.is_empty())
            .map(|p| {
                let instance = p.instance_id.clone();
                let disabled = !p.enabled
                    || p.models.is_empty()
                    || locked.as_ref().is_some_and(|l| l != &p.instance_id);
                let selected = self.tab == Tab::Provider(p.instance_id.clone());
                let tip = if locked.as_ref().is_some_and(|l| l != &p.instance_id) {
                    format!(
                        "{} is unavailable in this thread. Start a new thread to switch providers.",
                        p.display_name
                    )
                } else if !p.enabled {
                    format!("{}: Disabled in settings.", p.display_name)
                } else {
                    p.display_name.clone()
                };
                let icon = Icon::new(model::provider_icon(p.driver))
                    .size(px(20.))
                    .color(model::provider_icon_color(p.driver).unwrap_or(t.foreground));
                item(
                    SharedString::from(format!("picker-tab-{}", p.instance_id)),
                    selected,
                    disabled,
                )
                .t3_tooltip(
                    Tooltip::text(tip)
                        .side(TooltipSide::Left)
                        .delay(std::time::Duration::from_millis(300)),
                )
                .when(!disabled, |el| {
                    el.on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = Tab::Provider(instance.clone());
                        this.highlight = Some(0);
                        this.show_legacy = false;
                        cx.notify();
                    }))
                })
                .child(
                    div()
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon),
                )
                .into_any_element()
            })
            .collect();
        v_flex()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .bg(t.muted.opacity(0.3))
            .p(px(4.))
            .gap(px(4.))
            .child(favorites)
            .child(div().h(px(1.)).mx(px(4.)).bg(t.border.opacity(0.7)))
            .children(providers)
            .into_any_element()
    }

    fn render_list(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let rows = self.rows(cx);
        let selection = self.selection(cx);
        let search_focused = self.search.read(cx).focus_handle(cx).is_focused(window);
        let store = self.store.read(cx);
        let providers = store.snapshot().providers.clone();
        let mut items: Vec<AnyElement> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let highlighted = self.highlight == Some(i) && self.focus_area == Focus::List
                || self.hovered_row == Some(i);
            match row {
                Row::ShowMore => {
                    items.push(
                        div()
                            .id(("picker-more", i))
                            .min_h(px(28.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .rounded(radius::SM)
                            .text_scale(TextSize::Sm)
                            .text_color(if highlighted {
                                t.foreground
                            } else {
                                t.muted_foreground
                            })
                            .cursor_pointer()
                            .hover(|s| s.text_color(t.foreground))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_legacy = !this.show_legacy;
                                cx.notify();
                            }))
                            .child(if self.show_legacy {
                                "Show less"
                            } else {
                                "Show more"
                            })
                            .into_any_element(),
                    );
                }
                Row::Model { instance, slug } => {
                    let Some(p) = providers.iter().find(|p| &p.instance_id == instance) else {
                        continue;
                    };
                    let Some(m) = p.models.iter().find(|m| &m.slug == slug) else {
                        continue;
                    };
                    let selected = selection
                        .as_ref()
                        .is_some_and(|s| &s.instance_id == instance && &s.model == slug);
                    let favorite = self.is_favorite(instance, slug, cx);
                    let (inst, sl) = (instance.clone(), slug.clone());
                    let (inst2, sl2) = (instance.clone(), slug.clone());
                    let group: SharedString = format!("picker-row-{i}").into();
                    items.push(
                        h_flex()
                            .id(("picker-row", i))
                            .group(group.clone())
                            .min_h(px(28.))
                            .px(px(8.))
                            .py(px(4.))
                            .gap(px(8.))
                            .rounded(radius::SM)
                            .when(highlighted, |el| el.bg(t.foreground.opacity(0.09)))
                            .cursor_pointer()
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered {
                                    this.hovered_row = Some(i);
                                } else if this.hovered_row == Some(i) {
                                    this.hovered_row = None;
                                }
                                cx.notify();
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.highlight = Some(i);
                                this.pick_model(&inst, &sl, window, cx);
                            }))
                            .child(
                                Icon::new(model::provider_icon(p.driver))
                                    .size(px(16.))
                                    .color(
                                        model::provider_icon_color(p.driver)
                                            .unwrap_or(t.foreground),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_scale(TextSize::Sm)
                                            .text_color(t.foreground)
                                            .child(SharedString::from(m.name.clone())),
                                    )
                                    .when(selected, |el| {
                                        el.child(
                                            Icon::new(IconName::Check)
                                                .size(px(14.))
                                                .color(t.muted_foreground),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .id(("picker-star", i))
                                    .size(px(24.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(radius::MD)
                                    .when(!favorite && !highlighted, |el| el.opacity(0.))
                                    .hover(|s| s.bg(t.foreground.opacity(0.08)))
                                    .t3_tooltip(Tooltip::text(if favorite {
                                        "Remove from favorites"
                                    } else {
                                        "Add to favorites"
                                    }))
                                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        if let Some(c) = this.composer.upgrade() {
                                            c.update(cx, |c, cx| {
                                                c.toggle_favorite(&inst2, &sl2, cx)
                                            });
                                        }
                                        cx.notify();
                                    }))
                                    .child(if favorite {
                                        svg_star(12., true, t.warning).into_any_element()
                                    } else {
                                        Icon::new(IconName::Star)
                                            .size(px(12.))
                                            .color(t.muted_foreground)
                                            .into_any_element()
                                    }),
                            )
                            .into_any_element(),
                    );
                }
            }
        }
        let empty = if rows.is_empty() {
            Some(
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .text_scale(TextSize::Sm)
                    .text_color(t.muted_foreground)
                    .child(
                        if self.query.trim().is_empty() && self.tab == Tab::Favorites {
                            "No favorites yet"
                        } else {
                            "No models found"
                        },
                    ),
            )
        } else {
            None
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(t.muted.opacity(0.4))
            .border_l_1()
            .border_color(t.border.opacity(0.7))
            .child(
                div().px(px(12.)).pt(px(10.)).child(
                    div()
                        .relative()
                        .pb(px(6.))
                        .border_b_1()
                        .border_color(if search_focused {
                            t.ring
                        } else {
                            t.border.opacity(0.7)
                        })
                        .child(
                            div().absolute().left_0().top(px(5.)).child(
                                Icon::new(IconName::Search)
                                    .size(px(16.))
                                    .color(t.muted_foreground.opacity(0.55)),
                            ),
                        )
                        .child(
                            div().pl(px(20.)).child(
                                Input::new(&self.search)
                                    .appearance(false)
                                    .h(px(26.))
                                    .px(px(0.))
                                    .text_size(px(14.))
                                    .line_height(px(26.)),
                            ),
                        ),
                ),
            )
            .child(
                div()
                    .id("picker-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.list_scroll)
                    .py(px(6.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .child(v_flex().gap(px(2.)).children(items).children(empty)),
            )
            .into_any_element()
    }

    fn render_effort(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let store = self.store.read(cx);
        let providers = store.snapshot().providers.clone();
        let current = self.current_model(&providers, cx);
        let mut col = v_flex()
            .w(px(EFFORT_WIDTH))
            .flex_none()
            .h_full()
            .bg(t.muted.opacity(0.4))
            .border_l_1()
            .border_color(t.border.opacity(0.7));
        let Some((_, m, sel)) = current else {
            return col.into_any_element();
        };
        let reasoning = reasoning_descriptor(m);
        let header_label = reasoning
            .map(|d| d.label.clone())
            .unwrap_or_else(|| "Options".to_string());
        col = col.child(
            div().px(px(12.)).pt(px(10.)).child(
                h_flex()
                    .h(px(33.))
                    .pb(px(6.))
                    .border_b_1()
                    .border_color(t.border.opacity(0.7))
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_scale(TextSize::Sm)
                            .text_color(t.muted_foreground)
                            .child(SharedString::from(header_label)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_scale(TextSize::Xs)
                            .text_color(t.muted_foreground)
                            .child(SharedString::from(short_name(m))),
                    ),
            ),
        );
        let mut rows: Vec<AnyElement> = Vec::new();
        match reasoning {
            Some(d) if !d.boolean => {
                let value = selected_choice(d, &sel);
                let explicit = sel.option(&d.id).is_some();
                for (i, c) in d.choices.iter().enumerate() {
                    let checked =
                        value.as_deref() == Some(c.value.as_str()) && (explicit || c.is_default);
                    let highlighted =
                        self.focus_area == Focus::Effort && self.effort_highlight == Some(i);
                    let id = d.id.clone();
                    let v = c.value.clone();
                    let mut row = h_flex()
                        .id(("effort-row", i))
                        .h(px(32.))
                        .px(px(8.))
                        .rounded(radius::SM)
                        .justify_between()
                        .text_scale(TextSize::Sm)
                        .text_color(t.foreground)
                        .cursor_pointer()
                        .when(highlighted, |el| el.bg(t.foreground.opacity(0.09)))
                        .hover(|s| s.bg(t.foreground.opacity(0.09)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.focus_area = Focus::Effort;
                            this.effort_highlight = Some(i);
                            this.set_option(&id, OptionValue::Str(v.clone()), cx);
                        }))
                        .child(SharedString::from(c.label.clone()))
                        .child(if checked {
                            Icon::new(IconName::Check)
                                .size(px(14.))
                                .color(t.foreground)
                                .into_any_element()
                        } else if c.is_default {
                            div()
                                .text_scale(TextSize::Xs)
                                .text_color(t.muted_foreground.opacity(0.7))
                                .child("Default")
                                .into_any_element()
                        } else {
                            div().into_any_element()
                        });
                    if let Some(desc) = c.description.clone() {
                        row = row.t3_tooltip(Tooltip::text(desc).side(TooltipSide::Right));
                    }
                    rows.push(row.into_any_element());
                }
            }
            Some(d) => rows.push(self.bool_row(d, &sel, cx)),
            None => rows.push(
                div()
                    .h(px(32.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .text_scale(TextSize::Sm)
                    .text_color(t.muted_foreground)
                    .child("No effort levels")
                    .into_any_element(),
            ),
        }
        let others: Vec<&ModelOptionDescriptor> = m
            .options
            .iter()
            .filter(|o| reasoning.is_none_or(|r| r.id != o.id) && o.id != "fastMode" && o.boolean)
            .collect();
        if !others.is_empty() {
            rows.push(
                div()
                    .mt(px(4.))
                    .mb(px(4.))
                    .h(px(1.))
                    .bg(t.border.opacity(0.7))
                    .into_any_element(),
            );
            for o in others {
                rows.push(self.bool_row(o, &sel, cx));
            }
        }
        col.child(
            div()
                .id("effort-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pt(px(10.))
                .px(px(8.))
                .pb(px(6.))
                .child(v_flex().gap(px(2.)).children(rows)),
        )
        .into_any_element()
    }

    fn bool_row(
        &self,
        d: &ModelOptionDescriptor,
        sel: &ModelSelection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *cx.tokens();
        let on = bool_value(d, sel);
        let id = d.id.clone();
        let weak = cx.entity().downgrade();
        h_flex()
            .min_h(px(32.))
            .px(px(8.))
            .py(px(2.))
            .justify_between()
            .gap(px(8.))
            .child(
                v_flex()
                    .min_w_0()
                    .child(
                        div()
                            .text_scale(TextSize::Xs)
                            .text_color(t.muted_foreground)
                            .child(SharedString::from(d.label.clone())),
                    )
                    .child(
                        div()
                            .text_scale(TextSize::Sm)
                            .text_color(t.foreground)
                            .child(if on { "On" } else { "Off" }),
                    ),
            )
            .child(
                Switch::new(SharedString::from(format!("opt-{}", d.id)), on)
                    .size(SwitchSize::Sm)
                    .on_change(move |v, _, cx| {
                        let id = id.clone();
                        weak.update(cx, |this, cx| {
                            this.set_option(&id, OptionValue::Bool(v), cx)
                        })
                        .ok();
                    }),
            )
            .into_any_element()
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let store = self.store.read(cx);
        let providers = store.snapshot().providers.clone();
        let current = self.current_model(&providers, cx);
        let mode = self
            .composer
            .upgrade()
            .map(|c| c.read(cx).runtime_mode(cx))
            .unwrap_or(permissions::DEFAULT_MODE);
        let mut footer = h_flex()
            .flex_wrap()
            .gap(px(8.))
            .border_t_1()
            .border_color(t.border.opacity(0.7))
            .bg(t.muted.opacity(0.4))
            .px(px(12.))
            .py(px(8.));
        if let Some((_, m, sel)) = &current
            && let Some(fast) = model::fast_descriptor(m)
        {
            let on = bool_value(fast, sel);
            let id = fast.id.clone();
            let segment = |label: &'static str, bolt: bool, checked: bool| {
                h_flex()
                    .id(label)
                    .h(px(24.))
                    .px(px(8.))
                    .gap(px(4.))
                    .rounded(radius::MD)
                    .text_scale(TextSize::Xs)
                    .cursor_pointer()
                    .text_color(if checked {
                        t.foreground
                    } else {
                        t.muted_foreground
                    })
                    .when(checked, |el| el.bg(t.input.opacity(0.72)))
                    .hover(|s| s.text_color(t.foreground))
                    .when(bolt, |el| el.child(Icon::new(IconName::Zap).size(px(12.))))
                    .child(label)
            };
            let (id_a, id_b) = (id.clone(), id);
            footer = footer.child(
                h_flex()
                    .rounded(radius::LG)
                    .bg(t.input.opacity(0.4))
                    .p(px(2.))
                    .child(segment("Standard", false, !on).on_click(cx.listener(
                        move |this, _, _, cx| this.set_option(&id_a, OptionValue::Bool(false), cx),
                    )))
                    .child(segment("Fast", true, on).on_click(cx.listener(
                        move |this, _, _, cx| this.set_option(&id_b, OptionValue::Bool(true), cx),
                    ))),
            );
        }
        let bounds = std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
        let click_bounds = bounds.clone();
        let composer = self.composer.clone();
        footer
            .child(
                h_flex()
                    .id("picker-permissions")
                    .ml_auto()
                    .relative()
                    .h(px(28.))
                    .px(px(8.))
                    .gap(px(6.))
                    .rounded(radius::MD)
                    .text_scale(TextSize::Xs)
                    .text_color(t.muted_foreground)
                    .cursor_pointer()
                    .hover(|s| s.bg(t.foreground.opacity(0.09)))
                    .child(crate::ui::bounds_probe(bounds))
                    .child(Icon::new(permissions::icon(mode)).size(px(14.)))
                    .child(permissions::label(mode))
                    .child(Icon::new(IconName::ChevronUp).size(px(12.)))
                    .on_click(move |_, window, cx| {
                        let composer = composer.clone();
                        permissions::PermissionsView::open(
                            "picker-permissions-popover",
                            click_bounds.get(),
                            mode,
                            std::rc::Rc::new(move |mode, _, cx| {
                                if let Some(c) = composer.upgrade() {
                                    c.update(cx, |c, cx| c.set_runtime_mode(mode, cx));
                                }
                            }),
                            window,
                            cx,
                        );
                    }),
            )
            .into_any_element()
    }
}

/// lucide `star`, filled when `filled`.
fn svg_star(size: f32, filled: bool, color: gpui::Hsla) -> gpui::Svg {
    let path = if filled {
        crate::assets::icon_path(IconName::TernStarFilled)
    } else {
        crate::assets::icon_path(IconName::Star)
    };
    gpui::svg()
        .path(path)
        .size(px(size))
        .flex_none()
        .text_color(color)
}

impl Focusable for ModelPickerView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ModelPickerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.tern_theme().clone();
        let searching = !self.query.trim().is_empty();
        let sidebar = (!searching).then(|| self.render_sidebar(cx));
        let list = self.render_list(window, cx);
        let effort = self.render_effort(cx);
        let footer = self.render_footer(cx);
        v_flex()
            .id("model-picker")
            .key_context("ModelPicker")
            .track_focus(&self.focus)
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.move_highlight(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.move_highlight(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                this.confirm(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|_, _: &Escape, window, cx| {
                PopupLayer::close(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &MoveLeft, _, cx| {
                if this.focus_area == Focus::Effort {
                    this.focus_area = Focus::List;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveRight, _, cx| {
                let at_end = {
                    let s = this.search.read(cx);
                    s.cursor() >= s.value().len()
                };
                if this.focus_area == Focus::List && at_end {
                    this.focus_area = Focus::Effort;
                    this.effort_highlight = this.selected_effort_index(cx).or(Some(0));
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                let k = &e.keystroke;
                if k.key == "escape" {
                    PopupLayer::close(window, cx);
                    cx.stop_propagation();
                    return;
                }
                if k.modifiers.control && k.modifiers.shift && (k.key == "up" || k.key == "down") {
                    this.cycle_tab(if k.key == "up" { -1 } else { 1 }, cx);
                    cx.stop_propagation();
                    return;
                }
                if k.modifiers.control
                    && !k.modifiers.shift
                    && let Some(n) = k.key.parse::<usize>().ok().filter(|n| (1..=9).contains(n))
                {
                    let rows = this.rows(cx);
                    let models: Vec<_> = rows
                        .iter()
                        .filter_map(|r| match r {
                            Row::Model { instance, slug } => Some((instance.clone(), slug.clone())),
                            Row::ShowMore => None,
                        })
                        .collect();
                    if let Some((i, s)) = models.get(n - 1) {
                        this.pick_model(i, s, window, cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .relative()
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.glass.dropdown_border)
            .bg(theme.glass.dropdown)
            .shadow(theme.shadows.popup())
            .text_color(theme.colors.popover_foreground)
            .font_sans()
            .overflow_hidden()
            .w(px(if searching {
                LIST_WIDTH + EFFORT_WIDTH + 2.
            } else {
                SIDEBAR_WIDTH + LIST_WIDTH + EFFORT_WIDTH + 2.
            }))
            .child(
                h_flex()
                    .items_start()
                    .h(px(BODY_HEIGHT))
                    .children(sidebar)
                    .child(list)
                    .child(effort),
            )
            .child(footer)
    }
}

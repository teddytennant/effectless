//! One module per settings page. Each `render` returns the page's sections, top to
//! bottom; the view stacks them 32px apart in the page column.

pub mod about;
pub mod appearance;
pub mod archive;
pub mod chat;
pub mod diffs;
pub mod general;
pub mod keybindings;
pub mod maintenance;
pub mod projects;
pub mod providers;
pub mod source_control;
pub mod threads;
pub mod worktrees;

use gpui::{
    AnyElement, App, Context, ElementId, IntoElement, ParentElement as _, SharedString,
    Styled as _, Window, div, px,
};
use tern_core::settings::{ClientSettings, Settings};

use super::SettingsView;
use super::model::SettingsModel;
use super::widgets::{Row, select};
use crate::app_state::SettingsPage;
use crate::store::Store;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize};
use crate::ui::{Button, ButtonSize, ButtonVariant, IconName, Menu, MenuItem, Switch, h_flex};

pub fn render(
    view: &mut SettingsView,
    page: SettingsPage,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Vec<AnyElement> {
    match page {
        SettingsPage::General => general::render(view, window, cx),
        SettingsPage::Appearance => appearance::render(view, window, cx),
        SettingsPage::Chat => chat::render(view, window, cx),
        SettingsPage::Diffs => diffs::render(view, window, cx),
        SettingsPage::Keybindings => keybindings::render(view, window, cx),
        SettingsPage::Providers => providers::render(view, window, cx),
        SettingsPage::Threads => threads::render(view, window, cx),
        SettingsPage::Projects => projects::render(view, window, cx),
        SettingsPage::SourceControl => source_control::render(view, window, cx),
        SettingsPage::Worktrees => worktrees::render(view, window, cx),
        SettingsPage::Maintenance => maintenance::render(view, window, cx),
        SettingsPage::About => about::render(view, window, cx),
        SettingsPage::OpenSourceLicenses => super::licenses::render(view, window, cx),
        SettingsPage::Archive => archive::render(view, window, cx),
    }
}

/// The Advanced fold that holds a searchable row, so search can open it first.
pub fn fold_for(id: &str) -> Option<&'static str> {
    match id {
        "show-skills" => Some("chat-composer-advanced"),
        "panel-animations" | "composer-context" => Some("appearance-interface-advanced"),
        "separate-fonts" => Some("appearance-typography-advanced"),
        "thread-title-instructions" | "response-streaming" => Some("threads-textgen-advanced"),
        "submodules" => Some("worktrees-creation-advanced"),
        "worktree-settled" | "worktree-unchanged" | "worktree-keep-when" => {
            Some("worktrees-cleanup-advanced")
        }
        "pr-templates" | "writer-model" => Some("source-control-advanced"),
        _ => None,
    }
}

// --- write helpers -----------------------------------------------------------------

pub fn write_client(
    model: &gpui::Entity<SettingsModel>,
    f: impl FnOnce(&mut ClientSettings),
    window: &mut Window,
    cx: &mut App,
) {
    model.update(cx, |m, cx| m.update_client(f, window, cx));
}

pub fn write_server(
    model: &gpui::Entity<SettingsModel>,
    f: impl FnOnce(&mut Settings),
    cx: &mut App,
) {
    model.update(cx, |m, cx| m.update_settings(f, cx));
}

/// A client-setting switch row with a reset button.
pub fn client_switch(
    view: &SettingsView,
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    get: fn(&ClientSettings) -> bool,
    set: fn(&mut ClientSettings, bool),
    cx: &App,
) -> Row {
    let model = view.model().clone();
    let value = get(model.read(cx).client());
    let default = get(&ClientSettings::default());
    let m2 = model.clone();
    Row::new(SharedString::from(id), title)
        .description(desc, cx)
        .highlight(view.is_pulsing(id))
        .probe(view.probe_for(id))
        .reset_when(value != default, move |window, cx| {
            write_client(&m2, |c| set(c, default), window, cx)
        })
        .control(
            Switch::new(SharedString::from(format!("{id}-switch")), value)
                .on_change(move |v, window, cx| write_client(&model, |c| set(c, v), window, cx)),
        )
}

/// A server-setting switch row with a reset button.
pub fn server_switch(
    view: &SettingsView,
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    get: fn(&Settings) -> bool,
    set: fn(&mut Settings, bool),
    cx: &App,
) -> Row {
    let model = view.model().clone();
    let value = get(model.read(cx).settings());
    let default = get(&Settings::default());
    let m2 = model.clone();
    Row::new(SharedString::from(id), title)
        .description(desc, cx)
        .highlight(view.is_pulsing(id))
        .probe(view.probe_for(id))
        .reset_when(value != default, move |_, cx| {
            write_server(&m2, |s| set(s, default), cx)
        })
        .control(
            Switch::new(SharedString::from(format!("{id}-switch")), value)
                .on_change(move |v, _, cx| write_server(&model, |s| set(s, v), cx)),
        )
}

/// A select control over enum values, by position in `options`.
pub fn enum_select<T: PartialEq + Copy + 'static>(
    id: impl Into<ElementId>,
    options: &'static [(T, &'static str)],
    value: T,
    width: f32,
    on_pick: impl Fn(T, &mut Window, &mut App) + 'static,
) -> crate::ui::Select {
    let selected = options
        .iter()
        .position(|(v, _)| *v == value)
        .unwrap_or(0)
        .to_string();
    select(
        id,
        options
            .iter()
            .enumerate()
            .map(|(i, (_, l))| (SharedString::from(i.to_string()), SharedString::from(*l)))
            .collect(),
        selected,
        width,
        move |v, window, cx| {
            if let Some((value, _)) = v.parse::<usize>().ok().and_then(|i| options.get(i)) {
                on_pick(*value, window, cx);
            }
        },
    )
}

/// A client-setting select row.
#[allow(clippy::too_many_arguments)]
pub fn client_select<T: PartialEq + Copy + 'static>(
    view: &SettingsView,
    id: &'static str,
    title: &'static str,
    desc: impl Into<SharedString>,
    options: &'static [(T, &'static str)],
    width: f32,
    get: fn(&ClientSettings) -> T,
    set: fn(&mut ClientSettings, T),
    cx: &App,
) -> Row {
    let model = view.model().clone();
    let value = get(model.read(cx).client());
    let default = get(&ClientSettings::default());
    let m2 = model.clone();
    Row::new(SharedString::from(id), title)
        .description(desc, cx)
        .highlight(view.is_pulsing(id))
        .probe(view.probe_for(id))
        .reset_when(value != default, move |window, cx| {
            write_client(&m2, |c| set(c, default), window, cx)
        })
        .control(enum_select(
            SharedString::from(format!("{id}-select")),
            options,
            value,
            width,
            move |v, window, cx| write_client(&model, |c| set(c, v), window, cx),
        ))
}

/// A server-setting select row.
#[allow(clippy::too_many_arguments)]
pub fn server_select<T: PartialEq + Copy + 'static>(
    view: &SettingsView,
    id: &'static str,
    title: &'static str,
    desc: impl Into<SharedString>,
    options: &'static [(T, &'static str)],
    width: f32,
    get: fn(&Settings) -> T,
    set: fn(&mut Settings, T),
    cx: &App,
) -> Row {
    let model = view.model().clone();
    let value = get(model.read(cx).settings());
    let default = get(&Settings::default());
    let m2 = model.clone();
    Row::new(SharedString::from(id), title)
        .description(desc, cx)
        .highlight(view.is_pulsing(id))
        .probe(view.probe_for(id))
        .reset_when(value != default, move |_, cx| {
            write_server(&m2, |s| set(s, default), cx)
        })
        .control(enum_select(
            SharedString::from(format!("{id}-select")),
            options,
            value,
            width,
            move |v, _, cx| write_server(&model, |s| set(s, v), cx),
        ))
}

/// A text field committed on blur/Enter, width `w`.
#[allow(clippy::too_many_arguments)]
pub fn server_text(
    view: &mut SettingsView,
    id: &'static str,
    placeholder: &str,
    width: f32,
    get: fn(&Settings) -> String,
    set: fn(&mut Settings, String),
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let model = view.model().clone();
    let current = get(model.read(cx).settings());
    let state = view.input(
        format!("server-{id}"),
        || current,
        placeholder,
        move |this, value, _, cx| {
            let value = value.trim().to_string();
            let model = this.model().clone();
            write_server(&model, |s| set(s, value), cx);
        },
        window,
        cx,
    );
    div()
        .w(px(width))
        .child(crate::ui::TextField::new(&state).size(crate::ui::InputSize::Sm))
        .into_any_element()
}

/// `Applying settings for [All projects v]` (§1.7, single environment).
pub fn scope_sentence(view: &SettingsView, cx: &mut Context<SettingsView>) -> AnyElement {
    let t = *cx.tokens();
    let store = Store::global(cx);
    let projects = store.read(cx).snapshot().projects.clone();
    let current = view
        .scope_project
        .as_ref()
        .and_then(|id| projects.iter().find(|p| &p.id == id))
        .map(|p| p.title.clone())
        .unwrap_or_else(|| "All projects".into());
    let entity = cx.entity();
    let selected = view.scope_project.clone();
    h_flex()
        .px(px(16.))
        .flex_wrap()
        .gap(px(6.))
        .text_scale(TextSize::Sm)
        .text_color(t.muted_foreground)
        .child("Applying settings for")
        .child(crate::ui::DropdownMenu::new(
            "settings-scope-project",
            move |_, _| {
                let mut menu = Menu::new().min_width(px(224.));
                let e = entity.clone();
                menu = menu.item(
                    MenuItem::radio("All projects", selected.is_none()).on_select(move |_, cx| {
                        e.update(cx, |v, cx| {
                            v.scope_project = None;
                            cx.notify();
                        })
                    }),
                );
                for p in &projects {
                    let e = entity.clone();
                    let id = p.id.clone();
                    let label = if p.archived_at.is_some() {
                        format!("{} (archived)", p.title)
                    } else {
                        p.title.clone()
                    };
                    menu = menu.item(
                        MenuItem::radio(label, selected.as_deref() == Some(p.id.as_str()))
                            .on_select(move |_, cx| {
                                let id = id.clone();
                                e.update(cx, |v, cx| {
                                    v.scope_project = Some(id);
                                    cx.notify();
                                })
                            }),
                    );
                }
                menu
            },
            move |open| {
                Button::new("settings-scope-trigger")
                    .variant(ButtonVariant::Outline)
                    .size(ButtonSize::Sm)
                    .label(current)
                    .icon_end(IconName::ChevronDown)
                    .pressed(open)
                    .into_any_element()
            },
        ))
        .into_any_element()
}

/// 14px muted text in a control slot.
pub fn muted_text(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    div()
        .text_scale(TextSize::Sm)
        .text_color(cx.tokens().muted_foreground)
        .child(text.into())
        .into_any_element()
}

/// A right-aligned group for an Advanced fold's rows.
pub fn fold(
    view: &SettingsView,
    key: &'static str,
    rows: impl FnOnce() -> Vec<AnyElement>,
    cx: &mut Context<SettingsView>,
) -> Vec<AnyElement> {
    let open = view.fold_open(key);
    let entity = cx.entity();
    let trigger = super::widgets::advanced_trigger(
        SharedString::from(key),
        "Advanced",
        open,
        move |_, cx| entity.update(cx, |v, cx| v.toggle_fold(key, cx)),
        cx,
    );
    let mut out = vec![trigger];
    if open {
        out.extend(rows());
    }
    out
}

pub fn when_el(show: bool, el: impl FnOnce() -> AnyElement) -> Option<AnyElement> {
    show.then(el)
}

/// A titled card section from prebuilt rows.
pub fn section_of(
    id: &'static str,
    title: &'static str,
    rows: Vec<AnyElement>,
    _cx: &App,
) -> AnyElement {
    use gpui::ParentElement as _;
    super::widgets::Section::new(id, title)
        .children(rows)
        .into_any_element()
}

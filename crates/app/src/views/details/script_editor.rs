//! The project script editor dialog ("Add Action" / "Edit Action", spec/05 §6.4).

use std::path::PathBuf;

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, Keystroke, ParentElement as _, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::InputState;
use tern_core::settings::{KEYBINDINGS_FILE, KeybindingRule, save_keybindings};
use tern_core::{ProjectScript, ProjectScriptIcon};

use super::dialogs::refresh_dialogs;
use super::workspace::script_icon;
use super::{Ctx, DetailsCard, logic};
use crate::theme::{ActiveTheme as _, TextExt as _, radius};
use crate::ui::input::TextareaState;
use crate::ui::popover::close_popups;
use crate::ui::{
    AlertDialog, Button, ButtonSize, ButtonVariant, Dialog, DialogFooter, Dialogs, Icon, Popover,
    PopoverPadding, Switch, TextField, TextareaField, edge_line,
};

const ICONS: [(ProjectScriptIcon, &str); 6] = [
    (ProjectScriptIcon::Play, "Play"),
    (ProjectScriptIcon::Test, "Test"),
    (ProjectScriptIcon::Lint, "Lint"),
    (ProjectScriptIcon::Configure, "Configure"),
    (ProjectScriptIcon::Build, "Build"),
    (ProjectScriptIcon::Debug, "Debug"),
];

pub(super) struct ScriptEditor {
    name: Entity<InputState>,
    command: Entity<TextareaState>,
    preview: Entity<InputState>,
    icon: ProjectScriptIcon,
    key: Option<String>,
    key_focus: FocusHandle,
    setup: bool,
    blocking: bool,
    settle: bool,
    stop: bool,
    auto_preview: bool,
    error: Option<String>,
    editing: Option<ProjectScript>,
    project_id: String,
    scripts: Vec<ProjectScript>,
    userdata: Option<PathBuf>,
    id: usize,
}

/// `mod+shift+t` from a keystroke (T3's keybinding syntax; `mod` is Ctrl here).
pub fn chord(k: &Keystroke) -> Option<String> {
    let key = k.key.as_str();
    if matches!(
        key,
        "control" | "shift" | "alt" | "platform" | "function" | ""
    ) {
        return None;
    }
    let mut parts = Vec::new();
    if k.modifiers.control {
        parts.push("mod".to_string());
    }
    if k.modifiers.platform {
        parts.push("meta".into());
    }
    if k.modifiers.alt {
        parts.push("alt".into());
    }
    if k.modifiers.shift {
        parts.push("shift".into());
    }
    parts.push(key.to_lowercase());
    Some(parts.join("+"))
}

/// `Ctrl+Shift+T` for a chord.
pub fn chord_label(chord: &str) -> String {
    chord
        .split('+')
        .map(|p| match p {
            "mod" | "ctrl" => "Ctrl".to_string(),
            "meta" => "Super".into(),
            "alt" => "Alt".into(),
            "shift" => "Shift".into(),
            k if k.chars().count() == 1 => k.to_uppercase(),
            k => {
                let mut c = k.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// The user's own rules (not merged with defaults), with this script's binding
/// replaced.
fn write_binding(userdata: &PathBuf, script_id: &str, key: Option<&str>) -> Result<(), String> {
    let path = userdata.join(KEYBINDINGS_FILE);
    let mut rules: Vec<KeybindingRule> = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| e.to_string())?,
        Err(_) => Vec::new(),
    };
    let command = format!("script.{script_id}.run");
    rules.retain(|r| r.command != command);
    if let Some(key) = key {
        rules.push(KeybindingRule::new(key, &command, None));
    }
    std::fs::create_dir_all(userdata).map_err(|e| e.to_string())?;
    save_keybindings(userdata, &rules).map_err(|e| e.to_string())
}

fn read_binding(userdata: &PathBuf, script_id: &str) -> Option<String> {
    let text = std::fs::read_to_string(userdata.join(KEYBINDINGS_FILE)).ok()?;
    let rules: Vec<KeybindingRule> = serde_json::from_str(&text).ok()?;
    let command = format!("script.{script_id}.run");
    rules
        .into_iter()
        .find(|r| r.command == command)
        .map(|r| r.key)
}

impl ScriptEditor {
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "tab" => return,
            "backspace" | "delete" if !event.keystroke.modifiers.modified() => self.key = None,
            "escape" if !event.keystroke.modifiers.modified() => {
                Dialogs::close(self.id, window, cx);
                return;
            }
            _ => match chord(&event.keystroke) {
                Some(c) => self.key = Some(c),
                None => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
        refresh_dialogs(cx);
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_string();
        let command = self.command.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.error = Some("Name is required.".into());
            refresh_dialogs(cx);
            return;
        }
        if command.is_empty() {
            self.error = Some("Command is required.".into());
            refresh_dialogs(cx);
            return;
        }
        let preview = self.preview.read(cx).value().trim().to_string();
        let mut scripts = self.scripts.clone();
        let id = match &self.editing {
            Some(s) => s.id.clone(),
            None => {
                let ids: Vec<String> = scripts.iter().map(|s| s.id.clone()).collect();
                logic::next_script_id(&name, &ids)
            }
        };
        let script = ProjectScript {
            id: id.clone(),
            name,
            command,
            icon: self.icon,
            run_on_worktree_create: self.setup,
            blocking_setup: self.setup && self.blocking,
            run_on_settle: self.settle,
            stop_on_settle: self.stop,
            preview_url: (!preview.is_empty()).then_some(preview.clone()),
            auto_open_preview: self.auto_preview && !preview.is_empty(),
        };
        match scripts.iter_mut().find(|s| s.id == id) {
            Some(existing) => *existing = script,
            None => scripts.push(script),
        }
        logic::release_claimed_roles(&mut scripts, &id);
        if let Some(userdata) = &self.userdata {
            let before = read_binding(userdata, &id);
            if before != self.key
                && let Err(e) = write_binding(userdata, &id, self.key.as_deref())
            {
                self.error = Some(format!("Failed to save action. {e}"));
                refresh_dialogs(cx);
                return;
            }
        }
        crate::store::Store::global(cx)
            .read(cx)
            .send(tern_core::Command::UpdateProjectScripts {
                project_id: self.project_id.clone(),
                scripts,
            });
        Dialogs::close(self.id, window, cx);
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(script) = self.editing.clone() else {
            return;
        };
        let editor_id = self.id;
        let project_id = self.project_id.clone();
        let remaining: Vec<ProjectScript> = self
            .scripts
            .iter()
            .filter(|s| s.id != script.id)
            .cloned()
            .collect();
        let userdata = self.userdata.clone();
        let confirm_id = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let cid = confirm_id.clone();
        let id = Dialogs::open_alert(window, cx, move |_, _| {
            let (cancel_id, delete_id) = (cid.clone(), cid.clone());
            let (project_id, remaining, userdata, script_id) = (
                project_id.clone(),
                remaining.clone(),
                userdata.clone(),
                script.id.clone(),
            );
            AlertDialog::new()
                .title(format!("Delete action \"{}\"?", script.name))
                .description("This action cannot be undone.")
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("script-delete-cancel")
                                .label("Cancel")
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Sm)
                                .on_click(move |_, window, cx| {
                                    Dialogs::close(cancel_id.get(), window, cx)
                                }),
                        )
                        .child(
                            Button::new("script-delete-confirm")
                                .label("Delete action")
                                .variant(ButtonVariant::Destructive)
                                .size(ButtonSize::Sm)
                                .on_click(move |_, window, cx| {
                                    if let Some(dir) = &userdata {
                                        let _ = write_binding(dir, &script_id, None);
                                    }
                                    crate::store::Store::global(cx).read(cx).send(
                                        tern_core::Command::UpdateProjectScripts {
                                            project_id: project_id.clone(),
                                            scripts: remaining.clone(),
                                        },
                                    );
                                    Dialogs::close(delete_id.get(), window, cx);
                                    Dialogs::close(editor_id, window, cx);
                                }),
                        ),
                )
        });
        confirm_id.set(id);
    }
}

impl DetailsCard {
    pub(super) fn open_script_editor(
        &mut self,
        ctx: &Ctx,
        editing: Option<ProjectScript>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let userdata = self.store.read(cx).engine().map(|e| e.paths().userdata());
        let e = editing.clone().unwrap_or_default();
        let name = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("Test");
            s.set_value(e.name.clone(), window, cx);
            s
        });
        let command = cx.new(|cx| {
            let mut s = TextareaState::new(window, cx).placeholder("bun test");
            s.set_value(e.command.clone(), window, cx);
            s
        });
        let preview = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("http://localhost:5173");
            s.set_value(e.preview_url.clone().unwrap_or_default(), window, cx);
            s
        });
        let key = match (&userdata, &editing) {
            (Some(dir), Some(s)) => read_binding(dir, &s.id),
            _ => None,
        };
        let state = cx.new(|cx| ScriptEditor {
            name: name.clone(),
            command,
            preview,
            icon: e.icon,
            key,
            key_focus: cx.focus_handle(),
            setup: e.run_on_worktree_create,
            blocking: e.blocking_setup,
            settle: e.run_on_settle,
            stop: e.stop_on_settle,
            auto_preview: e.auto_open_preview,
            error: None,
            editing,
            project_id: ctx.project.id.clone(),
            scripts: ctx.project.scripts.clone(),
            userdata,
            id: 0,
        });
        let st = state.clone();
        let id = Dialogs::open_alert(window, cx, move |window, cx| build(&st, window, cx));
        state.update(cx, |s, _| s.id = id);
        name.update(cx, |n, cx| n.focus(window, cx));
    }
}

fn switch_row(
    id: &'static str,
    label: &'static str,
    checked: bool,
    disabled: bool,
    state: &Entity<ScriptEditor>,
    set: fn(&mut ScriptEditor, bool),
    cx: &App,
) -> gpui::AnyElement {
    let t = cx.tokens();
    let dark = cx.tern_theme().is_dark();
    let st = state.clone();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .px(px(12.))
        .py(px(8.))
        .rounded(radius::MD)
        .text_scale(crate::theme::TextSize::Sm)
        .map(|el| {
            if dark {
                el.bg(gpui::white().opacity(0.035))
            } else {
                el.border_1()
                    .border_color(t.border.opacity(t.border.a * 0.7))
            }
        })
        .when(disabled, |el| el.opacity(0.6))
        .child(label)
        .child(
            Switch::new(id, checked)
                .disabled(disabled)
                .on_change(move |v, _, cx| {
                    st.update(cx, |s, _| set(s, v));
                    refresh_dialogs(cx);
                }),
        )
        .into_any_element()
}

fn build(state: &Entity<ScriptEditor>, window: &mut Window, cx: &mut App) -> Dialog {
    let t = *cx.tokens();
    let s = state.read(cx);
    let is_edit = s.editing.is_some();
    let has_preview = !s.preview.read(cx).value().trim().is_empty();
    let field = |label: &'static str, input: gpui::AnyElement| {
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_scale(crate::theme::TextSize::Sm)
                    .font_medium()
                    .child(label),
            )
            .child(input)
    };
    let help = |text: &'static str| {
        div()
            .text_scale(crate::theme::TextSize::Xs)
            .text_color(t.muted_foreground)
            .child(text)
    };

    let icon_state = state.clone();
    let icon_button = Popover::new("script-icon-picker", {
        let icon = s.icon;
        move |open| {
            div()
                .id("script-icon-button")
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .size(px(36.))
                .rounded(radius::LG)
                .border_1()
                .border_color(t.input)
                .bg(t.popover)
                .when(open, |el| el.bg(t.accent))
                .cursor_pointer()
                .child(Icon::new(script_icon(icon)).size(px(18.)))
                .into_any_element()
        }
    })
    .padding(PopoverPadding::Compact)
    .content(move |_, cx| {
        let t = *cx.tokens();
        let selected = icon_state.read(cx).icon;
        let primary = t.primary;
        let cells = ICONS.iter().enumerate().map(|(ix, (icon, label))| {
            let st = icon_state.clone();
            let icon = *icon;
            let on = icon == selected;
            let accent = t.accent;
            div()
                .id(("script-icon-cell", ix))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .p(px(8.))
                .rounded(radius::MD)
                .border_1()
                .cursor_pointer()
                .map(|el| {
                    if on {
                        el.border_color(primary.opacity(0.7))
                            .bg(primary.opacity(0.1))
                    } else {
                        el.border_color(t.border.opacity(t.border.a * 0.7))
                            .hover(move |s| s.bg(accent.opacity(0.6 * accent.a)))
                    }
                })
                .child(Icon::new(script_icon(icon)).size(px(16.)))
                .child(div().text_scale(crate::theme::TextSize::Xs).child(*label))
                .on_click(move |_, window, cx| {
                    st.update(cx, |s, _| s.icon = icon);
                    close_popups(window, cx);
                    refresh_dialogs(cx);
                })
        });
        div()
            .grid()
            .grid_cols(3)
            .gap(px(8.))
            .children(cells)
            .into_any_element()
    });

    let key_focused = s.key_focus.is_focused(window);
    let key_field = {
        let st = state.clone();
        let label = s.key.as_deref().map(chord_label);
        div()
            .id("script-keybinding")
            .track_focus(&s.key_focus)
            .relative()
            .flex()
            .items_center()
            .h(px(32.))
            .px(px(11.))
            .rounded(radius::LG)
            .border_1()
            .border_color(if key_focused { t.ring } else { t.input })
            .when(key_focused, |el| {
                el.shadow(crate::ui::field_ring(cx.states().ring_focus))
            })
            .text_scale(crate::theme::TextSize::Sm)
            .cursor_text()
            .on_key_down(move |e, window, cx| st.update(cx, |s, cx| s.on_key(e, window, cx)))
            .on_click({
                let focus = s.key_focus.clone();
                move |_, window, cx| focus.focus(window, cx)
            })
            .child(match label {
                Some(l) => div().child(l),
                None => div().text_color(t.placeholder).child("Press shortcut"),
            })
            .child(edge_line(radius::LG, cx))
    };

    let save_state = state.clone();
    let cancel_id = s.id;
    let delete_state = state.clone();
    let footer = DialogFooter::new()
        .when(is_edit, |f| {
            f.child(
                div().mr_auto().child(
                    Button::new("script-delete")
                        .label("Delete")
                        .variant(ButtonVariant::DestructiveOutline)
                        .size(ButtonSize::Sm)
                        .on_click(move |_, window, cx| {
                            delete_state.update(cx, |s, cx| s.delete(window, cx))
                        }),
                ),
            )
        })
        .child(
            Button::new("script-cancel")
                .label("Cancel")
                .variant(ButtonVariant::Outline)
                .size(ButtonSize::Sm)
                .on_click(move |_, window, cx| Dialogs::close(cancel_id, window, cx)),
        )
        .child(
            Button::new("script-save")
                .label(if is_edit {
                    "Save changes"
                } else {
                    "Save action"
                })
                .size(ButtonSize::Sm)
                .on_click(move |_, window, cx| save_state.update(cx, |s, cx| s.save(window, cx))),
        );

    Dialog::new()
        .title(if is_edit { "Edit Action" } else { "Add Action" })
        .description(
            "Actions are project-scoped commands you can run from the top bar or keybindings.",
        )
        .close_button(false)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(field(
                    "Name",
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(icon_button)
                        .child(div().flex_1().child(TextField::new(&s.name)))
                        .into_any_element(),
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(field("Keybinding", key_field.into_any_element()))
                        .child(help("Press a shortcut. Use `Backspace` to clear. Shortcuts are environment-wide. Projects using the same action share its shortcut.")),
                )
                .child(field("Command", TextareaField::new(&s.command).into_any_element()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(field(
                            "Preview URL (optional)",
                            TextField::new(&s.preview).into_any_element(),
                        ))
                        .child(help("Open this URL in the in-app preview when this action runs.")),
                )
                .child(switch_row(
                    "script-setup",
                    "Run automatically on worktree creation",
                    s.setup,
                    false,
                    state,
                    |s, v| {
                        s.setup = v;
                        if !v {
                            s.blocking = false;
                        }
                    },
                    cx,
                ))
                .child(switch_row(
                    "script-blocking",
                    "Wait for it to finish before the agent starts",
                    s.blocking,
                    !s.setup,
                    state,
                    |s, v| s.blocking = v,
                    cx,
                ))
                .child(switch_row(
                    "script-settle",
                    "Run in the thread's worktree when the thread settles",
                    s.settle,
                    false,
                    state,
                    |s, v| s.settle = v,
                    cx,
                ))
                .child(switch_row(
                    "script-stop",
                    "Stop this action when the thread settles",
                    s.stop,
                    false,
                    state,
                    |s, v| s.stop = v,
                    cx,
                ))
                .child(switch_row(
                    "script-preview",
                    "Open preview automatically when this action runs",
                    s.auto_preview,
                    !has_preview,
                    state,
                    |s, v| s.auto_preview = v,
                    cx,
                ))
                .children(s.error.clone().map(|e| {
                    div()
                        .text_scale(crate::theme::TextSize::Sm)
                        .text_color(t.destructive_foreground)
                        .child(e)
                })),
        )
        .footer(footer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_round_trip_to_labels() {
        let k = Keystroke::parse("ctrl-shift-t").unwrap();
        assert_eq!(chord(&k).as_deref(), Some("mod+shift+t"));
        assert_eq!(chord_label("mod+shift+t"), "Ctrl+Shift+T");
        assert_eq!(chord_label("alt+f5"), "Alt+F5");
        let bare = Keystroke::parse("shift").unwrap();
        assert_eq!(chord(&bare), None);
    }

    #[test]
    fn bindings_file_keeps_other_rules() {
        let dir = std::env::temp_dir().join(format!("tern-kb-{}", tern_core::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(KEYBINDINGS_FILE),
            r#"[{"key":"mod+1","command":"script.other.run"}]"#,
        )
        .unwrap();
        write_binding(&dir, "test", Some("mod+shift+t")).unwrap();
        assert_eq!(read_binding(&dir, "test").as_deref(), Some("mod+shift+t"));
        assert_eq!(read_binding(&dir, "other").as_deref(), Some("mod+1"));
        write_binding(&dir, "test", None).unwrap();
        assert_eq!(read_binding(&dir, "test"), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}

//! The Workspace section: run context (§4a.1b, §4a.3), Open in (§5), project
//! scripts (§6).

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_base::Align;
use tern_core::{ProjectScript, ProjectScriptIcon, ThreadEnvMode};

use super::editors::{self, FILE_MANAGER};
use super::git::StatusRead;
use super::{Ctx, Density, DetailsCard, logic, style};
use crate::app_state::{DraftWorkspace, TerminalRequest};
use crate::theme::{ActiveTheme as _, TextExt as _};
use crate::ui::kbd::shortcut_text;
use crate::ui::menu::{Menu, MenuItem, item_base, open_menu};
use crate::ui::popup::{PopupAnchor, PopupLayer};
use crate::ui::{
    Icon, IconName, Toast, Toasts, Tooltip, TooltipExt as _, TooltipSide, bounds_probe,
};

const ENV_MENU: &str = "details-env-menu";
const EDITOR_MENU: &str = "details-editor-menu";
const SCRIPTS_MENU: &str = "details-scripts-menu";

pub fn script_icon(icon: ProjectScriptIcon) -> IconName {
    match icon {
        ProjectScriptIcon::Play => IconName::Play,
        ProjectScriptIcon::Test => IconName::FlaskConical,
        ProjectScriptIcon::Lint => IconName::ListChecks,
        ProjectScriptIcon::Configure => IconName::Wrench,
        ProjectScriptIcon::Build => IconName::Hammer,
        ProjectScriptIcon::Debug => IconName::Bug,
    }
}

fn editor_icon(id: &str) -> IconName {
    match id {
        FILE_MANAGER => IconName::Folder,
        "nvim" => IconName::SquareTerminal,
        _ => IconName::Code,
    }
}

/// A menu row the caller lays out (two-line items, rows with hover actions).
pub(crate) fn custom_row(
    id: impl Into<ElementId>,
    disabled: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let accent = cx.tokens().accent;
    let fg = cx.tokens().accent_foreground;
    item_base(false, cx)
        .id(id)
        .when(disabled, |el| el.opacity(crate::ui::DISABLED_OPACITY))
        .when(!disabled, |el| {
            el.cursor_pointer()
                .hover(move |s| s.bg(accent).text_color(fg))
        })
}

fn anchor_below(bounds: gpui::Bounds<gpui::Pixels>, align: Align) -> PopupAnchor {
    PopupAnchor::below(bounds, align)
}

impl DetailsCard {
    fn offers_workspace_modes(&self) -> bool {
        matches!(
            self.git.as_ref().and_then(|g| g.read.as_ref()),
            Some(Ok(StatusRead::Repo(_)))
        )
    }

    fn env_locked(&self, ctx: &Ctx) -> bool {
        ctx.env_locked() || !self.offers_workspace_modes()
    }

    fn workspace_icon(ctx: &Ctx) -> IconName {
        if ctx.worktree_path().is_some() {
            IconName::FolderGit
        } else if ctx.env_mode() == ThreadEnvMode::Worktree {
            IconName::FolderGit2
        } else {
            IconName::Folder
        }
    }

    fn workspace_label(ctx: &Ctx) -> &'static str {
        match (ctx.env_mode(), ctx.worktree_path().is_some()) {
            (ThreadEnvMode::Worktree, false) => "New worktree",
            (_, true) => "Current worktree",
            _ => "Current checkout",
        }
    }

    /// (rows at full density, rows at compact density).
    pub(super) fn workspace_rows(&self, _ctx: &Ctx, _cx: &App) -> (usize, usize) {
        (3, 2)
    }

    pub(super) fn render_workspace(
        &mut self,
        ctx: &Ctx,
        density: Density,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Vec<AnyElement>> {
        let mut rows = Vec::new();
        if density == Density::Full {
            rows.push(self.env_row(ctx, cx));
        }
        if density != Density::Essential {
            rows.push(self.open_in_row(ctx, cx));
        }
        rows.push(self.scripts_row(ctx, cx));
        Some(rows)
    }

    // ---- run context ---------------------------------------------------------

    fn env_row(&mut self, ctx: &Ctx, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let locked = self.env_locked(ctx);
        let dark = cx.tern_theme().is_dark();
        let path = ctx.cwd().to_string_lossy().to_string();
        let tooltip = if ctx.is_draft() && ctx.choosing_base() {
            Self::workspace_label(ctx).to_string()
        } else {
            path
        };
        let workspace_icon = div()
            .id("details-env-workspace-icon")
            .flex()
            .flex_none()
            .child(style::icon(Self::workspace_icon(ctx), cx))
            .t3_tooltip(Tooltip::text(tooltip));
        let machine = machine_label();
        let content = gpui::div()
            .flex()
            .items_center()
            .min_w_0()
            .child(
                div().flex_none().mx(px(-1.)).child(
                    Icon::new(IconName::Monitor)
                        .size(px(18.))
                        .color(t.muted_foreground),
                ),
            )
            .child(
                style::label(machine)
                    .flex_1()
                    .when(locked && dark, |el| el.text_color(t.muted_foreground)),
            )
            .child(workspace_icon);
        if locked {
            return style::locked_row(cx)
                .child(content.flex_1())
                .into_any_element();
        }
        let open = PopupLayer::is_open(&ENV_MENU.into(), cx);
        let probe = self.probe("env");
        let ctx = ctx.clone();
        style::select_row("details-env", open, false, content, cx)
            .relative()
            .child(bounds_probe(probe))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    if PopupLayer::is_open(&ENV_MENU.into(), cx) {
                        PopupLayer::close(window, cx);
                    } else {
                        this.open_env_menu(&ctx, window, cx);
                    }
                }),
            )
            .into_any_element()
    }

    pub(super) fn open_env_menu(&mut self, ctx: &Ctx, window: &mut Window, cx: &mut Context<Self>) {
        if self.env_locked(ctx) {
            return;
        }
        let bounds = self.bounds("env");
        let mode = ctx.env_mode();
        let local_label = if ctx.worktree_path().is_some() && mode == ThreadEnvMode::Local {
            "Current worktree"
        } else {
            "Current checkout"
        };
        let project_id = ctx.project.id.clone();
        let current = ctx.draft.clone().unwrap_or_default();
        let previous = self.previous_worktree(ctx, cx);
        let app = self.app.clone();
        let set = {
            let app = app.clone();
            let project_id = project_id.clone();
            Rc::new(move |ws: DraftWorkspace, cx: &mut App| {
                let project_id = project_id.clone();
                app.update(cx, |s, cx| s.set_draft_workspace(&project_id, ws, cx));
            })
        };
        let mut menu = Menu::new()
            .min_width(bounds.size.width)
            .label("Run on")
            .item(
                MenuItem::radio(machine_label(), true)
                    .icon(IconName::Monitor)
                    .on_select(|_, _| {}),
            )
            .separator()
            .label("Workspace")
            .item({
                let set = set.clone();
                let current = current.clone();
                MenuItem::radio(local_label, mode == ThreadEnvMode::Local)
                    .icon(if ctx.worktree_path().is_some() {
                        IconName::FolderGit
                    } else {
                        IconName::Folder
                    })
                    .on_select(move |_, cx| {
                        set(
                            DraftWorkspace {
                                env_mode: ThreadEnvMode::Local,
                                start_from_origin: false,
                                ..current.clone()
                            },
                            cx,
                        )
                    })
            })
            .item({
                let set = set.clone();
                let current = current.clone();
                MenuItem::radio(
                    "New worktree",
                    mode == ThreadEnvMode::Worktree && ctx.worktree_path().is_none(),
                )
                .icon(IconName::FolderGit2)
                .on_select(move |_, cx| {
                    set(
                        DraftWorkspace {
                            env_mode: ThreadEnvMode::Worktree,
                            worktree_path: None,
                            start_from_origin: true,
                            ..current.clone()
                        },
                        cx,
                    )
                })
            });
        if let Some((branch, path)) = previous {
            let set = set.clone();
            menu = menu.item(MenuItem::custom(move |_, cx| {
                let t = *cx.tokens();
                let (branch, path, set) = (branch.clone(), path.clone(), set.clone());
                custom_row("details-env-previous", false, cx)
                    .items_start()
                    .gap(px(6.))
                    .child(
                        div().mt(px(4.)).child(
                            Icon::new(IconName::History)
                                .size(px(12.))
                                .color(t.muted_foreground),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child("Previous worktree")
                            .child(
                                div()
                                    .text_scale(crate::theme::TextSize::Xs)
                                    .text_color(t.muted_foreground)
                                    .child(logic::middle_truncate(&branch, 40)),
                            ),
                    )
                    .on_click(move |_, window, cx| {
                        PopupLayer::close(window, cx);
                        set(
                            DraftWorkspace {
                                env_mode: ThreadEnvMode::Worktree,
                                branch: Some(branch.clone()),
                                worktree_path: Some(path.clone()),
                                start_from_origin: false,
                            },
                            cx,
                        );
                    })
                    .into_any_element()
            }));
        }
        open_menu(
            ENV_MENU.into(),
            anchor_below(bounds, Align::Start),
            menu,
            window,
            cx,
        );
        cx.notify();
    }

    /// The most recently updated unarchived thread in the project with another
    /// worktree: (branch, path).
    fn previous_worktree(&self, ctx: &Ctx, cx: &App) -> Option<(String, String)> {
        let current = ctx.worktree_path();
        self.store
            .read(cx)
            .snapshot()
            .threads
            .iter()
            .filter(|t| t.thread.project_id == ctx.project.id && t.thread.archived_at.is_none())
            .filter(|t| {
                t.thread.worktree_path.is_some() && t.thread.worktree_path.as_deref() != current
            })
            .max_by_key(|t| t.thread.updated_at)
            .map(|t| {
                (
                    t.thread.branch.clone().unwrap_or_default(),
                    t.thread.worktree_path.clone().unwrap_or_default(),
                )
            })
    }

    // ---- open in -------------------------------------------------------------

    fn open_in_row(&mut self, ctx: &Ctx, cx: &mut Context<Self>) -> AnyElement {
        let available = self.editors.clone().unwrap_or_default();
        let preferred = editors::resolve_preferred(&available, editors::preferred()).cloned();
        let cwd = ctx.cwd();
        let cwd_ok = cwd.is_dir();
        let open = PopupLayer::is_open(&EDITOR_MENU.into(), cx);
        let (icon, label) = match &preferred {
            Some(e) => (editor_icon(e.id), format!("Open in {}", e.label)),
            None => (
                IconName::SquareArrowOutUpRight,
                "Open in editor".to_string(),
            ),
        };
        let disabled = preferred.is_none() || !cwd_ok;
        let probe = self.probe("openin");
        let ctx2 = ctx.clone();
        let tooltip_label = label.clone();
        style::split_group("details-openin", open, false, cx)
            .relative()
            .child(bounds_probe(probe))
            .child(
                style::split_primary("details-openin-primary", disabled, cx)
                    .child(style::icon(icon, cx))
                    .child(style::label(label))
                    .t3_tooltip(Tooltip::text(tooltip_label).side(TooltipSide::Top))
                    .when(!disabled, |el| {
                        el.on_click(cx.listener(move |_, _, _, cx| {
                            if let Some(editor) = preferred.clone() {
                                open_in_editor(editor, cwd.clone(), cx);
                            }
                        }))
                    }),
            )
            .child(style::split_separator("details-openin", open, cx))
            .child(
                style::split_secondary("details-openin-chevron", open, false, cx)
                    .t3_tooltip(Tooltip::text("Choose editor").side(TooltipSide::Top))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            if PopupLayer::is_open(&EDITOR_MENU.into(), cx) {
                                PopupLayer::close(window, cx);
                            } else {
                                this.open_editor_menu(&ctx2, window, cx);
                            }
                        }),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn open_editor_menu(
        &mut self,
        ctx: &Ctx,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = self.bounds("openin");
        let available = self.editors.clone().unwrap_or_default();
        let preferred = editors::resolve_preferred(&available, editors::preferred()).map(|e| e.id);
        let cwd_ok = ctx.cwd().is_dir();
        let mut menu = Menu::new().min_width(bounds.size.width);
        if available.is_empty() {
            menu = menu.item(MenuItem::new("No installed editors found").disabled(true));
        }
        for e in available {
            let id = e.id;
            let mut item = MenuItem::new(e.label)
                .icon(editor_icon(id))
                .disabled(!cwd_ok);
            if Some(id) == preferred {
                item = item.shortcut("Ctrl+O");
            }
            menu = menu.item(item.on_select(move |_, _| editors::set_preferred(id)));
        }
        open_menu(
            EDITOR_MENU.into(),
            anchor_below(bounds, Align::End),
            menu,
            window,
            cx,
        );
        cx.notify();
    }

    // ---- project scripts -------------------------------------------------------

    fn scripts_row(&mut self, ctx: &Ctx, cx: &mut Context<Self>) -> AnyElement {
        let scripts = ctx.project.scripts.clone();
        let last = self.last_script.get(&ctx.project.id).map(String::as_str);
        let primary = logic::primary_script(&scripts, last).cloned();
        let open = PopupLayer::is_open(&SCRIPTS_MENU.into(), cx);
        let probe = self.probe("scripts");
        if scripts.is_empty() {
            let ctx = ctx.clone();
            return style::row("details-add-script", false, cx)
                .child(
                    Icon::new(IconName::Plus)
                        .size(px(14.))
                        .color(cx.tokens().muted_foreground),
                )
                .child(style::label("Add project script").ml(px(2.)))
                .t3_tooltip(Tooltip::text("Add project script").side(TooltipSide::Top))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_script_editor(&ctx, None, window, cx)
                }))
                .into_any_element();
        }
        let ctx_menu = ctx.clone();
        let chevron = style::split_secondary("details-scripts-chevron", open, false, cx)
            .t3_tooltip(Tooltip::text("Script actions").side(TooltipSide::Top))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    if PopupLayer::is_open(&SCRIPTS_MENU.into(), cx) {
                        PopupLayer::close(window, cx);
                    } else {
                        this.open_scripts_menu(&ctx_menu, window, cx);
                    }
                }),
            );
        let primary_half = match primary {
            Some(script) => {
                let ctx = ctx.clone();
                let name = script.name.clone();
                style::split_primary("details-scripts-primary", false, cx)
                    .child(style::icon(script_icon(script.icon), cx))
                    .child(style::label(script.name.clone()))
                    .t3_tooltip(Tooltip::text(format!("Run {name}")).side(TooltipSide::Top))
                    .on_click(cx.listener(move |this, _, _, cx| this.run_script(&ctx, &script, cx)))
            }
            None => {
                let ctx = ctx.clone();
                style::split_primary("details-scripts-primary", false, cx)
                    .child(style::icon(IconName::Wrench, cx))
                    .child(style::label("Actions"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_scripts_menu(&ctx, window, cx)
                    }))
            }
        };
        style::split_group("details-scripts", open, false, cx)
            .relative()
            .child(bounds_probe(probe))
            .child(primary_half)
            .child(style::split_separator("details-scripts", open, cx))
            .child(chevron)
            .into_any_element()
    }

    pub(super) fn open_scripts_menu(
        &mut self,
        ctx: &Ctx,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = self.bounds("scripts");
        let this = cx.entity().downgrade();
        let mut menu = Menu::new().min_width(bounds.size.width);
        for (ix, script) in ctx.project.scripts.iter().enumerate() {
            let script = script.clone();
            let ctx = ctx.clone();
            let this = this.clone();
            menu = menu.item(MenuItem::custom(move |_, cx| {
                let t = *cx.tokens();
                let group: SharedString = format!("details-script-row-{ix}").into();
                let (run_this, edit_this) = (this.clone(), this.clone());
                let (run_ctx, edit_ctx) = (ctx.clone(), ctx.clone());
                let (run_script, edit_script) = (script.clone(), script.clone());
                custom_row(("details-script-row", ix), false, cx)
                    .group(group.clone())
                    .child(
                        Icon::new(script_icon(script.icon))
                            .size(px(16.))
                            .color(t.muted_foreground)
                            .opacity(0.8),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(logic::script_menu_label(&script)),
                    )
                    .child(
                        div()
                            .id(("details-script-edit", ix))
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(24.))
                            .rounded(crate::theme::radius::MD)
                            .opacity(0.)
                            .group_hover(group, |s| s.opacity(1.))
                            .hover(|s| s.bg(t.accent))
                            .child(
                                Icon::new(IconName::Settings)
                                    .size(px(14.))
                                    .color(t.muted_foreground),
                            )
                            .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                            })
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                PopupLayer::close(window, cx);
                                let script = edit_script.clone();
                                let ctx = edit_ctx.clone();
                                let _ = edit_this.update(cx, |this, cx| {
                                    this.open_script_editor(&ctx, Some(script), window, cx)
                                });
                            }),
                    )
                    .on_click(move |_, window, cx| {
                        PopupLayer::close(window, cx);
                        let _ = run_this
                            .update(cx, |this, cx| this.run_script(&run_ctx, &run_script, cx));
                    })
                    .into_any_element()
            }));
        }
        let ctx = ctx.clone();
        let add = MenuItem::new("Add project script")
            .icon(IconName::Plus)
            .on_select(move |window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.open_script_editor(&ctx, None, window, cx)
                });
            });
        menu = menu.item(add);
        open_menu(
            SCRIPTS_MENU.into(),
            anchor_below(bounds, Align::End),
            menu,
            window,
            cx,
        );
        cx.notify();
    }

    /// `runProjectScript` (§6.2): remember it, then hand it to the drawer.
    pub(super) fn run_script(&mut self, ctx: &Ctx, script: &ProjectScript, cx: &mut Context<Self>) {
        self.last_script
            .insert(ctx.project.id.clone(), script.id.clone());
        let cwd = ctx.cwd();
        if !cwd.is_dir() {
            Toasts::push(
                Toast::error(format!("Failed to run script \"{}\".", script.name))
                    .description(format!("{} does not exist.", cwd.display())),
                cx,
            );
            cx.notify();
            return;
        }
        let mut env = vec![(
            "T3CODE_PROJECT_ROOT".to_string(),
            ctx.project.workspace_root.clone(),
        )];
        if let Some(wt) = ctx.worktree_path() {
            env.push(("T3CODE_WORKTREE_PATH".into(), wt.to_string()));
        }
        let request = TerminalRequest {
            cwd: cwd.to_string_lossy().to_string(),
            command: script.command.clone(),
            env,
            new_terminal: script.stop_on_settle,
            name: script.name.clone(),
        };
        self.app.update(cx, |s, cx| s.run_in_terminal(request, cx));
        cx.notify();
    }

    /// Saves the project's scripts (`Command::UpdateProjectScripts`).
    #[allow(dead_code)]
    pub(super) fn save_scripts(&self, project_id: &str, scripts: Vec<ProjectScript>, cx: &App) {
        self.store
            .read(cx)
            .send(tern_core::Command::UpdateProjectScripts {
                project_id: project_id.to_string(),
                scripts,
            });
    }
}

pub(super) fn machine_label() -> String {
    super::machine_label()
}

/// Launches `editor` on `cwd` off the main thread; failures are toasts.
fn open_in_editor(editor: editors::Available, cwd: PathBuf, cx: &mut App) {
    editors::set_preferred(editor.id);
    let task = cx.background_spawn(async move { editors::launch(&editor, &cwd) });
    cx.spawn(async move |cx| {
        if let Err(e) = task.await {
            let _ = cx.update(|cx| {
                Toasts::push(
                    Toast::error("Editor opening is unavailable.").description(e),
                    cx,
                )
            });
        }
    })
    .detach();
}

#[allow(dead_code)]
fn shortcut(label: &str, cx: &App) -> AnyElement {
    shortcut_text(label.to_string(), cx).into_any_element()
}

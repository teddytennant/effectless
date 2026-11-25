//! The new-thread screen (spec 02 §3): the "What should we build in {project}?"
//! hero with the composer centered under it and the context strip (workspace
//! mode, branch, permissions) below the card. With no projects it shows the
//! "What should we work on?" hero instead.
//!
//! The shell overlays this view on the chat area for `Index` and `Draft` routes
//! ([`is_hero`]); the composer entity is shared with the docked thread layout.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_base::Align;
use tern_core::ThreadEnvMode;

use crate::app_state::{AppState, Route};
use crate::assets::IconName;
use crate::store::Store;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, layout, radius};
use crate::ui::menu::DropdownMenu;
use crate::ui::popup::PopupLayer;
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Icon, Menu, MenuItem, Tooltip, TooltipExt as _,
    bounds_probe, h_flex, v_flex,
};
use crate::views::composer::{ComposerView, most_recent_project, permissions};

/// Below this window height the hero docks to the bottom instead of centering.
const CENTER_MIN_HEIGHT: f32 = 544.;

/// Whether the chat column shows the draft hero instead of the docked composer.
pub fn is_hero(route: &Route) -> bool {
    matches!(route, Route::Index | Route::Draft(_))
}

#[derive(Clone, Debug, Default)]
struct BranchInfo {
    current: Option<String>,
    branches: Vec<String>,
}

pub struct DraftView {
    app: Entity<AppState>,
    store: Entity<Store>,
    composer: Entity<ComposerView>,
    branches: HashMap<String, BranchInfo>,
    permission_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl DraftView {
    pub fn new(
        composer: Entity<ComposerView>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let app = AppState::global(cx);
        let store = Store::global(cx);
        cx.observe(&app, |this, _, cx| {
            this.resolve_index(cx);
            cx.notify();
        })
        .detach();
        cx.observe(&store, |this, _, cx| {
            this.resolve_index(cx);
            cx.notify();
        })
        .detach();
        cx.observe(&composer, |_, _, cx| cx.notify()).detach();
        // Shared with the composer so Ctrl+Shift+A can anchor to the strip control.
        let permission_bounds = composer.read(cx).permission_bounds.clone();
        let mut this = Self {
            app,
            store,
            composer,
            branches: HashMap::new(),
            permission_bounds,
        };
        this.resolve_index(cx);
        this
    }

    /// `/` resolves to a draft in the most recent project (replacing the route).
    fn resolve_index(&mut self, cx: &mut Context<Self>) {
        let route = self.app.read(cx).chat_route().clone();
        if route == Route::Index
            && !self.app.read(cx).route().is_settings()
            && let Some(project) = most_recent_project(self.store.read(cx).snapshot())
        {
            self.app
                .update(cx, |app, cx| app.replace(Route::Draft(project), cx));
        }
        if let Some(project) = self.project_id(cx) {
            self.load_branches(project, cx);
        }
    }

    fn project_id(&self, cx: &App) -> Option<String> {
        match self.app.read(cx).chat_route() {
            Route::Draft(p) => Some(p.clone()),
            Route::Index => most_recent_project(self.store.read(cx).snapshot()),
            _ => None,
        }
    }

    fn load_branches(&mut self, project: String, cx: &mut Context<Self>) {
        if self.branches.contains_key(&project) {
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
        self.branches.insert(project.clone(), BranchInfo::default());
        let task = cx.background_spawn(async move {
            let current = tern_git::refs::current_branch(&root).ok().flatten();
            let branches = tern_git::refs::local_branch_names(&root).unwrap_or_default();
            BranchInfo { current, branches }
        });
        cx.spawn(async move |this, cx| {
            let info = task.await;
            this.update(cx, |this, cx| {
                this.branches.insert(project, info);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_no_projects(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let app = self.app.clone();
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(30.))
                    .line_height(px(36.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.foreground)
                    .child("What should we work on?"),
            )
            .child(
                div()
                    .text_scale(TextSize::Sm)
                    .text_color(t.muted_foreground)
                    .child("Add a project, or start without one."),
            )
            .child(
                h_flex()
                    .mt(px(20.))
                    .gap(px(8.))
                    .child(
                        Button::new("draft-add-project")
                            .size(ButtonSize::Sm)
                            .icon(IconName::Plus)
                            .label("Add project")
                            .on_click(move |_, _, cx| {
                                app.update(cx, |a, cx| a.set_palette_open(true, cx))
                            }),
                    )
                    .child(
                        Button::new("draft-start-scratch")
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Sm)
                            .icon(IconName::Square)
                            .label("Start without a project")
                            .disabled(true),
                    ),
            )
            .into_any_element()
    }

    fn render_headline(&self, project_title: &str, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let store = self.store.clone();
        let app = self.app.clone();
        let title: SharedString = project_title.to_string().into();
        let picker = DropdownMenu::new(
            "draft-project-picker",
            move |_, cx| {
                let snapshot = store.read(cx).snapshot().clone();
                let mut menu = Menu::new().min_width(px(288.));
                for p in snapshot.projects.iter().filter(|p| p.archived_at.is_none()) {
                    let id = p.id.clone();
                    let app = app.clone();
                    menu = menu.item(
                        MenuItem::new(p.title.clone())
                            .icon(IconName::Folder)
                            .on_select(move |_, cx| {
                                let id = id.clone();
                                app.update(cx, |a, cx| a.navigate(Route::Draft(id), cx))
                            }),
                    );
                }
                let app = app.clone();
                menu.separator().item(
                    MenuItem::new("Add project")
                        .icon(IconName::FolderPlus)
                        .on_select(move |_, cx| {
                            app.update(cx, |a, cx| a.set_palette_open(true, cx))
                        }),
                )
            },
            move |_| {
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .child(title)
                    .into_any_element()
            },
        )
        .align(Align::Center);
        v_flex()
            .items_center()
            .child(
                h_flex()
                    .justify_center()
                    .flex_wrap()
                    .text_size(px(30.))
                    .line_height(px(36.))
                    .text_color(t.foreground)
                    .child("What should we build in\u{a0}")
                    .child(picker)
                    .child("?"),
            )
            .child(
                h_flex()
                    .h(px(24.))
                    .mt(px(4.))
                    .text_scale(TextSize::Sm)
                    .text_color(t.muted_foreground)
                    .child(
                        div()
                            .id("draft-without-project")
                            .cursor_pointer()
                            .hover(|s| s.text_color(t.foreground))
                            .t3_tooltip(Tooltip::text("Start a thread without a project"))
                            .child("or start without a project"),
                    ),
            )
            .into_any_element()
    }

    /// `ComposerControl` size `xs`: h24, px7, gap 6, normal 12, fg/70.
    fn control_xs(
        id: impl Into<gpui::ElementId>,
        open: bool,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = cx.tern_theme();
        let t = theme.colors;
        let rest = if theme.is_dark() {
            t.muted_foreground
        } else {
            t.foreground.opacity(0.7)
        };
        h_flex()
            .id(id)
            .relative()
            .min_w_0()
            .h(px(24.))
            .px(px(7.))
            .gap(px(6.))
            .rounded(radius::CONTROL)
            .text_size(px(12.))
            .line_height(px(16.))
            .whitespace_nowrap()
            .cursor_pointer()
            .text_color(if open { t.foreground } else { rest })
            .when(open, |el| el.bg(t.accent))
            .hover(move |s| s.bg(t.accent).text_color(t.foreground))
    }

    fn render_strip(&self, project: &str, cx: &mut Context<Self>) -> AnyElement {
        let composer = self.composer.read(cx);
        let env = composer.env_mode();
        let mode = composer.runtime_mode(cx);
        let info = self.branches.get(project).cloned().unwrap_or_default();
        let branch = composer
            .branch()
            .or(info.current.clone())
            .unwrap_or_else(|| "main".to_string());

        let weak = self.composer.downgrade();
        let env_menu = DropdownMenu::new(
            "draft-env-mode",
            move |_, _| {
                let (a, b) = (weak.clone(), weak.clone());
                Menu::new()
                    .label("Workspace")
                    .item(
                        MenuItem::radio("Current checkout", env == ThreadEnvMode::Local)
                            .icon(IconName::Folder)
                            .on_select(move |_, cx| {
                                a.update(cx, |c, cx| c.set_env_mode(ThreadEnvMode::Local, cx))
                                    .ok();
                            }),
                    )
                    .item(
                        MenuItem::radio("New worktree", env == ThreadEnvMode::Worktree)
                            .icon(IconName::FolderGit2)
                            .on_select(move |_, cx| {
                                b.update(cx, |c, cx| c.set_env_mode(ThreadEnvMode::Worktree, cx))
                                    .ok();
                            }),
                    )
            },
            move |open| {
                let (icon, label) = match env {
                    ThreadEnvMode::Local => (IconName::Folder, "Current checkout"),
                    ThreadEnvMode::Worktree => (IconName::FolderGit2, "New worktree"),
                };
                EnvTriggerEl { open, icon, label }.into_any_element()
            },
        )
        .align(Align::Start)
        .placement(gpui_base::Placement::Top);

        let weak = self.composer.downgrade();
        let branches = if info.branches.is_empty() {
            vec![branch.clone()]
        } else {
            info.branches.clone()
        };
        let current_branch = branch.clone();
        let branch_menu = DropdownMenu::new(
            "draft-branch",
            move |_, _| {
                let mut menu = Menu::new().label("Branches");
                for b in &branches {
                    let weak = weak.clone();
                    let name = b.clone();
                    menu = menu.item(MenuItem::radio(b.clone(), *b == current_branch).on_select(
                        move |_, cx| {
                            let name = name.clone();
                            weak.update(cx, |c, cx| c.set_branch(name, cx)).ok();
                        },
                    ));
                }
                menu
            },
            {
                let branch = branch.clone();
                move |open| branch_trigger(open, branch)
            },
        )
        .align(Align::End)
        .placement(gpui_base::Placement::Top);

        let bounds = self.permission_bounds.clone();
        let composer = self.composer.clone();
        let open = PopupLayer::is_open(&"draft-permissions".into(), cx);
        let permissions = Self::control_xs("draft-permissions-trigger", open, cx)
            .child(bounds_probe(self.permission_bounds.clone()))
            .child(Icon::new(permissions::icon(mode)).size(px(12.)))
            .child(permissions::label(mode))
            .t3_tooltip(Tooltip::text("Permissions").kbd("Ctrl+Shift+A"))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(move |_, window, cx| {
                composer.update(cx, |c, cx| {
                    c.open_permissions("draft-permissions", bounds.get(), window, cx)
                });
            });

        h_flex()
            .w_full()
            .px(px(8.))
            .pt(px(8.))
            .gap(px(4.))
            .bg(cx.tern_theme().glass.surface)
            .child(env_menu)
            .child(div().flex_1())
            .child(branch_menu)
            .child(permissions)
            .into_any_element()
    }
}

#[derive(IntoElement)]
struct EnvTriggerEl {
    open: bool,
    icon: IconName,
    label: &'static str,
}

impl gpui::RenderOnce for EnvTriggerEl {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        DraftView::control_xs("draft-env-trigger", self.open, cx)
            .t3_tooltip(Tooltip::text(self.label).kbd("Ctrl+Shift+X"))
            .child(Icon::new(self.icon).size(px(12.)))
            .child(self.label)
    }
}

fn branch_trigger(open: bool, branch: String) -> AnyElement {
    BranchTriggerEl { open, branch }.into_any_element()
}

#[derive(IntoElement)]
struct BranchTriggerEl {
    open: bool,
    branch: String,
}

impl gpui::RenderOnce for BranchTriggerEl {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        DraftView::control_xs("draft-branch-trigger", self.open, cx)
            .t3_tooltip(Tooltip::text("Branch").kbd("Ctrl+Shift+G"))
            .child(Icon::new(IconName::GitBranch).size(px(12.)))
            .child(div().min_w_0().truncate().child(self.branch))
    }
}

impl Render for DraftView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        if store.snapshot().projects.is_empty() {
            return self.render_no_projects(cx);
        }
        let Some(project_id) = self.project_id(cx) else {
            return div().into_any_element();
        };
        let title = store
            .snapshot()
            .project(&project_id)
            .map(|p| p.title.clone())
            .unwrap_or_default();
        let headline = self.render_headline(&title, cx);
        let strip = self.render_strip(&project_id, cx);
        let short = window.viewport_size().height < px(CENTER_MIN_HEIGHT);
        div()
            .size_full()
            .flex()
            .when(short, |el| el.items_end())
            .when(!short, |el| el.items_center())
            .child(
                div()
                    .w_full()
                    .flex()
                    .justify_center()
                    .px(layout::CHAT_LANE_PADDING)
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(layout::CHAT_CONTENT_MAX_WIDTH)
                            .child(
                                div().relative().h(px(0.)).child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .bottom_0()
                                        .pb(px(16.))
                                        .child(headline),
                                ),
                            )
                            .child(self.composer.clone())
                            .child(strip)
                            .child(div().h(px(20.))),
                    ),
            )
            .into_any_element()
    }
}

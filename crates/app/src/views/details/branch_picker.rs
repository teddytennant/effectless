//! The branch picker popup (`BranchPicker`, spec/05 §4a.7): a search field, the ref
//! list with badges, "Checkout pull request" and "Create new ref" rows, and the
//! footer (Start from remote, notices, loading status). Opens below the branch row at
//! the row's width.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels, Render,
    ScrollHandle, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use tern_git::refs::Branch;

use super::git;
use super::logic;
use crate::theme::{ActiveTheme as _, TextExt as _, radius};
use crate::ui::menu::popup_surface;
use crate::ui::{Icon, IconName, Switch, Tooltip, TooltipExt as _, TooltipSide};

/// What the user picked.
#[derive(Clone, Debug)]
pub enum Pick {
    Ref(Branch),
    Create(String),
    CheckoutPr(String),
    StartFromOrigin(bool),
}

#[derive(Clone, Debug)]
enum Row {
    CheckoutPr(String),
    Ref(Branch),
    Create(String),
}

type OnPick = Rc<dyn Fn(Pick, &mut Window, &mut App)>;

pub struct BranchPicker {
    input: Entity<InputState>,
    refs: Option<Result<Vec<Branch>, String>>,
    highlighted: usize,
    project_root: String,
    current: Option<String>,
    choosing_base: bool,
    start_from_origin: Option<bool>,
    running: bool,
    width: Pixels,
    on_pick: OnPick,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl BranchPicker {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cwd: PathBuf,
        project_root: String,
        current: Option<String>,
        choosing_base: bool,
        start_from_origin: Option<bool>,
        running: bool,
        width: Pixels,
        on_pick: impl Fn(Pick, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search refs..."));
        let subscriptions =
            vec![
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::Change => {
                            this.highlighted = 0;
                            this.scroll.scroll_to_item(0);
                            cx.notify();
                        }
                        InputEvent::PressEnter { .. } => this.choose(this.highlighted, window, cx),
                        _ => {}
                    }
                }),
            ];
        let load = cx.background_spawn(async move { git::list_refs(&cwd) });
        cx.spawn(async move |this, cx| {
            let refs = load.await;
            let _ = this.update(cx, |this, cx| {
                this.refs = Some(refs);
                cx.notify();
            });
        })
        .detach();
        Self {
            input,
            refs: None,
            highlighted: 0,
            project_root,
            current,
            choosing_base,
            start_from_origin,
            running,
            width,
            on_pick: Rc::new(on_pick),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    fn query(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    fn rows(&self, cx: &App) -> Vec<Row> {
        let query = self.query(cx);
        let mut rows = Vec::new();
        if !self.choosing_base
            && let Some(reference) = logic::parse_pr_reference(&query)
        {
            rows.push(Row::CheckoutPr(reference));
        }
        let refs: &[Branch] = match &self.refs {
            Some(Ok(refs)) => refs,
            _ => &[],
        };
        rows.extend(
            refs.iter()
                .filter(|r| logic::ref_matches(&r.name, &query))
                .cloned()
                .map(Row::Ref),
        );
        let name = logic::sanitize_new_ref_name(&query);
        if !name.is_empty()
            && !self.choosing_base
            && matches!(self.refs, Some(Ok(_)))
            && !refs.iter().any(|r| r.name == name)
        {
            rows.push(Row::Create(name));
        }
        rows
    }

    fn choose(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows(cx).get(ix).cloned() else {
            return;
        };
        let pick = match row {
            Row::CheckoutPr(r) => Pick::CheckoutPr(r),
            Row::Ref(b) => Pick::Ref(b),
            Row::Create(n) => Pick::Create(n),
        };
        (self.on_pick)(pick, window, cx);
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.rows(cx).len();
        match event.keystroke.key.as_str() {
            "down" if n > 0 => {
                self.highlighted = (self.highlighted + 1) % n;
                self.scroll.scroll_to_item(self.highlighted);
            }
            "up" if n > 0 => {
                self.highlighted = (self.highlighted + n - 1) % n;
                self.scroll.scroll_to_item(self.highlighted);
            }
            "escape" => {
                crate::ui::popup::PopupLayer::close(window, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn render_row(&self, ix: usize, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let highlighted = ix == self.highlighted;
        let fg = t.foreground;
        let base = div()
            .id(("branch-row", ix))
            .flex()
            .items_center()
            .min_h(px(28.))
            .px(px(8.))
            .py(px(4.))
            .gap(px(8.))
            .rounded(radius::SM)
            .text_scale(crate::theme::TextSize::Sm)
            .cursor_pointer()
            .when(highlighted, |el| el.bg(fg.opacity(0.09 * fg.a)))
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.highlighted != ix {
                    this.highlighted = ix;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.choose(ix, window, cx)));
        match row {
            Row::CheckoutPr(reference) => base
                .py(px(4.))
                .child(
                    Icon::new(IconName::Github)
                        .size(px(14.))
                        .color(t.muted_foreground),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(div().font_medium().child("Checkout pull request"))
                        .child(
                            div()
                                .text_scale(crate::theme::TextSize::Xs)
                                .text_color(t.muted_foreground)
                                .child(reference.clone()),
                        ),
                )
                .into_any_element(),
            Row::Create(name) => base
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .child(format!("Create new ref \"{name}\"")),
                )
                .into_any_element(),
            Row::Ref(branch) => {
                let selected = self.current.as_deref() == Some(branch.name.as_str());
                let disabled = self.running
                    && branch.worktree_path.as_deref().is_some_and(|p| {
                        p.trim_end_matches('/') != self.project_root.trim_end_matches('/')
                    });
                let badge = logic::ref_badge(
                    branch.is_current,
                    branch.worktree_path.as_deref(),
                    &self.project_root,
                    branch.is_remote,
                    branch.is_default,
                );
                base.when(selected && !highlighted, |el| {
                    el.bg(fg.opacity(0.06 * fg.a))
                })
                .when(disabled, |el| el.opacity(crate::ui::DISABLED_OPACITY))
                .justify_between()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(logic::middle_truncate(&branch.name, 34)),
                )
                .when_some(badge, |el, badge| {
                    el.child(
                        div()
                            .flex_none()
                            .text_scale(crate::theme::TextSize::Xs3)
                            .text_color(t.muted_foreground.opacity(0.45))
                            .child(badge),
                    )
                })
                .into_any_element()
            }
        }
    }
}

impl Focusable for BranchPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}

impl Render for BranchPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        let rows = self.rows(cx);
        if self.highlighted >= rows.len() {
            self.highlighted = 0;
        }
        let focused = self.input.read(cx).focus_handle(cx).is_focused(_window);
        let border = if focused {
            t.ring
        } else {
            t.border.opacity(t.border.a * 0.7)
        };
        let search = div().px(px(12.)).pt(px(10.)).child(
            div()
                .relative()
                .mt(px(-1.))
                .pb(px(6.))
                .border_b_1()
                .border_color(border)
                .child(
                    div().absolute().left_0().top(px(5.)).child(
                        Icon::new(IconName::Search)
                            .size(px(16.))
                            .color(t.muted_foreground.opacity(0.55)),
                    ),
                )
                .child(
                    div()
                        .pl(px(20.))
                        .h(px(26.))
                        .text_scale(crate::theme::TextSize::Sm)
                        .child(Input::new(&self.input).appearance(false).h(px(26.))),
                ),
        );
        let status: Option<String> = match &self.refs {
            None => Some("Loading refs...".into()),
            Some(Err(e)) => Some(e.clone()),
            Some(Ok(_)) => None,
        };
        let row_elements: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(ix, row)| self.render_row(ix, row, cx))
            .collect();
        let list = if row_elements.is_empty() && status.is_none() {
            div()
                .p(px(8.))
                .text_center()
                .text_scale(crate::theme::TextSize::Sm)
                .text_color(t.muted_foreground)
                .child("No refs found.")
                .into_any_element()
        } else {
            div()
                .id("branch-picker-list")
                .max_h(px(224.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .pt(px(8.))
                .pb(px(8.))
                .pl(px(4.))
                .child(div().flex().flex_col().p(px(4.)).children(row_elements))
                .into_any_element()
        };
        let notice = self.running.then(|| {
            matches!(&self.refs, Some(Ok(refs)) if refs.iter().any(|r| r.worktree_path.is_some()))
        });
        let footer_line = |text: String| {
            div()
                .px(px(12.))
                .py(px(8.))
                .text_scale(crate::theme::TextSize::Xs)
                .font_medium()
                .text_color(t.muted_foreground)
                .child(text)
        };
        let start_from_remote = self.choosing_base.then(|| {
            let on = self.start_from_origin.unwrap_or(true);
            let pick = self.on_pick.clone();
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .px(px(12.))
                .py(px(8.))
                .border_t_1()
                .border_color(t.border.opacity(t.border.a * 0.6))
                .text_scale(crate::theme::TextSize::Xs)
                .child(
                    div()
                        .id("start-from-remote")
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .font_medium()
                        .text_color(t.muted_foreground)
                        .child(Icon::new(IconName::RefreshCw).size(px(12.)))
                        .child("Start from remote")
                        .t3_tooltip(
                            Tooltip::text("Uses the latest commit of the base branch’s tracked remote branch, even if its name differs. Otherwise uses the same-named branch on origin. Uses your local branch if that remote branch is missing.")
                                .side(TooltipSide::Top),
                        ),
                )
                .child(
                    Switch::new("start-from-remote-switch", on)
                        .size(crate::ui::SwitchSize::Sm)
                        .on_change(move |v, window, cx| pick(Pick::StartFromOrigin(v), window, cx)),
                )
        });
        popup_surface(cx)
            .id("branch-picker")
            .w(self.width)
            .max_h(px(368.))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(Self::on_key))
            .child(search)
            .child(list)
            .children(start_from_remote)
            .when(notice == Some(true), |el| {
                el.child(footer_line(
                    "Stop the current turn to switch worktrees.".into(),
                ))
            })
            .children(status.map(footer_line))
    }
}

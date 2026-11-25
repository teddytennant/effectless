//! Row visuals (spec 03 §3): user and assistant messages, the turn fold, the working
//! header, the thinking placeholder, compaction and system dividers, the proposed
//! plan card and event rows. Work rows live in `work_view`.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, ClipboardItem, Context, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledText, TextRun, Window, div,
    linear_color_stop, linear_gradient, prelude::FluentBuilder as _, px,
};
use tern_core::{ItemStatus, TurnItemBody};

use super::entries::{MessageEntry, PlanEntry, Role};
use super::rows::{Row, RowKind};
use super::{ChatView, format, icon};
use crate::markdown;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, font, leading, motion, radius};
use crate::ui::{
    Badge, BadgeVariant, Button, ButtonSize, ButtonVariant, DropdownMenu, IconName, Menu, MenuItem,
    Toast, Toasts, Tooltip, TooltipExt as _, TooltipSide, h_flex, v_flex,
};

const LINE_SM: f32 = 14. * leading::RELAXED;

/// The 72px shine band (`live-tool-shine` / `ActivityShimmerOverlay`): mask alpha at
/// 0, 10.8, 25.2, 36, 46.8, 61.2 and 72px.
fn band_alpha(x: f32) -> f32 {
    const STOPS: [(f32, f32); 7] = [
        (0., 0.),
        (10.8, 0.12),
        (25.2, 0.55),
        (36., 1.),
        (46.8, 0.55),
        (61.2, 0.12),
        (72., 0.),
    ];
    if !(0. ..=72.).contains(&x) {
        return 0.;
    }
    for w in STOPS.windows(2) {
        let ((x0, a0), (x1, a1)) = (w[0], w[1]);
        if x <= x1 {
            return a0 + (a1 - a0) * (x - x0) / (x1 - x0);
        }
    }
    0.
}

fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let a = a.to_rgb();
    let b = b.to_rgb();
    gpui::Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
    .into()
}

/// Text with a highlight sweeping across it every 2.2s in 30 steps. Glyph positions
/// are estimated from the average advance of the 14px sans face.
pub(crate) fn shine_text(
    id: impl Into<SharedString>,
    text: String,
    base: Hsla,
    highlight: Hsla,
    weight: FontWeight,
) -> AnyElement {
    let id: SharedString = id.into();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let width = chars.len() as f32 * 7.1;
    div()
        .min_w_0()
        .truncate()
        .with_animation(
            id,
            Animation::new(motion::LIVE_TOOL_SHINE_PERIOD)
                .repeat()
                .with_easing(motion::steps(30)),
            move |el, d| {
                let center = -72. + d * (width + 144.);
                let mut runs = Vec::with_capacity(chars.len());
                for (i, (_, c)) in chars.iter().enumerate() {
                    let x = i as f32 * 7.1 + 3.5 - (center - 36.);
                    runs.push(TextRun {
                        len: c.len_utf8(),
                        font: gpui::Font {
                            family: font::SANS.into(),
                            weight,
                            ..Default::default()
                        },
                        color: mix(base, highlight, band_alpha(x)),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                }
                el.child(StyledText::new(text.clone()).with_runs(runs))
            },
        )
        .into_any_element()
}

/// A divider row: 1px rules either side of a centered label (§3.11).
pub(crate) fn divider(
    content: AnyElement,
    gap: f32,
    rule: Hsla,
) -> AnyElement {
    h_flex()
        .w_full()
        .gap(px(gap))
        .child(div().h(px(1.)).flex_1().bg(rule))
        .child(content)
        .child(div().h(px(1.)).flex_1().bg(rule))
        .into_any_element()
}

impl ChatView {
    pub(crate) fn render_row(
        &mut self,
        row: &Row,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match &row.kind {
            RowKind::Message {
                message,
                show_meta,
                copy_streaming,
                grouped_with_next,
                grouped_with_previous,
                continues_steer,
            } => match message.role {
                Role::User => self.render_user(
                    row,
                    message,
                    *grouped_with_next,
                    *grouped_with_previous,
                    *continues_steer,
                    window,
                    cx,
                ),
                Role::Assistant => {
                    self.render_assistant(row, message, *show_meta, *copy_streaming, window, cx)
                }
            },
            RowKind::AssistantMeta { message } => div()
                .px(px(4.))
                .mt(px(2.))
                .child(self.assistant_meta(row, message, true, cx))
                .into_any_element(),
            RowKind::TurnFold {
                run_id,
                label,
                expanded,
            } => self.render_turn_fold(row, run_id, label, *expanded, cx),
            RowKind::Working { compacting } => self.render_working(row, *compacting, cx),
            RowKind::Thinking {
                label,
                group_id,
                expanded,
            } => self.render_thinking(row, label.clone(), group_id.clone(), *expanded, cx),
            RowKind::Compaction {
                label,
                tokens,
                active,
                failed,
                failure,
            } => render_compaction(row, label, *tokens, *active, *failed, failure.as_deref(), cx),
            RowKind::Plan(plan) => self.render_plan(row, plan, window, cx),
            RowKind::Event { item, subagents } => self.render_event(row, item, subagents, cx),
            RowKind::Work { .. } | RowKind::WorkLive { .. } | RowKind::WorkToggle { .. } => {
                self.render_work_row(row, ix, window, cx)
            }
        }
    }

    fn file_handler(&self, cx: &Context<Self>) -> markdown::FileHandler {
        let root = self.workspace_root(cx);
        std::rc::Rc::new(move |link: &markdown::FileLink, _cmd, _window, cx| {
            let path = if link.path.starts_with('/') || link.path.starts_with('~') {
                link.path.clone()
            } else {
                match &root {
                    Some(r) => format!("{}/{}", r.trim_end_matches('/'), link.path.trim_start_matches("./")),
                    None => link.path.clone(),
                }
            };
            cx.open_url(&format!("file://{path}"));
        })
    }

    fn copy_button(
        &self,
        id: String,
        row_id: &str,
        text: String,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let copied = self.copied.as_deref() == Some(row_id);
        let row_id = row_id.to_string();
        Button::new(SharedString::from(id))
            .variant(ButtonVariant::GhostMuted)
            .size(ButtonSize::IconXs)
            .icon(if copied { IconName::Check } else { IconName::Copy })
            .icon_size(px(12.))
            .disabled(copied)
            .tooltip(Tooltip::text(label).side(TooltipSide::Top))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                this.mark_copied(row_id.clone(), cx);
            }))
            .into_any_element()
    }

    fn timestamp(&self, id: String, at: tern_core::Ts, cx: &Context<Self>) -> AnyElement {
        let t = cx.tokens();
        div()
            .id(SharedString::from(id))
            .flex_none()
            .text_scale(TextSize::Xs)
            .text_color(t.muted_foreground)
            .t3_tooltip(Tooltip::text(format::timestamp_long(at)).side(TooltipSide::Top))
            .child(format::timestamp(at, chrono::Utc::now()))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_user(
        &mut self,
        row: &Row,
        message: &MessageEntry,
        grouped_next: bool,
        grouped_prev: bool,
        continues_steer: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *cx.tokens();
        let doc = self.md.get(&format!("user:{}", message.id), &message.text, true);
        let mut opts = markdown::Options::new(format!("u-{}", row.id));
        opts.variant = markdown::Variant::User;
        opts.text_color = Some(t.message_foreground);
        opts.on_file = Some(self.file_handler(cx));
        let body = markdown::render(&doc, &opts, window, cx);
        let group = SharedString::from(format!("row-{}", row.id));
        let steer = message.is_steer() && !continues_steer;
        let attachments = (!message.attachments.is_empty()).then(|| {
            h_flex()
                .flex_wrap()
                .gap(px(8.))
                .mb(px(8.))
                .children(message.attachments.iter().map(|a| {
                    h_flex()
                        .gap(px(6.))
                        .text_scale(TextSize::Xs)
                        .child(icon(
                            match a {
                                tern_core::ChatAttachment::Image { .. } => IconName::Image,
                                _ => IconName::File,
                            },
                            14.,
                            t.muted_foreground,
                        ))
                        .child(div().max_w(px(220.)).truncate().child(a.name().to_string()))
                }))
        });
        let r_top = if grouped_prev { px(6.) } else { radius::XL2 };
        let r_bottom = if grouped_next { px(6.) } else { radius::XL2 };
        let can_edit = self.can_edit_from(message, cx);
        let edit_msg = message.clone();
        let actions = h_flex()
            .justify_end()
            .pr(px(4.))
            .gap(px(8.))
            .text_scale(TextSize::Xs)
            .opacity(0.)
            .group_hover(group.clone(), |s| s.opacity(1.))
            .child(self.timestamp(format!("{}-ts", row.id), message.created_at, cx))
            .child(
                h_flex()
                    .gap(px(2.))
                    .when(can_edit, |el| {
                        el.child(
                            Button::new(SharedString::from(format!("{}-edit", row.id)))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::IconXs)
                                .icon(IconName::Pencil)
                                .icon_size(px(12.))
                                .disabled(self.is_working(cx))
                                .tooltip(Tooltip::text("Edit from here").side(TooltipSide::Top))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.edit_from(&edit_msg, cx)
                                })),
                        )
                    })
                    .child(self.copy_button(
                        format!("{}-copy", row.id),
                        &row.id,
                        message.text.clone(),
                        "Copy message",
                        cx,
                    )),
            );
        v_flex()
            .id(SharedString::from(format!("row-{}", row.id)))
            .group(group)
            .w_full()
            .items_end()
            .gap(px(4.))
            .when(steer, |el| {
                el.child(
                    h_flex()
                        .gap(px(4.))
                        .mr(px(4.))
                        .text_scale(TextSize::Xs)
                        .line_height(px(12.))
                        .text_color(t.muted_foreground)
                        .child(icon(IconName::Redo2, 12., t.muted_foreground))
                        .child("Steer"),
                )
            })
            .child(
                div()
                    .max_w(gpui::relative(0.8))
                    .min_w_0()
                    .rounded_tl(radius::XL2)
                    .rounded_bl(radius::XL2)
                    .rounded_tr(r_top)
                    .rounded_br(r_bottom)
                    .bg(t.message_surface)
                    .text_color(t.message_foreground)
                    .p(px(12.))
                    .children(attachments)
                    .child(body),
            )
            .child(actions)
            .into_any_element()
    }

    /// "Edit from here" needs rollback support and a checkpoint from before the turn.
    fn can_edit_from(&self, message: &MessageEntry, cx: &Context<Self>) -> bool {
        let _ = cx;
        let Some(detail) = &self.detail else {
            return false;
        };
        let supports = detail
            .session
            .as_ref()
            .is_some_and(|s| s.capabilities.can_rollback_conversation);
        supports && self.checkpoint_before(message).is_some()
    }

    fn checkpoint_before(&self, message: &MessageEntry) -> Option<String> {
        let detail = self.detail.as_ref()?;
        let run = detail
            .runs
            .iter()
            .find(|r| Some(&r.id) == message.run_id.as_ref())?;
        detail
            .checkpoints
            .iter()
            .find(|c| c.ordinal + 1 == run.ordinal)
            .map(|c| c.id.clone())
    }

    fn edit_from(&mut self, message: &MessageEntry, cx: &mut Context<Self>) {
        if self.is_working(cx) {
            Toasts::push(
                Toast::error("Interrupt the current turn before reverting checkpoints."),
                cx,
            );
            return;
        }
        let (Some(detail), Some(checkpoint)) = (self.detail.clone(), self.checkpoint_before(message))
        else {
            return;
        };
        self.store.read(cx).send(tern_core::Command::RollbackToCheckpoint {
            thread_id: detail.thread.id.clone(),
            checkpoint_id: checkpoint,
            restore_files: true,
        });
    }

    fn render_assistant(
        &mut self,
        row: &Row,
        message: &MessageEntry,
        show_meta: bool,
        copy_streaming: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *cx.tokens();
        let group = SharedString::from(format!("row-{}", row.id));
        let body: AnyElement = if message.text.trim().is_empty() {
            if message.streaming {
                div().into_any_element()
            } else {
                div()
                    .text_scale(TextSize::Sm)
                    .text_color(t.muted_foreground)
                    .child("(empty response)")
                    .into_any_element()
            }
        } else {
            let doc = self.md.get(&format!("assistant:{}", message.id), &message.text, false);
            let mut opts = markdown::Options::new(format!("a-{}", row.id));
            opts.streaming = message.streaming;
            opts.on_file = Some(self.file_handler(cx));
            markdown::render(&doc, &opts, window, cx)
        };
        let interrupted = message.item.status == ItemStatus::Interrupted;
        v_flex()
            .id(SharedString::from(format!("row-{}", row.id)))
            .group(group)
            .w_full()
            .px(px(4.))
            .py(px(2.))
            .child(body)
            .when(interrupted && !show_meta, |el| {
                el.child(div().mt(px(6.)).child(status_badge("interrupted", cx)))
            })
            .when(show_meta, |el| {
                el.child(
                    div()
                        .mt(px(6.))
                        .opacity(0.)
                        .group_hover(SharedString::from(format!("row-{}", row.id)), |s| s.opacity(1.))
                        .child(self.assistant_meta(row, message, !copy_streaming, cx)),
                )
            })
            .into_any_element()
    }

    /// Copy button and timestamp under a finished reply (§3.2).
    fn assistant_meta(
        &mut self,
        row: &Row,
        message: &MessageEntry,
        copy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let status = match message.item.status {
            ItemStatus::Completed => None,
            ItemStatus::Interrupted => Some("interrupted"),
            ItemStatus::Failed => Some("failed"),
            ItemStatus::Cancelled => Some("cancelled"),
            _ => None,
        };
        let copy = copy && !message.text.trim().is_empty() && !message.streaming;
        h_flex()
            .gap(px(8.))
            .text_scale(TextSize::Xs)
            .children(status.map(|s| status_badge(s, cx)))
            .when(copy, |el| {
                el.child(self.copy_button(
                    format!("{}-copy", row.id),
                    &row.id,
                    message.text.clone(),
                    "Copy message",
                    cx,
                ))
            })
            .child(self.timestamp(format!("{}-ts", row.id), message.updated_at, cx))
            .into_any_element()
    }

    fn render_turn_fold(
        &mut self,
        row: &Row,
        run_id: &str,
        label: &str,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *cx.tokens();
        let run = run_id.to_string();
        let group = SharedString::from(format!("row-{}", row.id));
        h_flex()
            .id(SharedString::from(format!("row-{}", row.id)))
            .group(group.clone())
            .w_full()
            .pt(px(4.))
            .pb(px(8.))
            .pr(px(2.))
            .border_b_1()
            .border_color(t.border.opacity(0.6))
            .justify_between()
            .child(
                h_flex()
                    .id(SharedString::from(format!("{}-btn", row.id)))
                    .px(px(4.))
                    .gap(px(4.))
                    .rounded(radius::MD)
                    .h(px(LINE_SM))
                    .text_size(px(14.))
                    .line_height(px(LINE_SM))
                    .text_color(t.muted_foreground)
                    .hover(|s| s.text_color(t.foreground))
                    .cursor_pointer()
                    .child(label.to_string())
                    .child(icon(
                        if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        },
                        14.,
                        t.muted_foreground,
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let run = run.clone();
                        this.update_disclosures(cx, |d| {
                            if !d.runs.remove(&run) {
                                d.runs.insert(run);
                            }
                        });
                    })),
            )
            .children(row.created_at.map(|at| {
                div()
                    .opacity(0.)
                    .group_hover(group.clone(), |s| s.opacity(1.))
                    .child(self.timestamp(format!("{}-ts", row.id), at, cx))
            }))
            .into_any_element()
    }

    fn render_working(&mut self, row: &Row, compacting: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let label = match row.created_at {
            Some(start) => {
                let secs = (chrono::Utc::now() - start).num_seconds();
                if compacting {
                    format!("Compacting for {}", format::working_elapsed(secs))
                } else {
                    format!("Working for {}", format::working_elapsed(secs))
                }
            }
            None if compacting => "Compacting…".into(),
            None => "Working...".into(),
        };
        div()
            .w_full()
            .pt(px(4.))
            .pb(px(8.))
            .border_b_1()
            .border_color(t.border.opacity(0.6))
            .child(
                h_flex()
                    .h(px(24.))
                    .px(px(4.))
                    .gap(px(4.))
                    .text_size(px(14.))
                    .line_height(px(LINE_SM))
                    .text_color(t.muted_foreground)
                    .when(compacting, |el| el.child(icon(IconName::Minimize2, 12., t.muted_foreground)))
                    .child(label),
            )
            .into_any_element()
    }

    fn render_thinking(
        &mut self,
        row: &Row,
        label: Option<String>,
        group_id: Option<String>,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *cx.tokens();
        let pending = self
            .detail
            .as_ref()
            .is_some_and(|d| d.requests.iter().any(|r| r.status == tern_core::RequestStatus::Pending));
        let label = label.unwrap_or_else(|| "Thinking".into());
        let text: AnyElement = if pending {
            div().truncate().text_color(t.secondary_label).child(label).into_any_element()
        } else {
            shine_text(
                format!("{}-shine", row.id),
                label,
                t.secondary_label,
                t.foreground,
                FontWeight::NORMAL,
            )
        };
        let line = super::work_view::work_line(
            IconName::Brain,
            super::work_view::icon_color(cx),
            text,
            None,
            group_id.is_some().then_some(expanded),
            cx,
        );
        match group_id {
            Some(g) => div()
                .id(SharedString::from(format!("row-{}", row.id)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    let g = g.clone();
                    this.update_disclosures(cx, |d| {
                        if !d.groups.remove(&g) {
                            d.groups.insert(g);
                        }
                    })
                }))
                .child(line)
                .into_any_element(),
            None => line,
        }
    }

    fn render_plan(
        &mut self,
        row: &Row,
        plan: &PlanEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.tern_theme().clone();
        let t = theme.colors;
        let (title, body) = plan_title_and_body(&plan.markdown);
        let long = plan.markdown.chars().count() > 900 || plan.markdown.lines().count() > 20;
        let state = window.use_keyed_state(
            SharedString::from(format!("{}-plan", row.id)),
            cx,
            |_, _| false,
        );
        let expanded = *state.read(cx);
        let shown = if long && !expanded {
            let mut lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).take(10).collect();
            lines.push("...");
            lines.join("\n")
        } else {
            body.clone()
        };
        let doc = self.md.get(&format!("plan:{}:{}", plan.id, expanded), &shown, false);
        let mut opts = markdown::Options::new(format!("p-{}", row.id));
        opts.streaming = plan.streaming;
        opts.on_file = Some(self.file_handler(cx));
        let body_el = markdown::render(&doc, &opts, window, cx);
        let md = plan.markdown.clone();
        let slug = plan_slug(&title);
        let menu = move |_: &mut Window, _: &mut gpui::App| {
            let copy = md.clone();
            let save = md.clone();
            let file = format!("{slug}.md");
            Menu::new()
                .item(MenuItem::new("Copy to clipboard").on_select(move |_, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(format!("{copy}\n")));
                }))
                .item(MenuItem::new("Download as markdown").on_select(move |_, cx| {
                    let path = dirs_download().join(&file);
                    match std::fs::write(&path, format!("{save}\n")) {
                        Ok(()) => Toasts::push(Toast::success(format!("Saved {}", path.display())), cx),
                        Err(_) => Toasts::push(Toast::error("Could not save plan"), cx),
                    };
                }))
                .item(MenuItem::new("Save to workspace").disabled(true))
        };
        let state_toggle = state.clone();
        div()
            .w_full()
            .px(px(4.))
            .py(px(2.))
            .child(
                v_flex()
                    .w_full()
                    .rounded(radius::XL2)
                    .border_1()
                    .border_color(t.border.opacity(0.8))
                    .bg(t.card.opacity(0.7))
                    .p(px(20.))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap(px(8.))
                                    .child(Badge::new("Plan").variant(BadgeVariant::Secondary))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_scale(TextSize::Sm)
                                            .font_medium()
                                            .text_color(t.foreground)
                                            .child(title.clone()),
                                    ),
                            )
                            .child(DropdownMenu::new(
                                SharedString::from(format!("{}-plan-menu", row.id)),
                                menu,
                                {
                                    let id = SharedString::from(format!("{}-plan-menu-btn", row.id));
                                    move |open| {
                                        Button::new(id)
                                            .variant(ButtonVariant::Outline)
                                            .size(ButtonSize::IconXs)
                                            .icon(IconName::Ellipsis)
                                            .pressed(open)
                                            .tooltip(Tooltip::text("Plan actions").side(TooltipSide::Top))
                                            .into_any_element()
                                    }
                                },
                            )
                            .align(gpui_base::Align::End)),
                    )
                    .child(
                        div()
                            .relative()
                            .mt(px(16.))
                            .when(long && !expanded, |el| el.max_h(px(416.)).overflow_hidden())
                            .child(body_el)
                            .when(long && !expanded, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .bottom_0()
                                        .h(px(96.))
                                        .bg(linear_gradient(
                                            0.,
                                            linear_color_stop(t.card.opacity(0.95), 0.),
                                            linear_color_stop(t.card.opacity(0.), 1.),
                                        )),
                                )
                            }),
                    )
                    .when(long, |el| {
                        el.child(
                            h_flex().w_full().justify_center().mt(px(16.)).child(
                                Button::new(SharedString::from(format!("{}-plan-expand", row.id)))
                                    .variant(ButtonVariant::Outline)
                                    .size(ButtonSize::Sm)
                                    .label(if expanded { "Collapse plan" } else { "Expand plan" })
                                    .on_click(move |_, _, cx| {
                                        state_toggle.update(cx, |s, cx| {
                                            *s = !*s;
                                            cx.notify();
                                        })
                                    }),
                            ),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_event(
        &mut self,
        row: &Row,
        item: &tern_core::TurnItem,
        subagents: &[std::rc::Rc<tern_core::TurnItem>],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *cx.tokens();
        match &item.body {
            TurnItemBody::RunInterruptResult { message } => system_divider(
                Some(IconName::CircleStop),
                "Run interrupted",
                Some(message.clone()).filter(|m| !m.trim().is_empty()),
                false,
                cx,
            ),
            TurnItemBody::Subagent { .. } => {
                let list: Vec<&tern_core::TurnItem> = if subagents.is_empty() {
                    vec![item]
                } else {
                    subagents.iter().map(|s| s.as_ref()).collect()
                };
                v_flex()
                    .w_full()
                    .children(list.into_iter().enumerate().map(|(i, s)| {
                        let TurnItemBody::Subagent {
                            prompt, progress, ..
                        } = &s.body
                        else {
                            return div().into_any_element();
                        };
                        let failed = s.status == ItemStatus::Failed;
                        let status = match s.status {
                            ItemStatus::Pending => "Queued",
                            ItemStatus::Running => "Running",
                            ItemStatus::Waiting => "Waiting",
                            ItemStatus::Idle => "Idle · resumable",
                            ItemStatus::Completed => "Completed",
                            ItemStatus::Failed => "Failed",
                            ItemStatus::Cancelled | ItemStatus::Interrupted => "Stopped",
                        };
                        let elapsed = s.started_at.map(|start| {
                            let end = s.completed_at.unwrap_or_else(chrono::Utc::now);
                            format::duration((end - start).num_milliseconds())
                        });
                        h_flex()
                            .id(SharedString::from(format!("{}-sub{i}", row.id)))
                            .w_full()
                            .px(px(8.))
                            .py(px(6.))
                            .gap(px(10.))
                            .rounded(radius::MD)
                            .hover(|s| s.bg(t.accent.opacity(0.2)))
                            .child(
                                div()
                                    .size(px(24.))
                                    .flex_none()
                                    .rounded_full()
                                    .bg(t.muted)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(IconName::Bot, 14., t.muted_foreground)),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_scale(TextSize::Xs)
                                            .font_medium()
                                            .text_color(t.foreground)
                                            .child(s.title.clone().unwrap_or_else(|| prompt.clone())),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_scale(TextSize::Xs2)
                                            .text_color(if failed { t.destructive } else { t.muted_foreground })
                                            .child(progress.clone().unwrap_or_else(|| status.to_string())),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_scale(TextSize::Xs2)
                                    .text_color(t.muted_foreground)
                                    .child(match elapsed {
                                        Some(e) => format!("{status} · {e}"),
                                        None => status.to_string(),
                                    }),
                            )
                            .child(icon(IconName::ChevronRight, 14., t.muted_foreground))
                            .into_any_element()
                    }))
                    .into_any_element()
            }
            _ => div().into_any_element(),
        }
    }
}

pub(crate) fn status_badge(text: &str, cx: &gpui::App) -> AnyElement {
    let t = cx.tokens();
    div()
        .flex_none()
        .rounded_full()
        .border_1()
        .border_color(t.border.opacity(0.7))
        .px(px(6.))
        .py(px(2.))
        .font_family(font::MONO)
        .text_scale(TextSize::Xs3)
        .text_color(t.muted_foreground)
        .child(text.to_string())
        .into_any_element()
}

/// `TimelineSystemDivider` (§3.11).
pub(crate) fn system_divider(
    icon_name: Option<IconName>,
    label: &str,
    detail: Option<String>,
    danger: bool,
    cx: &gpui::App,
) -> AnyElement {
    let t = cx.tokens();
    let color = if danger { t.destructive } else { t.muted_foreground };
    let pill = h_flex()
        .px(px(8.))
        .py(px(4.))
        .gap(px(4.))
        .max_w(px(480.))
        .min_w_0()
        .children(icon_name.map(|i| icon(i, 12., color)))
        .child(div().flex_none().font_medium().child(label.to_string()))
        .children(detail.map(|d| {
            div()
                .min_w_0()
                .max_w(px(320.))
                .truncate()
                .opacity(0.7)
                .child(format!("· {d}"))
        }));
    div()
        .w_full()
        .py(px(8.))
        .text_scale(TextSize::Xs2)
        .text_color(color)
        .child(divider(pill.into_any_element(), 8., t.border.opacity(0.7)))
        .into_any_element()
}

fn render_compaction(
    row: &Row,
    label: &str,
    tokens: Option<(u32, u32)>,
    active: bool,
    failed: bool,
    failure: Option<&str>,
    cx: &mut Context<ChatView>,
) -> AnyElement {
    let t = *cx.tokens();
    let color = if failed { t.destructive } else { t.muted_foreground };
    let rule = if failed {
        t.destructive.opacity(0.3)
    } else {
        t.border.opacity(0.7)
    };
    let text: AnyElement = match tokens {
        Some((before, after)) if !active && !failed => h_flex()
            .gap(px(4.))
            .child(format!("{label} {}", format::tokens(before)))
            .child(icon(IconName::ArrowRight, 12., color))
            .child(format!("{} tokens", format::tokens(after)))
            .into_any_element(),
        _ if active => shine_text(
            format!("{}-shine", row.id),
            label.to_string(),
            t.muted_foreground,
            t.foreground,
            FontWeight::NORMAL,
        ),
        _ => div().child(label.to_string()).into_any_element(),
    };
    let content = h_flex()
        .gap(px(6.))
        .child(icon(
            if failed {
                IconName::CircleAlert
            } else {
                IconName::Minimize2
            },
            12.,
            color,
        ))
        .child(text)
        .children(failure.map(|f| {
            div()
                .max_w(px(240.))
                .truncate()
                .opacity(0.8)
                .child(format!(" · {f}"))
        }));
    div()
        .w_full()
        .py(px(4.))
        .text_scale(TextSize::Xs)
        .text_color(color)
        .child(divider(content.into_any_element(), 12., rule))
        .into_any_element()
}

/// Plan title = the first heading, else "Proposed plan"; the body drops that heading
/// and a following "Summary" heading (`proposedPlanText.ts`).
pub fn plan_title_and_body(markdown: &str) -> (String, String) {
    let mut lines: Vec<&str> = markdown.lines().collect();
    let heading_ix = lines
        .iter()
        .position(|l| l.trim_start().starts_with('#'));
    let Some(ix) = heading_ix else {
        return ("Proposed plan".into(), markdown.to_string());
    };
    let title = lines[ix].trim_start().trim_start_matches('#').trim().to_string();
    lines.remove(ix);
    if let Some(next) = lines.iter().skip(ix).position(|l| !l.trim().is_empty())
        && lines[ix + next].trim_start().starts_with('#')
        && lines[ix + next]
            .trim_start()
            .trim_start_matches('#')
            .trim()
            .eq_ignore_ascii_case("summary")
    {
        lines.remove(ix + next);
    }
    let title = if title.is_empty() {
        "Proposed plan".into()
    } else {
        title
    };
    (title, lines.join("\n").trim().to_string())
}

/// `{slug-of-title}.md`: lowercase, punctuation dropped, non-alnum → "-", default "plan".
pub fn plan_slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if c.is_whitespace() || c == '-' || c == '_' {
            if !out.ends_with('-') && !out.is_empty() {
                out.push('-');
            }
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "plan".into() } else { out }
}

fn dirs_download() -> std::path::PathBuf {
    std::env::var_os("XDG_DOWNLOAD_DIR")
        .map(Into::into)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join("Downloads")))
        .unwrap_or_else(|| ".".into())
}

#[allow(dead_code)]
const FADE: Duration = Duration::from_millis(200);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_titles() {
        let (title, body) = plan_title_and_body("# Settings page\n\n## Summary\nDo it.\n\n- a");
        assert_eq!(title, "Settings page");
        assert_eq!(body, "Do it.\n\n- a");
        assert_eq!(plan_title_and_body("Just text").0, "Proposed plan");
        assert_eq!(plan_slug("Plan: the Settings page!"), "plan-the-settings-page");
        assert_eq!(plan_slug("!!!"), "plan");
    }

    #[test]
    fn band_peaks_in_the_middle() {
        assert_eq!(band_alpha(36.), 1.);
        assert_eq!(band_alpha(-1.), 0.);
        assert!((band_alpha(18.) - 0.335).abs() < 0.01);
    }
}

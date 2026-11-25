//! Work log rows (spec 03 §3.3-§3.9): the shared 24px line, single entries with their
//! expanded tool panels, the collapsed group summary, the live activity line, the
//! pending approval row and provider errors.

use gpui::{
    AnyElement, App, Context, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, StyledText,
    Window, div, prelude::FluentBuilder as _, px, relative,
};
use tern_core::{
    ApprovalDecision, ApprovalOption, Command, FailureClass, ItemStatus, PlanStepStatus,
    RequestKind, TurnItemBody,
};

use super::entries::{ItemType, Lifecycle, SourceKind, Tone, WorkEntry, collapse_ws};
use super::rows::{Row, RowKind};
use super::row_view::shine_text;
use super::{ChatView, format, icon};
use crate::markdown;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, font, leading, radius};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, DropdownMenu, IconName, InlineButton, InlineTone, Menu,
    MenuItem, h_flex, v_flex,
};

const LINE: f32 = 14. * leading::RELAXED;

/// `icon-muted` at 70%; light mode also darkens it (`brightness(0.6)`).
pub(crate) fn icon_color(cx: &App) -> Hsla {
    let t = cx.tokens();
    if cx.tern_theme().is_dark() {
        t.icon_muted.opacity(0.7)
    } else {
        let mut c = t.icon_muted;
        c.l *= 0.6;
        c.opacity(0.7)
    }
}

/// The shared work log line (`WorkLog.tsx:39-148`): 24px min height, a 24px icon
/// column, the label, then trailing content and an optional chevron (open state).
pub(crate) fn work_line(
    icon_name: IconName,
    icon_tint: Hsla,
    label: AnyElement,
    trailing: Option<AnyElement>,
    chevron: Option<bool>,
    cx: &App,
) -> AnyElement {
    h_flex()
        .w_full()
        .min_h(px(24.))
        .px(px(2.))
        .gap(px(6.))
        .rounded(radius::MD)
        .text_size(px(14.))
        .line_height(px(LINE))
        .child(
            div()
                .w(px(24.))
                .flex_none()
                .flex()
                .justify_center()
                .child(icon(icon_name, 16., icon_tint)),
        )
        .child(div().flex_1().min_w_0().overflow_hidden().child(label))
        .children(trailing)
        .children(chevron.map(|open| {
            div()
                .size(px(16.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(
                    if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    },
                    12.,
                    icon_color(cx),
                ))
        }))
        .into_any_element()
}

fn mono_block(text: String, color: Hsla) -> gpui::Div {
    div()
        .w_full()
        .font_family(font::MONO)
        .text_size(px(font::CODE_SIZE))
        .line_height(relative(leading::RELAXED))
        .text_color(color)
        .child(text)
}

impl ChatView {
    pub(crate) fn render_work_row(
        &mut self,
        row: &Row,
        _ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match &row.kind {
            RowKind::Work {
                entries,
                expanded_group: true,
                ..
            } => {
                let lines: Vec<AnyElement> = entries
                    .iter()
                    .map(|e| self.render_entry(row, e, None, true, window, cx))
                    .collect();
                let max = px(288.).min(window.viewport_size().height * 0.5);
                let any_open = {
                    let d = self.disclosures();
                    entries.iter().any(|e| d.entries.contains(&e.id))
                };
                v_flex()
                    .id(SharedString::from(format!("{}-list", row.id)))
                    .w_full()
                    .when(!any_open, |el| el.max_h(max).overflow_y_scroll())
                    .children(lines)
                    .into_any_element()
            }
            RowKind::Work {
                entries,
                display_label,
                ..
            } => match entries.first() {
                Some(e) => self.render_entry(row, e, display_label.clone(), false, window, cx),
                None => div().into_any_element(),
            },
            RowKind::WorkToggle {
                group_id,
                expanded,
                summary,
                kind,
                has_failure,
                ..
            } => {
                let t = *cx.tokens();
                let tint = if *has_failure {
                    t.tool_error_icon.opacity(0.4)
                } else {
                    icon_color(cx)
                };
                let group = SharedString::from(format!("row-{}", row.id));
                let ts = row.created_at.map(|at| self.hover_timestamp(&row.id, at, &group, cx));
                let line = work_line(
                    kind.icon(),
                    tint,
                    div()
                        .truncate()
                        .text_color(t.secondary_label)
                        .child(summary.clone())
                        .into_any_element(),
                    ts,
                    None,
                    cx,
                );
                let g = group_id.clone();
                let _ = expanded;
                div()
                    .id(SharedString::from(format!("row-{}", row.id)))
                    .group(group)
                    .w_full()
                    .py(px(2.))
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
                    .into_any_element()
            }
            RowKind::WorkLive {
                entry,
                group_id,
                expanded,
                active,
                thought,
                ..
            } => {
                let t = *cx.tokens();
                let root = self.workspace_root(cx);
                let label = entry.live_label(root.as_deref(), *active);
                let label_el = if *active {
                    shine_text(
                        format!("{}-shine", row.id),
                        label,
                        t.secondary_label,
                        t.foreground,
                        FontWeight::NORMAL,
                    )
                } else {
                    div()
                        .truncate()
                        .text_color(t.secondary_label)
                        .child(label)
                        .into_any_element()
                };
                let thought_line = thought
                    .as_ref()
                    .filter(|_| !*expanded)
                    .and_then(|th| th.detail.as_deref())
                    .map(format::live_thought_line)
                    .filter(|l| !l.is_empty())
                    .map(|line| {
                        h_flex()
                            .items_start()
                            .w_full()
                            .px(px(2.))
                            .gap(px(6.))
                            .text_size(px(14.))
                            .line_height(px(LINE))
                            .child(
                                div()
                                    .w(px(24.))
                                    .h(px(LINE))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(IconName::Brain, 16., icon_color(cx))),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .max_h(px(LINE * 4.))
                                    .overflow_hidden()
                                    .text_color(t.secondary_label)
                                    .child(line),
                            )
                    });
                let g = group_id.clone();
                v_flex()
                    .id(SharedString::from(format!("row-{}", row.id)))
                    .w_full()
                    .py(px(2.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let g = g.clone();
                        this.update_disclosures(cx, |d| {
                            if !d.groups.remove(&g) {
                                d.groups.insert(g);
                            }
                        })
                    }))
                    .children(thought_line)
                    .child(
                        div().child(work_line(
                            entry.icon(),
                            icon_color(cx),
                            label_el,
                            None,
                            None,
                            cx,
                        )),
                    )
                    .into_any_element()
            }
            _ => div().into_any_element(),
        }
    }

    fn hover_timestamp(
        &self,
        row_id: &str,
        at: tern_core::Ts,
        group: &SharedString,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = cx.tokens();
        div()
            .flex_none()
            .text_scale(TextSize::Xs)
            .text_color(t.muted_foreground)
            .opacity(0.)
            .group_hover(group.clone(), |s| s.opacity(1.))
            .child(format::timestamp(at, chrono::Utc::now()))
            .id(SharedString::from(format!("{row_id}-hts")))
            .into_any_element()
    }

    /// One work entry: dispatches the special rows (approval, error, answered question)
    /// and otherwise draws the expandable line plus its details.
    fn render_entry(
        &mut self,
        row: &Row,
        e: &WorkEntry,
        display_label: Option<String>,
        in_group: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let TurnItemBody::ApprovalRequest { .. } = &e.item.body
            && self.approval_pending(e)
        {
            return self.render_approval(row, e, cx);
        }
        if e.item_type == ItemType::Error
            && (e.status == Lifecycle::Failed || e.source_kind == Some(SourceKind::RuntimeWarning))
        {
            return self.render_error(row, e, cx);
        }
        let t = *cx.tokens();
        let root = self.workspace_root(cx);
        let label = display_label.unwrap_or_else(|| e.display_label(root.as_deref()));
        let failed = e.display_failure();
        let warning = e.item_type == ItemType::SystemNotice
            && e.source_kind == Some(SourceKind::RuntimeWarning);
        let tint = if warning {
            t.warning
        } else if failed {
            t.tool_error_icon.opacity(0.4)
        } else {
            icon_color(cx)
        };
        let label_color = if warning {
            t.warning
        } else if e.is_tool_like() {
            t.secondary_label
        } else {
            t.foreground.opacity(0.8)
        };
        let expanded = self.disclosures().entries.contains(&e.id);
        let has_details = has_details(e);
        let group = SharedString::from(format!("entry-{}-{}", row.id, e.id));
        let label_el: AnyElement = if e.question_answer {
            let (q, a) = question_answer_preview(e);
            h_flex()
                .min_w_0()
                .gap(px(6.))
                .child(div().flex_none().max_w(relative(0.6)).truncate().text_color(label_color).child(q))
                .child(div().min_w_0().truncate().text_color(t.foreground).child(a))
                .into_any_element()
        } else if e.item_type == ItemType::Reasoning {
            let line = markdown::parse::parse(&first_line(&label), &markdown::ParseOptions::default());
            let inline = match line.blocks.first() {
                Some(markdown::Block::Paragraph(i)) => i.clone(),
                _ => markdown::Inline {
                    text: label.clone(),
                    spans: vec![],
                },
            };
            let mut opts = markdown::Options::new(format!("th-{}", e.id));
            opts.text_color = Some(label_color);
            div()
                .truncate()
                .child(markdown::render_inline(&inline, &opts, &format!("th-{}", e.id), None, cx))
                .into_any_element()
        } else if warning {
            div()
                .font_medium()
                .text_color(label_color)
                .child(label.clone())
                .into_any_element()
        } else {
            div()
                .truncate()
                .text_color(label_color)
                .child(label.clone())
                .into_any_element()
        };
        let trailing = Some(self.hover_timestamp(&format!("{}-{}", row.id, e.id), e.created_at, &group, cx));
        let line = work_line(
            e.icon(),
            tint,
            label_el,
            trailing,
            has_details.then_some(expanded),
            cx,
        );
        let id = e.id.clone();
        let details = (expanded && has_details).then(|| self.render_details(e, &label, window, cx));
        v_flex()
            .id(SharedString::from(format!("{}-e-{}", row.id, e.id)))
            .group(group)
            .w_full()
            .when(!in_group, |el| el.py(px(2.)))
            .child(
                div()
                    .id(SharedString::from(format!("{}-eline-{}", row.id, e.id)))
                    .w_full()
                    .when(has_details, |el| {
                        el.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
                            let id = id.clone();
                            this.update_disclosures(cx, |d| {
                                if !d.entries.remove(&id) {
                                    d.entries.insert(id);
                                }
                            })
                        }))
                    })
                    .child(line),
            )
            .children(details)
            .into_any_element()
    }

    fn approval_pending(&self, e: &WorkEntry) -> bool {
        let TurnItemBody::ApprovalRequest { request_id, .. } = &e.item.body else {
            return false;
        };
        self.detail.as_ref().is_some_and(|d| {
            d.requests
                .iter()
                .any(|r| &r.id == request_id && r.status == tern_core::RequestStatus::Pending)
        })
    }

    /// The pending approval row (`TimelinePendingApprovalRow.tsx`).
    fn render_approval(&mut self, row: &Row, e: &WorkEntry, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let TurnItemBody::ApprovalRequest {
            request_id,
            request_kind,
            prompt,
            reason,
            options,
            ..
        } = &e.item.body
        else {
            return div().into_any_element();
        };
        let kind_icon = match request_kind {
            RequestKind::Command => IconName::Terminal,
            RequestKind::FileChange => IconName::SquarePen,
            RequestKind::FileRead => IconName::Eye,
            RequestKind::Permission => IconName::Wrench,
            _ => IconName::Hammer,
        };
        let kind_label = match request_kind {
            RequestKind::Command => "Command approval",
            RequestKind::FileChange => "File change approval",
            RequestKind::FileRead => "File read approval",
            RequestKind::Permission => "App permission approval",
            _ => "App access approval",
        };
        let detail_text = prompt.clone().unwrap_or_else(|| kind_label.to_string());
        let session_gone = self
            .detail
            .as_ref()
            .is_some_and(|d| d.session.is_none());
        let defaults = vec![
            ApprovalOption {
                decision: ApprovalDecision::Cancel,
                label: "Cancel".into(),
                warning: None,
            },
            ApprovalOption {
                decision: ApprovalDecision::Decline,
                label: "Decline".into(),
                warning: None,
            },
            ApprovalOption {
                decision: ApprovalDecision::AcceptForSession,
                label: "Always allow this session".into(),
                warning: None,
            },
            ApprovalOption {
                decision: ApprovalDecision::Accept,
                label: "Approve".into(),
                warning: None,
            },
        ];
        let options = options.clone().filter(|o| !o.is_empty()).unwrap_or(defaults);
        let (primary, more): (Vec<_>, Vec<_>) = options.into_iter().partition(|o| {
            matches!(o.decision, ApprovalDecision::Decline | ApprovalDecision::Accept)
        });
        let thread_id = self.thread.clone().unwrap_or_default();
        let respond = {
            let store = self.store.clone();
            let thread_id = thread_id.clone();
            let request_id = request_id.clone();
            move |decision: ApprovalDecision| {
                let store = store.clone();
                let thread_id = thread_id.clone();
                let request_id = request_id.clone();
                move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                    store.read(cx).send(Command::RespondToRequest {
                        thread_id: thread_id.clone(),
                        request_id: request_id.clone(),
                        decision: Some(decision),
                        feedback: None,
                        answers: None,
                    })
                }
            }
        };
        let mut actions = h_flex()
            .gap(px(8.))
            .child(
                h_flex()
                    .gap(px(4.))
                    .text_color(t.warning)
                    .child(icon(IconName::Shield, 12., t.warning))
                    .child("Waiting for approval"),
            )
            .child(div().text_color(t.muted_foreground).child("·"));
        if session_gone {
            actions = h_flex().child(
                div()
                    .text_color(t.warning)
                    .child("Provider process is gone. Interrupt or restart the run to respond."),
            );
        } else {
            // Default layout: Decline then Approve, matching T3's order.
            let mut primary = primary;
            primary.sort_by_key(|o| matches!(o.decision, ApprovalDecision::Accept));
            for o in primary {
                actions = actions.child(
                    InlineButton::new(
                        SharedString::from(format!("{}-{:?}", row.id, o.decision)),
                        o.label.clone(),
                    )
                    .tone(if o.decision == ApprovalDecision::Accept {
                        InlineTone::Default
                    } else {
                        InlineTone::Muted
                    })
                    .on_click(respond(o.decision)),
                );
            }
            if !more.is_empty() {
                let more_items = more.clone();
                let store = self.store.clone();
                let thread_id = thread_id.clone();
                let request_id = request_id.clone();
                let menu = move |_: &mut Window, _: &mut App| {
                    let mut m = Menu::new().fit_content();
                    for o in &more_items {
                        let store = store.clone();
                        let thread_id = thread_id.clone();
                        let request_id = request_id.clone();
                        let decision = o.decision;
                        m = m.item(MenuItem::new(o.label.clone()).on_select(move |_, cx| {
                            store.read(cx).send(Command::RespondToRequest {
                                thread_id: thread_id.clone(),
                                request_id: request_id.clone(),
                                decision: Some(decision),
                                feedback: None,
                                answers: None,
                            })
                        }));
                    }
                    m
                };
                let trigger_id = SharedString::from(format!("{}-more", row.id));
                actions = actions.child(
                    DropdownMenu::new(SharedString::from(format!("{}-more-menu", row.id)), menu, move |open| {
                        InlineButton::new(trigger_id, "More")
                            .tone(InlineTone::Muted)
                            .when(open, |b| b.text_color(gpui::black()).underline())
                            .into_any_element()
                    })
                    .placement(gpui_base::Placement::Top)
                    .align(gpui_base::Align::Start),
                );
            }
        }
        v_flex()
            .w_full()
            .py(px(2.))
            .child(
                h_flex()
                    .items_start()
                    .w_full()
                    .px(px(2.))
                    .gap(px(6.))
                    .child(
                        div()
                            .w(px(24.))
                            .h(px(20.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(kind_icon, 16., t.icon_muted)),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("{}-approval-code", row.id)))
                            .flex_1()
                            .min_w_0()
                            .max_h(px(160.))
                            .overflow_y_scroll()
                            .font_family(font::MONO)
                            .text_size(px(12.))
                            .line_height(px(20.))
                            .text_color(t.foreground)
                            .child(detail_text),
                    ),
            )
            .child(
                v_flex()
                    .pl(px(30.))
                    .gap(px(4.))
                    .pb(px(4.))
                    .text_scale(TextSize::Xs)
                    .children(reason.clone().map(|r| div().text_color(t.muted_foreground).child(r)))
                    .child(actions),
            )
            .into_any_element()
    }

    /// Failed provider errors and usage limits (§3.7).
    fn render_error(&mut self, row: &Row, e: &WorkEntry, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let TurnItemBody::Error { failure, .. } = &e.item.body else {
            return div().into_any_element();
        };
        let usage = failure.class == FailureClass::UsageLimit;
        let color = if usage { t.warning } else { t.destructive };
        let label = match (&failure.reset_at, usage) {
            (Some(reset), true) => format!("Usage limit reached. Retry after {}.", reset_label(reset)),
            _ => e.label.clone(),
        };
        let detail = e.detail.clone().filter(|d| !d.trim().is_empty() && *d != label);
        v_flex()
            .w_full()
            .py(px(2.))
            .child(
                h_flex()
                    .items_start()
                    .w_full()
                    .px(px(2.))
                    .gap(px(6.))
                    .text_size(px(14.))
                    .line_height(px(LINE))
                    .child(
                        div()
                            .w(px(24.))
                            .h(px(LINE))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(IconName::CircleAlert, 16., color)),
                    )
                    .child(div().flex_1().min_w_0().font_medium().text_color(color).child(label))
                    .child(
                        div()
                            .flex_none()
                            .text_scale(TextSize::Xs)
                            .text_color(t.muted_foreground)
                            .child(format::timestamp(e.created_at, chrono::Utc::now())),
                    ),
            )
            .children(detail.map(|d| {
                div()
                    .pl(px(30.))
                    .py(px(4.))
                    .text_size(px(14.))
                    .line_height(px(LINE))
                    .text_color(t.foreground.opacity(0.8))
                    .child(d)
            }))
            .id(SharedString::from(format!("{}-err", row.id)))
            .into_any_element()
    }

    /// Expanded details of one entry (`V2ItemInspector`, §3.3).
    fn render_details(
        &mut self,
        e: &WorkEntry,
        visible_label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *cx.tokens();
        let root = self.workspace_root(cx);
        let muted = t.muted_foreground;
        let panel = v_flex()
            .w_full()
            .mt(px(2.))
            .mb(px(6.))
            .pb(px(8.))
            .gap(px(8.))
            .text_scale(TextSize::Xs);
        match &e.item.body {
            TurnItemBody::Reasoning { text, .. } => {
                let doc = self.md.get(&format!("reasoning:{}", e.id), text, false);
                let mut opts = markdown::Options::new(format!("r-{}", e.id));
                opts.text_color = Some(t.foreground);
                let body = markdown::render(&doc, &opts, window, cx);
                div()
                    .id(SharedString::from(format!("{}-reasoning", e.id)))
                    .pl(px(28.))
                    .px(px(4.))
                    .py(px(2.))
                    .max_h(px(384.))
                    .overflow_y_scroll()
                    .child(body)
                    .into_any_element()
            }
            TurnItemBody::CommandExecution {
                input,
                output,
                exit_code,
                ..
            } => {
                let live = self
                    .detail
                    .as_ref()
                    .and_then(|d| d.live_output.get(&e.id).cloned());
                let out = live.or_else(|| output.clone());
                let cmd_text = format::command_display(input);
                let highlights = markdown::highlight::highlight(Some("bash"), &cmd_text, cx.tern_theme().is_dark());
                let cmd = div()
                    .w_full()
                    .font_family(font::MONO)
                    .text_size(px(font::CODE_SIZE))
                    .line_height(relative(leading::RELAXED))
                    .text_color(t.foreground.opacity(0.85))
                    .child(StyledText::new(cmd_text.clone()).with_default_highlights(
                        &gpui::TextStyle {
                            color: t.foreground.opacity(0.85),
                            font_family: font::MONO.into(),
                            font_size: px(font::CODE_SIZE).into(),
                            line_height: relative(leading::RELAXED),
                            ..Default::default()
                        },
                        highlights.iter().cloned(),
                    ));
                let output_el: AnyElement = match out.as_deref().map(strip_ansi) {
                    Some(o) if !o.trim().is_empty() => div()
                        .id(SharedString::from(format!("{}-output", e.id)))
                        .w_full()
                        .max_h(px(320.))
                        .overflow_y_scroll()
                        .child(mono_block(o.trim_end().to_string(), muted))
                        .into_any_element(),
                    _ if e.status == Lifecycle::InProgress => {
                        mono_block("Loading output…".into(), muted).into_any_element()
                    }
                    _ => mono_block("No output.".into(), muted).into_any_element(),
                };
                panel
                    .child(cmd)
                    .child(output_el)
                    .children(
                        exit_code
                            .filter(|c| *c != 0)
                            .map(|c| div().font_family(font::MONO).text_color(t.destructive).child(format!("exit {c}"))),
                    )
                    .into_any_element()
            }
            TurnItemBody::FileChange {
                file_name,
                additions,
                deletions,
                changes,
                ..
            } => {
                let diff = cx.tern_theme().diff;
                let path = super::entries::relative_path(file_name, root.as_deref());
                let app = self.app.clone();
                panel
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(div().font_family(font::MONO).text_color(muted).child(path))
                            .children(additions.map(|a| {
                                div().font_family(font::MONO).text_color(diff.diff_addition).child(format!("+{a}"))
                            }))
                            .children(deletions.map(|d| {
                                div().font_family(font::MONO).text_color(diff.diff_deletion).child(format!("-{d}"))
                            }))
                            .child(
                                Button::new(SharedString::from(format!("{}-open-diff", e.id)))
                                    .variant(ButtonVariant::Outline)
                                    .size(ButtonSize::Xs)
                                    .label("Open diff")
                                    .on_click(move |_, _, cx| {
                                        app.update(cx, |s, cx| {
                                            if !s.right_panel.open {
                                                s.toggle_right_panel(cx);
                                            }
                                        })
                                    }),
                            ),
                    )
                    .children(changes.as_ref().filter(|c| c.len() > 1).map(|changes| {
                        v_flex().gap(px(2.)).children(changes.iter().map(|c| {
                            let text = match &c.old_path {
                                Some(old) => format!("{} {old} → {}", c.operation, c.path),
                                None => format!("{} {}", c.operation, c.path),
                            };
                            div().font_family(font::MONO).text_color(muted).child(text)
                        }))
                    }))
                    .into_any_element()
            }
            TurnItemBody::FileSearch { pattern, results } => panel
                .children(
                    pattern
                        .clone()
                        .filter(|p| !visible_label.contains(p.as_str()))
                        .map(|p| mono_block(p, t.foreground.opacity(0.85))),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("{}-results", e.id)))
                        .max_h(px(320.))
                        .overflow_y_scroll()
                        .child(mono_block(
                            results
                                .as_ref()
                                .map(|rs| {
                                    rs.iter()
                                        .map(|r| match (r.line, &r.preview) {
                                            (Some(l), Some(p)) => format!("{}:{l}  {}", r.path, p.trim()),
                                            (Some(l), None) => format!("{}:{l}", r.path),
                                            _ => r.path.clone(),
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                                .unwrap_or_else(|| "No output.".into()),
                            muted,
                        )),
                )
                .into_any_element(),
            TurnItemBody::WebSearch { patterns, results } => panel
                .children(patterns.as_ref().map(|p| mono_block(p.join("\n"), t.foreground.opacity(0.85))))
                .children(results.as_ref().map(|rs| {
                    v_flex().gap(px(4.)).children(rs.iter().enumerate().map(|(i, r)| {
                        let url = r.url.clone();
                        v_flex()
                            .child(
                                h_flex()
                                    .id(SharedString::from(format!("{}-web{i}", e.id)))
                                    .gap(px(4.))
                                    .font_medium()
                                    .text_color(t.foreground)
                                    .cursor_pointer()
                                    .hover(|s| s.underline())
                                    .on_click(move |_, _, cx| cx.open_url(&url))
                                    .child(r.title.clone().unwrap_or_else(|| r.url.clone()))
                                    .child(icon(IconName::ExternalLink, 12., muted)),
                            )
                            .child(div().text_color(muted).truncate().child(r.url.clone()))
                    }))
                }))
                .into_any_element(),
            TurnItemBody::DynamicTool { input, output, .. } => {
                let args: Vec<AnyElement> = match input.as_object() {
                    Some(map) => map
                        .iter()
                        .map(|(k, v)| {
                            let value = match v {
                                serde_json::Value::String(s) => s.clone(),
                                other => serde_json::to_string_pretty(other).unwrap_or_default(),
                            };
                            h_flex()
                                .items_start()
                                .gap(px(8.))
                                .font_family(font::MONO)
                                .text_size(px(font::CODE_SIZE))
                                .line_height(relative(leading::RELAXED))
                                .child(div().flex_none().text_color(muted).child(k.clone()))
                                .child(div().min_w_0().text_color(t.foreground.opacity(0.85)).child(value))
                                .into_any_element()
                        })
                        .collect(),
                    None => vec![mono_block(input.to_string(), t.foreground.opacity(0.85)).into_any_element()],
                };
                let out = output.as_ref().map(|o| match o {
                    serde_json::Value::String(s) => s.clone(),
                    other => serde_json::to_string_pretty(other).unwrap_or_default(),
                });
                panel
                    .children(args)
                    .children(out.filter(|o| !o.trim().is_empty()).map(|o| {
                        div()
                            .id(SharedString::from(format!("{}-dyn-out", e.id)))
                            .max_h(px(320.))
                            .overflow_y_scroll()
                            .child(mono_block(o, muted))
                    }))
                    .into_any_element()
            }
            TurnItemBody::ApprovalRequest { prompt, reason, .. } => panel
                .children(prompt.clone().map(|p| mono_block(p, t.foreground.opacity(0.85))))
                .children(reason.clone().map(|r| div().text_color(muted).child(r)))
                .children(self.request_resolution(e).map(|r| div().text_color(muted).child(r)))
                .into_any_element(),
            TurnItemBody::UserInputRequest {
                questions,
                question_answer,
                ..
            } => {
                let answers = question_answer.as_ref().map(|a| &a.answers);
                v_flex()
                    .pl(px(28.))
                    .mt(px(8.))
                    .mb(px(6.))
                    .gap(px(8.))
                    .children(questions.iter().map(|q| {
                        let answer = answers
                            .and_then(|a| a.get(&q.id))
                            .map(answer_text)
                            .unwrap_or_default();
                        v_flex()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_scale(TextSize::Sm)
                                    .text_color(muted)
                                    .child(q.question.clone()),
                            )
                            .child(
                                div()
                                    .pl(px(12.))
                                    .text_scale(TextSize::Sm)
                                    .text_color(t.foreground)
                                    .child(if answer.is_empty() {
                                        q.options
                                            .iter()
                                            .map(|o| format!("• {}", o.label))
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    } else {
                                        answer
                                    }),
                            )
                    }))
                    .into_any_element()
            }
            TurnItemBody::TodoList { steps, explanation, .. } => panel
                .children(explanation.clone().map(|x| div().text_color(muted).child(x)))
                .child(mono_block(
                    steps
                        .iter()
                        .map(|s| {
                            let mark = match s.status {
                                PlanStepStatus::Completed => "✓",
                                PlanStepStatus::Running => "◉",
                                PlanStepStatus::Pending => "○",
                            };
                            format!("{mark} {}", s.text)
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    t.foreground.opacity(0.85),
                ))
                .into_any_element(),
            TurnItemBody::SystemNotice { message, .. } => {
                panel.child(div().text_color(muted).child(message.clone())).into_any_element()
            }
            TurnItemBody::Compaction { summary, .. } => panel
                .children(summary.clone().map(|s| div().text_color(muted).child(s)))
                .into_any_element(),
            _ => match super::entries::expanded_body(e, root.as_deref(), visible_label) {
                Some(body) => div()
                    .id(SharedString::from(format!("{}-pre", e.id)))
                    .max_h(px(256.))
                    .overflow_y_scroll()
                    .child(mono_block(body, t.secondary_label))
                    .into_any_element(),
                None => div().into_any_element(),
            },
        }
    }

    fn request_resolution(&self, e: &WorkEntry) -> Option<String> {
        let TurnItemBody::ApprovalRequest { request_id, .. } = &e.item.body else {
            return None;
        };
        let req = self.detail.as_ref()?.requests.iter().find(|r| &r.id == request_id)?;
        let decision = match req.decision? {
            ApprovalDecision::Accept => "Approved",
            ApprovalDecision::AcceptForSession | ApprovalDecision::AcceptAlways => {
                "Approved for this session"
            }
            ApprovalDecision::Decline => "Declined",
            ApprovalDecision::Cancel => "Cancelled",
        };
        Some(match &req.feedback {
            Some(f) if !f.trim().is_empty() => format!("{decision} · {f}"),
            _ => decision.to_string(),
        })
    }
}

/// Whether an entry opens to anything.
fn has_details(e: &WorkEntry) -> bool {
    match &e.item.body {
        TurnItemBody::Reasoning { text, .. } => !text.trim().is_empty(),
        TurnItemBody::CommandExecution { .. }
        | TurnItemBody::FileChange { .. }
        | TurnItemBody::FileSearch { .. }
        | TurnItemBody::WebSearch { .. }
        | TurnItemBody::DynamicTool { .. }
        | TurnItemBody::ApprovalRequest { .. }
        | TurnItemBody::UserInputRequest { .. }
        | TurnItemBody::TodoList { .. } => true,
        TurnItemBody::Compaction { summary, .. } => summary.is_some(),
        _ => e.tone == Tone::Tool,
    }
}

fn first_line(s: &str) -> String {
    let line = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    line.chars().take(2048).collect()
}

fn answer_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a
            .iter()
            .map(answer_text)
            .collect::<Vec<_>>()
            .join(", "),
        serde_json::Value::Object(o) => o.get("answers").map(answer_text).unwrap_or_default(),
        other => other.to_string(),
    }
}

/// Question text and answer preview of an answered `user_input_request`.
fn question_answer_preview(e: &WorkEntry) -> (String, String) {
    let TurnItemBody::UserInputRequest {
        questions,
        question_answer,
        ..
    } = &e.item.body
    else {
        return (e.label.clone(), String::new());
    };
    let Some(q) = questions.first() else {
        return (e.label.clone(), String::new());
    };
    let answer = question_answer
        .as_ref()
        .and_then(|a| a.answers.get(&q.id))
        .map(answer_text)
        .unwrap_or_default();
    (collapse_ws(&q.question), collapse_ws(&answer))
}

/// "7:00 AM (UTC)"-style reset time from an RFC 3339 or epoch string.
fn reset_label(reset: &str) -> String {
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(reset) {
        let local = t.with_timezone(&chrono::Local);
        return format!("{} ({})", local.format("%-I:%M %p"), local.format("%Z"));
    }
    reset.to_string()
}

/// Drops ANSI escape sequences (colors are not drawn yet).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[allow(dead_code)]
fn unused(_: ItemStatus) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m ok"), "red ok");
    }

    #[test]
    fn answers_flatten() {
        assert_eq!(answer_text(&serde_json::json!("bash")), "bash");
        assert_eq!(answer_text(&serde_json::json!(["a", "b"])), "a, b");
        assert_eq!(answer_text(&serde_json::json!({"answers": ["x"]})), "x");
    }
}

//! Drawing the composer (spec 04 §1, §4.3, §5, §7.2, §8.1).

use gpui::{
    AnyElement, App, BoxShadow, Context, ExternalPaths, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, Window, div, img, point,
    prelude::FluentBuilder as _, px,
};
use gpui_component::Sizable as _;
use gpui_component::input::{Enter, Escape, IndentInline, MoveDown, MoveUp, OutdentInline};

use super::drafts::DraftKey;
use super::{ComposerView, MenuItem, file_index, model, pending, permissions};
use crate::app_state::Route;
use crate::assets::IconName;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, color, layout, radius};
use crate::ui::input::Textarea;
use crate::ui::popup::PopupLayer;
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Icon, Kbd, Tooltip, TooltipExt as _, bounds_probe, h_flex,
    v_flex,
};

/// Editor line height: 14px at `leading-relaxed`.
pub const LINE_HEIGHT: f32 = 22.75;
/// The attached-surface inset (`--chat-composer-drawer-inset`).
const DRAWER_INSET: f32 = 22.;
/// How far attached surfaces tuck under the card.
const TUCK: f32 = 17.;

/// The composer card's three layers (spec 04 §1.2), with GPUI's lack of backdrop
/// blur: the glass fill is the 80% surface over the page background.
pub struct CardLook {
    pub fill: Hsla,
    pub outline: Hsla,
    pub shadow: Vec<BoxShadow>,
}

pub fn card_look(cx: &App) -> CardLook {
    let theme = cx.tern_theme();
    let t = theme.colors;
    let dark = theme.is_dark();
    let surface = if dark { t.surface_raised } else { t.card };
    let fill = color::over(surface.opacity(0.8), t.background);
    let outline = if dark {
        gpui::white().opacity(0.05)
    } else {
        gpui::black().opacity(0.08)
    };
    let shadow = if dark {
        // `inset-shadow-2xs` in the composer highlight (white/3%).
        vec![BoxShadow {
            color: gpui::white().opacity(0.03),
            offset: point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
            inset: true,
        }]
    } else {
        theme.shadows.composer()
    };
    CardLook {
        fill,
        outline,
        shadow,
    }
}

/// `ComposerControl` size `sm`: h28, px8, gap 6, radius 8, medium 14, fg/80.
pub fn control_sm(
    id: impl Into<gpui::ElementId>,
    open: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let t = cx.tokens();
    let (fg, accent) = (t.foreground, t.accent);
    h_flex()
        .id(id)
        .relative()
        .flex_none()
        .h(px(28.))
        .px(px(8.))
        .gap(px(6.))
        .rounded(radius::CONTROL)
        .border_1()
        .border_color(gpui::transparent_black())
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(14.))
        .line_height(px(20.))
        .whitespace_nowrap()
        .cursor_pointer()
        .text_color(if open { fg } else { fg.opacity(0.8) })
        .when(open, |el| el.bg(accent))
        .hover(move |s| s.bg(accent).text_color(fg))
}

impl ComposerView {
    fn is_hero(&self, cx: &App) -> bool {
        matches!(self.key, Some(DraftKey::New(_)))
            && matches!(
                self.app.read(cx).chat_route(),
                Route::Draft(_) | Route::Index
            )
    }

    pub(crate) fn placeholder(&self, cx: &App) -> &'static str {
        if let Some(detail) = self.thread_detail(cx) {
            if !pending::pending_approvals(&detail).is_empty() {
                return "Send feedback to decline this request";
            }
            if let Some(q) = self.current_question(cx) {
                let custom = q.allow_custom_answer.unwrap_or(true);
                return if !q.options.is_empty() && !custom {
                    "Choose an option above"
                } else if q.options.is_empty() {
                    "Type your answer"
                } else {
                    "Type your own answer, or leave this blank to use the selected option"
                };
            }
        }
        if self.key.is_none() {
            return "Choose a project above to start a thread";
        }
        if model::usable(&self.store.read(cx).snapshot().providers)
            .next()
            .is_none()
        {
            return "Enable a provider in Settings to send a message";
        }
        "Ask anything. @ files, $ skills, / commands"
    }

    fn render_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let empty = self.text(cx).is_empty();
        let placeholder = self.placeholder(cx);
        let weak = cx.entity().downgrade();
        let paste = weak.clone();
        let readonly = self.key.is_none()
            || self
                .current_question(cx)
                .is_some_and(|q| !q.options.is_empty() && q.allow_custom_answer == Some(false));
        let editor = Textarea::new(&self.editor)
            .appearance(false)
            .xsmall()
            .readonly(readonly)
            .text_size(px(14.))
            .line_height(px(LINE_HEIGHT))
            .token(|token, _, cx| {
                let t = cx.tokens();
                let is_file = token.token().id().starts_with("file:");
                h_flex()
                    .gap(px(3.))
                    .font_weight(FontWeight::MEDIUM)
                    .when(token.is_selected(), |el| {
                        el.bg(t.primary.opacity(0.3)).rounded(radius::SM)
                    })
                    .child(
                        Icon::new(if is_file {
                            IconName::File
                        } else {
                            IconName::Zap
                        })
                        .size(px(14.)),
                    )
                    .child(token.token().label().clone())
            })
            .on_paste(move |item, _, cx| {
                paste
                    .update(cx, |this, cx| this.on_paste(item, cx))
                    .unwrap_or(false)
            });
        div()
            .relative()
            .min_h(px(LINE_HEIGHT * 2.))
            .text_size(px(14.))
            .line_height(px(LINE_HEIGHT))
            .text_color(t.foreground)
            .capture_action(cx.listener(|this, a: &Enter, window, cx| {
                if this.on_enter(a.shift, a.secondary, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                if this.on_arrow(true, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                if this.on_arrow(false, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                if this.on_tab(false, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &OutdentInline, window, cx| {
                if this.on_tab(true, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Escape, _, cx| {
                if this.on_escape(cx) {
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                // 1-9 pick a question option while the editor is empty.
                let k = &e.keystroke;
                if !this.question.request_id.is_empty()
                    && !k.modifiers.modified()
                    && this.text(cx).is_empty()
                    && let Ok(n) = k.key.parse::<usize>()
                    && (1..=9).contains(&n)
                    && this
                        .current_question(cx)
                        .is_some_and(|q| n <= q.options.len())
                {
                    this.select_option(n - 1, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(div().mx(px(-4.)).child(editor))
            .when(empty, |el| {
                el.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .text_color(t.placeholder)
                        .truncate()
                        .child(placeholder),
                )
            })
            .into_any_element()
    }

    fn render_attachments(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.attachments.is_empty() {
            return None;
        }
        let t = *cx.tokens();
        let mut images = Vec::new();
        let mut files = Vec::new();
        for (i, a) in self.attachments.iter().enumerate() {
            let id = a.attachment.id().to_string();
            let name = a.attachment.name().to_string();
            let remove_id = id.clone();
            match &a.attachment {
                tern_core::ChatAttachment::Image { .. } => images.push(
                    div()
                        .id(("composer-image", i))
                        .relative()
                        .size(px(64.))
                        .rounded(radius::LG)
                        .overflow_hidden()
                        .border_1()
                        .border_color(t.border.opacity(0.8))
                        .bg(t.background)
                        .cursor(gpui::CursorStyle::PointingHand)
                        .child(
                            img(a.path.clone())
                                .size_full()
                                .object_fit(gpui::ObjectFit::Cover),
                        )
                        .child(
                            div()
                                .id(("composer-image-remove", i))
                                .absolute()
                                .top(px(4.))
                                .right(px(4.))
                                .size(px(24.))
                                .rounded(radius::FULL)
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(gpui::black().opacity(0.65))
                                .border_1()
                                .border_color(gpui::white().opacity(0.2))
                                .hover(|s| s.bg(gpui::black().opacity(0.8)))
                                .t3_tooltip(Tooltip::text(format!("Remove {name}")))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_attachment(&remove_id, cx)
                                }))
                                .child(Icon::new(IconName::X).size(px(14.)).color(gpui::white())),
                        )
                        .into_any_element(),
                ),
                tern_core::ChatAttachment::File { size_bytes, .. } => files.push(
                    h_flex()
                        .gap(px(8.))
                        .py(px(4.))
                        .text_scale(TextSize::Sm)
                        .child(
                            Icon::new(IconName::File)
                                .size(px(16.))
                                .color(t.muted_foreground),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .child(SharedString::from(name.clone())),
                        )
                        .child(
                            div()
                                .text_scale(TextSize::Xs)
                                .text_color(t.secondary_label)
                                .child(format_size(*size_bytes)),
                        )
                        .child(
                            Button::new(("composer-file-remove", i))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::IconXs)
                                .icon(IconName::X)
                                .tooltip(Tooltip::text(format!("Remove {name}")))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_attachment(&id, cx)
                                })),
                        )
                        .into_any_element(),
                ),
            }
        }
        Some(
            v_flex()
                .when(!images.is_empty(), |el| {
                    el.child(
                        h_flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .mb(px(12.))
                            .children(images),
                    )
                })
                .when(!files.is_empty(), |el| {
                    el.child(v_flex().gap(px(4.)).mb(px(12.)).children(files))
                })
                .into_any_element(),
        )
    }

    fn render_model_trigger(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let store = self.store.read(cx);
        let providers = store.snapshot().providers.clone();
        let Some(sel) = self.selection(cx) else {
            return control_sm("composer-model", false, cx)
                .child(
                    Icon::new(IconName::CircleAlert)
                        .size(px(16.))
                        .color(t.muted_foreground),
                )
                .child("No provider available")
                .opacity(crate::ui::DISABLED_OPACITY)
                .into_any_element();
        };
        let open = PopupLayer::is_open(&"composer-model-picker".into(), cx);
        let found = model::find_model(&providers, &sel);
        let (name, short, effort, driver) = match found {
            Some((p, m)) => (
                m.name.clone(),
                model::short_name(m),
                model::effort_suffix(m, &sel),
                Some(p.driver),
            ),
            None => (
                sel.model.clone(),
                model::derive_short_name(&sel.model),
                None,
                None,
            ),
        };
        let tip = match &effort {
            Some(e) => format!("{name}, {e}"),
            None => name.clone(),
        };
        let icon = driver.map(|d| {
            Icon::new(model::provider_icon(d))
                .size(px(16.))
                .color(model::provider_icon_color(d).unwrap_or(t.foreground))
        });
        let group: SharedString = "composer-model-trigger".into();
        control_sm("composer-model", open, cx)
            .group(group.clone())
            .min_w(px(52.))
            .flex_shrink(1.)
            .child(bounds_probe(self.model_bounds.clone()))
            .children(icon)
            .child(div().min_w_0().max_w(px(224.)).truncate().child(short))
            .when_some(effort, |el, effort| {
                el.child(
                    div()
                        .ml(px(-2.))
                        .max_w(px(96.))
                        .truncate()
                        .text_color(if open {
                            t.foreground
                        } else {
                            t.muted_foreground
                        })
                        .group_hover(group, |s| s.text_color(t.foreground))
                        .child(effort),
                )
            })
            .t3_tooltip(Tooltip::text(tip).kbd("Ctrl+Shift+M"))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(|this, _, window, cx| this.open_model_picker(false, window, cx)))
            .into_any_element()
    }

    fn render_permissions_chip(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        // The new-thread strip already names the mode.
        if !matches!(self.key, Some(DraftKey::Thread(_))) {
            return None;
        }
        let mode = self.runtime_mode(cx);
        if mode == permissions::DEFAULT_MODE {
            return None;
        }
        let t = *cx.tokens();
        let open = PopupLayer::is_open(&"composer-permissions".into(), cx);
        let bounds = self.permission_bounds.clone();
        Some(
            control_sm("composer-permissions-chip", open, cx)
                .pl(px(6.))
                .text_color(t.foreground)
                .child(bounds_probe(self.permission_bounds.clone()))
                .child(Icon::new(permissions::icon(mode)).size(px(16.)))
                .child(permissions::label(mode))
                .t3_tooltip(Tooltip::text(format!(
                    "Permissions: {}. {}",
                    permissions::label(mode),
                    permissions::description(mode)
                )))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_permissions("composer-permissions", bounds.get(), window, cx)
                }))
                .into_any_element(),
        )
    }

    fn render_primary(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.tern_theme().clone();
        let t = theme.colors;
        let s = *cx.states();
        let has_content = self.has_content(cx);
        let running = self.is_running(cx);
        let question = (!self.question.request_id.is_empty()).then_some(());
        if question.is_some() {
            let count = self.question_count(cx);
            let page = self.question.page;
            let label = if page + 1 < count {
                "Next question"
            } else if count > 1 {
                "Submit answers"
            } else {
                "Submit answer"
            };
            let answered = self.current_question(cx).is_some_and(|q| {
                pending::is_answered(&q, self.question.answers.get(&q.id)) || has_content
            });
            let pill = h_flex()
                .id("composer-question-submit")
                .h(px(28.))
                .px(px(16.))
                .rounded(radius::FULL)
                .text_scale(TextSize::Sm)
                .font_weight(FontWeight::MEDIUM)
                .when(answered, |el| {
                    el.bg(t.message_action)
                        .text_color(t.message_action_foreground)
                        .cursor_pointer()
                        .hover(|st| st.bg(t.message_action_hover))
                        .on_click(cx.listener(|this, _, window, cx| this.submit(false, window, cx)))
                })
                .when(!answered, |el| {
                    el.bg(t.foreground.opacity(0.12))
                        .text_color(t.muted_foreground)
                })
                .child(label);
            return h_flex()
                .gap(px(8.))
                .when(page > 0, |el| {
                    el.child(
                        Button::new("composer-question-prev")
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Sm)
                            .label("Previous")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.go_to_question(page - 1, window, cx)
                            })),
                    )
                })
                .child(pill)
                .into_any_element();
        }
        if running && !has_content {
            let stopping = self.stopping.is_some();
            let bg = t.destructive;
            return div()
                .id("composer-stop")
                .size(px(32.))
                .flex_none()
                .rounded(radius::FULL)
                .flex()
                .items_center()
                .justify_center()
                .bg(bg.opacity(0.9))
                .shadow(vec![
                    BoxShadow {
                        color: s.destructive_shadow,
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(2.),
                        spread_radius: px(0.),
                        inset: false,
                    },
                    theme.shadows.control_highlight(),
                ])
                .when(!stopping, |el| {
                    el.cursor_pointer()
                        .hover(move |st| st.bg(bg))
                        .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
                })
                .when(stopping, |el| el.opacity(crate::ui::DISABLED_OPACITY))
                .t3_tooltip(Tooltip::text(if stopping {
                    "Stopping…"
                } else {
                    "Interrupt"
                }))
                .child(div().size(px(8.)).rounded(px(1.5)).bg(gpui::white()))
                .into_any_element();
        }
        let (glyph, tip): (AnyElement, String) = if running {
            if self.follow_up_steers(false, cx) {
                (
                    Icon::new(IconName::CornerUpRight)
                        .size(px(16.))
                        .into_any_element(),
                    "Click to steer, Ctrl-click or Ctrl+Enter to queue".into(),
                )
            } else {
                (
                    Icon::new(IconName::ListPlus)
                        .size(px(16.))
                        .into_any_element(),
                    "Click to queue, Ctrl-click or Ctrl+Enter to steer".into(),
                )
            }
        } else {
            (
                Icon::new(IconName::TernSend)
                    .size(px(14.))
                    .into_any_element(),
                "Submit message".into(),
            )
        };
        let enabled = has_content && self.key.is_some();
        let (bg, hover) = (t.message_action, t.message_action_hover);
        let send = div()
            .id("composer-send")
            .size(px(32.))
            .flex_none()
            .rounded(radius::FULL)
            .flex()
            .items_center()
            .justify_center()
            .t3_tooltip(Tooltip::text(tip))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .when(enabled, |el| {
                el.bg(bg)
                    .text_color(t.message_action_foreground)
                    .shadow(vec![
                        BoxShadow {
                            color: bg.opacity(0.24),
                            offset: point(px(0.), px(1.)),
                            blur_radius: px(2.),
                            spread_radius: px(0.),
                            inset: false,
                        },
                        theme.shadows.control_highlight(),
                    ])
                    .cursor_pointer()
                    .hover(move |st| st.bg(hover))
                    .on_click(cx.listener(|this, e: &gpui::ClickEvent, window, cx| {
                        let flip = e.modifiers().control;
                        this.submit(flip, window, cx);
                    }))
            })
            .when(!enabled, |el| {
                el.bg(t.foreground.opacity(0.12))
                    .text_color(t.muted_foreground)
            })
            .child(glyph);
        let _ = s;
        send.into_any_element()
    }

    fn render_footer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.render_model_trigger(cx);
        let chip = self.render_permissions_chip(cx);
        let primary = self.render_primary(cx);
        let question = !self.question.request_id.is_empty();
        let attach = (!question).then(|| {
            div().mr(px(8.)).flex_none().child(
                Button::new("composer-attach")
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::IconSm)
                    .icon(IconName::Paperclip)
                    .tooltip(Tooltip::text("Attach files"))
                    .on_click(cx.listener(|this, _, window, cx| this.pick_files(window, cx))),
            )
        });
        h_flex()
            .flex_nowrap()
            .px(px(8.))
            .pb(px(8.))
            .when(question, |el| el.pt(px(8.)))
            .children(attach)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .justify_end()
                            .gap(px(2.))
                            .child(model)
                            .children(chip),
                    )
                    .child(primary),
            )
            .into_any_element()
    }

    /// The slash / mention menu, attached above the card and tucked under it.
    fn render_menu(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.clone()?;
        let t = *cx.tokens();
        let look = card_look(cx);
        let dark = cx.tern_theme().is_dark();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (i, item) in menu.items.iter().enumerate() {
            let highlighted = i == menu.highlight;
            let (icon, label, desc) = match item {
                MenuItem::Command(c) => (
                    IconName::SquareTerminal,
                    format!("/{}", c.name),
                    c.description.to_string(),
                ),
                MenuItem::File(e) => (
                    if e.is_dir {
                        IconName::Folder
                    } else {
                        IconName::File
                    },
                    super::text::basename(&e.path).to_string(),
                    file_index::parent_dir(&e.path).to_string(),
                ),
            };
            rows.push(
                h_flex()
                    .id(("composer-menu-row", i))
                    .min_h(px(28.))
                    .px(px(8.))
                    .py(px(6.))
                    .gap(px(8.))
                    .rounded(radius::LG)
                    .text_scale(TextSize::Xs)
                    .when(highlighted, |el| {
                        el.bg(t.accent).text_color(t.accent_foreground)
                    })
                    .cursor_pointer()
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if let Some(m) = &mut this.menu
                            && m.highlight != i
                        {
                            m.highlight = i;
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.pick_menu_item(i, window, cx)),
                    )
                    .child(Icon::new(icon).size(px(16.)).color(t.muted_foreground))
                    .child(
                        div()
                            .flex_none()
                            .max_w(gpui::relative(0.45))
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(t.secondary_label)
                            .child(desc),
                    )
                    .into_any_element(),
            );
        }
        let empty = menu.items.is_empty().then(|| {
            let text = match (menu.trigger.kind, menu.loading) {
                (super::text::TriggerKind::Path, true) => "Searching workspace files...",
                (super::text::TriggerKind::Path, false) => "No matching files or folders.",
                (super::text::TriggerKind::Skill, _) => {
                    "No skills found. Try / to browse provider commands."
                }
                (super::text::TriggerKind::Slash, _) => "No matching command.",
            };
            div()
                .pt(px(14.))
                .px(px(20.))
                .pb(px(28.))
                .text_scale(TextSize::Xs)
                .text_color(t.secondary_label)
                .child(text)
        });
        let shadow = if dark {
            vec![BoxShadow {
                color: gpui::black().opacity(0.75),
                offset: point(px(0.), px(14.)),
                blur_radius: px(32.),
                spread_radius: px(-18.),
                inset: false,
            }]
        } else {
            cx.tern_theme().shadows.composer()
        };
        Some(
            div()
                .absolute()
                .left(px(DRAWER_INSET))
                .right(px(DRAWER_INSET))
                .bottom(px(-TUCK))
                .child(
                    v_flex()
                        .rounded_t(radius::XL2)
                        .bg(look.fill)
                        .border_1()
                        .border_color(look.outline)
                        .shadow(shadow)
                        .pb(px(TUCK))
                        .child(
                            div()
                                .id("composer-menu-list")
                                .max_h(px(288.))
                                .overflow_y_scroll()
                                .p(px(8.))
                                .child(v_flex().children(rows).children(empty)),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The pending question drawer (spec 04 §7.2).
    fn render_question(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let q = self.current_question(cx)?;
        let t = *cx.tokens();
        let look = card_look(cx);
        let count = self.question_count(cx);
        let page = self.question.page;
        let collapsed = self.question.collapsed;
        let answer = self
            .question
            .answers
            .get(&q.id)
            .cloned()
            .unwrap_or_default();
        let multi = q.multi_select.unwrap_or(false);
        let header = h_flex()
            .id("composer-question-header")
            .px(px(16.))
            .py(px(6.))
            .gap(px(0.))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.question.collapsed = !this.question.collapsed;
                cx.notify();
            }))
            .child(
                div().w(px(28.)).flex_none().child(
                    Icon::new(IconName::MessageCircleQuestionMark)
                        .size(px(14.))
                        .color(t.info),
                ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .text_scale(TextSize::Xs)
                    .text_color(t.muted_foreground)
                    .child(
                        div()
                            .flex_none()
                            .child(SharedString::from(q.header.clone())),
                    )
                    .when(collapsed, |el| {
                        el.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(t.secondary_label)
                                .child(SharedString::from(format!(" · {}", q.question))),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap(px(4.))
                    .text_scale(TextSize::Xs)
                    .text_color(t.muted_foreground)
                    .when(count > 1, |el| {
                        el.child(SharedString::from(format!("{}/{}", page + 1, count)))
                    })
                    .child(
                        Icon::new(if collapsed {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .size(px(14.)),
                    )
                    .child(
                        Button::new("composer-question-dismiss")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::IconXs)
                            .icon(IconName::X)
                            .icon_size(px(14.))
                            .tooltip(Tooltip::text("Dismiss question without answering"))
                            .on_click(cx.listener(|this, _, _, cx| this.dismiss_question(cx))),
                    ),
            );
        let options = q.options.iter().enumerate().map(|(i, o)| {
            let selected = answer.selected.contains(&i);
            let mark = if multi {
                div()
                    .size(px(16.))
                    .flex_none()
                    .rounded(radius::SM)
                    .border_1()
                    .border_color(if selected {
                        t.primary
                    } else {
                        t.foreground.opacity(0.55)
                    })
                    .when(selected, |el| {
                        el.bg(t.primary).child(
                            Icon::new(IconName::Check)
                                .size(px(12.))
                                .color(t.primary_foreground),
                        )
                    })
            } else {
                div()
                    .size(px(16.))
                    .flex_none()
                    .rounded(radius::FULL)
                    .border_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_color(if selected {
                        t.primary
                    } else {
                        t.foreground.opacity(0.55)
                    })
                    .when(selected, |el| {
                        el.bg(t.primary).child(
                            div()
                                .size(px(6.))
                                .rounded(radius::FULL)
                                .bg(t.primary_foreground),
                        )
                    })
            };
            h_flex()
                .id(("composer-question-option", i))
                .items_start()
                .gap(px(10.))
                .py(px(6.))
                .pl(px(6.))
                .pr(px(4.))
                .rounded(radius::MD)
                .cursor_pointer()
                .text_color(if selected {
                    t.foreground
                } else {
                    t.foreground.opacity(0.7)
                })
                .when(!selected, |el| el.hover(|s| s.bg(t.muted.opacity(0.3))))
                .on_click(cx.listener(move |this, _, window, cx| this.select_option(i, window, cx)))
                .child(div().pt(px(2.)).child(mark))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_scale(TextSize::Sm)
                                .font_weight(FontWeight::MEDIUM)
                                .child(SharedString::from(o.label.clone())),
                        )
                        .when_some(o.description.clone(), |el, d| {
                            el.child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(16.))
                                    .text_color(t.secondary_label)
                                    .child(SharedString::from(d)),
                            )
                        }),
                )
                .when(i < 9, |el| {
                    el.child(
                        div()
                            .w(px(24.))
                            .flex()
                            .justify_center()
                            .child(Kbd::new((i + 1).to_string())),
                    )
                })
        });
        let body = (!collapsed).then(|| {
            div()
                .id("composer-question-body")
                .max_h(px(384.))
                .overflow_y_scroll()
                .pl(px(22.))
                .pr(px(16.))
                .pb(px(4.))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(t.foreground.opacity(0.85))
                        .child(SharedString::from(q.question.clone())),
                )
                .child(v_flex().mt(px(6.)).children(options))
        });
        Some(
            div()
                .mx(px(DRAWER_INSET))
                .mb(px(-TUCK))
                .child(
                    v_flex()
                        .rounded_t(radius::XL2)
                        .bg(look.fill)
                        .border_1()
                        .border_color(look.outline)
                        .pb(px(TUCK))
                        .child(header)
                        .children(body),
                )
                .into_any_element(),
        )
    }

    /// Queued follow-ups above the composer (spec 04 §8.1).
    fn render_queue(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let detail = self.thread_detail(cx)?;
        let queued = pending::queued_runs(&detail);
        if queued.is_empty() {
            return None;
        }
        let t = *cx.tokens();
        let total = queued.len();
        let shown: Vec<pending::QueuedRun> = if total > 3 && !self.queue_expanded {
            queued.iter().take(2).cloned().collect()
        } else {
            queued.clone()
        };
        let live = pending::live_run(&detail).is_some();
        let items = shown.into_iter().enumerate().map(|(i, run)| {
            let (r1, r2, r3) = (run.clone(), run.clone(), run.run_id.clone());
            v_flex()
                .items_end()
                .gap(px(2.))
                .child(
                    div()
                        .max_w(gpui::relative(0.8))
                        .rounded(radius::XL2)
                        .border_1()
                        .border_dashed()
                        .border_color(t.muted_foreground.opacity(0.4))
                        .px(px(12.))
                        .py(px(8.))
                        .text_scale(TextSize::Sm)
                        .text_color(t.muted_foreground)
                        .whitespace_normal()
                        .child(SharedString::from(run.text.clone())),
                )
                .child(
                    h_flex()
                        .gap(px(2.))
                        .text_scale(TextSize::Xs)
                        .text_color(t.muted_foreground)
                        .child(
                            Button::new(("queued-send", i))
                                .variant(ButtonVariant::GhostMuted)
                                .size(ButtonSize::Xs)
                                .icon(IconName::ArrowUp)
                                .label("Send now")
                                .tooltip(Tooltip::text(if live {
                                    "Send into the current turn (Ctrl+Shift+Enter)"
                                } else {
                                    "Start a turn with this message"
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.steer_queued(r1.clone(), cx)
                                })),
                        )
                        .child(
                            Button::new(("queued-edit", i))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::IconXs)
                                .icon(IconName::Pencil)
                                .tooltip(Tooltip::text("Edit in the composer"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.edit_queued(r2.clone(), window, cx)
                                })),
                        )
                        .child(
                            Button::new(("queued-remove", i))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::IconXs)
                                .icon(IconName::X)
                                .tooltip(Tooltip::text("Remove from queue"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.cancel_queued(r3.clone(), cx)
                                })),
                        ),
                )
        });
        let toggle = (total > 3).then(|| {
            let expanded = self.queue_expanded;
            h_flex()
                .id("queued-toggle")
                .justify_end()
                .gap(px(4.))
                .text_scale(TextSize::Xs)
                .text_color(t.muted_foreground)
                .cursor_pointer()
                .hover(|s| s.text_color(t.foreground))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.queue_expanded = !this.queue_expanded;
                    cx.notify();
                }))
                .child(if expanded {
                    "Show fewer".to_string()
                } else {
                    format!("Show all {total} queued")
                })
                .child(
                    Icon::new(if expanded {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    })
                    .size(px(14.)),
                )
        });
        Some(
            v_flex()
                .items_end()
                .gap(px(12.))
                .pb(px(16.))
                .children(items)
                .children(toggle)
                .into_any_element(),
        )
    }

    /// The card alone (used by the draft hero and the docked lane).
    pub(crate) fn render_card(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let look = card_look(cx);
        let editor = self.render_editor(cx);
        let attachments = self.render_attachments(cx);
        let footer = self.render_footer(cx);
        let menu = self.render_menu(cx);
        let question = self.render_question(cx);
        let accent = t.accent.opacity(0.45);
        let ring = t.primary.opacity(0.7);
        let error = self.error.clone();
        let dimmed = self.key.is_none();
        v_flex()
            .w_full()
            .children(question)
            .child(div().relative().h(px(0.)).children(menu))
            .child(
                div()
                    .id("composer-card")
                    .relative()
                    .w_full()
                    .rounded(radius::XL2)
                    .bg(look.fill)
                    .border_1()
                    .border_color(look.outline)
                    .shadow(look.shadow)
                    .when(dimmed, |el| el.opacity(0.75))
                    .child(bounds_probe(self.card_bounds.clone()))
                    .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(accent).border_color(ring))
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                        this.on_drop_paths(paths, cx)
                    }))
                    .child(
                        v_flex()
                            .px(px(16.))
                            .pt(px(16.))
                            .pb(px(8.))
                            .children(attachments)
                            .child(editor),
                    )
                    .when_some(error, |el, e| {
                        el.child(
                            div()
                                .px(px(16.))
                                .pb(px(8.))
                                .text_scale(TextSize::Xs)
                                .text_color(t.destructive)
                                .child(e),
                        )
                    })
                    .child(footer),
            )
            .into_any_element()
    }
}

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{:.1} MB", b / KB / KB)
    }
}

impl Render for ComposerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pending_open.is_some() {
            self.take_pending_open(window, cx);
        }
        let hero = self.is_hero(cx);
        let queue = if hero { None } else { self.render_queue(cx) };
        let card = self.render_card(cx);
        let lane = v_flex()
            .w_full()
            .max_w(layout::CHAT_CONTENT_MAX_WIDTH)
            .children(queue)
            .child(card);
        if hero {
            return div().w_full().child(lane).into_any_element();
        }
        div()
            .w_full()
            .flex()
            .justify_center()
            .px(layout::CHAT_LANE_PADDING)
            .pt(px(8.))
            .pb(px(20.))
            .child(lane)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::format_size;

    #[test]
    fn sizes() {
        assert_eq!(format_size(12), "12 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(3 * 1024 * 1024), "3.0 MB");
    }
}

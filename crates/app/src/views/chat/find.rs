//! Find in thread (spec 03 §6.1), client-side over the loaded rows: Ctrl+F opens a
//! bar at the top right of the canvas, Enter / Shift+Enter step through matches,
//! Escape closes and the timeline scrolls the active match into view.

use gpui::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Styled as _, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use super::ChatView;
use super::rows::{Row, RowKind};
use crate::markdown;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize, radius};
use crate::ui::{Button, ButtonSize, ButtonVariant, IconName, Tooltip, TooltipSide, h_flex};

pub struct FindState {
    open: bool,
    input: Entity<InputState>,
    _sub: Option<Subscription>,
    query: String,
    /// Row index of every match, in order (a row with three hits appears three times).
    matches: Vec<usize>,
    active: usize,
}

/// Searchable text of a row (`data-thread-find-text`); code block headers and chips
/// are not part of it.
pub fn row_text(row: &Row) -> String {
    match &row.kind {
        RowKind::Message { message, .. } => message.text.clone(),
        RowKind::Work { entries, display_label, .. } => {
            let mut s = display_label.clone().unwrap_or_default();
            for e in entries {
                s.push('\n');
                s.push_str(&e.display_label(None));
            }
            s
        }
        RowKind::WorkLive { entry, .. } => entry.live_label(None, true),
        RowKind::WorkToggle { summary, .. } => summary.clone(),
        RowKind::TurnFold { label, .. } => label.clone(),
        RowKind::Compaction { label, .. } => label.clone(),
        RowKind::Plan(p) => p.markdown.clone(),
        RowKind::Event { item, .. } => match &item.body {
            tern_core::TurnItemBody::RunInterruptResult { message } => message.clone(),
            tern_core::TurnItemBody::Subagent { prompt, .. } => prompt.clone(),
            _ => String::new(),
        },
        RowKind::AssistantMeta { .. }
        | RowKind::Working { .. }
        | RowKind::Thinking { .. } => String::new(),
    }
}

/// Case-insensitive match count of `needle` in `hay`.
pub fn count_matches(hay: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let hay = hay.to_lowercase();
    let needle = needle.to_lowercase();
    hay.matches(needle.as_str()).count()
}

impl FindState {
    pub fn new(window: &mut Window, cx: &mut Context<ChatView>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in thread"));
        Self {
            open: false,
            input,
            _sub: None,
            query: String::new(),
            matches: Vec::new(),
            active: 0,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn close(&mut self) {
        self.open = false;
        self.matches.clear();
        self.active = 0;
    }

    pub fn row_is_active(&self, ix: usize) -> bool {
        self.open && self.matches.get(self.active) == Some(&ix)
    }

    pub fn refresh(&mut self, rows: &[Row], _md: &mut markdown::Cache) {
        if !self.open {
            return;
        }
        let q: String = self.query.chars().take(200).collect();
        let q = q.trim();
        self.matches.clear();
        if q.is_empty() {
            self.active = 0;
            return;
        }
        for (i, row) in rows.iter().enumerate() {
            let n = count_matches(&row_text(row), q);
            self.matches.extend(std::iter::repeat_n(i, n));
        }
        if self.active >= self.matches.len() {
            self.active = 0;
        }
    }

    fn count_label(&self) -> String {
        if self.query.trim().is_empty() || self.matches.is_empty() {
            "0/0".into()
        } else {
            format!("{}/{}", self.active + 1, self.matches.len())
        }
    }
}

impl ChatView {
    pub(crate) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.thread.is_none() {
            return;
        }
        if !self.find.open {
            self.find.open = true;
            let sub = cx.subscribe_in(
                &self.find.input,
                window,
                |this, input, event: &InputEvent, _, cx| match event {
                    InputEvent::Change => {
                        this.find.query = input.read(cx).value().to_string();
                        this.find.active = 0;
                        let rows = this.rows.clone();
                        this.find.refresh(&rows, &mut this.md);
                        this.reveal_find_match(cx);
                        cx.notify();
                    }
                    InputEvent::PressEnter { shift, .. } => {
                        this.find_step(if *shift { -1 } else { 1 }, cx)
                    }
                    _ => {}
                },
            );
            self.find._sub = Some(sub);
            let rows = self.rows.clone();
            self.find.refresh(&rows, &mut self.md);
        }
        self.find.input.update(cx, |s, cx| {
            s.select_all(window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn close_find(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.find.close();
        self.find._sub = None;
        cx.notify();
    }

    pub(crate) fn find_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.find.matches.len();
        if n == 0 {
            return;
        }
        self.find.active = (self.find.active as isize + delta).rem_euclid(n as isize) as usize;
        self.reveal_find_match(cx);
        cx.notify();
    }

    fn reveal_find_match(&mut self, _cx: &mut Context<Self>) {
        if let Some(&ix) = self.find.matches.get(self.find.active) {
            self.list.scroll_to_reveal_item(ix);
        }
    }

    pub(crate) fn render_find(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.tern_theme().clone();
        let t = theme.colors;
        let none = !self.find.query.trim().is_empty() && self.find.matches.is_empty();
        let disabled = self.find.matches.is_empty();
        div()
            .absolute()
            .top(px(12.))
            .right(px(12.))
            .w(px(280.))
            .key_context("ThreadFind")
            .child(
                h_flex()
                    .h(px(36.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .gap(px(2.))
                    .rounded(radius::LG)
                    .border_1()
                    .border_color(t.border)
                    .bg(t.popover)
                    .shadow(theme.shadows.lg(0.05))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.find.input).appearance(false).text_size(px(14.))),
                    )
                    .child(
                        div()
                            .min_w(px(40.))
                            .text_scale(TextSize::Xs)
                            .text_color(if none { t.destructive } else { t.muted_foreground })
                            .flex()
                            .justify_end()
                            .pr(px(4.))
                            .child(self.find.count_label()),
                    )
                    .child(
                        Button::new("find-prev")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::IconXs)
                            .icon(IconName::ChevronUp)
                            .disabled(disabled)
                            .tooltip(Tooltip::text("Previous match").side(TooltipSide::Bottom))
                            .on_click(cx.listener(|this, _, _, cx| this.find_step(-1, cx))),
                    )
                    .child(
                        Button::new("find-next")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::IconXs)
                            .icon(IconName::ChevronDown)
                            .disabled(disabled)
                            .tooltip(Tooltip::text("Next match").side(TooltipSide::Bottom))
                            .on_click(cx.listener(|this, _, _, cx| this.find_step(1, cx))),
                    )
                    .child(
                        Button::new("find-close")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::IconXs)
                            .icon(IconName::X)
                            .tooltip(Tooltip::text("Close find").side(TooltipSide::Bottom))
                            .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
                    ),
            )
            .when(false, |el| el)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_case_insensitively() {
        assert_eq!(count_matches("Foo foo FOO", "foo"), 3);
        assert_eq!(count_matches("abc", ""), 0);
        assert_eq!(count_matches("aaaa", "aa"), 2);
    }
}

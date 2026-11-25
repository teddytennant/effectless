//! Painting the code view's rows (spec 05 §2.6): file headers, hunk separators,
//! unified lines and split pairs. Stateless; [`super::DiffView`] owns the data and
//! hands each row the colors and metrics it needs.

use std::ops::Range;

use gpui::{
    AnyElement, App, Div, HighlightStyle, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, SharedString, Styled as _, StyledText, Window, div, pattern_slash,
    prelude::FluentBuilder as _, px,
};
use tern_git::{DiffFile, DiffLine, DiffLineKind, FileDiff};

use crate::theme::color::mix_srgb;
use crate::theme::{ActiveTheme as _, Theme, font};
use crate::tokens::c;

/// Item metrics (`StyledDiffCodeView.tsx:587-598`).
pub const HEADER_HEIGHT: Pixels = px(32.);
pub const GAP_HEIGHT: Pixels = px(24.);
pub const LINE_HEIGHT: Pixels = px(20.);
pub const FOOTER_HEIGHT: Pixels = px(8.);
pub const CODE_FONT_SIZE: Pixels = px(font::CODE_SIZE);

/// Every color the rows use, resolved once per frame.
#[derive(Clone, Copy, Debug)]
pub struct CodeColors {
    pub bg: Hsla,
    pub fg: Hsla,
    pub muted: Hsla,
    pub number: Hsla,
    pub added_row: Hsla,
    pub deleted_row: Hsla,
    pub added_hover: Hsla,
    pub deleted_hover: Hsla,
    pub hover_row: Hsla,
    /// `--diffs-addition-base` etc.: bars, numbers on changed rows, header counts.
    pub added_base: Hsla,
    pub deleted_base: Hsla,
    pub separator_bg: Hsla,
    pub separator_hover: Hsla,
    pub separator_fg: Hsla,
    pub buffer_stripe: Hsla,
    pub header_hover_bar: Hsla,
    pub modified_icon: Hsla,
    pub added_icon: Hsla,
    pub deleted_icon: Hsla,
    pub title_hover: Hsla,
}

impl CodeColors {
    /// `--code-background` is `--background` on the diff surface; the file viewer
    /// passes the code background.
    pub fn new(theme: &Theme, bg: Hsla) -> Self {
        let t = &theme.colors;
        let d = &theme.diff;
        let dark = theme.is_dark();
        let fg = t.foreground;
        // Row tints measured off the reference screenshots: addition 6% light,
        // 9% dark over the surface (T3's `mix(code-bg, --diff-addition)` after
        // Pierre's own layering).
        let tint = if dark { 0.91 } else { 0.94 };
        // Pierre's red-green bases: `created` / `deleted` of the pierre themes.
        let (added_base, deleted_base) = if dark {
            (c(0x07c480ff), c(0xff2e3fff))
        } else {
            (c(0x18a46cff), c(0xd52c36ff))
        };
        Self {
            bg,
            fg,
            muted: t.muted_foreground,
            number: mix_srgb(fg, bg, 0.65),
            added_row: mix_srgb(bg, d.diff_addition, tint),
            deleted_row: mix_srgb(bg, d.diff_deletion, tint),
            added_hover: mix_srgb(bg, d.diff_addition, 0.85),
            deleted_hover: mix_srgb(bg, d.diff_deletion, 0.85),
            hover_row: mix_srgb(bg, fg, 0.94),
            added_base,
            deleted_base,
            separator_bg: mix_srgb(bg, fg, 0.97),
            separator_hover: mix_srgb(bg, fg, 0.92),
            separator_fg: mix_srgb(fg, bg, 0.5),
            buffer_stripe: mix_srgb(bg, fg, 0.9),
            header_hover_bar: fg.opacity(0.24),
            modified_icon: d.diff_modified_icon,
            added_icon: if dark {
                d.diff_addition
            } else {
                d.diff_addition_foreground
            },
            deleted_icon: if dark {
                d.diff_deletion
            } else {
                d.diff_deletion_foreground
            },
            title_hover: mix_srgb(fg, t.primary, 0.84),
        }
    }
}

/// Per-frame layout inputs.
#[derive(Clone, Copy, Debug)]
pub struct CodeMetrics {
    /// Width of one monospace character at the code size.
    pub ch: Pixels,
    pub wrap: bool,
    /// Horizontal scroll of the code column when not wrapping.
    pub scroll_x: Pixels,
}

/// The text of a code line with its syntax spans (and nothing else).
pub fn code_text(text: &str, spans: &[(Range<usize>, HighlightStyle)]) -> StyledText {
    let text = if text.is_empty() { " " } else { text };
    let spans: Vec<(Range<usize>, HighlightStyle)> = spans
        .iter()
        .filter(|(r, _)| {
            r.end <= text.len() && text.is_char_boundary(r.start) && text.is_char_boundary(r.end)
        })
        .cloned()
        .collect();
    StyledText::new(SharedString::from(text.to_string())).with_highlights(spans)
}

fn code_cell(
    text: &str,
    spans: &[(Range<usize>, HighlightStyle)],
    metrics: CodeMetrics,
    color: Hsla,
) -> Div {
    let inner = div()
        .text_color(color)
        .child(code_text(text, spans))
        .map(|el| {
            if metrics.wrap {
                el.w_full()
            } else {
                el.whitespace_nowrap().ml(-metrics.scroll_x)
            }
        });
    div()
        .flex_1()
        .min_w_0()
        .px(metrics.ch)
        .overflow_hidden()
        .child(inner)
}

/// The number cell: right-aligned, `padding: 0 1ch 0 2ch`, with the 4px change bar
/// at its left edge.
fn gutter(
    number: Option<u32>,
    width: Pixels,
    metrics: CodeMetrics,
    color: Hsla,
    bg: Option<Hsla>,
    bar: Option<(Hsla, bool)>,
) -> Div {
    div()
        .relative()
        .flex_none()
        .w(width)
        .pr(metrics.ch)
        .flex()
        .justify_end()
        .text_color(color)
        .when_some(bg, |el, bg| el.bg(bg))
        .children(number.map(|n| n.to_string()))
        .when_some(bar, |el, (color, checker)| {
            el.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(px(4.))
                    .map(|bar| {
                        if checker {
                            // Deletions: a 2px checker of the base over the row tint.
                            bar.bg(pattern_slash(color, 2., 2.))
                        } else {
                            bar.bg(color)
                        }
                    }),
            )
        })
}

/// One unified-view line.
#[allow(clippy::too_many_arguments)]
pub fn unified_line(
    id: SharedString,
    line: &DiffLine,
    spans: &[(Range<usize>, HighlightStyle)],
    gutter_width: Pixels,
    colors: CodeColors,
    metrics: CodeMetrics,
) -> AnyElement {
    let (row_bg, hover_bg, number, num_color, bar) = match line.kind {
        DiffLineKind::Addition => (
            Some(colors.added_row),
            colors.added_hover,
            line.new_lineno,
            colors.added_base,
            Some((colors.added_base, false)),
        ),
        DiffLineKind::Deletion => (
            Some(colors.deleted_row),
            colors.deleted_hover,
            line.old_lineno,
            colors.deleted_base,
            Some((colors.deleted_base, true)),
        ),
        DiffLineKind::Context => (None, colors.hover_row, line.new_lineno, colors.number, None),
    };
    div()
        .id(id)
        .flex()
        .flex_row()
        .w_full()
        .line_height(LINE_HEIGHT)
        .when_some(row_bg, |el, bg| el.bg(bg))
        .hover(move |s| s.bg(hover_bg))
        .child(gutter(number, gutter_width, metrics, num_color, None, bar))
        .child(code_cell(&line.text, spans, metrics, colors.fg))
        .into_any_element()
}

/// One split-view row. `None` sides are the striped buffer.
#[allow(clippy::too_many_arguments)]
pub fn split_line(
    left: Option<(&DiffLine, &[(Range<usize>, HighlightStyle)])>,
    right: Option<(&DiffLine, &[(Range<usize>, HighlightStyle)])>,
    gutter_width: Pixels,
    colors: CodeColors,
    metrics: CodeMetrics,
) -> AnyElement {
    let half = |side: Option<(&DiffLine, &[(Range<usize>, HighlightStyle)])>, old: bool| {
        let Some((line, spans)) = side else {
            return div()
                .flex()
                .flex_row()
                .flex_1()
                .min_w_0()
                .min_h(LINE_HEIGHT)
                .child(div().flex_none().w(gutter_width))
                .child(
                    div()
                        .flex_1()
                        .bg(pattern_slash(colors.buffer_stripe, 1.5, 6.)),
                );
        };
        let (row_bg, number, num_color, bar) = match line.kind {
            DiffLineKind::Addition => (
                Some(colors.added_row),
                line.new_lineno,
                colors.added_base,
                Some((colors.added_base, false)),
            ),
            DiffLineKind::Deletion => (
                Some(colors.deleted_row),
                line.old_lineno,
                colors.deleted_base,
                Some((colors.deleted_base, true)),
            ),
            DiffLineKind::Context => (
                None,
                if old {
                    line.old_lineno
                } else {
                    line.new_lineno
                },
                colors.number,
                None,
            ),
        };
        div()
            .flex()
            .flex_row()
            .flex_1()
            .min_w_0()
            .when_some(row_bg, |el, bg| el.bg(bg))
            .child(gutter(number, gutter_width, metrics, num_color, None, bar))
            .child(code_cell(&line.text, spans, metrics, colors.fg))
    };
    div()
        .flex()
        .flex_row()
        .w_full()
        .line_height(LINE_HEIGHT)
        .child(half(left, true))
        .child(half(right, false))
        .into_any_element()
}

/// The 24px separator for unchanged lines (Pierre `line-info`): an expand cell and
/// the label, inset 8px, radius 6.
pub fn separator(id: SharedString, label: String, colors: CodeColors) -> AnyElement {
    div()
        .id(id)
        .h(GAP_HEIGHT)
        .px(px(8.))
        .flex()
        .flex_row()
        .gap(px(2.))
        .group("diff-gap")
        .child(
            div()
                .flex_none()
                .w(px(32.))
                .h_full()
                .rounded_l(px(6.))
                .bg(colors.separator_bg)
                .flex()
                .items_center()
                .justify_center()
                .text_color(colors.separator_fg)
                .group_hover("diff-gap", |s| {
                    s.bg(colors.separator_hover).text_color(colors.fg)
                })
                .child(
                    crate::ui::Icon::new(crate::ui::IconName::ChevronDown)
                        .size(px(14.))
                        .group_hover_color("diff-gap", colors.fg),
                ),
        )
        .child(
            div()
                .flex_1()
                .h_full()
                .rounded_r(px(6.))
                .bg(colors.separator_bg)
                .px(px(7.))
                .flex()
                .items_center()
                .font_family(font::SANS)
                .text_size(px(11.))
                .text_color(colors.separator_fg)
                .group_hover("diff-gap", |s| {
                    s.bg(colors.separator_hover).text_color(colors.fg)
                })
                .child(label),
        )
        .into_any_element()
}

/// The change glyph for a file header.
pub fn change_icon(file: &FileDiff, colors: CodeColors) -> AnyElement {
    use crate::ui::{Icon, IconName};
    let (icon, color) = match file.kind {
        DiffFile::Added => (IconName::DiffAdded, colors.added_icon),
        DiffFile::Deleted => (IconName::DiffDeleted, colors.deleted_icon),
        DiffFile::Renamed => (IconName::DiffRenamed, colors.modified_icon),
        DiffFile::Modified => (IconName::DiffModified, colors.modified_icon),
    };
    Icon::new(icon)
        .size(px(16.))
        .color(color)
        .into_any_element()
}

/// `+A -D` in two right-aligned 4ch columns (DiffStatLabel `aligned`).
pub fn header_counts(additions: u32, deletions: u32, colors: CodeColors, cx: &App) -> AnyElement {
    let _ = cx;
    let cell = |text: String, color: Hsla| {
        div()
            .min_w(px(31.))
            .flex()
            .justify_end()
            .text_color(color)
            .child(text)
    };
    div()
        .flex()
        .flex_row()
        .gap(px(8.))
        .font_family(font::MONO)
        .text_size(px(11.))
        .child(cell(
            format!("+{}", super::rows::compact_number(additions as u64)),
            colors.added_base,
        ))
        .child(cell(
            format!("-{}", super::rows::compact_number(deletions as u64)),
            colors.deleted_base,
        ))
        .into_any_element()
}

/// Measures the monospace advance at the code size.
pub fn char_width(window: &Window) -> Pixels {
    let ts = window.text_system();
    let id = ts.resolve_font(&gpui::font(font::MONO));
    ts.ch_advance(id, CODE_FONT_SIZE).unwrap_or(px(7.8))
}

/// Used by callers that only have `&App`.
pub fn colors_for(cx: &App, bg: Hsla) -> CodeColors {
    CodeColors::new(cx.tern_theme(), bg)
}

//! One row of a Pierre-style file tree (`@pierre/trees`, compact density, T3's
//! overrides in `pierre-tree-theme.ts`): 24px rows, 12px sans, radius 6, hover
//! `currentColor/7%`, selected `currentColor/12%`, a type icon, and the git status
//! letter on the right for diff trees.

use gpui::{
    App, Div, InteractiveElement as _, ParentElement as _, SharedString, Stateful, Styled as _,
    div, prelude::FluentBuilder as _, px,
};
use tern_git::DiffFile;

use super::file_tree::TreeRow;
use super::icons::file_icon;
use crate::theme::ActiveTheme as _;
use crate::ui::{Icon, IconName, h_flex};

/// Compact density: 30px * 0.8.
pub const ROW_HEIGHT: f32 = 24.;
/// `--trees-item-padding-x` * 0.8.
const PAD_X: f32 = 6.4;
/// `--trees-item-row-gap` * 0.8.
const GAP: f32 = 4.8;
/// Indent per level: the icon width plus the row gap.
const INDENT: f32 = 16. + GAP;

pub fn row(id: SharedString, row: &TreeRow, selected: bool, dark: bool, cx: &App) -> Stateful<Div> {
    let t = cx.tokens();
    let theme = cx.tern_theme();
    let fg = t.foreground;
    let icon = if row.is_dir {
        Icon::new(if row.expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        })
        .size(px(16.))
        .color(t.muted_foreground)
    } else {
        let (icon, color) = file_icon(&row.path, dark);
        Icon::new(icon).size(px(16.)).color(color)
    };
    let status = row.status.map(|s| {
        let (letter, color) = match s {
            DiffFile::Added => ("A", theme.diff.diff_addition_foreground),
            DiffFile::Deleted => ("D", theme.diff.diff_deletion_foreground),
            DiffFile::Renamed => ("R", theme.diff.diff_modified_icon),
            DiffFile::Modified => ("M", theme.diff.diff_modified_icon),
        };
        div()
            .flex_none()
            .font_weight(crate::theme::font::SEMIBOLD)
            .text_color(color)
            .child(letter)
    });
    h_flex()
        .id(id)
        .h(px(ROW_HEIGHT))
        .flex_none()
        .pl(px(PAD_X + INDENT * row.depth as f32))
        .pr(px(PAD_X))
        .gap(px(GAP))
        .rounded(px(6.))
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(fg)
        .cursor_pointer()
        .when(selected, |el| el.bg(fg.opacity(0.12)))
        .when(!selected, |el| el.hover(move |s| s.bg(fg.opacity(0.07))))
        .child(icon)
        .child(div().flex_1().min_w_0().truncate().child(row.label.clone()))
        .children(status)
}

//! Diffs & pull requests (spec 06 §4). Device-only page.

use gpui::{AnyElement, Context, IntoElement, ParentElement as _, Window};
use tern_core::settings::{DiffColorScheme, DiffLayout};

use super::{client_select, client_switch};
use crate::views::settings::SettingsView;
use crate::views::settings::widgets::Section;

const LAYOUT: &[(DiffLayout, &str)] = &[
    (DiffLayout::Stacked, "Stacked"),
    (DiffLayout::Split, "Split"),
];

/// `diffFilesCollapsed`: Expanded -> false, Collapsed -> true.
const FILE_STATE: &[(bool, &str)] = &[(false, "Expanded"), (true, "Collapsed")];

const COLORS: &[(DiffColorScheme, &str)] = &[
    (DiffColorScheme::RedGreen, "Red & green (default)"),
    (DiffColorScheme::BlueOrange, "Blue & orange"),
];

pub fn render(
    view: &mut SettingsView,
    _window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Vec<AnyElement> {
    vec![
        Section::new("diffs", "Diffs")
            .child(client_select(
                view,
                "diff-layout",
                "Diff layout",
                "Show diffs stacked or side by side. The toggle in the diff toolbar changes this too.",
                LAYOUT,
                160.,
                |c| c.diff_layout,
                |c, v| c.diff_layout = v,
                cx,
            ))
            .child(client_switch(
                view,
                "diff-whitespace",
                "Hide whitespace changes",
                "Set whether the diff panel ignores whitespace-only edits by default.",
                |c| c.diff_ignore_whitespace,
                |c, v| c.diff_ignore_whitespace = v,
                cx,
            ))
            .child(client_select(
                view,
                "diff-file-state",
                "Default diff file state",
                "Start with files expanded or collapsed when opening diffs or a pull request's Code tab.",
                FILE_STATE,
                160.,
                |c| c.diff_files_collapsed,
                |c, v| c.diff_files_collapsed = v,
                cx,
            ))
            .child(client_select(
                view,
                "diff-colors",
                "Diff colors",
                "Choose colors for additions and deletions, including change counts.",
                COLORS,
                160.,
                |c| c.diff_color_scheme,
                |c, v| c.diff_color_scheme = v,
                cx,
            ))
            .into_any_element(),
        Section::new("pull-requests", "Pull requests")
            .child(client_switch(
                view,
                "proactive-panels",
                "Open pull requests and changes panels proactively",
                "Open linked pull requests first. Otherwise, open Changes for edits to at least 3 files or 50 lines.",
                |c| c.proactive_panels_enabled,
                |c, v| c.proactive_panels_enabled = v,
                cx,
            ))
            .child(client_switch(
                view,
                "pr-links",
                "Open pull requests in Tern",
                "Clicking a pull request link opens it in Tern. Turn off to open pull request links like any other link, on your git host.",
                |c| c.pull_request_links_open_in_app,
                |c, v| c.pull_request_links_open_in_app = v,
                cx,
            ))
            .into_any_element(),
    ]
}

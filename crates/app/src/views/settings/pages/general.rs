//! General (spec 06 §2). Device-only page: client settings, no scope sentence.

use gpui::{AnyElement, Context, IntoElement, ParentElement as _, Window};
use tern_core::settings::{
    NotificationMode, QuitConfirmation, SidebarProjectGroupingMode, SortOrder, TimestampFormat,
};

use super::{client_select, client_switch, write_client};
use crate::ui::Switch;
use crate::views::settings::SettingsView;
use crate::views::settings::widgets::{Row, Section};

const NOTIFICATIONS: &[(NotificationMode, &str)] = &[
    (NotificationMode::Off, "Off"),
    (NotificationMode::Notifications, "Notifications only"),
    (NotificationMode::Sound, "Sound only"),
    (
        NotificationMode::NotificationsAndSound,
        "Notifications with sound",
    ),
];

const PROJECT_ORDER: &[(SortOrder, &str)] = &[
    (SortOrder::UpdatedAt, "Last user message"),
    (SortOrder::CreatedAt, "Created at"),
    (SortOrder::Manual, "Manual"),
];

const QUIT: &[(QuitConfirmation, &str)] = &[
    (QuitConfirmation::Direct, "Direct"),
    (QuitConfirmation::Hold, "Hold"),
    (QuitConfirmation::DoubleClick, "Double press"),
];

const TIME_FORMAT: &[(TimestampFormat, &str)] = &[
    (TimestampFormat::Locale, "System default"),
    (TimestampFormat::TwelveHour, "12-hour"),
    (TimestampFormat::TwentyFourHour, "24-hour"),
];

pub fn render(
    view: &mut SettingsView,
    _window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Vec<AnyElement> {
    let model = view.model().clone();
    let grouping = model.read(cx).client().sidebar_project_grouping_mode;
    let grouping_on = grouping != SidebarProjectGroupingMode::Separate;
    let m = model.clone();
    let m2 = model.clone();
    let project_grouping = Row::new("project-grouping", "Project grouping")
        .description("Combine matching repositories across environments.", cx)
        .highlight(view.is_pulsing("project-grouping"))
        .probe(view.probe_for("project-grouping"))
        .reset_when(
            grouping != SidebarProjectGroupingMode::Repository,
            move |window, cx| {
                write_client(
                    &m2,
                    |c| c.sidebar_project_grouping_mode = SidebarProjectGroupingMode::Repository,
                    window,
                    cx,
                )
            },
        )
        .control(
            Switch::new("project-grouping-switch", grouping_on).on_change(move |on, window, cx| {
                write_client(
                    &m,
                    |c| {
                        c.sidebar_project_grouping_mode = if on {
                            SidebarProjectGroupingMode::Repository
                        } else {
                            SidebarProjectGroupingMode::Separate
                        }
                    },
                    window,
                    cx,
                )
            }),
        );

    vec![
        Section::new("notifications", "Notifications")
            .child(client_select(
                view,
                "thread-notifications",
                "Thread notifications",
                "System alerts when a thread finishes, fails, or needs input or approval. Applies to this device while Tern is open.",
                NOTIFICATIONS,
                224.,
                |c| c.notification_mode,
                |c, v| c.notification_mode = v,
                cx,
            ))
            .child(client_switch(
                view,
                "in-app-notifications",
                "In-app notifications",
                "Show a toast when another thread finishes, fails, or needs input or approval while this app has focus.",
                |c| c.in_app_notifications_enabled,
                |c, v| c.in_app_notifications_enabled = v,
                cx,
            ))
            .into_any_element(),
        Section::new("sidebar", "Sidebar")
            .child(project_grouping)
            .child(client_select(
                view,
                "project-order",
                "Project order",
                "Order of projects in the sidebar project picker and command palette.",
                PROJECT_ORDER,
                176.,
                |c| c.sidebar_project_sort_order,
                |c, v| c.sidebar_project_sort_order = v,
                cx,
            ))
            .child(client_switch(
                view,
                "working-section",
                "Working section",
                "Fold working and monitoring threads into a Working section. They return to the top of the inbox when they need you.",
                |c| c.sidebar_working_shelf_enabled,
                |c, v| c.sidebar_working_shelf_enabled = v,
                cx,
            ))
            .into_any_element(),
        Section::new("confirmations", "Confirmations")
            .child(client_switch(
                view,
                "unpin-confirmation",
                "Unpin confirmation",
                "Ask before unpinning a thread from the pinned section.",
                |c| c.confirm_thread_unpin,
                |c, v| c.confirm_thread_unpin = v,
                cx,
            ))
            .child(client_switch(
                view,
                "archive-confirmation",
                "Archive confirmation",
                "Require a second click on the inline archive action before a thread is archived.",
                |c| c.confirm_thread_archive,
                |c, v| c.confirm_thread_archive = v,
                cx,
            ))
            .child(client_switch(
                view,
                "delete-confirmation",
                "Delete confirmation",
                "Ask before deleting a thread and its chat history.",
                |c| c.confirm_thread_delete,
                |c, v| c.confirm_thread_delete = v,
                cx,
            ))
            .child(client_switch(
                view,
                "terminal-close-confirmation",
                "Terminal close confirmation",
                "Ask before closing terminals and stopping their running processes.",
                |c| c.confirm_terminal_close,
                |c, v| c.confirm_terminal_close = v,
                cx,
            ))
            .child(client_select(
                view,
                "quit-shortcut",
                "Quit shortcut",
                "Hold mode also quits on two quick presses.",
                QUIT,
                160.,
                |c| c.confirm_quit,
                |c, v| c.confirm_quit = v,
                cx,
            ))
            .into_any_element(),
        Section::new("display", "Display")
            .child(client_select(
                view,
                "time-format",
                "Time format",
                "System default follows your browser or OS clock preference.",
                TIME_FORMAT,
                160.,
                |c| c.timestamp_format,
                |c, v| c.timestamp_format = v,
                cx,
            ))
            .into_any_element(),
    ]
}

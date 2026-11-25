//! Chat (spec 06 §3). Device-only page. `{mod}` is Ctrl off macOS.

use gpui::{AnyElement, Context, IntoElement, ParentElement as _, Window};
use tern_core::settings::{
    FollowUpBehavior, ScreenReaderAnnouncements, SendShortcut, SideThreadOpenTarget,
    ToolCallVisibility,
};

use super::{client_select, client_switch, fold};
use crate::views::settings::SettingsView;
use crate::views::settings::widgets::Section;

const MOD: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl"
};

const SEND: &[(SendShortcut, &str)] = &[
    (SendShortcut::Enter, "Enter"),
    (
        SendShortcut::ModEnterMultiline,
        if cfg!(target_os = "macos") {
            "⌘ + Enter for multiline prompts"
        } else {
            "Ctrl + Enter for multiline prompts"
        },
    ),
    (
        SendShortcut::ModEnter,
        if cfg!(target_os = "macos") {
            "⌘ + Enter always"
        } else {
            "Ctrl + Enter always"
        },
    ),
];

const FOLLOW_UP: &[(FollowUpBehavior, &str)] = &[
    (FollowUpBehavior::Queue, "Queue"),
    (FollowUpBehavior::Steer, "Steer"),
];

const SIDE: &[(SideThreadOpenTarget, &str)] = &[
    (SideThreadOpenTarget::Panel, "Side panel"),
    (SideThreadOpenTarget::Main, "Main view"),
];

const TOOLS: &[(ToolCallVisibility, &str)] = &[
    (ToolCallVisibility::ShowAll, "Show all"),
    (ToolCallVisibility::HideSuccessful, "Hide successful"),
    (ToolCallVisibility::HideAll, "Hide all"),
    (ToolCallVisibility::Zen, "Zen"),
];

const SCREEN_READER: &[(ScreenReaderAnnouncements, &str)] = &[
    (ScreenReaderAnnouncements::Status, "Status only"),
    (ScreenReaderAnnouncements::Messages, "Status and messages"),
    (ScreenReaderAnnouncements::Everything, "Everything"),
];

pub fn follow_up_description(send: SendShortcut) -> String {
    match send {
        SendShortcut::Enter => format!(
            "Queue follow-ups while the agent runs or steer the current run. Press {MOD} + Enter to do the opposite for one message."
        ),
        SendShortcut::ModEnterMultiline => format!(
            "Queue follow-ups while the agent runs or steer the current run. Press {MOD} + Enter for single-line prompts or {MOD} + Shift + Enter for multiline prompts to do the opposite for one message."
        ),
        SendShortcut::ModEnter => format!(
            "Queue follow-ups while the agent runs or steer the current run. Press {MOD} + Shift + Enter to do the opposite for one message."
        ),
    }
}

pub fn render(
    view: &mut SettingsView,
    _window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Vec<AnyElement> {
    let c = view.model().read(cx).client().clone();
    let side_desc = match c.side_thread_open_target {
        SideThreadOpenTarget::Panel => format!(
            "Where a click opens a subagent or a side chat. {MOD}-click or middle-click opens it in the main view instead."
        ),
        SideThreadOpenTarget::Main => format!(
            "Where a click opens a subagent or a side chat. {MOD}-click or middle-click opens it in the side panel instead."
        ),
    };
    let mut composer: Vec<AnyElement> = vec![
        client_switch(
            view,
            "rich-text-composer",
            "Rich text composer",
            "Show formatted Markdown as you type.",
            |c| c.composer_rich_text_enabled,
            |c, v| c.composer_rich_text_enabled = v,
            cx,
        )
        .into_any_element(),
        client_select(
            view,
            "send-shortcut",
            "Send shortcut",
            "Choose when Enter sends a prompt or inserts a new line",
            SEND,
            160.,
            |c| c.send_shortcut,
            |c, v| c.send_shortcut = v,
            cx,
        )
        .into_any_element(),
    ];
    let skills = client_switch(
        view,
        "show-skills",
        "Show skills in slash menu",
        "Also include skills in the / command menu. Skills always appear when you type $.",
        |c| c.show_skills_in_slash_menu,
        |c, v| c.show_skills_in_slash_menu = v,
        cx,
    )
    .into_any_element();
    composer.extend(fold(view, "chat-composer-advanced", || vec![skills], cx));

    vec![
        super::section_of("composer", "Composer", composer, cx),
        Section::new("follow-ups", "Follow-ups")
            .child(client_select(
                view,
                "follow-up-behavior",
                "Follow-up behavior",
                follow_up_description(c.send_shortcut),
                FOLLOW_UP,
                160.,
                |c| c.follow_up_behavior,
                |c, v| c.follow_up_behavior = v,
                cx,
            ))
            .into_any_element(),
        Section::new("subagents", "Subagents and side chats")
            .child(client_select(
                view,
                "side-thread-target",
                "Open subagents and side chats in",
                side_desc,
                SIDE,
                160.,
                |c| c.side_thread_open_target,
                |c, v| c.side_thread_open_target = v,
                cx,
            ))
            .into_any_element(),
        Section::new("activity", "Activity")
            .child(client_select(
                view,
                "tool-calls",
                "Tool calls in chat",
                "Hide the commands, edits, and searches agents run. Hide successful keeps failures. Zen shows only final answers. Approvals, questions, and errors always show.",
                TOOLS,
                160.,
                |c| c.tool_call_visibility,
                |c, v| c.tool_call_visibility = v,
                cx,
            ))
            .into_any_element(),
        Section::new("screen-reader-section", "Screen reader")
            .child(client_select(
                view,
                "screen-reader",
                "Screen reader announcements",
                "What a screen reader hears from the open thread: a turn starting, needing you, and ending. Status and messages adds what the agent writes before its last message. Everything adds each step.",
                SCREEN_READER,
                192.,
                |c| c.screen_reader_announcements,
                |c, v| c.screen_reader_announcements = v,
                cx,
            ))
            .child(client_switch(
                view,
                "screen-reader-title",
                "Screen reader window title",
                "Start the window title with the thread's status, like Needs approval, and its name, so screen readers read them first when you switch apps. Taskbars, shared screens, and time trackers see it too.",
                |c| c.screen_reader_window_title,
                |c, v| c.screen_reader_window_title = v,
                cx,
            ))
            .into_any_element(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_up_text_tracks_send_shortcut() {
        assert!(follow_up_description(SendShortcut::Enter).contains("+ Enter to do the opposite"));
        assert!(follow_up_description(SendShortcut::ModEnter).contains("+ Shift + Enter to do"));
        assert!(
            follow_up_description(SendShortcut::ModEnterMultiline).contains("single-line prompts")
        );
    }
}

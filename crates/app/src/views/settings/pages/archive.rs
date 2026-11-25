//! Archive (spec 06 §8): archived threads grouped by project, newest first, with
//! Unarchive and Delete.

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Styled as _, Window, div, px,
};
use tern_core::{Command, ThreadShell, Ts};

use super::scope_sentence;
use crate::store::Store;
use crate::theme::{ActiveTheme as _, radius};
use crate::ui::menu::ContextMenuExt as _;
use crate::ui::{
    Button, ButtonSize, ButtonVariant, ConfirmOptions, Dialogs, Icon, IconName, Menu, MenuItem,
    Toast, Toasts, Tooltip,
};
use crate::views::settings::SettingsView;
use crate::views::settings::widgets::{Row, Section, description};

/// `4m ago`, `3h ago`, `2d ago`, `just now`.
pub fn relative(ts: Ts, now: Ts) -> String {
    let secs = (now - ts).num_seconds().max(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        86_400..=2_591_999 => format!("{}d ago", secs / 86_400),
        2_592_000..=31_535_999 => format!("{}mo ago", secs / 2_592_000),
        _ => format!("{}y ago", secs / 31_536_000),
    }
}

/// Archived threads of `project` (all projects when None), newest archive first.
pub fn archived_threads<'a>(
    threads: &'a [ThreadShell],
    project: Option<&str>,
) -> Vec<&'a ThreadShell> {
    let mut out: Vec<&ThreadShell> = threads
        .iter()
        .filter(|t| t.thread.archived_at.is_some() && t.thread.deleted_at.is_none())
        .filter(|t| project.is_none_or(|p| t.thread.project_id == p))
        .collect();
    out.sort_by(|a, b| {
        let ka = a.thread.archived_at.unwrap_or(a.thread.created_at);
        let kb = b.thread.archived_at.unwrap_or(b.thread.created_at);
        kb.cmp(&ka)
    });
    out
}

/// Initials for a project monogram ("demo-notes" -> "DN").
pub fn monogram(title: &str) -> String {
    let words: Vec<&str> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    match words.as_slice() {
        [] => "?".into(),
        [one] => one.chars().take(2).collect::<String>().to_uppercase(),
        [a, b, ..] => format!(
            "{}{}",
            a.chars().next().unwrap_or('?'),
            b.chars().next().unwrap_or('?')
        )
        .to_uppercase(),
    }
}

pub fn project_icon(title: &str, cx: &App) -> AnyElement {
    let t = cx.tokens();
    div()
        .size(px(16.))
        .flex_none()
        .rounded(radius::SM)
        .bg(t.success.opacity(0.15))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(7.))
        .line_height(px(8.))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(t.success)
        .child(monogram(title))
        .into_any_element()
}

fn delete(thread_id: String, title: String, cx: &mut App, window: &mut Window) {
    let store = Store::global(cx);
    let confirm = crate::views::settings::SettingsModel::try_global(cx)
        .is_none_or(|m| m.read(cx).client().confirm_thread_delete);
    if !confirm {
        store.read(cx).send(Command::DeleteThread { thread_id });
        return;
    }
    let rx = Dialogs::confirm(
        format!(
            "Delete thread \"{title}\"?\nThis permanently clears conversation history for this thread."
        ),
        ConfirmOptions::destructive(),
        window,
        cx,
    );
    window
        .spawn(cx, async move |cx| {
            if rx.await.unwrap_or(false) {
                let _ = cx.update(|_, cx| {
                    Store::global(cx)
                        .read(cx)
                        .send(Command::DeleteThread { thread_id })
                });
            }
        })
        .detach();
}

fn unarchive(thread_id: String, cx: &mut App) {
    let store = Store::global(cx);
    if store.read(cx).is_fixture() {
        Toasts::push(Toast::error("Failed to unarchive thread"), cx);
        return;
    }
    store.read(cx).send(Command::SetArchived {
        thread_id,
        archived: false,
    });
}

pub fn render(
    view: &mut SettingsView,
    _window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Vec<AnyElement> {
    let t = *cx.tokens();
    let store = Store::global(cx);
    let snapshot = store.read(cx).snapshot().clone();
    let now = chrono::Utc::now();
    let threads = archived_threads(&snapshot.threads, view.scope_project.as_deref());
    let mut out = vec![scope_sentence(view, cx)];
    if threads.is_empty() {
        out.push(
            Section::new("archived-threads", "Archived threads")
                .child(
                    Row::new("archived-empty", "No archived threads")
                        .leading(
                            Icon::new(IconName::Archive)
                                .size(px(14.))
                                .color(t.muted_foreground),
                        )
                        .description("Archived threads will appear here.", cx)
                        .highlight(view.is_pulsing("archived-threads"))
                        .probe(view.probe_for("archived-threads")),
                )
                .into_any_element(),
        );
        return out;
    }
    // Group by project, in sidebar project order.
    for project in &snapshot.projects {
        let rows: Vec<&&ThreadShell> = threads
            .iter()
            .filter(|s| s.thread.project_id == project.id)
            .collect();
        if rows.is_empty() {
            continue;
        }
        let mut section = Section::new(
            SharedString::from(format!("archive-{}", project.id)),
            project.title.clone(),
        )
        .icon(project_icon(&project.title, cx));
        for shell in rows {
            let th = &shell.thread;
            let id = th.id.clone();
            let title = th.title.clone();
            let desc = format!(
                "Archived {} · Created {}",
                relative(th.archived_at.unwrap_or(th.created_at), now),
                relative(th.created_at, now)
            );
            let (i1, i2, t1, i3, t3) = (
                id.clone(),
                id.clone(),
                title.clone(),
                id.clone(),
                title.clone(),
            );
            let row = Row::new(SharedString::from(format!("archived-{id}")), title.clone())
                .description_el(description(desc, cx))
                .control(
                    Button::new(SharedString::from(format!("unarchive-{id}")))
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::IconSm)
                        .icon(IconName::ArchiveX)
                        .icon_size(px(14.))
                        .tooltip(Tooltip::text("Unarchive"))
                        .on_click(move |_, _, cx| unarchive(i1.clone(), cx)),
                )
                .control(
                    Button::new(SharedString::from(format!("delete-{id}")))
                        .variant(ButtonVariant::DestructiveOutline)
                        .size(ButtonSize::IconSm)
                        .icon(IconName::Trash2)
                        .icon_size(px(14.))
                        .tooltip(Tooltip::text("Delete"))
                        .on_click(move |_, window, cx| delete(i2.clone(), t1.clone(), cx, window)),
                );
            section = section.child(
                div()
                    .id(SharedString::from(format!("archived-ctx-{id}")))
                    .t3_context_menu(
                        SharedString::from(format!("archived-menu-{id}")),
                        move |_, _| {
                            let (a, b, bt) = (i3.clone(), i3.clone(), t3.clone());
                            Menu::new()
                                .item(
                                    MenuItem::new("Unarchive")
                                        .on_select(move |_, cx| unarchive(a.clone(), cx)),
                                )
                                .item(MenuItem::new("Delete").destructive().on_select(
                                    move |window, cx| delete(b.clone(), bt.clone(), cx, window),
                                ))
                        },
                    )
                    .child(row),
            );
        }
        out.push(section.into_any_element());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn relative_times() {
        let now = Utc.with_ymd_and_hms(2026, 11, 25, 12, 0, 0).unwrap();
        assert_eq!(relative(now - Duration::seconds(10), now), "just now");
        assert_eq!(relative(now - Duration::minutes(4), now), "4m ago");
        assert_eq!(relative(now - Duration::hours(5), now), "5h ago");
        assert_eq!(relative(now - Duration::days(3), now), "3d ago");
    }

    #[test]
    fn monograms() {
        assert_eq!(monogram("demo-notes"), "DN");
        assert_eq!(monogram("tern"), "TE");
        assert_eq!(monogram(""), "?");
    }
}

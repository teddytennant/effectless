//! A developer gallery of every primitive, for screenshots (`--route gallery`).

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::InputState;

use crate::theme::{ActiveTheme as _, TextExt as _, TextSize};
use crate::ui::{
    Badge, BadgeSize, BadgeVariant, Button, ButtonSize, ButtonVariant, CheckState, Checkbox,
    ConfirmOptions, Dialog, DialogFooter, Dialogs, DropdownMenu, Icon, IconName, InputSize, Kbd,
    KbdGroup, Label, Menu, MenuItem, Popover, PopoverWidth, Select, SelectOption, Separator,
    Skeleton, Spinner, Switch, TextField, TextareaField, Toast, Toasts, Toggle, ToggleGroup,
    ToggleVariant, Tooltip, TooltipExt as _, h_flex, v_flex,
};

pub struct GalleryView {
    input: Entity<InputState>,
    textarea: Entity<crate::ui::input::TextareaState>,
    switch_on: bool,
    checked: bool,
    segment: usize,
    select: SharedString,
}

impl GalleryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Screenshot hook: TERN_GALLERY_OPEN=menu,confirm,toast,dialog opens overlays
        // shortly after startup so headless shots can show them.
        if let Ok(open) = std::env::var("TERN_GALLERY_OPEN") {
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let _ = cx.update(|window, cx| open_overlays(&open, window, cx));
            })
            .detach();
        }
        Self {
            input: cx.new(|cx| InputState::new(window, cx).placeholder("Search projects…")),
            textarea: cx.new(|cx| crate::ui::input::TextareaState::new(window, cx)),
            switch_on: true,
            checked: true,
            segment: 0,
            select: "unified".into(),
        }
    }
}

fn section(title: &'static str, cx: &App) -> gpui::Div {
    v_flex().gap(px(10.)).child(
        div()
            .text_scale(TextSize::Xs)
            .font_medium()
            .text_color(cx.tokens().muted_foreground)
            .child(title),
    )
}

fn open_overlays(which: &str, window: &mut Window, cx: &mut App) {
    for part in which.split(',') {
        match part {
            "menu" => crate::ui::menu::open_menu(
                "gallery-menu".into(),
                crate::ui::popup::PopupAnchor::Point {
                    position: gpui::point(px(520.), px(640.)),
                    trigger: None,
                },
                demo_menu(window, cx),
                window,
                cx,
            ),
            "dialog" => {
                Dialogs::open(window, cx, |_, _| {
                    Dialog::new()
                        .title("Add project")
                        .description("Choose a folder to add as a project.")
                        .child(div().child("Panel content"))
                        .footer(
                            DialogFooter::new()
                                .child(
                                    Button::new("d-cancel")
                                        .variant(ButtonVariant::Outline)
                                        .label("Cancel"),
                                )
                                .child(Button::new("d-ok").label("Add")),
                        )
                });
            }
            "confirm" => {
                drop(Dialogs::confirm(
                    "Delete thread \"Fix flaky CI\"?\nThis permanently clears conversation history for this thread.",
                    ConfirmOptions::destructive(),
                    window,
                    cx,
                ));
            }
            "toast" => {
                Toasts::push(
                    Toast::success("Project created")
                        .description("tern")
                        .action("Open", |_, _| {})
                        .sticky(),
                    cx,
                );
                Toasts::push(
                    Toast::error("Failed to rename thread")
                        .description("Thread title cannot be empty")
                        .sticky(),
                    cx,
                );
            }
            _ => {}
        }
    }
}

fn demo_menu(_: &mut Window, _: &mut App) -> Menu {
    Menu::new()
        .item(
            MenuItem::new("Rename")
                .icon(IconName::Pencil)
                .shortcut("F2")
                .on_select(|_, _| {}),
        )
        .item(
            MenuItem::new("Pin")
                .icon(IconName::Pin)
                .shortcut("Ctrl+Shift+P")
                .on_select(|_, _| {}),
        )
        .item(MenuItem::submenu(
            "Move to",
            Some(IconName::FolderInput),
            Menu::new()
                .item(MenuItem::new("tern").on_select(|_, _| {}))
                .item(MenuItem::new("site").on_select(|_, _| {})),
        ))
        .separator()
        .label("View")
        .item(MenuItem::checkbox("Show diff stats", true).on_select(|_, _| {}))
        .item(MenuItem::switch("Compact rows", false).on_select(|_, _| {}))
        .separator()
        .item(
            MenuItem::new("Delete")
                .icon(IconName::Trash2)
                .destructive()
                .on_select(|_, _| {}),
        )
}

impl Render for GalleryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        let variants = [
            ("default", ButtonVariant::Default),
            ("destructive", ButtonVariant::Destructive),
            ("outline", ButtonVariant::Outline),
            ("destructive-outline", ButtonVariant::DestructiveOutline),
            ("secondary", ButtonVariant::Secondary),
            ("ghost", ButtonVariant::Ghost),
            ("ghost-muted", ButtonVariant::GhostMuted),
            ("glass", ButtonVariant::Glass),
            ("link", ButtonVariant::Link),
            ("warning-outline", ButtonVariant::WarningOutline),
        ];
        let sizes = [
            ("xs", ButtonSize::Xs),
            ("sm", ButtonSize::Sm),
            ("default", ButtonSize::Default),
            ("lg", ButtonSize::Lg),
            ("xl", ButtonSize::Xl),
            ("compact", ButtonSize::Compact),
            ("micro", ButtonSize::Micro),
            ("sm-pill", ButtonSize::SmPill),
        ];
        let icon_sizes = [
            ButtonSize::IconTiny,
            ButtonSize::IconMicro,
            ButtonSize::IconXs,
            ButtonSize::IconSm,
            ButtonSize::Icon,
            ButtonSize::IconLg,
            ButtonSize::IconXl,
            ButtonSize::IconSmPill,
        ];
        let entity = cx.entity();
        let segments = ["Branch", "Uncommitted", "Last turn"];
        let content: Vec<AnyElement> = vec![
            section("Button variants", cx)
                .child(h_flex().flex_wrap().gap(px(8.)).children(variants.iter().map(|(name, v)| {
                    Button::new(SharedString::from(format!("v-{name}")))
                        .variant(*v)
                        .icon(IconName::Plus)
                        .label(*name)
                })))
                .child(h_flex().gap(px(8.)).children(variants.iter().take(4).map(|(name, v)| {
                    Button::new(SharedString::from(format!("vd-{name}")))
                        .variant(*v)
                        .label("Disabled")
                        .disabled(true)
                })))
                .into_any_element(),
            section("Button sizes", cx)
                .child(h_flex().gap(px(8.)).children(sizes.iter().map(|(name, s)| {
                    Button::new(SharedString::from(format!("s-{name}")))
                        .variant(ButtonVariant::Outline)
                        .size(*s)
                        .icon(IconName::GitBranch)
                        .label(*name)
                })))
                .child(h_flex().gap(px(8.)).children(icon_sizes.iter().enumerate().map(|(i, s)| {
                    Button::new(("icon-size", i))
                        .variant(ButtonVariant::Ghost)
                        .size(*s)
                        .icon(IconName::Settings)
                        .tooltip(Tooltip::text(format!("{s:?}")))
                }))
                .child(Button::new("loading").label("Saving").loading(true)))
                .into_any_element(),
            section("Badge, Kbd, Label", cx)
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(Badge::new("Default"))
                        .child(Badge::new("Secondary").variant(BadgeVariant::Secondary))
                        .child(Badge::new("Outline").variant(BadgeVariant::Outline))
                        .child(Badge::new("Error").variant(BadgeVariant::Error).icon(IconName::CircleAlert))
                        .child(Badge::new("Info").variant(BadgeVariant::Info))
                        .child(Badge::new("Success").variant(BadgeVariant::Success))
                        .child(Badge::new("Warning").variant(BadgeVariant::Warning))
                        .child(Badge::new("Destructive").variant(BadgeVariant::Destructive))
                        .child(Badge::new("sm").size(BadgeSize::Sm))
                        .child(Badge::new("control").size(BadgeSize::Control).variant(BadgeVariant::Outline))
                        .child(KbdGroup::new().child(Kbd::new("Ctrl")).child(Kbd::new("K")))
                        .child(Kbd::new("Esc").outline())
                        .child(Label::new("Label")),
                )
                .into_any_element(),
            section("Switch, Checkbox, Toggle", cx)
                .child(
                    h_flex()
                        .gap(px(16.))
                        .child(Switch::new("sw", self.switch_on).on_change({
                            let entity = entity.clone();
                            move |v, _, cx| entity.update(cx, |g, cx| {
                                g.switch_on = v;
                                cx.notify();
                            })
                        }))
                        .child(Switch::new("sw-off", false))
                        .child(Switch::new("sw-sm", true).size(crate::ui::SwitchSize::Sm))
                        .child(Checkbox::new("cb", self.checked).on_change({
                            let entity = entity.clone();
                            move |v, _, cx| entity.update(cx, |g, cx| {
                                g.checked = v;
                                cx.notify();
                            })
                        }))
                        .child(Checkbox::new("cb-off", false))
                        .child(Checkbox::new("cb-ind", false).state(CheckState::Indeterminate))
                        .child(Toggle::new("tg").icon(IconName::Bold).pressed(true))
                        .child(Toggle::new("tg2").icon(IconName::Italic))
                        .child(Toggle::new("tg3").variant(ToggleVariant::Outline).label("Outline"))
                        .child(ToggleGroup::new().children_from(segments.iter().enumerate().map(|(i, s)| {
                            let entity = entity.clone();
                            Toggle::new(("seg", i)).label(*s).pressed(self.segment == i).on_click(
                                move |_, _, cx| entity.update(cx, |g, cx| {
                                    g.segment = i;
                                    cx.notify();
                                }),
                            )
                        }))),
                )
                .into_any_element(),
            section("Select, Menu, Popover, Tooltip", cx)
                .child(
                    h_flex()
                        .gap(px(12.))
                        .child(
                            div().w(px(200.)).child(
                                Select::new(
                                    "sel",
                                    vec![
                                        SelectOption::new("unified", "Unified"),
                                        SelectOption::new("split", "Split"),
                                        SelectOption::new("auto", "Automatic"),
                                    ],
                                )
                                .selected(self.select.clone())
                                .on_change({
                                    let entity = entity.clone();
                                    move |v, _, cx| entity.update(cx, |g, cx| {
                                        g.select = v.clone();
                                        cx.notify();
                                    })
                                }),
                            ),
                        )
                        .child(DropdownMenu::new("menu", demo_menu, |open| {
                            Button::new("menu-trigger")
                                .variant(ButtonVariant::Outline)
                                .label("Menu")
                                .icon_end(IconName::ChevronDown)
                                .pressed(open)
                                .into_any_element()
                        }))
                        .child(
                            Popover::new("pop", |open| {
                                Button::new("pop-trigger")
                                    .variant(ButtonVariant::Outline)
                                    .label("Popover")
                                    .pressed(open)
                                    .into_any_element()
                            })
                            .width(PopoverWidth::Sm)
                            .title("Popover title")
                            .content(|_, cx| {
                                div()
                                    .text_scale(TextSize::Sm)
                                    .text_color(cx.tokens().muted_foreground)
                                    .child("Popover body text, 14px muted.")
                                    .into_any_element()
                            }),
                        )
                        .child(
                            div()
                                .id("tip")
                                .t3_tooltip(Tooltip::text("Toggle main sidebar").kbd("Ctrl+B"))
                                .child(Badge::new("Hover me").variant(BadgeVariant::Outline)),
                        ),
                )
                .into_any_element(),
            section("Dialog, Confirm, Toast", cx)
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(Button::new("open-dialog").variant(ButtonVariant::Outline).label("Dialog").on_click(
                            |_, window, cx| {
                                Dialogs::open(window, cx, |_, _| {
                                    Dialog::new()
                                        .title("Add project")
                                        .description("Choose a folder to add as a project.")
                                        .child(div().child("Panel content"))
                                        .footer(
                                            DialogFooter::new()
                                                .child(Button::new("d-cancel").variant(ButtonVariant::Outline).label("Cancel").on_click(|_, w, cx| Dialogs::close_top(w, cx)))
                                                .child(Button::new("d-ok").label("Add")),
                                        )
                                });
                            },
                        ))
                        .child(Button::new("open-confirm").variant(ButtonVariant::DestructiveOutline).label("Confirm").on_click(
                            |_, window, cx| {
                                drop(Dialogs::confirm(
                                    "Delete thread \"Fix flaky CI\"?\nThis permanently clears conversation history for this thread.",
                                    ConfirmOptions::destructive(),
                                    window,
                                    cx,
                                ));
                            },
                        ))
                        .child(Button::new("toast-err").variant(ButtonVariant::Outline).label("Error toast").on_click(
                            |_, _, cx| {
                                Toasts::push(Toast::error("Failed to rename thread").description("Thread title cannot be empty"), cx);
                            },
                        ))
                        .child(Button::new("toast-ok").variant(ButtonVariant::Outline).label("Success toast").on_click(
                            |_, _, cx| {
                                Toasts::push(Toast::success("Project created").description("tern").action("Open", |_, _| {}), cx);
                            },
                        )),
                )
                .into_any_element(),
            section("Input, Textarea, Skeleton, Spinner, Icons", cx)
                .child(
                    h_flex()
                        .gap(px(12.))
                        .items_start()
                        .child(div().w(px(240.)).child(TextField::new(&self.input)))
                        .child(div().w(px(240.)).child(TextField::new(&self.input).size(InputSize::Sm).invalid(true)))
                        .child(div().w(px(240.)).child(TextareaField::new(&self.textarea)))
                        .child(
                            v_flex()
                                .gap(px(6.))
                                .child(Skeleton::new("sk1").w(px(160.)).h(px(12.)))
                                .child(Skeleton::new("sk2").w(px(120.)).h(px(12.)))
                                .child(Skeleton::new("sk3").w(px(48.)).h(px(48.)).shape(crate::ui::SkeletonShape::Card)),
                        )
                        .child(Spinner::new().muted())
                        .child(
                            h_flex().gap(px(8.)).text_color(t.foreground).children(
                                [
                                    IconName::FolderGit2,
                                    IconName::SquarePen,
                                    IconName::Terminal,
                                    IconName::GitBranch,
                                    IconName::CircleCheck,
                                    IconName::TriangleAlert,
                                ]
                                .into_iter()
                                .map(Icon::new),
                            ),
                        ),
                )
                .into_any_element(),
        ];
        div()
            .id("gallery")
            .size_full()
            .overflow_y_scroll()
            .bg(t.background)
            .child(
                v_flex()
                    .pt(px(56.))
                    .px(px(32.))
                    .pb(px(48.))
                    .gap(px(20.))
                    .children(content.into_iter().enumerate().flat_map(|(i, el)| {
                        let sep = (i > 0).then(|| Separator::horizontal().into_any_element());
                        sep.into_iter().chain(std::iter::once(el))
                    }))
                    .when(false, |el| el),
            )
    }
}

trait ChildrenFrom {
    fn children_from(self, items: impl Iterator<Item = Toggle>) -> Self;
}

impl ChildrenFrom for ToggleGroup {
    fn children_from(mut self, items: impl Iterator<Item = Toggle>) -> Self {
        for item in items {
            self = self.item(item);
        }
        self
    }
}

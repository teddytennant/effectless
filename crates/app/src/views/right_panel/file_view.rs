//! The file viewer (ref 19, `FilePreviewPanel.tsx`): a read-only, highlighted
//! source view with breadcrumbs and the workspace file explorer on the right.
//! With no file it is the "Files" surface: the explorer alone.

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, ClipboardItem, Context, Entity, EventEmitter,
    HighlightStyle, InteractiveElement as _, IntoElement, ListAlignment, ListState, MouseButton,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Task, Window, canvas, div, list, prelude::FluentBuilder as _, px,
};
use gpui_base::Align;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::Scrollbar;
use tern_core::ThreadId;

use super::diff::code::{self, CodeColors};
use super::diff::data::{self, RecordedPatch};
use super::file_tree::{FileTree, TreeOrder, filter_paths};
use super::tree_view;
use crate::store::{Store, StoreEvent};
use crate::syntax;
use crate::theme::{ActiveTheme as _, TextExt as _, TextSize};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, DropdownMenu, Icon, IconName, Menu, MenuItem, Toggle,
    ToggleSize, ToggleVariant, Tooltip, TooltipExt as _, TooltipSide, h_flex, v_flex,
};

const EXPLORER_MIN: f32 = 160.;
const EXPLORER_MAX: f32 = 560.;
const EXPLORER_DEFAULT: f32 = 256.;
const CONTENT_MIN: f32 = 256.;

pub enum FileEvent {
    OpenFile(String),
}

impl EventEmitter<FileEvent> for FileView {}

struct FileText {
    lines: Vec<String>,
    spans: Vec<Vec<(Range<usize>, HighlightStyle)>>,
    dark: bool,
}

pub struct FileView {
    thread_id: ThreadId,
    store: Entity<Store>,
    /// None: the Files surface (explorer only).
    path: Option<String>,
    text: Option<Rc<FileText>>,
    error: Option<String>,
    load: Option<Task<()>>,
    files: Rc<Vec<String>>,
    files_task: Option<Task<()>>,
    search: Entity<InputState>,
    explorer_open: bool,
    explorer_width: Option<Pixels>,
    explorer_drag: Option<(Pixels, Pixels)>,
    row_width: Pixels,
    word_wrap: bool,
    copied: bool,
    tree_collapsed: HashSet<String>,
    list: ListState,
    _subscriptions: Vec<gpui::Subscription>,
}

impl FileView {
    pub fn new(
        thread_id: ThreadId,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = Store::global(cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search files"));
        let subscriptions = vec![
            cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&store, |this: &mut Self, _, event: &StoreEvent, cx| {
                if matches!(event, StoreEvent::Thread(id) if *id == this.thread_id) {
                    this.refresh_files(cx);
                }
            }),
        ];
        let mut this = Self {
            thread_id,
            store,
            path,
            text: None,
            error: None,
            load: None,
            files: Rc::new(Vec::new()),
            files_task: None,
            search,
            explorer_open: true,
            explorer_width: None,
            explorer_drag: None,
            row_width: px(0.),
            word_wrap: true,
            copied: false,
            tree_collapsed: HashSet::new(),
            list: ListState::new(0, ListAlignment::Top, px(600.)),
            _subscriptions: subscriptions,
        };
        this.refresh_files(cx);
        this
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    fn cwd_and_patches(&self, cx: &App) -> Option<(PathBuf, Vec<RecordedPatch>, String)> {
        let store = self.store.read(cx);
        let shell = store.snapshot().thread(&self.thread_id)?;
        let project = store.snapshot().project(&shell.thread.project_id)?;
        let cwd = shell
            .thread
            .worktree_path
            .clone()
            .unwrap_or_else(|| project.workspace_root.clone());
        let patches = store
            .thread(&self.thread_id)
            .map(|d| data::recorded_patches(&d))
            .unwrap_or_default();
        Some((PathBuf::from(cwd), patches, project.title.clone()))
    }

    fn refresh_files(&mut self, cx: &mut Context<Self>) {
        let Some((cwd, patches, _)) = self.cwd_and_patches(cx) else {
            return;
        };
        let job = cx.background_spawn(async move { data::list_files(&cwd, &patches) });
        self.files_task = Some(cx.spawn(async move |this, cx| {
            let files = job.await;
            let _ = this.update(cx, |this, cx| {
                this.files = Rc::new(files);
                cx.notify();
            });
        }));
    }

    fn ensure_text(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let dark = cx.tern_theme().is_dark();
        if self.load.is_some() || self.text.as_ref().is_some_and(|t| t.dark == dark) {
            return;
        }
        if self.error.is_some() && self.text.is_none() {
            return;
        }
        let Some((cwd, patches, _)) = self.cwd_and_patches(cx) else {
            return;
        };
        let job = cx.background_spawn(async move {
            let text = data::read_file(&cwd, &path, &patches)?;
            let text = text.replace('\t', "    ");
            let language = syntax::language_for_path(&path);
            let spans = syntax::highlight_lines(&text, language, dark);
            let lines = text.split('\n').map(str::to_string).collect::<Vec<_>>();
            Ok::<_, String>(FileText { lines, spans, dark })
        });
        self.load = Some(cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.load = None;
                match result {
                    Ok(text) => {
                        this.list.reset(text.lines.len());
                        this.text = Some(Rc::new(text));
                        this.error = None;
                    }
                    Err(err) => this.error = Some(err),
                }
                cx.notify();
            });
        }));
    }

    fn render_line(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(text) = self.text.clone() else {
            return div().into_any_element();
        };
        let theme = cx.tern_theme().clone();
        let colors = CodeColors::new(&theme, theme.colors.code_background);
        let ch = code::char_width(window);
        let digits = text.lines.len().max(1).to_string().len();
        let gutter_w = ch * (3 + digits) as f32;
        let line = &text.lines[ix];
        let spans = text.spans.get(ix).map(Vec::as_slice).unwrap_or(&[]);
        div()
            .flex()
            .flex_row()
            .w_full()
            .font_family(crate::theme::font::MONO)
            .text_size(code::CODE_FONT_SIZE)
            .line_height(code::LINE_HEIGHT)
            .child(
                div()
                    .flex_none()
                    .w(gutter_w)
                    .pr(ch)
                    .flex()
                    .justify_end()
                    .text_color(colors.number)
                    .child((ix + 1).to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px(ch)
                    .overflow_hidden()
                    .text_color(colors.fg)
                    .when(!self.word_wrap, |el| el.whitespace_nowrap())
                    .child(code::code_text(line, spans)),
            )
            .into_any_element()
    }

    fn breadcrumbs(&self, project: &str, cx: &App) -> AnyElement {
        let t = cx.tokens();
        let mut el = h_flex()
            .min_w_0()
            .flex_1()
            .gap(px(6.))
            .overflow_hidden()
            .text_scale(TextSize::Xs)
            .whitespace_nowrap()
            .child(
                div()
                    .text_color(t.muted_foreground)
                    .child(project.to_string()),
            );
        if let Some(path) = &self.path {
            let segments: Vec<&str> = path.split('/').collect();
            let last = segments.len() - 1;
            for (i, segment) in segments.into_iter().enumerate() {
                el = el
                    .child(
                        Icon::new(IconName::ChevronRight)
                            .size(px(14.))
                            .color(t.muted_foreground)
                            .opacity(0.7),
                    )
                    .child(
                        div()
                            .text_color(if i == last {
                                t.foreground
                            } else {
                                t.muted_foreground
                            })
                            .when(i == last, |d| d.font_medium())
                            .child(segment.to_string()),
                    );
            }
        }
        el.into_any_element()
    }

    fn render_header(&self, project: &str, cwd: PathBuf, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let s = *cx.states();
        let this = cx.entity().downgrade();
        let abs = self
            .path
            .as_ref()
            .map(|p| cwd.join(p))
            .unwrap_or(cwd.clone());
        let reveal_target = abs.clone();
        let menu_target = abs.clone();
        let open_in = h_flex()
            .flex_none()
            .h(px(28.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.input)
            .bg(s.field_fill)
            .child(
                div()
                    .id("file-open-in")
                    .h_full()
                    .w(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_l(px(8.))
                    .cursor_pointer()
                    .hover(move |st| st.bg(t.accent))
                    .t3_tooltip(Tooltip::text("Reveal in Files").side(TooltipSide::Bottom))
                    .on_click(move |_, _, cx| cx.reveal_path(&reveal_target))
                    .child(Icon::new(IconName::Folder).size(px(16.)).opacity(0.8)),
            )
            .child(div().w(px(1.)).h(px(16.)).bg(t.input))
            .child(
                DropdownMenu::new(
                    "file-open-in-menu",
                    move |_, _| {
                        let open = menu_target.clone();
                        let reveal = menu_target.clone();
                        Menu::new()
                            .item(
                                MenuItem::new("Open")
                                    .icon(IconName::ExternalLink)
                                    .on_select(move |_, cx| cx.open_with_system(&open)),
                            )
                            .item(
                                MenuItem::new("Reveal in Files")
                                    .icon(IconName::FolderOpen)
                                    .on_select(move |_, cx| cx.reveal_path(&reveal)),
                            )
                    },
                    move |_| {
                        div()
                            .id("file-open-in-chevron")
                            .h(px(26.))
                            .w(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_r(px(8.))
                            .cursor_pointer()
                            .hover(move |st| st.bg(t.accent))
                            .child(Icon::new(IconName::ChevronDown).size(px(14.)).opacity(0.8))
                            .into_any_element()
                    },
                )
                .align(Align::End),
            );
        let icon14 = |icon| Icon::new(icon).size(px(14.)).opacity(0.8);
        let toggle = |f: fn(&mut FileView, &mut Context<FileView>)| {
            let this = this.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, f);
                }
            }
        };
        let copy_this = this.clone();
        let has_text = self.text.is_some();
        h_flex()
            .flex_none()
            .h(px(28.))
            .mb(px(12.))
            .px(px(12.))
            .gap(px(8.))
            .child(self.breadcrumbs(project, cx))
            .when(self.path.is_some(), |el| {
                el.child(open_in)
                    .child(
                        div()
                            .id("file-wrap")
                            .t3_tooltip(Tooltip::text(if self.word_wrap {
                                "Disable word wrap"
                            } else {
                                "Enable word wrap"
                            }))
                            .child(
                                Toggle::new("file-wrap-toggle")
                                    .variant(ToggleVariant::Ghost)
                                    .size(ToggleSize::Sm)
                                    .pressed(self.word_wrap)
                                    .child(icon14(IconName::WrapText))
                                    .on_click(toggle(|this, cx| {
                                        this.word_wrap = !this.word_wrap;
                                        this.list.remeasure();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when(has_text, |el| {
                        el.child(
                            div()
                                .id("file-copy")
                                .t3_tooltip(Tooltip::text(if self.copied {
                                    "Copied"
                                } else {
                                    "Copy contents"
                                }))
                                .child(
                                    Button::new("file-copy-btn")
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::IconSm)
                                        .icon(if self.copied {
                                            IconName::Check
                                        } else {
                                            IconName::Copy
                                        })
                                        .icon_size(px(14.))
                                        .on_click(move |_, _, cx| {
                                            if let Some(this) = copy_this.upgrade() {
                                                this.update(cx, |this, cx| {
                                                    if let Some(text) = &this.text {
                                                        cx.write_to_clipboard(
                                                            ClipboardItem::new_string(
                                                                text.lines.join("\n"),
                                                            ),
                                                        );
                                                        this.copied = true;
                                                        cx.notify();
                                                    }
                                                });
                                            }
                                        }),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id("file-explorer")
                            .t3_tooltip(Tooltip::text(if self.explorer_open {
                                "Hide file explorer"
                            } else {
                                "Show file explorer"
                            }))
                            .child(
                                Toggle::new("file-explorer-toggle")
                                    .variant(ToggleVariant::Ghost)
                                    .size(ToggleSize::Sm)
                                    .pressed(self.explorer_open)
                                    .child(icon14(IconName::FolderTree))
                                    .on_click(toggle(|this, cx| {
                                        this.explorer_open = !this.explorer_open;
                                        cx.notify();
                                    })),
                            ),
                    )
            })
            .into_any_element()
    }

    fn render_explorer(&self, resizable: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.tokens();
        let dark = cx.tern_theme().is_dark();
        let query = self.search.read(cx).value().to_string();
        let filtered = filter_paths(&self.files, &query);
        let entries: Vec<(String, Option<tern_git::DiffFile>)> =
            filtered.into_iter().map(|p| (p, None)).collect();
        let tree = FileTree::build(&entries, TreeOrder::Explorer);
        let rows = tree.rows(&self.tree_collapsed);
        let selected = self.path.clone();
        let this = cx.entity().downgrade();
        let items = rows.into_iter().enumerate().map(|(i, row)| {
            let this = this.clone();
            let path = row.path.clone();
            let is_dir = row.is_dir;
            tree_view::row(
                SharedString::from(format!("explorer-{i}")),
                &row,
                selected.as_deref() == Some(row.path.as_str()),
                dark,
                cx,
            )
            .on_click(move |_, _, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        if is_dir {
                            if !this.tree_collapsed.remove(&path) {
                                this.tree_collapsed.insert(path.clone());
                            }
                            cx.notify();
                        } else {
                            cx.emit(FileEvent::OpenFile(path.clone()));
                        }
                    });
                }
            })
            .into_any_element()
        });
        let refresh_this = this.clone();
        let width = self.explorer_width.unwrap_or(px(EXPLORER_DEFAULT)).clamp(
            px(EXPLORER_MIN),
            px(EXPLORER_MAX
                .min(self.row_width.as_f32() - CONTENT_MIN)
                .max(EXPLORER_MIN)),
        );
        v_flex()
            .id("file-explorer-pane")
            .relative()
            .h_full()
            .bg(t.background)
            .map(|el| {
                if resizable {
                    el.flex_none()
                        .w(width)
                        .border_l_1()
                        .border_color(t.border.opacity(0.6))
                } else {
                    el.flex_1()
                }
            })
            .child(
                h_flex()
                    .flex_none()
                    .h(px(32.))
                    .mt(px(2.))
                    .px(px(16.))
                    .gap(px(4.))
                    .child(
                        Button::new("explorer-refresh")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::IconXs)
                            .icon(IconName::RefreshCw)
                            .tooltip(Tooltip::text("Refresh files"))
                            .on_click(move |_, _, cx| {
                                if let Some(this) = refresh_this.upgrade() {
                                    this.update(cx, |this, cx| this.refresh_files(cx));
                                }
                            }),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.search)
                                .appearance(false)
                                .text_size(px(14.))
                                .h(px(28.)),
                        ),
                    ),
            )
            .child(
                v_flex()
                    .id("explorer-list")
                    .flex_1()
                    .px(px(16.))
                    .pt(px(2.))
                    .overflow_y_scroll()
                    .children(items),
            )
            .when(resizable, |el| {
                el.child(
                    div()
                        .id("explorer-resize")
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(-2.))
                        .w(px(4.))
                        .cursor_col_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                                this.explorer_drag = Some((e.position.x, width));
                                cx.stop_propagation();
                            }),
                        ),
                )
            })
            .into_any_element()
    }
}

impl Render for FileView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.tokens();
        self.ensure_text(cx);
        let (cwd, _, project) = self
            .cwd_and_patches(cx)
            .unwrap_or_else(|| (PathBuf::new(), Vec::new(), String::new()));
        let header = self.render_header(&project, cwd, cx);
        let weak = cx.entity().downgrade();
        let body: AnyElement = if self.path.is_none() {
            self.render_explorer(false, cx)
        } else {
            let content: AnyElement = if let Some(err) = &self.error {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_scale(TextSize::Xs)
                    .text_color(t.muted_foreground.opacity(0.7))
                    .child(err.clone())
                    .into_any_element()
            } else if self.text.is_some() {
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(t.code_background)
                    .pt(px(6.))
                    .child(
                        list(
                            self.list.clone(),
                            cx.processor(|this, ix, window, cx| this.render_line(ix, window, cx)),
                        )
                        .size_full(),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .bottom_0()
                            .w(px(12.))
                            .child(Scrollbar::vertical(&self.list)),
                    )
                    .into_any_element()
            } else {
                div().flex_1().bg(t.code_background).into_any_element()
            };
            h_flex()
                .relative()
                .flex_1()
                .min_h_0()
                .items_start()
                .child(
                    canvas(
                        move |bounds, _, cx| {
                            if let Some(this) = weak.upgrade() {
                                this.update(cx, |this, _| this.row_width = bounds.size.width);
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .child(content)
                .when(self.explorer_open, |el| {
                    el.child(self.render_explorer(true, cx))
                })
                .into_any_element()
        };
        v_flex()
            .id("file-view")
            .size_full()
            .min_w_0()
            .bg(t.background)
            .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, cx| {
                if let Some((start_x, start_w)) = this.explorer_drag {
                    if !e.dragging() {
                        this.explorer_drag = None;
                        return;
                    }
                    this.explorer_width = Some(start_w + (start_x - e.position.x));
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.explorer_drag = None),
            )
            .child(header)
            .child(v_flex().flex_1().min_h_0().child(body))
    }
}

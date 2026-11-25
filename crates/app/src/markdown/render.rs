//! Lays a parsed [`Document`] out as GPUI elements with T3's `chat-markdown` styles
//! (spec 03 §4): 14px / 22.75 text at foreground 90%, collapsed 10.4px block margins,
//! headings, lists, quotes and alerts, code blocks with a header (language, wrap, copy)
//! and tree-sitter highlighting, tables with a copy menu, links and file chips.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, ClipboardItem, ElementId, FontStyle,
    FontWeight, Hsla, InteractiveElement as _, InteractiveText, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, StrikethroughStyle, Styled as _, StyledText,
    TextRun, Window, div, prelude::FluentBuilder as _, px, relative,
};

use super::highlight;
use super::parse::{AlertKind, Align, Block, Document, FileLink, Inline, SpanStyle};
use crate::theme::{ActiveTheme as _, TextExt as _, font, leading, motion, radius};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, DropdownMenu, Icon, IconName, Menu, MenuItem, Tooltip,
    TooltipSide, h_flex, v_flex,
};

/// Callback when a file chip is activated (`cmd` = Ctrl/Cmd held).
pub type FileHandler = Rc<dyn Fn(&FileLink, bool, &mut Window, &mut App)>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Variant {
    /// Assistant replies, plans: `chat-markdown`.
    #[default]
    Assistant,
    /// `user-message-markdown`: inline code tinted with the bubble's text color.
    User,
    /// Reasoning inside a tool panel: italic, muted.
    Muted,
}

#[derive(Clone)]
pub struct Options {
    /// Unique per rendered document (row id); element ids derive from it.
    pub id: SharedString,
    pub variant: Variant,
    /// Fade each block in over 600ms when it first appears.
    pub streaming: bool,
    pub text_color: Option<Hsla>,
    /// Default for code block wrapping (`wordWrap` setting).
    pub wrap_code: bool,
    pub on_file: Option<FileHandler>,
}

impl Options {
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            variant: Variant::Assistant,
            streaming: false,
            text_color: None,
            wrap_code: false,
            on_file: None,
        }
    }
}

const BLOCK_MARGIN: f32 = 10.4;
const LINE: f32 = 14. * leading::RELAXED;

struct Margins {
    top: f32,
    bottom: f32,
}

fn margins(block: &Block) -> Margins {
    match block {
        Block::Heading { .. } => Margins {
            top: 20.,
            bottom: 8.,
        },
        Block::Alert { .. } => Margins {
            top: 4.,
            bottom: 4.,
        },
        Block::Rule => Margins {
            top: 16.,
            bottom: 16.,
        },
        Block::Footnotes(_) => Margins {
            top: 20.,
            bottom: 0.,
        },
        _ => Margins {
            top: BLOCK_MARGIN,
            bottom: BLOCK_MARGIN,
        },
    }
}

/// Renders a whole document. First block has no top margin, last no bottom.
pub fn render(doc: &Document, opts: &Options, window: &mut Window, cx: &mut App) -> AnyElement {
    let t = cx.tokens();
    let color = opts
        .text_color
        .unwrap_or_else(|| t.foreground.opacity(0.9));
    let mut col = v_flex()
        .w_full()
        .min_w_0()
        .text_size(px(14.))
        .line_height(px(LINE))
        .text_color(color)
        .font_family(font::SANS)
        .when(opts.variant == Variant::Muted, |el| el.italic());
    let mut prev: Option<f32> = None;
    for (i, block) in doc.blocks.iter().enumerate() {
        let m = margins(block);
        let gap = prev.map_or(0., |p| p.max(m.top));
        prev = Some(m.bottom);
        let el = render_block(block, opts, &format!("{}-b{i}", opts.id), 0, window, cx);
        let el = div().w_full().min_w_0().mt(px(gap)).child(el);
        if opts.streaming {
            col = col.child(
                el.with_animation(
                    SharedString::from(format!("{}-fade{i}", opts.id)),
                    Animation::new(Duration::from_millis(600)).with_easing(motion::cubic_bezier(0., 0., 0.58, 1.)),
                    |el, d| el.opacity(d),
                )
                .into_any_element(),
            );
        } else {
            col = col.child(el);
        }
    }
    col.into_any_element()
}

fn render_blocks(
    blocks: &[Block],
    opts: &Options,
    id: &str,
    depth: usize,
    tight: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let mut col = v_flex().w_full().min_w_0();
    let mut prev: Option<f32> = None;
    for (i, block) in blocks.iter().enumerate() {
        let m = margins(block);
        let gap = match prev {
            None => 0.,
            Some(_) if tight => 4.,
            Some(p) => p.max(m.top),
        };
        prev = Some(m.bottom);
        col = col.child(
            div()
                .w_full()
                .min_w_0()
                .mt(px(gap))
                .child(render_block(block, opts, &format!("{id}-{i}"), depth, window, cx)),
        );
    }
    col.into_any_element()
}

fn render_block(
    block: &Block,
    opts: &Options,
    id: &str,
    depth: usize,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let t = *cx.tokens();
    match block {
        Block::Paragraph(inline) => render_inline(inline, opts, id, None, cx),
        Block::Heading { depth: d, text } => {
            let size = match d {
                1 => 20.,
                2 => 18.,
                3 => 16.,
                _ => 14.,
            };
            let color = if *d >= 6 {
                t.muted_foreground
            } else {
                t.foreground
            };
            div()
                .text_size(px(size))
                .line_height(px(size * 1.3))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(render_inline(text, opts, id, Some(FontWeight::SEMIBOLD), cx))
                .into_any_element()
        }
        Block::List {
            ordered,
            start,
            items,
        } => {
            let digits = (*start as usize + items.len()).to_string().len();
            let marker_w = if *ordered {
                (8. * digits as f32 + 12.).max(20.)
            } else {
                20.
            };
            let mut col = v_flex().w_full();
            for (i, item) in items.iter().enumerate() {
                let marker: AnyElement = match item.checked {
                    Some(checked) => div()
                        .size(px(13.))
                        .mt(px(5.))
                        .rounded(px(3.))
                        .border_1()
                        .border_color(t.border)
                        .when(checked, |el| {
                            el.bg(t.primary).border_color(t.primary).child(
                                Icon::new(IconName::Check)
                                    .size(px(11.))
                                    .color(t.primary_foreground),
                            )
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .into_any_element(),
                    None if *ordered => div()
                        .text_color(t.foreground.opacity(0.9))
                        .child(ordered_marker(*start as usize + i, depth))
                        .into_any_element(),
                    None => div()
                        .text_color(t.foreground.opacity(0.9))
                        .child(match depth % 3 {
                            0 => "•",
                            1 => "◦",
                            _ => "▪",
                        })
                        .into_any_element(),
                };
                let marker_box = if item.checked.is_some() {
                    div().w(px(20.)).flex_none().child(marker)
                } else if *ordered {
                    div()
                        .w(px(marker_w))
                        .flex_none()
                        .flex()
                        .justify_end()
                        .pr(px(6.))
                        .child(marker)
                } else {
                    div().w(px(marker_w)).flex_none().pl(px(6.)).child(marker)
                };
                col = col.child(
                    h_flex()
                        .items_start()
                        .w_full()
                        .when(i > 0, |el| el.mt(px(4.)))
                        .child(marker_box)
                        .child(div().flex_1().min_w_0().child(render_blocks(
                            &item.blocks,
                            opts,
                            &format!("{id}-li{i}"),
                            depth + 1,
                            true,
                            window,
                            cx,
                        ))),
                );
            }
            col.into_any_element()
        }
        Block::Code { lang, title, code } => code_block(lang.as_deref(), title.as_deref(), code, opts, id, window, cx),
        Block::Quote(blocks) => h_flex()
            .items_stretch()
            .child(div().w(px(2.)).flex_none().bg(t.foreground.opacity(0.9 * 0.35)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pl(px(12.8))
                    .child(render_blocks(blocks, opts, id, depth, false, window, cx)),
            )
            .into_any_element(),
        Block::Alert { kind, blocks } => {
            let dark = cx.tern_theme().is_dark();
            let (icon, border, title) = alert_style(*kind, dark);
            h_flex()
                .items_stretch()
                .child(div().w(px(2.)).flex_none().bg(border))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .pl(px(12.))
                        .child(
                            h_flex()
                                .gap(px(6.))
                                .font_medium()
                                .text_color(title)
                                .child(Icon::new(icon).size(px(14.)).color(title))
                                .child(kind.label()),
                        )
                        .child(render_blocks(blocks, opts, id, depth, false, window, cx)),
                )
                .into_any_element()
        }
        Block::Table {
            align,
            header,
            rows,
        } => table(align, header, rows, opts, id, cx),
        Block::Rule => div()
            .h(px(1.))
            .w_full()
            .bg(if cx.tern_theme().is_dark() {
                t.muted_foreground
            } else {
                t.border
            })
            .into_any_element(),
        Block::Footnotes(notes) => {
            let mut col = v_flex()
                .w_full()
                .pt(px(12.))
                .border_t_1()
                .border_color(t.border)
                .text_size(px(12.))
                .line_height(px(18.))
                .text_color(t.muted_foreground)
                .gap(px(4.));
            for (i, (label, blocks)) in notes.iter().enumerate() {
                col = col.child(
                    h_flex()
                        .items_start()
                        .gap(px(6.))
                        .child(div().font_weight(FontWeight::BOLD).child(format!("{label}.")))
                        .child(div().flex_1().min_w_0().child(render_blocks(
                            blocks,
                            opts,
                            &format!("{id}-fn{i}"),
                            depth,
                            true,
                            window,
                            cx,
                        ))),
                );
            }
            col.into_any_element()
        }
    }
}

fn ordered_marker(n: usize, depth: usize) -> String {
    match depth % 3 {
        0 => format!("{n}."),
        1 => {
            let mut s = String::new();
            let mut v = n;
            while v > 0 {
                v -= 1;
                s.insert(0, (b'a' + (v % 26) as u8) as char);
                v /= 26;
            }
            format!("{s}.")
        }
        _ => format!("{}.", roman(n)),
    }
}

fn roman(mut n: usize) -> String {
    let table = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (v, s) in table {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

fn rgb(hex: u32, alpha: f32) -> Hsla {
    crate::tokens::c((hex << 8) | 0xff).opacity(alpha)
}

/// GitHub alert icon, border and title colors (tailwind 500/70 borders).
fn alert_style(kind: AlertKind, dark: bool) -> (IconName, Hsla, Hsla) {
    match kind {
        AlertKind::Note => (
            IconName::Info,
            rgb(0x3b82f6, 0.7),
            rgb(if dark { 0x60a5fa } else { 0x2563eb }, 1.),
        ),
        AlertKind::Tip => (
            IconName::Lightbulb,
            rgb(0x10b981, 0.7),
            rgb(if dark { 0x34d399 } else { 0x059669 }, 1.),
        ),
        AlertKind::Important => (
            IconName::MessageSquareWarning,
            rgb(0xa855f7, 0.7),
            rgb(if dark { 0xc084fc } else { 0x9333ea }, 1.),
        ),
        AlertKind::Warning => (
            IconName::TriangleAlert,
            rgb(0xf59e0b, 0.7),
            rgb(if dark { 0xf59e0b } else { 0xd97706 }, 1.),
        ),
        AlertKind::Caution => (
            IconName::OctagonAlert,
            rgb(0xef4444, 0.7),
            rgb(if dark { 0xf87171 } else { 0xdc2626 }, 1.),
        ),
    }
}

/// A narrow no-break space pads inline code inside its tinted run (GPUI runs have
/// no padding).
const PAD: &str = "\u{202f}";

/// Inline text as one wrapping `StyledText`, with click ranges for links and files.
pub fn render_inline(
    inline: &Inline,
    opts: &Options,
    id: &str,
    weight: Option<FontWeight>,
    cx: &App,
) -> AnyElement {
    let t = cx.tokens();
    let base_color = opts.text_color.unwrap_or_else(|| t.foreground.opacity(0.9));
    let code_fg = match opts.variant {
        Variant::User => t.message_foreground,
        _ => t.foreground,
    };
    let code_bg = match opts.variant {
        Variant::User => t.message_foreground.opacity(0.1),
        _ => t.foreground.opacity(0.1),
    };
    let chip = chip_color(cx);
    let base_weight = weight.unwrap_or(FontWeight::NORMAL);
    let italic_base = opts.variant == Variant::Muted;

    // Segment boundaries from every span edge.
    let text = &inline.text;
    let mut cuts: Vec<usize> = vec![0, text.len()];
    for s in &inline.spans {
        cuts.push(s.range.start.min(text.len()));
        cuts.push(s.range.end.min(text.len()));
    }
    cuts.sort_unstable();
    cuts.dedup();

    let mut display = String::with_capacity(text.len() + 16);
    let mut runs: Vec<TextRun> = Vec::new();
    let mut clicks: Vec<(std::ops::Range<usize>, Click)> = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= b || !text.is_char_boundary(a) || !text.is_char_boundary(b) {
            continue;
        }
        let mut bold = false;
        let mut italic = italic_base;
        let mut strike = false;
        let mut code = false;
        let mut link: Option<String> = None;
        let mut file: Option<FileLink> = None;
        for s in &inline.spans {
            if s.range.start <= a && b <= s.range.end {
                match &s.style {
                    SpanStyle::Strong => bold = true,
                    SpanStyle::Emphasis => italic = true,
                    SpanStyle::Strike => strike = true,
                    SpanStyle::Code => code = true,
                    SpanStyle::Link(u) => link = Some(u.clone()),
                    SpanStyle::File(f) => file = Some(f.clone()),
                    SpanStyle::FootnoteRef => bold = true,
                }
            }
        }
        let seg = &text[a..b];
        let starts = inline.spans.iter().any(|s| s.range.start == a && matches!(s.style, SpanStyle::Code));
        let ends = inline.spans.iter().any(|s| s.range.end == b && matches!(s.style, SpanStyle::Code));
        let start = display.len();
        let (color, bg, family, weight) = if code {
            (code_fg, Some(code_bg), font::MONO, FontWeight::NORMAL)
        } else if file.is_some() {
            (chip, None, font::SANS, FontWeight::MEDIUM)
        } else if link.is_some() {
            (t.info_foreground, None, font::SANS, base_weight)
        } else {
            (
                base_color,
                None,
                font::SANS,
                if bold { FontWeight::BOLD } else { base_weight },
            )
        };
        let mut push = |s: &str, display: &mut String| {
            display.push_str(s);
            runs.push(TextRun {
                len: s.len(),
                font: gpui::Font {
                    family: family.into(),
                    weight,
                    style: if italic {
                        FontStyle::Italic
                    } else {
                        FontStyle::Normal
                    },
                    ..Default::default()
                },
                color,
                background_color: bg,
                underline: None,
                strikethrough: strike.then(|| StrikethroughStyle {
                    thickness: px(1.),
                    color: Some(color),
                }),
            });
        };
        if code && starts {
            push(PAD, &mut display);
        }
        push(seg, &mut display);
        if code && ends {
            push(PAD, &mut display);
        }
        let range = start..display.len();
        if let Some(f) = file {
            clicks.push((range, Click::File(f)));
        } else if let Some(u) = link {
            clicks.push((range, Click::Url(u)));
        }
    }
    if display.is_empty() {
        return div().into_any_element();
    }
    let styled = StyledText::new(display).with_runs(runs);
    if clicks.is_empty() {
        return div().w_full().min_w_0().child(styled).into_any_element();
    }
    let ranges: Vec<_> = clicks.iter().map(|(r, _)| r.clone()).collect();
    let targets: Rc<Vec<Click>> = Rc::new(clicks.into_iter().map(|(_, c)| c).collect());
    let on_file = opts.on_file.clone();
    div()
        .w_full()
        .min_w_0()
        .child(
            InteractiveText::new(ElementId::Name(SharedString::from(format!("{id}-txt"))), styled)
                .on_click(ranges, move |ix, window, cx| match &targets[ix] {
                    Click::Url(u) => cx.open_url(u),
                    Click::File(f) => {
                        let cmd = window.modifiers().secondary(cx);
                        if let Some(h) = &on_file {
                            h(f, cmd, window, cx);
                        }
                    }
                }),
        )
        .into_any_element()
}

/// `--context-chip-mention`: oklch(0.62 0.11 215) solved to 5:1 contrast on the
/// canvas (darker in light mode, lighter in dark).
pub fn chip_color(cx: &App) -> Hsla {
    if cx.tern_theme().is_dark() {
        rgb(0x5cc3db, 1.)
    } else {
        rgb(0x0b6f86, 1.)
    }
}

enum Click {
    Url(String),
    File(FileLink),
}

#[derive(Default)]
struct CodeState {
    wrap: Option<bool>,
    copied: bool,
}

fn code_block(
    lang: Option<&str>,
    title: Option<&str>,
    code: &str,
    opts: &Options,
    id: &str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.tern_theme().clone();
    let t = theme.colors;
    let dark = theme.is_dark();
    let state = window.use_keyed_state(
        ElementId::Name(SharedString::from(format!("{id}-code"))),
        cx,
        |_, _| CodeState::default(),
    );
    let (wrap, copied) = {
        let s = state.read(cx);
        (s.wrap.unwrap_or(opts.wrap_code), s.copied)
    };
    let header_color = t.foreground.opacity(0.72);
    let label: AnyElement = match (title, lang) {
        (Some(title), _) => h_flex()
            .gap(px(6.))
            .child(Icon::new(IconName::File).size(px(14.)).color(header_color))
            .child(title.to_string())
            .into_any_element(),
        (None, Some(lang)) => div().child(lang.to_string()).into_any_element(),
        (None, None) => div().into_any_element(),
    };
    let code_text = code.strip_suffix('\n').unwrap_or(code).to_string();
    let highlights = highlight::highlight(lang, &code_text, dark);
    let fg = if highlight::is_plain(lang) {
        opts.text_color.unwrap_or_else(|| t.foreground.opacity(0.9))
    } else {
        t.foreground
    };
    let text = StyledText::new(code_text.clone()).with_default_highlights(
        &gpui::TextStyle {
            color: fg,
            font_family: font::MONO.into(),
            font_size: px(font::CODE_SIZE).into(),
            line_height: relative(leading::SNUG),
            ..Default::default()
        },
        highlights.iter().cloned(),
    );
    let wrap_state = state.clone();
    let copy_state = state.clone();
    let copy_text = code_text.clone();
    let toolbar = h_flex()
        .gap(px(2.))
        .child(
            Button::new(SharedString::from(format!("{id}-wrap")))
                .variant(if wrap {
                    ButtonVariant::Secondary
                } else {
                    ButtonVariant::GhostMuted
                })
                .size(ButtonSize::IconXs)
                .icon(IconName::WrapText)
                .icon_size(px(12.))
                .tooltip(
                    Tooltip::text(if wrap { "Disable line wrap" } else { "Wrap lines" })
                        .side(TooltipSide::Top),
                )
                .on_click(move |_, _, cx| {
                    wrap_state.update(cx, |s, cx| {
                        s.wrap = Some(!wrap);
                        cx.notify();
                    })
                }),
        )
        .child(
            Button::new(SharedString::from(format!("{id}-copy")))
                .variant(ButtonVariant::GhostMuted)
                .size(ButtonSize::IconXs)
                .icon(if copied { IconName::Check } else { IconName::Copy })
                .icon_size(px(12.))
                .tooltip(Tooltip::text(if copied { "Copied" } else { "Copy code" }).side(TooltipSide::Top))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                    flash(&copy_state, Duration::from_millis(1200), cx);
                }),
        );
    let (bg, border) = if dark {
        (t.input.opacity(0.32), gpui::transparent_black())
    } else {
        (t.secondary, t.border.opacity(0.7))
    };
    let body = div()
        .id(SharedString::from(format!("{id}-pre")))
        .w_full()
        .px(px(14.4))
        .pt(px(12.8 - 6.))
        .pb(px(12.8))
        .font_family(font::MONO)
        .text_size(px(font::CODE_SIZE))
        .line_height(relative(leading::SNUG))
        .map(|el| {
            if wrap {
                el.child(text)
            } else {
                el.overflow_x_scroll()
                    .child(div().whitespace_nowrap().child(text))
            }
        });
    v_flex()
        .w_full()
        .rounded(radius::LG)
        .border_1()
        .border_color(border)
        .bg(bg)
        .overflow_hidden()
        .child(
            h_flex()
                .justify_between()
                .pt(px(6.))
                .pr(px(6.))
                .pl(px(12.))
                .font_family(font::MONO)
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(header_color)
                .child(label)
                .child(toolbar),
        )
        .child(body)
        .into_any_element()
}

/// Sets `copied` and clears it after `after`.
fn flash(state: &gpui::Entity<CodeState>, after: Duration, cx: &mut App) {
    state.update(cx, |s, cx| {
        s.copied = true;
        cx.notify();
    });
    let state = state.downgrade();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(after).await;
        let _ = state.update(cx, |s, cx| {
            s.copied = false;
            cx.notify();
        });
    })
    .detach();
}

fn cell_text_len(i: &Inline) -> usize {
    i.text.chars().count()
}

fn table(
    align: &[Align],
    header: &[Inline],
    rows: &[Vec<Inline>],
    opts: &Options,
    id: &str,
    cx: &mut App,
) -> AnyElement {
    let t = *cx.tokens();
    let cols = header.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
    // Auto layout: each column grows in proportion to its max-content width, capped
    // at the 384px cell max width.
    let weights: Vec<f32> = (0..cols)
        .map(|c| {
            let chars = std::iter::once(header.get(c))
                .chain(rows.iter().map(|r| r.get(c)))
                .flatten()
                .map(cell_text_len)
                .max()
                .unwrap_or(1);
            (chars as f32 * 7.4 + 12.).clamp(24., 396.)
        })
        .collect();
    let line = t.border.opacity(0.6);
    let cell = |c: usize, content: AnyElement, head: bool| {
        let a = align.get(c).copied().unwrap_or(Align::Left);
        div()
            .flex_grow(weights[c])
            .flex_shrink(1.)
            .flex_basis(px(0.))
            .min_w(px(weights[c].min(80.)))
            .when(c > 0, |el| el.pl(px(12.)))
            .when(c + 1 < cols, |el| el.pr(px(12.)))
            .py(px(if head { 8.8 } else { 7.2 }))
            .flex()
            .map(|el| match a {
                Align::Left => el.justify_start(),
                Align::Center => el.justify_center(),
                Align::Right => el.justify_end(),
            })
            .child(content)
    };
    let mut col = v_flex().w_full().min_w_0();
    let mut head = h_flex().items_start().w_full().border_b_1().border_color(line);
    for c in 0..cols {
        let content = header
            .get(c)
            .map(|i| render_inline(i, opts, &format!("{id}-h{c}"), Some(FontWeight::SEMIBOLD), cx))
            .unwrap_or_else(|| div().into_any_element());
        head = head.child(cell(c, content, true));
    }
    col = col.child(head.font_weight(FontWeight::SEMIBOLD));
    for (ri, row) in rows.iter().enumerate() {
        let mut r = h_flex().items_start().w_full().border_b_1().border_color(line);
        for c in 0..cols {
            let content = row
                .get(c)
                .map(|i| render_inline(i, opts, &format!("{id}-r{ri}c{c}"), None, cx))
                .unwrap_or_else(|| div().into_any_element());
            r = r.child(cell(c, content, false));
        }
        col = col.child(r);
    }
    let md = table_markdown(header, rows);
    let csv = table_csv(header, rows);
    let html = table_html(header, rows);
    let menu_id = SharedString::from(format!("{id}-copy-table"));
    let menu = move |_: &mut Window, _: &mut App| {
        let (md, csv, html) = (md.clone(), csv.clone(), html.clone());
        Menu::new()
            .item(MenuItem::new("Copy as Markdown").on_select(move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(md.clone()))
            }))
            .item(MenuItem::new("Copy as CSV").on_select(move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(csv.clone()))
            }))
            .item(MenuItem::new("Copy as HTML").on_select(move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(html.clone()))
            }))
    };
    let trigger_id = SharedString::from(format!("{id}-copy-table-btn"));
    v_flex()
        .w_full()
        .min_w_0()
        .child(
            div()
                .id(SharedString::from(format!("{id}-table")))
                .w_full()
                .overflow_x_scroll()
                .child(col),
        )
        .child(
            h_flex().w_full().justify_end().mt(px(2.)).child(
                DropdownMenu::new(menu_id, menu, move |open| {
                    Button::new(trigger_id)
                        .variant(ButtonVariant::GhostMuted)
                        .size(ButtonSize::IconXs)
                        .icon(IconName::Copy)
                        .icon_size(px(12.))
                        .pressed(open)
                        .tooltip(Tooltip::text("Copy table").side(TooltipSide::Top))
                        .into_any_element()
                })
                .align(gpui_base::Align::End),
            ),
        )
        .into_any_element()
}

fn table_markdown(header: &[Inline], rows: &[Vec<Inline>]) -> String {
    let line = |cells: &[Inline]| {
        format!(
            "| {} |",
            cells
                .iter()
                .map(|c| c.text.replace('|', "\\|"))
                .collect::<Vec<_>>()
                .join(" | ")
        )
    };
    let mut out = vec![
        line(header),
        format!("|{}|", vec![" --- "; header.len()].join("|")),
    ];
    out.extend(rows.iter().map(|r| line(r)));
    out.join("\n")
}

fn table_csv(header: &[Inline], rows: &[Vec<Inline>]) -> String {
    let field = |s: &str| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    std::iter::once(header)
        .chain(rows.iter().map(Vec::as_slice))
        .map(|r| r.iter().map(|c| field(&c.text)).collect::<Vec<_>>().join(","))
        .collect::<Vec<_>>()
        .join("\n")
}

fn table_html(header: &[Inline], rows: &[Vec<Inline>]) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut out = String::from("<table><thead><tr>");
    for h in header {
        out.push_str(&format!("<th>{}</th>", esc(&h.text)));
    }
    out.push_str("</tr></thead><tbody>");
    for r in rows {
        out.push_str("<tr>");
        for c in r {
            out.push_str(&format!("<td>{}</td>", esc(&c.text)));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    out
}

/// Plain text of a document for copy and find.
pub fn plain_text(doc: &Document) -> String {
    fn walk(blocks: &[Block], out: &mut String) {
        for b in blocks {
            match b {
                Block::Paragraph(i) | Block::Heading { text: i, .. } => {
                    out.push_str(&i.text);
                    out.push('\n');
                }
                Block::List { items, .. } => {
                    for item in items {
                        walk(&item.blocks, out);
                    }
                }
                Block::Code { code, .. } => {
                    out.push_str(code);
                    out.push('\n');
                }
                Block::Quote(b) | Block::Alert { blocks: b, .. } => walk(b, out),
                Block::Table { header, rows, .. } => {
                    for c in header.iter().chain(rows.iter().flatten()) {
                        out.push_str(&c.text);
                        out.push(' ');
                    }
                    out.push('\n');
                }
                Block::Rule => {}
                Block::Footnotes(n) => {
                    for (_, b) in n {
                        walk(b, out);
                    }
                }
            }
        }
    }
    let mut out = String::new();
    walk(&doc.blocks, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers() {
        assert_eq!(ordered_marker(3, 0), "3.");
        assert_eq!(ordered_marker(1, 1), "a.");
        assert_eq!(ordered_marker(28, 1), "ab.");
        assert_eq!(ordered_marker(4, 2), "iv.");
    }

    #[test]
    fn table_exports() {
        let h = vec![
            Inline {
                text: "File".into(),
                spans: vec![],
            },
            Inline {
                text: "Note".into(),
                spans: vec![],
            },
        ];
        let r = vec![vec![
            Inline {
                text: "a.py".into(),
                spans: vec![],
            },
            Inline {
                text: "x, y".into(),
                spans: vec![],
            },
        ]];
        assert_eq!(table_markdown(&h, &r), "| File | Note |\n| --- | --- |\n| a.py | x, y |");
        assert_eq!(table_csv(&h, &r), "File,Note\na.py,\"x, y\"");
    }
}

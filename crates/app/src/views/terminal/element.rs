//! The terminal grid as one GPUI element (spec 05 §3.9), drawn like Zed's
//! `terminal_element`: cell backgrounds merged into quads per row, text shaped in
//! runs per row with every glyph forced onto the cell grid, then decorations, the
//! cursor and the overlay scrollbar. Block elements, box lines and the powerline
//! triangles are drawn as geometry so they tile without gaps, as Ghostty does.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, Bounds, ContentMask, Element, ElementId, Font, FontFeatures, FontStyle, FontWeight,
    GlobalElementId, Hsla, InspectorElementId, LayoutId, PathBuilder, Pixels, Point,
    ShapedLine, SharedString, Style, TextAlign, TextRun, Window, fill, font, outline, point,
    px, relative, size,
};
use tern_terminal::{CursorShape, Terminal, TerminalSnapshot};

use super::colors::TermColors;
use super::layout::{self, CellSize, GridLayout};

/// Glyph fallbacks after the code font (spec 05 §3.9).
pub const FALLBACKS: &[&str] = &[
    "Symbols Nerd Font Mono",
    "Symbols Nerd Font",
    "JetBrainsMono Nerd Font",
    "JetBrainsMono NF",
    "FiraCode Nerd Font",
    "Hack Nerd Font",
    "MesloLGS NF",
    "CaskaydiaCove Nerd Font",
    "PowerlineSymbols",
    "DejaVu Sans Mono",
    "Noto Color Emoji",
];

/// Everything the element needs for one frame.
#[derive(Clone)]
pub struct TerminalFrame {
    pub terminal: Option<Arc<Terminal>>,
    pub snapshot: Arc<TerminalSnapshot>,
    pub colors: TermColors,
    pub font_family: SharedString,
    pub font_size: Pixels,
    pub focused: bool,
    /// Blink phase; the cursor hides while false (only when focused and blinking).
    pub cursor_on: bool,
    /// Viewport cells, inclusive, start before end.
    pub selection: Option<((u16, u16), (u16, u16))>,
    /// Hovered link, end exclusive.
    pub link: Option<((u16, u16), (u16, u16))>,
    pub scrollbar_thumb: Hsla,
    pub scrollbar_thumb_hover: Hsla,
    pub scrollbar_hover: bool,
    /// Receives the grid placement each frame (mouse mapping).
    pub layout_sink: Rc<Cell<GridLayout>>,
}

pub struct TerminalElement {
    frame: TerminalFrame,
}

impl TerminalElement {
    pub fn new(frame: TerminalFrame) -> Self {
        Self { frame }
    }
}

/// Cell metrics for a font, measured once per family and size.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub cell: CellSize,
    pub ascent: f32,
    pub descent: f32,
}

pub fn terminal_font(family: &SharedString, weight: FontWeight, style: FontStyle) -> Font {
    let mut f = font(family.clone());
    f.weight = weight;
    f.style = style;
    f.features = FontFeatures::disable_ligatures();
    f.fallbacks = Some(gpui::FontFallbacks::from_fonts(
        FALLBACKS.iter().map(|s| s.to_string()).collect(),
    ));
    f
}

thread_local! {
    static METRICS: std::cell::RefCell<Vec<(SharedString, f32, Metrics)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The cell size: advance of `M` wide, `max(round(size × 1.35), ceil(ascent+descent))`
/// tall.
pub fn measure(family: &SharedString, font_size: Pixels, window: &Window) -> Metrics {
    let size_f = f32::from(font_size);
    if let Some(m) = METRICS.with(|c| {
        c.borrow()
            .iter()
            .find(|(f, s, _)| f == family && *s == size_f)
            .map(|(_, _, m)| *m)
    }) {
        return m;
    }
    let ts = window.text_system();
    let font_id = ts.resolve_font(&terminal_font(family, FontWeight::NORMAL, FontStyle::Normal));
    let width = ts
        .advance(font_id, font_size, 'M')
        .map(|s| f32::from(s.width))
        .unwrap_or(size_f * 0.6);
    let ascent = f32::from(ts.ascent(font_id, font_size));
    let descent = f32::from(ts.descent(font_id, font_size)).abs();
    let m = Metrics {
        cell: CellSize {
            width,
            height: layout::line_height(size_f, ascent, descent),
        },
        ascent,
        descent,
    };
    METRICS.with(|c| c.borrow_mut().push((family.clone(), size_f, m)));
    m
}

/// Combining marks, variation selectors and joiners attach to the previous cell.
pub fn is_zero_width(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f
        | 0x200b..=0x200f | 0xfe00..=0xfe0f | 0xe0100..=0xe01ef)
}

/// Shapes drawn as geometry instead of glyphs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sprite {
    /// Fill a fraction of the cell: (x0, y0, x1, y1) in 0..=1, with alpha.
    Rect(f32, f32, f32, f32, f32),
    /// Box line arms from the center: up, right, down, left; heavy = 2px.
    Lines {
        up: bool,
        right: bool,
        down: bool,
        left: bool,
        heavy: bool,
    },
    PowerlineRight { solid: bool },
    PowerlineLeft { solid: bool },
}

pub fn sprite_for(c: char) -> Option<Sprite> {
    let lines = |up, right, down, left, heavy| {
        Some(Sprite::Lines {
            up,
            right,
            down,
            left,
            heavy,
        })
    };
    let eighth = |n: u32| n as f32 / 8.;
    match c as u32 {
        0x2500 => lines(false, true, false, true, false),
        0x2501 => lines(false, true, false, true, true),
        0x2502 => lines(true, false, true, false, false),
        0x2503 => lines(true, false, true, false, true),
        0x250c | 0x256d => lines(false, true, true, false, false),
        0x2510 | 0x256e => lines(false, false, true, true, false),
        0x2514 | 0x2570 => lines(true, true, false, false, false),
        0x2518 | 0x256f => lines(true, false, false, true, false),
        0x251c => lines(true, true, true, false, false),
        0x2524 => lines(true, false, true, true, false),
        0x252c => lines(false, true, true, true, false),
        0x2534 => lines(true, true, false, true, false),
        0x253c => lines(true, true, true, true, false),
        0x2574 => lines(false, false, false, true, false),
        0x2575 => lines(true, false, false, false, false),
        0x2576 => lines(false, true, false, false, false),
        0x2577 => lines(false, false, true, false, false),
        0x2580 => Some(Sprite::Rect(0., 0., 1., 0.5, 1.)),
        n @ 0x2581..=0x2587 => Some(Sprite::Rect(0., 1. - eighth(n - 0x2580), 1., 1., 1.)),
        0x2588 => Some(Sprite::Rect(0., 0., 1., 1., 1.)),
        n @ 0x2589..=0x258f => Some(Sprite::Rect(0., 0., eighth(0x2590 - n), 1., 1.)),
        0x2590 => Some(Sprite::Rect(0.5, 0., 1., 1., 1.)),
        0x2591 => Some(Sprite::Rect(0., 0., 1., 1., 0.25)),
        0x2592 => Some(Sprite::Rect(0., 0., 1., 1., 0.5)),
        0x2593 => Some(Sprite::Rect(0., 0., 1., 1., 0.75)),
        0x2594 => Some(Sprite::Rect(0., 0., 1., eighth(1), 1.)),
        0x2595 => Some(Sprite::Rect(1. - eighth(1), 0., 1., 1., 1.)),
        0xe0b0 => Some(Sprite::PowerlineRight { solid: true }),
        0xe0b1 => Some(Sprite::PowerlineRight { solid: false }),
        0xe0b2 => Some(Sprite::PowerlineLeft { solid: true }),
        0xe0b3 => Some(Sprite::PowerlineLeft { solid: false }),
        _ => None,
    }
}

/// A run of text drawn with one style starting at a column.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub col: u16,
    pub text: String,
    pub fg: Hsla,
    pub bold: bool,
    pub italic: bool,
    /// Wide glyphs aren't forced onto single cells.
    pub wide: bool,
}

/// A span of cells on a row: [start, end) columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub start: u16,
    pub end: u16,
    pub color: Hsla,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecorationKind {
    Underline,
    Strike,
}

/// One row, laid out into draw operations.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowOps {
    pub backgrounds: Vec<Span>,
    pub batches: Vec<Batch>,
    pub sprites: Vec<(u16, Sprite, Hsla)>,
    pub decorations: Vec<(DecorationKind, Span)>,
}

fn push_span(spans: &mut Vec<Span>, start: u16, end: u16, color: Hsla) {
    match spans.last_mut() {
        Some(last) if last.end == start && last.color == color => last.end = end,
        _ => spans.push(Span { start, end, color }),
    }
}

/// Lays out one snapshot line. `cursor_col` asks for the char under the cursor.
pub fn layout_row(
    line: &tern_terminal::TerminalLine,
    colors: &TermColors,
    cursor_col: Option<u16>,
) -> (RowOps, Option<(String, bool, bool, bool)>) {
    let mut ops = RowOps::default();
    let mut col: u16 = 0;
    let mut wide_ix = 0;
    let mut under_cursor = None;
    for (text, style) in &line.runs {
        let (fg, bg) = colors.cell(style);
        for c in text.chars() {
            if is_zero_width(c) {
                if let Some(b) = ops.batches.last_mut()
                    && b.fg == fg
                {
                    b.text.push(c);
                }
                continue;
            }
            while wide_ix < line.wide.len() && line.wide[wide_ix] < col {
                wide_ix += 1;
            }
            let wide = line.wide.get(wide_ix) == Some(&col);
            let width: u16 = if wide { 2 } else { 1 };
            if let Some(bg) = bg {
                push_span(&mut ops.backgrounds, col, col + width, bg);
            }
            if style.underline {
                match ops.decorations.last_mut() {
                    Some((DecorationKind::Underline, s)) if s.end == col && s.color == fg => {
                        s.end = col + width
                    }
                    _ => ops.decorations.push((
                        DecorationKind::Underline,
                        Span {
                            start: col,
                            end: col + width,
                            color: fg,
                        },
                    )),
                }
            }
            if style.strikethrough {
                ops.decorations.push((
                    DecorationKind::Strike,
                    Span {
                        start: col,
                        end: col + width,
                        color: fg,
                    },
                ));
            }
            if cursor_col == Some(col) && c != ' ' {
                under_cursor = Some((c.to_string(), style.bold, style.italic, wide));
            }
            if let Some(sprite) = sprite_for(c) {
                ops.sprites.push((col, sprite, fg));
            } else if c != ' ' {
                let joins = match ops.batches.last() {
                    Some(b) => {
                        !wide
                            && !b.wide
                            && b.fg == fg
                            && b.bold == style.bold
                            && b.italic == style.italic
                            && b.col as usize + b.text.chars().filter(|c| !is_zero_width(*c)).count()
                                == col as usize
                    }
                    None => false,
                };
                if joins {
                    ops.batches.last_mut().unwrap().text.push(c);
                } else {
                    ops.batches.push(Batch {
                        col,
                        text: c.to_string(),
                        fg,
                        bold: style.bold,
                        italic: style.italic,
                        wide,
                    });
                }
            } else if let Some(b) = ops.batches.last_mut()
                && !b.wide
                && b.fg == fg
                && b.bold == style.bold
                && b.italic == style.italic
                && b.col as usize + b.text.chars().filter(|c| !is_zero_width(*c)).count() == col as usize
            {
                // Inner spaces keep a batch going (fewer shaping calls).
                b.text.push(' ');
            }
            col += width;
        }
    }
    for b in &mut ops.batches {
        let trimmed = b.text.trim_end_matches(' ').len();
        b.text.truncate(trimmed);
    }
    (ops, under_cursor)
}

pub struct Prepaint {
    origin: Point<Pixels>,
    metrics: Metrics,
    rows: Vec<RowOps>,
    shaped: Vec<(usize, u16, ShapedLine)>,
    cursor: Option<CursorPaint>,
    background: Hsla,
}

struct CursorPaint {
    bounds: Bounds<Pixels>,
    shape: CursorShape,
    hollow: bool,
    color: Hsla,
    glyph: Option<ShapedLine>,
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) -> Prepaint {
        let f = &self.frame;
        let metrics = measure(&f.font_family, f.font_size, window);
        let cell = metrics.cell;
        let (cols, rows) = layout::grid_size(
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
            cell,
        );
        if let Some(term) = &f.terminal
            && (cols != f.snapshot.cols || rows != f.snapshot.rows)
        {
            term.set_cell_size(cell.width.round() as u16, cell.height.round() as u16);
            term.resize(cols, rows);
        }
        let (ox, oy) = layout::content_origin();
        let origin = bounds.origin + point(px(ox), px(oy));
        f.layout_sink.set(GridLayout {
            origin_x: f32::from(origin.x),
            origin_y: f32::from(origin.y),
            cell,
            cols: f.snapshot.cols.min(cols),
            rows: f.snapshot.rows.min(rows),
            pane_x: f32::from(bounds.origin.x),
            pane_y: f32::from(bounds.origin.y),
            pane_width: f32::from(bounds.size.width),
            pane_height: f32::from(bounds.size.height),
        });

        let cursor = f.snapshot.cursor;
        let mut under_cursor = None;
        let mut row_ops = Vec::with_capacity(f.snapshot.lines.len());
        for (r, line) in f.snapshot.lines.iter().enumerate() {
            let ccol = cursor.filter(|c| c.1 as usize == r).map(|c| c.0);
            let (ops, uc) = layout_row(line, &f.colors, ccol);
            if uc.is_some() {
                under_cursor = uc;
            }
            row_ops.push(ops);
        }

        let ts = window.text_system().clone();
        let mut shaped = Vec::new();
        for (r, ops) in row_ops.iter().enumerate() {
            for b in &ops.batches {
                if b.text.is_empty() {
                    continue;
                }
                let weight = if b.bold { FontWeight::BOLD } else { FontWeight::NORMAL };
                let style = if b.italic { FontStyle::Italic } else { FontStyle::Normal };
                let run = TextRun {
                    len: b.text.len(),
                    font: terminal_font(&f.font_family, weight, style),
                    color: b.fg,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let force = (!b.wide).then_some(px(cell.width));
                let line = ts.shape_line(SharedString::from(b.text.clone()), f.font_size, &[run], force);
                shaped.push((r, b.col, line));
            }
        }

        let cursor_paint = cursor.and_then(|(ccol, crow)| {
            let blinking_off = f.focused && f.snapshot.cursor_blinking && !f.cursor_on;
            if blinking_off {
                return None;
            }
            let wide = f
                .snapshot
                .lines
                .get(crow as usize)
                .is_some_and(|l| l.wide.contains(&ccol));
            let w = cell.width * if wide { 2. } else { 1. };
            let cb = Bounds::new(
                origin + point(px(ccol as f32 * cell.width), px(crow as f32 * cell.height)),
                size(px(w), px(cell.height)),
            );
            let shape = f.snapshot.cursor_shape;
            let hollow = !f.focused || shape == CursorShape::HollowBlock;
            let glyph = (!hollow && shape == CursorShape::Block)
                .then_some(())
                .and(under_cursor.clone())
                .map(|(text, bold, italic, wide)| {
                    let run = TextRun {
                        len: text.len(),
                        font: terminal_font(
                            &f.font_family,
                            if bold { FontWeight::BOLD } else { FontWeight::NORMAL },
                            if italic { FontStyle::Italic } else { FontStyle::Normal },
                        ),
                        color: f.colors.background,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    ts.shape_line(
                        SharedString::from(text),
                        f.font_size,
                        &[run],
                        (!wide).then_some(px(cell.width)),
                    )
                });
            Some(CursorPaint {
                bounds: cb,
                shape,
                hollow,
                color: f.colors.cursor,
                glyph,
            })
        });

        Prepaint {
            origin,
            metrics,
            rows: row_ops,
            shaped,
            cursor: cursor_paint,
            background: f.colors.background,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        pre: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let f = &self.frame;
        let cell = pre.metrics.cell;
        let origin = pre.origin;
        let cw = cell.width;
        let ch = cell.height;
        let cell_rect = |col: u16, row: usize, cols: f32| {
            Bounds::new(
                origin + point(px(col as f32 * cw), px(row as f32 * ch)),
                size(px(cols * cw), px(ch)),
            )
        };
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, pre.background));

            for (r, ops) in pre.rows.iter().enumerate() {
                for s in &ops.backgrounds {
                    window.paint_quad(fill(cell_rect(s.start, r, (s.end - s.start) as f32), s.color));
                }
            }

            if let Some(((sc, sr), (ec, er))) = f.selection {
                let last_col = f.snapshot.cols.saturating_sub(1);
                for r in sr..=er {
                    let a = if r == sr { sc } else { 0 };
                    let b = if r == er { ec } else { last_col };
                    if b >= a {
                        window.paint_quad(fill(
                            cell_rect(a, r as usize, (b - a + 1) as f32),
                            f.colors.selection,
                        ));
                    }
                }
            }

            for (r, ops) in pre.rows.iter().enumerate() {
                for &(col, sprite, color) in &ops.sprites {
                    paint_sprite(window, cell_rect(col, r, 1.), sprite, color);
                }
            }

            for (r, col, line) in &pre.shaped {
                let at = origin + point(px(*col as f32 * cw), px(*r as f32 * ch));
                let _ = line.paint(at, px(ch), TextAlign::Left, None, window, cx);
            }

            for (r, ops) in pre.rows.iter().enumerate() {
                for (kind, span) in &ops.decorations {
                    let rect = cell_rect(span.start, r, (span.end - span.start) as f32);
                    let y = match kind {
                        DecorationKind::Underline => ch - 2.,
                        DecorationKind::Strike => (ch * 0.55).floor(),
                    };
                    window.paint_quad(fill(
                        Bounds::new(rect.origin + point(px(0.), px(y)), size(rect.size.width, px(1.))),
                        span.color,
                    ));
                }
            }

            if let Some(((sc, sr), (ec, er))) = f.link {
                for r in sr..=er {
                    let a = if r == sr { sc } else { 0 };
                    let b = if r == er { ec } else { f.snapshot.cols };
                    if b > a {
                        let rect = cell_rect(a, r as usize, (b - a) as f32);
                        window.paint_quad(fill(
                            Bounds::new(
                                rect.origin + point(px(0.), px(ch - 2.)),
                                size(rect.size.width, px(1.)),
                            ),
                            f.colors.foreground,
                        ));
                    }
                }
            }

            if let Some(c) = &pre.cursor {
                let b = c.bounds;
                if c.hollow {
                    window.paint_quad(outline(b, c.color, gpui::BorderStyle::Solid));
                } else {
                    match c.shape {
                        CursorShape::Bar => window.paint_quad(fill(
                            Bounds::new(b.origin, size(px(2.), b.size.height)),
                            c.color,
                        )),
                        CursorShape::Underline => window.paint_quad(fill(
                            Bounds::new(
                                b.origin + point(px(0.), b.size.height - px(2.)),
                                size(b.size.width, px(2.)),
                            ),
                            c.color,
                        )),
                        _ => {
                            window.paint_quad(fill(b, c.color));
                            if let Some(glyph) = &c.glyph {
                                let _ = glyph.paint(b.origin, px(ch), TextAlign::Left, None, window, cx);
                            }
                        }
                    }
                }
            }

            let track = f32::from(bounds.size.height) - 2. * layout::SCROLLBAR_INSET_Y;
            if let Some((top, height)) = layout::scrollbar_thumb(
                track,
                f.snapshot.rows as usize,
                f.snapshot.history_size,
                f.snapshot.display_offset,
            ) {
                let x = bounds.origin.x + bounds.size.width
                    - px(layout::SCROLLBAR_INSET_RIGHT + layout::SCROLLBAR_WIDTH - layout::SCROLLBAR_THUMB_INSET_X);
                let thumb = Bounds::new(
                    point(x, bounds.origin.y + px(layout::SCROLLBAR_INSET_Y + top)),
                    size(
                        px(layout::SCROLLBAR_WIDTH - 2. * layout::SCROLLBAR_THUMB_INSET_X),
                        px(height),
                    ),
                );
                let color = if f.scrollbar_hover {
                    f.scrollbar_thumb_hover
                } else {
                    f.scrollbar_thumb
                };
                window.paint_quad(
                    fill(thumb, color).corner_radii(px(layout::SCROLLBAR_THUMB_RADIUS)),
                );
            }
        });
    }
}

fn paint_sprite(window: &mut Window, cell: Bounds<Pixels>, sprite: Sprite, color: Hsla) {
    let o = cell.origin;
    let (w, h) = (f32::from(cell.size.width), f32::from(cell.size.height));
    match sprite {
        Sprite::Rect(x0, y0, x1, y1, alpha) => {
            let r = Bounds::new(
                o + point(px((x0 * w).round()), px((y0 * h).round())),
                size(px(((x1 - x0) * w).round()), px(((y1 - y0) * h).round())),
            );
            window.paint_quad(fill(r, color.opacity(alpha)));
        }
        Sprite::Lines {
            up,
            right,
            down,
            left,
            heavy,
        } => {
            let t = if heavy { 2. } else { 1. };
            let cx = (w / 2.).floor();
            let cy = (h / 2.).floor();
            let mut q = |x: f32, y: f32, ww: f32, hh: f32| {
                window.paint_quad(fill(Bounds::new(o + point(px(x), px(y)), size(px(ww), px(hh))), color));
            };
            if left {
                q(0., cy, cx + t, t);
            }
            if right {
                q(cx, cy, w - cx, t);
            }
            if up {
                q(cx, 0., t, cy + t);
            }
            if down {
                q(cx, cy, t, h - cy);
            }
        }
        Sprite::PowerlineRight { solid } | Sprite::PowerlineLeft { solid } => {
            let right = matches!(sprite, Sprite::PowerlineRight { .. });
            let (tip, base) = if right { (w, 0.) } else { (0., w) };
            let p = |x: f32, y: f32| o + point(px(x), px(y));
            let path = if solid {
                let mut b = PathBuilder::fill();
                b.move_to(p(base, 0.));
                b.line_to(p(tip, h / 2.));
                b.line_to(p(base, h));
                b.close();
                b.build()
            } else {
                let mut b = PathBuilder::stroke(px(1.));
                b.move_to(p(base, 0.));
                b.line_to(p(tip, h / 2.));
                b.line_to(p(base, h));
                b.build()
            };
            if let Ok(path) = path {
                window.paint_path(path, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::Palette;
    use tern_terminal::{CellStyle, TerminalLine};

    fn colors() -> TermColors {
        TermColors::new(&Palette::dark(), true)
    }

    fn line(runs: &[(&str, CellStyle)], wide: &[u16]) -> TerminalLine {
        TerminalLine {
            runs: runs.iter().map(|(t, s)| (t.to_string(), *s)).collect(),
            wide: wide.to_vec(),
            ..Default::default()
        }
    }

    #[test]
    fn batches_split_on_style_and_skip_spaces() {
        let green = CellStyle {
            fg: Some(2),
            bold: true,
            ..Default::default()
        };
        let l = line(&[("$ ", green), ("ls  -la", CellStyle::default())], &[]);
        let (ops, _) = layout_row(&l, &colors(), None);
        let texts: Vec<(&str, u16)> = ops.batches.iter().map(|b| (b.text.as_str(), b.col)).collect();
        assert_eq!(texts, [("$", 0), ("ls  -la", 2)]);
        assert!(ops.batches[0].bold);
        assert!(ops.backgrounds.is_empty());
    }

    #[test]
    fn backgrounds_merge() {
        let red = CellStyle {
            bg: Some(1),
            ..Default::default()
        };
        let red_bold = CellStyle { bold: true, ..red };
        let l = line(&[("ab", red), ("cd", red_bold), ("e", CellStyle::default())], &[]);
        let (ops, _) = layout_row(&l, &colors(), None);
        assert_eq!(ops.backgrounds.len(), 1);
        assert_eq!((ops.backgrounds[0].start, ops.backgrounds[0].end), (0, 4));
    }

    #[test]
    fn wide_chars_take_two_cells() {
        let l = line(&[("a漢b", CellStyle::default())], &[1]);
        let (ops, under) = layout_row(&l, &colors(), Some(1));
        let cols: Vec<(String, u16, bool)> = ops
            .batches
            .iter()
            .map(|b| (b.text.clone(), b.col, b.wide))
            .collect();
        assert_eq!(
            cols,
            [("a".into(), 0, false), ("漢".into(), 1, true), ("b".into(), 3, false)]
        );
        assert_eq!(under, Some(("漢".into(), false, false, true)));
    }

    #[test]
    fn combining_marks_stay_in_cell() {
        let l = line(&[("e\u{301}x", CellStyle::default())], &[]);
        let (ops, _) = layout_row(&l, &colors(), None);
        assert_eq!(ops.batches.len(), 1);
        assert_eq!(ops.batches[0].text, "e\u{301}x");
    }

    #[test]
    fn sprites_and_decorations() {
        let u = CellStyle {
            underline: true,
            ..Default::default()
        };
        let l = line(&[("─█", CellStyle::default()), ("ab", u)], &[]);
        let (ops, _) = layout_row(&l, &colors(), None);
        assert_eq!(ops.sprites.len(), 2);
        assert_eq!(ops.sprites[1].1, Sprite::Rect(0., 0., 1., 1., 1.));
        assert_eq!(ops.decorations.len(), 1);
        assert_eq!((ops.decorations[0].1.start, ops.decorations[0].1.end), (2, 4));
        assert_eq!(ops.batches[0].text, "ab");
    }

    #[test]
    fn sprite_table() {
        assert_eq!(sprite_for('▄'), Some(Sprite::Rect(0., 0.5, 1., 1., 1.)));
        assert_eq!(sprite_for('▌'), Some(Sprite::Rect(0., 0., 0.5, 1., 1.)));
        assert_eq!(sprite_for('a'), None);
        assert!(matches!(sprite_for('┼'), Some(Sprite::Lines { up: true, right: true, down: true, left: true, heavy: false })));
    }
}

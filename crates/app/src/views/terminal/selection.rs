//! Mouse selection in a terminal (spec 05 §3.8, §3.12). Points are kept in absolute
//! rows (counted from the top of the scrollback) so a selection stays on its text while
//! the viewport scrolls; they're mapped back to viewport cells to draw and copy.

use super::layout::Side;

/// (col, absolute row).
pub type AbsPoint = (u16, i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    Simple,
    Word,
    Line,
}

/// Scroll state that maps viewport rows to absolute rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scroll {
    pub history: usize,
    pub display_offset: usize,
    pub rows: u16,
    pub cols: u16,
}

impl Scroll {
    pub fn to_abs(&self, (col, row): (u16, u16)) -> AbsPoint {
        (
            col,
            row as i64 - self.display_offset as i64 + self.history as i64,
        )
    }

    /// The viewport row of an absolute row (may be off screen).
    pub fn to_viewport_row(&self, abs_row: i64) -> i64 {
        abs_row - self.history as i64 + self.display_offset as i64
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub anchor: AbsPoint,
    pub anchor_side: Side,
    pub head: AbsPoint,
    pub head_side: Side,
    pub mode: SelectMode,
    /// "Select all": the whole scrollback and screen.
    pub all: bool,
}

fn before(a: AbsPoint, b: AbsPoint) -> bool {
    (a.1, a.0) < (b.1, b.0)
}

fn step_right(p: AbsPoint, cols: u16) -> AbsPoint {
    if p.0 + 1 >= cols {
        (0, p.1 + 1)
    } else {
        (p.0 + 1, p.1)
    }
}

fn step_left(p: AbsPoint, cols: u16) -> AbsPoint {
    if p.0 == 0 {
        (cols.saturating_sub(1), p.1 - 1)
    } else {
        (p.0 - 1, p.1)
    }
}

impl Selection {
    pub fn new(anchor: AbsPoint, side: Side, mode: SelectMode) -> Self {
        Self {
            anchor,
            anchor_side: side,
            head: anchor,
            head_side: side,
            mode,
            all: false,
        }
    }

    pub fn all(scroll: Scroll) -> Self {
        let last = (
            scroll.cols.saturating_sub(1),
            scroll.history as i64 + scroll.rows as i64 - 1,
        );
        Self {
            anchor: (0, 0),
            anchor_side: Side::Left,
            head: last,
            head_side: Side::Right,
            mode: SelectMode::Simple,
            all: true,
        }
    }

    /// The selected cells as an inclusive absolute range, or None when a simple drag
    /// hasn't crossed a cell's middle yet.
    pub fn range(&self, cols: u16) -> Option<(AbsPoint, AbsPoint)> {
        if self.mode != SelectMode::Simple || self.all {
            let (a, b) = if before(self.head, self.anchor) {
                (self.head, self.anchor)
            } else {
                (self.anchor, self.head)
            };
            return Some((a, b));
        }
        let (mut start, start_side, mut end, end_side) = if before(self.head, self.anchor) {
            (self.head, self.head_side, self.anchor, self.anchor_side)
        } else {
            (self.anchor, self.anchor_side, self.head, self.head_side)
        };
        if start == end && start_side == end_side {
            return None;
        }
        if start_side == Side::Right {
            start = step_right(start, cols);
        }
        if end_side == Side::Left {
            end = step_left(end, cols);
        }
        if before(end, start) {
            return None;
        }
        Some((start, end))
    }

    /// The range clipped to the viewport, in viewport cells.
    pub fn viewport_range(&self, scroll: Scroll) -> Option<((u16, u16), (u16, u16))> {
        let (a, b) = self.range(scroll.cols)?;
        clip_to_viewport(a, b, scroll)
    }

    /// Whether the whole range is on screen (copy only reads the viewport).
    pub fn fully_visible(&self, scroll: Scroll) -> bool {
        let Some((a, b)) = self.range(scroll.cols) else {
            return false;
        };
        let (ra, rb) = (scroll.to_viewport_row(a.1), scroll.to_viewport_row(b.1));
        ra >= 0 && rb < scroll.rows as i64
    }

    /// Cell count, for the "clear on output" size limit.
    pub fn cell_count(&self, cols: u16) -> usize {
        match self.range(cols) {
            Some((a, b)) => {
                let rows = (b.1 - a.1) as usize;
                rows * cols as usize + b.0 as usize + 1 - a.0 as usize
            }
            None => 0,
        }
    }
}

pub fn clip_to_viewport(
    a: AbsPoint,
    b: AbsPoint,
    scroll: Scroll,
) -> Option<((u16, u16), (u16, u16))> {
    let rows = scroll.rows as i64;
    let (ra, rb) = (scroll.to_viewport_row(a.1), scroll.to_viewport_row(b.1));
    if rb < 0 || ra >= rows {
        return None;
    }
    let start = if ra < 0 { (0, 0) } else { (a.0, ra as u16) };
    let end = if rb >= rows {
        (scroll.cols.saturating_sub(1), (rows - 1) as u16)
    } else {
        (b.0, rb as u16)
    };
    Some((start, end))
}

/// "Add to chat" text: CRLF to LF, leading and trailing blank lines dropped.
pub fn chat_text(raw: &str) -> String {
    let text = raw.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let first = lines.iter().position(|l| !l.trim().is_empty());
    let last = lines.iter().rposition(|l| !l.trim().is_empty());
    match (first, last) {
        (Some(a), Some(b)) => lines[a..=b].join("\n"),
        _ => String::new(),
    }
}

/// The context chip label: `{label} line N` or `{label} lines A-B`.
pub fn chat_chip_label(terminal_label: &str, line_start: u32, line_end: u32) -> String {
    if line_start == line_end {
        format!("{terminal_label} line {line_start}")
    } else {
        format!("{terminal_label} lines {line_start}-{line_end}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCROLL: Scroll = Scroll {
        history: 100,
        display_offset: 0,
        rows: 10,
        cols: 20,
    };

    #[test]
    fn abs_round_trip() {
        assert_eq!(SCROLL.to_abs((3, 0)), (3, 100));
        let scrolled = Scroll {
            display_offset: 5,
            ..SCROLL
        };
        assert_eq!(scrolled.to_abs((3, 0)), (3, 95));
        assert_eq!(scrolled.to_viewport_row(100), 5);
    }

    #[test]
    fn simple_needs_half_cell() {
        let mut s = Selection::new((5, 100), Side::Left, SelectMode::Simple);
        assert_eq!(s.range(20), None);
        s.head_side = Side::Right;
        assert_eq!(s.range(20), Some(((5, 100), (5, 100))));
        // Dragging left from the right half of a cell excludes it.
        let mut s = Selection::new((5, 100), Side::Right, SelectMode::Simple);
        s.head = (2, 100);
        s.head_side = Side::Left;
        assert_eq!(s.range(20), Some(((2, 100), (5, 100))));
        s.head_side = Side::Right;
        assert_eq!(s.range(20), Some(((3, 100), (5, 100))));
    }

    #[test]
    fn wraps_across_rows() {
        let mut s = Selection::new((19, 100), Side::Right, SelectMode::Simple);
        s.head = (0, 102);
        s.head_side = Side::Left;
        assert_eq!(s.range(20), Some(((0, 101), (19, 101))));
    }

    #[test]
    fn clipping() {
        let s = Selection {
            head: (4, 105),
            head_side: Side::Right,
            ..Selection::new((2, 95), Side::Left, SelectMode::Simple)
        };
        assert_eq!(s.viewport_range(SCROLL), Some(((0, 0), (4, 5))));
        assert!(!s.fully_visible(SCROLL));
        let off = Selection {
            head: (4, 85),
            head_side: Side::Right,
            ..Selection::new((2, 80), Side::Left, SelectMode::Simple)
        };
        assert_eq!(off.viewport_range(SCROLL), None);
        let scrolled = Scroll {
            display_offset: 20,
            ..SCROLL
        };
        assert_eq!(off.viewport_range(scrolled), Some(((2, 0), (4, 5))));
        assert!(off.fully_visible(scrolled));
    }

    #[test]
    fn select_all_covers_everything() {
        let s = Selection::all(SCROLL);
        assert_eq!(s.range(20), Some(((0, 0), (19, 109))));
        assert_eq!(s.cell_count(20), 110 * 20);
    }

    #[test]
    fn word_mode_orders() {
        let mut s = Selection::new((10, 103), Side::Left, SelectMode::Word);
        s.head = (1, 101);
        assert_eq!(s.range(20), Some(((1, 101), (10, 103))));
        assert_eq!(s.cell_count(20), 2 * 20 + 10 + 1 - 1);
    }

    #[test]
    fn chat_text_trims_blank_lines() {
        assert_eq!(chat_text("\r\n\r\n$ ls\r\na  b\r\n\r\n"), "$ ls\na  b");
        assert_eq!(chat_text("  \n "), "");
        assert_eq!(chat_chip_label("Terminal 1", 3, 3), "Terminal 1 line 3");
        assert_eq!(chat_chip_label("vim", 2, 5), "vim lines 2-5");
    }
}

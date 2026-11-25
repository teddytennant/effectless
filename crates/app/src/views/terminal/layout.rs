//! Terminal geometry (spec 05 §3.3-3.9): cell metrics, the grid that fits a pane,
//! pointer to cell mapping, split pane rects, the overlay scrollbar, wheel deltas and
//! the drawer height clamp. Plain f32 math so it is testable without a window.

/// Padding on every side of the grid.
pub const PADDING: f32 = 4.;
/// `size × 1.35`, rounded, is the line height unless the font is taller.
pub const LINE_HEIGHT_RATIO: f32 = 1.35;

/// The drawer's default and minimum height, and its share of the window.
pub const DEFAULT_HEIGHT: f32 = 280.;
pub const MIN_HEIGHT: f32 = 180.;
pub const MAX_WINDOW_SHARE: f32 = 0.75;

/// Overlay scrollbar: `absolute top-1 right-px bottom-1`, 6px wide, thumb inset 1px,
/// radius 3, at least 18px tall.
pub const SCROLLBAR_INSET_Y: f32 = 4.;
pub const SCROLLBAR_INSET_RIGHT: f32 = 1.;
pub const SCROLLBAR_WIDTH: f32 = 6.;
pub const SCROLLBAR_THUMB_INSET_X: f32 = 1.;
pub const SCROLLBAR_THUMB_MIN: f32 = 18.;
pub const SCROLLBAR_THUMB_RADIUS: f32 = 3.;

/// Sidebar tab list width once a thread has two or more terminals.
pub const SIDEBAR_WIDTH: f32 = 144.;
/// Gap between the panes and the sidebar.
pub const SIDEBAR_GAP: f32 = 6.;

/// `max(round(size × 1.35), ceil(ascent + descent))`: 18 at 13px, 16 at 12px.
pub fn line_height(font_size: f32, ascent: f32, descent: f32) -> f32 {
    (font_size * LINE_HEIGHT_RATIO)
        .round()
        .max((ascent + descent.abs()).ceil())
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellSize {
    pub width: f32,
    pub height: f32,
}

/// Columns × rows that fit a pane after the 4px padding. Never below 1×1.
pub fn grid_size(width: f32, height: f32, cell: CellSize) -> (u16, u16) {
    if cell.width <= 0. || cell.height <= 0. {
        return (1, 1);
    }
    let cols = ((width - 2. * PADDING) / cell.width).floor().max(1.);
    let rows = ((height - 2. * PADDING) / cell.height).floor().max(1.);
    (cols.min(u16::MAX as f32) as u16, rows.min(u16::MAX as f32) as u16)
}

/// Where a pane's grid starts, relative to its top-left.
pub fn content_origin() -> (f32, f32) {
    (PADDING, PADDING)
}

/// The grid placed in a pane, recorded each frame for mouse mapping.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GridLayout {
    /// Window position of cell (0, 0).
    pub origin_x: f32,
    pub origin_y: f32,
    pub cell: CellSize,
    pub cols: u16,
    pub rows: u16,
    /// The pane's bounds, for the scrollbar hit area.
    pub pane_x: f32,
    pub pane_y: f32,
    pub pane_width: f32,
    pub pane_height: f32,
}

/// Which half of a cell the pointer is in; selections include a cell only when the
/// pointer crossed its middle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl GridLayout {
    /// The cell under a window position, clamped to the grid.
    pub fn cell_at(&self, x: f32, y: f32) -> ((u16, u16), Side) {
        if self.cols == 0 || self.rows == 0 || self.cell.width <= 0. || self.cell.height <= 0. {
            return ((0, 0), Side::Left);
        }
        let fx = (x - self.origin_x) / self.cell.width;
        let fy = (y - self.origin_y) / self.cell.height;
        let col = fx.floor().clamp(0., (self.cols - 1) as f32) as u16;
        let row = fy.floor().clamp(0., (self.rows - 1) as f32) as u16;
        let side = if fx < 0. {
            Side::Left
        } else if fx >= self.cols as f32 {
            Side::Right
        } else if fx - fx.floor() >= 0.5 {
            Side::Right
        } else {
            Side::Left
        };
        ((col, row), side)
    }

    /// The 1-based cell for mouse reports; may sit outside the grid while dragging.
    pub fn report_cell(&self, x: f32, y: f32) -> (u16, u16) {
        let ((col, row), _) = self.cell_at(x, y);
        (col + 1, row + 1)
    }

    pub fn in_scrollbar(&self, x: f32, y: f32) -> bool {
        let right = self.pane_x + self.pane_width - SCROLLBAR_INSET_RIGHT;
        x >= right - SCROLLBAR_WIDTH
            && x <= right
            && y >= self.pane_y + SCROLLBAR_INSET_Y
            && y <= self.pane_y + self.pane_height - SCROLLBAR_INSET_Y
    }
}

/// Equal split of a group's area: horizontal = columns, vertical = rows. Returns
/// (x, y, w, h) per pane relative to the group's origin, with rounding so the panes
/// tile without gaps.
pub fn pane_rects(count: usize, side_by_side: bool, width: f32, height: f32) -> Vec<(f32, f32, f32, f32)> {
    let n = count.max(1);
    let total = if side_by_side { width } else { height };
    (0..n)
        .map(|i| {
            let start = (total * i as f32 / n as f32).round();
            let end = (total * (i + 1) as f32 / n as f32).round();
            if side_by_side {
                (start, 0., end - start, height)
            } else {
                (0., start, width, end - start)
            }
        })
        .collect()
}

/// The scrollbar thumb as (top, height) inside the track, or None when there is no
/// scrollback. `display_offset` lines are scrolled up from the bottom.
pub fn scrollbar_thumb(
    track: f32,
    rows: usize,
    history: usize,
    display_offset: usize,
) -> Option<(f32, f32)> {
    if history == 0 || rows == 0 || track <= 0. {
        return None;
    }
    let total = (rows + history) as f32;
    let height = (track * rows as f32 / total).max(SCROLLBAR_THUMB_MIN).min(track);
    let first_visible = history.saturating_sub(display_offset.min(history)) as f32;
    let top = (track - height) * (first_visible / history as f32);
    Some((top, height))
}

/// The display offset that puts the thumb's top at `thumb_top` (dragging the thumb).
pub fn offset_for_thumb(track: f32, thumb: f32, thumb_top: f32, history: usize) -> usize {
    let span = track - thumb;
    if span <= 0. || history == 0 {
        return 0;
    }
    let ratio = (thumb_top / span).clamp(0., 1.);
    let first_visible = (ratio * history as f32).round() as usize;
    history - first_visible.min(history)
}

/// Accumulates a pixel wheel delta into whole lines. Returns (lines, remainder);
/// positive lines scroll up into history (wheel moved up).
pub fn wheel_lines(accum: f32, delta_y: f32, line_height: f32) -> (i32, f32) {
    if line_height <= 0. {
        return (0, 0.);
    }
    let total = accum + delta_y;
    let lines = (total / line_height).trunc();
    (lines as i32, total - lines * line_height)
}

/// The drawer's max height: `min(max(180, floor(window × 0.75)), available)` where
/// `available` is the chat column minus its header and the composer.
pub fn drawer_max_height(window_height: f32, available: f32) -> f32 {
    MIN_HEIGHT
        .max((window_height * MAX_WINDOW_SHARE).floor())
        .min(available)
}

/// The height a drawer renders at for a stored preference.
pub fn clamp_drawer_height(preferred: f32, window_height: f32, available: f32) -> f32 {
    let max = drawer_max_height(window_height, available);
    preferred.min(max).max(MIN_HEIGHT).round()
}

/// Drag the handle: up is taller.
pub fn drag_height(start_height: f32, start_y: f32, y: f32) -> f32 {
    (start_height + (start_y - y)).round()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: CellSize = CellSize {
        width: 7.8,
        height: 18.,
    };

    #[test]
    fn line_heights_match_spec() {
        // Liberation Mono: ascent ~0.83em, descent ~0.3em.
        assert_eq!(line_height(13., 11.6, 3.1), 18.);
        assert_eq!(line_height(12., 10.7, 2.9), 16.);
        // A tall font wins.
        assert_eq!(line_height(13., 14.2, 5.1), 20.);
    }

    #[test]
    fn grid_fits_after_padding() {
        // 1184 wide, 279 tall drawer at 13px Liberation Mono.
        assert_eq!(grid_size(1184., 279., CELL), (150, 15));
        assert_eq!(grid_size(5., 5., CELL), (1, 1));
        assert_eq!(grid_size(100., 100., CellSize::default()), (1, 1));
    }

    #[test]
    fn cell_mapping_clamps() {
        let g = GridLayout {
            origin_x: 100.,
            origin_y: 50.,
            cell: CELL,
            cols: 80,
            rows: 24,
            ..Default::default()
        };
        assert_eq!(g.cell_at(100., 50.), ((0, 0), Side::Left));
        assert_eq!(g.cell_at(100. + 7.8 * 2. + 5., 50. + 18. * 3. + 1.), ((2, 3), Side::Right));
        assert_eq!(g.cell_at(0., 0.).0, (0, 0));
        assert_eq!(g.cell_at(5000., 5000.), ((79, 23), Side::Right));
        assert_eq!(g.report_cell(100., 50.), (1, 1));
    }

    #[test]
    fn panes_tile() {
        let r = pane_rects(3, true, 1000., 200.);
        assert_eq!(r[0], (0., 0., 333., 200.));
        assert_eq!(r[1], (333., 0., 334., 200.));
        assert_eq!(r[2], (667., 0., 333., 200.));
        let v = pane_rects(2, false, 500., 279.);
        assert_eq!(v[0], (0., 0., 500., 140.));
        assert_eq!(v[1], (0., 140., 500., 139.));
    }

    #[test]
    fn thumb_geometry() {
        assert_eq!(scrollbar_thumb(100., 10, 0, 0), None);
        // At the bottom: thumb at the end of the track.
        let (top, h) = scrollbar_thumb(100., 10, 90, 0).unwrap();
        assert_eq!(h, 18.);
        assert_eq!(top, 82.);
        // Scrolled to the top.
        assert_eq!(scrollbar_thumb(100., 10, 90, 90).unwrap().0, 0.);
        // Half scrollback: proportional height.
        let (top, h) = scrollbar_thumb(100., 50, 50, 25).unwrap();
        assert_eq!(h, 50.);
        assert_eq!(top, 25.);
        assert_eq!(offset_for_thumb(100., 50., 25., 50), 25);
        assert_eq!(offset_for_thumb(100., 50., 0., 50), 50);
        assert_eq!(offset_for_thumb(100., 50., 80., 50), 0);
    }

    #[test]
    fn wheel_accumulates() {
        assert_eq!(wheel_lines(0., 10., 18.), (0, 10.));
        assert_eq!(wheel_lines(10., 10., 18.), (1, 2.));
        assert_eq!(wheel_lines(0., -40., 18.), (-2, -4.));
    }

    #[test]
    fn drawer_height_clamps() {
        assert_eq!(drawer_max_height(900., 10_000.), 675.);
        assert_eq!(drawer_max_height(200., 10_000.), 180.);
        assert_eq!(drawer_max_height(900., 400.), 400.);
        assert_eq!(clamp_drawer_height(280., 900., 700.), 280.);
        assert_eq!(clamp_drawer_height(1000., 900., 700.), 675.);
        assert_eq!(clamp_drawer_height(50., 900., 700.), 180.);
        assert_eq!(drag_height(280., 600., 550.), 330.);
    }

    #[test]
    fn scrollbar_hit() {
        let g = GridLayout {
            pane_x: 0.,
            pane_y: 0.,
            pane_width: 400.,
            pane_height: 200.,
            ..Default::default()
        };
        assert!(g.in_scrollbar(396., 100.));
        assert!(!g.in_scrollbar(390., 100.));
        assert!(!g.in_scrollbar(396., 2.));
    }
}

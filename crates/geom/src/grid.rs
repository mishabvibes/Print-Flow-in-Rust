//! The print-sheet grid: how many cards fit, where each one goes, whether a custom grid overflows.
//!
//! Direct port of the previous version's `computeGrid`/`cellRects`, with the same maths (cards are
//! centred in the printable area) but total: zero or negative sizes give an empty grid.

use serde::{Deserialize, Serialize};

use crate::rect::{Rect, finite_or};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GridMode {
    /// As many columns and rows as fit.
    #[default]
    Auto,
    /// The user's own columns × rows.
    Custom,
}

impl<'de> Deserialize<'de> for GridMode {
    /// Lenient: anything but "custom" is `Auto`, so an odd value in an old file can't break a load.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Option::<String>::deserialize(d)?.as_deref() {
            Some(s) if s.eq_ignore_ascii_case("custom") => GridMode::Custom,
            _ => GridMode::Auto,
        })
    }
}

/// Everything the grid depends on, in centimetres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridSpec {
    pub sheet_w: f64,
    pub sheet_h: f64,
    pub margin_top: f64,
    pub margin_right: f64,
    pub margin_bottom: f64,
    pub margin_left: f64,
    pub card_w: f64,
    pub card_h: f64,
    pub gap_x: f64,
    pub gap_y: f64,
    pub mode: GridMode,
    pub cols: u32,
    pub rows: u32,
}

impl Default for GridSpec {
    fn default() -> Self {
        Self {
            sheet_w: 21.0,
            sheet_h: 29.7,
            margin_top: 1.0,
            margin_right: 1.0,
            margin_bottom: 1.0,
            margin_left: 1.0,
            card_w: 8.6,
            card_h: 5.4,
            gap_x: 0.3,
            gap_y: 0.3,
            mode: GridMode::Auto,
            cols: 3,
            rows: 4,
        }
    }
}

/// Upper bound on a custom grid so a typo can't allocate millions of cells.
pub const MAX_CELLS_PER_AXIS: u32 = 200;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetGrid {
    pub cols: u32,
    pub rows: u32,
    pub capacity: usize,
    pub grid_w: f64,
    pub grid_h: f64,
    pub printable_w: f64,
    pub printable_h: f64,
    /// False when a custom grid is bigger than the printable area.
    pub fits: bool,
    pub start_x: f64,
    pub start_y: f64,
    pub card_w: f64,
    pub card_h: f64,
    pub gap_x: f64,
    pub gap_y: f64,
}

pub fn compute_grid(s: &GridSpec) -> SheetGrid {
    let f = |v: f64| finite_or(v, 0.0).max(0.0);
    let (card_w, card_h, gap_x, gap_y) = (f(s.card_w), f(s.card_h), f(s.gap_x), f(s.gap_y));
    let printable_w = (f(s.sheet_w) - f(s.margin_left) - f(s.margin_right)).max(0.0);
    let printable_h = (f(s.sheet_h) - f(s.margin_top) - f(s.margin_bottom)).max(0.0);
    let fit = |printable: f64, card: f64, gap: f64| -> u32 {
        if card <= 0.0 {
            return 0;
        }
        let n = ((printable + gap) / (card + gap)).floor();
        if n.is_finite() && n > 0.0 { (n as u32).min(MAX_CELLS_PER_AXIS) } else { 0 }
    };
    let (cols, rows) = match s.mode {
        GridMode::Auto => (fit(printable_w, card_w, gap_x), fit(printable_h, card_h, gap_y)),
        GridMode::Custom => (s.cols.clamp(1, MAX_CELLS_PER_AXIS), s.rows.clamp(1, MAX_CELLS_PER_AXIS)),
    };
    let span = |n: u32, card: f64, gap: f64| if n > 0 { n as f64 * card + (n as f64 - 1.0) * gap } else { 0.0 };
    let grid_w = span(cols, card_w, gap_x);
    let grid_h = span(rows, card_h, gap_y);
    let fits = grid_w <= printable_w + 0.01 && grid_h <= printable_h + 0.01;
    SheetGrid {
        cols,
        rows,
        capacity: cols as usize * rows as usize,
        grid_w,
        grid_h,
        printable_w,
        printable_h,
        fits,
        start_x: f(s.margin_left) + ((printable_w - grid_w) / 2.0).max(0.0),
        start_y: f(s.margin_top) + ((printable_h - grid_h) / 2.0).max(0.0),
        card_w,
        card_h,
        gap_x,
        gap_y,
    }
}

impl SheetGrid {
    /// The rectangle of cell `index` (row-major), in centimetres.
    pub fn cell(&self, index: usize) -> Option<Rect> {
        if self.cols == 0 || index >= self.capacity {
            return None;
        }
        let (r, c) = (index / self.cols as usize, index % self.cols as usize);
        Some(Rect::new(
            self.start_x + c as f64 * (self.card_w + self.gap_x),
            self.start_y + r as f64 * (self.card_h + self.gap_y),
            self.card_w,
            self.card_h,
        ))
    }
    pub fn cells(&self) -> impl Iterator<Item = Rect> + '_ {
        (0..self.capacity).filter_map(|i| self.cell(i))
    }
    /// Number of sheets needed for `cards` cards (0 when the grid is empty).
    pub fn sheets_for(&self, cards: usize) -> usize {
        if self.capacity == 0 { 0 } else { cards.div_ceil(self.capacity) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_credit_cards_fit_2_by_5() {
        let g = compute_grid(&GridSpec::default());
        // printable 19 x 27.7; (19+.3)/(8.6+.3)=2.16 -> 2 ; (27.7+.3)/(5.4+.3)=4.9 -> 4
        assert_eq!((g.cols, g.rows), (2, 4));
        assert_eq!(g.capacity, 8);
        assert!(g.fits);
        let c0 = g.cell(0).unwrap();
        let c7 = g.cell(7).unwrap();
        assert!(c0.x >= 1.0 && c7.right() <= 20.0 + 1e-9);
        assert!(g.cell(8).is_none());
        assert_eq!(g.sheets_for(9), 2);
    }

    #[test]
    fn custom_overflow_is_reported() {
        let g = compute_grid(&GridSpec { mode: GridMode::Custom, cols: 5, rows: 5, ..Default::default() });
        assert!(!g.fits);
        assert_eq!(g.capacity, 25);
    }

    #[test]
    fn degenerate_inputs_are_empty_not_panics() {
        let g = compute_grid(&GridSpec { card_w: 0.0, ..Default::default() });
        assert_eq!(g.capacity, 0);
        assert!(g.cell(0).is_none());
        let g = compute_grid(&GridSpec { sheet_w: f64::NAN, card_w: f64::INFINITY, ..Default::default() });
        assert_eq!(g.sheets_for(10), 0);
        let g = compute_grid(&GridSpec { mode: GridMode::Custom, cols: u32::MAX, rows: u32::MAX, ..Default::default() });
        assert!(g.cols <= MAX_CELLS_PER_AXIS);
    }
}

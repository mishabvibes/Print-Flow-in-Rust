//! Geometry shared by every layer: rectangles, physical units and the sheet-grid maths.
//!
//! Layer 0: depends on nothing in the workspace. Everything here is total: no function panics,
//! and non-finite input is turned into a harmless default instead of propagating NaN.
#![forbid(unsafe_code)]

mod grid;
mod rect;
mod unit;

pub use grid::{GridMode, GridSpec, SheetGrid, compute_grid};
pub use rect::{Point, Rect, clamp, finite_or, round_to};
pub use unit::{PAPER_SIZES, Paper, Unit};

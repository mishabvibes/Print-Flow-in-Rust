//! Print sheets: cards arranged on a page at a chosen DPI.
//!
//! Every card is rendered straight at the pixel size of its cell (text is vector, photos are
//! resampled once), the cards of one sheet in parallel, then pasted onto a white page. A 1000-card
//! job therefore never holds more than one sheet of cards in memory.

use forge_doc::{CardRef, Project, SheetSettings};
use forge_geom::compute_grid;
use forge_image::Image;
use rayon::prelude::*;

use crate::card::{RenderOpts, Renderer, blit_over};

const MAX_SHEET_PIXELS: u64 = 300_000_000;

/// A cell in sheet pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PxRect {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
}

/// Where everything goes on a sheet at a given DPI.
#[derive(Clone, Debug)]
pub struct SheetPlan {
    pub dpi: u32,
    pub px_w: u32,
    pub px_h: u32,
    pub cells: Vec<PxRect>,
    /// The same cells in PDF points (1/72 in) from the page's top-left, for vector placement.
    pub cells_pt: Vec<(f64, f64, f64, f64)>,
    pub cols: u32,
    pub rows: u32,
    /// The settings fit the page (cards do not overflow the printable area).
    pub fits: bool,
}

impl SheetPlan {
    pub fn capacity(&self) -> usize {
        self.cells.len()
    }

    /// Sheets needed for `cards` cards.
    pub fn sheets_for(&self, cards: usize) -> usize {
        if self.cells.is_empty() { 0 } else { cards.div_ceil(self.cells.len()) }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SheetError {
    #[error("no card fits on the sheet with these settings")]
    NoCells,
    #[error("the sheet would be {0} x {1} px: lower the DPI")]
    TooLarge(u64, u64),
}

pub fn cm_to_px(cm: f64, dpi: u32) -> f64 {
    cm / 2.54 * f64::from(dpi)
}

/// Plans a sheet at `dpi` (sheet settings are in centimetres).
pub fn plan(s: &SheetSettings, dpi: u32) -> Result<SheetPlan, SheetError> {
    let dpi = dpi.clamp(36, 1200);
    let grid = compute_grid(&s.grid_spec());
    let (pw, ph) = (cm_to_px(s.sheet.w, dpi).round().max(1.0), cm_to_px(s.sheet.h, dpi).round().max(1.0));
    if pw * ph > MAX_SHEET_PIXELS as f64 || !pw.is_finite() || !ph.is_finite() {
        return Err(SheetError::TooLarge(pw as u64, ph as u64));
    }
    let cells: Vec<PxRect> = grid
        .cells()
        .filter_map(|c| {
            let (x0, y0) = (cm_to_px(c.x, dpi).round(), cm_to_px(c.y, dpi).round());
            let (x1, y1) = (cm_to_px(c.right(), dpi).round(), cm_to_px(c.bottom(), dpi).round());
            ((x1 - x0) >= 1.0 && (y1 - y0) >= 1.0).then_some(PxRect { x: x0 as i64, y: y0 as i64, w: (x1 - x0) as u32, h: (y1 - y0) as u32 })
        })
        .collect();
    if cells.is_empty() {
        return Err(SheetError::NoCells);
    }
    let pt = |cm: f64| cm / 2.54 * 72.0;
    let cells_pt = grid.cells().map(|c| (pt(c.x), pt(c.y), pt(c.w), pt(c.h))).collect();
    Ok(SheetPlan { dpi, px_w: pw as u32, px_h: ph as u32, cells, cells_pt, cols: grid.cols, rows: grid.rows, fits: grid.fits })
}

pub struct SheetOutput {
    pub image: Image,
    /// (card index, message) for cards that could not be rendered (their cell stays blank).
    pub errors: Vec<(usize, String)>,
}

fn fill_rect(img: &mut Image, x: i64, y: i64, w: i64, h: i64, c: [u8; 4]) {
    let (iw, ih) = (i64::from(img.width), i64::from(img.height));
    for yy in y.max(0)..(y + h).min(ih) {
        for xx in x.max(0)..(x + w).min(iw) {
            let o = (yy * iw + xx) as usize * 4;
            if let Some(px) = img.rgba.get_mut(o..o + 4) {
                px.copy_from_slice(&c);
            }
        }
    }
}

/// The cards of sheet `index`, each rendered at its cell's pixel size, in parallel:
/// `(slot in the sheet, card index, image or error)`.
pub fn render_sheet_cards(
    r: &Renderer,
    project: &Project,
    plan: &SheetPlan,
    index: usize,
    opts: RenderOpts,
) -> Vec<(usize, usize, Result<Image, String>)> {
    let cap = plan.cells.len().max(1);
    let start = index * cap;
    let refs: Vec<(usize, CardRef)> = (start..(start + cap).min(project.card_count())).filter_map(|i| project.card_ref(i).map(|c| (i, c))).collect();
    refs.par_iter()
        .enumerate()
        .map(|(slot, (i, card))| {
            let cell = plan.cells.get(slot).copied().unwrap_or(PxRect { x: 0, y: 0, w: 1, h: 1 });
            // A panic in one card must not lose the sheet.
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                r.render_card(project, card, cell.w, cell.h, opts).map_err(|e| e.to_string())
            }))
            .unwrap_or_else(|_| Err("internal error while rendering this card".into()));
            (slot, *i, res)
        })
        .collect()
}

/// Renders sheet `index` (0-based) of the project's cards onto one white page image.
pub fn render_sheet(r: &Renderer, project: &Project, plan: &SheetPlan, index: usize, opts: RenderOpts) -> SheetOutput {
    let mut page = Image { width: plan.px_w, height: plan.px_h, rgba: [255u8, 255, 255, 255].repeat(plan.px_w as usize * plan.px_h as usize) };
    let rendered = render_sheet_cards(r, project, plan, index, opts);
    let used = rendered.len();
    let mut errors = Vec::new();
    for (slot, i, res) in rendered {
        let Some(cell) = plan.cells.get(slot) else { continue };
        match res {
            Ok(img) => blit_over(&mut page, &img, cell.x, cell.y),
            Err(e) => errors.push((i, e)),
        }
    }

    let b = &project.sheet.grid_border;
    if b.enabled && b.size > 0.0 {
        let t = (b.size / 72.0 * f64::from(plan.dpi)).round().max(1.0) as i64;
        let col = [b.color.r, b.color.g, b.color.b, 255];
        for cell in plan.cells.iter().take(used) {
            let (w, h) = (i64::from(cell.w), i64::from(cell.h));
            // Centred on the cell edge, like a stroked rectangle.
            let (x, y) = (cell.x - t / 2, cell.y - t / 2);
            fill_rect(&mut page, x, y, w + t, t, col);
            fill_rect(&mut page, x, y + h, w + t, t, col);
            fill_rect(&mut page, x, y, t, h + t, col);
            fill_rect(&mut page, x + w, y, t, h + t, col);
        }
    }
    SheetOutput { image: page, errors }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_plan_at_300dpi() {
        let s = SheetSettings::default();
        let p = plan(&s, 300).unwrap();
        assert!((p.px_w as i64 - 2480).abs() <= 1 && (p.px_h as i64 - 3508).abs() <= 1, "{}x{}", p.px_w, p.px_h);
        assert!(p.capacity() >= 1);
        assert!(p.cells.iter().all(|c| c.x >= 0 && c.y >= 0 && c.x + i64::from(c.w) <= i64::from(p.px_w) + 1));
    }

    #[test]
    fn absurd_dpi_is_clamped_and_huge_sheets_refused() {
        let mut s = SheetSettings::default();
        assert_eq!(plan(&s, 100_000).unwrap().dpi, 1200);
        s.sheet.w = 10_000.0;
        s.sheet.h = 10_000.0;
        assert!(matches!(plan(&s, 600), Err(SheetError::TooLarge(..))));
    }
}

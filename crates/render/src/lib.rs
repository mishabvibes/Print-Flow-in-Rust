//! Forge Studio renderer (layer 4): the one place pixels are made.
//!
//! * [`Renderer::render_row`] draws a card (template, photos, shaped text) on the CPU with
//!   `vello_cpu`. Text is drawn as filled glyph outlines from `forge-text`, so the editor preview and
//!   the exported PNG are the same pixels, at any scale.
//! * [`sheet`] arranges cards on print sheets, rendering each card directly at its final pixel size
//!   and in parallel.
#![forbid(unsafe_code)]

mod card;
mod pixmap;
pub mod sample;
pub mod sheet;
mod store;

pub use card::{RenderError, RenderOpts, Renderer, blit_over, cover_image};
pub use pixmap::{image_to_pixmap, pixmap_to_image};
pub use sheet::{PxRect, SheetPlan};
pub use store::ImageStore;

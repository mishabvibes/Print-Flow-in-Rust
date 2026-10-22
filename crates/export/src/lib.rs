//! Forge Studio export (layer 5): turns a project into files.
//!
//! * [`export_cards_zip`]: one PNG per card.
//! * [`export_cropped_zip`]: the cropped photos as JPEGs.
//! * [`export_sheets_zip`] / [`export_sheets_pdf`]: print sheets, as PNGs or a PDF whose cards are
//!   placed at their exact positions with vector cut-guide borders.
//!
//! All of them render in parallel in bounded batches, stream to disk, report progress, honour a
//! cancel flag, and write to a temp file that is renamed on success so a cancelled or failed export
//! never leaves a broken file behind.
#![forbid(unsafe_code)]

mod jobs;
pub mod pdf;

pub use jobs::*;

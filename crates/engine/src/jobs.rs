//! Long-running work that must not hold the engine: the UI takes a snapshot, runs the job on a worker
//! thread and applies the result back through a command (so it is journaled and undoable).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use forge_doc::{CropStatus, FaceDetection, ImageSource, Project};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

pub struct DetectItem {
    pub key: String,
    pub source: ImageSource,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetectOutcome {
    pub key: String,
    pub detection: Option<FaceDetection>,
    /// Pixel size of the decoded (EXIF-rotated) image.
    pub width: u32,
    pub height: u32,
    pub error: Option<String>,
}

/// The photos to detect: the given keys, or every photo not processed yet.
pub fn detect_items(p: &Project, keys: Option<&[String]>) -> Vec<DetectItem> {
    p.photos
        .iter()
        .filter(|(k, e)| match keys {
            Some(ks) => ks.contains(k),
            None => e.status == CropStatus::Pending,
        })
        .map(|(k, e)| DetectItem { key: k.clone(), source: e.source.clone() })
        .collect()
}

/// Decodes each photo and finds its face, in parallel. `progress(done, total)` is called as photos
/// finish; setting `cancel` stops early (unfinished photos are left out).
pub fn run_detect(items: &[DetectItem], cancel: &AtomicBool, progress: &(dyn Fn(usize, usize) + Sync)) -> Vec<DetectOutcome> {
    let done = AtomicUsize::new(0);
    let total = items.len();
    items
        .par_iter()
        .filter_map(|it| {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let out = crate::guard::catch_panic(|| detect_one(it)).unwrap_or_else(|m| DetectOutcome {
                key: it.key.clone(),
                detection: None,
                width: 0,
                height: 0,
                error: Some(format!("internal error: {m}")),
            });
            progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
            Some(out)
        })
        .collect()
}

fn detect_one(it: &DetectItem) -> DetectOutcome {
    let fail = |e: String| DetectOutcome { key: it.key.clone(), detection: None, width: 0, height: 0, error: Some(e) };
    let bytes = match it.source.bytes() {
        Ok(b) => b,
        Err(e) => return fail(e.to_string()),
    };
    match forge_image::decode(&bytes) {
        Ok(img) => {
            DetectOutcome { key: it.key.clone(), detection: forge_vision::detect_face(&img), width: img.width, height: img.height, error: None }
        }
        Err(e) => fail(e.to_string()),
    }
}

/// An export, described as data so the UI can run it on a worker thread and the CLI can run it inline.
#[derive(Clone, Debug)]
pub enum ExportJob {
    CardsZip { path: std::path::PathBuf },
    CroppedZip { path: std::path::PathBuf, long_edge: u32, quality: u8 },
    SheetsPdf { path: std::path::PathBuf, dpi: u32, jpeg_quality: Option<u8> },
    SheetsZip { path: std::path::PathBuf, dpi: u32 },
}

/// Runs `job` against a snapshot of the project.
pub fn run_export(
    job: &ExportJob,
    p: &Project,
    r: &forge_render::Renderer,
    ctl: &forge_export::Control,
) -> Result<forge_export::Report, forge_export::ExportError> {
    use forge_export::*;
    match job {
        ExportJob::CardsZip { path } => export_cards_zip(r, p, path, ctl),
        ExportJob::CroppedZip { path, long_edge, quality } => export_cropped_zip(r, p, path, *long_edge, *quality, ctl),
        ExportJob::SheetsPdf { path, dpi, jpeg_quality } => {
            let plan = forge_render::sheet::plan(&p.sheet, *dpi)?;
            export_sheets_pdf(r, p, path, &plan, *jpeg_quality, ctl)
        }
        ExportJob::SheetsZip { path, dpi } => {
            let plan = forge_render::sheet::plan(&p.sheet, *dpi)?;
            export_sheets_zip(r, p, path, &plan, ctl)
        }
    }
}

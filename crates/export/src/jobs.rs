use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use forge_doc::util::{UniqueNames, sanitize_filename};
use forge_doc::{CardRef, EffectiveCrop, Project};
use forge_image::{Image, encode_jpeg, encode_png};
use forge_render::{RenderOpts, Renderer, cover_image, sheet};
use rayon::prelude::*;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::pdf::{PdfBorder, PdfCodec, PdfImage, PdfWriter, deflate_rgb};

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("export cancelled")]
    Cancelled,
    #[error("there is nothing to export")]
    Nothing,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("{0}")]
    Render(String),
    #[error("{0}")]
    Sheet(#[from] sheet::SheetError),
}

/// Progress reporting and cancellation shared with the UI thread.
#[derive(Default)]
pub struct Control {
    pub cancel: AtomicBool,
    progress: Option<Box<dyn Fn(usize, usize) + Send + Sync>>,
}

impl Control {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_progress(f: impl Fn(usize, usize) + Send + Sync + 'static) -> Self {
        Self { cancel: AtomicBool::new(false), progress: Some(Box::new(f)) }
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    fn report(&self, done: usize, total: usize) {
        if let Some(f) = &self.progress {
            f(done, total);
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub written: usize,
    /// (name, reason) of items that were skipped.
    pub failed: Vec<(String, String)>,
    pub path: PathBuf,
}

/// Writes through a `.part` file that is renamed on success.
fn atomic<T>(path: &Path, f: impl FnOnce(BufWriter<File>) -> Result<(T, BufWriter<File>), ExportError>) -> Result<T, ExportError> {
    let tmp = path.with_extension(format!("{}.part", path.extension().and_then(|e| e.to_str()).unwrap_or("out")));
    let file = BufWriter::new(File::create(&tmp)?);
    match f(file) {
        Ok((v, mut file)) => {
            file.flush()?;
            drop(file);
            std::fs::rename(&tmp, path)?;
            Ok(v)
        }
        Err(e) => {
            // The partial file is useless; failing to delete it changes nothing for the caller.
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn batch_size() -> usize {
    (rayon::current_num_threads() * 2).max(4)
}

fn flatten_white(mut img: Image) -> Image {
    img.flatten_on([255, 255, 255]);
    img
}

/// Renders `jobs` in parallel batches and hands each result, in order, to `sink`.
fn pipeline<J: Sync, R: Send>(
    jobs: &[J],
    ctl: &Control,
    work: impl Fn(&J) -> R + Sync,
    mut sink: impl FnMut(usize, R) -> Result<(), ExportError>,
) -> Result<(), ExportError> {
    let total = jobs.len();
    let mut done = 0;
    for chunk in jobs.chunks(batch_size()) {
        if ctl.cancelled() {
            return Err(ExportError::Cancelled);
        }
        let results: Vec<R> = chunk.par_iter().map(&work).collect();
        for r in results {
            sink(done, r)?;
            done += 1;
        }
        ctl.report(done, total);
    }
    Ok(())
}

/// One PNG per card, at the template's pixel size.
pub fn export_cards_zip(r: &Renderer, p: &Project, path: &Path, ctl: &Control) -> Result<Report, ExportError> {
    let cards = p.card_refs();
    if cards.is_empty() {
        return Err(ExportError::Nothing);
    }
    let (w, h) = match &p.design.template {
        Some(t) => (t.w.max(1), t.h.max(1)),
        None => return Err(ExportError::Render("the design has no template image".into())),
    };
    let mut names = UniqueNames::new();
    let jobs: Vec<(CardRef, String)> = cards.into_iter().map(|c| (c.clone(), names.unique(&format!("{}.png", p.card_name(&c))))).collect();
    let mut report = Report { path: path.to_path_buf(), ..Default::default() };
    atomic(path, |file| {
        let mut zip = ZipWriter::new(file);
        // PNG data is already compressed.
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        pipeline(
            &jobs,
            ctl,
            |(card, _)| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let img = r.render_card(p, card, w, h, RenderOpts::EXPORT).map_err(|e| e.to_string())?;
                    encode_png(&img, false).map_err(|e| e.to_string())
                }))
                .unwrap_or_else(|_| Err("internal error".into()))
            },
            |i, res| {
                let name = &jobs[i].1;
                match res {
                    Ok(bytes) => {
                        zip.start_file(name.as_str(), opts)?;
                        zip.write_all(&bytes)?;
                        report.written += 1;
                    }
                    Err(e) => report.failed.push((name.clone(), e)),
                }
                Ok(())
            },
        )?;
        Ok(((), zip.finish()?))
    })?;
    Ok(report)
}

/// The cropped photos of the library as JPEGs, `long_edge` pixels on the longer side (the original app
/// used 600).
pub fn export_cropped_zip(r: &Renderer, p: &Project, path: &Path, long_edge: u32, quality: u8, ctl: &Control) -> Result<Report, ExportError> {
    if p.photos.is_empty() {
        return Err(ExportError::Nothing);
    }
    let aspect = p.crop.target_aspect.ratio();
    let le = long_edge.clamp(16, 8000);
    let (w, h) =
        if aspect >= 1.0 { (le, ((f64::from(le) / aspect).round() as u32).max(1)) } else { (((f64::from(le) * aspect).round() as u32).max(1), le) };
    let mut names = UniqueNames::new();
    let jobs: Vec<(String, String)> = p
        .photos
        .iter()
        .map(|(k, e)| {
            let base = e.name.rsplit_once('.').map_or(e.name.as_str(), |(b, _)| b);
            let base = if base.is_empty() { k.as_str() } else { base };
            (k.clone(), names.unique(&format!("{}.jpg", sanitize_filename(base))))
        })
        .collect();
    let mut report = Report { path: path.to_path_buf(), ..Default::default() };
    atomic(path, |file| {
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        pipeline(
            &jobs,
            ctl,
            |(key, _)| -> Result<Vec<u8>, String> {
                let entry = p.photos.get(key).ok_or("photo missing")?;
                let img = r.store.load(&entry.source, false)?;
                let crop: EffectiveCrop = entry.effective_crop(aspect, None);
                let out = flatten_white(cover_image(&img, &crop, w, h));
                encode_jpeg(&out, quality.clamp(10, 100)).map_err(|e| e.to_string())
            },
            |i, res| {
                let name = &jobs[i].1;
                match res {
                    Ok(bytes) => {
                        zip.start_file(name.as_str(), opts)?;
                        zip.write_all(&bytes)?;
                        report.written += 1;
                    }
                    Err(e) => report.failed.push((name.clone(), e)),
                }
                Ok(())
            },
        )?;
        Ok(((), zip.finish()?))
    })?;
    Ok(report)
}

/// One PNG per print sheet.
pub fn export_sheets_zip(r: &Renderer, p: &Project, path: &Path, plan: &sheet::SheetPlan, ctl: &Control) -> Result<Report, ExportError> {
    let sheets = plan.sheets_for(p.card_count());
    if sheets == 0 {
        return Err(ExportError::Nothing);
    }
    let mut report = Report { path: path.to_path_buf(), ..Default::default() };
    atomic(path, |file| {
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for s in 0..sheets {
            if ctl.cancelled() {
                return Err(ExportError::Cancelled);
            }
            let out = sheet::render_sheet(r, p, plan, s, RenderOpts::EXPORT);
            for (i, e) in out.errors {
                report.failed.push((format!("card {}", i + 1), e));
            }
            let bytes = encode_png(&out.image, false).map_err(|e| ExportError::Render(e.to_string()))?;
            zip.start_file(format!("sheet_{:03}.png", s + 1), opts)?;
            zip.write_all(&bytes)?;
            report.written += 1;
            ctl.report(s + 1, sheets);
        }
        Ok(((), zip.finish()?))
    })?;
    Ok(report)
}

/// A PDF with one page per sheet. `jpeg_quality = None` stores cards losslessly.
pub fn export_sheets_pdf(
    r: &Renderer,
    p: &Project,
    path: &Path,
    plan: &sheet::SheetPlan,
    jpeg_quality: Option<u8>,
    ctl: &Control,
) -> Result<Report, ExportError> {
    let sheets = plan.sheets_for(p.card_count());
    if sheets == 0 {
        return Err(ExportError::Nothing);
    }
    let pt = |cm: f64| cm / 2.54 * 72.0;
    let (page_w, page_h) = (pt(p.sheet.sheet.w), pt(p.sheet.sheet.h));
    let border = &p.sheet.grid_border;
    let mut report = Report { path: path.to_path_buf(), ..Default::default() };
    atomic(path, |file| {
        let mut pdf = PdfWriter::new(file)?;
        for s in 0..sheets {
            if ctl.cancelled() {
                return Err(ExportError::Cancelled);
            }
            let rendered = sheet::render_sheet_cards(r, p, plan, s, RenderOpts::EXPORT);
            // Encode each card in parallel too.
            let encoded: Vec<(usize, usize, Result<PdfImage, String>)> = rendered
                .into_par_iter()
                .map(|(slot, idx, res)| {
                    let (x, y, w, h) = plan.cells_pt.get(slot).copied().unwrap_or((0.0, 0.0, 1.0, 1.0));
                    let enc = res.and_then(|img| {
                        let img = flatten_white(img);
                        let (data, codec) = match jpeg_quality {
                            Some(q) => (encode_jpeg(&img, q.clamp(10, 100)).map_err(|e| e.to_string())?, PdfCodec::Jpeg),
                            None => (deflate_rgb(&img.rgba).map_err(|e| e.to_string())?, PdfCodec::Flate),
                        };
                        Ok(PdfImage { px_w: img.width, px_h: img.height, codec, data, x, y, w, h })
                    });
                    (slot, idx, enc)
                })
                .collect();
            let mut images = Vec::new();
            let mut borders = Vec::new();
            for (slot, idx, enc) in encoded {
                match enc {
                    Ok(im) => {
                        if border.enabled {
                            borders.push(PdfBorder { x: im.x, y: im.y, w: im.w, h: im.h });
                        }
                        images.push(im);
                    }
                    Err(e) => report.failed.push((format!("card {}", idx + 1), e)),
                }
                let _ = slot;
            }
            pdf.add_page(page_w, page_h, &images, &borders, border.size, [border.color.r, border.color.g, border.color.b])?;
            report.written += 1;
            ctl.report(s + 1, sheets);
        }
        Ok(((), pdf.finish()?))
    })?;
    Ok(report)
}

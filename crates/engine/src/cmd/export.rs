use std::path::PathBuf;

use serde::Deserialize;

use super::args;
use crate::jobs::{ExportJob, run_export};
use crate::registry::CommandDef;
use crate::{Engine, EngineError, EngineResult, Value, json};

fn finish(e: &Engine, job: ExportJob) -> EngineResult<Value> {
    let rep = run_export(&job, &e.project, &e.renderer, &forge_export::Control::new()).map_err(EngineError::failed)?;
    Ok(json!({ "written": rep.written, "failed": rep.failed, "path": rep.path }))
}

#[derive(Deserialize)]
struct PathArg {
    path: PathBuf,
}

fn cards_zip(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let PathArg { path } = args(v)?;
    finish(e, ExportJob::CardsZip { path })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CroppedArgs {
    path: PathBuf,
    /// Pixels on the longer side (the original app used 600).
    #[serde(default = "d_edge")]
    long_edge: u32,
    #[serde(default = "d_quality")]
    quality: u8,
}

fn d_edge() -> u32 {
    600
}
fn d_quality() -> u8 {
    92
}

fn cropped_zip(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: CroppedArgs = args(v)?;
    finish(e, ExportJob::CroppedZip { path: a.path, long_edge: a.long_edge, quality: a.quality })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SheetArgs {
    path: PathBuf,
    /// Defaults to the sheet settings' DPI.
    dpi: Option<u32>,
    /// PDF only: JPEG quality for cards; `null` stores them losslessly.
    #[serde(default = "d_jpeg")]
    jpeg_quality: Option<u8>,
}

fn d_jpeg() -> Option<u8> {
    Some(92)
}

fn sheets_pdf(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: SheetArgs = args(v)?;
    let dpi = a.dpi.unwrap_or(e.project.sheet.dpi);
    finish(e, ExportJob::SheetsPdf { path: a.path, dpi, jpeg_quality: a.jpeg_quality })
}

fn sheets_zip(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: SheetArgs = args(v)?;
    let dpi = a.dpi.unwrap_or(e.project.sheet.dpi);
    finish(e, ExportJob::SheetsZip { path: a.path, dpi })
}

fn has_cards(e: &Engine) -> bool {
    e.project.card_count() > 0
}
fn has_generated_cards(e: &Engine) -> bool {
    e.project.card_count() > 0 && (e.project.design.template.is_some() || !matches!(e.project.card_source, forge_doc::CardSource::Generated))
}
fn has_photos(e: &Engine) -> bool {
    !e.project.photos.is_empty()
}

pub(crate) const COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "export.cards_zip",
        label: "Export cards (ZIP of PNG)...",
        menu: "File/Export/Cards as ZIP...",
        shortcut: None,
        doc: "{path} One PNG per data row.",
        undoable: false,
        enabled: has_generated_cards,
        run: cards_zip,
    },
    CommandDef {
        id: "export.cropped_zip",
        label: "Export cropped photos (ZIP)...",
        menu: "File/Export/Cropped photos...",
        shortcut: None,
        doc: "{path, longEdge?, quality?} The cropped photos as JPEG.",
        undoable: false,
        enabled: has_photos,
        run: cropped_zip,
    },
    CommandDef {
        id: "export.sheets_pdf",
        label: "Export sheets (PDF)...",
        menu: "File/Export/Sheets as PDF...",
        shortcut: None,
        doc: "{path, dpi?, jpegQuality?} One PDF page per sheet.",
        undoable: false,
        enabled: has_cards,
        run: sheets_pdf,
    },
    CommandDef {
        id: "export.sheets_zip",
        label: "Export sheets (ZIP of PNG)...",
        menu: "File/Export/Sheets as ZIP...",
        shortcut: None,
        doc: "{path, dpi?} One PNG per sheet.",
        undoable: false,
        enabled: has_cards,
        run: sheets_zip,
    },
];

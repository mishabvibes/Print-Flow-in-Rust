use std::path::PathBuf;

use forge_render::{RenderOpts, sheet};
use serde::Deserialize;

use super::{args, ok};
use crate::registry::{CommandDef, always};
use crate::{Engine, EngineError, EngineResult, Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CardArgs {
    #[serde(default)]
    index: usize,
    path: PathBuf,
    /// Output width in pixels (default: the template's own).
    width: Option<u32>,
    #[serde(default = "yes")]
    placeholders: bool,
}

fn yes() -> bool {
    true
}

/// Renders one card to a PNG: the headless equivalent of looking at the preview.
fn card(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: CardArgs = args(v)?;
    let cref = e.project.card_ref(a.index).ok_or_else(|| EngineError::NotFound(format!("no card {}", a.index)))?;
    let (tw, th) = e.project.design.template.as_ref().map(|t| (t.w.max(1), t.h.max(1))).unwrap_or((1000, 630));
    let w = a.width.unwrap_or(tw).clamp(8, 20_000);
    let h = ((f64::from(w) * f64::from(th) / f64::from(tw)).round() as u32).max(1);
    let opts = RenderOpts { placeholders: a.placeholders, cache: true };
    let img = e.renderer.render_card(&e.project, &cref, w, h, opts).map_err(EngineError::failed)?;
    std::fs::write(&a.path, forge_image::encode_png(&img, false).map_err(EngineError::failed)?).map_err(EngineError::failed)?;
    Ok(json!({ "width": img.width, "height": img.height }))
}

#[derive(Deserialize)]
struct SheetArgs {
    #[serde(default)]
    index: usize,
    path: PathBuf,
    dpi: Option<u32>,
}

fn sheet_png(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: SheetArgs = args(v)?;
    let plan = sheet::plan(&e.project.sheet, a.dpi.unwrap_or(e.project.sheet.dpi)).map_err(EngineError::failed)?;
    let out = sheet::render_sheet(&e.renderer, &e.project, &plan, a.index, RenderOpts::EXPORT);
    std::fs::write(&a.path, forge_image::encode_png(&out.image, true).map_err(EngineError::failed)?).map_err(EngineError::failed)?;
    Ok(json!({ "width": out.image.width, "height": out.image.height, "errors": out.errors.len() }))
}

#[derive(Deserialize)]
struct FontArg {
    path: PathBuf,
}

fn add_font(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let FontArg { path } = args(v)?;
    let n = e.fonts.add_file(&path);
    if n == 0 {
        return Err(EngineError::failed("that file has no usable font"));
    }
    if !e.prefs.fonts.contains(&path) {
        e.prefs.fonts.push(path);
        e.save_prefs();
    }
    Ok(json!({ "faces": n }))
}

fn scan_fonts(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    e.fonts.scan_system_fonts();
    for p in e.prefs.fonts.clone() {
        e.fonts.add_file(&p);
    }
    ok()
}

fn list_fonts(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    Ok(json!(e.fonts.families()))
}

pub(crate) const COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "render.card",
        label: "Render card to PNG",
        menu: "",
        shortcut: None,
        doc: "{index?, path, width?, placeholders?} Render one card (headless preview).",
        undoable: false,
        enabled: always,
        run: card,
    },
    CommandDef {
        id: "render.sheet",
        label: "Render sheet to PNG",
        menu: "",
        shortcut: None,
        doc: "{index?, path, dpi?} Render one print sheet.",
        undoable: false,
        enabled: always,
        run: sheet_png,
    },
    CommandDef {
        id: "fonts.add",
        label: "Add font file...",
        menu: "Design/Add font...",
        shortcut: None,
        doc: "{path} Make a .ttf/.otf/.ttc font available (remembered).",
        undoable: false,
        enabled: always,
        run: add_font,
    },
    CommandDef {
        id: "fonts.scan",
        label: "Scan system fonts",
        menu: "",
        shortcut: None,
        doc: "Index the installed fonts (the app does this in the background at start).",
        undoable: false,
        enabled: always,
        run: scan_fonts,
    },
    CommandDef {
        id: "fonts.list",
        label: "List font families",
        menu: "",
        shortcut: None,
        doc: "The available font family names.",
        undoable: false,
        enabled: always,
        run: list_fonts,
    },
];

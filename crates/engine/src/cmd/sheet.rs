use std::path::PathBuf;

use forge_doc::util::{is_image_name, natural_cmp};
use forge_doc::{CardSource, ImageSource, Orientation};
use serde::Deserialize;

use super::{args, finite, merge, ok};
use crate::registry::{CommandDef, always};
use crate::{Engine, EngineError, EngineResult, Value, json};

fn sanitize(s: &mut forge_doc::SheetSettings) {
    s.sheet.w = finite(s.sheet.w, 1.0, 1000.0, 21.0);
    s.sheet.h = finite(s.sheet.h, 1.0, 1000.0, 29.7);
    s.card.w = finite(s.card.w, 0.1, 1000.0, 8.6);
    s.card.h = finite(s.card.h, 0.1, 1000.0, 5.4);
    for m in [&mut s.margin.top, &mut s.margin.right, &mut s.margin.bottom, &mut s.margin.left, &mut s.gap.x, &mut s.gap.y] {
        *m = finite(*m, 0.0, 500.0, 0.0);
    }
    s.dpi = s.dpi.clamp(36, 1200);
    s.custom_grid.cols = s.custom_grid.cols.clamp(1, 200);
    s.custom_grid.rows = s.custom_grid.rows.clamp(1, 200);
    s.grid_border.size = finite(s.grid_border.size, 0.0, 50.0, 0.5);
}

#[derive(Deserialize)]
struct PatchArg {
    patch: Value,
}

/// `{patch}` merges into the sheet settings, e.g. `{"card": {"w": 9}, "dpi": 600}`.
fn update(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let PatchArg { patch } = args(v)?;
    if !patch.is_object() {
        return Err(EngineError::invalid("`patch` must be an object"));
    }
    let mut cur = serde_json::to_value(&e.project.sheet).map_err(EngineError::failed)?;
    merge(&mut cur, &patch);
    let mut n: forge_doc::SheetSettings = serde_json::from_value(cur).map_err(|err| EngineError::invalid(format!("bad sheet value: {err}")))?;
    sanitize(&mut n);
    e.project.sheet = n;
    ok()
}

#[derive(Deserialize)]
struct NameArg {
    name: String,
}

fn preset(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let NameArg { name } = args(v)?;
    if e.project.sheet.apply_preset(&name) { ok() } else { Err(EngineError::NotFound(format!("no paper preset {name}"))) }
}

#[derive(Deserialize)]
struct OrientArg {
    value: String,
}

fn orientation(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let OrientArg { value } = args(v)?;
    let o = match value.as_str() {
        "portrait" => Orientation::Portrait,
        "landscape" => Orientation::Landscape,
        _ => return Err(EngineError::invalid("orientation must be \"portrait\" or \"landscape\"")),
    };
    e.project.sheet.set_orientation(o);
    ok()
}

#[derive(Deserialize)]
struct MarginArg {
    value: f64,
}

fn uniform_margin(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let MarginArg { value } = args(v)?;
    e.project.sheet.set_uniform_margin(finite(value, 0.0, 500.0, 0.0));
    ok()
}

#[derive(Deserialize)]
struct KindArg {
    kind: String,
}

fn set_source(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let KindArg { kind } = args(v)?;
    e.project.card_source = match kind.as_str() {
        "generated" => CardSource::Generated,
        "photos" => CardSource::CroppedPhotos,
        "files" => match std::mem::take(&mut e.project.card_source) {
            f @ CardSource::Files(_) => f,
            _ => CardSource::Files(Vec::new()),
        },
        _ => return Err(EngineError::invalid("kind must be \"generated\", \"photos\" or \"files\"")),
    };
    ok()
}

#[derive(Deserialize)]
struct PathsArg {
    paths: Vec<PathBuf>,
}

/// Imports ready-made card images to arrange on sheets.
fn add_files(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let PathsArg { mut paths } = args(v)?;
    paths.retain(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(is_image_name));
    paths.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    let mut files = match std::mem::take(&mut e.project.card_source) {
        CardSource::Files(f) => f,
        _ => Vec::new(),
    };
    let n = paths.len();
    for p in paths {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        files.push((name, ImageSource::from_path(p)));
    }
    e.project.card_source = CardSource::Files(files);
    Ok(json!({ "added": n }))
}

fn clear_files(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    if matches!(e.project.card_source, CardSource::Files(_)) {
        e.project.card_source = CardSource::Files(Vec::new());
    }
    ok()
}

#[derive(Deserialize)]
struct PathArg {
    path: PathBuf,
}

fn save_layout(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let PathArg { path } = args(v)?;
    forge_format::save_layout_preset(&e.project.sheet, &path).map_err(EngineError::failed)?;
    ok()
}

fn load_layout(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let PathArg { path } = args(v)?;
    let mut s = forge_format::load_layout_preset(&path, &e.project.sheet).map_err(EngineError::failed)?;
    sanitize(&mut s);
    e.project.sheet = s;
    ok()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WidthArg {
    width_cm: f64,
}

/// Sizes the card to the template's aspect at the given width.
fn card_from_template(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let WidthArg { width_cm } = args(v)?;
    let t = e.project.design.template.as_ref().ok_or_else(|| EngineError::failed("the design has no template"))?;
    let w = finite(width_cm, 0.5, 100.0, 8.6);
    e.project.sheet.card.w = w;
    e.project.sheet.card.h = w * f64::from(t.h.max(1)) / f64::from(t.w.max(1));
    ok()
}

pub(crate) const COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "sheet.update",
        label: "Change sheet settings",
        menu: "",
        shortcut: None,
        doc: "{patch} Merge into the sheet settings: sheet, margin, card, gap, gridMode, customGrid, gridBorder, dpi (sizes in cm).",
        undoable: true,
        enabled: always,
        run: update,
    },
    CommandDef {
        id: "sheet.preset",
        label: "Paper size",
        menu: "",
        shortcut: None,
        doc: "{name} A3, A4, A5, Letter or Legal.",
        undoable: true,
        enabled: always,
        run: preset,
    },
    CommandDef {
        id: "sheet.orientation",
        label: "Orientation",
        menu: "",
        shortcut: None,
        doc: "{value: portrait|landscape}",
        undoable: true,
        enabled: always,
        run: orientation,
    },
    CommandDef {
        id: "sheet.uniform_margin",
        label: "Set all margins",
        menu: "",
        shortcut: None,
        doc: "{value} Margin in cm on all sides.",
        undoable: true,
        enabled: always,
        run: uniform_margin,
    },
    CommandDef {
        id: "sheet.set_source",
        label: "Cards to print",
        menu: "",
        shortcut: None,
        doc: "{kind: generated|photos|files} What fills the sheets.",
        undoable: true,
        enabled: always,
        run: set_source,
    },
    CommandDef {
        id: "sheet.add_files",
        label: "Add card images...",
        menu: "Sheets/Add card images...",
        shortcut: None,
        doc: "{paths[]} Import ready-made card images.",
        undoable: true,
        enabled: always,
        run: add_files,
    },
    CommandDef {
        id: "sheet.clear_files",
        label: "Remove card images",
        menu: "",
        shortcut: None,
        doc: "Remove the imported card images.",
        undoable: true,
        enabled: always,
        run: clear_files,
    },
    CommandDef {
        id: "sheet.card_from_template",
        label: "Card size from template",
        menu: "Sheets/Card size from template",
        shortcut: None,
        doc: "{widthCm} Card height from the template's aspect.",
        undoable: true,
        enabled: always,
        run: card_from_template,
    },
    CommandDef {
        id: "sheet.save_layout",
        label: "Save layout preset...",
        menu: "Sheets/Save layout...",
        shortcut: None,
        doc: "{path}",
        undoable: false,
        enabled: always,
        run: save_layout,
    },
    CommandDef {
        id: "sheet.load_layout",
        label: "Load layout preset...",
        menu: "Sheets/Load layout...",
        shortcut: None,
        doc: "{path}",
        undoable: true,
        enabled: always,
        run: load_layout,
    },
];

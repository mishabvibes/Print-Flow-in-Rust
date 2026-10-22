use std::path::PathBuf;

use forge_doc::{Field, FieldKind, ImageSource, Template};
use serde::Deserialize;

use super::{args, finite, merge, ok};
use crate::registry::{CommandDef, always};
use crate::{Engine, EngineError, EngineResult, Value, json};

/// Keeps a field's numbers in a sane range (a hostile or buggy value never reaches the renderer).
pub(crate) fn sanitize_field(f: &mut Field) {
    f.x = finite(f.x, -200.0, 300.0, 0.0);
    f.y = finite(f.y, -200.0, 300.0, 0.0);
    f.w = finite(f.w, 0.1, 400.0, 20.0);
    f.h = finite(f.h, 0.1, 400.0, 10.0);
    f.opacity = finite(f.opacity, 0.0, 100.0, 100.0);
    f.rotation = finite(f.rotation, -360.0, 360.0, 0.0);
    match &mut f.kind {
        FieldKind::Text(t) => {
            t.font_size = finite(t.font_size, 1.0, 4000.0, 18.0);
            t.line_height = finite(t.line_height, 0.5, 5.0, 1.3);
            t.weight = t.weight.clamp(1, 1000);
            t.max_lines = t.max_lines.min(1000);
        }
        FieldKind::Photo(p) => {
            p.border_w = finite(p.border_w, 0.0, 500.0, 0.0);
            p.zoom = finite(p.zoom, 10.0, 1000.0, 100.0);
            p.offset_x = finite(p.offset_x, -100.0, 100.0, 0.0);
            p.offset_y = finite(p.offset_y, -100.0, 100.0, 0.0);
        }
    }
}

#[derive(Deserialize)]
struct PathArg {
    path: PathBuf,
}

fn set_template(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let PathArg { path } = args(v)?;
    let bytes = std::fs::read(&path).map_err(|err| EngineError::failed(format!("cannot read {}: {err}", path.display())))?;
    let (w, h) = forge_image::probe_dimensions(&bytes).map_err(|err| EngineError::failed(format!("not an image: {err}")))?;
    e.project.design.template = Some(Template { src: ImageSource::from_path(path), w, h });
    Ok(json!({ "width": w, "height": h }))
}

fn clear_template(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    e.project.design.template = None;
    ok()
}

fn add_field(e: &mut Engine, text: bool, v: &Value) -> EngineResult<Value> {
    let id = e.project.design.next_id();
    let mut f = if text { Field::new_text(id.clone()) } else { Field::new_photo(id.clone()) };
    // Optional initial values: {x, y, w, h, name, ...}.
    if v.is_object() {
        let mut cur = serde_json::to_value(&f).map_err(EngineError::failed)?;
        let mut patch = v.clone();
        if let Value::Object(m) = &mut patch {
            m.remove("id");
            m.remove("type");
        }
        merge(&mut cur, &patch);
        if let Ok(n) = serde_json::from_value::<Field>(cur) {
            f = n;
        }
    }
    sanitize_field(&mut f);
    e.project.design.fields.push(f);
    e.project.selected_field = Some(id.clone());
    Ok(json!({ "id": id }))
}

fn add_text(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    add_field(e, true, v)
}
fn add_photo(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    add_field(e, false, v)
}

#[derive(Deserialize)]
struct IdArg {
    id: String,
}

fn remove_field(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let IdArg { id } = args(v)?;
    if e.project.design.remove_field(&id) { ok() } else { Err(EngineError::NotFound(format!("no field {id}"))) }
}

fn duplicate(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let IdArg { id } = args(v)?;
    let mut f = e.project.design.field(&id).cloned().ok_or_else(|| EngineError::NotFound(format!("no field {id}")))?;
    let new_id = e.project.design.next_id();
    f.id = new_id.clone();
    f.x += 2.0;
    f.y += 2.0;
    f.name = format!("{} copy", f.name);
    if let Some(col) = e.project.design.mapping.get(&id).cloned() {
        e.project.design.mapping.insert(new_id.clone(), col);
    }
    sanitize_field(&mut f);
    e.project.design.fields.push(f);
    e.project.selected_field = Some(new_id.clone());
    Ok(json!({ "id": new_id }))
}

#[derive(Deserialize)]
struct UpdateArgs {
    id: String,
    patch: Value,
}

/// Changes any properties of a field: `patch` is a JSON object with the field's own (camelCase) names.
fn update_field(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: UpdateArgs = args(v)?;
    if !a.patch.is_object() {
        return Err(EngineError::invalid("`patch` must be an object"));
    }
    let f = e.project.design.field_mut(&a.id).ok_or_else(|| EngineError::NotFound(format!("no field {}", a.id)))?;
    let mut cur = serde_json::to_value(&*f).map_err(EngineError::failed)?;
    let mut patch = a.patch;
    if let Value::Object(m) = &mut patch {
        // The id is the mapping key and the type decides which properties exist: neither changes here.
        m.remove("id");
        m.remove("type");
    }
    merge(&mut cur, &patch);
    let mut n: Field = serde_json::from_value(cur).map_err(|err| EngineError::invalid(format!("bad field value: {err}")))?;
    sanitize_field(&mut n);
    *f = n;
    ok()
}

#[derive(Deserialize)]
struct ReorderArgs {
    id: String,
    index: usize,
}

fn reorder(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: ReorderArgs = args(v)?;
    let fields = &mut e.project.design.fields;
    let from = fields.iter().position(|f| f.id == a.id).ok_or_else(|| EngineError::NotFound(format!("no field {}", a.id)))?;
    let f = fields.remove(from);
    fields.insert(a.index.min(fields.len()), f);
    ok()
}

#[derive(Deserialize)]
struct SelectArg {
    id: Option<String>,
}

fn select(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let SelectArg { id } = args(v)?;
    e.project.selected_field = id.filter(|i| e.project.design.field(i).is_some());
    ok()
}

fn save_preset(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let PathArg { path } = args(v)?;
    forge_format::save_design_preset(&e.project.design, &path).map_err(EngineError::failed)?;
    ok()
}

fn load_preset(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let PathArg { path } = args(v)?;
    let (template, mut fields, skipped) = forge_format::load_design_preset(&path).map_err(EngineError::failed)?;
    fields.iter_mut().for_each(sanitize_field);
    if template.is_some() {
        e.project.design.template = template;
    }
    if !fields.is_empty() || skipped > 0 {
        e.project.design.fields = fields;
    }
    let ids: Vec<String> = e.project.design.fields.iter().map(|f| f.id.clone()).collect();
    e.project.design.mapping.retain(|k, _| ids.contains(k));
    let headers = e.project.data.headers.clone();
    e.project.design.auto_map(&headers);
    e.project.selected_field = None;
    Ok(json!({ "fields": e.project.design.fields.len(), "skipped": skipped }))
}

fn has_template(e: &Engine) -> bool {
    e.project.design.template.is_some()
}

pub(crate) const COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "design.set_template",
        label: "Choose template image...",
        menu: "Design/Template image...",
        shortcut: None,
        doc: "{path} The card background; cards are rendered at its pixel size.",
        undoable: true,
        enabled: always,
        run: set_template,
    },
    CommandDef {
        id: "design.clear_template",
        label: "Remove template",
        menu: "Design/Remove template",
        shortcut: None,
        doc: "Remove the template image.",
        undoable: true,
        enabled: has_template,
        run: clear_template,
    },
    CommandDef {
        id: "design.add_text_field",
        label: "Add text field",
        menu: "Design/Add text field",
        shortcut: Some("T"),
        doc: "{x?,y?,w?,h?,name?...} A text field (percent of the template).",
        undoable: true,
        enabled: always,
        run: add_text,
    },
    CommandDef {
        id: "design.add_photo_field",
        label: "Add photo field",
        menu: "Design/Add photo field",
        shortcut: Some("P"),
        doc: "{x?,y?,w?,h?,name?...} A photo field.",
        undoable: true,
        enabled: always,
        run: add_photo,
    },
    CommandDef {
        id: "design.remove_field",
        label: "Delete field",
        menu: "",
        shortcut: Some("Delete"),
        doc: "{id}",
        undoable: true,
        enabled: always,
        run: remove_field,
    },
    CommandDef {
        id: "design.duplicate_field",
        label: "Duplicate field",
        menu: "Design/Duplicate field",
        shortcut: Some("Ctrl+J"),
        doc: "{id}",
        undoable: true,
        enabled: always,
        run: duplicate,
    },
    CommandDef {
        id: "design.update_field",
        label: "Change field",
        menu: "",
        shortcut: None,
        doc: "{id, patch} Change field properties (x, y, w, h, rotation, opacity, fontSize, fontFamily, color, align...).",
        undoable: true,
        enabled: always,
        run: update_field,
    },
    CommandDef {
        id: "design.reorder_field",
        label: "Reorder field",
        menu: "",
        shortcut: None,
        doc: "{id, index} Move a field in the stacking order.",
        undoable: true,
        enabled: always,
        run: reorder,
    },
    CommandDef {
        id: "design.select_field",
        label: "Select field",
        menu: "",
        shortcut: None,
        doc: "{id|null}",
        undoable: false,
        enabled: always,
        run: select,
    },
    CommandDef {
        id: "design.save_preset",
        label: "Save design preset...",
        menu: "Design/Save preset...",
        shortcut: None,
        doc: "{path} Save template and fields as JSON.",
        undoable: false,
        enabled: always,
        run: save_preset,
    },
    CommandDef {
        id: "design.load_preset",
        label: "Load design preset...",
        menu: "Design/Load preset...",
        shortcut: None,
        doc: "{path} Replace the template and fields from a preset file.",
        undoable: true,
        enabled: always,
        run: load_preset,
    },
];

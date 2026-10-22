use std::path::PathBuf;
use std::sync::Arc;

use forge_doc::{Design, Table};
use serde::Deserialize;

use super::{args, ok};
use crate::registry::{CommandDef, always};
use crate::{Engine, EngineError, EngineResult, Value, json};

#[derive(Deserialize)]
struct PathArg {
    path: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadArgs {
    path: PathBuf,
    /// Worksheet index for Excel files.
    sheet: Option<usize>,
    /// Also create a text field for every column that has none.
    #[serde(default)]
    create_fields: bool,
}

/// Installs a table: maps fields by name, guesses the photo key column, keeps indices valid.
fn install(e: &mut Engine, table: Table, create_fields: bool) -> Value {
    let headers = table.headers.clone();
    let (rows, cols) = (table.len(), headers.len());
    e.project.data = Arc::new(table);
    // Mappings to columns that no longer exist are dropped.
    e.project.design.mapping.retain(|_, col| headers.contains(col));
    let created = if create_fields { e.project.design.auto_create_fields(&headers) } else { 0 };
    e.project.design.auto_map(&headers);
    if !headers.contains(&e.project.design.photo_key_column) {
        e.project.design.photo_key_column = Design::guess_key_column(&headers).unwrap_or_default();
    }
    e.project.preview_index = 0;
    json!({ "rows": rows, "columns": cols, "createdFields": created, "keyColumn": e.project.design.photo_key_column })
}

fn load(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: LoadArgs = args(v)?;
    let table = forge_data::read_table(&a.path, a.sheet).map_err(EngineError::failed)?;
    Ok(install(e, table, a.create_fields))
}

fn sheets(_: &mut Engine, v: &Value) -> EngineResult<Value> {
    let PathArg { path } = args(v)?;
    Ok(json!(forge_data::sheet_names(&path).map_err(EngineError::failed)?))
}

fn load_sample(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    let table = forge_data::parse_csv(forge_data::SAMPLE_CSV.as_bytes()).map_err(EngineError::failed)?;
    Ok(install(e, table, true))
}

fn clear(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    e.project.data = Arc::new(Table::default());
    e.project.design.mapping.clear();
    e.project.preview_index = 0;
    ok()
}

#[derive(Deserialize)]
struct ColumnArg {
    column: String,
}

fn set_key_column(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let ColumnArg { column } = args(v)?;
    if !column.is_empty() && e.project.data.column_index(&column).is_none() {
        return Err(EngineError::NotFound(format!("no column named {column}")));
    }
    e.project.design.photo_key_column = column;
    ok()
}

#[derive(Deserialize)]
struct MapArgs {
    field: String,
    column: Option<String>,
}

fn map_field(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: MapArgs = args(v)?;
    if e.project.design.field(&a.field).is_none() {
        return Err(EngineError::NotFound(format!("no field {}", a.field)));
    }
    match a.column.filter(|c| !c.is_empty()) {
        Some(c) => {
            if e.project.data.column_index(&c).is_none() {
                return Err(EngineError::NotFound(format!("no column named {c}")));
            }
            e.project.design.mapping.insert(a.field, c);
        }
        None => {
            e.project.design.mapping.remove(&a.field);
        }
    }
    ok()
}

fn auto_map(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    let headers = e.project.data.headers.clone();
    e.project.design.auto_map(&headers);
    ok()
}

fn create_fields(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    let headers = e.project.data.headers.clone();
    let n = e.project.design.auto_create_fields(&headers);
    e.project.design.auto_map(&headers);
    Ok(json!({ "created": n }))
}

#[derive(Deserialize)]
struct CellArgs {
    row: usize,
    column: String,
    value: String,
}

fn set_cell(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: CellArgs = args(v)?;
    let col = e.project.data.column_index(&a.column).ok_or_else(|| EngineError::NotFound(format!("no column named {}", a.column)))?;
    let table = Arc::make_mut(&mut e.project.data);
    let cell = table.rows.get_mut(a.row).and_then(|r| r.get_mut(col)).ok_or_else(|| EngineError::NotFound(format!("no row {}", a.row)))?;
    *cell = a.value;
    ok()
}

#[derive(Deserialize)]
struct IndexArg {
    index: usize,
}

fn set_preview(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let IndexArg { index } = args(v)?;
    e.project.preview_index = index.min(e.project.data.len().saturating_sub(1));
    ok()
}

fn has_data(e: &Engine) -> bool {
    !e.project.data.is_empty()
}

pub(crate) const COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "data.load",
        label: "Load data...",
        menu: "Data/Load CSV or Excel...",
        shortcut: None,
        doc: "{path, sheet?, createFields?} Load a CSV/TSV/Excel/ODS file; maps columns to fields by name.",
        undoable: true,
        enabled: always,
        run: load,
    },
    CommandDef {
        id: "data.sheets",
        label: "List worksheets",
        menu: "",
        shortcut: None,
        doc: "{path} Worksheet names of an Excel/ODS file.",
        undoable: false,
        enabled: always,
        run: sheets,
    },
    CommandDef {
        id: "data.load_sample",
        label: "Load sample data",
        menu: "Data/Sample data",
        shortcut: None,
        doc: "Load a small sample table and create fields for its columns.",
        undoable: true,
        enabled: always,
        run: load_sample,
    },
    CommandDef {
        id: "data.clear",
        label: "Clear data",
        menu: "Data/Clear",
        shortcut: None,
        doc: "Remove the table.",
        undoable: true,
        enabled: has_data,
        run: clear,
    },
    CommandDef {
        id: "data.set_key_column",
        label: "Set photo key column",
        menu: "",
        shortcut: None,
        doc: "{column} The column whose value names each row's photo and card file.",
        undoable: true,
        enabled: has_data,
        run: set_key_column,
    },
    CommandDef {
        id: "data.map_field",
        label: "Map field to column",
        menu: "",
        shortcut: None,
        doc: "{field, column|null} Choose the column a field shows.",
        undoable: true,
        enabled: has_data,
        run: map_field,
    },
    CommandDef {
        id: "data.auto_map",
        label: "Auto-map columns",
        menu: "Data/Auto-map",
        shortcut: None,
        doc: "Map unmapped fields to columns of the same name.",
        undoable: true,
        enabled: has_data,
        run: auto_map,
    },
    CommandDef {
        id: "data.create_fields",
        label: "Create fields from columns",
        menu: "Data/Create fields from columns",
        shortcut: None,
        doc: "Add a text field for every column that has none.",
        undoable: true,
        enabled: has_data,
        run: create_fields,
    },
    CommandDef {
        id: "data.set_cell",
        label: "Edit cell",
        menu: "",
        shortcut: None,
        doc: "{row, column, value} Change one cell.",
        undoable: true,
        enabled: has_data,
        run: set_cell,
    },
    CommandDef {
        id: "data.set_preview",
        label: "Preview row",
        menu: "",
        shortcut: None,
        doc: "{index} Row shown in the design preview.",
        undoable: false,
        enabled: has_data,
        run: set_preview,
    },
];

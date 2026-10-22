use std::path::PathBuf;

use forge_doc::Project;
use forge_format::{SaveOptions, load_project, save_project};
use serde::Deserialize;

use super::{args, ok};
use crate::registry::{CommandDef, always};
use crate::{Engine, EngineError, EngineResult, Value, json};

#[derive(Deserialize)]
struct PathArg {
    path: PathBuf,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SaveArgs {
    path: Option<PathBuf>,
    /// Keep image files as paths instead of copying them into the project.
    link_files: bool,
}

fn new_project(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    e.replace_project(Project::new(), None);
    e.load_warnings.clear();
    e.legacy_source = false;
    ok()
}

fn demo(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    e.replace_project(forge_render::sample::project(), None);
    e.load_warnings.clear();
    e.legacy_source = false;
    ok()
}

fn open(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let PathArg { path } = args(v)?;
    let loaded = load_project(&path).map_err(EngineError::failed)?;
    let legacy = loaded.legacy;
    e.load_warnings = loaded.warnings.clone();
    e.legacy_source = legacy;
    e.replace_project(loaded.project, if legacy { None } else { Some(path.clone()) });
    e.prefs.add_recent(&path);
    e.save_prefs();
    Ok(json!({ "legacy": legacy, "warnings": loaded.warnings }))
}

fn save(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: SaveArgs = args(v)?;
    let mut path = a.path.or_else(|| e.path.clone()).ok_or_else(|| EngineError::invalid("no file name: pass `path`"))?;
    if path.extension().is_none() {
        path.set_extension(forge_format::PROJECT_EXTENSION);
    }
    save_project(&e.project, &path, &SaveOptions { embed_files: !a.link_files }).map_err(EngineError::failed)?;
    e.path = Some(path.clone());
    e.legacy_source = false;
    e.mark_saved();
    e.prefs.add_recent(&path);
    e.save_prefs();
    Ok(json!({ "path": path }))
}

fn recent_remove(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let PathArg { path } = args(v)?;
    e.prefs.remove_recent(&path);
    e.save_prefs();
    ok()
}

pub(crate) const COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "project.new",
        label: "New project",
        menu: "File/New",
        shortcut: Some("Ctrl+N"),
        doc: "Start an empty project (discards the current one).",
        undoable: false,
        enabled: always,
        run: new_project,
    },
    CommandDef {
        id: "project.demo",
        label: "Open demo project",
        menu: "File/Demo project",
        shortcut: None,
        doc: "A ready-made project to try the app: template, fields, rows and portraits.",
        undoable: false,
        enabled: always,
        run: demo,
    },
    CommandDef {
        id: "project.open",
        label: "Open project...",
        menu: "File/Open...",
        shortcut: Some("Ctrl+O"),
        doc: "{path} Open a .forgeproject, or a legacy .idcardproject.",
        undoable: false,
        enabled: always,
        run: open,
    },
    CommandDef {
        id: "project.save",
        label: "Save project",
        menu: "File/Save",
        shortcut: Some("Ctrl+S"),
        doc: "{path?, linkFiles?} Save as .forgeproject (to the current file when no path).",
        undoable: false,
        enabled: always,
        run: save,
    },
    CommandDef {
        id: "project.recent_remove",
        label: "Forget recent project",
        menu: "",
        shortcut: None,
        doc: "{path} Remove a file from the recent list.",
        undoable: false,
        enabled: always,
        run: recent_remove,
    },
];

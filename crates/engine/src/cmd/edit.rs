use super::ok;
use crate::registry::CommandDef;
use crate::{Engine, EngineError, EngineResult, Value};

fn undo(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    if e.undo() { ok() } else { Err(EngineError::failed("nothing to undo")) }
}

fn redo(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    if e.redo() { ok() } else { Err(EngineError::failed("nothing to redo")) }
}

pub(crate) const COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "edit.undo",
        label: "Undo",
        menu: "Edit/Undo",
        shortcut: Some("Ctrl+Z"),
        doc: "Undo the last change.",
        undoable: false,
        enabled: |e| e.can_undo(),
        run: undo,
    },
    CommandDef {
        id: "edit.redo",
        label: "Redo",
        menu: "Edit/Redo",
        shortcut: Some("Ctrl+Shift+Z"),
        doc: "Redo the change that was undone.",
        undoable: false,
        enabled: |e| e.can_redo(),
        run: redo,
    },
];

//! The command table. Each module holds one group.

mod data;
mod design;
mod edit;
mod export;
mod photos;
mod project;
mod render;
mod sheet;

use serde::de::DeserializeOwned;

use crate::registry::CommandDef;
use crate::{EngineError, EngineResult, Value};

pub(crate) const GROUPS: &[&[CommandDef]] =
    &[project::COMMANDS, edit::COMMANDS, data::COMMANDS, photos::COMMANDS, design::COMMANDS, sheet::COMMANDS, export::COMMANDS, render::COMMANDS];

/// Parses command params (`null` counts as `{}`).
pub(crate) fn args<T: DeserializeOwned>(v: &Value) -> EngineResult<T> {
    let v = if v.is_null() { Value::Object(Default::default()) } else { v.clone() };
    serde_json::from_value(v).map_err(|e| EngineError::invalid(format!("bad parameters: {e}")))
}

pub(crate) fn ok() -> EngineResult<Value> {
    Ok(Value::Null)
}

/// A shallow-then-deep JSON merge: objects merge key by key, everything else replaces.
pub(crate) fn merge(into: &mut Value, patch: &Value) {
    match (into, patch) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                match a.get_mut(k) {
                    Some(existing) if existing.is_object() && v.is_object() => merge(existing, v),
                    _ => {
                        a.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (slot, v) => *slot = v.clone(),
    }
}

pub(crate) fn finite(v: f64, lo: f64, hi: f64, default: f64) -> f64 {
    if v.is_finite() { v.clamp(lo, hi) } else { default }
}

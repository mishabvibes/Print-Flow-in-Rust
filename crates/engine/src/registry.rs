use crate::cmd;
use crate::{Engine, EngineResult, Value};

/// One user-visible behaviour.
pub struct CommandDef {
    /// Stable id: `group.verb`.
    pub id: &'static str,
    pub label: &'static str,
    /// Menu path (`File/Open...`), empty when it has no menu entry.
    pub menu: &'static str,
    pub shortcut: Option<&'static str>,
    /// One line for the command palette and docs, naming the params.
    pub doc: &'static str,
    /// Produces an undo step when it changes the project.
    pub undoable: bool,
    pub enabled: fn(&Engine) -> bool,
    pub run: fn(&mut Engine, &Value) -> EngineResult<Value>,
}

pub(crate) fn always(_: &Engine) -> bool {
    true
}

/// Every command, in menu order.
pub fn commands() -> impl Iterator<Item = &'static CommandDef> {
    cmd::GROUPS.iter().flat_map(|g| g.iter())
}

pub fn find_command(id: &str) -> Option<&'static CommandDef> {
    commands().find(|c| c.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_documented() {
        let mut seen = std::collections::HashSet::new();
        for c in commands() {
            assert!(seen.insert(c.id), "duplicate command id {}", c.id);
            assert!(c.id.contains('.'), "{}", c.id);
            assert!(!c.label.is_empty() && !c.doc.is_empty(), "{}", c.id);
        }
        assert!(seen.len() > 40);
    }
}

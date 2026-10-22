use std::sync::Arc;
use std::time::{Duration, Instant};

use forge_doc::Project;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_UNDO: usize = 200;
const MAX_JOURNAL: usize = 10_000;
const COALESCE_WINDOW: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default = "yes")]
    pub ok: bool,
}

fn yes() -> bool {
    true
}

/// Are two project states equal for undo purposes? Cheap: the big parts are compared by pointer.
pub fn same_project(a: &Project, b: &Project) -> bool {
    Arc::ptr_eq(&a.data, &b.data)
        && (Arc::ptr_eq(&a.photos, &b.photos)
            || (a.photos.len() == b.photos.len()
                && a.photos.iter().zip(b.photos.iter()).all(|((ka, va), (kb, vb))| ka == kb && (Arc::ptr_eq(va, vb) || **va == **vb))))
        && a.design == b.design
        && a.crop == b.crop
        && a.sheet == b.sheet
        && a.card_source == b.card_source
        && a.preview_index == b.preview_index
        && a.selected_field == b.selected_field
}

#[derive(Default)]
pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
    last: Option<(String, Instant)>,
    journal: Vec<JournalEntry>,
}

impl History {
    /// Records the state before a change. With a coalesce key equal to the previous change's and
    /// recent enough, the earlier snapshot stands and no new step is added.
    pub fn record(&mut self, before: Project, coalesce: Option<&str>) {
        let now = Instant::now();
        let merge = match (&self.last, coalesce) {
            (Some((k, t)), Some(c)) => k == c && now.duration_since(*t) < COALESCE_WINDOW && !self.undo.is_empty(),
            _ => false,
        };
        self.last = coalesce.map(|c| (c.to_string(), now));
        self.redo.clear();
        if merge {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self, current: Project) -> Option<Project> {
        let p = self.undo.pop()?;
        self.redo.push(current);
        self.last = None;
        Some(p)
    }

    pub fn redo(&mut self, current: Project) -> Option<Project> {
        let p = self.redo.pop()?;
        self.undo.push(current);
        self.last = None;
        Some(p)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last = None;
    }

    pub fn journal(&mut self, id: &str, params: Value, ok: bool) {
        // Undo/redo and pure-UI commands are in the journal too: replaying needs them in order.
        self.journal.push(JournalEntry { id: id.to_string(), params, ok });
        if self.journal.len() > MAX_JOURNAL {
            self.journal.drain(..MAX_JOURNAL / 10);
        }
    }

    pub fn entries(&self) -> &[JournalEntry] {
        &self.journal
    }

    pub fn journal_lines(&self) -> String {
        self.journal.iter().filter_map(|e| serde_json::to_string(e).ok()).collect::<Vec<_>>().join("\n")
    }
}

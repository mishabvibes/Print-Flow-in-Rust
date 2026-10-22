//! Forge Studio engine (layer 5): the only way to change a project.
//!
//! Everything a user can do is a **command**: an id, a label, a menu path, a shortcut, a one-line
//! doc, and a function over `(engine, JSON params)`. The UI, the CLI, scripts and tests all go
//! through [`Engine::execute`], so there is exactly one implementation of each behaviour and one place
//! that handles undo, the journal and failures:
//!
//! * **History** is a stack of cheap `Project` snapshots (the heavy parts are `Arc`s), so undo/redo
//!   is exact and costs a pointer copy per step; slider drags coalesce into one step.
//! * **Journal**: every executed command is recorded (id + params), serialisable as JSON lines and
//!   replayable: a failing session can be reproduced from its journal.
//! * **Guard**: a panic inside a command is caught, the project is rolled back to what it was before the
//!   command, and the caller gets an [`EngineError::Internal`] instead of a crash.
#![forbid(unsafe_code)]

mod cmd;
mod error;
mod guard;
mod history;
pub mod jobs;
mod registry;

use std::path::PathBuf;
use std::sync::Arc;

use forge_doc::Project;
use forge_format::Prefs;
use forge_render::Renderer;
use forge_text::FontDb;
pub use serde_json::{Value, json};

pub use error::{EngineError, EngineResult};
pub use history::JournalEntry;
pub use registry::{CommandDef, commands, find_command};

/// Options of one [`Engine::execute_with`] call.
#[derive(Clone, Debug, Default)]
pub struct ExecOpts {
    /// Commands with the same key within 1.5 s share one undo step (slider and drag gestures).
    pub coalesce: Option<String>,
}

pub struct Engine {
    pub project: Project,
    pub fonts: Arc<FontDb>,
    pub renderer: Arc<Renderer>,
    pub prefs: Prefs,
    /// Write preferences to the user's config folder (tests turn this off).
    pub persist_prefs: bool,
    /// The file the project was opened from / saved to.
    pub path: Option<PathBuf>,
    /// Problems found by the last file load (shown once by the UI).
    pub load_warnings: Vec<String>,
    /// The project came from the previous app's format.
    pub legacy_source: bool,
    history: history::History,
    rev: u64,
    saved_rev: u64,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        let fonts = Arc::new(FontDb::new());
        let renderer = Arc::new(Renderer::new(fonts.clone()));
        Self {
            project: Project::new(),
            fonts,
            renderer,
            prefs: Prefs::default(),
            persist_prefs: true,
            path: None,
            load_warnings: Vec::new(),
            legacy_source: false,
            history: history::History::default(),
            rev: 0,
            saved_rev: 0,
        }
    }

    /// Runs command `id` with `params` (a JSON object, or `null` for none). Never panics.
    pub fn execute(&mut self, id: &str, params: Value) -> EngineResult<Value> {
        self.execute_with(id, params, ExecOpts::default())
    }

    pub fn execute_with(&mut self, id: &str, params: Value, opts: ExecOpts) -> EngineResult<Value> {
        let def = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        if !(def.enabled)(self) {
            return Err(EngineError::Disabled(def.label.to_string()));
        }
        let before = self.project.clone();
        let params_for_journal = params.clone();
        let outcome = guard::catch_panic(|| (def.run)(self, &params));
        let result = match outcome {
            Ok(r) => r,
            Err(msg) => {
                // Roll back whatever the command had half done.
                self.project = before.clone();
                Err(EngineError::Internal(msg))
            }
        };
        match &result {
            Ok(_) => {
                if def.undoable && !history::same_project(&before, &self.project) {
                    self.history.record(before, opts.coalesce.as_deref());
                    self.rev += 1;
                    self.project.clamp_indices();
                }
            }
            Err(_) => {
                // A failed command leaves the document as it was.
                self.project = before;
            }
        }
        self.history.journal(id, params_for_journal, result.is_ok());
        result
    }

    /// Switches the theme and remembers it.
    pub fn set_theme(&mut self, theme: forge_format::Theme) {
        self.prefs.theme = theme;
        self.save_prefs();
    }

    pub(crate) fn save_prefs(&self) {
        if self.persist_prefs {
            self.prefs.save();
        }
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub(crate) fn undo(&mut self) -> bool {
        match self.history.undo(self.project.clone()) {
            Some(p) => {
                self.project = p;
                self.project.clamp_indices();
                self.rev += 1;
                true
            }
            None => false,
        }
    }

    pub(crate) fn redo(&mut self) -> bool {
        match self.history.redo(self.project.clone()) {
            Some(p) => {
                self.project = p;
                self.project.clamp_indices();
                self.rev += 1;
                true
            }
            None => false,
        }
    }

    /// Replaces the whole project (open / new): clears history.
    pub(crate) fn replace_project(&mut self, p: Project, path: Option<PathBuf>) {
        self.project = p;
        self.path = path;
        self.history.clear();
        self.rev += 1;
        self.saved_rev = self.rev;
    }

    pub(crate) fn mark_saved(&mut self) {
        self.saved_rev = self.rev;
    }

    /// Changes whenever the document changes (caches key on it).
    pub fn revision(&self) -> u64 {
        self.rev
    }

    pub fn is_dirty(&self) -> bool {
        self.rev != self.saved_rev
    }

    /// A cheap copy of the project for a background job (export, detection).
    pub fn snapshot(&self) -> Project {
        self.project.clone()
    }

    /// The journal as JSON lines (one `{"id", "params", "ok"}` per line).
    pub fn journal_lines(&self) -> String {
        self.history.journal_lines()
    }

    pub fn journal(&self) -> &[JournalEntry] {
        self.history.entries()
    }

    /// Re-runs journal lines on this engine (failures are reported, not fatal).
    pub fn replay(&mut self, lines: &str) -> Vec<(usize, EngineError)> {
        let mut errs = Vec::new();
        for (i, line) in lines.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<JournalEntry>(line) {
                Ok(e) if e.ok => {
                    if let Err(err) = self.execute(&e.id, e.params) {
                        errs.push((i + 1, err));
                    }
                }
                Ok(_) => {}
                Err(err) => errs.push((i + 1, EngineError::Invalid(format!("bad journal line: {err}")))),
            }
        }
        errs
    }
}

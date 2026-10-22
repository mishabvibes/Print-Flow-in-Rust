//! State every tab shares: the engine, dialogs, background jobs and notifications. Tabs get
//! `&mut Shell`; nothing here draws a panel.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use forge_doc::ImageSource;
use forge_engine::jobs::{self, DetectOutcome, ExportJob};
use forge_engine::{Engine, ExecOpts, Value, json};
use forge_export::Control;

use crate::platform::{self, Platform};
use crate::shortcuts::Action;
use crate::view::ViewCmd;
use crate::workers::{Msg, Workers};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Info,
    Ok,
    Error,
}

pub struct Toast {
    pub text: String,
    pub kind: Kind,
    pub born: f64,
}

pub struct JobState {
    pub label: String,
    pub done: usize,
    pub total: usize,
    pub control: Arc<Control>,
    pub finished: Option<Result<forge_export::Report, String>>,
}

pub enum Confirm {
    Discard(&'static str),
}

pub struct Shell {
    pub engine: Engine,
    pub platform: Box<dyn Platform>,
    pub w: Workers,
    pub toasts: Vec<Toast>,
    pub job: Option<JobState>,
    pub detect: Option<(usize, usize)>,
    pub families: Vec<String>,
    pub confirm: Option<(Confirm, String)>,
    pub now: f64,
    /// Photo keys with a thumbnail request in flight.
    pub thumb_pending: HashSet<String>,
    pub quit_requested: bool,
    pub palette_open: bool,
    /// Keeps the one-time load warnings from repeating.
    pub warnings_shown: usize,
    pub extra: HashMap<String, String>,
    /// Shortcut actions for the active workspace to handle this frame.
    pub actions: Vec<Action>,
    /// A pending zoom command for the active workspace.
    pub view_cmd: Option<ViewCmd>,
    /// Tab: hide the tool strip, option bar, docks and status bar.
    pub hide_panels: bool,
    pub shortcuts_open: bool,
}

impl Shell {
    pub fn new(engine: Engine, platform: Box<dyn Platform>, w: Workers) -> Self {
        let families = engine.fonts.families();
        Self {
            engine,
            platform,
            w,
            toasts: Vec::new(),
            job: None,
            detect: None,
            families,
            confirm: None,
            now: 0.0,
            thumb_pending: HashSet::new(),
            quit_requested: false,
            palette_open: false,
            warnings_shown: 0,
            extra: HashMap::new(),
            actions: Vec::new(),
            view_cmd: None,
            hide_panels: false,
            shortcuts_open: false,
        }
    }

    pub fn toast(&mut self, kind: Kind, text: impl Into<String>) {
        let text = text.into();
        self.toasts.push(Toast { text, kind, born: self.now });
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    /// Runs a command; failures become a toast. Returns the result on success.
    pub fn cmd(&mut self, id: &str, params: Value) -> Option<Value> {
        self.cmd_with(id, params, ExecOpts::default())
    }

    pub fn cmd_with(&mut self, id: &str, params: Value, opts: ExecOpts) -> Option<Value> {
        match self.engine.execute_with(id, params, opts) {
            Ok(v) => Some(v),
            Err(e) => {
                self.toast(Kind::Error, e.to_string());
                None
            }
        }
    }

    /// One gesture (a drag, a slider) = one undo step.
    pub fn cmd_drag(&mut self, id: &str, params: Value, key: &str) -> Option<Value> {
        self.cmd_with(id, params, ExecOpts { coalesce: Some(key.to_string()) })
    }

    pub fn refresh_fonts(&mut self) {
        self.families = self.engine.fonts.families();
    }

    pub fn busy(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.finished.is_none())
    }

    // ---- background jobs -------------------------------------------------------------------

    pub fn start_detect(&mut self, keys: Option<Vec<String>>) {
        if self.detect.is_some() {
            return;
        }
        let items = jobs::detect_items(&self.engine.project, keys.as_deref());
        if items.is_empty() {
            self.toast(Kind::Info, "Every photo has already been processed.");
            return;
        }
        self.detect = Some((0, items.len()));
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ctx = self.w.repaint_handle();
        self.w.spawn(move |tx| {
            let tx2 = tx.clone();
            let progress = move |d: usize, t: usize| {
                let _ = tx2.send(Msg::DetectProgress(d, t));
                ctx.request_repaint();
            };
            let out = jobs::run_detect(&items, &cancel, &progress);
            let _ = tx.send(Msg::Detected(out));
        });
    }

    pub fn start_export(&mut self, label: &str, job: ExportJob) {
        if self.busy() {
            self.toast(Kind::Info, "Another export is still running.");
            return;
        }
        let snapshot = self.engine.snapshot();
        let renderer = self.engine.renderer.clone();
        let tx = self.w.sender();
        let ctx = self.w.repaint_handle();
        let (ptx, pctx) = (tx.clone(), ctx.clone());
        let control = Arc::new(Control::with_progress(move |d, t| {
            let _ = ptx.send(Msg::ExportProgress(d, t));
            pctx.request_repaint();
        }));
        self.job = Some(JobState { label: label.to_string(), done: 0, total: 0, control: control.clone(), finished: None });
        self.w.spawn(move |tx| {
            let r = jobs::run_export(&job, &snapshot, &renderer, &control).map_err(|e| e.to_string());
            let _ = tx.send(Msg::ExportDone(r));
        });
    }

    /// Applies finished background work. Returns the photo messages for the Crop tab and the preview
    /// messages for the canvases (they own their textures).
    pub fn handle(&mut self, msg: Msg, out: &mut Vec<Msg>) {
        match msg {
            Msg::Detected(res) => {
                self.detect = None;
                let n = res.len();
                let failed = res.iter().filter(|r| r.error.is_some()).count();
                let found = res.iter().filter(|r| r.detection.is_some()).count();
                if let Ok(v) = serde_json::to_value::<&Vec<DetectOutcome>>(&res) {
                    self.cmd("photos.apply_detections", json!({ "results": v }));
                }
                let mut s = format!("Detected faces in {found} of {n} photos.");
                if failed > 0 {
                    s.push_str(&format!(" {failed} could not be read."));
                }
                self.toast(if failed > 0 { Kind::Error } else { Kind::Ok }, s);
            }
            Msg::DetectProgress(d, t) => self.detect = Some((d, t)),
            Msg::ExportProgress(d, t) => {
                if let Some(j) = &mut self.job {
                    j.done = d;
                    j.total = t;
                }
            }
            Msg::ExportDone(r) => {
                if let Some(j) = &mut self.job {
                    j.finished = Some(r);
                }
            }
            Msg::FontsReady => self.refresh_fonts(),
            other => out.push(other),
        }
    }

    // ---- commands that need a dialog ---------------------------------------------------------

    /// Runs a command the way the UI does: commands that need a file or folder ask for one first.
    pub fn run_ui_command(&mut self, id: &str) {
        use platform as p;
        let proj_name =
            self.engine.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
        match id {
            "project.new" => {
                if self.engine.is_dirty() {
                    self.confirm = Some((Confirm::Discard("project.new"), "Start a new project? Unsaved changes will be lost.".into()));
                } else {
                    self.cmd("project.new", Value::Null);
                }
            }
            "project.demo" => {
                if self.engine.is_dirty() {
                    self.confirm = Some((Confirm::Discard("project.demo"), "Open the demo project? Unsaved changes will be lost.".into()));
                } else {
                    self.cmd("project.demo", Value::Null);
                }
            }
            "project.open" => {
                if self.engine.is_dirty() {
                    self.confirm = Some((Confirm::Discard("project.open"), "Open another project? Unsaved changes will be lost.".into()));
                } else {
                    self.open_project_dialog();
                }
            }
            "project.save" => {
                let path = match &self.engine.path {
                    Some(p) => Some(p.clone()),
                    None => self.platform.save_file("Save project", &format!("{proj_name}.forgeproject"), &[p::SAVE_PROJECT]),
                };
                if let Some(path) = path
                    && self.cmd("project.save", json!({ "path": path })).is_some()
                {
                    self.toast(Kind::Ok, "Project saved.");
                }
            }
            "project.save_as" => {
                if let Some(path) = self.platform.save_file("Save project as", &format!("{proj_name}.forgeproject"), &[p::SAVE_PROJECT])
                    && self.cmd("project.save", json!({ "path": path })).is_some()
                {
                    self.toast(Kind::Ok, "Project saved.");
                }
            }
            "data.load" => {
                if let Some(path) = self.platform.open_file("Load data", &[p::TABLES]) {
                    self.load_data(path);
                }
            }
            "photos.add" => {
                let files = self.platform.open_files("Add photos", &[p::IMAGES]);
                self.add_photos(json!({ "paths": files }), "photos.add", !files_empty(&files));
            }
            "photos.add_folder" => {
                if let Some(dir) = self.platform.open_folder("Add photo folder") {
                    self.add_photos(json!({ "path": dir }), "photos.add_folder", true);
                }
            }
            "photos.detect" => self.start_detect(None),
            "photos.replace_background" => {
                if self.cmd("photos.replace_background", json!({ "keys": "all" })).is_some() {
                    self.toast(Kind::Ok, "Backgrounds replaced.");
                }
            }
            "design.set_template" => {
                if let Some(path) = self.platform.open_file("Template image", &[p::IMAGES]) {
                    self.cmd("design.set_template", json!({ "path": path }));
                }
            }
            "design.save_preset" => {
                if let Some(path) = self.platform.save_file("Save design preset", "design.json", &[p::JSON]) {
                    if self.cmd("design.save_preset", json!({ "path": path })).is_some() {
                        self.toast(Kind::Ok, "Design preset saved.");
                    }
                }
            }
            "design.load_preset" => {
                if let Some(path) = self.platform.open_file("Load design preset", &[p::JSON]) {
                    self.cmd("design.load_preset", json!({ "path": path }));
                }
            }
            "sheet.save_layout" => {
                if let Some(path) = self.platform.save_file("Save layout preset", "layout.json", &[p::JSON]) {
                    if self.cmd("sheet.save_layout", json!({ "path": path })).is_some() {
                        self.toast(Kind::Ok, "Layout preset saved.");
                    }
                }
            }
            "sheet.load_layout" => {
                if let Some(path) = self.platform.open_file("Load layout preset", &[p::JSON]) {
                    self.cmd("sheet.load_layout", json!({ "path": path }));
                }
            }
            "sheet.add_files" => {
                let files = self.platform.open_files("Add card images", &[p::IMAGES]);
                if !files.is_empty() {
                    self.cmd("sheet.add_files", json!({ "paths": files }));
                }
            }
            "fonts.add" => {
                if let Some(path) = self.platform.open_file("Add font", &[p::FONTS]) {
                    if self.cmd("fonts.add", json!({ "path": path })).is_some() {
                        self.refresh_fonts();
                        self.toast(Kind::Ok, "Font added.");
                    }
                }
            }
            "export.cards_zip" => {
                if let Some(path) = self.platform.save_file("Export cards", &format!("{proj_name}-cards.zip"), &[p::ZIP]) {
                    self.start_export("Exporting cards", ExportJob::CardsZip { path });
                }
            }
            "export.cropped_zip" => {
                if let Some(path) = self.platform.save_file("Export cropped photos", &format!("{proj_name}-cropped.zip"), &[p::ZIP]) {
                    self.start_export("Exporting cropped photos", ExportJob::CroppedZip { path, long_edge: 600, quality: 92 });
                }
            }
            "export.sheets_pdf" => {
                if let Some(path) = self.platform.save_file("Export sheets", &format!("{proj_name}-sheets.pdf"), &[p::PDF]) {
                    let dpi = self.engine.project.sheet.dpi;
                    self.start_export("Exporting PDF", ExportJob::SheetsPdf { path, dpi, jpeg_quality: Some(92) });
                }
            }
            "export.sheets_zip" => {
                if let Some(path) = self.platform.save_file("Export sheets", &format!("{proj_name}-sheets.zip"), &[p::ZIP]) {
                    let dpi = self.engine.project.sheet.dpi;
                    self.start_export("Exporting sheet images", ExportJob::SheetsZip { path, dpi });
                }
            }
            other => {
                self.cmd(other, Value::Null);
            }
        }
    }

    pub fn open_project_dialog(&mut self) {
        if let Some(path) = self.platform.open_file("Open project", &[platform::PROJECTS]) {
            self.open_project(path);
        }
    }

    pub fn open_project(&mut self, path: PathBuf) {
        if self.cmd("project.open", json!({ "path": path })).is_some() {
            let warnings = std::mem::take(&mut self.engine.load_warnings);
            if self.engine.legacy_source {
                self.toast(Kind::Info, "Imported a project from the previous version. Saving writes the new format.");
            }
            for w in warnings.into_iter().take(3) {
                self.toast(Kind::Error, w);
            }
        } else {
            // Forget a dead entry in the recent list.
            self.cmd("project.recent_remove", json!({ "path": path }));
        }
    }

    pub fn load_data(&mut self, path: PathBuf) {
        if self.cmd("data.load", json!({ "path": path, "createFields": self.engine.project.design.fields.is_empty() })).is_some() {
            let n = self.engine.project.data.len();
            self.toast(Kind::Ok, format!("Loaded {n} rows."));
        }
    }

    fn add_photos(&mut self, params: Value, id: &str, go: bool) {
        if !go {
            return;
        }
        let before = self.engine.project.photos.len();
        if self.cmd(id, params).is_some() {
            let added = self.engine.project.photos.len().saturating_sub(before);
            self.toast(Kind::Ok, format!("Added {added} photos."));
            if self.engine.project.crop.auto_crop_on_import {
                self.start_detect(None);
            }
        }
    }

    pub fn confirmed(&mut self, id: &str) {
        match id {
            "project.open" => self.open_project_dialog_unguarded(),
            "app.quit" => self.quit_requested = true,
            other => {
                self.cmd(other, Value::Null);
            }
        }
    }

    fn open_project_dialog_unguarded(&mut self) {
        self.open_project_dialog();
    }
}

fn files_empty(v: &[PathBuf]) -> bool {
    v.is_empty()
}

/// A cheap identity for an image source, so caches notice a replaced photo.
pub fn source_version(s: &ImageSource) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match s {
        ImageSource::None => 0u8.hash(&mut h),
        ImageSource::Path(p) => p.hash(&mut h),
        ImageSource::Data(d) => (Arc::as_ptr(d) as usize).hash(&mut h),
    }
    h.finish()
}

//! Forge Studio desktop app: an eframe window around `forge_ui_egui::App`.
//!
//! Usage: `forge-studio [project-file]`. For automated checks, `--screenshot out.png [--tab
//! crop|design|arrange] [--demo]` renders one frame set, saves the window and exits.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use forge_engine::{Engine, Value, json};
use forge_ui_egui::App;
use forge_ui_egui::platform::{FileFilter, Platform};

struct Dialogs;

fn dialog(title: &str, filters: &[FileFilter]) -> rfd::FileDialog {
    let mut d = rfd::FileDialog::new().set_title(title);
    for f in filters {
        d = d.add_filter(f.name, f.extensions);
    }
    d
}

impl Platform for Dialogs {
    fn open_file(&self, title: &str, filters: &[FileFilter]) -> Option<PathBuf> {
        dialog(title, filters).pick_file()
    }
    fn open_files(&self, title: &str, filters: &[FileFilter]) -> Vec<PathBuf> {
        dialog(title, filters).pick_files().unwrap_or_default()
    }
    fn open_folder(&self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }
    fn save_file(&self, title: &str, file_name: &str, filters: &[FileFilter]) -> Option<PathBuf> {
        dialog(title, filters).set_file_name(file_name).save_file()
    }
}

struct Options {
    open: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    tab: String,
    demo: bool,
    light: bool,
}

fn parse_args() -> Options {
    let mut o = Options { open: None, screenshot: None, tab: "design".into(), demo: false, light: false };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--screenshot" => o.screenshot = it.next().map(PathBuf::from),
            "--tab" => o.tab = it.next().unwrap_or_default(),
            "--demo" => o.demo = true,
            "--light" => o.light = true,
            other if !other.starts_with("--") => o.open = Some(PathBuf::from(other)),
            _ => {}
        }
    }
    o
}

struct Host {
    app: App,
    shot: Option<PathBuf>,
    frames: u32,
    started: Option<std::time::Instant>,
    requested: bool,
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        let Some(path) = self.shot.clone() else { return };
        self.frames += 1;
        ui.ctx().request_repaint();
        // Give the background renders time to land, then take the picture.
        let t0 = *self.started.get_or_insert_with(std::time::Instant::now);
        if t0.elapsed().as_secs_f32() > 12.0 && !self.requested {
            self.requested = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let shot = ui.ctx().input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let (w, h) = (img.width() as u32, img.height() as u32);
            let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Ok(im) = forge_image::Image::from_rgba(w, h, rgba)
                && let Ok(png) = forge_image::encode_png(&im, true)
            {
                let _ = std::fs::write(&path, png);
            }
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn icon() -> Option<egui::IconData> {
    let img = forge_image::decode(include_bytes!("../../../assets/app-icon/logo.png")).ok()?;
    Some(egui::IconData { rgba: img.rgba, width: img.width, height: img.height })
}

fn open(engine: &mut Engine, p: &Path) {
    if engine.execute("project.open", json!({ "path": p })).is_err() {
        log::warn!("could not open {}", p.display());
    }
}

fn main() -> eframe::Result {
    env_logger::init();
    let o = parse_args();
    let mut viewport =
        egui::ViewportBuilder::default().with_title("Forge Studio").with_inner_size([1400.0, 860.0]).with_min_inner_size([980.0, 600.0]);
    if let Some(i) = icon() {
        viewport = viewport.with_icon(i);
    }
    eframe::run_native(
        "Forge Studio",
        eframe::NativeOptions { viewport, ..Default::default() },
        Box::new(move |cc| {
            let mut engine = Engine::new();
            engine.prefs = forge_format::Prefs::load();
            for f in engine.prefs.fonts.clone() {
                engine.fonts.add_file(&f);
            }
            if o.demo {
                let _ = engine.execute("project.demo", Value::Null);
            } else if let Some(p) = &o.open {
                open(&mut engine, p);
            }
            if o.screenshot.is_some() {
                engine.persist_prefs = false;
            }
            if o.light {
                engine.prefs.theme = forge_format::Theme::Light;
            }
            let mut app = App::new(&cc.egui_ctx, engine, Box::new(Dialogs));
            app.tab = match o.tab.as_str() {
                "crop" => forge_ui_egui::Tab::Crop,
                "arrange" => forge_ui_egui::Tab::Arrange,
                _ => forge_ui_egui::Tab::Design,
            };
            Ok(Box::new(Host { app, shot: o.screenshot.clone(), frames: 0, started: None, requested: false }))
        }),
    )
}

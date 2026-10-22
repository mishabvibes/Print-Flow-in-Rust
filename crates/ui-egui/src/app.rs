use egui::{Color32, Key, RichText, Ui};
use forge_engine::{Engine, Value, commands};
use forge_format::Theme;

use crate::platform::Platform;
use crate::shell::{Confirm, Kind, Shell};
use crate::shortcuts::{self, Action, BINDINGS};
use crate::tabs::{arrange, crop, design};
use crate::theme::{self, Tokens};
use crate::view::ViewCmd;
use crate::widgets::{dim, primary_button};
use crate::workers::{Msg, Slot, Workers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Crop,
    Design,
    Arrange,
}

pub struct App {
    pub shell: Shell,
    pub tab: Tab,
    crop: crop::State,
    design: design::State,
    arrange: arrange::State,
    palette: String,
    about: bool,
    dark_applied: Option<bool>,
    started: bool,
    palette_focus: bool,
}

impl App {
    pub fn new(ctx: &egui::Context, engine: Engine, platform: Box<dyn Platform>) -> Self {
        let w = Workers::new(ctx.clone());
        let fonts = engine.fonts.clone();
        // The font scan runs on its own single-thread pool so that it never starves the previews
        // and thumbnails that share the global pool.
        let (tx, scan_ctx) = (w.sender(), ctx.clone());
        std::thread::spawn(move || {
            match rayon::ThreadPoolBuilder::new().num_threads(1).build() {
                Ok(pool) => pool.install(|| fonts.scan_system_fonts()),
                Err(_) => fonts.scan_system_fonts(),
            }
            let _ = tx.send(Msg::FontsReady);
            scan_ctx.request_repaint();
        });
        Self {
            shell: Shell::new(engine, platform, w),
            tab: Tab::Design,
            crop: crop::State::default(),
            design: design::new_state(),
            arrange: arrange::State::default(),
            palette: String::new(),
            about: false,
            dark_applied: None,
            started: false,
            palette_focus: false,
        }
    }

    fn dark(&self) -> bool {
        self.shell.engine.prefs.theme == Theme::Dark
    }

    fn blank(&self) -> bool {
        let p = &self.shell.engine.project;
        p.design.template.is_none() && p.design.fields.is_empty() && p.data.is_empty() && p.photos.is_empty() && !self.started
    }

    /// Draws one frame. Call from the host's `ui` callback.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let dark = self.dark();
        if self.dark_applied != Some(dark) {
            theme::apply(&ctx, dark);
            self.dark_applied = Some(dark);
        }
        let t = Tokens::of(dark);
        self.shell.now = ctx.input(|i| i.time);
        self.pump(&ctx);
        self.shortcuts(&ctx);

        egui::Panel::top("menubar")
            .frame(egui::Frame::new().fill(t.header).stroke(egui::Stroke::new(1.0, t.line)).inner_margin(egui::Margin::symmetric(6, 2)))
            .show(ui, |ui| self.menubar(ui, &t));

        if self.blank() {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(t.canvas)).show(ui, |ui| self.start_page(ui, &t));
        } else {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| match self.tab {
                Tab::Crop => crop::show(ui, &mut self.shell, &mut self.crop, &t, dark),
                Tab::Design => design::show(ui, &mut self.shell, &mut self.design, &t, dark),
                Tab::Arrange => arrange::show(ui, &mut self.shell, &mut self.arrange, &t, dark),
            });
        }
        self.dialogs(&ctx, &t);
        self.toasts(&ctx, &t);

        let name = self
            .shell
            .engine
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        let dirty = if self.shell.engine.is_dirty() { " •" } else { "" };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{name}{dirty} — Forge Studio")));
        if self.shell.detect.is_some() || self.shell.busy() || self.design.preview.pending() || self.arrange.preview.pending() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    fn pump(&mut self, ctx: &egui::Context) {
        let mut rest = Vec::new();
        for m in self.shell.w.poll() {
            self.shell.handle(m, &mut rest);
        }
        for m in rest {
            match m {
                Msg::Preview { slot: Slot::Design, token, key, image } => self.design.preview.accept(ctx, "design-preview", token, key, image),
                Msg::Preview { slot: Slot::Sheet, token, key, image } => self.arrange.preview.accept(ctx, "sheet-preview", token, key, image),
                m @ Msg::Photo { .. } => self.crop.on_photo(ctx, &mut self.shell, m),
                _ => {}
            }
        }
        // Load warnings from a project opened through a command (e.g. by the host at start-up).
        if !self.shell.engine.load_warnings.is_empty() {
            let w = std::mem::take(&mut self.shell.engine.load_warnings);
            for s in w.into_iter().take(3) {
                self.shell.toast(Kind::Error, s);
            }
        }
        // A finished export becomes a toast and closes the dialog's busy state.
        if let Some(j) = &self.shell.job {
            if let Some(r) = &j.finished {
                let (kind, text) = match r {
                    Ok(rep) if rep.failed.is_empty() => (Kind::Ok, format!("Wrote {} items to {}", rep.written, rep.path.display())),
                    Ok(rep) => (Kind::Error, format!("Wrote {} items; {} failed (first: {})", rep.written, rep.failed.len(), rep.failed[0].1)),
                    Err(e) => (Kind::Error, e.clone()),
                };
                let _ = ctx;
                self.shell.toast(kind, text);
                self.shell.job = None;
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        for a in shortcuts::poll(ctx, typing) {
            self.do_action(a);
        }
        if self.shell.quit_requested {
            self.shell.quit_requested = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Runs one shortcut or menu action. Anything that belongs to the active workspace is queued for it.
    pub fn do_action(&mut self, a: Action) {
        let blank = self.blank();
        match a {
            Action::New => self.shell.run_ui_command("project.new"),
            Action::Open => {
                self.started = true;
                self.shell.run_ui_command("project.open");
            }
            Action::Save => self.shell.run_ui_command("project.save"),
            Action::SaveAs => self.shell.run_ui_command("project.save_as"),
            Action::Export => self.shell.run_ui_command(match self.tab {
                Tab::Crop => "export.cropped_zip",
                Tab::Design => "export.cards_zip",
                Tab::Arrange => "export.sheets_pdf",
            }),
            Action::Quit => {
                if self.shell.engine.is_dirty() {
                    self.shell.confirm = Some((Confirm::Discard("app.quit"), "Quit? Unsaved changes will be lost.".into()));
                } else {
                    self.shell.quit_requested = true;
                }
            }
            Action::Undo => {
                self.shell.cmd("edit.undo", Value::Null);
            }
            Action::Redo => {
                self.shell.cmd("edit.redo", Value::Null);
            }
            Action::ZoomIn => self.shell.view_cmd = Some(ViewCmd::In),
            Action::ZoomOut => self.shell.view_cmd = Some(ViewCmd::Out),
            Action::ZoomFit => self.shell.view_cmd = Some(ViewCmd::Fit),
            Action::Zoom100 => self.shell.view_cmd = Some(ViewCmd::Actual),
            Action::TogglePanels => self.shell.hide_panels = !self.shell.hide_panels,
            Action::TabCrop => self.switch(Tab::Crop),
            Action::TabDesign => self.switch(Tab::Design),
            Action::TabArrange => self.switch(Tab::Arrange),
            Action::Palette => {
                self.shell.palette_open = !self.shell.palette_open;
                self.palette_focus = true;
            }
            Action::Help => self.shell.shortcuts_open = !self.shell.shortcuts_open,
            other => {
                if !blank {
                    self.shell.actions.push(other);
                }
            }
        }
    }

    fn switch(&mut self, tab: Tab) {
        self.tab = tab;
        self.started = true;
        self.shell.actions.clear();
        self.shell.view_cmd = None;
    }

    fn menubar(&mut self, ui: &mut Ui, t: &Tokens) {
        egui::MenuBar::new().ui(ui, |ui| {
            // Menus come from the command table, so a new command shows up here without UI code.
            let mut groups: Vec<(&str, Vec<&'static forge_engine::CommandDef>)> = Vec::new();
            for c in commands() {
                let mut parts = c.menu.splitn(2, '/');
                let (Some(top), Some(_)) = (parts.next(), parts.next()) else { continue };
                match groups.iter_mut().find(|(g, _)| *g == top) {
                    Some((_, v)) => v.push(c),
                    None => groups.push((top, vec![c])),
                }
            }
            let order = ["File", "Edit", "Data", "Photos", "Design", "Sheets"];
            groups.sort_by_key(|(g, _)| order.iter().position(|o| o == g).unwrap_or(99));
            for (g, items) in groups {
                ui.menu_button(g, |ui| {
                    for c in items {
                        let label = c.menu.split_once('/').map_or(c.label, |x| x.1);
                        let enabled = (c.enabled)(&self.shell.engine) && !(c.id.starts_with("export.") && self.shell.busy());
                        let mut b = egui::Button::new(label);
                        if let Some(s) = c.shortcut {
                            b = b.shortcut_text(s.replace("Ctrl", if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" }));
                        }
                        if ui.add_enabled(enabled, b).on_hover_text(c.doc).clicked() {
                            self.shell.run_ui_command(c.id);
                            ui.close();
                        }
                    }
                    if g == "File" {
                        if ui.button("Save as…").on_hover_text(shortcuts::label(ui.ctx(), Action::SaveAs).unwrap_or_default()).clicked() {
                            self.shell.run_ui_command("project.save_as");
                            ui.close();
                        }
                        ui.separator();
                        ui.menu_button("Recent", |ui| {
                            let recent = self.shell.engine.prefs.recent.clone();
                            if recent.is_empty() {
                                ui.label("(none)");
                            }
                            for r in recent {
                                if ui.button(&r.name).on_hover_text(r.path.display().to_string()).clicked() {
                                    self.shell.open_project(r.path.clone());
                                    ui.close();
                                }
                            }
                        });
                    }
                });
            }
            ui.menu_button("View", |ui| {
                for (label, act) in
                    [("Zoom in", Action::ZoomIn), ("Zoom out", Action::ZoomOut), ("Fit on screen", Action::ZoomFit), ("Actual size", Action::Zoom100)]
                {
                    let b = egui::Button::new(label).shortcut_text(shortcuts::label(ui.ctx(), act).unwrap_or_default());
                    if ui.add(b).clicked() {
                        self.do_action(act);
                        ui.close();
                    }
                }
                ui.separator();
                let b = egui::Button::new("Show / hide panels").shortcut_text(shortcuts::label(ui.ctx(), Action::TogglePanels).unwrap_or_default());
                if ui.add(b).clicked() {
                    self.do_action(Action::TogglePanels);
                    ui.close();
                }
                let b = egui::Button::new("Snap on / off").shortcut_text(shortcuts::label(ui.ctx(), Action::ToggleSnap).unwrap_or_default());
                if ui.add(b).clicked() {
                    self.do_action(Action::ToggleSnap);
                    ui.close();
                }
            });
            ui.menu_button("Help", |ui| {
                let b = egui::Button::new("Keyboard shortcuts").shortcut_text(shortcuts::label(ui.ctx(), Action::Help).unwrap_or_default());
                if ui.add(b).clicked() {
                    self.shell.shortcuts_open = true;
                    ui.close();
                }
                let b = egui::Button::new("Command palette").shortcut_text(shortcuts::label(ui.ctx(), Action::Palette).unwrap_or_default());
                if ui.add(b).clicked() {
                    self.shell.palette_open = true;
                    self.palette_focus = true;
                    ui.close();
                }
                if ui.button("About").clicked() {
                    self.about = true;
                    ui.close();
                }
            });
            ui.separator();
            for (tab, label, act) in [
                (Tab::Crop, "Crop Photos", Action::TabCrop),
                (Tab::Design, "Design & Generate", Action::TabDesign),
                (Tab::Arrange, "Arrange & Print", Action::TabArrange),
            ] {
                let on = self.tab == tab;
                let r = ui.selectable_label(on, RichText::new(label).strong()).on_hover_text(shortcuts::label(ui.ctx(), act).unwrap_or_default());
                if r.clicked() {
                    self.switch(tab);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let dark = self.dark();
                if ui.button(if dark { "Light" } else { "Dark" }).on_hover_text("Switch theme").clicked() {
                    self.shell.engine.set_theme(if dark { Theme::Light } else { Theme::Dark });
                }
                if ui.add_enabled(self.shell.engine.can_redo(), egui::Button::new("Redo")).on_hover_text("Redo").clicked() {
                    self.shell.cmd("edit.redo", Value::Null);
                }
                if ui.add_enabled(self.shell.engine.can_undo(), egui::Button::new("Undo")).on_hover_text("Undo").clicked() {
                    self.shell.cmd("edit.undo", Value::Null);
                }
            });
        });
        let _ = t;
    }

    fn start_page(&mut self, ui: &mut Ui, t: &Tokens) {
        ui.vertical_centered(|ui| {
            ui.add_space(70.0);
            ui.label(RichText::new("Forge Studio").size(34.0).strong());
            dim(ui, t, "Crop photos, merge data into ID card designs, and print them on sheets.");
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 480.0).max(0.0) / 2.0);
                if primary_button(ui, t, "Start a new project").clicked() {
                    self.started = true;
                }
                if ui.button("Open project…").clicked() {
                    self.shell.run_ui_command("project.open");
                    self.started = true;
                }
                if ui.button("Try the demo").clicked() {
                    self.shell.run_ui_command("project.demo");
                    self.started = true;
                }
            });
            ui.add_space(18.0);
            let recent = self.shell.engine.prefs.recent.clone();
            if !recent.is_empty() {
                ui.label(RichText::new("Recent").strong());
                for r in recent.iter().take(8) {
                    if ui.link(&r.name).on_hover_text(r.path.display().to_string()).clicked() {
                        self.shell.open_project(r.path.clone());
                        self.started = true;
                    }
                }
            }
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context, t: &Tokens) {
        // Confirmations.
        if let Some((Confirm::Discard(id), msg)) = self.shell.confirm.as_ref().map(|(c, m)| {
            (
                match c {
                    Confirm::Discard(i) => Confirm::Discard(i),
                },
                m.clone(),
            )
        }) {
            let mut close = false;
            egui::Window::new("Unsaved changes").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
                ctx,
                |ui| {
                    ui.label(msg);
                    ui.horizontal(|ui| {
                        if ui.button("Discard and continue").clicked() {
                            self.shell.confirmed(id);
                            self.started = true;
                            close = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                },
            );
            if close {
                self.shell.confirm = None;
            }
        }
        // Export progress.
        if let Some(j) = &self.shell.job {
            let (label, done, total, control) = (j.label.clone(), j.done, j.total, j.control.clone());
            egui::Window::new(label).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.add(egui::ProgressBar::new(done as f32 / total.max(1) as f32).desired_width(320.0).text(format!("{done} / {total}")));
                if ui.button("Cancel").clicked() {
                    control.cancel();
                }
            });
        }
        // Command palette.
        if self.shell.palette_open {
            let mut run: Option<&'static str> = None;
            egui::Window::new("Commands")
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
                .fixed_size([460.0, 320.0])
                .show(ctx, |ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut self.palette).hint_text("Type a command…").desired_width(f32::INFINITY));
                    if self.palette_focus {
                        r.request_focus();
                        self.palette_focus = false;
                    }
                    let q = self.palette.to_lowercase();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for c in commands().filter(|c| q.is_empty() || c.label.to_lowercase().contains(&q) || c.id.contains(&q)) {
                            let enabled = (c.enabled)(&self.shell.engine);
                            let resp = ui
                                .add_enabled(enabled, egui::Button::new(c.label).frame(false).min_size(egui::vec2(ui.available_width(), 22.0)))
                                .on_hover_text(c.doc);
                            if resp.clicked() {
                                run = Some(c.id);
                            }
                        }
                    });
                });
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.shell.palette_open = false;
            }
            if let Some(id) = run {
                self.shell.palette_open = false;
                self.palette.clear();
                self.started = true;
                self.shell.run_ui_command(id);
            }
        }
        if self.shell.shortcuts_open {
            let mut open = true;
            egui::Window::new("Keyboard shortcuts")
                .open(&mut open)
                .collapsible(false)
                .default_width(560.0)
                .default_height(520.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let mut groups: Vec<&str> = Vec::new();
                        for b in BINDINGS {
                            if !groups.contains(&b.group) {
                                groups.push(b.group);
                            }
                        }
                        for g in groups {
                            ui.label(RichText::new(g).strong());
                            egui::Grid::new(("sc", g)).num_columns(2).spacing([24.0, 3.0]).show(ui, |ui| {
                                let mut shown: Vec<&'static str> = Vec::new();
                                for b in BINDINGS.iter().filter(|b| b.group == g) {
                                    if shown.contains(&b.desc) {
                                        continue;
                                    }
                                    shown.push(b.desc);
                                    let keys: Vec<String> = BINDINGS
                                        .iter()
                                        .filter(|o| o.desc == b.desc && o.group == g)
                                        .map(|o| ctx.format_shortcut(&egui::KeyboardShortcut::new(o.mods, o.key)))
                                        .collect();
                                    ui.label(RichText::new(keys.join("   or   ")).monospace().color(t.accent));
                                    ui.label(b.desc);
                                    ui.end_row();
                                }
                            });
                            ui.add_space(8.0);
                        }
                        ui.label(RichText::new("Mouse").strong());
                        egui::Grid::new("sc_mouse").num_columns(2).spacing([24.0, 3.0]).show(ui, |ui| {
                            for (k, d) in [
                                ("Wheel", "Pan the canvas"),
                                ("Ctrl + wheel   or   Alt + wheel", "Zoom about the pointer (pinch works too)"),
                                ("Space + drag   or   middle button drag", "Pan with any tool"),
                                ("Drag a field", "Move it (Alt disables snapping)"),
                                ("Drag a handle", "Resize (Shift keeps the ratio)"),
                                ("Drag the round handle", "Rotate (Shift snaps to 15°)"),
                                ("Crop: drag / wheel", "Move / zoom the photo inside the frame"),
                            ] {
                                ui.label(RichText::new(k).monospace().color(t.accent));
                                ui.label(d);
                                ui.end_row();
                            }
                        });
                    });
                });
            if !open {
                self.shell.shortcuts_open = false;
            }
        }
        if self.about {
            egui::Window::new("About Forge Studio").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
                ctx,
                |ui| {
                    ui.label(RichText::new("Forge Studio").size(20.0).strong());
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    dim(ui, t, "Pure Rust: vello_cpu rendering, skrifa/harfrust text, a built-in TFLite interpreter for face detection.");
                    if ui.button("Close").clicked() {
                        self.about = false;
                    }
                },
            );
        }
    }

    fn toasts(&mut self, ctx: &egui::Context, t: &Tokens) {
        let now = self.shell.now;
        self.shell.toasts.retain(|x| now - x.born < if x.kind == Kind::Error { 9.0 } else { 4.0 });
        if self.shell.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        egui::Area::new(egui::Id::new("toasts")).anchor(egui::Align2::RIGHT_BOTTOM, [-14.0, -40.0]).order(egui::Order::Foreground).show(ctx, |ui| {
            for x in &self.shell.toasts {
                let col = match x.kind {
                    Kind::Info => t.accent,
                    Kind::Ok => t.ok,
                    Kind::Error => t.bad,
                };
                egui::Frame::new()
                    .fill(t.raised)
                    .stroke(egui::Stroke::new(1.0, col))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_max_width(380.0);
                        ui.label(RichText::new(&x.text).color(if x.kind == Kind::Error { Color32::from_rgb(255, 160, 150) } else { t.text }));
                    });
            }
        });
    }
}

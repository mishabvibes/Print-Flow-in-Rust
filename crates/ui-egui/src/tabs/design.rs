//! "Design & Generate": the card canvas with draggable fields (Move / Text / Photo / Hand / Zoom
//! tools), an options bar, a docked Properties / Layers / Template / Data column and a status bar.
//! All edits are `design.update_field` commands; a drag is one undo step.

use std::hash::{Hash, Hasher};

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use forge_doc::{Align, Field, FieldKind, Overflow, ScriptPos, Shape, TextCase, VAlign, Wrap};
use forge_engine::{Value, json};
use forge_render::RenderOpts;

use crate::geometry::{self, Guides, Handle, R};
use crate::shell::Shell;
use crate::shortcuts::Action;
use crate::theme::{DOCK_W, TOOL_W, Tokens};
use crate::view::{View, ViewCmd};
use crate::widgets::{self, dim, dock, icons, section, tool_button};
use crate::workers::{Preview, Slot};

#[derive(Clone)]
enum Drag {
    Move { id: String, grab: (f64, f64), start: R },
    Resize { id: String, handle: Handle },
    Rotate { id: String },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Move,
    Text,
    Photo,
    Hand,
    Zoom,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Move => "Move",
            Tool::Text => "Text field",
            Tool::Photo => "Photo field",
            Tool::Hand => "Hand",
            Tool::Zoom => "Zoom",
        }
    }
}

pub struct State {
    pub preview: Preview,
    drag: Option<Drag>,
    guides: Guides,
    pub show_table: bool,
    pub snap: bool,
    font_filter: String,
    pub tool: Tool,
    pub view: View,
    /// Where a Text/Photo tool drag began (screen space).
    create: Option<Pos2>,
    /// Artwork coordinates under the pointer, for the status bar.
    cursor: Option<(f64, f64)>,
    /// (canvas area, fitted width, template width) of the last frame.
    last: Option<(Rect, f32, f32)>,
}

pub fn new_state() -> State {
    State {
        preview: Preview::default(),
        drag: None,
        guides: Guides::default(),
        show_table: true,
        snap: true,
        font_filter: String::new(),
        tool: Tool::Move,
        view: View::default(),
        create: None,
        cursor: None,
        last: None,
    }
}

fn tpl_size(sh: &Shell) -> Option<(f64, f64)> {
    sh.engine.project.design.template.as_ref().map(|t| (f64::from(t.w.max(1)), f64::from(t.h.max(1))))
}

/// A field's rectangle in template pixels.
fn field_px(f: &Field, tw: f64, th: f64) -> R {
    R::new(f.x / 100.0 * tw, f.y / 100.0 * th, f.w / 100.0 * tw, f.h / 100.0 * th)
}

fn rot(p: (f64, f64), a: f64) -> (f64, f64) {
    let (s, c) = a.sin_cos();
    (p.0 * c - p.1 * s, p.0 * s + p.1 * c)
}

fn bar_frame(t: &Tokens) -> egui::Frame {
    egui::Frame::new().fill(t.panel).stroke(Stroke::new(1.0, t.line)).inner_margin(egui::Margin::symmetric(8, 4))
}

pub fn show(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens, dark: bool) {
    do_actions(sh, st);
    if !sh.hide_panels {
        egui::Panel::top("design_options").exact_size(32.0).frame(bar_frame(t)).show(ui, |ui| options_bar(ui, sh, st, t));
        egui::Panel::left("design_tools")
            .exact_size(TOOL_W)
            .resizable(false)
            .frame(egui::Frame::new().fill(t.header).stroke(Stroke::new(1.0, t.line)).inner_margin(egui::Margin::symmetric(3, 6)))
            .show(ui, |ui| tool_strip(ui, st, t));
        egui::Panel::right("design_dock")
            .default_size(DOCK_W)
            .size_range(240.0..=520.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(t.panel).stroke(Stroke::new(1.0, t.line)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    dock(ui, t, "design_props", "Properties", true, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        inspector(ui, sh, st, t)
                    });
                    dock(ui, t, "design_layers", "Layers", true, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        layers(ui, sh, t)
                    });
                    dock(ui, t, "design_template", "Template", true, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        template_panel(ui, sh, t)
                    });
                    dock(ui, t, "design_data", "Data", true, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        data_panel(ui, sh, st, t)
                    });
                });
            });
        egui::Panel::bottom("design_status").exact_size(24.0).frame(bar_frame(t)).show(ui, |ui| status_bar(ui, sh, st, t));
        if st.show_table && !sh.engine.project.data.is_empty() {
            egui::Panel::bottom("design_table")
                .resizable(true)
                .default_size(170.0)
                .frame(egui::Frame::new().fill(t.panel).stroke(Stroke::new(1.0, t.line)))
                .show(ui, |ui| table(ui, sh, t));
        }
    }
    egui::CentralPanel::default().frame(egui::Frame::new().fill(t.canvas)).show(ui, |ui| canvas(ui, sh, st, t, dark));
}

// ---- keyboard actions -----------------------------------------------------------------------

fn do_actions(sh: &mut Shell, st: &mut State) {
    let actions = std::mem::take(&mut sh.actions);
    let sel = sh.engine.project.selected_field.clone();
    let fields = sh.engine.project.design.fields.clone();
    for a in actions {
        match a {
            Action::ToolMove => st.tool = Tool::Move,
            Action::ToolText => st.tool = Tool::Text,
            Action::ToolPhoto => st.tool = Tool::Photo,
            Action::ToolHand => st.tool = Tool::Hand,
            Action::ToolZoom => st.tool = Tool::Zoom,
            Action::ToggleSnap => st.snap = !st.snap,
            Action::Deselect => {
                st.tool = Tool::Move;
                sh.cmd("design.select_field", json!({ "id": Value::Null }));
            }
            Action::Duplicate => {
                if let Some(id) = &sel {
                    sh.cmd("design.duplicate_field", json!({ "id": id }));
                }
            }
            Action::Delete => {
                if let Some(id) = &sel {
                    sh.cmd("design.remove_field", json!({ "id": id }));
                }
            }
            Action::Forward | Action::Backward | Action::ToFront | Action::ToBack => {
                if let Some(id) = &sel
                    && let Some(i) = fields.iter().position(|f| &f.id == id)
                {
                    let n = match a {
                        Action::Forward => i + 1,
                        Action::Backward => i.saturating_sub(1),
                        Action::ToFront => fields.len(),
                        _ => 0,
                    };
                    sh.cmd("design.reorder_field", json!({ "id": id, "index": n }));
                }
            }
            Action::NextField | Action::PrevField => {
                if !fields.is_empty() {
                    let cur = sel.as_ref().and_then(|id| fields.iter().position(|f| &f.id == id));
                    let n = match (a, cur) {
                        (Action::NextField, Some(i)) => (i + 1) % fields.len(),
                        (Action::PrevField, Some(i)) => (i + fields.len() - 1) % fields.len(),
                        (Action::NextField, None) => 0,
                        _ => fields.len() - 1,
                    };
                    sh.cmd("design.select_field", json!({ "id": fields[n].id }));
                }
            }
            Action::NudgeLeft
            | Action::NudgeRight
            | Action::NudgeUp
            | Action::NudgeDown
            | Action::NudgeLeftBig
            | Action::NudgeRightBig
            | Action::NudgeUpBig
            | Action::NudgeDownBig => {
                let big = matches!(a, Action::NudgeLeftBig | Action::NudgeRightBig | Action::NudgeUpBig | Action::NudgeDownBig);
                let step = if big { 2.0 } else { 0.2 };
                let (dx, dy) = match a {
                    Action::NudgeLeft | Action::NudgeLeftBig => (-step, 0.0),
                    Action::NudgeRight | Action::NudgeRightBig => (step, 0.0),
                    Action::NudgeUp | Action::NudgeUpBig => (0.0, -step),
                    _ => (0.0, step),
                };
                if let Some(f) = sel.as_ref().and_then(|id| fields.iter().find(|f| &f.id == id)) {
                    sh.cmd_drag(
                        "design.update_field",
                        json!({ "id": f.id, "patch": { "x": (f.x + dx).clamp(-50.0, 150.0), "y": (f.y + dy).clamp(-50.0, 150.0) } }),
                        &format!("nudge:{}", f.id),
                    );
                }
            }
            _ => {}
        }
    }
}

// ---- tool strip and options bar ---------------------------------------------------------------

fn tool_strip(ui: &mut Ui, st: &mut State, t: &Tokens) {
    ui.spacing_mut().item_spacing.y = 2.0;
    ui.vertical_centered(|ui| {
        for (tool, icon, tip) in [
            (Tool::Move, icons::move_tool as widgets::IconFn, "Move tool (V)"),
            (Tool::Text, icons::text_tool, "Text field tool (T)"),
            (Tool::Photo, icons::photo_tool, "Photo field tool (P)"),
        ] {
            if tool_button(ui, t, st.tool == tool, icon, tip).clicked() {
                st.tool = tool;
            }
        }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        for (tool, icon, tip) in [
            (Tool::Hand, icons::hand_tool as widgets::IconFn, "Hand tool (H); hold Space to pan"),
            (Tool::Zoom, icons::zoom_tool, "Zoom tool (Z); Alt+click zooms out"),
        ] {
            if tool_button(ui, t, st.tool == tool, icon, tip).clicked() {
                st.tool = tool;
            }
        }
    });
}

fn options_bar(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens) {
    ui.horizontal_centered(|ui| {
        ui.label(RichText::new(st.tool.name()).strong());
        ui.separator();
        let sel = sh.engine.project.selected_field.clone();
        let field = sel.as_ref().and_then(|id| sh.engine.project.design.field(id)).cloned();
        match (st.tool, field) {
            (Tool::Move, Some(f)) => {
                let mut patch = serde_json::Map::new();
                let (mut x, mut y, mut w, mut h, mut r) = (f.x, f.y, f.w, f.h, f.rotation);
                for (label, v, key, lo, hi) in [
                    ("X", &mut x, "x", -100.0, 200.0),
                    ("Y", &mut y, "y", -100.0, 200.0),
                    ("W", &mut w, "w", 0.5, 200.0),
                    ("H", &mut h, "h", 0.5, 200.0),
                ] {
                    ui.label(RichText::new(label).color(t.dim));
                    if ui.add(egui::DragValue::new(v).speed(0.1).range(lo..=hi).fixed_decimals(1).suffix(" %")).changed() {
                        patch.insert(key.into(), json!(*v));
                    }
                }
                ui.label(RichText::new("∠").color(t.dim));
                if ui.add(egui::DragValue::new(&mut r).speed(0.5).range(-180.0..=180.0).suffix("°")).changed() {
                    patch.insert("rotation".into(), json!(r));
                }
                if !patch.is_empty() {
                    sh.cmd_drag("design.update_field", json!({ "id": f.id, "patch": Value::Object(patch) }), &format!("opt:{}", f.id));
                }
            }
            (Tool::Move, None) => dim(ui, t, "Select a field on the card, or choose a tool on the left."),
            (Tool::Text, _) => dim(ui, t, "Click or drag on the card to add a text field."),
            (Tool::Photo, _) => dim(ui, t, "Click or drag on the card to add a photo field."),
            (Tool::Hand, _) => dim(ui, t, "Drag to pan. Hold Space with any tool to pan."),
            (Tool::Zoom, _) => dim(ui, t, "Click to zoom in, Alt+click to zoom out."),
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(&mut st.snap, "Snap");
            ui.separator();
            if ui.button("100%").on_hover_text("Actual size (Ctrl+1)").clicked() {
                sh.view_cmd = Some(ViewCmd::Actual);
            }
            if ui.button("Fit").on_hover_text("Fit on screen (Ctrl+0)").clicked() {
                sh.view_cmd = Some(ViewCmd::Fit);
            }
        });
    });
}

fn status_bar(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens) {
    ui.horizontal_centered(|ui| {
        if let Some((area, fit_w, native_w)) = st.last {
            let mut pct = st.view.percent(fit_w, native_w);
            let r = ui.add(egui::DragValue::new(&mut pct).speed(1.0).range(5.0..=3200.0).fixed_decimals(0).suffix(" %"));
            if r.changed() {
                st.view.set_percent(area, pct, fit_w, native_w);
            }
            r.on_hover_text("Zoom (Ctrl +/−, Ctrl+0 fit, Ctrl+1 100%, Alt+wheel)");
        }
        ui.separator();
        if let Some((tw, th)) = tpl_size(sh) {
            dim(ui, t, format!("{} × {} px", tw as u32, th as u32));
        }
        if let Some((x, y)) = st.cursor {
            ui.separator();
            dim(ui, t, format!("{x:.0}, {y:.0} px"));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let n = sh.engine.project.data.len();
            dim(
                ui,
                t,
                format!(
                    "Row {} of {}   ·   {} fields",
                    sh.engine.project.preview_index.min(n.saturating_sub(1)) + 1,
                    n.max(1),
                    sh.engine.project.design.fields.len()
                ),
            );
        });
    });
}

// ---- docked panels ------------------------------------------------------------------------------

fn layers(ui: &mut Ui, sh: &mut Shell, t: &Tokens) {
    let fields = sh.engine.project.design.fields.clone();
    let sel = sh.engine.project.selected_field.clone();
    let mapping = sh.engine.project.design.mapping.clone();
    if fields.is_empty() {
        dim(ui, t, "No fields yet. Pick the Text or Photo tool and click the card.");
    }
    let mut pick: Option<String> = None;
    // Topmost layer first, like a layers panel.
    for f in fields.iter().rev() {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
        let is_sel = sel.as_deref() == Some(&f.id);
        let p = ui.painter();
        if is_sel {
            p.rect_filled(rect, 2.0, t.accent);
        } else if resp.hovered() {
            p.rect_filled(rect, 2.0, t.hover.gamma_multiply(0.7));
        }
        let fg = if is_sel { Color32::WHITE } else { t.text };
        let icon_rect = Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(14.0, 14.0));
        if f.is_photo() {
            icons::photo_tool(p, icon_rect, fg);
        } else {
            icons::text_tool(p, icon_rect, fg);
        }
        let name = if f.name.is_empty() { f.id.clone() } else { f.name.clone() };
        p.text(rect.left_center() + vec2(30.0, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(12.0), fg);
        if let Some(col) = mapping.get(&f.id) {
            p.text(
                rect.right_center() - vec2(8.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                col,
                egui::FontId::proportional(10.5),
                if is_sel { Color32::from_white_alpha(190) } else { t.dim },
            );
        }
        if resp.clicked() {
            pick = Some(f.id.clone());
        }
    }
    if let Some(id) = pick {
        sh.cmd("design.select_field", json!({ "id": id }));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("+ Text").on_hover_text("Add a text field (T)").clicked() {
            add_field(sh, "design.add_text_field");
        }
        if ui.button("+ Photo").on_hover_text("Add a photo field (P)").clicked() {
            add_field(sh, "design.add_photo_field");
        }
        let has = sel.is_some();
        if ui.add_enabled(has, egui::Button::new("Duplicate")).on_hover_text("Ctrl+J").clicked() {
            sh.actions.push(Action::Duplicate);
        }
        if ui.add_enabled(has, egui::Button::new("Delete")).on_hover_text("Delete").clicked() {
            sh.actions.push(Action::Delete);
        }
    });
}

fn template_panel(ui: &mut Ui, sh: &mut Shell, t: &Tokens) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Choose template…").clicked() {
            sh.run_ui_command("design.set_template");
        }
        if ui.add_enabled(sh.engine.project.design.template.is_some(), egui::Button::new("Remove")).clicked() {
            sh.cmd("design.clear_template", Value::Null);
        }
    });
    if let Some(tpl) = &sh.engine.project.design.template {
        dim(ui, t, format!("{} · {}×{} px", tpl.src.label(), tpl.w, tpl.h));
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button("Save preset…").clicked() {
            sh.run_ui_command("design.save_preset");
        }
        if ui.button("Load preset…").clicked() {
            sh.run_ui_command("design.load_preset");
        }
        if ui.button("Demo project").clicked() {
            sh.run_ui_command("project.demo");
        }
    });
}

fn data_panel(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Load CSV / Excel…").clicked() {
            sh.run_ui_command("data.load");
        }
        if ui.button("Sample data").clicked() {
            sh.cmd("data.load_sample", Value::Null);
        }
    });
    let headers = sh.engine.project.data.headers.clone();
    if headers.is_empty() {
        dim(ui, t, "No data loaded.");
        return;
    }
    dim(ui, t, format!("{} columns · {} rows", headers.len(), sh.engine.project.data.len()));
    ui.horizontal(|ui| {
        ui.label("Photo key");
        let cur = sh.engine.project.design.photo_key_column.clone();
        egui::ComboBox::from_id_salt("keycol").selected_text(if cur.is_empty() { "(none)" } else { &cur }).show_ui(ui, |ui| {
            for h in &headers {
                if ui.selectable_label(&cur == h, h).clicked() {
                    sh.cmd("data.set_key_column", json!({ "column": h }));
                }
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        if ui.button("Auto-map").clicked() {
            sh.cmd("data.auto_map", Value::Null);
        }
        if ui.button("Create fields").clicked() {
            sh.cmd("data.create_fields", Value::Null);
        }
        ui.checkbox(&mut st.show_table, "Table");
    });
    let n = sh.engine.project.data.len();
    ui.horizontal(|ui| {
        let i = sh.engine.project.preview_index;
        ui.label("Preview row");
        if ui.add_enabled(i > 0, egui::Button::new("◀")).clicked() {
            sh.cmd("data.set_preview", json!({ "index": i - 1 }));
        }
        ui.label(format!("{} / {}", i + 1, n));
        if ui.add_enabled(i + 1 < n, egui::Button::new("▶")).clicked() {
            sh.cmd("data.set_preview", json!({ "index": i + 1 }));
        }
    });
}

fn add_field(sh: &mut Shell, id: &str) {
    sh.cmd(id, json!({ "x": 10.0, "y": 10.0 }));
}

// ---- centre: the canvas -----------------------------------------------------------------------

fn canvas(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens, dark: bool) {
    let avail = ui.available_rect_before_wrap();
    let (area, resp) = ui.allocate_exact_size(avail.size(), Sense::click_and_drag());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, t.canvas);

    let Some((tw, th)) = tpl_size(sh) else {
        st.last = None;
        painter.text(
            area.center() - vec2(0.0, 18.0),
            egui::Align2::CENTER_CENTER,
            "Choose a template image",
            egui::FontId::proportional(18.0),
            t.text,
        );
        painter.text(
            area.center() + vec2(0.0, 6.0),
            egui::Align2::CENTER_CENTER,
            "The card is rendered at its pixel size and fields are placed on top.",
            egui::FontId::proportional(12.0),
            t.dim,
        );
        if ui.put(Rect::from_center_size(area.center() + vec2(0.0, 40.0), vec2(150.0, 26.0)), egui::Button::new("Choose template…")).clicked() {
            sh.run_ui_command("design.set_template");
        }
        return;
    };
    let fit = widgets::fit_rect(area.shrink(40.0), (tw / th) as f32);
    st.last = Some((area, fit.width(), tw as f32));
    if let Some(cmd) = sh.view_cmd.take() {
        st.view.apply(cmd, area, fit.width(), tw as f32);
    }
    let typing = ui.ctx().egui_wants_keyboard_input();
    // Space turns any tool into the Hand tool while held.
    let space = !typing && ui.input(|i| i.key_down(egui::Key::Space));
    let hand = st.tool == Tool::Hand;
    let panning = st.view.input(ui, &resp, area, hand, typing);
    let rect = st.view.rect(area, fit);
    let scale = f64::from(rect.width()) / tw; // screen px per template px

    // Render request (latest wins). Size follows the zoom so text stays sharp.
    let ppp = ui.ctx().pixels_per_point();
    let visible_w = rect.width().min(area.width() * 2.0);
    let ow = ((f64::from(visible_w.max(rect.width().min(3000.0 / ppp)) * ppp)).round() as u32).clamp(64, 3000);
    let oh = ((f64::from(ow) * th / tw).round() as u32).max(1);
    let proj = sh.engine.snapshot();
    let idx = proj.preview_index;
    let mut hs = std::collections::hash_map::DefaultHasher::new();
    (sh.engine.revision(), idx, ow / 64, oh / 64, sh.families.len()).hash(&mut hs);
    let (ow, oh) = ((ow / 64).max(1) * 64, (((ow / 64).max(1) * 64) as f64 * th / tw).round() as u32);
    let key = hs.finish();
    let r = sh.engine.renderer.clone();
    st.preview
        .request(&sh.w, Slot::Design, key, move || r.render_row_sized(&proj, idx, ow, oh.max(1), RenderOpts::PREVIEW).map_err(|e| e.to_string()));

    painter.rect_filled(rect.translate(vec2(2.0, 3.0)), 0.0, Color32::from_black_alpha(70));
    widgets::checker(&painter, rect, dark);
    if let Some(tex) = &st.preview.texture {
        painter.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    if let Some(e) = &st.preview.error {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, e, egui::FontId::proportional(13.0), t.bad);
    }
    if st.preview.pending() {
        painter.circle_filled(area.right_top() + vec2(-14.0, 14.0), 4.0, t.accent);
    }

    let to_screen = |x: f64, y: f64| pos2(rect.min.x + (x * scale) as f32, rect.min.y + (y * scale) as f32);
    let to_tpl = |p: Pos2| (f64::from(p.x - rect.min.x) / scale, f64::from(p.y - rect.min.y) / scale);
    st.cursor = resp.hover_pos().map(to_tpl).filter(|(x, y)| *x >= 0.0 && *y >= 0.0 && *x <= tw && *y <= th);
    let fields = sh.engine.project.design.fields.clone();
    let sel = sh.engine.project.selected_field.clone();
    let move_tool = st.tool == Tool::Move && !space;

    // Outlines.
    for f in &fields {
        let r = field_px(f, tw, th);
        let a = f.rotation.to_radians();
        let is_sel = sel.as_deref() == Some(&f.id);
        let corners: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|(dx, dy)| {
                let p = rot((dx * r.w / 2.0, dy * r.h / 2.0), a);
                to_screen(r.cx() + p.0, r.cy() + p.1)
            })
            .collect();
        let col = if is_sel { t.accent } else { Color32::from_rgba_unmultiplied(80, 150, 240, 120) };
        painter.add(egui::Shape::closed_line(corners.clone(), Stroke::new(if is_sel { 1.5 } else { 1.0 }, col)));
    }

    // Selected field's handles.
    let handle_px = 4.5f32;
    let mut handle_pos: Vec<(Handle, Pos2)> = Vec::new();
    let mut rotate_pos: Option<Pos2> = None;
    if let Some(f) = fields.iter().find(|f| sel.as_deref() == Some(&f.id))
        && st.tool == Tool::Move
    {
        let r = field_px(f, tw, th);
        let a = f.rotation.to_radians();
        for h in Handle::ALL {
            let (dx, dy) = h.dir();
            let p = rot((dx * r.w / 2.0, dy * r.h / 2.0), a);
            handle_pos.push((h, to_screen(r.cx() + p.0, r.cy() + p.1)));
        }
        let p = rot((0.0, -r.h / 2.0 - 24.0 / scale), a);
        let rp = to_screen(r.cx() + p.0, r.cy() + p.1);
        let top = handle_pos.iter().find(|(h, _)| *h == Handle::N).map(|(_, p)| *p).unwrap_or(rp);
        painter.line_segment([top, rp], Stroke::new(1.0, t.accent));
        painter.circle(rp, handle_px, Color32::WHITE, Stroke::new(1.5, t.accent));
        rotate_pos = Some(rp);
        for (_, p) in &handle_pos {
            painter.rect(
                Rect::from_center_size(*p, vec2(handle_px * 2.0, handle_px * 2.0)),
                0.0,
                Color32::WHITE,
                Stroke::new(1.0, t.accent),
                egui::StrokeKind::Inside,
            );
        }
    }

    // Guides while dragging.
    let guide = Color32::from_rgb(0xff, 0x40, 0xa0);
    for x in &st.guides.vertical {
        let p = to_screen(*x, 0.0);
        painter.line_segment([pos2(p.x, area.min.y), pos2(p.x, area.max.y)], Stroke::new(1.0, guide));
    }
    for y in &st.guides.horizontal {
        let p = to_screen(0.0, *y);
        painter.line_segment([pos2(area.min.x, p.y), pos2(area.max.x, p.y)], Stroke::new(1.0, guide));
    }

    // ---- tools ----
    match st.tool {
        Tool::Zoom if !space => {
            if resp.hovered() {
                let alt = ui.input(|i| i.modifiers.alt);
                ui.ctx().set_cursor_icon(if alt { egui::CursorIcon::ZoomOut } else { egui::CursorIcon::ZoomIn });
            }
            if resp.clicked_by(egui::PointerButton::Primary) {
                if let Some(p) = resp.interact_pointer_pos() {
                    let alt = ui.input(|i| i.modifiers.alt);
                    st.view.zoom_about(area, p, if alt { 1.0 / 1.5 } else { 1.5 });
                }
            }
            return;
        }
        Tool::Text | Tool::Photo if !space => {
            if resp.hovered() && !panning {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            let id = if st.tool == Tool::Text { "design.add_text_field" } else { "design.add_photo_field" };
            let (dw, dh) = if st.tool == Tool::Text { (30.0, 8.0) } else { (22.0, 40.0) };
            if resp.drag_started_by(egui::PointerButton::Primary) {
                st.create = resp.interact_pointer_pos();
            }
            if let (Some(a), Some(b)) = (st.create, ui.input(|i| i.pointer.latest_pos())) {
                if resp.dragged_by(egui::PointerButton::Primary) {
                    let m = Rect::from_two_pos(a, b);
                    painter.rect_filled(m, 0.0, t.accent.gamma_multiply(0.18));
                    painter.rect_stroke(m, 0.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
                }
                if resp.drag_stopped() {
                    let (p0, p1) = (to_tpl(a), to_tpl(b));
                    let (x0, y0, x1, y1) = (p0.0.min(p1.0), p0.1.min(p1.1), p0.0.max(p1.0), p0.1.max(p1.1));
                    let (px, py, pw, ph) = (x0 / tw * 100.0, y0 / th * 100.0, (x1 - x0) / tw * 100.0, (y1 - y0) / th * 100.0);
                    if pw >= 1.0 && ph >= 1.0 {
                        sh.cmd(id, json!({ "x": px, "y": py, "w": pw, "h": ph }));
                        st.tool = Tool::Move;
                    }
                    st.create = None;
                }
            }
            if resp.clicked_by(egui::PointerButton::Primary) {
                if let Some(p) = resp.interact_pointer_pos() {
                    let (x, y) = to_tpl(p);
                    sh.cmd(id, json!({ "x": (x / tw * 100.0 - dw / 2.0).clamp(0.0, 100.0 - dw), "y": (y / th * 100.0 - dh / 2.0).clamp(0.0, 100.0 - dh), "w": dw, "h": dh }));
                    st.tool = Tool::Move;
                }
            }
            return;
        }
        _ => {}
    }
    if !move_tool || panning {
        if resp.drag_stopped() {
            st.drag = None;
            st.guides = Guides::default();
        }
        return;
    }

    // ---- Move tool ----
    let ptr = resp.interact_pointer_pos().or(resp.hover_pos());
    let hit_test = |px: f64, py: f64| {
        fields.iter().rev().find(|f| {
            let r = field_px(f, tw, th);
            let l = rot((px - r.cx(), py - r.cy()), -f.rotation.to_radians());
            l.0.abs() <= r.w / 2.0 && l.1.abs() <= r.h / 2.0
        })
    };
    if resp.drag_started_by(egui::PointerButton::Primary) {
        if let Some(p) = resp.interact_pointer_pos() {
            let near = |q: Pos2| (q - p).length() <= handle_px + 5.0;
            let sel_field = fields.iter().find(|f| sel.as_deref() == Some(&f.id));
            if let (Some(f), Some(rp)) = (sel_field, rotate_pos)
                && near(rp)
            {
                st.drag = Some(Drag::Rotate { id: f.id.clone() });
            }
            if st.drag.is_none()
                && let Some(f) = sel_field
                && let Some((h, _)) = handle_pos.iter().find(|(_, q)| near(*q))
            {
                st.drag = Some(Drag::Resize { id: f.id.clone(), handle: *h });
            }
            if st.drag.is_none() {
                let (px, py) = to_tpl(p);
                if let Some(f) = hit_test(px, py) {
                    if sel.as_deref() != Some(&f.id) {
                        sh.cmd("design.select_field", json!({ "id": f.id }));
                    }
                    let r = field_px(f, tw, th);
                    st.drag = Some(Drag::Move { id: f.id.clone(), grab: (px - r.x, py - r.y), start: r });
                }
            }
        }
    }
    if resp.clicked_by(egui::PointerButton::Primary) {
        if let Some(p) = resp.interact_pointer_pos() {
            let (px, py) = to_tpl(p);
            sh.cmd("design.select_field", json!({ "id": hit_test(px, py).map(|f| f.id.clone()) }));
        }
    }
    if resp.dragged_by(egui::PointerButton::Primary) {
        if let (Some(drag), Some(p)) = (st.drag.clone(), ptr) {
            let (px, py) = to_tpl(p);
            let shift = ui.input(|i| i.modifiers.shift);
            let alt = ui.input(|i| i.modifiers.alt);
            match drag {
                Drag::Move { id, grab, start } => {
                    let mut r = R::new(px - grab.0, py - grab.1, start.w, start.h);
                    st.guides = Guides::default();
                    if st.snap && !alt {
                        let others: Vec<R> = fields.iter().filter(|f| f.id != id && f.rotation == 0.0).map(|f| field_px(f, tw, th)).collect();
                        let (nr, g) = geometry::snap_move(r, &others, (tw, th), 6.0 / scale);
                        r = nr;
                        st.guides = g;
                    }
                    patch_geom(sh, &id, r, tw, th);
                }
                Drag::Resize { id, handle } => {
                    if let Some(f) = fields.iter().find(|f| f.id == id) {
                        let cur = field_px(f, tw, th);
                        let nr = geometry::resize(cur, f.rotation.to_radians(), handle, (px, py), shift);
                        patch_geom(sh, &id, nr, tw, th);
                    }
                }
                Drag::Rotate { id } => {
                    if let Some(f) = fields.iter().find(|f| f.id == id) {
                        let r = field_px(f, tw, th);
                        let deg = geometry::rotation_from_pointer((r.cx(), r.cy()), (px, py), shift);
                        sh.cmd_drag("design.update_field", json!({ "id": id, "patch": { "rotation": deg } }), &format!("drag:{id}"));
                    }
                }
            }
        }
    }
    if resp.drag_stopped() {
        st.drag = None;
        st.guides = Guides::default();
    }
}

fn patch_geom(sh: &mut Shell, id: &str, r: R, tw: f64, th: f64) {
    let patch = json!({ "x": r.x / tw * 100.0, "y": r.y / th * 100.0, "w": r.w / tw * 100.0, "h": r.h / th * 100.0 });
    sh.cmd_drag("design.update_field", json!({ "id": id, "patch": patch }), &format!("drag:{id}"));
}

// ---- right: inspector ------------------------------------------------------------------------

fn inspector(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens) {
    let Some(id) = sh.engine.project.selected_field.clone() else {
        dim(ui, t, "Select a field on the card to edit it.");
        return;
    };
    let Some(f) = sh.engine.project.design.field(&id).cloned() else { return };
    let key = format!("ins:{id}");
    let mut patch = serde_json::Map::new();
    let mut put = |k: &str, v: Value| {
        patch.insert(k.into(), v);
    };

    section(ui, t, if f.is_photo() { "Photo field" } else { "Text field" });
    let mut name = f.name.clone();
    ui.horizontal(|ui| {
        ui.label("Name");
        if ui.text_edit_singleline(&mut name).changed() {
            put("name", json!(name));
        }
    });

    // Column mapping.
    let headers = sh.engine.project.data.headers.clone();
    if !headers.is_empty() && !f.is_photo() {
        let cur = sh.engine.project.design.mapping.get(&id).cloned();
        let mut chosen: Option<Option<String>> = None;
        ui.horizontal(|ui| {
            ui.label("Column");
            egui::ComboBox::from_id_salt("map").selected_text(cur.clone().unwrap_or_else(|| "(none)".into())).show_ui(ui, |ui| {
                if ui.selectable_label(cur.is_none(), "(none)").clicked() {
                    chosen = Some(None);
                }
                for h in &headers {
                    if ui.selectable_label(cur.as_deref() == Some(h), h).clicked() {
                        chosen = Some(Some(h.clone()));
                    }
                }
            });
        });
        if let Some(c) = chosen {
            sh.cmd("data.map_field", json!({ "field": id, "column": c }));
        }
    }

    section(ui, t, "Position");
    let (mut x, mut y, mut w, mut h, mut rt, mut op) = (f.x, f.y, f.w, f.h, f.rotation, f.opacity);
    egui::Grid::new("geom").num_columns(4).show(ui, |ui| {
        ui.label("X");
        if ui.add(egui::DragValue::new(&mut x).speed(0.1).suffix("%").fixed_decimals(1)).changed() {
            put("x", json!(x));
        }
        ui.label("Y");
        if ui.add(egui::DragValue::new(&mut y).speed(0.1).suffix("%").fixed_decimals(1)).changed() {
            put("y", json!(y));
        }
        ui.end_row();
        ui.label("W");
        if ui.add(egui::DragValue::new(&mut w).speed(0.1).range(0.5..=200.0).suffix("%").fixed_decimals(1)).changed() {
            put("w", json!(w));
        }
        ui.label("H");
        if ui.add(egui::DragValue::new(&mut h).speed(0.1).range(0.5..=200.0).suffix("%").fixed_decimals(1)).changed() {
            put("h", json!(h));
        }
        ui.end_row();
        ui.label("Rot");
        if ui.add(egui::DragValue::new(&mut rt).speed(0.5).range(-180.0..=180.0).suffix("°")).changed() {
            put("rotation", json!(rt));
        }
        ui.label("Opac");
        if ui.add(egui::DragValue::new(&mut op).speed(0.5).range(0.0..=100.0).suffix("%")).changed() {
            put("opacity", json!(op));
        }
        ui.end_row();
    });

    match &f.kind {
        FieldKind::Text(p) => {
            section(ui, t, "Font");
            let cur_family = p.font_family.split(',').next().unwrap_or("").trim().trim_matches(['\'', '"']).to_string();
            ui.horizontal(|ui| {
                ui.label("Family");
                egui::ComboBox::from_id_salt("family").width(170.0).selected_text(cur_family.clone()).show_ui(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut st.font_filter).hint_text("Search fonts…"));
                    let q = st.font_filter.to_lowercase();
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for fam in sh.families.iter().filter(|n| q.is_empty() || n.to_lowercase().contains(&q)) {
                            if ui.selectable_label(*fam == cur_family, fam).clicked() {
                                put("fontFamily", json!(format!("'{fam}', sans-serif")));
                            }
                        }
                    });
                });
                if ui.small_button("+").on_hover_text("Add a font file…").clicked() {
                    sh.run_ui_command("fonts.add");
                }
            });
            let (mut size, mut weight) = (p.font_size, p.weight);
            ui.horizontal(|ui| {
                ui.label("Size");
                if ui.add(egui::DragValue::new(&mut size).speed(0.5).range(4.0..=1000.0).suffix(" px")).changed() {
                    put("fontSize", json!(size));
                }
                ui.label("Weight");
                egui::ComboBox::from_id_salt("weight").width(64.0).selected_text(weight.to_string()).show_ui(ui, |ui| {
                    for wv in [100u32, 200, 300, 400, 500, 600, 700, 800, 900] {
                        if ui.selectable_value(&mut weight, wv, wv.to_string()).clicked() {
                            put("weight", json!(wv));
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Colour");
                if let Some(c) = widgets::color_edit(ui, p.color) {
                    put("color", json!(c.to_hex()));
                }
            });
            ui.horizontal(|ui| {
                for (label, on, k) in [("I", p.italic, "italic"), ("U", p.underline, "underline"), ("S̶", p.strikethrough, "strikethrough")] {
                    if ui.selectable_label(on, label).clicked() {
                        put(k, json!(!on));
                    }
                }
                ui.separator();
                for (label, a) in [("Left", Align::Left), ("Center", Align::Center), ("Right", Align::Right), ("Justify", Align::Justify)] {
                    if ui.selectable_label(p.align == a, label).on_hover_text(a.as_str()).clicked() {
                        put("align", json!(a.as_str()));
                    }
                }
            });
            section(ui, t, "Layout");
            enum_combo(ui, "Wrap", "wrap", Wrap::ALL, p.wrap, |v| v.as_str(), &mut put);
            enum_combo(ui, "Overflow", "overflow", Overflow::ALL, p.overflow, |v| v.as_str(), &mut put);
            enum_combo(ui, "Vertical", "vAlign", VAlign::ALL, p.v_align, |v| v.as_str(), &mut put);
            enum_combo(ui, "Case", "textCase", TextCase::ALL, p.text_case, |v| v.as_str(), &mut put);
            enum_combo(ui, "Script", "scriptPos", ScriptPos::ALL, p.script_pos, |v| v.as_str(), &mut put);
            let (mut lh, mut ml, mut hy) = (p.line_height, p.max_lines, p.hyphenate);
            ui.horizontal(|ui| {
                ui.label("Line height");
                if ui.add(egui::DragValue::new(&mut lh).speed(0.01).range(0.6..=3.0).fixed_decimals(2)).changed() {
                    put("lineHeight", json!(lh));
                }
                ui.label("Max lines");
                if ui.add(egui::DragValue::new(&mut ml).range(0..=50)).on_hover_text("0 = as many as fit").changed() {
                    put("maxLines", json!(ml));
                }
            });
            if ui.checkbox(&mut hy, "Hyphenate").changed() {
                put("hyphenate", json!(hy));
            }
        }
        FieldKind::Photo(p) => {
            section(ui, t, "Photo");
            enum_combo(ui, "Shape", "shape", Shape::ALL, p.shape, |v| v.as_str(), &mut put);
            let mut bw = p.border_w;
            ui.horizontal(|ui| {
                ui.label("Border");
                if ui.add(egui::DragValue::new(&mut bw).speed(0.2).range(0.0..=100.0).suffix(" px")).changed() {
                    put("borderW", json!(bw));
                }
                if let Some(c) = widgets::color_edit(ui, p.border_color) {
                    put("borderColor", json!(c.to_hex()));
                }
            });
            let (mut z, mut ox, mut oy) = (p.zoom, p.offset_x, p.offset_y);
            dim(ui, t, "Fallback framing, used when a photo has no automatic or manual crop.");
            ui.horizontal(|ui| {
                ui.label("Zoom");
                if ui.add(egui::DragValue::new(&mut z).speed(1.0).range(50.0..=500.0).suffix("%")).changed() {
                    put("zoom", json!(z));
                }
                ui.label("Pan");
                if ui.add(egui::DragValue::new(&mut ox).speed(0.5).range(-100.0..=100.0)).changed() {
                    put("offsetX", json!(ox));
                }
                if ui.add(egui::DragValue::new(&mut oy).speed(0.5).range(-100.0..=100.0)).changed() {
                    put("offsetY", json!(oy));
                }
            });
        }
    }
    ui.add_space(8.0);
    if ui.button(RichText::new("Delete field").color(t.bad)).clicked() {
        sh.cmd("design.remove_field", json!({ "id": id }));
        return;
    }
    if !patch.is_empty() {
        sh.cmd_drag("design.update_field", json!({ "id": id, "patch": Value::Object(patch) }), &key);
    }
}

fn enum_combo<T: Copy + PartialEq + 'static>(
    ui: &mut Ui,
    label: &str,
    key: &str,
    all: &'static [T],
    cur: T,
    name: impl Fn(T) -> &'static str,
    put: &mut impl FnMut(&str, Value),
) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(key).selected_text(name(cur)).show_ui(ui, |ui| {
            for v in all {
                if ui.selectable_label(*v == cur, name(*v)).clicked() {
                    put(key, json!(name(*v)));
                }
            }
        });
    });
}

// ---- bottom: data table ----------------------------------------------------------------------

fn table(ui: &mut Ui, sh: &mut Shell, t: &Tokens) {
    use egui_extras::{Column, TableBuilder};
    let data = sh.engine.project.data.clone();
    let cur = sh.engine.project.preview_index;
    let mut select: Option<usize> = None;
    let mut edit: Option<(usize, String, String)> = None;
    let mut tb = TableBuilder::new(ui).striped(true).resizable(true).max_scroll_height(f32::INFINITY).column(Column::exact(44.0));
    for _ in &data.headers {
        tb = tb.column(Column::initial(130.0).at_least(50.0).clip(true));
    }
    tb.header(22.0, |mut hr| {
        hr.col(|ui| {
            ui.label(RichText::new("#").color(t.dim));
        });
        for h in &data.headers {
            hr.col(|ui| {
                ui.label(RichText::new(h).strong());
            });
        }
    })
    .body(|body| {
        body.rows(24.0, data.rows.len(), |mut row| {
            let i = row.index();
            row.set_selected(i == cur);
            row.col(|ui| {
                if ui.selectable_label(i == cur, (i + 1).to_string()).clicked() {
                    select = Some(i);
                }
            });
            for (c, h) in data.headers.iter().enumerate() {
                row.col(|ui| {
                    let mut v = data.rows[i].get(c).cloned().unwrap_or_default();
                    let r = ui.add(egui::TextEdit::singleline(&mut v).frame(egui::Frame::NONE).desired_width(f32::INFINITY));
                    if r.changed() {
                        edit = Some((i, h.clone(), v));
                    }
                    if r.gained_focus() {
                        select = Some(i);
                    }
                });
            }
        });
    });
    if let Some(i) = select {
        if i != cur {
            sh.cmd("data.set_preview", json!({ "index": i }));
        }
    }
    if let Some((row, column, value)) = edit {
        sh.cmd_drag("data.set_cell", json!({ "row": row, "column": column, "value": value }), &format!("cell:{row}:{column}"));
    }
}

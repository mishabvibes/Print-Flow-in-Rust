//! "Arrange & Print": sheet settings in docked panels on the right, a zoomable paged sheet preview in
//! the middle, Hand and Zoom tools on the left and a status bar with the zoom and the sheet count.

use std::hash::{Hash, Hasher};

use egui::{Color32, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use forge_doc::{CardSource, Orientation, SheetSettings, sheet::PRESETS};
use forge_engine::json;
use forge_geom::GridMode;
use forge_render::RenderOpts;

use crate::shell::Shell;
use crate::shortcuts::Action;
use crate::theme::{DOCK_W, TOOL_W, Tokens};
use crate::view::View;
use crate::widgets::{self, dim, dock, icons, section, tool_button, unit_picker, unit_value};
use crate::workers::{Preview, Slot};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Hand,
    Zoom,
}

pub struct State {
    pub preview: Preview,
    pub page: usize,
    view: View,
    tool: Tool,
    /// (preview area, fitted width, sheet width in px at 96 dpi) of the last frame.
    last: Option<(Rect, f32, f32)>,
}

impl Default for State {
    fn default() -> Self {
        Self { preview: Preview::default(), page: 0, view: View::default(), tool: Tool::Hand, last: None }
    }
}

fn bar_frame(t: &Tokens) -> egui::Frame {
    egui::Frame::new().fill(t.panel).stroke(Stroke::new(1.0, t.line)).inner_margin(egui::Margin::symmetric(8, 4))
}

pub fn show(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens, dark: bool) {
    let pages = pages_of(sh);
    for a in std::mem::take(&mut sh.actions) {
        match a {
            Action::ToolHand => st.tool = Tool::Hand,
            Action::ToolZoom => st.tool = Tool::Zoom,
            Action::PrevItem => st.page = st.page.saturating_sub(1),
            Action::NextItem => st.page = (st.page + 1).min(pages - 1),
            Action::FirstItem => st.page = 0,
            Action::LastItem => st.page = pages - 1,
            _ => {}
        }
    }
    st.page = st.page.min(pages - 1);
    if !sh.hide_panels {
        egui::Panel::top("arrange_options").exact_size(32.0).frame(bar_frame(t)).show(ui, |ui| options_bar(ui, sh, st, t, pages));
        egui::Panel::left("arrange_tools")
            .exact_size(TOOL_W)
            .resizable(false)
            .frame(egui::Frame::new().fill(t.header).stroke(Stroke::new(1.0, t.line)).inner_margin(egui::Margin::symmetric(3, 6)))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.vertical_centered(|ui| {
                    if tool_button(ui, t, st.tool == Tool::Hand, icons::hand_tool, "Hand tool (H); hold Space to pan").clicked() {
                        st.tool = Tool::Hand;
                    }
                    if tool_button(ui, t, st.tool == Tool::Zoom, icons::zoom_tool, "Zoom tool (Z); Alt+click zooms out").clicked() {
                        st.tool = Tool::Zoom;
                    }
                });
            });
        egui::Panel::right("arrange_dock")
            .default_size(DOCK_W)
            .size_range(250.0..=520.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(t.panel).stroke(Stroke::new(1.0, t.line)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| controls(ui, sh, t));
            });
        egui::Panel::bottom("arrange_status").exact_size(24.0).frame(bar_frame(t)).show(ui, |ui| status(ui, sh, st, t, pages));
    }
    egui::CentralPanel::default().frame(egui::Frame::new().fill(t.canvas)).show(ui, |ui| preview(ui, sh, st, t, dark));
}

fn pages_of(sh: &Shell) -> usize {
    let p = &sh.engine.project;
    forge_render::sheet::plan(&p.sheet, 72).map(|pl| pl.sheets_for(p.card_count()).max(1)).unwrap_or(1)
}

fn options_bar(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens, pages: usize) {
    ui.horizontal_centered(|ui| {
        if ui.add_enabled(st.page > 0, egui::Button::new("◀")).on_hover_text("Previous sheet (Page Up)").clicked() {
            st.page -= 1;
        }
        ui.label(format!("Sheet {} of {}", st.page + 1, pages));
        if ui.add_enabled(st.page + 1 < pages, egui::Button::new("▶")).on_hover_text("Next sheet (Page Down)").clicked() {
            st.page += 1;
        }
        if st.preview.pending() {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ok = sh.engine.project.card_count() > 0 && !sh.busy();
            if ui.add_enabled(ok, egui::Button::new(RichText::new("Export PDF…").strong())).on_hover_text("Ctrl+E").clicked() {
                sh.run_ui_command("export.sheets_pdf");
            }
            if ui.add_enabled(ok, egui::Button::new("Export PNG ZIP…")).clicked() {
                sh.run_ui_command("export.sheets_zip");
            }
            ui.separator();
            if ui.button("100%").on_hover_text("Actual size (Ctrl+1)").clicked() {
                sh.view_cmd = Some(crate::view::ViewCmd::Actual);
            }
            if ui.button("Fit").on_hover_text("Fit on screen (Ctrl+0)").clicked() {
                sh.view_cmd = Some(crate::view::ViewCmd::Fit);
            }
        });
        let _ = t;
    });
}

fn status(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens, pages: usize) {
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
        let s = &sh.engine.project.sheet;
        dim(ui, t, format!("{:.1} × {:.1} cm · {} dpi", s.sheet.w, s.sheet.h, s.dpi));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            dim(ui, t, format!("{} cards · {} sheets", sh.engine.project.card_count(), pages));
        });
    });
}

fn controls(ui: &mut Ui, sh: &mut Shell, t: &Tokens) {
    let cur = sh.engine.project.sheet.clone();
    let mut s = cur.clone();
    ui.spacing_mut().item_spacing.y = 0.0;

    dock(ui, t, "arr_cards", "Cards", true, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let kind = match &sh.engine.project.card_source {
            CardSource::Generated => "generated",
            CardSource::CroppedPhotos => "photos",
            CardSource::Files(_) => "files",
        };
        let mut pick = None;
        ui.horizontal_wrapped(|ui| {
            for (k, label) in [("generated", "Generated cards"), ("photos", "Cropped photos"), ("files", "Card images")] {
                if ui.selectable_label(kind == k, label).clicked() {
                    pick = Some(k);
                }
            }
        });
        if let Some(k) = pick {
            sh.cmd("sheet.set_source", json!({ "kind": k }));
        }
        if let CardSource::Files(f) = &sh.engine.project.card_source {
            let n = f.len();
            ui.horizontal(|ui| {
                dim(ui, t, format!("{n} images"));
                if ui.button("Add…").clicked() {
                    sh.run_ui_command("sheet.add_files");
                }
                if ui.button("Clear").clicked() {
                    sh.cmd("sheet.clear_files", forge_engine::Value::Null);
                }
            });
        }
        dim(ui, t, format!("{} cards in total", sh.engine.project.card_count()));
    });

    dock(ui, t, "arr_paper", "Paper", true, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            let name = cur.preset_name().unwrap_or("Custom");
            egui::ComboBox::from_id_salt("paper").width(90.0).selected_text(name).show_ui(ui, |ui| {
                for (n, _, _) in PRESETS {
                    if ui.selectable_label(*n == name, *n).clicked() {
                        sh.cmd("sheet.preset", json!({ "name": n }));
                    }
                }
            });
            for (o, label) in [(Orientation::Portrait, "Portrait"), (Orientation::Landscape, "Landscape")] {
                if ui.selectable_label(cur.sheet.orientation == o, label).clicked() && cur.sheet.orientation != o {
                    sh.cmd("sheet.orientation", json!({ "value": if o == Orientation::Portrait { "portrait" } else { "landscape" } }));
                }
            }
        });
        ui.horizontal(|ui| {
            unit_picker(ui, "unit_sheet", &mut s.unit_sheet);
            let u = s.unit_sheet;
            ui.label("W");
            unit_value(ui, &mut s.sheet.w, u, 3.0, 200.0);
            ui.label("H");
            unit_value(ui, &mut s.sheet.h, u, 3.0, 200.0);
        });
        section(ui, t, "Margins");
        ui.checkbox(&mut s.margin.uniform, "All equal");
        let u = s.unit_sheet;
        if s.margin.uniform {
            let mut m = s.margin.top;
            if unit_value(ui, &mut m, u, 0.0, 30.0) {
                sh.cmd_drag("sheet.uniform_margin", json!({ "value": m }), "margin");
            }
        } else {
            egui::Grid::new("margins").num_columns(4).show(ui, |ui| {
                ui.label("Top");
                unit_value(ui, &mut s.margin.top, u, 0.0, 30.0);
                ui.label("Right");
                unit_value(ui, &mut s.margin.right, u, 0.0, 30.0);
                ui.end_row();
                ui.label("Bottom");
                unit_value(ui, &mut s.margin.bottom, u, 0.0, 30.0);
                ui.label("Left");
                unit_value(ui, &mut s.margin.left, u, 0.0, 30.0);
                ui.end_row();
            });
        }
    });

    dock(ui, t, "arr_card", "Card size and gap", true, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            unit_picker(ui, "unit_card", &mut s.unit_card);
            let u = s.unit_card;
            ui.label("W");
            unit_value(ui, &mut s.card.w, u, 0.5, 100.0);
            ui.label("H");
            unit_value(ui, &mut s.card.h, u, 0.5, 100.0);
        });
        ui.horizontal(|ui| {
            let u = s.unit_card;
            ui.label("Gap X");
            unit_value(ui, &mut s.gap.x, u, 0.0, 20.0);
            ui.label("Y");
            unit_value(ui, &mut s.gap.y, u, 0.0, 20.0);
        });
        if ui.add_enabled(sh.engine.project.design.template.is_some(), egui::Button::new("Card size from template")).clicked() {
            sh.cmd("sheet.card_from_template", json!({ "widthCm": s.card.w }));
        }
    });

    dock(ui, t, "arr_grid", "Grid", true, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut s.grid_mode, GridMode::Auto, "Auto fit");
            ui.selectable_value(&mut s.grid_mode, GridMode::Custom, "Custom");
        });
        if s.grid_mode == GridMode::Custom {
            ui.horizontal(|ui| {
                ui.label("Columns");
                ui.add(egui::DragValue::new(&mut s.custom_grid.cols).range(1..=50));
                ui.label("Rows");
                ui.add(egui::DragValue::new(&mut s.custom_grid.rows).range(1..=50));
            });
        }
        ui.checkbox(&mut s.grid_border.enabled, "Cell borders");
        if s.grid_border.enabled {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut s.grid_border.size).range(0.1..=20.0).speed(0.05).suffix(" pt"));
                if let Some(c) = widgets::color_edit(ui, s.grid_border.color) {
                    s.grid_border.color = c;
                }
            });
        }
        sheet_info(ui, sh, t, &s);
    });

    dock(ui, t, "arr_output", "Output", true, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            ui.label("Resolution");
            for d in [150u32, 300, 600] {
                ui.selectable_value(&mut s.dpi, d, format!("{d} dpi"));
            }
        });
        let ok = sh.engine.project.card_count() > 0 && !sh.busy();
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(ok, egui::Button::new("Export PDF…")).clicked() {
                sh.run_ui_command("export.sheets_pdf");
            }
            if ui.add_enabled(ok, egui::Button::new("Export PNG ZIP…")).clicked() {
                sh.run_ui_command("export.sheets_zip");
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Save layout…").clicked() {
                sh.run_ui_command("sheet.save_layout");
            }
            if ui.button("Load layout…").clicked() {
                sh.run_ui_command("sheet.load_layout");
            }
        });
    });

    if s != cur {
        let patch = serde_json::to_value(&s).unwrap_or_default();
        sh.cmd_drag("sheet.update", json!({ "patch": patch }), "sheet");
    }
}

fn sheet_info(ui: &mut Ui, sh: &Shell, t: &Tokens, s: &SheetSettings) {
    match forge_render::sheet::plan(s, 72) {
        Ok(p) => {
            let n = sh.engine.project.card_count();
            dim(ui, t, format!("{}×{} grid · {} per sheet · {} sheets", p.cols, p.rows, p.capacity(), p.sheets_for(n)));
            if !p.fits {
                ui.label(RichText::new("Cards do not fit inside the margins.").color(t.warn).size(11.0));
            }
        }
        Err(e) => {
            ui.label(RichText::new(e.to_string()).color(t.bad).size(11.0));
        }
    }
}

fn preview(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens, dark: bool) {
    let proj = sh.engine.snapshot();
    let s = proj.sheet.clone();
    let n = proj.card_count();
    let avail = ui.available_rect_before_wrap();
    let (area, resp) = ui.allocate_exact_size(avail.size(), Sense::click_and_drag());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, t.canvas);
    let sheet_aspect = (s.sheet.w / s.sheet.h) as f32;
    let fit = widgets::fit_rect(area.shrink(36.0), sheet_aspect);
    // 100 % is the sheet at 96 dpi.
    let native_w = (s.sheet.w / 2.54 * 96.0) as f32;
    st.last = Some((area, fit.width(), native_w));
    if let Some(cmd) = sh.view_cmd.take() {
        st.view.apply(cmd, area, fit.width(), native_w);
    }
    let typing = ui.ctx().egui_wants_keyboard_input();
    let panning = st.view.input(ui, &resp, area, st.tool == Tool::Hand, typing);
    if st.tool == Tool::Zoom && !panning && resp.clicked_by(egui::PointerButton::Primary) {
        if let Some(p) = resp.interact_pointer_pos() {
            let alt = ui.input(|i| i.modifiers.alt);
            st.view.zoom_about(area, p, if alt { 1.0 / 1.5 } else { 1.5 });
        }
    }
    if st.tool == Tool::Zoom && resp.hovered() && !panning {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ZoomIn);
    }
    let rect = st.view.rect(area, fit);

    // The preview resolution follows the zoom (bucketed so that small changes do not re-render).
    let ppp = ui.ctx().pixels_per_point();
    let want_dpi = (f64::from(rect.width() * ppp) / (s.sheet.w / 2.54)).clamp(48.0, 300.0);
    let dpi = [72u32, 96, 150, 200, 300].into_iter().find(|d| f64::from(*d) >= want_dpi).unwrap_or(300);
    let Ok(plan) = forge_render::sheet::plan(&s, dpi) else {
        painter.text(
            area.center(),
            egui::Align2::CENTER_CENTER,
            "Nothing fits on the sheet with these settings",
            egui::FontId::proportional(14.0),
            t.warn,
        );
        return;
    };
    if n == 0 {
        painter.text(
            area.center_bottom() - vec2(0.0, 20.0),
            egui::Align2::CENTER_BOTTOM,
            "No cards yet: load data and a template, add photos, or import card images.",
            egui::FontId::proportional(12.0),
            t.dim,
        );
    }
    let mut hs = std::collections::hash_map::DefaultHasher::new();
    (sh.engine.revision(), st.page, sh.families.len(), dpi).hash(&mut hs);
    let key = hs.finish();
    let page = st.page;
    let r = sh.engine.renderer.clone();
    st.preview.request(&sh.w, Slot::Sheet, key, move || {
        let out = forge_render::sheet::render_sheet(&r, &proj, &plan, page, RenderOpts::PREVIEW);
        Ok(out.image)
    });
    painter.rect_filled(rect.translate(vec2(2.0, 3.0)), 0.0, Color32::from_black_alpha(80));
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    if let Some(tex) = &st.preview.texture {
        painter.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    let _ = dark;
}

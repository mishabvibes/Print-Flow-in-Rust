//! "Crop Photos": an options bar, the crop editor in the middle and a docked photo grid. Dragging in
//! the editor pans the photo, the wheel / Ctrl +/- zoom it; both end up as `photos.set_manual_crop`
//! (one undo step per gesture).

use std::collections::HashMap;

use egui::{Color32, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use forge_doc::{CropStatus, ManualCrop, PhotoEntry, placement};
use forge_engine::json;
use forge_geom::Rect as GRect;

use crate::shell::{Shell, source_version};
use crate::shortcuts::Action;
use crate::theme::{DOCK_W, TOOL_W, Tokens};
use crate::view::{STEP, ViewCmd};
use crate::widgets::{self, dim, dock, icons, tool_button};
use crate::workers::{Msg, texture_from};

type Thumb = (u64, egui::TextureHandle);

#[derive(Default)]
pub struct State {
    thumbs: HashMap<String, Thumb>,
    full: Option<(String, u64, egui::TextureHandle)>,
    full_pending: Option<(String, u64)>,
    bg_color: Option<forge_color::Rgba>,
    bg_tolerance: Option<f32>,
}

impl State {
    pub fn on_photo(&mut self, ctx: &egui::Context, sh: &mut Shell, msg: Msg) {
        if let Msg::Photo { key, version, full, image } = msg {
            if full {
                if self.full_pending.as_ref() == Some(&(key.clone(), version)) {
                    self.full_pending = None;
                }
                if let Ok(img) = image {
                    self.full = Some((key, version, texture_from(ctx, "photo-full", &img)));
                }
            } else {
                sh.thumb_pending.remove(&key);
                if let Ok(img) = image {
                    let t = texture_from(ctx, "thumb", &img);
                    self.thumbs.insert(key, (version, t));
                }
            }
        }
    }
}

fn aspect_presets() -> [(&'static str, f64, f64); 3] {
    [("35 × 45 mm", 35.0, 45.0), ("2 × 2 in", 50.8, 50.8), ("Square", 40.0, 40.0)]
}

fn bar_frame(t: &Tokens) -> egui::Frame {
    egui::Frame::new().fill(t.panel).stroke(Stroke::new(1.0, t.line)).inner_margin(egui::Margin::symmetric(8, 4))
}

pub fn show(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens, _dark: bool) {
    let photos = sh.engine.project.photos.clone();
    let sel = sh.engine.project.crop.selected_key.clone().filter(|k| photos.contains_key(k)).or_else(|| photos.keys().next().cloned());
    let aspect = sh.engine.project.crop.target_aspect.ratio();
    do_actions(sh, &photos, &sel, aspect);

    if !sh.hide_panels {
        egui::Panel::top("crop_options").exact_size(32.0).frame(bar_frame(t)).show(ui, |ui| toolbar(ui, sh, st, t));
        egui::Panel::left("crop_tools")
            .exact_size(TOOL_W)
            .resizable(false)
            .frame(egui::Frame::new().fill(t.header).stroke(Stroke::new(1.0, t.line)).inner_margin(egui::Margin::symmetric(3, 6)))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    tool_button(ui, t, true, icons::crop_tool, "Crop: drag the photo to move it, wheel or Ctrl +/- to zoom");
                });
            });
        egui::Panel::right("crop_dock")
            .default_size(DOCK_W)
            .size_range(240.0..=520.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(t.panel).stroke(Stroke::new(1.0, t.line)))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                dock(ui, t, "crop_photos", "Photos", true, |ui| grid(ui, sh, st, t, &sel));
            });
        egui::Panel::bottom("crop_status").exact_size(24.0).frame(bar_frame(t)).show(ui, |ui| status(ui, sh, t, &sel, aspect));
    }
    egui::CentralPanel::default().frame(egui::Frame::new().fill(t.canvas)).show(ui, |ui| {
        if photos.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.3);
                ui.label(RichText::new("Add photos to start").size(18.0).strong());
                dim(ui, t, "Faces are found automatically and framed for the card. Drag a photo to adjust it.");
                ui.add_space(10.0);
                if ui.button("Add photos…").clicked() {
                    sh.run_ui_command("photos.add");
                }
                if ui.button("Add a folder…").clicked() {
                    sh.run_ui_command("photos.add_folder");
                }
            });
        } else if let Some(key) = &sel {
            editor(ui, sh, st, t, key, aspect);
        }
    });
}

/// The photo's current manual crop as seen in a canonical box of the target aspect: zoom and offsets
/// do not depend on the box's size, so the status bar can work without knowing the editor's geometry.
fn canonical(entry: &PhotoEntry, aspect: f64) -> (f64, f64, f64, f64, GRect) {
    let (nw, nh) = (f64::from(entry.natural_w.max(1)), f64::from(entry.natural_h.max(1)));
    let (bw, bh) = (aspect * 100.0, 100.0);
    let crop = entry.effective_crop(aspect, None);
    (nw, nh, bw, bh, placement(nw, nh, bw, bh, &crop).dest)
}

fn adjust(sh: &mut Shell, key: &str, entry: &PhotoEntry, aspect: f64, f: impl Fn(GRect, f64, f64) -> GRect) {
    let (nw, nh, bw, bh, d) = canonical(entry, aspect);
    let m: ManualCrop = forge_doc::manual_from_dest(nw, nh, bw, bh, f(d, bw, bh));
    sh.cmd_drag(
        "photos.set_manual_crop",
        json!({ "key": key, "zoom": m.zoom, "offsetX": m.offset_x, "offsetY": m.offset_y }),
        &format!("crop:{key}"),
    );
}

fn zoom_by(sh: &mut Shell, key: &str, entry: &PhotoEntry, aspect: f64, factor: f64) {
    adjust(sh, key, entry, aspect, |d, bw, bh| {
        let c = (bw / 2.0, bh / 2.0);
        GRect::new(c.0 - (c.0 - d.x) * factor, c.1 - (c.1 - d.y) * factor, d.w * factor, d.h * factor)
    });
}

fn do_actions(sh: &mut Shell, photos: &std::collections::BTreeMap<String, std::sync::Arc<PhotoEntry>>, sel: &Option<String>, aspect: f64) {
    let actions = std::mem::take(&mut sh.actions);
    let cmd = sh.view_cmd.take();
    let keys: Vec<&String> = photos.keys().collect();
    let pos = sel.as_ref().and_then(|k| keys.iter().position(|x| *x == k));
    let mut go: Option<usize> = None;
    for a in &actions {
        match (a, pos) {
            (Action::PrevItem, Some(i)) => go = Some(i.saturating_sub(1)),
            (Action::NextItem, Some(i)) => go = Some((i + 1).min(keys.len().saturating_sub(1))),
            (Action::FirstItem, _) => go = Some(0),
            (Action::LastItem, _) => go = Some(keys.len().saturating_sub(1)),
            _ => {}
        }
    }
    if let Some(i) = go
        && let Some(k) = keys.get(i)
    {
        sh.cmd("photos.select", json!({ "key": k }));
    }
    let (Some(key), true) = (sel, !photos.is_empty()) else { return };
    let Some(entry) = photos.get(key).cloned() else { return };
    match cmd {
        Some(ViewCmd::In) => zoom_by(sh, key, &entry, aspect, f64::from(STEP)),
        Some(ViewCmd::Out) => zoom_by(sh, key, &entry, aspect, 1.0 / f64::from(STEP)),
        Some(ViewCmd::Fit) => {
            sh.cmd("photos.clear_manual_crop", json!({ "keys": [key] }));
        }
        Some(ViewCmd::Actual) => {
            let cur = forge_doc::manual_from_dest(
                f64::from(entry.natural_w.max(1)),
                f64::from(entry.natural_h.max(1)),
                aspect * 100.0,
                100.0,
                canonical(&entry, aspect).4,
            );
            zoom_by(sh, key, &entry, aspect, 100.0 / cur.zoom.max(1.0));
        }
        None => {}
    }
    for a in actions {
        let (dx, dy) = match a {
            Action::NudgeLeft => (-1.0, 0.0),
            Action::NudgeRight => (1.0, 0.0),
            Action::NudgeUp => (0.0, -1.0),
            Action::NudgeDown => (0.0, 1.0),
            Action::NudgeLeftBig => (-10.0, 0.0),
            Action::NudgeRightBig => (10.0, 0.0),
            Action::NudgeUpBig => (0.0, -10.0),
            Action::NudgeDownBig => (0.0, 10.0),
            _ => continue,
        };
        adjust(sh, key, &entry, aspect, |d, bw, bh| GRect::new(d.x + dx / 100.0 * bw, d.y + dy / 100.0 * bh, d.w, d.h));
    }
}

fn toolbar(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens) {
    ui.horizontal_centered(|ui| {
        if ui.button("Add photos…").clicked() {
            sh.run_ui_command("photos.add");
        }
        if ui.button("Add folder…").clicked() {
            sh.run_ui_command("photos.add_folder");
        }
        let pending = sh.engine.project.photos.values().filter(|p| p.status == CropStatus::Pending).count();
        let busy = sh.detect.is_some();
        if ui.add_enabled(!busy && pending > 0, egui::Button::new(format!("Detect faces ({pending})"))).clicked() {
            sh.start_detect(None);
        }
        if let Some((d, n)) = sh.detect {
            ui.add(egui::ProgressBar::new(d as f32 / n.max(1) as f32).desired_width(110.0).text(format!("{d}/{n}")));
        }
        ui.separator();
        ui.label(RichText::new("Shape").color(t.dim));
        let cur = sh.engine.project.crop.target_aspect;
        let name = aspect_presets().iter().find(|(_, w, h)| (cur.w - w).abs() < 0.01 && (cur.h - h).abs() < 0.01).map(|p| p.0).unwrap_or("Custom");
        egui::ComboBox::from_id_salt("aspect").width(100.0).selected_text(name).show_ui(ui, |ui| {
            for (n, w, h) in aspect_presets() {
                if ui.selectable_label(n == name, n).clicked() {
                    sh.cmd("photos.set_target_aspect", json!({ "w": w, "h": h }));
                }
            }
        });
        let (mut w, mut h) = (cur.w, cur.h);
        let a = ui.add(egui::DragValue::new(&mut w).range(5.0..=500.0).suffix(" mm")).changed();
        ui.label("×");
        let b = ui.add(egui::DragValue::new(&mut h).range(5.0..=500.0).suffix(" mm")).changed();
        if a || b {
            sh.cmd_drag("photos.set_target_aspect", json!({ "w": w, "h": h }), "aspect");
        }
        ui.separator();
        let mut on = sh.engine.project.crop.auto_crop_on_import;
        if ui.checkbox(&mut on, "Auto-crop on import").changed() {
            sh.cmd("photos.set_auto_crop_on_import", json!({ "on": on }));
        }
        ui.separator();
        ui.menu_button("Background", |ui| {
            let mut c = st.bg_color.unwrap_or(forge_color::Rgba::WHITE);
            let mut tol = st.bg_tolerance.unwrap_or(48.0);
            dim(ui, t, "Replace a plain backdrop with a colour");
            if let Some(n) = widgets::color_edit(ui, c) {
                c = n;
            }
            ui.add(egui::Slider::new(&mut tol, 5.0..=150.0).text("Tolerance"));
            st.bg_color = Some(c);
            st.bg_tolerance = Some(tol);
            if ui.button("Replace in all photos").clicked() {
                if sh.cmd("photos.replace_background", json!({ "keys": "all", "color": c.to_hex(), "tolerance": tol })).is_some() {
                    sh.toast(crate::shell::Kind::Ok, "Backgrounds replaced.");
                }
                ui.close();
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Export cropped ZIP…").clicked() {
                sh.run_ui_command("export.cropped_zip");
            }
        });
    });
}

fn status(ui: &mut Ui, sh: &mut Shell, t: &Tokens, sel: &Option<String>, aspect: f64) {
    ui.horizontal_centered(|ui| {
        let entry = sel.as_ref().and_then(|k| sh.engine.project.photos.get(k)).cloned();
        if let (Some(key), Some(entry)) = (sel, entry) {
            let (nw, nh, bw, bh, d) = canonical(&entry, aspect);
            let cur = forge_doc::manual_from_dest(nw, nh, bw, bh, d);
            let mut zoom = cur.zoom;
            ui.label(RichText::new("Zoom").color(t.dim));
            if ui.add(egui::Slider::new(&mut zoom, 100.0..=400.0).suffix("%").show_value(true)).changed() {
                zoom_by(sh, key, &entry, aspect, zoom / cur.zoom.max(1.0));
            }
            ui.separator();
            if ui.add_enabled(entry.manual_crop.is_some(), egui::Button::new("Reset")).on_hover_text("Back to the automatic crop (Ctrl+0)").clicked()
            {
                sh.cmd("photos.clear_manual_crop", json!({ "keys": [key] }));
            }
            let has_manual = entry.manual_crop.is_some();
            if ui
                .add_enabled(has_manual, egui::Button::new("Apply to unaligned"))
                .on_hover_text("Copy this crop to photos with no face or manual crop")
                .clicked()
            {
                sh.cmd("photos.apply_crop_to", json!({ "from": key, "to": "unaligned" }));
            }
            if ui.add_enabled(has_manual, egui::Button::new("Apply to all")).clicked() {
                sh.cmd("photos.apply_crop_to", json!({ "from": key, "to": "all" }));
            }
            if ui.button("Re-detect").clicked() {
                sh.start_detect(Some(vec![key.clone()]));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let p = &sh.engine.project.photos;
            let ready = p.values().filter(|e| e.status == CropStatus::Ready).count();
            let review = p.values().filter(|e| e.status == CropStatus::NeedsReview).count();
            dim(ui, t, format!("{} photos · {ready} framed · {review} need review", p.len()));
        });
    });
}

fn grid(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens, sel: &Option<String>) {
    let photos = sh.engine.project.photos.clone();
    let cell = vec2(88.0, 112.0);
    let mut requests = 0;
    let mut clicked: Option<String> = None;
    let mut remove: Option<String> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (key, entry) in photos.iter() {
            let (rect, resp) = ui.allocate_exact_size(cell, Sense::click());
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let version = source_version(&entry.source);
            let fresh = st.thumbs.get(key).is_some_and(|(v, _)| *v == version);
            if !fresh && !sh.thumb_pending.contains(key) && requests < 6 {
                requests += 1;
                sh.thumb_pending.insert(key.clone());
                let (k, src) = (key.clone(), entry.source.clone());
                sh.w.spawn(move |tx| {
                    let image = src
                        .bytes()
                        .map_err(|e| e.to_string())
                        .and_then(|b| forge_image::decode(&b).map_err(|e| e.to_string()))
                        .and_then(|i| forge_image::thumbnail(&i, 220).map_err(|e| e.to_string()));
                    let _ = tx.send(Msg::Photo { key: k, version, full: false, image });
                });
            }
            let p = ui.painter();
            let is_sel = sel.as_ref() == Some(key);
            p.rect_filled(rect, 2.0, t.field);
            let img_rect = Rect::from_min_max(rect.min + vec2(4.0, 4.0), rect.max - vec2(4.0, 20.0));
            if let Some((_, tex)) = st.thumbs.get(key) {
                let sz = tex.size_vec2();
                let r = widgets::fit_rect(img_rect, sz.x / sz.y.max(1.0));
                p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            let (label, col) = match entry.status {
                CropStatus::Ready => ("framed", t.ok),
                CropStatus::NeedsReview => ("review", t.warn),
                CropStatus::Processing => ("…", t.dim),
                CropStatus::Pending => ("new", t.dim),
            };
            let label = if entry.manual_crop.is_some() { "manual" } else { label };
            p.text(pos2(rect.min.x + 6.0, rect.max.y - 10.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(10.5), col);
            p.circle_filled(pos2(rect.max.x - 10.0, rect.max.y - 10.0), 3.5, col);
            if is_sel {
                p.rect_stroke(rect, 2.0, Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
            }
            let resp = resp.on_hover_text(&entry.name);
            if resp.clicked() {
                clicked = Some(key.clone());
            }
            resp.context_menu(|ui| {
                if ui.button("Re-detect face").clicked() {
                    sh.start_detect(Some(vec![key.clone()]));
                    ui.close();
                }
                if ui.button("Reset crop").clicked() {
                    sh.cmd("photos.clear_manual_crop", json!({ "keys": [key] }));
                    ui.close();
                }
                if ui.button("Remove photo").clicked() {
                    remove = Some(key.clone());
                    ui.close();
                }
            });
        }
    });
    if let Some(k) = clicked {
        sh.cmd("photos.select", json!({ "key": k }));
    }
    if let Some(k) = remove {
        sh.cmd("photos.remove", json!({ "keys": [k] }));
    }
}

fn editor(ui: &mut Ui, sh: &mut Shell, st: &mut State, t: &Tokens, key: &str, aspect: f64) {
    let Some(entry) = sh.engine.project.photos.get(key).cloned() else { return };
    let version = source_version(&entry.source);
    let have = st.full.as_ref().is_some_and(|(k, v, _)| k == key && *v == version);
    if !have && st.full_pending.as_ref() != Some(&(key.to_string(), version)) {
        st.full_pending = Some((key.to_string(), version));
        let (k, src) = (key.to_string(), entry.source.clone());
        sh.w.spawn(move |tx| {
            let image = src
                .bytes()
                .map_err(|e| e.to_string())
                .and_then(|b| forge_image::decode(&b).map_err(|e| e.to_string()))
                .and_then(|i| forge_image::thumbnail(&i, 1800).map_err(|e| e.to_string()));
            let _ = tx.send(Msg::Photo { key: k, version, full: true, image });
        });
    }
    let avail = ui.available_rect_before_wrap();
    let (area_rect, resp) = ui.allocate_exact_size(avail.size(), Sense::click_and_drag());
    let painter = ui.painter_at(area_rect);
    painter.rect_filled(area_rect, 0.0, t.canvas);
    let box_rect = widgets::fit_rect(area_rect.shrink(48.0), aspect as f32);
    let (bw, bh) = (f64::from(box_rect.width()), f64::from(box_rect.height()));
    let (nw, nh) = (f64::from(entry.natural_w.max(1)), f64::from(entry.natural_h.max(1)));
    let crop = entry.effective_crop(aspect, None);
    let pl = placement(nw, nh, bw, bh, &crop);
    let d = pl.dest;

    if let Some((_, _, tex)) = st.full.as_ref().filter(|_| have) {
        let dest = Rect::from_min_size(box_rect.min + vec2(d.x as f32, d.y as f32), vec2(d.w as f32, d.h as f32));
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(tex.id(), dest, uv, Color32::from_white_alpha(80));
        painter.with_clip_rect(box_rect).image(tex.id(), dest, uv, Color32::WHITE);
    } else {
        painter.text(box_rect.center(), egui::Align2::CENTER_CENTER, "Loading…", egui::FontId::proportional(13.0), t.dim);
    }
    painter.rect_stroke(box_rect, 0.0, Stroke::new(1.5, t.accent), egui::StrokeKind::Outside);
    // Rule-of-thirds guide.
    let g = Stroke::new(1.0, Color32::from_white_alpha(60));
    for i in 1..3 {
        let f = i as f32 / 3.0;
        painter.line_segment(
            [pos2(box_rect.min.x + box_rect.width() * f, box_rect.min.y), pos2(box_rect.min.x + box_rect.width() * f, box_rect.max.y)],
            g,
        );
        painter.line_segment(
            [pos2(box_rect.min.x, box_rect.min.y + box_rect.height() * f), pos2(box_rect.max.x, box_rect.min.y + box_rect.height() * f)],
            g,
        );
    }
    // Face marker.
    if let Some(det) = &entry.raw_detection {
        let sx = d.w / nw;
        let b = &det.bounding_box;
        let r = Rect::from_min_size(
            box_rect.min + vec2((d.x + b.origin_x * sx) as f32, (d.y + b.origin_y * sx) as f32),
            vec2((b.width * sx) as f32, (b.height * sx) as f32),
        );
        painter.with_clip_rect(box_rect).rect_stroke(r, 0.0, Stroke::new(1.0, t.ok.gamma_multiply(0.9)), egui::StrokeKind::Inside);
    }
    painter.text(
        box_rect.left_top() - vec2(0.0, 8.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{}   {}×{} px", entry.name, entry.natural_w, entry.natural_h),
        egui::FontId::proportional(11.0),
        t.dim,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }

    // Drag pans the photo, the wheel zooms about the pointer.
    let mut new_dest: Option<GRect> = None;
    if resp.dragged_by(egui::PointerButton::Primary) {
        let dl = resp.drag_delta();
        new_dest = Some(GRect::new(d.x + f64::from(dl.x), d.y + f64::from(dl.y), d.w, d.h));
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        let pinch = ui.input(|i| i.zoom_delta());
        let f = if (pinch - 1.0).abs() > f32::EPSILON { f64::from(pinch) } else { 1.0 + f64::from(scroll) * 0.0015 };
        if (f - 1.0).abs() > 1e-6 {
            let f = f.clamp(0.8, 1.25);
            let base = new_dest.unwrap_or(d);
            let p = resp.hover_pos().map(|p| p - box_rect.min).unwrap_or(vec2(box_rect.width() / 2.0, box_rect.height() / 2.0));
            let (px, py) = (f64::from(p.x), f64::from(p.y));
            new_dest = Some(GRect::new(px - (px - base.x) * f, py - (py - base.y) * f, base.w * f, base.h * f));
        }
    }
    if let Some(nd) = new_dest {
        let m = forge_doc::manual_from_dest(nw, nh, bw, bh, nd);
        sh.cmd_drag(
            "photos.set_manual_crop",
            json!({ "key": key, "zoom": m.zoom, "offsetX": m.offset_x, "offsetY": m.offset_y }),
            &format!("crop:{key}"),
        );
    }
}

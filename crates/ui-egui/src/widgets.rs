//! Small reusable controls.

use egui::{Color32, Rect, Response, RichText, Sense, Stroke, Ui, vec2};
use forge_color::Rgba;
use forge_geom::Unit;

use crate::theme::Tokens;

pub fn to_c32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// A colour button + hex field. Returns the new colour when it changed.
pub fn color_edit(ui: &mut Ui, c: Rgba) -> Option<Rgba> {
    let mut rgb = [c.r, c.g, c.b];
    let mut out = None;
    ui.horizontal(|ui| {
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            out = Some(Rgba::rgba(rgb[0], rgb[1], rgb[2], c.a));
        }
        let mut hex = c.to_hex();
        let r = ui.add(egui::TextEdit::singleline(&mut hex).desired_width(70.0));
        if r.changed() {
            if let Some(p) = Rgba::parse(&hex) {
                out = Some(p);
            }
        }
    });
    out
}

/// A length stored in centimetres, shown and edited in `unit`.
pub fn unit_value(ui: &mut Ui, cm: &mut f64, unit: Unit, min_cm: f64, max_cm: f64) -> bool {
    let mut v = unit.from_cm(*cm);
    let (lo, hi) = (unit.from_cm(min_cm), unit.from_cm(max_cm));
    let decimals = match unit {
        Unit::Cm | Unit::In => 2,
        Unit::Mm => 1,
        Unit::Px => 0,
    };
    let speed = (hi - lo) / 400.0;
    let r = ui
        .add(egui::DragValue::new(&mut v).range(lo..=hi).speed(speed.clamp(0.01, 5.0)).fixed_decimals(decimals).suffix(format!(" {}", unit.label())));
    if r.changed() {
        *cm = unit.to_cm(v).clamp(min_cm, max_cm);
        true
    } else {
        false
    }
}

pub fn section(ui: &mut Ui, t: &Tokens, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(11.0).color(t.dim));
}

pub fn dim(ui: &mut Ui, t: &Tokens, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(t.dim).size(12.0));
}

pub fn primary_button(ui: &mut Ui, t: &Tokens, text: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(text).color(Color32::WHITE).strong()).fill(t.accent))
}

pub fn unit_picker(ui: &mut Ui, id: &str, unit: &mut Unit) -> bool {
    let before = *unit;
    egui::ComboBox::from_id_salt(id).width(52.0).selected_text(unit.label()).show_ui(ui, |ui| {
        for u in Unit::ALL {
            ui.selectable_value(unit, u, u.label());
        }
    });
    before != *unit
}

/// Draws `tex` as large as fits in `avail` keeping its aspect; returns the rect it occupies.
pub fn fit_rect(avail: egui::Rect, aspect: f32) -> egui::Rect {
    let mut w = avail.width();
    let mut h = w / aspect.max(0.0001);
    if h > avail.height() {
        h = avail.height();
        w = h * aspect;
    }
    egui::Rect::from_center_size(avail.center(), egui::vec2(w.max(1.0), h.max(1.0)))
}

/// A transparency checkerboard over `rect`, drawn only where the painter's clip rectangle shows it.
pub fn checker(painter: &egui::Painter, rect: egui::Rect, dark: bool) {
    let (a, b) = if dark { (Color32::from_gray(74), Color32::from_gray(62)) } else { (Color32::from_gray(255), Color32::from_gray(224)) };
    let vis = rect.intersect(painter.clip_rect());
    if vis.width() <= 0.0 || vis.height() <= 0.0 {
        return;
    }
    painter.rect_filled(vis, 0.0, a);
    let s = 10.0;
    let (x0, y0) = (((vis.min.x - rect.min.x) / s).floor() as i32, ((vis.min.y - rect.min.y) / s).floor() as i32);
    let (x1, y1) = (((vis.max.x - rect.min.x) / s).ceil() as i32, ((vis.max.y - rect.min.y) / s).ceil() as i32);
    for y in y0..y1.min(y0 + 400) {
        for x in x0..x1.min(x0 + 600) {
            if (x + y) & 1 == 0 {
                let r = egui::Rect::from_min_size(rect.min + vec2(x as f32 * s, y as f32 * s), vec2(s, s)).intersect(vis);
                painter.rect_filled(r, 0.0, b);
            }
        }
    }
}

/// A collapsible dock panel: a thin title strip with a chevron, then the body.
pub fn dock(ui: &mut Ui, t: &Tokens, id: &str, title: &str, open: bool, body: impl FnOnce(&mut Ui)) {
    let cid = ui.make_persistent_id(("dock", id));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), cid, open);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, if resp.hovered() { t.hover.gamma_multiply(0.6) } else { t.header });
    p.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, t.line));
    // Chevron.
    let c = rect.left_center() + vec2(12.0, 0.0);
    let k = state.openness(ui.ctx());
    let pts = if k > 0.5 {
        vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
    } else {
        vec![c + vec2(-2.0, -4.0), c + vec2(-2.0, 4.0), c + vec2(3.0, 0.0)]
    };
    p.add(egui::Shape::convex_polygon(pts, t.dim, Stroke::NONE));
    p.text(rect.left_center() + vec2(24.0, 0.0), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(12.0), t.text);
    if resp.clicked() {
        state.toggle(ui);
    }
    state.show_body_unindented(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui);
        });
        let r = ui.min_rect();
        ui.painter().hline(r.x_range(), r.bottom(), Stroke::new(1.0, t.line));
    });
    state.store(ui.ctx());
}

/// What a tool icon draws: the painter, the icon's square and the colour to use.
pub type IconFn = fn(&egui::Painter, Rect, Color32);

/// A square tool-strip button with a painted icon.
pub fn tool_button(ui: &mut Ui, t: &Tokens, active: bool, icon: IconFn, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 30.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, 2.0, t.accent);
    } else if resp.hovered() {
        p.rect_filled(rect, 2.0, t.hover);
    }
    icon(p, Rect::from_center_size(rect.center(), vec2(18.0, 18.0)), if active { Color32::WHITE } else { t.text });
    resp.on_hover_text(tip)
}

pub mod icons {
    use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

    fn at(r: Rect, x: f32, y: f32) -> Pos2 {
        pos2(r.min.x + x * r.width() / 16.0, r.min.y + y * r.height() / 16.0)
    }

    /// An arrow pointer.
    pub fn move_tool(p: &Painter, r: Rect, c: Color32) {
        let pts = [(3.0, 1.0), (3.0, 13.0), (6.0, 10.0), (8.5, 15.0), (10.5, 14.0), (8.0, 9.0), (12.0, 9.0)].map(|(x, y)| at(r, x, y)).to_vec();
        p.add(Shape::convex_polygon(pts, c, Stroke::new(1.0, c)));
    }

    pub fn text_tool(p: &Painter, r: Rect, c: Color32) {
        let s = Stroke::new(2.0, c);
        p.line_segment([at(r, 3.0, 3.0), at(r, 13.0, 3.0)], s);
        p.line_segment([at(r, 8.0, 3.0), at(r, 8.0, 14.0)], s);
        p.line_segment([at(r, 6.0, 14.0), at(r, 10.0, 14.0)], Stroke::new(1.5, c));
    }

    /// A picture frame: a box with a sun and hills.
    pub fn photo_tool(p: &Painter, r: Rect, c: Color32) {
        p.rect_stroke(Rect::from_min_max(at(r, 1.5, 2.5), at(r, 14.5, 13.5)), 1.0, Stroke::new(1.5, c), egui::StrokeKind::Inside);
        p.circle_filled(at(r, 5.0, 6.0), 1.4, c);
        p.add(Shape::line(vec![at(r, 2.5, 12.5), at(r, 7.0, 8.0), at(r, 10.0, 11.0), at(r, 11.5, 9.5), at(r, 14.0, 12.5)], Stroke::new(1.5, c)));
    }

    /// An open hand.
    pub fn hand_tool(p: &Painter, r: Rect, c: Color32) {
        let s = Stroke::new(1.6, c);
        for (x, top) in [(5.0, 4.0), (7.0, 2.5), (9.0, 2.5), (11.0, 4.0)] {
            p.line_segment([at(r, x, 9.0), at(r, x, top)], s);
        }
        p.add(Shape::line(vec![at(r, 3.0, 9.5), at(r, 3.0, 7.0), at(r, 5.0, 9.0), at(r, 5.0, 13.0), at(r, 11.0, 13.0), at(r, 12.5, 9.0)], s));
        p.line_segment([at(r, 5.0, 13.0), at(r, 5.5, 14.5)], s);
    }

    pub fn zoom_tool(p: &Painter, r: Rect, c: Color32) {
        p.circle_stroke(at(r, 6.5, 6.5), 4.6, Stroke::new(1.7, c));
        p.line_segment([at(r, 10.0, 10.0), at(r, 14.0, 14.0)], Stroke::new(2.2, c));
        p.line_segment([at(r, 4.5, 6.5), at(r, 8.5, 6.5)], Stroke::new(1.2, c));
        p.line_segment([at(r, 6.5, 4.5), at(r, 6.5, 8.5)], Stroke::new(1.2, c));
    }

    pub fn crop_tool(p: &Painter, r: Rect, c: Color32) {
        let s = Stroke::new(1.8, c);
        p.line_segment([at(r, 4.0, 1.0), at(r, 4.0, 12.0)], s);
        p.line_segment([at(r, 4.0, 12.0), at(r, 15.0, 12.0)], s);
        p.line_segment([at(r, 1.0, 4.0), at(r, 12.0, 4.0)], s);
        p.line_segment([at(r, 12.0, 4.0), at(r, 12.0, 15.0)], s);
        let _ = vec2(0.0, 0.0);
    }
}

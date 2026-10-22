//! Pan and zoom of a canvas, with the conventions of professional editors:
//! the wheel pans, Ctrl/Cmd+wheel, Alt+wheel and pinch zoom about the pointer, Space+drag, the Hand
//! tool, the middle or right button pan, Ctrl+0 fits, Ctrl+1 shows 100 %, Ctrl +/- zoom in steps.

use egui::{Key, PointerButton, Rect, Response, Ui, Vec2};

pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 64.0;
/// One step of Ctrl +/- (the previous version used the same factor).
pub const STEP: f32 = 1.15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewCmd {
    Fit,
    Actual,
    In,
    Out,
}

/// `zoom` is relative to "fit" (1.0 = the artwork fills the available area).
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub zoom: f32,
    /// Offset of the artwork's centre from the centre of the area.
    pub pan: Vec2,
}

impl Default for View {
    fn default() -> Self {
        Self { zoom: 1.0, pan: Vec2::ZERO }
    }
}

impl View {
    /// The artwork rectangle: the fitted rectangle scaled by the zoom and moved by the pan.
    pub fn rect(&self, area: Rect, fit: Rect) -> Rect {
        Rect::from_center_size(area.center() + self.pan, fit.size() * self.zoom)
    }

    /// Zooms by `factor` keeping the point `p` (screen space) where it is.
    pub fn zoom_about(&mut self, area: Rect, p: egui::Pos2, factor: f32) {
        let nz = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let f = nz / self.zoom;
        let centre = area.center() + self.pan;
        let nc = p + (centre - p) * f;
        self.zoom = nz;
        self.pan = nc - area.center();
    }

    pub fn apply(&mut self, cmd: ViewCmd, area: Rect, fit_w: f32, native_w: f32) {
        match cmd {
            ViewCmd::Fit => *self = Self::default(),
            ViewCmd::Actual => {
                let nz = (native_w / fit_w.max(1.0)).clamp(MIN_ZOOM, MAX_ZOOM);
                let f = nz / self.zoom;
                self.zoom = nz;
                self.pan *= f;
            }
            ViewCmd::In => self.zoom_about(area, area.center(), STEP),
            ViewCmd::Out => self.zoom_about(area, area.center(), 1.0 / STEP),
        }
    }

    /// The zoom in percent of the artwork's own size.
    pub fn percent(&self, fit_w: f32, native_w: f32) -> f32 {
        self.zoom * fit_w / native_w.max(1.0) * 100.0
    }

    pub fn set_percent(&mut self, area: Rect, pct: f32, fit_w: f32, native_w: f32) {
        let target = (pct / 100.0 * native_w / fit_w.max(1.0)).clamp(MIN_ZOOM, MAX_ZOOM);
        self.zoom_about(area, area.center(), target / self.zoom);
    }

    /// Reads wheel, pinch, Space and drag input. `hand` is true while the Hand tool is active.
    /// Returns true while the user is panning (the caller must then not treat the drag as an edit).
    pub fn input(&mut self, ui: &Ui, resp: &Response, area: Rect, hand: bool, typing: bool) -> bool {
        let space = !typing && ui.input(|i| i.key_down(Key::Space));
        let mut panning = false;
        let (scroll, pinch, alt) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.alt));
        if resp.hovered() {
            let at = resp.hover_pos().unwrap_or(area.center());
            if (pinch - 1.0).abs() > f32::EPSILON {
                self.zoom_about(area, at, pinch);
            } else if alt && scroll.y != 0.0 {
                self.zoom_about(area, at, (1.0 + scroll.y * 0.002).clamp(0.8, 1.25));
            } else if scroll != Vec2::ZERO {
                self.pan += scroll;
            }
        }
        let drag_pan = resp.dragged_by(PointerButton::Middle)
            || resp.dragged_by(PointerButton::Secondary)
            || ((space || hand) && resp.dragged_by(PointerButton::Primary));
        if drag_pan {
            self.pan += resp.drag_delta();
            panning = true;
        }
        if resp.hovered() || panning {
            if panning {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if space || hand {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
        }
        // Keep some of the artwork reachable.
        let lim = area.size() * (0.5 + self.zoom);
        self.pan = self.pan.clamp(-lim, lim);
        panning || space || hand
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn area() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor_fixed() {
        let fit = Rect::from_center_size(area().center(), vec2(400.0, 300.0));
        let mut v = View::default();
        let p = pos2(500.0, 250.0);
        // Where p sits inside the artwork, as a fraction.
        let before = v.rect(area(), fit);
        let (fx, fy) = ((p.x - before.min.x) / before.width(), (p.y - before.min.y) / before.height());
        v.zoom_about(area(), p, 2.5);
        let after = v.rect(area(), fit);
        assert!((after.min.x + fx * after.width() - p.x).abs() < 1e-3);
        assert!((after.min.y + fy * after.height() - p.y).abs() < 1e-3);
        assert!((v.zoom - 2.5).abs() < 1e-6);
    }

    #[test]
    fn zoom_is_clamped_and_commands_work() {
        let mut v = View::default();
        for _ in 0..200 {
            v.apply(ViewCmd::In, area(), 400.0, 1000.0);
        }
        assert!((v.zoom - MAX_ZOOM).abs() < 1e-3);
        v.apply(ViewCmd::Fit, area(), 400.0, 1000.0);
        assert_eq!(v.zoom, 1.0);
        v.apply(ViewCmd::Actual, area(), 400.0, 1000.0);
        assert!((v.percent(400.0, 1000.0) - 100.0).abs() < 1e-3);
        v.set_percent(area(), 50.0, 400.0, 1000.0);
        assert!((v.percent(400.0, 1000.0) - 50.0).abs() < 1e-2);
    }
}

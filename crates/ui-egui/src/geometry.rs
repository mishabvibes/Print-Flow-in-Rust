//! Pure editing geometry for the design canvas: smart-guide snapping and rotated resizing. No egui
//! types here, so it is unit-tested in isolation.

/// An axis-aligned rectangle (x, y, w, h) in any consistent unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct R {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl R {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn cx(&self) -> f64 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f64 {
        self.y + self.h / 2.0
    }
}

/// Snap lines found for a drag: positions of the vertical / horizontal guides to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guides {
    pub vertical: Vec<f64>,
    pub horizontal: Vec<f64>,
}

/// The lines a moving rectangle snaps to: the template's edges and centre, and every other field's
/// edges and centre. `others` are in template percent.
fn targets(others: &[R], extent: f64, axis_x: bool) -> Vec<f64> {
    let mut t = vec![0.0, extent / 2.0, extent];
    for o in others {
        if axis_x {
            t.extend([o.x, o.cx(), o.x + o.w]);
        } else {
            t.extend([o.y, o.cy(), o.y + o.h]);
        }
    }
    t
}

fn snap_axis(start: f64, size: f64, targets: &[f64], threshold: f64) -> Option<(f64, f64)> {
    // Left/top, centre, right/bottom of the moving rectangle against each target.
    let mut best: Option<(f64, f64)> = None; // (delta, guide position)
    for t in targets {
        for edge in [start, start + size / 2.0, start + size] {
            let d = t - edge;
            if d.abs() <= threshold && best.is_none_or(|(bd, _)| d.abs() < bd.abs()) {
                best = Some((d, *t));
            }
        }
    }
    best
}

/// Snaps a rectangle being moved. `bounds` is (width, height) of the template in the same unit.
/// Returns the adjusted rectangle and the guides to draw.
pub fn snap_move(r: R, others: &[R], bounds: (f64, f64), threshold: f64) -> (R, Guides) {
    let mut out = r;
    let mut g = Guides::default();
    if let Some((d, at)) = snap_axis(r.x, r.w, &targets(others, bounds.0, true), threshold) {
        out.x += d;
        g.vertical.push(at);
    }
    if let Some((d, at)) = snap_axis(r.y, r.h, &targets(others, bounds.1, false), threshold) {
        out.y += d;
        g.horizontal.push(at);
    }
    (out, g)
}

/// The eight resize handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

impl Handle {
    pub const ALL: [Handle; 8] = [Handle::NW, Handle::N, Handle::NE, Handle::E, Handle::SE, Handle::S, Handle::SW, Handle::W];

    /// (-1|0|1, -1|0|1): which sides the handle moves.
    pub fn dir(self) -> (f64, f64) {
        match self {
            Handle::N => (0.0, -1.0),
            Handle::NE => (1.0, -1.0),
            Handle::E => (1.0, 0.0),
            Handle::SE => (1.0, 1.0),
            Handle::S => (0.0, 1.0),
            Handle::SW => (-1.0, 1.0),
            Handle::W => (-1.0, 0.0),
            Handle::NW => (-1.0, -1.0),
        }
    }
}

fn rot(p: (f64, f64), a: f64) -> (f64, f64) {
    let (s, c) = a.sin_cos();
    (p.0 * c - p.1 * s, p.0 * s + p.1 * c)
}

const MIN_SIZE: f64 = 1.0;

/// Resizes `r` (rotated by `angle` radians about its centre) by dragging `handle` to `pointer`.
/// The opposite edge/corner stays where it is on screen. `keep_aspect` (corner handles) keeps the
/// width/height ratio.
pub fn resize(r: R, angle: f64, handle: Handle, pointer: (f64, f64), keep_aspect: bool) -> R {
    let (dx, dy) = handle.dir();
    let c = (r.cx(), r.cy());
    // Pointer in the field's own (unrotated) frame, relative to its centre.
    let p = rot((pointer.0 - c.0, pointer.1 - c.1), -angle);
    let (hw, hh) = (r.w / 2.0, r.h / 2.0);
    // The fixed anchor, in the local frame: the opposite side (or the centre line on a free axis).
    let (ax, ay) = (-dx * hw, -dy * hh);
    let mut nw = if dx == 0.0 { r.w } else { ((p.0 - ax) * dx).max(MIN_SIZE) };
    let mut nh = if dy == 0.0 { r.h } else { ((p.1 - ay) * dy).max(MIN_SIZE) };
    if keep_aspect && dx != 0.0 && dy != 0.0 && r.w > 0.0 && r.h > 0.0 {
        let k = (nw / r.w).max(nh / r.h);
        nw = (r.w * k).max(MIN_SIZE);
        nh = (r.h * k).max(MIN_SIZE);
    }
    // The new local centre sits halfway between the anchor and the moved side.
    let local_c = (ax + dx * nw / 2.0, ay + dy * nh / 2.0);
    let g = rot(local_c, angle);
    let new_c = (c.0 + g.0, c.1 + g.1);
    R::new(new_c.0 - nw / 2.0, new_c.1 - nh / 2.0, nw, nh)
}

/// The rotation (degrees, -180..=180) for a pointer at `pointer` around centre `c`, measured from
/// straight up. `snap` rounds to 15-degree steps.
pub fn rotation_from_pointer(c: (f64, f64), pointer: (f64, f64), snap: bool) -> f64 {
    let a = (pointer.0 - c.0).atan2(-(pointer.1 - c.1)).to_degrees();
    let a = if snap { (a / 15.0).round() * 15.0 } else { a };
    if a > 180.0 { a - 360.0 } else { a }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_centre_and_other_edges() {
        let others = [R::new(60.0, 10.0, 20.0, 10.0)];
        // A 20-wide box whose centre is 1.5 off the template centre snaps to it.
        let (r, g) = snap_move(R::new(38.5, 40.0, 20.0, 10.0), &others, (100.0, 100.0), 2.0);
        assert!((r.cx() - 50.0).abs() < 1e-9 && g.vertical == vec![50.0]);
        // Its right edge near another field's left edge (60).
        let (r, g) = snap_move(R::new(40.5, 40.0, 20.0, 10.0), &others, (100.0, 100.0), 2.0);
        assert!((r.x + r.w - 60.0).abs() < 1e-9 || (r.cx() - 50.0).abs() < 1e-9);
        assert_eq!(g.vertical.len(), 1);
        // Nothing within the threshold: unchanged.
        let (r, g) = snap_move(R::new(12.0, 33.0, 10.0, 7.0), &[], (100.0, 100.0), 1.0);
        assert_eq!(r, R::new(12.0, 33.0, 10.0, 7.0));
        assert_eq!(g, Guides::default());
    }

    #[test]
    fn resize_keeps_the_opposite_corner_fixed_when_rotated() {
        let r = R::new(100.0, 100.0, 80.0, 40.0);
        let angle = 30f64.to_radians();
        // Where the NW corner is on screen before.
        let nw_before = {
            let l = rot((-40.0, -20.0), angle);
            (r.cx() + l.0, r.cy() + l.1)
        };
        // Drag the SE corner outwards.
        let se_local = rot((50.0, 30.0), angle);
        let pointer = (r.cx() + se_local.0, r.cy() + se_local.1);
        let n = resize(r, angle, Handle::SE, pointer, false);
        assert!((n.w - 90.0).abs() < 1e-6 && (n.h - 50.0).abs() < 1e-6, "{n:?}");
        let l = rot((-n.w / 2.0, -n.h / 2.0), angle);
        let nw_after = (n.cx() + l.0, n.cy() + l.1);
        assert!((nw_before.0 - nw_after.0).abs() < 1e-6 && (nw_before.1 - nw_after.1).abs() < 1e-6);
    }

    #[test]
    fn resize_edges_only_change_one_axis_and_never_collapse() {
        let r = R::new(10.0, 10.0, 40.0, 20.0);
        let n = resize(r, 0.0, Handle::E, (80.0, 500.0), false);
        assert_eq!((n.x, n.h), (10.0, 20.0));
        assert!((n.w - 70.0).abs() < 1e-9);
        let tiny = resize(r, 0.0, Handle::E, (-500.0, 0.0), false);
        assert!(tiny.w >= MIN_SIZE);
        let a = resize(r, 0.0, Handle::SE, (90.0, 40.0), true);
        assert!((a.w / a.h - 2.0).abs() < 1e-9);
    }

    #[test]
    fn rotation_angles() {
        assert!((rotation_from_pointer((0.0, 0.0), (0.0, -10.0), false)).abs() < 1e-9);
        assert!((rotation_from_pointer((0.0, 0.0), (10.0, 0.0), false) - 90.0).abs() < 1e-9);
        assert!((rotation_from_pointer((0.0, 0.0), (10.0, -9.0), true) % 15.0).abs() < 1e-9);
        assert!((rotation_from_pointer((0.0, 0.0), (-10.0, 0.0), false) + 90.0).abs() < 1e-9);
    }
}

use serde::{Deserialize, Serialize};

/// `v` if it is finite, else `default`.
pub fn finite_or(v: f64, default: f64) -> f64 {
    if v.is_finite() { v } else { default }
}

/// Clamp that never panics (the std one panics when `min > max`) and maps NaN to `min`.
pub fn clamp(v: f64, min: f64, max: f64) -> f64 {
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    if v.is_nan() { lo } else { v.max(lo).min(hi) }
}

/// Round to `decimals` decimal places (display helper).
pub fn round_to(v: f64, decimals: u32) -> f64 {
    let p = 10f64.powi(decimals.min(9) as i32);
    (v * p).round() / p
}

/// A point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// An axis-aligned rectangle (origin + size), in whatever space the caller works in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.right() && py >= self.y && py <= self.bottom()
    }
    pub fn aspect(&self) -> Option<f64> {
        (self.h > 0.0 && self.w > 0.0).then(|| self.w / self.h)
    }
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.w.is_finite() && self.h.is_finite()
    }
    /// Intersection with `other`, if the two overlap with positive area.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        // f64::min/max ignore NaN, so reject non-finite input explicitly.
        if !(self.is_finite() && other.is_finite()) {
            return None;
        }
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }
    /// Scale all four components (e.g. cm -> px).
    pub fn scaled(&self, k: f64) -> Rect {
        Rect::new(self.x * k, self.y * k, self.w * k, self.h * k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_is_total() {
        assert_eq!(clamp(5.0, 0.0, 3.0), 3.0);
        assert_eq!(clamp(5.0, 3.0, 0.0), 3.0);
        assert_eq!(clamp(f64::NAN, 1.0, 2.0), 1.0);
    }

    #[test]
    fn intersect_basic() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.intersect(&b), Some(Rect::new(5.0, 5.0, 5.0, 5.0)));
        assert_eq!(a.intersect(&Rect::new(20.0, 0.0, 1.0, 1.0)), None);
    }
}

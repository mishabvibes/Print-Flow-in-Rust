use serde::{Deserialize, Serialize};

/// A physical length unit. Internal storage everywhere is centimetres; units only exist at the UI
/// boundary (`Unit::to_cm` / `Unit::from_cm`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    #[default]
    Cm,
    Mm,
    In,
    /// CSS pixel: 96 per inch (what the previous Electron version used).
    Px,
}

impl<'de> Deserialize<'de> for Unit {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Option::<String>::deserialize(d)?.and_then(|s| Unit::parse(&s)).unwrap_or_default())
    }
}

impl Unit {
    pub const ALL: [Unit; 4] = [Unit::Cm, Unit::Mm, Unit::In, Unit::Px];

    /// Centimetres in one of this unit.
    pub fn cm_per_unit(self) -> f64 {
        match self {
            Unit::Cm => 1.0,
            Unit::Mm => 0.1,
            Unit::In => 2.54,
            Unit::Px => 2.54 / 96.0,
        }
    }
    pub fn to_cm(self, v: f64) -> f64 {
        if v.is_finite() { v * self.cm_per_unit() } else { 0.0 }
    }
    pub fn from_cm(self, cm: f64) -> f64 {
        if cm.is_finite() { cm / self.cm_per_unit() } else { 0.0 }
    }
    pub fn label(self) -> &'static str {
        match self {
            Unit::Cm => "cm",
            Unit::Mm => "mm",
            Unit::In => "in",
            Unit::Px => "px",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Unit::Cm => "Centimeters",
            Unit::Mm => "Millimeters",
            Unit::In => "Inches",
            Unit::Px => "Pixels",
        }
    }
    /// A sensible spinner step for this unit.
    pub fn step(self) -> f64 {
        match self {
            Unit::Cm => 0.1,
            Unit::Mm => 1.0,
            Unit::In => 0.05,
            Unit::Px => 1.0,
        }
    }
    /// Decimals worth showing.
    pub fn decimals(self) -> usize {
        match self {
            Unit::Px | Unit::Mm => 1,
            _ => 2,
        }
    }
    pub fn parse(s: &str) -> Option<Unit> {
        Unit::ALL.into_iter().find(|u| u.label().eq_ignore_ascii_case(s.trim()) || u.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// A named paper size (portrait, centimetres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paper {
    pub id: &'static str,
    pub label: &'static str,
    pub w: f64,
    pub h: f64,
}

pub const PAPER_SIZES: &[Paper] = &[
    Paper { id: "A3", label: "A3 — 29.7 × 42 cm", w: 29.7, h: 42.0 },
    Paper { id: "A4", label: "A4 — 21 × 29.7 cm", w: 21.0, h: 29.7 },
    Paper { id: "A5", label: "A5 — 14.8 × 21 cm", w: 14.8, h: 21.0 },
    Paper { id: "Letter", label: "Letter — 21.59 × 27.94 cm", w: 21.59, h: 27.94 },
    Paper { id: "Legal", label: "Legal — 21.59 × 35.56 cm", w: 21.59, h: 35.56 },
];

impl Paper {
    pub fn find(id: &str) -> Option<&'static Paper> {
        PAPER_SIZES.iter().find(|p| p.id.eq_ignore_ascii_case(id))
    }
    /// The paper size that matches `w` × `h` (either orientation) within 0.05 cm.
    pub fn matching(w: f64, h: f64) -> Option<&'static Paper> {
        PAPER_SIZES.iter().find(|p| {
            let same = |a: f64, b: f64| (a - b).abs() < 0.05;
            (same(p.w, w) && same(p.h, h)) || (same(p.w, h) && same(p.h, w))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_round_trip() {
        for u in Unit::ALL {
            let v = u.from_cm(u.to_cm(12.5));
            assert!((v - 12.5).abs() < 1e-9, "{u:?}");
        }
        assert_eq!(Unit::In.to_cm(1.0), 2.54);
        assert_eq!(Unit::Mm.to_cm(10.0), 1.0);
        assert_eq!(Unit::Cm.to_cm(f64::NAN), 0.0);
    }

    #[test]
    fn paper_lookup() {
        assert_eq!(Paper::matching(29.7, 21.0).map(|p| p.id), Some("A4"));
        assert!(Paper::matching(10.0, 10.0).is_none());
    }
}

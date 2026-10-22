//! Sheet (print layout) settings. Every length is in centimetres.

use serde::{Deserialize, Deserializer, Serialize};

use forge_color::Rgba;
use forge_geom::{GridMode, GridSpec, Unit};

/// Named paper sizes, portrait, in cm.
pub const PRESETS: &[(&str, f64, f64)] =
    &[("A3", 29.7, 42.0), ("A4", 21.0, 29.7), ("A5", 14.8, 21.0), ("Letter", 21.59, 27.94), ("Legal", 21.59, 35.56)];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

macro_rules! str_enum_serde {
    ($t:ty { $($v:ident => $s:literal),+ } default $d:ident) => {
        impl Serialize for $t {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(match self { $(Self::$v => $s),+ })
            }
        }
        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Ok(match Option::<String>::deserialize(d)?.as_deref() { $(Some($s) => Self::$v,)+ _ => Self::$d })
            }
        }
    };
}
str_enum_serde!(Orientation { Portrait => "portrait", Landscape => "landscape" } default Portrait);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetSize {
    pub w: f64,
    pub h: f64,
    #[serde(default)]
    pub orientation: Orientation,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Margins {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
    pub uniform: bool,
}

impl Default for Margins {
    fn default() -> Self {
        Self { top: 1.0, right: 1.0, bottom: 1.0, left: 1.0, uniform: true }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardSize {
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gap {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomGrid {
    pub cols: u32,
    pub rows: u32,
}

/// Cut-guide borders around each card. `size` is in points, `color` is `#rrggbb`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridBorder {
    pub enabled: bool,
    pub size: f64,
    pub color: Rgba,
}

impl Default for GridBorder {
    fn default() -> Self {
        Self { enabled: false, size: 0.5, color: Rgba::BLACK }
    }
}

/// All composer settings (what the original called `composerState`, minus the image list).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SheetSettings {
    pub sheet: SheetSize,
    pub margin: Margins,
    pub card: CardSize,
    pub gap: Gap,
    pub grid_mode: GridMode,
    pub custom_grid: CustomGrid,
    pub grid_border: GridBorder,
    pub dpi: u32,
    pub unit_sheet: Unit,
    pub unit_card: Unit,
}

impl Default for SheetSettings {
    fn default() -> Self {
        Self {
            sheet: SheetSize { w: 21.0, h: 29.7, orientation: Orientation::Portrait },
            margin: Margins::default(),
            card: CardSize { w: 8.6, h: 5.4 },
            gap: Gap { x: 0.3, y: 0.3 },
            grid_mode: GridMode::Auto,
            custom_grid: CustomGrid { cols: 3, rows: 4 },
            grid_border: GridBorder::default(),
            dpi: 300,
            unit_sheet: Unit::Cm,
            unit_card: Unit::Cm,
        }
    }
}

impl SheetSettings {
    /// The inputs of the grid maths (`forge_geom::compute_grid`).
    pub fn grid_spec(&self) -> GridSpec {
        GridSpec {
            sheet_w: self.sheet.w,
            sheet_h: self.sheet.h,
            margin_top: self.margin.top,
            margin_right: self.margin.right,
            margin_bottom: self.margin.bottom,
            margin_left: self.margin.left,
            card_w: self.card.w,
            card_h: self.card.h,
            gap_x: self.gap.x,
            gap_y: self.gap.y,
            mode: self.grid_mode,
            cols: self.custom_grid.cols,
            rows: self.custom_grid.rows,
        }
    }
    /// Sets the paper to a named preset in the current orientation. Unknown names (`"Custom"`) do nothing.
    pub fn apply_preset(&mut self, name: &str) -> bool {
        let Some(&(_, w, h)) = PRESETS.iter().find(|(n, ..)| *n == name) else { return false };
        match self.sheet.orientation {
            Orientation::Landscape => (self.sheet.w, self.sheet.h) = (h, w),
            Orientation::Portrait => (self.sheet.w, self.sheet.h) = (w, h),
        }
        true
    }
    /// The preset matching the current paper size in either orientation, if any.
    pub fn preset_name(&self) -> Option<&'static str> {
        let (a, b) = (self.sheet.w.min(self.sheet.h), self.sheet.w.max(self.sheet.h));
        PRESETS.iter().find(|(_, w, h)| (a - w).abs() < 0.005 && (b - h).abs() < 0.005).map(|p| p.0)
    }
    pub fn set_orientation(&mut self, o: Orientation) {
        if self.sheet.orientation == o {
            return;
        }
        self.sheet.orientation = o;
        let (lo, hi) = (self.sheet.w.min(self.sheet.h), self.sheet.w.max(self.sheet.h));
        match o {
            Orientation::Portrait => (self.sheet.w, self.sheet.h) = (lo, hi),
            Orientation::Landscape => (self.sheet.w, self.sheet.h) = (hi, lo),
        }
    }
    pub fn set_uniform_margin(&mut self, v: f64) {
        let v = v.max(0.0);
        self.margin.top = v;
        self.margin.right = v;
        self.margin.bottom = v;
        self.margin.left = v;
        self.margin.uniform = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_orientation() {
        let mut s = SheetSettings::default();
        assert_eq!(s.preset_name(), Some("A4"));
        s.set_orientation(Orientation::Landscape);
        assert_eq!((s.sheet.w, s.sheet.h), (29.7, 21.0));
        assert_eq!(s.preset_name(), Some("A4"));
        assert!(s.apply_preset("A3"));
        assert_eq!((s.sheet.w, s.sheet.h), (42.0, 29.7));
        assert!(!s.apply_preset("Custom"));
    }

    #[test]
    fn empty_legacy_composer_object_loads() {
        let s: SheetSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, SheetSettings::default());
        let s: SheetSettings = serde_json::from_str(r#"{"gridMode":"custom","customGrid":{"cols":2,"rows":5},"unitSheet":"mm"}"#).unwrap();
        assert_eq!(s.grid_mode, GridMode::Custom);
        assert_eq!(s.unit_sheet, Unit::Mm);
    }
}

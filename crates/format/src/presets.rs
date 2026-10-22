use std::path::Path;

use forge_doc::{Design, Field, SheetSettings, Template};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{FormatError, FormatResult};

/// The previous app's design preset: `{ template, fields }`.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DesignPreset {
    pub template: Option<Template>,
    pub fields: Vec<Value>,
}

/// The previous app's layout preset: sheet, margins, card, gap, grid mode, custom grid and border.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutPreset(pub SheetSettings);

pub fn save_design_preset(design: &Design, path: &Path) -> FormatResult<()> {
    let preset =
        DesignPreset { template: design.template.clone(), fields: design.fields.iter().map(serde_json::to_value).collect::<Result<_, _>>()? };
    std::fs::write(path, serde_json::to_vec_pretty(&preset)?)?;
    Ok(())
}

/// Loads a design preset: the template and the fields it holds (fields that cannot be read are
/// skipped; the second value is how many).
pub fn load_design_preset(path: &Path) -> FormatResult<(Option<Template>, Vec<Field>, usize)> {
    let bytes = std::fs::read(path)?;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    let v: Value = serde_json::from_slice(bytes)?;
    if !v.is_object() {
        return Err(FormatError::Invalid("not a design preset".into()));
    }
    let template = v.get("template").filter(|t| !t.is_null()).and_then(|t| serde_json::from_value(t.clone()).ok());
    let mut skipped = 0;
    let mut fields: Vec<Field> = Vec::new();
    if let Some(Value::Array(a)) = v.get("fields") {
        for f in a {
            match serde_json::from_value::<Field>(f.clone()) {
                Ok(f) => fields.push(f),
                Err(_) => skipped += 1,
            }
        }
    }
    Ok((template, fields, skipped))
}

pub fn save_layout_preset(s: &SheetSettings, path: &Path) -> FormatResult<()> {
    let v = serde_json::json!({
        "sheet": s.sheet, "margin": s.margin, "card": s.card, "gap": s.gap,
        "gridMode": s.grid_mode, "customGrid": s.custom_grid, "gridBorder": s.grid_border,
    });
    std::fs::write(path, serde_json::to_vec_pretty(&v)?)?;
    Ok(())
}

/// Applies the layout preset in `path` on top of `base` (keys the preset lacks keep their value, as
/// the previous app did).
pub fn load_layout_preset(path: &Path, base: &SheetSettings) -> FormatResult<SheetSettings> {
    let bytes = std::fs::read(path)?;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    let v: Value = serde_json::from_slice(bytes)?;
    let Value::Object(patch) = v else { return Err(FormatError::Invalid("not a layout preset".into())) };
    let mut cur = serde_json::to_value(base)?;
    if let Value::Object(cur_map) = &mut cur {
        for key in ["sheet", "margin", "card", "gap", "gridMode", "customGrid", "gridBorder"] {
            if let Some(val) = patch.get(key).filter(|v| !v.is_null()) {
                cur_map.insert(key.to_string(), val.clone());
            }
        }
    }
    Ok(serde_json::from_value(cur).unwrap_or_else(|_| base.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_preset_round_trips_and_keeps_other_settings() {
        let dir = std::env::temp_dir().join(format!("forge-preset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = SheetSettings::default();
        s.card.w = 6.0;
        s.gap.x = 0.8;
        s.grid_border.enabled = true;
        let p = dir.join("l.json");
        save_layout_preset(&s, &p).unwrap();
        let base = SheetSettings { dpi: 600, ..SheetSettings::default() };
        let loaded = load_layout_preset(&p, &base).unwrap();
        assert_eq!(loaded.card.w, 6.0);
        assert_eq!(loaded.gap.x, 0.8);
        assert!(loaded.grid_border.enabled);
        assert_eq!(loaded.dpi, 600);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn design_preset_round_trips() {
        let dir = std::env::temp_dir().join(format!("forge-dpreset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let d = Design { fields: vec![Field::new_text("f1".into()), Field::new_photo("f2".into())], ..Design::default() };
        let p = dir.join("d.json");
        save_design_preset(&d, &p).unwrap();
        let (t, f, skipped) = load_design_preset(&p).unwrap();
        assert!(t.is_none());
        assert_eq!((f.len(), skipped), (2, 0));
        std::fs::write(&p, b"[1]").unwrap();
        assert!(load_design_preset(&p).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}

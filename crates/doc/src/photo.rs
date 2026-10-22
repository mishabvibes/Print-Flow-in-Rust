//! A photo as the project knows it: its source plus the crop decisions made for it.

use serde::{Deserialize, Deserializer, Serialize};

use crate::crop::{CropRect, EffectiveCrop, FaceDetection, ManualCrop, compute_auto_crop};
use crate::field::PhotoProps;
use crate::source::ImageSource;

/// Where a photo is in the cropping workflow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum CropStatus {
    #[default]
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "processing")]
    Processing,
    /// A face was found and framed.
    #[serde(rename = "ready")]
    Ready,
    /// No face (or detection off): needs a manual crop.
    #[serde(rename = "needs-review")]
    NeedsReview,
}

impl<'de> Deserialize<'de> for CropStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Option::<String>::deserialize(d)?.as_deref() {
            Some("processing") => Self::Processing,
            Some("ready") => Self::Ready,
            Some("needs-review") => Self::NeedsReview,
            _ => Self::Pending,
        })
    }
}

/// A photo and its crop state. Field names match the original project format.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PhotoEntry {
    /// The file name it was imported under.
    pub name: String,
    #[serde(rename = "dataUrl")]
    pub source: ImageSource,
    pub natural_w: u32,
    pub natural_h: u32,
    pub auto_crop: Option<CropRect>,
    pub manual_crop: Option<ManualCrop>,
    pub faces_detected: bool,
    pub status: CropStatus,
    /// The detector's raw answer, so the auto crop can be recomputed for any aspect without
    /// detecting again.
    #[serde(rename = "_rawDetection")]
    pub raw_detection: Option<FaceDetection>,
}

impl PhotoEntry {
    pub fn new(name: impl Into<String>, source: ImageSource) -> Self {
        Self { name: name.into(), source, ..Self::default() }
    }

    /// Re-derives `auto_crop` from the stored detection for `aspect`. No-op without a detection.
    pub fn recompute_auto_crop(&mut self, aspect: f64) {
        if let Some(det) = &self.raw_detection
            && self.natural_w > 0
        {
            self.auto_crop = compute_auto_crop(det, f64::from(self.natural_w), f64::from(self.natural_h), aspect);
        }
    }

    /// The crop to render with for a box of `aspect`: manual > automatic (recomputed live from the
    /// detection when there is one, so a resized photo field never shows a stale crop) > field defaults.
    pub fn effective_crop(&self, aspect: f64, field: Option<&PhotoProps>) -> EffectiveCrop {
        if let Some(m) = self.manual_crop {
            return EffectiveCrop::Manual(m);
        }
        if let Some(det) = &self.raw_detection
            && self.natural_w > 0
            && let Some(c) = compute_auto_crop(det, f64::from(self.natural_w), f64::from(self.natural_h), aspect)
        {
            return EffectiveCrop::Auto(c);
        }
        if let Some(c) = self.auto_crop {
            return EffectiveCrop::Auto(c);
        }
        let f = field.map(|p| ManualCrop { zoom: p.zoom, offset_x: p.offset_x, offset_y: p.offset_y }).unwrap_or_default();
        EffectiveCrop::Default(f)
    }
}

/// The original stored a photo as a bare data-URL string in very old projects; accept that too.
pub fn photos_from_json(v: serde_json::Value) -> std::collections::BTreeMap<String, PhotoEntry> {
    let mut out = std::collections::BTreeMap::new();
    let serde_json::Value::Object(map) = v else { return out };
    for (k, val) in map {
        let entry = match &val {
            serde_json::Value::String(s) => {
                let source = serde_json::from_value::<ImageSource>(serde_json::Value::String(s.clone())).unwrap_or_default();
                PhotoEntry { name: k.clone(), source, ..PhotoEntry::default() }
            }
            other => serde_json::from_value::<PhotoEntry>(other.clone()).unwrap_or_default(),
        };
        out.insert(k, entry);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crop::FaceBox;

    #[test]
    fn priority_manual_then_auto_then_default() {
        let mut p = PhotoEntry { natural_w: 1000, natural_h: 1000, ..PhotoEntry::default() };
        assert!(matches!(p.effective_crop(1.0, None), EffectiveCrop::Default(_)));
        p.auto_crop = Some(CropRect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 });
        assert!(matches!(p.effective_crop(1.0, None), EffectiveCrop::Auto(_)));
        p.manual_crop = Some(ManualCrop::default());
        assert!(matches!(p.effective_crop(1.0, None), EffectiveCrop::Manual(_)));
    }

    #[test]
    fn auto_crop_follows_field_aspect() {
        let p = PhotoEntry {
            natural_w: 1000,
            natural_h: 1000,
            raw_detection: Some(FaceDetection {
                bounding_box: FaceBox { origin_x: 400.0, origin_y: 400.0, width: 100.0, height: 100.0 },
                keypoints: vec![],
                score: 0.0,
            }),
            ..PhotoEntry::default()
        };
        let EffectiveCrop::Auto(a) = p.effective_crop(1.0, None) else { panic!("auto") };
        let EffectiveCrop::Auto(b) = p.effective_crop(0.5, None) else { panic!("auto") };
        // a narrower box needs a taller crop around the same face
        assert!(b.height > a.height);
    }

    #[test]
    fn legacy_string_photos() {
        let v = serde_json::json!({"a": "data:image/png;base64,iVBORw0KGgo=", "b": {"dataUrl": "", "naturalW": 5, "status": "ready"}});
        let m = photos_from_json(v);
        assert_eq!(m.len(), 2);
        assert_eq!(m["b"].natural_w, 5);
        assert_eq!(m["b"].status, CropStatus::Ready);
    }
}

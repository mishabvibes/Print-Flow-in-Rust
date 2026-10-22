//! The whole project, as one cheap-to-clone value.
//!
//! The previous version kept two photo stores (one per workspace) and copied entries between them
//! on "Send to Design". Here there is **one** photo library shared by every workspace; the automatic
//! crop is derived from the stored face detection for whatever box it is drawn into, so changing a
//! photo field's size or the crop aspect can never leave a stale crop behind.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::design::Design;
use crate::photo::PhotoEntry;
use crate::sheet::SheetSettings;
use crate::source::ImageSource;
use crate::table::Table;

/// Crop target in millimetres (35 x 45 = passport).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TargetAspect {
    pub w: f64,
    pub h: f64,
}

impl Default for TargetAspect {
    fn default() -> Self {
        Self { w: 35.0, h: 45.0 }
    }
}

impl TargetAspect {
    pub fn ratio(&self) -> f64 {
        if self.h > 0.0 && self.w > 0.0 && self.w.is_finite() && self.h.is_finite() { self.w / self.h } else { crate::crop::PASSPORT_ASPECT }
    }
}

/// Settings of the "Crop Photos" workspace.
#[derive(Clone, Debug, PartialEq)]
pub struct CropSettings {
    pub target_aspect: TargetAspect,
    pub auto_crop_on_import: bool,
    /// The photo selected in the grid.
    pub selected_key: Option<String>,
}

impl Default for CropSettings {
    fn default() -> Self {
        Self { target_aspect: TargetAspect::default(), auto_crop_on_import: true, selected_key: None }
    }
}

/// What the "Arrange & Print" workspace lays out on sheets.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CardSource {
    /// One generated card per data row (rendered on demand, never held in memory all at once).
    #[default]
    Generated,
    /// The cropped photos of the photo library, one per sheet cell.
    CroppedPhotos,
    /// Card images imported from files.
    Files(Vec<(String, ImageSource)>),
}

/// One cell's worth of content.
#[derive(Clone, Debug, PartialEq)]
pub enum CardRef {
    /// Data row `n`, rendered through the design.
    Row(usize),
    /// A photo of the library, cropped to the sheet's card aspect.
    Photo(String),
    /// Imported file `n` of [`CardSource::Files`].
    File(usize),
}

/// A whole project. Cloning copies a few pointers.
#[derive(Clone, Debug, Default)]
pub struct Project {
    pub design: Design,
    pub data: Arc<Table>,
    pub photos: Arc<BTreeMap<String, Arc<PhotoEntry>>>,
    pub crop: CropSettings,
    pub sheet: SheetSettings,
    pub card_source: CardSource,
    /// The data row the design preview shows.
    pub preview_index: usize,
    /// The field selected in the design workspace.
    pub selected_field: Option<String>,
}

impl Project {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn photo(&self, key: &str) -> Option<&Arc<PhotoEntry>> {
        self.photos.get(key)
    }

    /// The photo for data row `row` (matched on the photo key column), if any.
    pub fn photo_for_row(&self, row: usize) -> Option<&Arc<PhotoEntry>> {
        let key = self.design.photo_key(&self.data, row);
        if key.is_empty() { None } else { self.photos.get(&key) }
    }

    /// Number of cards the sheets will hold.
    pub fn card_count(&self) -> usize {
        match &self.card_source {
            CardSource::Generated => self.data.len(),
            CardSource::CroppedPhotos => self.photos.len(),
            CardSource::Files(f) => f.len(),
        }
    }

    /// The `i`-th card's content reference.
    pub fn card_ref(&self, i: usize) -> Option<CardRef> {
        match &self.card_source {
            CardSource::Generated => (i < self.data.len()).then_some(CardRef::Row(i)),
            CardSource::CroppedPhotos => self.photos.keys().nth(i).map(|k| CardRef::Photo(k.clone())),
            CardSource::Files(f) => (i < f.len()).then_some(CardRef::File(i)),
        }
    }

    /// All cards in order.
    pub fn card_refs(&self) -> Vec<CardRef> {
        match &self.card_source {
            CardSource::Generated => (0..self.data.len()).map(CardRef::Row).collect(),
            CardSource::CroppedPhotos => self.photos.keys().map(|k| CardRef::Photo(k.clone())).collect(),
            CardSource::Files(f) => (0..f.len()).map(CardRef::File).collect(),
        }
    }

    /// The file name (no extension) a card is exported under.
    pub fn card_name(&self, r: &CardRef) -> String {
        match r {
            CardRef::Row(i) => {
                let col = if self.design.photo_key_column.is_empty() {
                    self.data.headers.first().cloned().unwrap_or_default()
                } else {
                    self.design.photo_key_column.clone()
                };
                let v = self.data.cell(*i, &col).unwrap_or("").trim();
                if v.is_empty() { format!("card_{}", i + 1) } else { crate::util::sanitize_filename(v) }
            }
            CardRef::Photo(k) => self.photos.get(k).map(|p| strip_ext(&p.name).to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| k.clone()),
            CardRef::File(i) => match &self.card_source {
                CardSource::Files(f) => f.get(*i).map(|(n, _)| strip_ext(n).to_string()).unwrap_or_else(|| format!("card_{}", i + 1)),
                _ => format!("card_{}", i + 1),
            },
        }
    }

    /// Keeps indices valid after the data or photo set changed.
    pub fn clamp_indices(&mut self) {
        self.preview_index = self.preview_index.min(self.data.len().saturating_sub(1));
        if let Some(k) = &self.crop.selected_key
            && !self.photos.contains_key(k)
        {
            self.crop.selected_key = None;
        }
        if let Some(id) = &self.selected_field
            && self.design.field(id).is_none()
        {
            self.selected_field = None;
        }
    }
}

fn strip_ext(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 5 => stem,
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_lookup_and_card_names() {
        let mut p = Project::new();
        p.data = Arc::new(Table::new(vec!["Roll".into(), "Name".into()], vec![vec!["A1.jpg".into(), "Anu".into()], vec!["".into(), "Bo".into()]]));
        p.design.photo_key_column = "Roll".into();
        let mut photos = BTreeMap::new();
        photos.insert("a1".to_string(), Arc::new(PhotoEntry::new("A1.jpg", ImageSource::None)));
        p.photos = Arc::new(photos);
        assert!(p.photo_for_row(0).is_some());
        assert!(p.photo_for_row(1).is_none());
        assert_eq!(p.card_name(&CardRef::Row(0)), "A1.jpg");
        assert_eq!(p.card_name(&CardRef::Row(1)), "card_2");
        assert_eq!(p.card_count(), 2);
        assert_eq!(p.card_ref(5), None);
        p.card_source = CardSource::CroppedPhotos;
        assert_eq!(p.card_refs(), vec![CardRef::Photo("a1".into())]);
        assert_eq!(p.card_name(&CardRef::Photo("a1".into())), "A1");
    }

    #[test]
    fn clamping_after_data_shrinks() {
        let mut p = Project::new();
        p.preview_index = 9;
        p.crop.selected_key = Some("x".into());
        p.selected_field = Some("f1".into());
        p.clamp_indices();
        assert_eq!(p.preview_index, 0);
        assert!(p.crop.selected_key.is_none() && p.selected_field.is_none());
    }

    #[test]
    fn clone_shares_bulk_data() {
        let mut p = Project::new();
        p.data = Arc::new(Table::new(vec!["A".into()], vec![vec!["1".into()]; 1000]));
        let q = p.clone();
        assert!(Arc::ptr_eq(&p.data, &q.data));
    }
}

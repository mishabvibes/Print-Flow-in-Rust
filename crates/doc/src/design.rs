//! The card design: template, fields and how fields map to data columns.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::field::{Field, FieldKind};
use crate::source::ImageSource;
use crate::table::Table;
use crate::util::normalize_key;

/// The card template image and its pixel size (the size cards are rendered at).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub src: ImageSource,
    pub w: u32,
    pub h: u32,
}

/// Template + fields + data mapping. Cheap to clone (the template bytes are shared).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Design {
    pub template: Option<Template>,
    pub fields: Vec<Field>,
    /// field id -> column header.
    pub mapping: BTreeMap<String, String>,
    /// The column whose value names each row's photo (and the exported card).
    pub photo_key_column: String,
}

impl Design {
    pub fn field(&self, id: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.id == id)
    }
    pub fn field_mut(&mut self, id: &str) -> Option<&mut Field> {
        self.fields.iter_mut().find(|f| f.id == id)
    }

    /// The next free `fN` id. Computed from the existing ids, so loading a project and adding fields
    /// never collides (the original used a counter that restarted at 1 after a load).
    pub fn next_id(&self) -> String {
        let max = self.fields.iter().filter_map(|f| f.id.strip_prefix('f').and_then(|n| n.parse::<u64>().ok())).max().unwrap_or(0);
        format!("f{}", max + 1)
    }

    pub fn add_text_field(&mut self) -> String {
        let id = self.next_id();
        self.fields.push(Field::new_text(id.clone()));
        id
    }
    pub fn add_photo_field(&mut self) -> String {
        let id = self.next_id();
        self.fields.push(Field::new_photo(id.clone()));
        id
    }
    pub fn remove_field(&mut self, id: &str) -> bool {
        let n = self.fields.len();
        self.fields.retain(|f| f.id != id);
        self.mapping.remove(id);
        self.fields.len() != n
    }

    /// The pixel aspect (w/h) the first photo field crops to; a passport photo when there is none.
    ///
    /// Field geometry is a percentage of the template, so the real aspect also needs the template's
    /// own aspect (the previous version used the raw percentages, which framed photos wrongly on any
    /// template that is not square).
    pub fn photo_aspect(&self) -> f64 {
        let (tw, th) = self.template.as_ref().map(|t| (f64::from(t.w.max(1)), f64::from(t.h.max(1)))).unwrap_or((1.0, 1.0));
        self.fields.iter().find(|f| f.is_photo() && f.w > 0.0 && f.h > 0.0).map(|f| (f.w * tw) / (f.h * th)).unwrap_or(crate::crop::PASSPORT_ASPECT)
    }

    /// Maps every unmapped field whose name equals a header (case-insensitive, trimmed).
    pub fn auto_map(&mut self, headers: &[String]) {
        for f in &self.fields {
            if let Some(existing) = self.mapping.get(&f.id)
                && headers.contains(existing)
            {
                continue;
            }
            let name = f.name.trim().to_lowercase();
            if let Some(h) = headers.iter().find(|h| h.trim().to_lowercase() == name) {
                self.mapping.insert(f.id.clone(), h.clone());
            }
        }
    }

    /// Creates a text field for every header that has no field yet, cascading them down the card.
    /// Returns how many were created.
    pub fn auto_create_fields(&mut self, headers: &[String]) -> usize {
        let mut cascade = self.fields.iter().filter(|f| f.is_text()).count();
        let mut created = 0;
        for h in headers {
            let clean = h.trim();
            if clean.is_empty() {
                continue;
            }
            let has = self.fields.iter().any(|f| f.name.trim().eq_ignore_ascii_case(clean)) || self.mapping.values().any(|v| v == h);
            if has {
                continue;
            }
            let id = self.next_id();
            let mut f = Field::new_text(id);
            f.name = clean.to_string();
            f.y = 10.0 + cascade as f64 * 8.0;
            if f.y > 85.0 {
                f.y = 10.0 + (cascade % 10) as f64 * 8.0;
            }
            self.fields.push(f);
            cascade += 1;
            created += 1;
        }
        created
    }

    /// The text a text field shows for `row`: the mapped column's value, or empty.
    pub fn text_for(&self, table: &Table, row: usize, field: &Field) -> String {
        self.mapping.get(&field.id).and_then(|col| table.cell(row, col)).unwrap_or("").to_string()
    }

    /// What the preview shows: the value, or `{{Name}}` when unmapped / no data.
    pub fn preview_text_for(&self, table: &Table, row: usize, field: &Field) -> String {
        match self.mapping.get(&field.id).and_then(|col| table.cell(row, col)) {
            Some(v) => v.to_string(),
            None => format!("{{{{{}}}}}", field.name),
        }
    }

    /// The normalised photo key of `row` (empty when no key column is chosen).
    pub fn photo_key(&self, table: &Table, row: usize) -> String {
        if self.photo_key_column.is_empty() {
            return String::new();
        }
        table.cell(row, &self.photo_key_column).map(normalize_key).unwrap_or_default()
    }

    /// A good default key column: the first header that looks like an id, else the first header.
    pub fn guess_key_column(headers: &[String]) -> Option<String> {
        let looks_like_id = |h: &str| {
            let l = h.to_lowercase();
            ["admission", "roll", "id", "reg"].iter().any(|k| l.contains(k))
        };
        headers.iter().find(|h| looks_like_id(h)).or_else(|| headers.first()).cloned()
    }

    pub fn photo_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter().filter(|f| matches!(f.kind, FieldKind::Photo(_)))
    }
    pub fn text_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter().filter(|f| matches!(f.kind, FieldKind::Text(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(h: &[&str]) -> Vec<String> {
        h.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn ids_never_collide_after_load() {
        let mut d = Design::default();
        d.fields.push(Field::new_text("f7".into()));
        assert_eq!(d.add_text_field(), "f8");
        assert_eq!(d.add_photo_field(), "f9");
        d.remove_field("f8");
        assert_eq!(d.next_id(), "f10");
    }

    #[test]
    fn auto_create_and_map() {
        let mut d = Design::default();
        let h = headers(&["Name", " Class ", "", "name"]);
        assert_eq!(d.auto_create_fields(&h), 2); // "name" duplicates "Name"
        assert_eq!(d.fields[1].name, "Class");
        d.auto_map(&h);
        assert_eq!(d.mapping.len(), 2);
        assert_eq!(d.mapping[&d.fields[0].id], "Name");
        // running again creates nothing
        assert_eq!(d.auto_create_fields(&h), 0);
    }

    #[test]
    fn cascade_wraps_after_85() {
        let mut d = Design::default();
        let hs: Vec<String> = (0..14).map(|i| format!("H{i}")).collect();
        d.auto_create_fields(&hs);
        assert!(d.fields.iter().all(|f| f.y <= 85.0));
    }

    #[test]
    fn preview_and_export_text() {
        let mut d = Design::default();
        d.add_text_field();
        d.fields[0].name = "Name".into();
        let t = Table::new(vec!["Name".into()], vec![vec!["Anu".into()]]);
        d.auto_map(&t.headers);
        assert_eq!(d.text_for(&t, 0, &d.fields[0]), "Anu");
        assert_eq!(d.preview_text_for(&Table::default(), 0, &d.fields[0]), "{{Name}}");
        assert_eq!(d.text_for(&Table::default(), 0, &d.fields[0]), "");
    }

    #[test]
    fn key_column_guess() {
        assert_eq!(Design::guess_key_column(&headers(&["Name", "Admission No"])), Some("Admission No".into()));
        assert_eq!(Design::guess_key_column(&headers(&["Name", "Class"])), Some("Name".into()));
        assert_eq!(Design::guess_key_column(&[]), None);
    }

    #[test]
    fn photo_key_normalises() {
        let d = Design { photo_key_column: "Roll".into(), ..Design::default() };
        let t = Table::new(vec!["Roll".into()], vec![vec![" A12.JPG".into()]]);
        assert_eq!(d.photo_key(&t, 0), "a12");
        assert_eq!(d.photo_key(&t, 5), "");
    }
}

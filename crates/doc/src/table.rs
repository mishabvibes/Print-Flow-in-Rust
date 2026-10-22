//! The merged data: a header row and string cells. Serialises like the original app
//! (`{ headers: [...], rows: [ { header: value, ... } ] }`) so old projects load.

use serde::de::{self, Deserializer};
use serde::ser::{SerializeMap, SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self { headers, rows }
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn column_index(&self, header: &str) -> Option<usize> {
        self.headers.iter().position(|h| h == header)
    }
    /// The text of `column` in row `row` (`None` when the row or column doesn't exist).
    pub fn cell(&self, row: usize, column: &str) -> Option<&str> {
        let c = self.column_index(column)?;
        self.rows.get(row)?.get(c).map(String::as_str)
    }
}

/// How a JSON cell value reads as text: numbers without a trailing `.0`, `null` as empty.
fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

#[derive(Serialize, Deserialize, Default)]
struct Repr {
    #[serde(default)]
    headers: Vec<String>,
    #[serde(default)]
    rows: Vec<serde_json::Map<String, Value>>,
}

impl Serialize for Table {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        struct Rows<'a>(&'a Table);
        struct Row<'a>(&'a Table, &'a [String]);
        impl Serialize for Row<'_> {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut m = s.serialize_map(Some(self.0.headers.len()))?;
                for (i, h) in self.0.headers.iter().enumerate() {
                    m.serialize_entry(h, self.1.get(i).map(String::as_str).unwrap_or(""))?;
                }
                m.end()
            }
        }
        impl Serialize for Rows<'_> {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut seq = s.serialize_seq(Some(self.0.rows.len()))?;
                for r in &self.0.rows {
                    seq.serialize_element(&Row(self.0, r))?;
                }
                seq.end()
            }
        }
        let mut m = s.serialize_map(Some(2))?;
        m.serialize_entry("headers", &self.headers)?;
        m.serialize_entry("rows", &Rows(self))?;
        m.end()
    }
}

impl<'de> Deserialize<'de> for Table {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let repr = Repr::deserialize(d).map_err(de::Error::custom)?;
        let mut headers = repr.headers;
        if headers.is_empty()
            && let Some(first) = repr.rows.first()
        {
            headers = first.keys().cloned().collect();
        }
        let rows = repr.rows.iter().map(|r| headers.iter().map(|h| r.get(h).map(value_text).unwrap_or_default()).collect()).collect();
        Ok(Table { headers, rows })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_json() {
        let t: Table = serde_json::from_str(r#"{"headers":["Name","Roll"],"rows":[{"Name":"Anu","Roll":12},{"Name":"Bo","Roll":"13"}]}"#).unwrap();
        assert_eq!(t.cell(0, "Roll"), Some("12"));
        assert_eq!(t.cell(1, "Name"), Some("Bo"));
        assert_eq!(t.cell(2, "Name"), None);
        let back: Table = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn headers_from_first_row() {
        let t: Table = serde_json::from_str(r#"{"rows":[{"A":1,"B":2}]}"#).unwrap();
        assert_eq!(t.headers, vec!["A", "B"]);
    }
}

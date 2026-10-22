#![forbid(unsafe_code)]
//! Spreadsheet import (layer 1): CSV and Excel files into a [`Table`] of text cells.
//!
//! Dates become `dd-mm-yyyy` (never an Excel serial number such as `45123`); whole numbers lose
//! their `.0`; headers are trimmed, never empty and unique.

use std::collections::HashSet;
use std::io::Cursor;
use std::path::Path;

use calamine::{Data, Reader};
use forge_doc::Table;

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("cannot read {path}: {source}")]
    Io { path: String, source: std::io::Error },
    #[error("cannot parse spreadsheet: {0}")]
    Parse(String),
    #[error("the file has no data rows")]
    Empty,
    #[error("unsupported file type: .{0}")]
    Unsupported(String),
}

/// Maximum rows / columns read, so a hostile or corrupt file can't exhaust memory.
pub const MAX_ROWS: usize = 500_000;
pub const MAX_COLS: usize = 512;

/// Extensions the importer understands.
pub const EXTENSIONS: &[&str] = &["csv", "tsv", "txt", "xlsx", "xlsm", "xlsb", "xls", "ods"];

/// Sheet names of a workbook (`["Sheet1", ...]`); a CSV has none.
pub fn sheet_names(path: &Path) -> Result<Vec<String>, DataError> {
    if is_text_ext(path) {
        return Ok(vec![]);
    }
    let wb = calamine::open_workbook_auto(path).map_err(|e| DataError::Parse(e.to_string()))?;
    Ok(wb.sheet_names())
}

/// Reads the first (or `sheet`-th) sheet of a workbook, or a CSV/TSV file, into a table.
pub fn read_table(path: &Path, sheet: Option<usize>) -> Result<Table, DataError> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !EXTENSIONS.contains(&ext.as_str()) {
        return Err(DataError::Unsupported(ext));
    }
    if is_text_ext(path) {
        let bytes = std::fs::read(path).map_err(|source| DataError::Io { path: path.display().to_string(), source })?;
        return parse_csv(&bytes);
    }
    let mut wb = calamine::open_workbook_auto(path).map_err(|e| DataError::Parse(e.to_string()))?;
    let range = match wb.worksheet_range_at(sheet.unwrap_or(0)) {
        Some(Ok(r)) => r,
        Some(Err(e)) => return Err(DataError::Parse(e.to_string())),
        None => return Err(DataError::Empty),
    };
    let rows: Vec<Vec<String>> = range.rows().take(MAX_ROWS + 1).map(|r| r.iter().take(MAX_COLS).map(cell_text).collect()).collect();
    table_from_rows(rows)
}

fn is_text_ext(path: &Path) -> bool {
    matches!(path.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref(), Some("csv" | "tsv" | "txt"))
}

/// Builds a table from raw rows (the first row is the header row).
pub fn table_from_rows(mut rows: Vec<Vec<String>>) -> Result<Table, DataError> {
    // Skip leading fully-empty rows, then take the header row.
    let first = rows.iter().position(|r| r.iter().any(|c| !c.trim().is_empty())).ok_or(DataError::Empty)?;
    rows.drain(..first);
    let mut it = rows.into_iter();
    let raw_headers = it.next().ok_or(DataError::Empty)?;
    // Trailing empty header cells with no data under them are just formatting.
    let width = raw_headers.len();
    let headers = clean_headers(raw_headers);
    let mut body: Vec<Vec<String>> = Vec::new();
    for mut r in it {
        if r.iter().all(|c| c.trim().is_empty()) {
            continue; // blank rows are skipped
        }
        r.resize(width.max(headers.len()), String::new());
        r.truncate(headers.len());
        body.push(r);
    }
    if body.is_empty() {
        return Err(DataError::Empty);
    }
    Ok(Table::new(headers, body))
}

/// Trims headers, names empty ones `Column N` and makes duplicates unique (`Name`, `Name_1`, ...).
pub fn clean_headers(raw: Vec<String>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::with_capacity(raw.len());
    for (i, h) in raw.into_iter().enumerate() {
        let mut name = h.trim().to_string();
        if name.is_empty() {
            name = format!("Column {}", i + 1);
        }
        let base = name.clone();
        let mut n = 0;
        while seen.contains(&name.to_lowercase()) {
            n += 1;
            name = format!("{base}_{n}");
        }
        seen.insert(name.to_lowercase());
        out.push(name);
    }
    out
}

// ---- CSV ---------------------------------------------------------------------------------------

/// Parses CSV/TSV bytes: UTF-8 (BOM allowed) or, failing that, Windows-1252 (what Excel's "CSV"
/// export writes on Windows); the delimiter (`,` `;` tab) is detected from the first line.
pub fn parse_csv(bytes: &[u8]) -> Result<Table, DataError> {
    let text = decode_text(bytes);
    let delim = detect_delimiter(&text);
    let mut rdr = csv::ReaderBuilder::new().has_headers(false).flexible(true).delimiter(delim).from_reader(Cursor::new(text.as_bytes()));
    let mut rows = Vec::new();
    for rec in rdr.records().take(MAX_ROWS + 1) {
        let rec = rec.map_err(|e| DataError::Parse(e.to_string()))?;
        rows.push(rec.iter().take(MAX_COLS).map(|c| c.trim_end_matches('\r').to_string()).collect());
    }
    table_from_rows(rows)
}

fn decode_text(bytes: &[u8]) -> String {
    let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| cp1252(c)).collect(),
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—',
        '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH.get((b - 0x80) as usize).copied().unwrap_or('\u{FFFD}'),
        _ => char::from(b),
    }
}

fn detect_delimiter(text: &str) -> u8 {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    // Count outside quotes.
    let (mut comma, mut semi, mut tab, mut inq) = (0, 0, 0, false);
    for c in line.chars() {
        match c {
            '"' => inq = !inq,
            ',' if !inq => comma += 1,
            ';' if !inq => semi += 1,
            '\t' if !inq => tab += 1,
            _ => {}
        }
    }
    if tab > comma && tab >= semi {
        b'\t'
    } else if semi > comma {
        b';'
    } else {
        b','
    }
}

// ---- Excel cells ---------------------------------------------------------------------------------

fn cell_text(d: &Data) -> String {
    match d {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => number_text(*f),
        Data::DateTime(dt) => {
            if dt.is_duration() {
                duration_text(dt.as_f64())
            } else {
                serial_to_text(dt.as_f64())
            }
        }
        Data::DateTimeIso(s) => iso_date_text(s),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("{e}"),
    }
}

/// `5` not `5.0`; no exponent for whole numbers up to 1e15 (phone numbers, ids); otherwise the
/// shortest text that reads back as the same number.
pub fn number_text(f: f64) -> String {
    if !f.is_finite() {
        return String::new();
    }
    if f.fract() == 0.0 && f.abs() < 1e15 {
        return format!("{}", f as i64);
    }
    format!("{f}")
}

fn duration_text(days: f64) -> String {
    let total_min = (days * 24.0 * 60.0).round() as i64;
    format!("{:02}:{:02}", total_min / 60, total_min % 60)
}

/// An Excel serial date as `dd-mm-yyyy` (or `HH:MM` for a time of day).
pub fn serial_to_text(serial: f64) -> String {
    if !serial.is_finite() || !(0.0..=2_958_465.0).contains(&serial) {
        return number_text(serial);
    }
    if serial < 1.0 {
        let mins = (serial * 24.0 * 60.0).round() as i64;
        return format!("{:02}:{:02}", mins / 60 % 24, mins % 60);
    }
    // Excel's 1900 leap-year bug: serial 60 is the non-existent 1900-02-29; dates after it are
    // offset by one extra day. 25569 = days from 1899-12-30 to 1970-01-01.
    let days = serial.floor() as i64;
    let unix_days = if days >= 61 { days - 25569 } else { days - 25568 };
    let (y, m, d) = civil_from_days(unix_days);
    format!("{d:02}-{m:02}-{y:04}")
}

fn iso_date_text(s: &str) -> String {
    // `2024-05-17` or `2024-05-17T10:30:00` -> `17-05-2024`
    let date = s.split('T').next().unwrap_or(s);
    let parts: Vec<&str> = date.split('-').collect();
    match parts.as_slice() {
        [y, m, d] if y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()) => format!("{d}-{m}-{y}"),
        _ => s.to_string(),
    }
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A sample CSV the user can start from (header names match the default auto-created fields).
pub const SAMPLE_CSV: &str = "Name,Class,AdmissionNo,DOB,Phone\nAnu Raj,10-A,1001,12-05-2009,9876543210\nBibin Thomas,10-B,1002,03-11-2008,9876501234\nChitra Menon,9-A,1003,21-01-2010,9812345678\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excel_serials() {
        assert_eq!(serial_to_text(45_000.0), "15-03-2023"); // 2023-03-15
        assert_eq!(serial_to_text(36_526.0), "01-01-2000");
        assert_eq!(serial_to_text(61.0), "01-03-1900");
        assert_eq!(serial_to_text(1.0), "01-01-1900");
        assert_eq!(serial_to_text(0.5), "12:00");
        assert_eq!(serial_to_text(45_000.75), "15-03-2023");
        assert_eq!(serial_to_text(-5.0), "-5");
    }

    #[test]
    fn numbers() {
        assert_eq!(number_text(5.0), "5");
        assert_eq!(number_text(9_876_543_210.0), "9876543210");
        assert_eq!(number_text(12.5), "12.5");
        assert_eq!(number_text(f64::NAN), "");
        assert_eq!(number_text(-0.0), "0");
    }

    #[test]
    fn csv_basic_and_blank_rows() {
        let t = parse_csv(b"Name,Roll\nAnu,12\n,\nBo,13\n").unwrap();
        assert_eq!(t.headers, vec!["Name", "Roll"]);
        assert_eq!(t.len(), 2);
        assert_eq!(t.cell(1, "Name"), Some("Bo"));
    }

    #[test]
    fn csv_bom_semicolons_quotes() {
        let t = parse_csv("\u{FEFF}Name;City\n\"Raju; K\";\"Kochi\"\n".as_bytes()).unwrap();
        assert_eq!(t.headers, vec!["Name", "City"]);
        assert_eq!(t.cell(0, "Name"), Some("Raju; K"));
    }

    #[test]
    fn csv_keeps_dates_and_leading_zeros_as_typed() {
        let t = parse_csv(b"Dob,Phone\n05/12/2008,007\n").unwrap();
        assert_eq!(t.cell(0, "Dob"), Some("05/12/2008"));
        assert_eq!(t.cell(0, "Phone"), Some("007"));
    }

    #[test]
    fn csv_windows_1252() {
        let t = parse_csv(b"Name\nJos\xE9\n").unwrap();
        assert_eq!(t.cell(0, "Name"), Some("José"));
    }

    #[test]
    fn csv_ragged_rows() {
        let t = parse_csv(b"A,B,C\n1\n1,2,3,4\n").unwrap();
        assert_eq!(t.rows[0], vec!["1", "", ""]);
        assert_eq!(t.rows[1], vec!["1", "2", "3"]);
    }

    #[test]
    fn headers_cleaned() {
        assert_eq!(clean_headers(vec![" Name ".into(), "".into(), "name".into(), "Name".into()]), vec!["Name", "Column 2", "name_1", "Name_2"]);
    }

    #[test]
    fn empty_inputs() {
        assert!(matches!(parse_csv(b""), Err(DataError::Empty)));
        assert!(matches!(parse_csv(b"A,B\n"), Err(DataError::Empty)));
        assert!(matches!(parse_csv(b"\n\n,\n"), Err(DataError::Empty)));
    }

    #[test]
    fn iso_dates() {
        assert_eq!(iso_date_text("2024-05-17T10:30:00"), "17-05-2024");
        assert_eq!(iso_date_text("garbage"), "garbage");
    }

    #[test]
    fn unsupported_extension() {
        assert!(matches!(read_table(Path::new("a.exe"), None), Err(DataError::Unsupported(_))));
    }
}

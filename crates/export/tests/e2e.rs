#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::io::Read;
use std::sync::Arc;

use forge_export::*;
use forge_render::{Renderer, sample, sheet};
use forge_text::FontDb;

fn renderer() -> Renderer {
    Renderer::new(Arc::new(FontDb::new()))
}

fn zip_names(path: &std::path::Path) -> Vec<String> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect()
}

#[test]
fn exports_everything() {
    let dir = std::env::temp_dir().join(format!("forge-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let r = renderer();
    let mut p = sample::project();

    let cards = dir.join("cards.zip");
    let rep = export_cards_zip(&r, &p, &cards, &Control::new()).unwrap();
    assert_eq!((rep.written, rep.failed.len()), (6, 0));
    let names = zip_names(&cards);
    assert_eq!(names.len(), 6);
    assert!(names.contains(&"a001.png".to_string()));
    // PNG signature.
    let mut z = zip::ZipArchive::new(std::fs::File::open(&cards).unwrap()).unwrap();
    let mut head = [0u8; 4];
    z.by_name("a001.png").unwrap().read_exact(&mut head).unwrap();
    assert_eq!(&head, b"\x89PNG");

    let cropped = dir.join("crop.zip");
    let rep = export_cropped_zip(&r, &p, &cropped, 600, 92, &Control::new()).unwrap();
    assert_eq!(rep.written, 6);
    assert!(zip_names(&cropped).iter().all(|n| n.ends_with(".jpg")));

    p.sheet.card.w = 8.6;
    p.sheet.card.h = 5.4;
    let plan = sheet::plan(&p.sheet, 150).unwrap();
    let sheets = dir.join("sheets.zip");
    assert_eq!(export_sheets_zip(&r, &p, &sheets, &plan, &Control::new()).unwrap().written, plan.sheets_for(6));

    p.sheet.grid_border.enabled = true;
    let pdf = dir.join("sheets.pdf");
    let rep = export_sheets_pdf(&r, &p, &pdf, &plan, Some(90), &Control::new()).unwrap();
    assert!(rep.failed.is_empty());
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.4") && bytes.ends_with(b"%%EOF\n"));
    let lossless = dir.join("lossless.pdf");
    export_sheets_pdf(&r, &p, &lossless, &plan, None, &Control::new()).unwrap();

    if let Ok(o) = std::env::var("FORGE_OUT") {
        std::fs::copy(&pdf, format!("{o}/sheets.pdf")).unwrap();
        std::fs::copy(&lossless, format!("{o}/lossless.pdf")).unwrap();
    }
    // Cancel leaves no file behind.
    let ctl = Control::new();
    ctl.cancel();
    let out = dir.join("never.zip");
    assert!(matches!(export_cards_zip(&r, &p, &out, &ctl), Err(ExportError::Cancelled)));
    assert!(!out.exists());
    assert!(!dir.join("never.zip.part").exists());
    std::fs::remove_dir_all(&dir).ok();
}

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! End to end: template + CSV rows + real face photos -> card PNG and a print sheet.

use std::sync::Arc;

use forge_doc::*;
use forge_image::Image;
use forge_render::{RenderOpts, Renderer, sheet};
use forge_text::FontDb;

/// Optional folder of real face photos (set `FORGE_TEST_FACES`); the tests that need it skip when unset.
fn faces_dir() -> String {
    std::env::var("FORGE_TEST_FACES").unwrap_or_default()
}

fn template(w: u32, h: u32) -> Image {
    let mut img = Image::filled(w, h, [30, 70, 140, 255]).unwrap();
    // A transparent window where the photo shows through.
    let (x0, y0, x1, y1) = ((w as f64 * 0.06) as u32, (h as f64 * 0.14) as u32, (w as f64 * 0.36) as u32, (h as f64 * 0.74) as u32);
    for y in y0..y1 {
        for x in x0..x1 {
            let o = ((y * w + x) * 4) as usize;
            img.rgba[o + 3] = 0;
        }
    }
    img
}

fn project() -> Project {
    let mut p = Project::new();
    let tpl = template(1000, 630);
    p.design.template = Some(Template { src: ImageSource::from_bytes(forge_image::encode_png(&tpl, true).unwrap()), w: 1000, h: 630 });
    let mut photo = Field::new_photo("p1".into());
    (photo.x, photo.y, photo.w, photo.h) = (6.0, 14.0, 30.0, 60.0);
    let mut name = Field::new_text("t1".into());
    (name.x, name.y, name.w, name.h) = (40.0, 16.0, 56.0, 12.0);
    if let Some(t) = name.text_mut() {
        t.font_family = "Stem Bold".into();
        t.font_size = 52.0;
        t.color = forge_color::Rgba::WHITE;
        t.overflow = Overflow::Shrink;
    }
    let mut addr = Field::new_text("t2".into());
    (addr.x, addr.y, addr.w, addr.h) = (40.0, 34.0, 56.0, 30.0);
    if let Some(t) = addr.text_mut() {
        t.font_family = "Stem".into();
        t.font_size = 30.0;
        t.color = forge_color::Rgba::WHITE;
        t.wrap = Wrap::Multi;
        t.v_align = VAlign::Top;
        t.align = Align::Justify;
    }
    let mut ml = Field::new_text("t3".into());
    (ml.x, ml.y, ml.w, ml.h) = (40.0, 70.0, 56.0, 14.0);
    if let Some(t) = ml.text_mut() {
        t.font_family = "Stem".into();
        t.font_size = 40.0;
        t.color = forge_color::Rgba::rgb(255, 220, 120);
    }
    p.design.fields = vec![photo, name, addr, ml];
    p.data = Arc::new(Table::new(
        vec!["Name".into(), "Roll".into(), "Address".into(), "Local".into()],
        vec![
            vec![
                "Barack Obama".into(),
                "obama".into(),
                "1600 Pennsylvania Avenue NW, Washington, DC 20500, United States of America".into(),
                "മലയാളം ഭാഷ".into(),
            ],
            vec!["Joe Biden with a very long name that must shrink to fit".into(), "biden".into(), "Delaware".into(), "കേരളം".into()],
        ],
    ));
    p.design.mapping = [("t1", "Name"), ("t2", "Address"), ("t3", "Local")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    p.design.photo_key_column = "Roll".into();
    p
}

#[test]
fn renders_cards_and_sheet() {
    let Ok(rd) = std::fs::read_dir(faces_dir()) else { return };
    let mut p = project();
    let mut photos = std::collections::BTreeMap::new();
    for e in rd.flatten() {
        let path = e.path();
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(img) = forge_image::decode(&bytes) else { continue };
        let key = path.file_stem().unwrap().to_string_lossy().to_string();
        let mut entry = PhotoEntry::new(format!("{key}.jpg"), ImageSource::from_bytes(bytes));
        entry.natural_w = img.width;
        entry.natural_h = img.height;
        entry.raw_detection = forge_vision::detect_face(&img);
        entry.faces_detected = entry.raw_detection.is_some();
        entry.recompute_auto_crop(p.design.photo_aspect());
        photos.insert(key, Arc::new(entry));
    }
    p.photos = Arc::new(photos);

    let db = Arc::new(FontDb::new());
    db.scan_system_fonts();
    let r = Renderer::new(db);
    let t = std::time::Instant::now();
    let a = r.render_row(&p, 0, RenderOpts::PREVIEW).expect("render");
    eprintln!("first card {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let b = r.render_row(&p, 1, RenderOpts::PREVIEW).expect("render");
    eprintln!("second card {:?}", t.elapsed());
    assert_eq!((a.width, a.height), (1000, 630));
    // The photo window shows face pixels, not the template colour.
    let px = a.get(200, 250).unwrap();
    assert_ne!(px, [30, 70, 140, 255]);
    assert_eq!(a.get(990, 620).unwrap(), [30, 70, 140, 255]);

    let plan = sheet::plan(&p.sheet, 150).unwrap();
    let out = sheet::render_sheet(&r, &p, &plan, 0, RenderOpts::EXPORT);
    assert!(out.errors.is_empty(), "{:?}", out.errors);

    if let Ok(dir) = std::env::var("FORGE_OUT") {
        std::fs::write(format!("{dir}/card0.png"), forge_image::encode_png(&a, true).unwrap()).unwrap();
        std::fs::write(format!("{dir}/card1.png"), forge_image::encode_png(&b, true).unwrap()).unwrap();
        std::fs::write(format!("{dir}/sheet.png"), forge_image::encode_png(&out.image, true).unwrap()).unwrap();
    }
}

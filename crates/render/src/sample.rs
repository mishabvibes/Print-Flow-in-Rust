//! A self-contained demo project (generated template, fields, rows and synthetic portraits): used by
//! "New demo project" in the app, the CLI's `demo` command and the tests, so nothing needs files on
//! disk to try the whole pipeline.

use std::collections::BTreeMap;
use std::sync::Arc;

use forge_color::Rgba;
use forge_doc::*;
use forge_image::{Image, encode_png};

/// A 1000 x 630 card template with a transparent photo window.
pub fn template_image() -> Image {
    let (w, h) = (1000u32, 630u32);
    let mut img = Image { width: w, height: h, rgba: vec![255; (w * h * 4) as usize] };
    for y in 0..h {
        let t = y as f64 / h as f64;
        let c = [(24.0 + 20.0 * t) as u8, (60.0 + 30.0 * t) as u8, (130.0 + 40.0 * t) as u8];
        for x in 0..w {
            let o = ((y * w + x) * 4) as usize;
            img.rgba[o..o + 3].copy_from_slice(&c);
        }
    }
    for y in 88..466 {
        for x in 60..360 {
            img.rgba[((y * w + x) * 4 + 3) as usize] = 0;
        }
    }
    img
}

/// A synthetic portrait (flat background, shoulders, head): enough for the pipeline, no real person.
pub fn portrait(seed: u32) -> Image {
    let (w, h) = (600u32, 800u32);
    let bg = [[200u8, 210, 225], [225, 205, 200], [205, 225, 205]][(seed % 3) as usize];
    let skin = [[224u8, 172, 140], [198, 134, 96], [141, 85, 60]][(seed % 3) as usize];
    let mut img = Image { width: w, height: h, rgba: vec![255; (w * h * 4) as usize] };
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (f64::from(x), f64::from(y));
            let head = ((fx - 300.0) / 110.0).powi(2) + ((fy - 300.0) / 140.0).powi(2) <= 1.0;
            let body = ((fx - 300.0) / 260.0).powi(2) + ((fy - 820.0) / 330.0).powi(2) <= 1.0;
            let neck = (fx - 300.0).abs() < 45.0 && fy > 380.0 && fy < 520.0;
            let c = if head || neck {
                skin
            } else if body {
                [40, 50, 90]
            } else {
                bg
            };
            let o = ((y * w + x) * 4) as usize;
            img.rgba[o..o + 3].copy_from_slice(&c);
        }
    }
    img
}

/// The demo project: 6 rows, a photo field and three text fields (one Malayalam).
pub fn project() -> Project {
    let mut p = Project::new();
    let tpl = template_image();
    p.design.template = Some(Template { src: ImageSource::from_bytes(encode_png(&tpl, true).unwrap_or_default()), w: tpl.width, h: tpl.height });
    let mut photo = Field::new_photo("p1".into());
    (photo.x, photo.y, photo.w, photo.h) = (6.0, 14.0, 30.0, 60.0);
    photo.name = "Photo".into();
    let mut name = Field::new_text("t1".into());
    (name.x, name.y, name.w, name.h, name.name) = (40.0, 16.0, 56.0, 12.0, "Name".into());
    if let Some(t) = name.text_mut() {
        t.font_family = "Stem Bold".into();
        t.font_size = 52.0;
        t.color = Rgba::WHITE;
        t.overflow = Overflow::Shrink;
    }
    let mut addr = Field::new_text("t2".into());
    (addr.x, addr.y, addr.w, addr.h, addr.name) = (40.0, 34.0, 56.0, 30.0, "Address".into());
    if let Some(t) = addr.text_mut() {
        t.font_family = "Stem".into();
        t.font_size = 30.0;
        t.color = Rgba::WHITE;
        t.wrap = Wrap::Multi;
        t.v_align = VAlign::Top;
    }
    let mut ml = Field::new_text("t3".into());
    (ml.x, ml.y, ml.w, ml.h, ml.name) = (40.0, 70.0, 56.0, 14.0, "Local".into());
    if let Some(t) = ml.text_mut() {
        t.font_family = "Stem".into();
        t.font_size = 40.0;
        t.color = Rgba::rgb(255, 220, 120);
    }
    p.design.fields = vec![photo, name, addr, ml];

    let rows = [
        ("Asha Menon", "a001", "12 Lake Road, Kochi, Kerala 682001", "ആശ മേനോൻ"),
        ("Ravi Kumar", "a002", "4 Park Street, Chennai", "രവി കുമാർ"),
        ("Fatima Noor", "a003", "88 Hill View Apartments, Bengaluru, Karnataka 560001", "ഫാത്തിമ"),
        ("John Thomas Kuruvilla Mathew", "a004", "Kottayam", "ജോൺ തോമസ്"),
        ("Priya Nair", "a005", "Thiruvananthapuram", "പ്രിയ"),
        ("Arun Das", "a006", "Kozhikode, Kerala", "അരുൺ"),
    ];
    p.data = Arc::new(Table::new(
        vec!["Name".into(), "ID".into(), "Address".into(), "Local".into()],
        rows.iter().map(|r| vec![r.0.into(), r.1.into(), r.2.into(), r.3.into()]).collect(),
    ));
    p.design.mapping = [("t1", "Name"), ("t2", "Address"), ("t3", "Local")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    p.design.photo_key_column = "ID".into();

    let mut photos: BTreeMap<String, Arc<PhotoEntry>> = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        let img = portrait(i as u32);
        let mut e = PhotoEntry::new(format!("{}.png", r.1), ImageSource::from_bytes(encode_png(&img, true).unwrap_or_default()));
        e.natural_w = img.width;
        e.natural_h = img.height;
        photos.insert(r.1.to_string(), Arc::new(e));
    }
    p.photos = Arc::new(photos);
    p
}

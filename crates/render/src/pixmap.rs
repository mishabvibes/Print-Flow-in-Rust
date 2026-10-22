//! Conversions between [`forge_image::Image`] (straight alpha) and vello's premultiplied pixmaps.

use forge_image::Image;
use rayon::prelude::*;
use vello_cpu::Pixmap;
use vello_cpu::color::PremulRgba8;

/// Largest side vello's pixmaps support.
pub const MAX_SIDE: u32 = u16::MAX as u32;

fn premul(c: u8, a: u8) -> u8 {
    ((u16::from(c) * u16::from(a) + 127) / 255) as u8
}

pub fn image_to_pixmap(img: &Image) -> Option<Pixmap> {
    if img.width == 0
        || img.height == 0
        || img.width > MAX_SIDE
        || img.height > MAX_SIDE
        || img.rgba.len() < img.width as usize * img.height as usize * 4
    {
        return None;
    }
    let data: Vec<PremulRgba8> = img
        .rgba
        .par_chunks_exact(4)
        .map(|p| match p {
            [r, g, b, 255] => PremulRgba8 { r: *r, g: *g, b: *b, a: 255 },
            [r, g, b, a] => PremulRgba8 { r: premul(*r, *a), g: premul(*g, *a), b: premul(*b, *a), a: *a },
            _ => PremulRgba8 { r: 0, g: 0, b: 0, a: 0 },
        })
        .collect();
    Some(Pixmap::from_parts(data, img.width as u16, img.height as u16))
}

pub fn pixmap_to_image(pm: &Pixmap) -> Image {
    let (w, h) = (u32::from(pm.width()), u32::from(pm.height()));
    let mut rgba = vec![0u8; w as usize * h as usize * 4];
    rgba.par_chunks_exact_mut(4).zip(pm.data().par_iter()).for_each(|(o, p)| {
        let unp = |c: u8| if p.a == 0 { 0 } else { ((u32::from(c) * 255 + u32::from(p.a) / 2) / u32::from(p.a)).min(255) as u8 };
        o.copy_from_slice(&[unp(p.r), unp(p.g), unp(p.b), p.a]);
    });
    Image { width: w, height: h, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_opaque_and_transparent() {
        let img = Image { width: 2, height: 1, rgba: vec![10, 20, 30, 255, 99, 99, 99, 0] };
        let back = pixmap_to_image(&image_to_pixmap(&img).unwrap());
        assert_eq!(&back.rgba[..4], &[10, 20, 30, 255]);
        assert_eq!(back.rgba[7], 0);
    }

    #[test]
    fn rejects_oversize_and_empty() {
        assert!(image_to_pixmap(&Image { width: 0, height: 0, rgba: vec![] }).is_none());
        assert!(image_to_pixmap(&Image { width: 70_000, height: 1, rgba: vec![] }).is_none());
    }
}

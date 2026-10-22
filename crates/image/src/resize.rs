//! High-quality resizing (SIMD convolution via `fast_image_resize`), with sub-pixel source crops.

use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use forge_geom::Rect;

use crate::image8::Image;
use crate::{ImageError, ImageResult, MAX_PIXELS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// Lanczos3: sharp, for final output.
    High,
    /// Bilinear: fast, for on-screen previews and thumbnails.
    Fast,
}

impl Quality {
    fn alg(self) -> ResizeAlg {
        match self {
            Quality::High => ResizeAlg::Convolution(FilterType::Lanczos3),
            Quality::Fast => ResizeAlg::Convolution(FilterType::Bilinear),
        }
    }
}

thread_local! {
    // A Resizer caches its coefficient tables; one per thread (rayon workers each get theirs).
    static RESIZER: std::cell::RefCell<Resizer> = std::cell::RefCell::new(Resizer::new());
}

/// Resizes the `src_rect` region of `src` (pixels; clipped to the image) to `dw` x `dh`.
/// Transparency is handled correctly (alpha-weighted).
pub fn resize_region(src: &Image, src_rect: Rect, dw: u32, dh: u32, quality: Quality) -> ImageResult<Image> {
    if dw == 0 || dh == 0 {
        return Err(ImageError::Other("zero-sized resize target".into()));
    }
    if u64::from(dw) * u64::from(dh) > MAX_PIXELS {
        return Err(ImageError::TooLarge(u64::from(dw) * u64::from(dh) / 1_000_000));
    }
    let whole = Rect::new(0.0, 0.0, f64::from(src.width), f64::from(src.height));
    let r = src_rect.intersect(&whole).ok_or_else(|| ImageError::Other("crop is outside the image".into()))?;
    if !(r.w.is_finite() && r.h.is_finite() && r.w > 0.0 && r.h > 0.0) {
        return Err(ImageError::Other("empty crop".into()));
    }
    let src_img = FirImage::from_vec_u8(src.width, src.height, src.rgba.clone(), PixelType::U8x4).map_err(|e| ImageError::Other(e.to_string()))?;
    resize_fir(&src_img, r, dw, dh, quality)
}

fn resize_fir(src: &FirImage<'_>, r: Rect, dw: u32, dh: u32, quality: Quality) -> ImageResult<Image> {
    let mut dst = FirImage::new(dw, dh, PixelType::U8x4);
    let opts = ResizeOptions::new().resize_alg(quality.alg()).crop(r.x, r.y, r.w, r.h).use_alpha(true);
    RESIZER.with(|cell| {
        let mut resizer = cell.borrow_mut();
        resizer.resize(src, &mut dst, &opts).map_err(|e| ImageError::Other(e.to_string()))
    })?;
    Image::from_rgba(dw, dh, dst.into_vec())
}

/// A copy of `img` scaled to fit within `max_edge` on its longest side (never enlarged).
pub fn thumbnail(img: &Image, max_edge: u32) -> ImageResult<Image> {
    let long = img.width.max(img.height);
    if long <= max_edge || max_edge == 0 {
        return Ok(img.clone());
    }
    let s = f64::from(max_edge) / f64::from(long);
    let (w, h) = (((f64::from(img.width) * s).round() as u32).max(1), ((f64::from(img.height) * s).round() as u32).max(1));
    resize_region(img, Rect::new(0.0, 0.0, f64::from(img.width), f64::from(img.height)), w, h, Quality::Fast)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 4]) -> Image {
        Image::filled(w, h, c).unwrap()
    }

    #[test]
    fn solid_color_survives_resize() {
        let src = solid(100, 50, [10, 200, 30, 255]);
        let out = resize_region(&src, Rect::new(0.0, 0.0, 100.0, 50.0), 20, 10, Quality::High).unwrap();
        assert_eq!((out.width, out.height), (20, 10));
        assert_eq!(out.get(5, 5), Some([10, 200, 30, 255]));
    }

    #[test]
    fn crop_selects_the_region() {
        // left half red, right half blue
        let mut src = solid(100, 10, [255, 0, 0, 255]);
        for y in 0..10usize {
            for x in 50..100usize {
                let i = (y * 100 + x) * 4;
                src.rgba[i..i + 4].copy_from_slice(&[0, 0, 255, 255]);
            }
        }
        let right = resize_region(&src, Rect::new(60.0, 0.0, 30.0, 10.0), 6, 2, Quality::Fast).unwrap();
        assert_eq!(right.get(3, 1), Some([0, 0, 255, 255]));
    }

    #[test]
    fn crop_is_clipped_and_bad_requests_are_errors() {
        let src = solid(10, 10, [1, 2, 3, 255]);
        assert!(resize_region(&src, Rect::new(-5.0, -5.0, 8.0, 8.0), 4, 4, Quality::Fast).is_ok());
        assert!(resize_region(&src, Rect::new(20.0, 20.0, 5.0, 5.0), 4, 4, Quality::Fast).is_err());
        assert!(resize_region(&src, Rect::new(0.0, 0.0, 10.0, 10.0), 0, 4, Quality::Fast).is_err());
        assert!(resize_region(&src, Rect::new(0.0, 0.0, f64::NAN, 10.0), 4, 4, Quality::Fast).is_err());
    }

    #[test]
    fn transparent_pixels_do_not_bleed_black() {
        let src = solid(8, 8, [255, 255, 255, 0]);
        let out = resize_region(&src, Rect::new(0.0, 0.0, 8.0, 8.0), 2, 2, Quality::High).unwrap();
        assert_eq!(out.get(0, 0).map(|p| p[3]), Some(0));
    }

    #[test]
    fn thumbnails_keep_aspect() {
        let t = thumbnail(&solid(400, 200, [0; 4]), 100).unwrap();
        assert_eq!((t.width, t.height), (100, 50));
        let same = thumbnail(&solid(40, 20, [0; 4]), 100).unwrap();
        assert_eq!((same.width, same.height), (40, 20));
    }
}

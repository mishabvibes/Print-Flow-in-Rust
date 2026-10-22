//! Decoding (EXIF-aware, size-limited) and encoding.

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageReader};

use crate::image8::Image;
use crate::{ImageError, ImageResult, MAX_PIXELS};

fn reader(bytes: &[u8]) -> ImageResult<ImageReader<Cursor<&[u8]>>> {
    ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| ImageError::Decode(e.to_string()))
}

/// Pixel size of an image as it is *displayed* (EXIF rotation applied), without decoding the pixels.
pub fn probe_dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    let mut decoder = reader(bytes)?.into_decoder().map_err(|e| ImageError::Decode(e.to_string()))?;
    let (w, h) = decoder.dimensions();
    let swap = decoder
        .orientation()
        .map(|o| {
            matches!(
                o,
                image::metadata::Orientation::Rotate90
                    | image::metadata::Orientation::Rotate270
                    | image::metadata::Orientation::Rotate90FlipH
                    | image::metadata::Orientation::Rotate270FlipH
            )
        })
        .unwrap_or(false);
    Ok(if swap { (h, w) } else { (w, h) })
}

/// Decodes any supported image to straight RGBA8, applying the EXIF orientation (phones store
/// portrait photos sideways), and refusing absurd sizes before allocating.
pub fn decode(bytes: &[u8]) -> ImageResult<Image> {
    let mut decoder = reader(bytes)?.into_decoder().map_err(|e| ImageError::Decode(e.to_string()))?;
    let (w, h) = decoder.dimensions();
    let px = u64::from(w) * u64::from(h);
    if px == 0 {
        return Err(ImageError::Decode("empty image".into()));
    }
    if px > MAX_PIXELS {
        return Err(ImageError::TooLarge(px / 1_000_000));
    }
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| ImageError::Decode(e.to_string()))?;
    img.apply_orientation(orientation);
    let rgba = img.into_rgba8();
    Image::from_rgba(rgba.width(), rgba.height(), rgba.into_raw())
}

/// PNG. `fast` trades file size for speed (card batches); otherwise default compression.
pub fn encode_png(img: &Image, fast: bool) -> ImageResult<Vec<u8>> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    let mut out = Vec::with_capacity(img.rgba.len() / 3);
    let (comp, filter) = if fast { (CompressionType::Fast, FilterType::Sub) } else { (CompressionType::Default, FilterType::Adaptive) };
    PngEncoder::new_with_quality(&mut out, comp, filter)
        .write_image(&img.rgba, img.width, img.height, image::ExtendedColorType::Rgba8)
        .map_err(|e| ImageError::Encode(e.to_string()))?;
    Ok(out)
}

/// Baseline JPEG, transparency flattened on white.
pub fn encode_jpeg(img: &Image, quality: u8) -> ImageResult<Vec<u8>> {
    let (w, h) = (
        u16::try_from(img.width).map_err(|_| ImageError::Encode("image too wide for JPEG".into()))?,
        u16::try_from(img.height).map_err(|_| ImageError::Encode("image too tall for JPEG".into()))?,
    );
    let mut rgb = Vec::with_capacity(img.pixel_count() * 3);
    for p in img.rgba.chunks_exact(4) {
        let a = u32::from(p[3]);
        for c in &p[..3] {
            rgb.push(((u32::from(*c) * a + 255 * (255 - a) + 127) / 255) as u8);
        }
    }
    let mut out = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut out, quality.clamp(1, 100));
    enc.encode(&rgb, w, h, jpeg_encoder::ColorType::Rgb).map_err(|e| ImageError::Encode(e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> Image {
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x * 255 / w.max(1)) as u8, (y * 255 / h.max(1)) as u8, 128, 255]);
            }
        }
        Image::from_rgba(w, h, rgba).unwrap()
    }

    #[test]
    fn png_round_trip_is_lossless() {
        let img = gradient(37, 23);
        for fast in [true, false] {
            let png = encode_png(&img, fast).unwrap();
            assert_eq!(decode(&png).unwrap(), img);
            assert_eq!(probe_dimensions(&png).unwrap(), (37, 23));
        }
    }

    #[test]
    fn jpeg_round_trip_is_close() {
        let img = gradient(64, 48);
        let jpg = encode_jpeg(&img, 95).unwrap();
        let back = decode(&jpg).unwrap();
        assert_eq!((back.width, back.height), (64, 48));
        let (a, b) = (img.get(10, 10).unwrap(), back.get(10, 10).unwrap());
        for i in 0..3 {
            assert!((i32::from(a[i]) - i32::from(b[i])).abs() < 12, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(decode(b"definitely not an image").is_err());
        assert!(decode(&[]).is_err());
        assert!(probe_dimensions(b"xx").is_err());
    }
}

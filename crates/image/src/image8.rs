use crate::{ImageError, ImageResult, MAX_PIXELS};

/// An 8-bit RGBA image with straight (non-premultiplied) alpha, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// A `width` x `height` image filled with `color`.
    pub fn filled(width: u32, height: u32, color: [u8; 4]) -> ImageResult<Self> {
        let px = u64::from(width) * u64::from(height);
        if px > MAX_PIXELS {
            return Err(ImageError::TooLarge(px / 1_000_000));
        }
        let mut rgba = Vec::with_capacity((px as usize).saturating_mul(4));
        for _ in 0..px {
            rgba.extend_from_slice(&color);
        }
        Ok(Self { width, height, rgba })
    }

    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> ImageResult<Self> {
        if (width as usize).saturating_mul(height as usize).saturating_mul(4) != rgba.len() {
            return Err(ImageError::Other("pixel buffer does not match the image size".into()));
        }
        Ok(Self { width, height, rgba })
    }

    pub fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Any pixel that is not fully opaque?
    pub fn has_alpha(&self) -> bool {
        self.rgba.chunks_exact(4).any(|p| p.get(3).is_some_and(|a| *a != 255))
    }

    /// Composites over an opaque background colour and makes every pixel opaque.
    pub fn flatten_on(&mut self, bg: [u8; 3]) {
        for p in self.rgba.chunks_exact_mut(4) {
            let a = u32::from(p.get(3).copied().unwrap_or(255));
            if a == 255 {
                continue;
            }
            for (c, b) in p.iter_mut().zip(bg) {
                *c = ((u32::from(*c) * a + u32::from(b) * (255 - a) + 127) / 255) as u8;
            }
            if let Some(alpha) = p.get_mut(3) {
                *alpha = 255;
            }
        }
    }

    /// Rec. 601 luma (what the face detector reads).
    pub fn to_gray(&self) -> Vec<u8> {
        self.rgba
            .chunks_exact(4)
            .map(|p| {
                let (r, g, b) = (u32::from(p[0]), u32::from(p[1]), u32::from(p[2]));
                ((r * 299 + g * 587 + b * 114) / 1000) as u8
            })
            .collect()
    }

    pub fn get(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba.get(i..i + 4).and_then(|s| s.try_into().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_blends_over_white() {
        let mut i = Image::from_rgba(1, 1, vec![0, 0, 0, 128]).unwrap();
        assert!(i.has_alpha());
        i.flatten_on([255, 255, 255]);
        assert!(!i.has_alpha());
        assert_eq!(i.get(0, 0), Some([127, 127, 127, 255]));
    }

    #[test]
    fn bad_buffers_are_errors() {
        assert!(Image::from_rgba(2, 2, vec![0; 3]).is_err());
        assert!(Image::filled(100_000, 100_000, [0; 4]).is_err());
    }
}

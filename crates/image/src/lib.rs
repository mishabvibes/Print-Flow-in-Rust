//! Raster images for Forge Studio (layer 1): decoding with EXIF orientation, fast high-quality
//! resizing and encoding. Pure Rust, total functions (errors, never panics), and every allocation
//! is checked against [`MAX_PIXELS`] first.
#![forbid(unsafe_code)]

mod codec;
mod image8;
mod resize;

pub use codec::{decode, encode_jpeg, encode_png, probe_dimensions};
pub use image8::Image;
pub use resize::{Quality, resize_region, thumbnail};

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("cannot decode image: {0}")]
    Decode(String),
    #[error("cannot encode image: {0}")]
    Encode(String),
    #[error("image is too large ({0} megapixels)")]
    TooLarge(u64),
    #[error("{0}")]
    Other(String),
}

pub type ImageResult<T> = Result<T, ImageError>;

/// Largest image decoded or created, in pixels (about 150 megapixels: a 12000 x 12000 scan).
pub const MAX_PIXELS: u64 = 150_000_000;

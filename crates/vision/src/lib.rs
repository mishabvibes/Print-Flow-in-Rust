//! Forge Studio vision (layer 3): face detection and backdrop replacement, all pure Rust and offline.
//!
//! * [`detect_face`] runs BlazeFace (the same model the original app used through MediaPipe/WASM, with
//!   eye key points) on a built-in TFLite interpreter, falling back to SeetaFace if it finds nothing.
//! * [`replace_background`] swaps a plain studio backdrop for a solid colour.
#![forbid(unsafe_code)]

pub mod bg;
pub mod blaze;
mod flat;
mod seeta;
pub mod tflite;

use std::cell::RefCell;

use forge_doc::FaceDetection;
use forge_image::Image;

pub use bg::{BgOptions, replace_background};
pub use blaze::BlazeFace;

thread_local! {
    static BLAZE: RefCell<Option<Option<BlazeFace>>> = const { RefCell::new(None) };
    static SEETA: RefCell<Option<Option<seeta::SeetaDetector>>> = const { RefCell::new(None) };
}

/// The main face of a photo, in pixel coordinates, or `None`. Models are created once per thread, so
/// this is safe and cheap to call from rayon workers.
pub fn detect_face(img: &Image) -> Option<FaceDetection> {
    let blaze = BLAZE.with(|c| {
        let mut slot = c.borrow_mut();
        let m = slot.get_or_insert_with(BlazeFace::new);
        m.as_ref().and_then(|m| m.detect(img).into_iter().next())
    });
    if blaze.is_some() {
        return blaze;
    }
    SEETA.with(|c| {
        let mut slot = c.borrow_mut();
        let m = slot.get_or_insert_with(seeta::SeetaDetector::new);
        m.as_mut().and_then(|m| m.detect(img))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_images_have_no_face() {
        assert!(detect_face(&Image::filled(300, 300, [200, 200, 200, 255]).unwrap()).is_none());
        assert!(detect_face(&Image::filled(10, 10, [0, 0, 0, 255]).unwrap()).is_none());
    }
}

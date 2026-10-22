//! Fallback face detector: the SeetaFace funnel-structured cascade through `rustface`, model
//! embedded. Frontal faces only and no key points, so it is used when BlazeFace finds nothing.

use forge_doc::{FaceBox, FaceDetection};
use forge_geom::Rect;
use forge_image::{Image, Quality, resize_region};

static MODEL: &[u8] = include_bytes!("../../../assets/seeta_fd_frontal_v1.0.bin");

/// Longest side the image is shrunk to before detection.
const DETECT_EDGE: u32 = 640;

pub struct SeetaDetector {
    inner: Box<dyn rustface::Detector>,
}

impl SeetaDetector {
    pub fn new() -> Option<Self> {
        let model = rustface::read_model(MODEL).ok()?;
        let mut inner = rustface::create_detector_with_model(model);
        inner.set_score_thresh(2.0);
        inner.set_pyramid_scale_factor(0.8);
        inner.set_slide_window_step(4, 4);
        Some(Self { inner })
    }

    /// The largest face, in pixel coordinates of `img`.
    pub fn detect(&mut self, img: &Image) -> Option<FaceDetection> {
        if img.width < 24 || img.height < 24 {
            return None;
        }
        let long = img.width.max(img.height);
        let (gray, w, h, scale) = if long > DETECT_EDGE {
            let s = f64::from(DETECT_EDGE) / f64::from(long);
            let (w, h) = (((f64::from(img.width) * s).round() as u32).max(24), ((f64::from(img.height) * s).round() as u32).max(24));
            let small = resize_region(img, Rect::new(0.0, 0.0, f64::from(img.width), f64::from(img.height)), w, h, Quality::Fast).ok()?;
            (small.to_gray(), w, h, f64::from(img.width) / f64::from(w))
        } else {
            (img.to_gray(), img.width, img.height, 1.0)
        };
        // Faces under ~7% of the short side are not an ID subject.
        self.inner.set_min_face_size(((w.min(h) as f32) * 0.07).max(20.0) as u32);
        let data = rustface::ImageData::new(&gray, w, h);
        // The detector library can panic on malformed input; that must never reach the user.
        let faces = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.inner.detect(&data))).ok()?;
        let area = |r: &rustface::Rectangle| f64::from(r.width()) * f64::from(r.height());
        let best = faces.iter().max_by(|a, b| area(a.bbox()).partial_cmp(&area(b.bbox())).unwrap_or(std::cmp::Ordering::Equal))?;
        let b = best.bbox();
        Some(FaceDetection {
            bounding_box: FaceBox {
                origin_x: f64::from(b.x().max(0)) * scale,
                origin_y: f64::from(b.y().max(0)) * scale,
                width: f64::from(b.width()) * scale,
                height: f64::from(b.height()) * scale,
            },
            keypoints: vec![],
            score: 0.0,
        })
    }
}

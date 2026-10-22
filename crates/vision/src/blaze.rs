//! BlazeFace (MediaPipe "short range") face detection on top of the TFLite interpreter.
//!
//! Same pipeline as MediaPipe's face detector: letterbox to 128x128, normalise to [-1, 1], decode the
//! 896 SSD anchors (offsets + 6 key points), keep scores >= 0.5, weighted non-maximum suppression.
//! Key points: right eye, left eye, nose, mouth, right ear, left ear (the crop uses the eyes).

use forge_doc::{FaceBox, FaceDetection};
use forge_geom::{Point, Rect};
use forge_image::{Image, Quality, resize_region};

use crate::tflite::{Model, Tensor};

static MODEL_BYTES: &[u8] = include_bytes!("../../../assets/blaze_face_short_range.tflite");

const SIZE: usize = 128;
const KEYPOINTS: usize = 6;
const MIN_SCORE: f32 = 0.5;
const NMS_IOU: f32 = 0.3;

pub struct BlazeFace {
    model: Model,
    anchors: Vec<(f32, f32)>,
}

/// SSD anchor centres for the short-range model: strides [8,16,16,16], 2 anchors per cell on the
/// stride-8 layer and 6 on the (merged) stride-16 layers = 896. Anchor width/height are fixed to 1.
fn anchors() -> Vec<(f32, f32)> {
    let strides = [8usize, 16, 16, 16];
    let mut out = Vec::new();
    let mut layer = 0;
    while layer < strides.len() {
        let mut per_cell = 0;
        let mut last = layer;
        while last < strides.len() && strides[last] == strides[layer] {
            per_cell += 2; // one aspect ratio + the interpolated-scale anchor
            last += 1;
        }
        let fm = SIZE.div_ceil(strides[layer]);
        for y in 0..fm {
            for x in 0..fm {
                for _ in 0..per_cell {
                    out.push(((x as f32 + 0.5) / fm as f32, (y as f32 + 0.5) / fm as f32));
                }
            }
        }
        layer = last;
    }
    out
}

#[derive(Clone)]
struct Cand {
    score: f32,
    /// xmin, ymin, xmax, ymax in letterboxed normalised units.
    b: [f32; 4],
    kp: [(f32, f32); KEYPOINTS],
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let inter = w * h;
    let u = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter;
    if u <= 0.0 { 0.0 } else { inter / u }
}

impl BlazeFace {
    pub fn new() -> Option<Self> {
        let model = Model::parse(MODEL_BYTES)?;
        let anchors = anchors();
        // Sanity: input is 1x128x128x3 and one output has 896 x 16 values, the other 896 x 1.
        let ok = model.input_shape()? == [1, SIZE, SIZE, 3] && model.output_count() == 2;
        ok.then_some(Self { model, anchors })
    }

    /// All faces with score >= 0.5, best first, in pixel coordinates of `img`.
    pub fn detect(&self, img: &Image) -> Vec<FaceDetection> {
        let (w, h) = (img.width as f64, img.height as f64);
        if img.width < 16 || img.height < 16 {
            return vec![];
        }
        let s = SIZE as f64 / w.max(h);
        let (rw, rh) = (((w * s).round() as u32).clamp(1, SIZE as u32), ((h * s).round() as u32).clamp(1, SIZE as u32));
        let Ok(small) = resize_region(img, Rect::new(0.0, 0.0, w, h), rw, rh, Quality::High) else { return vec![] };
        let (padx, pady) = ((SIZE as u32 - rw) / 2, (SIZE as u32 - rh) / 2);
        let mut input = vec![-1.0f32; SIZE * SIZE * 3];
        for y in 0..rh as usize {
            for x in 0..rw as usize {
                let si = (y * rw as usize + x) * 4;
                let di = ((y + pady as usize) * SIZE + x + padx as usize) * 3;
                let a = f32::from(small.rgba.get(si + 3).copied().unwrap_or(255)) / 255.0;
                for c in 0..3 {
                    let v = f32::from(small.rgba.get(si + c).copied().unwrap_or(0));
                    // Transparent pixels count as white, like the rest of the app.
                    let v = v * a + 255.0 * (1.0 - a);
                    if let Some(d) = input.get_mut(di + c) {
                        *d = v / 127.5 - 1.0;
                    }
                }
            }
        }
        let Some(outs) = self.model.run(&input) else { return vec![] };
        let (Some(reg), Some(cls)) = (pick(&outs, KEYPOINTS * 2 + 4), pick(&outs, 1)) else { return vec![] };
        let mut cands: Vec<Cand> = Vec::new();
        for (i, &(ax, ay)) in self.anchors.iter().enumerate() {
            let Some(&raw_s) = cls.data.get(i) else { break };
            let score = 1.0 / (1.0 + (-raw_s.clamp(-100.0, 100.0)).exp());
            if score < MIN_SCORE {
                continue;
            }
            let Some(r) = reg.data.get(i * 16..i * 16 + 16) else { break };
            let sz = SIZE as f32;
            let (cx, cy) = (r[0] / sz + ax, r[1] / sz + ay);
            let (bw, bh) = (r[2] / sz, r[3] / sz);
            let mut kp = [(0.0, 0.0); KEYPOINTS];
            for (k, p) in kp.iter_mut().enumerate() {
                *p = (r[4 + 2 * k] / sz + ax, r[5 + 2 * k] / sz + ay);
            }
            cands.push(Cand { score, b: [cx - bw / 2.0, cy - bh / 2.0, cx + bw / 2.0, cy + bh / 2.0], kp });
        }
        cands.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // Weighted NMS: overlapping candidates are blended into the best one by score.
        let mut results: Vec<Cand> = Vec::new();
        let mut rest = cands;
        while !rest.is_empty() {
            let best = rest.remove(0);
            let (mut group, keep): (Vec<Cand>, Vec<Cand>) = rest.into_iter().partition(|c| iou(&best.b, &c.b) > NMS_IOU);
            rest = keep;
            group.push(best.clone());
            let tot: f32 = group.iter().map(|c| c.score).sum();
            let mut m = Cand { score: best.score, b: [0.0; 4], kp: [(0.0, 0.0); KEYPOINTS] };
            for c in &group {
                let wgt = c.score / tot;
                for i in 0..4 {
                    m.b[i] += c.b[i] * wgt;
                }
                for k in 0..KEYPOINTS {
                    m.kp[k].0 += c.kp[k].0 * wgt;
                    m.kp[k].1 += c.kp[k].1 * wgt;
                }
            }
            results.push(m);
        }

        // Back to image pixels: undo the letterbox.
        let to_px = |x: f32, y: f32| -> (f64, f64) {
            ((f64::from(x) * SIZE as f64 - f64::from(padx)) / s, (f64::from(y) * SIZE as f64 - f64::from(pady)) / s)
        };
        results
            .iter()
            .filter_map(|c| {
                let (x0, y0) = to_px(c.b[0], c.b[1]);
                let (x1, y1) = to_px(c.b[2], c.b[3]);
                let (x0, y0, x1, y1) = (x0.max(0.0), y0.max(0.0), x1.min(w), y1.min(h));
                if !(x1 > x0 && y1 > y0) {
                    return None;
                }
                let keypoints = c.kp.iter().map(|&(x, y)| {
                    let (px, py) = to_px(x, y);
                    Point::new(px, py)
                });
                Some(FaceDetection {
                    bounding_box: FaceBox { origin_x: x0, origin_y: y0, width: x1 - x0, height: y1 - y0 },
                    keypoints: keypoints.collect(),
                    score: c.score,
                })
            })
            .collect()
    }
}

/// The output whose last dimension is `last`.
fn pick(outs: &[Tensor], last: usize) -> Option<&Tensor> {
    outs.iter().find(|t| t.shape.last() == Some(&last))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_count_matches_model() {
        assert_eq!(anchors().len(), 896);
    }

    #[test]
    fn model_loads_and_runs_on_blank() {
        let m = BlazeFace::new().expect("model");
        let blank = Image::filled(320, 240, [128, 128, 128, 255]).unwrap();
        assert!(m.detect(&blank).is_empty());
        let tiny = Image::filled(4, 4, [0, 0, 0, 255]).unwrap();
        assert!(m.detect(&tiny).is_empty());
    }

    #[test]
    fn finds_faces_in_real_photos() {
        let dir = std::env::var("FORGE_TEST_FACES").unwrap_or_default();
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        let m = BlazeFace::new().expect("model");
        let mut n = 0;
        for e in rd.flatten() {
            let Ok(bytes) = std::fs::read(e.path()) else { continue };
            let Ok(img) = forge_image::decode(&bytes) else { continue };
            let d = m.detect(&img);
            eprintln!("{:?}: {} faces {:?}", e.file_name(), d.len(), d.first().map(|f| (f.score, f.bounding_box, f.keypoints.first().cloned())));
            // Portraits must be found; a full-body sports shot with tiny faces may legitimately be a miss
            // for the short-range model.
            if !e.file_name().to_string_lossy().starts_with("messi") {
                assert!(!d.is_empty(), "{:?}", e.file_name());
            }
            n += 1;
        }
        eprintln!("{n} test images");
    }
}

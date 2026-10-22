//! Crop maths shared by the on-screen preview and every exporter, so what you see is what you print.

use serde::{Deserialize, Serialize};

use forge_geom::{Point, Rect};

/// An automatic crop, in source-image pixels. May extend outside the image: the missing part is
/// painted white (intentional, so a face near the edge keeps its framing).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A manual crop: zoom in percent, pan offsets in percent of the overflow (-100..=100).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ManualCrop {
    pub zoom: f64,
    pub offset_x: f64,
    pub offset_y: f64,
}

impl Default for ManualCrop {
    fn default() -> Self {
        Self { zoom: 100.0, offset_x: 0.0, offset_y: 0.0 }
    }
}

/// A detected face's bounding box in source pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceBox {
    pub origin_x: f64,
    pub origin_y: f64,
    pub width: f64,
    pub height: f64,
}

/// A face detection result. `keypoints[0]`/`[1]` are the right and left eye when the detector
/// provides them (normalised 0..1 or pixels; both are understood, like the original app).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceDetection {
    pub bounding_box: FaceBox,
    #[serde(default)]
    pub keypoints: Vec<Point>,
    /// The detector's confidence, 0..=1 (0 when unknown).
    #[serde(default)]
    pub score: f32,
}

/// Default aspect when none is given: a 35 x 45 mm passport photo.
pub const PASSPORT_ASPECT: f64 = 35.0 / 45.0;

/// A crop rectangle that frames the face in the centre third of a 3x3 grid.
///
/// The crop is three times the face (by height or width, whichever gives the larger crop, so the
/// face never overflows), centred horizontally on the midpoint between the eyes (symmetric even with
/// hats and hair) and vertically on the face centre. Ported from the original app's `computeAutoCrop`.
pub fn compute_auto_crop(det: &FaceDetection, img_w: f64, _img_h: f64, target_aspect: f64) -> Option<CropRect> {
    let face = det.bounding_box;
    if !(face.width > 0.0 && face.height > 0.0) || !face.width.is_finite() || !face.height.is_finite() {
        return None;
    }
    let aspect = if target_aspect > 0.0 && target_aspect.is_finite() { target_aspect } else { PASSPORT_ASPECT };

    let face_cy = face.origin_y + face.height / 2.0;
    let mut eye_x = face.origin_x + face.width / 2.0;
    if let (Some(r), Some(l)) = (det.keypoints.first(), det.keypoints.get(1)) {
        let px = |v: f64, size: f64| if v <= 1.0 { v * size } else { v };
        eye_x = (px(r.x, img_w) + px(l.x, img_w)) / 2.0;
    }

    let h_from_height = face.height * 3.0;
    let w_from_width = face.width * 3.0;
    let h_from_width = w_from_width / aspect;
    let (crop_w, crop_h) = if h_from_height > h_from_width { (h_from_height * aspect, h_from_height) } else { (w_from_width, h_from_width) };

    let r = |v: f64| v.round();
    Some(CropRect { x: r(eye_x - crop_w / 2.0), y: r(face_cy - crop_h / 2.0), width: r(crop_w), height: r(crop_h) })
}

/// Which crop applies to a photo. Priority: manual > automatic > the field's own defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectiveCrop {
    Manual(ManualCrop),
    Auto(CropRect),
    /// Fallback: the photo field's zoom/offsets.
    Default(ManualCrop),
}

/// Where the whole source image lands, in the coordinates of a box of `box_w` x `box_h` whose
/// top-left is (0, 0). The renderer draws the image into `dest` clipped to the box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub dest: Rect,
}

/// "Cover" a box with an image of `iw` x `ih`: the smaller side fits exactly, the other overflows.
fn cover(iw: f64, ih: f64, bw: f64, bh: f64) -> (f64, f64) {
    if iw <= 0.0 || ih <= 0.0 || bw <= 0.0 || bh <= 0.0 {
        return (bw.max(0.0), bh.max(0.0));
    }
    let ir = iw / ih;
    let br = bw / bh;
    if ir > br { (bh * ir, bh) } else { (bw, bw / ir) }
}

/// Computes where the image goes for `crop` inside a `box_w` x `box_h` box.
///
/// * Manual / default: cover the box, scale by `zoom`, pan by `offset` (a fraction of the overflow).
/// * Auto: the crop rectangle is scaled uniformly to cover the box and centred on the box, so a
///   crop of a slightly different aspect than the box is never stretched (the original stretched it).
pub fn placement(img_w: f64, img_h: f64, box_w: f64, box_h: f64, crop: &EffectiveCrop) -> Placement {
    match crop {
        EffectiveCrop::Auto(c) if c.width > 0.0 && c.height > 0.0 => {
            let s = (box_w / c.width).max(box_h / c.height);
            let (cx, cy) = (c.x + c.width / 2.0, c.y + c.height / 2.0);
            Placement { dest: Rect::new(box_w / 2.0 - cx * s, box_h / 2.0 - cy * s, img_w * s, img_h * s) }
        }
        EffectiveCrop::Auto(_) => placement(img_w, img_h, box_w, box_h, &EffectiveCrop::Default(ManualCrop::default())),
        EffectiveCrop::Manual(m) | EffectiveCrop::Default(m) => {
            let (bw, bh) = cover(img_w, img_h, box_w, box_h);
            let zoom = if m.zoom > 0.0 { m.zoom / 100.0 } else { 1.0 };
            let (dw, dh) = (bw * zoom, bh * zoom);
            let (ox, oy) = (m.offset_x / 100.0, m.offset_y / 100.0);
            Placement { dest: Rect::new(-(dw - box_w) / 2.0 - ox * (dw - box_w), -(dh - box_h) / 2.0 - oy * (dh - box_h), dw, dh) }
        }
    }
}

/// The manual crop (zoom percent + offsets) that places the whole image at `dest` inside a
/// `box_w` x `box_h` box: the inverse of [`placement`] for `Manual`/`Default` crops. It lets an
/// editor start from the automatic crop and drag or zoom it as a manual one. Along an axis with no
/// overflow the offset is 0.
pub fn manual_from_dest(img_w: f64, img_h: f64, box_w: f64, box_h: f64, dest: Rect) -> ManualCrop {
    let (bw, _) = cover(img_w, img_h, box_w, box_h);
    let zoom = if bw > 0.0 && dest.w.is_finite() { (dest.w / bw) * 100.0 } else { 100.0 };
    let (dw, dh) = (dest.w, dest.h);
    let off = |pos: f64, d: f64, b: f64| {
        let over = d - b;
        if over.abs() < 1e-9 { 0.0 } else { (-pos - over / 2.0) / over * 100.0 }
    };
    ManualCrop {
        zoom: zoom.clamp(10.0, 1000.0),
        offset_x: off(dest.x, dw, box_w).clamp(-100.0, 100.0),
        offset_y: off(dest.y, dh, box_h).clamp(-100.0, 100.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(ox: f64, oy: f64, w: f64, h: f64) -> FaceDetection {
        FaceDetection { bounding_box: FaceBox { origin_x: ox, origin_y: oy, width: w, height: h }, keypoints: vec![], score: 0.0 }
    }

    #[test]
    fn auto_crop_is_three_faces_tall() {
        let c = compute_auto_crop(&det(400.0, 300.0, 150.0, 250.0), 1000.0, 1000.0, 35.0 / 45.0).unwrap();
        // face height 250 -> crop height 750 (3 x 150 wide would only give 578); width = 750 * 35/45 = 583.33 -> 583
        assert_eq!(c.height, 750.0);
        assert_eq!(c.width, 583.0);
        // centred on face centre x = 475, y = 425
        assert_eq!(c.x, (475.0f64 - 583.333_333_333_333_3 / 2.0).round());
        assert_eq!(c.y, (425.0f64 - 375.0).round());
    }

    #[test]
    fn auto_crop_picks_the_larger_of_height_and_width_basis() {
        // 200 x 250 face at 35:45: 3 x width (600) implies a taller crop (771) than 3 x height (750).
        let c = compute_auto_crop(&det(400.0, 300.0, 200.0, 250.0), 1000.0, 1000.0, 35.0 / 45.0).unwrap();
        assert_eq!((c.width, c.height), (600.0, 771.0));
    }

    #[test]
    fn auto_crop_wide_face_uses_width() {
        // very wide box: 3 * width gives the larger crop
        let c = compute_auto_crop(&det(0.0, 0.0, 400.0, 100.0), 2000.0, 2000.0, 1.0).unwrap();
        assert_eq!((c.width, c.height), (1200.0, 1200.0));
    }

    #[test]
    fn auto_crop_uses_eye_midpoint_normalised_or_pixels() {
        let mut d = det(100.0, 100.0, 100.0, 100.0);
        d.keypoints = vec![Point::new(0.4, 0.5), Point::new(0.6, 0.5)];
        let a = compute_auto_crop(&d, 1000.0, 1000.0, 1.0).unwrap();
        d.keypoints = vec![Point::new(400.0, 500.0), Point::new(600.0, 500.0)];
        let b = compute_auto_crop(&d, 1000.0, 1000.0, 1.0).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.x, 500.0 - 150.0);
    }

    #[test]
    fn auto_crop_rejects_empty_faces_and_bad_aspect() {
        assert!(compute_auto_crop(&det(0.0, 0.0, 0.0, 10.0), 100.0, 100.0, 1.0).is_none());
        assert!(compute_auto_crop(&det(0.0, 0.0, f64::NAN, 10.0), 100.0, 100.0, 1.0).is_none());
        let c = compute_auto_crop(&det(10.0, 10.0, 10.0, 10.0), 100.0, 100.0, -5.0).unwrap();
        // falls back to the passport aspect: 3 x width = 30 -> height 30 / (35/45) = 38.57
        assert_eq!((c.width, c.height), (30.0, 39.0));
    }

    #[test]
    fn manual_default_covers_box() {
        // 2000x1000 image in a 100x100 box: height fits, width overflows and is centred
        let p = placement(2000.0, 1000.0, 100.0, 100.0, &EffectiveCrop::Default(ManualCrop::default()));
        assert_eq!(p.dest, Rect::new(-50.0, 0.0, 200.0, 100.0));
        // zoom 200% keeps the centre
        let p = placement(2000.0, 1000.0, 100.0, 100.0, &EffectiveCrop::Manual(ManualCrop { zoom: 200.0, offset_x: 0.0, offset_y: 0.0 }));
        assert_eq!(p.dest, Rect::new(-150.0, -50.0, 400.0, 200.0));
        // pan right by half the overflow moves the image left by half of it
        let p = placement(1000.0, 1000.0, 100.0, 100.0, &EffectiveCrop::Manual(ManualCrop { zoom: 200.0, offset_x: 50.0, offset_y: 0.0 }));
        assert_eq!(p.dest.x, -50.0 - 0.5 * 100.0);
    }

    #[test]
    fn auto_placement_maps_crop_onto_box() {
        let crop = CropRect { x: 100.0, y: 50.0, width: 200.0, height: 400.0 };
        let p = placement(1000.0, 1000.0, 50.0, 100.0, &EffectiveCrop::Auto(crop));
        // scale 0.25: crop origin lands on the box origin
        assert!((p.dest.x - (-25.0)).abs() < 1e-9);
        assert!((p.dest.y - (-12.5)).abs() < 1e-9);
        assert!((p.dest.w - 250.0).abs() < 1e-9);
    }

    #[test]
    fn auto_placement_never_stretches() {
        // crop aspect 0.5, box aspect 1.0: uniform scale
        let crop = CropRect { x: 0.0, y: 0.0, width: 100.0, height: 200.0 };
        let p = placement(400.0, 400.0, 100.0, 100.0, &EffectiveCrop::Auto(crop));
        assert!((p.dest.w / p.dest.h - 1.0).abs() < 1e-9);
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        let _ = placement(0.0, 0.0, 0.0, 0.0, &EffectiveCrop::Default(ManualCrop::default()));
        let _ = placement(10.0, 10.0, 10.0, 10.0, &EffectiveCrop::Auto(CropRect::default()));
        let _ = placement(f64::NAN, 1.0, 1.0, 1.0, &EffectiveCrop::Manual(ManualCrop { zoom: -3.0, offset_x: 0.0, offset_y: 0.0 }));
    }

    #[test]
    fn manual_from_dest_inverts_placement() {
        for (zoom, ox, oy) in [(100.0, 0.0, 0.0), (180.0, 12.0, -30.0), (250.0, -40.0, 25.0)] {
            let m = ManualCrop { zoom, offset_x: ox, offset_y: oy };
            let dest = placement(1200.0, 800.0, 300.0, 400.0, &EffectiveCrop::Manual(m)).dest;
            let back = manual_from_dest(1200.0, 800.0, 300.0, 400.0, dest);
            let again = placement(1200.0, 800.0, 300.0, 400.0, &EffectiveCrop::Manual(back)).dest;
            assert!((dest.x - again.x).abs() < 1e-6 && (dest.w - again.w).abs() < 1e-6 && (dest.y - again.y).abs() < 1e-6, "{m:?} -> {back:?}");
        }
    }
}

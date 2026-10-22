//! Background clean-up for studio-style ID photos: replaces a plain backdrop with a solid colour.
//!
//! The original app offered an optional neural background remover (downloaded on first use). This
//! is a dependency-free alternative that works offline: it samples the backdrop colour from the
//! photo's border, flood-fills the connected region that matches it, feathers the matte and
//! composites with the same "choke + feather" blend as the original.

use forge_image::Image;

/// Settings for [`replace_background`].
#[derive(Clone, Copy, Debug)]
pub struct BgOptions {
    /// New background colour.
    pub color: [u8; 3],
    /// Colour distance (0..=441) below which a pixel counts as backdrop.
    pub tolerance: f32,
    /// Matte blur radius in pixels (0 = automatic from the image width).
    pub feather: u32,
}

impl Default for BgOptions {
    fn default() -> Self {
        Self { color: [255, 255, 255], tolerance: 48.0, feather: 0 }
    }
}

fn dist(a: [u8; 3], b: [f32; 3]) -> f32 {
    let (dr, dg, db) = (f32::from(a[0]) - b[0], f32::from(a[1]) - b[1], f32::from(a[2]) - b[2]);
    (dr * dr + dg * dg + db * db).sqrt()
}

/// Median-ish backdrop colour from the outer rim of the image.
fn border_color(img: &Image) -> [f32; 3] {
    let (w, h) = (img.width as usize, img.height as usize);
    let rim = (w.min(h) / 50).max(1);
    let (mut r, mut g, mut b, mut n) = (0f64, 0f64, 0f64, 0f64);
    for y in 0..h {
        for x in 0..w {
            if let (true, Some(p)) = (x < rim || y < rim || x >= w - rim || y >= h - rim, img.rgba.get((y * w + x) * 4..(y * w + x) * 4 + 3)) {
                r += f64::from(p[0]);
                g += f64::from(p[1]);
                b += f64::from(p[2]);
                n += 1.0;
            }
        }
    }
    if n == 0.0 { [255.0; 3] } else { [(r / n) as f32, (g / n) as f32, (b / n) as f32] }
}

/// Returns a copy of `img` whose backdrop is replaced by `opts.color`. Images without a clearly
/// uniform border are returned unchanged (the second value is `false`).
pub fn replace_background(img: &Image, opts: BgOptions) -> (Image, bool) {
    let (w, h) = (img.width as usize, img.height as usize);
    if w < 8 || h < 8 {
        return (img.clone(), false);
    }
    let bg = border_color(img);
    let px = |i: usize| -> [u8; 3] {
        let o = i * 4;
        [img.rgba.get(o).copied().unwrap_or(0), img.rgba.get(o + 1).copied().unwrap_or(0), img.rgba.get(o + 2).copied().unwrap_or(0)]
    };
    // Is the rim uniform enough to trust? (mean distance of rim pixels to the rim colour)
    let mut spread = 0f32;
    let mut count = 0f32;
    for x in (0..w).step_by(7) {
        spread += dist(px(x), bg) + dist(px((h - 1) * w + x), bg);
        count += 2.0;
    }
    if spread / count.max(1.0) > opts.tolerance {
        return (img.clone(), false);
    }

    // Flood fill from every border pixel through pixels close to the backdrop colour.
    let mut mask = vec![0u8; w * h]; // 255 = backdrop
    let mut stack: Vec<usize> = Vec::new();
    let push = |i: usize, mask: &mut Vec<u8>, stack: &mut Vec<usize>| {
        if mask.get(i) == Some(&0) && dist(px(i), bg) <= opts.tolerance {
            if let Some(m) = mask.get_mut(i) {
                *m = 255;
            }
            stack.push(i);
        }
    };
    for x in 0..w {
        push(x, &mut mask, &mut stack);
        push((h - 1) * w + x, &mut mask, &mut stack);
    }
    for y in 0..h {
        push(y * w, &mut mask, &mut stack);
        push(y * w + w - 1, &mut mask, &mut stack);
    }
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        if x > 0 {
            push(i - 1, &mut mask, &mut stack);
        }
        if x + 1 < w {
            push(i + 1, &mut mask, &mut stack);
        }
        if y > 0 {
            push(i - w, &mut mask, &mut stack);
        }
        if y + 1 < h {
            push(i + w, &mut mask, &mut stack);
        }
    }

    // Foreground alpha = 255 - backdrop, blurred, then choked like the original (cut below 140).
    let radius = if opts.feather > 0 { opts.feather as usize } else { (w / 800).max(2) };
    let fg: Vec<u8> = mask.iter().map(|m| 255 - m).collect();
    let alpha = box_blur(&fg, w, h, radius);

    let mut out = img.clone();
    let choke = 140.0f32;
    for (i, a) in alpha.iter().enumerate() {
        let a = f32::from(*a);
        let a = if a < choke { 0.0 } else { ((a - choke) / (255.0 - choke) * 255.0).min(255.0) };
        let k = a / 255.0;
        let o = i * 4;
        for c in 0..3 {
            if let (Some(dst), Some(&src)) = (out.rgba.get_mut(o + c), img.rgba.get(o + c)) {
                *dst = (f32::from(src) * k + f32::from(opts.color[c]) * (1.0 - k)).round() as u8;
            }
        }
        if let Some(al) = out.rgba.get_mut(o + 3) {
            *al = 255;
        }
    }
    (out, true)
}

/// Separable box blur (two passes approximate a smooth falloff well enough for a matte).
fn box_blur(src: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    let r = r.min(w.min(h) / 2).max(1);
    let horiz = |data: &[u8], w: usize, h: usize| -> Vec<u8> {
        let mut out = vec![0u8; data.len()];
        for y in 0..h {
            let row = y * w;
            let mut sum: usize = 0;
            let win = 2 * r + 1;
            for x in 0..w.min(r + 1) {
                sum += usize::from(data.get(row + x).copied().unwrap_or(0));
            }
            sum += usize::from(data.get(row).copied().unwrap_or(0)) * r; // clamp-left padding
            for x in 0..w {
                if let Some(o) = out.get_mut(row + x) {
                    *o = (sum / win) as u8;
                }
                let add = data.get(row + (x + r + 1).min(w - 1)).copied().unwrap_or(0);
                let sub = data.get(row + x.saturating_sub(r)).copied().unwrap_or(0);
                sum = sum + usize::from(add) - usize::from(sub);
            }
        }
        out
    };
    let transpose = |data: &[u8], w: usize, h: usize| -> Vec<u8> {
        let mut t = vec![0u8; data.len()];
        for y in 0..h {
            for x in 0..w {
                if let (Some(d), Some(&s)) = (t.get_mut(x * h + y), data.get(y * w + x)) {
                    *d = s;
                }
            }
        }
        t
    };
    let a = horiz(src, w, h);
    let t = transpose(&a, w, h);
    let b = horiz(&t, h, w);
    transpose(&b, h, w)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grey backdrop with a red square "subject" in the middle.
    fn subject() -> Image {
        let mut img = Image::filled(120, 120, [120, 120, 125, 255]).unwrap();
        for y in 40..80usize {
            for x in 40..80usize {
                let i = (y * 120 + x) * 4;
                img.rgba[i..i + 4].copy_from_slice(&[200, 30, 30, 255]);
            }
        }
        img
    }

    #[test]
    fn plain_backdrop_becomes_white_subject_stays() {
        let (out, changed) = replace_background(&subject(), BgOptions { feather: 1, ..BgOptions::default() });
        assert!(changed);
        assert_eq!(out.get(3, 3), Some([255, 255, 255, 255]));
        assert_eq!(out.get(60, 60), Some([200, 30, 30, 255]));
    }

    #[test]
    fn busy_border_is_left_alone() {
        let mut img = Image::filled(64, 64, [0, 0, 0, 255]).unwrap();
        for x in 0..64usize {
            let v = if x % 2 == 0 { 0 } else { 255 };
            for y in [0usize, 63] {
                let i = (y * 64 + x) * 4;
                img.rgba[i..i + 3].copy_from_slice(&[v, v, v]);
            }
        }
        let (out, changed) = replace_background(&img, BgOptions::default());
        assert!(!changed);
        assert_eq!(out, img);
    }

    #[test]
    fn tiny_images_do_not_panic() {
        let img = Image::filled(3, 3, [1, 2, 3, 255]).unwrap();
        assert!(!replace_background(&img, BgOptions::default()).1);
    }
}

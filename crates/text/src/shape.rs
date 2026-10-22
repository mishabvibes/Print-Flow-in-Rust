//! Shaping: text -> positioned glyphs, with per-grapheme font fallback.

use std::collections::HashMap;
use std::sync::Arc;

use harfrust::{Direction, Feature, ShapeOptions, Tag, UnicodeBuffer};
use unicode_segmentation::UnicodeSegmentation;

use crate::fontdb::{FontDb, FontFace};

/// Synthetic small capitals are drawn at this fraction of the size.
const SYNTH_SMALL_CAPS: f64 = 0.8;

/// One shaped glyph. Metrics are in pixels at the size it was shaped at.
#[derive(Clone, Debug)]
pub struct Glyph {
    pub face: Arc<FontFace>,
    pub gid: u32,
    /// Byte offset of the source cluster in the shaped text.
    pub byte: usize,
    pub adv: f64,
    pub dx: f64,
    pub dy: f64,
    /// Size the glyph is drawn at (differs from the requested size for synthetic small caps).
    pub size: f64,
    pub is_space: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    /// Real OpenType small capitals.
    Smcp,
    /// Lower case drawn as reduced upper case.
    Synth,
}

fn ignorable(c: char) -> bool {
    c.is_control() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{2060}' | '\u{feff}' | '\u{fe00}'..='\u{fe0f}')
}

/// Shapes `text` (no newlines) with the faces of `chain` in order, falling back through the catalogue
/// for characters none of them has. Glyphs come back in visual order, advances in pixels at `size`.
///
/// Right-to-left runs are shaped correctly (letter forms, ligatures) and drawn in the right order
/// within one run, but there is no full bidirectional reordering of mixed-direction text.
pub fn shape_text(db: &FontDb, chain: &[Arc<FontFace>], text: &str, size: f64, weight: u32, italic: bool, small_caps: bool) -> Vec<Glyph> {
    let mut out = Vec::new();
    let Some(primary) = chain.first() else { return out };
    if text.is_empty() || !size.is_finite() || size <= 0.0 {
        return out;
    }
    let mut cover_cache: HashMap<char, usize> = HashMap::new();
    let mut face_of = |c: char| -> Option<usize> {
        if let Some(i) = cover_cache.get(&c) {
            return Some(*i);
        }
        let i = chain.iter().position(|f| f.covers(c))?;
        cover_cache.insert(c, i);
        Some(i)
    };

    // (byte range, face, mode); faces beyond the chain are appended to `extra`.
    let mut extra: Vec<Arc<FontFace>> = Vec::new();
    let mut segs: Vec<(usize, usize, usize, Mode)> = Vec::new();
    let mut last_face = 0usize;
    for (start, g) in text.grapheme_indices(true) {
        let probe: Vec<char> = g.chars().filter(|c| !ignorable(*c)).collect();
        let fi = match probe.first() {
            None => last_face,
            Some(&first) => {
                let all = probe.iter().try_fold(None::<usize>, |acc, &c| {
                    let i = face_of(c);
                    match (acc, i) {
                        (None, i) => Some(i),
                        (Some(a), Some(b)) if a == b => Some(Some(a)),
                        _ => None,
                    }
                });
                match all {
                    // Every character of the grapheme comes from the same face.
                    Some(Some(i)) => i,
                    _ => match face_of(first).or_else(|| probe.iter().find_map(|&c| face_of(c))) {
                        Some(i) if probe.iter().all(|&c| chain.get(i).is_some_and(|f| f.covers(c))) => i,
                        _ => {
                            // Search the catalogue for a face with the grapheme's base character.
                            match db.fallback_for(first, weight, italic) {
                                Some(f) => match chain.iter().position(|c| c.id == f.id) {
                                    Some(i) => i,
                                    None => match extra.iter().position(|c| c.id == f.id) {
                                        Some(i) => chain.len() + i,
                                        None => {
                                            extra.push(f);
                                            chain.len() + extra.len() - 1
                                        }
                                    },
                                },
                                None => 0,
                            }
                        }
                    },
                }
            }
        };
        last_face = fi;
        let face = chain.get(fi).or_else(|| extra.get(fi - chain.len())).unwrap_or(primary);
        let lower = small_caps && g.chars().next().is_some_and(|c| c.is_lowercase());
        let mode = if !lower {
            Mode::Normal
        } else if face.has_small_caps() {
            Mode::Smcp
        } else {
            Mode::Synth
        };
        match segs.last_mut() {
            Some(last) if last.2 == fi && last.3 == mode => last.1 = start + g.len(),
            _ => segs.push((start, start + g.len(), fi, mode)),
        }
    }

    for (a, b, fi, mode) in segs {
        let face = chain.get(fi).or_else(|| extra.get(fi - chain.len())).unwrap_or(primary);
        let Some(seg) = text.get(a..b) else { continue };
        shape_segment(face, seg, a, size, mode, &mut out);
    }
    out
}

fn shape_segment(face: &Arc<FontFace>, seg: &str, base: usize, size: f64, mode: Mode, out: &mut Vec<Glyph>) {
    let gsize = if mode == Mode::Synth { size * SYNTH_SMALL_CAPS } else { size };
    let k = gsize / face.upem;
    let mut raw: Vec<(u32, usize, i32, i32, i32)> = Vec::with_capacity(seg.len());
    if let Some(hb) = face.hb() {
        let shaper = face.shaper.shaper(&hb).instance(face.instance.as_ref()).build();
        let mut buf = UnicodeBuffer::new();
        for (i, c) in seg.char_indices() {
            let cl = (base + i) as u32;
            if mode == Mode::Synth {
                for u in c.to_uppercase() {
                    buf.add(u, cl);
                }
            } else {
                buf.add(c, cl);
            }
        }
        buf.set_direction(Direction::LeftToRight);
        buf.guess_segment_properties();
        let feats: Vec<Feature> = if mode == Mode::Smcp { vec![Feature::new(Tag::new(b"smcp"), 1, ..)] } else { vec![] };
        let gb = shaper.shape(buf, ShapeOptions::new().features(&feats));
        for (info, pos) in gb.glyph_infos().iter().zip(gb.glyph_positions()) {
            raw.push((info.glyph_id, info.cluster as usize, pos.x_advance, pos.x_offset, pos.y_offset));
        }
    }
    for (gid, cl, xa, xo, yo) in raw {
        let ch = seg.get(cl.saturating_sub(base)..).and_then(|s| s.chars().next()).unwrap_or(' ');
        let soft_hyphen = ch == '\u{ad}';
        out.push(Glyph {
            face: face.clone(),
            gid,
            byte: cl,
            adv: if soft_hyphen { 0.0 } else { xa as f64 * k },
            dx: xo as f64 * k,
            dy: -(yo as f64) * k,
            size: gsize,
            is_space: ch.is_whitespace(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fontdb::FontRequest;

    fn chain(db: &FontDb) -> Vec<Arc<FontFace>> {
        db.resolve_chain(&FontRequest::new("Stem", 400, false))
    }

    #[test]
    fn shapes_latin_with_advances() {
        let db = FontDb::new();
        let g = shape_text(&db, &chain(&db), "Hello", 20.0, 400, false, false);
        assert_eq!(g.len(), 5);
        assert!(g.iter().all(|g| g.adv > 0.0));
        let w2: f64 = shape_text(&db, &chain(&db), "Hello", 40.0, 400, false, false).iter().map(|g| g.adv).sum();
        let w1: f64 = g.iter().map(|g| g.adv).sum();
        assert!((w2 - 2.0 * w1).abs() < 1e-6, "advances scale linearly");
    }

    #[test]
    fn empty_and_bad_sizes() {
        let db = FontDb::new();
        let c = chain(&db);
        assert!(shape_text(&db, &c, "", 10.0, 400, false, false).is_empty());
        assert!(shape_text(&db, &c, "x", f64::NAN, 400, false, false).is_empty());
        assert!(shape_text(&db, &c, "x", 0.0, 400, false, false).is_empty());
        assert!(shape_text(&db, &[], "x", 10.0, 400, false, false).is_empty());
    }

    #[test]
    fn synthetic_small_caps_shrink_lowercase() {
        let db = FontDb::new();
        let g = shape_text(&db, &chain(&db), "aB", 20.0, 400, false, true);
        if !chain(&db)[0].has_small_caps() {
            assert!((g[0].size - 16.0).abs() < 1e-9);
            assert!((g[1].size - 20.0).abs() < 1e-9);
        }
    }

    #[test]
    fn clusters_point_back_into_text() {
        let db = FontDb::new();
        let g = shape_text(&db, &chain(&db), "ab cd", 12.0, 400, false, false);
        assert_eq!(g.iter().map(|g| g.byte).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        assert!(g[2].is_space && !g[1].is_space);
    }
}

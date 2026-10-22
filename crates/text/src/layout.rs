//! Field text layout: wrapping, alignment, overflow handling and decoration lines.
//!
//! Text is shaped once at the field's font size; every later step (shrink-to-fit included) works in
//! those units and applies a single scale factor when emitting glyphs, because unhinted advances scale
//! linearly. Lines follow the CSS model: a line box of `line_height * font_size` with the glyph
//! content area centred in it (half-leading).

use std::collections::HashSet;
use std::sync::Arc;

use forge_doc::{Align, Overflow, ScriptPos, TextCase, TextProps, VAlign, Wrap};
use unicode_segmentation::UnicodeSegmentation;

use crate::fontdb::{FontDb, FontFace, FontRequest};
use crate::shape::{Glyph, shape_text};

const MIN_SHRINK_PX: f64 = 6.0;
/// Longest text laid out (untrusted spreadsheet cells can be huge).
const MAX_CHARS: usize = 20_000;
const MAX_LINES: usize = 2_000;
const ITALIC_SHEAR: f64 = 0.2;
const EPS: f64 = 1e-6;

/// A glyph to draw: baseline origin in field-box pixels.
#[derive(Clone, Debug)]
pub struct GlyphItem {
    pub face: Arc<FontFace>,
    pub gid: u32,
    pub x: f64,
    pub y: f64,
    pub size: f64,
    /// Synthetic italic: horizontal shear per unit of height above the baseline.
    pub shear: f64,
}

/// An underline or strikethrough: a filled bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    pub x0: f64,
    pub x1: f64,
    /// Centre line.
    pub y: f64,
    pub thickness: f64,
}

#[derive(Clone, Debug, Default)]
pub struct TextLayout {
    pub items: Vec<GlyphItem>,
    pub rules: Vec<Rule>,
    /// Font size actually used (smaller than requested when shrunk).
    pub font_size: f64,
    pub lines: usize,
    /// The text still did not fit (it is clipped or cut).
    pub overflowed: bool,
}

struct Line {
    glyphs: Vec<Glyph>,
    /// Last line of its paragraph (not justified unless `justifyall`).
    last: bool,
}

fn width(g: &[Glyph]) -> f64 {
    g.iter().map(|g| g.adv).sum()
}

fn trim_trailing_space(g: &mut Vec<Glyph>) {
    while g.last().is_some_and(|g| g.is_space) {
        g.pop();
    }
}

/// Indices in `g` where a break may fall without splitting a grapheme cluster.
fn break_points(g: &[Glyph], text: &str) -> HashSet<usize> {
    let starts: HashSet<usize> = text.grapheme_indices(true).map(|(i, _)| i).collect();
    (1..g.len()).filter(|&i| g[i].byte != g[i - 1].byte && starts.contains(&g[i].byte)).collect()
}

struct Shaped {
    glyphs: Vec<Glyph>,
    text: String,
}

/// Greedy wrap of one paragraph into lines no wider than `w` (in shaped units).
fn wrap_paragraph(p: &Shaped, w: f64, hyphen: &[Glyph], hyphenate: bool, out: &mut Vec<Line>) {
    let g = &p.glyphs;
    if g.is_empty() {
        out.push(Line { glyphs: vec![], last: true });
        return;
    }
    let cuts = break_points(g, &p.text);
    let hw = width(hyphen);
    let mut cur: Vec<Glyph> = Vec::new();
    let mut cur_w = 0.0;
    let mut i = 0;
    while i < g.len() {
        // A word: non-space glyphs, then the spaces after it.
        let ws = i;
        while i < g.len() && !g[i].is_space {
            i += 1;
        }
        let we = i;
        while i < g.len() && g[i].is_space {
            i += 1;
        }
        let word = &g[ws..we];
        let spaces = &g[we..i];
        let ww = width(word);
        if !cur.is_empty() && cur_w + ww > w + EPS && !word.is_empty() {
            trim_trailing_space(&mut cur);
            out.push(Line { glyphs: std::mem::take(&mut cur), last: false });
            cur_w = 0.0;
        }
        if ww > w + EPS {
            // Longer than a whole line: split between grapheme clusters.
            let mut start = 0;
            while start < word.len() {
                let rest = &word[start..];
                if width(rest) <= w + EPS {
                    cur.extend_from_slice(rest);
                    cur_w += width(rest);
                    break;
                }
                let budget = w - if hyphenate { hw } else { 0.0 };
                let mut best = None;
                let mut acc = 0.0;
                for (k, gl) in rest.iter().enumerate() {
                    if k > 0 && cuts.contains(&(start + k + ws)) && acc <= budget + EPS {
                        best = Some(k);
                    }
                    acc += gl.adv;
                    if acc > budget + EPS && best.is_some() {
                        break;
                    }
                }
                // Not even one cluster fits: take the first cluster anyway so we always progress.
                let k = best.or_else(|| (1..rest.len()).find(|&k| cuts.contains(&(start + k + ws)))).unwrap_or(rest.len());
                let mut line: Vec<Glyph> = rest[..k].to_vec();
                if hyphenate && k < rest.len() {
                    line.extend_from_slice(hyphen);
                }
                out.push(Line { glyphs: line, last: false });
                start += k;
                if out.len() >= MAX_LINES {
                    return;
                }
            }
        } else {
            cur.extend_from_slice(word);
            cur_w += ww;
        }
        cur.extend_from_slice(spaces);
        cur_w += width(spaces);
        if out.len() >= MAX_LINES {
            return;
        }
    }
    trim_trailing_space(&mut cur);
    out.push(Line { glyphs: cur, last: true });
}

/// Cuts a line to fit `w` with the ellipsis appended.
fn ellipsize(line: &mut Line, w: f64, ell: &[Glyph], text_cuts: &HashSet<usize>) {
    let ew = width(ell);
    trim_trailing_space(&mut line.glyphs);
    while !line.glyphs.is_empty() && width(&line.glyphs) + ew > w + EPS {
        // Drop one whole cluster from the end.
        let Some(last_byte) = line.glyphs.last().map(|g| g.byte) else { break };
        while line.glyphs.last().is_some_and(|g| g.byte == last_byte) {
            line.glyphs.pop();
        }
        let _ = text_cuts;
        trim_trailing_space(&mut line.glyphs);
    }
    line.glyphs.extend_from_slice(ell);
}

/// Lays `text` out in a `box_w` x `box_h` field.
pub fn layout_text(db: &FontDb, text: &str, p: &TextProps, box_w: f64, box_h: f64) -> TextLayout {
    let empty = TextLayout { font_size: p.font_size, ..Default::default() };
    if !box_w.is_finite() || !box_h.is_finite() || box_w <= 0.0 || box_h <= 0.0 || !p.font_size.is_finite() || p.font_size <= 0.0 {
        return empty;
    }
    let mut src: String = text.chars().take(MAX_CHARS).collect();
    src = src.replace("\r\n", "\n").replace('\r', "\n");
    let multi = p.wrap == Wrap::Multi;
    if !multi {
        src = src.replace('\n', " ");
    }
    if p.text_case == TextCase::Upper {
        src = src.to_uppercase();
    }
    if src.trim().is_empty() {
        return empty;
    }

    let chain = db.resolve_chain(&FontRequest::new(p.font_family.clone(), p.weight, p.italic));
    let Some(primary) = chain.first().cloned() else { return empty };
    let pos_scale = if p.script_pos == ScriptPos::None { 1.0 } else { 0.65 };
    let shape_size = p.font_size * pos_scale;
    let small = p.text_case == TextCase::SmallCaps;
    let shape = |t: &str| shape_text(db, &chain, t, shape_size, p.weight, p.italic, small);

    let paras: Vec<Shaped> = src.split('\n').take(MAX_LINES).map(|t| Shaped { glyphs: shape(t), text: t.to_string() }).collect();
    let hyphen = shape("-");
    let ell = shape("\u{2026}");

    // Metrics at the requested size.
    let asc = primary.ascent / primary.upem * p.font_size;
    let desc = primary.descent / primary.upem * p.font_size;
    let lh = (p.line_height.clamp(0.5, 5.0)) * p.font_size;
    let (shift_up, _) = match p.script_pos {
        ScriptPos::Super => (-0.35 * p.font_size, 0.0),
        ScriptPos::Sub => (0.25 * p.font_size, 0.0),
        ScriptPos::None => (0.0, 0.0),
    };

    let build = |scale: f64| -> (Vec<Line>, bool) {
        let w = box_w / scale;
        let mut lines: Vec<Line> = Vec::new();
        if multi {
            for para in &paras {
                wrap_paragraph(para, w, &hyphen, p.hyphenate, &mut lines);
                if lines.len() >= MAX_LINES {
                    break;
                }
            }
        } else {
            let mut g: Vec<Glyph> = paras.iter().flat_map(|x| x.glyphs.iter().cloned()).collect();
            trim_trailing_space(&mut g);
            lines.push(Line { glyphs: g, last: true });
        }
        (lines, false)
    };

    let limit_for = |scale: f64| -> usize {
        if !multi {
            return 1;
        }
        if p.max_lines > 0 {
            return p.max_lines as usize;
        }
        match p.overflow {
            Overflow::Ellipsis => ((box_h / (lh * scale)).floor() as usize).max(1),
            _ => usize::MAX,
        }
    };

    let fits = |lines: &[Line], scale: f64| -> bool {
        let wide = lines.iter().all(|l| width(&l.glyphs) * scale <= box_w + 1e-3);
        let tall = lines.len() as f64 * lh * scale <= box_h + 1e-3;
        let few = lines.len() <= if multi && p.max_lines > 0 { p.max_lines as usize } else { usize::MAX };
        wide && few && (tall || !multi)
    };

    let mut scale = 1.0;
    let (mut lines, _) = build(scale);
    if p.overflow == Overflow::Shrink {
        let mut size = p.font_size;
        let floor = MIN_SHRINK_PX.min(p.font_size);
        while !fits(&lines, scale) && size > floor + EPS {
            size = (size.ceil() - 1.0).max(floor);
            scale = size / p.font_size;
            lines = build(scale).0;
        }
    }

    let mut overflowed = !fits(&lines, scale);
    let limit = limit_for(scale);
    if lines.len() > limit {
        lines.truncate(limit);
        overflowed = true;
        if p.overflow == Overflow::Ellipsis
            && let Some(l) = lines.last_mut()
        {
            // Force the ellipsis even if the last kept line fits.
            let w = box_w / scale;
            let mut probe = Line { glyphs: l.glyphs.clone(), last: true };
            probe.glyphs.extend_from_slice(&ell);
            if width(&probe.glyphs) > w + EPS {
                ellipsize(l, w, &ell, &HashSet::new());
            } else {
                l.glyphs.extend_from_slice(&ell);
            }
            l.last = true;
        }
    } else if p.overflow == Overflow::Ellipsis {
        let w = box_w / scale;
        for l in lines.iter_mut() {
            if width(&l.glyphs) > w + EPS {
                ellipsize(l, w, &ell, &HashSet::new());
                overflowed = true;
            }
        }
    }

    // Emit.
    let w_units = box_w / scale;
    let n = lines.len();
    let line_px = lh * scale;
    let total = n as f64 * line_px;
    let top = match p.v_align {
        VAlign::Top => 0.0,
        VAlign::Middle => (box_h - total) / 2.0,
        VAlign::Bottom => box_h - total,
    };
    let half_leading = (lh - (asc + desc)) / 2.0;
    let mut out = TextLayout { font_size: p.font_size * scale, lines: n, overflowed, ..Default::default() };
    for (li, line) in lines.iter().enumerate() {
        let baseline = top + li as f64 * line_px + (half_leading + asc) * scale;
        let lw = width(&line.glyphs);
        let justify = match p.align {
            Align::Justify => !line.last,
            Align::JustifyAll => true,
            _ => false,
        };
        let spaces = line.glyphs.iter().filter(|g| g.is_space).count();
        let extra = if justify && spaces > 0 && lw < w_units { (w_units - lw) / spaces as f64 } else { 0.0 };
        let lw_final = lw + extra * spaces as f64;
        let x0 = match p.align {
            Align::Center => (w_units - lw_final) / 2.0,
            Align::Right => w_units - lw_final,
            _ => 0.0,
        };
        let mut pen = x0;
        for g in &line.glyphs {
            if g.gid != 0 || !g.is_space {
                out.items.push(GlyphItem {
                    face: g.face.clone(),
                    gid: g.gid,
                    x: (pen + g.dx) * scale,
                    y: baseline + (g.dy + shift_up) * scale,
                    size: g.size * scale,
                    shear: if p.italic && !g.face.italic { ITALIC_SHEAR } else { 0.0 },
                });
            }
            pen += g.adv + if g.is_space { extra } else { 0.0 };
        }
        if lw_final > 0.0 && (p.underline || p.strikethrough) {
            let size = p.font_size * scale;
            let thickness = (size * 0.06).max(1.0);
            let (xa, xb) = (x0 * scale, (x0 + lw_final) * scale);
            if p.underline {
                out.rules.push(Rule { x0: xa, x1: xb, y: baseline + 0.12 * size + shift_up * scale, thickness });
            }
            if p.strikethrough {
                out.rules.push(Rule { x0: xa, x1: xb, y: baseline - 0.3 * size + shift_up * scale, thickness });
            }
        }
    }
    out
}

/// Width in pixels of `text` on one line (no wrapping), at the field's font settings.
pub fn text_width(db: &FontDb, text: &str, p: &TextProps) -> f64 {
    let chain = db.resolve_chain(&FontRequest::new(p.font_family.clone(), p.weight, p.italic));
    let one: String = text.chars().take(MAX_CHARS).map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    width(&shape_text(db, &chain, &one, p.font_size, p.weight, p.italic, p.text_case == TextCase::SmallCaps))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props() -> TextProps {
        TextProps { font_family: "Stem".into(), font_size: 20.0, ..TextProps::default() }
    }

    fn xs(l: &TextLayout) -> (f64, f64) {
        let lo = l.items.iter().map(|i| i.x).fold(f64::MAX, f64::min);
        let hi = l.items.iter().map(|i| i.x).fold(f64::MIN, f64::max);
        (lo, hi)
    }

    #[test]
    fn single_line_aligns() {
        let db = FontDb::new();
        let mut p = props();
        let left = layout_text(&db, "Hi", &p, 200.0, 40.0);
        p.align = Align::Right;
        let right = layout_text(&db, "Hi", &p, 200.0, 40.0);
        p.align = Align::Center;
        let centre = layout_text(&db, "Hi", &p, 200.0, 40.0);
        assert!(xs(&left).0 < 1.0);
        assert!(xs(&right).1 > 170.0);
        let c = (xs(&centre).0 + xs(&centre).1) / 2.0;
        assert!((c - 100.0).abs() < 15.0);
    }

    #[test]
    fn shrink_fits_width_with_integer_steps() {
        let db = FontDb::new();
        let mut p = props();
        p.overflow = Overflow::Shrink;
        let l = layout_text(&db, "A fairly long name that cannot fit", &p, 100.0, 40.0);
        assert!(l.font_size < 20.0 && l.font_size >= 6.0);
        assert_eq!(l.font_size.fract(), 0.0);
        let right = l.items.iter().map(|i| i.x).fold(0.0, f64::max);
        assert!(right <= 100.0 + 1e-3);
    }

    #[test]
    fn wraps_and_respects_max_lines_with_ellipsis() {
        let db = FontDb::new();
        let mut p = props();
        p.wrap = Wrap::Multi;
        let text = "one two three four five six seven eight nine ten eleven twelve";
        let all = layout_text(&db, text, &p, 120.0, 400.0);
        assert!(all.lines > 2);
        p.max_lines = 2;
        p.overflow = Overflow::Ellipsis;
        let cut = layout_text(&db, text, &p, 120.0, 400.0);
        assert_eq!(cut.lines, 2);
        assert!(cut.overflowed);
        let right = cut.items.iter().map(|i| i.x).fold(0.0, f64::max);
        assert!(right <= 120.0);
    }

    #[test]
    fn long_word_splits_and_hyphenates() {
        let db = FontDb::new();
        let mut p = props();
        p.wrap = Wrap::Multi;
        p.hyphenate = true;
        let l = layout_text(&db, "Pneumonoultramicroscopicsilicovolcanoconiosis", &p, 100.0, 400.0);
        assert!(l.lines >= 3);
        // Nothing runs past the box.
        assert!(l.items.iter().all(|i| i.x <= 100.0 + 1e-3));
    }

    #[test]
    fn explicit_newlines_in_multi_not_single() {
        let db = FontDb::new();
        let mut p = props();
        p.wrap = Wrap::Multi;
        assert_eq!(layout_text(&db, "a\nb\nc", &p, 200.0, 200.0).lines, 3);
        p.wrap = Wrap::Single;
        assert_eq!(layout_text(&db, "a\nb\nc", &p, 200.0, 200.0).lines, 1);
    }

    #[test]
    fn justify_fills_width() {
        let db = FontDb::new();
        let mut p = props();
        p.wrap = Wrap::Multi;
        p.align = Align::Justify;
        let l = layout_text(&db, "aa bb cc dd ee ff gg hh ii jj kk ll", &p, 150.0, 400.0);
        assert!(l.lines >= 2);
        // First line's last glyph reaches (nearly) the right edge.
        let first_y = l.items[0].y;
        let hi = l.items.iter().filter(|i| (i.y - first_y).abs() < 0.5).map(|i| i.x + i.size * 0.3).fold(0.0, f64::max);
        assert!(hi > 130.0, "justified first line reaches the right edge: {hi}");
    }

    #[test]
    fn vertical_alignment_orders_baselines() {
        let db = FontDb::new();
        let mut p = props();
        p.v_align = VAlign::Top;
        let top = layout_text(&db, "x", &p, 100.0, 100.0).items[0].y;
        p.v_align = VAlign::Middle;
        let mid = layout_text(&db, "x", &p, 100.0, 100.0).items[0].y;
        p.v_align = VAlign::Bottom;
        let bot = layout_text(&db, "x", &p, 100.0, 100.0).items[0].y;
        assert!(top < mid && mid < bot);
    }

    #[test]
    fn decorations_and_degenerate_inputs() {
        let db = FontDb::new();
        let mut p = props();
        p.underline = true;
        p.strikethrough = true;
        assert_eq!(layout_text(&db, "text", &p, 100.0, 50.0).rules.len(), 2);
        assert!(layout_text(&db, "", &p, 100.0, 50.0).items.is_empty());
        assert!(layout_text(&db, "x", &p, 0.0, 50.0).items.is_empty());
        assert!(layout_text(&db, "x", &p, f64::NAN, 50.0).items.is_empty());
        p.font_size = 0.0;
        assert!(layout_text(&db, "x", &p, 10.0, 10.0).items.is_empty());
    }

    #[test]
    fn huge_text_is_bounded() {
        let db = FontDb::new();
        let mut p = props();
        p.wrap = Wrap::Multi;
        let big = "word ".repeat(100_000);
        let l = layout_text(&db, &big, &p, 100.0, 100.0);
        assert!(l.lines <= MAX_LINES);
    }
}

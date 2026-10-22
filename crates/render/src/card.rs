//! Card rendering: template + photo fields + text fields -> RGBA pixels.
//!
//! Layer order matches the original app: photos first, then the template image over them (so a
//! template with a transparent window shows the photo through it), then text.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use forge_doc::{CardRef, Design, EffectiveCrop, Field, FieldKind, PhotoEntry, PhotoProps, Project, Shape, Table, TextProps, placement};
use forge_geom::Rect;
use forge_image::{Image, Quality, resize_region};
use forge_text::{FontDb, layout_text};
use kurbo::{Affine, BezPath, Ellipse, Shape as _, Stroke};
use vello_cpu::peniko::{self, ImageQuality, ImageSampler};
use vello_cpu::{Pixmap, RenderContext, RenderSettings, Resources};

use crate::pixmap::{MAX_SIDE, image_to_pixmap, pixmap_to_image};
use crate::store::ImageStore;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("the design has no template image")]
    NoTemplate,
    #[error("template image: {0}")]
    Template(String),
    #[error("card size {0} x {1} is out of range")]
    Size(u32, u32),
    #[error("{0}")]
    Other(String),
}

#[derive(Clone, Copy, Debug)]
pub struct RenderOpts {
    /// Show `{{Field}}` for unmapped text fields (editor preview) instead of nothing (export).
    pub placeholders: bool,
    /// Keep decoded photos in the cache (editor) or not (batch export).
    pub cache: bool,
}

impl RenderOpts {
    pub const PREVIEW: Self = Self { placeholders: true, cache: true };
    pub const EXPORT: Self = Self { placeholders: false, cache: false };
}

/// Draws cards. Cheap to share between threads (`&Renderer` is `Sync`).
pub struct Renderer {
    pub fonts: Arc<FontDb>,
    pub store: ImageStore,
    templates: Mutex<HashMap<String, Arc<Pixmap>>>,
}

fn rgba_color(c: forge_color::Rgba) -> peniko::Color {
    peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn rect_path(w: f64, h: f64) -> BezPath {
    kurbo::Rect::new(0.0, 0.0, w, h).to_path(0.1)
}

/// Resamples the part of an image that lands inside a `bw` x `bh` box (template units) to device
/// pixels. Returns the pixels and their device offset inside the box.
fn resample_visible(img: &Image, dest: Rect, bw: f64, bh: f64, sx: f64, sy: f64) -> Option<(Image, f64, f64)> {
    let vis = dest.intersect(&Rect::new(0.0, 0.0, bw, bh))?;
    if dest.w <= 0.0 || dest.h <= 0.0 {
        return None;
    }
    let (x0, y0) = ((vis.x * sx).round(), (vis.y * sy).round());
    let (x1, y1) = ((vis.right() * sx).round(), (vis.bottom() * sy).round());
    let (pw, ph) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    if pw > f64::from(MAX_SIDE) || ph > f64::from(MAX_SIDE) {
        return None;
    }
    let (iw, ih) = (f64::from(img.width), f64::from(img.height));
    let src = Rect::new((vis.x - dest.x) / dest.w * iw, (vis.y - dest.y) / dest.h * ih, vis.w / dest.w * iw, vis.h / dest.h * ih);
    let out = resize_region(img, src, pw as u32, ph as u32, Quality::High).ok()?;
    Some((out, x0, y0))
}

impl Renderer {
    pub fn new(fonts: Arc<FontDb>) -> Self {
        Self { fonts, store: ImageStore::new(384 << 20), templates: Mutex::new(HashMap::new()) }
    }

    fn template_pixmap(&self, design: &Design, w: u32, h: u32, cache: bool) -> Result<Arc<Pixmap>, RenderError> {
        let tpl = design.template.as_ref().ok_or(RenderError::NoTemplate)?;
        let id = match &tpl.src {
            forge_doc::ImageSource::Path(p) => format!("p:{}", p.display()),
            forge_doc::ImageSource::Data(d) => format!("d:{:p}:{}", Arc::as_ptr(d), d.len()),
            forge_doc::ImageSource::None => return Err(RenderError::NoTemplate),
        };
        let key = format!("{id}@{w}x{h}");
        if let Some(p) = self.templates.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Ok(p.clone());
        }
        let img = self.store.load(&tpl.src, cache).map_err(RenderError::Template)?;
        let fitted = if img.width == w && img.height == h {
            (*img).clone()
        } else {
            resize_region(&img, Rect::new(0.0, 0.0, f64::from(img.width), f64::from(img.height)), w, h, Quality::High)
                .map_err(|e| RenderError::Template(e.to_string()))?
        };
        let pm = Arc::new(image_to_pixmap(&fitted).ok_or(RenderError::Size(w, h))?);
        let mut g = self.templates.lock().unwrap_or_else(|e| e.into_inner());
        if g.len() >= 6 {
            g.clear();
        }
        g.insert(key, pm.clone());
        Ok(pm)
    }

    /// Renders data row `row` of `project` at the template's own size.
    pub fn render_row(&self, project: &Project, row: usize, opts: RenderOpts) -> Result<Image, RenderError> {
        let tpl = project.design.template.as_ref().ok_or(RenderError::NoTemplate)?;
        self.render_row_sized(project, row, tpl.w.max(1), tpl.h.max(1), opts)
    }

    /// Renders data row `row` into exactly `w` x `h` pixels (vector scaling: text stays sharp).
    pub fn render_row_sized(&self, project: &Project, row: usize, w: u32, h: u32, opts: RenderOpts) -> Result<Image, RenderError> {
        let photo = project.photo_for_row(row).map(|p| p.as_ref());
        self.render_design(&project.design, &project.data, Some(row), photo, w, h, opts)
    }

    /// Renders any card reference at `w` x `h` pixels.
    pub fn render_card(&self, project: &Project, card: &CardRef, w: u32, h: u32, opts: RenderOpts) -> Result<Image, RenderError> {
        match card {
            CardRef::Row(i) => self.render_row_sized(project, *i, w, h, opts),
            CardRef::Photo(k) => {
                let entry = project.photo(k).ok_or_else(|| RenderError::Other(format!("photo {k} is missing")))?;
                let img = self.store.load(&entry.source, opts.cache).map_err(RenderError::Other)?;
                let crop = entry.effective_crop(f64::from(w) / f64::from(h.max(1)), None);
                Ok(cover_image(&img, &crop, w, h))
            }
            CardRef::File(i) => {
                let forge_doc::CardSource::Files(files) = &project.card_source else { return Err(RenderError::Other("no imported cards".into())) };
                let (_, src) = files.get(*i).ok_or_else(|| RenderError::Other("card file index out of range".into()))?;
                let img = self.store.load(src, opts.cache).map_err(RenderError::Other)?;
                if img.width == w && img.height == h {
                    return Ok((*img).clone());
                }
                resize_region(&img, Rect::new(0.0, 0.0, f64::from(img.width), f64::from(img.height)), w, h, Quality::High)
                    .map_err(|e| RenderError::Other(e.to_string()))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_design(
        &self,
        design: &Design,
        table: &Table,
        row: Option<usize>,
        photo: Option<&PhotoEntry>,
        out_w: u32,
        out_h: u32,
        opts: RenderOpts,
    ) -> Result<Image, RenderError> {
        let tpl = design.template.as_ref().ok_or(RenderError::NoTemplate)?;
        if out_w == 0 || out_h == 0 || out_w > MAX_SIDE || out_h > MAX_SIDE || u64::from(out_w) * u64::from(out_h) > 400_000_000 {
            return Err(RenderError::Size(out_w, out_h));
        }
        let (tw, th) = (f64::from(tpl.w.max(1)), f64::from(tpl.h.max(1)));
        let (sx, sy) = (f64::from(out_w) / tw, f64::from(out_h) / th);
        let base = Affine::scale_non_uniform(sx, sy);
        // Single-threaded context: cards are rendered in parallel one level up.
        let mut ctx = RenderContext::new_with(out_w as u16, out_h as u16, RenderSettings { num_threads: 0, ..Default::default() });

        if let Some(entry) = photo {
            for f in design.photo_fields() {
                if let FieldKind::Photo(props) = &f.kind {
                    self.draw_photo(&mut ctx, base, f, props, entry, tw, th, sx, sy, opts);
                }
            }
        }
        let tp = self.template_pixmap(design, out_w, out_h, opts.cache)?;
        ctx.reset_transform();
        ctx.reset_paint_transform();
        ctx.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(tp), sampler: ImageSampler::default() });
        ctx.fill_rect(&kurbo::Rect::new(0.0, 0.0, f64::from(out_w), f64::from(out_h)));

        for f in design.text_fields() {
            if let FieldKind::Text(props) = &f.kind {
                let text = match row {
                    Some(r) if opts.placeholders => design.preview_text_for(table, r, f),
                    Some(r) => design.text_for(table, r, f),
                    None => String::new(),
                };
                self.draw_text(&mut ctx, base, f, props, &text, tw, th);
            }
        }
        ctx.flush();
        let mut pm = Pixmap::new(out_w as u16, out_h as u16);
        ctx.render(&mut pm, &mut Resources::new());
        Ok(pixmap_to_image(&pm))
    }

    /// Transform of a field: template px -> device, rotated about the field's centre.
    fn field_xf(base: Affine, f: &Field, tw: f64, th: f64) -> (Affine, f64, f64) {
        let (x, y, w, h) = (f.x / 100.0 * tw, f.y / 100.0 * th, f.w / 100.0 * tw, f.h / 100.0 * th);
        let rot = if f.rotation != 0.0 && f.rotation.is_finite() {
            Affine::rotate_about(f.rotation.to_radians(), kurbo::Point::new(x + w / 2.0, y + h / 2.0))
        } else {
            Affine::IDENTITY
        };
        (base * rot * Affine::translate((x, y)), w, h)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text(&self, ctx: &mut RenderContext, base: Affine, f: &Field, p: &TextProps, text: &str, tw: f64, th: f64) {
        let (xf, w, h) = Self::field_xf(base, f, tw, th);
        if text.trim().is_empty() || w <= 0.0 || h <= 0.0 {
            return;
        }
        let lay = layout_text(&self.fonts, text, p, w, h);
        if lay.items.is_empty() && lay.rules.is_empty() {
            return;
        }
        let opacity = (f.opacity / 100.0).clamp(0.0, 1.0);
        ctx.set_transform(xf);
        if opacity < 1.0 {
            ctx.push_layer(None, None, Some(opacity as f32), None, None);
        }
        ctx.push_clip_layer(&rect_path(w, h));
        ctx.set_paint(rgba_color(p.color));
        ctx.set_fill_rule(peniko::Fill::NonZero);
        let mut path = BezPath::new();
        for it in &lay.items {
            let outline = it.face.outline(it.gid);
            if outline.elements().is_empty() {
                continue;
            }
            let k = it.size / it.face.upem;
            let glyph_xf = Affine::new([k, 0.0, -it.shear * k, k, it.x, it.y]);
            let mut g = (*outline).clone();
            g.apply_affine(glyph_xf);
            path.extend(g.elements().iter().copied());
        }
        if !path.elements().is_empty() {
            ctx.fill_path(&path);
        }
        for r in &lay.rules {
            ctx.fill_rect(&kurbo::Rect::new(r.x0, r.y - r.thickness / 2.0, r.x1, r.y + r.thickness / 2.0));
        }
        ctx.pop_layer();
        if opacity < 1.0 {
            ctx.pop_layer();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_photo(
        &self,
        ctx: &mut RenderContext,
        base: Affine,
        f: &Field,
        props: &PhotoProps,
        entry: &PhotoEntry,
        tw: f64,
        th: f64,
        sx: f64,
        sy: f64,
        opts: RenderOpts,
    ) {
        let (xf, w, h) = Self::field_xf(base, f, tw, th);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let img = match self.store.load(&entry.source, opts.cache) {
            Ok(i) => i,
            Err(e) => {
                log::warn!("photo {}: {e}", entry.name);
                return;
            }
        };
        let crop = entry.effective_crop(w / h, Some(props));
        let dest = placement(f64::from(img.width), f64::from(img.height), w, h, &crop).dest;
        let Some((small, dx, dy)) = resample_visible(&img, dest, w, h, sx, sy) else { return };
        let Some(pm) = image_to_pixmap(&small) else { return };
        let (pw, ph) = (f64::from(small.width), f64::from(small.height));

        let opacity = (f.opacity / 100.0).clamp(0.0, 1.0);
        let shape: BezPath = if props.shape == Shape::Circle {
            let r = w.min(h) / 2.0;
            Ellipse::new((w / 2.0, h / 2.0), (r, r), 0.0).to_path(0.1)
        } else {
            rect_path(w, h)
        };
        ctx.set_transform(xf);
        if opacity < 1.0 {
            ctx.push_layer(None, None, Some(opacity as f32), None, None);
        }
        ctx.push_clip_layer(&shape);
        if matches!(crop, EffectiveCrop::Auto(_)) {
            // The auto-crop may reach outside the photo: the original padded with white.
            ctx.set_paint(peniko::Color::WHITE);
            ctx.fill_rect(&kurbo::Rect::new(0.0, 0.0, w, h));
        }
        // The pixels are already at device resolution: draw them 1:1.
        let (lx, ly) = (dx / sx, dy / sy);
        ctx.set_paint(vello_cpu::Image {
            image: vello_cpu::ImageSource::Pixmap(Arc::new(pm)),
            sampler: ImageSampler { quality: ImageQuality::Medium, ..Default::default() },
        });
        ctx.set_paint_transform(Affine::translate((lx, ly)) * Affine::scale_non_uniform(1.0 / sx, 1.0 / sy));
        ctx.fill_rect(&kurbo::Rect::new(lx, ly, lx + pw / sx, ly + ph / sy));
        ctx.reset_paint_transform();
        ctx.pop_layer();

        if props.border_w > 0.0 {
            let bw = props.border_w;
            let outline: BezPath = if props.shape == Shape::Circle {
                let r = (w.min(h) / 2.0 - bw / 2.0).max(0.0);
                Ellipse::new((w / 2.0, h / 2.0), (r, r), 0.0).to_path(0.1)
            } else {
                kurbo::Rect::new(bw / 2.0, bw / 2.0, w - bw / 2.0, h - bw / 2.0).to_path(0.1)
            };
            ctx.set_paint(rgba_color(props.border_color));
            ctx.set_stroke(Stroke::new(bw));
            ctx.stroke_path(&outline);
        }
        if opacity < 1.0 {
            ctx.pop_layer();
        }
    }
}

/// `img` cropped to `w` x `h` as the crop says, uncovered area white (a card made of just a photo).
pub fn cover_image(img: &Image, crop: &EffectiveCrop, w: u32, h: u32) -> Image {
    let (w, h) = (w.max(1), h.max(1));
    let mut out = Image { width: w, height: h, rgba: [255u8, 255, 255, 255].repeat(w as usize * h as usize) };
    let dest = placement(f64::from(img.width), f64::from(img.height), f64::from(w), f64::from(h), crop).dest;
    if let Some((small, dx, dy)) = resample_visible(img, dest, f64::from(w), f64::from(h), 1.0, 1.0) {
        blit_over(&mut out, &small, dx as i64, dy as i64);
    }
    out
}

/// Draws `src` over `dst` at (`x`, `y`) with normal alpha blending (clipped to `dst`).
pub fn blit_over(dst: &mut Image, src: &Image, x: i64, y: i64) {
    let (dw, dh, sw, sh) = (dst.width as i64, dst.height as i64, src.width as i64, src.height as i64);
    for sy in 0..sh {
        let dy = y + sy;
        if dy < 0 || dy >= dh {
            continue;
        }
        let x0 = (-x).max(0);
        let x1 = sw.min(dw - x);
        if x1 <= x0 {
            continue;
        }
        let srow = &src.rgba[(sy * sw + x0) as usize * 4..(sy * sw + x1) as usize * 4];
        let drow = &mut dst.rgba[(dy * dw + x + x0) as usize * 4..(dy * dw + x + x1) as usize * 4];
        for (d, s) in drow.chunks_exact_mut(4).zip(srow.chunks_exact(4)) {
            let a = u32::from(s[3]);
            match a {
                255 => d.copy_from_slice(s),
                0 => {}
                _ => {
                    let da = u32::from(d[3]);
                    let oa = a + da * (255 - a) / 255;
                    if oa == 0 {
                        continue;
                    }
                    for c in 0..3 {
                        let v = (u32::from(s[c]) * a * 255 + u32::from(d[c]) * da * (255 - a)) / (oa * 255);
                        d[c] = v.min(255) as u8;
                    }
                    d[3] = oa.min(255) as u8;
                }
            }
        }
    }
}

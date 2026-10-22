//! The font catalogue.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use kurbo::BezPath;
use rayon::prelude::*;
use skrifa::instance::{Location, LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::FileRef;
use skrifa::string::StringId;
use skrifa::{GlyphId, MetadataProvider, Tag};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Largest font file read (a CJK collection can be 100 MB; anything bigger is not a font we need).
const MAX_FONT_FILE: u64 = 150 << 20;

/// What the caller wants: a CSS-style family list ("'Stem Bold', Georgia, serif"), weight, italic.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontRequest {
    pub family: String,
    pub weight: u32,
    pub italic: bool,
}

impl FontRequest {
    pub fn new(family: impl Into<String>, weight: u32, italic: bool) -> Self {
        Self { family: family.into(), weight, italic }
    }
}

/// Splits a CSS font-family list into family names (quotes removed, empties dropped).
pub fn parse_family_list(s: &str) -> Vec<String> {
    s.split(',').map(|p| p.trim().trim_matches(|c| c == '\'' || c == '"').trim().to_string()).filter(|p| !p.is_empty()).collect()
}

fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

/// A catalogued face, as shown in font pickers.
#[derive(Clone, Debug)]
pub struct FaceInfo {
    pub family: String,
    pub style: String,
    pub weight: u16,
    pub italic: bool,
}

enum Source {
    Static(&'static [u8]),
    Owned(Arc<Vec<u8>>),
    Path(PathBuf),
}

struct Entry {
    family: String,
    style: String,
    weight: u16,
    italic: bool,
    /// `wght` axis range of a variable font.
    wght: Option<(f32, f32)>,
    /// Lower-cased names the face answers to (families, full name, PostScript name).
    keys: Vec<String>,
    source: Source,
    index: u32,
    /// Loaded faces by effective weight.
    loaded: Mutex<HashMap<u16, Option<Arc<FontFace>>>>,
}

/// A loaded face: the font file's bytes plus what shaping and drawing need.
pub struct FontFace {
    pub id: u64,
    pub family: String,
    pub style: String,
    pub weight: u16,
    pub italic: bool,
    data: Arc<Vec<u8>>,
    index: u32,
    pub upem: f64,
    /// Positive, in font units.
    pub ascent: f64,
    /// Positive, in font units.
    pub descent: f64,
    pub(crate) shaper: harfrust::ShaperData,
    pub(crate) instance: Option<harfrust::ShaperInstance>,
    location: Location,
    smcp: bool,
    outlines: Mutex<HashMap<u32, Arc<BezPath>>>,
}

impl std::fmt::Debug for FontFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FontFace({} {})", self.family, self.style)
    }
}

const OUTLINE_CACHE_MAX: usize = 20_000;

impl FontFace {
    pub(crate) fn skrifa(&self) -> Option<skrifa::FontRef<'_>> {
        skrifa::FontRef::from_index(&self.data, self.index).ok()
    }
    pub(crate) fn hb(&self) -> Option<harfrust::FontRef<'_>> {
        harfrust::FontRef::from_index(&self.data, self.index).ok()
    }
    /// Does the face map `c` to a glyph?
    pub fn covers(&self, c: char) -> bool {
        self.skrifa().is_some_and(|f| f.charmap().map(c).is_some())
    }
    /// Has an OpenType `smcp` (small capitals) feature.
    pub fn has_small_caps(&self) -> bool {
        self.smcp
    }
    /// The glyph outline in font units, y-down, cached per glyph.
    pub fn outline(&self, gid: u32) -> Arc<BezPath> {
        if let Some(p) = self.outlines.lock().unwrap_or_else(|e| e.into_inner()).get(&gid) {
            return p.clone();
        }
        let mut pen = FlipPen(BezPath::new());
        if let Some(f) = self.skrifa()
            && let Some(g) = f.outline_glyphs().get(GlyphId::new(gid))
        {
            // A glyph that fails to draw is just left empty.
            let _ = g.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::from(&self.location)), &mut pen);
        }
        let p = Arc::new(pen.0);
        let mut cache = self.outlines.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= OUTLINE_CACHE_MAX {
            cache.clear();
        }
        cache.insert(gid, p.clone());
        p
    }
}

struct FlipPen(BezPath);

impl OutlinePen for FlipPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((x as f64, -y as f64));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((x as f64, -y as f64));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to((cx0 as f64, -cy0 as f64), (x as f64, -y as f64));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to((cx0 as f64, -cy0 as f64), (cx1 as f64, -cy1 as f64), (x as f64, -y as f64));
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}

fn name_of(font: &skrifa::FontRef<'_>, ids: &[StringId]) -> Option<String> {
    ids.iter().find_map(|id| font.localized_strings(*id).english_or_first().map(|s| s.to_string()).filter(|s| !s.is_empty()))
}

/// Catalogue entries of every face in a font file.
fn entries_of(data: &[u8], source: impl Fn(u32) -> Source) -> Vec<Entry> {
    let Ok(file) = FileRef::new(data) else { return vec![] };
    let count = match &file {
        FileRef::Font(_) => 1,
        FileRef::Collection(c) => c.len(),
    };
    let mut out = Vec::new();
    for index in 0..count {
        let Ok(font) = skrifa::FontRef::from_index(data, index) else { continue };
        let Some(family) = name_of(&font, &[StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]) else { continue };
        let style = name_of(&font, &[StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME]).unwrap_or_else(|| "Regular".into());
        let attrs = font.attributes();
        let weight = attrs.weight.value().round().clamp(1.0, 1000.0) as u16;
        let italic = !matches!(attrs.style, skrifa::attribute::Style::Normal);
        let wght = font.axes().iter().find(|a| a.tag() == Tag::new(b"wght")).map(|a| (a.min_value(), a.max_value()));
        let mut keys = vec![norm(&family)];
        for id in [StringId::FAMILY_NAME, StringId::FULL_NAME, StringId::POSTSCRIPT_NAME] {
            for s in font.localized_strings(id).map(|s| s.to_string()) {
                let k = norm(&s);
                if !k.is_empty() && !keys.contains(&k) {
                    keys.push(k);
                }
            }
        }
        out.push(Entry { family, style, weight, italic, wght, keys, source: source(index), index, loaded: Mutex::new(HashMap::new()) });
    }
    out
}

fn load_face(entry: &Entry, weight: u16) -> Option<Arc<FontFace>> {
    let data: Arc<Vec<u8>> = match &entry.source {
        Source::Static(b) => Arc::new(b.to_vec()),
        Source::Owned(d) => d.clone(),
        Source::Path(p) => {
            if std::fs::metadata(p).ok()?.len() > MAX_FONT_FILE {
                return None;
            }
            Arc::new(std::fs::read(p).ok()?)
        }
    };
    let f = skrifa::FontRef::from_index(&data, entry.index).ok()?;
    let variations: Vec<(Tag, f32)> = match entry.wght {
        Some((lo, hi)) => vec![(Tag::new(b"wght"), (weight as f32).clamp(lo, hi))],
        None => vec![],
    };
    let location = if variations.is_empty() { Location::default() } else { f.axes().location(variations.iter().copied()) };
    let m = f.metrics(Size::unscaled(), &location);
    let hb = harfrust::FontRef::from_index(&data, entry.index).ok()?;
    let shaper = harfrust::ShaperData::new(&hb);
    let instance = (!variations.is_empty())
        .then(|| harfrust::ShaperInstance::from_variations(&hb, variations.iter().map(|(t, v)| (harfrust::Tag::new(&t.to_be_bytes()), *v))));
    let smcp = {
        use skrifa::raw::TableProvider;
        f.gsub().ok().and_then(|g| g.feature_list().ok()).is_some_and(|fl| fl.feature_records().iter().any(|r| r.feature_tag() == Tag::new(b"smcp")))
    };
    Some(Arc::new(FontFace {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        family: entry.family.clone(),
        style: entry.style.clone(),
        weight: if entry.wght.is_some() { weight } else { entry.weight },
        italic: entry.italic,
        data,
        index: entry.index,
        upem: (m.units_per_em as f64).max(1.0),
        ascent: m.ascent as f64,
        descent: -(m.descent as f64),
        shaper,
        instance,
        location,
        smcp,
        outlines: Mutex::new(HashMap::new()),
    }))
}

#[derive(Default)]
struct Catalog {
    entries: Vec<Arc<Entry>>,
    by_key: HashMap<String, Vec<usize>>,
}

impl Catalog {
    fn push(&mut self, e: Entry) {
        let i = self.entries.len();
        for k in &e.keys {
            self.by_key.entry(k.clone()).or_default().push(i);
        }
        self.entries.push(Arc::new(e));
    }
}

type FallbackKey = (char, u16, bool);

/// The font catalogue: bundled fonts, system fonts and fonts the user added.
pub struct FontDb {
    catalog: RwLock<Catalog>,
    /// char -> face that covers it, for fallback (`None` = nothing covers it).
    fallback: Mutex<HashMap<FallbackKey, Option<Arc<FontFace>>>>,
    scanned: std::sync::atomic::AtomicBool,
}

impl Default for FontDb {
    fn default() -> Self {
        Self::new()
    }
}

impl FontDb {
    /// A catalogue with the bundled fonts only. Call [`FontDb::scan_system_fonts`] to add the
    /// system's (it can take a second or two on a machine with many fonts: run it on a thread).
    pub fn new() -> Self {
        let db = Self { catalog: RwLock::new(Catalog::default()), fallback: Mutex::new(HashMap::new()), scanned: false.into() };
        for &(bytes, family, weight, italic) in crate::bundled::FONTS {
            let mut es = entries_of(bytes, |_| Source::Static(bytes));
            for e in &mut es {
                e.family = family.to_string();
                e.style = if italic { "Italic".into() } else { "Regular".into() };
                e.weight = weight;
                e.italic = italic;
                e.keys = vec![norm(family)];
            }
            db.add_entries(es);
        }
        db
    }

    /// A catalogue with no fonts at all (tests).
    pub fn empty() -> Self {
        Self { catalog: RwLock::new(Catalog::default()), fallback: Mutex::new(HashMap::new()), scanned: false.into() }
    }

    fn add_entries(&self, es: Vec<Entry>) {
        let mut c = self.catalog.write().unwrap_or_else(|e| e.into_inner());
        for e in es {
            c.push(e);
        }
        drop(c);
        self.fallback.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// Adds a font file (`.ttf` `.otf` `.ttc` `.otc`). Returns how many faces it holds.
    pub fn add_file(&self, path: &Path) -> usize {
        let Ok(meta) = std::fs::metadata(path) else { return 0 };
        if meta.len() > MAX_FONT_FILE {
            return 0;
        }
        let Ok(data) = std::fs::read(path) else { return 0 };
        let es = entries_of(&data, |_| Source::Path(path.to_path_buf()));
        let n = es.len();
        self.add_entries(es);
        n
    }

    /// Adds font bytes (a font the user dropped in).
    pub fn add_bytes(&self, bytes: Vec<u8>) -> usize {
        let data = Arc::new(bytes);
        let es = entries_of(&data, |_| Source::Owned(data.clone()));
        let n = es.len();
        self.add_entries(es);
        n
    }

    /// Scans the platform's font folders. Safe to call from any thread; call once.
    pub fn scan_system_fonts(&self) {
        if self.scanned.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut files = Vec::new();
        for d in system_font_dirs() {
            collect_fonts(&d, 0, &mut files);
        }
        let found: Vec<Entry> = files
            .par_iter()
            .flat_map_iter(|p| {
                let es = match std::fs::metadata(p) {
                    Ok(m) if m.len() <= MAX_FONT_FILE => std::fs::read(p).map(|d| entries_of(&d, |_| Source::Path(p.clone()))).unwrap_or_default(),
                    _ => vec![],
                };
                es.into_iter()
            })
            .collect();
        log::info!("font scan: {} faces from {} files", found.len(), files.len());
        self.add_entries(found);
    }

    /// Distinct family names, sorted case-insensitively.
    pub fn families(&self) -> Vec<String> {
        let c = self.catalog.read().unwrap_or_else(|e| e.into_inner());
        let mut v: Vec<String> = c.entries.iter().map(|e| e.family.clone()).collect();
        v.sort_by_key(|s| s.to_lowercase());
        v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        v
    }

    /// The faces of a family (font pickers).
    pub fn faces_of(&self, family: &str) -> Vec<FaceInfo> {
        let c = self.catalog.read().unwrap_or_else(|e| e.into_inner());
        c.entries
            .iter()
            .filter(|e| e.family.eq_ignore_ascii_case(family))
            .map(|e| FaceInfo { family: e.family.clone(), style: e.style.clone(), weight: e.weight, italic: e.italic })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.catalog.read().unwrap_or_else(|e| e.into_inner()).entries.is_empty()
    }

    fn load(&self, e: &Arc<Entry>, weight: u32) -> Option<Arc<FontFace>> {
        let w = if e.wght.is_some() { weight.clamp(1, 1000) as u16 } else { e.weight };
        let mut map = e.loaded.lock().unwrap_or_else(|x| x.into_inner());
        if let Some(f) = map.get(&w) {
            return f.clone();
        }
        let f = load_face(e, w);
        map.insert(w, f.clone());
        f
    }

    fn pick(&self, name: &str, weight: u32, italic: bool) -> Option<Arc<FontFace>> {
        let entry = {
            let c = self.catalog.read().unwrap_or_else(|e| e.into_inner());
            let idxs = c.by_key.get(&norm(name))?;
            let score = |e: &Entry| -> u32 {
                let wd = match e.wght {
                    Some((lo, hi)) => (weight as f32).clamp(lo, hi).round() as i64 - weight as i64,
                    None => e.weight as i64 - weight as i64,
                };
                wd.unsigned_abs() as u32 + if e.italic == italic { 0 } else { 1000 }
            };
            idxs.iter().filter_map(|&i| c.entries.get(i)).min_by_key(|e| score(e))?.clone()
        };
        self.load(&entry, weight)
    }

    /// The faces of the request's family list in order, then the generic and default fallbacks.
    pub fn resolve_chain(&self, req: &FontRequest) -> Vec<Arc<FontFace>> {
        fn add(out: &mut Vec<Arc<FontFace>>, f: Option<Arc<FontFace>>) {
            if let Some(f) = f
                && !out.iter().any(|o| o.id == f.id)
            {
                out.push(f);
            }
        }
        let mut out: Vec<Arc<FontFace>> = Vec::new();
        for fam in parse_family_list(&req.family) {
            let found = match fam.to_lowercase().as_str() {
                "sans-serif" | "system-ui" | "ui-sans-serif" => {
                    self.generic(&["Segoe UI", "Arial", "Helvetica", "Helvetica Neue", "DejaVu Sans", "Liberation Sans", "Noto Sans", "Roboto"], req)
                }
                "serif" | "ui-serif" => self.generic(&["Times New Roman", "Georgia", "DejaVu Serif", "Liberation Serif", "Noto Serif"], req),
                "monospace" | "ui-monospace" => {
                    self.generic(&["Consolas", "Courier New", "DejaVu Sans Mono", "Liberation Mono", "Noto Sans Mono"], req)
                }
                "cursive" | "fantasy" => self.generic(&["Comic Sans MS", "Segoe Script"], req),
                _ => self.pick(&fam, req.weight, req.italic),
            };
            add(&mut out, found);
        }
        // Last resorts: the bundled design font, then anything.
        add(&mut out, self.pick("Stem", req.weight, req.italic));
        if out.is_empty() {
            let first = self.catalog.read().unwrap_or_else(|e| e.into_inner()).entries.first().cloned();
            if let Some(e) = first {
                let f = self.load(&e, req.weight);
                add(&mut out, f);
            }
        }
        out
    }

    fn generic(&self, names: &[&str], req: &FontRequest) -> Option<Arc<FontFace>> {
        names.iter().find_map(|n| self.pick(n, req.weight, req.italic))
    }

    /// The primary face for `req` (first of [`FontDb::resolve_chain`]).
    pub fn resolve(&self, req: &FontRequest) -> Option<Arc<FontFace>> {
        self.resolve_chain(req).into_iter().next()
    }

    /// A face that has a glyph for `c`, searching the whole catalogue (cached). Used when none of the
    /// requested families covers a character.
    pub fn fallback_for(&self, c: char, weight: u32, italic: bool) -> Option<Arc<FontFace>> {
        let key = (c, (weight / 100) as u16, italic);
        if let Some(hit) = self.fallback.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return hit.clone();
        }
        let entries: Vec<Arc<Entry>> = self.catalog.read().unwrap_or_else(|e| e.into_inner()).entries.clone();
        // Prefer well-known broad-coverage families, then everything else.
        const PREFERRED: &[&str] = &[
            "noto",
            "nirmala",
            "segoe ui",
            "arial unicode",
            "gadugi",
            "mangal",
            "kartika",
            "gautami",
            "latha",
            "dejavu",
            "free",
            "lohit",
            "arial",
            "tahoma",
            "calibri",
        ];
        let rank = |e: &Entry| PREFERRED.iter().position(|p| e.family.to_lowercase().contains(p)).unwrap_or(PREFERRED.len());
        let mut order: Vec<&Arc<Entry>> = entries.iter().collect();
        order.sort_by_key(|e| (rank(e), (e.weight as i32 - weight as i32).abs(), e.italic != italic));
        let mut found = None;
        for e in order {
            if let Some(face) = self.load(e, weight)
                && face.covers(c)
            {
                found = Some(face);
                break;
            }
        }
        self.fallback.lock().unwrap_or_else(|e| e.into_inner()).insert(key, found.clone());
        found
    }
}

fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        dirs.extend(["/System/Library/Fonts", "/Library/Fonts"].map(PathBuf::from));
        if let Some(h) = &home {
            dirs.push(h.join("Library/Fonts"));
        }
    } else if cfg!(windows) {
        let root = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        dirs.push(root.join("Fonts"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(l).join("Microsoft\\Windows\\Fonts"));
        }
    } else {
        dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from));
        if let Some(h) = &home {
            dirs.push(h.join(".fonts"));
            dirs.push(h.join(".local/share/fonts"));
        }
    }
    dirs
}

fn collect_fonts(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            collect_fonts(&p, depth + 1, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc")) {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_lists() {
        assert_eq!(parse_family_list("'Stem Bold', Georgia, serif"), vec!["Stem Bold", "Georgia", "serif"]);
        assert!(parse_family_list(" , ").is_empty());
    }

    #[test]
    fn bundled_stem_resolves_by_family_and_full_name() {
        let db = FontDb::new();
        let regular = db.resolve(&FontRequest::new("Stem", 400, false)).expect("stem");
        assert_eq!(regular.weight, 400);
        // As in the original @font-face rules, each Stem weight is its own family.
        let bold = db.resolve(&FontRequest::new("Stem Bold", 700, false)).expect("bold");
        assert_eq!(bold.weight, 700);
        let by_name = db.resolve(&FontRequest::new("'Stem Bold', sans-serif", 400, false)).expect("named");
        assert_eq!(by_name.weight, 700, "legacy 'Stem Bold' family names pick the bold face");
        let italic = db.resolve(&FontRequest::new("Stem", 400, true)).expect("italic");
        assert!(italic.italic);
        assert!(db.families().iter().any(|f| f.starts_with("Stem")));
    }

    #[test]
    fn unknown_family_falls_back_to_bundled() {
        let db = FontDb::new();
        assert!(db.resolve(&FontRequest::new("No Such Font", 400, false)).is_some());
        assert!(FontDb::empty().resolve(&FontRequest::new("x", 400, false)).is_none());
    }

    #[test]
    fn outlines_have_contours() {
        let db = FontDb::new();
        let f = db.resolve(&FontRequest::new("Stem", 400, false)).expect("stem");
        let gid = f.skrifa().and_then(|s| s.charmap().map('A')).map(|g| g.to_u32()).unwrap_or(0);
        assert!(gid != 0);
        assert!(!f.outline(gid).elements().is_empty());
    }
}

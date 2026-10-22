use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use forge_doc::source::sniff_mime;
use forge_doc::{CardSource, Design, Field, ImageSource, PhotoEntry, Project, SheetSettings, Table, TargetAspect, Template};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{FormatError, FormatResult};

pub const FORMAT_NAME: &str = "forge-studio";
pub const PROJECT_EXTENSION: &str = "forgeproject";
pub const LEGACY_EXTENSION: &str = "idcardproject";
const VERSION: u32 = 1;
const JSON_NAME: &str = "project.json";
const MEDIA_PREFIX: &str = "zip:";
/// Largest single entry read from an archive (a zip bomb guard).
const MAX_ENTRY: u64 = 512 << 20;
const MAX_JSON: u64 = 256 << 20;

pub struct SaveOptions {
    /// Copy photos/template that live in files on disk into the project (portable) instead of
    /// storing their paths.
    pub embed_files: bool,
}

impl Default for SaveOptions {
    fn default() -> Self {
        Self { embed_files: true }
    }
}

pub struct Loaded {
    pub project: Project,
    /// Things that could not be restored (dropped fields, missing image files...).
    pub warnings: Vec<String>,
    /// Opened from the previous app's `.idcardproject` (so "Save" should offer the new format).
    pub legacy: bool,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct DesignFile {
    template: Option<Template>,
    fields: Vec<Value>,
    mapping: BTreeMap<String, String>,
    photo_key_column: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct CropFile {
    target_aspect: Option<TargetAspect>,
    auto_crop_on_import: Option<bool>,
    selected_key: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct CardSourceFile {
    kind: String,
    files: Vec<(String, ImageSource)>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct FileV1 {
    format: String,
    version: u32,
    design: DesignFile,
    data: Table,
    photos: BTreeMap<String, Value>,
    crop: CropFile,
    sheet: Value,
    card_source: CardSourceFile,
    preview_index: usize,
}

fn ext_for(bytes: &[u8]) -> &'static str {
    match sniff_mime(bytes) {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/tiff" => "tif",
        _ => "bin",
    }
}

/// Collects media while rewriting sources to `zip:media/..` references.
struct Media {
    embed_files: bool,
    names: HashMap<usize, String>,
    entries: Vec<(String, Arc<Vec<u8>>)>,
    files: Vec<(String, PathBuf)>,
    counter: usize,
}

impl Media {
    fn externalize(&mut self, kind: &str, src: &ImageSource) -> ImageSource {
        match src {
            ImageSource::Data(d) => {
                let key = Arc::as_ptr(d) as usize;
                if let Some(n) = self.names.get(&key) {
                    return ImageSource::Path(PathBuf::from(format!("{MEDIA_PREFIX}{n}")));
                }
                self.counter += 1;
                let name = format!("media/{kind}{:05}.{}", self.counter, ext_for(d));
                self.names.insert(key, name.clone());
                self.entries.push((name.clone(), d.clone()));
                ImageSource::Path(PathBuf::from(format!("{MEDIA_PREFIX}{name}")))
            }
            ImageSource::Path(p) if self.embed_files => {
                let key = p.to_string_lossy().to_string();
                if let Some(n) = self.names.get(&(hash_str(&key))) {
                    return ImageSource::Path(PathBuf::from(format!("{MEDIA_PREFIX}{n}")));
                }
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("bin").to_ascii_lowercase();
                self.counter += 1;
                let name = format!("media/{kind}{:05}.{ext}", self.counter);
                self.names.insert(hash_str(&key), name.clone());
                self.files.push((name.clone(), p.clone()));
                ImageSource::Path(PathBuf::from(format!("{MEDIA_PREFIX}{name}")))
            }
            other => other.clone(),
        }
    }
}

fn hash_str(s: &str) -> usize {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish() as usize
}

/// Saves `p` to `path` (written to a temp file and renamed, so a crash never corrupts the old file).
pub fn save_project(p: &Project, path: &Path, opts: &SaveOptions) -> FormatResult<()> {
    let mut media = Media { embed_files: opts.embed_files, names: HashMap::new(), entries: Vec::new(), files: Vec::new(), counter: 0 };
    let template = p.design.template.as_ref().map(|t| Template { src: media.externalize("template", &t.src), w: t.w, h: t.h });
    let mut photos = BTreeMap::new();
    for (k, e) in p.photos.iter() {
        let mut e = (**e).clone();
        e.source = media.externalize("photo", &e.source);
        photos.insert(k.clone(), serde_json::to_value(&e)?);
    }
    let (kind, files) = match &p.card_source {
        CardSource::Generated => ("generated", vec![]),
        CardSource::CroppedPhotos => ("croppedPhotos", vec![]),
        CardSource::Files(f) => ("files", f.iter().map(|(n, s)| (n.clone(), media.externalize("card", s))).collect()),
    };
    let file = FileV1 {
        format: FORMAT_NAME.into(),
        version: VERSION,
        design: DesignFile {
            template,
            fields: p.design.fields.iter().map(serde_json::to_value).collect::<Result<_, _>>()?,
            mapping: p.design.mapping.clone(),
            photo_key_column: p.design.photo_key_column.clone(),
        },
        data: (*p.data).clone(),
        photos,
        crop: CropFile {
            target_aspect: Some(p.crop.target_aspect),
            auto_crop_on_import: Some(p.crop.auto_crop_on_import),
            selected_key: p.crop.selected_key.clone(),
        },
        sheet: serde_json::to_value(&p.sheet)?,
        card_source: CardSourceFile { kind: kind.into(), files },
        preview_index: p.preview_index,
    };

    let tmp = path.with_extension(format!("{PROJECT_EXTENSION}.part"));
    let result = (|| -> FormatResult<()> {
        let mut zip = ZipWriter::new(std::io::BufWriter::new(std::fs::File::create(&tmp)?));
        zip.start_file(JSON_NAME, SimpleFileOptions::default().compression_method(CompressionMethod::Deflated))?;
        zip.write_all(&serde_json::to_vec(&file)?)?;
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, data) in &media.entries {
            zip.start_file(name.as_str(), stored)?;
            zip.write_all(data)?;
        }
        for (name, src) in &media.files {
            zip.start_file(name.as_str(), stored)?;
            let mut f = std::fs::File::open(src)?;
            std::io::copy(&mut f, &mut zip)?;
        }
        zip.finish()?.flush()?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            std::fs::rename(&tmp, path)?;
            Ok(())
        }
        Err(e) => {
            // The partial file is useless; failing to delete it changes nothing for the caller.
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub fn load_project(path: &Path) -> FormatResult<Loaded> {
    let len = std::fs::metadata(path)?.len();
    if len > (4u64 << 30) {
        return Err(FormatError::Invalid("file is too large".into()));
    }
    let bytes = std::fs::read(path)?;
    let mut l = load_project_bytes(&bytes)?;
    // Relative image paths in the file are relative to the project's folder.
    if let Some(dir) = path.parent() {
        rebase_paths(&mut l.project, dir);
    }
    Ok(l)
}

fn rebase_paths(p: &mut Project, dir: &Path) {
    let fix = |s: &mut ImageSource| {
        if let ImageSource::Path(q) = s
            && q.is_relative()
        {
            *q = dir.join(&*q);
        }
    };
    if let Some(t) = &mut p.design.template {
        fix(&mut t.src);
    }
    let photos: BTreeMap<String, Arc<PhotoEntry>> = p
        .photos
        .iter()
        .map(|(k, e)| {
            let mut e = (**e).clone();
            fix(&mut e.source);
            (k.clone(), Arc::new(e))
        })
        .collect();
    p.photos = Arc::new(photos);
}

pub fn load_project_bytes(bytes: &[u8]) -> FormatResult<Loaded> {
    if bytes.starts_with(b"PK") {
        let mut zip = ZipArchive::new(Cursor::new(bytes))?;
        let json = {
            let f = zip.by_name(JSON_NAME).map_err(|_| FormatError::Invalid("project.json is missing".into()))?;
            if f.size() > MAX_JSON {
                return Err(FormatError::Invalid("project.json is too large".into()));
            }
            let mut buf = Vec::new();
            f.take(MAX_JSON).read_to_end(&mut buf)?;
            buf
        };
        let v: Value = serde_json::from_slice(&json)?;
        let mut loaded = from_value(v)?;
        resolve_media(&mut loaded.project, &mut zip, &mut loaded.warnings);
        Ok(loaded)
    } else {
        let v: Value = serde_json::from_slice(strip_bom(bytes))?;
        from_value(v)
    }
}

fn strip_bom(b: &[u8]) -> &[u8] {
    b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b)
}

fn resolve_media(p: &mut Project, zip: &mut ZipArchive<Cursor<&[u8]>>, warnings: &mut Vec<String>) {
    let mut cache: HashMap<String, Arc<Vec<u8>>> = HashMap::new();
    let mut load = |src: &mut ImageSource| {
        let ImageSource::Path(q) = src else { return };
        let Some(name) = q.to_str().and_then(|s| s.strip_prefix(MEDIA_PREFIX)) else { return };
        let name = name.to_string();
        if let Some(d) = cache.get(&name) {
            *src = ImageSource::Data(d.clone());
            return;
        }
        let data = zip.by_name(&name).ok().and_then(|f| {
            if f.size() > MAX_ENTRY {
                return None;
            }
            let mut buf = Vec::new();
            f.take(MAX_ENTRY).read_to_end(&mut buf).ok()?;
            Some(buf)
        });
        match data {
            Some(d) => {
                let d = Arc::new(d);
                cache.insert(name, d.clone());
                *src = ImageSource::Data(d);
            }
            None => {
                warnings.push(format!("image {name} is missing from the project"));
                *src = ImageSource::None;
            }
        }
    };
    if let Some(t) = &mut p.design.template {
        load(&mut t.src);
    }
    let photos: BTreeMap<String, Arc<PhotoEntry>> = p
        .photos
        .iter()
        .map(|(k, e)| {
            let mut e = (**e).clone();
            load(&mut e.source);
            (k.clone(), Arc::new(e))
        })
        .collect();
    p.photos = Arc::new(photos);
    if let CardSource::Files(files) = &mut p.card_source {
        for (_, s) in files.iter_mut() {
            load(s);
        }
    }
}

fn parse_fields(values: &[Value], warnings: &mut Vec<String>) -> Vec<Field> {
    let mut out = Vec::new();
    for (i, v) in values.iter().enumerate() {
        match serde_json::from_value::<Field>(v.clone()) {
            Ok(f) => out.push(f),
            Err(e) => warnings.push(format!("field {} could not be read and was skipped ({e})", i + 1)),
        }
    }
    // Ids must be unique: the mapping is keyed on them.
    let mut seen = std::collections::HashSet::new();
    for f in &mut out {
        if f.id.is_empty() || !seen.insert(f.id.clone()) {
            let mut n = 1;
            while seen.contains(&format!("f{n}")) {
                n += 1;
            }
            f.id = format!("f{n}");
            seen.insert(f.id.clone());
        }
    }
    out
}

fn parse_sheet(v: &Value) -> SheetSettings {
    if v.is_null() { SheetSettings::default() } else { serde_json::from_value(v.clone()).unwrap_or_default() }
}

fn photo_map(values: &BTreeMap<String, Value>) -> BTreeMap<String, PhotoEntry> {
    forge_doc::photo::photos_from_json(Value::Object(values.iter().map(|(k, v)| (k.clone(), v.clone())).collect()))
}

fn from_value(v: Value) -> FormatResult<Loaded> {
    let obj = v.as_object().ok_or_else(|| FormatError::Invalid("expected a JSON object".into()))?;
    if obj.get("format").and_then(Value::as_str) == Some(FORMAT_NAME) {
        return from_v1(v);
    }
    if obj.contains_key("studio") || obj.contains_key("cropping") || obj.contains_key("composer") {
        return from_legacy(&v);
    }
    Err(FormatError::Invalid("this is not a Forge Studio or ID Card Studio project".into()))
}

fn from_v1(v: Value) -> FormatResult<Loaded> {
    let file: FileV1 = serde_json::from_value(v)?;
    if file.version > VERSION {
        return Err(FormatError::Invalid(format!("this project was saved by a newer version (format {})", file.version)));
    }
    let mut warnings = Vec::new();
    let mut p = Project::new();
    p.design = Design {
        template: file.design.template,
        fields: parse_fields(&file.design.fields, &mut warnings),
        mapping: file.design.mapping,
        photo_key_column: file.design.photo_key_column,
    };
    p.data = Arc::new(file.data);
    p.photos = Arc::new(photo_map(&file.photos).into_iter().map(|(k, e)| (k, Arc::new(e))).collect());
    if let Some(t) = file.crop.target_aspect {
        p.crop.target_aspect = t;
    }
    if let Some(b) = file.crop.auto_crop_on_import {
        p.crop.auto_crop_on_import = b;
    }
    p.crop.selected_key = file.crop.selected_key;
    p.sheet = parse_sheet(&file.sheet);
    p.card_source = match file.card_source.kind.as_str() {
        "croppedPhotos" => CardSource::CroppedPhotos,
        "files" => CardSource::Files(file.card_source.files),
        _ => CardSource::Generated,
    };
    p.preview_index = file.preview_index;
    p.clamp_indices();
    Ok(Loaded { project: p, warnings, legacy: false })
}

/// The previous app's file: `{ cropping, studio, composer }`.
fn from_legacy(v: &Value) -> FormatResult<Loaded> {
    let mut warnings = Vec::new();
    let mut p = Project::new();
    let studio = v.get("studio").cloned().unwrap_or(Value::Null);
    let cropping = v.get("cropping").cloned().unwrap_or(Value::Null);

    if let Some(t) = studio.get("template").filter(|t| !t.is_null()) {
        match serde_json::from_value::<Template>(t.clone()) {
            Ok(t) => p.design.template = Some(t),
            Err(e) => warnings.push(format!("the template image could not be read ({e})")),
        }
    }
    if let Some(Value::Array(fields)) = studio.get("fields") {
        p.design.fields = parse_fields(fields, &mut warnings);
    }
    if let Some(d) = studio.get("data") {
        p.data = Arc::new(serde_json::from_value::<Table>(d.clone()).unwrap_or_default());
    }
    if let Some(m) = studio.get("mapping") {
        p.design.mapping = serde_json::from_value(m.clone()).unwrap_or_default();
    }
    p.design.photo_key_column = studio.get("photoKeyColumn").and_then(Value::as_str).unwrap_or_default().to_string();
    p.preview_index = studio.get("previewIndex").and_then(Value::as_u64).unwrap_or(0) as usize;

    // Two photo stores in the old format: merge, the cropping store winning (it holds the crops the
    // user made), the design store filling gaps.
    let mut merged = photo_map(
        &studio.get("photos").and_then(|x| x.as_object()).map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default(),
    );
    let crop_photos = photo_map(
        &cropping.get("photos").and_then(|x| x.as_object()).map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default(),
    );
    for (k, e) in crop_photos {
        match merged.get_mut(&k) {
            Some(existing) => {
                let source = if e.source.is_none() { existing.source.clone() } else { e.source.clone() };
                *existing = PhotoEntry { source, ..e };
            }
            None => {
                merged.insert(k, e);
            }
        }
    }
    for (k, e) in merged.iter_mut() {
        if e.natural_w == 0
            && let Ok(bytes) = e.source.bytes()
            && let Ok((w, h)) = forge_image::probe_dimensions(&bytes)
        {
            (e.natural_w, e.natural_h) = (w, h);
        }
        if e.name.is_empty() {
            e.name = k.clone();
        }
    }
    p.photos = Arc::new(merged.into_iter().map(|(k, e)| (k, Arc::new(e))).collect());

    if let Some(t) = cropping.get("targetAspect").and_then(|t| serde_json::from_value::<TargetAspect>(t.clone()).ok()) {
        p.crop.target_aspect = t;
    }
    if let Some(b) = cropping.get("autoCropOnImport").and_then(Value::as_bool) {
        p.crop.auto_crop_on_import = b;
    }
    p.crop.selected_key = cropping.get("selectedKey").and_then(Value::as_str).map(str::to_string);
    p.sheet = parse_sheet(v.get("composer").unwrap_or(&Value::Null));
    p.clamp_indices();
    Ok(Loaded { project: p, warnings, legacy: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("forge-format-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn small_project() -> Project {
        let mut p = Project::new();
        p.design.template = Some(Template { src: ImageSource::from_bytes(vec![0x89, b'P', b'N', b'G', 1, 2, 3]), w: 100, h: 60 });
        p.design.fields = vec![Field::new_text("f1".into()), Field::new_photo("f2".into())];
        p.design.mapping.insert("f1".into(), "Name".into());
        p.design.photo_key_column = "Roll".into();
        p.data = Arc::new(Table::new(vec!["Name".into(), "Roll".into()], vec![vec!["Anu".into(), "7".into()]]));
        let shared = ImageSource::from_bytes(vec![0xFF, 0xD8, 9, 9]);
        let mut photos = BTreeMap::new();
        photos.insert("7".to_string(), Arc::new(PhotoEntry { natural_w: 10, natural_h: 20, ..PhotoEntry::new("7.jpg", shared.clone()) }));
        photos.insert("8".to_string(), Arc::new(PhotoEntry::new("8.jpg", shared)));
        p.photos = Arc::new(photos);
        p.sheet.dpi = 600;
        p
    }

    #[test]
    fn zip_round_trip_with_shared_media() {
        let d = tmpdir("rt");
        let path = d.join("a.forgeproject");
        let p = small_project();
        save_project(&p, &path, &SaveOptions::default()).unwrap();
        let l = load_project(&path).unwrap();
        assert!(!l.legacy && l.warnings.is_empty(), "{:?}", l.warnings);
        assert_eq!(l.project.design.fields, p.design.fields);
        assert_eq!(l.project.design.mapping, p.design.mapping);
        assert_eq!(*l.project.data, *p.data);
        assert_eq!(l.project.sheet.dpi, 600);
        assert_eq!(l.project.photos["7"].source, p.photos["7"].source);
        assert_eq!(l.project.design.template, p.design.template);
        // The two photos share one image entry.
        let z = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(z.len(), 3, "project.json + template + one shared photo");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn legacy_project_opens_and_merges_photo_stores() {
        let json = serde_json::json!({
            "cropping": {
                "photos": { "a1": { "name": "a1.jpg", "dataUrl": "data:image/png;base64,iVBORw0KGgo=", "naturalW": 400, "naturalH": 500, "manualCrop": {"zoom": 120, "offsetX": 5, "offsetY": 0}, "status": "ready" } },
                "targetAspect": { "w": 51, "h": 51 }, "autoCropOnImport": false
            },
            "studio": {
                "template": { "src": "data:image/png;base64,iVBORw0KGgo=", "w": 1000, "h": 600 },
                "fields": [ {"id":"f1","type":"text","name":"Name","x":10,"y":10,"w":50,"h":10,"fontSize":24,"color":"#ff0000","align":"center"}, {"type":"weird"}, 5 ],
                "data": { "headers": ["Name","Roll"], "rows": [ {"Name":"Anu","Roll":"a1"} ] },
                "mapping": { "f1": "Name" }, "photoKeyColumn": "Roll", "previewIndex": 9,
                "photos": { "a1": "data:image/png;base64,iVBORw0KGgo=", "b2": { "dataUrl": "data:image/png;base64,iVBORw0KGgo=", "naturalW": 3 } }
            },
            "composer": { "sheet": {"w": 29.7, "h": 42, "orientation": "portrait"}, "card": {"w": 5, "h": 5}, "images": [1,2,3], "dpi": 150 }
        });
        let l = load_project_bytes(json.to_string().as_bytes()).unwrap();
        assert!(l.legacy);
        let p = &l.project;
        assert_eq!(p.design.fields.len(), 1, "unreadable fields are skipped");
        assert_eq!(l.warnings.len(), 2);
        assert_eq!(p.photos.len(), 2);
        assert!(p.photos["a1"].manual_crop.is_some(), "the cropping store's manual crop survives");
        assert!(!p.photos["a1"].source.is_none());
        assert_eq!(p.crop.target_aspect, TargetAspect { w: 51.0, h: 51.0 });
        assert!(!p.crop.auto_crop_on_import);
        assert_eq!(p.sheet.card.w, 5.0);
        assert_eq!(p.sheet.dpi, 150);
        assert_eq!(p.preview_index, 0, "clamped to the data");
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        for bad in [&b""[..], b"{", b"[]", b"{\"x\":1}", b"PK\x03\x04junk", b"\xEF\xBB\xBF{\"format\":\"forge-studio\",\"version\":99}"] {
            assert!(load_project_bytes(bad).is_err());
        }
    }

    #[test]
    fn missing_media_becomes_a_warning() {
        let d = tmpdir("mm");
        let path = d.join("b.forgeproject");
        save_project(&small_project(), &path, &SaveOptions::default()).unwrap();
        // Rewrite the archive without the media.
        let mut z = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut json = Vec::new();
        z.by_name(JSON_NAME).unwrap().read_to_end(&mut json).unwrap();
        let mut out = ZipWriter::new(Cursor::new(Vec::new()));
        out.start_file(JSON_NAME, SimpleFileOptions::default()).unwrap();
        out.write_all(&json).unwrap();
        let bytes = out.finish().unwrap().into_inner();
        let l = load_project_bytes(&bytes).unwrap();
        assert!(!l.warnings.is_empty());
        std::fs::remove_dir_all(d).ok();
    }
}

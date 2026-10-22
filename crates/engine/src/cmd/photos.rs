use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use forge_doc::util::{is_image_name, natural_cmp, normalize_key};
use forge_doc::{CropStatus, ImageSource, ManualCrop, PhotoEntry};
use rayon::prelude::*;
use serde::Deserialize;

use super::{args, finite, ok};
use crate::jobs::{DetectOutcome, detect_items, run_detect};
use crate::registry::{CommandDef, always};
use crate::{Engine, EngineError, EngineResult, Value, json};

/// Header bytes read to learn an image's size without decoding it (covers EXIF in practice).
const PROBE_BYTES: usize = 512 * 1024;

fn probe(path: &std::path::Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(path).ok()?.take(PROBE_BYTES as u64).read_to_end(&mut head).ok()?;
    forge_image::probe_dimensions(&head).ok().or_else(|| forge_image::probe_dimensions(&std::fs::read(path).ok()?).ok())
}

#[derive(Deserialize)]
struct AddArgs {
    paths: Vec<PathBuf>,
}

fn add(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let AddArgs { paths } = args(v)?;
    let (images, skipped): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(is_image_name));
    let aspect = e.project.crop.target_aspect.ratio();
    let entries: Vec<(String, PhotoEntry)> = images
        .par_iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().to_string();
            let key = normalize_key(&name);
            if key.is_empty() {
                return None;
            }
            let (w, h) = probe(p)?;
            let mut entry = PhotoEntry::new(name, ImageSource::from_path(p.clone()));
            entry.natural_w = w;
            entry.natural_h = h;
            Some((key, entry))
        })
        .collect();
    let photos = Arc::make_mut(&mut e.project.photos);
    let (mut added, mut replaced) = (0, 0);
    for (key, mut entry) in entries {
        entry.recompute_auto_crop(aspect);
        if photos.insert(key, Arc::new(entry)).is_some() {
            replaced += 1;
        } else {
            added += 1;
        }
    }
    Ok(json!({ "added": added, "replaced": replaced, "skipped": skipped.len() + images.len().saturating_sub(added + replaced) }))
}

#[derive(Deserialize)]
struct FolderArg {
    path: PathBuf,
}

fn add_folder(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let FolderArg { path } = args(v)?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&path)
        .map_err(|err| EngineError::failed(format!("cannot read {}: {err}", path.display())))?
        .flatten()
        .map(|d| d.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    add(e, &json!({ "paths": files }))
}

#[derive(Deserialize)]
struct KeysArg {
    keys: Vec<String>,
}

fn remove(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let KeysArg { keys } = args(v)?;
    let photos = Arc::make_mut(&mut e.project.photos);
    let n = keys.iter().filter(|k| photos.remove(*k).is_some()).count();
    Ok(json!({ "removed": n }))
}

fn clear(e: &mut Engine, _: &Value) -> EngineResult<Value> {
    e.project.photos = Arc::new(Default::default());
    e.project.crop.selected_key = None;
    ok()
}

#[derive(Deserialize)]
struct ApplyArgs {
    results: Vec<DetectOutcome>,
}

fn apply_detections(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let ApplyArgs { results } = args(v)?;
    let aspect = e.project.crop.target_aspect.ratio();
    let photos = Arc::make_mut(&mut e.project.photos);
    let (mut found, mut missing, mut failed) = (0, 0, 0);
    for r in results {
        let Some(slot) = photos.get_mut(&r.key) else { continue };
        let entry = Arc::make_mut(slot);
        if r.error.is_some() {
            failed += 1;
            entry.status = CropStatus::NeedsReview;
            continue;
        }
        if r.width > 0 {
            entry.natural_w = r.width;
            entry.natural_h = r.height;
        }
        entry.faces_detected = r.detection.is_some();
        entry.raw_detection = r.detection;
        entry.auto_crop = None;
        entry.recompute_auto_crop(aspect);
        if entry.auto_crop.is_some() {
            entry.status = CropStatus::Ready;
            found += 1;
        } else {
            entry.status = CropStatus::NeedsReview;
            missing += 1;
        }
    }
    Ok(json!({ "faces": found, "noFace": missing, "failed": failed }))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DetectArgs {
    keys: Option<Vec<String>>,
}

/// Synchronous detection (CLI, scripts). The UI runs `jobs::run_detect` on a worker instead.
fn detect(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: DetectArgs = args(v)?;
    let items = detect_items(&e.project, a.keys.as_deref());
    let results = run_detect(&items, &AtomicBool::new(false), &|_, _| {});
    apply_detections(e, &json!({ "results": results }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManualArgs {
    key: String,
    zoom: f64,
    #[serde(default)]
    offset_x: f64,
    #[serde(default)]
    offset_y: f64,
}

fn set_manual_crop(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: ManualArgs = args(v)?;
    let photos = Arc::make_mut(&mut e.project.photos);
    let entry = Arc::make_mut(photos.get_mut(&a.key).ok_or_else(|| EngineError::NotFound(format!("no photo {}", a.key)))?);
    entry.manual_crop = Some(ManualCrop {
        zoom: finite(a.zoom, 10.0, 1000.0, 100.0),
        offset_x: finite(a.offset_x, -100.0, 100.0, 0.0),
        offset_y: finite(a.offset_y, -100.0, 100.0, 0.0),
    });
    Ok(Value::Null)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Targets {
    Word(String),
    Keys(Vec<String>),
}

fn targets(e: &Engine, t: &Targets, from: &str) -> EngineResult<Vec<String>> {
    Ok(match t {
        Targets::Keys(k) => k.clone(),
        Targets::Word(w) if w == "all" => e.project.photos.keys().filter(|k| *k != from).cloned().collect(),
        Targets::Word(w) if w == "unaligned" => e
            .project
            .photos
            .iter()
            .filter(|(k, p)| *k != from && (p.status == CropStatus::NeedsReview || (!p.faces_detected && p.manual_crop.is_none())))
            .map(|(k, _)| k.clone())
            .collect(),
        Targets::Word(w) => return Err(EngineError::invalid(format!("unknown target set {w}: use \"all\", \"unaligned\" or a list of keys"))),
    })
}

#[derive(Deserialize)]
struct ApplyCropArgs {
    from: String,
    to: Targets,
}

fn apply_crop_to(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: ApplyCropArgs = args(v)?;
    let m = e.project.photos.get(&a.from).and_then(|p| p.manual_crop).ok_or_else(|| EngineError::failed("that photo has no manual crop to copy"))?;
    let keys = targets(e, &a.to, &a.from)?;
    let photos = Arc::make_mut(&mut e.project.photos);
    let mut n = 0;
    for k in keys {
        if let Some(slot) = photos.get_mut(&k) {
            Arc::make_mut(slot).manual_crop = Some(m);
            n += 1;
        }
    }
    Ok(json!({ "applied": n }))
}

fn clear_manual_crop(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let KeysArg { keys } = args(v)?;
    let photos = Arc::make_mut(&mut e.project.photos);
    for k in keys {
        if let Some(slot) = photos.get_mut(&k) {
            Arc::make_mut(slot).manual_crop = None;
        }
    }
    ok()
}

#[derive(Deserialize)]
struct AspectArgs {
    w: f64,
    h: f64,
}

fn set_target_aspect(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: AspectArgs = args(v)?;
    if !(a.w.is_finite() && a.h.is_finite() && a.w > 0.0 && a.h > 0.0 && a.w <= 10_000.0 && a.h <= 10_000.0) {
        return Err(EngineError::invalid("width and height must be positive numbers"));
    }
    e.project.crop.target_aspect = forge_doc::TargetAspect { w: a.w, h: a.h };
    let aspect = e.project.crop.target_aspect.ratio();
    let photos = Arc::make_mut(&mut e.project.photos);
    for slot in photos.values_mut() {
        if slot.raw_detection.is_some() {
            Arc::make_mut(slot).recompute_auto_crop(aspect);
        }
    }
    ok()
}

#[derive(Deserialize)]
struct SelectArg {
    key: Option<String>,
}

fn select(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let SelectArg { key } = args(v)?;
    e.project.crop.selected_key = key.filter(|k| e.project.photos.contains_key(k));
    ok()
}

#[derive(Deserialize)]
struct OnArg {
    on: bool,
}

fn set_auto_on_import(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let OnArg { on } = args(v)?;
    e.project.crop.auto_crop_on_import = on;
    ok()
}

#[derive(Deserialize)]
struct BgArgs {
    keys: Targets,
    /// `#rrggbb`.
    #[serde(default = "white")]
    color: String,
    #[serde(default = "tolerance")]
    tolerance: f32,
}

fn white() -> String {
    "#ffffff".into()
}
fn tolerance() -> f32 {
    48.0
}

/// Replaces a plain studio backdrop with a solid colour (the photo is re-stored as a high-quality JPEG).
fn replace_background(e: &mut Engine, v: &Value) -> EngineResult<Value> {
    let a: BgArgs = args(v)?;
    let color = forge_color_parse(&a.color)?;
    let keys = targets(e, &a.keys, "")?;
    let jobs: Vec<(String, ImageSource)> = keys.iter().filter_map(|k| e.project.photos.get(k).map(|p| (k.clone(), p.source.clone()))).collect();
    let done: Vec<(String, Option<Vec<u8>>)> = jobs
        .par_iter()
        .map(|(k, src)| {
            let r = (|| {
                let img = forge_image::decode(&src.bytes().ok()?).ok()?;
                let (out, changed) =
                    forge_vision::replace_background(&img, forge_vision::BgOptions { color, tolerance: a.tolerance.clamp(1.0, 200.0), feather: 0 });
                if !changed {
                    return None;
                }
                forge_image::encode_jpeg(&out, 95).ok()
            })();
            (k.clone(), r)
        })
        .collect();
    let photos = Arc::make_mut(&mut e.project.photos);
    let mut n = 0;
    for (k, bytes) in done {
        if let (Some(bytes), Some(slot)) = (bytes, photos.get_mut(&k)) {
            Arc::make_mut(slot).source = ImageSource::from_bytes(bytes);
            n += 1;
        }
    }
    Ok(json!({ "changed": n, "unchanged": keys.len() - n }))
}

fn forge_color_parse(s: &str) -> EngineResult<[u8; 3]> {
    let c = s.trim().trim_start_matches('#');
    if c.len() != 6 {
        return Err(EngineError::invalid("color must look like #rrggbb"));
    }
    let n = u32::from_str_radix(c, 16).map_err(|_| EngineError::invalid("color must look like #rrggbb"))?;
    Ok([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

fn has_photos(e: &Engine) -> bool {
    !e.project.photos.is_empty()
}

pub(crate) const COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "photos.add",
        label: "Add photos...",
        menu: "Photos/Add photos...",
        shortcut: None,
        doc: "{paths[]} Add image files to the photo library (matched to rows by file name).",
        undoable: true,
        enabled: always,
        run: add,
    },
    CommandDef {
        id: "photos.add_folder",
        label: "Add photo folder...",
        menu: "Photos/Add folder...",
        shortcut: None,
        doc: "{path} Add every image in a folder.",
        undoable: true,
        enabled: always,
        run: add_folder,
    },
    CommandDef {
        id: "photos.remove",
        label: "Remove photos",
        menu: "",
        shortcut: Some("Delete"),
        doc: "{keys[]} Remove photos from the library.",
        undoable: true,
        enabled: has_photos,
        run: remove,
    },
    CommandDef {
        id: "photos.clear",
        label: "Remove all photos",
        menu: "Photos/Remove all",
        shortcut: None,
        doc: "Empty the photo library.",
        undoable: true,
        enabled: has_photos,
        run: clear,
    },
    CommandDef {
        id: "photos.detect",
        label: "Detect faces",
        menu: "Photos/Detect faces",
        shortcut: None,
        doc: "{keys[]?} Find the face in photos not processed yet (blocking; the UI uses a background job).",
        undoable: true,
        enabled: has_photos,
        run: detect,
    },
    CommandDef {
        id: "photos.apply_detections",
        label: "Apply face detections",
        menu: "",
        shortcut: None,
        doc: "{results[]} Store the results of a background detection job.",
        undoable: true,
        enabled: always,
        run: apply_detections,
    },
    CommandDef {
        id: "photos.set_manual_crop",
        label: "Set photo crop",
        menu: "",
        shortcut: None,
        doc: "{key, zoom, offsetX, offsetY} Manual zoom (percent) and pan (percent of overflow) of one photo.",
        undoable: true,
        enabled: has_photos,
        run: set_manual_crop,
    },
    CommandDef {
        id: "photos.clear_manual_crop",
        label: "Reset photo crop",
        menu: "",
        shortcut: None,
        doc: "{keys[]} Go back to the automatic crop.",
        undoable: true,
        enabled: has_photos,
        run: clear_manual_crop,
    },
    CommandDef {
        id: "photos.apply_crop_to",
        label: "Apply crop to others",
        menu: "",
        shortcut: None,
        doc: "{from, to: \"all\"|\"unaligned\"|[keys]} Copy one photo's manual crop to others.",
        undoable: true,
        enabled: has_photos,
        run: apply_crop_to,
    },
    CommandDef {
        id: "photos.set_target_aspect",
        label: "Set crop shape",
        menu: "",
        shortcut: None,
        doc: "{w, h} Crop aspect in millimetres (35 x 45 = passport); recomputes automatic crops.",
        undoable: true,
        enabled: always,
        run: set_target_aspect,
    },
    CommandDef {
        id: "photos.select",
        label: "Select photo",
        menu: "",
        shortcut: None,
        doc: "{key|null} The photo shown in the crop editor.",
        undoable: false,
        enabled: always,
        run: select,
    },
    CommandDef {
        id: "photos.set_auto_crop_on_import",
        label: "Auto-crop on import",
        menu: "",
        shortcut: None,
        doc: "{on} Detect faces automatically after adding photos.",
        undoable: true,
        enabled: always,
        run: set_auto_on_import,
    },
    CommandDef {
        id: "photos.replace_background",
        label: "Replace photo background",
        menu: "Photos/Replace background...",
        shortcut: None,
        doc: "{keys: \"all\"|[keys], color?, tolerance?} Replace a plain backdrop with a solid colour.",
        undoable: true,
        enabled: has_photos,
        run: replace_background,
    },
];

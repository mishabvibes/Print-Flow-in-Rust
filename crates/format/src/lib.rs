//! Forge Studio file formats (layer 5).
//!
//! * `.forgeproject`: a ZIP holding `project.json` plus the embedded media (template, photos). Images
//!   are stored as they are (no base64 inflation, no re-encoding), the JSON is small and diffable.
//! * Legacy `.idcardproject`: the previous app's single JSON file (images as data URLs) opens
//!   unchanged; the two photo stores of that format (cropping and design) are merged into one.
//! * Design presets (`template` + `fields`) and layout presets (sheet settings) in the previous app's
//!   JSON shape, in both directions.
//! * [`Prefs`]: theme and recent projects.
//!
//! Everything read from a file is untrusted: parsing is lenient (a bad field is dropped with a
//! warning, never a failed load) and bounded.
#![forbid(unsafe_code)]

mod prefs;
mod presets;
mod project_file;

pub use prefs::{Prefs, RecentProject, Theme};
pub use presets::{DesignPreset, LayoutPreset, load_design_preset, load_layout_preset, save_design_preset, save_layout_preset};
pub use project_file::{FORMAT_NAME, LEGACY_EXTENSION, Loaded, PROJECT_EXTENSION, SaveOptions, load_project, load_project_bytes, save_project};

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("cannot read or write {0}")]
    Io(#[from] std::io::Error),
    #[error("not a valid project file: {0}")]
    Invalid(String),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type FormatResult<T> = Result<T, FormatError>;

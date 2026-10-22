//! Forge Studio document model (layer 1): the project, the card design, photos and their crops,
//! the merged data table and the print-sheet settings. Nothing here depends on a UI toolkit, the
//! renderer or image codecs.
//!
//! Coordinates: field geometry is a percentage of the template (like the previous version, so old
//! project files open unchanged); sheet geometry is in centimetres.
//!
//! Every type is cheap to clone (`Arc`-shared bulk data), which is what makes whole-project undo
//! snapshots free: see `forge_engine::History`.
#![forbid(unsafe_code)]

pub mod crop;
pub mod design;
pub mod error;
pub mod field;
pub mod photo;
pub mod project;
pub mod sheet;
pub mod source;
pub mod table;
pub mod util;

pub use crop::{
    CropRect, EffectiveCrop, FaceBox, FaceDetection, ManualCrop, PASSPORT_ASPECT, Placement, compute_auto_crop, manual_from_dest, placement,
};
pub use design::{Design, Template};
pub use error::{DocError, DocResult};
pub use field::{Align, Field, FieldKind, Overflow, PhotoProps, ScriptPos, Shape, TextCase, TextProps, VAlign, Wrap};
pub use photo::{CropStatus, PhotoEntry};
pub use project::{CardRef, CardSource, CropSettings, Project, TargetAspect};
pub use sheet::{GridBorder, Margins, Orientation, PRESETS, SheetSettings, SheetSize};
pub use source::ImageSource;
pub use table::Table;

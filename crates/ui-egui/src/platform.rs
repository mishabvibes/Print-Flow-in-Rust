//! Everything the UI needs from the operating system that is not drawing: file dialogs and the
//! clipboard. The app crate implements it with `rfd`; tests use [`NoPlatform`].

use std::path::PathBuf;

pub struct FileFilter {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
}

pub trait Platform: Send + Sync {
    fn open_file(&self, title: &str, filters: &[FileFilter]) -> Option<PathBuf>;
    fn open_files(&self, title: &str, filters: &[FileFilter]) -> Vec<PathBuf>;
    fn open_folder(&self, title: &str) -> Option<PathBuf>;
    fn save_file(&self, title: &str, file_name: &str, filters: &[FileFilter]) -> Option<PathBuf>;
    fn reveal(&self, _path: &std::path::Path) {}
}

/// No dialogs (headless runs and tests): every picker is cancelled.
pub struct NoPlatform;

impl Platform for NoPlatform {
    fn open_file(&self, _: &str, _: &[FileFilter]) -> Option<PathBuf> {
        None
    }
    fn open_files(&self, _: &str, _: &[FileFilter]) -> Vec<PathBuf> {
        Vec::new()
    }
    fn open_folder(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn save_file(&self, _: &str, _: &str, _: &[FileFilter]) -> Option<PathBuf> {
        None
    }
}

pub const IMAGES: FileFilter = FileFilter { name: "Images", extensions: &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff"] };
pub const PROJECTS: FileFilter = FileFilter { name: "Forge project", extensions: &["forgeproject", "idcardproject", "json"] };
pub const SAVE_PROJECT: FileFilter = FileFilter { name: "Forge project", extensions: &["forgeproject"] };
pub const TABLES: FileFilter = FileFilter { name: "CSV / Excel", extensions: &["csv", "tsv", "txt", "xlsx", "xlsm", "xls", "ods"] };
pub const JSON: FileFilter = FileFilter { name: "Preset (JSON)", extensions: &["json"] };
pub const ZIP: FileFilter = FileFilter { name: "ZIP", extensions: &["zip"] };
pub const PDF: FileFilter = FileFilter { name: "PDF", extensions: &["pdf"] };
pub const FONTS: FileFilter = FileFilter { name: "Fonts", extensions: &["ttf", "otf", "ttc", "otc"] };

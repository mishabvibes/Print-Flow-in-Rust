use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_RECENT: usize = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentProject {
    pub path: PathBuf,
    pub name: String,
}

/// User preferences, stored as JSON in the platform config folder. Every problem reading them yields
/// defaults: preferences must never stop the app from starting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: Theme,
    pub recent: Vec<RecentProject>,
    /// Extra font files the user added.
    pub fonts: Vec<PathBuf>,
}

impl Prefs {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("ForgeStudio").join("prefs.json"))
    }

    pub fn load() -> Self {
        Self::path().map(|p| Self::load_from(&p)).unwrap_or_default()
    }

    pub fn load_from(p: &Path) -> Self {
        std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = Self::path() {
            self.save_to(&p);
        }
    }

    pub fn save_to(&self, p: &Path) {
        if let Some(dir) = p.parent() {
            // Preferences are a convenience: a failure to write them is logged, not shown as an error.
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::warn!("prefs: {e}");
                return;
            }
        }
        match serde_json::to_vec_pretty(self) {
            Ok(b) => {
                if let Err(e) = std::fs::write(p, b) {
                    log::warn!("prefs: {e}");
                }
            }
            Err(e) => log::warn!("prefs: {e}"),
        }
    }

    /// Moves `path` to the top of the recent list.
    pub fn add_recent(&mut self, path: &Path) {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        self.recent.retain(|r| r.path != path);
        self.recent.insert(0, RecentProject { path: path.to_path_buf(), name });
        self.recent.truncate(MAX_RECENT);
    }

    pub fn remove_recent(&mut self, path: &Path) {
        self.recent.retain(|r| r.path != path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_is_deduped_capped_and_ordered() {
        let mut p = Prefs::default();
        for i in 0..20 {
            p.add_recent(Path::new(&format!("/x/{i}.forgeproject")));
        }
        p.add_recent(Path::new("/x/5.forgeproject"));
        assert_eq!(p.recent.len(), MAX_RECENT);
        assert_eq!(p.recent[0].name, "5.forgeproject");
        assert_eq!(p.recent.iter().filter(|r| r.name == "5.forgeproject").count(), 1);
    }

    #[test]
    fn bad_files_give_defaults() {
        let d = std::env::temp_dir().join(format!("forge-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("p.json");
        std::fs::write(&f, b"not json").unwrap();
        assert_eq!(Prefs::load_from(&f), Prefs::default());
        let p = Prefs { theme: Theme::Light, ..Prefs::default() };
        p.save_to(&f);
        assert_eq!(Prefs::load_from(&f).theme, Theme::Light);
        std::fs::remove_dir_all(d).ok();
    }
}

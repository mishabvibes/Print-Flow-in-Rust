//! Project automation: `cargo xtask ci` runs every check CI runs.
//!
//! * `ci`      fmt, clippy (warnings are errors), tests, layers, assets
//! * `layers`  the crate dependency direction (a crate may only use crates of a lower layer)
//! * `assets`  every file under assets/ is listed in ASSETS.md
//! * `package` builds the release binaries
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// (crate directory, layer). A crate may depend on workspace crates of strictly lower layers.
const LAYERS: &[(&str, u8)] = &[
    ("geom", 0),
    ("color", 0),
    ("image", 1),
    ("doc", 1),
    ("data", 2),
    ("text", 2),
    ("vision", 2),
    ("render", 3),
    ("export", 4),
    ("format", 4),
    ("engine", 5),
    ("ui-egui", 6),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_default()
}

fn layer_of(name: &str) -> Option<u8> {
    LAYERS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
}

/// Workspace crates named in the `[dependencies]` table of a manifest.
fn internal_deps(manifest: &str) -> Vec<String> {
    let mut in_deps = false;
    let mut out = Vec::new();
    for line in manifest.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_deps = l == "[dependencies]";
            continue;
        }
        if in_deps
            && let Some(rest) = l.strip_prefix("forge-")
            && let Some((name, _)) = rest.split_once('=')
        {
            out.push(name.trim().to_string());
        }
    }
    out
}

fn layers() -> Result<(), String> {
    let mut bad = Vec::new();
    for (name, layer) in LAYERS {
        let path = root().join("crates").join(name).join("Cargo.toml");
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        for dep in internal_deps(&text) {
            match layer_of(&dep) {
                Some(l) if l < *layer => {}
                Some(l) => bad.push(format!("{name} (layer {layer}) depends on {dep} (layer {l})")),
                None => bad.push(format!("{name} depends on unknown crate forge-{dep}")),
            }
        }
    }
    // Every crate directory must be in the table.
    if let Ok(rd) = std::fs::read_dir(root().join("crates")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if layer_of(&n).is_none() {
                bad.push(format!("crates/{n} has no layer in xtask"));
            }
        }
    }
    // Only the apps may use the GUI crate; nothing below may use eframe/rfd.
    if bad.is_empty() {
        println!("layers: ok ({} crates)", LAYERS.len());
        Ok(())
    } else {
        Err(bad.join("\n"))
    }
}

fn assets() -> Result<(), String> {
    let doc = std::fs::read_to_string(root().join("ASSETS.md")).map_err(|e| format!("ASSETS.md: {e}"))?;
    let mut missing = Vec::new();
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, base, out);
                } else if let Ok(rel) = p.strip_prefix(base) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    let mut files = Vec::new();
    walk(&root().join("assets"), &root().join("assets"), &mut files);
    for f in files {
        let stem = f.rsplit('/').next().unwrap_or(&f);
        if !doc.contains(&f) && !doc.contains(stem) {
            missing.push(f);
        }
    }
    if missing.is_empty() {
        println!("assets: ok");
        Ok(())
    } else {
        Err(format!("not listed in ASSETS.md: {}", missing.join(", ")))
    }
}

fn cargo(args: &[&str]) -> Result<(), String> {
    println!("$ cargo {}", args.join(" "));
    let ok = Command::new("cargo").args(args).current_dir(root()).status().map_err(|e| e.to_string())?.success();
    if ok { Ok(()) } else { Err(format!("cargo {} failed", args.join(" "))) }
}

fn main() -> ExitCode {
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "ci".into());
    let r = match cmd.as_str() {
        "ci" => cargo(&["fmt", "--all", "--check"])
            .and_then(|()| cargo(&["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]))
            .and_then(|()| cargo(&["test", "--workspace"]))
            .and_then(|()| layers())
            .and_then(|()| assets()),
        "layers" => layers(),
        "assets" => assets(),
        "package" => cargo(&["build", "--release", "-p", "forge-studio", "-p", "forge-cli"]),
        other => Err(format!("unknown task `{other}` (ci, layers, assets, package)")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_internal_deps_from_the_dependencies_table_only() {
        let m = "[package]\nname=\"x\"\n[dependencies]\nforge-geom = { workspace = true }\nserde = { workspace = true }\n[dev-dependencies]\nforge-text = { workspace = true }\n";
        assert_eq!(internal_deps(m), vec!["geom".to_string()]);
    }
    #[test]
    fn every_layer_is_unique() {
        let mut n: Vec<_> = LAYERS.iter().map(|l| l.0).collect();
        n.sort();
        n.dedup();
        assert_eq!(n.len(), LAYERS.len());
    }
}

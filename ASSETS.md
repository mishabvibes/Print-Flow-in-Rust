# Asset attribution

Every non-code file under `assets/` is listed here. `cargo xtask assets` (part of `cargo xtask ci`) fails if a file is missing from this table.

| Asset | Source | Licence | Notes |
|---|---|---|---|
| `assets/blaze_face_short_range.tflite` | Google MediaPipe, BlazeFace short-range face detector | Apache-2.0 (upstream model card) | Run by the built-in pure-Rust TFLite interpreter in `crates/vision` |
| `assets/seeta_fd_frontal_v1.0.bin` | SeetaFace Engine frontal face detection model, as distributed with the `rustface` crate | BSD-2-Clause (upstream) | Fallback detector when BlazeFace finds no face |
| `assets/app-icon/logo.png` | Original Forge Studio / ID Card Studio artwork (previous Electron app) | Project licence (MIT) | Window icon |
| `assets/app-icon/software-icon.png` | Original Forge Studio artwork (previous Electron app) | Project licence (MIT) | Packaging |
| `assets/app-icon/app-logo.ico` | Original Forge Studio artwork (previous Electron app) | Project licence (MIT) | Windows packaging |
| `assets/rotate-white.svg` | Original Forge Studio artwork (previous Electron app) | Project licence (MIT) | Rotation-handle glyph of the old UI; unused by the egui frontend |
| `assets/fonts/Stem-Thin.otf`, `Stem-Hairline.otf`, `Stem-ExtraLight.otf`, `Stem-ExtraLightItalic.otf`, `Stem-Light.otf`, `Stem-LightItalic.otf`, `Stem-Regular.otf`, `Stem-Italic.otf`, `Stem-Medium.otf`, `Stem-MediumItalic.otf`, `Stem-Bold.otf`, `Stem-BoldItalic.otf` | Bundled with the previous Electron app | **UNKNOWN: no licence file came with them** | Kept so existing designs that use "Stem" render the same. **Before publishing or redistributing this project, confirm that you may redistribute these fonts, or delete them (and the table in `crates/text/src/bundled.rs`).** |

Not shipped: no Indic (Malayalam, Devanagari, ...) font is bundled. Text in those scripts is shaped correctly (harfrust) but needs a font that covers it on the machine (system fonts are scanned at start-up, and "Add font" registers any `.ttf/.otf/.ttc`).

//! Fonts carried over from the original app (the "Stem" family), embedded so the app has a design
//! font and a guaranteed fallback even on a machine with no fonts installed.
//!
//! The files' own name tables are unreliable (every face reports weight 400 and a symbol as its
//! style), so the family / weight / italic metadata is the one the original app declared in its
//! `@font-face` rules: that is also what existing projects' `fontFamily` values refer to.

/// (bytes, family, weight, italic)
pub const FONTS: &[(&[u8], &str, u16, bool)] = &[
    (include_bytes!("../../../assets/fonts/Stem-Hairline.otf"), "Stem Hairline", 100, false),
    (include_bytes!("../../../assets/fonts/Stem-Thin.otf"), "Stem Thin", 100, false),
    (include_bytes!("../../../assets/fonts/Stem-ExtraLight.otf"), "Stem ExtraLight", 200, false),
    (include_bytes!("../../../assets/fonts/Stem-ExtraLightItalic.otf"), "Stem ExtraLight", 200, true),
    (include_bytes!("../../../assets/fonts/Stem-Light.otf"), "Stem Light", 300, false),
    (include_bytes!("../../../assets/fonts/Stem-LightItalic.otf"), "Stem Light", 300, true),
    (include_bytes!("../../../assets/fonts/Stem-Regular.otf"), "Stem", 400, false),
    (include_bytes!("../../../assets/fonts/Stem-Italic.otf"), "Stem", 400, true),
    (include_bytes!("../../../assets/fonts/Stem-Medium.otf"), "Stem Medium", 500, false),
    (include_bytes!("../../../assets/fonts/Stem-MediumItalic.otf"), "Stem Medium", 500, true),
    (include_bytes!("../../../assets/fonts/Stem-Bold.otf"), "Stem Bold", 700, false),
    (include_bytes!("../../../assets/fonts/Stem-BoldItalic.otf"), "Stem Bold", 700, true),
];

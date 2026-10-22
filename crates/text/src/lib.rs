//! Forge Studio text (layer 2): everything between "a string and a text field's settings" and
//! "positioned glyph outlines".
//!
//! * [`FontDb`] catalogues the bundled fonts and the system fonts (scanned in the background,
//!   loaded lazily), resolves CSS-style family lists with weight/italic, and finds fallback faces for
//!   characters the requested font lacks (Malayalam, Devanagari, Arabic... on any machine).
//! * [`shape`] runs HarfRust (a Rust port of HarfBuzz), so Indic conjuncts, ligatures and kerning are
//!   right.
//! * [`layout`] is the field layout: single line with shrink-to-fit, wrapped text with hyphenation,
//!   justify, max lines, ellipsis, auto-shrink and vertical alignment.
//!
//! The renderer draws [`TextLayout`] items as filled outlines ([`FontFace::outline`]), so preview
//! and export are the same pixels.
#![forbid(unsafe_code)]

mod bundled;
mod fontdb;
mod layout;
mod shape;

pub use fontdb::{FaceInfo, FontDb, FontFace, FontRequest, parse_family_list};
pub use layout::{GlyphItem, Rule, TextLayout, layout_text, text_width};
pub use shape::{Glyph, shape_text};

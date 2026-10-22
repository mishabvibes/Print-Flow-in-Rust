//! An sRGB colour with straight alpha and CSS-style hex parsing. Layer 0.
#![forbid(unsafe_code)]

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);
    pub const TRANSPARENT: Rgba = Rgba { r: 0, g: 0, b: 0, a: 0 };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    #[allow(clippy::self_named_constructors)]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` (the `#` is optional). `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        let h = s.trim().trim_start_matches('#');
        if !h.is_ascii() {
            return None;
        }
        let nib = |i: usize| u8::from_str_radix(h.get(i..i + 1)?, 16).ok().map(|v| v * 17);
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            3 => Some(Self::rgb(nib(0)?, nib(1)?, nib(2)?)),
            4 => Some(Self::rgba(nib(0)?, nib(1)?, nib(2)?, nib(3)?)),
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    /// `#rrggbb`, or `#rrggbbaa` when not opaque.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    pub fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Same colour with the alpha multiplied by `opacity` (0..=1).
    pub fn with_opacity(self, opacity: f32) -> Self {
        let o = if opacity.is_finite() { opacity.clamp(0.0, 1.0) } else { 1.0 };
        Self { a: (self.a as f32 * o).round() as u8, ..self }
    }
}

impl Default for Rgba {
    fn default() -> Self {
        Rgba::BLACK
    }
}

impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        // Lenient: an unreadable colour from an old or hand-edited file becomes black, not an error.
        Ok(Rgba::parse(&s).unwrap_or(Rgba::BLACK))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_forms() {
        assert_eq!(Rgba::parse("#111111"), Some(Rgba::rgb(0x11, 0x11, 0x11)));
        assert_eq!(Rgba::parse("fff"), Some(Rgba::WHITE));
        assert_eq!(Rgba::parse("#ff000080"), Some(Rgba::rgba(255, 0, 0, 0x80)));
        assert_eq!(Rgba::parse("#12"), None);
        assert_eq!(Rgba::parse("#ggg"), None);
        assert_eq!(Rgba::parse("#é€é"), None);
    }

    #[test]
    fn hex_round_trip() {
        let c = Rgba::rgba(1, 2, 3, 4);
        assert_eq!(Rgba::parse(&c.to_hex()), Some(c));
        assert_eq!(Rgba::rgb(255, 0, 0).to_hex(), "#ff0000");
    }

    #[test]
    fn serde_is_lenient() {
        let c: Rgba = serde_json::from_str("\"nonsense\"").unwrap_or_default();
        assert_eq!(c, Rgba::BLACK);
    }
}

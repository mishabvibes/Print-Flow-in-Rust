//! Template fields: text and photo boxes placed on the card template (percent geometry).

use forge_color::Rgba;
use serde::{Deserialize, Deserializer, Serialize};

/// Declares a string enum that serialises as its lower-case name and deserialises leniently: an
/// unknown or mistyped value in a project file becomes the default instead of failing the whole load.
macro_rules! lenient_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? } default $def:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($variant),+ }
        impl Default for $name { fn default() -> Self { Self::$def } }
        impl $name {
            pub const ALL: &'static [$name] = &[$(Self::$variant),+];
            pub fn as_str(self) -> &'static str { match self { $(Self::$variant => $text),+ } }
            pub fn parse(s: &str) -> Self { match s.trim().to_ascii_lowercase().as_str() { $($text => Self::$variant,)+ _ => Self::$def } }
        }
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(self.as_str()) }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Ok(Self::parse(&Option::<String>::deserialize(d)?.unwrap_or_default()))
            }
        }
    };
}

lenient_enum!(Align { Left => "left", Center => "center", Right => "right", Justify => "justify", JustifyAll => "justifyall" } default Left);
lenient_enum!(Wrap { Single => "single", Multi => "multi" } default Single);
lenient_enum!(Overflow { Clip => "clip", Ellipsis => "ellipsis", Shrink => "shrink" } default Clip);
lenient_enum!(VAlign { Top => "top", Middle => "middle", Bottom => "bottom" } default Middle);
lenient_enum!(TextCase { None => "none", Upper => "upper", SmallCaps => "smallcaps" } default None);
lenient_enum!(ScriptPos { None => "none", Super => "super", Sub => "sub" } default None);
lenient_enum!(Shape { Rect => "rect", Circle => "circle" } default Rect);

fn d_font_size() -> f64 {
    18.0
}
fn d_font_family() -> String {
    "Georgia, serif".into()
}
fn d_weight() -> u32 {
    400
}
fn d_color() -> Rgba {
    Rgba::rgb(0x11, 0x11, 0x11)
}
fn d_line_height() -> f64 {
    1.3
}
fn d_opacity() -> f64 {
    100.0
}
fn d_zoom() -> f64 {
    100.0
}
fn d_border_w() -> f64 {
    2.0
}

/// Properties of a text field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextProps {
    /// Font size in template pixels.
    #[serde(default = "d_font_size")]
    pub font_size: f64,
    /// A CSS-style family list, e.g. `'Stem Bold', sans-serif`.
    #[serde(default = "d_font_family")]
    pub font_family: String,
    #[serde(default = "d_weight")]
    pub weight: u32,
    #[serde(default = "d_color")]
    pub color: Rgba,
    pub italic: bool,
    pub align: Align,
    pub wrap: Wrap,
    pub hyphenate: bool,
    #[serde(default = "d_line_height")]
    pub line_height: f64,
    /// 0 = as many as fit.
    pub max_lines: u32,
    pub overflow: Overflow,
    pub v_align: VAlign,
    pub text_case: TextCase,
    pub script_pos: ScriptPos,
    pub underline: bool,
    pub strikethrough: bool,
}

impl Default for TextProps {
    fn default() -> Self {
        Self {
            font_size: d_font_size(),
            font_family: d_font_family(),
            weight: d_weight(),
            color: d_color(),
            italic: false,
            align: Align::Left,
            wrap: Wrap::Single,
            hyphenate: false,
            line_height: d_line_height(),
            max_lines: 0,
            overflow: Overflow::Clip,
            v_align: VAlign::Middle,
            text_case: TextCase::None,
            script_pos: ScriptPos::None,
            underline: false,
            strikethrough: false,
        }
    }
}

/// Properties of a photo field. `zoom`/`offset_*` are the field-level fallback used when a photo has
/// neither an automatic nor a manual crop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PhotoProps {
    pub shape: Shape,
    #[serde(default = "d_border_w")]
    pub border_w: f64,
    #[serde(default = "d_color")]
    pub border_color: Rgba,
    #[serde(default = "d_zoom")]
    pub zoom: f64,
    pub offset_x: f64,
    pub offset_y: f64,
}

impl Default for PhotoProps {
    fn default() -> Self {
        Self { shape: Shape::Rect, border_w: d_border_w(), border_color: d_color(), zoom: d_zoom(), offset_x: 0.0, offset_y: 0.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FieldKind {
    Text(TextProps),
    Photo(PhotoProps),
}

/// A field on the template. Geometry is in percent of the template (0..100).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default = "d_w")]
    pub w: f64,
    #[serde(default = "d_h")]
    pub h: f64,
    /// 0..=100.
    #[serde(default = "d_opacity")]
    pub opacity: f64,
    /// Degrees, -180..=180, clockwise.
    #[serde(default)]
    pub rotation: f64,
    #[serde(flatten)]
    pub kind: FieldKind,
}

fn d_w() -> f64 {
    20.0
}
fn d_h() -> f64 {
    10.0
}

impl Field {
    pub fn new_text(id: String) -> Self {
        Self {
            id,
            name: "Field".into(),
            x: 10.0,
            y: 10.0,
            w: 40.0,
            h: 8.0,
            opacity: 100.0,
            rotation: 0.0,
            kind: FieldKind::Text(TextProps::default()),
        }
    }
    pub fn new_photo(id: String) -> Self {
        Self {
            id,
            name: "Photo".into(),
            x: 5.0,
            y: 5.0,
            w: 22.0,
            h: 28.0,
            opacity: 100.0,
            rotation: 0.0,
            kind: FieldKind::Photo(PhotoProps::default()),
        }
    }
    pub fn is_text(&self) -> bool {
        matches!(self.kind, FieldKind::Text(_))
    }
    pub fn is_photo(&self) -> bool {
        matches!(self.kind, FieldKind::Photo(_))
    }
    pub fn text(&self) -> Option<&TextProps> {
        match &self.kind {
            FieldKind::Text(t) => Some(t),
            _ => None,
        }
    }
    pub fn text_mut(&mut self) -> Option<&mut TextProps> {
        match &mut self.kind {
            FieldKind::Text(t) => Some(t),
            _ => None,
        }
    }
    pub fn photo(&self) -> Option<&PhotoProps> {
        match &self.kind {
            FieldKind::Photo(p) => Some(p),
            _ => None,
        }
    }
    pub fn photo_mut(&mut self) -> Option<&mut PhotoProps> {
        match &mut self.kind {
            FieldKind::Photo(p) => Some(p),
            _ => None,
        }
    }
    /// Width / height of the box (the aspect photos are cropped to).
    pub fn aspect(&self) -> f64 {
        if self.h > 0.0 { self.w / self.h } else { 1.0 }
    }
    /// The edges (percent) smart guides snap to: left, centre, right / top, middle, bottom.
    pub fn edges_x(&self) -> [f64; 3] {
        [self.x, self.x + self.w / 2.0, self.x + self.w]
    }
    pub fn edges_y(&self) -> [f64; 3] {
        [self.y, self.y + self.h / 2.0, self.y + self.h]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_text_field() {
        let j = r##"{"id":"f1","type":"text","name":"Name","x":10,"y":10,"w":40,"h":8,"fontSize":18,"fontFamily":"Georgia, serif",
            "weight":400,"color":"#111111","italic":false,"align":"left","wrap":"single","hyphenate":false,"lineHeight":1.3,
            "maxLines":0,"overflow":"clip","vAlign":"middle","textCase":"none","scriptPos":"none","underline":false,
            "strikethrough":false,"opacity":100,"rotation":0}"##;
        let f: Field = serde_json::from_str(j).unwrap();
        assert!(f.is_text());
        assert_eq!(f.text().unwrap().font_size, 18.0);
        let back: Field = serde_json::from_str(&serde_json::to_string(&f).unwrap()).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn legacy_photo_field_and_defaults() {
        let j = r##"{"id":"f2","type":"photo","name":"Photo","x":5,"y":5,"w":22,"h":28,"shape":"circle","borderW":2,"borderColor":"#111111","zoom":100,"offsetX":0,"offsetY":0}"##;
        let f: Field = serde_json::from_str(j).unwrap();
        assert_eq!(f.photo().unwrap().shape, Shape::Circle);
        assert_eq!(f.opacity, 100.0);
    }

    #[test]
    fn lenient_enums() {
        let t: TextProps = serde_json::from_str(r#"{"align":"diagonal","overflow":null,"wrap":"MULTI"}"#).unwrap();
        assert_eq!(t.align, Align::Left);
        assert_eq!(t.overflow, Overflow::Clip);
        assert_eq!(t.wrap, Wrap::Multi);
        assert_eq!(t.font_size, 18.0);
    }

    #[test]
    fn justifyall_round_trips() {
        let t = TextProps { align: Align::JustifyAll, ..TextProps::default() };
        let s = serde_json::to_string(&t).unwrap();
        assert!(s.contains("\"justifyall\""));
        let back: TextProps = serde_json::from_str(&s).unwrap();
        assert_eq!(back.align, Align::JustifyAll);
    }
}

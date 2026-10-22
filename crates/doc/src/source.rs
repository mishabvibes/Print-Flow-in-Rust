//! Where an image's bytes come from. Serialises as a single string, exactly like the original app's
//! project files: a `data:` URL for embedded images, a file path otherwise.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{DocError, DocResult};

#[derive(Clone, Debug, Default)]
pub enum ImageSource {
    #[default]
    None,
    /// A file on disk (read on demand; nothing is held in memory).
    Path(PathBuf),
    /// Bytes embedded in the project (legacy `data:` URLs, pasted images).
    Data(Arc<Vec<u8>>),
}

impl PartialEq for ImageSource {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::None, Self::None) => true,
            (Self::Path(a), Self::Path(b)) => a == b,
            (Self::Data(a), Self::Data(b)) => Arc::ptr_eq(a, b) || a == b,
            _ => false,
        }
    }
}

impl ImageSource {
    pub fn from_path(p: impl Into<PathBuf>) -> Self {
        Self::Path(p.into())
    }
    pub fn from_bytes(b: Vec<u8>) -> Self {
        Self::Data(Arc::new(b))
    }
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Path(p) => Some(p),
            _ => None,
        }
    }
    /// The image file's bytes.
    pub fn bytes(&self) -> DocResult<Cow<'_, [u8]>> {
        match self {
            Self::None => Err(DocError::Other("no image".into())),
            Self::Data(d) => Ok(Cow::Borrowed(d.as_slice())),
            Self::Path(p) => std::fs::read(p).map(Cow::Owned).map_err(|source| DocError::Io { path: p.clone(), source }),
        }
    }
    /// A short label for lists and error messages.
    pub fn label(&self) -> String {
        match self {
            Self::None => "(none)".into(),
            Self::Path(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()),
            Self::Data(d) => format!("(embedded, {} KB)", d.len() / 1024),
        }
    }
    /// Does the source still resolve (file exists / bytes present)?
    pub fn is_available(&self) -> bool {
        match self {
            Self::None => false,
            Self::Data(_) => true,
            Self::Path(p) => p.is_file(),
        }
    }
    /// Reads a path source into an embedded one (used when saving a self-contained project).
    pub fn embedded(&self) -> DocResult<Self> {
        match self {
            Self::Path(_) => Ok(Self::Data(Arc::new(self.bytes()?.into_owned()))),
            other => Ok(other.clone()),
        }
    }
    /// The `data:` URL for the image (MIME type sniffed from the magic bytes).
    pub fn to_data_url(&self) -> DocResult<String> {
        let bytes = self.bytes()?;
        Ok(format!("data:{};base64,{}", sniff_mime(&bytes), base64::engine::general_purpose::STANDARD.encode(bytes.as_ref())))
    }
    pub fn from_data_url(s: &str) -> DocResult<Self> {
        let rest = s.strip_prefix("data:").ok_or_else(|| DocError::DataUrl("missing data: prefix".into()))?;
        let (meta, payload) = rest.split_once(',').ok_or_else(|| DocError::DataUrl("missing comma".into()))?;
        if !meta.ends_with(";base64") {
            return Err(DocError::DataUrl("only base64 data URLs are supported".into()));
        }
        // Browsers allow line breaks / URL-encoding in the payload; be lenient.
        let cleaned: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
        let cleaned = cleaned.replace("%2B", "+").replace("%2F", "/").replace("%3D", "=");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(cleaned.trim_end_matches('='))
            .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(cleaned.trim_end_matches('=')))
            .map_err(|e| DocError::DataUrl(e.to_string()))?;
        Ok(Self::from_bytes(bytes))
    }
}

pub fn sniff_mime(b: &[u8]) -> &'static str {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if b.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if b.starts_with(b"GIF8") {
        "image/gif"
    } else if b.starts_with(b"BM") {
        "image/bmp"
    } else if b.len() > 12 && b.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

impl Serialize for ImageSource {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::None => s.serialize_str(""),
            Self::Path(p) => s.serialize_str(&p.to_string_lossy()),
            Self::Data(_) => match self.to_data_url() {
                Ok(u) => s.serialize_str(&u),
                Err(e) => Err(serde::ser::Error::custom(e.to_string())),
            },
        }
    }
}

impl<'de> Deserialize<'de> for ImageSource {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = Option::<String>::deserialize(d)?.unwrap_or_default();
        if s.is_empty() {
            Ok(Self::None)
        } else if s.starts_with("data:") {
            // A corrupt embedded image must not make the whole project unreadable.
            Ok(Self::from_data_url(&s).unwrap_or(Self::None))
        } else {
            Ok(Self::Path(PathBuf::from(s)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_url_round_trip() {
        let png = [0x89, b'P', b'N', b'G', 1, 2, 3, 4];
        let src = ImageSource::from_bytes(png.to_vec());
        let url = src.to_data_url().unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        let back = ImageSource::from_data_url(&url).unwrap();
        assert_eq!(back.bytes().unwrap().as_ref(), &png);
    }

    #[test]
    fn serde_forms() {
        let s: ImageSource = serde_json::from_str("\"C:/a/b.jpg\"").unwrap();
        assert_eq!(s.path(), Some(Path::new("C:/a/b.jpg")));
        let n: ImageSource = serde_json::from_str("null").unwrap();
        assert!(n.is_none());
        let bad: ImageSource = serde_json::from_str("\"data:image/png;base64,@@@\"").unwrap();
        assert!(bad.is_none());
        assert_eq!(serde_json::to_string(&ImageSource::None).unwrap(), "\"\"");
    }
}

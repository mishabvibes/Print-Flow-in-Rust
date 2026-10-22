//! Small helpers shared by the data and file handling.

/// Image file extensions the app reads.
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "jpe", "jfif", "gif", "webp", "bmp", "tif", "tiff"];

/// Is `name` the file name of an image the app can read (by extension)?
pub fn is_image_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| IMAGE_EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext)))
}

/// The key photos are matched on: trimmed, lower-cased, without a trailing image extension.
/// `"  IMG_001.JPG "` and `"img_001"` give the same key.
///
/// Unlike the previous version this only strips *image* extensions, so a roll number such as
/// `"12.5"` or a name like `"Mr. Smith"` keeps its dot instead of being cut at it.
pub fn normalize_key(v: &str) -> String {
    let s = v.trim().to_lowercase();
    if let Some((stem, ext)) = s.rsplit_once('.')
        && !stem.is_empty()
        && IMAGE_EXTENSIONS.contains(&ext)
    {
        return stem.to_string();
    }
    s
}

/// A file-name-safe version of `s`: characters that are illegal in file names on Windows, macOS or
/// Linux (`\ / : * ? " < > |` and control characters) become `_`. Letters of every script are kept, so a
/// card for "രാജു" is exported as `രാജു.png`, not `____.png` (the previous version replaced them).
pub fn sanitize_filename(s: &str) -> String {
    let out: String =
        s.chars().map(|c| if c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
    let t = out.trim().trim_end_matches('.').trim();
    if t.is_empty() {
        return "card".to_string();
    }
    // Windows reserves these device names, with or without an extension.
    let stem = t.split('.').next().unwrap_or(t).to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit());
    let mut t = if reserved { format!("_{t}") } else { t.to_string() };
    // Keep names well inside every file system's 255-byte limit.
    while t.len() > 120 {
        t.pop();
    }
    t
}

/// Makes names unique within one archive: the second `a.png` becomes `a_2.png`, and so on.
#[derive(Default)]
pub struct UniqueNames {
    seen: std::collections::HashMap<String, usize>,
}

impl UniqueNames {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn unique(&mut self, name: &str) -> String {
        let key = name.to_lowercase();
        let n = self.seen.entry(key).or_insert(0);
        *n += 1;
        if *n == 1 {
            return name.to_string();
        }
        let count = *n;
        match name.rfind('.') {
            Some(dot) => format!("{}_{}{}", &name[..dot], count, &name[dot..]),
            None => format!("{name}_{count}"),
        }
    }
}

/// Natural ordering like `localeCompare(..., {numeric: true, sensitivity: 'base'})`: digits compare by
/// value, letters case-insensitively. `img2` < `img10`.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(&c) = ai.peek() {
                    if c.is_ascii_digit() {
                        na.push(c);
                        ai.next();
                    } else {
                        break;
                    }
                }
                let mut nb = String::new();
                while let Some(&c) = bi.peek() {
                    if c.is_ascii_digit() {
                        nb.push(c);
                        bi.next();
                    } else {
                        break;
                    }
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let (lx, ly) = (x.to_lowercase().next().unwrap_or(x), y.to_lowercase().next().unwrap_or(y));
                if lx != ly {
                    return lx.cmp(&ly);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(normalize_key("  IMG_001.JPG "), "img_001");
        assert_eq!(normalize_key("A.B.png"), "a.b");
        assert_eq!(normalize_key("1001"), "1001");
        assert_eq!(normalize_key("12.5"), "12.5");
        assert_eq!(normalize_key("Mr. Smith"), "mr. smith");
        assert_eq!(normalize_key(".jpg"), ".jpg");
        assert!(is_image_name("a.JPG") && !is_image_name("a.txt") && !is_image_name("jpg"));
        assert_eq!(normalize_key(""), "");
    }

    #[test]
    fn filenames() {
        assert_eq!(sanitize_filename("a/b:c"), "a_b_c");
        assert_eq!(sanitize_filename("  "), "card");
        assert_eq!(sanitize_filename("രാജു"), "രാജു");
        assert_eq!(sanitize_filename("CON"), "_CON");
        assert_eq!(sanitize_filename("a."), "a");
        assert!(sanitize_filename(&"x".repeat(500)).len() <= 120);
    }

    #[test]
    fn unique() {
        let mut u = UniqueNames::new();
        assert_eq!(u.unique("a.png"), "a.png");
        assert_eq!(u.unique("a.png"), "a_2.png");
        assert_eq!(u.unique("A.png"), "A_3.png");
    }

    #[test]
    fn natural() {
        use std::cmp::Ordering::*;
        assert_eq!(natural_cmp("img2", "img10"), Less);
        assert_eq!(natural_cmp("IMG2", "img2"), Equal);
        assert_eq!(natural_cmp("a", "a1"), Less);
    }
}

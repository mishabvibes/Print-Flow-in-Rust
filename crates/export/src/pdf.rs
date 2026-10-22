//! A small streaming PDF writer: one page per sheet, cards placed as image XObjects at exact
//! positions (so cut-guide borders stay vector and crisp), written to disk page by page. Memory use is
//! one page, not the whole job.

use std::io::{self, Write};

use flate2::Compression;
use flate2::write::ZlibEncoder;

/// How an image's pixels are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfCodec {
    /// Lossy, small (DCT).
    Jpeg,
    /// Lossless (Flate).
    Flate,
}

pub struct PdfImage {
    pub px_w: u32,
    pub px_h: u32,
    pub codec: PdfCodec,
    /// JPEG file bytes, or zlib-compressed 8-bit RGB.
    pub data: Vec<u8>,
    /// Position from the page's top-left, in points.
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub struct PdfBorder {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

struct Counting<W: Write> {
    inner: W,
    pos: u64,
}

impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.pos += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub struct PdfWriter<W: Write> {
    out: Counting<W>,
    /// Byte offset of object `n` at index `n - 1`.
    offsets: Vec<u64>,
    pages: Vec<u32>,
}

const CATALOG: u32 = 1;
const PAGES: u32 = 2;

fn fmt(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Zlib-compresses 8-bit RGB pixels taken from RGBA (alpha dropped: callers flatten first).
pub fn deflate_rgb(rgba: &[u8]) -> io::Result<Vec<u8>> {
    let mut z = ZlibEncoder::new(Vec::with_capacity(rgba.len() / 4), Compression::new(4));
    let mut row = Vec::with_capacity(3 * 4096);
    for chunk in rgba.chunks(4 * 4096) {
        row.clear();
        for px in chunk.chunks_exact(4) {
            row.extend_from_slice(&px[..3]);
        }
        z.write_all(&row)?;
    }
    z.finish()
}

impl<W: Write> PdfWriter<W> {
    pub fn new(inner: W) -> io::Result<Self> {
        let mut out = Counting { inner, pos: 0 };
        out.write_all(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n")?;
        Ok(Self { out, offsets: vec![0, 0], pages: Vec::new() })
    }

    fn alloc(&mut self) -> u32 {
        self.offsets.push(0);
        self.offsets.len() as u32
    }

    fn begin(&mut self, id: u32) -> io::Result<()> {
        if let Some(o) = self.offsets.get_mut(id as usize - 1) {
            *o = self.out.pos;
        }
        writeln!(self.out, "{id} 0 obj")
    }

    /// Adds a page of `w_pt` x `h_pt` points.
    pub fn add_page(
        &mut self,
        w_pt: f64,
        h_pt: f64,
        images: &[PdfImage],
        borders: &[PdfBorder],
        border_width: f64,
        border_rgb: [u8; 3],
    ) -> io::Result<()> {
        let mut xobjs = String::new();
        let mut content = String::new();
        let mut image_ids = Vec::new();
        for (i, im) in images.iter().enumerate() {
            let id = self.alloc();
            image_ids.push(id);
            self.begin(id)?;
            let filter = if im.codec == PdfCodec::Jpeg { "/DCTDecode" } else { "/FlateDecode" };
            write!(
                self.out,
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter {} /Length {} >>\nstream\n",
                im.px_w,
                im.px_h,
                filter,
                im.data.len()
            )?;
            self.out.write_all(&im.data)?;
            self.out.write_all(b"\nendstream\nendobj\n")?;
            xobjs.push_str(&format!("/Im{i} {id} 0 R "));
            content.push_str(&format!("q {} 0 0 {} {} {} cm /Im{i} Do Q\n", fmt(im.w), fmt(im.h), fmt(im.x), fmt(h_pt - im.y - im.h)));
        }
        if !borders.is_empty() && border_width > 0.0 {
            let c = |v: u8| fmt(f64::from(v) / 255.0);
            content.push_str(&format!("{} {} {} RG {} w\n", c(border_rgb[0]), c(border_rgb[1]), c(border_rgb[2]), fmt(border_width)));
            for b in borders {
                content.push_str(&format!("{} {} {} {} re S\n", fmt(b.x), fmt(h_pt - b.y - b.h), fmt(b.w), fmt(b.h)));
            }
        }
        let content_id = self.alloc();
        self.begin(content_id)?;
        write!(self.out, "<< /Length {} >>\nstream\n{}endstream\nendobj\n", content.len(), content)?;
        let page_id = self.alloc();
        self.begin(page_id)?;
        write!(
            self.out,
            "<< /Type /Page /Parent {PAGES} 0 R /MediaBox [0 0 {} {}] /Resources << /XObject << {} >> >> /Contents {content_id} 0 R >>\nendobj\n",
            fmt(w_pt),
            fmt(h_pt),
            xobjs
        )?;
        self.pages.push(page_id);
        Ok(())
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Writes the page tree, catalog and cross-reference table, and returns the inner writer.
    pub fn finish(mut self) -> io::Result<W> {
        self.begin(PAGES)?;
        let kids: Vec<String> = self.pages.iter().map(|p| format!("{p} 0 R")).collect();
        write!(self.out, "<< /Type /Pages /Count {} /Kids [{}] >>\nendobj\n", self.pages.len(), kids.join(" "))?;
        self.begin(CATALOG)?;
        write!(self.out, "<< /Type /Catalog /Pages {PAGES} 0 R >>\nendobj\n")?;
        let xref = self.out.pos;
        write!(self.out, "xref\n0 {}\n0000000000 65535 f \n", self.offsets.len() + 1)?;
        for o in &self.offsets {
            writeln!(self.out, "{o:010} 00000 n ")?;
        }
        write!(self.out, "trailer\n<< /Size {} /Root {CATALOG} 0 R >>\nstartxref\n{xref}\n%%EOF\n", self.offsets.len() + 1)?;
        self.out.flush()?;
        Ok(self.out.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_well_formed_file() {
        let mut w = PdfWriter::new(Vec::new()).unwrap();
        let img =
            PdfImage { px_w: 1, px_h: 1, codec: PdfCodec::Flate, data: deflate_rgb(&[255, 0, 0, 255]).unwrap(), x: 10.0, y: 10.0, w: 50.0, h: 30.0 };
        w.add_page(200.0, 100.0, &[img], &[PdfBorder { x: 10.0, y: 10.0, w: 50.0, h: 30.0 }], 0.5, [0, 0, 0]).unwrap();
        w.add_page(200.0, 100.0, &[], &[], 0.0, [0, 0, 0]).unwrap();
        let bytes = w.finish().unwrap();
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        let find = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).rposition(|w| w == needle);
        assert!(find(&bytes, b"/Count 2").is_some());
        let sx = find(&bytes, b"startxref\n").unwrap() + 10;
        let xref_pos: usize = std::str::from_utf8(&bytes[sx..]).unwrap().lines().next().unwrap().parse().unwrap();
        assert!(bytes[xref_pos..].starts_with(b"xref"));
        // Objects: catalog, pages, image, content, page, content, page = 7.
        let table = std::str::from_utf8(&bytes[xref_pos..]).unwrap();
        let entries: Vec<&str> = table.lines().skip(3).take(7).collect();
        for (i, line) in entries.iter().enumerate() {
            let off: usize = line[..10].parse().unwrap();
            assert!(bytes[off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "obj {} at {off}", i + 1);
        }
    }
}

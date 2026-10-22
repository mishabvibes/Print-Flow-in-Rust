//! A minimal, bounds-checked FlatBuffers reader: just enough for TFLite model files. Every accessor
//! returns `None`/a default on malformed data, so a corrupt model can never panic.

#[derive(Clone, Copy)]
pub struct Table<'a> {
    buf: &'a [u8],
    pos: usize,
}

fn rd_u16(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(p..p.checked_add(2)?)?.try_into().ok()?))
}
fn rd_u32(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(p..p.checked_add(4)?)?.try_into().ok()?))
}
fn rd_i32(b: &[u8], p: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(p..p.checked_add(4)?)?.try_into().ok()?))
}

impl<'a> Table<'a> {
    pub fn root(buf: &'a [u8]) -> Option<Self> {
        let pos = rd_u32(buf, 0)? as usize;
        Some(Self { buf, pos })
    }

    fn field(&self, idx: usize) -> Option<usize> {
        let vt = (self.pos as i64).checked_sub(rd_i32(self.buf, self.pos)? as i64)?;
        let vt = usize::try_from(vt).ok()?;
        let vt_len = rd_u16(self.buf, vt)? as usize;
        let slot = 4 + 2 * idx;
        if slot + 2 > vt_len {
            return None;
        }
        let off = rd_u16(self.buf, vt + slot)? as usize;
        if off == 0 { None } else { self.pos.checked_add(off) }
    }

    pub fn u8(&self, idx: usize, default: u8) -> u8 {
        self.field(idx).and_then(|p| self.buf.get(p).copied()).unwrap_or(default)
    }
    pub fn i32(&self, idx: usize, default: i32) -> i32 {
        self.field(idx).and_then(|p| rd_i32(self.buf, p)).unwrap_or(default)
    }
    pub fn table(&self, idx: usize) -> Option<Table<'a>> {
        let p = self.field(idx)?;
        Some(Table { buf: self.buf, pos: p.checked_add(rd_u32(self.buf, p)? as usize)? })
    }
    /// (start of elements, length) of a vector field.
    fn vec(&self, idx: usize) -> Option<(usize, usize)> {
        let p = self.field(idx)?;
        let v = p.checked_add(rd_u32(self.buf, p)? as usize)?;
        Some((v + 4, rd_u32(self.buf, v)? as usize))
    }
    pub fn vec_len(&self, idx: usize) -> usize {
        self.vec(idx).map_or(0, |v| v.1)
    }
    pub fn vec_table(&self, idx: usize, i: usize) -> Option<Table<'a>> {
        let (s, n) = self.vec(idx)?;
        if i >= n {
            return None;
        }
        let p = s.checked_add(i.checked_mul(4)?)?;
        Some(Table { buf: self.buf, pos: p.checked_add(rd_u32(self.buf, p)? as usize)? })
    }
    pub fn vec_i32(&self, idx: usize) -> Vec<i32> {
        let Some((s, n)) = self.vec(idx) else { return vec![] };
        (0..n.min(1 << 20)).filter_map(|i| rd_i32(self.buf, s + i * 4)).collect()
    }
    pub fn vec_bytes(&self, idx: usize) -> &'a [u8] {
        let Some((s, n)) = self.vec(idx) else { return &[] };
        s.checked_add(n).and_then(|e| self.buf.get(s..e)).unwrap_or(&[])
    }
}

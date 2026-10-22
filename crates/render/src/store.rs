//! Decoded-image cache. Decoding a photo dominates card rendering, and the editor re-renders the same
//! card on every tweak, so decoded sources are kept (within a byte budget) and shared.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use forge_doc::ImageSource;
use forge_image::Image;

fn key_of(src: &ImageSource) -> Option<String> {
    match src {
        ImageSource::None => None,
        ImageSource::Path(p) => Some(format!("p:{}", p.display())),
        ImageSource::Data(d) => Some(format!("d:{:p}:{}", Arc::as_ptr(d), d.len())),
    }
}

struct Slot {
    img: Arc<Image>,
    tick: u64,
}

pub struct ImageStore {
    budget: usize,
    inner: Mutex<(HashMap<String, Slot>, u64, usize)>,
}

impl ImageStore {
    pub fn new(budget_bytes: usize) -> Self {
        Self { budget: budget_bytes, inner: Mutex::new((HashMap::new(), 0, 0)) }
    }

    /// Decodes `src` (or returns the cached decode). `cache = false` is for batch jobs that touch each
    /// image once.
    pub fn load(&self, src: &ImageSource, cache: bool) -> Result<Arc<Image>, String> {
        let key = key_of(src).ok_or("no image")?;
        if cache {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.1 += 1;
            let tick = g.1;
            if let Some(s) = g.0.get_mut(&key) {
                s.tick = tick;
                return Ok(s.img.clone());
            }
        }
        let bytes = src.bytes().map_err(|e| e.to_string())?;
        let img = Arc::new(forge_image::decode(&bytes).map_err(|e| e.to_string())?);
        if cache {
            let size = img.rgba.len();
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if size <= self.budget {
                while g.2 + size > self.budget {
                    let Some(oldest) = g.0.iter().min_by_key(|(_, s)| s.tick).map(|(k, _)| k.clone()) else { break };
                    if let Some(s) = g.0.remove(&oldest) {
                        g.2 -= s.img.rgba.len();
                    }
                }
                g.1 += 1;
                let tick = g.1;
                g.2 += size;
                g.0.insert(key, Slot { img: img.clone(), tick });
            }
        }
        Ok(img)
    }

    pub fn clear(&self) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.0.clear();
        g.2 = 0;
    }
}

//! Background work. The UI thread never decodes, detects, renders sheets or exports: it sends a job
//! here and polls the results each frame. Preview jobs are "latest wins": a request that was
//! superseded before it started does nothing.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use forge_engine::jobs::DetectOutcome;
use forge_export::Report;
use forge_image::Image;

pub enum Msg {
    /// A rendered preview: which slot, the request token and the content key it was made for.
    Preview {
        slot: Slot,
        token: u64,
        key: u64,
        image: Result<Image, String>,
    },
    /// A decoded photo for the Crop tab.
    Photo {
        key: String,
        version: u64,
        full: bool,
        image: Result<Image, String>,
    },
    Detected(Vec<DetectOutcome>),
    DetectProgress(usize, usize),
    ExportProgress(usize, usize),
    ExportDone(Result<Report, String>),
    FontsReady,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Design,
    Sheet,
}

pub struct Workers {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ctx: egui::Context,
}

impl Workers {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, ctx }
    }

    /// Runs `f` on the shared thread pool; `f` reports through the sender it is given.
    pub fn spawn(&self, f: impl FnOnce(&Sender<Msg>) + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        rayon::spawn(move || {
            f(&tx);
            ctx.request_repaint();
        });
    }

    pub fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
    }

    pub fn repaint_handle(&self) -> egui::Context {
        self.ctx.clone()
    }

    pub fn poll(&self) -> Vec<Msg> {
        self.rx.try_iter().collect()
    }
}

/// Tracks one "latest wins" preview: what is on screen and what has been asked for.
pub struct Preview {
    pub texture: Option<egui::TextureHandle>,
    /// Content key of the texture on screen.
    pub shown: Option<u64>,
    requested: Option<u64>,
    token: u64,
    latest: Arc<AtomicU64>,
    pub error: Option<String>,
    pub size: [usize; 2],
}

impl Default for Preview {
    fn default() -> Self {
        Self { texture: None, shown: None, requested: None, token: 0, latest: Arc::new(AtomicU64::new(0)), error: None, size: [0, 0] }
    }
}

impl Preview {
    /// Asks for `key` unless it is already shown or in flight. `make` runs on a worker.
    pub fn request(&mut self, w: &Workers, slot: Slot, key: u64, make: impl FnOnce() -> Result<Image, String> + Send + 'static) {
        if self.shown == Some(key) || self.requested == Some(key) {
            return;
        }
        self.token += 1;
        self.requested = Some(key);
        let token = self.token;
        self.latest.store(token, Ordering::SeqCst);
        let latest = self.latest.clone();
        w.spawn(move |tx| {
            if latest.load(Ordering::SeqCst) != token {
                return; // superseded before it began
            }
            let image = make();
            let _ = tx.send(Msg::Preview { slot, token, key, image });
        });
    }

    pub fn accept(&mut self, ctx: &egui::Context, name: &str, token: u64, key: u64, image: Result<Image, String>) {
        if token != self.token {
            return;
        }
        self.requested = None;
        match image {
            Ok(img) => {
                self.size = [img.width as usize, img.height as usize];
                let ci = egui::ColorImage::from_rgba_unmultiplied(self.size, &img.rgba);
                self.texture = Some(ctx.load_texture(name, ci, egui::TextureOptions::LINEAR));
                self.shown = Some(key);
                self.error = None;
            }
            Err(e) => {
                self.error = Some(e);
                self.shown = Some(key);
            }
        }
    }

    pub fn pending(&self) -> bool {
        self.requested.is_some()
    }

    pub fn token(&self) -> u64 {
        self.token
    }
}

pub fn texture_from(ctx: &egui::Context, name: &str, img: &Image) -> egui::TextureHandle {
    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.rgba);
    ctx.load_texture(name, ci, egui::TextureOptions::LINEAR)
}

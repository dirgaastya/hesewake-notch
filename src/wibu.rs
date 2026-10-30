//! Wibu mode: an image the user picked, behind the expanded notch (under the glow and the slides). Off by default.
//! The file stays where it is (only its path is saved). It is decoded and scaled to the canvas on a worker thread, so
//! a big wallpaper never blocks the UI, and only the small scaled copy is kept. A missing or broken file leaves the
//! notch black and the Settings row says so.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// Messages to the window: the file dialog closed (`take_picked`), the image is ready (`take_loaded`).
pub(crate) const WM_WIBU_FILE: u32 = WM_APP + 18;
pub(crate) const WM_WIBU_READY: u32 = WM_APP + 19;

/// "Image dim" choices, in percent. Below 50 white text gets hard to read on a bright image.
const DIMS: [u8; 4] = [50, 60, 70, 80];
pub(crate) const DEFAULT_DIM: u8 = 70;

/// Bigger files are refused. Wallpapers go past `decode_limited`'s 32 MB, so this has its own, higher limits; the
/// full-size image only lives on the worker thread.
const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_ALLOC: u64 = 256 * 1024 * 1024;

/// The nearest "Image dim" choice.
pub(crate) fn snap_dim(n: u8) -> u8 {
    DIMS.into_iter().min_by_key(|d| d.abs_diff(n)).unwrap_or(DEFAULT_DIM)
}

/// Next or previous "Image dim" choice, stopping at the ends.
pub(crate) fn step_dim(dim: u8, up: bool) -> u8 {
    let i = DIMS.iter().position(|&d| d == snap_dim(dim)).unwrap_or(0);
    DIMS[if up { (i + 1).min(DIMS.len() - 1) } else { i.saturating_sub(1) }]
}

/// Decode `path` and scale it to fill `w` x `h`, cropping what sticks out. Transparent parts become black, the notch's
/// own color, so the result is opaque. None if the file is missing, too big or not a readable image.
fn load(path: &str, w: u32, h: u32) -> Option<Pixmap> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_FILE {
        return None;
    }
    let mut r = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let mut lim = image::Limits::default();
    lim.max_alloc = Some(MAX_ALLOC);
    r.limits(lim);
    let img = r.decode().ok()?.resize_to_fill(w, h, image::imageops::FilterType::CatmullRom).to_rgba8();
    let mut pm = Pixmap::new(w, h)?;
    for (d, s) in pm.data_mut().chunks_exact_mut(4).zip(img.as_raw().chunks_exact(4)) {
        let al = s[3] as u32;
        d[0] = (s[0] as u32 * al / 255) as u8;
        d[1] = (s[1] as u32 * al / 255) as u8;
        d[2] = (s[2] as u32 * al / 255) as u8;
        d[3] = 255;
    }
    Some(pm)
}

/// The file the user picked (Some(None) = the dialog was cancelled).
static PICKED: filepick::Picked = Mutex::new(None);

/// Show the Open dialog for an image; `WM_WIBU_FILE` reports back.
pub(crate) fn pick_file(hwnd: isize) {
    filepick::open(hwnd, tr("Wibu image"), "Images (*.jpg, *.jpeg, *.png, *.webp)", "*.jpg;*.jpeg;*.png;*.webp", &PICKED, WM_WIBU_FILE);
}

/// The answer of the file dialog: Some(Some(path)), Some(None) when cancelled, None when nothing is waiting.
pub(crate) fn take_picked() -> Option<Option<String>> {
    PICKED.lock().ok()?.take()
}

/// The newest load; a worker whose number is no longer current drops its result (the file or scale changed meanwhile).
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// The result of the worker that finished last.
static LOADED: Mutex<Option<Option<Pixmap>>> = Mutex::new(None);

/// What the worker left: Some(Some(image)), Some(None) when the file could not be read, None when nothing is waiting.
pub(crate) fn take_loaded() -> Option<Option<Pixmap>> {
    LOADED.lock().ok()?.take()
}

#[derive(Default)]
pub(crate) struct Wibu {
    /// The image at canvas size, opaque and not yet darkened.
    pub(crate) bg: Option<Pixmap>,
    /// The picked file could not be read (the Settings row shows it).
    pub(crate) failed: bool,
}

impl App {
    /// Load or release the image to match the settings. An image already showing stays until the new one is ready.
    pub(crate) fn wibu_apply(&mut self) {
        let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
        if !self.cfg.wibu || self.cfg.wibu_file.is_empty() {
            self.wibu = Wibu::default();
            return;
        }
        let (w, h) = scale::canvas(self.scale);
        let path = self.cfg.wibu_file.clone();
        if cfg!(test) {
            self.wibu_loaded(load(&path, w as u32, h as u32));
            return;
        }
        let hwnd = self.hwnd;
        std::thread::spawn(move || {
            let img = load(&path, w as u32, h as u32);
            if GENERATION.load(Ordering::Relaxed) != generation {
                return;
            }
            if let Ok(mut l) = LOADED.lock() {
                *l = Some(img);
            }
            unsafe {
                let _ = PostMessageW(HWND(hwnd as *mut c_void), WM_WIBU_READY, WPARAM(0), LPARAM(0));
            }
        });
    }

    /// The worker finished.
    pub(crate) fn wibu_loaded(&mut self, img: Option<Pixmap>) {
        if !self.cfg.wibu {
            return; // turned off while it was loading
        }
        self.wibu.failed = img.is_none();
        self.wibu.bg = img;
    }

    /// The file dialog answered: keep the file, or turn the mode off when cancelled with no file to show.
    pub(crate) fn wibu_picked(&mut self, picked: Option<String>) {
        match picked {
            Some(path) => {
                self.cfg.wibu_file = path;
                self.cfg.wibu = true;
                self.wibu_apply();
            }
            None if self.cfg.wibu_file.is_empty() => self.cfg.wibu = false,
            None => {}
        }
    }

    /// Paint the image over the notch's black fill, darkened and fading in with the expand progress `p`. Under it is
    /// only black, so scaling the colors is enough; `render::apply_mask` cuts the result to the notch shape.
    pub(crate) fn draw_wibu(&self, g: &mut Gfx, p: f32) {
        let Some(bg) = self.wibu.bg.as_ref() else { return };
        if (bg.width(), bg.height()) != (g.pm.width(), g.pm.height()) {
            return; // scaled for the previous canvas; the new image is on its way
        }
        let bright = (100 - self.cfg.wibu_dim.min(100)) as f32 / 100.0;
        let k = (bright * p.clamp(0.0, 1.0) * 256.0) as u32;
        for (d, s) in g.pm.data_mut().chunks_exact_mut(4).zip(bg.data().chunks_exact(4)) {
            d[0] = ((s[0] as u32 * k) >> 8) as u8;
            d[1] = ((s[1] as u32 * k) >> 8) as u8;
            d[2] = ((s[2] as u32 * k) >> 8) as u8;
            d[3] = 255;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("hesewake-wibu-{}-{name}", std::process::id()))
    }

    /// A file of `w` x `h` pixels in one color.
    fn image_file(name: &str, w: u32, h: u32, rgba: [u8; 4]) -> std::path::PathBuf {
        let f = temp(name);
        image::RgbaImage::from_pixel(w, h, image::Rgba(rgba)).save(&f).unwrap();
        f
    }

    #[test]
    fn dim_choices() {
        assert_eq!(snap_dim(63), 60);
        assert_eq!(snap_dim(0), 50);
        assert_eq!(snap_dim(200), 80);
        assert_eq!(step_dim(70, true), 80);
        assert_eq!(step_dim(80, true), 80, "stops at the end");
        assert_eq!(step_dim(50, false), 50, "stops at the start");
    }

    #[test]
    fn png_and_webp_are_scaled_to_the_canvas() {
        for name in ["a.png", "a.webp"] {
            let f = image_file(name, 64, 64, [200, 100, 50, 255]);
            let pm = load(f.to_str().unwrap(), 810, 288).expect("readable image");
            assert_eq!((pm.width(), pm.height()), (810, 288));
            assert_eq!(&pm.data()[..4], &[200, 100, 50, 255], "{name}");
            let _ = std::fs::remove_file(&f);
        }
    }

    #[test]
    fn transparency_becomes_black() {
        let f = image_file("alpha.png", 8, 8, [255, 255, 255, 0]);
        let pm = load(f.to_str().unwrap(), 16, 16).unwrap();
        assert_eq!(&pm.data()[..4], &[0, 0, 0, 255]);
        let _ = std::fs::remove_file(&f);
    }

    /// The limit of `decode_limited` would refuse this one.
    #[test]
    fn a_wallpaper_bigger_than_the_cover_limit_loads() {
        let f = image_file("big.png", 5120, 2880, [10, 20, 30, 255]);
        assert!(load(f.to_str().unwrap(), 540, 192).is_some());
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn unreadable_files_give_nothing() {
        assert!(load("C:\\does\\not\\exist.png", 54, 19).is_none());
        let f = temp("broken.png");
        std::fs::write(&f, b"not an image").unwrap();
        assert!(load(f.to_str().unwrap(), 54, 19).is_none());
        std::fs::write(&f, b"").unwrap();
        assert!(load(f.to_str().unwrap(), 54, 19).is_none());
        let _ = std::fs::remove_file(&f);
    }
}

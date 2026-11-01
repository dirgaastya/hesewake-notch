//! Skala DPI. Semua ukuran di `config.rs` adalah piksel logis pada 100%; kanvas, window, dan mouse dikali skala ini.

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

/// Pilihan "Notch size" di Settings, dalam persen. 0 = Auto (ikut scaling Windows di monitor notch).
pub(crate) const SIZES: [u16; 6] = [0, 100, 125, 150, 175, 200];
pub(crate) const MIN: f32 = 1.0;
pub(crate) const MAX: f32 = 3.0;

/// Skala yang dipakai: flag `--scale=` menang, lalu pilihan manual, lalu DPI monitor (96 = 100%).
pub(crate) fn effective(size_idx: usize, dpi: u32, forced: Option<f32>) -> f32 {
    let s = match (forced, SIZES.get(size_idx)) {
        (Some(f), _) => f,
        (None, Some(&pct)) if pct > 0 => pct as f32 / 100.0,
        _ if dpi == 0 => 1.0,
        _ => dpi as f32 / 96.0,
    };
    s.clamp(MIN, MAX)
}

pub(crate) fn size_label(size_idx: usize) -> String {
    match SIZES.get(size_idx) {
        Some(&pct) if pct > 0 => format!("{pct}%"),
        _ => "Auto".to_string(),
    }
}

/// Pembulatan ke atas yang tahan galat float (540 * 1.1 = 594.00001 tetap 594).
fn ceil_px(v: f32) -> usize {
    (v - 0.001).ceil().max(0.0) as usize
}

/// Ukuran kanvas/window dalam piksel fisik.
pub(crate) fn canvas(s: f32) -> (i32, i32) {
    (ceil_px(W as f32 * s) as i32, ceil_px(H as f32 * s) as i32)
}

/// Kotak piksel logis -> kotak piksel fisik yang menutupinya penuh.
pub(crate) fn to_phys(b: Box4, s: f32) -> Box4 {
    let lo = |v: usize| (v as f32 * s + 0.001).floor() as usize;
    let hi = |v: usize| ceil_px(v as f32 * s);
    (lo(b.0), lo(b.1), hi(b.2), hi(b.3))
}

/// Nilai flag `--scale=1.5` (untuk mencoba skala lain tanpa mengubah setelan Windows).
pub(crate) fn forced_from_args<I: IntoIterator<Item = String>>(args: I) -> Option<f32> {
    args.into_iter().find_map(|a| a.strip_prefix("--scale=").and_then(|v| v.parse().ok()))
}

/// Skala aktif, untuk kode yang tidak memegang `App` (mis. koordinat mouse di wndproc). Awal: 1.0.
static CURRENT: AtomicU32 = AtomicU32::new(0x3f80_0000);

pub(crate) fn current() -> f32 {
    f32::from_bits(CURRENT.load(Ordering::Relaxed))
}

pub(crate) fn set_current(s: f32) {
    CURRENT.store(s.to_bits(), Ordering::Relaxed);
}

/// Kanvas baru dengan cache glyph yang sudah dipanaskan.
pub(crate) fn new_gfx(s: f32) -> Gfx {
    let mut g = Gfx::new(s);
    if let Some(t) = g.text.as_mut() {
        t.warm();
    }
    g
}

/// Bitmap 32-bit untuk UpdateLayeredWindow seukuran kanvas skala `s`.
pub(crate) unsafe fn make_surface(dc: HDC, s: f32) -> Option<(HBITMAP, *mut c_void)> {
    let (w, h) = canvas(s);
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
    Some((bmp, bits))
}

impl App {
    /// Hitung skala untuk monitor notch. Bila berubah: buat ulang bitmap window dan kanvas. True bila berubah.
    pub(crate) fn rescale(&mut self) -> bool {
        let s = effective(self.cfg.notch_size, monitors::dpi_of(&self.mon), self.scale_forced);
        if (s - self.scale).abs() < 0.001 {
            return false;
        }
        unsafe {
            let dc = HDC(self.mem_dc as *mut c_void);
            let Some((bmp, bits)) = make_surface(dc, s) else { return false };
            SelectObject(dc, bmp);
            let _ = DeleteObject(HGDIOBJ(self.bmp as *mut c_void));
            self.bmp = bmp.0 as isize;
            self.bits = bits as usize;
        }
        self.scale = s;
        set_current(s);
        self.gfx = Some(new_gfx(s));
        self.wibu_apply(); // the image is scaled to the canvas
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_monitor_dpi() {
        assert_eq!(effective(0, 96, None), 1.0);
        assert_eq!(effective(0, 120, None), 1.25);
        assert_eq!(effective(0, 144, None), 1.5);
        assert_eq!(effective(0, 0, None), 1.0, "DPI gagal dibaca = 100%");
        assert_eq!(effective(0, 480, None), MAX);
    }

    #[test]
    fn manual_size_ignores_dpi() {
        assert_eq!(effective(2, 96, None), 1.25);
        assert_eq!(effective(5, 144, None), 2.0);
        assert_eq!(effective(1, 192, None), 1.0);
        assert_eq!(effective(99, 120, None), 1.25, "indeks tak dikenal = Auto");
    }

    #[test]
    fn forced_flag_wins_and_is_clamped() {
        assert_eq!(effective(2, 144, Some(1.75)), 1.75);
        assert_eq!(effective(0, 96, Some(0.5)), MIN);
        assert_eq!(effective(0, 96, Some(9.0)), MAX);
    }

    #[test]
    fn labels() {
        assert_eq!(size_label(0), "Auto");
        assert_eq!(size_label(1), "100%");
        assert_eq!(size_label(3), "150%");
    }

    #[test]
    fn canvas_rounds_up() {
        assert_eq!(canvas(1.0), (W, H));
        assert_eq!(canvas(1.25), (675, 240));
        assert_eq!(canvas(1.1), (594, 212));
    }

    #[test]
    fn phys_box_covers_logical_box() {
        assert_eq!(to_phys((10, 0, 100, 50), 1.0), (10, 0, 100, 50));
        assert_eq!(to_phys((10, 0, 100, 50), 1.5), (15, 0, 150, 75));
        assert_eq!(to_phys((3, 0, 7, 1), 1.25), (3, 0, 9, 2));
    }

    #[test]
    fn current_scale_is_shared() {
        set_current(1.25);
        assert_eq!(current(), 1.25);
        set_current(1.0);
        assert_eq!(current(), 1.0);
    }

    #[test]
    fn scale_flag() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(forced_from_args(args(&["exe", "--scale=1.5"])), Some(1.5));
        assert_eq!(forced_from_args(args(&["exe", "--demo-slide=2"])), None);
        assert_eq!(forced_from_args(args(&["exe", "--scale=abc"])), None);
    }
}

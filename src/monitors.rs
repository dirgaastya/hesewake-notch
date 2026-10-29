//! Pilihan monitor untuk notch: layar utama, mengikuti kursor, atau monitor tertentu (Settings > Screens).

use super::*;
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

/// Nilai `monitor` di pengaturan: 0 = layar utama, 1 = ikuti kursor, `n >= 2` = monitor ke-(n-1) (urut kiri ke kanan).
pub(crate) const PRIMARY: usize = 0;
pub(crate) const FOLLOW: usize = 1;

fn rect_of(h: HMONITOR) -> Option<RECT> {
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(h, &mut mi).as_bool().then_some(mi.rcMonitor) }
}

/// Semua monitor aktif, urut dari kiri ke kanan (lalu atas ke bawah).
pub(crate) fn list() -> Vec<RECT> {
    unsafe extern "system" fn cb(h: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
        let v = &mut *(lp.0 as *mut Vec<RECT>);
        if let Some(r) = rect_of(h) {
            v.push(r);
        }
        BOOL(1)
    }
    let mut v: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut v as *mut Vec<RECT> as isize));
    }
    v.sort_by_key(|r| (r.left, r.top));
    v
}

fn primary() -> RECT {
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    RECT { left: 0, top: 0, right: w, bottom: h }
}

fn under_cursor() -> Option<RECT> {
    unsafe {
        let mut p = POINT::default();
        GetCursorPos(&mut p).ok()?;
        rect_of(MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST))
    }
}

/// Monitor yang dipakai notch untuk `mode`. Monitor yang sudah dicabut jatuh kembali ke layar utama.
pub(crate) fn resolve(mode: usize) -> RECT {
    match mode {
        PRIMARY => primary(),
        FOLLOW => under_cursor().unwrap_or_else(primary),
        n => list().get(n - 2).copied().unwrap_or_else(primary),
    }
}

/// Banyaknya pilihan: Primary, Follow cursor, lalu satu per monitor (hanya bila lebih dari satu monitor).
pub(crate) fn mode_count() -> usize {
    let n = list().len();
    if n > 1 { 2 + n } else { 2 }
}

pub(crate) fn mode_label(mode: usize) -> String {
    match mode {
        PRIMARY => tr("Primary").to_string(),
        FOLLOW => tr("Follow").to_string(),
        n => fill(tr("Display {}"), &[&(n - 1)]),
    }
}

/// Posisi window (x, y) selebar `cw` piksel fisik agar notch berada di tengah atas `r`.
pub(crate) fn origin(r: &RECT, cw: i32) -> (i32, i32) {
    (r.left + (r.right - r.left - cw) / 2, r.top)
}

/// DPI efektif monitor `r` (96 = 100%), 0 bila tidak terbaca.
pub(crate) fn dpi_of(r: &RECT) -> u32 {
    unsafe {
        let c = POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 };
        let (mut x, mut y) = (0u32, 0u32);
        match GetDpiForMonitor(MonitorFromPoint(c, MONITOR_DEFAULTTONEAREST), MDT_EFFECTIVE_DPI, &mut x, &mut y) {
            Ok(()) => x,
            Err(_) => 0,
        }
    }
}

pub(crate) fn same(a: &RECT, b: &RECT) -> bool {
    a.left == b.left && a.top == b.top && a.right == b.right && a.bottom == b.bottom
}

/// Putar pilihan monitor satu langkah (+1 / -1), melingkar.
pub(crate) fn step_mode(mode: usize, forward: bool) -> usize {
    let n = mode_count();
    let m = mode.min(n - 1);
    if forward { (m + 1) % n } else { (m + n - 1) % n }
}

impl App {
    /// Hitung ulang monitor target dan pindahkan window bila berubah. True bila berpindah.
    pub(crate) fn place(&mut self, force: bool) -> bool {
        let r = resolve(self.cfg.monitor);
        if !force && same(&r, &self.mon) {
            return false;
        }
        self.mon = r;
        // Monitor lain bisa punya scaling lain: sesuaikan kanvas dulu, baru pusatkan window.
        self.rescale();
        let (cw, ch) = scale::canvas(self.scale);
        let (x, y) = origin(&r, cw);
        self.wx = x;
        self.wy = y;
        unsafe {
            let _ = SetWindowPos(HWND(self.hwnd as *mut c_void), None, x, y, cw, ch, SWP_NOACTIVATE | SWP_NOZORDER);
        }
        true
    }

    /// Mode "ikuti kursor": pindah ke monitor tempat kursor berada, hanya saat notch sedang tidak dipakai.
    pub(crate) fn monitor_follow(&mut self) -> bool {
        if self.cfg.monitor != FOLLOW || self.hover || self.expanded() || self.drag.is_some() {
            return false;
        }
        self.place(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_centers_notch_on_monitor() {
        let r = RECT { left: 1920, top: 0, right: 1920 + 1600, bottom: 900 };
        assert_eq!(origin(&r, W), (1920 + (1600 - W) / 2, 0));
        let neg = RECT { left: -1280, top: -200, right: 0, bottom: 824 };
        assert_eq!(origin(&neg, W), (-1280 + (1280 - W) / 2, -200));
        // kanvas 125% (675 px) tetap di tengah
        let big = RECT { left: 0, top: 0, right: 2560, bottom: 1600 };
        assert_eq!(origin(&big, 675), ((2560 - 675) / 2, 0));
    }

    #[test]
    fn labels() {
        assert_eq!(mode_label(PRIMARY), "Primary");
        assert_eq!(mode_label(FOLLOW), "Follow");
        assert_eq!(mode_label(2), "Display 1");
        assert_eq!(mode_label(3), "Display 2");
    }

    #[test]
    fn same_rect() {
        let a = RECT { left: 0, top: 0, right: 10, bottom: 10 };
        assert!(same(&a, &a));
        assert!(!same(&a, &RECT { left: 1, ..a }));
    }
}

//! Busy mode: deteksi app Adobe / maximized / fullscreen.

use super::*;

const BUSY_APPS: &[&str] = &[
    "photoshop", "illustrator", "premiere", "afterfx", "indesign", "lightroom", "acrobat", "animate", "audition",
    "adobe",
];

// ---------------------------------------------------------------- busy mode
unsafe fn exe_name(hwnd: HWND) -> String {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return String::new();
    }
    let h = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
        Ok(h) => h,
        Err(_) => return String::new(),
    };
    let mut buf = [0u16; 520];
    let mut len = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
    let _ = CloseHandle(h);
    if !ok {
        return String::new();
    }
    let full = String::from_utf16_lossy(&buf[..len as usize]);
    full.rsplit('\\').next().unwrap_or("").to_lowercase()
}

/// `maximized_is_busy`: jendela maximized biasa juga dianggap sibuk (bukan hanya layar penuh / app Adobe).
pub(crate) unsafe fn is_busy(own: HWND, maximized_is_busy: bool, mon: &RECT) -> bool {
    let fg = GetForegroundWindow();
    if fg.0.is_null() || fg == own {
        return false;
    }
    let mut cls = [0u16; 64];
    let n = GetClassNameW(fg, &mut cls);
    let class = String::from_utf16_lossy(&cls[..(n.max(0) as usize)]);
    if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
        return false;
    }
    let exe = exe_name(fg);
    if !exe.is_empty() && BUSY_APPS.iter().any(|k| exe.contains(k)) {
        return true;
    }
    if maximized_is_busy && IsZoomed(fg).as_bool() {
        return true;
    }
    {
        // Layar penuh sungguhan (F11, video): jendela menutupi seluruh layar.
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_ok() {
            // hanya layar penuh di monitor tempat notch berada yang menyusutkannya
            if r.left <= mon.left && r.top <= mon.top && r.right >= mon.right && r.bottom >= mon.bottom {
                return true;
            }
        }
    }
    false
}

/// What the foreground window is doing to the notch's neighbourhood: 0 = nothing special, 1 = maximized, 2 = fullscreen
/// (covers the whole monitor the notch is on). The desktop, the taskbar and the notch's own window count as nothing.
pub(crate) unsafe fn fg_state(own: HWND, mon: &RECT) -> u8 {
    let fg = GetForegroundWindow();
    if fg.0.is_null() || fg == own {
        return 0;
    }
    let mut cls = [0u16; 64];
    let n = GetClassNameW(fg, &mut cls);
    let class = String::from_utf16_lossy(&cls[..(n.max(0) as usize)]);
    if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
        return 0;
    }
    let mut r = RECT::default();
    if GetWindowRect(fg, &mut r).is_ok() && r.left <= mon.left && r.top <= mon.top && r.right >= mon.right && r.bottom >= mon.bottom {
        return 2;
    }
    if IsZoomed(fg).as_bool() {
        return 1;
    }
    0
}

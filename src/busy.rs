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
    // Layar penuh sungguhan (F11, video): hanya di monitor tempat notch berada yang menyusutkannya
    fills_monitor(fg, mon)
}

/// Does the window cover the whole monitor the way a fullscreen app does?
unsafe fn fills_monitor(fg: HWND, mon: &RECT) -> bool {
    let mut r = RECT::default();
    if GetWindowRect(fg, &mut r).is_err() {
        return false;
    }
    let style = GetWindowLongW(fg, GWL_STYLE) as u32;
    is_fullscreen(&r, mon, IsZoomed(fg).as_bool(), style)
}

/// The rule behind `fills_monitor`, without a window. A maximized window that still has its frame also covers the whole
/// monitor when the taskbar hides itself (its invisible resize borders reach past the screen edges): that is a maximized
/// window, not a fullscreen app. A fullscreen app (F11 in a browser, a video, a game) has dropped its frame.
pub(crate) fn is_fullscreen(r: &RECT, mon: &RECT, zoomed: bool, style: u32) -> bool {
    let covers = r.left <= mon.left && r.top <= mon.top && r.right >= mon.right && r.bottom >= mon.bottom;
    let framed = style & (WS_CAPTION.0 | WS_THICKFRAME.0) != 0;
    covers && !(zoomed && framed)
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
    if fills_monitor(fg, mon) {
        return 2;
    }
    if IsZoomed(fg).as_bool() {
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: RECT = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
    const FRAME: u32 = 0x00CF0000; // WS_OVERLAPPEDWINDOW
    const NONE: u32 = 0x90000000; // WS_POPUP | WS_VISIBLE

    #[test]
    fn a_maximized_window_over_a_hidden_taskbar_is_not_fullscreen() {
        // maximized: the invisible resize borders (8 px) reach past every edge of the screen
        let r = RECT { left: -8, top: -8, right: 1928, bottom: 1088 };
        assert!(!is_fullscreen(&r, &MON, true, FRAME));
    }

    #[test]
    fn a_window_without_a_frame_that_covers_the_monitor_is_fullscreen() {
        let r = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        assert!(is_fullscreen(&r, &MON, false, NONE), "a game or a video");
        assert!(is_fullscreen(&r, &MON, true, NONE), "a browser on F11 is still zoomed but has dropped its frame");
    }

    #[test]
    fn a_window_that_does_not_cover_the_monitor_is_never_fullscreen() {
        // maximized with the taskbar showing: the bottom stops above the bar
        let r = RECT { left: -8, top: -8, right: 1928, bottom: 1040 };
        assert!(!is_fullscreen(&r, &MON, true, FRAME));
        assert!(!is_fullscreen(&r, &MON, false, NONE));
    }

    #[test]
    fn a_framed_window_that_is_not_maximized_but_covers_the_monitor_counts_as_fullscreen() {
        let r = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        assert!(is_fullscreen(&r, &MON, false, FRAME), "e.g. an app that sized itself to the screen");
    }
}

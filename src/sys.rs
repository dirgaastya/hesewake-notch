//! Integrasi sistem: single instance, autostart (registry Run), tray icon + menu, status layar (power), About.

use super::*;
use windows::Win32::System::Power::{GetSystemPowerStatus, RegisterPowerSettingNotification, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    KEY_SET_VALUE, REG_SZ,
};
use windows::Win32::System::SystemServices::{GUID_CONSOLE_DISPLAY_STATE, GUID_DEVICE_POWER_POLICY_VIDEO_BRIGHTNESS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW};

/// Pesan callback tray icon.
pub(crate) const WM_TRAY: u32 = WM_APP + 1;

// ---- id menu bersama (context menu notch + tray)
/// Pesan ke window: kecerahan berubah (`wParam` = persen).
pub(crate) const WM_BRIGHTNESS: u32 = WM_APP + 7;
/// Pesan ke window: status daya berubah (charger/baterai) dan layar menyala/mati (`wParam` != 0 = mati).
pub(crate) const WM_POWERCHECK: u32 = WM_APP + 8;
pub(crate) const WM_DISPLAY: u32 = WM_APP + 9;

pub(crate) const CMD_EXIT: i32 = 1;
pub(crate) const CMD_AUTOSTART: i32 = 40;
pub(crate) const CMD_MINIMAL: i32 = 41;
pub(crate) const CMD_ABOUT: i32 = 42;
pub(crate) const CMD_NOTIF: i32 = 43;
pub(crate) const CMD_NOTIF_ACCESS: i32 = 44;
pub(crate) const CMD_SETTINGS: i32 = 45;
pub(crate) const CMD_UPDATE_OPEN: i32 = 46;
pub(crate) const CMD_UPDATE_CHECK: i32 = 47;
pub(crate) const CMD_UNLOCK: i32 = 48;

/// Mutex single instance. Mengembalikan false bila Hesewake Notch lain sudah jalan.
pub(crate) fn single_instance() -> bool {
    unsafe {
        match CreateMutexW(None, true, w!("Local\\HesewakeNotch.SingleInstance")) {
            // Handle sengaja dibiarkan terbuka sampai proses selesai.
            Ok(_) => GetLastError() != ERROR_ALREADY_EXISTS,
            Err(_) => true,
        }
    }
}

// ---------------------------------------------------------------- autostart
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_NAME: PCWSTR = w!("HesewakeNotch");
/// Nama entri autostart sebelum rebrand.
const RUN_NAME_OLD: PCWSTR = w!("Poch");

pub(crate) fn autostart_enabled() -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_READ, &mut key).is_err() {
            return false;
        }
        let r = RegQueryValueExW(key, RUN_NAME, None, None, None, None);
        let _ = RegCloseKey(key);
        r.is_ok()
    }
}

/// Pindahkan entri autostart lama ("Poch") ke nama baru dengan status aktif yang sama.
pub(crate) fn migrate_autostart() {
    let had_old = unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_READ | KEY_SET_VALUE, &mut key).is_err() {
            return;
        }
        let had = RegQueryValueExW(key, RUN_NAME_OLD, None, None, None, None).is_ok();
        if had {
            let _ = RegDeleteValueW(key, RUN_NAME_OLD);
        }
        let _ = RegCloseKey(key);
        had
    };
    if had_old {
        set_autostart(true);
    }
}

pub(crate) fn set_autostart(on: bool) {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_SET_VALUE, &mut key).is_err() {
            return;
        }
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let cmd = format!("\"{}\"", exe.display());
                let wide: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
                let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
                let _ = RegSetValueExW(key, RUN_NAME, 0, REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(key, RUN_NAME);
        }
        let _ = RegCloseKey(key);
    }
}

// ---------------------------------------------------------------- settings
/// Windows melaporkan layar penuh / presentasi (toast notifikasi ditahan agar tidak mengganggu).
pub(crate) fn user_fullscreen() -> bool {
    use windows::Win32::UI::Shell::SHQueryUserNotificationState;
    match unsafe { SHQueryUserNotificationState() } {
        Ok(st) => matches!(st.0, 2 | 3 | 4), // QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, QUNS_PRESENTATION_MODE
        Err(_) => false,
    }
}

pub(crate) unsafe fn open_notification_settings() {
    let _ = ShellExecuteW(None, w!("open"), w!("ms-settings:privacy-notifications"), None, None, SW_SHOWNORMAL);
}

// ---------------------------------------------------------------- power (layar nyala/mati)
pub(crate) unsafe fn register_display_notifications(hwnd: HWND) {
    let _ = RegisterPowerSettingNotification(HANDLE(hwnd.0), &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);
    // Perubahan kecerahan layar (tombol Fn / slider Windows) untuk HUD kecerahan.
    let _ = RegisterPowerSettingNotification(HANDLE(hwnd.0), &GUID_DEVICE_POWER_POLICY_VIDEO_BRIGHTNESS, DEVICE_NOTIFY_WINDOW_HANDLE);
}

/// Kecerahan (0..100) dari WM_POWERBROADCAST bila pesan itu perubahan kecerahan.
pub(crate) unsafe fn brightness_from(wp: WPARAM, lp: LPARAM) -> Option<u8> {
    const PBT_POWERSETTINGCHANGE: usize = 0x8013;
    if wp.0 != PBT_POWERSETTINGCHANGE || lp.0 == 0 {
        return None;
    }
    let s = &*(lp.0 as *const windows::Win32::System::Power::POWERBROADCAST_SETTING);
    if s.PowerSetting != GUID_DEVICE_POWER_POLICY_VIDEO_BRIGHTNESS || s.DataLength < 4 {
        return None;
    }
    Some(std::ptr::read_unaligned(s.Data.as_ptr() as *const u32).min(100) as u8)
}

/// Baca WM_POWERBROADCAST: Some(true) = layar mati, Some(false) = layar menyala/redup.
pub(crate) unsafe fn display_off_from(wp: WPARAM, lp: LPARAM) -> Option<bool> {
    const PBT_POWERSETTINGCHANGE: usize = 0x8013;
    if wp.0 != PBT_POWERSETTINGCHANGE || lp.0 == 0 {
        return None;
    }
    let s = &*(lp.0 as *const windows::Win32::System::Power::POWERBROADCAST_SETTING);
    if s.PowerSetting != GUID_CONSOLE_DISPLAY_STATE || s.DataLength < 4 {
        return None;
    }
    let state = std::ptr::read_unaligned(s.Data.as_ptr() as *const u32);
    Some(state == 0)
}

// ---------------------------------------------------------------- tray
unsafe fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        ..Default::default()
    };
    // Ikon exe (ditanam build.rs sebagai resource id 1); cadangan: ikon aplikasi standar.
    let hinst = GetModuleHandleW(None).unwrap_or_default();
    nid.hIcon = LoadIconW(hinst, PCWSTR(1 as *const u16)).or_else(|_| LoadIconW(None, IDI_APPLICATION)).unwrap_or_default();
    for (d, s) in nid.szTip.iter_mut().zip("Hesewake Notch".encode_utf16().chain(std::iter::once(0))) {
        *d = s;
    }
    nid
}

pub(crate) unsafe fn tray_add(hwnd: HWND) {
    let nid = tray_data(hwnd);
    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
}

pub(crate) unsafe fn tray_remove(hwnd: HWND) {
    let nid = tray_data(hwnd);
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

/// Pesan "TaskbarCreated": dikirim saat Explorer restart, ikon tray harus dipasang ulang.
pub(crate) fn taskbar_created_msg() -> u32 {
    use std::sync::OnceLock;
    static MSG: OnceLock<u32> = OnceLock::new();
    *MSG.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

pub(crate) unsafe fn tray_menu(hwnd: HWND, autostart: bool, minimal: bool, notif_on: bool, update: Option<&str>, lock: Lock) -> i32 {
    let Ok(menu) = CreatePopupMenu() else { return 0 };
    let chk = |on: bool| if on { MF_STRING | MF_CHECKED } else { MF_STRING };
    if lock != Lock::Off {
        let _ = AppendMenuW(menu, MF_STRING, CMD_UNLOCK as usize, &HSTRING::from(tr("Unlock notch")));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    }
    let _ = AppendMenuW(menu, chk(autostart), CMD_AUTOSTART as usize, &HSTRING::from(tr("Start with Windows")));
    let _ = AppendMenuW(menu, chk(minimal), CMD_MINIMAL as usize, &HSTRING::from(tr("Minimal notch")));
    let _ = AppendMenuW(menu, chk(notif_on), CMD_NOTIF as usize, &HSTRING::from(tr("Windows notifications")));
    let _ = AppendMenuW(menu, MF_STRING, CMD_NOTIF_ACCESS as usize, &HSTRING::from(tr("Notification access...")));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    if let Some(v) = update {
        let _ = AppendMenuW(menu, MF_STRING, CMD_UPDATE_OPEN as usize, &HSTRING::from(fill(tr("Update available ({})..."), &[&v])));
    }
    let _ = AppendMenuW(menu, MF_STRING, CMD_UPDATE_CHECK as usize, &HSTRING::from(tr("Check for updates")));
    let settings = if lock == Lock::Min { MF_STRING | MF_GRAYED } else { MF_STRING };
    let _ = AppendMenuW(menu, settings, CMD_SETTINGS as usize, &HSTRING::from(tr("Settings")));
    let _ = AppendMenuW(menu, MF_STRING, CMD_ABOUT as usize, &HSTRING::from(tr("About Hesewake Notch")));
    let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT as usize, &HSTRING::from(tr("Exit")));
    track(hwnd, menu)
}

pub(crate) unsafe fn track(hwnd: HWND, menu: HMENU) -> i32 {
    let prev_fg = GetForegroundWindow();
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, None);
    let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
    if !prev_fg.0.is_null() && prev_fg != hwnd {
        let _ = SetForegroundWindow(prev_fg);
    }
    cmd.0
}

/// Open a web link in the default browser / meeting app. Tests only record it (see `opened`).
pub(crate) fn open_url(url: &str) {
    #[cfg(test)]
    OPENED.with(|o| o.borrow_mut().push(url.to_string()));
    #[cfg(not(test))]
    unsafe {
        let _ = ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
    }
}

#[cfg(test)]
thread_local! {
    static OPENED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Seconds since the last keyboard or mouse input anywhere in the session.
pub(crate) fn input_idle_secs() -> f32 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if !unsafe { GetLastInputInfo(&mut li) }.as_bool() {
        return 0.0;
    }
    unsafe { GetTickCount() }.wrapping_sub(li.dwTime) as f32 / 1000.0
}

/// Open Explorer on the folder of `path` with that file selected. Tests record it as "select:<path>".
pub(crate) fn show_in_folder(path: &str) {
    #[cfg(test)]
    OPENED.with(|o| o.borrow_mut().push(format!("select:{path}")));
    #[cfg(not(test))]
    unsafe {
        let args = HSTRING::from(format!("/select,\"{path}\""));
        let _ = ShellExecuteW(None, w!("open"), w!("explorer.exe"), &args, None, SW_SHOWNORMAL);
    }
}

/// Tests: the links `open_url` was asked to open on this thread (and forget them).
#[cfg(test)]
pub(crate) fn opened() -> Vec<String> {
    OPENED.with(|o| std::mem::take(&mut *o.borrow_mut()))
}

pub(crate) unsafe fn about(hwnd: HWND) {
    let text = fill(
        tr("Hesewake Notch {}\nA personal notch for the top of your screen.\n\nBased on Poch by Commonly Works\n(github.com/klawcodes/poch), MIT License.\nLyrics: LRCLIB (lrclib.net) and lrc.red.\nRomaji: Lindera (IPADIC) and WanaKana.\nFont: Instrument Sans."),
        &[&env!("CARGO_PKG_VERSION")],
    );
    let _ = MessageBoxW(hwnd, &HSTRING::from(text), &HSTRING::from(tr("About Hesewake Notch")), MB_OK | MB_ICONINFORMATION);
}

/// Keadaan baterai terperinci (slide System).
#[derive(Clone, Copy)]
pub(crate) struct Battery {
    pub(crate) ac: bool,
    pub(crate) pct: u8,
    pub(crate) charging: bool,
    /// Sisa waktu (detik) saat memakai baterai, bila Windows tahu.
    pub(crate) secs: Option<u32>,
}

pub(crate) fn battery() -> Option<Battery> {
    let mut st = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut st).ok()? };
    if st.BatteryFlag & 128 != 0 || st.BatteryLifePercent > 100 {
        return None;
    }
    Some(Battery {
        ac: st.ACLineStatus == 1,
        pct: st.BatteryLifePercent,
        charging: st.BatteryFlag & 8 != 0,
        secs: (st.BatteryLifeTime != u32::MAX).then_some(st.BatteryLifeTime),
    })
}

/// (charger tersambung, persen baterai). None bila tidak ada baterai atau status tidak diketahui.
pub(crate) fn power_status() -> Option<(bool, u8)> {
    let mut st = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut st).ok()? };
    if st.BatteryFlag & 128 != 0 || st.BatteryLifePercent > 100 {
        return None;
    }
    Some((st.ACLineStatus == 1, st.BatteryLifePercent))
}

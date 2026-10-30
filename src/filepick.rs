//! The Windows Open dialog, for settings that point at a file the user picked (adhan sound, Wibu image).

use super::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};

/// Where the dialog leaves its answer (Some(None) = cancelled).
pub(crate) type Picked = Mutex<Option<Option<String>>>;

/// Show the Open dialog. It needs a single-threaded COM apartment, so it runs on its own thread, leaves the answer
/// in `slot` and posts `msg` to the window.
pub(crate) fn open(hwnd: isize, title: &'static str, filter_name: &'static str, filter_spec: &'static str, slot: &'static Picked, msg: u32) {
    if cfg!(test) {
        return;
    }
    std::thread::spawn(move || {
        let path = unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let r = (|| -> windows::core::Result<String> {
                let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
                let (name, spec) = (HSTRING::from(filter_name), HSTRING::from(filter_spec));
                let filters = [COMDLG_FILTERSPEC { pszName: PCWSTR(name.as_ptr()), pszSpec: PCWSTR(spec.as_ptr()) }];
                dlg.SetFileTypes(&filters)?;
                dlg.SetTitle(&HSTRING::from(title))?;
                dlg.Show(HWND(hwnd as *mut c_void))?;
                let item = dlg.GetResult()?;
                let p = item.GetDisplayName(SIGDN_FILESYSPATH)?;
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const c_void));
                Ok(s)
            })();
            CoUninitialize();
            r.ok().filter(|s| !s.is_empty())
        };
        if let Ok(mut p) = slot.lock() {
            *p = Some(path);
        }
        unsafe {
            let _ = PostMessageW(HWND(hwnd as *mut c_void), msg, WPARAM(0), LPARAM(0));
        }
    });
}

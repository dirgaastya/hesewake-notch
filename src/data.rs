//! Penyimpanan data di %APPDATA%\hesewake-notch dan trim memori.

use super::*;

pub(crate) fn data_path(name: &str) -> std::path::PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    std::path::Path::new(&base).join("hesewake-notch").join(name)
}

/// Salin data dari nama aplikasi sebelumnya (`poch`, lalu `notch`) ke folder baru bila berkasnya belum ada.
/// Folder lama tidak disentuh. Berkas diagnostik (crash/perf log) tidak ikut dipindah.
pub(crate) fn migrate_old_data() {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    for old_name in ["poch", "notch"] {
        let Ok(entries) = std::fs::read_dir(std::path::Path::new(&base).join(old_name)) else { continue };
        for e in entries.flatten() {
            let src = e.path();
            let Some(name) = src.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
            if !src.is_file() || matches!(name.as_str(), "crash.log" | "perf.log") {
                continue;
            }
            let dst = data_path(&name);
            if !dst.exists() {
                if let Some(dir) = dst.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::copy(&src, &dst);
            }
        }
    }
}

pub(crate) fn trim_memory() {
    unsafe {
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

pub(crate) fn write_data(name: &str, s: String) {
    if cfg!(test) {
        return; // tests must never overwrite the user's own data in %APPDATA%
    }
    let p = data_path(name);
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, s);
}

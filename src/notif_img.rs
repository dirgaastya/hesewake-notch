//! Foto profil pada notifikasi (mis. kontak WhatsApp Web).
//!
//! API `UserNotificationListener` hanya memberi teks toast. Gambar `appLogoOverride` ada di payload XML toast pada
//! `%LOCALAPPDATA%\Microsoft\Windows\Notifications\wpndatabase.db` (tabel `Notification`, kolom `Id`, `Payload`),
//! dengan `Id` yang sama dengan `UserNotification.Id`. Database dibuka **read-only**, hanya untuk satu id, hanya atribut
//! `src` gambar yang diambil, dan hanya berkas **lokal** yang dibaca. Semua langkah gagal-aman (tanpa avatar).

use super::*;
use std::ffi::{c_char, CStr, CString};
use std::sync::OnceLock;
use windows::core::PCSTR;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE,
    FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

/// Sisi (px) avatar yang disimpan; digambar ulang lebih kecil.
pub(crate) const AVATAR_PX: u32 = 48;
const MAX_FILE: u64 = 2_000_000;

// ---------------------------------------------------------------- winsqlite3.dll (bawaan Windows), dimuat dinamis
type Open = unsafe extern "C" fn(*const c_char, *mut *mut c_void, i32, *const c_char) -> i32;
type Prepare = unsafe extern "C" fn(*mut c_void, *const c_char, i32, *mut *mut c_void, *mut *const c_char) -> i32;
type BindInt64 = unsafe extern "C" fn(*mut c_void, i32, i64) -> i32;
type Step = unsafe extern "C" fn(*mut c_void) -> i32;
type ColText = unsafe extern "C" fn(*mut c_void, i32) -> *const c_char;
type Finalize = unsafe extern "C" fn(*mut c_void) -> i32;
type Close = unsafe extern "C" fn(*mut c_void) -> i32;
type BusyTimeout = unsafe extern "C" fn(*mut c_void, i32) -> i32;

struct Sq {
    open: Open,
    prepare: Prepare,
    bind: BindInt64,
    step: Step,
    col_text: ColText,
    finalize: Finalize,
    close: Close,
    busy: BusyTimeout,
}

fn sq() -> Option<&'static Sq> {
    static S: OnceLock<Option<Sq>> = OnceLock::new();
    S.get_or_init(|| unsafe {
        let h = LoadLibraryW(w!("winsqlite3.dll")).ok()?;
        macro_rules! f {
            ($n:literal) => {
                std::mem::transmute(GetProcAddress(h, PCSTR(concat!($n, "\0").as_ptr()))?)
            };
        }
        Some(Sq {
            open: f!("sqlite3_open_v2"),
            prepare: f!("sqlite3_prepare_v2"),
            bind: f!("sqlite3_bind_int64"),
            step: f!("sqlite3_step"),
            col_text: f!("sqlite3_column_text"),
            finalize: f!("sqlite3_finalize"),
            close: f!("sqlite3_close"),
            busy: f!("sqlite3_busy_timeout"),
        })
    })
    .as_ref()
}

fn db_uri() -> Option<CString> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    let p = format!("{base}\\Microsoft\\Windows\\Notifications\\wpndatabase.db");
    if !std::path::Path::new(&p).exists() {
        return None;
    }
    let mut uri = String::from("file:");
    for c in p.replace('\\', "/").chars() {
        match c {
            ' ' => uri.push_str("%20"),
            '#' => uri.push_str("%23"),
            '?' => uri.push_str("%3F"),
            '%' => uri.push_str("%25"),
            c => uri.push(c),
        }
    }
    uri.push_str("?mode=ro");
    CString::new(uri).ok()
}

/// Payload XML satu notifikasi (None bila tidak ada/gagal). Read-only; hanya baris dengan `Id` tersebut.
pub(crate) fn payload_for(id: u32) -> Option<String> {
    let s = sq()?;
    let uri = db_uri()?;
    unsafe {
        let mut db: *mut c_void = std::ptr::null_mut();
        // SQLITE_OPEN_READONLY (0x1) | SQLITE_OPEN_URI (0x40)
        if (s.open)(uri.as_ptr(), &mut db, 0x41, std::ptr::null()) != 0 || db.is_null() {
            if !db.is_null() {
                (s.close)(db);
            }
            return None;
        }
        (s.busy)(db, 250);
        let sql = c"SELECT Payload FROM Notification WHERE Id = ?1 LIMIT 1";
        let mut st: *mut c_void = std::ptr::null_mut();
        let mut out = None;
        if (s.prepare)(db, sql.as_ptr(), -1, &mut st, std::ptr::null_mut()) == 0 && !st.is_null() {
            (s.bind)(st, 1, id as i64);
            if (s.step)(st) == 100 {
                let p = (s.col_text)(st, 0);
                if !p.is_null() {
                    out = Some(CStr::from_ptr(p).to_string_lossy().into_owned());
                }
            }
            (s.finalize)(st);
        }
        (s.close)(db);
        out
    }
}

// ---------------------------------------------------------------- pemantau folder gambar browser
static CACHE: Mutex<Vec<(String, Vec<u8>)>> = Mutex::new(Vec::new());
const CACHE_MAX: usize = 24;

fn cache_put(name: String, bytes: Vec<u8>) {
    let mut c = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    c.retain(|(n, _)| *n != name);
    c.push((name, bytes));
    while c.len() > CACHE_MAX {
        c.remove(0);
    }
}

fn cache_get(name: &str) -> Option<Vec<u8>> {
    CACHE.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|(n, _)| n == name).map(|(_, b)| b.clone())
}

/// Pantau folder "Notification Resources" milik Chrome/Edge/Brave: salin berkas gambar begitu muncul (sebelum
/// browser menghapusnya). Tidur di `ReadDirectoryChangesW` sampai ada perubahan: tanpa polling, tanpa CPU saat diam.
pub(crate) fn start_watchers() {
    static ONCE: OnceLock<()> = OnceLock::new();
    if ONCE.set(()).is_err() {
        return;
    }
    let Ok(base) = std::env::var("LOCALAPPDATA") else { return };
    for rel in [
        r"Google\Chrome\User Data\Notification Resources",
        r"Microsoft\Edge\User Data\Notification Resources",
        r"BraveSoftware\Brave-Browser\User Data\Notification Resources",
    ] {
        let dir = std::path::Path::new(&base).join(rel);
        if dir.is_dir() {
            std::thread::spawn(move || watch_dir(dir));
        }
    }
}

fn watch_dir(dir: std::path::PathBuf) {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let h = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    };
    let Ok(h) = h else { return };
    let mut buf = vec![0u32; 4096]; // selaras DWORD
    loop {
        let mut returned = 0u32;
        // nama berkas (0x1) | ukuran (0x8) | waktu tulis (0x10): berkas dibuat kosong lalu diisi
        let ok = unsafe {
            ReadDirectoryChangesW(h, buf.as_mut_ptr() as *mut c_void, (buf.len() * 4) as u32, false, FILE_NOTIFY_CHANGE(0x19), Some(&mut returned), None, None)
        };
        if ok.is_err() {
            return;
        }
        if returned == 0 || !notif::avatar_enabled() {
            continue;
        }
        let mut off = 0usize;
        loop {
            let (name, action, next) = unsafe {
                let info = &*((buf.as_ptr() as *const u8).add(off) as *const FILE_NOTIFY_INFORMATION);
                let n = std::slice::from_raw_parts(info.FileName.as_ptr(), info.FileNameLength as usize / 2);
                (String::from_utf16_lossy(n), info.Action.0, info.NextEntryOffset as usize)
            };
            // 1 = ditambah, 3 = diubah, 5 = nama baru
            if matches!(action, 1 | 3 | 5) {
                let path = dir.join(&name);
                for _ in 0..8 {
                    match std::fs::read(&path) {
                        Ok(b) if !b.is_empty() && b.len() as u64 <= MAX_FILE => {
                            cache_put(name.to_ascii_lowercase(), b);
                            break;
                        }
                        _ => std::thread::sleep(Duration::from_millis(12)),
                    }
                }
            }
            if next == 0 {
                break;
            }
            off += next;
        }
    }
}

// ---------------------------------------------------------------- parsing (murni, bisa dites)
pub(crate) fn xml_unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// Nilai atribut `name` pada satu tag (`<image a="x" b='y'>`), mendukung kutip ganda/tunggal.
fn attr(tag: &str, name: &str) -> Option<String> {
    let mut i = 0;
    let b = tag.as_bytes();
    while let Some(pos) = tag[i..].find(name) {
        let start = i + pos;
        let before_ok = start == 0 || !(b[start - 1] as char).is_ascii_alphanumeric();
        let rest = tag[start + name.len()..].trim_start();
        if before_ok && rest.starts_with('=') {
            let rest = rest[1..].trim_start();
            let q = rest.chars().next()?;
            if q == '"' || q == '\'' {
                let end = rest[1..].find(q)?;
                return Some(xml_unescape(&rest[1..1 + end]));
            }
        }
        i = start + name.len();
    }
    None
}

/// `src` dari `<image placement="appLogoOverride" ...>` pada payload toast.
pub(crate) fn parse_avatar_src(xml: &str) -> Option<String> {
    let lower = xml.to_ascii_lowercase();
    let mut from = 0;
    while let Some(p) = lower[from..].find("<image") {
        let start = from + p;
        let end = start + lower[start..].find('>')?;
        let tag = &xml[start..=end];
        if attr(tag, "placement").map_or(false, |v| v.eq_ignore_ascii_case("appLogoOverride")) {
            if let Some(src) = attr(tag, "src") {
                return Some(src);
            }
        }
        from = end;
    }
    None
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Ubah `src` jadi path lokal yang aman: hanya `X:\...` atau `file:///X:/...`; tolak UNC, URL jaringan, dan path relatif.
pub(crate) fn local_image_path(src: &str) -> Option<std::path::PathBuf> {
    let s = src.trim();
    let p = if let Some(rest) = s.strip_prefix("file:///") { percent_decode(rest) } else { s.to_string() };
    let p = p.replace('/', "\\");
    let b = p.as_bytes();
    let drive = b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    if !drive || p.starts_with("\\\\") || p.contains("..") {
        return None;
    }
    let path = std::path::PathBuf::from(p);
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    // Chrome/Edge menulis gambar notifikasi sebagai `*.tmp` di folder "Notification Resources" (isinya tetap gambar,
    // dikenali dari isi berkas, bukan ekstensi).
    let browser_tmp = ext == "tmp"
        && path.parent().and_then(|d| d.file_name()).map_or(false, |n| n.to_string_lossy().eq_ignore_ascii_case("Notification Resources"));
    if !browser_tmp && !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp") {
        return None;
    }
    Some(path)
}

/// Avatar RGBA (AVATAR_PX x AVATAR_PX) untuk notifikasi `id`, atau None. Memanggil database (blocking singkat).
pub(crate) fn avatar_rgba(id: u32) -> Option<Vec<u8>> {
    let mut payload = payload_for(id);
    if payload.is_none() {
        // baris mungkin belum ditulis saat listener melihatnya
        std::thread::sleep(Duration::from_millis(300));
        payload = payload_for(id);
    }
    let src = parse_avatar_src(&payload?)?;
    let path = local_image_path(&src)?;
    // Browser menghapus berkas gambar sesaat setelah toast tampil, jadi salinan dari pemantau folder dipakai lebih dulu.
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let bytes = match cache_get(&name) {
        Some(b) => b,
        None => {
            let meta = std::fs::metadata(&path).ok()?;
            if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_FILE {
                return None;
            }
            std::fs::read(&path).ok()?
        }
    };
    let img = decode_limited(&bytes)?;
    Some(img.resize_to_fill(AVATAR_PX, AVATAR_PX, image::imageops::FilterType::Triangle).to_rgba8().into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<toast launch="x"><visual><binding template="ToastGeneric"><image placement="appLogoOverride" hint-crop="circle" src="C:\Users\A B\AppData\Local\Google\Chrome\User Data\Notification Resources\abc-1.png"/><text>Mas</text></binding></visual></toast>"#;

    #[test]
    fn finds_app_logo_override_src() {
        assert_eq!(
            parse_avatar_src(XML).as_deref(),
            Some(r"C:\Users\A B\AppData\Local\Google\Chrome\User Data\Notification Resources\abc-1.png")
        );
    }

    #[test]
    fn handles_attribute_order_quotes_and_entities() {
        let x = "<toast><image src='C:\\p\\a&amp;b.jpg' placement='appLogoOverride'/></toast>";
        assert_eq!(parse_avatar_src(x).as_deref(), Some("C:\\p\\a&b.jpg"));
        // gambar bukan appLogoOverride (hero/inline) diabaikan
        let hero = r#"<toast><image placement="hero" src="C:\x\y.png"/></toast>"#;
        assert_eq!(parse_avatar_src(hero), None);
        assert_eq!(parse_avatar_src("<toast><text>hi</text></toast>"), None);
    }

    #[test]
    fn only_safe_local_paths() {
        assert!(local_image_path(r"C:\Users\x\a.png").is_some());
        assert_eq!(local_image_path("file:///C:/Users/x%20y/a.JPG").unwrap().to_str().unwrap(), r"C:\Users\x y\a.JPG");
        assert!(local_image_path(r"\\server\share\a.png").is_none());
        assert!(local_image_path("https://example.com/a.png").is_none());
        assert!(local_image_path(r"C:\Users\x\a.exe").is_none());
        // .tmp hanya diterima di folder Notification Resources milik browser
        assert!(local_image_path(r"C:\U\Google\Chrome\User Data\Notification Resources\3299.tmp").is_some());
        assert!(local_image_path(r"C:\Users\x\a.tmp").is_none());
        assert!(local_image_path(r"C:\Users\..\Windows\a.png").is_none());
        assert!(local_image_path("a.png").is_none());
    }
}

//! Membuka aplikasi asal notifikasi saat diklik.
//!
//! `UserNotificationListener` tidak punya cara "mengeklik" notifikasi, jadi langkahnya ditentukan dari payload toast
//! (`wpndatabase.db`, lihat `notif_img.rs`), dicoba berurutan sampai ada yang berhasil:
//! 1. `activationType="protocol"`: `launch` dibuka lewat shell (skema berbahaya ditolak).
//! 2. COM activator aplikasi (`INotificationActivationCallback`), jalur yang sama dengan klik di Action Center. CLSID-nya
//!    dari registri atau shortcut Start Menu, dan hanya dibuat sebagai local server: DLL aplikasi lain tidak pernah masuk
//!    ke proses ini.
//! 3. Membuka aplikasinya saja lewat AUMID (`shell:AppsFolder`).
//!
//! `launch` bisa berisi data pribadi (URL, id percakapan): dibaca saat diklik, tidak disimpan, dan tidak dicatat di log.

use super::*;
use windows::core::GUID;
use windows::Win32::System::Com::StructuredStorage::{PropVariantToBSTR, PropVariantToGUID};
use windows::Win32::System::Com::{CLSIDFromString, CoAllowSetForegroundWindow, CoCreateInstance, CLSCTX_LOCAL_SERVER};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CLASSES_ROOT, RRF_RT_REG_SZ};
use windows::Win32::UI::Notifications::INotificationActivationCallback;
use windows::Win32::UI::Shell::PropertiesSystem::{
    IPropertyStore, SHGetPropertyStoreFromParsingName, GPS_DEFAULT, PROPERTYKEY,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};

const MAX_LNK: usize = 3000;

// ---------------------------------------------------------------- aturan (murni, bisa dites)
#[derive(Debug, PartialEq)]
enum Step {
    /// Buka `launch` lewat shell.
    Protocol(String),
    /// Panggil COM activator aplikasi dengan `launch`.
    Activator(String),
    /// Buka aplikasinya saja (AUMID).
    App,
}

/// Atribut `launch` dan `activationType` pada elemen `<toast>` saja (bukan pada `<action>`).
fn toast_attrs(xml: &str) -> (Option<String>, Option<String>) {
    let lower = xml.to_ascii_lowercase();
    let Some(start) = lower.find("<toast") else { return (None, None) };
    let mut s = &xml[start + "<toast".len()..];
    let (mut launch, mut activation) = (None, None);
    loop {
        s = s.trim_start();
        if s.is_empty() || s.starts_with('>') || s.starts_with('/') {
            return (launch, activation);
        }
        let Some(eq) = s.find('=') else { return (launch, activation) };
        let name = s[..eq].trim();
        s = s[eq + 1..].trim_start();
        let Some(q) = s.chars().next().filter(|c| *c == '"' || *c == '\'') else { return (launch, activation) };
        let Some(end) = s[1..].find(q) else { return (launch, activation) };
        let value = notif_img::xml_unescape(&s[1..1 + end]);
        if name.eq_ignore_ascii_case("launch") {
            launch = Some(value);
        } else if name.eq_ignore_ascii_case("activationType") {
            activation = Some(value);
        }
        s = &s[1 + end + 1..];
    }
}

/// `launch` berbentuk `skema:...` dan skemanya boleh dibuka. Skema aplikasi tidak bisa didaftar satu per satu, jadi yang
/// ditolak adalah yang bisa menjalankan berkas atau perintah.
fn safe_protocol(launch: &str) -> bool {
    const DENY: [&str; 13] = [
        "file", "ms-msdt", "search", "search-ms", "ms-search", "shell", "javascript", "vbscript", "data", "mk", "res", "hcp", "ms-officecmd",
    ];
    if launch.chars().any(|c| c.is_control()) {
        return false;
    }
    let Some((scheme, _)) = launch.split_once(':') else { return false };
    // satu huruf = drive (C:\...), bukan skema
    let valid = scheme.len() >= 2
        && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid && !DENY.iter().any(|d| scheme.eq_ignore_ascii_case(d))
}

/// Urutan langkah untuk satu notifikasi. Tanpa payload (mis. database terkunci) hanya aplikasinya yang dibuka.
fn plan(payload: Option<&str>) -> Vec<Step> {
    let Some(xml) = payload else { return vec![Step::App] };
    let (launch, activation) = toast_attrs(xml);
    let launch = launch.unwrap_or_default();
    match activation.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("protocol") if safe_protocol(&launch) => vec![Step::Protocol(launch), Step::App],
        Some("protocol") | Some("background") => vec![Step::App],
        _ => vec![Step::Activator(launch), Step::App],
    }
}

/// AUMID untuk `shell:AppsFolder`: tidak kosong dan tanpa karakter yang bisa keluar dari satu nama.
fn app_target(aumid: &str) -> Option<String> {
    let ok = !aumid.is_empty() && !aumid.contains("..") && !aumid.chars().any(|c| c.is_control() || matches!(c, '"' | '<' | '>' | '|'));
    ok.then(|| format!("shell:AppsFolder\\{aumid}"))
}

// ---------------------------------------------------------------- mencari activator
static LNK: Mutex<Option<HashMap<String, GUID>>> = Mutex::new(None);

fn walk(dir: &std::path::Path, depth: u8, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if out.len() >= MAX_LNK {
            return;
        }
        let p = e.path();
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() && depth > 0 {
            walk(&p, depth - 1, out);
        } else if t.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
            out.push(p);
        }
    }
}

/// (AUMID, CLSID activator) dari satu shortcut, bila keduanya ada.
fn read_lnk(path: &std::path::Path) -> Option<(String, GUID)> {
    const FMT: GUID = GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3);
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreFromParsingName(&HSTRING::from(path.as_os_str()), None, GPS_DEFAULT).ok()?;
        let clsid = store.GetValue(&PROPERTYKEY { fmtid: FMT, pid: 26 }).ok()?;
        let clsid = PropVariantToGUID(&clsid).ok()?;
        let id = store.GetValue(&PROPERTYKEY { fmtid: FMT, pid: 5 }).ok()?;
        let id = PropVariantToBSTR(&id).ok()?.to_string();
        (!id.is_empty()).then(|| (id.to_ascii_lowercase(), clsid))
    }
}

fn scan_start_menu() -> HashMap<String, GUID> {
    let mut files = Vec::new();
    for (var, rel) in [("ProgramData", r"Microsoft\Windows\Start Menu\Programs"), ("APPDATA", r"Microsoft\Windows\Start Menu\Programs")] {
        if let Ok(base) = std::env::var(var) {
            walk(&std::path::Path::new(&base).join(rel), 6, &mut files);
        }
    }
    let mut map = HashMap::new();
    for f in files {
        // Satu aplikasi bisa punya beberapa shortcut, dan tidak semuanya berisi activator.
        if let Some((id, clsid)) = read_lnk(&f) {
            map.entry(id).or_insert(clsid);
        }
    }
    map
}

fn registry_activator(aumid: &str) -> Option<GUID> {
    let mut buf = [0u16; 64];
    let mut bytes = std::mem::size_of_val(&buf) as u32;
    let key = HSTRING::from(format!("AppUserModelId\\{aumid}"));
    let err = unsafe {
        RegGetValueW(HKEY_CLASSES_ROOT, &key, w!("CustomActivator"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut c_void), Some(&mut bytes))
    };
    if err != ERROR_SUCCESS {
        return None;
    }
    unsafe { CLSIDFromString(PCWSTR(buf.as_ptr())) }.ok()
}

/// CLSID activator aplikasi: registri lebih dulu, lalu shortcut Start Menu. Hasil pindaian disimpan; aplikasi yang tidak
/// ketemu memicu pindai ulang karena mungkin baru dipasang (klik jarang terjadi, dan ini berjalan di luar UI thread).
fn activator_for(aumid: &str) -> Option<GUID> {
    if let Some(c) = registry_activator(aumid) {
        return Some(c);
    }
    let key = aumid.to_ascii_lowercase();
    if let Some(c) = LNK.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(|m| m.get(&key).copied()) {
        return Some(c);
    }
    let map = scan_start_menu();
    let found = map.get(&key).copied();
    *LNK.lock().unwrap_or_else(|p| p.into_inner()) = Some(map);
    found
}

// ---------------------------------------------------------------- menjalankan
fn activate(aumid: &str, launch: &str) -> bool {
    let Some(clsid) = activator_for(aumid) else { return false };
    unsafe {
        let cb: INotificationActivationCallback = match CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER) {
            Ok(c) => c,
            Err(e) => {
                perf::note(&format!("notifopen: activator gagal dibuat: {e}"));
                return false;
            }
        };
        let _ = CoAllowSetForegroundWindow(&cb, None);
        match cb.Activate(&HSTRING::from(aumid), &HSTRING::from(launch), &[]) {
            Ok(()) => true,
            Err(e) => {
                perf::note(&format!("notifopen: Activate gagal: {e}"));
                false
            }
        }
    }
}

fn shell_open(target: &str) -> bool {
    let file = HSTRING::from(target);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
        lpVerb: w!("open"),
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.is_ok()
}

#[cfg_attr(test, allow(dead_code))]
fn run(id: u32, aumid: Option<&str>) -> bool {
    let payload = notif_img::payload_for(id);
    for step in plan(payload.as_deref()) {
        let (name, ok) = match &step {
            Step::Protocol(launch) => ("protocol", shell_open(launch)),
            Step::Activator(launch) => ("activator", aumid.is_some_and(|a| activate(a, launch))),
            Step::App => ("app", aumid.and_then(app_target).is_some_and(|t| shell_open(&t))),
        };
        perf::note(&format!("notifopen: id={id} langkah={name} ok={ok}"));
        if ok {
            return true;
        }
    }
    false
}

/// Buka notifikasi `id` tanpa menahan UI thread; hasilnya kembali sebagai `NotifEvent::Opened`. Dipanggil langsung dari
/// klik: hak membawa jendela ke depan hanya ada sesaat setelah input pengguna, jadi diteruskan di sini.
#[cfg(not(test))]
pub(crate) fn open(hwnd: isize, slot: notif::NotifSlot, id: u32, aumid: Option<String>) {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
    /// Batas tunggu satu klik: aplikasi yang macet tidak boleh membuat notifikasi menggantung selamanya.
    const TIMEOUT: Duration = Duration::from_secs(8);
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    std::thread::spawn(move || {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            }
            let _ = tx.send(run(id, aumid.as_deref()));
        });
        let ok = rx.recv_timeout(TIMEOUT).unwrap_or(false);
        notif::push(&slot, hwnd, notif::NotifEvent::Opened { id, ok });
    });
}

/// Tes hanya mencatat id-nya (lihat `opened`).
#[cfg(test)]
pub(crate) fn open(_hwnd: isize, _slot: notif::NotifSlot, id: u32, _aumid: Option<String>) {
    OPENED.with(|o| o.borrow_mut().push(id));
}

#[cfg(test)]
thread_local! {
    static OPENED: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Tes: id notifikasi yang diminta dibuka di thread ini (lalu dilupakan).
#[cfg(test)]
pub(crate) fn opened() -> Vec<u32> {
    OPENED.with(|o| std::mem::take(&mut *o.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_the_toast_element() {
        let x = r#"<toast launch="a&amp;b" activationType='protocol' scenario="x"><actions><action activationType="background" launch="no"/></actions></toast>"#;
        assert_eq!(toast_attrs(x), (Some("a&b".to_string()), Some("protocol".to_string())));
        let none = r#"<toast><actions><action launch="no"/></actions></toast>"#;
        assert_eq!(toast_attrs(none), (None, None));
        assert_eq!(toast_attrs("<other/>"), (None, None));
    }

    #[test]
    fn attribute_values_cannot_fake_other_attributes() {
        let x = r#"<toast launch="activationType='protocol' &gt; x"><visual/></toast>"#;
        assert_eq!(toast_attrs(x), (Some("activationType='protocol' > x".to_string()), None));
    }

    #[test]
    fn protocol_schemes() {
        for ok in ["https://example.com/a", "ms-screensketch:edit?x=1", "slack://channel", "app+x.y-z:1"] {
            assert!(safe_protocol(ok), "{ok}");
        }
        for bad in [
            "", "notepad.exe", r"C:\Windows\system32\calc.exe", r"\\server\share\a.exe", "file:///C:/a.exe", "FILE:///C:/a.exe", "ms-msdt:/id x",
            "search-ms:query=a", "shell:AppsFolder\\x", "javascript:alert(1)", "1abc:x", "https://a\nb", ":x",
        ] {
            assert!(!safe_protocol(bad), "{bad}");
        }
    }

    #[test]
    fn plans_follow_the_activation_type() {
        let t = |x: &str| plan(Some(x));
        assert_eq!(t(r#"<toast launch="1|2|p"/>"#), vec![Step::Activator("1|2|p".into()), Step::App]);
        assert_eq!(t(r#"<toast activationType="foreground" launch="x"/>"#), vec![Step::Activator("x".into()), Step::App]);
        assert_eq!(t("<toast/>"), vec![Step::Activator(String::new()), Step::App]);
        assert_eq!(t(r#"<toast activationType="Protocol" launch="ms-x:1"/>"#), vec![Step::Protocol("ms-x:1".into()), Step::App]);
        assert_eq!(t(r#"<toast activationType="protocol" launch="file:///C:/a.exe"/>"#), vec![Step::App]);
        assert_eq!(t(r#"<toast activationType="background" launch="x"/>"#), vec![Step::App]);
        assert_eq!(plan(None), vec![Step::App]);
    }

    #[test]
    fn app_targets() {
        assert_eq!(app_target("Chrome").as_deref(), Some("shell:AppsFolder\\Chrome"));
        assert_eq!(app_target("A_8wek!App").as_deref(), Some("shell:AppsFolder\\A_8wek!App"));
        assert!(app_target("{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\cmd.exe").is_some());
        for bad in ["", "a\"b", "a|b", "..\\x", "a\nb"] {
            assert!(app_target(bad).is_none(), "{bad}");
        }
    }
}

//! Unduh kamus IPADIC Lindera untuk "Romaji lyrics" (tombol Get di Settings > Music & lyrics) ke
//! %APPDATA%\hesewake-notch\dict\ipadic. Arsip dipin ukuran + SHA-256, diekstrak ke folder sementara lalu di-rename,
//! jadi kamus setengah jadi tidak pernah terbaca.

use super::*;
use flate2::read::DeflateDecoder;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use windows::Foundation::AsyncOperationProgressHandler;
use windows::Web::Http::HttpProgress;

/// Versi harus sama dengan crate `lindera` di Cargo.toml (format kamus ikut versi crate).
const URL: &str = "https://github.com/lindera/lindera/releases/download/v6.2.0/lindera-ipadic-6.2.0.zip";
const ZIP_SIZE: u64 = 10_519_550;
const ZIP_SHA256: &str = "5ed4bba6b429030b0387df67d40d5751d35dedef0306f4bac6d957cad5f04b72";

const IDLE: u8 = 0;
const FAILED: u8 = 255;
/// 1..=101 = sedang berjalan (persen + 1).
static STATE: AtomicU8 = AtomicU8::new(IDLE);
/// Unduhan baru selesai: UI menyalakan romaji dan mengonversi lirik lagu sekarang (lihat `take_ready`).
static READY: AtomicBool = AtomicBool::new(false);

pub(crate) enum Dl {
    Idle,
    Running(u8),
    Failed,
}

pub(crate) fn state() -> Dl {
    match STATE.load(Ordering::Relaxed) {
        IDLE => Dl::Idle,
        FAILED => Dl::Failed,
        s => Dl::Running(s - 1),
    }
}

/// Folder tujuan unduhan (juga salah satu lokasi yang dicari `romaji`).
pub(crate) fn user_dir() -> PathBuf {
    data_path("dict").join("ipadic")
}

/// Folder kamus yang sedang dipakai dan boleh dihapus dari Settings: hanya folder `...\dict\ipadic` milik app
/// (hasil unduhan di APPDATA, di samping exe, atau salinan dev di repo), bukan folder pilihan `HESEWAKE_DICT`.
pub(crate) fn removable() -> Option<PathBuf> {
    let dir = romaji::active_dir()?;
    let named = |p: Option<&Path>, n: &str| p.and_then(|p| p.file_name()).map_or(false, |f| f == n);
    let from_env = std::env::var_os("HESEWAKE_DICT").map_or(false, |e| Path::new(&e) == dir);
    (named(Some(&dir), "ipadic") && named(dir.parent(), "dict") && !from_env).then_some(dir)
}

/// Hapus kamus yang sedang dipakai (lihat `removable`). Folder di-rename dulu supaya kamus tidak pernah tertinggal
/// setengah terhapus; gagal (mis. sedang dibaca thread lirik) -> false dan kamus tetap utuh.
pub(crate) fn remove() -> bool {
    if matches!(state(), Dl::Running(_)) {
        return false;
    }
    let Some(dir) = removable() else { return false };
    let trash = dir.with_extension("del");
    let _ = std::fs::remove_dir_all(&trash);
    if let Err(e) = std::fs::rename(&dir, &trash) {
        perf::note(&format!("dict: gagal menghapus: {e}"));
        return false;
    }
    let _ = std::fs::remove_dir_all(&trash);
    romaji::forget_dir();
    true
}

/// True sekali setelah unduhan berhasil.
pub(crate) fn take_ready() -> bool {
    READY.swap(false, Ordering::Relaxed)
}

/// Mulai unduh di thread latar (dipanggil dari UI thread). Kemajuan dan hasil memicu WM_APP untuk redraw.
pub(crate) fn start(hwnd: isize) {
    if matches!(state(), Dl::Running(_)) {
        return;
    }
    STATE.store(1, Ordering::Relaxed);
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let t0 = Instant::now();
        let r = install(hwnd);
        perf::note(&format!("dict: {:?} dalam {} ms", r, t0.elapsed().as_millis()));
        STATE.store(if r.is_ok() { IDLE } else { FAILED }, Ordering::Relaxed);
        READY.store(r.is_ok(), Ordering::Relaxed);
        music::post_refresh(hwnd);
    });
}

fn install(hwnd: isize) -> std::result::Result<(), String> {
    let zip = download(hwnd).ok_or("unduhan gagal")?;
    if zip.len() as u64 != ZIP_SIZE || sha256(&zip).map(|d| hex(&d)).as_deref() != Some(ZIP_SHA256) {
        return Err(format!("checksum tidak cocok ({} byte)", zip.len()));
    }
    let dest = user_dir();
    let part = dest.with_extension("part");
    let _ = std::fs::remove_dir_all(&part);
    std::fs::create_dir_all(&part).map_err(|e| e.to_string())?;
    let r = unzip(&zip, &part).and_then(|_| {
        if part.join("metadata.json").is_file() { Ok(()) } else { Err("metadata.json tidak ada di arsip".into()) }
    });
    if let Err(e) = r {
        let _ = std::fs::remove_dir_all(&part);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&part, &dest).map_err(|e| e.to_string())
}

fn download(hwnd: isize) -> Option<Vec<u8>> {
    let client = HttpClient::new().ok()?;
    let uri = Uri::CreateUri(&HSTRING::from(URL)).ok()?;
    let op = client.GetBufferAsync(&uri).ok()?;
    let _ = op.SetProgress(&AsyncOperationProgressHandler::new(move |_, p: &HttpProgress| {
        let pct = (p.BytesReceived.saturating_mul(100) / ZIP_SIZE).min(99) as u8;
        // Redraw hanya saat persen berubah.
        if STATE.swap(pct + 1, Ordering::Relaxed) != pct + 1 {
            music::post_refresh(hwnd);
        }
        Ok(())
    }));
    let buf = op.get().ok()?;
    let len = buf.Length().ok()? as usize;
    if len as u64 != ZIP_SIZE {
        return None;
    }
    let r = DataReader::FromBuffer(&buf).ok()?;
    let mut v = vec![0u8; len];
    r.ReadBytes(&mut v).ok()?;
    Some(v)
}

pub(crate) fn sha256(data: &[u8]) -> Option<[u8; 32]> {
    use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
    let mut out = [0u8; 32];
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) }.is_ok().then_some(out)
}

pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

fn le(b: &[u8], i: usize, n: usize) -> std::result::Result<usize, String> {
    let s = b.get(i..i + n).ok_or("arsip terpotong")?;
    Ok(s.iter().rev().fold(0usize, |acc, &x| (acc << 8) | x as usize))
}

/// Pembaca zip minimal (stored / deflate, tanpa zip64), cukup untuk arsip kamus yang SHA-256-nya sudah dicek.
/// Folder di dalam arsip diratakan: semua berkas ditulis langsung ke `dest`. Mengembalikan jumlah berkas.
fn unzip(zip: &[u8], dest: &Path) -> std::result::Result<usize, String> {
    let eocd = (0..zip.len().saturating_sub(21)).rev().find(|&i| zip[i..].starts_with(b"PK\x05\x06")).ok_or("bukan zip")?;
    let count = le(zip, eocd + 10, 2)?;
    let mut p = le(zip, eocd + 16, 4)?;
    let mut files = 0;
    for _ in 0..count {
        if !zip.get(p..).map_or(false, |s| s.starts_with(b"PK\x01\x02")) {
            return Err("central directory rusak".into());
        }
        let method = le(zip, p + 10, 2)?;
        let (csize, usize_) = (le(zip, p + 20, 4)?, le(zip, p + 24, 4)?);
        let (nl, el, cl) = (le(zip, p + 28, 2)?, le(zip, p + 30, 2)?, le(zip, p + 32, 2)?);
        let off = le(zip, p + 42, 4)?;
        let name = zip.get(p + 46..p + 46 + nl).and_then(|n| std::str::from_utf8(n).ok()).ok_or("nama rusak")?.to_string();
        p += 46 + nl + el + cl;
        let file = name.rsplit('/').next().unwrap_or("");
        if file.is_empty() {
            continue; // entri folder
        }
        if file.starts_with('.') || !file.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
            return Err(format!("nama berkas tidak sah: {name}"));
        }
        if !zip.get(off..).map_or(false, |s| s.starts_with(b"PK\x03\x04")) {
            return Err("local header rusak".into());
        }
        let start = off + 30 + le(zip, off + 26, 2)? + le(zip, off + 28, 2)?;
        let data = zip.get(start..start + csize).ok_or("data terpotong")?;
        let mut out = std::fs::File::create(dest.join(file)).map_err(|e| e.to_string())?;
        // Didekompresi langsung ke berkas (dict.words ±32 MB tidak ditampung di memori).
        let n = match method {
            0 => std::io::copy(&mut &data[..], &mut out),
            8 => std::io::copy(&mut DeflateDecoder::new(data), &mut out),
            m => return Err(format!("metode kompresi {m} tidak didukung")),
        }
        .map_err(|e| e.to_string())?;
        if n as usize != usize_ {
            return Err(format!("ukuran {name} tidak cocok"));
        }
        files += 1;
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::DeflateEncoder;
    use std::io::Write;

    /// Zip kecil buatan sendiri: (nama, isi, deflate?).
    fn make_zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let (mut out, mut cd) = (Vec::new(), Vec::new());
        let u16le = |v: usize| (v as u16).to_le_bytes();
        let u32le = |v: usize| (v as u32).to_le_bytes();
        for &(name, data, deflate) in entries {
            let body = if deflate {
                let mut e = DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            } else {
                data.to_vec()
            };
            let (off, method) = (out.len(), if deflate { 8 } else { 0 });
            out.extend(b"PK\x03\x04");
            out.extend([20, 0, 0, 0]);
            out.extend(u16le(method));
            out.extend([0; 8]); // waktu, tanggal, crc
            out.extend(u32le(body.len()));
            out.extend(u32le(data.len()));
            out.extend(u16le(name.len()));
            out.extend(u16le(0));
            out.extend(name.as_bytes());
            out.extend(&body);
            cd.extend(b"PK\x01\x02");
            cd.extend([20, 0, 20, 0, 0, 0]);
            cd.extend(u16le(method));
            cd.extend([0; 8]);
            cd.extend(u32le(body.len()));
            cd.extend(u32le(data.len()));
            cd.extend(u16le(name.len()));
            cd.extend([0; 12]); // extra, komentar, disk, atribut internal + eksternal
            cd.extend(u32le(off));
            cd.extend(name.as_bytes());
        }
        let cd_off = out.len();
        out.extend(&cd);
        out.extend(b"PK\x05\x06");
        out.extend([0; 4]);
        out.extend(u16le(entries.len()));
        out.extend(u16le(entries.len()));
        out.extend(u32le(cd.len()));
        out.extend(u32le(cd_off));
        out.extend([0; 2]);
        out
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hesewake-dict-test-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(hex(&sha256(b"abc").unwrap()), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn unzips_flat_stored_and_deflated() {
        let big = vec![b'x'; 50_000];
        let zip = make_zip(&[("lindera-ipadic/", b"", false), ("lindera-ipadic/metadata.json", b"{}", false), ("lindera-ipadic/dict.words", &big, true)]);
        let d = temp_dir("ok");
        assert_eq!(unzip(&zip, &d), Ok(2));
        assert_eq!(std::fs::read(d.join("metadata.json")).unwrap(), b"{}");
        assert_eq!(std::fs::read(d.join("dict.words")).unwrap(), big);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_bad_names_and_garbage() {
        let d = temp_dir("bad");
        assert!(unzip(&make_zip(&[("../..", b"x", false)]), &d).is_err());
        assert!(unzip(&make_zip(&[("a/..\\evil.dll", b"x", false)]), &d).is_err());
        assert!(unzip(b"not a zip at all, definitely not", &d).is_err());
        let mut cut = make_zip(&[("a.bin", &[1u8; 100], false)]);
        cut.drain(40..90);
        assert!(unzip(&cut, &d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}

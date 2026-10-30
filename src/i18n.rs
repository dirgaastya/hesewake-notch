//! UI language. The English text is the key: `tr("Settings")` gives "Pengaturan" when the UI is in Indonesian, and
//! any text without a translation stays English. Text with values uses `{}` placeholders filled by `fill`.
//! A test scans the source for every `tr("...")` so a missing (or stale) translation fails the build.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Language names as shown in Settings (each in its own language). Index = `Settings::lang`.
pub(crate) const LANGS: [&str; 2] = ["English", "Indonesia"];
pub(crate) const EN: usize = 0;
#[cfg(test)]
pub(crate) const ID: usize = 1;

// Tests run in parallel threads, so each test thread has its own language.
#[cfg(not(test))]
static LANG: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(EN);
#[cfg(test)]
thread_local! {
    static LANG: std::cell::Cell<usize> = const { std::cell::Cell::new(EN) };
}

pub(crate) fn set(lang: usize) {
    let lang = if lang < LANGS.len() { lang } else { EN };
    #[cfg(not(test))]
    LANG.store(lang, std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    LANG.with(|l| l.set(lang));
}

pub(crate) fn current() -> usize {
    #[cfg(not(test))]
    return LANG.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    return LANG.with(|l| l.get());
}

/// Tests: puts the language back when dropped (also on panic). The test harness reuses threads, so a test that
/// changes the language must hold one of these or a later test on the same thread runs in that language.
#[cfg(test)]
pub(crate) struct Restore(usize);

#[cfg(test)]
impl Drop for Restore {
    fn drop(&mut self) {
        set(self.0);
    }
}

#[cfg(test)]
pub(crate) fn restore_on_drop() -> Restore {
    Restore(current())
}

/// Next / previous language, wrapping around.
pub(crate) fn step(lang: usize, up: bool) -> usize {
    let n = LANGS.len();
    if up {
        (lang + 1) % n
    } else {
        (lang + n - 1) % n
    }
}

/// `en` in the current UI language. Always call it with a string literal (the source scan relies on that).
/// A `##...` suffix gives the same English text its own translation where space is short, e.g.
/// `tr("Settings##short")` shows "Settings" in English but "Atur" (not "Pengaturan") in Indonesian.
/// (Two `#` so text like "Focus #2" is left alone.)
pub(crate) fn tr(en: &'static str) -> &'static str {
    let base = en.split_once("##").map_or(en, |(b, _)| b);
    if current() == EN {
        return base;
    }
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| ID_TEXT.iter().copied().collect()).get(en).copied().unwrap_or(base)
}

/// Replace each `{}` in `tpl` with the next value: `fill(tr("{} min"), &[5])`.
pub(crate) fn fill(tpl: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(tpl.len() + 8);
    let mut args = args.iter();
    let mut rest = tpl;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        if let Some(a) = args.next() {
            out.push_str(&a.to_string());
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// English -> Indonesian.
const ID_TEXT: &[(&str, &str)] = &[
    // menus and dialogs
    ("Start with Windows", "Mulai bersama Windows"),
    ("Minimal notch", "Notch minimal"),
    ("Windows notifications", "Notifikasi Windows"),
    ("Notification access...", "Akses notifikasi..."),
    ("Settings", "Pengaturan"),
    ("About Hesewake Notch", "Tentang Hesewake Notch"),
    ("Exit", "Keluar"),
    (
        "Hesewake Notch {}\nA personal notch for the top of your screen.\n\nBased on Poch by Commonly Works\n(github.com/klawcodes/poch), MIT License.\nLyrics: LRCLIB (lrclib.net) and lrc.red.\nRomaji: Lindera (IPADIC) and WanaKana.\nFont: Instrument Sans.",
        "Hesewake Notch {}\nNotch pribadi di bagian atas layar.\n\nBerdasarkan Poch karya Commonly Works\n(github.com/klawcodes/poch), Lisensi MIT.\nLirik: LRCLIB (lrclib.net) dan lrc.red.\nRomaji: Lindera (IPADIC) dan WanaKana.\nFont: Instrument Sans.",
    ),
    // slide names
    ("Music", "Musik"),
    ("Water", "Minum"),
    ("To-do", "Tugas"),
    ("Notifications", "Notifikasi"),
    ("System", "Sistem"),
    ("Notifs", "Notif"),
    ("Clips", "Klip"),
    // Settings tabs and rows
    ("Slides", "Slide"),
    ("General", "Umum"),
    ("Alerts", "Indikator"),
    ("Widgets", "Widget"),
    ("Thin line while idle", "Garis tipis saat diam"),
    ("Thin on maximize", "Tipis saat maksimal"),
    ("Off: only on fullscreen", "Mati: hanya layar penuh"),
    ("Follow = cursor", "Ikuti = kursor"),
    ("Lyric timing", "Waktu lirik"),
    ("Show earlier by", "Tampil lebih awal"),
    ("Notch size", "Ukuran notch"),
    ("Idle display", "Tampilan diam"),
    ("Dodge cursor", "Menyingkir"),
    ("Steps aside", "Dari kursor"),
    ("Fullscreen", "Layar penuh"),
    ("Maximized", "Maksimal"),
    ("Cycle: clock/eyes", "Gilir: jam/mata"),
    ("Windows toasts + bell", "Toast Windows + lonceng"),
    ("Contact photos", "Foto kontak"),
    ("In notifications", "Di notifikasi"),
    ("Autostart", "Mulai otomatis"),
    ("Slide hotkeys", "Hotkey slide"),
    ("Ctrl+Alt + slide no.", "Ctrl+Alt + no. slide"),
    ("Language", "Bahasa"),
    ("Menus and slides", "Menu dan slide"),
    ("Volume HUD", "HUD volume"),
    ("Keys and mixer", "Tombol dan mixer"),
    ("Brightness HUD", "HUD kecerahan"),
    ("Fn keys / slider", "Tombol Fn / slider"),
    ("Lock keys", "Tombol kunci"),
    ("Mic / camera dot", "Titik mik / kamera"),
    ("When an app uses them", "Saat dipakai aplikasi"),
    ("Weather", "Cuaca"),
    ("Left of the clock", "Di kiri jam"),
    ("City", "Kota"),
    ("Location", "Lokasi"),
    ("Prayer times", "Waktu sholat"),
    ("Countdown + toast", "Hitung mundur + toast"),
    ("Accent", "Aksen"),
    ("Highlight color", "Warna sorotan"),
    ("Romanize lyrics", "Romanisasi lirik"),
    ("Japanese and Korean", "Jepang dan Korea"),
    ("Downloading dictionary", "Mengunduh kamus"),
    ("Download failed", "Unduhan gagal"),
    ("Retry", "Ulangi"),
    ("Needs 10 MB dictionary", "Perlu kamus 10 MB"),
    ("Get", "Unduh"),
    ("Japanese dictionary", "Kamus Jepang"),
    ("47 MB on disk", "47 MB di disk"),
    ("Sure?", "Yakin?"),
    ("Delete", "Hapus"),
    // Settings values
    ("Primary", "Utama"),
    ("Follow", "Ikuti"),
    ("Display {}", "Layar {}"),
    ("Cycle", "Gilir"),
    ("Clock", "Jam"),
    ("Eyes", "Mata"),
    ("Blue", "Biru"),
    ("Purple", "Ungu"),
    ("Orange", "Oranye"),
    ("Green", "Hijau"),
    ("Teal", "Toska"),
    // dates
    ("Sun", "Min"),
    ("Mon", "Sen"),
    ("Tue", "Sel"),
    ("Wed", "Rab"),
    ("Thu", "Kam"),
    ("Fri", "Jum"),
    ("Sat", "Sab"),
    ("May", "Mei"),
    ("Aug", "Agu"),
    ("Oct", "Okt"),
    ("Dec", "Des"),
    // time ago ("5m" stays the same)
    ("now", "baru"),
    ("{}h", "{}j"),
    ("{}d", "{}h"),
    // toasts and HUD
    ("Brightness", "Kecerahan"),
    ("Charging", "Mengisi daya"),
    ("Charger connected", "Charger tersambung"),
    ("On battery", "Pakai baterai"),
    ("Charger disconnected", "Charger dilepas"),
    ("Battery low", "Baterai lemah"),
    ("Plug in your charger", "Colokkan charger"),
    ("Connected \u{b7} {}%", "Tersambung \u{b7} {}%"),
    ("Connected", "Tersambung"),
    ("Disconnected", "Terputus"),
    ("Prayer time", "Waktu sholat"),
    ("Prayer", "Sholat"),
    ("Sunrise", "Terbit"),
    ("Iftar", "Buka puasa"),
    ("Iftar##short", "Buka"),
    ("Fasting starts", "Mulai puasa"),
    ("Time to break the fast", "Waktunya berbuka"),
    ("Imsak & sunrise", "Imsak & terbit"),
    ("In the prayer list", "Di daftar sholat"),
    ("Ramadan mode", "Mode Ramadan"),
    ("Imsak and iftar", "Imsak dan buka"),
    ("Adhan sound", "Suara adzan"),
    ("At prayer times", "Saat sholat"),
    ("System sound", "Suara sistem"),
    ("File", "File"),
    ("Adhan file", "File adzan"),
    ("mp3 or wav", "mp3 atau wav"),
    ("Choose", "Pilih"),
    ("Change", "Ganti"),
    ("Stop", "Stop"),
    ("Stop adhan", "Hentikan adzan"),
    ("in {}", "{} lagi"),
    ("Mute", "Senyap"),
    ("On", "Nyala"),
    ("Off", "Mati"),
    ("Time to Drink", "Waktunya Minum"),
    ("Stay hydrated! \u{b7} {} / {} L", "Ayo minum! \u{b7} {} / {} L"),
    ("Snooze", "Tunda"),
    ("Drank", "Sudah"),
    ("Camera in use", "Kamera dipakai"),
    ("Microphone in use", "Mikrofon dipakai"),
    ("Download progress", "Progres unduhan"),
    ("Downloads folder", "Folder Downloads"),
    ("Downloaded", "Selesai diunduh"),
    ("Open", "Buka"),
    ("Show in folder", "Lihat di folder"),
    ("+{} more", "+{} lagi"),
    ("Microphone", "Mikrofon"),
    ("Muted", "Senyap"),
    ("Not found", "Tidak ada"),
    ("Mic mute key", "Tombol bisu mic"),
    ("Ctrl+Alt+M, all apps", "Ctrl+Alt+M, semua app"),
    ("Unknown device", "Perangkat tak dikenal"),
    // Timer
    ("Focus", "Fokus"),
    ("Short break", "Istirahat pendek"),
    ("Long break", "Istirahat panjang"),
    ("Focus #{} done", "Fokus #{} selesai"),
    ("Break over", "Istirahat selesai"),
    ("Focus \u{b7} {}", "Fokus \u{b7} {}"),
    ("{} min", "{} mnt"),
    ("{} h", "{} j"),
    ("{} h {} min", "{} j {} mnt"),
    ("{} timer", "Timer {}"),
    ("Time's up", "Waktu habis"),
    ("Countdown##short", "Mundur"),
    ("\u{2212} 1 min", "\u{2212} 1 mnt"),
    ("+ 1 min", "+ 1 mnt"),
    ("Pause", "Jeda"),
    ("Resume", "Lanjut"),
    ("Start", "Mulai"),
    ("Skip to break", "Lewati ke istirahat"),
    ("Skip break", "Lewati istirahat"),
    ("Sessions", "Sesi"),
    ("{} done", "{} selesai"),
    ("Countdown \u{b7} {}", "Hitung mundur \u{b7} {}"),
    // Water
    ("Water |Reminder", "Pengingat |Minum"),
    ("Time to drink", "Waktunya minum"),
    ("Today \u{b7} ", "Hari ini \u{b7} "),
    (" \u{b7} Goal reached", " \u{b7} Target tercapai"),
    (" \u{b7} {} L to go", " \u{b7} kurang {} L"),
    ("Drink", "Minum"),
    ("Settings##short", "Atur"),
    ("Snooze 10 min", "Tunda 10 mnt"),
    ("Bottle", "Botol"),
    ("Glass", "Gelas"),
    ("Goal", "Target"),
    ("Remind", "Ingatkan"),
    ("Hours", "Jam"),
    ("to", "s.d."),
    ("Reset today", "Reset hari ini"),
    ("reminders", "pengingat"),
    ("refills", "isi ulang"),
    // To-do
    ("Tap Add Task to create a task", "Ketuk Tambah untuk membuat tugas"),
    ("Todo|List", "Daftar |Tugas"),
    ("Type a task, Enter to save, Esc to cancel", "Ketik tugas, Enter simpan, Esc batal"),
    ("Tasks done today", "Tugas selesai hari ini"),
    ("Add Task", "Tambah"),
    ("Done", "Selesai"),
    ("Edit Task", "Ubah"),
    ("No tasks yet", "Belum ada tugas"),
    // System
    ("MEMORY", "MEMORI"),
    ("of {} GB", "dari {} GB"),
    ("NETWORK", "JARINGAN"),
    ("BATTERY", "BATERAI"),
    ("Plugged in", "Tercolok"),
    ("{} left", "sisa {}"),
    ("No battery", "Tanpa baterai"),
    ("Desktop PC", "PC desktop"),
    // Clipboard
    ("Copy some text and it shows up here", "Copy teks apa saja, nanti muncul di sini"),
    ("Kept in memory only, gone when the app closes", "Hanya di memori, hilang saat aplikasi ditutup"),
    ("Clear", "Hapus"),
    ("{} of {}", "{} dari {}"),
    ("Copied", "Disalin"),
    // Notifications, Music, Scrap, Ledge
    ("Clear all", "Hapus semua"),
    ("Notification access is off", "Akses notifikasi mati"),
    ("Allow Hesewake Notch in Windows Settings to see them here.", "Izinkan Hesewake Notch di Pengaturan Windows agar tampil di sini."),
    ("Open Settings", "Buka Pengaturan"),
    ("Windows notifications are off", "Notifikasi Windows mati"),
    ("Turn them on from the menu (right-click the notch).", "Nyalakan dari menu (klik kanan notch)."),
    ("No notifications", "Tidak ada notifikasi"),
    ("No media", "Tidak ada media"),
    ("via {}", "dari {}"),
    ("Install", "Pasang"),
    ("Downloading update", "Mengunduh pembaruan"),
    ("Please wait \u{b7} {}%", "Harap tunggu \u{b7} {}%"),
    ("Couldn't download the update", "Gagal mengunduh pembaruan"),
    ("Open the release page instead", "Buka halaman rilis saja"),
    ("Loading lyrics", "Memuat lirik"),
    ("Lyrics not available", "Lirik tidak tersedia"),
    ("Lyrics not synced", "Lirik tidak sinkron"),
    ("Now Playing \u{b7} {}", "Sedang diputar \u{b7} {}"),
    ("Now Playing", "Sedang diputar"),
    ("Esc to save", "Esc untuk simpan"),
    ("Click to write a note", "Klik untuk menulis catatan"),
    ("{} is full", "{} penuh"),
    ("Drop files or folders here", "Taruh file atau folder di sini"),
    // Calendar
    ("Calendar", "Kalender"),
    ("Copy an ICS link first", "Copy link ICS dulu"),
    ("No ICS link copied", "Belum ada link ICS"),
    ("Press again to remove", "Tekan lagi untuk hapus"),
    ("Loading\u{2026}", "Memuat\u{2026}"),
    ("Synced", "Tersinkron"),
    ("Couldn't load it", "Gagal dimuat"),
    ("Paste", "Tempel"),
    ("Remove", "Hapus"),
    ("Join", "Gabung"),
    ("Starts now \u{b7} {}", "Mulai sekarang \u{b7} {}"),
    ("Version {} is available", "Versi {} sudah tersedia"),
    ("Release notes and installer", "Catatan rilis dan installer"),
    ("Download", "Unduh"),
    ("You're up to date", "Sudah versi terbaru"),
    ("Version {}", "Versi {}"),
    ("Couldn't check for updates", "Gagal memeriksa pembaruan"),
    ("Check your internet connection", "Periksa koneksi internet"),
    ("Update available ({})...", "Pembaruan tersedia ({})..."),
    ("Check for updates", "Periksa pembaruan"),
    ("Update notice", "Info pembaruan"),
    ("When a new version is out", "Saat ada versi baru"),
    ("All day", "Seharian"),
    ("Now", "Sekarang"),
    ("Link your calendar in Settings \u{203a} Features", "Hubungkan kalender di Pengaturan \u{203a} Fitur"),
    ("Paste its secret ICS address there", "Tempel alamat ICS rahasianya di sana"),
    ("Loading your calendar\u{2026}", "Memuat kalender\u{2026}"),
    ("Couldn't load your calendar", "Kalender gagal dimuat"),
    ("Check the link in Settings \u{203a} Features", "Cek link-nya di Pengaturan \u{203a} Fitur"),
    ("Nothing else today", "Tidak ada lagi hari ini"),
    ("Next: {} {} \u{b7} {}", "Berikutnya: {} {} \u{b7} {}"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The string literal at the start of `s` (just after its opening quote), unescaped.
    fn literal(s: &str) -> String {
        let mut out = String::new();
        let mut it = s.chars();
        while let Some(c) = it.next() {
            match c {
                '"' => return out,
                '\\' => match it.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('u') => {
                        let hex: String = it.by_ref().skip(1).take_while(|&c| c != '}').collect();
                        out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                    }
                    Some(c) => out.push(c),
                    None => break,
                },
                c => out.push(c),
            }
        }
        panic!("unterminated string literal");
    }

    /// Every `tr("...")` in src/, with the file it is in.
    fn tr_calls() -> Vec<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap() {
            let path = e.unwrap().path();
            if path.extension().is_none_or(|x| x != "rs") || path.file_name().unwrap() == "i18n.rs" {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            let file = path.file_name().unwrap().to_string_lossy().to_string();
            for (i, _) in src.match_indices("tr(") {
                let before = src[..i].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                    continue; // attr(, str(, ...
                }
                let rest = &src[i + 3..];
                assert!(rest.starts_with('"'), "{file}: tr() must be called with a string literal: tr({}", &rest[..rest.len().min(30)]);
                out.push((file.clone(), literal(&rest[1..])));
            }
        }
        out
    }

    #[test]
    fn every_ui_text_has_an_indonesian_translation() {
        let table: HashMap<_, _> = ID_TEXT.iter().copied().collect();
        let missing: Vec<_> = tr_calls().into_iter().filter(|(_, en)| !table.contains_key(en.as_str())).collect();
        assert!(missing.is_empty(), "no Indonesian text for: {missing:#?}");
    }

    #[test]
    fn no_unused_or_duplicate_translations() {
        let used: HashSet<String> = tr_calls().into_iter().map(|(_, en)| en).collect();
        let mut seen = HashSet::new();
        for (en, _) in ID_TEXT {
            assert!(seen.insert(*en), "{en:?} is in the table twice");
            assert!(used.contains(*en), "{en:?} is translated but never used with tr()");
        }
    }

    #[test]
    fn translations_keep_their_placeholders() {
        for (en, id) in ID_TEXT {
            assert_eq!(en.matches("{}").count(), id.matches("{}").count(), "{en:?} -> {id:?}");
            assert_eq!(en.matches('|').count(), id.matches('|').count(), "{en:?} -> {id:?}");
            assert!(!id.is_empty(), "{en:?}");
        }
    }

    #[test]
    fn switching_language() {
        let _lang = restore_on_drop();
        assert_eq!(tr("Settings"), "Settings");
        set(ID);
        assert_eq!(tr("Settings"), "Pengaturan");
        assert_eq!(tr("not in the table"), "not in the table", "falls back to English");
        set(EN);
        assert_eq!(tr("Settings"), "Settings");
        set(99);
        assert_eq!(current(), EN, "unknown language: English");
        // "##..." picks a separate translation for the same English text; English never shows it
        assert_eq!(tr("Focus #{} done"), "Focus #{} done", "a single # is plain text");
        assert_eq!(tr("Settings##short"), "Settings");
        set(ID);
        assert_eq!(tr("Settings##short"), "Atur");
        assert_eq!(tr("Settings"), "Pengaturan");
        set(EN);
        assert_eq!(step(EN, true), ID);
        assert_eq!(step(ID, true), EN);
        assert_eq!(step(EN, false), ID);
    }

    #[test]
    fn fill_replaces_placeholders_in_order() {
        assert_eq!(fill("{} of {}", &[&4, &10]), "4 of 10");
        assert_eq!(fill("Focus #{} done", &[&2]), "Focus #2 done");
        assert_eq!(fill("no placeholders", &[&1]), "no placeholders");
    }
}

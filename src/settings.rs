//! Pengaturan aplikasi (`settings.txt`, baris `kunci=nilai`). Kompatibel dengan berkas lama yang hanya berisi
//! `minimal=` dan `notifications=`; kunci tak dikenal dibuang saat menyimpan.

use super::*;
use crate::slides::{self, SlideCfg};

/// Pilihan "Idle display" di Settings.
pub(crate) const IDLE_DISPLAY_LABELS: [&str; 3] = ["Cycle", "Clock", "Eyes"];

/// `IDLE_DISPLAY_LABELS[i]` in the UI language.
pub(crate) fn idle_display_label(i: usize) -> &'static str {
    [tr("Cycle"), tr("Clock"), tr("Eyes")][i.min(IDLE_DISPLAY_LABELS.len() - 1)]
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Settings {
    /// Mode "Minimal notch": garis tipis saat tidak ada musik.
    pub(crate) minimal: bool,
    /// Tampilkan notifikasi Windows di notch.
    pub(crate) notifications: bool,
    /// Jendela maximized (bukan hanya layar penuh) membuat notch menyusut jadi garis tipis.
    pub(crate) busy_on_maximized: bool,
    /// Baris lirik ditampilkan sekian ms lebih awal dari timestamp (negatif = lebih lambat).
    pub(crate) lyric_lead_ms: i32,
    /// Lirik Jepang (kana/kanji) ditampilkan dalam romaji (butuh kamus `dict\ipadic`, lihat romaji.rs).
    pub(crate) romaji_lyrics: bool,
    /// Foto profil kontak (mis. WhatsApp) pada notifikasi bila tersedia.
    pub(crate) notif_avatar: bool,
    /// Toast saat Caps/Num/Scroll Lock ditekan.
    pub(crate) lock_toast: bool,
    /// Titik + toast saat aplikasi memakai mikrofon/kamera.
    pub(crate) privacy_dot: bool,
    /// Ctrl+Alt+1..8 open the Nth enabled slide (hotkeys.rs).
    pub(crate) slide_hotkeys: bool,
    /// Ctrl+Alt+M mutes the default microphone for every app (mic.rs).
    pub(crate) mic_hotkey: bool,
    /// Running downloads in the Downloads folder: speed in the notch, list on Ledge, toast when done (downloads.rs).
    pub(crate) downloads: bool,
    /// HUD saat volume / kecerahan berubah dari luar (tombol keyboard, mixer).
    pub(crate) hud_volume: bool,
    pub(crate) hud_brightness: bool,
    /// Cuaca di kiri jam (suhu dan ikon kondisi bergantian).
    pub(crate) weather_on: bool,
    /// Monitor tempat notch tampil: 0 = utama, 1 = ikuti kursor, n >= 2 = monitor ke-(n-1).
    pub(crate) monitor: usize,
    /// Indeks di `weather::CITIES`.
    pub(crate) weather_city: usize,
    /// Prayer times: 15-minute countdown + a toast when the prayer starts (city = weather city).
    pub(crate) prayer_on: bool,
    /// Imsak and Terbit in the prayer list.
    pub(crate) imsak_terbit: bool,
    /// Ramadan mode, turned on by hand: Imsak countdown and toast, "Iftar" at Maghrib.
    pub(crate) ramadan: bool,
    /// Adhan when a prayer starts: adhan::OFF / SYSTEM / FILE, and the file the user picked.
    pub(crate) adhan: u8,
    pub(crate) adhan_file: String,
    /// Indeks di `scale::SIZES`: 0 = Auto (ikut scaling Windows), selebihnya ukuran tetap.
    pub(crate) notch_size: usize,
    /// Tampilan notch idle: 0 = bergantian jam/mata (10/15 menit), 1 = jam saja, 2 = mata saja.
    pub(crate) idle_display: usize,
    /// Index into `accent::ACCENTS` (0 = default blue).
    pub(crate) accent: usize,
    /// UI language, index into `i18n::LANGS` (0 = English).
    pub(crate) lang: usize,
    /// Look for a newer release at startup and every 24 hours.
    pub(crate) update_check: bool,
    /// The newest version the person was already told about (told once per version).
    pub(crate) update_seen: String,
    pub(crate) slides: SlideCfg,
}

pub(crate) const LEAD_MIN: i32 = -500;
pub(crate) const LEAD_MAX: i32 = 1000;
pub(crate) const LEAD_STEP: i32 = 50;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            minimal: false,
            notifications: true,
            busy_on_maximized: false,
            lyric_lead_ms: 150,
            romaji_lyrics: false,
            notif_avatar: true,
            lock_toast: true,
            privacy_dot: true,
            slide_hotkeys: true,
            mic_hotkey: true,
            downloads: true,
            hud_volume: true,
            hud_brightness: true,
            weather_on: true,
            monitor: 0,
            weather_city: 0,
            prayer_on: true,
            imsak_terbit: true,
            ramadan: false,
            adhan: adhan::OFF,
            adhan_file: String::new(),
            notch_size: 0,
            idle_display: 0,
            accent: 0,
            lang: 0,
            update_check: true,
            update_seen: String::new(),
            slides: slides::default_cfg(),
        }
    }
}

impl Settings {
    pub(crate) fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let mut slides_val: Option<&str> = None;
        for line in text.lines() {
            let Some((k, v)) = line.trim().split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let flag = v == "1";
            match k {
                "minimal" => s.minimal = flag,
                "notifications" => s.notifications = flag,
                "busy_on_maximized" => s.busy_on_maximized = flag,
                "romaji_lyrics" => s.romaji_lyrics = flag,
                "notif_avatar" => s.notif_avatar = flag,
                "lock_toast" => s.lock_toast = flag,
                "privacy_dot" => s.privacy_dot = flag,
                "slide_hotkeys" => s.slide_hotkeys = flag,
                "mic_hotkey" => s.mic_hotkey = flag,
                "downloads" => s.downloads = flag,
                "hud_volume" => s.hud_volume = flag,
                "hud_brightness" => s.hud_brightness = flag,
                "weather" => s.weather_on = flag,
                "prayer" => s.prayer_on = flag,
                "imsak_terbit" => s.imsak_terbit = flag,
                "ramadan" => s.ramadan = flag,
                "adhan" => s.adhan = v.parse::<u8>().unwrap_or(adhan::OFF).min(adhan::MODES - 1),
                "adhan_file" => s.adhan_file = v.to_string(),
                "update_check" => s.update_check = flag,
                "update_seen" => s.update_seen = v.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')).take(32).collect(),
                "monitor" => {
                    if let Ok(n) = v.parse::<usize>() {
                        s.monitor = n.min(32);
                    }
                }
                "weather_city" => {
                    if let Ok(n) = v.parse::<usize>() {
                        s.weather_city = n.min(weather::CITIES.len() - 1);
                    }
                }
                "idle_display" => {
                    if let Ok(n) = v.parse::<usize>() {
                        s.idle_display = n.min(IDLE_DISPLAY_LABELS.len() - 1);
                    }
                }
                "accent" => {
                    if let Ok(n) = v.parse::<usize>() {
                        s.accent = n.min(crate::accent::ACCENTS.len() - 1);
                    }
                }
                "lang" => {
                    if let Ok(n) = v.parse::<usize>() {
                        s.lang = if n < crate::i18n::LANGS.len() { n } else { crate::i18n::EN };
                    }
                }
                "notch_size" => {
                    if let Ok(n) = v.parse::<usize>() {
                        s.notch_size = n.min(scale::SIZES.len() - 1);
                    }
                }
                "lyric_lead_ms" => {
                    if let Ok(n) = v.parse::<i32>() {
                        s.lyric_lead_ms = n.clamp(LEAD_MIN, LEAD_MAX);
                    }
                }
                "slides" => slides_val = Some(v),
                _ => {}
            }
        }
        if let Some(v) = slides_val {
            s.slides = slides::parse_slides(v);
        }
        s
    }

    pub(crate) fn serialize(&self) -> String {
        format!(
            "minimal={}\nnotifications={}\nbusy_on_maximized={}\nlyric_lead_ms={}\nromaji_lyrics={}\nnotif_avatar={}\nlock_toast={}\nprivacy_dot={}\nslide_hotkeys={}\nmic_hotkey={}\ndownloads={}\nhud_volume={}\nhud_brightness={}\nweather={}\nweather_city={}\nprayer={}\nimsak_terbit={}\nramadan={}\nadhan={}\nadhan_file={}\nmonitor={}\nnotch_size={}\nidle_display={}\naccent={}\nlang={}\nupdate_check={}\nupdate_seen={}\nslides={}\n",
            self.minimal as u8,
            self.notifications as u8,
            self.busy_on_maximized as u8,
            self.lyric_lead_ms,
            self.romaji_lyrics as u8,
            self.notif_avatar as u8,
            self.lock_toast as u8,
            self.privacy_dot as u8,
            self.slide_hotkeys as u8,
            self.mic_hotkey as u8,
            self.downloads as u8,
            self.hud_volume as u8,
            self.hud_brightness as u8,
            self.weather_on as u8,
            self.weather_city,
            self.prayer_on as u8,
            self.imsak_terbit as u8,
            self.ramadan as u8,
            self.adhan,
            self.adhan_file.replace(['\r', '\n'], ""),
            self.monitor,
            self.notch_size,
            self.idle_display,
            self.accent,
            self.lang,
            self.update_check as u8,
            self.update_seen,
            slides::format_slides(&self.slides),
        )
    }

    pub(crate) fn load() -> Settings {
        std::fs::read_to_string(data_path("settings.txt")).map(|t| Settings::parse(&t)).unwrap_or_default()
    }

    pub(crate) fn save(&self) {
        if cfg!(test) {
            return; // tes tidak boleh menimpa pengaturan asli
        }
        write_data("settings.txt", self.serialize());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slides::SlideId;

    #[test]
    fn legacy_file_keeps_defaults_for_new_keys() {
        let s = Settings::parse("minimal=1\nnotifications=0\n");
        assert!(s.minimal);
        assert!(!s.notifications);
        assert!(!s.busy_on_maximized);
        assert_eq!(s.lyric_lead_ms, 150);
        assert!(!s.romaji_lyrics);
        assert_eq!(s.slides.len(), SlideId::USER.len());
    }

    #[test]
    fn roundtrip_and_clamp() {
        let mut s = Settings::default();
        s.minimal = true;
        s.busy_on_maximized = true;
        s.lyric_lead_ms = -200;
        s.romaji_lyrics = true;
        s.slides = slides::parse_slides("scrap,!music,water,todo,ledge,notifs");
        assert_eq!(Settings::parse(&s.serialize()), s);
        assert_eq!(Settings::parse("lyric_lead_ms=99999").lyric_lead_ms, LEAD_MAX);
        assert_eq!(Settings::parse("lyric_lead_ms=-99999").lyric_lead_ms, LEAD_MIN);
        assert_eq!(Settings::parse("lyric_lead_ms=abc").lyric_lead_ms, 150);
        assert_eq!(s.slides[1], (SlideId::Music, false));
    }

    #[test]
    fn prayer_toggle_roundtrip_and_default_on() {
        assert!(Settings::parse("minimal=1
").prayer_on, "berkas lama = nyala");
        let mut s = Settings::default();
        s.prayer_on = false;
        assert!(!Settings::parse(&s.serialize()).prayer_on);
    }

    #[test]
    fn notch_size_roundtrip_and_default() {
        assert_eq!(Settings::parse("minimal=1
").notch_size, 0, "berkas lama = Auto");
        let mut s = Settings::default();
        s.notch_size = 3;
        assert_eq!(Settings::parse(&s.serialize()).notch_size, 3);
        assert_eq!(Settings::parse("notch_size=99").notch_size, scale::SIZES.len() - 1);
        assert_eq!(Settings::parse("notch_size=x").notch_size, 0);
    }

    #[test]
    fn accent_roundtrip_and_default() {
        assert_eq!(Settings::parse("minimal=1\n").accent, 0, "old file = blue");
        let mut s = Settings::default();
        s.accent = 3;
        assert_eq!(Settings::parse(&s.serialize()).accent, 3);
        assert_eq!(Settings::parse("accent=99").accent, crate::accent::ACCENTS.len() - 1);
        assert_eq!(Settings::parse("accent=x").accent, 0);
    }

    #[test]
    fn update_settings_roundtrip_and_default() {
        let s = Settings::parse("minimal=1\n");
        assert!(s.update_check, "on by default, also for old files");
        assert_eq!(s.update_seen, "");
        let mut s = Settings::default();
        s.update_check = false;
        s.update_seen = "0.9.0".into();
        let back = Settings::parse(&s.serialize());
        assert_eq!((back.update_check, back.update_seen.as_str()), (false, "0.9.0"));
        assert_eq!(Settings::parse("update_seen=0.9.0\nx\r\n").update_seen, "0.9.0");
    }

    #[test]
    fn lang_roundtrip_and_default() {
        assert_eq!(Settings::parse("minimal=1\n").lang, crate::i18n::EN, "old file = English");
        let mut s = Settings::default();
        s.lang = crate::i18n::ID;
        assert_eq!(Settings::parse(&s.serialize()).lang, crate::i18n::ID);
        assert_eq!(Settings::parse("lang=99").lang, crate::i18n::EN, "unknown = English");
        assert_eq!(Settings::parse("lang=x").lang, crate::i18n::EN);
    }

    #[test]
    fn downloads_roundtrip_and_default() {
        assert!(Settings::parse("minimal=1\n").downloads, "old file = on");
        let mut s = Settings::default();
        s.downloads = false;
        assert!(!Settings::parse(&s.serialize()).downloads);
    }

    #[test]
    fn prayer_extras_roundtrip_and_defaults() {
        let s = Settings::parse("prayer=1\n");
        assert!(s.imsak_terbit, "Imsak and Terbit shown by default");
        assert!(!s.ramadan, "Ramadan mode is turned on by hand");
        assert_eq!((s.adhan, s.adhan_file.as_str()), (adhan::OFF, ""), "no sound unless the user turns it on");
        let mut s = Settings::default();
        s.ramadan = true;
        s.imsak_terbit = false;
        s.adhan = adhan::FILE;
        s.adhan_file = "C:\\Users\\me\\Music\\adzan = makkah.mp3".into();
        let back = Settings::parse(&s.serialize());
        assert!(back.ramadan && !back.imsak_terbit);
        assert_eq!((back.adhan, back.adhan_file.as_str()), (adhan::FILE, "C:\\Users\\me\\Music\\adzan = makkah.mp3"));
        assert_eq!(Settings::parse("adhan=9\n").adhan, adhan::MODES - 1, "out of range is clamped");
    }

    #[test]
    fn mic_hotkey_roundtrip_and_default() {
        assert!(Settings::parse("minimal=1\n").mic_hotkey, "old file = on");
        let mut s = Settings::default();
        s.mic_hotkey = false;
        assert!(!Settings::parse(&s.serialize()).mic_hotkey);
    }

    #[test]
    fn slide_hotkeys_roundtrip_and_default() {
        assert!(Settings::parse("minimal=1\n").slide_hotkeys, "old file = on");
        let mut s = Settings::default();
        s.slide_hotkeys = false;
        assert!(!Settings::parse(&s.serialize()).slide_hotkeys);
    }

    #[test]
    fn idle_display_roundtrip_and_default() {
        assert_eq!(Settings::default().idle_display, 0);
        let mut s = Settings::default();
        s.idle_display = 2;
        assert_eq!(Settings::parse(&s.serialize()).idle_display, 2);
        assert_eq!(Settings::parse("idle_display=9").idle_display, IDLE_DISPLAY_LABELS.len() - 1);
        assert_eq!(Settings::parse("idle_display=x").idle_display, 0);
    }

    #[test]
    fn garbage_lines_are_ignored() {
        let s = Settings::parse("???\n=1\nminimal\nslides=\n");
        assert_eq!(s, Settings::default());
    }
}

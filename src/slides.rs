//! Registri slide: identitas slide, urutan tampil, dan penyimpanannya di `settings.txt`.

/// Identitas slide. Slide baru ditambahkan di sini (dan di `USER`) agar bisa diatur dari Settings.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum SlideId {
    Music,
    Water,
    Todo,
    Ledge,
    Scrap,
    Notifications,
    /// CPU, memori, jaringan, baterai. Default tersembunyi.
    System,
    /// Stopwatch dan hitung mundur. Default tersembunyi.
    Timer,
    /// Recently copied texts. Hidden by default.
    Clipboard,
    /// What's left on today's calendar. Hidden by default.
    Agenda,
    /// Today's prayer times (prayer.rs). Only in the carousel while "Prayer times" is on.
    Prayer,
}

impl SlideId {
    /// Slide yang bisa diatur pengguna (urutan bawaan).
    pub(crate) const USER: [SlideId; 11] = [
        SlideId::Music,
        SlideId::Water,
        SlideId::Todo,
        SlideId::Ledge,
        SlideId::Scrap,
        SlideId::Notifications,
        SlideId::System,
        SlideId::Timer,
        SlideId::Clipboard,
        SlideId::Agenda,
        SlideId::Prayer,
    ];

    /// Aktif secara bawaan? (slide opsional dimulai tersembunyi)
    pub(crate) fn default_on(self) -> bool {
        !matches!(self, SlideId::System | SlideId::Timer | SlideId::Clipboard | SlideId::Agenda)
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            SlideId::Music => "music",
            SlideId::Water => "water",
            SlideId::Todo => "todo",
            SlideId::Ledge => "ledge",
            SlideId::Scrap => "scrap",
            SlideId::Notifications => "notifs",
            SlideId::System => "system",
            SlideId::Timer => "timer",
            SlideId::Clipboard => "clipboard",
            SlideId::Agenda => "agenda",
            SlideId::Prayer => "prayer",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            SlideId::Music => crate::i18n::tr("Music"),
            SlideId::Water => crate::i18n::tr("Water"),
            SlideId::Todo => crate::i18n::tr("To-do"),
            SlideId::Ledge => "Ledge",
            SlideId::Scrap => "Scrap",
            SlideId::Notifications => crate::i18n::tr("Notifications"),
            SlideId::System => crate::i18n::tr("System"),
            SlideId::Timer => "Timer",
            SlideId::Clipboard => "Clipboard",
            SlideId::Agenda => "Agenda",
            SlideId::Prayer => crate::i18n::tr("Prayer"),
        }
    }

    pub(crate) fn from_key(k: &str) -> Option<SlideId> {
        SlideId::USER.iter().copied().find(|s| s.key() == k)
    }
}

/// Konfigurasi slide: urutan tampil + apakah aktif.
pub(crate) type SlideCfg = Vec<(SlideId, bool)>;

pub(crate) fn default_cfg() -> SlideCfg {
    SlideId::USER.iter().map(|&s| (s, s.default_on())).collect()
}

/// `music,water,!todo,...` -> konfigurasi. Awalan `!` = tersembunyi. Id tak dikenal dan duplikat diabaikan; slide yang
/// belum disebut ditambahkan di akhir (aktif), supaya versi baru dengan slide baru tetap menampilkannya.
pub(crate) fn parse_slides(s: &str) -> SlideCfg {
    let mut out: SlideCfg = Vec::new();
    for tok in s.split(',') {
        let tok = tok.trim();
        let (on, key) = match tok.strip_prefix('!') {
            Some(k) => (false, k),
            None => (true, tok),
        };
        if let Some(id) = SlideId::from_key(key) {
            if !out.iter().any(|(s, _)| *s == id) {
                out.push((id, on));
            }
        }
    }
    for id in SlideId::USER {
        if !out.iter().any(|(s, _)| *s == id) {
            out.push((id, id.default_on()));
        }
    }
    out
}

pub(crate) fn format_slides(cfg: &[(SlideId, bool)]) -> String {
    cfg.iter().map(|(s, on)| if *on { s.key().to_string() } else { format!("!{}", s.key()) }).collect::<Vec<_>>().join(",")
}

/// The slide config as it applies now: the Prayer slide counts as off while prayer times are off, so the carousel and
/// the slide hotkey numbers both skip it.
pub(crate) fn effective(cfg: &[(SlideId, bool)], prayer_on: bool) -> SlideCfg {
    cfg.iter().map(|&(s, on)| (s, on && (prayer_on || s != SlideId::Prayer))).collect()
}

/// Urutan slide yang benar-benar tampil: yang aktif. Tanpa slide aktif, slide pertama tetap ditampilkan agar carousel
/// tidak kosong.
pub(crate) fn visible_order(cfg: &[(SlideId, bool)]) -> Vec<SlideId> {
    let mut v: Vec<SlideId> = cfg.iter().filter(|(_, on)| *on).map(|(s, _)| *s).collect();
    if v.is_empty() {
        v.push(cfg.first().map_or(SlideId::Music, |(s, _)| *s));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_order_and_hidden() {
        let c = parse_slides("water,!music,todo");
        assert_eq!(&c[..3], &[(SlideId::Water, true), (SlideId::Music, false), (SlideId::Todo, true)]);
        // slide yang tidak disebut ditambahkan di akhir
        assert_eq!(c.len(), SlideId::USER.len());
        assert_eq!(c[3], (SlideId::Ledge, true));
        // slide opsional yang baru muncul di versi baru tetap tersembunyi; Prayer is on (it follows the prayer times setting)
        assert_eq!(
            &c[c.len() - 5..],
            &[(SlideId::System, false), (SlideId::Timer, false), (SlideId::Clipboard, false), (SlideId::Agenda, false), (SlideId::Prayer, true)]
        );
    }

    #[test]
    fn ignores_unknown_and_duplicates() {
        let c = parse_slides("music,bogus,music,!music,settings");
        assert_eq!(c.iter().filter(|(s, _)| *s == SlideId::Music).count(), 1);
        assert_eq!(c.len(), SlideId::USER.len());
    }

    #[test]
    fn roundtrips() {
        let c = parse_slides("scrap,!ledge,music,water,todo,notifs");
        assert_eq!(parse_slides(&format_slides(&c)), c);
        assert_eq!(format_slides(&c), "scrap,!ledge,music,water,todo,notifs,!system,!timer,!clipboard,!agenda,prayer");
    }

    #[test]
    fn only_enabled_slides_show() {
        let c = parse_slides("!music,water");
        let v = visible_order(&c);
        assert!(!v.contains(&SlideId::Music));
        assert_eq!(v[0], SlideId::Water);
        // semua disembunyikan: satu slide cadangan
        let all_off: SlideCfg = SlideId::USER.iter().map(|&s| (s, false)).collect();
        assert_eq!(visible_order(&all_off), vec![SlideId::Music]);
    }
}

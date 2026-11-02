//! Lirik sinkron dari LRCLIB (https://lrclib.net), dengan lrc.red (https://lrc.red) sebagai cadangan: fetch di thread
//! terpisah, parser LRC, pencarian baris aktif.

use super::*;
use serde::Deserialize;

/// lrc.red mewajibkan User-Agent berisi nama aplikasi, tautan, dan email kontak.
const UA: &str = concat!("HesewakeNotch/", env!("CARGO_PKG_VERSION"), " (+https://github.com/dirgaastya/hesewake-notch; hesewake@gmail.com)");

/// Dari mana lirik berasal; ditampilkan sebagai kredit di mode lirik.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Source {
    #[default]
    Lrclib,
    LrcRed,
}

impl Source {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Source::Lrclib => "LRCLIB",
            Source::LrcRed => "lrc.red",
        }
    }

    pub(crate) fn url(self) -> &'static str {
        match self {
            Source::Lrclib => "https://lrclib.net",
            Source::LrcRed => "https://lrc.red",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LyricLine {
    pub(crate) at_ms: u32,
    pub(crate) text: String,
    /// Romaji untuk baris Jepang (diisi `romaji::annotate`), kosong bila tidak ada.
    pub(crate) roma: String,
}

impl LyricLine {
    /// Teks yang ditampilkan: romaji bila diminta dan tersedia, selain itu teks asli.
    pub(crate) fn shown(&self, romaji: bool) -> &str {
        if romaji && !self.roma.is_empty() {
            &self.roma
        } else {
            &self.text
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) enum Lyrics {
    #[default]
    None,
    Loading,
    Synced(Vec<LyricLine>, Source),
    /// Hanya ada lirik tanpa timestamp: tidak ditampilkan per baris.
    Plain,
    Unavailable,
}

pub(crate) type LyricsSlot = Arc<Mutex<Option<(String, Lyrics)>>>;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Entry {
    duration: Option<f64>,
    instrumental: Option<bool>,
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
}

fn pct(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            _ => o.push_str(&format!("%{:02X}", b)),
        }
    }
    o
}

pub(crate) fn get_string(url: &str) -> Option<String> {
    let client = HttpClient::new().ok()?;
    if let Ok(h) = client.DefaultRequestHeaders() {
        if let Ok(ua) = h.UserAgent() {
            let _ = ua.TryParseAdd(&HSTRING::from(UA));
        }
    }
    let uri = Uri::CreateUri(&HSTRING::from(url)).ok()?;
    let body = match client.GetStringAsync(&uri).and_then(|op| op.get()) {
        Ok(b) => b,
        Err(e) => {
            perf::note(&format!("http gagal: {:?}", e));
            return None;
        }
    };
    let s = body.to_string();
    // Batasi ukuran respons (lirik normal < 100 KB).
    if s.is_empty() || s.len() > 2_000_000 {
        None
    } else {
        Some(s)
    }
}

/// Layanan video yang tidak punya lirik lagu (judul serial/film malah menarik lirik lagu yang salah).
const NO_LYRICS_SOURCES: [&str; 1] = ["netflix"];
/// Media yang lebih panjang dari ini (film, episode, podcast) tidak dicarikan lirik.
const MAX_LYRIC_SECS: u32 = 20 * 60;

/// True bila media ini bukan lagu: tidak perlu fetch lirik (dan tidak menampilkan "Loading lyrics").
pub(crate) fn skip(source: &str, title: &str, artist: &str, dur_secs: u32) -> bool {
    let hit = |s: &str| {
        let s = s.to_lowercase();
        NO_LYRICS_SOURCES.iter().any(|k| s.contains(k))
    };
    hit(source) || hit(title) || hit(artist) || dur_secs >= MAX_LYRIC_SECS
}

/// Cari lirik untuk satu lagu: LRCLIB dulu, lalu lrc.red bila LRCLIB tidak punya lirik sinkron. Blocking: panggil dari
/// thread terpisah.
pub(crate) fn fetch(title: &str, artist: &str, dur_secs: u32) -> Lyrics {
    let first = lookup(title, artist, dur_secs);
    if matches!(first, Lyrics::Synced(..)) {
        return first;
    }
    // some players join two names of a song ("ハッピーエンド - Happy End"); a database knows it by one of them
    for part in title_parts(title) {
        let l = lookup(&part, artist, dur_secs);
        if matches!(l, Lyrics::Synced(..)) {
            return l;
        }
    }
    first
}

fn lookup(title: &str, artist: &str, dur_secs: u32) -> Lyrics {
    let first = lrclib(title, artist, dur_secs);
    if matches!(first, Lyrics::Synced(..)) {
        return first;
    }
    lrcred(title, artist, dur_secs).unwrap_or(first)
}

/// The parts of a title written as "A - B", for another try when the whole title finds nothing. Empty if it is not.
fn title_parts(title: &str) -> Vec<String> {
    let parts: Vec<String> = title.split(" - ").map(|p| p.trim().to_string()).filter(|p| p.chars().count() >= 2).collect();
    if parts.len() < 2 {
        return Vec::new();
    }
    parts
}

fn lrclib(title: &str, artist: &str, dur_secs: u32) -> Lyrics {
    let base = format!("track_name={}&artist_name={}", pct(title), pct(artist));
    // 1) Pencocokan tepat (pakai durasi bila ada); 404 -> get_string mengembalikan None.
    let exact = if dur_secs > 0 {
        format!("https://lrclib.net/api/get?{}&duration={}", base, dur_secs)
    } else {
        format!("https://lrclib.net/api/get?{}", base)
    };
    if let Some(body) = get_string(&exact) {
        if let Ok(e) = serde_json::from_str::<Entry>(&body) {
            if let Some(l) = from_entry(&e) {
                return l;
            }
        }
    }
    // 2) Pencarian: ambil yang punya lirik sinkron dengan durasi paling mendekati.
    let Some(body) = get_string(&format!("https://lrclib.net/api/search?{}", base)) else {
        return Lyrics::Unavailable;
    };
    let entries: Vec<Entry> = serde_json::from_str(&body).unwrap_or_default();
    let dist = |e: &Entry| e.duration.map_or(f64::MAX, |d| (d - dur_secs as f64).abs());
    let best = entries
        .iter()
        .filter(|e| from_entry(e).is_some())
        .min_by(|a, b| {
            let (sa, sb) = (a.synced_lyrics.as_deref().map_or(true, str::is_empty), b.synced_lyrics.as_deref().map_or(true, str::is_empty));
            sa.cmp(&sb).then(dist(a).total_cmp(&dist(b)))
        });
    best.and_then(from_entry).unwrap_or(Lyrics::Unavailable)
}

fn from_entry(e: &Entry) -> Option<Lyrics> {
    if let Some(s) = e.synced_lyrics.as_deref().filter(|s| !s.trim().is_empty()) {
        let lines = parse_lrc(s);
        if !lines.is_empty() {
            return Some(Lyrics::Synced(lines, Source::Lrclib));
        }
    }
    if e.instrumental == Some(true) {
        return Some(Lyrics::Unavailable);
    }
    if e.plain_lyrics.as_deref().map_or(false, |s| !s.trim().is_empty()) {
        return Some(Lyrics::Plain);
    }
    None
}

/// `[mm:ss.xx]teks`, termasuk beberapa timestamp per baris. Tag metadata (`[ar:...]`) diabaikan.
pub(crate) fn parse_lrc(body: &str) -> Vec<LyricLine> {
    let mut out = Vec::new();
    for raw in body.lines() {
        let mut rest = raw.trim_start();
        let mut stamps: Vec<u32> = Vec::new();
        while let Some(r) = rest.strip_prefix('[') {
            let Some((ts, after)) = r.split_once(']') else { break };
            match parse_ts(ts) {
                Some(ms) => stamps.push(ms),
                None => break,
            }
            rest = after;
        }
        let text = strip_word_tags(rest);
        for ms in stamps {
            out.push(LyricLine { at_ms: ms, text: text.clone(), roma: String::new() });
        }
    }
    out.sort_by_key(|l| l.at_ms);
    out
}

/// Hapus penanda waktu per kata (`<00:19.76>`, "enhanced LRC" dari lrc.red) dan rapikan spasi.
fn strip_word_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        let (head, tail) = rest.split_at(i);
        out.push_str(head);
        match tail.find('>') {
            Some(j) if tail[1..j].bytes().all(|b| b.is_ascii_digit() || b == b':' || b == b'.') => rest = &tail[j + 1..],
            _ => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------- lrc.red

/// Satu hasil pencarian lrc.red (`/search.json`).
#[derive(Deserialize, Default)]
#[serde(default)]
struct Hit {
    isrc: String,
    title: String,
    artist: String,
    duration: f64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Hits {
    hits: Vec<Hit>,
}

/// Huruf dan angka saja, huruf kecil: "Hati-Hati di Jalan" -> "hatihatidijalan".
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Hasil yang cocok dengan lagu ini: judul sama persis (versi lain seperti "(Acoustic)" tidak ikut), artis saling
/// mengandung, dan durasi paling dekat (maksimal 3 detik bila durasi lagu diketahui).
fn pick_hit<'a>(hits: &'a [Hit], title: &str, artist: &str, dur_secs: u32) -> Option<&'a Hit> {
    let t = norm(title);
    if t.is_empty() {
        return None;
    }
    hits.iter()
        .filter(|h| !h.isrc.is_empty() && norm(&h.title) == t)
        .filter(|h| artist.trim().is_empty() || same_artist(artist, &h.artist))
        .filter(|h| dur_secs == 0 || (h.duration - dur_secs as f64).abs() <= 3.0)
        .min_by(|x, y| {
            let d = |h: &Hit| (h.duration - dur_secs as f64).abs();
            d(x).total_cmp(&d(y))
        })
}

/// Artis yang sama: satu nama mengandung yang lain, atau berbagi satu kata (urutan nama Jepang bisa terbalik:
/// "Hata Motohiro" / "Motohiro Hata"). Kata pendek ("the", "feat") tidak dihitung.
fn same_artist(a: &str, b: &str) -> bool {
    let (na, nb) = (norm(a), norm(b));
    if na.is_empty() || nb.is_empty() {
        return true;
    }
    if na.contains(&nb) || nb.contains(&na) {
        return true;
    }
    let words = |s: &str| -> Vec<String> { s.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 4).map(|w| w.to_lowercase()).collect() };
    let wb = words(b);
    words(a).iter().any(|w| wb.contains(w))
}

/// ISRC dari kode lrc.red hanya huruf dan angka; tolak yang lain sebelum dipakai di URL.
fn valid_isrc(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Cari di lrc.red: pencarian judul + artis (lalu judul saja), lalu unduh file LRC lagu yang cocok.
fn lrcred(title: &str, artist: &str, dur_secs: u32) -> Option<Lyrics> {
    for q in [format!("{title} {artist}"), title.to_string()] {
        let body = get_string(&format!("https://lrc.red/search.json?q={}", pct(&q)))?;
        let hits: Hits = serde_json::from_str(&body).unwrap_or_default();
        let Some(hit) = pick_hit(&hits.hits, title, artist, dur_secs) else { continue };
        if !valid_isrc(&hit.isrc) {
            return None;
        }
        let lrc = get_string(&format!("https://lrc.red/s/{}.lrc", hit.isrc))?;
        let lines = parse_lrc(&lrc);
        return (!lines.is_empty()).then(|| Lyrics::Synced(lines, Source::LrcRed));
    }
    None
}

fn parse_ts(ts: &str) -> Option<u32> {
    let (m, rest) = ts.split_once(':')?;
    let (s, frac) = rest.split_once(['.', ':']).unwrap_or((rest, "0"));
    let m: u32 = m.trim().parse().ok()?;
    let s: u32 = s.trim().parse().ok()?;
    if frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // ".5" = 500 ms, ".50" = 500 ms, ".500" = 500 ms
    let ms: u32 = match frac.len() {
        1 => frac.parse::<u32>().ok()? * 100,
        2 => frac.parse::<u32>().ok()? * 10,
        _ => frac[..3].parse().ok()?,
    };
    Some((m * 60 + s) * 1000 + ms)
}

/// Indeks baris aktif pada posisi `pos_ms`, atau None sebelum baris pertama.
pub(crate) fn current_line(lines: &[LyricLine], pos_ms: u32, lead_ms: i32) -> Option<usize> {
    // `lead_ms` > 0: baris tampil lebih awal dari timestamp supaya terasa tepat dengan suara.
    let p = (pos_ms as i64 + lead_ms as i64).max(0) as u32;
    lines.partition_point(|l| l.at_ms <= p).checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(isrc: &str, title: &str, artist: &str, duration: f64) -> Hit {
        Hit { isrc: isrc.into(), title: title.into(), artist: artist.into(), duration }
    }

    #[test]
    fn word_timestamps_are_stripped_from_lrc_lines() {
        let lines = parse_lrc("[ti:Song]
[length:03:58]

[00:19.76]<00:19.76>When <00:20.11>you were <00:20.33>here <00:20.61>before <00:21.90>
[00:24.59]<00:24.59>Couldn't <00:25.03>look");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "When you were here before");
        assert_eq!(lines[0].at_ms, 19_760);
        assert_eq!(lines[1].text, "Couldn't look");
        // text that only looks like a tag stays
        assert_eq!(strip_word_tags("a <3 b > c <i>x</i>"), "a <3 b > c <i>x</i>");
        assert_eq!(strip_word_tags("<00:01.00>"), "");
    }

    #[test]
    fn a_title_with_two_names_is_tried_by_each_name() {
        assert_eq!(title_parts("ハッピーエンド - Happy End"), vec!["ハッピーエンド", "Happy End"]);
        assert!(title_parts("Happy End").is_empty(), "one name: nothing more to try");
        assert!(title_parts("Hati-Hati di Jalan").is_empty(), "a hyphen without spaces is part of the name");
        assert!(title_parts("A - B").is_empty(), "one letter is not a name worth a search");
    }

    #[test]
    fn lrcred_hit_must_match_title_artist_and_duration() {
        let hits = vec![
            hit("GBAYE9300465", "Creep (Acoustic)", "Radiohead", 258.64),
            hit("GBAYE9200070", "Creep", "Radiohead", 238.64),
            hit("XXAYE9200070", "Creep", "Someone Else", 238.0),
            hit("", "Creep", "Radiohead", 238.0),
        ];
        let best = pick_hit(&hits, "Creep", "Radiohead", 238).expect("a match");
        assert_eq!(best.isrc, "GBAYE9200070", "the exact version, not the acoustic one or another artist");
        assert!(pick_hit(&hits, "Creep", "Radiohead", 300).is_none(), "duration too far off");
        assert!(pick_hit(&hits, "Creep", "Radiohead", 0).is_some(), "unknown duration still matches by title and artist");
        assert!(pick_hit(&hits, "Karma Police", "Radiohead", 238).is_none());
        assert!(pick_hit(&hits, "", "Radiohead", 238).is_none());
        // "Hati-Hati di Jalan" and "hati hati di jalan" are the same title; the artist may be a longer credit
        let h2 = vec![hit("IDA012300001", "Hati-Hati di Jalan", "Tulus", 245.0)];
        assert!(pick_hit(&h2, "hati hati di jalan", "Tulus", 246).is_some());
        let h3 = vec![hit("JPA012300001", "Himawari no Yakusoku", "Hata Motohiro", 315.0)];
        assert!(pick_hit(&h3, "Himawari no Yakusoku", "Motohiro Hata, Someone", 315).is_some());
    }

    #[test]
    fn isrc_is_checked_before_it_goes_into_a_url() {
        assert!(valid_isrc("GBAYE9200070"));
        for bad in ["", "GBAYE920007", "GBAYE92000700", "GBAYE9200/70", "GBAYE9200 70", "../../etc/pw"] {
            assert!(!valid_isrc(bad), "{bad:?}");
        }
    }

    /// Talks to the real lrc.red (run with `cargo test -- --ignored`): the search finds the song and its LRC parses.
    #[test]
    #[ignore = "network"]
    fn real_lrcred_finds_a_song() {
        let Some(Lyrics::Synced(lines, Source::LrcRed)) = lrcred("Creep", "Radiohead", 238) else { panic!("lrc.red answered with lyrics") };
        assert!(lines.len() > 20 && lines.iter().all(|l| !l.text.contains('<')), "word tags are gone");
        assert!(lrcred("zzzz qqqq not a song", "nobody", 100).is_none());
    }

    #[test]
    fn sources_have_a_name_and_a_link() {
        assert_eq!((Source::Lrclib.name(), Source::LrcRed.name()), ("LRCLIB", "lrc.red"));
        assert!(Source::LrcRed.url().starts_with("https://lrc.red") && Source::Lrclib.url().starts_with("https://lrclib.net"));
    }

    #[test]
    fn user_agent_names_the_app_with_a_link_and_contact() {
        assert!(UA.starts_with("HesewakeNotch/") && UA.contains("https://github.com/dirgaastya/hesewake-notch") && UA.contains("hesewake@gmail.com"));
    }

    #[test]
    fn skips_video_services_and_long_media() {
        assert!(skip("Netflix", "Stranger Things", "", 0));
        assert!(skip("Chrome", "Money Heist", "NETFLIX", 2400));
        assert!(skip("Chrome", "Watch Netflix", "", 0));
        assert!(skip("Spotify", "Long mix", "DJ", 20 * 60));
        assert!(!skip("Spotify", "Lemon", "Kenshi Yonezu", 255));
        assert!(!skip("Chrome", "One more time, One more chance", "Masayoshi Yamazaki", 0));
    }

    #[test]
    fn parses_basic_and_multi_stamps() {
        let l = parse_lrc("[ar:Someone]\n[00:12.34]Hello\n[00:10.00][00:50.5]Again\n[01:00.123]\n");
        assert_eq!(l.len(), 4);
        assert_eq!(l[0], LyricLine { at_ms: 10_000, text: "Again".into(), roma: String::new() });
        assert_eq!(l[1].at_ms, 12_340);
        assert_eq!(l[2].at_ms, 50_500);
        assert_eq!(l[3], LyricLine { at_ms: 60_123, text: String::new(), roma: String::new() });
    }

    #[test]
    fn shown_prefers_romaji_only_when_present() {
        let mut l = LyricLine { at_ms: 0, text: "夜に駆ける".into(), roma: String::new() };
        assert_eq!(l.shown(true), "夜に駆ける");
        l.roma = "yoru ni kakeru".into();
        assert_eq!(l.shown(true), "yoru ni kakeru");
        assert_eq!(l.shown(false), "夜に駆ける");
    }

    #[test]
    fn current_line_edges() {
        let l = parse_lrc("[00:10.00]a\n[00:20.00]b\n");
        const LEAD: i32 = 150;
        assert_eq!(current_line(&l, 0, LEAD), None);
        assert_eq!(current_line(&l, 10_000 - LEAD as u32, LEAD), Some(0));
        assert_eq!(current_line(&l, 19_000, LEAD), Some(0));
        assert_eq!(current_line(&l, 20_000 - LEAD as u32, LEAD), Some(1));
        assert_eq!(current_line(&l, 999_999, LEAD), Some(1));
        // offset negatif menunda baris; offset besar tidak membuat posisi negatif
        assert_eq!(current_line(&l, 10_000, -500), None);
        assert_eq!(current_line(&l, 0, -500), None);
    }

    #[test]
    fn rejects_garbage_stamps() {
        assert!(parse_lrc("[xx:yy]nope\nplain text").is_empty());
    }
}

// ---------------------------------------------------------------- state & helper di App
const FADE_SECS: f32 = 0.2;

impl App {
    /// Posisi lagu sekarang (ms): sampel terakhir dari media thread + waktu berjalan sejak sampel itu.
    pub(crate) fn lyric_pos_ms(&self) -> u32 {
        let extra = if self.media.playing { self.pos_at.elapsed().as_millis().min(5_000) as u32 } else { 0 };
        let p = self.media.pos_ms.saturating_add(extra);
        if self.media.dur > 0 {
            p.min(self.media.dur.saturating_mul(1000))
        } else {
            p
        }
    }

    /// Hitung ulang baris aktif. True bila berubah (perlu redraw).
    pub(crate) fn lyric_update(&mut self) -> bool {
        let idx = match &self.lyrics {
            Lyrics::Synced(lines, _) => current_line(lines, self.lyric_pos_ms(), self.cfg.lyric_lead_ms),
            _ => None,
        };
        if idx == self.lyric_idx {
            return false;
        }
        self.lyric_idx = idx;
        self.lyric_t = Instant::now();
        // Lebar teks baris baru (tebal) menentukan lebar notch kecil.
        let text = self.lyric_text().map(str::to_string);
        self.lyric_w = match (text, self.gfx.as_mut()) {
            (Some(t), Some(g)) => g.width_ex(&t, LYRIC_SIZE, true),
            _ => 0.0,
        };
        true
    }

    /// Lirik sinkron tersedia untuk lagu yang sedang main (menentukan lebar notch kecil).
    pub(crate) fn lyric_synced(&self) -> bool {
        self.media.playing && matches!(self.lyrics, Lyrics::Synced(..))
    }

    /// Teks baris aktif (None sebelum baris pertama atau saat jeda instrumental/baris kosong).
    pub(crate) fn lyric_text(&self) -> Option<&str> {
        match (&self.lyrics, self.lyric_idx) {
            (Lyrics::Synced(lines, _), Some(i)) => lines.get(i).map(|l| l.shown(self.cfg.romaji_lyrics)).filter(|t| !t.is_empty()),
            _ => None,
        }
    }

    /// Setelah toggle "Romaji lyrics": lirik lagu sekarang yang belum punya romaji dikonversi di thread terpisah
    /// (hasilnya lewat slot lirik + WM_APP, jalur yang sama dengan fetch), dan baris aktif dihitung ulang.
    pub(crate) fn romaji_refresh(&mut self) {
        if let (true, Lyrics::Synced(lines, src)) = (self.cfg.romaji_lyrics, &self.lyrics) {
            if lines.iter().all(|l| l.roma.is_empty()) && lines.iter().any(|l| romaji::needs_roma(&l.text)) {
                let (mut v, src, key, slot, hwnd) = (lines.clone(), *src, self.art_key.clone(), self.lyr_shared.clone(), self.hwnd);
                std::thread::spawn(move || {
                    romaji::annotate(&mut v);
                    let mut s = slot.lock().unwrap_or_else(|p| p.into_inner());
                    // Jangan menimpa hasil fetch lagu berikutnya yang sudah menunggu di slot.
                    if s.as_ref().map_or(true, |(k, _)| *k == key) {
                        *s = Some((key, Lyrics::Synced(v, src)));
                    }
                    drop(s);
                    music::post_refresh(hwnd);
                });
            }
        }
        // `lyric_update` menghitung ulang teks dan lebar notch kecil.
        self.lyric_idx = None;
    }

    /// 0..1: fade-in baris yang baru ganti.
    pub(crate) fn lyric_alpha(&self) -> f32 {
        (self.lyric_t.elapsed().as_secs_f32() / FADE_SECS).clamp(0.0, 1.0)
    }

    pub(crate) fn lyric_fading(&self) -> bool {
        self.lyric_synced() && self.lyric_text().is_some() && self.lyric_t.elapsed().as_secs_f32() < FADE_SECS
    }
}

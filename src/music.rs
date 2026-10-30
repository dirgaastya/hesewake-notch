//! Fitur Music: GSMTC media thread, artwork, slide Now Playing.

use super::*;
use std::sync::atomic::{AtomicU8, Ordering};

const ART: u32 = 88;
const ART_ANIM_SECS: f32 = 0.35;

pub(crate) const MARQUEE_SPEED: f32 = 38.0;
pub(crate) const MARQUEE_GAP: f32 = 34.0;
pub(crate) const MARQUEE_PAUSE: f32 = 1.2;

const ONLINE_ARTWORK: bool = true;

/// Fingerprint cover terakhir yang dikirim ke UI (GSMTC maupun online). Dipakai untuk mengenali thumbnail basi:
/// GSMTC sering masih mengembalikan thumbnail lagu sebelumnya sesaat setelah judul berganti.
static LAST_ART_SIG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) const MX: f32 = LX + 104.0;
pub(crate) const MW: f32 = RX - MX; // kolom teks/kontrol sampai tepi kanan (blok jam di kanan sudah dihapus)
pub(crate) const MCX: f32 = MX + MW / 2.0;
pub(crate) const MCY: f32 = 128.0;

#[derive(Clone, Default, PartialEq)]
pub(crate) struct Media {
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) playing: bool,
    pub(crate) pos: u32,
    pub(crate) dur: u32,
    /// Posisi dalam milidetik (sudah diekstrapolasi dari LastUpdatedTime) untuk sinkron lirik.
    pub(crate) pos_ms: u32,
    /// Nama pemutar yang dikontrol dan jumlah pemutar media yang aktif (>1 = bisa berpindah sumber).
    pub(crate) source: String,
    pub(crate) sources: u32,
}

impl Media {
    pub(crate) fn key(&self) -> String {
        format!("{}|{}", self.title, self.artist)
    }
}

pub(crate) struct ArtData {
    pub(crate) key: String,
    pub(crate) rgba: Vec<u8>,
}

pub(crate) enum Cmd {
    Prev,
    Toggle,
    Next,
    Seek(f32),
    /// Pindah ke pemutar media berikutnya (bila ada lebih dari satu).
    CycleSource,
}

use windows::Media::Control::GlobalSystemMediaTransportControlsSession as MediaSession;

/// Sesi yang dikontrol: pilihan pengguna (AUMID) bila masih ada, selain itu sesi sistem saat ini.
fn pick_session(mgr: &SessionManager, chosen: &mut Option<String>) -> Option<MediaSession> {
    if let Some(id) = chosen.as_ref() {
        if let Ok(list) = mgr.GetSessions() {
            for s in list {
                if s.SourceAppUserModelId().map(|h| h.to_string()).ok().as_deref() == Some(id.as_str()) {
                    return Some(s);
                }
            }
        }
        *chosen = None; // aplikasinya sudah tutup
    }
    mgr.GetCurrentSession().ok()
}

fn cycle_source(mgr: &SessionManager, chosen: &mut Option<String>) {
    let ids: Vec<String> = mgr
        .GetSessions()
        .map(|l| l.into_iter().filter_map(|s| s.SourceAppUserModelId().ok().map(|h| h.to_string())).collect())
        .unwrap_or_default();
    if ids.len() < 2 {
        *chosen = None;
        return;
    }
    let cur = chosen
        .clone()
        .or_else(|| mgr.GetCurrentSession().ok().and_then(|s| s.SourceAppUserModelId().ok()).map(|h| h.to_string()));
    let pos = cur.and_then(|c| ids.iter().position(|i| *i == c)).unwrap_or(0);
    *chosen = Some(ids[(pos + 1) % ids.len()].clone());
}

/// Nama pendek dari AUMID: `SpotifyAB.SpotifyMusic_zpdn...!Spotify` -> `Spotify`, `chrome.exe` -> `Chrome`.
pub(crate) fn friendly_source(id: &str) -> String {
    let id = id.strip_suffix(".exe").unwrap_or(id);
    let base = match id.rsplit_once('!') {
        Some((_, after)) => after,
        None => id.split('_').next().unwrap_or(id),
    };
    let base = base.rsplit('.').next().unwrap_or(base);
    let mut c = base.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn fmt_time(s: u32) -> String {
    format!("{}:{:02}", s / 60, s % 60)
}

// ---------------------------------------------------------------- media thread
fn read_thumbnail(props: &MediaProps) -> Option<Vec<u8>> {
    let r = props.Thumbnail().ok()?;
    let s = r.OpenReadAsync().ok()?.get().ok()?;
    let size = s.Size().ok()? as u32;
    if size == 0 || size > 8_000_000 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&s).ok()?;
    reader.LoadAsync(size).ok()?.get().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    Some(buf)
}

fn http_get(url: &str) -> Option<Vec<u8>> {
    let client = HttpClient::new().ok()?;
    let uri = Uri::CreateUri(&HSTRING::from(url)).ok()?;
    let buf = client.GetBufferAsync(&uri).ok()?.get().ok()?;
    let len = buf.Length().ok()? as usize;
    if len == 0 || len > 8_000_000 {
        return None;
    }
    let r = DataReader::FromBuffer(&buf).ok()?;
    let mut v = vec![0u8; len];
    r.ReadBytes(&mut v).ok()?;
    Some(v)
}

fn urlenc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => o.push(b as char),
            b' ' => o.push('+'),
            _ => o.push_str(&format!("%{:02X}", b)),
        }
    }
    o
}

/// Buang tambahan di judul yang membuat pencarian gagal: "Judul / Subjudul", "Judul - Remastered", "(feat. X)", "[Live]".
fn clean_title(t: &str) -> String {
    let mut end = t.len();
    for sep in [" / ", " - ", " (", " [", " feat", " ft."] {
        if let Some(i) = t.find(sep) {
            end = end.min(i);
        }
    }
    let c = t[..end].trim_matches(|c: char| c.is_whitespace() || matches!(c, '/' | '-' | '|' | ','));
    if c.is_empty() { t.trim().to_string() } else { c.to_string() }
}

fn fetch_online(title: &str, artist: &str) -> Option<Vec<u8>> {
    let term = format!("{} {}", artist, clean_title(title));
    let url = format!("https://itunes.apple.com/search?media=music&entity=song&limit=1&term={}", urlenc(&term));
    let json = String::from_utf8(http_get(&url)?).ok()?;
    let k = json.find("\"artworkUrl100\"")? + "\"artworkUrl100\"".len();
    let rest = &json[k..];
    let a = rest.find('"')? + 1;
    let rest = &rest[a..];
    let b = rest.find('"')?;
    let art = rest[..b].replace("100x100bb", "300x300bb");
    http_get(&art)
}

fn decode_art(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = decode_limited(bytes)?;
    let img = img.resize_to_fill(ART, ART, image::imageops::FilterType::Triangle);
    Some(img.to_rgba8().into_raw())
}

/// Waktu sekarang dalam satuan 100 ns sejak 1601-01-01 (basis DateTime WinRT).
fn now_filetime() -> i64 {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    (d.as_nanos() / 100) as i64 + 116_444_736_000_000_000
}

pub(crate) fn post_refresh(hwnd: isize) {
    unsafe {
        let _ = PostMessageW(HWND(hwnd as *mut c_void), WM_APP, WPARAM(0), LPARAM(0));
    }
}

pub(crate) fn media_thread(
    hwnd: isize,
    shared: Arc<Mutex<Media>>,
    art_shared: Arc<Mutex<Option<ArtData>>>,
    lyr_shared: lyrics::LyricsSlot,
    rx: Receiver<Cmd>,
) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mgr = match SessionManager::RequestAsync().and_then(|a| a.get()) {
        Ok(m) => m,
        Err(_) => return,
    };
    let mut art_key = String::new();
    let mut tries = 0u32;
    let mut art_done = false;
    let mut online_started = false;
    let mut prev_sig: u64 = 0; // cover lagu sebelumnya
    let mut last_sig: u64 = 0; // cover GSMTC terakhir yang diposting untuk lagu ini
    let mut verify_left: u32 = 0; // pembacaan ulang thumbnail GSMTC setelah diposting
    let mut online_state = Arc::new(AtomicU8::new(0)); // 0 = berjalan/belum, 1 = berhasil, 2 = gagal
    let mut last_dur: u32 = 0;
    let mut net_tick: u32 = 0;
    let mut chosen: Option<String> = None; // pemutar pilihan pengguna (AUMID)

    loop {
        if let Ok(cmd) = rx.recv_timeout(Duration::from_millis(1000)) {
            if matches!(cmd, Cmd::CycleSource) {
                cycle_source(&mgr, &mut chosen);
            } else if let Some(s) = pick_session(&mgr, &mut chosen) {
                match cmd {
                    Cmd::Prev => {
                        let _ = s.TrySkipPreviousAsync();
                    }
                    Cmd::Toggle => {
                        let _ = s.TryTogglePlayPauseAsync();
                    }
                    Cmd::Next => {
                        let _ = s.TrySkipNextAsync();
                    }
                    Cmd::Seek(f) => {
                        let ticks = (f as f64 * last_dur as f64 * 10_000_000.0) as i64;
                        let _ = s.TryChangePlaybackPositionAsync(ticks);
                    }
                    Cmd::CycleSource => {}
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }

        net_tick = net_tick.wrapping_add(1);
        if net_tick % 5 == 1 && net::poll() {
            post_refresh(hwnd);
        }

        let mut new = Media::default();
        let mut props: Option<MediaProps> = None;
        let mut is_spotify = false;
        new.sources = mgr.GetSessions().and_then(|l| l.Size()).unwrap_or(0);
        if let Some(s) = pick_session(&mgr, &mut chosen) {
            new.source = s.SourceAppUserModelId().map(|h| friendly_source(&h.to_string())).unwrap_or_default();
            is_spotify = s
                .SourceAppUserModelId()
                .map(|h| h.to_string().to_lowercase().contains("spotify"))
                .unwrap_or(false);
            if let Ok(p) = s.TryGetMediaPropertiesAsync().and_then(|a| a.get()) {
                new.title = p.Title().map(|h| h.to_string()).unwrap_or_default();
                new.artist = p.Artist().map(|h| h.to_string()).unwrap_or_default();
                props = Some(p);
            }
            if let Ok(i) = s.GetPlaybackInfo() {
                if let Ok(st) = i.PlaybackStatus() {
                    new.playing = st == PlayStatus::Playing;
                }
            }
            if let Ok(tl) = s.GetTimelineProperties() {
                let pos = tl.Position().map(|t| t.Duration).unwrap_or(0);
                let start = tl.StartTime().map(|t| t.Duration).unwrap_or(0);
                let end = tl.EndTime().map(|t| t.Duration).unwrap_or(0);
                new.pos = ((pos - start).max(0) / 10_000_000) as u32;
                new.dur = ((end - start).max(0) / 10_000_000) as u32;
                let mut ms = (pos - start).max(0) / 10_000;
                if new.playing {
                    // Posisi dilaporkan pada LastUpdatedTime (Spotify jarang memperbaruinya), jadi tambahkan
                    // seluruh waktu yang sudah lewat sejak itu. Dibatasi durasi lagu di bawah.
                    if let Some(t) = tl.LastUpdatedTime().ok().filter(|t| t.UniversalTime > 0) {
                        let since = (now_filetime() - t.UniversalTime).max(0);
                        ms += since / 10_000;
                    }
                }
                let dur_ms = ((end - start).max(0) / 10_000).max(0);
                let ms = if dur_ms > 0 { ms.min(dur_ms) } else { ms };
                new.pos_ms = ms as u32;
                new.pos = (ms / 1000) as u32;
            }
        }
        last_dur = new.dur;

        let key = new.key();
        if key != art_key {
            art_key = key.clone();
            perf::note(&format!("art: track '{}' spotify={}", key, is_spotify));
            tries = 0;
            prev_sig = LAST_ART_SIG.load(Ordering::Relaxed);
            last_sig = 0;
            verify_left = 0;
            art_done = new.title.is_empty();
            online_started = false;
            online_state = Arc::new(AtomicU8::new(0));
            if !new.title.is_empty() && lyrics::skip(&new.source, &new.title, &new.artist, new.dur) {
                perf::note(&format!("lyrics: dilewati (bukan lagu) source={} dur={}", new.source, new.dur));
            } else if !new.title.is_empty() {
                let (t, ar, k, d) = (new.title.clone(), new.artist.clone(), key.clone(), new.dur);
                let slot = lyr_shared.clone();
                std::thread::spawn(move || {
                    unsafe {
                        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                    }
                    let mut l = lyrics::fetch(&t, &ar, d);
                    if let lyrics::Lyrics::Synced(lines, _) = &mut l {
                        romaji::annotate(lines);
                    }
                    *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some((k, l));
                    post_refresh(hwnd);
                });
            }
        }
        // Spotify: thumbnail GSMTC-nya bisa memuat logo/teks Spotify, jadi pakai cover bersih dari iTunes dulu.
        // Thumbnail GSMTC hanya dipakai kalau pencarian online gagal.
        let spotify_wait = is_spotify && ONLINE_ARTWORK && online_state.load(Ordering::Relaxed) != 2;
        if spotify_wait && online_state.load(Ordering::Relaxed) == 1 {
            art_done = true;
        }
        if !art_done && tries < 15 {
            if !spotify_wait {
                tries += 1;
            }
            let bytes = if spotify_wait { None } else { props.as_ref().and_then(|p| read_thumbnail(p)) };
            perf::note(&format!("art: try={} spotify_wait={} thumb={:?} props={}", tries, spotify_wait, bytes.as_ref().map(|b| b.len()), props.is_some()));
            if let Some(rgba) = bytes.and_then(|b| decode_art(&b)) {
                let sig = sig_of(&rgba);
                // Thumbnail sama dengan lagu sebelumnya = kemungkinan belum diperbarui; tunggu beberapa detik dulu
                // (album yang sama untuk dua lagu berurutan tetap diterima setelah itu).
                if sig == prev_sig && tries < 4 {
                    // coba lagi di putaran berikutnya
                } else {
                    *art_shared.lock().unwrap() = Some(ArtData { key: key.clone(), rgba });
                    LAST_ART_SIG.store(sig, Ordering::Relaxed);
                    last_sig = sig;
                    verify_left = 4;
                    art_done = true;
                    post_refresh(hwnd);
                }
            } else if ONLINE_ARTWORK && (spotify_wait || tries >= 3) && !online_started {
                online_started = true;
                let (t, ar, k) = (new.title.clone(), new.artist.clone(), key.clone());
                let slot = art_shared.clone();
                let state = online_state.clone();
                std::thread::spawn(move || {
                    unsafe {
                        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                    }
                    // Percobaan pertama gagal -> state=2 segera (thumbnail GSMTC boleh dipakai), tapi tetap
                    // mencoba lagi di latar belakang; cover bersih menimpa thumbnail bila akhirnya berhasil.
                    for (i, wait) in [0u64, 3, 8, 15].iter().enumerate() {
                        if *wait > 0 {
                            std::thread::sleep(Duration::from_secs(*wait));
                        }
                        if let Some(rgba) = fetch_online(&t, &ar).and_then(|b| decode_art(&b)) {
                            LAST_ART_SIG.store(sig_of(&rgba), Ordering::Relaxed);
                            *slot.lock().unwrap() = Some(ArtData { key: k.clone(), rgba });
                            state.store(1, Ordering::Relaxed);
                            post_refresh(hwnd);
                            perf::note(&format!("art online ok '{} - {}' (attempt {})", ar, t, i + 1));
                            break;
                        }
                        perf::note(&format!("art online FAIL '{} - {}' (attempt {})", ar, t, i + 1));
                        state.store(2, Ordering::Relaxed);
                    }
                    trim_memory();
                });
            }
        }

        // Thumbnail GSMTC kadang baru benar beberapa detik setelah judul berganti: baca ulang sebentar dan
        // perbarui bila berbeda dari yang sudah diposting (dan bukan cover lagu sebelumnya).
        if art_done && verify_left > 0 && !spotify_wait {
            verify_left -= 1;
            if let Some(rgba) = props.as_ref().and_then(|p| read_thumbnail(p)).and_then(|b| decode_art(&b)) {
                let sig = sig_of(&rgba);
                if sig != last_sig && sig != prev_sig {
                    *art_shared.lock().unwrap() = Some(ArtData { key: key.clone(), rgba });
                    LAST_ART_SIG.store(sig, Ordering::Relaxed);
                    last_sig = sig;
                    post_refresh(hwnd);
                }
            }
        }

        let changed = {
            let mut g = shared.lock().unwrap();
            if *g != new {
                *g = new;
                true
            } else {
                false
            }
        };
        if changed {
            post_refresh(hwnd);
        }
    }
}

// ---------------------------------------------------------------- artwork tint (colour bleed under the artwork)
// Two dominant colours are extracted ONCE per artwork (in set_art, from the already decoded 88x88 RGBA that the
// media thread produced), then cross-faded over TINT_SECS whenever the artwork changes.
type Pal = [[f32; 3]; 2];

const TINT_SECS: f32 = 0.7; // palette cross-fade duration
const TINT_PEAK: f32 = 0.45; // max opacity of the tint at its strongest point (keeps text contrast)
const TF_W: u32 = 16; // the tint is a tiny 16x24 field, bilinearly stretched over the notch (=> very smooth)
const TF_H: u32 = 24;
const HUE_BINS: usize = 24;

#[derive(Clone)]
pub(crate) struct Tint {
    from: Pal,
    to: Pal,
    from_a: f32,
    to_a: f32,
    t0: Option<Instant>,
    sig: u64, // fingerprint of the artwork the palette was extracted from
    field: Option<Pixmap>, // reused 16x24 colour field (avoids an allocation every frame)
}

impl Default for Tint {
    fn default() -> Self {
        Tint { from: [[0.0; 3]; 2], to: [[0.0; 3]; 2], from_a: 0.0, to_a: 0.0, t0: None, sig: 0, field: None }
    }
}

impl Tint {
    fn progress(&self) -> f32 {
        match self.t0 {
            Some(t) => {
                let x = (t.elapsed().as_secs_f32() / TINT_SECS).clamp(0.0, 1.0);
                x * x * (3.0 - 2.0 * x)
            }
            None => 1.0,
        }
    }

    /// Current (palette, strength), i.e. the in-flight cross-fade value.
    fn eval(&self) -> (Pal, f32) {
        let p = self.progress();
        let mut c = self.to;
        for i in 0..2 {
            for k in 0..3 {
                c[i][k] = self.from[i][k] + (self.to[i][k] - self.from[i][k]) * p;
            }
        }
        (c, self.from_a + (self.to_a - self.from_a) * p)
    }

    pub(crate) fn animating(&self) -> bool {
        self.t0.map_or(false, |t| t.elapsed().as_secs_f32() < TINT_SECS)
    }

    /// Start a transition from whatever is on screen right now to `pal` (None = fade out).
    fn retarget(&mut self, pal: Option<Pal>) {
        let (cur, cur_a) = self.eval();
        match pal {
            Some(p) => {
                self.from = if cur_a < 0.02 { p } else { cur }; // nothing visible yet -> just fade the new one in
                self.to = p;
                self.to_a = 1.0;
            }
            None => {
                self.from = cur;
                self.to = cur;
                self.to_a = 0.0;
            }
        }
        self.from_a = cur_a;
        self.t0 = Some(Instant::now());
    }
}

fn sig_of(b: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rgb_to_hsv(c: [f32; 3]) -> (f32, f32, f32) {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    let d = mx - mn;
    let h = if d < 1e-6 {
        0.0
    } else if mx == c[0] {
        ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if mx == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    (h * 60.0, if mx <= 0.0 { 0.0 } else { d / mx }, mx)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}

/// Pull a colour into a range that looks good as a soft tint over black (not neon, not muddy).
fn fit_tint(c: [f32; 3]) -> [f32; 3] {
    let (h, s, v) = rgb_to_hsv(c);
    let s = if s > 0.12 { (s * 1.1).clamp(0.45, 0.95) } else { s };
    let mut o = hsv_to_rgb(h, s, v.clamp(0.45, 0.72));
    let lum = 0.2126 * o[0] + 0.7152 * o[1] + 0.0722 * o[2];
    if lum > 0.55 {
        for k in o.iter_mut() {
            *k *= 0.55 / lum;
        }
    }
    o
}

/// Two dominant colours of the artwork: a hue histogram weighted by saturation/brightness (and slightly by
/// being low in the image, where the bleed happens). Near-black pixels and greys are ignored unless the whole
/// artwork is grey. Returns None if the artwork has nothing usable (e.g. fully black / transparent).
fn extract_palette(rgba: &[u8]) -> Option<Pal> {
    let n = (ART * ART) as usize;
    if rgba.len() < n * 4 {
        return None;
    }
    let mut wsum = [0f32; HUE_BINS];
    let mut csum = [[0f32; 3]; HUE_BINS];
    let (mut gw, mut gsum) = (0f32, [0f32; 3]);
    for (i, px) in rgba.chunks_exact(4).take(n).enumerate() {
        if px[3] < 128 {
            continue;
        }
        let c = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0];
        let (h, s, v) = rgb_to_hsv(c);
        if v < 0.10 {
            continue;
        }
        let y = (i / ART as usize) as f32 / ART as f32;
        let w = (0.4 + 0.6 * v) * (1.0 + 0.5 * y);
        gw += w;
        for k in 0..3 {
            gsum[k] += c[k] * w;
        }
        if s < 0.15 {
            continue;
        }
        let wc = w * (0.15 + s);
        let b = ((h / 360.0 * HUE_BINS as f32) as usize).min(HUE_BINS - 1);
        wsum[b] += wc;
        for k in 0..3 {
            csum[b][k] += c[k] * wc;
        }
    }
    if gw <= 0.0 {
        return None;
    }
    let tot: f32 = wsum.iter().sum();
    if tot < 0.05 * gw {
        // (nearly) greyscale artwork
        let avg = fit_tint([gsum[0] / gw, gsum[1] / gw, gsum[2] / gw]);
        return Some([avg, avg]);
    }
    let score = |b: usize| wsum[b] + 0.5 * (wsum[(b + 1) % HUE_BINS] + wsum[(b + HUE_BINS - 1) % HUE_BINS]);
    let colour = |b: usize| {
        let (mut w, mut c) = (0f32, [0f32; 3]);
        for d in [HUE_BINS - 1, 0, 1] {
            let j = (b + d) % HUE_BINS;
            w += wsum[j];
            for k in 0..3 {
                c[k] += csum[j][k];
            }
        }
        if w <= 0.0 {
            [0.0; 3]
        } else {
            [c[0] / w, c[1] / w, c[2] / w]
        }
    };
    let b1 = (0..HUE_BINS).max_by(|&a, &b| score(a).total_cmp(&score(b)))?;
    let s1 = score(b1);
    let dist = |a: usize, b: usize| {
        let d = (a as i32 - b as i32).unsigned_abs() as usize;
        d.min(HUE_BINS - d)
    };
    // second colour: strongest hue at least 60 degrees away, if it carries a meaningful share
    let b2 = (0..HUE_BINS).filter(|&b| dist(b, b1) >= 4).max_by(|&a, &b| score(a).total_cmp(&score(b)));
    let c1 = colour(b1);
    let c2 = match b2 {
        Some(b) if score(b) >= 0.25 * s1 => colour(b),
        _ => c1,
    };
    Some([fit_tint(c1), fit_tint(c2)])
}

impl App {
    pub(crate) fn set_art(&mut self, rgba: &[u8]) {
        self.art = pixmap_from_rgba(rgba, ART);
        if self.art.is_none() {
            return; // music_glow fades the tint out
        }
        // Extract colours only when the artwork really changed.
        let sig = sig_of(rgba);
        if sig == self.tint.sig {
            return;
        }
        self.art_t0 = Instant::now();
        self.tint.sig = sig;
        self.tint.retarget(extract_palette(rgba));
    }

    /// Artwork colour bleed for the Music slide. Drawn under the slide content (like `glow` for the other slides),
    /// clipped to the notch shape: strongest at the bottom, fading to fully transparent towards the top, with
    /// the dominant colour sitting under the artwork and the second one drifting to the right.
    pub(crate) fn music_glow(&mut self, g: &mut Gfx, shape: &Path, w: f32, h: f32, al: f32) {
        if self.art.is_none() && self.tint.to_a > 0.0 {
            // Artwork went away (new track, nothing yet / failed): fade out instead of snapping.
            self.tint.retarget(None);
            self.tint.sig = 0;
        }
        let (pal, amt) = self.tint.eval();
        let opacity = amt * al;
        if opacity <= 0.01 || w < 1.0 || h < 1.0 {
            return;
        }
        let l = CX - w / 2.0;
        let ax = ((LX + 44.0 - l) / w).clamp(0.0, 1.0); // artwork centre, as a fraction of the notch width
        let Some(mut pm) = self.tint.field.take().or_else(|| Pixmap::new(TF_W, TF_H)) else { return; };
        let data = pm.data_mut();
        for j in 0..TF_H {
            let v = (j as f32 + 0.5) / TF_H as f32;
            let f = smoothstep(0.2, 0.85, v); // 0 at the top ... full from ~85% down
            for i in 0..TF_W {
                let u = (i as f32 + 0.5) / TF_W as f32;
                let d = (u - ax) / 0.38;
                let gx = 0.55 + 0.45 * (-d * d).exp(); // a bit stronger under the artwork than behind the controls
                let m = smoothstep(ax - 0.1, ax + 0.7, u); // 0 = dominant colour, 1 = secondary
                let a = TINT_PEAK * f * gx;
                let o = ((j * TF_W + i) * 4) as usize;
                for k in 0..3 {
                    let c = pal[0][k] + (pal[1][k] - pal[0][k]) * m;
                    data[o + k] = (c * a * 255.0 + 0.5) as u8; // premultiplied
                }
                data[o + 3] = (a * 255.0 + 0.5) as u8;
            }
        }
        let mut p = Paint::default();
        p.shader = Pattern::new(
            pm.as_ref(),
            SpreadMode::Pad,
            FilterQuality::Bilinear,
            opacity.clamp(0.0, 1.0),
            Transform::from_row(w / TF_W as f32, 0.0, 0.0, h / TF_H as f32, l, 0.0),
        );
        p.anti_alias = true;
        g.fill(shape, &p);
        drop(p);
        self.tint.field = Some(pm);
    }

    /// Cover bulat-membulat pada (x, y) dengan ukuran `size`.
    fn draw_cover(g: &mut Gfx, art: &Pixmap, x: f32, y: f32, size: f32, r: f32, al: f32) {
        let s = size / ART as f32;
        let mut p = Paint::default();
        p.shader = Pattern::new(
            art.as_ref(),
            SpreadMode::Pad,
            FilterQuality::Bilinear,
            al,
            Transform::from_row(s, 0.0, 0.0, s, x, y),
        );
        p.anti_alias = true;
        g.fill(&rrect(x, y, size, size, r), &p);
    }

    /// Detik sejak cover terbaru datang, selama transisi (None bila sudah selesai atau tidak ada cover).
    fn art_anim_t(&self) -> Option<f32> {
        let t = self.art_t0.elapsed().as_secs_f32();
        (self.art.is_some() && t < ART_ANIM_SECS).then_some(t)
    }

    pub(crate) fn art_fading(&self) -> bool {
        self.media.playing && self.art_anim_t().is_some()
    }

    /// Ikon kiri notch kecil saat lagu main: cover album (ikon aplikasi bila cover belum ada).
    /// Cover baru masuk dengan pegas (membesar dari 85% dengan sedikit overshoot) sambil cover lama memudar.
    pub(crate) fn collapsed_cover(&self, g: &mut Gfx, x: f32, y: f32, size: f32, r: f32, al: f32) {
        let Some(art) = &self.art else {
            self.icon_at(g, x, y, size, r, al);
            return;
        };
        let Some(t) = self.art_anim_t() else {
            Self::draw_cover(g, art, x, y, size, r, al);
            return;
        };
        let k = t / ART_ANIM_SECS;
        let out = 1.0 - smoothstep(0.0, 0.6, k);
        match &self.prev_art {
            Some(prev) => Self::draw_cover(g, prev, x, y, size, r, al * out),
            None => self.icon_at(g, x, y, size, r, al * out),
        }
        let spring = 1.0 - (-6.0 * t).exp() * (10.0 * t).cos();
        let sc = 0.85 + 0.15 * spring;
        let sz = size * sc;
        Self::draw_cover(g, art, x + (size - sz) / 2.0, y + (size - sz) / 2.0, sz, r * sc, al * smoothstep(0.0, 0.5, k));
    }

    pub(crate) fn art_at(&self, g: &mut Gfx, x: f32, y: f32, size: f32, r: f32, al: f32) {
        let path = rrect(x, y, size, size, r);
        if let Some(art) = &self.art {
            Self::draw_cover(g, art, x, y, size, r, al);
        } else {
            g.fill(&path, &paint(46, 46, 52, al));
            let (cx, cy, s) = (x + size / 2.0, y + size / 2.0, size / 88.0);
            let n = paint(150, 150, 160, al);
            g.circle(cx - 5.0 * s, cy + 12.0 * s, 7.0 * s, &n);
            g.rect(cx + 1.0 * s, cy - 18.0 * s, 3.5 * s, 30.0 * s, &n);
            g.rect(cx + 1.0 * s, cy - 18.0 * s, 13.0 * s, 6.0 * s, &n);
        }
    }

    /// Titik + teks status koneksi (kiri bawah slide Music). Hijau saat tersambung, kuning/merah saat bermasalah.
    fn net_indicator(&self, g: &mut Gfx, x: f32, y: f32, al: f32) {
        let st = net::state();
        let label = net::label(st);
        if label.is_empty() {
            return;
        }
        let (rgb, text) = match st {
            net::CONNECTED => ((60, 200, 110), TEXT_FAINT),
            net::NO_INTERNET => ((255, 190, 60), [255, 205, 120]),
            _ => ((255, 69, 58), [255, 130, 120]),
        };
        g.circle(x + 3.0, y + 7.5, 3.0, &paint(rgb.0, rgb.1, rgb.2, al));
        g.txt(label, x + 12.0, y, 11.0, text, al, 140.0);
    }

    /// 0..1: seberapa terlihat indikator volume (muncul cepat, memudar di akhir); 0 bila tidak aktif.
    pub(crate) fn vol_overlay_alpha(&self) -> f32 {
        let e = self.vol_t.elapsed().as_secs_f32();
        if e >= 1.5 {
            return 0.0;
        }
        (e / 0.1).min(1.0) * (1.0 - ((e - 1.2) / 0.3).clamp(0.0, 1.0))
    }

    pub(crate) fn vol_overlay_active(&self) -> bool {
        self.expanded() && self.is_slide(SlideId::Music) && self.vol_t.elapsed().min(self.out_t.elapsed()).as_secs_f32() < 1.5
    }

    /// Indikator volume yang menimpa baris kontrol: ikon speaker, bar, persen.
    fn draw_vol_overlay(&self, g: &mut Gfx, ox: f32, al: f32) {
        let a = al * self.vol_overlay_alpha();
        if a <= 0.01 {
            return;
        }
        let (pct, muted) = self.vol_state;
        let (x, y, w, h) = output::OVERLAY;
        let x = x + ox;
        g.fill(&rrect(x, y, w, h, 14.0), &paint(8, 10, 18, a));
        let level = if muted || pct == 0 { 0 } else if pct < 34 { 1 } else if pct < 67 { 2 } else { 3 };
        g.icon_speaker(x + 20.0, y + h / 2.0, 16.0, level, muted, &paint(255, 255, 255, a));
        let (bx, by, bw) = (x + 40.0, y + h / 2.0 - 3.0, w - 40.0 - 50.0);
        g.fill(&rrect(bx, by, bw, 6.0, 3.0), &paint(255, 255, 255, 0.18 * a));
        let fw = (bw * pct as f32 / 100.0).max(if pct > 0 { 6.0 } else { 0.0 });
        if fw > 0.0 {
            let c = if muted { TEXT_DIM } else { g.acc.main };
            g.fill(&rrect(bx, by, fw, 6.0, 3.0), &paint(c[0], c[1], c[2], a));
        }
        let txt = if muted { tr("Mute").to_string() } else { format!("{pct}%") };
        let tw = g.width_ex(&txt, 12.0, true);
        g.txtb(&txt, x + w - 12.0 - tw, y + h / 2.0 - 7.0, 12.0, [255, 255, 255], a, 48.0);
    }

    pub(crate) fn slide_music(&self, g: &mut Gfx, ox: f32, al: f32) {
        if self.lyric_mode {
            self.slide_music_lyrics(g, ox, al);
            return;
        }
        match (&self.lyrics, self.lyric_text()) {
            (_, Some(line)) => {
                let a = al * self.lyric_alpha();
                let line = if g.width_ex(line, 13.0, true) > CW { ellipsize(g, line, 13.0, CW) } else { line.to_string() };
                g.txt_c(&line, (LX + RX) / 2.0 + ox, 13.0, 13.0, [255, 255, 255], a, CW + 2.0, true);
            }
            (lyrics::Lyrics::Loading, None) => g.txt(tr("Loading lyrics"), LX + ox, 14.0, 12.0, [150, 152, 160], al, 200.0),
            (lyrics::Lyrics::Unavailable, None) => g.txt(tr("Lyrics not available"), LX + ox, 14.0, 12.0, [150, 152, 160], al, 200.0),
            _ => {
                if self.media.sources > 1 && !self.media.source.is_empty() {
                    let label = fill(tr("Now Playing \u{b7} {}"), &[&self.media.source]);
                    g.txt(&label, LX + ox, 14.0, 12.0, TEXT_DIM, al, 180.0);
                    // ikon tukar sumber (dua panah) setelah teks
                    let sx = LX + ox + g.width(&label, 12.0) + 10.0;
                    let c = paint(150, 150, 158, al);
                    g.line(sx - 4.0, 18.0, sx + 4.0, 18.0, 1.3, &c);
                    g.line(sx + 1.5, 15.5, sx + 4.0, 18.0, 1.3, &c);
                    g.line(sx - 4.0, 23.0, sx + 4.0, 23.0, 1.3, &c);
                    g.line(sx - 4.0, 23.0, sx - 1.5, 20.5, 1.3, &c);
                } else {
                    g.txt(tr("Now Playing"), LX + ox, 14.0, 12.0, TEXT_DIM, al, 200.0);
                }
            }
        }
        self.art_at(g, LX + ox, 44.0, 88.0, 12.0, al);
        let title = if self.media.title.is_empty() { tr("No media") } else { &self.media.title };
        g.txtb(title, MX + ox, 40.0, 17.0, [255, 255, 255], al, MW);
        g.txt(&self.media.artist, MX + ox, 64.0, 13.0, [170, 170, 175], al, MW);
        if self.media.dur > 0 {
            let fr = (self.media.pos as f32 / self.media.dur as f32).clamp(0.0, 1.0);
            g.fill(&rrect(MX + ox, 96.0, MW, 4.0, 2.0), &paint(255, 255, 255, 0.18 * al));
            g.fill(&rrect(MX + ox, 96.0, (MW * fr).max(4.0), 4.0, 2.0), &g.sacc(al));
            g.txt(&fmt_time(self.media.pos), MX + ox, 104.0, 11.0, TEXT_DIM, al, 60.0);
            let ds = fmt_time(self.media.dur);
            let dw = g.width(&ds, 11.0);
            g.txt(&ds, MX + ox + MW - dw, 104.0, 11.0, TEXT_DIM, al, 60.0);
        }
        self.net_indicator(g, LX + ox, 148.0, al);
        let cx = MCX + ox;
        let cy = MCY;
        let has = matches!(self.lyrics, lyrics::Lyrics::Synced(..));
        if !self.out_list_shown() {
            // the device list reaches up to here
            g.icon_bubble(cx + 92.0, cy, 15.0, &paint(255, 255, 255, if has { 0.9 * al } else { 0.4 * al }), &paint(12, 12, 16, al));
        }
        let white = paint(255, 255, 255, al);
        let px = cx - 56.0;
        g.poly(&[(px + 6.0, cy - 7.0), (px + 6.0, cy + 7.0), (px - 6.0, cy)], &white);
        g.rect(px - 9.0, cy - 7.0, 2.5, 14.0, &white);
        let nx = cx + 56.0;
        g.poly(&[(nx - 6.0, cy - 7.0), (nx - 6.0, cy + 7.0), (nx + 6.0, cy)], &white);
        g.rect(nx + 6.5, cy - 7.0, 2.5, 14.0, &white);
        g.circle(cx, cy, 17.0, &white);
        let blk = paint(0, 0, 0, al);
        if self.media.playing {
            g.rect(cx - 6.0, cy - 7.0, 4.0, 14.0, &blk);
            g.rect(cx + 2.0, cy - 7.0, 4.0, 14.0, &blk);
        } else {
            g.poly(&[(cx - 4.0, cy - 8.0), (cx - 4.0, cy + 8.0), (cx + 8.0, cy)], &blk);
        }
        self.draw_out_button(g, ox, al);
        // the newer of the two overlays (volume, or the output device after a switch)
        if self.out_t > self.vol_t {
            self.draw_out_overlay(g, ox, al);
        } else {
            self.draw_vol_overlay(g, ox, al);
        }
        self.draw_out_list(g, ox, al);
    }
}

#[cfg(test)]
mod tests {
    use super::clean_title;

    #[test]
    fn clean_title_strips_suffixes() {
        assert_eq!(clean_title("Here We Go Again /"), "Here We Go Again");
        assert_eq!(clean_title("Song - Remastered 2011"), "Song");
        assert_eq!(clean_title("Song (feat. X)"), "Song");
        assert_eq!(clean_title("Plain Title"), "Plain Title");
        assert_eq!(clean_title("/"), "/");
    }
}

#[cfg(test)]
mod source_tests {
    use super::friendly_source;

    #[test]
    fn friendly_source_names() {
        assert_eq!(friendly_source("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"), "Spotify");
        assert_eq!(friendly_source("chrome.exe"), "Chrome");
        assert_eq!(friendly_source("Chrome"), "Chrome");
        assert_eq!(friendly_source("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "ZuneMusic");
        assert_eq!(friendly_source(""), "");
    }
}

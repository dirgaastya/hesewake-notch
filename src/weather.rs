//! Cuaca di kiri jam (notch idle, tanpa lagu): suhu dan ikon kondisi bergantian dengan transisi geser + fade.
//! Data dari Open-Meteo (tanpa API key), diperbarui tiap 30 menit di thread latar belakang dan di-cache.

use super::*;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;

/// Pesan ke window: data cuaca baru tersedia.
pub(crate) const WM_WEATHER: u32 = WM_APP + 10;

const REFRESH: Duration = Duration::from_secs(30 * 60);
const RETRY: Duration = Duration::from_secs(5 * 60);
/// Lama tiap tahap (suhu / ikon) dan transisi di antaranya.
const STAGE_SECS: f32 = 4.0;
const TRANS_SECS: f32 = 0.38;
/// Frame interval while a moving weather icon shows (rain, drizzle, storm, snow, the hot sun). The motions are small
/// and slow, so 20 fps looks the same as 60.
pub(crate) const ICON_MS: u32 = 50;

/// Timer period the weather slot needs: 16 ms during the short change between temperature and icon, `ICON_MS` while a
/// moving icon shows, and nothing otherwise. A moving icon holds still while nobody is at the computer (no input for
/// `eye::EYE_STILL_AFTER_SECS`), like the idle eye.
pub(crate) fn slot_frame_ms(transition: bool, icon_moving: bool, input_idle_secs: f32) -> u32 {
    if transition {
        16
    } else if icon_moving && input_idle_secs < eye::EYE_STILL_AFTER_SECS {
        ICON_MS
    } else {
        0
    }
}
const HOT_C: f32 = 32.0;

/// Kota bawaan untuk dipilih di Settings (nama, lintang, bujur). Tambah kota baru di sini (urutan lama dipertahankan agar indeks tersimpan tetap valid).
pub(crate) const CITIES: &[(&str, f32, f32)] = &[
    ("Jakarta", -6.2088, 106.8456),
    ("Bandung", -6.9175, 107.6191),
    ("Surabaya", -7.2575, 112.7521),
    ("Yogyakarta", -7.7956, 110.3695),
    ("Semarang", -6.9667, 110.4167),
    ("Medan", 3.5952, 98.6722),
    ("Makassar", -5.1477, 119.4327),
    ("Denpasar", -8.6705, 115.2126),
    ("Palembang", -2.9761, 104.7754),
    ("Malang", -7.9797, 112.6304),
    ("Bogor", -6.5950, 106.8166),
    ("Depok", -6.4025, 106.7942),
    ("Bekasi", -6.2383, 106.9756),
    ("Tangerang", -6.1783, 106.6319),
    ("Balikpapan", -1.2379, 116.8529),
    ("Pontianak", -0.0263, 109.3425),
    ("Karawang", -6.3215, 107.3376),
    ("Cikarang", -6.2615, 107.1527),
    ("Purwakarta", -6.5569, 107.4431),
    ("Subang", -6.5716, 107.7586),
    ("Cirebon", -6.7063, 108.5570),
    ("Sukabumi", -6.9277, 106.9300),
    ("Tasikmalaya", -7.3274, 108.2207),
    ("Serang", -6.1201, 106.1503),
    ("Cilegon", -6.0025, 106.0111),
    ("Surakarta", -7.5666, 110.8168),
    ("Bandar Lampung", -5.4292, 105.2610),
    ("Pekanbaru", 0.5071, 101.4478),
    ("Batam", 1.0456, 104.0305),
    ("Padang", -0.9471, 100.4172),
    ("Banda Aceh", 5.5483, 95.3238),
    ("Banjarmasin", -3.3194, 114.5908),
    ("Manado", 1.4748, 124.8421),
    ("Mataram", -8.5833, 116.1167),
    ("Kupang", -10.1772, 123.6070),
    ("Jayapura", -2.5337, 140.7181),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Weather {
    pub(crate) temp: f32,
    pub(crate) code: u8,
    pub(crate) day: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Cond {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Storm,
}

/// Kode cuaca WMO -> kondisi.
pub(crate) fn cond_of(code: u8) -> Cond {
    match code {
        0 | 1 => Cond::Clear,
        2 => Cond::PartlyCloudy,
        3 => Cond::Cloudy,
        45 | 48 => Cond::Fog,
        51..=57 => Cond::Drizzle,
        61..=67 | 80..=82 => Cond::Rain,
        71..=77 | 85 | 86 => Cond::Snow,
        95..=99 => Cond::Storm,
        _ => Cond::Cloudy,
    }
}

impl Weather {
    /// Cerah/berawan tipis dan suhu >= 32 C: tampilkan varian "panas".
    pub(crate) fn hot(&self) -> bool {
        self.temp >= HOT_C && matches!(cond_of(self.code), Cond::Clear | Cond::PartlyCloudy) && self.day
    }
}

/// Parse respons Open-Meteo (`current.temperature_2m`, `current.weather_code`, `current.is_day`).
pub(crate) fn parse(json: &str) -> Option<Weather> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let c = v.get("current")?;
    let temp = c.get("temperature_2m")?.as_f64()? as f32;
    let code = c.get("weather_code")?.as_u64()?.min(255) as u8;
    let day = c.get("is_day").and_then(|d| d.as_u64()).unwrap_or(1) == 1;
    Some(Weather { temp, code, day })
}

// ---------------------------------------------------------------- thread data
static CURRENT: Mutex<Option<Weather>> = Mutex::new(None);
static DEMO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static T0: OnceLock<Instant> = OnceLock::new();

fn secs() -> f32 {
    T0.get_or_init(Instant::now).elapsed().as_secs_f32()
}
static CMD: OnceLock<Mutex<Sender<Cmd>>> = OnceLock::new();

enum Cmd {
    /// Lokasi / status aktif berubah: ambil ulang sekarang.
    Refresh { on: bool, lat: f32, lon: f32 },
}

pub(crate) fn current() -> Option<Weather> {
    *CURRENT.lock().unwrap_or_else(|p| p.into_inner())
}

/// Ubah pengaturan (nyala/mati, lokasi); thread latar belakang mengambil data baru.
pub(crate) fn configure(on: bool, lat: f32, lon: f32) {
    if let Some(tx) = CMD.get() {
        let _ = tx.lock().unwrap_or_else(|p| p.into_inner()).send(Cmd::Refresh { on, lat, lon });
    }
}

pub(crate) fn start(hwnd: isize, on: bool, lat: f32, lon: f32) {
    let (tx, rx) = channel();
    if CMD.set(Mutex::new(tx)).is_err() {
        return;
    }
    load_cache(lat, lon);
    // `--demo-weather=kode,suhu,siang`: paksa data (uji visual); thread jaringan tidak menimpanya
    if let Some(a) = std::env::args().find_map(|a| a.strip_prefix("--demo-weather=").map(str::to_string)) {
        let p: Vec<&str> = a.split(',').collect();
        if let (Some(Ok(code)), Some(Ok(temp))) = (p.first().map(|s| s.parse::<u8>()), p.get(1).map(|s| s.parse::<f32>())) {
            *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Weather { temp, code, day: p.get(2).map_or(true, |s| *s != "0") });
            DEMO.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    std::thread::spawn(move || run(hwnd, rx, on, lat, lon));
}

fn run(hwnd: isize, rx: Receiver<Cmd>, mut on: bool, mut lat: f32, mut lon: f32) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    loop {
        let mut wait = REFRESH;
        if on && !DEMO.load(std::sync::atomic::Ordering::Relaxed) {
            let url = format!(
                "https://api.open-meteo.com/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=temperature_2m,weather_code,is_day&timezone=auto"
            );
            match http_get(&url, 200_000).and_then(|b| parse(&b)) {
                Some(w) => {
                    *CURRENT.lock().unwrap_or_else(|p| p.into_inner()) = Some(w);
                    save_cache(w, lat, lon);
                    unsafe {
                        let _ = PostMessageW(HWND(hwnd as *mut c_void), WM_WEATHER, WPARAM(0), LPARAM(0));
                    }
                }
                None => {
                    perf::note("weather: ambil data gagal");
                    wait = RETRY;
                }
            }
        }
        // Tunggu jadwal berikutnya, atau perubahan pengaturan.
        match rx.recv_timeout(wait) {
            Ok(Cmd::Refresh { on: o, lat: la, lon: lo }) => {
                on = o;
                lat = la;
                lon = lo;
                if !on {
                    *CURRENT.lock().unwrap_or_else(|p| p.into_inner()) = None;
                    unsafe {
                        let _ = PostMessageW(HWND(hwnd as *mut c_void), WM_WEATHER, WPARAM(0), LPARAM(0));
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return,
        }
    }
}

/// GET lewat WinINet (TLS bawaan sistem). WinRT HttpClient gagal handshake ke api.open-meteo.com (0x80072F8F).
/// GET `url` as text; None on failure or when the body is bigger than `max` bytes.
pub(crate) fn http_get(url: &str, max: usize) -> Option<String> {
    use windows::Win32::Networking::WinInet::*;
    unsafe {
        let ses = InternetOpenW(w!("HesewakeNotch"), INTERNET_OPEN_TYPE_PRECONFIG.0, None, None, 0);
        if ses.is_null() {
            return None;
        }
        let ms: u32 = 15_000;
        for opt in [INTERNET_OPTION_RECEIVE_TIMEOUT, INTERNET_OPTION_CONNECT_TIMEOUT] {
            let _ = InternetSetOptionW(Some(ses), opt, Some(&ms as *const u32 as *const c_void), 4);
        }
        let wurl: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
        let f = InternetOpenUrlW(ses, PCWSTR(wurl.as_ptr()), None, INTERNET_FLAG_RELOAD | INTERNET_FLAG_NO_COOKIES | INTERNET_FLAG_SECURE, 0);
        let mut out = Vec::new();
        if !f.is_null() {
            let mut buf = [0u8; 4096];
            loop {
                let mut n = 0u32;
                if InternetReadFile(f, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut n).is_err() || n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n as usize]);
                if out.len() > max {
                    out.clear();
                    break;
                }
            }
            let _ = InternetCloseHandle(f);
        } else {
            perf::note("weather: WinINet gagal membuka URL");
        }
        let _ = InternetCloseHandle(ses);
        Some(String::from_utf8_lossy(&out).into_owned()).filter(|s| !s.is_empty())
    }
}

// ---------------------------------------------------------------- cache (tampil langsung saat start)
fn save_cache(w: Weather, lat: f32, lon: f32) {
    let at = notif::now_unix();
    write_data("weather.txt", format!("{}|{}|{}|{:.4}|{:.4}|{}\n", w.temp, w.code, w.day as u8, lat, lon, at));
}

fn load_cache(lat: f32, lon: f32) {
    let Ok(s) = std::fs::read_to_string(data_path("weather.txt")) else { return };
    if let Some(w) = parse_cache(&s, lat, lon, notif::now_unix()) {
        *CURRENT.lock().unwrap_or_else(|p| p.into_inner()) = Some(w);
    }
}

/// Cache berlaku bila lokasi sama (toleransi 0,01 derajat) dan umurnya < 3 jam.
pub(crate) fn parse_cache(s: &str, lat: f32, lon: f32, now: u64) -> Option<Weather> {
    let p: Vec<&str> = s.trim().split('|').collect();
    if p.len() < 6 {
        return None;
    }
    let (clat, clon): (f32, f32) = (p[3].parse().ok()?, p[4].parse().ok()?);
    let at: u64 = p[5].parse().ok()?;
    if (clat - lat).abs() > 0.01 || (clon - lon).abs() > 0.01 || now.saturating_sub(at) > 3 * 3600 {
        return None;
    }
    Some(Weather { temp: p[0].parse().ok()?, code: p[1].parse().ok()?, day: p[2] == "1" })
}

// ---------------------------------------------------------------- tahap bergantian
/// (indeks tahap global, tahap 0 = suhu / 1 = ikon, progres transisi 0..1; 1 = transisi selesai).
pub(crate) fn stage_at(secs: f32) -> (u64, u8, f32) {
    let (idx, st, prog) = stage_of(secs, 2);
    (idx, st as u8, prog)
}

/// Like `stage_at` for `n` stages. A single stage stays still, with no transition.
pub(crate) fn stage_of(secs: f32, n: usize) -> (u64, usize, f32) {
    let idx = (secs / STAGE_SECS) as u64;
    if n <= 1 {
        return (idx, 0, 1.0);
    }
    let since = secs - idx as f32 * STAGE_SECS;
    (idx, (idx % n as u64) as usize, (since / TRANS_SECS).clamp(0.0, 1.0))
}

/// Contents of the slot left of the clock: temperature, weather icon, and (15 minutes before a prayer) the countdown.
#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Temp,
    Cond,
    Prayer,
}

fn ss(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl App {
    /// Stages that alternate in the slot left of the clock.
    fn slots(&self) -> ([Slot; 3], usize) {
        let mut v = [Slot::Temp; 3];
        let mut n = 0;
        if self.cfg.weather_on && current().is_some() {
            v[0] = Slot::Temp;
            v[1] = Slot::Cond;
            n = 2;
        }
        if self.countdown_on() {
            v[n] = Slot::Prayer;
            n += 1;
        }
        (v, n)
    }

    /// The slot left of the clock is showing: idle, clock phase, no music/toast, and it has content (weather / countdown).
    pub(crate) fn weather_shown(&self) -> bool {
        !self.expanded()
            && !self.media.playing
            && !self.thin()
            && self.toast.is_none()
            && self.splash_t().is_none()
            && self.idle_phase() == IdlePhase::Clock
            && self.slots().1 > 0
    }

    /// Indeks tahap saat ini (untuk memicu redraw tiap pergantian).
    pub(crate) fn weather_stage_idx(&self) -> u64 {
        stage_at(secs()).0
    }

    /// Butuh frame: saat transisi, atau saat tahap ikon yang bergerak.
    /// Timer period for the weather slot now (0 = no frames needed); see `slot_frame_ms`.
    pub(crate) fn weather_frame_ms(&self, input_idle_secs: f32) -> u32 {
        if !self.weather_shown() {
            return 0;
        }
        let (v, n) = self.slots();
        let (_, stage, prog) = stage_of(secs(), n);
        slot_frame_ms(n > 1 && prog < 1.0, v[stage] == Slot::Cond && current().map_or(false, animated), input_idle_secs)
    }

    pub(crate) fn draw_weather(&self, g: &mut Gfx, w: f32, h: f32, ca: f32) {
        if !self.weather_shown() {
            return;
        }
        let wx = current();
        let label = self.countdown_text(g);
        // ikut memudar bersama jam saat fase mata selesai
        let fade = ((self.idle_secs() % IDLE_CYCLE_SECS) / CLOCK_FADE_IN).clamp(0.0, 1.0);
        let ca = ca * fade;
        let t = secs();
        let (v, n) = self.slots();
        let (_, stage, prog) = stage_of(t, n);
        let (cx, cy) = (self.idle_weather_cx(w), h / 2.0);
        let draw_stage = |g: &mut Gfx, st: Slot, a: f32, dy: f32| {
            if a <= 0.01 {
                return;
            }
            match (st, wx, label.as_ref()) {
                (Slot::Temp, Some(wx), _) => {
                    let txt = format!("{:.0}\u{b0}", wx.temp);
                    let col = if wx.hot() { [255, 150, 70] } else { [255, 255, 255] };
                    g.txt_c(&txt, cx, ty(h, 13.0) + dy, 13.0, col, a, 40.0, true);
                }
                (Slot::Cond, Some(wx), _) => draw_cond(g, wx, t, cx, cy + dy, 15.0, a),
                (Slot::Prayer, _, Some((lbl, col))) => g.txt_c(lbl, cx, ty(h, 12.0) + dy, 12.0, *col, a, 80.0, false),
                _ => {}
            }
        };
        // transisi: tahap lama naik + memudar, tahap baru naik dari bawah
        let e = ss(0.0, 1.0, prog);
        if n > 1 {
            draw_stage(g, v[(stage + n - 1) % n], ca * (1.0 - e), -7.0 * e);
        }
        draw_stage(g, v[stage], ca * e, 7.0 * (1.0 - e));
    }
}

fn animated(w: Weather) -> bool {
    matches!(cond_of(w.code), Cond::Drizzle | Cond::Rain | Cond::Storm | Cond::Snow) || w.hot()
}

// ---------------------------------------------------------------- ikon vektor
fn cloud_path(cx: f32, cy: f32, s: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx - 0.24 * s, cy + 0.06 * s, 0.2 * s);
    pb.push_circle(cx + 0.0 * s, cy - 0.1 * s, 0.3 * s);
    pb.push_circle(cx + 0.26 * s, cy + 0.04 * s, 0.22 * s);
    pb.push_rect(Rect::from_xywh(cx - 0.24 * s, cy + 0.06 * s, 0.5 * s, 0.2 * s)?);
    pb.finish()
}

/// Ikon kondisi bergaya satu warna-aksen, terpusat di (cx, cy), tinggi ±`s` px.
fn draw_cond(g: &mut Gfx, w: Weather, t: f32, cx: f32, cy: f32, s: f32, a: f32) {
    let cond = cond_of(w.code);
    let cloud_col = [205, 214, 230];
    let cloud = |g: &mut Gfx, x: f32, y: f32, sc: f32, col: [u8; 3]| {
        if let Some(p) = cloud_path(x, y, sc) {
            g.fill(&p, &paint(col[0], col[1], col[2], a));
        }
    };
    let drops = |g: &mut Gfx, n: usize, col: [u8; 3], len: f32| {
        for i in 0..n {
            let ph = ((t * 1.25) + i as f32 * 0.33).rem_euclid(1.0);
            let x = cx + (i as f32 - (n as f32 - 1.0) / 2.0) * s * 0.3 - ph * s * 0.06;
            let y0 = cy + s * 0.3 + ph * s * 0.34;
            let al = a * (1.0 - ph).clamp(0.0, 1.0) * 1.2;
            g.line(x, y0, x - s * 0.05, y0 + len, 1.4, &paint(col[0], col[1], col[2], al.min(a)));
        }
    };
    match cond {
        Cond::Clear => {
            if !w.day {
                // bulan sabit: lingkaran terang dikurangi lingkaran gelap (latar notch hitam)
                g.circle(cx, cy, s * 0.36, &paint(225, 230, 255, a));
                g.circle(cx + s * 0.18, cy - s * 0.1, s * 0.32, &paint(0, 0, 0, a));
            } else if w.hot() {
                let pulse = 0.5 + 0.5 * (t * 3.0).sin();
                g.circle(cx, cy, s * (0.46 + 0.05 * pulse), &paint(255, 120, 40, a * (0.16 + 0.12 * pulse)));
                g.icon_sun(cx, cy, s * 1.15, &paint(255, 140, 50, a));
            } else {
                g.icon_sun(cx, cy, s * 1.1, &paint(255, 204, 64, a));
            }
        }
        Cond::PartlyCloudy => {
            if w.day {
                let col = if w.hot() { (255, 140, 50) } else { (255, 204, 64) };
                g.icon_sun(cx - s * 0.2, cy - s * 0.18, s * 0.8, &paint(col.0, col.1, col.2, a));
            } else {
                g.circle(cx - s * 0.2, cy - s * 0.18, s * 0.26, &paint(225, 230, 255, a));
                g.circle(cx - s * 0.08, cy - s * 0.26, s * 0.23, &paint(0, 0, 0, a));
            }
            cloud(g, cx + s * 0.1, cy + s * 0.12, s * 0.95, cloud_col);
        }
        Cond::Cloudy => cloud(g, cx, cy, s * 1.15, cloud_col),
        Cond::Fog => {
            for (i, wd) in [0.8f32, 0.62, 0.78].iter().enumerate() {
                let y = cy - s * 0.24 + i as f32 * s * 0.24;
                g.line(cx - wd * s * 0.5, y, cx + wd * s * 0.5, y, 1.8, &paint(190, 198, 215, a * 0.9));
            }
        }
        Cond::Drizzle => {
            cloud(g, cx, cy - s * 0.12, s, cloud_col);
            drops(g, 2, [120, 190, 255], s * 0.14);
        }
        Cond::Rain => {
            cloud(g, cx, cy - s * 0.14, s, [170, 184, 208]);
            drops(g, 3, [90, 170, 255], s * 0.2);
        }
        Cond::Snow => {
            cloud(g, cx, cy - s * 0.14, s, cloud_col);
            for i in 0..3 {
                let ph = ((t * 0.7) + i as f32 * 0.33).rem_euclid(1.0);
                let x = cx + (i as f32 - 1.0) * s * 0.3 + (ph * 6.0).sin() * s * 0.04;
                g.circle(x, cy + s * 0.32 + ph * s * 0.3, 1.3, &paint(255, 255, 255, a * (1.0 - ph)));
            }
        }
        Cond::Storm => {
            cloud(g, cx, cy - s * 0.16, s, [140, 150, 175]);
            let flash = if (t * 1.5).rem_euclid(2.0) < 0.35 { 1.0 } else { 0.75 };
            g.icon_bolt(cx + s * 0.02, cy + s * 0.3, s * 0.55, &paint(255, 220, 60, a * flash));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_meteo() {
        let j = r#"{"latitude":-6.9,"current":{"time":"2026-10-25T00:00","interval":900,"temperature_2m":22.7,"weather_code":51,"is_day":0}}"#;
        assert_eq!(parse(j), Some(Weather { temp: 22.7, code: 51, day: false }));
        assert_eq!(parse("{}"), None);
        assert_eq!(parse("bukan json"), None);
        // is_day hilang -> dianggap siang
        assert_eq!(parse(r#"{"current":{"temperature_2m":30,"weather_code":0}}"#).map(|w| w.day), Some(true));
    }

    #[test]
    fn maps_wmo_codes() {
        assert_eq!(cond_of(0), Cond::Clear);
        assert_eq!(cond_of(2), Cond::PartlyCloudy);
        assert_eq!(cond_of(3), Cond::Cloudy);
        assert_eq!(cond_of(45), Cond::Fog);
        assert_eq!(cond_of(53), Cond::Drizzle);
        assert_eq!(cond_of(63), Cond::Rain);
        assert_eq!(cond_of(81), Cond::Rain);
        assert_eq!(cond_of(73), Cond::Snow);
        assert_eq!(cond_of(95), Cond::Storm);
        assert_eq!(cond_of(200), Cond::Cloudy);
    }

    #[test]
    fn hot_only_for_clear_daytime_over_threshold() {
        let w = |temp, code, day| Weather { temp, code, day };
        assert!(w(33.0, 0, true).hot());
        assert!(w(32.0, 2, true).hot());
        assert!(!w(31.9, 0, true).hot());
        assert!(!w(35.0, 63, true).hot()); // hujan tidak "panas"
        assert!(!w(35.0, 0, false).hot()); // malam
    }

    #[test]
    fn stages_alternate_with_transition() {
        assert_eq!(stage_at(0.0), (0, 0, 0.0));
        let (i, s, p) = stage_at(0.19);
        assert_eq!((i, s), (0, 0));
        assert!((p - 0.5).abs() < 0.01);
        assert_eq!(stage_at(1.0).2, 1.0); // transisi selesai
        assert_eq!(stage_at(4.0).1, 1);
        assert_eq!(stage_at(8.5).1, 0);
    }

    #[test]
    fn moving_icon_runs_at_20_fps_and_holds_still_when_nobody_is_there() {
        assert_eq!(slot_frame_ms(true, true, 0.0), 16, "the change between stages stays smooth");
        assert_eq!(slot_frame_ms(false, true, 0.0), ICON_MS);
        assert_eq!(slot_frame_ms(false, true, eye::EYE_STILL_AFTER_SECS + 1.0), 0, "nobody there: the icon holds still");
        assert_eq!(slot_frame_ms(true, true, eye::EYE_STILL_AFTER_SECS + 1.0), 16, "a stage change is still drawn through");
        assert_eq!(slot_frame_ms(false, false, 0.0), 0, "temperature: nothing moves");
    }

    /// Frames per second of the clock phase with a moving icon, over one temperature + icon cycle: about 36 before
    /// (the laptop measured 48 in the seconds that had frames, 30 over the whole minute), well under half now.
    #[test]
    fn clock_phase_draws_far_fewer_frames() {
        let fps = |icon_ms: u32| {
            let (mut t, mut frames) = (0.0f32, 0u32);
            while t < 2.0 * STAGE_SECS {
                let (_, stage, prog) = stage_of(t, 2);
                let ms = if prog < 1.0 { 16 } else if stage == 1 { icon_ms } else { 0 };
                // no frames: the poll timer looks again in 100 ms
                t += if ms == 0 { 0.1 } else { ms as f32 / 1000.0 };
                frames += (ms != 0) as u32;
            }
            frames as f32 / (2.0 * STAGE_SECS)
        };
        let (before, after) = (fps(16), fps(ICON_MS));
        assert!((30.0..40.0).contains(&before), "before {before:.1} fps");
        assert!(after < before * 0.5, "after {after:.1} fps vs {before:.1}");
    }

    #[test]
    fn three_stages_cycle_when_prayer_countdown_joins() {
        assert_eq!(stage_of(0.5, 3).1, 0);
        assert_eq!(stage_of(4.5, 3).1, 1);
        assert_eq!(stage_of(8.5, 3).1, 2);
        assert_eq!(stage_of(12.5, 3).1, 0);
        // a single stage: still, no transition
        assert_eq!(stage_of(4.1, 1), (1, 0, 1.0));
        let (i, s, p) = stage_of(5.0, 2);
        assert_eq!((i, s as u8, p), stage_at(5.0));
    }

    #[test]
    fn cache_validity() {
        let line = "29.1|3|1|-6.9175|107.6191|1000";
        let ok = parse_cache(line, -6.9175, 107.6191, 2000).unwrap();
        assert_eq!((ok.temp, ok.code, ok.day), (29.1, 3, true));
        assert!(parse_cache(line, -6.2088, 106.8456, 2000).is_none()); // kota lain
        assert!(parse_cache(line, -6.9175, 107.6191, 1000 + 4 * 3600).is_none()); // basi
        assert!(parse_cache("rusak", 0.0, 0.0, 0).is_none());
    }
}

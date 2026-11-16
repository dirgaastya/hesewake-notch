//! Slide System (default tersembunyi): CPU, memori, jaringan, baterai. Pengukuran hanya berjalan selama slide terlihat
//! (tiap ±1 dtk), jadi tidak ada biaya saat slide tersembunyi atau notch menutup.

use super::*;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

const HIST: usize = 36;
const GREY: [u8; 3] = TEXT_DIM;
const GREEN: [u8; 3] = [60, 200, 110];
const SAMPLE_SECS: f32 = 1.0;
const CARD_W: f32 = 178.0;
const CARD_H: f32 = 54.0;

// ---------------------------------------------------------------- pengukuran murni (bisa dites)
/// Persentase CPU terpakai dari dua pembacaan `(idle, kernel, user)`; kernel sudah memuat waktu idle.
pub(crate) fn cpu_percent(prev: (u64, u64, u64), cur: (u64, u64, u64)) -> f32 {
    let idle = cur.0.saturating_sub(prev.0) as f32;
    let total = (cur.1.saturating_sub(prev.1) + cur.2.saturating_sub(prev.2)) as f32;
    if total <= 0.0 {
        return 0.0;
    }
    ((total - idle) / total * 100.0).clamp(0.0, 100.0)
}

/// "1.2 MB/s", "340 KB/s", "12 B/s".
pub(crate) fn fmt_rate(bps: f32) -> String {
    if bps >= 1_048_576.0 {
        format!("{:.1} MB/s", bps / 1_048_576.0)
    } else if bps >= 1024.0 {
        format!("{:.0} KB/s", bps / 1024.0)
    } else {
        format!("{:.0} B/s", bps)
    }
}

/// "3h 12m" / "45m".
pub(crate) fn fmt_remaining(secs: u32) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    if h > 0 { format!("{h}h {m:02}m") } else { format!("{m}m") }
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

fn cpu_times() -> Option<(u64, u64, u64)> {
    let (mut i, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetSystemTimes(Some(&mut i), Some(&mut k), Some(&mut u)).ok()? };
    Some((ft(i), ft(k), ft(u)))
}

/// (terpakai GB, total GB, persen)
fn ram() -> Option<(f32, f32, f32)> {
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe { GlobalMemoryStatusEx(&mut m).ok()? };
    let gb = 1024.0 * 1024.0 * 1024.0;
    let total = m.ullTotalPhys as f32;
    let used = total - m.ullAvailPhys as f32;
    Some((used / gb, total / gb, m.dwMemoryLoad as f32))
}

/// Total byte masuk/keluar semua antarmuka fisik yang aktif (tanpa loopback dan adapter virtual).
fn net_bytes() -> Option<(u64, u64)> {
    unsafe {
        let mut t: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut t).is_err() || t.is_null() {
            return None;
        }
        let n = (*t).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*t).Table.as_ptr(), n);
        let (mut rx, mut tx) = (0u64, 0u64);
        for r in rows {
            let hardware = r.InterfaceAndOperStatusFlags._bitfield & 1 == 1;
            if hardware && r.OperStatus.0 == 1 && r.Type != 24 {
                rx += r.InOctets;
                tx += r.OutOctets;
            }
        }
        FreeMibTable(t as *const c_void);
        Some((rx, tx))
    }
}

pub(crate) struct SysUi {
    entered: bool,
    enter_t: Instant,
    last: Instant,
    prev_cpu: Option<(u64, u64, u64)>,
    prev_net: Option<(u64, u64)>,
    cpu: f32,
    cpu_known: bool,
    ram: (f32, f32, f32),
    down: f32,
    up: f32,
    bat: Option<sys::Battery>,
    hist_cpu: Vec<f32>,
    hist_net: Vec<f32>,
    /// Nilai yang ditampilkan (mengejar nilai asli dengan pegas): cpu%, ram%, baterai%.
    shown: [f32; 3],
    vel: [f32; 3],
}

impl SysUi {
    pub(crate) fn new() -> SysUi {
        let old = Instant::now() - Duration::from_secs(10);
        SysUi {
            entered: false,
            enter_t: old,
            last: old,
            prev_cpu: None,
            prev_net: None,
            cpu: 0.0,
            cpu_known: false,
            ram: (0.0, 0.0, 0.0),
            down: 0.0,
            up: 0.0,
            bat: None,
            hist_cpu: Vec::new(),
            hist_net: Vec::new(),
            shown: [0.0; 3],
            vel: [0.0; 3],
        }
    }
}

fn ss(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn push_hist(v: &mut Vec<f32>, x: f32) {
    v.push(x);
    if v.len() > HIST {
        v.remove(0);
    }
}

impl App {
    fn sys_visible(&self) -> bool {
        self.expanded() && self.is_slide(SlideId::System) && self.h > 100.0
    }

    /// Dipanggil tiap poll (100 ms): ambil sampel baru bila slide terlihat dan sudah ±1 dtk. True bila perlu redraw.
    pub(crate) fn sys_sample(&mut self) -> bool {
        if !self.sys_visible() {
            let u = &mut self.sys_ui;
            u.entered = false;
            u.prev_cpu = None;
            u.prev_net = None;
            u.cpu_known = false;
            u.hist_cpu.clear();
            u.hist_net.clear();
            return false;
        }
        let u = &mut self.sys_ui;
        if !u.entered {
            u.entered = true;
            u.enter_t = Instant::now();
            u.shown = [0.0; 3];
            u.vel = [0.0; 3];
            u.last = Instant::now() - Duration::from_secs(5);
        }
        let dt = u.last.elapsed().as_secs_f32();
        if dt < SAMPLE_SECS {
            return false;
        }
        u.last = Instant::now();
        if let Some(cur) = cpu_times() {
            if let Some(prev) = u.prev_cpu {
                u.cpu = cpu_percent(prev, cur);
                u.cpu_known = true;
                push_hist(&mut u.hist_cpu, u.cpu);
            }
            u.prev_cpu = Some(cur);
        }
        if let Some(r) = ram() {
            u.ram = r;
        }
        if let Some((rx, tx)) = net_bytes() {
            if let Some((prx, ptx)) = u.prev_net {
                let secs = dt.max(0.2);
                u.down = rx.saturating_sub(prx) as f32 / secs;
                u.up = tx.saturating_sub(ptx) as f32 / secs;
                push_hist(&mut u.hist_net, u.down);
            }
            u.prev_net = Some((rx, tx));
        }
        u.bat = sys::battery();
        true
    }

    /// Satu langkah animasi pegas untuk angka/ring (dipanggil tiap frame dari `tick`).
    pub(crate) fn sys_step(&mut self, dt: f32) {
        if !self.sys_visible() {
            return;
        }
        let u = &mut self.sys_ui;
        let target = [u.cpu, u.ram.2, u.bat.map_or(0.0, |b| b.pct as f32)];
        let n = ((dt / 0.008).ceil() as i32).max(1);
        let sdt = dt / n as f32;
        for i in 0..3 {
            for _ in 0..n {
                u.vel[i] += (170.0 * (target[i] - u.shown[i]) - 22.0 * u.vel[i]) * sdt;
                u.shown[i] += u.vel[i] * sdt;
            }
        }
    }

    /// Selama terlihat, sparkline bergeser halus: butuh frame terus-menerus (hanya saat slide ini terbuka).
    pub(crate) fn sys_animating(&self) -> bool {
        self.sys_visible()
    }

    pub(crate) fn slide_system(&self, g: &mut Gfx, ox: f32, al: f32) {
        let blue = g.acc.bright;
        let cur = self.cursor_canvas();
        g.txtb(tr("System"), LX + ox, 10.0, 15.0, [255, 255, 255], al, 120.0);
        let u = &self.sys_ui;
        let el = u.enter_t.elapsed().as_secs_f32();
        let frac = (u.last.elapsed().as_secs_f32() / SAMPLE_SECS).clamp(0.0, 1.0);
        let card = |i: usize| -> (f32, f32, f32) {
            let enter = ss(0.0, 0.3, el - i as f32 * 0.05);
            let (col, row) = (i % 2, i / 2);
            (LX + ox + col as f32 * (CARD_W + 8.0), 38.0 + row as f32 * (CARD_H + 6.0) + (1.0 - enter) * 8.0, al * enter)
        };

        // CPU
        let (x, y, a) = card(0);
        g.fill(&rrect(x, y, CARD_W, CARD_H, 12.0), &paint(255, 255, 255, 0.07 * a));
        let hot = cur.is_some_and(|(hx, hy)| in_rect(hx, hy, x, y, CARD_W, CARD_H));
        if hot {
            let peak = u.hist_cpu.iter().cloned().fold(0.0, f32::max);
            g.txtb(&fill(tr("peak {}"), &[&format!("{:.0}%", peak)]), x + 14.0, y + 8.0, 10.0, blue, a, 100.0);
        } else {
            g.txtb("CPU", x + 14.0, y + 8.0, 10.0, GREY, a, 60.0);
        }
        let cpu_txt = if u.cpu_known { format!("{:.0}%", u.shown[0].clamp(0.0, 100.0)) } else { "...".to_string() };
        g.txtb(&cpu_txt, x + 14.0, y + 18.0, 22.0, [255, 255, 255], a, 70.0);
        // Beban naik: sparkline bergeser dari aksen ke merah.
        let load = ((u.shown[0] - 55.0) / 35.0).clamp(0.0, 1.0);
        let mix = |p: u8, q: u8| (p as f32 + (q as f32 - p as f32) * load) as u8;
        let cpu_col = [mix(blue[0], 255), mix(blue[1], 110), mix(blue[2], 90)];
        self.spark(g, &u.hist_cpu, 100.0, frac, x + 88.0, y + 12.0, 76.0, 30.0, cpu_col, a);

        // Memori
        let (x, y, a) = card(1);
        g.fill(&rrect(x, y, CARD_W, CARD_H, 12.0), &paint(255, 255, 255, 0.07 * a));
        if cur.is_some_and(|(hx, hy)| in_rect(hx, hy, x, y, CARD_W, CARD_H)) {
            g.txtb(&fill(tr("{} free"), &[&format!("{:.1} GB", (u.ram.1 - u.ram.0).max(0.0))]), x + 14.0, y + 8.0, 10.0, blue, a, 100.0);
        } else {
            g.txtb(tr("MEMORY"), x + 14.0, y + 8.0, 10.0, GREY, a, 70.0);
        }
        let used = format!("{:.1} GB", u.ram.0);
        g.txtb(&used, x + 14.0, y + 19.0, 20.0, [255, 255, 255], a, 90.0);
        // "of 16 GB" di baris yang sama dengan angkanya supaya tidak menempel di dasar kartu.
        let uw = g.width_ex(&used, 20.0, true);
        g.txt(&fill(tr("of {} GB"), &[&format!("{:.0}", u.ram.1)]), x + 14.0 + uw + 4.0, y + 29.0, 10.0, GREY, a, 56.0);
        let rc = if u.shown[1] > 85.0 { ACCENT } else { blue };
        g.ring(x + CARD_W - 30.0, y + 28.0, 13.0, (u.shown[1] / 100.0).clamp(0.0, 1.0), 3.5, &paint(255, 255, 255, 0.14 * a), &paint(rc[0], rc[1], rc[2], a));
        g.txt_c(&format!("{:.0}", u.shown[1].clamp(0.0, 100.0)), x + CARD_W - 30.0, y + 28.0 - 6.0, 10.5, [235, 235, 240], a, 30.0, true);

        // Jaringan
        let (x, y, a) = card(2);
        g.fill(&rrect(x, y, CARD_W, CARD_H, 12.0), &paint(255, 255, 255, 0.07 * a));
        if cur.is_some_and(|(hx, hy)| in_rect(hx, hy, x, y, CARD_W, CARD_H)) {
            let pk = u.hist_net.iter().cloned().fold(0.0, f32::max);
            g.txtb(&fill(tr("peak {}"), &[&fmt_rate(pk)]), x + 14.0, y + 8.0, 10.0, blue, a, 120.0);
        } else {
            g.txtb(tr("NETWORK"), x + 14.0, y + 8.0, 10.0, GREY, a, 70.0);
        }
        let arrow = |g: &mut Gfx, ax: f32, ay: f32, down: bool, c: [u8; 3]| {
            let p = paint(c[0], c[1], c[2], a);
            let d = if down { 1.0 } else { -1.0 };
            g.line(ax, ay - 3.5 * d, ax, ay + 3.5 * d, 1.5, &p);
            g.line(ax - 2.5, ay + 1.0 * d, ax, ay + 3.5 * d, 1.5, &p);
            g.line(ax + 2.5, ay + 1.0 * d, ax, ay + 3.5 * d, 1.5, &p);
        };
        let t = self.t;
        let sway = |rate: f32, ph: f32| (t * 5.0 + ph).sin() * 0.8 * (rate / 2.0e6).clamp(0.0, 1.0);
        arrow(g, x + 16.0, y + 29.0 + sway(u.down, 0.0), true, blue);
        g.txtb(&fmt_rate(u.down), x + 26.0, y + 21.0, 12.0, [255, 255, 255], a, 80.0);
        arrow(g, x + 16.0, y + 43.0 - sway(u.up * 4.0, 1.0), false, GREEN);
        g.txt(&fmt_rate(u.up), x + 26.0, y + 35.0, 11.0, [200, 200, 208], a, 80.0);
        let peak = u.hist_net.iter().cloned().fold(256.0 * 1024.0, f32::max);
        self.spark(g, &u.hist_net, peak, frac, x + 104.0, y + 12.0, 60.0, 30.0, blue, a);

        // Baterai
        let (x, y, a) = card(3);
        g.fill(&rrect(x, y, CARD_W, CARD_H, 12.0), &paint(255, 255, 255, 0.07 * a));
        g.txtb(tr("BATTERY"), x + 14.0, y + 8.0, 10.0, GREY, a, 70.0);
        match u.bat {
            Some(b) => {
                g.txtb(&format!("{:.0}%", u.shown[2].clamp(0.0, 100.0)), x + 14.0, y + 18.0, 22.0, [255, 255, 255], a, 70.0);
                let sub = if b.charging {
                    tr("Charging").to_string()
                } else if b.ac {
                    tr("Plugged in").to_string()
                } else {
                    b.secs.map_or(tr("On battery").to_string(), |s| fill(tr("{} left"), &[&fmt_remaining(s)]))
                };
                g.txt(&sub, x + 86.0, y + 28.0, 11.0, GREY, a, 90.0);
                let col = if b.pct <= 20 && !b.ac { ACCENT } else { GREEN };
                let low = b.pct <= 20 && !b.ac;
                let pul = if low { 0.6 + 0.4 * (self.t * 5.0).sin() } else { 1.0 };
                g.ring(x + CARD_W - 28.0, y + 28.0, 11.0, (u.shown[2] / 100.0).clamp(0.0, 1.0), 3.0, &paint(255, 255, 255, 0.14 * a), &paint(col[0], col[1], col[2], a * pul));
                if b.charging {
                    // sorot putih berputar di cincin saat mengisi
                    let a0 = self.t * 3.0;
                    g.arc(x + CARD_W - 28.0, y + 28.0, 11.0, a0, a0 + 0.7, 3.0, &paint(255, 255, 255, 0.7 * a));
                }
                if b.ac {
                    g.icon_bolt(x + CARD_W - 28.0, y + 28.0, 10.0 * (1.0 + 0.08 * (self.t * 4.0).sin()), &paint(col[0], col[1], col[2], a));
                }
            }
            None => {
                g.txtb(tr("No battery"), x + 14.0, y + 21.0, 16.0, [200, 200, 208], a, 100.0);
                let nw = g.width_ex(tr("No battery"), 16.0, true);
                g.txt(tr("Desktop PC"), x + 14.0 + nw + 6.0, y + 27.0, 10.0, GREY, a, 70.0);
            }
        }
    }

    /// Sparkline area + garis; bergeser halus sesuai waktu sejak sampel terakhir (`frac` 0..1).
    #[allow(clippy::too_many_arguments)]
    fn spark(&self, g: &mut Gfx, hist: &[f32], max: f32, frac: f32, x: f32, y: f32, w: f32, h: f32, rgb: [u8; 3], a: f32) {
        g.fill(&rrect(x, y, w, h, 6.0), &paint(255, 255, 255, 0.05 * a));
        if hist.len() < 2 {
            return;
        }
        let step = w / (HIST as f32 - 1.0);
        let n = hist.len();
        let pts: Vec<(f32, f32)> = hist
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let px = x + w - (n - 1 - i) as f32 * step - frac * step;
                (px, y + h - 2.0 - (v / max).clamp(0.0, 1.0) * (h - 4.0))
            })
            .filter(|(px, _)| *px >= x)
            .collect();
        let n = pts.len();
        if n < 2 {
            return;
        }
        // area di bawah kurva
        let mut area = vec![(pts[0].0, y + h - 2.0)];
        area.extend(pts.iter().copied());
        area.push((pts[n - 1].0, y + h - 2.0));
        let col = |al: f32| Color::from_rgba8(rgb[0], rgb[1], rgb[2], (al.clamp(0.0, 1.0) * 255.0) as u8);
        let mut ap = Paint::default();
        if let Some(sh) = LinearGradient::new(Point::from_xy(0.0, y), Point::from_xy(0.0, y + h), vec![GradientStop::new(0.0, col(0.38 * a)), GradientStop::new(1.0, col(0.0))], SpreadMode::Pad, Transform::identity()) {
            ap.shader = sh;
            ap.anti_alias = true;
            g.poly(&area, &ap);
        } else {
            g.poly(&area, &paint(rgb[0], rgb[1], rgb[2], 0.22 * a));
        }
        // glow tipis di bawah garis
        let glow = paint(rgb[0], rgb[1], rgb[2], 0.12 * a);
        for pair in pts.windows(2) {
            g.line(pair[0].0, pair[0].1, pair[1].0, pair[1].1, 4.0, &glow);
        }
        let lp = paint(rgb[0], rgb[1], rgb[2], a);
        for pair in pts.windows(2) {
            g.line(pair[0].0, pair[0].1, pair[1].0, pair[1].1, 1.4, &lp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_percent_from_deltas() {
        // total 1000 (kernel 400 + user 600), idle 250 -> 75% terpakai
        assert!((cpu_percent((0, 0, 0), (250, 400, 600)) - 75.0).abs() < 0.01);
        // tidak ada waktu berjalan -> 0, bukan NaN
        assert_eq!(cpu_percent((5, 10, 10), (5, 10, 10)), 0.0);
        // idle > total (pembacaan tidak konsisten) tetap dalam 0..100
        assert_eq!(cpu_percent((0, 0, 0), (900, 100, 100)), 0.0);
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_rate(12.0), "12 B/s");
        assert_eq!(fmt_rate(2048.0), "2 KB/s");
        assert_eq!(fmt_rate(1_572_864.0), "1.5 MB/s");
        assert_eq!(fmt_remaining(45 * 60), "45m");
        assert_eq!(fmt_remaining(3 * 3600 + 5 * 60), "3h 05m");
    }
}

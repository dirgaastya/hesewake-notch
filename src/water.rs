//! Fitur Water: pelacak minum, pengingat, slide Water (angka besar, toggle reminder, grafik per jam, Settings).

use super::*;
use icons::{hgrad, vgrad};

pub(crate) const SNOOZE: Duration = Duration::from_secs(10 * 60);

// ---------------------------------------------------------------- layout (koordinat kanvas, slide di LX..RX)
const GREEN: [u8; 3] = [60, 200, 110];
const GREY: [u8; 3] = TEXT_DIM;

const TOGGLE: (f32, f32, f32, f32) = (LX + 152.0, 42.0, 46.0, 26.0);
const DRINK: (f32, f32, f32, f32) = (LX, 116.0, 84.0, 32.0);
const SIDE: (f32, f32, f32, f32) = (LX + 92.0, 116.0, 106.0, 32.0); // Settings, atau Snooze saat reminder aktif
const BACK: (f32, f32, f32, f32) = (LX, 8.0, 26.0, 26.0); // tombol kembali bulat di header Settings
const DIV_X: f32 = LX + 210.0;
const PLOT_L: f32 = LX + 224.0;
const PLOT_R: f32 = RX - 36.0;
const PLOT_T: f32 = 54.0;
const PLOT_B: f32 = 126.0;
const N_BARS: usize = 8;

const GOALS: [u32; 4] = [1500, 2000, 2500, 3000];
const REMINDS: [u32; 4] = [0, 30, 60, 90];
const BOTTLE_DEFAULT: u32 = 750;

// Tata letak Settings (koordinat kanvas, x relatif ke LX): empat baris di kiri, kartu statistik + Reset di kanan.
const ROW_Y: [f32; 4] = [36.0, 66.0, 96.0, 126.0];
const ROW_H: f32 = 24.0;
const LBL_W: f32 = 40.0;
const BTN: f32 = 22.0;
const VAL_W: f32 = 48.0;
const CHIP_X: f32 = 52.0;
const CHIP_W: f32 = 52.0;
const CHIP_P: f32 = 56.0;
const CARD: (f32, f32, f32, f32) = (RX - 112.0, 36.0, 112.0, 82.0);
const RESET: (f32, f32, f32, f32) = (RX - 112.0, 126.0, 112.0, 24.0);

/// Elemen yang bisa diklik di Settings Water.
#[derive(Clone, Copy, PartialEq, Debug)]
enum SetHit {
    BottleDn,
    BottleUp,
    GlassDn,
    GlassUp,
    Goal(usize),
    Remind(usize),
    StartDn,
    StartUp,
    EndDn,
    EndUp,
    Reset,
}

/// Naikkan/turunkan `v` sebanyak `d` langkah `step`, dibatasi `lo..=hi`.
pub(crate) fn stepped(v: u32, d: i32, step: u32, lo: u32, hi: u32) -> u32 {
    (v as i64 + d as i64 * step as i64).clamp(lo as i64, hi.max(lo) as i64) as u32
}

/// Kotak tombol "-" dan "+" serta titik tengah nilai untuk stepper yang labelnya mulai di `x`.
fn stepper_rects(x: f32, y: f32) -> ((f32, f32, f32, f32), (f32, f32, f32, f32), f32) {
    let dn = (x + LBL_W, y + 1.0, BTN, BTN);
    let up = (x + LBL_W + BTN + VAL_W, y + 1.0, BTN, BTN);
    (dn, up, x + LBL_W + BTN + VAL_W / 2.0)
}

const GLASS_X: f32 = LX + 140.0;
const HOURS_END_X: f32 = LX + LBL_W + BTN + VAL_W + BTN + 22.0 - LBL_W;

/// Semua elemen klik Settings beserta kotaknya; dipakai bersama oleh gambar dan klik supaya tidak bergeser.
fn set_hits() -> Vec<(SetHit, (f32, f32, f32, f32))> {
    let mut v = Vec::new();
    let (d, u, _) = stepper_rects(LX, ROW_Y[0]);
    v.push((SetHit::BottleDn, d));
    v.push((SetHit::BottleUp, u));
    let (d, u, _) = stepper_rects(GLASS_X, ROW_Y[0]);
    v.push((SetHit::GlassDn, d));
    v.push((SetHit::GlassUp, u));
    for i in 0..4 {
        v.push((SetHit::Goal(i), (LX + CHIP_X + i as f32 * CHIP_P, ROW_Y[1], CHIP_W, ROW_H)));
        v.push((SetHit::Remind(i), (LX + CHIP_X + i as f32 * CHIP_P, ROW_Y[2], CHIP_W, ROW_H)));
    }
    let (d, u, _) = stepper_rects(LX, ROW_Y[3]);
    v.push((SetHit::StartDn, d));
    v.push((SetHit::StartUp, u));
    let (d, u, _) = stepper_rects(HOURS_END_X, ROW_Y[3]);
    v.push((SetHit::EndDn, d));
    v.push((SetHit::EndUp, u));
    v.push((SetHit::Reset, RESET));
    v
}

// ---------------------------------------------------------------- data
pub(crate) struct Water {
    pub(crate) day: String,
    pub(crate) ml: u32,
    pub(crate) goal: u32,
    pub(crate) remind_min: u32,
    pub(crate) glass: u32,
    pub(crate) hours: [u32; 24],
    /// Kapasitas botol (ml); dipakai untuk menghitung berapa kali botol diisi ulang.
    pub(crate) bottle: u32,
    /// Jam aktif pengingat: dari `start_h` (termasuk) sampai `end_h` (tidak termasuk), 0..=24.
    pub(crate) start_h: u32,
    pub(crate) end_h: u32,
    /// Jumlah pengingat yang sudah muncul hari ini.
    pub(crate) reminders: u32,
    /// Interval terakhir yang aktif, dipakai saat toggle reminder dinyalakan lagi.
    pub(crate) last_remind: u32,
    pub(crate) next: Instant,
}

impl Water {
    /// Format: `day|ml|goal|remind[|glass[|h0,...,h23[|bottle|start_h|end_h|reminders]]]` (field setelah `remind` opsional).
    pub(crate) fn parse(s: &str, today: &str) -> Water {
        let mut w = Water {
            day: today.to_string(),
            ml: 0,
            goal: 2000,
            remind_min: 60,
            glass: 250,
            hours: [0; 24],
            bottle: BOTTLE_DEFAULT,
            start_h: 8,
            end_h: 22,
            reminders: 0,
            last_remind: 60,
            next: Instant::now(),
        };
        let p: Vec<&str> = s.trim().split('|').collect();
        if p.len() >= 4 {
            w.goal = p[2].parse().unwrap_or(2000);
            w.remind_min = p[3].parse().unwrap_or(60);
            if let Some(g) = p.get(4).and_then(|g| g.parse::<u32>().ok()).filter(|g| (50..=2000).contains(g)) {
                w.glass = g;
            }
            if let Some(b) = p.get(6).and_then(|b| b.parse::<u32>().ok()).filter(|b| (100..=3000).contains(b)) {
                w.bottle = b;
            }
            if let (Some(s), Some(e)) = (p.get(7).and_then(|v| v.parse::<u32>().ok()), p.get(8).and_then(|v| v.parse::<u32>().ok())) {
                if s < e && e <= 24 {
                    (w.start_h, w.end_h) = (s, e);
                }
            }
            if p[0] == today {
                w.reminders = p.get(9).and_then(|v| v.parse().ok()).unwrap_or(0);
                w.ml = p[1].parse().unwrap_or(0);
                if let Some(h) = p.get(5) {
                    for (i, v) in h.split(',').take(24).enumerate() {
                        w.hours[i] = v.parse().unwrap_or(0);
                    }
                }
            }
        }
        if w.remind_min > 0 {
            w.last_remind = w.remind_min;
        }
        w
    }

    pub(crate) fn serialize(&self) -> String {
        let h: Vec<String> = self.hours.iter().map(|v| v.to_string()).collect();
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
            self.day, self.ml, self.goal, self.remind_min, self.glass, h.join(","), self.bottle, self.start_h, self.end_h, self.reminders
        )
    }

    pub(crate) fn load() -> Water {
        let s = std::fs::read_to_string(data_path("water.txt")).unwrap_or_default();
        let mut w = Water::parse(&s, &today());
        w.reset_timer();
        w
    }
    pub(crate) fn save(&self) {
        if cfg!(test) {
            return; // tes tidak boleh menimpa data asli di %APPDATA%
        }
        write_data("water.txt", self.serialize());
    }
    pub(crate) fn reset_timer(&mut self) {
        self.next = Instant::now() + Duration::from_secs(self.remind_min.max(1) as u64 * 60);
    }
    /// Tunda pengingat berikutnya.
    pub(crate) fn snooze(&mut self) {
        self.next = Instant::now() + SNOOZE;
    }
    pub(crate) fn roll(&mut self) -> bool {
        let d = today();
        if d != self.day {
            self.day = d;
            self.ml = 0;
            self.hours = [0; 24];
            self.reminders = 0;
            self.save();
            return true;
        }
        false
    }
    /// Tambah/kurangi `d` ml; dicatat pada bucket jam `hour`.
    pub(crate) fn add(&mut self, d: i32, hour: usize) {
        self.ml = (self.ml as i32 + d).max(0) as u32;
        let h = hour.min(23);
        self.hours[h] = (self.hours[h] as i32 + d).max(0) as u32;
        self.reset_timer();
        self.save();
    }
    /// Jam `h` (0..=23) termasuk jam aktif pengingat.
    pub(crate) fn in_window(&self, h: u32) -> bool {
        h >= self.start_h && h < self.end_h
    }
    /// Berapa kali botol penuh sudah habis hari ini.
    pub(crate) fn refills(&self) -> u32 {
        if self.bottle == 0 { 0 } else { self.ml / self.bottle }
    }
    /// Hapus progres hari ini beserta hitungan pengingat dan refill.
    pub(crate) fn reset_today(&mut self) {
        self.ml = 0;
        self.hours = [0; 24];
        self.reminders = 0;
        self.reset_timer();
        self.save();
    }
    pub(crate) fn remind_on(&self) -> bool {
        self.remind_min > 0
    }
    pub(crate) fn toggle_remind(&mut self) {
        if self.remind_min > 0 {
            self.last_remind = self.remind_min;
            self.remind_min = 0;
        } else {
            self.remind_min = self.last_remind.max(30);
            self.reset_timer();
        }
        self.save();
    }
    /// Delapan jam yang ditampilkan di grafik: dimulai `start`, jam sekarang di dekat ujung kanan.
    pub(crate) fn window_start(now_hour: usize) -> usize {
        (now_hour as i32 - 5).clamp(0, (24 - N_BARS) as i32) as usize
    }
    /// Batas atas sumbu grafik (kelipatan 200 ml, minimal 400).
    pub(crate) fn axis_max(&self, start: usize) -> u32 {
        let m = self.hours[start..start + N_BARS].iter().copied().max().unwrap_or(0).max(400);
        (m + 199) / 200 * 200
    }
}

pub(crate) fn check_reminder(a: &mut App) -> bool {
    if a.water.remind_min == 0 {
        return false;
    }
    let now = Instant::now();
    if now < a.water.next {
        return false;
    }
    a.water.reset_timer();
    let st = now_tm();
    if !a.water.in_window(st.wHour as u32) || a.water.ml >= a.water.goal || sys::user_fullscreen() {
        return false;
    }
    a.water.reminders += 1;
    a.water.save();
    if a.expanded() {
        // Notch sedang terbuka: pengingat tampil di dalam slide Water (tombol Snooze menggantikan Settings).
        a.show_id(SlideId::Water);
        a.water_view(0);
        a.reminding = true;
        a.peek_until = Some(now + Duration::from_secs(10));
    } else {
        a.push_toast(toast::ToastKind::Water);
    }
    true
}

// ---------------------------------------------------------------- animasi
/// State animasi slide Water (pegas ala carousel): angka/bar/ring menghitung naik, batang tumbuh bertahap,
/// knob toggle bergeser, tombol menyusut saat ditekan, riak dari ikon tetes saat Drink.
pub(crate) struct WaterUi {
    entered: bool,
    enter_t: Instant,
    shown: f32,
    vel: f32,
    knob: f32,
    kvel: f32,
    bars: [f32; N_BARS],
    bvel: [f32; N_BARS],
    press: Option<(Instant, u8)>,
    ripple: Option<Instant>,
    bell: Option<Instant>,
    view: u8,
    view_t0: Instant,
    view_dir: f32,
}

impl WaterUi {
    pub(crate) fn new() -> WaterUi {
        let old = Instant::now() - Duration::from_secs(10);
        WaterUi {
            entered: false,
            enter_t: old,
            shown: 0.0,
            vel: 0.0,
            knob: 0.0,
            kvel: 0.0,
            bars: [0.0; N_BARS],
            bvel: [0.0; N_BARS],
            press: None,
            ripple: None,
            bell: None,
            view: 0,
            view_t0: old,
            view_dir: 1.0,
        }
    }
}

fn ss(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn spring(x: &mut f32, v: &mut f32, target: f32, k: f32, c: f32, dt: f32) {
    let n = ((dt / 0.008).ceil() as i32).max(1);
    let sdt = dt / n as f32;
    for _ in 0..n {
        *v += (k * (target - *x) - c * *v) * sdt;
        *x += *v * sdt;
    }
}

pub(crate) fn fmt_l(ml: f32) -> String {
    let s = format!("{:.2}", ml / 1000.0);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() { "0".to_string() } else { s.to_string() }
}

impl App {
    fn water_visible(&self) -> bool {
        self.expanded() && self.is_slide(SlideId::Water) && self.h > 100.0
    }

    pub(crate) fn water_view(&mut self, v: u8) {
        if self.wui.view != v {
            self.wui.view_dir = if v == 1 { 1.0 } else { -1.0 };
            self.wui.view = v;
            self.wui.view_t0 = Instant::now();
        }
    }

    fn water_bar_targets(&self) -> [f32; N_BARS] {
        let start = Water::window_start(now_tm().wHour as usize);
        let max = self.water.axis_max(start) as f32;
        let mut t = [0.0; N_BARS];
        for i in 0..N_BARS {
            t[i] = self.water.hours[start + i] as f32 / max;
        }
        t
    }

    /// Maju satu langkah animasi Water (dipanggil tiap frame dari `tick`).
    pub(crate) fn water_ui_step(&mut self, dt: f32) {
        if !self.water_visible() {
            // Slide tidak terlihat: reset supaya animasi masuk berjalan lagi saat dibuka.
            let u = &mut self.wui;
            u.entered = false;
            u.view = 0;
            u.press = None;
            u.shown = 0.0;
            u.vel = 0.0;
            u.knob = if self.water.remind_on() { 1.0 } else { 0.0 };
            u.kvel = 0.0;
            u.bars = [0.0; N_BARS];
            u.bvel = [0.0; N_BARS];
            return;
        }
        let targets = self.water_bar_targets();
        let (ml, on) = (self.water.ml as f32, self.water.remind_on());
        let u = &mut self.wui;
        if !u.entered {
            u.entered = true;
            u.enter_t = Instant::now();
            u.shown = 0.0;
            u.knob = if on { 1.0 } else { 0.0 };
            // `--demo-water-settings`: buka langsung tampilan Settings (uji visual)
            if std::env::args().any(|a| a == "--demo-water-settings") {
                u.view = 1;
                u.view_dir = 1.0;
                u.view_t0 = Instant::now() - Duration::from_secs(1);
            }
        }
        spring(&mut u.shown, &mut u.vel, ml, 190.0, 24.0, dt);
        spring(&mut u.knob, &mut u.kvel, if on { 1.0 } else { 0.0 }, 380.0, 30.0, dt);
        let el = u.enter_t.elapsed().as_secs_f32();
        for i in 0..N_BARS {
            if el >= i as f32 * 0.03 {
                spring(&mut u.bars[i], &mut u.bvel[i], targets[i], 260.0, 22.0, dt);
            }
        }
        for slot in [&mut u.press] {
            if slot.map_or(false, |(t, _)| t.elapsed().as_secs_f32() > 0.35) {
                *slot = None;
            }
        }
        if u.ripple.map_or(false, |t| t.elapsed().as_secs_f32() > 0.8) {
            u.ripple = None;
        }
        if u.bell.map_or(false, |t| t.elapsed().as_secs_f32() > 0.7) {
            u.bell = None;
        }
    }

    /// True selama ada animasi Water yang masih bergerak (menahan timer 16 ms).
    pub(crate) fn water_animating(&self) -> bool {
        if !self.water_visible() {
            return false;
        }
        let u = &self.wui;
        let targets = self.water_bar_targets();
        let on = if self.water.remind_on() { 1.0 } else { 0.0 };
        (u.shown - self.water.ml as f32).abs() > 1.0
            || u.vel.abs() > 2.0
            || (u.knob - on).abs() > 0.01
            || u.kvel.abs() > 0.05
            || u.bars.iter().zip(targets.iter()).any(|(b, t)| (b - t).abs() > 0.004)
            || u.bvel.iter().any(|v| v.abs() > 0.02)
            || u.press.is_some()
            || u.ripple.is_some()
            || u.bell.is_some()
            || !u.entered
            || u.view_t0.elapsed().as_secs_f32() < 0.3
    }

    pub(crate) fn water_add(&mut self, d: i32) {
        self.water.add(d, now_tm().wHour as usize);
        self.reminding = false;
        self.peek_until = None;
    }

    pub(crate) fn water_snooze(&mut self) {
        self.water.snooze();
        self.reminding = false;
        self.peek_until = None;
    }

    /// Terapkan satu aksi Settings ke data Water lalu simpan.
    fn water_set_apply(&mut self, h: SetHit) {
        let w = &mut self.water;
        match h {
            SetHit::BottleDn => w.bottle = stepped(w.bottle, -1, 50, 250, 2000),
            SetHit::BottleUp => w.bottle = stepped(w.bottle, 1, 50, 250, 2000),
            SetHit::GlassDn => w.glass = stepped(w.glass, -1, 50, 50, 1000),
            SetHit::GlassUp => w.glass = stepped(w.glass, 1, 50, 50, 1000),
            SetHit::Goal(i) => w.goal = GOALS[i],
            SetHit::Remind(i) => {
                w.remind_min = REMINDS[i];
                if REMINDS[i] > 0 {
                    w.last_remind = REMINDS[i];
                    w.reset_timer();
                }
            }
            SetHit::StartDn => w.start_h = stepped(w.start_h, -1, 1, 0, w.end_h.saturating_sub(1)),
            SetHit::StartUp => w.start_h = stepped(w.start_h, 1, 1, 0, w.end_h.saturating_sub(1)),
            SetHit::EndDn => w.end_h = stepped(w.end_h, -1, 1, w.start_h + 1, 24),
            SetHit::EndUp => w.end_h = stepped(w.end_h, 1, 1, w.start_h + 1, 24),
            SetHit::Reset => {
                w.reset_today();
                self.wui.ripple = None;
                return;
            }
        }
        w.save();
    }

    /// Klik pada slide Water (koordinat kanvas).
    pub(crate) fn water_click(&mut self, x: f32, y: f32) {
        let hit = |r: (f32, f32, f32, f32)| in_rect(x, y, r.0, r.1, r.2, r.3);
        if self.wui.view == 1 {
            if hit(BACK) {
                self.water_view(0);
                return;
            }
            if let Some((h, _)) = set_hits().into_iter().find(|(_, r)| hit(*r)) {
                self.water_set_apply(h);
            }
            return;
        }
        if hit(TOGGLE) {
            self.water.toggle_remind();
            if self.water.remind_on() {
                self.wui.bell = Some(Instant::now());
            }
        } else if hit(DRINK) {
            self.wui.press = Some((Instant::now(), 0));
            self.wui.ripple = Some(Instant::now());
            let g = self.water.glass as i32;
            self.water_add(g);
        } else if hit(SIDE) {
            if self.reminding {
                self.water_snooze();
            } else {
                self.wui.press = Some((Instant::now(), 1));
                self.water_view(1);
            }
        }
    }

    // ------------------------------------------------------------ gambar
    pub(crate) fn slide_water(&self, g: &mut Gfx, ox: f32, al: f32) {
        // Transisi Main <-> Settings: konten masuk bergeser 16 px + fade.
        let k = ss(0.0, 0.24, self.wui.view_t0.elapsed().as_secs_f32());
        let vx = ox + (1.0 - k) * 16.0 * self.wui.view_dir;
        let va = al * k;
        self.water_header(g, ox, al);
        if self.wui.view == 1 {
            self.water_settings(g, vx, va);
        } else {
            self.water_main(g, vx, va, al);
        }
    }

    fn water_header(&self, g: &mut Gfx, ox: f32, al: f32) {
        let wt = &self.water;
        let y = 10.0;
        let settings = self.wui.view == 1;
        let k = ss(0.0, 0.24, self.wui.view_t0.elapsed().as_secs_f32());
        // Judul: "Water Reminder", atau "Settings" dengan tombol kembali bulat di kirinya.
        let (lead, title_w) = if settings {
            let (bx, by, bw, bh) = BACK;
            let (cx, cy) = (bx + ox + bw / 2.0, by + bh / 2.0);
            g.circle(cx, cy, bw / 2.0, &paint(255, 255, 255, 0.12 * al * k));
            let ch = paint(235, 238, 245, al * k);
            g.line(cx + 2.0, cy - 5.0, cx - 3.0, cy, 1.8, &ch);
            g.line(cx - 3.0, cy, cx + 2.0, cy + 5.0, 1.8, &ch);
            let x = LX + ox + 36.0;
            g.txtb(tr("Settings"), x, y, 15.0, [255, 255, 255], al, 120.0);
            (x, g.width_ex(tr("Settings"), 15.0, true))
        } else {
            // two-colour title; "|" marks where the blue part starts
            let (t1, t2) = tr("Water |Reminder").split_once('|').unwrap_or(("", ""));
            g.txtb(t1, LX + ox, y, 15.0, [255, 255, 255], al, 120.0);
            let w1 = g.width_ex(t1, 15.0, true);
            g.txtb(t2, LX + ox + w1, y, 15.0, g.acc.bright, al, 120.0);
            (LX + ox, w1 + g.width_ex(t2, 15.0, true))
        };
        // Ring progres kecil + tanggal + jam, atau "Time to drink" saat reminder aktif.
        let rx = lead + title_w + 22.0;
        let frac = if wt.goal > 0 { (self.wui.shown / wt.goal as f32).clamp(0.0, 1.0) } else { 0.0 };
        let rc = if wt.ml >= wt.goal { GREEN } else { g.acc.bright };
        g.ring(rx, y + 9.5, 8.0, frac, 2.6, &paint(255, 255, 255, 0.14 * al), &paint(rc[0], rc[1], rc[2], al));
        let tx = rx + 16.0;
        if self.reminding && !settings {
            g.txtb(tr("Time to drink"), tx, y + 1.0, 13.0, g.acc.bright, al, 140.0);
        } else {
            let st = now_tm();
            let date = format!("{} {} {} {}", clock::day_short(st.wDayOfWeek as usize), st.wDay, clock::month((st.wMonth as usize).max(1) - 1), st.wYear);
            g.txt(&date, tx, y + 1.0, 13.0, [225, 225, 230], al, 150.0);
            let dw = g.width(&date, 13.0);
            g.txtb(&format!("{:02}:{:02}", st.wHour, st.wMinute), tx + dw + 8.0, y + 1.0, 13.0, [255, 255, 255], al, 60.0);
        }
    }

    fn water_main(&self, g: &mut Gfx, ox: f32, al: f32, hal: f32) {
        let wt = &self.water;
        let shown = self.wui.shown.max(0.0);
        // Angka besar + target.
        let big = fmt_l(shown);
        g.txtb(&big, LX + ox, 36.0, 42.0, [255, 255, 255], al, 120.0);
        let bw = g.width_ex(&big, 42.0, true);
        g.txt(&format!("/ {} L", fmt_l(wt.goal as f32)), LX + ox + bw + 6.0, 56.0, 14.0, GREY, al, 80.0);

        // Toggle reminder (+ goyang bel saat dinyalakan).
        let swing = self.wui.bell.map_or(0.0, |t| {
            let e = t.elapsed().as_secs_f32();
            (e * 22.0).sin() * (1.0 - e / 0.7).max(0.0)
        });
        g.toggle(TOGGLE.0 + ox, TOGGLE.1, TOGGLE.2, TOGGLE.3, self.wui.knob, swing, al);

        // "Today · 100% · Goal reached"
        let pct = if wt.goal > 0 { (wt.ml * 100 / wt.goal).min(999) } else { 0 };
        let sy = 84.0;
        let part1 = tr("Today \u{b7} ");
        g.txt(part1, LX + ox, sy, 12.0, GREY, al, 120.0);
        let mut x = LX + ox + g.width(part1, 12.0);
        let pc = format!("{}%", pct);
        g.txtb(&pc, x, sy, 12.0, g.acc.bright, al, 80.0);
        x += g.width_ex(&pc, 12.0, true);
        let rest = if wt.ml >= wt.goal {
            tr(" \u{b7} Goal reached").to_string()
        } else {
            fill(tr(" \u{b7} {} L to go"), &[&fmt_l((wt.goal - wt.ml) as f32)])
        };
        g.txt(&rest, x, sy, 12.0, GREY, al, 140.0);

        // Progress bar gradasi.
        let (bx, by, bwid) = (LX + ox, 102.0, 150.0);
        g.fill(&rrect(bx, by, bwid, 7.0, 3.5), &paint(255, 255, 255, 0.12 * al));
        let frac = if wt.goal > 0 { (shown / wt.goal as f32).clamp(0.0, 1.0) } else { 0.0 };
        if frac > 0.005 {
            let fw = (bwid * frac).max(7.0);
            g.fill(&rrect(bx, by, fw, 7.0, 3.5), &hgrad(bx, bx + bwid, g.acc.pale, g.acc.bright, al));
        }

        // Tombol Drink (+ efek tekan + riak dari ikon tetes) dan Settings / Snooze.
        let press = |which: u8| -> f32 {
            match self.wui.press {
                Some((t, w)) if w == which => 1.0 - 0.05 * (1.0 - ss(0.0, 0.3, t.elapsed().as_secs_f32())) * (t.elapsed().as_secs_f32() * 14.0).cos().abs(),
                _ => 1.0,
            }
        };
        let (dx, dy, dw, dh) = (DRINK.0 + ox, DRINK.1, DRINK.2, DRINK.3);
        g.pill(dx, dy, dw, dh, g.acc.dark, 0.85 * al, press(0));
        let (ix, iy) = (dx + 22.0, dy + dh / 2.0);
        if let Some(t) = self.wui.ripple {
            let e = t.elapsed().as_secs_f32() / 0.8;
            let r = 6.0 + 20.0 * e;
            let c = g.acc.bright;
            let p = paint(c[0], c[1], c[2], (1.0 - e).max(0.0) * 0.55 * al);
            g.ring(ix, iy, r, 0.0, 1.5, &p, &p);
        }
        g.icon_water_drop(ix, iy, 14.0, g.acc.pale, g.acc.bright, al);
        g.txtb(tr("Drink"), dx + 36.0, dy + ty(dh, 14.0), 14.0, g.acc.pale, al, 60.0);

        let (sx, sy2, sw, sh) = (SIDE.0 + ox, SIDE.1, SIDE.2, SIDE.3);
        if self.reminding {
            g.pill(sx, sy2, sw, sh, [255, 255, 255], 0.14 * al, press(1));
            g.txt_c(tr("Snooze 10 min"), sx + sw / 2.0, sy2 + ty(sh, 13.0), 13.0, [225, 230, 240], al, sw, true);
        } else {
            g.pill(sx, sy2, sw, sh, [255, 255, 255], 0.10 * al, press(1));
            g.icon_sliders(sx + 20.0, sy2 + sh / 2.0, 14.0, &paint(225, 225, 232, al));
            g.txtb(tr("Settings##short"), sx + 36.0, sy2 + ty(sh, 13.0), 13.0, [225, 225, 232], al, 64.0);
        }

        // Garis pemisah + grafik per jam.
        g.rect(DIV_X + ox, 46.0, 1.0, 104.0, &paint(255, 255, 255, 0.12 * hal));
        self.water_chart(g, ox, hal);
    }

    fn water_chart(&self, g: &mut Gfx, ox: f32, al: f32) {
        let wt = &self.water;
        let now_h = now_tm().wHour as usize;
        let start = Water::window_start(now_h);
        let max = wt.axis_max(start) as f32;
        let (pl, pr) = (PLOT_L + ox, PLOT_R + ox);
        let gap = 6.0;
        let bw = (pr - pl - gap * (N_BARS as f32 - 1.0)) / N_BARS as f32;
        let ph = PLOT_B - PLOT_T;

        // Garis bantu putus-putus + label sumbu (0, tengah, atas).
        for (frac, label) in [(1.0f32, format!("{}", max as u32)), (0.5, format!("{}", (max / 2.0) as u32)), (0.0, "0 ml".to_string())] {
            let y = PLOT_B - ph * frac;
            if frac > 0.0 {
                let mut x = pl;
                while x < pr {
                    g.rect(x, y, 3.0, 1.0, &paint(255, 255, 255, 0.16 * al));
                    x += 6.0;
                }
            }
            g.txt(&label, pr + 8.0, y - 6.0, 11.0, GREY, al, 40.0);
        }
        // Batang: latar redup penuh + isi bergradasi yang tumbuh dari dasar.
        for i in 0..N_BARS {
            let x = pl + i as f32 * (bw + gap);
            g.fill(&rrect(x, PLOT_T, bw, ph, bw / 2.0), &paint(255, 255, 255, 0.06 * al));
            let f = self.wui.bars[i].clamp(0.0, 1.05);
            let target = wt.hours[start + i];
            if target > 0 || f > 0.003 {
                let hgt = (ph * f).max(if target > 0 { bw } else { 0.0 }).min(ph);
                if hgt > 0.5 {
                    let top = PLOT_B - hgt;
                    g.fill(&rrect(x, top, bw, hgt, bw / 2.0), &vgrad(top, PLOT_B, g.acc.bright, g.acc.deep, al));
                }
            }
            if i % 3 == 0 {
                g.txt_c(&format!("{:02}", start + i), x + bw / 2.0, PLOT_B + 6.0, 11.0, GREY, al, 24.0, false);
            }
        }
    }

    /// Tombol bulat "-" / "+" di stepper; redup bila nilai sudah di batas.
    fn round_btn(g: &mut Gfx, r: (f32, f32, f32, f32), plus: bool, ox: f32, al: f32, enabled: bool) {
        let (cx, cy) = (r.0 + ox + r.2 / 2.0, r.1 + r.3 / 2.0);
        let a = if enabled { al } else { al * 0.35 };
        g.circle(cx, cy, r.2 / 2.0, &paint(255, 255, 255, 0.12 * a));
        let p = paint(235, 238, 245, a);
        g.line(cx - 4.0, cy, cx + 4.0, cy, 1.8, &p);
        if plus {
            g.line(cx, cy - 4.0, cx, cy + 4.0, 1.8, &p);
        }
    }

    fn water_settings(&self, g: &mut Gfx, ox: f32, al: f32) {
        let wt = &self.water;
        let lbl = |g: &mut Gfx, s: &str, x: f32, row: usize| g.txt(s, x + ox, ROW_Y[row] + ty(ROW_H, 12.0), 12.0, GREY, al, LBL_W + 4.0);
        lbl(g, tr("Bottle"), LX, 0);
        lbl(g, tr("Glass"), GLASS_X, 0);
        lbl(g, tr("Goal"), LX, 1);
        lbl(g, tr("Remind"), LX, 2);
        lbl(g, tr("Hours"), LX, 3);
        g.txt(tr("to"), HOURS_END_X + LBL_W - 16.0 + ox, ROW_Y[3] + ty(ROW_H, 12.0), 12.0, GREY, al, 20.0);

        let val = |g: &mut Gfx, s: &str, cx: f32, row: usize| g.txt_c(s, cx + ox, ROW_Y[row] + ty(ROW_H, 13.0), 13.0, [240, 242, 248], al, VAL_W, true);
        val(g, &format!("{} ml", wt.bottle), stepper_rects(LX, 0.0).2, 0);
        val(g, &format!("{} ml", wt.glass), stepper_rects(GLASS_X, 0.0).2, 0);
        val(g, &format!("{:02}:00", wt.start_h), stepper_rects(LX, 0.0).2, 3);
        val(g, &format!("{:02}:00", wt.end_h), stepper_rects(HOURS_END_X, 0.0).2, 3);

        for (h, r) in set_hits() {
            match h {
                SetHit::BottleDn => Self::round_btn(g, r, false, ox, al, wt.bottle > 250),
                SetHit::BottleUp => Self::round_btn(g, r, true, ox, al, wt.bottle < 2000),
                SetHit::GlassDn => Self::round_btn(g, r, false, ox, al, wt.glass > 50),
                SetHit::GlassUp => Self::round_btn(g, r, true, ox, al, wt.glass < 1000),
                SetHit::StartDn => Self::round_btn(g, r, false, ox, al, wt.start_h > 0),
                SetHit::StartUp => Self::round_btn(g, r, true, ox, al, wt.start_h + 1 < wt.end_h),
                SetHit::EndDn => Self::round_btn(g, r, false, ox, al, wt.end_h > wt.start_h + 1),
                SetHit::EndUp => Self::round_btn(g, r, true, ox, al, wt.end_h < 24),
                SetHit::Goal(i) => g.chip(r.0 + ox, r.1, r.2, r.3, &format!("{} L", fmt_l(GOALS[i] as f32)), GOALS[i] == wt.goal, al),
                SetHit::Remind(i) => {
                    let label = if REMINDS[i] == 0 { tr("Off").to_string() } else { fill(tr("{} min"), &[&REMINDS[i]]) };
                    g.chip(r.0 + ox, r.1, r.2, r.3, &label, REMINDS[i] == wt.remind_min, al);
                }
                SetHit::Reset => {
                    g.pill(r.0 + ox, r.1, r.2, r.3, [255, 255, 255], 0.10 * al, 1.0);
                    g.txt_c(tr("Reset today"), r.0 + ox + r.2 / 2.0, r.1 + ty(r.3, 12.0), 12.0, [225, 228, 236], al, r.2, true);
                }
            }
        }

        // Kartu statistik hari ini: pengingat yang sudah muncul dan botol yang sudah habis.
        g.fill(&rrect(CARD.0 + ox, CARD.1, CARD.2, CARD.3, 14.0), &paint(255, 255, 255, 0.07 * al));
        let stat = |g: &mut Gfx, n: u32, cap: &str, y: f32, rgb: [u8; 3]| {
            g.txt_c(&n.to_string(), CARD.0 + ox + CARD.2 / 2.0, y, 19.0, rgb, al, CARD.2, true);
            g.txt_c(cap, CARD.0 + ox + CARD.2 / 2.0, y + 23.0, 11.0, GREY, al, CARD.2, false);
        };
        let c = g.acc.bright;
        stat(g, wt.reminders, tr("reminders"), CARD.1 + 6.0, c);
        stat(g, wt.refills(), tr("refills"), CARD.1 + 43.0, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_old_format() {
        let w = Water::parse("2026-10-24|750|2500|90\n", "2026-10-24");
        assert_eq!((w.ml, w.goal, w.remind_min, w.glass), (750, 2500, 90, 250));
        assert_eq!(w.hours.iter().sum::<u32>(), 0);
    }

    #[test]
    fn roundtrips_new_format() {
        let mut w = Water::parse("2026-10-24|0|2000|60", "2026-10-24");
        w.add(250, 9);
        w.add(500, 14);
        w.glass = 330;
        let r = Water::parse(&w.serialize(), "2026-10-24");
        assert_eq!((r.ml, r.glass, r.hours[9], r.hours[14]), (750, 330, 250, 500));
    }

    #[test]
    fn other_day_resets_ml_and_hours() {
        let w = Water::parse("2026-10-23|900|2000|60|250|0,0,0,0,0,0,0,0,0,900", "2026-10-24");
        assert_eq!((w.ml, w.hours[9]), (0, 0));
        assert_eq!(w.goal, 2000);
    }

    #[test]
    fn remove_never_goes_negative() {
        let mut w = Water::parse("2026-10-24|0|2000|60", "2026-10-24");
        w.add(250, 10);
        w.add(-250, 10);
        w.add(-250, 10);
        assert_eq!((w.ml, w.hours[10]), (0, 0));
    }

    #[test]
    fn chart_window_and_axis() {
        assert_eq!(Water::window_start(2), 0);
        assert_eq!(Water::window_start(14), 9);
        assert_eq!(Water::window_start(23), 16);
        let mut w = Water::parse("2026-10-24|0|2000|60", "2026-10-24");
        assert_eq!(w.axis_max(9), 400);
        w.hours[10] = 700;
        assert_eq!(w.axis_max(9), 800);
    }

    #[test]
    fn toggle_restores_last_interval() {
        let mut w = Water::parse("2026-10-24|0|2000|90", "2026-10-24");
        w.toggle_remind();
        assert_eq!(w.remind_min, 0);
        w.toggle_remind();
        assert_eq!(w.remind_min, 90);
    }

    #[test]
    fn new_fields_default_for_old_files_and_roundtrip() {
        let w = Water::parse("2026-10-24|750|2500|90|330|0,0,0,0,0,0,0,0,0,750", "2026-10-24");
        assert_eq!((w.bottle, w.start_h, w.end_h, w.reminders), (BOTTLE_DEFAULT, 8, 22, 0));
        let mut w = w;
        (w.bottle, w.start_h, w.end_h, w.reminders) = (850, 6, 15, 4);
        let r = Water::parse(&w.serialize(), "2026-10-24");
        assert_eq!((r.bottle, r.start_h, r.end_h, r.reminders, r.ml), (850, 6, 15, 4, 750));
        // hari lain: pengaturan tetap, hitungan hari ini nol
        let r = Water::parse(&w.serialize(), "2026-10-25");
        assert_eq!((r.bottle, r.start_h, r.end_h, r.reminders, r.ml), (850, 6, 15, 0, 0));
    }

    #[test]
    fn rejects_bad_window_and_bottle() {
        let w = Water::parse("2026-10-24|0|2000|60|250|0|50|20|10|3", "2026-10-24");
        assert_eq!((w.bottle, w.start_h, w.end_h), (BOTTLE_DEFAULT, 8, 22));
    }

    #[test]
    fn window_and_refills() {
        let mut w = Water::parse("2026-10-24|0|2000|60", "2026-10-24");
        (w.start_h, w.end_h, w.bottle, w.ml) = (6, 15, 800, 2100);
        assert!(!w.in_window(5) && w.in_window(6) && w.in_window(14) && !w.in_window(15));
        assert_eq!(w.refills(), 2);
    }

    #[test]
    fn reset_today_clears_progress_and_counters() {
        let mut w = Water::parse("2026-10-24|900|2000|60", "2026-10-24");
        w.reminders = 5;
        w.hours[9] = 900;
        w.reset_today();
        assert_eq!((w.ml, w.reminders, w.hours[9], w.refills()), (0, 0, 0, 0));
        assert_eq!(w.goal, 2000);
    }

    #[test]
    fn stepper_clamps() {
        assert_eq!(stepped(250, -1, 50, 250, 2000), 250);
        assert_eq!(stepped(2000, 1, 50, 250, 2000), 2000);
        assert_eq!(stepped(850, 1, 50, 250, 2000), 900);
        assert_eq!(stepped(5, 1, 1, 0, 0), 0);
    }

    #[test]
    fn settings_hits_do_not_overlap_and_fit_the_slide() {
        let hits = set_hits();
        for (i, (_, a)) in hits.iter().enumerate() {
            assert!(a.0 >= LX - 0.1 && a.0 + a.2 <= RX + 0.1 && a.1 >= 30.0 && a.1 + a.3 <= 156.0, "{a:?} di luar slide");
            for (_, b) in &hits[i + 1..] {
                let apart = a.0 + a.2 <= b.0 || b.0 + b.2 <= a.0 || a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1;
                assert!(apart, "{a:?} menimpa {b:?}");
            }
        }
        // kolom kiri tidak menyentuh kartu statistik
        assert!(hits.iter().filter(|(h, _)| *h != SetHit::Reset).all(|(_, r)| r.0 + r.2 <= CARD.0 - 4.0));
    }
}

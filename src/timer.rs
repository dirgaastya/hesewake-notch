//! Slide Timer (default tersembunyi): stopwatch dan hitung mundur dengan preset. Saat berjalan di notch kecil yang idle,
//! sisa/lama waktunya tampil kecil di samping jam; hitung mundur yang habis memunculkan toast "Time's up".

use super::*;

const GREEN: [u8; 3] = [60, 200, 110];
const GREY: [u8; 3] = TEXT_DIM;

/// Preset hitung mundur (detik): 1, 5, 10, 15, 25, 45 menit.
pub(crate) const PRESETS: [u32; 6] = [60, 300, 600, 900, 1500, 2700];
const MIN_PRESET: u32 = 60;
const MAX_PRESET: u32 = 3 * 3600;

// Tata letak notch idle saat timer berjalan (px). Isi disusun sebagai satu deret dengan jarak sama:
// [margin][cuaca][jam][pil][margin atau lonceng]; lebar notch mengikuti isi supaya kiri-kanan seimbang.
const PAD: f32 = 18.0;
const GAP: f32 = 12.0;
const CLOCK_W: f32 = 38.0;
const WEATHER_W: f32 = 24.0;
const PILL_W: f32 = 62.0;
const PILL_W_HOURS: f32 = 80.0;
/// Ruang di kanan pil bila ada lonceng (ikon 16 px + jarak).
const BELL_PAD: f32 = 36.0;
/// Fase mata: mata tetap di tengah, pil di kanannya pada jarak ini dari tengah.
const EYE_GAP: f32 = 24.0;

/// Posisi elemen idle saat timer berjalan, relatif ke tepi kiri notch (fase jam) atau ke tengah (fase mata).
pub(crate) struct IdleSlots {
    /// Lebar isi (tanpa slot titik mic/kamera): notch idle dilebarkan sampai ini bila lebih lebar dari `IDLE`.
    pub(crate) raw: f32,
    /// Fase jam: (pusat ikon cuaca bila tampil, pusat jam, awal pil), semuanya dari tepi kiri notch.
    clock: Option<(Option<f32>, f32, f32)>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum TMode {
    Stopwatch,
    Countdown,
    /// Automatic focus/break cycle on top of the same countdown engine.
    Pomodoro,
}

// Classic Pomodoro: 25 min focus, 5 min break, a 15 min long break after every 4 focus sessions.
const FOCUS_SECS: u32 = 25 * 60;
const SHORT_SECS: u32 = 5 * 60;
const LONG_SECS: u32 = 15 * 60;
const LONG_EVERY: u32 = 4;

const TOMATO: [u8; 3] = [255, 110, 90];

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Phase {
    Focus,
    Short,
    Long,
}

impl Phase {
    fn secs(self) -> u32 {
        match self {
            Phase::Focus => FOCUS_SECS,
            Phase::Short => SHORT_SECS,
            Phase::Long => LONG_SECS,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Phase::Focus => tr("Focus"),
            Phase::Short => tr("Short break"),
            Phase::Long => tr("Long break"),
        }
    }

    fn color(self) -> [u8; 3] {
        if self == Phase::Focus { TOMATO } else { GREEN }
    }
}

/// Next phase and the number of finished focus sessions. `completed` = the current phase ran out
/// (it was not skipped): a skipped focus session does not count and never triggers the long break.
fn next_phase(phase: Phase, done: u32, completed: bool) -> (Phase, u32) {
    match phase {
        Phase::Focus if completed => {
            let d = done + 1;
            (if d.is_multiple_of(LONG_EVERY) { Phase::Long } else { Phase::Short }, d)
        }
        Phase::Focus => (Phase::Short, done),
        Phase::Short | Phase::Long => (Phase::Focus, done),
    }
}

/// Timer state. One mechanism for every mode: `elapsed` = accumulated time + time since start.
pub(crate) struct TimerState {
    pub(crate) mode: TMode,
    /// Countdown length (seconds); Pomodoro uses its own phase lengths and leaves this alone.
    pub(crate) preset: u32,
    base: Duration,
    started: Option<Instant>,
    /// Detik yang terakhir ditampilkan; berubah = perlu redraw.
    shown: u64,
    /// Pomodoro: current phase and focus sessions finished since the cycle started (not saved to disk).
    pub(crate) phase: Phase,
    pub(crate) done: u32,
}

impl TimerState {
    pub(crate) fn new(mode: TMode, preset: u32) -> TimerState {
        TimerState {
            mode,
            preset: preset.clamp(MIN_PRESET, MAX_PRESET),
            base: Duration::ZERO,
            started: None,
            shown: u64::MAX,
            phase: Phase::Focus,
            done: 0,
        }
    }

    /// File format: `mode|preset_secs` (`s` = stopwatch, `c` = countdown, `p` = pomodoro).
    /// Bad content -> default (5 minute countdown).
    pub(crate) fn parse(s: &str) -> TimerState {
        let mut it = s.trim().split('|');
        let mode = match it.next() {
            Some("s") => TMode::Stopwatch,
            Some("p") => TMode::Pomodoro,
            _ => TMode::Countdown,
        };
        let preset = it.next().and_then(|v| v.parse().ok()).unwrap_or(300);
        TimerState::new(mode, preset)
    }

    pub(crate) fn serialize(&self) -> String {
        let m = match self.mode {
            TMode::Stopwatch => "s",
            TMode::Countdown => "c",
            TMode::Pomodoro => "p",
        };
        format!("{m}|{}\n", self.preset)
    }

    pub(crate) fn load() -> TimerState {
        TimerState::parse(&std::fs::read_to_string(data_path("timer.txt")).unwrap_or_default())
    }

    pub(crate) fn save(&self) {
        if cfg!(test) {
            return; // tes tidak boleh menimpa data asli di %APPDATA%
        }
        write_data("timer.txt", self.serialize());
    }

    pub(crate) fn running(&self) -> bool {
        self.started.is_some()
    }

    /// Countdown or Pomodoro (not stopwatch).
    pub(crate) fn counts_down(&self) -> bool {
        self.mode != TMode::Stopwatch
    }

    /// The countdown length in effect (seconds).
    pub(crate) fn duration(&self) -> u32 {
        if self.mode == TMode::Pomodoro { self.phase.secs() } else { self.preset }
    }

    fn total(&self) -> Duration {
        Duration::from_secs(self.duration() as u64)
    }

    /// Waktu yang sudah berjalan; pada hitung mundur dibatasi sampai durasinya.
    pub(crate) fn elapsed(&self, now: Instant) -> Duration {
        let e = self.base + self.started.map_or(Duration::ZERO, |s| now.saturating_duration_since(s));
        if self.counts_down() { e.min(self.total()) } else { e }
    }

    pub(crate) fn remaining(&self, now: Instant) -> Duration {
        self.total().saturating_sub(self.elapsed(now))
    }

    /// Hitung mundur sudah habis (mode stopwatch tidak pernah habis).
    pub(crate) fn finished(&self, now: Instant) -> bool {
        self.counts_down() && self.elapsed(now) >= self.total()
    }

    /// Detik yang ditampilkan: sisa (dibulatkan ke atas) untuk hitung mundur, lama berjalan untuk stopwatch.
    pub(crate) fn display_secs(&self, now: Instant) -> u64 {
        if self.counts_down() {
            let r = self.remaining(now);
            r.as_secs() + (r.subsec_nanos() > 0) as u64
        } else {
            self.elapsed(now).as_secs()
        }
    }

    pub(crate) fn start(&mut self, now: Instant) {
        if self.finished(now) {
            self.reset();
        }
        if self.started.is_none() {
            self.started = Some(now);
        }
    }

    pub(crate) fn pause(&mut self, now: Instant) {
        if let Some(s) = self.started.take() {
            self.base += now.saturating_duration_since(s);
        }
    }

    pub(crate) fn reset(&mut self) {
        self.base = Duration::ZERO;
        self.started = None;
        self.shown = u64::MAX;
    }

    pub(crate) fn set_mode(&mut self, m: TMode) {
        if self.mode != m {
            self.mode = m;
            self.reset_all();
        }
    }

    /// Reset button: zero the time; in Pomodoro also restart the cycle from the first focus session.
    pub(crate) fn reset_all(&mut self) {
        self.reset();
        self.phase = Phase::Focus;
        self.done = 0;
    }

    /// Move to the next Pomodoro phase. If the timer is running (or the phase ran out), the new phase starts
    /// right away; if paused, it waits for Start.
    pub(crate) fn advance(&mut self, now: Instant, completed: bool) {
        let go = self.running() || completed;
        (self.phase, self.done) = next_phase(self.phase, self.done, completed);
        self.reset();
        if go {
            self.start(now);
        }
    }

    pub(crate) fn set_preset(&mut self, secs: u32) {
        self.preset = secs.clamp(MIN_PRESET, MAX_PRESET);
        self.reset();
    }

    /// Geser durasi `delta` detik (kelipatan menit), dibatasi 1 menit sampai 3 jam.
    pub(crate) fn adjust(&mut self, delta: i32) {
        self.set_preset((self.preset as i64 + delta as i64).clamp(MIN_PRESET as i64, MAX_PRESET as i64) as u32);
    }

    /// Hitung mundur habis: hentikan dan kunci di nol.
    fn finish(&mut self) {
        self.base = self.total();
        self.started = None;
    }
}

/// "04:59" atau "1:02:03" untuk 3600 detik ke atas.
pub(crate) fn fmt_clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

/// Toast title and body when Pomodoro phase `ended` runs out and `next` begins; `done` = finished focus sessions.
fn pomo_toast(ended: Phase, next: Phase, done: u32) -> (String, String) {
    match ended {
        Phase::Focus => (fill(tr("Focus #{} done"), &[&done]), format!("{} \u{b7} {}", next.name(), fmt_preset(next.secs()))),
        Phase::Short | Phase::Long => (tr("Break over").into(), fill(tr("Focus \u{b7} {}"), &[&fmt_preset(next.secs())])),
    }
}

/// Pomodoro progress dots: how many of the `LONG_EVERY` dots are filled in this cycle.
fn pomo_dots(phase: Phase, done: u32) -> u32 {
    if phase == Phase::Long && done > 0 { LONG_EVERY } else { done % LONG_EVERY }
}

/// "5 min", "1 h", "1 h 30 min".
pub(crate) fn fmt_preset(secs: u32) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    match (h, m) {
        (0, m) => fill(tr("{} min"), &[&m]),
        (h, 0) => fill(tr("{} h"), &[&h]),
        (h, m) => fill(tr("{} h {} min"), &[&h, &m]),
    }
}

// ---------------------------------------------------------------- tata letak slide (koordinat kanvas)
#[derive(Clone, Copy, PartialEq, Debug)]
enum THit {
    Mode(TMode),
    Toggle,
    Reset,
    Preset(usize),
    Minus,
    Plus,
    /// Pomodoro: skip this phase.
    Skip,
}

const MODE_Y: f32 = 6.0;
const MODE_W: f32 = 84.0;
const TOGGLE: (f32, f32, f32, f32) = (LX, 116.0, 96.0, 32.0);
const RESET: (f32, f32, f32, f32) = (LX + 104.0, 116.0, 84.0, 32.0);
const PX: f32 = LX + 216.0;
const PRESET_W: f32 = 58.0;
const PRESET_H: f32 = 26.0;
const ADJ_W: f32 = 90.0;

fn t_hits(mode: TMode) -> Vec<(THit, (f32, f32, f32, f32))> {
    let mut v = vec![
        (THit::Mode(TMode::Stopwatch), (RX - 3.0 * MODE_W - 12.0, MODE_Y, MODE_W, 22.0)),
        (THit::Mode(TMode::Countdown), (RX - 2.0 * MODE_W - 6.0, MODE_Y, MODE_W, 22.0)),
        (THit::Mode(TMode::Pomodoro), (RX - MODE_W, MODE_Y, MODE_W, 22.0)),
        (THit::Toggle, TOGGLE),
        (THit::Reset, RESET),
    ];
    if mode == TMode::Pomodoro {
        v.push((THit::Skip, (PX, 106.0, 2.0 * ADJ_W + 8.0, PRESET_H)));
    }
    if mode == TMode::Countdown {
        for i in 0..PRESETS.len() {
            let (c, r) = ((i % 3) as f32, (i / 3) as f32);
            v.push((THit::Preset(i), (PX + c * (PRESET_W + 7.0), 40.0 + r * (PRESET_H + 6.0), PRESET_W, PRESET_H)));
        }
        v.push((THit::Minus, (PX, 106.0, ADJ_W, PRESET_H)));
        v.push((THit::Plus, (PX + ADJ_W + 8.0, 106.0, ADJ_W, PRESET_H)));
    }
    v
}

/// Pure layout: eye / clock phase, weather slot width (None = hidden), bell shown, wide pill (>= 1 h).
/// The weather slot gets wider when it also shows the prayer countdown.
fn idle_slots(eye: bool, weather: Option<f32>, bell: bool, wide: bool) -> IdleSlots {
    let pw = if wide { PILL_W_HOURS } else { PILL_W };
    let right = if bell { BELL_PAD } else { PAD };
    if eye {
        return IdleSlots { raw: 2.0 * (EYE_GAP + pw + right), clock: None };
    }
    let mut x = PAD;
    let weather_c = weather.map(|ww| {
        let c = x + ww / 2.0;
        x += ww + GAP;
        c
    });
    let clock_c = x + CLOCK_W / 2.0;
    x += CLOCK_W + GAP;
    IdleSlots { raw: x + pw + right, clock: Some((weather_c, clock_c, x)) }
}

/// Idle notch layout without a timer while the prayer countdown shows, using the same spacing:
/// [margin][label][clock][margin or bell]. The (missing) pill position is set to the clock's right edge.
/// With weather: [margin][weather][clock][label][margin or bell], the label where the timer pill would be.
fn prayer_layout(bell: bool, weather: bool) -> IdleSlots {
    let right = if bell { BELL_PAD } else { PAD };
    if weather {
        let weather_c = PAD + WEATHER_W / 2.0;
        let clock_c = PAD + WEATHER_W + GAP + CLOCK_W / 2.0;
        let label = clock_c + CLOCK_W / 2.0 + GAP;
        return IdleSlots { raw: label + prayer::LABEL_W + right, clock: Some((Some(weather_c), clock_c, label)) };
    }
    let label_c = PAD + prayer::LABEL_W / 2.0;
    let clock_c = PAD + prayer::LABEL_W + GAP + CLOCK_W / 2.0;
    let end = clock_c + CLOCK_W / 2.0;
    IdleSlots { raw: end + right, clock: Some((Some(label_c), clock_c, end)) }
}

impl App {
    fn timer_visible(&self) -> bool {
        self.expanded() && self.is_slide(SlideId::Timer) && self.h > 100.0
    }

    /// Slide terbuka dan timer berjalan: butuh frame terus-menerus (angka/ring bergerak).
    pub(crate) fn timer_animating(&self) -> bool {
        self.timer_visible() && self.tm.running()
    }

    /// Pil timer tampil di notch idle: timer berjalan dan notch kecil dalam keadaan diam.
    pub(crate) fn timer_chip_active(&self) -> bool {
        self.tm.running() && !self.expanded() && self.toast.is_none() && !self.thin() && !self.media.playing && self.splash_t().is_none()
    }

    /// Susunan notch idle saat timer berjalan; None bila pil tidak tampil.
    pub(crate) fn timer_slots(&self) -> Option<IdleSlots> {
        if !self.timer_chip_active() {
            return None;
        }
        let wide = self.tm.display_secs(Instant::now()) >= 3600;
        let weather = self.weather_shown().then(|| if self.countdown_on() { prayer::LABEL_W } else { WEATHER_W });
        Some(idle_slots(self.idle_phase() == IdlePhase::Eye, weather, self.bell_pos().is_some(), wide))
    }

    /// Tambahan lebar notch idle saat timer berjalan (di atas `IDLE`; slot titik mic/kamera ditambah terpisah).
    pub(crate) fn timer_extra(&self) -> f32 {
        self.timer_slots().map_or(0.0, |s| (s.raw - IDLE.0).max(0.0))
    }

    /// Idle notch layout while the prayer countdown shows without a timer pill; None otherwise.
    pub(crate) fn prayer_slots(&self) -> Option<IdleSlots> {
        let on = self.timer_slots().is_none() && self.weather_shown() && self.countdown_on();
        on.then(|| prayer_layout(self.bell_pos().is_some(), self.countdown_right()))
    }

    /// The prayer countdown sits right of the clock, so the weather stays on the left: there is weather to show and no timer
    /// pill wants that place. Otherwise the countdown takes turns with the weather in the slot left of the clock.
    pub(crate) fn countdown_right(&self) -> bool {
        self.countdown_on() && self.cfg.weather_on && weather::current().is_some() && !self.timer_chip_active()
    }

    /// Center of the countdown label right of the clock, if it sits there.
    pub(crate) fn idle_label_cx(&self, w: f32) -> Option<f32> {
        if !self.countdown_right() {
            return None;
        }
        let (_, _, label) = self.prayer_slots()?.clock?;
        Some(CX - w / 2.0 + label + prayer::LABEL_W / 2.0)
    }

    /// The idle notch layout in effect: timer pill, or prayer countdown.
    fn idle_layout(&self) -> Option<IdleSlots> {
        self.timer_slots().or_else(|| self.prayer_slots())
    }

    /// Clock center in the idle notch (shifts when the timer pill / prayer countdown shows next to it).
    pub(crate) fn idle_clock_cx(&self, w: f32) -> f32 {
        match self.idle_layout().and_then(|s| s.clock) {
            Some((_, c, _)) => CX - w / 2.0 + c,
            None => CX,
        }
    }

    /// Center of the weather icon/temperature (and prayer countdown) in the idle notch.
    pub(crate) fn idle_weather_cx(&self, w: f32) -> f32 {
        match self.idle_layout().and_then(|s| s.clock).and_then(|(wc, _, _)| wc) {
            Some(c) => CX - w / 2.0 + c,
            None => CX - w / 2.0 + 32.0,
        }
    }

    /// Dipanggil tiap poll: selesaikan hitung mundur (toast) dan minta redraw saat detik yang tampil berubah.
    pub(crate) fn timer_poll(&mut self) -> bool {
        let now = Instant::now();
        if self.tm.running() && self.tm.finished(now) {
            if self.tm.mode == TMode::Pomodoro {
                // phase ran out: announce the next phase and start it right away
                let ended = self.tm.phase;
                self.tm.advance(now, true);
                let (title, label) = pomo_toast(ended, self.tm.phase, self.tm.done);
                self.push_toast(toast::ToastKind::Timer { title, label });
                return true;
            }
            let label = fill(tr("{} timer"), &[&fmt_preset(self.tm.preset)]);
            self.tm.finish();
            self.push_toast(toast::ToastKind::Timer { title: tr("Time's up").into(), label });
            return true;
        }
        let s = self.tm.display_secs(now);
        if s != self.tm.shown {
            self.tm.shown = s;
            return self.tm.running();
        }
        false
    }

    pub(crate) fn timer_click(&mut self, x: f32, y: f32) {
        let now = Instant::now();
        let Some((h, _)) = t_hits(self.tm.mode).into_iter().find(|(_, r)| in_rect(x, y, r.0, r.1, r.2, r.3)) else { return };
        let running = self.tm.running();
        match h {
            THit::Mode(m) => self.tm.set_mode(m),
            THit::Toggle => {
                if running {
                    self.tm.pause(now);
                } else {
                    self.tm.start(now);
                }
            }
            THit::Reset => self.tm.reset_all(),
            THit::Skip => self.tm.advance(now, false),
            // durasi hanya bisa diubah saat tidak berjalan
            THit::Preset(i) if !running => self.tm.set_preset(PRESETS[i]),
            THit::Minus if !running => self.tm.adjust(-60),
            THit::Plus if !running => self.tm.adjust(60),
            _ => return,
        }
        self.tm.save();
    }

    pub(crate) fn slide_timer(&self, g: &mut Gfx, ox: f32, al: f32) {
        let tm = &self.tm;
        let now = Instant::now();
        let running = tm.running();
        let count = tm.counts_down();
        let pomo = tm.mode == TMode::Pomodoro;
        g.txtb("Timer", LX + ox, 10.0, 15.0, [255, 255, 255], al, 100.0);
        for (h, r) in t_hits(tm.mode) {
            match h {
                THit::Mode(m) => {
                    let label = match m {
                        TMode::Stopwatch => "Stopwatch",
                        TMode::Countdown => tr("Countdown##short"),
                        TMode::Pomodoro => "Pomodoro",
                    };
                    g.chip(r.0 + ox, r.1, r.2, r.3, label, m == tm.mode, al)
                }
                THit::Toggle => {
                    let label = if running { tr("Pause") } else if tm.elapsed(now) > Duration::ZERO && !tm.finished(now) { tr("Resume") } else { tr("Start") };
                    g.pill(r.0 + ox, r.1, r.2, r.3, if running { [255, 255, 255] } else { g.acc.dark }, if running { 0.14 } else { 0.85 } * al, 1.0);
                    g.txt_c(label, r.0 + ox + r.2 / 2.0, r.1 + ty(r.3, 14.0), 14.0, [225, 238, 255], al, r.2, true);
                }
                THit::Reset => {
                    g.pill(r.0 + ox, r.1, r.2, r.3, [255, 255, 255], 0.10 * al, 1.0);
                    g.txt_c("Reset", r.0 + ox + r.2 / 2.0, r.1 + ty(r.3, 13.0), 13.0, [225, 228, 236], al, r.2, true);
                }
                THit::Preset(i) => {
                    let label = format!("{} m", PRESETS[i] / 60);
                    g.chip(r.0 + ox, r.1, r.2, r.3, &label, PRESETS[i] == tm.preset, if running { al * 0.45 } else { al });
                }
                THit::Minus | THit::Plus => {
                    let label = if h == THit::Minus { tr("\u{2212} 1 min") } else { tr("+ 1 min") };
                    g.pill(r.0 + ox, r.1, r.2, r.3, [255, 255, 255], 0.10 * al, 1.0);
                    g.txt_c(label, r.0 + ox + r.2 / 2.0, r.1 + ty(r.3, 12.0), 12.0, [225, 228, 236], if running { al * 0.45 } else { al }, r.2, true);
                }
                THit::Skip => {
                    let label = if tm.phase == Phase::Focus { tr("Skip to break") } else { tr("Skip break") };
                    g.pill(r.0 + ox, r.1, r.2, r.3, [255, 255, 255], 0.10 * al, 1.0);
                    g.txt_c(label, r.0 + ox + r.2 / 2.0, r.1 + ty(r.3, 12.0), 12.0, [225, 228, 236], al, r.2, true);
                }
            }
        }

        // Pomodoro: cycle progress dots (4 focus sessions) and finished-session count, in the right column.
        if pomo {
            let filled = pomo_dots(tm.phase, tm.done);
            g.txt(tr("Sessions"), PX + ox, 40.0, 12.0, GREY, al, 120.0);
            for i in 0..LONG_EVERY {
                let (cx, cy) = (PX + ox + 8.0 + i as f32 * 24.0, 74.0);
                if i < filled {
                    g.circle(cx, cy, 7.0, &paint(TOMATO[0], TOMATO[1], TOMATO[2], al));
                } else if i == filled && tm.phase == Phase::Focus {
                    // the session in progress: a ring
                    let frac = tm.elapsed(now).as_secs_f32() / FOCUS_SECS as f32;
                    g.ring(cx, cy, 6.0, frac, 2.5, &paint(255, 255, 255, 0.16 * al), &paint(TOMATO[0], TOMATO[1], TOMATO[2], al));
                } else {
                    g.circle(cx, cy, 7.0, &paint(255, 255, 255, 0.12 * al));
                }
            }
            let total = fill(tr("{} done"), &[&tm.done]);
            g.txt(&total, PX + ox + 104.0, ty(0.0, 12.0) + 74.0, 12.0, GREY, al, 84.0);
        }

        // Angka besar (+ persepuluh detik untuk stopwatch) dan keterangan.
        let secs = tm.display_secs(now);
        let done = count && tm.finished(now);
        let col = if done { GREEN } else { [255, 255, 255] };
        let big = fmt_clock(secs);
        g.txtb(&big, LX + ox, 36.0, 44.0, col, al, 215.0);
        if !count {
            let w = g.width_ex(&big, 44.0, true);
            let tenth = tm.elapsed(now).subsec_millis() / 100;
            g.txtb(&format!(".{tenth}"), LX + ox + w + 2.0, 54.0, 20.0, GREY, al, 40.0);
        }
        let sub = match tm.mode {
            TMode::Pomodoro => format!("{} \u{b7} {}", tm.phase.name(), fmt_preset(tm.phase.secs())),
            TMode::Countdown if done => tr("Time's up").to_string(),
            TMode::Countdown => fill(tr("Countdown \u{b7} {}"), &[&fmt_preset(tm.preset)]),
            TMode::Stopwatch => "Stopwatch".to_string(),
        };
        let sub_col = if pomo { tm.phase.color() } else if done { GREEN } else { GREY };
        g.txt(&sub, LX + ox, 92.0, 12.0, sub_col, al, 200.0);

        // Stopwatch: ring yang menyapu sekali tiap menit di sisi kanan.
        if !count {
            let (cx, cy) = (PX + 94.0 + ox, 90.0);
            let frac = (tm.elapsed(now).as_secs_f32() % 60.0) / 60.0;
            g.ring(cx, cy, 36.0, frac, 5.0, &paint(255, 255, 255, 0.10 * al), &g.bright(al));
            g.txt_c(&format!("{:02}", tm.elapsed(now).as_secs() % 60), cx, cy - 10.0, 20.0, [235, 238, 245], al, 60.0, true);
        }
    }

    /// Pil waktu di kanan jam saat timer berjalan dan notch idle: ring progres + waktu, biru (tidak menyerupai mata).
    pub(crate) fn draw_timer_chip(&self, g: &mut Gfx, w: f32, h: f32, ca: f32) {
        // Check the layout, not `timer_extra`: without weather the row (148 px) fits in the IDLE width (150 px),
        // so the extra width is 0, but the pill must still show.
        if self.timer_slots().is_none() || ca <= 0.05 {
            return;
        }
        let now = Instant::now();
        let text = fmt_clock(self.tm.display_secs(now));
        let frac = if self.tm.counts_down() {
            self.tm.remaining(now).as_secs_f32() / self.tm.duration().max(1) as f32
        } else {
            (self.tm.elapsed(now).as_secs_f32() % 60.0) / 60.0
        };
        let tw = g.width_ex(&text, 12.0, true);
        let x = match self.timer_slots().and_then(|s| s.clock) {
            Some((_, _, px)) => CX - w / 2.0 + px,
            None => CX + EYE_GAP,
        };
        // Pomodoro: tomato red while focusing, green on a break; other modes blue.
        let (fg, bg) = if self.tm.mode == TMode::Pomodoro { (self.tm.phase.color(), self.tm.phase.color()) } else { (g.acc.bright, g.acc.dark) };
        let (pw, ph) = (24.0 + tw + 8.0, 20.0);
        g.fill(&rrect(x, h / 2.0 - ph / 2.0, pw, ph, ph / 2.0), &paint(bg[0], bg[1], bg[2], 0.22 * ca));
        g.ring(x + 12.0, h / 2.0, 5.5, frac.clamp(0.0, 1.0), 2.0, &paint(255, 255, 255, 0.18 * ca), &paint(fg[0], fg[1], fg[2], ca));
        g.txtb(&text, x + 24.0, ty(h, 12.0), 12.0, fg, ca, tw + 4.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn stopwatch_accumulates_across_pauses() {
        let t0 = Instant::now();
        let mut t = TimerState::new(TMode::Stopwatch, 300);
        t.start(at(t0, 0));
        assert!(t.running());
        t.pause(at(t0, 2500));
        assert_eq!(t.elapsed(at(t0, 9000)), Duration::from_millis(2500)); // berhenti saat dijeda
        t.start(at(t0, 10_000));
        assert_eq!(t.elapsed(at(t0, 11_000)), Duration::from_millis(3500));
        assert_eq!(t.display_secs(at(t0, 11_000)), 3);
        t.reset();
        assert!(!t.running() && t.elapsed(at(t0, 20_000)) == Duration::ZERO);
    }

    #[test]
    fn countdown_finishes_and_restarts() {
        let t0 = Instant::now();
        let mut t = TimerState::new(TMode::Countdown, 60);
        t.start(t0);
        assert_eq!(t.display_secs(at(t0, 0)), 60);
        assert_eq!(t.display_secs(at(t0, 1)), 60); // dibulatkan ke atas
        assert_eq!(t.display_secs(at(t0, 59_001)), 1);
        assert!(!t.finished(at(t0, 59_999)) && t.finished(at(t0, 60_000)));
        // elapsed tidak melewati durasi walau poll terlambat
        assert_eq!(t.remaining(at(t0, 90_000)), Duration::ZERO);
        t.finish();
        assert!(!t.running() && t.finished(at(t0, 100_000)) && t.display_secs(at(t0, 100_000)) == 0);
        // mulai lagi setelah habis: dari awal
        t.start(at(t0, 100_000));
        assert!(t.running() && !t.finished(at(t0, 100_000)) && t.display_secs(at(t0, 100_000)) == 60);
    }

    #[test]
    fn presets_clamp_and_reset_progress() {
        let t0 = Instant::now();
        let mut t = TimerState::new(TMode::Countdown, 300);
        t.start(t0);
        t.pause(at(t0, 5000));
        t.set_preset(600);
        assert_eq!((t.preset, t.elapsed(at(t0, 6000))), (600, Duration::ZERO));
        t.adjust(-60);
        assert_eq!(t.preset, 540);
        t.adjust(-100_000);
        assert_eq!(t.preset, MIN_PRESET);
        t.adjust(1_000_000);
        assert_eq!(t.preset, MAX_PRESET);
        t.set_preset(5);
        assert_eq!(t.preset, MIN_PRESET);
    }

    #[test]
    fn switching_mode_resets() {
        let t0 = Instant::now();
        let mut t = TimerState::new(TMode::Countdown, 300);
        t.start(t0);
        t.set_mode(TMode::Countdown); // mode sama: tidak mengganggu
        assert!(t.running());
        t.set_mode(TMode::Stopwatch);
        assert!(!t.running() && t.elapsed(at(t0, 1000)) == Duration::ZERO);
    }

    #[test]
    fn parse_serialize_and_garbage() {
        let t = TimerState::parse("s|900\n");
        assert_eq!((t.mode, t.preset), (TMode::Stopwatch, 900));
        let r = TimerState::parse(&t.serialize());
        assert_eq!((r.mode, r.preset), (TMode::Stopwatch, 900));
        let d = TimerState::parse("???");
        assert_eq!((d.mode, d.preset), (TMode::Countdown, 300));
        assert_eq!(TimerState::parse("c|99999999").preset, MAX_PRESET);
        let p = TimerState::parse("p|600");
        assert_eq!((p.mode, p.preset), (TMode::Pomodoro, 600));
        assert_eq!(TimerState::parse(&p.serialize()).mode, TMode::Pomodoro);
    }

    #[test]
    fn pomodoro_cycle_has_long_break_every_fourth() {
        let mut seq = Vec::new();
        let (mut p, mut d) = (Phase::Focus, 0);
        for _ in 0..16 {
            (p, d) = next_phase(p, d, true);
            seq.push(p);
        }
        use Phase::*;
        assert_eq!(seq, [Short, Focus, Short, Focus, Short, Focus, Long, Focus, Short, Focus, Short, Focus, Short, Focus, Long, Focus]);
        assert_eq!(d, 8);
        // a skipped focus session: not counted, always a short break
        assert_eq!(next_phase(Focus, 3, false), (Short, 3));
        assert_eq!(next_phase(Long, 4, false), (Focus, 4));
    }

    #[test]
    fn pomodoro_auto_advances_and_keeps_countdown_preset() {
        let t0 = Instant::now();
        let mut t = TimerState::new(TMode::Countdown, 600);
        t.set_mode(TMode::Pomodoro);
        assert_eq!((t.preset, t.duration(), t.phase), (600, FOCUS_SECS, Phase::Focus));
        t.start(t0);
        assert_eq!(t.display_secs(t0), FOCUS_SECS as u64);
        let end = at(t0, FOCUS_SECS as u64 * 1000);
        assert!(t.finished(end));
        t.advance(end, true);
        assert!(t.running() && t.phase == Phase::Short && t.done == 1);
        assert_eq!(t.display_secs(end), SHORT_SECS as u64);
        // skip while paused: change phase without starting
        t.pause(at(t0, FOCUS_SECS as u64 * 1000 + 1000));
        t.advance(at(t0, FOCUS_SECS as u64 * 1000 + 2000), false);
        assert!(!t.running() && t.phase == Phase::Focus && t.done == 1);
        // reset restarts the cycle; going back to Countdown keeps the old preset
        t.reset_all();
        assert_eq!((t.phase, t.done), (Phase::Focus, 0));
        t.set_mode(TMode::Countdown);
        assert_eq!(t.duration(), 600);
    }

    #[test]
    fn pomodoro_toast_and_dots() {
        assert_eq!(pomo_toast(Phase::Focus, Phase::Short, 1), ("Focus #1 done".to_string(), "Short break \u{b7} 5 min".to_string()));
        assert_eq!(pomo_toast(Phase::Focus, Phase::Long, 4).1, "Long break \u{b7} 15 min");
        assert_eq!(pomo_toast(Phase::Long, Phase::Focus, 4), ("Break over".to_string(), "Focus \u{b7} 25 min".to_string()));
        assert_eq!(pomo_dots(Phase::Focus, 0), 0);
        assert_eq!(pomo_dots(Phase::Short, 3), 3);
        assert_eq!(pomo_dots(Phase::Long, 4), 4);
        assert_eq!(pomo_dots(Phase::Focus, 4), 0);
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_clock(0), "00:00");
        assert_eq!(fmt_clock(299), "04:59");
        assert_eq!(fmt_clock(3723), "1:02:03");
        assert_eq!(fmt_preset(300), "5 min");
        assert_eq!(fmt_preset(3600), "1 h");
        assert_eq!(fmt_preset(5400), "1 h 30 min");
    }

    #[test]
    fn layout_fits_and_does_not_overlap() {
        for mode in [TMode::Stopwatch, TMode::Countdown, TMode::Pomodoro] {
            let hits = t_hits(mode);
            for (i, (_, a)) in hits.iter().enumerate() {
                assert!(a.0 >= LX - 0.1 && a.0 + a.2 <= RX + 0.1 && a.1 >= 0.0 && a.1 + a.3 <= 152.0, "{a:?} di luar slide");
                for (_, b) in &hits[i + 1..] {
                    let apart = a.0 + a.2 <= b.0 || b.0 + b.2 <= a.0 || a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1;
                    assert!(apart, "{a:?} menimpa {b:?}");
                }
            }
        }
    }

    #[test]
    fn idle_slots_are_evenly_spaced() {
        // cuaca + jam + pil: margin kiri = margin kanan, jarak antar elemen sama
        let s = idle_slots(false, Some(WEATHER_W), false, false);
        let (wc, cc, px) = s.clock.unwrap();
        let wc = wc.unwrap();
        assert!((wc - WEATHER_W / 2.0 - PAD).abs() < 0.01, "margin kiri");
        assert!(((cc - CLOCK_W / 2.0) - (wc + WEATHER_W / 2.0) - GAP).abs() < 0.01, "jarak cuaca-jam");
        assert!((px - (cc + CLOCK_W / 2.0) - GAP).abs() < 0.01, "jarak jam-pil");
        assert!((s.raw - (px + PILL_W) - PAD).abs() < 0.01, "margin kanan");
        // tanpa cuaca: jam menempel di margin kiri
        let n = idle_slots(false, None, false, false);
        let (w0, c0, _) = n.clock.unwrap();
        assert!(w0.is_none() && (c0 - CLOCK_W / 2.0 - PAD).abs() < 0.01);
        // lonceng menambah ruang kanan; pil jam lebih lebar
        assert!(idle_slots(false, Some(WEATHER_W), true, false).raw > s.raw);
        assert!(idle_slots(false, Some(WEATHER_W), false, true).raw > s.raw);
        // weather slot that also shows the prayer countdown: wider, same spacing
        let p = idle_slots(false, Some(72.0), false, false);
        let (pwc, pcc, ppx) = p.clock.unwrap();
        let pwc = pwc.unwrap();
        assert!((pwc - 36.0 - PAD).abs() < 0.01, "left margin of the wide slot");
        assert!(((pcc - CLOCK_W / 2.0) - (pwc + 36.0) - GAP).abs() < 0.01, "slot-clock gap");
        assert!((p.raw - (ppx + PILL_W) - PAD).abs() < 0.01, "right margin");
        assert!((p.raw - s.raw - (72.0 - WEATHER_W)).abs() < 0.01);
        // fase mata simetris terhadap tengah
        // prayer countdown without a timer: label + clock, left margin = right margin, same gap
        let q = prayer_layout(false, false);
        let (qlc, qcc, qend) = q.clock.unwrap();
        let qlc = qlc.unwrap();
        assert!((qlc - prayer::LABEL_W / 2.0 - PAD).abs() < 0.01, "left margin");
        assert!(((qcc - CLOCK_W / 2.0) - (qlc + prayer::LABEL_W / 2.0) - GAP).abs() < 0.01, "label-clock gap");
        assert!((q.raw - qend - PAD).abs() < 0.01, "right margin");
        assert!((prayer_layout(true, false).raw - q.raw - (BELL_PAD - PAD)).abs() < 0.01, "the bell adds room on the right");
        // with weather: weather, clock, label in the pill's place, same spacing
        let r = prayer_layout(false, true);
        let (rwc, rcc, rlx) = r.clock.unwrap();
        assert!((rwc.unwrap() - WEATHER_W / 2.0 - PAD).abs() < 0.01, "left margin");
        assert!((rlx - (rcc + CLOCK_W / 2.0) - GAP).abs() < 0.01, "clock-label gap");
        assert!((r.raw - (rlx + prayer::LABEL_W) - PAD).abs() < 0.01, "right margin");
        assert!((r.raw - idle_slots(false, Some(WEATHER_W), false, false).raw - (prayer::LABEL_W - PILL_W)).abs() < 0.01, "the label takes the pill's place");
        let e = idle_slots(true, None, false, false);
        assert!(e.clock.is_none() && (e.raw / 2.0 - (EYE_GAP + PILL_W + PAD)).abs() < 0.01);
    }
}

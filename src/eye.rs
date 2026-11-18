//! Animasi idle: siklus clock<->eye, splash awal, hotkey, mata berkedip + glow.

use super::*;

// Idle animation cycle: 10 min clock, then 15 min eye, loop.
const CLOCK_PHASE_SECS: f32 = 10.0 * 60.0;
const EYE_PHASE_SECS: f32 = 15.0 * 60.0;
pub(crate) const IDLE_CYCLE_SECS: f32 = CLOCK_PHASE_SECS + EYE_PHASE_SECS;
/// Hotkey that toggles clock <-> eye. A *bare* global \"H\" would swallow the letter in every app, so by
/// default it is Ctrl+Alt+H. Set to true for a bare global H (not recommended). A plain \"h\" keypress
/// is always handled when the Hesewake Notch window itself has keyboard focus.
pub(crate) const HOTKEY_PLAIN_H: bool = false;
pub(crate) const HOTKEY_ID: i32 = 1;
/// Eye + glow splash right after launch, then a cross-fade into the clock.
pub(crate) const SPLASH_SECS: f32 = 3.0;
pub(crate) const SPLASH_FADE: f32 = 0.35;

const EYE_FADE_IN: f32 = 0.45;
const EYE_FADE_OUT: f32 = 0.50;
/// Timer period while the eye rests (only the glow flows and the eye breathes): about 12 fps. Blinks and glances
/// run at 16 ms.
pub(crate) const EYE_REST_MS: u32 = 83;
/// A blink or glance due this soon switches to 16 ms frames first, so it starts as smoothly as before.
const EYE_SOON_SECS: f32 = 0.12;
/// A glance counts as moving until it is this close to its target and this slow: the rest of the spring's tail is
/// a tenth of a pixel, too small to see, so it runs at the resting rate.
const LOOK_SETTLE_PX: f32 = 0.1;
const LOOK_SETTLE_V: f32 = 1.0;
/// With no keyboard or mouse input for this long the eye stops moving (no frames at all) until input comes back.
pub(crate) const EYE_STILL_AFTER_SECS: f32 = 300.0;

// Eye geometry. The two shapes below are the eyes of eye.svg (viewBox 1000x1000), baked in as cubic
// beziers so nothing has to be parsed at runtime. The white plate of the SVG is just the bounding box.
const EYE_SVG_H: f32 = 348.83; // true height of both eyes together (SVG units)
const EYE_PIVOT: (f32, f32) = (499.98, 500.35); // centre of both eyes (SVG units)
const EYE_H_PX: f32 = 19.0; // on-screen height of the open eyes (notch is 32 px tall)

// Blink timeline (seconds), GSAP-style: anticipate -> close -> hold -> elastic open.
const BLINK_ANTICIPATE: f32 = 0.07;
const BLINK_CLOSE: f32 = 0.085;
const BLINK_HOLD: f32 = 0.05;
const BLINK_OPEN: f32 = 0.65;
const EYE_ANTICIPATE_SY: f32 = 1.10; // eyes widen slightly before the blink
const EYE_CLOSED_SY: f32 = 0.06; // closed = thin line
const EYE_SQUASH: f32 = 0.14; // horizontal squash & stretch (volume feel)
const EYE_DROP_PX: f32 = 1.0; // eyes sink a little while closing
const EYE_LOOK_MAX: f32 = 2.2; // glance range in px
const EYE_LOOK_K: f32 = 140.0;
const EYE_LOOK_C: f32 = 24.0;

// Glow: running palette of blue shades along X, fading to transparent upwards.
const GLOW_ALPHA: f32 = 0.85;
const GLOW_REACH: f32 = 0.95; // fraction of the notch height the glow climbs
const GLOW_FLOW: f32 = 0.10; // palette cycles per second
const EYE_PAL: [[f32; 3]; 5] = [
    [90.0, 150.0, 255.0],  // bright blue
    [40.0, 110.0, 235.0],  // royal blue
    [130.0, 190.0, 255.0], // light sky blue
    [20.0, 70.0, 190.0],   // deep blue
    [70.0, 140.0, 255.0],  // blue
];
type Pal = [[f32; 3]; 5];

// Moods. The pose eases toward the mood's target (`EyeAnim::update`); the glow palette and strength do the same.
/// The user is away this long (no keyboard or mouse): the eye gets sleepy.
pub(crate) const EYE_SLEEPY_AFTER_SECS: f32 = 120.0;
/// After a notification-like toast ends the eye is surprised this long; after a good-news toast it is happy this long.
const EYE_SURPRISE_SECS: f32 = 1.2;
const EYE_HAPPY_SECS: f32 = 2.6;
const EYE_LOOK_Y_MAX: f32 = 0.8; // vertical glance range in px
const EYE_FOLLOW_X: f32 = 4.0; // glance range while following the cursor (wider than the random glances)
const EYE_FOLLOW_Y: f32 = 1.6;
const EYE_SLEEPY_HOLD: f32 = 0.30; // a sleepy blink stays closed longer
const EYE_ARC_W: f32 = 2.8; // stroke of the happy "^ ^" eyes
const EYE_BOX: f32 = 34.0; // logical box around one eye for the lid cut

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum EyeMood {
    Normal,
    Sleepy,
    Happy,
    Focus,
    Surprised,
}

/// How the eyes look in a mood. `lid`: fraction of the eye hidden from the top, `tilt`: how much lower the lid is on the
/// inner side (focused look), `hap`: 0..1 cross-fade to the happy arcs, `sc`: size, `op`: how open (height).
#[derive(Clone, Copy)]
struct Pose {
    lid: f32,
    tilt: f32,
    hap: f32,
    sc: f32,
    op: f32,
}

impl Pose {
    fn ease_to(&mut self, t: &Pose, k: f32) {
        self.lid += (t.lid - self.lid) * k;
        self.tilt += (t.tilt - self.tilt) * k;
        self.hap += (t.hap - self.hap) * k;
        self.sc += (t.sc - self.sc) * k;
        self.op += (t.op - self.op) * k;
    }

    fn near(&self, t: &Pose) -> bool {
        const E: f32 = 0.004;
        (self.lid - t.lid).abs() < E && (self.tilt - t.tilt).abs() < E && (self.hap - t.hap).abs() < E && (self.sc - t.sc).abs() < E && (self.op - t.op).abs() < E
    }
}

impl EyeMood {
    fn pose(self) -> Pose {
        let p = |lid, tilt, hap, sc, op| Pose { lid, tilt, hap, sc, op };
        match self {
            EyeMood::Normal => p(0.0, 0.0, 0.0, 1.0, 1.0),
            EyeMood::Sleepy => p(0.36, -0.1, 0.0, 0.96, 0.92),
            EyeMood::Happy => p(0.0, 0.0, 1.0, 1.0, 1.0),
            EyeMood::Focus => p(0.04, 1.5, 0.0, 1.0, 1.0),
            EyeMood::Surprised => p(0.0, 0.0, 0.0, 1.2, 1.08),
        }
    }

    /// Glow strength and palette.
    fn glow(self) -> (f32, Pal) {
        match self {
            EyeMood::Normal => (1.0, EYE_PAL),
            EyeMood::Sleepy => (0.5, [[70.0, 90.0, 170.0], [50.0, 65.0, 135.0], [95.0, 115.0, 185.0], [40.0, 55.0, 115.0], [62.0, 82.0, 155.0]]),
            EyeMood::Happy => (1.0, [[70.0, 210.0, 140.0], [40.0, 180.0, 120.0], [120.0, 230.0, 170.0], [30.0, 150.0, 110.0], [60.0, 200.0, 150.0]]),
            EyeMood::Focus => (1.0, [[150.0, 110.0, 255.0], [110.0, 70.0, 230.0], [180.0, 150.0, 255.0], [80.0, 50.0, 190.0], [130.0, 100.0, 245.0]]),
            EyeMood::Surprised => (1.0, [[255.0, 190.0, 80.0], [255.0, 150.0, 50.0], [255.0, 220.0, 130.0], [230.0, 120.0, 40.0], [255.0, 170.0, 70.0]]),
        }
    }
}

/// Glance toward the cursor at (`x`, `y`) in canvas coordinates; None when it is not over the canvas.
pub(crate) fn follow_look(x: f32, y: f32, h: f32) -> Option<(f32, f32)> {
    if !(0.0..W as f32).contains(&x) || !(0.0..H as f32).contains(&y) {
        return None;
    }
    // tanh: the eyes turn quickly while the cursor is close and ease into the end of their range, with no hard stop
    Some((((x - CX) / 70.0).tanh() * EYE_FOLLOW_X, ((y - h / 2.0) / 45.0).tanh() * EYE_FOLLOW_Y))
}

/// Which mood wins: a short event (surprise, then happy) over a lasting state (focus work, then away).
pub(crate) fn pick_mood(surprised: bool, happy: bool, focus: bool, input_idle_secs: f32) -> EyeMood {
    if surprised {
        EyeMood::Surprised
    } else if happy {
        EyeMood::Happy
    } else if focus {
        EyeMood::Focus
    } else if input_idle_secs >= EYE_SLEEPY_AFTER_SECS {
        EyeMood::Sleepy
    } else {
        EyeMood::Normal
    }
}

/// Centre and size of each eye in SVG units (same order as `build_eye_paths`), and which side is the inner one.
struct EyeGeo {
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
    inner: f32,
}
const EYE_GEO: [EyeGeo; 2] = [
    EyeGeo { cx: 337.4, cy: 498.1, w: 254.8, h: 340.6, inner: 1.0 },
    EyeGeo { cx: 705.3, cy: 508.1, w: 169.6, h: 182.1, inner: -1.0 },
];

// ---------------------------------------------------------------- idle animation
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdlePhase {
    Clock,
    Eye,
}

// GSAP-like easings.
fn ease_out_quad(u: f32) -> f32 {
    1.0 - (1.0 - u) * (1.0 - u)
}

/// elastic.out(1, 0.42): overshoots ~25% then wobbles to rest -> the "bouncy" pop.
fn elastic_out(u: f32) -> f32 {
    if u <= 0.0 {
        0.0
    } else if u >= 1.0 {
        1.0
    } else {
        let p = 0.42;
        2f32.powf(-10.0 * u) * ((u - p / 4.0) * std::f32::consts::TAU / p).sin() + 1.0
    }
}

/// Procedural eye: keyframed blink timeline (anticipate, close, hold, elastic open), squash & stretch,
/// random double blinks, breathing and small glances driven by a critically damped spring.
pub(crate) struct EyeAnim {
    pub(crate) age: f32,
    pub(crate) wait: f32,
    pub(crate) blinking: bool,
    pub(crate) bt: f32,
    pub(crate) hold: f32,
    pub(crate) double_next: bool,
    pub(crate) sy: f32,
    pub(crate) sx: f32,
    pub(crate) dy: f32,
    pub(crate) look: f32,
    pub(crate) look_v: f32,
    pub(crate) look_to: f32,
    pub(crate) look_wait: f32,
    pub(crate) boost: f32,
    pub(crate) rng: u64,
    pub(crate) look_y: f32,
    look_y_to: f32,
    /// Where the eyes look while the cursor is near: (x, y) glance in px (`follow_look`); None = glance around by themselves.
    pub(crate) follow: Option<(f32, f32)>,
    /// Mood to ease toward (set by `App::eye_mood_step`), the pose, glow strength and palette right now.
    pub(crate) mood: EyeMood,
    pose: Pose,
    gs: f32,
    pal: Pal,
    /// A toast is showing: whether it is good news. When it ends the eye reacts (surprised or happy for a moment).
    pending_toast: Option<bool>,
    pub(crate) surprised_until: Option<Instant>,
    pub(crate) happy_until: Option<Instant>,
    /// The daily water goal was just reached: the eye turns happy once the notch is small again (`seen_goal`: the one already counted).
    pending_goal: bool,
    seen_goal: Option<Instant>,
}

impl EyeAnim {
    /// Starts with closed eyes that pop open with an elastic bounce.
    pub(crate) fn new() -> Self {
        let mut e = Self {
            age: 0.0,
            wait: 0.0,
            blinking: true,
            bt: BLINK_ANTICIPATE + BLINK_CLOSE,
            hold: 0.32,
            double_next: false,
            sy: EYE_CLOSED_SY,
            sx: 1.0,
            dy: 0.0,
            look: 0.0,
            look_v: 0.0,
            look_to: 0.0,
            look_wait: 2.0,
            boost: 0.0,
            rng: 0x9E37_79B9_7F4A_7C15,
            look_y: 0.0,
            look_y_to: 0.0,
            follow: None,
            mood: EyeMood::Normal,
            pose: EyeMood::Normal.pose(),
            gs: 1.0,
            pal: EYE_PAL,
            pending_toast: None,
            surprised_until: None,
            happy_until: None,
            pending_goal: false,
            seen_goal: None,
        };
        e.set_pose(EYE_CLOSED_SY);
        e
    }

    fn rand(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.rng >> 33) as f32) / ((1u64 << 31) as f32)
    }

    pub(crate) fn total(&self) -> f32 {
        BLINK_ANTICIPATE + BLINK_CLOSE + self.hold + BLINK_OPEN
    }

    fn sy_at(&self, bt: f32) -> f32 {
        let (a, c) = (BLINK_ANTICIPATE, BLINK_CLOSE);
        if bt < a {
            1.0 + (EYE_ANTICIPATE_SY - 1.0) * ease_out_quad(bt / a)
        } else if bt < a + c {
            let u = (bt - a) / c;
            EYE_ANTICIPATE_SY + (EYE_CLOSED_SY - EYE_ANTICIPATE_SY) * u * u * u
        } else if bt < a + c + self.hold {
            EYE_CLOSED_SY
        } else {
            let u = ((bt - a - c - self.hold) / BLINK_OPEN).min(1.0);
            EYE_CLOSED_SY + (1.0 - EYE_CLOSED_SY) * elastic_out(u)
        }
    }

    fn set_pose(&mut self, sy: f32) {
        self.sy = sy;
        self.sx = (1.0 + EYE_SQUASH * (1.0 - sy)).clamp(0.90, 1.15);
        self.dy = ((1.0 - sy) * EYE_DROP_PX).clamp(-1.5, 1.2);
        self.boost = ((sy - 1.0).max(0.0) * 1.5).min(0.5);
    }

    /// True while something visibly moves (needs the fast 16 ms timer).
    pub(crate) fn active(&self) -> bool {
        self.moving(LOOK_SETTLE_PX, LOOK_SETTLE_V)
    }

    /// Blinking, fading in, or a glance not yet within `px` of its target (or faster than `v` px/s).
    fn moving(&self, px: f32, v: f32) -> bool {
        self.blinking
            || self.age < EYE_FADE_IN + 0.1
            || (self.look - self.look_to).abs() > px
            || (self.look_y - self.look_y_to).abs() > px
            || self.look_v.abs() > v
            || !self.mood_settled()
    }

    /// The pose, glow strength and palette have reached the current mood.
    fn mood_settled(&self) -> bool {
        let (gs, pal) = self.mood.glow();
        self.pose.near(&self.mood.pose()) && (self.gs - gs).abs() < 0.004 && self.pal.iter().zip(pal.iter()).all(|(a, b)| (0..3).all(|i| (a[i] - b[i]).abs() < 0.5))
    }

    /// A blink or a glance starts within `EYE_SOON_SECS`.
    fn soon(&self) -> bool {
        !self.blinking && (self.wait < EYE_SOON_SECS || self.look_wait < EYE_SOON_SECS)
    }

    /// Timer period for the eye phase: 16 ms while (or just before) the eye moves, `EYE_REST_MS` while it rests,
    /// and 0 (no frames) once nobody has used the keyboard or mouse for `EYE_STILL_AFTER_SECS`. A blink that is
    /// already running finishes first, so the eye never stops half closed.
    pub(crate) fn frame_ms(&self, input_idle_secs: f32) -> u32 {
        if self.active() {
            16
        } else if input_idle_secs >= EYE_STILL_AFTER_SECS {
            0
        } else if self.soon() {
            16
        } else {
            EYE_REST_MS
        }
    }

    fn fade_in(&self) -> f32 {
        let u = (self.age / EYE_FADE_IN).clamp(0.0, 1.0);
        u * u * (3.0 - 2.0 * u)
    }

    pub(crate) fn update(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        self.age += dt;
        let n = ((dt / 0.008).ceil() as i32).max(1);
        let sdt = dt / n as f32;
        for _ in 0..n {
            self.look_v += (EYE_LOOK_K * (self.look_to - self.look) - EYE_LOOK_C * self.look_v) * sdt;
            self.look += self.look_v * sdt;
        }
        self.look_y += (self.look_y_to - self.look_y) * (1.0 - (-dt * 10.0).exp());
        let k = 1.0 - (-dt * if self.mood == EyeMood::Surprised { 20.0 } else { 8.0 }).exp();
        self.pose.ease_to(&self.mood.pose(), k);
        let (gs, pal) = self.mood.glow();
        let kg = 1.0 - (-dt * 5.0).exp();
        self.gs += (gs - self.gs) * kg;
        for (c, t) in self.pal.iter_mut().zip(pal.iter()) {
            for i in 0..3 {
                c[i] += (t[i] - c[i]) * kg;
            }
        }
        let sleepy = self.mood == EyeMood::Sleepy;
        if let Some((fx, fy)) = self.follow {
            (self.look_to, self.look_y_to) = (fx, fy);
            self.look_wait = 1.0; // when the cursor leaves, the eyes glance around again after a moment
        }
        if self.blinking {
            self.bt += dt;
            if self.bt >= self.total() {
                self.blinking = false;
                self.bt = 0.0;
                self.set_pose(1.0);
                if self.double_next {
                    self.double_next = false;
                    self.wait = 0.09 + self.rand() * 0.07;
                } else if sleepy {
                    self.wait = 4.0 + self.rand() * 3.0;
                } else {
                    self.wait = 2.4 + self.rand() * 3.8;
                    if self.rand() < 0.16 {
                        self.double_next = true;
                    }
                }
            } else {
                let sy = self.sy_at(self.bt);
                self.set_pose(sy);
            }
        } else {
            self.wait -= dt;
            self.set_pose(1.0 + 0.012 * (self.age * 2.1).sin());
            self.look_wait -= dt;
            if self.look_wait <= 0.0 && self.follow.is_none() {
                let calm = if sleepy { 0.3 } else { 1.0 };
                self.look_to = if self.rand() < 0.65 { (self.rand() * 2.0 - 1.0) * EYE_LOOK_MAX * calm } else { 0.0 };
                self.look_y_to = if self.rand() < 0.65 { (self.rand() * 2.0 - 1.0) * EYE_LOOK_Y_MAX * calm } else { 0.0 };
                self.look_wait = 1.8 + self.rand() * 3.2;
            }
            if self.wait <= 0.0 {
                self.blinking = true;
                self.bt = 0.0;
                self.hold = if sleepy { EYE_SLEEPY_HOLD } else { BLINK_HOLD };
            }
        }
    }
}

const EYE_L_START: (f32, f32) = (464.64, 478.34);
const EYE_L: [[f32; 6]; 10] = [
    [464.83, 528.40, 455.22, 571.76, 430.28, 610.90],
    [413.71, 636.90, 392.13, 657.52, 362.70, 668.39],
    [327.41, 681.39, 295.70, 674.21, 267.95, 649.49],
    [245.04, 629.05, 231.95, 602.39, 222.73, 573.65],
    [213.58, 545.58, 209.29, 516.16, 210.05, 486.65],
    [211.17, 447.32, 220.99, 410.65, 245.43, 379.05],
    [266.06, 352.36, 292.50, 333.79, 326.01, 327.77],
    [364.61, 320.85, 396.94, 333.64, 422.62, 363.03],
    [444.73, 388.33, 456.09, 418.56, 461.57, 451.22],
    [463.30, 461.52, 463.94, 472.00, 464.64, 478.34],
];

const EYE_R_START: (f32, f32) = (790.00, 503.06);
const EYE_R: [[f32; 6]; 6] = [
    [790.09, 547.33, 763.48, 585.25, 722.53, 599.15],
    [688.15, 610.82, 652.67, 596.64, 636.89, 563.92],
    [619.75, 528.41, 620.49, 492.83, 641.06, 458.92],
    [655.45, 435.22, 676.88, 420.53, 704.89, 417.07],
    [734.47, 413.41, 762.80, 428.86, 777.82, 456.07],
    [785.83, 470.43, 790.03, 486.61, 790.00, 503.06],
];

fn eye_path(start: (f32, f32), segs: &[[f32; 6]]) -> Path {
    let mut pb = PathBuilder::new();
    pb.move_to(start.0, start.1);
    for c in segs {
        pb.cubic_to(c[0], c[1], c[2], c[3], c[4], c[5]);
    }
    pb.close();
    pb.finish().unwrap_or_else(dummy_path)
}

pub(crate) fn build_eye_paths() -> Vec<Path> {
    vec![eye_path(EYE_L_START, &EYE_L), eye_path(EYE_R_START, &EYE_R)]
}

/// Kolom maksimum kanvas (lebar notch pada skala terbesar).
const GLOW_COLS: usize = W as usize * scale::MAX as usize + 2;

/// Running gradient behind the eye: a palette (shades of blue) flows along X while the
/// alpha fades to transparent towards the top. Written straight into the pixmap, limited to the notch
/// rectangle (about 150x32 px), no allocations, no shaders -> negligible CPU.
fn eye_glow(g: &mut Gfx, w: f32, h: f32, t: f32, strength: f32, pal: &Pal) {
    let strength = strength.clamp(0.0, 1.0);
    if strength <= 0.01 {
        return;
    }
    // Loop ini bekerja per piksel fisik; rumus warnanya tetap memakai koordinat logis (dibagi skala).
    let k = g.s;
    let pw = g.pm.width() as usize;
    let ph = g.pm.height() as usize;
    let x0 = ((CX - w / 2.0) * k).floor().max(0.0) as usize;
    let x1 = (((CX + w / 2.0) * k).ceil().max(0.0) as usize).min(pw).min(GLOW_COLS);
    let y1 = ((h * k).ceil().max(0.0) as usize).min(ph);
    if x1 <= x0 || y1 == 0 {
        return;
    }
    // Per-column colour and horizontal intensity (computed once, reused for every row).
    let mut cols = [[0.0f32; 4]; GLOW_COLS];
    let np = pal.len();
    for x in x0..x1 {
        let xf = (x as f32 + 0.5) / k;
        let pos = (xf - CX) * 0.0042 - t * GLOW_FLOW;
        let f = pos.rem_euclid(1.0) * np as f32;
        let i = (f.floor() as usize) % np;
        let fr = f - f.floor();
        let sm = fr * fr * (3.0 - 2.0 * fr);
        let (c0, c1) = (pal[i], pal[(i + 1) % np]);
        let hl = 0.78 + 0.22 * (std::f32::consts::TAU * (xf * 0.0065 - t * 0.35)).sin();
        cols[x] = [
            c0[0] + (c1[0] - c0[0]) * sm,
            c0[1] + (c1[1] - c0[1]) * sm,
            c0[2] + (c1[2] - c0[2]) * sm,
            hl,
        ];
    }
    let data = g.pm.data_mut();
    for y in 0..y1 {
        let d = (h - (y as f32 + 0.5) / k).max(0.0) / (h * GLOW_REACH);
        if d >= 1.0 {
            continue;
        }
        let v = 1.0 - d;
        let ra = strength * GLOW_ALPHA * v.powf(1.7);
        if ra <= 0.004 {
            continue;
        }
        let row = y * pw;
        for x in x0..x1 {
            let i = (row + x) * 4;
            let da = data[i + 3] as f32 * (1.0 / 255.0);
            if da <= 0.0 {
                continue;
            }
            let c = &cols[x];
            let a = ra * c[3];
            let inv = 1.0 - a;
            let k = a * da;
            data[i] = (data[i] as f32 * inv + c[0] * k) as u8;
            data[i + 1] = (data[i + 1] as f32 * inv + c[1] * k) as u8;
            data[i + 2] = (data[i + 2] as f32 * inv + c[2] * k) as u8;
        }
    }
}

/// Ends the startup splash once it is over: the idle cycle starts fresh at the clock phase (already
/// faded in, so the cross-fade stays smooth) and from now on only counts while idle.
/// Returns true when the splash just ended.
pub(crate) fn splash_tick(a: &mut App) -> bool {
    let Some(t0) = a.splash else { return false; };
    if t0.elapsed().as_secs_f32() < SPLASH_SECS {
        return false;
    }
    a.splash = None;
    // `--demo-eye`: langsung ke fase mata (uji tata letak notch idle tanpa menunggu 10 menit)
    let demo_eye = std::env::args().any(|x| x == "--demo-eye");
    a.idle_base = if demo_eye { CLOCK_PHASE_SECS + 1.0 } else { CLOCK_FADE_IN };
    a.idle_mark = if a.idle() { Some(Instant::now()) } else { None };
    a.cur_phase = if demo_eye { IdlePhase::Eye } else { IdlePhase::Clock };
    a.was_idle = a.idle();
    true
}

/// Manual toggle: jump the idle cycle to the start of the eye phase (if showing the clock) or to the
/// start of the clock phase (if showing the eye). The normal 10 min / 15 min loop continues from there.
pub(crate) unsafe fn toggle_idle_phase(a: &mut App) {
    if a.cfg.idle_display != 0 {
        return; // tampilan dikunci di Settings (Idle display)
    }
    a.splash = None;
    let to_eye = a.idle_phase() == IdlePhase::Clock;
    a.idle_base = if to_eye { CLOCK_PHASE_SECS } else { 0.0 };
    if a.idle_mark.is_some() {
        a.idle_mark = Some(Instant::now());
    }
    a.cur_phase = a.idle_phase();
    if to_eye {
        a.eye = EyeAnim::new();
    }
    a.last = Instant::now();
    present(a);
    schedule(a);
}

/// Draws one eye with its top part (the lid) cut away: the eye goes into a small scratch pixmap, the lid area is cleared there
/// (so the glow behind stays untouched) and the result is composited. `tr` is the eye transform, `(ex, ey)` its centre, `sy` the
/// vertical scale (px per SVG unit), `lid` the fraction hidden and `tilt` how much lower the lid sits on the inner side.
fn fill_eye_under_lid(g: &mut Gfx, path: &Path, tr: Transform, (ex, ey): (f32, f32), geo: &EyeGeo, sy: f32, lid: f32, tilt: f32, a: f32) {
    let s = g.s;
    let n = (EYE_BOX * s).ceil() as u32;
    let Some(mut sp) = Pixmap::new(n, n) else { return };
    let (ox, oy) = (((ex - EYE_BOX / 2.0) * s).floor(), ((ey - EYE_BOX / 2.0) * s).floor());
    sp.fill_path(path, &paint(255, 255, 255, 1.0), FillRule::Winding, tr.post_translate(-ox, -oy), None);
    let hgt = geo.h * sy;
    let top = ey - hgt / 2.0;
    let (xo, xi) = (ex - geo.inner * 30.0, ex + geo.inner * 30.0);
    let mut pb = PathBuilder::new();
    pb.move_to(xo, top - 40.0);
    pb.line_to(xi, top - 40.0);
    pb.line_to(xi, top + (lid + tilt * 0.35) * hgt);
    pb.line_to(xo, top + lid * hgt);
    pb.close();
    if let Some(lid_path) = pb.finish() {
        let mut clear = Paint::default();
        clear.blend_mode = BlendMode::Clear;
        clear.anti_alias = true;
        sp.fill_path(&lid_path, &clear, FillRule::Winding, g.ts.post_translate(-ox, -oy), None);
    }
    let pp = PixmapPaint { opacity: a.clamp(0.0, 1.0), ..PixmapPaint::default() };
    g.pm.draw_pixmap(ox as i32, oy as i32, sp.as_ref(), &pp, Transform::identity(), None);
}

impl App {
    /// Seconds spent idle inside the current clock+eye cycle. Wall-clock based (not tied to timer ticks),
    /// frozen while the notch is not idle.
    pub(crate) fn idle_secs(&self) -> f32 {
        self.idle_base + self.idle_mark.map_or(0.0, |m| m.elapsed().as_secs_f32())
    }

    /// Seconds since launch while the startup splash is still running.
    pub(crate) fn splash_t(&self) -> Option<f32> {
        self.splash.map(|t| t.elapsed().as_secs_f32()).filter(|&t| t < SPLASH_SECS)
    }

    pub(crate) fn idle_phase(&self) -> IdlePhase {
        // Pengaturan "Idle display": jam saja / mata saja menimpa siklus bergantian.
        match self.cfg.idle_display {
            1 => return IdlePhase::Clock,
            2 => return IdlePhase::Eye,
            _ => {}
        }
        let cycle_t = self.idle_secs() % IDLE_CYCLE_SECS;
        if cycle_t < CLOCK_PHASE_SECS {
            IdlePhase::Clock
        } else {
            IdlePhase::Eye
        }
    }

    /// The blinking eye from eye.svg over the running glow. Everything is cheap: two pre-built paths
    /// drawn with a transform, and the glow is computed analytically per pixel inside the notch only.
    pub(crate) fn draw_eye(&self, g: &mut Gfx, w: f32, h: f32, ca: f32) {
        let e = &self.eye;
        let out = match self.splash_t() {
            Some(st) => ((SPLASH_SECS - st) / SPLASH_FADE).clamp(0.0, 1.0),
            None => {
                let remaining = IDLE_CYCLE_SECS - (self.idle_secs() % IDLE_CYCLE_SECS);
                (remaining / EYE_FADE_OUT).clamp(0.0, 1.0)
            }
        };
        let al = ca * e.fade_in() * out;
        if al <= 0.01 {
            return;
        }

        eye_glow(g, w, h, self.t, al * e.gs * (1.0 + e.boost), &e.pal);

        let p = e.pose;
        let sleepy = e.mood == EyeMood::Sleepy;
        // A sleepy eye droops a little and its lid nods slowly.
        let lid = (p.lid + if sleepy { 0.1 * (0.5 + 0.5 * (e.age * 0.8).sin()) } else { 0.0 }).clamp(0.0, 0.9);
        let k = EYE_H_PX / EYE_SVG_H;
        let (sx, sy) = (k * e.sx * p.sc, k * e.sy * p.sc * p.op);
        let (cx, cy) = (CX + e.look, h / 2.0 + e.dy + e.look_y + if sleepy { 1.0 } else { 0.0 } - 1.2 * p.hap);
        let tr = g.ts.pre_concat(Transform::from_row(sx, 0.0, 0.0, sy, cx - EYE_PIVOT.0 * sx, cy - EYE_PIVOT.1 * sy));
        let eye_a = al * (1.0 - p.hap);
        let white = paint(255, 255, 255, eye_a);
        for (path, geo) in self.eye_paths.iter().zip(EYE_GEO.iter()) {
            let (ex, ey) = (cx + (geo.cx - EYE_PIVOT.0) * sx, cy + (geo.cy - EYE_PIVOT.1) * sy);
            if eye_a > 0.01 {
                if lid > 0.01 {
                    fill_eye_under_lid(g, path, tr, (ex, ey), geo, sy, lid, p.tilt, eye_a);
                } else {
                    g.pm.fill_path(path, &white, FillRule::Winding, tr, None);
                }
            }
            if p.hap > 0.01 {
                // happy: the eye turns into an arc "^"
                let hw = geo.w * sx * 0.5 * 0.85;
                let mut pb = PathBuilder::new();
                pb.move_to(ex - hw, ey + hw * 0.5);
                pb.quad_to(ex, ey - hw * 1.05, ex + hw, ey + hw * 0.5);
                if let Some(arc) = pb.finish() {
                    let stroke = Stroke { width: EYE_ARC_W, line_cap: LineCap::Round, ..Stroke::default() };
                    g.pm.stroke_path(&arc, &paint(255, 255, 255, al * p.hap), &stroke, g.ts, None);
                }
            }
        }
    }

    /// The glance toward the cursor, when it is over the notch canvas (and the moods are on in Settings).
    pub(crate) unsafe fn eye_follow_target(&self) -> Option<(f32, f32)> {
        if !self.cfg.eye_moods || self.menu_open {
            return None;
        }
        let mut p = POINT::default();
        GetCursorPos(&mut p).ok()?;
        follow_look((p.x - self.wx) as f32 / self.scale, (p.y - self.wy) as f32 / self.scale, self.h)
    }

    /// Settles the eye's mood for this frame. A toast makes no difference while it shows (the notch is open then); when it ends
    /// the eye reacts for a moment: happy after good news (timer done, download finished, charging), surprised after the rest.
    pub(crate) fn eye_mood_step(&mut self) {
        if !self.cfg.eye_moods {
            self.eye.mood = EyeMood::Normal;
            self.eye.pending_toast = None;
            return;
        }
        let now = Instant::now();
        match &self.toast {
            Some(t) => self.eye.pending_toast = Some(matches!(t.kind, ToastKind::Timer { .. } | ToastKind::Downloaded { .. } | ToastKind::Charging { .. })),
            None => {
                if let Some(good) = self.eye.pending_toast.take() {
                    if good {
                        self.eye.happy_until = Some(now + Duration::from_secs_f32(EYE_HAPPY_SECS));
                    } else {
                        self.eye.surprised_until = Some(now + Duration::from_secs_f32(EYE_SURPRISE_SECS));
                    }
                }
            }
        }
        // The water goal is reached with the Water slide open; the eye cheers once the notch has closed again.
        let goal = self.water_goal_t.filter(|t| t.elapsed().as_secs() < 30);
        if goal.is_some() && goal != self.eye.seen_goal {
            self.eye.seen_goal = goal;
            self.eye.pending_goal = true;
        }
        if self.eye.pending_goal && !self.expanded() && self.toast.is_none() {
            self.eye.pending_goal = false;
            self.eye.happy_until = Some(now + Duration::from_secs_f32(EYE_HAPPY_SECS));
        }
        let live = |u: Option<Instant>| u.is_some_and(|t| now < t);
        let (surprised, happy) = (live(self.eye.surprised_until), live(self.eye.happy_until));
        let focus = self.tm.mode == timer::TMode::Pomodoro && self.tm.running() && self.tm.phase == timer::Phase::Focus;
        let idle = if surprised || happy || focus { 0.0 } else { sys::input_idle_secs() };
        self.eye.mood = pick_mood(surprised, happy, focus, idle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open eyes in the middle of a rest: no blink or glance due for a while.
    fn resting() -> EyeAnim {
        let mut e = EyeAnim::new();
        for _ in 0..120 {
            e.update(1.0 / 60.0);
        }
        e.blinking = false;
        e.bt = 0.0;
        e.set_pose(1.0);
        (e.wait, e.look_wait) = (2.0, 2.0);
        (e.look, e.look_to, e.look_v) = (0.0, 0.0, 0.0);
        e
    }

    #[test]
    fn resting_eye_runs_at_about_12_fps() {
        let e = resting();
        assert!(!e.active());
        assert_eq!(e.frame_ms(0.0), EYE_REST_MS);
        assert!((70..=100).contains(&EYE_REST_MS), "10 to 14 fps");
    }

    #[test]
    fn blinks_and_glances_stay_at_60_fps() {
        let mut e = resting();
        e.wait = 0.1;
        assert_eq!(e.frame_ms(0.0), 16, "a blink is due: 60 fps before it starts");
        let mut e = resting();
        e.look_wait = 0.05;
        assert_eq!(e.frame_ms(0.0), 16, "a glance is due");
        let mut e = resting();
        e.wait = 0.0;
        e.update(1.0 / 60.0);
        assert!(e.blinking);
        assert_eq!(e.frame_ms(0.0), 16, "blinking");
    }

    /// Driven by its own timer from a rest, every blink frame is 16 ms, so the short anticipation (70 ms) is seen.
    #[test]
    fn blink_frames_are_short_even_coming_from_a_rest() {
        let mut e = resting();
        e.wait = 1.0;
        let (mut t, mut blink_frames, mut long) = (0.0, 0, 0);
        while t < 2.5 {
            let dt = e.frame_ms(0.0) as f32 / 1000.0;
            let was = e.blinking;
            e.update(dt);
            t += dt;
            if was {
                blink_frames += 1;
                if dt > 0.017 {
                    long += 1;
                }
            }
        }
        assert!(blink_frames > 20, "a blink happened ({blink_frames} frames)");
        assert_eq!(long, 0, "no blink frame at the rest rate");
    }

    /// A glance runs at 60 fps for as long as it visibly moves; what is left at the resting rate is under 0.15 px.
    #[test]
    fn glances_move_at_60_fps_until_the_motion_is_invisible() {
        let mut e = resting();
        e.look_to = EYE_LOOK_MAX;
        (e.wait, e.look_wait) = (100.0, 100.0); // only this glance, no blink or next glance meanwhile
        let mut slow_moves = 0.0f32;
        for _ in 0..200 {
            let ms = e.frame_ms(0.0);
            let before = e.look;
            e.update(ms as f32 / 1000.0);
            if ms != 16 {
                slow_moves += (e.look - before).abs();
            }
        }
        assert!((e.look - EYE_LOOK_MAX).abs() < 0.02, "the glance arrives");
        assert!(slow_moves < 0.15, "only {slow_moves} px of the glance is drawn at the resting rate");
    }

    /// Frames per second of the eye phase over 10 minutes of its own timeline (fixed random seed).
    fn simulated_fps(policy: impl Fn(&EyeAnim) -> u32) -> f32 {
        let mut e = EyeAnim::new();
        let (mut t, mut frames) = (0.0f32, 0u32);
        while t < 600.0 {
            let dt = policy(&e) as f32 / 1000.0;
            e.update(dt);
            t += dt;
            frames += 1;
        }
        frames as f32 / t
    }

    /// The old policy (40 ms at rest, glances moving until 0.02 px) gives the frame rate measured on a real laptop
    /// for issue #26 (36.8 fps), so the model holds. The new one gives about 26.6 fps: what is left is the blinks and
    /// the visible part of the glances, which stay at 60 fps.
    #[test]
    fn fewer_frames_than_before() {
        let old = simulated_fps(|e| if e.moving(0.02, 0.05) { 16 } else { 40 });
        let new = simulated_fps(|e| e.frame_ms(0.0));
        eprintln!("eye phase: old {old:.1} fps, new {new:.1} fps");
        assert!((31.0..43.0).contains(&old), "old policy {old:.1} fps (measured: 36.8)");
        assert!(new < old * 0.75, "new {new:.1} fps vs old {old:.1} fps");
    }

    #[test]
    fn eye_stops_when_nobody_is_there() {
        let e = resting();
        assert_eq!(e.frame_ms(EYE_STILL_AFTER_SECS + 1.0), 0, "away: no frames");
        assert_eq!(e.frame_ms(EYE_STILL_AFTER_SECS - 1.0), EYE_REST_MS);
        let mut e = resting();
        e.wait = 0.0;
        e.update(1.0 / 60.0);
        assert_eq!(e.frame_ms(EYE_STILL_AFTER_SECS + 1.0), 16, "a blink in progress finishes first");
    }

    #[test]
    fn the_eyes_follow_the_cursor_only_over_the_canvas() {
        assert_eq!(follow_look(-5.0, 10.0, 32.0), None);
        assert_eq!(follow_look(100.0, H as f32 + 1.0, 32.0), None);
        let (x, y) = follow_look(CX + 200.0, 16.0, 32.0).unwrap();
        assert!((x - EYE_FOLLOW_X).abs() < 0.05 && y.abs() < 1e-4, "far right, level: all the way right ({x}, {y})");
        let (x, y) = follow_look(CX - 30.0, 100.0, 32.0).unwrap();
        assert!(x < 0.0 && x > -EYE_FOLLOW_X && y > 0.9 * EYE_FOLLOW_Y && y < EYE_FOLLOW_Y, "left and below ({x}, {y})");
        // the range grows smoothly with the distance: no hard stop
        let step = |d: f32| follow_look(CX + d, 16.0, 32.0).unwrap().0;
        assert!(step(20.0) < step(60.0) && step(60.0) < step(120.0) && step(120.0) < step(240.0));
        assert!(step(20.0) > 0.0 && step(240.0) > EYE_LOOK_MAX, "reaches further than a random glance");
    }

    #[test]
    fn following_the_cursor_replaces_the_random_glances() {
        let mut e = resting();
        e.follow = Some((2.0, 1.0));
        e.look_wait = 0.0;
        for _ in 0..120 {
            e.update(1.0 / 60.0);
        }
        assert!((e.look - 2.0).abs() < 0.2 && (e.look_y - 1.0).abs() < 0.2, "looks at the cursor ({}, {})", e.look, e.look_y);
        e.follow = None;
        e.look_wait = 0.0;
        e.update(1.0 / 60.0);
        assert!(e.look_wait > 1.0, "glances around again once the cursor is gone");
    }

    #[test]
    fn a_short_event_beats_a_lasting_state() {
        assert_eq!(pick_mood(false, false, false, 0.0), EyeMood::Normal);
        assert_eq!(pick_mood(false, false, false, EYE_SLEEPY_AFTER_SECS), EyeMood::Sleepy);
        assert_eq!(pick_mood(false, false, true, EYE_SLEEPY_AFTER_SECS), EyeMood::Focus, "working is not sleeping");
        assert_eq!(pick_mood(false, true, true, 0.0), EyeMood::Happy);
        assert_eq!(pick_mood(true, true, true, EYE_SLEEPY_AFTER_SECS), EyeMood::Surprised);
    }

    #[test]
    fn the_mood_eases_in_and_then_costs_no_extra_frames() {
        let mut e = resting();
        assert!(!e.moving(LOOK_SETTLE_PX, LOOK_SETTLE_V), "normal mood at rest: nothing moves");
        e.mood = EyeMood::Happy;
        assert!(e.moving(LOOK_SETTLE_PX, LOOK_SETTLE_V), "a new mood is animated at full rate");
        let mut frames = 0;
        while !e.mood_settled() && frames < 600 {
            e.update(1.0 / 60.0);
            frames += 1;
        }
        assert!(frames < 240, "settled after {frames} frames");
        assert!(e.pose.hap > 0.99 && e.gs > 0.99);
        assert!((e.pal[0][1] - 210.0).abs() < 1.0, "glow palette turned green");
    }

    #[test]
    fn a_sleepy_eye_blinks_slowly_and_rarely() {
        let mut e = resting();
        e.mood = EyeMood::Sleepy;
        e.wait = 0.0;
        e.update(1.0 / 60.0);
        assert!(e.blinking);
        assert!(e.hold >= EYE_SLEEPY_HOLD);
        while e.blinking {
            e.update(1.0 / 60.0);
        }
        assert!(e.wait >= 4.0, "next blink in {} s", e.wait);
        assert!(!e.double_next);
    }
}

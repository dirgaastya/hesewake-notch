//! Ikon vektor kecil (dibangun dari path tiny-skia) untuk toast dan slide. Semua ikon digambar terpusat di (cx, cy)
//! dengan `s` = tinggi/lebar kotak ikon dalam piksel.

use super::*;

/// A drop (point at the top) centred on (cx, cy), `s` tall.
fn drop_path(cx: f32, cy: f32, s: f32) -> Option<Path> {
    let h = s / 2.0;
    let q = |x: f32, y: f32| (cx + x * h, cy + y * h);
    let mut pb = PathBuilder::new();
    let a = q(0.0, -1.0);
    pb.move_to(a.0, a.1);
    let (c1, c2, e) = (q(0.55, -0.35), q(0.7, 0.05), q(0.7, 0.3));
    pb.cubic_to(c1.0, c1.1, c2.0, c2.1, e.0, e.1);
    let (c1, c2, e) = (q(0.7, 0.7), q(0.38, 1.0), q(0.0, 1.0));
    pb.cubic_to(c1.0, c1.1, c2.0, c2.1, e.0, e.1);
    let (c1, c2, e) = (q(-0.38, 1.0), q(-0.7, 0.7), q(-0.7, 0.3));
    pb.cubic_to(c1.0, c1.1, c2.0, c2.1, e.0, e.1);
    let (c1, c2, e) = (q(-0.7, 0.05), q(-0.55, -0.35), q(0.0, -1.0));
    pb.cubic_to(c1.0, c1.1, c2.0, c2.1, e.0, e.1);
    pb.close();
    pb.finish()
}

fn path_of(pts: &[(f32, f32)], cx: f32, cy: f32, s: f32, close: bool) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for (i, &(x, y)) in pts.iter().enumerate() {
        let (px, py) = (cx + x * s / 2.0, cy + y * s / 2.0);
        if i == 0 {
            pb.move_to(px, py);
        } else {
            pb.line_to(px, py);
        }
    }
    if close {
        pb.close();
    }
    pb.finish()
}

impl Gfx {
    /// Petir (terisi).
    pub(crate) fn icon_bolt(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        let pts = [(0.15, -1.0), (-0.62, 0.12), (-0.06, 0.12), (-0.2, 1.0), (0.62, -0.18), (0.05, -0.18)];
        if let Some(path) = path_of(&pts, cx, cy, s, true) {
            self.fill(&path, p);
        }
    }

    /// Crescent: a bright circle minus a black one (the notch background is always black), like the night weather icon.
    pub(crate) fn icon_crescent(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        self.circle(cx, cy, s * 0.42, p);
        self.circle(cx + s * 0.22, cy - s * 0.12, s * 0.36, &paint(0, 0, 0, 1.0));
    }

    /// Rune Bluetooth (garis).
    pub(crate) fn icon_bluetooth(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        let pts = [(-0.5, -0.5), (0.5, 0.42), (0.0, 0.95), (0.0, -0.95), (0.5, -0.42), (-0.5, 0.5)];
        if let Some(path) = path_of(&pts, cx, cy, s, false) {
            let st = Stroke { width: (s * 0.13).max(1.2), line_cap: LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Default::default() };
            self.pm.stroke_path(&path, p, &st, self.ts, None);
        }
    }

    /// Headphone: pita setengah lingkaran + dua earcup.
    pub(crate) fn icon_headphone(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        let r = s * 0.36;
        let mut pts = Vec::new();
        for i in 0..=16 {
            let a = std::f32::consts::PI + std::f32::consts::PI * i as f32 / 16.0;
            pts.push((a.cos() * 0.72, 0.12 + a.sin() * 0.78));
        }
        if let Some(path) = path_of(&pts, cx, cy, s, false) {
            let st = Stroke { width: (s * 0.11).max(1.2), line_cap: LineCap::Round, ..Default::default() };
            self.pm.stroke_path(&path, p, &st, self.ts, None);
        }
        let (cw, ch) = (s * 0.2, s * 0.42);
        for sign in [-1.0f32, 1.0] {
            let x = cx + sign * r * 1.0 + if sign < 0.0 { -cw / 2.0 - s * 0.0 } else { -cw / 2.0 };
            self.fill(&rrect(x, cy + s * 0.02, cw, ch, cw / 2.0), p);
        }
    }

    /// Baterai (badan + kepala) terisi `frac` (0..1) dari kiri.
    pub(crate) fn icon_battery(&mut self, cx: f32, cy: f32, s: f32, frac: f32, p: &Paint) {
        let (w, h) = (s * 1.0, s * 0.56);
        let (x, y) = (cx - w / 2.0 - s * 0.04, cy - h / 2.0);
        let st = Stroke { width: (s * 0.09).max(1.0), ..Default::default() };
        self.pm.stroke_path(&rrect(x, y, w, h, h * 0.28), p, &st, self.ts, None);
        self.rect(x + w + s * 0.03, cy - h * 0.2, s * 0.07, h * 0.4, p);
        let pad = s * 0.1;
        let fw = (w - pad * 2.0) * frac.clamp(0.0, 1.0);
        if fw > 0.5 {
            self.fill(&rrect(x + pad, y + pad, fw, h - pad * 2.0, h * 0.14), p);
        }
    }

    /// Rewind (dua segitiga ke kiri).
    pub(crate) fn icon_prev(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        for dx in [-0.25f32, 0.25] {
            let pts = [(dx + 0.25, -0.45), (dx + 0.25, 0.45), (dx - 0.25, 0.0)];
            if let Some(path) = path_of(&pts, cx, cy, s * 2.0, true) {
                self.fill(&path, p);
            }
        }
    }

    /// Fast-forward (dua segitiga ke kanan).
    pub(crate) fn icon_next(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        for dx in [-0.25f32, 0.25] {
            let pts = [(dx - 0.25, -0.45), (dx - 0.25, 0.45), (dx + 0.25, 0.0)];
            if let Some(path) = path_of(&pts, cx, cy, s * 2.0, true) {
                self.fill(&path, p);
            }
        }
    }

    pub(crate) fn icon_pause(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        let (w, h) = (s * 0.26, s * 0.9);
        self.fill(&rrect(cx - s * 0.36, cy - h / 2.0, w, h, w * 0.3), p);
        self.fill(&rrect(cx + s * 0.1, cy - h / 2.0, w, h, w * 0.3), p);
    }

    pub(crate) fn icon_play(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        let pts = [(-0.34, -0.9), (-0.34, 0.9), (0.9, 0.0)];
        if let Some(path) = path_of(&pts, cx + s * 0.04, cy, s, true) {
            self.fill(&path, p);
        }
    }

    /// Balon percakapan (tombol mode lirik): `dots` = warna dua titik di dalamnya.
    pub(crate) fn icon_bubble(&mut self, cx: f32, cy: f32, s: f32, p: &Paint, dots: &Paint) {
        self.fill(&rrect(cx - s * 0.5, cy - s * 0.44, s, s * 0.72, s * 0.22), p);
        let pts = [(-0.4, 0.4), (-0.52, 0.98), (0.0, 0.4)];
        if let Some(path) = path_of(&pts, cx, cy, s * 0.9, true) {
            self.fill(&path, p);
        }
        self.circle(cx - s * 0.17, cy - s * 0.08, s * 0.075, dots);
        self.circle(cx + s * 0.17, cy - s * 0.08, s * 0.075, dots);
    }

    /// Pin (tombol sematkan): kepala bulat, badan, dan jarum; `on` = terisi.
    pub(crate) fn icon_pin(&mut self, cx: f32, cy: f32, s: f32, p: &Paint, on: bool) {
        let top = cy - s * 0.46;
        if on {
            self.fill(&rrect(cx - s * 0.3, top, s * 0.6, s * 0.2, s * 0.06), p);
            self.poly(&[(cx - s * 0.2, top + s * 0.2), (cx + s * 0.2, top + s * 0.2), (cx + s * 0.34, top + s * 0.56), (cx - s * 0.34, top + s * 0.56)], p);
        } else {
            let lw = (s * 0.12).max(1.2);
            self.line(cx - s * 0.3, top + s * 0.1, cx + s * 0.3, top + s * 0.1, lw, p);
            self.line(cx - s * 0.18, top + s * 0.1, cx - s * 0.3, top + s * 0.56, lw, p);
            self.line(cx + s * 0.18, top + s * 0.1, cx + s * 0.3, top + s * 0.56, lw, p);
            self.line(cx - s * 0.3, top + s * 0.56, cx + s * 0.3, top + s * 0.56, lw, p);
        }
        self.line(cx, top + s * 0.56, cx, cy + s * 0.52, (s * 0.1).max(1.2), p);
    }

    /// Matahari (ikon kecerahan): lingkaran + 8 sinar.
    pub(crate) fn icon_sun(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        self.circle(cx, cy, s * 0.2, p);
        for k in 0..8 {
            let a = std::f32::consts::TAU * k as f32 / 8.0;
            let (sn, cs) = a.sin_cos();
            self.line(cx + cs * s * 0.34, cy + sn * s * 0.34, cx + cs * s * 0.5, cy + sn * s * 0.5, (s * 0.09).max(1.1), p);
        }
    }

    /// Speaker; `level` 0..=3 gelombang suara, `muted` menggambar tanda silang.
    pub(crate) fn icon_speaker(&mut self, cx: f32, cy: f32, s: f32, level: u8, muted: bool, p: &Paint) {
        let pts = [(-0.62, -0.22), (-0.3, -0.22), (0.06, -0.52), (0.06, 0.52), (-0.3, 0.22), (-0.62, 0.22)];
        if let Some(path) = path_of(&pts, cx, cy, s, true) {
            self.fill(&path, p);
        }
        let st = Stroke { width: (s * 0.1).max(1.1), line_cap: LineCap::Round, ..Default::default() };
        if muted {
            self.line(cx + s * 0.22, cy - s * 0.2, cx + s * 0.62, cy + s * 0.2, (s * 0.1).max(1.1), p);
            self.line(cx + s * 0.62, cy - s * 0.2, cx + s * 0.22, cy + s * 0.2, (s * 0.1).max(1.1), p);
            return;
        }
        for k in 0..level.min(3) {
            let r = 0.28 + 0.2 * k as f32;
            let mut v = Vec::new();
            for j in 0..=8 {
                let a = -0.9 + 1.8 * j as f32 / 8.0;
                v.push((0.06 + r * a.cos(), r * a.sin()));
            }
            if let Some(path) = path_of(&v, cx, cy, s, false) {
                self.pm.stroke_path(&path, p, &st, self.ts, None);
            }
        }
    }

    /// Mikrofon: kapsul + lengkung penyangga + kaki.
    pub(crate) fn icon_mic(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        self.fill(&rrect(cx - s * 0.17, cy - s * 0.46, s * 0.34, s * 0.58, s * 0.17), p);
        let mut pts = Vec::new();
        for k in 0..=12 {
            let a = std::f32::consts::PI * k as f32 / 12.0;
            pts.push((a.cos() * 0.62, 0.02 + a.sin() * 0.5));
        }
        if let Some(path) = path_of(&pts, cx, cy, s, false) {
            let st = Stroke { width: (s * 0.1).max(1.1), line_cap: LineCap::Round, ..Default::default() };
            self.pm.stroke_path(&path, p, &st, self.ts, None);
        }
        self.rect(cx - s * 0.05, cy + s * 0.5, s * 0.1, s * 0.16, p);
    }

    /// Crossed-out microphone. A dark gap around the slash keeps it readable at small sizes.
    pub(crate) fn icon_mic_off(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        self.icon_mic(cx, cy, s, p);
        let lw = (s * 0.1).max(1.1);
        let (x1, y1, x2, y2) = (cx - s * 0.46, cy - s * 0.5, cx + s * 0.46, cy + s * 0.6);
        self.line(x1, y1, x2, y2, lw * 2.4, &paint(0, 0, 0, 1.0));
        self.line(x1, y1, x2, y2, lw, p);
    }

    /// Kamera: badan + "kepala" lensa segitiga di kanan.
    pub(crate) fn icon_camera(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        self.fill(&rrect(cx - s * 0.5, cy - s * 0.26, s * 0.66, s * 0.52, s * 0.12), p);
        let pts = [(0.26, -0.1), (0.78, -0.42), (0.78, 0.42), (0.26, 0.1)];
        if let Some(path) = path_of(&pts, cx, cy, s, true) {
            self.fill(&path, p);
        }
    }

    /// A drop of water in the accent's colours that still reads as water: a very light tint of `pale` (mixed with
    /// icy white-blue) at the top, a little more of `bright` at the bottom, and a white glint down the left side,
    /// which is what makes it look wet and fresh in any colour.
    pub(crate) fn icon_water_drop(&mut self, cx: f32, cy: f32, s: f32, pale: [u8; 3], bright: [u8; 3], a: f32) {
        let mix = |c: [u8; 3], d: [u8; 3], t: f32| -> [u8; 3] { std::array::from_fn(|i| (c[i] as f32 + (d[i] as f32 - c[i] as f32) * t).round() as u8) };
        let h = s / 2.0;
        let (top, bottom) = (mix(pale, [235, 250, 255], 0.35), mix(bright, pale, 0.35));
        if let Some(path) = drop_path(cx, cy, s) {
            self.fill(&path, &vgrad(cy - h, cy + h, top, bottom, a));
        }
        let glint = [(-0.42, 0.12), (-0.43, 0.36), (-0.33, 0.58), (-0.18, 0.7)];
        if let Some(path) = path_of(&glint, cx, cy, s, false) {
            let st = Stroke { width: (s * 0.09).max(1.1), line_cap: LineCap::Round, ..Default::default() };
            self.pm.stroke_path(&path, &paint(255, 255, 255, 0.85 * a), &st, self.ts, None);
        }
    }

    /// Lonceng (terisi); `swing` (-1..1) memiringkan lonceng sedikit untuk animasi goyang.
    pub(crate) fn icon_bell(&mut self, cx: f32, cy: f32, s: f32, swing: f32, p: &Paint) {
        let (sn, cs) = (swing * 0.35).sin_cos();
        let rot = |x: f32, y: f32| (cx + (x * cs - y * sn) * s / 2.0, cy + (x * sn + y * cs) * s / 2.0);
        let pts = [(-0.62, 0.42), (-0.46, 0.42), (-0.42, -0.1), (-0.26, -0.5), (0.0, -0.64), (0.26, -0.5), (0.42, -0.1), (0.46, 0.42), (0.62, 0.42), (0.62, 0.58), (-0.62, 0.58)];
        let mut pb = PathBuilder::new();
        for (i, &(x, y)) in pts.iter().enumerate() {
            let (px, py) = rot(x, y);
            if i == 0 {
                pb.move_to(px, py);
            } else {
                pb.line_to(px, py);
            }
        }
        pb.close();
        if let Some(path) = pb.finish() {
            self.fill(&path, p);
        }
        let (bx, by) = rot(0.0, 0.84);
        self.circle(bx, by, s * 0.1, p);
    }

    /// Tiga slider horizontal (ikon Settings).
    pub(crate) fn icon_sliders(&mut self, cx: f32, cy: f32, s: f32, p: &Paint) {
        let w = s * 0.11;
        for (y, k) in [(-0.55f32, -0.3f32), (0.0, 0.4), (0.55, -0.1)] {
            self.line(cx - s * 0.46, cy + y * s / 2.0 * 1.0, cx + s * 0.46, cy + y * s / 2.0 * 1.0, w, &with_alpha(p, 0.55));
            self.circle(cx + k * s, cy + y * s / 2.0, s * 0.15, p);
        }
    }

    /// Toggle: `knob` 0 (mati) .. 1 (nyala). Ikon bel kecil muncul di knob saat nyala.
    pub(crate) fn toggle(&mut self, x: f32, y: f32, w: f32, h: f32, knob: f32, swing: f32, al: f32) {
        let k = knob.clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * k;
        let on = self.acc.deep.map(f32::from);
        let track = paint(lerp(62.0, on[0]) as u8, lerp(62.0, on[1]) as u8, lerp(68.0, on[2]) as u8, al);
        self.fill(&rrect(x, y, w, h, h / 2.0), &track);
        let r = h / 2.0 - 3.0;
        let kx = x + h / 2.0 + (w - h) * knob;
        self.circle(kx, y + h / 2.0, r, &paint(255, 255, 255, al));
        if k > 0.3 {
            self.icon_bell(kx, y + h / 2.0 - 0.5, r * 1.15, swing, &paint(on[0] as u8, on[1] as u8, on[2] as u8, al * ((k - 0.3) / 0.7).clamp(0.0, 1.0)));
        }
    }

    /// Pill/tombol: isi `fill` (RGBA 0..1 sudah termasuk alpha), `scale` untuk efek tekan.
    pub(crate) fn pill(&mut self, x: f32, y: f32, w: f32, h: f32, rgb: [u8; 3], a: f32, scale: f32) {
        let (nw, nh) = (w * scale, h * scale);
        self.fill(&rrect(x + (w - nw) / 2.0, y + (h - nh) / 2.0, nw, nh, nh / 2.0), &paint(rgb[0], rgb[1], rgb[2], a));
    }

    /// Chip pilihan di Settings.
    pub(crate) fn chip(&mut self, x: f32, y: f32, w: f32, h: f32, label: &str, on: bool, al: f32) {
        if on {
            let (fill, text) = (self.acc.bright, self.acc.pale);
            self.fill(&rrect(x, y, w, h, h / 2.0), &paint(fill[0], fill[1], fill[2], 0.28 * al));
            self.txt_c(label, x + w / 2.0, y + ty(h, 12.0), 12.0, text, al, w, true);
        } else {
            self.fill(&rrect(x, y, w, h, h / 2.0), &paint(255, 255, 255, 0.08 * al));
            self.txt_c(label, x + w / 2.0, y + ty(h, 12.0), 12.0, [170, 170, 176], al, w, false);
        }
    }
}

fn with_alpha(p: &Paint, k: f32) -> Paint<'static> {
    let mut q = Paint::default();
    if let tiny_skia::Shader::SolidColor(c) = &p.shader {
        let mut c = *c;
        c.apply_opacity(k);
        q.shader = tiny_skia::Shader::SolidColor(c);
    }
    q.anti_alias = true;
    q
}

/// Gradien vertikal y0 -> y1 (c0 di atas, c1 di bawah).
pub(crate) fn vgrad(y0: f32, y1: f32, c0: [u8; 3], c1: [u8; 3], a: f32) -> Paint<'static> {
    let col = |c: [u8; 3]| Color::from_rgba8(c[0], c[1], c[2], (a.clamp(0.0, 1.0) * 255.0) as u8);
    let mut p = Paint::default();
    if let Some(sh) = LinearGradient::new(
        Point::from_xy(0.0, y0),
        Point::from_xy(0.0, y1),
        vec![GradientStop::new(0.0, col(c0)), GradientStop::new(1.0, col(c1))],
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        p.shader = sh;
    }
    p.anti_alias = true;
    p
}

/// Gradien horizontal x0 -> x1.
pub(crate) fn hgrad(x0: f32, x1: f32, c0: [u8; 3], c1: [u8; 3], a: f32) -> Paint<'static> {
    let col = |c: [u8; 3]| Color::from_rgba8(c[0], c[1], c[2], (a.clamp(0.0, 1.0) * 255.0) as u8);
    let mut p = Paint::default();
    if let Some(sh) = LinearGradient::new(
        Point::from_xy(x0, 0.0),
        Point::from_xy(x1.max(x0 + 1.0), 0.0),
        vec![GradientStop::new(0.0, col(c0)), GradientStop::new(1.0, col(c1))],
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        p.shader = sh;
    }
    p.anti_alias = true;
    p
}

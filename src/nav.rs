//! Carousel: tombol panah, dots, perpindahan slide.

use super::*;

pub(crate) fn cyc(d: f32, n: usize) -> f32 {
    let n = n as f32;
    d - n * (d / n).round()
}

/// A slide with less than this share of the carousel is not drawn. The carousel's spring comes to rest slowly, so a
/// plain linear fade kept a faint copy of the slide that left (an album cover over an empty slide, for example) on
/// screen for a moment after the move looked finished.
const FADE_GONE: f32 = 0.08;

/// How far a slide that is being left travels before it is gone. Halfway, so it is gone while the carousel still
/// moves fast (about 80 ms into a move) instead of fading together with the slide that arrives.
const LEAVE_GONE: f32 = 0.5;

fn smoothstep01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The slides seen at carousel position `pos` of `n`: (index, horizontal shift, opacity). `to` is the slide the
/// carousel is moving to (None while dragging): it fades in over the whole move, and every other slide is gone
/// halfway. Without `to` both sides fade alike, so a drag reveals the next slide as the finger moves.
pub(crate) fn visible_slides(pos: f32, n: usize, to: Option<usize>) -> Vec<(usize, f32, f32)> {
    (0..n)
        .filter_map(|i| {
            let d = cyc(i as f32 - pos, n);
            let al = match to {
                Some(t) if t != i => smoothstep01((LEAVE_GONE - d.abs()) / LEAVE_GONE),
                _ => smoothstep01((1.0 - d.abs() - FADE_GONE) / (1.0 - FADE_GONE)),
            };
            (al > 0.0).then_some((i, d * 48.0, al))
        })
        .collect()
}

pub(crate) fn dot_x(i: usize, n: usize) -> f32 {
    CX + (i as f32 - (n as f32 - 1.0) / 2.0) * 14.0
}

pub(crate) const NAV_L: (f32, f32) = (CL + 22.0, NAVY);
pub(crate) const NAV_R: (f32, f32) = (CL + EXP_W - 22.0, NAVY);

impl App {
    /// Jumlah slide yang tampil (slide aktif + Settings).
    pub(crate) fn n_slides(&self) -> usize {
        self.order.len().max(1)
    }

    /// Indeks slide yang sedang dituju carousel.
    pub(crate) fn cur(&self) -> usize {
        self.slide.rem_euclid(self.n_slides() as i32) as usize
    }

    pub(crate) fn cur_id(&self) -> SlideId {
        self.order.get(self.cur()).copied().unwrap_or(SlideId::Music)
    }

    pub(crate) fn is_slide(&self, id: SlideId) -> bool {
        self.cur_id() == id
    }

    pub(crate) fn step(&mut self, dir: i32) {
        self.out_close();
        self.slide += dir;
    }

    /// Pindah ke slide pada indeks `idx` lewat jalur terpendek.
    pub(crate) fn show(&mut self, idx: usize) {
        let n = self.n_slides() as i32;
        self.out_close();
        let mut d = (idx as i32 - self.cur() as i32).rem_euclid(n);
        if d > n / 2 {
            d -= n;
        }
        self.slide += d;
    }

    /// Pindah ke slide `id`; tidak berbuat apa-apa bila slide itu disembunyikan.
    pub(crate) fn show_id(&mut self, id: SlideId) {
        if let Some(idx) = self.order.iter().position(|&s| s == id) {
            self.show(idx);
        }
    }

    pub(crate) fn draw_nav(&self, g: &mut Gfx, p: f32) {
        let bg = paint(255, 255, 255, 0.12 * p);
        let wh = paint(255, 255, 255, 0.95 * p);
        let (lx, ly) = NAV_L;
        g.circle(lx, ly, 13.0, &bg);
        g.line(lx + 2.5, ly - 5.5, lx - 2.5, ly, 2.2, &wh);
        g.line(lx - 2.5, ly, lx + 2.5, ly + 5.5, 2.2, &wh);
        let (rx, ry) = NAV_R;
        g.circle(rx, ry, 13.0, &bg);
        g.line(rx - 2.5, ry - 5.5, rx + 2.5, ry, 2.2, &wh);
        g.line(rx + 2.5, ry, rx - 2.5, ry + 5.5, 2.2, &wh);
    }

    pub(crate) fn draw_dots(&self, g: &mut Gfx, pos: f32, p: f32) {
        let n = self.n_slides();
        for i in 0..n {
            let near = (1.0 - cyc(i as f32 - pos, n).abs()).clamp(0.0, 1.0);
            let r = 2.3 + 1.2 * near;
            let a = (0.28 + 0.72 * near) * p;
            if near > 0.5 {
                g.circle(dot_x(i, n), EXP_H - 11.0, r, &g.sacc(a));
            } else {
                g.circle(dot_x(i, n), EXP_H - 11.0, r, &paint(255, 255, 255, a));
            }
            // an extension has something new
            if self.ext_dot_badge(i) {
                let c = g.acc.bright;
                g.circle(dot_x(i, n) + 5.0, EXP_H - 17.0, 2.4, &paint(c[0], c[1], c[2], p));
            }
        }
    }
}

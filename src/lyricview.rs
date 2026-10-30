//! Mode lirik pada slide Music: panel kiri (cover, judul, progres, kontrol) + panel kanan berisi daftar lirik yang
//! meluncur mengikuti baris aktif (pegas yang sama dengan carousel). Klik baris = seek ke baris itu.

use super::*;
use gfx::wrap_lines_ex;
use lyrics::Lyrics;

const ART_SZ: f32 = 64.0;
pub(crate) const LM_CY: f32 = 132.0;
const X_BUBBLE: f32 = LX + 14.0;
const X_PREV: f32 = LX + 62.0;
const X_PLAY: f32 = LX + 100.0;
const X_NEXT: f32 = LX + 138.0;
const BAR: (f32, f32, f32, f32) = (LX + 42.0, 106.0, 88.0, 4.0);
const DIV_X: f32 = LX + 190.0;
const PX: f32 = LX + 206.0;
const PW: f32 = RX - PX;
const CENTER: f32 = 86.0;
const SIZE: f32 = 15.0;
const LH: f32 = 19.0;
const GAP: f32 = 12.0;
const SK: f32 = 320.0;
const SC: f32 = 36.0;
const GREY: [u8; 3] = [140, 148, 168];
/// Tombol sematkan: pojok kanan atas panel kiri.
pub(crate) const X_PIN: f32 = DIV_X - 16.0;
pub(crate) const Y_PIN: f32 = 18.0;
/// Kredit sumber lirik ("via lrc.red") di pojok kanan bawah panel lirik, dan lebar area kliknya.
const CREDIT_Y: f32 = 154.0;
const CREDIT_W: f32 = 72.0;

pub(crate) struct LineBox {
    pub(crate) idx: usize,
    pub(crate) top: f32,
    pub(crate) h: f32,
    pub(crate) text: Vec<String>,
}

fn ss(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mmss(secs: u32) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

impl App {
    /// 0..1: kemajuan transisi masuk/keluar mode lirik.
    fn lyric_mode_k(&self) -> f32 {
        let k = ss(0.0, 0.32, self.lyric_mode_t0.elapsed().as_secs_f32());
        if self.lyric_mode { k } else { 1.0 }
    }

    pub(crate) fn lyric_toggle_mode(&mut self) {
        self.lyric_mode = !self.lyric_mode;
        // the pin only means something while the lyrics view is on
        self.pinned &= self.lyric_mode;
        self.lyric_mode_t0 = Instant::now();
        self.lyric_scroll = self.lyric_idx.unwrap_or(0) as f32;
        self.lyric_sv = 0.0;
    }

    /// Notch tetap terbuka karena mode lirik disematkan (tidak di atas aplikasi layar penuh, tempat notch menyusut).
    pub(crate) fn pinned_open(&self) -> bool {
        self.pinned && self.lyric_mode && !self.busy
    }

    /// Mode lirik sedang tampil di slide Music (arah/dots disembunyikan).
    pub(crate) fn lyric_screen(&self) -> bool {
        self.lyric_mode && self.is_slide(SlideId::Music)
    }

    /// Maju satu langkah pegas scroll lirik (dipanggil tiap frame dari `tick`).
    pub(crate) fn lyric_scroll_step(&mut self, dt: f32) {
        let target = self.lyric_idx.unwrap_or(0) as f32;
        if !(self.lyric_mode && self.expanded() && self.h > 100.0) {
            self.lyric_scroll = target;
            self.lyric_sv = 0.0;
            return;
        }
        let n = ((dt / 0.008).ceil() as i32).max(1);
        let sdt = dt / n as f32;
        for _ in 0..n {
            self.lyric_sv += (SK * (target - self.lyric_scroll) - SC * self.lyric_sv) * sdt;
            self.lyric_scroll += self.lyric_sv * sdt;
        }
        if (self.lyric_scroll - target).abs() < 0.004 && self.lyric_sv.abs() < 0.02 {
            self.lyric_scroll = target;
            self.lyric_sv = 0.0;
        }
    }

    pub(crate) fn lyric_scroll_animating(&self) -> bool {
        self.lyric_mode
            && self.expanded()
            && self.h > 100.0
            && ((self.lyric_scroll - self.lyric_idx.unwrap_or(0) as f32).abs() > 0.004
                || self.lyric_sv.abs() > 0.02
                || self.lyric_mode_t0.elapsed().as_secs_f32() < 0.35)
    }

    /// Posisi tiap baris lirik di sekitar posisi scroll (sama untuk gambar dan hit-test klik).
    pub(crate) fn lyric_layout(&self, g: &mut Gfx) -> Vec<LineBox> {
        let Lyrics::Synced(lines, _) = &self.lyrics else { return Vec::new() };
        if lines.is_empty() {
            return Vec::new();
        }
        let n = lines.len();
        let sc = self.lyric_scroll.clamp(0.0, (n - 1) as f32);
        let r = (sc.floor() as usize).min(n - 1);
        let frac = (sc - r as f32).clamp(0.0, 1.0);
        let (lo, hi) = (r.saturating_sub(4), (r + 5).min(n - 1));
        let mut boxes: Vec<LineBox> = (lo..=hi)
            .map(|i| {
                let t = lines[i].shown(self.cfg.romaji_lyrics);
                let t = if t.is_empty() { "\u{b7} \u{b7} \u{b7}" } else { t };
                let wrapped = wrap_lines_ex(g, t, SIZE, PW, true);
                let h = wrapped.len() as f32 * LH;
                LineBox { idx: i, top: 0.0, h, text: wrapped }
            })
            .collect();
        // Posisi relatif terhadap atas baris r.
        let ri = r - lo;
        let mut rel = vec![0.0f32; boxes.len()];
        for i in ri + 1..boxes.len() {
            rel[i] = rel[i - 1] + boxes[i - 1].h + GAP;
        }
        for i in (0..ri).rev() {
            rel[i] = rel[i + 1] - boxes[i].h - GAP;
        }
        let c_r = boxes[ri].h / 2.0;
        let c_next = if ri + 1 < boxes.len() { rel[ri + 1] + boxes[ri + 1].h / 2.0 } else { c_r };
        let s = c_r + (c_next - c_r) * frac;
        for (i, b) in boxes.iter_mut().enumerate() {
            b.top = CENTER - s + rel[i];
        }
        boxes
    }

    pub(crate) fn slide_music_lyrics(&self, g: &mut Gfx, ox: f32, al: f32) {
        let k = self.lyric_mode_k();
        let a = al * k;
        let slide = (1.0 - k) * 24.0;

        // ---- panel kiri
        self.art_at(g, LX + ox, 24.0, ART_SZ, 10.0, a);
        let tx = LX + ox + ART_SZ + 12.0;
        let tw = DIV_X - (LX + ART_SZ + 12.0) - 8.0;
        let title = if self.media.title.is_empty() { tr("No media") } else { &self.media.title };
        let title = if g.width_ex(title, 16.0, true) > tw { ellipsize(g, title, 16.0, tw) } else { title.to_string() };
        g.txtb(&title, tx, 31.0, 16.0, [255, 255, 255], a, tw + 2.0);
        let artist = if g.width(&self.media.artist, 12.0) > tw { ellipsize(g, &self.media.artist, 12.0, tw) } else { self.media.artist.clone() };
        g.txt(&artist, tx, 53.0, 12.0, [150, 150, 158], a, tw + 2.0);
        let pc = if self.pinned { g.acc.bright } else { [140, 148, 168] };
        g.icon_pin(X_PIN + ox, Y_PIN, 13.0, &paint(pc[0], pc[1], pc[2], if self.pinned { a } else { 0.8 * a }), self.pinned);

        // progres: 02:59 ----- -01:17
        let pos = self.lyric_pos_ms() / 1000;
        let dur = self.media.dur;
        g.txt(&mmss(pos), LX + ox, BAR.1 - 5.0, 11.0, [190, 190, 198], a, 40.0);
        let rem = format!("-{}", mmss(dur.saturating_sub(pos)));
        let rw = g.width(&rem, 11.0);
        g.txt(&rem, DIV_X - 8.0 + ox - rw, BAR.1 - 5.0, 11.0, [190, 190, 198], a, 50.0);
        g.fill(&rrect(BAR.0 + ox, BAR.1, BAR.2, BAR.3, 2.0), &paint(255, 255, 255, 0.18 * a));
        if dur > 0 {
            let fr = (self.lyric_pos_ms() as f32 / (dur as f32 * 1000.0)).clamp(0.0, 1.0);
            g.fill(&rrect(BAR.0 + ox, BAR.1, (BAR.2 * fr).max(4.0), BAR.3, 2.0), &icons::hgrad(BAR.0, BAR.0 + BAR.2, [255, 255, 255], [140, 200, 255], a));
        }

        // kontrol datar
        let white = paint(255, 255, 255, a);
        let dim = paint(255, 255, 255, 0.85 * a);
        g.icon_bubble(X_BUBBLE + ox, LM_CY, 17.0, &white, &paint(12, 20, 40, a));
        g.icon_prev(X_PREV + ox, LM_CY, 9.0, &dim);
        if self.media.playing {
            g.icon_pause(X_PLAY + ox, LM_CY, 17.0, &white);
        } else {
            g.icon_play(X_PLAY + ox, LM_CY, 17.0, &white);
        }
        g.icon_next(X_NEXT + ox, LM_CY, 9.0, &dim);

        // ---- panel kanan
        g.rect(DIV_X + ox, 22.0, 1.0, 128.0, &paint(255, 255, 255, 0.12 * a));
        let msg = match &self.lyrics {
            Lyrics::Loading => Some(tr("Loading lyrics")),
            Lyrics::Unavailable => Some(tr("Lyrics not available")),
            Lyrics::Plain => Some(tr("Lyrics not synced")),
            Lyrics::None => Some(""),
            Lyrics::Synced(..) => None,
        };
        if let Some(m) = msg {
            g.txt_c(m, PX + PW / 2.0 + ox + slide, CENTER - 7.0, 13.0, [120, 126, 140], a, PW, false);
            return;
        }
        for b in self.lyric_layout(g) {
            let mid = b.top + b.h / 2.0;
            let near = (1.0 - (mid - CENTER).abs() / 62.0).clamp(0.0, 1.0);
            let edge = ss(8.0, 46.0, mid) * ss(8.0, 46.0, 168.0 - mid);
            let alpha = (0.26 + 0.74 * near.powf(1.4)) * edge * a;
            if alpha <= 0.015 {
                continue;
            }
            let t = near * near;
            let col = [
                (GREY[0] as f32 + (255.0 - GREY[0] as f32) * t) as u8,
                (GREY[1] as f32 + (255.0 - GREY[1] as f32) * t) as u8,
                (GREY[2] as f32 + (255.0 - GREY[2] as f32) * t) as u8,
            ];
            for (li, line) in b.text.iter().enumerate() {
                g.txtb(line, PX + ox + slide, b.top + li as f32 * LH, SIZE, col, alpha, PW + 2.0);
            }
        }
        // kredit sumber lirik (syarat lrc.red); klik membuka situsnya
        if let Lyrics::Synced(_, src) = &self.lyrics {
            let label = fill(tr("via {}"), &[&src.name()]);
            let w = g.width(&label, 10.0);
            g.txt(&label, RX - w + ox + slide, CREDIT_Y, 10.0, [165, 171, 186], 0.9 * a, w + 2.0);
        }
    }

    /// Klik pada slide Music saat mode lirik aktif. Mengembalikan true bila klik ditangani.
    pub(crate) fn lyric_click(&mut self, x: f32, y: f32) -> bool {
        let d = |px: f32, py: f32| ((x - px).powi(2) + (y - py).powi(2)).sqrt();
        if d(X_BUBBLE, LM_CY) < 16.0 {
            self.lyric_toggle_mode();
        } else if d(X_PIN, Y_PIN) < 12.0 {
            self.pinned = !self.pinned;
        } else if d(X_PREV, LM_CY) < 16.0 {
            let _ = self.tx.send(Cmd::Prev);
        } else if d(X_PLAY, LM_CY) < 18.0 {
            let _ = self.tx.send(Cmd::Toggle);
        } else if d(X_NEXT, LM_CY) < 16.0 {
            let _ = self.tx.send(Cmd::Next);
        } else if let (Lyrics::Synced(_, src), true) = (&self.lyrics, in_rect(x, y, RX - CREDIT_W, CREDIT_Y - 3.0, CREDIT_W, 16.0)) {
            sys::open_url(src.url());
        } else if self.media.dur > 0 && in_rect(x, y, BAR.0, BAR.1 - 8.0, BAR.2, 20.0) {
            let _ = self.tx.send(Cmd::Seek(((x - BAR.0) / BAR.2).clamp(0.0, 1.0)));
        } else if x >= PX && x <= RX && self.media.dur > 0 {
            // Klik baris lirik = seek ke awal baris itu.
            let mut gfx = self.gfx.take();
            let boxes = gfx.as_mut().map(|g| self.lyric_layout(g)).unwrap_or_default();
            self.gfx = gfx;
            if let (Lyrics::Synced(lines, _), Some(b)) = (&self.lyrics, boxes.iter().find(|b| y >= b.top && y <= b.top + b.h)) {
                let f = lines[b.idx].at_ms as f32 / 1000.0 / self.media.dur as f32;
                let _ = self.tx.send(Cmd::Seek(f.clamp(0.0, 1.0)));
            }
        } else {
            return false;
        }
        true
    }
}

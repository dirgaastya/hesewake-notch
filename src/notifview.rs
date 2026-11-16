//! Slide "Notifications" (slide ke-6): daftar notifikasi Windows dengan ikon aplikasi, judul, isi, waktu relatif,
//! hapus per item dan Clear all. Baris masuk bertahap dari atas; baris yang dihapus meluncur ke kanan lalu baris di
//! bawahnya naik (semua pegas/ease-out, bukan lompatan).

use super::*;
use notif::{Access, NotifItem};

const Y0: f32 = 40.0;
const ROW_H: f32 = 34.0;
const ROW_P: f32 = 38.0;
const VIS: usize = 3;
const CLEAR: (f32, f32, f32, f32) = (RX - 88.0, 8.0, 88.0, 24.0);
const OPEN_BTN: (f32, f32, f32, f32) = (LX + CW / 2.0 - 84.0, 100.0, 168.0, 30.0);
const DISMISS_SECS: f32 = 0.25;
/// Lebar area tombol hapus di ujung kanan baris; sisa baris membuka aplikasinya.
const DISMISS_W: f32 = 32.0;
const GREY: [u8; 3] = TEXT_DIM;

pub(crate) struct NotifUi {
    entered: bool,
    enter_t: Instant,
    seen_t: Option<Instant>,
}

impl NotifUi {
    pub(crate) fn new() -> NotifUi {
        NotifUi { entered: false, enter_t: Instant::now() - Duration::from_secs(10), seen_t: None }
    }
}

fn ss(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn dismiss_p(n: &NotifItem) -> f32 {
    n.dismiss.map_or(0.0, |t| (t.elapsed().as_secs_f32() / DISMISS_SECS).clamp(0.0, 1.0))
}

/// Satu baris yang tampil: indeks di `notifs`, y, alpha, geser x.
struct Row {
    idx: usize,
    y: f32,
    alpha: f32,
    dx: f32,
}

impl App {
    fn notif_visible(&self) -> bool {
        self.expanded() && self.is_slide(SlideId::Notifications) && self.h > 100.0
    }

    /// Dipanggil tiap frame/poll: animasi masuk, tandai dibaca setelah ±0,8 dtk terlihat, bersihkan baris yang selesai
    /// dihapus.
    pub(crate) fn notif_ui_step(&mut self) {
        self.notifs.retain(|n| n.dismiss.map_or(true, |t| t.elapsed().as_secs_f32() < DISMISS_SECS));
        let max = self.notifs.len().saturating_sub(VIS);
        self.notif_scroll = self.notif_scroll.min(max);
        if !self.notif_visible() {
            self.nui.entered = false;
            self.nui.seen_t = None;
            return;
        }
        if !self.nui.entered {
            self.nui.entered = true;
            self.nui.enter_t = Instant::now();
            self.nui.seen_t = Some(Instant::now());
        }
        if self.nui.seen_t.map_or(false, |t| t.elapsed().as_secs_f32() > 0.8) {
            for n in &mut self.notifs {
                n.unread = false;
            }
        }
    }

    pub(crate) fn notif_ui_animating(&self) -> bool {
        self.notif_visible()
            && (self.nui.enter_t.elapsed().as_secs_f32() < 0.6
                || self.bell_t.elapsed().as_secs_f32() < 0.35
                || self.notifs.iter().any(|n| n.dismiss.is_some() || n.opening)
                || (self.notif_on && self.notifs.is_empty())
                || self.nt_cnt.get().1.is_some_and(|t| t.elapsed().as_secs_f32() < 0.65))
    }

    fn notif_rows(&self) -> Vec<Row> {
        let el = self.nui.enter_t.elapsed().as_secs_f32();
        let mut rows = Vec::new();
        let mut acc = 0.0f32;
        let mut k = 0usize;
        for (idx, n) in self.notifs.iter().enumerate().skip(self.notif_scroll) {
            let dp = dismiss_p(n);
            if dp >= 1.0 {
                continue;
            }
            let y = Y0 + acc;
            if y > Y0 + VIS as f32 * ROW_P - 4.0 {
                break;
            }
            let enter = ss(0.0, 0.3, el - k as f32 * 0.05);
            let ease = 1.0 - (1.0 - dp).powi(2);
            // notifikasi yang baru masuk (baris teratas) meluncur dari atas sambil memudar
            let arr = if idx == 0 { ss(0.0, 0.3, self.bell_t.elapsed().as_secs_f32()) } else { 1.0 };
            rows.push(Row { idx, y: y + (1.0 - enter) * 10.0 - (1.0 - arr) * 12.0, alpha: enter * arr * (1.0 - dp), dx: ease * 48.0 });
            acc += ROW_P * (1.0 - ease);
            k += 1;
        }
        rows
    }

    fn notif_dismiss(&mut self, ids_idx: &[usize], stagger: f32) {
        let now = Instant::now();
        let mut ids = Vec::new();
        for (i, &idx) in ids_idx.iter().enumerate() {
            if let Some(n) = self.notifs.get_mut(idx) {
                if n.dismiss.is_none() {
                    n.dismiss = Some(now + Duration::from_secs_f32(stagger * i as f32));
                    ids.push(n.id);
                }
            }
        }
        if !ids.is_empty() {
            notif::remove_from_windows(ids);
        }
    }

    /// Tombol dan baris notifikasi yang bisa diklik (kursor tangan).
    pub(crate) fn notif_hand(&self, x: f32, y: f32) -> bool {
        if self.notif_access == Access::Denied {
            return in_rect(x, y, OPEN_BTN.0, OPEN_BTN.1, OPEN_BTN.2, OPEN_BTN.3);
        }
        if !self.notif_on {
            return false;
        }
        in_rect(x, y, CLEAR.0, CLEAR.1, CLEAR.2, CLEAR.3) || self.notif_rows().iter().any(|r| r.alpha > 0.5 && in_rect(x, y, LX, r.y, CW, ROW_H))
    }

    pub(crate) fn notif_click(&mut self, x: f32, y: f32) {
        let hit = |r: (f32, f32, f32, f32)| in_rect(x, y, r.0, r.1, r.2, r.3);
        if self.notif_access == Access::Denied {
            if hit(OPEN_BTN) {
                unsafe { sys::open_notification_settings() };
            }
            return;
        }
        if !self.notif_on {
            return;
        }
        if hit(CLEAR) {
            let all: Vec<usize> = (0..self.notifs.len()).collect();
            self.notif_dismiss(&all, 0.04);
            return;
        }
        let rows = self.notif_rows();
        if let Some(r) = rows.iter().find(|r| r.alpha > 0.5 && in_rect(x, y, LX, r.y, CW, ROW_H)) {
            if in_rect(x, y, LX + CW - DISMISS_W, r.y, DISMISS_W, ROW_H) {
                self.notif_dismiss(&[r.idx], 0.0);
            } else {
                let id = self.notifs[r.idx].id;
                self.notif_open(id);
            }
        }
    }

    /// Buka aplikasi asal notifikasi; hasilnya datang lewat `notif_opened`.
    pub(crate) fn notif_open(&mut self, id: u32) {
        let Some(n) = self.notifs.iter_mut().find(|n| n.id == id && n.dismiss.is_none() && !n.opening) else { return };
        n.opening = true;
        let aumid = n.aumid.clone();
        notifopen::open(self.hwnd, self.notif_slot.clone(), id, aumid);
    }

    /// Berhasil: notifikasi selesai, sama seperti klik di Windows. Gagal: notifikasi tetap ada dan pengguna diberi tahu.
    pub(crate) fn notif_opened(&mut self, id: u32, ok: bool) {
        let Some(idx) = self.notifs.iter().position(|n| n.id == id) else { return };
        self.notifs[idx].opening = false;
        if ok {
            self.notif_dismiss(&[idx], 0.0);
        } else {
            let app = self.notifs[idx].app.clone();
            self.push_toast(toast::ToastKind::NotifOpenFailed { app });
        }
    }

    pub(crate) fn notif_wheel(&mut self, d: i32) -> bool {
        if self.notifs.len() <= VIS {
            return false;
        }
        let max = self.notifs.len() - VIS;
        self.notif_scroll = if d > 0 { self.notif_scroll.saturating_sub(1) } else { (self.notif_scroll + 1).min(max) };
        true
    }

    pub(crate) fn slide_notifs(&self, g: &mut Gfx, ox: f32, al: f32) {
        // Header
        g.txtb(tr("Notifications"), LX + ox, 10.0, 15.0, [255, 255, 255], al, 160.0);
        let live = self.notifs.iter().filter(|n| n.dismiss.is_none()).count();
        if live > 0 && self.notif_on && self.notif_access == Access::Allowed {
            // Jumlah sebagai badge bulat kecil, sejajar dengan tengah judul (y ≈ 18).
            let label = if live > 99 { "99+".to_string() } else { live.to_string() };
            let hw = g.width_ex(tr("Notifications"), 15.0, true);
            let lw = g.width_ex(&label, 11.0, true);
            let (bw, bh) = ((lw + 12.0).max(20.0), 18.0);
            let bx = LX + ox + hw + 9.0;
            // lencana berdenyut saat jumlahnya berubah
            let (last_n, mut last_t) = self.nt_cnt.get();
            if last_n != live {
                last_t = Some(Instant::now());
                self.nt_cnt.set((live, last_t));
            }
            let pop = last_t.map_or(1.0, |t| {
                let e = t.elapsed().as_secs_f32();
                if e < 0.6 { 1.0 + 0.28 * (-9.0 * e).exp() * (26.0 * e).sin() } else { 1.0 }
            });
            g.pill(bx, 9.0, bw, bh, [255, 255, 255], 0.14 * al, pop);
            g.txt_c(&label, bx + bw / 2.0, 9.0 + ty(bh, 11.0), 11.0, [225, 225, 232], al, bw, true);
            g.pill(CLEAR.0 + ox, CLEAR.1, CLEAR.2, CLEAR.3, [255, 255, 255], 0.12 * al, 1.0);
            g.txt_c(tr("Clear all"), CLEAR.0 + ox + CLEAR.2 / 2.0, CLEAR.1 + ty(CLEAR.3, 12.0), 12.0, [225, 225, 232], al, CLEAR.2, true);
        }

        // Keadaan khusus: izin ditolak / fitur dimatikan / kosong.
        // lonceng berayun sebentar tiap 4 detik di keadaan kosong
        let q = (self.t % 4.0) / 4.0;
        let bell_swing = if q < 0.3 { (q * 40.0).sin() * 0.22 * (1.0 - q / 0.3) } else { 0.0 };
        let center_msg = |g: &mut Gfx, a: &str, b: &str| {
            g.icon_bell(LX + CW / 2.0 + ox, 66.0, 26.0, bell_swing, &paint(255, 255, 255, 0.28 * al));
            g.txt_c(a, LX + CW / 2.0 + ox, 90.0, 13.0, [220, 220, 228], al, CW, true);
            if !b.is_empty() {
                g.txt_c(b, LX + CW / 2.0 + ox, 110.0, 12.0, GREY, al, CW, false);
            }
        };
        if self.notif_access == Access::Denied {
            g.txt_c(tr("Notification access is off"), LX + CW / 2.0 + ox, 62.0, 13.0, [220, 220, 228], al, CW, true);
            g.txt_c(tr("Allow Hesewake Notch in Windows Settings to see them here."), LX + CW / 2.0 + ox, 80.0, 12.0, GREY, al, CW, false);
            let hov = self.cursor_canvas().is_some_and(|(hx, hy)| in_rect(hx, hy, OPEN_BTN.0 + ox, OPEN_BTN.1, OPEN_BTN.2, OPEN_BTN.3));
            g.pill(OPEN_BTN.0 + ox, OPEN_BTN.1, OPEN_BTN.2, OPEN_BTN.3, g.acc.bright, if hov { 0.42 } else { 0.28 } * al, if hov { 1.04 } else { 1.0 });
            g.txt_c(tr("Open Settings"), OPEN_BTN.0 + ox + OPEN_BTN.2 / 2.0, OPEN_BTN.1 + ty(OPEN_BTN.3, 13.0), 13.0, [200, 228, 255], al, OPEN_BTN.2, true);
            return;
        }
        if !self.notif_on {
            center_msg(g, tr("Windows notifications are off"), tr("Turn them on from the menu (right-click the notch)."));
            return;
        }
        let rows = self.notif_rows();
        if rows.is_empty() {
            center_msg(g, tr("No notifications"), "");
            return;
        }

        let now = notif::now_unix();
        for r in rows {
            let n = &self.notifs[r.idx];
            let a = al * r.alpha;
            if a <= 0.01 {
                continue;
            }
            let x = LX + ox + r.dx;
            let hov = self.cursor_canvas().is_some_and(|(hx, hy)| in_rect(hx, hy, x, r.y, CW, ROW_H));
            g.fill(&rrect(x, r.y, CW, ROW_H, 12.0), &paint(255, 255, 255, (if n.unread { 0.13 } else { 0.07 } + if hov { 0.05 } else { 0.0 }) * a));
            notif::draw_avatar(g, n, x + 8.0, r.y + 5.0, 24.0, a);
            if n.opening {
                // membuka aplikasi asal: cincin berputar di avatar dan kilau menyapu baris
                let a0 = self.t * 8.0;
                g.arc(x + 20.0, r.y + 17.0, 15.0, a0, a0 + 1.6, 2.0, &g.bright(a));
                let sx = x + (((self.t * 1.4) % 1.4) - 0.2) * CW;
                let (l, rr_) = (sx.max(x + 4.0), (sx + 110.0).min(x + CW - 4.0));
                if rr_ > l {
                    g.fill(&rrect(l, r.y + 2.0, rr_ - l, ROW_H - 4.0, 10.0), &paint(255, 255, 255, 0.10 * a));
                }
            }
            let tx = x + 40.0;
            let max_w = CW - 40.0 - 52.0;
            let title = if n.title.is_empty() { n.app.as_str() } else { n.title.as_str() };
            let title = if g.width_ex(title, 12.0, true) > max_w { ellipsize(g, title, 12.0, max_w) } else { title.to_string() };
            g.txtb(&title, tx, r.y + 3.0, 12.0, [255, 255, 255], a, max_w + 2.0);
            let body = if n.opening { tr("Opening\u{2026}").to_string() } else if n.body.is_empty() { n.app.clone() } else { n.body.replace('\n', " ") };
            let body = if g.width(&body, 12.0) > max_w { ellipsize(g, &body, 12.0, max_w) } else { body };
            g.txt(&body, tx, r.y + 18.0, 12.0, if n.opening { g.acc.bright } else { GREY }, a, max_w + 2.0);
            // waktu relatif + tombol hapus
            let t = notif::relative_time(n.at_unix, now);
            let tw = g.width(&t, 11.0);
            g.txt(&t, x + CW - 30.0 - tw, r.y + 4.0, 11.0, GREY, a, 40.0);
            if n.unread {
                g.circle(x + CW - 40.0 - tw, r.y + 10.0, 2.6, &g.bright(a));
            }
            let (cx, cy) = (x + CW - 16.0, r.y + ROW_H / 2.0 + 3.0);
            if hov {
                g.circle(cx, cy, 9.0, &paint(255, 255, 255, 0.14 * a));
            }
            let xc = paint(220, 220, 228, (if hov { 1.0 } else { 0.8 }) * a);
            g.line(cx - 3.5, cy - 3.5, cx + 3.5, cy + 3.5, 1.4, &xc);
            g.line(cx - 3.5, cy + 3.5, cx + 3.5, cy - 3.5, 1.4, &xc);
        }
    }
}

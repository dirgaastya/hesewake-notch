//! Toast di notch: charger tersambung/dicabut, baterai rendah, perangkat Bluetooth tersambung.
//! Notch melebar dengan pegas (lihat `target`), isi masuk bertahap, lalu menutup sendiri.

use super::*;

pub(crate) const TOAST_CHARGE: (f32, f32) = (336.0, 70.0);
pub(crate) const TOAST_BT: (f32, f32) = (372.0, 70.0);
/// HUD kecil (tombol kunci, volume, kecerahan): lebih pendek dari toast biasa.
pub(crate) const TOAST_HUD: (f32, f32) = (244.0, 46.0);
pub(crate) const TOAST_PRIV: (f32, f32) = (292.0, 46.0);
/// Pengingat minum dengan tombol Snooze / Drank.
pub(crate) const TOAST_WATER: (f32, f32) = (396.0, 66.0);
pub(crate) const TOAST_MEETING: (f32, f32) = (396.0, 66.0);
pub(crate) const TOAST_UPDATE: (f32, f32) = (396.0, 66.0);

/// Jenis HUD kecil.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum HudKind {
    Caps,
    Num,
    Scroll,
    /// Volume master (persen, bisu).
    Volume(u8, bool),
    /// Kecerahan layar (persen).
    Brightness(u8),
    /// Microphone mute from Ctrl+Alt+M: muted, on, or None when there is no microphone.
    Mic(Option<bool>),
}

impl HudKind {
    /// Jenis yang sama (abaikan nilai): HUD beruntun diperbarui di tempat, tanpa animasi masuk ulang.
    pub(crate) fn same_class(&self, o: &HudKind) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(o)
    }

    fn label(self) -> &'static str {
        match self {
            HudKind::Volume(..) => "Volume",
            HudKind::Brightness(..) => tr("Brightness"),
            HudKind::Mic(..) => tr("Microphone"),
            HudKind::Caps => "Caps Lock",
            HudKind::Num => "Num Lock",
            HudKind::Scroll => "Scroll Lock",
        }
    }
    fn glyph(self) -> &'static str {
        match self {
            HudKind::Caps => "A",
            HudKind::Num => "1",
            HudKind::Scroll => "S",
            HudKind::Volume(..) => "V",
            HudKind::Brightness(..) => "B",
            HudKind::Mic(..) => "M",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ToastKind {
    Charging { pct: u8 },
    Unplugged { pct: u8 },
    LowBattery { pct: u8 },
    Bluetooth { name: String, pct: Option<u8>, connected: bool },
    /// Notifikasi Windows; isinya dicari di `App.notifs` berdasarkan id.
    Notification { id: u32 },
    /// HUD kecil: tombol kunci (On/Off).
    Hud { kind: HudKind, on: bool },
    /// Aplikasi mulai memakai mikrofon (`cam: false`) atau kamera (`cam: true`).
    Privacy { cam: bool, app: String },
    /// A prayer time started: index into `prayer::NAMES` and its time (minutes since 00:00).
    Prayer { idx: u8, at: u16 },
    /// Pengingat minum; teksnya dibaca dari `App.water`, tombol Snooze/Drank bisa diklik (lihat `toast_click`).
    Water,
    /// A countdown ended; `title` e.g. "Time's up" / "Break over", `label` e.g. "5 min timer".
    Timer { title: String, label: String },
    /// A meeting starts now: title, start (unix seconds) and its Join link.
    Meeting { title: String, at: i64, link: Option<String> },
    /// A newer release is out, or the answer to "Check for updates".
    Update(update::Note),
    /// A download finished: full path of the file, with Open / Show in folder (downloads.rs).
    Downloaded { path: String },
}

pub(crate) struct Toast {
    pub(crate) kind: ToastKind,
    pub(crate) t0: Instant,
    pub(crate) dur: f32,
}

const GREEN: [u8; 3] = [60, 200, 110];
const BLUE: [u8; 3] = [90, 190, 255];
const GREY: [u8; 3] = TEXT_DIM;

pub(crate) fn ss(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Pegas teredam 0 -> 1 dengan sedikit overshoot (dipakai untuk ikon yang "muncul").
pub(crate) fn pop(t: f32) -> f32 {
    1.0 - (-8.0 * t).exp() * (14.0 * t).cos()
}

impl App {
    pub(crate) fn push_toast(&mut self, kind: ToastKind) {
        if self.display_off {
            return;
        }
        // HUD beruntun (mis. Caps Lock ditekan dua kali) menggantikan HUD yang sedang tampil, tanpa antre.
        if let (Some(cur), ToastKind::Hud { .. }) = (self.toast.as_mut(), &kind) {
            if matches!(cur.kind, ToastKind::Hud { .. }) {
                let same = cur.kind == kind || matches!((&cur.kind, &kind), (ToastKind::Hud { kind: a, .. }, ToastKind::Hud { kind: b, .. }) if a.same_class(b));
                cur.kind = kind;
                // jenis sama: lewati animasi masuk (isi sudah tampil), hanya perpanjang waktu
                cur.t0 = if same { Instant::now() - Duration::from_millis(450) } else { Instant::now() };
                cur.dur = 1.9;
                return;
            }
        }
        // HUD cepat basi: tidak diantre. Jatuh bila ada toast lain atau notch sedang terbuka.
        if matches!(kind, ToastKind::Hud { .. }) {
            if self.toast.is_none() && !self.expanded() {
                self.start_toast(kind);
            }
            return;
        }
        if self.toast.is_none() && !self.expanded() {
            self.start_toast(kind);
        } else if self.toast_q.len() < 3 {
            self.toast_q.push(kind);
        }
    }

    fn start_toast(&mut self, kind: ToastKind) {
        let dur = match kind {
            ToastKind::LowBattery { .. } => 4.0,
            ToastKind::Bluetooth { .. } => 3.8,
            ToastKind::Notification { .. } => 4.5,
            ToastKind::Hud { .. } => 1.9,
            ToastKind::Privacy { .. } => 2.6,
            ToastKind::Prayer { .. } => 6.0,
            ToastKind::Water => 12.0,
            ToastKind::Timer { .. } => 6.0,
            ToastKind::Meeting { .. } => 15.0,
            ToastKind::Update(update::Note::Available(_)) => 10.0,
            ToastKind::Update(update::Note::Downloading(_)) => 20.0,
            ToastKind::Update(update::Note::DownloadFailed(_)) => 12.0,
            ToastKind::Update(_) => 4.0,
            ToastKind::Downloaded { .. } => 8.0,
            _ => 3.4,
        };
        perf::note(&format!("toast start {:?} mem={:.1}MB", kind, perf::mem_mb()));
        self.toast = Some(Toast { kind, t0: Instant::now(), dur });
        self.drop_pulse = Some(Instant::now()); // riak tepi bawah, sama dengan saat drop file di Ledge
        self.last = Instant::now();
    }

    /// Selesaikan toast yang habis waktu / dibatalkan hover, lalu mulai berikutnya. True bila ada perubahan.
    pub(crate) fn toast_update(&mut self) -> bool {
        let mut changed = false;
        if let Some(t) = &self.toast {
            if self.expanded() || t.t0.elapsed().as_secs_f32() >= t.dur {
                let to_notif = self.expanded() && matches!(t.kind, ToastKind::Notification { .. });
                self.toast = None;
                if to_notif {
                    self.show_id(SlideId::Notifications);
                }
                self.toast_end = Instant::now();
                changed = true;
            }
        }
        if self.toast.is_none() && !self.expanded() && !self.toast_q.is_empty() {
            let k = self.toast_q.remove(0);
            self.start_toast(k);
            changed = true;
        }
        changed
    }

    /// Notch sedang (atau baru saja) berukuran toast: jangan hitung progres expand dari tinggi notch.
    pub(crate) fn toast_hold(&self) -> bool {
        !self.expanded() && (self.toast.is_some() || self.toast_end.elapsed().as_secs_f32() < 0.8)
    }

    pub(crate) fn toast_size(&self) -> (f32, f32) {
        match self.toast.as_ref().map(|t| &t.kind) {
            Some(ToastKind::Bluetooth { .. }) | Some(ToastKind::Notification { .. }) => TOAST_BT,
            Some(ToastKind::Hud { .. }) => TOAST_HUD,
            Some(ToastKind::Privacy { .. }) => TOAST_PRIV,
            Some(ToastKind::Water) => TOAST_WATER,
            Some(ToastKind::Meeting { .. }) => TOAST_MEETING,
            Some(ToastKind::Update(_)) => TOAST_UPDATE,
            Some(ToastKind::Downloaded { .. }) => downloads::TOAST_DOWNLOAD,
            _ => TOAST_CHARGE,
        }
    }

    /// Dipanggil saat status daya berubah dan berkala: toast saat charger dicolok/dicabut, baterai <= 20%.
    pub(crate) fn power_check(&mut self) {
        let Some((ac, pct)) = sys::power_status() else { return };
        if let Some(prev) = self.ac_prev {
            if prev != ac {
                self.push_toast(if ac { ToastKind::Charging { pct } } else { ToastKind::Unplugged { pct } });
            }
        }
        self.ac_prev = Some(ac);
        if ac || pct > 25 {
            self.low_armed = true;
        } else if pct <= 20 && self.low_armed {
            self.low_armed = false;
            self.push_toast(ToastKind::LowBattery { pct });
        }
    }

    pub(crate) fn draw_toast(&self, g: &mut Gfx, w: f32, h: f32, ca: f32) {
        let Some(ts) = &self.toast else { return };
        let t = ts.t0.elapsed().as_secs_f32();
        let al = ca * (1.0 - ss(ts.dur - 0.25, ts.dur, t));
        if al <= 0.01 {
            return;
        }
        let (x0, cy) = (CX - w / 2.0, h / 2.0);
        if let ToastKind::Notification { id } = &ts.kind {
            self.draw_notif_toast(g, w, h, al, t, *id);
            return;
        }
        if let ToastKind::Hud { kind, on } = &ts.kind {
            Self::draw_hud_toast(g, w, h, al, t, *kind, *on);
            return;
        }
        if let ToastKind::Privacy { cam, app } = &ts.kind {
            Self::draw_privacy_toast(g, (w, h), al, t, *cam, app, !*cam && self.mic_muted == Some(true));
            return;
        }
        if let ToastKind::Water = &ts.kind {
            self.draw_water_toast(g, w, h, al, t);
            return;
        }
        if let ToastKind::Meeting { title, at, link } = &ts.kind {
            self.draw_meeting_toast(g, (w, h), al, t, (title, *at, link.is_some()));
            return;
        }
        if let ToastKind::Update(note) = &ts.kind {
            self.draw_update_toast(g, (w, h), al, t, note);
            return;
        }
        if let ToastKind::Downloaded { path } = &ts.kind {
            self.draw_download_toast(g, (w, h), al, t, path);
            return;
        }

        // (warna aksen, judul, subjudul, fraksi ring, teks kanan)
        let (rgb, title, sub, frac, right) = match &ts.kind {
            ToastKind::Charging { pct } => (GREEN, tr("Charging").to_string(), tr("Charger connected").to_string(), *pct as f32 / 100.0, Some(format!("{}%", pct))),
            ToastKind::Unplugged { pct } => ([235, 235, 240], tr("On battery").to_string(), tr("Charger disconnected").to_string(), *pct as f32 / 100.0, Some(format!("{}%", pct))),
            ToastKind::LowBattery { pct } => (ACCENT, tr("Battery low").to_string(), tr("Plug in your charger").to_string(), *pct as f32 / 100.0, Some(format!("{}%", pct))),
            ToastKind::Timer { title, label } => ([255, 170, 60], title.clone(), label.clone(), 1.0, None),
            ToastKind::Bluetooth { name, pct, connected } => {
                let s = match (connected, pct) {
                    (true, Some(p)) => fill(tr("Connected \u{b7} {}%"), &[p]),
                    (true, None) => tr("Connected").to_string(),
                    (false, _) => tr("Disconnected").to_string(),
                };
                (if *connected { BLUE } else { GREY }, name.clone(), s, pct.map_or(1.0, |p| p as f32 / 100.0), None)
            }
            ToastKind::Prayer { idx, at } => {
                let (title, sub) = prayer::toast_text(*idx as usize, self.cfg.ramadan);
                // while the adhan plays, its Stop button takes the place of the time
                let right = (!self.adhan_playing()).then(|| prayer::hhmm(*at));
                (prayer::GOLD, title.to_string(), sub.to_string(), 1.0, right)
            }
            ToastKind::Notification { .. } | ToastKind::Hud { .. } | ToastKind::Privacy { .. } | ToastKind::Water | ToastKind::Meeting { .. } | ToastKind::Update(_) | ToastKind::Downloaded { .. } => {
                unreachable!()
            }
        };

        // Ring + ikon: membesar dengan pegas, ring menyapu dari 0.
        let sc = 0.55 + 0.45 * pop(t).clamp(0.0, 1.2);
        let (rcx, r) = (x0 + 36.0, 17.0 * sc);
        let sweep = {
            let k = ((t - 0.1) / 0.7).clamp(0.0, 1.0);
            1.0 - (1.0 - k).powi(3)
        };
        let breathe = if matches!(ts.kind, ToastKind::Charging { .. } | ToastKind::LowBattery { .. }) {
            0.78 + 0.22 * (t * 5.0).sin()
        } else {
            1.0
        };
        let fg = paint(rgb[0], rgb[1], rgb[2], al);
        g.ring(rcx, cy, r, frac * sweep, 3.5, &paint(rgb[0], rgb[1], rgb[2], 0.2 * al), &fg);
        let ic = paint(rgb[0], rgb[1], rgb[2], al * breathe);
        match &ts.kind {
            ToastKind::Charging { .. } => g.icon_bolt(rcx, cy, 17.0 * sc, &ic),
            ToastKind::Unplugged { pct } => g.icon_battery(rcx, cy, 16.0 * sc, *pct as f32 / 100.0, &ic),
            ToastKind::LowBattery { .. } => g.icon_battery(rcx, cy, 16.0 * sc, 0.12, &ic),
            ToastKind::Bluetooth { .. } => g.icon_bluetooth(rcx, cy, 17.0 * sc, &ic),
            ToastKind::Prayer { .. } => g.icon_crescent(rcx, cy, 18.0 * sc, &ic),
            ToastKind::Timer { .. } => g.icon_bell(rcx, cy, 17.0 * sc, (t * 22.0).sin() * (1.0 - t / 1.5).max(0.0), &ic),
            ToastKind::Notification { .. } | ToastKind::Hud { .. } | ToastKind::Privacy { .. } | ToastKind::Water | ToastKind::Meeting { .. } | ToastKind::Update(_) | ToastKind::Downloaded { .. } => {}
        }

        // Teks: geser 6 px + fade dengan jeda singkat.
        let k = ss(0.12, 0.4, t);
        let off = (1.0 - k) * 6.0;
        let tx = x0 + 66.0 + off;
        let max_w = w - 66.0 - 76.0;
        let title = if g.width_ex(&title, 15.0, true) > max_w { ellipsize(g, &title, 15.0, max_w) } else { title };
        g.txtb(&title, tx, cy - 21.0, 15.0, [255, 255, 255], al * k, max_w + 2.0);
        g.txt(&sub, tx, cy + 2.0, 12.0, GREY, al * k, max_w + 2.0);

        // Kanan: persen (charger) atau orb headphone dengan riak (Bluetooth).
        if matches!(ts.kind, ToastKind::Prayer { .. }) && self.adhan_playing() {
            let (bx, by, bw, bh) = prayer::toast_stop_rect(w, h);
            let over = self.cursor_canvas().is_some_and(|(x, y)| in_rect(x, y, bx, by, bw, bh));
            g.pill(bx, by, bw, bh, g.acc.deep, if over { 1.0 } else { 0.85 } * al * k, 1.0);
            g.txt_c(tr("Stop"), bx + bw / 2.0, by + ty(bh, 13.0), 13.0, [255, 255, 255], al * k, bw, true);
        } else if let Some(txt) = right {
            let col = if matches!(ts.kind, ToastKind::LowBattery { .. }) { ACCENT } else { [255, 255, 255] };
            let tw = g.width(&txt, 24.0);
            g.txt(&txt, x0 + w - 26.0 - tw - (1.0 - k) * -6.0, cy - 16.0, 24.0, col, al * k, 100.0);
        } else if let ToastKind::Bluetooth { connected: true, .. } = &ts.kind {
            let (ox, oy) = (x0 + w - 40.0, cy);
            for i in 0..2 {
                let ph = ((t * 0.85) + i as f32 * 0.5).rem_euclid(1.0);
                let rr = 20.0 + 13.0 * ph;
                let a = (1.0 - ph) * 0.4 * al * k;
                let p = paint(BLUE[0], BLUE[1], BLUE[2], a);
                g.ring(ox, oy, rr, 0.0, 1.5, &p, &p);
            }
            g.circle(ox, oy, 19.0 * sc, &paint(20, 40, 58, al * k));
            let edge = paint(BLUE[0], BLUE[1], BLUE[2], 0.7 * al * k);
            g.ring(ox, oy, 19.0 * sc, 0.0, 1.5, &edge, &edge);
            g.icon_headphone(ox, oy, 17.0 * sc, &paint(205, 235, 255, al * k));
        }
    }
}

impl App {
    /// Toast notifikasi Windows: ikon aplikasi, judul tebal, isi satu baris, lonceng bergoyang di kanan.
    fn draw_notif_toast(&self, g: &mut Gfx, w: f32, h: f32, al: f32, t: f32, id: u32) {
        let Some(item) = self.notifs.iter().find(|n| n.id == id) else { return };
        let (x0, cy) = (CX - w / 2.0, h / 2.0);
        let sc = 0.55 + 0.45 * pop(t).clamp(0.0, 1.2);
        let isz = 34.0 * sc;
        notif::draw_avatar(g, item, x0 + 20.0 + (34.0 - isz) / 2.0, cy - isz / 2.0, isz, al);

        let k = ss(0.12, 0.4, t);
        let tx = x0 + 66.0 + (1.0 - k) * 6.0;
        let max_w = w - 66.0 - 56.0;
        let title = if item.title.is_empty() { item.app.as_str() } else { item.title.as_str() };
        let title = if g.width_ex(title, 14.0, true) > max_w { ellipsize(g, title, 14.0, max_w) } else { title.to_string() };
        g.txtb(&title, tx, cy - 21.0, 14.0, [255, 255, 255], al * k, max_w + 2.0);
        let body = if item.body.is_empty() { item.app.clone() } else { item.body.replace('\n', " ") };
        let body = if g.width(&body, 12.0) > max_w { ellipsize(g, &body, 12.0, max_w) } else { body };
        g.txt(&body, tx, cy + 2.0, 12.0, GREY, al * k, max_w + 2.0);

        let swing = (t * 22.0).sin() * (1.0 - t / 0.9).max(0.0);
        g.icon_bell(x0 + w - 30.0, cy, 16.0 * sc, swing, &paint(255, 255, 255, al * k * 0.9));
    }
}

impl App {
    /// HUD kecil untuk tombol kunci: ikon huruf dalam ring, nama tombol, dan status On/Off berwarna.
    fn draw_hud_toast(g: &mut Gfx, w: f32, h: f32, al: f32, t: f32, kind: HudKind, on: bool) {
        let (x0, cy) = (CX - w / 2.0, h / 2.0);
        if let HudKind::Mic(s) = kind {
            Self::draw_mic_hud(g, (w, h), al, t, s);
            return;
        }
        let bar = match kind {
            HudKind::Volume(p, m) => Some((p, m, false)),
            HudKind::Brightness(p) => Some((p, false, true)),
            _ => None,
        };
        if let Some((pct, muted, sun)) = bar {
            let col = if muted { GREY } else { g.acc.main };
            let sc = 0.7 + 0.3 * pop(t).clamp(0.0, 1.2);
            let level = if muted || pct == 0 { 0 } else if pct < 34 { 1 } else if pct < 67 { 2 } else { 3 };
            if sun {
                g.icon_sun(x0 + 28.0, cy, 20.0 * sc, &paint(255, 255, 255, al));
            } else {
                g.icon_speaker(x0 + 28.0, cy, 18.0 * sc, level, muted, &paint(255, 255, 255, al));
            }
            let k = ss(0.04, 0.26, t);
            g.txtb(kind.label(), x0 + 54.0, cy - 17.0, 12.0, [200, 200, 208], al * k, 90.0);
            let (bx, by, bw) = (x0 + 54.0, cy + 4.0, w - 54.0 - 62.0);
            g.fill(&rrect(bx, by, bw, 6.0, 3.0), &paint(255, 255, 255, 0.16 * al));
            let fw = (bw * pct as f32 / 100.0).max(if pct > 0 { 6.0 } else { 0.0 });
            if fw > 0.0 {
                g.fill(&rrect(bx, by, fw, 6.0, 3.0), &paint(col[0], col[1], col[2], al));
            }
            let txt = if muted { tr("Mute").to_string() } else { format!("{pct}%") };
            let tw = g.width_ex(&txt, 14.0, true);
            g.txtb(&txt, x0 + w - 20.0 - tw, cy - 9.0, 14.0, [255, 255, 255], al * k, 60.0);
            return;
        }
        let col = if on { g.acc.main } else { GREY };
        let sc = 0.6 + 0.4 * pop(t).clamp(0.0, 1.2);
        let (rcx, r) = (x0 + 28.0, 14.0 * sc);
        // ring terisi penuh saat On, redup saat Off (sapuan singkat saat muncul)
        let sweep = 1.0 - (1.0 - ((t - 0.05) / 0.35).clamp(0.0, 1.0)).powi(3);
        let frac = if on { sweep } else { 0.0 };
        g.ring(rcx, cy, r, frac, 3.0, &paint(col[0], col[1], col[2], 0.28 * al), &paint(col[0], col[1], col[2], al));
        g.txt_c(kind.glyph(), rcx, cy - 7.0 * sc, 13.0 * sc, [255, 255, 255], al * if on { 1.0 } else { 0.55 }, 20.0, true);
        let k = ss(0.08, 0.32, t);
        g.txtb(kind.label(), x0 + 54.0 + (1.0 - k) * 6.0, cy - 9.0, 14.0, [255, 255, 255], al * k, 120.0);
        let state = if on { tr("On") } else { tr("Off") };
        let sw = g.width_ex(state, 14.0, true);
        g.txtb(state, x0 + w - 20.0 - sw, cy - 9.0, 14.0, col, al * k, 50.0);
    }
}

/// Name and state shown by the mic HUD.
pub(crate) fn mic_hud_text(s: Option<bool>) -> (&'static str, &'static str) {
    let state = match s {
        Some(true) => tr("Muted"),
        Some(false) => tr("On"),
        None => tr("Not found"),
    };
    (tr("Microphone"), state)
}

impl App {
    /// Mic HUD: a mic (crossed out while muted) in a ring, "Microphone", and the state on the right in red
    /// (muted), the accent color (on) or grey (no microphone).
    fn draw_mic_hud(g: &mut Gfx, (w, h): (f32, f32), al: f32, t: f32, s: Option<bool>) {
        let (x0, cy) = (CX - w / 2.0, h / 2.0);
        let col = match s {
            Some(true) => ACCENT,
            Some(false) => g.acc.main,
            None => GREY,
        };
        let sc = 0.6 + 0.4 * pop(t).clamp(0.0, 1.2);
        let (rcx, r) = (x0 + 28.0, 14.0 * sc);
        let sweep = 1.0 - (1.0 - ((t - 0.05) / 0.35).clamp(0.0, 1.0)).powi(3);
        let frac = if s.is_some() { sweep } else { 0.0 };
        g.ring(rcx, cy, r, frac, 3.0, &paint(col[0], col[1], col[2], 0.28 * al), &paint(col[0], col[1], col[2], al));
        let ic = paint(255, 255, 255, al * if s == Some(false) { 1.0 } else { 0.8 });
        if s == Some(false) {
            g.icon_mic(rcx, cy, 15.0 * sc, &ic);
        } else {
            g.icon_mic_off(rcx, cy, 15.0 * sc, &ic);
        }
        let k = ss(0.08, 0.32, t);
        let (label, state) = mic_hud_text(s);
        g.txtb(label, x0 + 54.0 + (1.0 - k) * 6.0, cy - 9.0, 14.0, [255, 255, 255], al * k, 120.0);
        let sw = g.width_ex(state, 14.0, true);
        g.txtb(state, x0 + w - 20.0 - sw, cy - 9.0, 14.0, col, al * k, sw + 2.0);
    }
}

/// Kotak tombol Snooze dan Drank pada toast Water berukuran (w, h), dalam koordinat kanvas.
/// The Join button on a meeting toast of size (w, h): x, y, w, h.
pub(crate) fn meeting_join_rect(w: f32, h: f32) -> (f32, f32, f32, f32) {
    let (x0, cy) = (CX - w / 2.0, h / 2.0);
    (x0 + w - 16.0 - 70.0, cy - 14.0, 70.0, 28.0)
}

fn water_btns(w: f32, h: f32) -> ((f32, f32, f32, f32), (f32, f32, f32, f32)) {
    let (x0, cy) = (CX - w / 2.0, h / 2.0);
    let drank = (x0 + w - 16.0 - 70.0, cy - 14.0, 70.0, 28.0);
    let snooze = (drank.0 - 8.0 - 76.0, cy - 14.0, 76.0, 28.0);
    (snooze, drank)
}

impl App {
    /// Toast dengan tombol yang bisa diklik (Water: Snooze/Drank, rapat: Join, pembaruan: Download): hover di atasnya tidak
    /// boleh membuka notch, karena itu membatalkan toast sebelum tombolnya sempat dipencet.
    pub(crate) fn toast_has_button(&self) -> bool {
        matches!(
            self.toast.as_ref().map(|t| &t.kind),
            Some(
                ToastKind::Water
                    | ToastKind::Meeting { .. }
                    | ToastKind::Update(update::Note::Available(_) | update::Note::Downloading(_) | update::Note::DownloadFailed(_))
                    | ToastKind::Downloaded { .. }
            )
        ) || (matches!(self.toast.as_ref().map(|t| &t.kind), Some(ToastKind::Prayer { .. })) && self.adhan_playing())
    }

    pub(crate) fn end_toast(&mut self) {
        self.toast = None;
        self.toast_end = Instant::now();
    }

    /// Klik pada toast Water: Snooze menunda, Drank mencatat satu Glass, bagian lain membuka slide Water.
    /// True bila klik dipakai oleh toast.
    pub(crate) fn toast_click(&mut self, x: f32, y: f32) -> bool {
        let Some(ts) = &self.toast else { return false };
        if ts.t0.elapsed().as_secs_f32() < 0.35 {
            return false;
        }
        if matches!(ts.kind, ToastKind::Downloaded { .. }) {
            return self.dl_toast_click(x, y);
        }
        if matches!(ts.kind, ToastKind::Prayer { .. }) && self.adhan_playing() {
            let (w, h) = TOAST_CHARGE;
            if !in_rect(x, y, CX - w / 2.0, 0.0, w, h) {
                return false;
            }
            let (bx, by, bw, bh) = prayer::toast_stop_rect(w, h);
            if in_rect(x, y, bx, by, bw, bh) {
                self.adhan_stop();
            }
            self.end_toast();
            return true;
        }
        if let ToastKind::Update(note) = &ts.kind {
            let (w, h) = TOAST_UPDATE;
            if !in_rect(x, y, CX - w / 2.0, 0.0, w, h) {
                return false;
            }
            match note.clone() {
                // one click: download the installer, check it and run it (the toast shows the progress)
                update::Note::Available(r) if r.installer.is_some() => {
                    self.update_install(&r);
                    return true;
                }
                update::Note::Available(r) | update::Note::DownloadFailed(r) => sys::open_url(&r.url),
                // nothing to press while downloading
                update::Note::Downloading(_) => return true,
                _ => {}
            }
            self.end_toast();
            return true;
        }
        if let ToastKind::Meeting { link, .. } = &ts.kind {
            let (w, h) = TOAST_MEETING;
            if !in_rect(x, y, CX - w / 2.0, 0.0, w, h) {
                return false;
            }
            let (bx, by, bw, bh) = meeting_join_rect(w, h);
            match link.clone() {
                Some(link) if in_rect(x, y, bx, by, bw, bh) => sys::open_url(&link),
                _ if self.cfg.slides.iter().any(|&(s, on)| s == SlideId::Agenda && on) => {
                    self.end_toast();
                    self.show_id(SlideId::Agenda);
                    self.peek_until = Some(Instant::now() + Duration::from_secs(8));
                    return true;
                }
                _ => {}
            }
            self.end_toast();
            return true;
        }
        if !matches!(ts.kind, ToastKind::Water) {
            return false;
        }
        let (w, h) = TOAST_WATER;
        if !in_rect(x, y, CX - w / 2.0, 0.0, w, h) {
            return false;
        }
        let (sn, dr) = water_btns(w, h);
        if in_rect(x, y, sn.0, sn.1, sn.2, sn.3) {
            self.water_snooze();
            self.end_toast();
        } else if in_rect(x, y, dr.0, dr.1, dr.2, dr.3) {
            let glass = self.water.glass as i32;
            self.water_add(glass);
            self.end_toast();
        } else {
            self.end_toast();
            self.show_id(SlideId::Water);
            self.water_view(0);
            self.peek_until = Some(Instant::now() + Duration::from_secs(8));
        }
        true
    }

    /// Posisi pointer dalam koordinat kanvas (untuk highlight tombol saat hover).
    pub(crate) fn cursor_canvas(&self) -> Option<(f32, f32)> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p).ok()? };
        Some(((p.x - self.wx) as f32 / self.scale, (p.y - self.wy) as f32 / self.scale))
    }

    /// "Time to Drink": ikon tetes, progres hari ini, dan tombol Snooze / Drank.
    fn draw_water_toast(&self, g: &mut Gfx, w: f32, h: f32, al: f32, t: f32) {
        let (x0, cy) = (CX - w / 2.0, h / 2.0);
        let sc = 0.6 + 0.4 * pop(t).clamp(0.0, 1.2);
        let (icx, r) = (x0 + 34.0, 18.0 * sc);
        // accent colours, like the Water slide
        let (dark, bright, deep) = (g.acc.dark, g.acc.bright, g.acc.deep);
        g.circle(icx, cy, r, &paint(dark[0], dark[1], dark[2], 0.5 * al));
        let edge = paint(bright[0], bright[1], bright[2], 0.55 * al);
        g.ring(icx, cy, r, 0.0, 1.5, &edge, &edge);
        g.icon_water_drop(icx, cy, 17.0 * sc, g.acc.pale, g.acc.bright, al);

        let k = ss(0.12, 0.4, t);
        let tx = x0 + 66.0 + (1.0 - k) * 6.0;
        g.txtb(tr("Time to Drink"), tx, cy - 19.0, 14.0, [255, 255, 255], al * k, 150.0);
        let wt = &self.water;
        let sub = fill(tr("Stay hydrated! \u{b7} {} / {} L"), &[&water::fmt_l(wt.ml as f32), &water::fmt_l(wt.goal as f32)]);
        g.txt(&sub, tx, cy + 2.0, 12.0, GREY, al * k, 150.0);

        let (sn, dr) = water_btns(w, h);
        let cur = self.cursor_canvas();
        let over = |b: (f32, f32, f32, f32)| cur.map_or(false, |(x, y)| in_rect(x, y, b.0, b.1, b.2, b.3));
        let ba = al * k;
        g.pill(sn.0, sn.1, sn.2, sn.3, [255, 255, 255], if over(sn) { 0.24 } else { 0.13 } * ba, 1.0);
        g.txt_c(tr("Snooze"), sn.0 + sn.2 / 2.0, sn.1 + ty(sn.3, 13.0), 13.0, [228, 232, 240], ba, sn.2, true);
        g.pill(dr.0, dr.1, dr.2, dr.3, deep, if over(dr) { 1.0 } else { 0.85 } * ba, 1.0);
        g.txt_c(tr("Drank"), dr.0 + dr.2 / 2.0, dr.1 + ty(dr.3, 13.0), 13.0, [255, 255, 255], ba, dr.2, true);
    }
}

impl App {
    /// "Microphone in use" / "Camera in use" + nama aplikasi; ikon mic/kamera dalam ring berwarna (oranye / hijau).
    /// While the microphone is muted its icon is crossed out.
    fn draw_privacy_toast(g: &mut Gfx, (w, h): (f32, f32), al: f32, t: f32, cam: bool, app: &str, muted: bool) {
        let (x0, cy) = (CX - w / 2.0, h / 2.0);
        let rgb = if cam { [48u8, 209, 88] } else { [255u8, 159, 10] };
        let sc = 0.6 + 0.4 * pop(t).clamp(0.0, 1.2);
        let (rcx, r) = (x0 + 28.0, 14.0 * sc);
        g.ring(rcx, cy, r, 1.0, 3.0, &paint(rgb[0], rgb[1], rgb[2], 0.28 * al), &paint(rgb[0], rgb[1], rgb[2], al));
        let ic = paint(rgb[0], rgb[1], rgb[2], al);
        if cam {
            g.icon_camera(rcx, cy, 14.0 * sc, &ic);
        } else if muted {
            g.icon_mic_off(rcx, cy, 14.0 * sc, &ic);
        } else {
            g.icon_mic(rcx, cy, 14.0 * sc, &ic);
        }
        let k = ss(0.08, 0.32, t);
        let label = if cam { tr("Camera in use") } else { tr("Microphone in use") };
        g.txtb(label, x0 + 54.0 + (1.0 - k) * 6.0, cy - 9.0, 14.0, [255, 255, 255], al * k, 150.0);
        let name = if g.width(app, 12.0) > 96.0 { ellipsize(g, app, 12.0, 96.0) } else { app.to_string() };
        let nw = g.width(&name, 12.0);
        g.txt(&name, x0 + w - 16.0 - nw, cy - 7.0, 12.0, GREY, al * k, 100.0);
    }
}

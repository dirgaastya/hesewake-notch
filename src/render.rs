//! Menggambar satu frame (notch kecil + slide) dan menampilkannya ke window.

use super::*;

/// Apply the notch-shape mask, but only over `bx` (cheap even at 60 fps).
fn apply_mask(g: &mut Gfx, shape: &Path, bx: Box4) {
    let Some(mut mask) = g.mask.take() else { return; };
    mask.data_mut().fill(0);
    mask.fill_path(shape, FillRule::Winding, true, g.ts);
    let pm_w = g.pm.width() as usize;
    let pm_h = g.pm.height() as usize;
    let x0 = bx.0.min(pm_w);
    let y0 = bx.1.min(pm_h);
    let x1 = bx.2.min(pm_w).max(x0);
    let y1 = bx.3.min(pm_h).max(y0);
    let mask_data = mask.data();
    let pm_data = g.pm.data_mut();
    for y in y0..y1 {
        let row = y * pm_w;
        for x in x0..x1 {
            let idx = row + x;
            let c = mask_data[idx];
            if c == 255 {
                continue;
            }
            let cf = c as u32;
            let base = idx * 4;
            pm_data[base] = ((pm_data[base] as u32 * cf + 127) / 255) as u8;
            pm_data[base + 1] = ((pm_data[base + 1] as u32 * cf + 127) / 255) as u8;
            pm_data[base + 2] = ((pm_data[base + 2] as u32 * cf + 127) / 255) as u8;
            pm_data[base + 3] = ((pm_data[base + 3] as u32 * cf + 127) / 255) as u8;
        }
    }
    g.mask = Some(mask);
}

/// Notch yang seluruh bentuknya bergelombang setelah ada item di-drop (t = detik sejak drop).
/// - Tepi bawah melorot (sag) lalu memantul, paling kuat di tengah.
/// - Riak merambat dari tengah ke kiri/kanan, meredam seiring waktu dan jarak.
/// - Sisi kiri/kanan ikut menyempit saat melorot dan melebar saat memantul.
fn notch_wave(w: f32, h: f32, t: f32) -> Path {
    use std::f32::consts::{PI, TAU};

    let ear = (h * 0.35).min(10.0);
    let rb = (h * 0.5).min(22.0);
    let l = CX - w / 2.0;
    let r = CX + w / 2.0;
    let a = r - rb; // ujung kanan tepi bawah datar
    let b = l + rb; // ujung kiri tepi bawah datar
    let span = (a - b).max(1.0);

    // Ujung animasi mulus kembali ke bentuk normal.
    let fade = 1.0 - ((t / DROP_ANIM_SECS - 0.8) / 0.2).clamp(0.0, 1.0);
    // Gerak utama: pegas teredam (turun dulu, lalu memantul).
    let sag = (-3.4 * t).exp() * (TAU * 1.5 * t).sin();
    let max_down = (H as f32 - 2.0 - h).max(0.0);

    // Pergeseran vertikal tepi bawah pada posisi x.
    let off = |x: f32| -> f32 {
        let d = (x - CX).abs();
        let e = ((x - b) / span).clamp(0.0, 1.0);
        let win = (PI * e).sin().max(0.0).sqrt(); // 0 di sudut, 1 di tengah
        let body = sag * (-(d * d) / (2.0 * 110.0 * 110.0)).exp();
        let front = ((300.0 * t - d) / 50.0).clamp(0.0, 1.0); // riak baru ada setelah gelombang sampai
        let ripple = (-1.7 * t).exp() * (-d / 200.0).exp() * (TAU * (2.0 * t - d / 110.0)).sin() * front;
        let v = (body * DROP_AMP * 0.75 + ripple * DROP_AMP * 0.35) * win * fade;
        v.clamp(-h * 0.3, max_down)
    };

    // Sisi: positif = melebar keluar, negatif = menyempit.
    let sb = (-sag * DROP_AMP * 0.2 * fade).clamp(-5.0, 5.0);
    let mid = (ear + h - rb) / 2.0;

    let mut pb = PathBuilder::new();
    pb.move_to(l - ear, 0.0);
    pb.line_to(r + ear, 0.0);
    pb.quad_to(r, 0.0, r, ear);
    pb.quad_to(r + 2.0 * sb, mid, r, h - rb);
    pb.quad_to(r, h, a, h + off(a));

    let steps = ((span / 3.0).ceil() as i32).max(8);
    for i in 1..=steps {
        let x = a - span * i as f32 / steps as f32;
        pb.line_to(x, h + off(x));
    }

    pb.quad_to(l, h, l, h - rb);
    pb.quad_to(l - 2.0 * sb, mid, l, ear);
    pb.quad_to(l, 0.0, l - ear, 0.0);
    pb.close();
    pb.finish().unwrap_or_else(dummy_path)
}

pub(crate) fn draw(a: &mut App) {
    // Kalau panic sebelumnya terjadi saat gfx sedang dipinjam, buat ulang supaya tidak panic berulang.
    let mut g = match a.gfx.take() {
        Some(g) => g,
        None => Gfx::new(a.scale),
    };
    g.acc = accent::get(a.cfg.accent);
    let (w, h) = (a.w, a.h);
    // Toast lebih tinggi dari notch kecil, tapi bukan expand: jangan turunkan progres dari tinggi.
    let hold = a.toast_hold();
    let p = if hold { 0.0 } else { ((h - 40.0) / (EXP_H - 40.0)).clamp(0.0, 1.0) };

    // Waktu animasi gelombang (None bila tidak ada drop yang sedang berjalan).
    let wave_t = a.drop_t();
    let dip = if wave_t.is_some() { DROP_AMP } else { 0.0 };

    // Only the area the notch can touch this frame (whole canvas while expanded).
    // Dihitung dalam piksel logis, lalu dikonversi ke piksel fisik kanvas.
    let (cw, ch) = (W as usize, H as usize);
    let cur_log: Box4 = if p > 0.04 {
        (0, 0, cw, ch)
    } else {
        let x1 = ((CX + w / 2.0 + 16.0).ceil().max(0.0) as usize).min(cw);
        let x0 = ((CX - w / 2.0 - 16.0).floor().max(0.0) as usize).min(x1);
        // Sisakan ruang ekstra di bawah notch supaya gelombang ikut tergambar.
        let y1 = ((h + 5.0 + dip * 1.6).ceil().max(0.0) as usize).min(ch);
        (x0, 0, x1, y1)
    };
    let cur = scale::to_phys(cur_log, g.s);
    let dirty = union_box(g.prev_box, cur);
    g.clear_box(dirty);
    g.prev_box = cur;
    g.dirty = dirty;

    let shape = match wave_t {
        Some(t) => notch_wave(w, h, t),
        None => notch_path(w, h),
    };
    g.fill(&shape, &paint(0, 0, 0, 1.0));

    // Collapsed content only shows once the notch is tall enough for it, also right after a toast, when the notch
    // may be shrinking back to the thin busy line.
    let fit = ((h - 16.0) / 12.0).clamp(0.0, 1.0);
    let ca = if hold { fit } else { (1.0 - p * 2.0).clamp(0.0, 1.0) * fit };
    if ca > 0.03 {
        a.draw_collapsed(&mut g, w, h, ca);
        if a.toast.is_none() {
            a.draw_privacy_dots(&mut g, w, h, ca);
            a.draw_download_strip(&mut g, w, h, ca);
        }
    }
    if p > 0.04 {
        let pos = a.sp - a.drag_dx / 160.0;
        let mut vis: Vec<(usize, f32, f32)> = Vec::new();
        let n = a.n_slides();
        for i in 0..n {
            let d = cyc(i as f32 - pos, n);
            if d.abs() >= 1.0 {
                continue;
            }
            vis.push((i, d * 48.0, (1.0 - d.abs()) * p));
        }
        for &(i, _, al) in &vis {
            match a.order[i] {
                SlideId::Music => a.music_glow(&mut g, &shape, w, h, al),
                // Other slides: one glow color (derived from the accent) so the slides feel consistent.
                _ => {
                    let c = g.acc.glow;
                    glow(&mut g, &shape, h, (c[0], c[1], c[2]), 0.40 * al)
                }
            }
        }
        for &(i, ox, al) in &vis {
            match a.order[i] {
                SlideId::Music => a.slide_music(&mut g, ox, al),
                SlideId::Water => a.slide_water(&mut g, ox, al),
                SlideId::Todo => a.slide_todo(&mut g, ox, al),
                SlideId::Ledge => a.slide_shelf(&mut g, ox, al),
                SlideId::Notifications => a.slide_notifs(&mut g, ox, al),
                SlideId::Settings => a.slide_settings(&mut g, ox, al),
                SlideId::System => a.slide_system(&mut g, ox, al),
                SlideId::Timer => a.slide_timer(&mut g, ox, al),
                SlideId::Scrap => a.slide_note(&mut g, ox, al),
                SlideId::Clipboard => a.slide_clipboard(&mut g, ox, al),
                SlideId::Agenda => a.slide_agenda(&mut g, ox, al),
                SlideId::Prayer => a.slide_prayer(&mut g, ox, al),
            }
        }
        if !a.lyric_screen() {
            a.draw_nav(&mut g, p);
            a.draw_dots(&mut g, pos, p);
        }
    }
    apply_mask(&mut g, &shape, cur);
    a.gfx = Some(g);
}

pub(crate) fn glow(g: &mut Gfx, shape: &Path, h: f32, rgb: (u8, u8, u8), a: f32) {
    if a <= 0.01 {
        return;
    }
    let c = |al: f32| Color::from_rgba8(rgb.0, rgb.1, rgb.2, (al.clamp(0.0, 1.0) * 255.0) as u8);
    let stops = vec![GradientStop::new(0.0, c(a)), GradientStop::new(0.55, c(a * 0.28)), GradientStop::new(1.0, c(0.0))];
    if let Some(sh) =
        LinearGradient::new(Point::from_xy(0.0, h), Point::from_xy(0.0, h * 0.08), stops, SpreadMode::Pad, Transform::identity())
    {
        let mut p = Paint::default();
        p.shader = sh;
        p.anti_alias = true;
        g.fill(shape, &p);
    }
}

pub(crate) unsafe fn present(a: &mut App) {
    let t_draw = Instant::now();
    draw(a);
    let draw_dur = t_draw.elapsed();
    let t_blit = Instant::now();
    let Some(g) = a.gfx.as_ref() else { return; };
    // Bitmap window selalu seukuran kanvas (keduanya dibuat ulang bersama di `rescale`).
    let (pw, ph) = (g.pm.width() as usize, g.pm.height() as usize);
    if scale::canvas(a.scale) != (pw as i32, ph as i32) {
        return; // jangan pernah menulis melewati bitmap window
    }
    let dst = std::slice::from_raw_parts_mut(a.bits as *mut u8, pw * ph * 4);
    // RGBA (premultiplied) -> BGRA, only for the rows/columns that changed.
    let (bx0, by0, bx1, by1) = g.dirty;
    let pix = g.pm.data();
    for y in by0..by1.min(ph) {
        let (lo, hi) = ((y * pw + bx0) * 4, (y * pw + bx1.min(pw)) * 4);
        if hi <= lo {
            continue;
        }
        for (d, s) in dst[lo..hi].chunks_exact_mut(4).zip(pix[lo..hi].chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }
    let size = SIZE { cx: pw as i32, cy: ph as i32 };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let _ = UpdateLayeredWindow(
        HWND(a.hwnd as *mut c_void),
        HDC::default(),
        None,
        Some(&size),
        HDC(a.mem_dc as *mut c_void),
        Some(&src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );
    perf::frame(draw_dur, t_blit.elapsed());
}

impl App {
    fn draw_collapsed(&self, g: &mut Gfx, w: f32, h: f32, ca: f32) {
        if self.toast.is_some() {
            self.draw_toast(g, w, h, ca);
        } else if self.media.playing {
            let x0 = CX - w / 2.0;
            self.collapsed_cover(g, x0 + 14.0, (h - 20.0) / 2.0, 20.0, 5.0, ca);
            for i in 0..4 {
                let bh = 4.0 + 12.0 * (0.5 + 0.5 * (self.t * 6.0 + i as f32 * 1.7).sin());
                g.rect(x0 + w - 34.0 - self.priv_extra() + i as f32 * 6.0, h / 2.0 - bh / 2.0, 3.0, bh, &g.sacc(ca));
            }
            let (rl, mut rr) = (x0 + 44.0, x0 + w - 44.0 - self.bell_extra() - self.priv_extra());
            // Prayer countdown: right-aligned before the bell/equalizer; the title/lyrics make room.
            if let Some((lbl, col)) = self.countdown_text(g) {
                let tw = g.width(&lbl, 12.0);
                g.txt(&lbl, rr - tw, ty(h, 12.0), 12.0, col, ca, tw + 2.0);
                rr -= prayer::PLAY_EXTRA;
            }
            let rw = rr - rl;
            let lyric = self.lyric_text();
            let title = lyric.unwrap_or(self.media.title.as_str());
            // Baris lirik: fade-in saat ganti; marquee mulai dari awal tiap baris baru.
            let ca = if lyric.is_some() { ca * self.lyric_alpha() } else { ca };
            let t_el = if lyric.is_some() { self.lyric_t.elapsed().as_secs_f32() } else { self.t - self.title_t0 };
            if lyric.is_some() {
                // Lirik: tebal, tanpa running text. Notch sudah dilebarkan (lihat `target`); bila masih
                // kelewat panjang dipotong dengan elipsis.
                let size = LYRIC_SIZE;
                let y = ty(h, size);
                let mut line = title.to_string();
                if g.width_ex(&line, size, true) > rw {
                    line = ellipsize(g, &line, size, rw);
                }
                g.txt_c(&line, (rl + rr) / 2.0, y, size, [255, 255, 255], ca, rw + 2.0, true);
                self.draw_bell(g, ca);
                return;
            }
            let size = 12.0;
            let tw = g.width(title, size);
            let y = ty(h, size);
            if tw <= rw {
                g.txt(title, rl + (rw - tw) / 2.0, y, size, [255, 255, 255], ca, rw + 2.0);
            } else {
                let cycle = tw + MARQUEE_GAP;
                let period = MARQUEE_PAUSE + cycle / MARQUEE_SPEED;
                let e = t_el.max(0.0) % period;
                let off = if e < MARQUEE_PAUSE { 0.0 } else { (e - MARQUEE_PAUSE) * MARQUEE_SPEED };
                let (cl, cr) = (rl - 6.0, rr + 6.0);
                g.txt_clip(title, rl - off, y, size, [255, 255, 255], ca, cl, cr, 6.0);
                g.txt_clip(title, rl - off + cycle, y, size, [255, 255, 255], ca, cl, cr, 6.0);
            }
            self.draw_bell(g, ca);
        } else if let Some(st) = self.splash_t() {
            // Startup splash: eye + glow, cross-fading into the clock during the last SPLASH_FADE seconds.
            self.draw_eye(g, w, h, ca);
            let cf = ((st - (SPLASH_SECS - SPLASH_FADE)) / SPLASH_FADE).clamp(0.0, 1.0);
            if cf > 0.0 {
                self.clock_text(g, w, h, ca * cf);
            }
        } else {
            match self.idle_phase() {
                IdlePhase::Clock => {
                    self.draw_clock_collapsed(g, w, h, ca);
                    self.draw_weather(g, w, h, ca);
                }
                IdlePhase::Eye => self.draw_eye(g, w, h, ca),
            }
            self.draw_timer_chip(g, w, h, ca);
            self.draw_bell(g, ca);
        }
    }
}
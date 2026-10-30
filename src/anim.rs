//! Loop animasi: ukuran target notch, timer, polling hover/busy/media.

use super::*;

// ---------------------------------------------------------------- animation
pub(crate) fn target(a: &App) -> (f32, f32) {
    if a.expanded() {
        (EXP_W, EXP_H)
    } else if a.toast.is_some() {
        a.toast_size()
    } else if a.thin() {
        THIN
    } else if a.media.playing && a.lyric_w > 0.0 {
        // Lebar menyesuaikan baris lirik (ruang teks = lebar - 88 px untuk ikon dan equalizer, + lonceng).
        let extra = a.bell_extra() + a.priv_extra() + a.prayer_play_extra();
        ((a.lyric_w + 88.0 + 16.0 + extra).clamp(PLAYING.0 + extra, EXP_W), PLAYING.1)
    } else if a.media.playing {
        (PLAYING.0 + a.bell_extra() + a.priv_extra() + a.prayer_play_extra(), PLAYING.1)
    } else {
        (IDLE.0 + a.priv_extra() + a.timer_extra() + a.prayer_idle_extra(), IDLE.1)
    }
}

fn settled(a: &App) -> bool {
    let (tw, th) = target(a);
    (a.w - tw).abs() < 0.3
        && (a.h - th).abs() < 0.3
        && a.vw.abs() < 2.0
        && a.vh.abs() < 2.0
        && (a.sp - a.slide as f32).abs() < 0.003
        && a.sv.abs() < 0.05
}

/// Timer: 16 ms saat bergerak / animasi droplet / mid-blink, 40 ms saat mata istirahat (glow mengalir),
/// 33 ms untuk equalizer, dan mati kalau tidak ada yang bergerak.
pub(crate) unsafe fn schedule(a: &mut App) {
    let pulsing = a.drop_pulse.map_or(false, |t| t.elapsed().as_secs_f32() < DROP_ANIM_SECS);
    let want = if a.display_off {
        0
    } else if pulsing {
        16
    } else if !settled(a) || a.lyric_fading() || a.art_fading() || a.toast.is_some() || a.water_animating() || a.lyric_scroll_animating() || a.bell_animating() || a.notif_ui_animating() || a.settings_animating() || a.privacy_animating() || a.vol_overlay_active() || a.sys_animating() || a.weather_animating() || a.timer_animating() || a.clip_animating() {
        16
    } else if a.expanded() && a.is_slide(SlideId::Music) && a.tint.animating() {
        16 // artwork colour cross-fade on the Music slide
    } else if a.media.playing && !a.expanded() && !a.busy {
        33
    } else if a.splash_t().is_some() && !a.media.playing {
        a.eye.frame_ms(0.0)
    } else if a.idle() && a.idle_phase() == IdlePhase::Eye {
        // 60 fps while blinking / glancing, ~12 fps at rest, nothing while nobody is at the computer
        a.eye.frame_ms(sys::input_idle_secs())
    } else if a.idle() && a.idle_phase() == IdlePhase::Clock && a.idle_secs() < CLOCK_FADE_IN + 0.1 {
        33 // clock fade-in right after the eye phase
    } else {
        0
    };
    if want != a.interval {
        let hwnd = HWND(a.hwnd as *mut c_void);
        if want == 0 {
            // Tidak trim working set di sini: tiap animasi selesai, halaman yang dibuang harus
            // di-page-fault lagi saat animasi berikutnya (patah-patah). Trim jarang di poll().
            if pace::available() {
                pace::cancel();
            } else {
                let _ = KillTimer(hwnd, 1);
            }
        } else if pace::available() {
            pace::set(want);
        } else {
            SetTimer(hwnd, 1, want, None);
        }
        a.interval = want;
    }
}

unsafe fn poll(a: &mut App) {
    a.tick_n = a.tick_n.wrapping_add(1);
    let mut changed = false;
    let mut redraw = false;
    if splash_tick(a) {
        redraw = true;
    }
    if a.lyric_update() {
        redraw = true;
    }
    if a.toast_update() {
        redraw = true;
    }
    if a.settings_open && settled(a) && a.settings_autoclose() {
        redraw = true;
    }
    a.notif_ui_step();
    if a.tick_n % 5 == 0 && a.monitor_follow() {
        redraw = true;
    }
    if a.sys_sample() {
        redraw = true;
    }
    if a.timer_poll() {
        redraw = true;
    }
    // pergantian tahap suhu <-> ikon memicu transisi
    if a.weather_shown() {
        let idx = a.weather_stage_idx();
        if idx != a.weather_idx {
            a.weather_idx = idx;
            redraw = true;
        }
    }
    if a.splash.is_none() {
        if let Some(k) = a.demo_toast.take() {
            if let toast::ToastKind::Privacy { cam, app } = &k {
                if *cam {
                    a.priv_cam = vec![app.clone()];
                } else {
                    a.priv_mic = vec![app.clone()];
                }
                a.priv_t = Instant::now();
            }
            if let toast::ToastKind::Notification { id } = &k {
                // `--demo-text=judul|isi`: uji tampilan teks non-Latin (Jepang, Korea, Mandarin)
                let demo_text = std::env::args()
                    .find_map(|a| a.strip_prefix("--demo-text=").map(str::to_string))
                    .and_then(|t| t.split_once('|').map(|(a, b)| (a.to_string(), b.to_string())))
                    .unwrap_or_else(|| ("Alice".to_string(), "Are we still on for dinner tonight?".to_string()));
                a.notifs.insert(0, notif::NotifItem {
                    id: *id,
                    app: "Messages".into(),
                    title: demo_text.0,
                    body: demo_text.1,
                    at_unix: notif::now_unix(),
                    unread: true,
                    icon: None,
                    avatar: None,
                    dismiss: None,
                });
                a.notif_access = notif::Access::Allowed;
                a.bell_t = Instant::now();
            }
            a.push_toast(k);
            redraw = true;
        }
    }
    if a.splash.is_none() {
        if let Some(n) = a.demo_slide.take() {
            if n == usize::MAX {
                a.open_settings();
            } else {
                a.show(n);
            }
            a.peek_until = Some(Instant::now() + Duration::from_secs(25));
            redraw = true;
        }
    }
    // Baterai rendah tidak selalu memicu pesan power; cek berkala (~30 dtk).
    if a.tick_n % 300 == 0 {
        a.power_check();
    }

    // Animasi droplet selesai -> lepas state
    if let Some(t) = a.drop_pulse {
        if t.elapsed().as_secs_f32() >= DROP_ANIM_SECS {
            a.drop_pulse = None;
        }
        redraw = true;
    }

    // Idle clock: start/stop the stopwatch, wrap it once per clock+eye cycle.
    let now = Instant::now();
    let idle_now = a.idle();
    if idle_now {
        // The idle stopwatch only runs while idle, and not during the startup splash.
        if a.idle_mark.is_none() && a.splash.is_none() {
            a.idle_mark = Some(now);
        }
    } else if a.idle_mark.take().is_some() {
        // Ada aktivitas (notch dibuka, toast, musik, app layar penuh): saat idle lagi, siklus mulai dari jam
        // (bukan melanjutkan fase mata yang tertunda).
        a.idle_base = 0.0;
    }
    let secs = a.idle_secs();
    if secs >= IDLE_CYCLE_SECS {
        a.idle_base = secs % IDLE_CYCLE_SECS;
        if a.idle_mark.is_some() {
            a.idle_mark = Some(now);
        }
    }
    let cur_phase = a.idle_phase();
    let resumed = idle_now && !a.was_idle;
    a.was_idle = idle_now;
    if cur_phase != a.cur_phase || resumed {
        a.cur_phase = cur_phase;
        if cur_phase == IdlePhase::Eye {
            a.eye = EyeAnim::new(); // eyes pop open again
        }
        redraw = true;
    }

    if !a.menu_open {
        let mut p = POINT::default();
        if GetCursorPos(&mut p).is_ok() {
            let x = (p.x - a.wx) as f32 / a.scale;
            let y = (p.y - a.wy) as f32 / a.scale;
            if a.pointer_step(x, y) {
                changed = true;
            }
        }
    }
    if a.tick_n % 5 == 0 {
        let b = is_busy(HWND(a.hwnd as *mut c_void), a.cfg.busy_on_maximized, &a.mon);
        if b != a.busy {
            a.busy = b;
            changed = true;
        }
        let f = busy::fg_state(HWND(a.hwnd as *mut c_void), &a.mon);
        if f != a.fg {
            a.fg = f;
            changed = true;
        }
    }
    // Window TOPMOST bisa "tenggelam" (Start menu, taskbar, app fullscreen, explorer restart, dll).
    // Cek tiap ~1 detik; kalau flag topmost hilang, pasang lagi tanpa merebut fokus.
    if a.tick_n % 10 == 0 {
        let hwnd = HWND(a.hwnd as *mut c_void);
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOPMOST.0 == 0 {
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
    // Update check: looked at once a minute, runs at most every 24 hours.
    if a.tick_n % 600 == 30 {
        a.update_tick(Instant::now());
    }
    if a.tick_n % 50 == 0 && a.water.roll() {
        redraw = true;
    }
    // Trim working set ±tiap 60 detik. Tidak lagi menunggu timer animasi mati: saat musik main timer tidak pernah
    // mati, sehingga memori sisa decode (cover, ikon) tidak pernah dikembalikan.
    if a.tick_n % 600 == 0 {
        trim_memory();
    }
    if let Some(t) = a.peek_until {
        if Instant::now() >= t {
            a.peek_until = None;
            a.reminding = false;
            changed = true;
        }
    }
    if check_reminder(a) {
        changed = true;
    }

    let st = now_tm();
    let expanded = a.expanded();
    let cur = a.cur();
    // Slide 0 (Music + jam) dan slide 2 (To-do) menampilkan menit/tanggal.
    if expanded && (cur == 0 || cur == 2) && st.wMinute != a.last_min {
        redraw = true;
    }
    if !expanded && !a.thin() && !a.media.playing && cur_phase == IdlePhase::Clock && st.wMinute != a.last_min {
        redraw = true;
    }
    // Minute change: prayer schedule (toast when a prayer time starts, countdown ticks down).
    if st.wMinute != a.last_min {
        a.prayer_minute();
        a.meeting_minute(notif::now_unix() as i64);
        if a.countdown_on() || a.toast.is_some() || (expanded && a.is_slide(SlideId::Agenda)) {
            redraw = true;
        }
    }
    a.last_sec = st.wSecond;
    a.last_min = st.wMinute;

    if let Some(t) = a.shelf_flash {
        if t.elapsed() >= Duration::from_millis(1800) {
            a.shelf_flash = None;
            redraw = true;
        }
    }
    if a.editing {
        let on = (a.edit_start.elapsed().as_millis() / 500) % 2 == 0;
        if on != a.caret_on {
            a.caret_on = on;
            redraw = true;
        }
    }

    if changed || redraw {
        present(a);
    }
    if changed {
        a.last = Instant::now();
    }
    schedule(a);
}

/// Time steps for one frame `raw` seconds after the previous one: (step for the UI springs and fades, capped at
/// 50 ms so a long pause does not jump them; step for the clock, the eye and its glow, which follows real time at the
/// slower resting frame rate too).
pub(crate) fn frame_dt(raw: f32) -> (f32, f32) {
    (raw.min(0.05), raw.min(0.15))
}

pub(crate) unsafe fn tick(id: usize) {
    with_app(|a| {
        if id == 2 {
            poll(a);
            return;
        }
        splash_tick(a);
        a.lyric_update();
        a.toast_update();
        let now = Instant::now();
        let (dt, clock_dt) = frame_dt((now - a.last).as_secs_f32());
        a.last = now;
        a.t += clock_dt;
        a.water_ui_step(dt);
        a.lyric_scroll_step(dt);
        a.settings_ui_step(dt);
        a.sys_step(dt);
        a.notif_ui_step();

        // Only advance the eye once the notch is back to its small size, so the pop-open is actually seen.
        let eye_on = (a.splash_t().is_some() && !a.media.playing) || (a.idle() && a.idle_phase() == IdlePhase::Eye);
        if eye_on && a.h < IDLE.1 + 10.0 {
            a.eye.update(clock_dt);
        }

        let (tw, th) = target(a);
        const K: f32 = 380.0;
        const C: f32 = 40.0;
        const SK: f32 = 320.0;
        const SC: f32 = 36.0;
        let n = ((dt / 0.008).ceil() as i32).max(1);
        let sdt = dt / n as f32;
        for _ in 0..n {
            a.vw += (K * (tw - a.w) - C * a.vw) * sdt;
            a.vh += (K * (th - a.h) - C * a.vh) * sdt;
            a.w += a.vw * sdt;
            a.h += a.vh * sdt;
            a.sv += (SK * (a.slide as f32 - a.sp) - SC * a.sv) * sdt;
            a.sp += a.sv * sdt;
        }
        a.w = a.w.max(20.0);
        a.h = a.h.max(2.0);
        if settled(a) {
            a.w = tw;
            a.h = th;
            a.vw = 0.0;
            a.vh = 0.0;
            a.sp = a.slide as f32;
            a.sv = 0.0;
            let n = a.n_slides() as i32;
            let k = a.slide.div_euclid(n) * n;
            a.slide -= k;
            a.sp -= k as f32;
            a.settings_autoclose();
        }
        present(a);
        schedule(a);
    });
}

#[cfg(test)]
mod frame_tests {
    use super::*;

    #[test]
    fn eye_and_glow_follow_real_time_at_the_resting_rate() {
        let (ui, clock) = frame_dt(eye::EYE_REST_MS as f32 / 1000.0);
        assert!((clock - 0.083).abs() < 0.001, "the glow does not slow down at 12 fps");
        assert_eq!(ui, 0.05, "UI springs keep their 50 ms cap");
        assert_eq!(frame_dt(0.016), (0.016, 0.016));
        assert_eq!(frame_dt(5.0).1, 0.15, "after a long pause nothing jumps far");
    }
}

impl App {
    /// The notch dodges the pointer over this kind of foreground window (setting "Dodge cursor"). Whenever the notch is
    /// thin because of a busy window (fullscreen, an Adobe app, or a maximized one with "Thin on maximize" on), the
    /// pointer must not open it either: that thin line is the "minimal notch" the pointer should be able to pass.
    pub(crate) fn dodge_applies(&self) -> bool {
        match self.cfg.dodge {
            settings::DODGE_FULLSCREEN => self.fg == 2 || self.busy,
            settings::DODGE_MAXIMIZED => self.fg >= 1 || self.busy,
            _ => false,
        }
    }

    /// The pointer is at (`x`, `y`) in canvas coordinates: open the notch when it comes near, close it when it leaves, or,
    /// over a fullscreen / maximized window with "Dodge pointer" on, step aside (thin, never expanded) so the window's own
    /// tabs and buttons under the notch can be reached. True if something changed.
    pub(crate) fn pointer_step(&mut self, x: f32, y: f32) -> bool {
        let a = self;
        let mut changed = false;
        if a.dodge_applies() && !a.held_open() {
            // the area of the notch as it is without the pointer (the thin line when it is already thin)
            let fresh = if a.busy { (a.w.max(THIN.0) + 60.0, 10.0) } else { (a.w.max(IDLE.0) + 12.0, a.h.max(IDLE.1) + 6.0) };
            let (zw, zh) = if a.dodging { a.dodge_zone } else { fresh };
            let inside = (x - CX).abs() <= zw / 2.0 && y >= 0.0 && y <= zh;
            if inside != a.dodging {
                a.dodging = inside;
                if inside {
                    a.dodge_zone = fresh;
                }
                changed = true;
            }
            if a.hover {
                a.hover = false;
                a.out_close();
                changed = true;
            }
            return changed;
        }
        if a.dodging {
            a.dodging = false;
            changed = true;
        }
        let (zw, zh) = if a.hover || a.expanded() {
            (EXP_W + 20.0, EXP_H + 8.0)
        } else if a.thin() {
            (a.w.max(THIN.0) + 60.0, 10.0)
        } else {
            (a.w.max(IDLE.0) + 12.0, a.h.max(IDLE.1) + 6.0)
        };
        // Toast dengan tombol (Water, rapat, pembaruan): hover di atasnya tidak boleh membuka notch (itu membatalkan toast-nya).
        let inside = (x - CX).abs() <= zw / 2.0 && y >= 0.0 && y <= zh && !a.toast_has_button();
        if inside != a.hover {
            // Pointer masuk tepat di lonceng saat ada yang belum dibaca: buka langsung slide Notifications.
            let to_notif = inside
                && a.unread_count() > 0
                && a.bell_pos().map_or(false, |(bx, by)| (x - bx).abs() < 14.0 && (y - by).abs() < 14.0);
            a.hover = inside;
            if !inside {
                a.out_close();
            }
            changed = true;
            if to_notif {
                a.show_id(SlideId::Notifications);
            } else if inside {
                a.dl_on_hover();
            }
        }
        changed
    }
}

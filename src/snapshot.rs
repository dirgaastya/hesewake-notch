//! UI snapshots for visual review without running the exe. Each test builds one `App` state (no window),
//! draws it with `render::draw` into an in-memory canvas, and checks that the notch was actually drawn.
//! When the `SNAPSHOT_DIR` env var is set (CI does this), the result is saved as a PNG over a grey background
//! so the black notch edges stay visible; CI uploads them as the `ui-snapshots` artifact.
//!
//! Add a new state with one `#[test]` function that calls `shot`.

use super::*;
use tiny_skia::PixmapPaint;

/// An App with no window, user data or background threads: every slide enabled, splash already over.
/// Weather never shows (tests have no weather data), so the idle notch is always in its "no weather" state.
fn test_app(s: f32) -> App {
    let (tx, _rx) = channel();
    let mut cfg = Settings::default();
    for (_, on) in cfg.slides.iter_mut() {
        *on = true;
    }
    let past = Instant::now() - Duration::from_secs(10);
    App {
        hwnd: 0,
        wx: 0,
        wy: 0,
        mon: RECT::default(),
        mem_dc: 0,
        bmp: 0,
        bits: 0,
        scale: s,
        scale_forced: Some(s),
        gfx: Some(scale::new_gfx(s)),
        w: IDLE.0,
        h: IDLE.1,
        vw: 0.0,
        vh: 0.0,
        slide: 0,
        sp: 0.0,
        sv: 0.0,
        drag: None,
        drag_dx: 0.0,
        shelf_drag: None,
        last_wheel: past,
        t: 0.0,
        last: Instant::now(),
        interval: 0,
        tick_n: 0,
        last_sec: 0,
        last_min: 0,
        hover: false,
        busy: false,
        minimal: false,
        toast: None,
        bt_shared: Arc::new(Mutex::new(Vec::new())),
        notifs: Vec::new(),
        notif_access: notif::Access::Unknown,
        notif_slot: Arc::new(Mutex::new(Vec::new())),
        notif_on: false,
        notif_started: false,
        notif_scroll: 0,
        nui: notifview::NotifUi::new(),
        bell_t: past,
        toast_q: Vec::new(),
        toast_end: past,
        ac_prev: None,
        low_armed: true,
        demo_toast: None,
        demo_slide: None,
        display_off: false,
        menu_open: false,
        lock: Lock::Off,
        lock_menu: false,
        peek_until: None,
        reminding: false,
        prayer: None,
        prayer_fired: None,
        prayer_sun: 0,
        adhan_until: None,
        splash: None,
        // the idle clock has finished fading in (clock phase, see `idle_phase`)
        idle_base: 5.0,
        idle_mark: None,
        was_idle: false,
        eye: EyeAnim::new(),
        eye_paths: build_eye_paths(),
        cur_phase: IdlePhase::Clock,
        shared: Arc::new(Mutex::new(Media::default())),
        art_shared: Arc::new(Mutex::new(None)),
        lyr_shared: Arc::new(Mutex::new(None)),
        lyrics: lyrics::Lyrics::None,
        lyric_idx: None,
        lyric_w: 0.0,
        lyric_mode: false,
        lyric_mode_t0: past,
        lyric_scroll: 0.0,
        lyric_sv: 0.0,
        lyric_t: Instant::now(),
        pos_at: Instant::now(),
        media: Media::default(),
        art: None,
        prev_art: None,
        art_t0: past,
        tint: music::Tint::default(),
        icon: None,
        art_key: String::new(),
        title_t0: 0.0,
        todos: vec![(true, "Display DPI scaling".into()), (false, "Pomodoro mode".into()), (false, "Clipboard history".into())],
        todo_scroll: 0,
        edit_mode: false,
        editing: false,
        edit_idx: None,
        input: String::new(),
        caret_on: true,
        edit_start: Instant::now(),
        prev_fg: 0,
        shelf: Vec::new(),
        shelf_flash: None,
        clips: clipboard::History::default(),
        clip_scroll: 0,
        clip_flash: None,
        cal_link: None,
        cal_status: calendar::CalStatus::Off,
        cal: Vec::new(),
        cal_fired: Vec::new(),
        cal_paste_err: None,
        cal_confirm: None,
        agenda_scroll: 0,
        update: None,
        update_last: None,
        update_busy: false,
        drop_pulse: None,
        note: String::new(),
        note_mode: false,
        nv: NoteView::default(),
        water: Water::parse("", &clock::today()),
        wui: WaterUi::new(),
        tm: timer::TimerState::new(timer::TMode::Countdown, 300),
        tx,
        priv_mic: Vec::new(),
        priv_cam: Vec::new(),
        priv_t: past,
        mic_muted: Some(false),
        dls: Vec::new(),
        dl_seen: false,
        vol_t: past,
        bright_seen: false,
        vol_state: (0, false),
        outs: output::Outputs::default(),
        out_open: false,
        out_scroll: 0,
        out_t: past,
        out_acc: 0,
        order: slides::visible_order(&slides::effective(&cfg.slides, cfg.prayer_on), false),
        settings_open: false,
        settings_prev: None,
        sui: settingsview::SettingsUi::new(),
        sys_ui: sysview::SysUi::new(),
        weather_idx: 0,
        cfg,
    }
}

/// Open slide `id` in the expanded notch.
fn open_slide(a: &mut App, id: SlideId) {
    a.hover = true;
    a.slide = a.order.iter().position(|&s| s == id).expect("slide is enabled") as i32;
}

/// A toast whose pop-in animation has finished.
fn show_toast(a: &mut App, kind: ToastKind) {
    a.toast = Some(Toast { kind, t0: Instant::now() - Duration::from_millis(1500), dur: 60.0 });
}

/// Draw one state at scale `s`, check it has content, and save it as `<name>@<percent>.png` when asked.
fn shot(name: &str, s: f32, setup: impl FnOnce(&mut App)) {
    let mut a = test_app(s);
    setup(&mut a);
    let (w, h) = anim::target(&a);
    (a.w, a.h) = (w, h);
    a.sp = a.slide as f32;
    render::draw(&mut a);
    let g = a.gfx.as_ref().expect("gfx");
    let opaque = g.pm.data().chunks_exact(4).filter(|p| p[3] == 255).count();
    assert!(opaque > (w * h * s * s) as usize / 2, "{name}: notch is almost empty ({opaque} pixels)");

    let Some(dir) = std::env::var_os("SNAPSHOT_DIR") else { return };
    let mut out = Pixmap::new(g.pm.width(), g.pm.height()).expect("canvas size");
    out.fill(Color::from_rgba8(118, 124, 134, 255));
    out.draw_pixmap(0, 0, g.pm.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    std::fs::create_dir_all(&dir).expect("create snapshot folder");
    let file = std::path::Path::new(&dir).join(format!("{name}@{}.png", (s * 100.0).round() as u32));
    out.save_png(&file).expect("save png");
}

const SCALES: [f32; 2] = [1.0, 1.5];

#[test]
fn snap_idle_clock() {
    for s in SCALES {
        shot("idle-clock", s, |_| {});
    }
}

#[test]
fn snap_idle_eye() {
    shot("idle-eye", 1.0, |a| {
        a.cfg.idle_display = 2;
        // run 3 seconds of animation: the eyes have appeared and are open
        for _ in 0..180 {
            a.eye.update(1.0 / 60.0);
        }
        a.t = 3.0;
    });
}

#[test]
fn snap_playing() {
    shot("playing", 1.0, |a| {
        a.media = Media { title: "Almost Easy".into(), artist: "Avenged Sevenfold".into(), playing: true, pos: 60, dur: 240, pos_ms: 60_000, source: "Demo".into(), sources: 1 };
    });
}

#[test]
fn snap_timer_slide() {
    for s in SCALES {
        shot("timer-countdown", s, |a| open_slide(a, SlideId::Timer));
    }
    shot("timer-stopwatch", 1.0, |a| {
        a.tm.set_mode(timer::TMode::Stopwatch);
        a.tm.start(Instant::now() - Duration::from_millis(83_400));
        open_slide(a, SlideId::Timer);
    });
}

#[test]
fn snap_timer_chip_idle() {
    for s in SCALES {
        shot("timer-chip", s, |a| a.tm.start(Instant::now() - Duration::from_secs(61)));
    }
}

#[test]
fn snap_timer_toast() {
    shot("toast-timer", 1.0, |a| show_toast(a, ToastKind::Timer { title: "Time's up".into(), label: "5 min timer".into() }));
}

/// A Pomodoro that has finished `done` focus sessions and is in phase `phase`, running for `secs` seconds.
fn pomo(a: &mut App, done: u32, phase: timer::Phase, secs: u64) {
    a.tm.set_mode(timer::TMode::Pomodoro);
    a.tm.done = done;
    a.tm.phase = phase;
    a.tm.start(Instant::now() - Duration::from_secs(secs));
}

#[test]
fn snap_pomodoro() {
    for s in SCALES {
        shot("pomo-focus", s, |a| {
            pomo(a, 2, timer::Phase::Focus, 9 * 60 + 12);
            open_slide(a, SlideId::Timer);
        });
        shot("pomo-chip-focus", s, |a| pomo(a, 2, timer::Phase::Focus, 9 * 60 + 12));
    }
    shot("pomo-short-break", 1.0, |a| {
        pomo(a, 3, timer::Phase::Short, 70);
        open_slide(a, SlideId::Timer);
    });
    shot("pomo-long-break", 1.0, |a| {
        pomo(a, 4, timer::Phase::Long, 70);
        open_slide(a, SlideId::Timer);
    });
    shot("pomo-paused-fresh", 1.0, |a| {
        a.tm.set_mode(timer::TMode::Pomodoro);
        open_slide(a, SlideId::Timer);
    });
    shot("pomo-chip-break", 1.0, |a| pomo(a, 1, timer::Phase::Short, 70));
    shot("toast-pomo-focus-done", 1.0, |a| {
        show_toast(a, ToastKind::Timer { title: "Focus #4 done".into(), label: "Long break \u{b7} 15 min".into() })
    });
    shot("toast-pomo-break-over", 1.0, |a| show_toast(a, ToastKind::Timer { title: "Break over".into(), label: "Focus \u{b7} 25 min".into() }));
}

#[test]
fn snap_slide_todo() {
    shot("slide-todo", 1.0, |a| open_slide(a, SlideId::Todo));
}

#[test]
fn snap_slide_water() {
    shot("slide-water", 1.0, |a| open_slide(a, SlideId::Water));
}

#[test]
fn snap_slide_system() {
    shot("slide-system", 1.0, |a| open_slide(a, SlideId::System));
}

#[test]
fn snap_slide_music_empty() {
    shot("slide-music-empty", 1.0, |a| open_slide(a, SlideId::Music));
}

/// Open Settings on tab `tab`. Toggle knobs are snapped to their real state while the slide is still hidden,
/// just like when the app opens Settings.
fn open_settings_tab(a: &mut App, tab: u8) {
    a.open_settings();
    a.sui.tab = tab;
    a.settings_ui_step(0.0);
    a.hover = true;
}

#[test]
fn snap_settings() {
    for tab in 0..4u8 {
        shot(&format!("settings-tab{tab}"), 1.0, |a| open_settings_tab(a, tab));
    }
}

/// Settings › Widgets with a long city name: the stepper grows to the left instead of cutting the name.
#[test]
fn snap_settings_long_city() {
    for (name, city) in [("settings-city-short", "Jakarta"), ("settings-city-long", "Balikpapan"), ("settings-city-longest", "Bandar Lampung")] {
        let i = weather::CITIES.iter().position(|c| c.0 == city).unwrap();
        shot(name, 1.0, |a| {
            a.cfg.weather_city = i;
            open_settings_tab(a, 3);
        });
    }
}

/// Every stepper value is shown in full (no ellipsis) and never runs into its row's label: all cities, the
/// lyric timing extremes, every notch size and idle display.
#[test]
fn stepper_values_fit_without_cutting() {
    let _lang = i18n::restore_on_drop();
    let mut a = test_app(1.0);
    let mut g = Gfx::new(1.0);
    let mut check = |a: &App, tab: u8| {
        for r in a.settings_rows(tab) {
            let settingsview::RowKind::Stepper(v) = &r.kind else { continue };
            let f = settingsview::stepper_fit(&mut g, &r, v);
            assert_eq!(&f.value, v, "{:?}: value was cut", r.label);
            let label_end = 12.0 + g.width_ex(r.label, 12.0, true).max(g.width(r.sub, 11.0));
            assert!(f.minus_x - 9.0 >= label_end + 4.0 - 0.01, "{:?} = {v:?}: the - button overlaps the label", r.label);
            assert!(f.size >= 9.0, "{:?} = {v:?}: text too small", r.label);
        }
    };
    for i in 0..weather::CITIES.len() {
        a.cfg.weather_city = i;
        check(&a, 4);
    }
    for lead in [settings::LEAD_MIN, settings::LEAD_MAX] {
        a.cfg.lyric_lead_ms = lead;
        check(&a, 1);
    }
    for lang in 0..i18n::LANGS.len() {
        i18n::set(lang);
        for n in 0..scale::SIZES.len() {
            a.cfg.notch_size = n;
            a.cfg.idle_display = n % settings::IDLE_DISPLAY_LABELS.len();
            check(&a, 1);
        }
        for m in 0..monitors::mode_count() {
            a.cfg.monitor = m;
            check(&a, 1);
        }
        for ac in 0..accent::ACCENTS.len() {
            a.cfg.accent = ac;
            check(&a, 4);
        }
        for l in 0..i18n::LANGS.len() {
            a.cfg.lang = l;
            check(&a, 2);
        }
    }
}

// ---------------------------------------------------------------- calendar

/// Jakarta time, UTC+7.
const WIB: i32 = 7 * 3600;

fn event(title: &str, start: i64, mins: i64, link: Option<&str>) -> ics::Event {
    ics::Event { summary: title.into(), start, end: start + mins * 60, all_day: false, link: link.map(str::to_string) }
}

fn now_s() -> i64 {
    notif::now_unix() as i64
}

#[test]
fn meeting_countdown_in_the_last_ten_minutes() {
    let mut a = test_app(1.0);
    let now = now_s();
    a.cal = vec![
        ics::Event { all_day: true, ..event("Holiday", now + 60, 24 * 60, None) },
        event("Standup", now + 8 * 60 + 30, 15, Some("https://meet.google.com/abc")),
        event("Review", now + 9 * 60, 60, None),
    ];
    let (e, m) = a.meeting_countdown(now).expect("countdown");
    assert_eq!((e.summary.as_str(), m), ("Standup", 9), "the soonest timed meeting, minutes rounded up; all-day ignored");
    a.cal = vec![event("Later", now + 11 * 60, 30, None)];
    assert!(a.meeting_countdown(now).is_none(), "more than 10 minutes away");
    a.cal = vec![event("Running", now - 60, 30, None)];
    assert!(a.meeting_countdown(now).is_none(), "already started");
}

#[test]
fn meeting_toast_once_when_it_starts() {
    let mut a = test_app(1.0);
    let start = now_s() - 20;
    a.cal = vec![event("Standup", start, 15, Some("https://meet.google.com/abc")), ics::Event { all_day: true, ..event("Holiday", start, 1440, None) }];
    assert!(a.meeting_minute(start + 20));
    let Some(Toast { kind: ToastKind::Meeting { title, at, link }, .. }) = &a.toast else { panic!("meeting toast") };
    assert_eq!((title.as_str(), *at, link.as_deref()), ("Standup", start, Some("https://meet.google.com/abc")));
    a.toast = None;
    assert!(!a.meeting_minute(start + 60), "only once per meeting");
    assert!(a.toast.is_none() && a.toast_q.is_empty(), "no toast for the all-day event");
    assert!(!a.meeting_minute(start + 10 * 60), "long after the start: no toast");
}

#[test]
fn meeting_toast_join_opens_the_link() {
    let mut a = test_app(1.0);
    let link = "https://meet.google.com/abc-defg-hij";
    show_toast(&mut a, ToastKind::Meeting { title: "Standup".into(), at: now_s(), link: Some(link.into()) });
    let (w, h) = a.toast_size();
    let (bx, by, bw, bh) = toast::meeting_join_rect(w, h);
    assert!(a.toast_click(bx + bw / 2.0, by + bh / 2.0));
    assert_eq!(sys::opened(), [link]);
    assert!(a.toast.is_none(), "the toast closes");
}

#[test]
fn calendar_button_pastes_and_removes_the_link() {
    let mut a = test_app(1.0);
    a.cal_button(Some("not a link".into()));
    assert_eq!(a.cal_link, None);
    assert!(a.cal_paste_err.is_some(), "tells that the clipboard has no ICS link");
    a.cal_button(Some(" webcal://calendar.google.com/x/basic.ics ".into()));
    assert_eq!(a.cal_link.as_deref(), Some("https://calendar.google.com/x/basic.ics"));
    assert_eq!(a.cal_status, calendar::CalStatus::Loading);
    a.cal_update(calendar::CalStatus::Ready, vec![event("Standup", now_s() + 600, 15, None)]);
    assert_eq!(a.cal.len(), 1);
    a.cal_button(None);
    assert!(a.cal_link.is_some(), "first press only asks to confirm");
    a.cal_button(None);
    assert_eq!(a.cal_link, None, "second press removes it");
    assert_eq!(a.cal_status, calendar::CalStatus::Off);
    assert!(a.cal.is_empty());
}

#[test]
fn agenda_lists_what_is_left_today_and_join_opens_the_link() {
    let mut a = test_app(1.0);
    // 28 Oct 2026 10:00 WIB
    let now = ics::unix_utc(2026, 10, 28, 3, 0, 0);
    let at = |h: u32, m: u32| ics::unix_utc(2026, 10, 28, h, m, 0) - WIB as i64;
    a.cal = vec![
        event("Done already", at(8, 0), 30, None),
        event("Design review", at(9, 45), 30, Some("https://zoom.us/j/1")),
        event("Lunch", at(12, 0), 60, None),
        event("Sprint planning", at(14, 0), 60, Some("https://meet.google.com/xyz")),
        event("Tomorrow", at(24 + 9, 0), 30, None),
        ics::Event { all_day: true, ..event("Company holiday", at(0, 0), 24 * 60, None) },
    ];
    let names: Vec<_> = a.agenda(now, WIB).iter().map(|e| e.summary.clone()).collect();
    assert_eq!(names, ["Company holiday", "Design review", "Lunch", "Sprint planning"], "all-day first, ended and tomorrow left out");
    open_slide(&mut a, SlideId::Agenda);
    let (x, y, w, h) = calendar::join_rect(1);
    assert_eq!(a.agenda_hit(x + w / 2.0, y + h / 2.0, now, WIB), Some(calendar::AgendaHit::Join(1)));
    let (x, y, w, h) = calendar::join_rect(2);
    assert_eq!(a.agenda_hit(x + w / 2.0, y + h / 2.0, now, WIB), None, "Lunch has no link, so no button");
}

#[test]
fn countdown_shows_whichever_comes_first() {
    let mut a = test_app(1.0);
    a.cfg.prayer_on = true;
    prayer_in(&mut a, 2, 12);
    a.cal = vec![event("Standup", now_s() + 5 * 60 - 10, 15, None)];
    assert_eq!(a.upcoming(), Some(calendar::Upcoming::Meeting("Standup".into(), 5)));
    a.cal = vec![event("Later", now_s() + 14 * 60, 15, None)];
    assert_eq!(a.upcoming(), Some(calendar::Upcoming::Prayer(2, 12)), "the meeting is not within 10 minutes yet");
    a.cfg.prayer_on = false;
    assert_eq!(a.upcoming(), None);
}

/// Settings > Slides holds 10 slides in rows of 3: the wheel brings the 4th row into view, and its cells work.
#[test]
fn slides_grid_scrolls_to_the_tenth_slide() {
    let mut a = test_app(1.0);
    open_settings_tab(&mut a, 0);
    assert_eq!(a.cfg.slides.len(), SlideId::USER.len());
    assert!(settingsview::slide_cells_visible(9, 0).is_none(), "the 10th slide starts below the grid");
    assert!(a.settings_slides_wheel(true));
    assert_eq!(a.sui.slides_scroll, 1);
    assert!(!a.settings_slides_wheel(true), "only one extra row");
    let (x, y) = settingsview::slide_cells_visible(9, 1).expect("visible after scrolling");
    assert!(settingsview::slide_cells_visible(0, 1).is_none(), "the first row scrolled out");
    let was = a.cfg.slides[9].1;
    a.settings_click(x + settingsview::SC_W - 10.0, y + 10.0);
    assert_eq!(a.cfg.slides[9].1, !was, "its toggle works");
    assert!(a.settings_slides_wheel(false));
    assert_eq!(a.sui.slides_scroll, 0);
}

/// A day with meetings: one running with a Meet link, then two more today.
fn with_meetings(a: &mut App) {
    let now = now_s();
    a.cal_link = Some("https://calendar.google.com/x/basic.ics".into());
    a.cal_status = calendar::CalStatus::Ready;
    a.cal = vec![
        event("Design review", now - 10 * 60, 45, Some("https://meet.google.com/abc-defg-hij")),
        event("Lunch with the team", now + 90 * 60, 60, None),
        event("Sprint planning for the clipboard and calendar work", now + 3 * 3600, 60, Some("https://zoom.us/j/1")),
    ];
}

#[test]
fn snap_calendar() {
    for s in SCALES {
        shot("meeting-idle", s, |a| a.cal = vec![event("Standup", now_s() + 8 * 60, 15, None)]);
    }
    shot("meeting-playing", 1.0, |a| {
        a.cal = vec![event("Standup", now_s() + 8 * 60, 15, None)];
        a.media = Media { title: "Almost Easy".into(), artist: "Avenged Sevenfold".into(), playing: true, pos: 60, dur: 240, pos_ms: 60_000, source: "Demo".into(), sources: 1 };
    });
    shot("toast-meeting", 1.0, |a| {
        show_toast(a, ToastKind::Meeting { title: "Daily Standup".into(), at: now_s(), link: Some("https://meet.google.com/abc".into()) })
    });
    shot("toast-meeting-nolink", 1.0, |a| show_toast(a, ToastKind::Meeting { title: "1:1 with Rina".into(), at: now_s(), link: None }));
    shot("agenda", 1.0, |a| {
        with_meetings(a);
        open_slide(a, SlideId::Agenda);
    });
    shot("agenda-no-link", 1.0, |a| open_slide(a, SlideId::Agenda));
    shot("agenda-loading", 1.0, |a| {
        a.cal_link = Some("https://x.com/a.ics".into());
        a.cal_status = calendar::CalStatus::Loading;
        open_slide(a, SlideId::Agenda);
    });
    shot("agenda-failed", 1.0, |a| {
        a.cal_link = Some("https://x.com/a.ics".into());
        a.cal_status = calendar::CalStatus::Failed;
        open_slide(a, SlideId::Agenda);
    });
    shot("agenda-done-for-today", 1.0, |a| {
        a.cal_link = Some("https://x.com/a.ics".into());
        a.cal_status = calendar::CalStatus::Ready;
        a.cal = vec![event("Standup", now_s() + 30 * 3600, 15, None)];
        open_slide(a, SlideId::Agenda);
    });
    shot("settings-calendar-off", 1.0, |a| open_settings_tab(a, 3));
    shot("settings-calendar-synced", 1.0, |a| {
        with_meetings(a);
        open_settings_tab(a, 3);
    });
    shot("settings-slides-scrolled", 1.0, |a| {
        open_settings_tab(a, 0);
        a.sui.slides_scroll = 1;
    });
    let id = |a: &mut App| {
        a.cfg.lang = i18n::ID;
        i18n::set(i18n::ID);
    };
    let _lang = i18n::restore_on_drop();
    shot("id-agenda", 1.0, |a| {
        id(a);
        with_meetings(a);
        open_slide(a, SlideId::Agenda);
    });
    shot("id-toast-meeting", 1.0, |a| {
        id(a);
        show_toast(a, ToastKind::Meeting { title: "Daily Standup".into(), at: now_s(), link: Some("https://meet.google.com/abc".into()) })
    });
    shot("id-settings-calendar", 1.0, |a| {
        id(a);
        with_meetings(a);
        open_settings_tab(a, 3);
    });
}

/// The Calendar row in Settings > Widgets: Paste reads the clipboard, and every state's text fits, in both languages.
#[test]
fn calendar_row_pastes_and_fits_in_every_state() {
    let _lang = i18n::restore_on_drop();
    let mut a = test_app(1.0);
    let row = |a: &App| a.settings_rows(3).into_iter().position(|r| r.label == i18n::tr("Calendar")).expect("Calendar row");
    let i = row(&a);
    let (cx, cy) = settingsview::cell_pos(i);
    calendar::TEST_CLIPBOARD.with(|c| *c.borrow_mut() = Some("https://calendar.google.com/x/basic.ics".into()));
    a.settings_click_rows(3, cx + settingsview::CELL_W - 30.0, cy + 17.0);
    assert_eq!(a.cal_link.as_deref(), Some("https://calendar.google.com/x/basic.ics"), "Paste took the link from the clipboard");

    let mut g = Gfx::new(1.0);
    for lang in 0..i18n::LANGS.len() {
        i18n::set(lang);
        let states: [(&str, fn(&mut App)); 6] = [
            ("no link", |a| a.cal_link = None),
            ("nothing on the clipboard", |a| {
                a.cal_link = None;
                a.cal_paste_err = Some(Instant::now());
            }),
            ("loading", |a| {
                a.cal_link = Some("https://x.com/a.ics".into());
                a.cal_status = calendar::CalStatus::Loading;
            }),
            ("ready", |a| {
                a.cal_status = calendar::CalStatus::Ready;
                a.cal = (0..12).map(|k| event("Meeting", now_s() + k * 60, 10, None)).collect();
            }),
            ("failed", |a| a.cal_status = calendar::CalStatus::Failed),
            ("confirm remove", |a| a.cal_confirm = Some(Instant::now())),
        ];
        for (name, set) in states {
            set(&mut a);
            let r = a.settings_rows(3).swap_remove(row(&a));
            let w = settingsview::text_w(&r.kind);
            assert!(g.width_ex(r.label, 12.0, true) <= w && g.width(r.sub, 11.0) <= w, "lang {lang}, {name}: {:?} / {:?}", r.label, r.sub);
            let settingsview::RowKind::Button(b) = &r.kind else { panic!("{name}: a button") };
            assert!(g.width_ex(b, 11.0, true) <= 46.0, "lang {lang}, {name}: button {b:?}");
        }
    }
}

/// Short values keep the old stepper; "Bandar Lampung" moves the "-" button left and clicking it still steps back.
#[test]
fn moved_minus_button_is_still_clickable() {
    let mut a = test_app(1.0);
    let row = |a: &App| a.settings_rows(3).into_iter().position(|r| r.label == "City").unwrap();
    let i = row(&a);
    let minus_x = |a: &mut App| {
        let r = a.settings_rows(3).swap_remove(i);
        let settingsview::RowKind::Stepper(v) = &r.kind else { unreachable!() };
        settingsview::stepper_fit(a.gfx.as_mut().unwrap(), &r, v).minus_x
    };
    a.cfg.weather_city = 0; // Jakarta
    assert_eq!(minus_x(&mut a), settingsview::STEP_PLUS_X - settingsview::STEP_MIN_SPAN);

    let long = weather::CITIES.iter().position(|c| c.0 == "Bandar Lampung").unwrap();
    a.cfg.weather_city = long;
    let mx = minus_x(&mut a);
    assert!(mx < settingsview::STEP_PLUS_X - settingsview::STEP_MIN_SPAN, "the - button should move left");
    let (cx, cy) = settingsview::cell_pos(i);
    a.settings_click_rows(3, cx + mx, cy + 17.0);
    assert_eq!(a.cfg.weather_city, long - 1, "clicking the moved - button steps back");
    a.cfg.weather_city = long;
    a.settings_click_rows(3, cx + settingsview::STEP_PLUS_X, cy + 17.0);
    assert_eq!(a.cfg.weather_city, (long + 1) % weather::CITIES.len(), "+ still steps forward");
}

/// Every slide name fits in its Settings > Slides cell instead of running into the chevrons.
#[test]
fn slide_names_fit_settings_cells() {
    let _lang = i18n::restore_on_drop();
    let mut g = Gfx::new(1.0);
    for lang in 0..i18n::LANGS.len() {
        i18n::set(lang);
        for id in SlideId::USER {
            let w = g.width_ex(id.short_label(), 11.0, true);
            assert!(w <= settingsview::SLIDE_NAME_W, "{:?} is {w:.1} px wide", id.short_label());
        }
    }
}

/// Every Settings row's label and sub-label fit to the left of its control (no ellipsis needed).
#[test]
fn settings_text_fits_cells() {
    let _lang = i18n::restore_on_drop();
    let a = test_app(1.0);
    let mut g = Gfx::new(1.0);
    for lang in 0..i18n::LANGS.len() {
        i18n::set(lang);
        for tab in 1..=3u8 {
            for r in a.settings_rows(tab) {
                let w = settingsview::text_w(&r.kind);
                let (lw, sw) = (g.width_ex(r.label, 12.0, true), g.width(r.sub, 11.0));
                assert!(lw <= w, "tab {tab}: label {:?} {lw:.1} px > {w} px", r.label);
                assert!(sw <= w, "tab {tab}: sub {:?} {sw:.1} px > {w} px", r.sub);
            }
        }
    }
}

/// The Settings tabs sit right of the title and inside the notch, in every language.
#[test]
fn settings_tabs_fit_next_to_the_title() {
    let _lang = i18n::restore_on_drop();
    let mut g = Gfx::new(1.0);
    for lang in 0..i18n::LANGS.len() {
        i18n::set(lang);
        let tabs = settingsview::tab_rects(&mut g);
        let title_end = LX + g.width_ex(i18n::tr("Settings"), 15.0, true);
        assert!(tabs[0].0 >= title_end + 8.0, "lang {lang}: tabs start at {:.1}, title ends at {title_end:.1}", tabs[0].0);
        let (x, w) = tabs[tabs.len() - 1];
        assert!(x + w <= RX + 0.01, "lang {lang}: tabs end at {:.1} > {RX}", x + w);
    }
}

/// The Language row in Settings > General switches the whole UI right away, and back.
#[test]
fn language_row_switches_the_ui() {
    let _lang = i18n::restore_on_drop();
    let mut a = test_app(1.0);
    let i = a.settings_rows(1).iter().position(|r| r.label == "Language").expect("Language row in General");
    let (cx, cy) = settingsview::cell_pos(i);
    a.settings_click_rows(1, cx + settingsview::STEP_PLUS_X, cy + 17.0);
    assert_eq!(a.cfg.lang, i18n::ID);
    assert_eq!(i18n::current(), i18n::ID, "applied right away");
    assert_eq!(a.settings_rows(1)[i].label, "Bahasa");
    assert_eq!(a.settings_rows(3)[1].label, "Kota");
    assert_eq!(SlideId::Todo.label(), "Tugas");
    assert_eq!(clock::day_short(1), "Sen");
    assert_eq!(clock::month(4), "Mei");
    assert_eq!(timer::fmt_preset(90 * 60), "1 j 30 mnt");
    a.settings_click_rows(1, cx + settingsview::STEP_PLUS_X, cy + 17.0);
    assert_eq!(a.cfg.lang, i18n::EN);
    assert_eq!(a.settings_rows(3)[1].label, "City");
    assert_eq!(clock::day_short(1), "Mon");
    assert_eq!(timer::fmt_preset(90 * 60), "1 h 30 min");
}

/// The main screens in Indonesian, for review.
#[test]
fn snap_indonesian() {
    let _lang = i18n::restore_on_drop();
    let id = |a: &mut App| {
        a.cfg.lang = i18n::ID;
        i18n::set(i18n::ID);
    };
    for tab in 0..4u8 {
        shot(&format!("id-settings-tab{tab}"), 1.0, |a| {
            id(a);
            open_settings_tab(a, tab);
        });
    }
    for (name, slide) in [("id-todo", SlideId::Todo), ("id-water", SlideId::Water), ("id-system", SlideId::System), ("id-notifs", SlideId::Notifications), ("id-music", SlideId::Music), ("id-clipboard", SlideId::Clipboard), ("id-ledge", SlideId::Ledge), ("id-scrap", SlideId::Scrap)] {
        shot(name, 1.0, |a| {
            id(a);
            open_slide(a, slide);
        });
    }
    shot("id-timer", 1.0, |a| {
        id(a);
        a.tm.start(Instant::now() - Duration::from_secs(100));
        open_slide(a, SlideId::Timer);
    });
    shot("id-pomodoro", 1.0, |a| {
        id(a);
        pomo(a, 2, timer::Phase::Focus, 9 * 60 + 12);
        open_slide(a, SlideId::Timer);
    });
    shot("id-clipboard-full", 1.0, |a| {
        id(a);
        with_clips(a, &CLIPS);
        open_slide(a, SlideId::Clipboard);
    });
    shot("id-toast-prayer", 1.0, |a| {
        id(a);
        show_toast(a, ToastKind::Prayer { idx: 2, at: 15 * 60 + 12 });
    });
    shot("id-toast-battery", 1.0, |a| {
        id(a);
        show_toast(a, ToastKind::LowBattery { pct: 15 });
    });
    shot("id-toast-water", 1.0, |a| {
        id(a);
        show_toast(a, ToastKind::Water);
    });
    shot("id-toast-mic", 1.0, |a| {
        id(a);
        show_toast(a, ToastKind::Privacy { cam: false, app: "Zoom".into() });
    });
}

/// Fake prayer times: prayer `idx` is `mins` minutes from now, the others are outside the countdown window.
fn prayer_in(a: &mut App, idx: usize, mins: u16) {
    let st = now_tm();
    let now = st.wHour * 60 + st.wMinute;
    let mut t = [0u16; 5];
    for (i, v) in t.iter_mut().enumerate() {
        *v = if i < idx { 0 } else if i == idx { now + mins } else { now + mins + 600 };
    }
    a.prayer = Some((0, t));
}

#[test]
fn snap_prayer() {
    for s in SCALES {
        shot("prayer-idle", s, |a| prayer_in(a, 2, 12));
    }
    shot("prayer-playing", 1.0, |a| {
        prayer_in(a, 3, 5);
        a.media = Media { title: "Almost Easy".into(), artist: "Avenged Sevenfold".into(), playing: true, pos: 60, dur: 240, pos_ms: 60_000, source: "Demo".into(), sources: 1 };
    });
    shot("prayer-with-timer", 1.0, |a| {
        prayer_in(a, 4, 9);
        a.tm.start(Instant::now() - Duration::from_secs(61));
    });
    shot("toast-prayer", 1.0, |a| show_toast(a, ToastKind::Prayer { idx: 2, at: 15 * 60 + 12 }));
    // the "Prayer times" row in Settings > Extras: see settings-tab3
}

#[test]
fn snap_accents() {
    for (i, ac) in accent::ACCENTS.iter().enumerate() {
        let n = ac.name.to_lowercase();
        shot(&format!("accent-{n}-settings"), 1.0, |a| {
            a.cfg.accent = i;
            open_settings_tab(a, 1);
        });
        shot(&format!("accent-{n}-todo"), 1.0, |a| {
            a.cfg.accent = i;
            open_slide(a, SlideId::Todo);
        });
        shot(&format!("accent-{n}-timer"), 1.0, |a| {
            a.cfg.accent = i;
            a.tm.start(Instant::now() - Duration::from_secs(100));
            open_slide(a, SlideId::Timer);
        });
    }
}

/// The accent setting really changes the render, and every accent produces a different image.
#[test]
fn accent_changes_the_rendered_slide() {
    let render = |i: usize| {
        let mut a = test_app(1.0);
        a.cfg.accent = i;
        open_slide(&mut a, SlideId::Todo);
        (a.w, a.h) = anim::target(&a);
        a.sp = a.slide as f32;
        render::draw(&mut a);
        a.gfx.as_ref().expect("gfx").pm.data().to_vec()
    };
    let shots: Vec<_> = (0..accent::ACCENTS.len()).map(render).collect();
    for i in 0..shots.len() {
        for j in i + 1..shots.len() {
            assert_ne!(shots[i], shots[j], "accents {i} and {j} render the same");
        }
    }
}

#[test]
fn snap_slide_hotkeys() {
    // Settings > Slides with each slide's shortcut, and General with the "Slide hotkeys" toggle
    shot("hotkeys-settings-slides", 1.0, |a| open_settings_tab(a, 0));
    shot("hotkeys-settings-general", 1.0, |a| open_settings_tab(a, 1));
    shot("hotkeys-settings-slides-off", 1.0, |a| {
        a.cfg.slide_hotkeys = false;
        open_settings_tab(a, 0);
    });
    // Ctrl+Alt+3 from the idle notch
    shot("hotkeys-open-third", 1.0, |a| {
        assert!(a.slide_hotkey(2));
    });
}

/// Ctrl+Alt+N opens the Nth slide, the same key closes it again, and a missing slide is ignored.
#[test]
fn slide_hotkey_opens_and_closes() {
    let mut a = test_app(1.0);
    assert!(!a.expanded());
    assert!(a.slide_hotkey(2));
    assert!(a.expanded());
    assert_eq!(a.cur_id(), a.order[2]);
    assert!(a.slide_hotkey(0), "another hotkey switches slides");
    assert_eq!(a.cur(), 0);
    assert!(a.expanded());
    assert!(a.slide_hotkey(0), "same hotkey again closes");
    assert!(!a.expanded());
    assert!(!a.slide_hotkey(a.order.len()), "no slide at that position");
}

/// Fill the clipboard history, oldest first: (text, seconds ago).
fn with_clips(a: &mut App, clips: &[(&str, u64)]) {
    let now = Instant::now();
    for (s, ago) in clips {
        a.clips.push(s, now - Duration::from_secs(*ago));
    }
}

const CLIPS: [(&str, u64); 4] = [
    ("https://github.com/dirgaastya/hesewake-notch/issues/13", 3 * 86400),
    ("fn main() {\r\n    println!(\"hello\");\r\n}", 2 * 3600),
    ("Meeting notes: ship 0.6 after the clipboard slide lands, then look at the ICS calendar idea and the update check", 600),
    ("rhezaardiansyah", 20),
];

/// Middle of visible row `vi` on the Clipboard slide.
fn clip_row_y(vi: usize) -> f32 {
    clipboard::ROW_Y0 + vi as f32 * clipboard::ROW_P + clipboard::ROW_H / 2.0
}

fn set_slide(a: &mut App, id: SlideId, on: bool) {
    a.cfg.slides.iter_mut().find(|(s, _)| *s == id).expect("slide in config").1 = on;
}

#[test]
fn snap_clipboard() {
    shot("clipboard-empty", 1.0, |a| open_slide(a, SlideId::Clipboard));
    for s in SCALES {
        shot("clipboard", s, |a| {
            with_clips(a, &CLIPS);
            open_slide(a, SlideId::Clipboard);
        });
    }
    shot("clipboard-copied", 1.0, |a| {
        with_clips(a, &CLIPS);
        open_slide(a, SlideId::Clipboard);
        let hit = a.clip_hit(LX + 60.0, clip_row_y(2)).expect("third row");
        a.clip_apply(hit);
    });
    shot("clipboard-full-scrolled", 1.0, |a| {
        for i in 0..clipboard::MAX_ITEMS {
            a.clips.push(&format!("Copied text number {}", i + 1), Instant::now() - Duration::from_secs(60 * (10 - i as u64)));
        }
        open_slide(a, SlideId::Clipboard);
        a.clip_scroll = clipboard::MAX_ITEMS - 3;
    });
}

/// Clicking an entry gives its full text back for the clipboard and moves it to the top; the clipboard update that
/// follows does not add it twice.
#[test]
fn clipboard_click_copies_back_and_moves_to_top() {
    let mut a = test_app(1.0);
    with_clips(&mut a, &CLIPS);
    open_slide(&mut a, SlideId::Clipboard);
    let hit = a.clip_hit(LX + 60.0, clip_row_y(2));
    assert_eq!(hit, Some(clipboard::ClipHit::Copy(2)));
    assert_eq!(a.clip_apply(hit.unwrap()).as_deref(), Some(CLIPS[1].0), "full text, newlines kept");
    assert_eq!(a.clips.items()[0].text, CLIPS[1].0, "moved to the top");
    assert!(a.clip_flash.is_some(), "the row shows \"Copied\"");
    assert!(!a.on_clipboard(Some(CLIPS[1].0.into()), Instant::now()), "our own copy is not added again");
    assert_eq!(a.clips.len(), CLIPS.len());
    assert_eq!(a.clip_hit(LX + 60.0, EXP_H - 10.0), None, "below the rows");
}

#[test]
fn clipboard_remove_and_clear_buttons() {
    let mut a = test_app(1.0);
    with_clips(&mut a, &CLIPS);
    open_slide(&mut a, SlideId::Clipboard);
    let x_btn = LX + clipboard::row_w(a.clips.len()) - 15.0;
    assert_eq!(a.clip_hit(x_btn, clip_row_y(0)), Some(clipboard::ClipHit::Remove(0)));
    assert_eq!(a.clip_apply(clipboard::ClipHit::Remove(0)), None, "nothing to copy");
    assert_eq!(a.clips.items()[0].text, CLIPS[2].0);
    let (cx, cy, cw, ch) = clipboard::CLEAR_BTN;
    assert_eq!(a.clip_hit(cx + cw / 2.0, cy + ch / 2.0), Some(clipboard::ClipHit::Clear));
    a.clip_apply(clipboard::ClipHit::Clear);
    assert!(a.clips.is_empty());
    assert_eq!(a.clip_hit(cx + cw / 2.0, cy + ch / 2.0), None, "no Clear button on an empty list");
}

#[test]
fn clipboard_scrolls_with_the_wheel() {
    let mut a = test_app(1.0);
    for i in 0..5 {
        a.clips.push(&format!("clip {i}"), Instant::now());
    }
    open_slide(&mut a, SlideId::Clipboard);
    assert!(a.clip_wheel(true));
    assert_eq!(a.clip_scroll, 1);
    assert_eq!(a.clip_hit(LX + 60.0, clip_row_y(0)), Some(clipboard::ClipHit::Copy(1)), "rows follow the scroll");
    for _ in 0..5 {
        a.clip_wheel(true);
    }
    assert_eq!(a.clip_scroll, 2, "stops at the last full page");
    assert!(!a.clip_wheel(true));
    assert!(a.clip_wheel(false));
    assert_eq!(a.clip_scroll, 1);
    a.clip_apply(clipboard::ClipHit::Clear);
    assert_eq!(a.clip_scroll, 0);
}

/// Nothing is recorded while the Clipboard slide is off, and turning it off forgets what was kept.
#[test]
fn clipboard_is_only_recorded_while_the_slide_is_on() {
    let mut a = test_app(1.0);
    set_slide(&mut a, SlideId::Clipboard, false);
    assert!(!a.on_clipboard(Some("secret".into()), Instant::now()));
    assert!(a.clips.is_empty());
    set_slide(&mut a, SlideId::Clipboard, true);
    assert!(a.on_clipboard(Some("hello".into()), Instant::now()));
    assert!(!a.on_clipboard(None, Instant::now()), "no text (or not allowed): nothing changes");
    assert_eq!(a.clips.len(), 1);
    set_slide(&mut a, SlideId::Clipboard, false);
    a.on_clipboard(Some("another".into()), Instant::now());
    assert!(a.clips.is_empty(), "turned off: history forgotten");
}

/// Busy mode (fullscreen video) with music playing: right after a toast such as the volume HUD closes, the notch
/// shrinks back to the thin line. The cover and the equalizer must not be drawn into that line, because the timer
/// can stop before the toast's grace period ends and leave them on screen.
#[test]
fn thin_line_stays_empty_right_after_a_toast() {
    let mut a = test_app(1.0);
    a.media = Media { title: "Netflix".into(), artist: String::new(), playing: true, pos: 60, dur: 240, pos_ms: 60_000, source: "Demo".into(), sources: 1 };
    a.busy = true;
    a.toast_end = Instant::now();
    assert!(a.toast_hold(), "still inside the grace period after the toast");
    (a.w, a.h) = anim::target(&a);
    assert_eq!((a.w, a.h), THIN);
    render::draw(&mut a);
    let g = a.gfx.as_ref().expect("gfx");
    let lit = g.pm.data().chunks_exact(4).filter(|p| p[0] > 8 || p[1] > 8 || p[2] > 8).count();
    assert_eq!(lit, 0, "the thin line shows {lit} coloured pixels");
}

/// Ctrl+Alt+M mutes every default microphone, the next press unmutes them, and each press shows the HUD.
#[test]
fn mic_hotkey_mutes_and_unmutes() {
    let mut a = test_app(1.0);
    let hud = |a: &App| match a.toast.as_ref().map(|t| &t.kind) {
        Some(ToastKind::Hud { kind: HudKind::Mic(s), .. }) => Some(*s),
        _ => None,
    };
    mic::set_fake(&[false, true]);
    a.mic_hotkey();
    assert_eq!(mic::fake(), vec![true, true], "one device was still on: mute both");
    assert_eq!(a.mic_muted, Some(true));
    assert_eq!(hud(&a), Some(Some(true)), "HUD says muted");
    a.mic_hotkey();
    assert_eq!(mic::fake(), vec![false, false], "all muted: unmute");
    assert_eq!(a.mic_muted, Some(false));
    assert_eq!(hud(&a), Some(Some(false)), "the HUD is updated in place");
}

/// Without a microphone the hotkey only says so: no mute state and nothing new in the notch.
#[test]
fn no_microphone_changes_nothing() {
    let mut a = test_app(1.0);
    mic::set_fake(&[]);
    let before = anim::target(&a);
    a.mic_hotkey();
    assert_eq!(mic::fake(), Vec::<bool>::new());
    assert_eq!(a.mic_muted, None);
    assert!(matches!(a.toast.as_ref().map(|t| &t.kind), Some(ToastKind::Hud { kind: HudKind::Mic(None), .. })), "HUD says there is no microphone");
    a.toast = None;
    assert_eq!(anim::target(&a), before, "the idle notch keeps its size");
    a.mic_apply(None);
    assert_eq!(a.priv_extra(), 0.0);
}

/// A mute made elsewhere (Windows, a keyboard mic key, another app) shows a crossed-out mic, without a toast.
#[test]
fn mute_from_elsewhere_shows_in_the_notch() {
    let mut a = test_app(1.0);
    assert_eq!(a.priv_extra(), 0.0, "mic on and unused: nothing");
    a.mic_apply(Some(true));
    assert!(a.toast.is_none(), "no toast for a change made elsewhere");
    let muted = a.priv_extra();
    assert!(muted > 0.0, "muted while no app uses the mic: the crossed-out mic still shows");
    a.priv_mic = vec!["Zoom".into()];
    assert_eq!(a.priv_extra(), muted, "in use and muted: one crossed-out mic, not a dot next to it");
    a.cfg.privacy_dot = false;
    assert_eq!(a.priv_extra(), muted, "with the dots off the mute still shows while the hotkey is on");
    a.cfg.mic_hotkey = false;
    assert_eq!(a.priv_extra(), 0.0, "both settings off: nothing");
    a.cfg.privacy_dot = true;
    a.cfg.mic_hotkey = true;
    a.mic_apply(Some(false));
    let dot = a.priv_extra();
    assert!(dot > 0.0 && dot < muted, "unmuted and in use: the orange dot again ({dot} < {muted})");
}

/// Settings > Alerts has the "Mic mute key" toggle, on by default.
#[test]
fn mic_hotkey_row_in_alerts() {
    let mut a = test_app(1.0);
    let rows = a.settings_rows(2);
    let i = rows.iter().position(|r| r.label == "Mic mute key").expect("row in Alerts");
    assert!(matches!(rows[i].kind, settingsview::RowKind::Toggle(true)), "on by default");
    open_settings_tab(&mut a, 2);
    assert!(a.settings_rows_wheel(true), "the 8th row sits below the grid until it scrolls");
    let (cx, cy) = settingsview::row_cell_visible(i, a.sui.rows_scroll).expect("visible after scrolling");
    a.settings_click_rows(2, cx + settingsview::CELL_W - 20.0, cy + 17.0);
    assert!(!a.cfg.mic_hotkey);
}

/// The mic HUD's name and state fit side by side in every language.
#[test]
fn mic_hud_text_fits() {
    let _lang = i18n::restore_on_drop();
    let mut g = Gfx::new(1.0);
    for lang in 0..i18n::LANGS.len() {
        i18n::set(lang);
        for s in [None, Some(false), Some(true)] {
            let (label, state) = toast::mic_hud_text(s);
            let used = g.width_ex(label, 14.0, true) + 8.0 + g.width_ex(state, 14.0, true);
            let room = toast::TOAST_HUD.0 - 54.0 - 20.0;
            assert!(used <= room, "lang {lang} {s:?}: {label:?} + {state:?} = {used:.1} px > {room} px");
        }
    }
}

#[test]
fn snap_mic_mute() {
    let _lang = i18n::restore_on_drop();
    for s in SCALES {
        shot("idle-mic-muted", s, |a| a.mic_muted = Some(true));
    }
    shot("idle-mic-in-use", 1.0, |a| a.priv_mic = vec!["Zoom".into()]);
    shot("idle-mic-muted-in-use", 1.0, |a| {
        a.mic_muted = Some(true);
        a.priv_mic = vec!["Zoom".into()];
    });
    shot("idle-cam-and-mic-muted", 1.0, |a| {
        a.mic_muted = Some(true);
        a.priv_mic = vec!["Zoom".into()];
        a.priv_cam = vec!["Zoom".into()];
    });
    shot("playing-mic-muted", 1.0, |a| {
        a.mic_muted = Some(true);
        a.media = Media { title: "Almost Easy".into(), artist: "Avenged Sevenfold".into(), playing: true, pos: 60, dur: 240, pos_ms: 60_000, source: "Demo".into(), sources: 1 };
    });
    shot("toast-mic-in-use-muted", 1.0, |a| {
        a.mic_muted = Some(true);
        show_toast(a, ToastKind::Privacy { cam: false, app: "Zoom".into() });
    });
    for lang in [i18n::EN, i18n::ID] {
        let pre = if lang == i18n::ID { "id-" } else { "" };
        for (name, s) in [("hud-mic-muted", Some(true)), ("hud-mic-on", Some(false)), ("hud-mic-none", None)] {
            shot(&format!("{pre}{name}"), 1.0, |a| {
                a.cfg.lang = lang;
                i18n::set(lang);
                show_toast(a, ToastKind::Hud { kind: HudKind::Mic(s), on: s == Some(false) });
            });
        }
    }
}

// ---------------------------------------------------------------- update check

fn release(v: &str) -> update::Release {
    update::Release { version: v.into(), url: format!("https://github.com/dirgaastya/hesewake-notch/releases/tag/v{v}"), installer: None }
}

/// A release that has an installer and its checksum file (so the toast offers one-click Install).
fn release_with_installer(v: &str) -> update::Release {
    let base = format!("https://github.com/dirgaastya/hesewake-notch/releases/download/v{v}");
    let installer = update::Installer { file: format!("HesewakeNotch-Setup-{v}.exe"), url: format!("{base}/HesewakeNotch-Setup-{v}.exe"), size: 3_300_000, sums_url: format!("{base}/SHA256SUMS") };
    update::Release { installer: Some(installer), ..release(v) }
}

fn update_note(a: &App) -> Option<update::Note> {
    match &a.toast {
        Some(Toast { kind: ToastKind::Update(n), .. }) => Some(n.clone()),
        _ => None,
    }
}

#[test]
fn update_toast_once_per_version() {
    let mut a = test_app(1.0);
    a.on_update(false, Some(release("99.0.0")));
    assert_eq!(update_note(&a), Some(update::Note::Available(release("99.0.0"))));
    assert_eq!(a.cfg.update_seen, "99.0.0");
    assert_eq!(a.update, Some(release("99.0.0")), "kept for the menu");
    a.toast = None;
    a.on_update(false, Some(release("99.0.0")));
    assert_eq!(update_note(&a), None, "the same version is told once");
    a.on_update(false, Some(release("99.0.1")));
    assert_eq!(update_note(&a), Some(update::Note::Available(release("99.0.1"))), "a newer one again");
}

#[test]
fn update_quiet_when_current_or_offline() {
    let mut a = test_app(1.0);
    a.on_update(false, Some(release(env!("CARGO_PKG_VERSION"))));
    assert_eq!((update_note(&a), a.update.clone()), (None, None), "same version: nothing");
    a.on_update(false, Some(release("0.0.1")));
    assert_eq!(update_note(&a), None, "older: nothing");
    a.on_update(false, None);
    assert_eq!(update_note(&a), None, "offline: silent");
}

#[test]
fn update_check_from_the_menu_always_answers() {
    let mut a = test_app(1.0);
    a.on_update(true, Some(release(env!("CARGO_PKG_VERSION"))));
    assert_eq!(update_note(&a), Some(update::Note::UpToDate));
    a.toast = None;
    a.on_update(true, None);
    assert_eq!(update_note(&a), Some(update::Note::Failed));
    a.toast = None;
    a.cfg.update_seen = "99.0.0".into();
    a.on_update(true, Some(release("99.0.0")));
    assert_eq!(update_note(&a), Some(update::Note::Available(release("99.0.0"))), "asked: shown even if seen before");
}

#[test]
fn update_checks_daily_and_never_when_off() {
    let mut a = test_app(1.0);
    let t0 = Instant::now();
    a.cfg.update_check = false;
    assert!(!a.update_tick(t0), "off: no network call");
    a.cfg.update_check = true;
    assert!(a.update_tick(t0), "first check");
    assert!(!a.update_tick(t0 + Duration::from_secs(60)), "still running");
    a.on_update(false, None);
    assert!(!a.update_tick(t0 + Duration::from_secs(3600)), "not before 24 hours");
    assert!(a.update_tick(t0 + update::INTERVAL + Duration::from_secs(1)), "again after 24 hours");
    assert_eq!(update::fetches(), vec![false, false], "two automatic checks, no manual one");
}

#[test]
fn update_toast_download_opens_the_release() {
    let mut a = test_app(1.0);
    a.on_update(false, Some(release("99.0.0")));
    if let Some(t) = a.toast.as_mut() {
        t.t0 = Instant::now() - Duration::from_millis(1500);
    }
    let (bx, by, bw, bh) = update::button_rect(toast::TOAST_UPDATE.0, toast::TOAST_UPDATE.1);
    let _ = sys::opened();
    assert!(a.toast_click(bx + bw / 2.0, by + bh / 2.0));
    assert_eq!(sys::opened(), vec![release("99.0.0").url]);
    assert!(a.toast.is_none());
}

#[test]
fn update_row_in_alerts() {
    let mut a = test_app(1.0);
    let i = a.settings_rows(2).iter().position(|r| r.label == "Update notice").expect("Update notice row in Alerts");
    open_settings_tab(&mut a, 2);
    assert!(settingsview::row_cell_visible(i, 0).is_none(), "the 7th row starts below the grid");
    assert!(a.settings_rows_wheel(true));
    let (cx, cy) = settingsview::row_cell_visible(i, a.sui.rows_scroll).expect("visible after scrolling");
    assert!(a.cfg.update_check, "on by default");
    a.settings_click_rows(2, cx + settingsview::CELL_W - 30.0, cy + 17.0);
    assert!(!a.cfg.update_check);
}

#[test]
fn snap_update_toasts() {
    let _lang = i18n::restore_on_drop();
    for (lang, prefix) in [(i18n::EN, "update"), (i18n::ID, "id-update")] {
        i18n::set(lang);
        shot(&format!("{prefix}-toast-available"), 1.0, |a| show_toast(a, ToastKind::Update(update::Note::Available(release("0.9.0")))));
        shot(&format!("{prefix}-toast-up-to-date"), 1.0, |a| show_toast(a, ToastKind::Update(update::Note::UpToDate)));
        shot(&format!("{prefix}-toast-failed"), 1.0, |a| show_toast(a, ToastKind::Update(update::Note::Failed)));
        shot(&format!("{prefix}-settings-alerts"), 1.0, |a| open_settings_tab(a, 2));
    }
    shot("update-toast-available", 1.5, |a| show_toast(a, ToastKind::Update(update::Note::Available(release("0.9.0")))));
}

/// General has 9 rows: the first six show, the wheel brings the rest into view, and rows keep working there.
#[test]
fn rows_tab_scrolls_to_the_last_rows() {
    let mut a = test_app(1.0);
    open_settings_tab(&mut a, 1);
    assert_eq!(a.settings_rows(1).len(), 9);
    assert!(a.rows_scrollable());
    assert!(settingsview::row_cell_visible(8, 0).is_none(), "the 9th row starts below the grid");
    assert!(a.settings_rows_wheel(true));
    assert!(a.settings_rows_wheel(true));
    assert_eq!(a.sui.rows_scroll, 2, "9 rows = 5 rows of cells, 3 show");
    assert!(!a.settings_rows_wheel(true), "nothing left to scroll");
    assert!(settingsview::row_cell_visible(0, a.sui.rows_scroll).is_none(), "the first row scrolled out");
    assert!(settingsview::row_cell_visible(8, a.sui.rows_scroll).is_some(), "the last row is in view");
    let (x, y) = settingsview::row_cell_visible(6, a.sui.rows_scroll).expect("Minimal notch is visible");
    let was = a.minimal;
    a.settings_click_rows(1, x + settingsview::CELL_W - 30.0, y + 17.0);
    assert_eq!(a.minimal, !was, "its toggle works after scrolling");
    shot("settings-general-scrolled", 1.0, |a| {
        open_settings_tab(a, 1);
        a.settings_rows_wheel(true);
        a.settings_rows_wheel(true);
    });
    assert!(a.settings_rows_wheel(false));
    assert!(a.settings_rows_wheel(false));
    assert_eq!(a.sui.rows_scroll, 0);
    // changing tab starts at the top again
    a.settings_rows_wheel(true);
    a.settings_set_tab(2);
    assert_eq!(a.sui.rows_scroll, 0);
}

// ---------------------------------------------------------------- audio output switcher

fn three_outputs() {
    output::set_fake(
        &[
            ("spk", "Speakers (Realtek(R) Audio)", false),
            ("hp", "Headphones (WH-1000XM4 Stereo)", true),
            ("mon", "LG ULTRAGEAR (NVIDIA High Definition Audio)", false),
        ],
        Some("spk"),
    );
}

/// The Music slide with three playback devices, Speakers being the default.
fn music_with_outputs(a: &mut App) {
    three_outputs();
    a.out_refresh();
    open_slide(a, SlideId::Music);
}

fn default_output() -> Option<String> {
    output::with_fake(|f| f.cur.clone())
}

/// A click on the output button opens the list; a click on a row makes that device the default for every role.
#[test]
fn output_button_opens_the_list_and_picks_a_device() {
    let mut a = test_app(1.0);
    music_with_outputs(&mut a);
    assert!(!a.out_list_shown());
    assert!(a.out_click(output::BTN.0, output::BTN.1));
    assert!(a.out_list_shown());
    let (rx, ry) = output::row_pos(1);
    assert!(a.out_click(rx + 60.0, ry + 12.0));
    assert_eq!(default_output().as_deref(), Some("hp"));
    assert_eq!(output::with_fake(|f| f.calls.len()), 3, "Console, Multimedia and Communications");
    assert!(!a.out_list_shown(), "the list closes after a pick");
    assert_eq!(a.outs.cur.as_deref(), Some("hp"));
    assert!(a.out_t.elapsed() < Duration::from_secs(1), "the overlay shows the new device");
}

/// The list closes again on the button, on a click outside it (which does nothing else) and on a slide change.
#[test]
fn output_list_closes_on_button_outside_click_and_slide_change() {
    let mut a = test_app(1.0);
    music_with_outputs(&mut a);
    a.out_click(output::BTN.0, output::BTN.1);
    assert!(a.out_click(output::BTN.0, output::BTN.1));
    assert!(!a.out_list_shown(), "the button closes it again");
    a.out_click(output::BTN.0, output::BTN.1);
    assert!(a.out_click(RX - 10.0, 60.0), "a click outside only closes the list");
    assert!(!a.out_list_shown());
    assert_eq!(output::with_fake(|f| f.calls.len()), 0, "nothing was switched");
    a.out_click(output::BTN.0, output::BTN.1);
    a.step(1);
    assert!(!a.out_open, "another slide closes it");
    a.step(-1);
    assert!(!a.out_list_shown());
    a.out_click(output::BTN.0, output::BTN.1);
    a.hover = false;
    assert!(!a.out_list_shown(), "never drawn while the notch is closed");
    a.hover = true;
    a.lyric_mode = true;
    assert!(!a.out_list_shown(), "nor in the lyrics view");
    assert!(!a.out_click(output::BTN.0, output::BTN.1), "the lyrics view has no output button");
}

/// The wheel on the button moves to the next (down) or previous (up) device, wrapping around.
#[test]
fn output_wheel_on_the_button_cycles_devices() {
    let mut a = test_app(1.0);
    music_with_outputs(&mut a);
    let (bx, by) = output::BTN;
    for want in ["hp", "mon", "spk"] {
        assert!(a.out_wheel(bx, by, -120));
        assert_eq!(default_output().as_deref(), Some(want));
        assert_eq!(a.outs.cur.as_deref(), Some(want));
    }
    assert!(a.out_wheel(bx, by, 120));
    assert_eq!(default_output().as_deref(), Some("mon"), "wheel up goes back");
    assert!(!a.out_wheel(MCX, MCY, -120), "elsewhere on the controls the wheel still sets the volume");
}

/// Plugging and unplugging devices updates the list; when the last one goes the list closes.
#[test]
fn output_list_follows_devices_added_and_removed() {
    let mut a = test_app(1.0);
    music_with_outputs(&mut a);
    a.out_click(output::BTN.0, output::BTN.1);
    // speakers unplugged: Windows moves the default to the headphones
    output::with_fake(|f| {
        f.list.retain(|d| d.id != "spk");
        f.cur = Some("hp".into());
    });
    a.out_refresh();
    assert_eq!(a.outs.list.len(), 2);
    assert_eq!(a.outs.current().map(|d| d.id.as_str()), Some("hp"));
    assert!(a.out_list_shown());
    output::with_fake(|f| f.list.push(output::Device { id: "usb".into(), name: "Speakers (USB Audio)".into(), phones: false }));
    a.out_refresh();
    assert_eq!(a.outs.list.len(), 3, "a new device shows up");
    output::with_fake(|f| {
        f.list.clear();
        f.cur = None;
    });
    a.out_refresh();
    assert!(!a.out_list_shown(), "no devices left: the list closes");
    assert!(a.outs.current().is_none());
}

/// If Windows refuses the switch, the sound settings open; nothing else happens.
#[test]
fn output_switch_failure_opens_sound_settings() {
    let mut a = test_app(1.0);
    music_with_outputs(&mut a);
    output::with_fake(|f| f.refuse = true);
    let _ = sys::opened();
    a.out_pick("hp");
    assert_eq!(sys::opened(), vec!["ms-settings:sound".to_string()]);
    assert_eq!(a.outs.cur.as_deref(), Some("spk"), "still the old device");
    assert!(a.out_t.elapsed() > Duration::from_secs(2), "no overlay");
}

/// Picking the current device, or the wheel with only one device, switches nothing; the wheel still shows the name.
#[test]
fn output_single_device_changes_nothing() {
    let mut a = test_app(1.0);
    output::set_fake(&[("spk", "Speakers (Realtek(R) Audio)", false)], Some("spk"));
    a.out_refresh();
    open_slide(&mut a, SlideId::Music);
    assert!(a.out_wheel(output::BTN.0, output::BTN.1, -120), "taken, so the volume does not move");
    assert_eq!(output::with_fake(|f| f.calls.len()), 0);
    assert!(a.out_t.elapsed() < Duration::from_secs(1), "shows which device plays");
    a.out_pick("spk");
    assert_eq!(output::with_fake(|f| f.calls.len()), 0, "already the default");
}

/// Without any playback device the button opens the Windows sound settings.
#[test]
fn output_without_devices_opens_sound_settings() {
    let mut a = test_app(1.0);
    output::set_fake(&[], None);
    a.out_refresh();
    open_slide(&mut a, SlideId::Music);
    let _ = sys::opened();
    assert!(a.out_click(output::BTN.0, output::BTN.1));
    assert!(!a.out_list_shown());
    assert_eq!(sys::opened(), vec!["ms-settings:sound".to_string()]);
}

/// More devices than rows: the wheel over the list scrolls it, and rows map to the scrolled devices.
#[test]
fn output_list_scrolls_with_many_devices() {
    let mut a = test_app(1.0);
    let names: Vec<(String, String)> = (0..6).map(|i| (format!("d{i}"), format!("Speakers {i} (USB Audio)"))).collect();
    let list: Vec<(&str, &str, bool)> = names.iter().map(|(id, n)| (id.as_str(), n.as_str(), false)).collect();
    output::set_fake(&list, Some("d0"));
    a.out_refresh();
    open_slide(&mut a, SlideId::Music);
    a.out_click(output::BTN.0, output::BTN.1);
    let (px, py) = (output::PANEL.0 + 40.0, output::PANEL.1 + 40.0);
    assert!(a.out_wheel(px, py, -120));
    assert_eq!(a.out_scroll, 1);
    for _ in 0..5 {
        a.out_wheel(px, py, -120);
    }
    assert_eq!(a.out_scroll, 6 - output::MAX_ROWS, "stops at the last row");
    assert_eq!(output::with_fake(|f| f.calls.len()), 0, "scrolling the list switches nothing");
    let (rx, ry) = output::row_pos(0);
    a.out_click(rx + 60.0, ry + 12.0);
    assert_eq!(default_output().as_deref(), Some("d2"));
}

#[test]
fn snap_output() {
    shot("music-output-speaker", 1.0, music_with_outputs);
    shot("music-output-headphones", 1.0, |a| {
        music_with_outputs(a);
        output::with_fake(|f| f.cur = Some("hp".into()));
        a.out_refresh();
    });
    for s in SCALES {
        shot("music-output-list", s, |a| {
            music_with_outputs(a);
            a.out_click(output::BTN.0, output::BTN.1);
        });
    }
    shot("music-output-switched", 1.0, |a| {
        music_with_outputs(a);
        a.out_pick("hp");
        a.out_t = Instant::now() - Duration::from_millis(300);
    });
    // the volume overlay is as wide as the output overlay, so it covers the output button too
    shot("music-volume-overlay", 1.0, |a| {
        music_with_outputs(a);
        a.vol_state = (64, false);
        a.vol_t = Instant::now() - Duration::from_millis(300);
    });
    shot("music-output-many", 1.0, |a| {
        output::set_fake(
            &[
                ("a", "Speakers (Realtek(R) Audio)", false),
                ("b", "Headset Earphone (Jabra Evolve2 65 with a long product name)", true),
                ("c", "LG ULTRAGEAR (NVIDIA High Definition Audio)", false),
                ("d", "Headphones (AirPods Pro)", true),
                ("e", "Digital Output (S/PDIF)", false),
            ],
            Some("d"),
        );
        a.out_refresh();
        open_slide(a, SlideId::Music);
        a.out_click(output::BTN.0, output::BTN.1);
        a.out_wheel(output::PANEL.0 + 40.0, output::PANEL.1 + 40.0, -120);
    });
}

/// The lyrics mode shows where the lyrics came from ("via lrc.red"), in both languages.
#[test]
fn lyric_mode_credits_the_source() {
    let _lang = i18n::restore_on_drop();
    for (lang, prefix) in [(i18n::EN, "lyrics"), (i18n::ID, "id-lyrics")] {
        i18n::set(lang);
        for (src, name) in [(lyrics::Source::Lrclib, "lrclib"), (lyrics::Source::LrcRed, "lrcred")] {
            shot(&format!("{prefix}-credit-{name}"), 1.0, |a| {
                a.media.title = "Song".into();
                a.media.artist = "Artist".into();
                a.media.dur = 200;
                a.media.playing = true;
                a.lyrics = lyrics::Lyrics::Synced(lyrics::parse_lrc("[00:01.00]First line
[00:05.00]Second line
[00:09.00]Third line"), src);
                a.lyric_mode = true;
                open_slide(a, SlideId::Music);
            });
        }
    }
}

/// Toasts with a button must not let the hover open the notch (that cancels the toast before the button is pressed).
#[test]
fn toasts_with_buttons_are_not_cancelled_by_hover() {
    let mut a = test_app(1.0);
    assert!(!a.toast_has_button(), "no toast");
    show_toast(&mut a, ToastKind::Water);
    assert!(a.toast_has_button(), "Snooze / Drank");
    show_toast(&mut a, ToastKind::Update(update::Note::Available(release("9.9.9"))));
    assert!(a.toast_has_button(), "Download");
    show_toast(&mut a, ToastKind::Meeting { title: "Standup".into(), at: now_s(), link: Some("https://meet.google.com/abc-defg-hij".into()) });
    assert!(a.toast_has_button(), "Join");
    show_toast(&mut a, ToastKind::Update(update::Note::UpToDate));
    assert!(!a.toast_has_button(), "nothing to press");
    show_toast(&mut a, ToastKind::Charging { pct: 50 });
    assert!(!a.toast_has_button());
}

/// One click on Install starts the download and turns the toast into a progress toast; a failure falls back to the
/// release page; without an installer the button still just opens the release page.
#[test]
fn update_install_flow() {
    let mut a = test_app(1.0);
    let r = release_with_installer("99.0.0");
    show_toast(&mut a, ToastKind::Update(update::Note::Available(r.clone())));
    let (w, h) = toast::TOAST_UPDATE;
    let (bx, by, bw, bh) = update::button_rect(w, h);
    assert!(a.toast_click(bx + bw / 2.0, by + bh / 2.0));
    assert_eq!(update::installs(), vec!["HesewakeNotch-Setup-99.0.0.exe".to_string()], "the download started");
    assert!(sys::opened().is_empty(), "the browser is not opened");
    assert_eq!(update_note(&a), Some(update::Note::Downloading(0)));
    assert!(a.toast_has_button(), "hover must not cancel the progress toast");
    assert!(a.toast_click(bx + bw / 2.0, by + bh / 2.0), "clicks are swallowed while downloading");
    assert!(update::installs().is_empty(), "no second download");
    assert!(a.toast.is_some());

    // a failed download offers the release page
    a.update = Some(r.clone());
    a.set_update_note(update::Note::DownloadFailed(r.clone()));
    assert!(a.toast_click(bx + bw / 2.0, by + bh / 2.0));
    assert_eq!(sys::opened(), vec![r.url.clone()]);
    assert!(a.toast.is_none());

    // no installer asset: the old behaviour
    let plain = release("99.0.0");
    show_toast(&mut a, ToastKind::Update(update::Note::Available(plain.clone())));
    assert!(a.toast_click(bx + bw / 2.0, by + bh / 2.0));
    assert_eq!(sys::opened(), vec![plain.url]);
    assert!(update::installs().is_empty());
}

#[test]
fn snap_update_install_toasts() {
    let _lang = i18n::restore_on_drop();
    for (lang, prefix) in [(i18n::EN, "update"), (i18n::ID, "id-update")] {
        i18n::set(lang);
        shot(&format!("{prefix}-toast-install"), 1.0, |a| show_toast(a, ToastKind::Update(update::Note::Available(release_with_installer("9.9.9")))));
        shot(&format!("{prefix}-toast-downloading"), 1.0, |a| show_toast(a, ToastKind::Update(update::Note::Downloading(63))));
        shot(&format!("{prefix}-toast-download-failed"), 1.0, |a| show_toast(a, ToastKind::Update(update::Note::DownloadFailed(release("9.9.9")))));
    }
}

// ---------------------------------------------------------------- download progress

const MIB: u64 = 1024 * 1024;

fn dl(name: &str, size: u64, speed: u64) -> downloads::DlView {
    downloads::DlView { name: name.into(), path: format!("C:\\Users\\me\\Downloads\\{name}.crdownload"), size, speed }
}

/// A running download takes the countdown slot ("↓ 4.1 MB/s"); a prayer or meeting that is near goes first.
#[test]
fn download_speed_shows_in_the_countdown_slot() {
    let mut a = test_app(1.0);
    a.cfg.prayer_on = false;
    a.dl_apply(vec![dl("report.pdf", 23 * MIB, 3 * MIB), dl("setup.exe", 5 * MIB, MIB + 100 * 1024)], Vec::new());
    assert_eq!(a.dl_speed(), Some(4 * MIB + 100 * 1024), "total of both");
    assert_eq!(a.upcoming(), Some(calendar::Upcoming::Download(4 * MIB + 100 * 1024)));
    let mut g = Gfx::new(1.0);
    let (label, _) = a.countdown_text(&mut g).expect("label");
    assert!(label.ends_with("4.1 MB/s"), "{label}");
    a.cfg.prayer_on = true;
    prayer_in(&mut a, 2, 12);
    assert_eq!(a.upcoming(), Some(calendar::Upcoming::Prayer(2, 12)), "the prayer countdown goes first");
    a.cfg.prayer_on = false;
    a.dl_apply(Vec::new(), Vec::new());
    assert_eq!(a.upcoming(), None, "done: the slot is free again");
}

/// A finished download pops a toast; Open opens the file, Show in folder selects it in Explorer.
#[test]
fn finished_download_toast_opens_or_shows_the_file() {
    let path = "C:\\Users\\me\\Downloads\\report.pdf";
    let mut a = test_app(1.0);
    a.dl_apply(Vec::new(), vec![path.to_string()]);
    assert!(matches!(a.toast.as_ref().map(|t| &t.kind), Some(ToastKind::Downloaded { path: p }) if p == path));
    assert!(a.toast_has_button(), "hovering the toast must not cancel it");
    assert_eq!(a.toast_size(), downloads::TOAST_DOWNLOAD);
    a.toast.as_mut().unwrap().t0 = Instant::now() - Duration::from_secs(1);
    let _ = sys::opened();
    let (open, folder) = downloads::toast_buttons();
    assert!(a.toast_click(open.0 + open.2 / 2.0, open.1 + open.3 / 2.0));
    assert_eq!(sys::opened(), vec![path.to_string()]);
    assert!(a.toast.is_none());

    a.dl_apply(Vec::new(), vec![path.to_string()]);
    a.toast.as_mut().unwrap().t0 = Instant::now() - Duration::from_secs(1);
    assert!(a.toast_click(folder.0 + folder.2 / 2.0, folder.1 + folder.3 / 2.0));
    assert_eq!(sys::opened(), vec![format!("select:{path}")]);
}

/// With the setting off nothing shows, even if the watcher still sent something.
#[test]
fn downloads_off_shows_nothing() {
    let mut a = test_app(1.0);
    a.cfg.downloads = false;
    a.dl_apply(vec![dl("report.pdf", MIB, MIB)], vec!["C:\\x\\a.zip".into()]);
    assert!(a.dls.is_empty());
    assert!(a.toast.is_none());
    assert_eq!(a.dl_speed(), None);
}

/// An App at the size its current state wants, as after the spring has settled.
fn settle(a: &mut App) {
    (a.w, a.h) = anim::target(a);
}

#[test]
fn lock_decides_whether_the_notch_is_open() {
    let mut a = test_app(1.0);
    a.lock = Lock::Min;
    a.hover = true;
    a.peek_until = Some(Instant::now() + Duration::from_secs(8));
    a.drag = Some((0.0, 0.0));
    assert!(!a.expanded(), "locked small: hover, peek and drag do not open it");

    let mut a = test_app(1.0);
    a.lock = Lock::Open;
    assert!(a.expanded(), "locked open: stays open without a hover");
    a.splash = Some(Instant::now());
    assert!(!a.expanded(), "the startup splash still plays");
    a.splash = None;
    show_toast(&mut a, ToastKind::Charging { pct: 50 });
    assert!(!a.expanded(), "shrinks for a toast");
    a.toast = None;
    assert!(a.expanded(), "and opens again after it");
}

#[test]
fn locked_open_shows_toasts_and_opens_again() {
    let mut a = test_app(1.0);
    a.lock = Lock::Open;
    a.push_toast(ToastKind::Charging { pct: 50 });
    assert!(a.toast.is_some(), "not held back by the lock");
    a.toast_update();
    assert!(a.toast.is_some(), "not cancelled by the lock");
    a.toast.as_mut().unwrap().dur = 0.0;
    a.toast_update();
    assert!(a.toast.is_none() && a.expanded(), "open again once the toast ends");

    a.hover = true;
    a.push_toast(ToastKind::Charging { pct: 50 });
    assert!(a.toast.is_none(), "pointer on the panel: the toast waits, as in a normal open notch");
    assert_eq!(a.toast_q.len(), 1);
    a.hover = false;
    a.toast_update();
    assert!(a.toast.is_some(), "and shows once the pointer leaves");
}

#[test]
fn locked_small_keeps_toasts_under_the_pointer() {
    let mut a = test_app(1.0);
    a.lock = Lock::Min;
    a.hover = true;
    a.push_toast(ToastKind::Charging { pct: 50 });
    a.toast_update();
    assert!(a.toast.is_some(), "a hover cancels a toast only by opening the notch");
    assert!(!a.lock_chip(), "the padlock waits until the toast is gone");
}

#[test]
fn padlock_popover_locks_and_unlocks() {
    let (bx, by) = lock::LOCK_BTN;
    let mut a = test_app(1.0);
    open_slide(&mut a, SlideId::Todo);
    settle(&mut a);
    assert!(a.lock_click(bx, by));
    assert!(a.lock_menu, "the padlock opens the popover");
    assert!(!a.lock_click(300.0, 100.0), "a click elsewhere reaches the slide");
    assert!(!a.lock_menu, "and closes the popover");

    a.lock_click(bx, by);
    assert!(a.lock_click(bx + 20.0, 56.0));
    assert_eq!((a.lock, a.lock_menu), (Lock::Open, false), "first row: Lock expanded");
    assert!(a.lock_click(bx, by));
    assert_eq!(a.lock, Lock::Off, "the padlock unlocks without the popover");

    a.lock_click(bx, by);
    assert!(a.lock_click(bx + 20.0, 80.0));
    assert_eq!(a.lock, Lock::Min, "second row: Lock minimize");

    let mut a = test_app(1.0);
    open_slide(&mut a, SlideId::Todo);
    settle(&mut a);
    a.editing = true;
    assert!(!a.lock_click(bx, by), "ignored while editing, like the arrows");
    assert!(!a.lock_menu);
}

#[test]
fn locked_small_hover_reveals_the_padlock() {
    let mut a = test_app(1.0);
    a.lock = Lock::Min;
    a.minimal = true;
    assert!(a.thin());
    assert_eq!(anim::target(&a), THIN);
    a.hover = true;
    assert!(a.lock_chip() && !a.thin(), "the thin line grows so the padlock fits");
    assert_eq!(anim::target(&a), (IDLE.0 + lock::CHIP_W, IDLE.1));
    settle(&mut a);

    assert!(!a.lock_click(CX - 60.0, 16.0), "only the padlock unlocks");
    assert_eq!(a.lock, Lock::Min);
    assert!(a.lock_click(CX + a.w / 2.0 - lock::CHIP_W / 2.0, 16.0));
    assert_eq!(a.lock, Lock::Off);
    assert!(a.expanded(), "unlocked under the pointer: the normal hover opens it");
}

#[test]
fn locked_small_ignores_slide_hotkeys() {
    let mut a = test_app(1.0);
    a.lock = Lock::Min;
    assert!(!a.slide_hotkey(1));
    assert_eq!((a.cur(), a.peek_until), (0, None));
}

#[test]
fn locking_clears_a_pending_peek() {
    let mut a = test_app(1.0);
    a.peek_until = Some(Instant::now() + Duration::from_secs(8));
    a.lock_set(Lock::Min);
    a.lock_set(Lock::Off);
    assert!(!a.expanded(), "a peek from before the lock does not open the notch after unlocking");
    assert_eq!(a.cfg.notch_lock, 0);
    a.lock_set(Lock::Open);
    assert_eq!(a.cfg.notch_lock, 1, "saved for the next start");
}

#[test]
fn snap_lock() {
    let playing = || Media { title: "Almost Easy".into(), artist: "Avenged Sevenfold".into(), playing: true, pos: 60, dur: 240, pos_ms: 60_000, source: "Demo".into(), sources: 1 };
    for s in SCALES {
        shot("lock-button", s, |a| open_slide(a, SlideId::Music));
        shot("lock-popover", s, |a| {
            open_slide(a, SlideId::Music);
            a.lock_menu = true;
        });
        shot("lock-open", s, |a| a.lock = Lock::Open);
        shot("lock-chip-idle", s, |a| {
            a.lock = Lock::Min;
            a.hover = true;
        });
    }
    shot("lock-chip-playing", 1.0, |a| {
        a.lock = Lock::Min;
        a.hover = true;
        a.media = playing();
    });
    shot("lock-chip-thin", 1.0, |a| {
        a.lock = Lock::Min;
        a.minimal = true;
        a.hover = true;
    });
    shot("lock-chip-with-privacy-dots", 1.0, |a| {
        a.lock = Lock::Min;
        a.hover = true;
        a.priv_mic = vec!["Zoom".into()];
    });
}

/// The pointer entering the notch opens Ledge once per new download, not on every hover.
#[test]
fn first_hover_after_a_new_download_opens_ledge() {
    let mut a = test_app(1.0);
    let ledge = a.order.iter().position(|&s| s == SlideId::Ledge).expect("Ledge is on");
    assert!(!a.dl_on_hover(), "no download: nothing");
    a.dl_apply(vec![dl("report.pdf", MIB, MIB)], Vec::new());
    assert!(a.dl_on_hover());
    assert_eq!(a.cur(), ledge);
    a.show(0);
    assert!(!a.dl_on_hover(), "already seen");
    a.dl_apply(vec![dl("report.pdf", 2 * MIB, MIB)], Vec::new());
    assert!(!a.dl_on_hover(), "the same download growing is not new");
    a.dl_apply(vec![dl("report.pdf", 2 * MIB, MIB), dl("setup.exe", MIB, MIB)], Vec::new());
    assert!(a.dl_on_hover(), "a second download is new");
}

/// Ledge lists the running downloads; a click on one selects its file in Explorer.
#[test]
fn ledge_rows_show_the_download_in_its_folder() {
    let mut a = test_app(1.0);
    a.dl_apply(vec![dl("report.pdf", MIB, MIB), dl("setup.exe", MIB, MIB)], Vec::new());
    open_slide(&mut a, SlideId::Ledge);
    let _ = sys::opened();
    let (x, y) = downloads::ledge_row(1);
    assert!(a.dl_ledge_click(x + 40.0, y + downloads::LEDGE_ROW_H / 2.0));
    assert_eq!(sys::opened(), vec![format!("select:{}", a.dls[1].path)]);
    assert!(!a.dl_ledge_click(x + 40.0, downloads::ledge_row(2).1 + 10.0), "below the last row");
}

#[test]
fn download_rows_in_alerts() {
    let a = test_app(1.0);
    let i = a.settings_rows(2).iter().position(|r| r.label == "Download progress").expect("row in Alerts");
    assert!(matches!(a.settings_rows(2)[i].kind, settingsview::RowKind::Toggle(true)), "on by default");
}

#[test]
fn snap_downloads() {
    let _lang = i18n::restore_on_drop();
    for (lang, pre) in [(i18n::EN, ""), (i18n::ID, "id-")] {
        i18n::set(lang);
        let set = move |a: &mut App| {
            a.cfg.lang = lang;
            a.cfg.prayer_on = false;
        };
        shot(&format!("{pre}download-toast"), 1.0, |a| {
            set(a);
            show_toast(a, ToastKind::Downloaded { path: "C:\\Users\\me\\Downloads\\Quarterly report 2026 final version.pdf".into() });
        });
        shot(&format!("{pre}download-ledge"), 1.0, |a| {
            set(a);
            a.dl_apply(vec![dl("report.pdf", 23 * MIB + 400 * 1024, 4 * MIB + 100 * 1024), dl("ubuntu-26.04-desktop-amd64.iso", 1900 * MIB, 11 * MIB)], Vec::new());
            open_slide(a, SlideId::Ledge);
        });
        shot(&format!("{pre}download-ledge-many"), 1.0, |a| {
            set(a);
            a.dl_apply((0..5).map(|i| dl(&format!("photo-{i}.jpg"), (i + 1) * MIB, 300 * 1024)).collect(), Vec::new());
            open_slide(a, SlideId::Ledge);
        });
    }
    shot("download-idle", 1.0, |a| {
        a.cfg.prayer_on = false;
        a.cfg.weather_on = false;
        a.dl_apply(vec![dl("report.pdf", 23 * MIB, 4 * MIB + 100 * 1024)], Vec::new());
    });
    shot("download-idle", 1.5, |a| {
        a.cfg.prayer_on = false;
        a.cfg.weather_on = false;
        a.dl_apply(vec![dl("report.pdf", 23 * MIB, 4 * MIB + 100 * 1024)], Vec::new());
    });
    shot("download-playing", 1.0, |a| {
        a.cfg.prayer_on = false;
        a.media.playing = true;
        a.media.title = "Almost Easy".into();
        a.dl_apply(vec![dl("report.pdf", 23 * MIB, 12 * MIB)], Vec::new());
    });
}

// ---------------------------------------------------------------- prayer list, Ramadan, adhan

/// Subuh 04:11 (Imsak 04:01), Terbit 05:25, Dzuhur 11:40, Ashar 14:52, Maghrib 17:49, Isya 18:59.
fn prayer_day(a: &mut App) {
    a.cfg.prayer_on = true;
    a.prayer = Some((0, [251, 700, 892, 1069, 1139]));
    a.prayer_sun = 325;
}

/// The Prayer slide is in the carousel only while prayer times are on, and the slide hotkey numbers follow.
#[test]
fn prayer_slide_follows_the_prayer_times_setting() {
    let mut a = test_app(1.0);
    assert!(a.order.contains(&SlideId::Prayer), "on by default");
    let i = a.settings_rows(3).iter().position(|r| r.label == "Prayer times").expect("row in Widgets");
    let (cx, cy) = settingsview::cell_pos(i);
    a.sui.tab = 3;
    a.settings_click_rows(3, cx + settingsview::CELL_W - 20.0, cy + 17.0);
    assert!(!a.cfg.prayer_on);
    assert!(!a.order.contains(&SlideId::Prayer), "gone with prayer times off");
    let p = a.cfg.slides.iter().position(|&(s, _)| s == SlideId::Prayer).unwrap();
    assert_eq!(hotkeys::number(&a.slide_cfg(), p), None, "and no hotkey number");
    a.settings_click_rows(3, cx + settingsview::CELL_W - 20.0, cy + 17.0);
    assert!(a.order.contains(&SlideId::Prayer), "back");
}

/// In Ramadan, Imsak gets a toast (no adhan) and Maghrib is "Iftar"; the adhan plays for prayers only, once.
#[test]
fn ramadan_toasts_and_the_adhan() {
    let mut a = test_app(1.0);
    prayer_day(&mut a);
    a.cfg.ramadan = true;
    a.cfg.adhan = adhan::SYSTEM;
    let _ = adhan::played();
    assert!(a.prayer_fire(241));
    assert!(matches!(a.toast.as_ref().map(|t| &t.kind), Some(ToastKind::Prayer { idx: 5, at: 241 })), "Imsak toast");
    assert!(adhan::played().is_empty(), "Imsak is not a prayer: no adhan");
    a.toast = None;
    assert!(a.prayer_fire(1069));
    assert_eq!(prayer::toast_text(3, a.cfg.ramadan).0, "Iftar");
    assert_eq!(adhan::played(), vec!["system".to_string()]);
    a.toast = None;
    assert!(!a.prayer_fire(1069), "once per time");
}

/// Outside Ramadan nothing changes: no Imsak toast or countdown, and Maghrib keeps its name.
#[test]
fn outside_ramadan_nothing_changes() {
    let mut a = test_app(1.0);
    prayer_day(&mut a);
    assert!(!a.prayer_fire(241), "no Imsak toast");
    let mut g = Gfx::new(1.0);
    {
        let _now = prayer::fake_now(241 - 12);
        assert_eq!(a.upcoming(), None, "no Imsak countdown");
        a.cfg.ramadan = true;
        assert_eq!(a.countdown_text(&mut g).map(|(l, _)| l).as_deref(), Some("Imsak 12m"));
        a.cfg.ramadan = false;
    }
    let _now = prayer::fake_now(1069 - 5);
    assert_eq!(a.countdown_text(&mut g).map(|(l, _)| l).as_deref(), Some("Maghrib 5m"));
    a.cfg.ramadan = true;
    assert_eq!(a.countdown_text(&mut g).map(|(l, _)| l).as_deref(), Some("Iftar 5m"));
}

/// No sound at all unless the user turned the adhan on.
#[test]
fn no_adhan_unless_turned_on() {
    let mut a = test_app(1.0);
    prayer_day(&mut a);
    assert_eq!(a.cfg.adhan, adhan::OFF, "off by default");
    let _ = adhan::played();
    assert!(a.prayer_fire(892), "the toast still comes");
    assert!(adhan::played().is_empty());
    assert!(!a.adhan_playing());
}

/// A picked file plays when a prayer starts; the toast stays longer and its Stop button stops it.
#[test]
fn adhan_file_plays_and_stops_from_the_toast() {
    let f = std::env::temp_dir().join(format!("hesewake-adhan-app-{}.mp3", std::process::id()));
    std::fs::write(&f, b"x").unwrap();
    let mut a = test_app(1.0);
    prayer_day(&mut a);
    a.adhan_picked(Some(f.to_string_lossy().into_owned()));
    assert_eq!(a.cfg.adhan, adhan::FILE);
    let _ = adhan::played();
    assert!(a.prayer_fire(892));
    assert_eq!(adhan::played(), vec![format!("file:{}", f.to_string_lossy())]);
    assert!(a.adhan_playing());
    assert!(a.toast.as_ref().unwrap().dur >= 15.0, "the toast stays up while it plays");
    assert!(a.toast_has_button(), "hovering the toast does not cancel it");
    a.toast.as_mut().unwrap().t0 = Instant::now() - Duration::from_secs(1);
    let (bx, by, bw, bh) = prayer::toast_stop_rect(toast::TOAST_CHARGE.0, toast::TOAST_CHARGE.1);
    assert!(a.toast_click(bx + bw / 2.0, by + bh / 2.0));
    assert_eq!(adhan::played(), vec!["stop".to_string()]);
    assert!(!a.adhan_playing());
    assert!(a.toast.is_none());
    let _ = std::fs::remove_file(&f);
}

/// The Prayer slide's Stop button while the adhan plays.
#[test]
fn adhan_stops_from_the_prayer_slide() {
    let mut a = test_app(1.0);
    prayer_day(&mut a);
    a.adhan_until = Some(Instant::now() + Duration::from_secs(60));
    let (bx, by, bw, bh) = prayer::slide_stop_rect();
    let _ = adhan::played();
    assert!(a.prayer_click(bx + bw / 2.0, by + bh / 2.0));
    assert!(!a.adhan_playing());
    assert_eq!(adhan::played(), vec!["stop".to_string()]);
    assert!(!a.prayer_click(bx + bw / 2.0, by + bh / 2.0), "nothing to stop");
}

/// Cancelling the file dialog without a file falls back to the system sound; a file keeps "File".
#[test]
fn adhan_file_dialog_answers() {
    let mut a = test_app(1.0);
    a.cfg.adhan = adhan::FILE;
    a.adhan_picked(None);
    assert_eq!(a.cfg.adhan, adhan::SYSTEM, "cancelled with no file");
    a.adhan_picked(Some("C:\\Music\\adzan.mp3".into()));
    assert_eq!((a.cfg.adhan, a.cfg.adhan_file.as_str()), (adhan::FILE, "C:\\Music\\adzan.mp3"));
    a.adhan_picked(None);
    assert_eq!((a.cfg.adhan, a.cfg.adhan_file.as_str()), (adhan::FILE, "C:\\Music\\adzan.mp3"), "cancelling a change keeps the file");
}

#[test]
fn snap_prayer_slide() {
    let _lang = i18n::restore_on_drop();
    let _now = prayer::fake_now(860); // 14:20: Ashar (14:52) is next
    for (lang, pre) in [(i18n::EN, ""), (i18n::ID, "id-")] {
        i18n::set(lang);
        let set = move |a: &mut App| {
            a.cfg.lang = lang;
            prayer_day(a);
        };
        shot(&format!("{pre}prayer-slide"), 1.0, |a| {
            set(a);
            open_slide(a, SlideId::Prayer);
        });
        shot(&format!("{pre}prayer-slide-ramadan"), 1.0, |a| {
            set(a);
            a.cfg.ramadan = true;
            open_slide(a, SlideId::Prayer);
        });
        shot(&format!("{pre}prayer-toast-imsak"), 1.0, |a| {
            set(a);
            a.cfg.ramadan = true;
            show_toast(a, ToastKind::Prayer { idx: prayer::IMSAK as u8, at: 241 });
        });
        shot(&format!("{pre}prayer-toast-iftar"), 1.0, |a| {
            set(a);
            a.cfg.ramadan = true;
            show_toast(a, ToastKind::Prayer { idx: 3, at: 1069 });
        });
        shot(&format!("{pre}prayer-toast-adhan"), 1.0, |a| {
            set(a);
            a.adhan_until = Some(Instant::now() + Duration::from_secs(60));
            show_toast(a, ToastKind::Prayer { idx: 2, at: 892 });
        });
        shot(&format!("{pre}prayer-settings-widgets"), 1.0, |a| {
            set(a);
            a.cfg.adhan = adhan::FILE;
            a.cfg.adhan_file = "C:\\Users\\me\\Music\\Adzan Makkah.mp3".into();
            open_settings_tab(a, 3);
            a.settings_rows_wheel(true);
            a.settings_rows_wheel(true);
        });
    }
    shot("prayer-slide", 1.5, |a| {
        prayer_day(a);
        a.cfg.ramadan = true;
        open_slide(a, SlideId::Prayer);
    });
    shot("prayer-slide-five", 1.0, |a| {
        prayer_day(a);
        a.cfg.imsak_terbit = false;
        open_slide(a, SlideId::Prayer);
    });
    shot("prayer-slide-adhan", 1.0, |a| {
        prayer_day(a);
        a.adhan_until = Some(Instant::now() + Duration::from_secs(60));
        open_slide(a, SlideId::Prayer);
    });
    {
        let _imsak = prayer::fake_now(241 - 12);
        shot("prayer-idle-imsak", 1.0, |a| {
            prayer_day(a);
            a.cfg.ramadan = true;
            a.cfg.weather_on = false;
        });
    }
    let _iftar = prayer::fake_now(1069 - 12);
    shot("prayer-idle-iftar", 1.0, |a| {
        prayer_day(a);
        a.cfg.ramadan = true;
        a.cfg.weather_on = false;
    });
}

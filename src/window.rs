//! Window procedure Win32 (pesan mouse, timer, hotkey, drop file, dll).

use super::*;

use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};

// ---------------------------------------------------------------- window
/// Panic di dalam window procedure (extern "system") akan langsung mematikan proses tanpa pesan.
/// Dibungkus catch_unwind supaya app tetap hidup; detail panic ditulis ke %APPDATA%\hesewake-notch\crash.log.
pub(crate) unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match std::panic::catch_unwind(|| wndproc_inner(hwnd, msg, wp, lp)) {
        Ok(r) => r,
        Err(_) => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn wndproc_inner(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            tick(wp.0);
            LRESULT(0)
        }
        WM_APP => {
            if demo_playing() {
                return LRESULT(0);
            }
            with_app(|a| {
                let was = a.media.playing;
                let prev_pos = a.media.pos_ms;
                a.media = a.shared.lock().unwrap().clone();
                let key = a.media.key();
                if key != a.art_key {
                    a.prev_art = a.art.take();
                    a.art_key = key;
                    a.title_t0 = a.t;
                    a.lyrics = if a.media.title.is_empty() || lyrics::skip(&a.media.source, &a.media.title, &a.media.artist, a.media.dur) {
                        lyrics::Lyrics::None
                    } else {
                        lyrics::Lyrics::Loading
                    };
                    a.lyric_idx = None;
                    a.lyric_w = 0.0;
                }
                if a.media.pos_ms != prev_pos {
                    a.pos_at = Instant::now();
                }
                let lyr = {
                    let mut slot = a.lyr_shared.lock().unwrap_or_else(|p| p.into_inner());
                    // Hasil untuk lagu lain dibiarkan (mungkin menunggu WM_APP berikutnya) atau dibuang bila sudah basi.
                    match slot.as_ref() {
                        Some((k, _)) if *k == a.art_key => slot.take(),
                        _ => None,
                    }
                };
                if let Some((_, l)) = lyr {
                    a.lyrics = l;
                    a.lyric_idx = None;
                    a.lyric_w = 0.0;
                }
                // Ambil cover hanya bila kuncinya cocok dengan lagu yang tampil. Cover yang datang lebih dulu dari
                // info lagu (thread media memposting art sebelum memperbarui `shared`) dibiarkan di slot sampai
                // WM_APP berikutnya; kalau dibuang di sini, cover tidak pernah dikirim ulang.
                let art = {
                    let mut slot = a.art_shared.lock().unwrap_or_else(|p| p.into_inner());
                    match slot.as_ref() {
                        Some(d) if d.key == a.art_key => slot.take(),
                        _ => None,
                    }
                };
                if let Some(d) = art {
                    a.set_art(&d.rgba);
                }
                // Kamus romaji baru selesai diunduh (tombol Get di Settings): nyalakan fitur dan konversi lagu sekarang.
                if dict::take_ready() {
                    a.cfg.romaji_lyrics = true;
                    romaji::set_enabled(true);
                    a.save_settings();
                    a.romaji_refresh();
                }
                if a.media.playing && !was && !a.expanded() {
                    a.show_id(SlideId::Music);
                }
                a.last = Instant::now();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            // Koordinat client fisik -> logis.
            let k = scale::current();
            let x = (lp.0 & 0xffff) as i16 as f32 / k;
            let y = ((lp.0 >> 16) & 0xffff) as i16 as f32 / k;
            let _ = SetCapture(hwnd);

            // Press inside the note card while editing: caret placement / drag-selection,
            // not a swipe or a click on the slide.
            let shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
            let mut in_note = false;
            with_app(|a| {
                in_note = a.note_mouse_down(x, y, shift);
            });
            if in_note {
                return LRESULT(0);
            }

            with_app(|a| {
                a.drag = Some((x, y));
                a.drag_dx = 0.0;
                a.shelf_drag = None;
                // Deteksi kalau tombol ditekan di atas item shelf (bukan tombol ×): siapkan drag-out.
                if a.ready() && !a.editing && a.is_slide(SlideId::Ledge) {
                    for i in 0..a.shelf.len() {
                        let tx = LX + i as f32 * (SH_W + SH_GAP);
                        if in_rect(x, y, tx, SH_Y, SH_W, SH_H) {
                            let (bx, by) = (tx + SH_W - 10.0, SH_Y + 10.0);
                            let d2 = (x - bx).powi(2) + (y - by).powi(2);
                            if d2.sqrt() >= 11.0 {
                                a.shelf_drag = Some((i, x, y));
                            }
                            break;
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            // Koordinat client fisik -> logis.
            let k = scale::current();
            let x = (lp.0 & 0xffff) as i16 as f32 / k;
            let y = ((lp.0 >> 16) & 0xffff) as i16 as f32 / k;
            let mut start: Option<(usize, String)> = None;
            with_app(|a| {
                // Drag-selecting text in the note: extend the selection (scrolls with the caret).
                if a.nv.sel_drag {
                    a.note_mouse_drag(x, y);
                    return;
                }
                // Bila user sudah cukup jauh dari posisi awal shelf: mulai OLE drag-out.
                if let Some((idx, sx, sy)) = a.shelf_drag {
                    let d2 = (x - sx).powi(2) + (y - sy).powi(2);
                    if d2 >= 36.0 {
                        if let Some(it) = a.shelf.get(idx) {
                            start = Some((idx, it.path.clone()));
                        }
                        a.shelf_drag = None;
                        a.drag = None;
                        a.drag_dx = 0.0;
                    }
                }
                if let Some((sx, _)) = a.drag {
                    let dx = x - sx;
                    let nd = if dx.abs() > 8.0 { dx } else { 0.0 };
                    if nd != a.drag_dx {
                        a.drag_dx = nd;
                        present(a);
                    }
                }
            });
            if let Some((idx, path)) = start {
                // DoDragDrop punya loop sendiri; keluar dulu dari lock & capture.
                let _ = ReleaseCapture();
                let moved = dragdrop::start_shelf_drag(&path);
                with_app(|a| {
                    if moved && idx < a.shelf.len() {
                        a.shelf.remove(idx);
                        save_shelf(&a.shelf);
                    }
                    a.last = Instant::now();
                    present(a);
                    schedule(a);
                });
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            let mut click_at: Option<(f32, f32)> = None;
            with_app(|a| {
                a.shelf_drag = None;
                a.nv.sel_drag = false;
                if let Some((sx, sy)) = a.drag.take() {
                    let dx = a.drag_dx;
                    a.drag_dx = 0.0;
                    if dx.abs() >= 40.0 && !a.editing {
                        a.sp -= dx / 160.0;
                        a.sv = 0.0;
                        a.step(if dx < 0.0 { 1 } else { -1 });
                        a.last = Instant::now();
                    } else if dx.abs() < 8.0 {
                        click_at = Some((sx, sy));
                    } else {
                        a.sp -= dx / 160.0;
                        a.sv = 0.0;
                        a.last = Instant::now();
                    }
                    present(a);
                    schedule(a);
                }
            });
            if let Some((x, y)) = click_at {
                click(x, y);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let d = ((wp.0 >> 16) & 0xffff) as i16 as i32;
            with_app(|a| {
                if d == 0 || !a.ready() {
                    return;
                }
                let vertical = msg == WM_MOUSEWHEEL;
                // Slide Music: scroll di atas area kontrol memutar volume master (bukan pindah slide).
                if vertical && a.is_slide(SlideId::Music) && !a.lyric_mode {
                    let mut p = POINT::default();
                    if GetCursorPos(&mut p).is_ok() {
                        let (x, y) = ((p.x - a.wx) as f32 / a.scale, (p.y - a.wy) as f32 / a.scale);
                        // the output button and list come first (the button sits inside the volume area)
                        if a.out_wheel(x, y, d) {
                            a.last = Instant::now();
                            present(a);
                            schedule(a);
                            return;
                        }
                        if in_rect(x, y, MX - 10.0, 100.0, MW + 20.0, 56.0) {
                            if let Some((vol, muted)) = audio::adjust(audio::wheel_step(d)) {
                                // Notch sedang terbuka: indikator di dalam slide (toast dibatalkan saat hover).
                                a.vol_state = ((vol * 100.0).round() as u8, muted);
                                a.vol_t = Instant::now();
                                a.last = Instant::now();
                                present(a);
                                schedule(a);
                            }
                            return;
                        }
                    }
                }
                // Scrap slide: wheel / trackpad scrolls a long note instead of switching slides.
                if vertical && a.is_slide(SlideId::Scrap) && a.note_wheel(d) {
                    present(a);
                    return;
                }
                if vertical && a.is_slide(SlideId::Notifications) && a.last_wheel.elapsed() > Duration::from_millis(90) && a.notif_wheel(d) {
                    a.last_wheel = Instant::now();
                    present(a);
                    return;
                }
                if vertical && a.is_slide(SlideId::Settings) && a.sui.tab == 0 && a.slides_scrollable() {
                    if a.last_wheel.elapsed() >= Duration::from_millis(90) {
                        a.last_wheel = Instant::now();
                        if a.settings_slides_wheel(d < 0) {
                            present(a);
                        }
                    }
                    return;
                }
                if vertical && a.is_slide(SlideId::Settings) && a.rows_scrollable() {
                    if a.last_wheel.elapsed() >= Duration::from_millis(90) {
                        a.last_wheel = Instant::now();
                        if a.settings_rows_wheel(d < 0) {
                            present(a);
                        }
                    }
                    return;
                }
                if vertical && a.is_slide(SlideId::Agenda) && a.agenda_scrollable() {
                    if a.last_wheel.elapsed() >= Duration::from_millis(90) {
                        a.last_wheel = Instant::now();
                        if a.agenda_wheel(d < 0) {
                            present(a);
                        }
                    }
                    return;
                }
                if vertical && a.is_slide(SlideId::Clipboard) && a.clips.len() > clipboard::ROWS {
                    if a.last_wheel.elapsed() >= Duration::from_millis(90) {
                        a.last_wheel = Instant::now();
                        if a.clip_wheel(d < 0) {
                            present(a);
                        }
                    }
                    return;
                }
                if vertical && a.is_slide(SlideId::Todo) && !a.editing && a.todos.len() > 3 {
                    if a.last_wheel.elapsed() < Duration::from_millis(90) {
                        return;
                    }
                    a.last_wheel = Instant::now();
                    let max = a.todos.len() - 3;
                    a.todo_scroll = if d > 0 { a.todo_scroll.saturating_sub(1) } else { (a.todo_scroll + 1).min(max) };
                    present(a);
                    return;
                }
                if a.editing {
                    return;
                }
                if a.last_wheel.elapsed() < Duration::from_millis(260) {
                    return;
                }
                let dir = if vertical {
                    if d > 0 { -1 } else { 1 }
                } else if d > 0 {
                    1
                } else {
                    -1
                };
                a.last_wheel = Instant::now();
                a.step(dir);
                a.last = Instant::now();
                schedule(a);
            });
            LRESULT(0)
        }
        WM_KEYDOWN => {
            // Arrows, Home/End, PageUp/PageDown, Delete (+ Shift / Ctrl) while editing the note.
            let vk = wp.0 as u32;
            let shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
            let ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
            let mut handled = false;
            with_app(|a| {
                if a.editing && a.note_mode {
                    handled = a.note_key(vk, shift, ctrl);
                }
            });
            if handled {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        WM_CHAR => {
            let c = wp.0 as u32;
            with_app(|a| {
                if !a.editing {
                    if c == 'h' as u32 || c == 'H' as u32 {
                        toggle_idle_phase(a);
                    }
                    return;
                }
                if a.note_mode {
                    a.note_char(c);
                    return;
                }
                match c {
                    13 => a.finish_edit(true, true),
                    27 => a.finish_edit(false, true),
                    8 => {
                        a.input.pop();
                    }
                    c if c >= 32 => {
                        if let Some(ch) = char::from_u32(c) {
                            if a.input.chars().count() < 60 {
                                a.input.push(ch);
                            }
                        }
                    }
                    _ => {}
                }
                a.edit_start = Instant::now();
                a.caret_on = true;
                a.last = Instant::now();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        WM_KILLFOCUS => {
            with_app(|a| {
                a.nv.sel_drag = false;
                if a.editing {
                    a.finish_edit(false, false);
                    a.last = Instant::now();
                    present(a);
                    schedule(a);
                }
            });
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let mut minimal = false;
            let mut notif_on = true;
            with_app(|a| {
                a.menu_open = true;
                minimal = a.minimal;
                notif_on = a.notif_on;
            });
            let cmd = context_menu(hwnd, sys::autostart_enabled(), minimal, notif_on);
            run_command(hwnd, cmd);
            LRESULT(0)
        }
        update::WM_UPDATE => {
            if let Some((manual, release)) = update::take() {
                with_app(|a| {
                    a.on_update(manual, release);
                    present(a);
                    schedule(a);
                });
            }
            LRESULT(0)
        }
        update::WM_UPDATE_DL => {
            with_app(|a| {
                a.on_update_dl();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        calendar::WM_CALENDAR => {
            with_app(|a| {
                // ignore a late result for a link that was just removed or replaced
                if let Some((link, status, events)) = calendar::take().filter(|(l, ..)| Some(l) == a.cal_link.as_ref()) {
                    let _ = link;
                    a.cal_update(status, events);
                    a.last = Instant::now();
                    present(a);
                    schedule(a);
                }
            });
            LRESULT(0)
        }
        weather::WM_WEATHER => {
            with_app(|a| {
                a.last = Instant::now();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        sys::WM_BRIGHTNESS => {
            let pct = wp.0 as u8;
            perf::note(&format!("brightness event {pct}%"));
            with_app(|a| {
                // peristiwa pertama datang saat pendaftaran: hanya dicatat
                if a.bright_seen && a.cfg.hud_brightness {
                    a.push_toast(ToastKind::Hud { kind: HudKind::Brightness(pct), on: true });
                    a.last = Instant::now();
                    present(a);
                    schedule(a);
                }
                a.bright_seen = true;
            });
            LRESULT(0)
        }
        output::WM_OUTPUT => {
            with_app(|a| {
                a.out_refresh();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        audio::WM_VOLUME => {
            let (pct, muted) = audio::unpack(wp.0);
            with_app(|a| {
                // indikator di slide Music (bila terbuka) + HUD di notch kecil
                a.vol_state = (pct, muted);
                a.vol_t = Instant::now();
                if a.cfg.hud_volume {
                    a.push_toast(ToastKind::Hud { kind: HudKind::Volume(pct, muted), on: true });
                }
                a.last = Instant::now();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        mic::WM_MIC => {
            with_app(|a| {
                a.mic_apply(mic::unpack(wp.0));
                a.last = Instant::now();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        downloads::WM_DOWNLOADS => {
            if let Some((list, finished)) = downloads::take_update() {
                with_app(|a| {
                    a.dl_apply(list, finished);
                    a.last = Instant::now();
                    present(a);
                    schedule(a);
                });
            }
            LRESULT(0)
        }
        adhan::WM_ADHAN_FILE => {
            if let Some(picked) = adhan::take_picked() {
                with_app(|a| {
                    a.adhan_picked(picked);
                    a.cfg.save();
                    present(a);
                    schedule(a);
                });
            }
            LRESULT(0)
        }
        wibu::WM_WIBU_FILE => {
            if let Some(picked) = wibu::take_picked() {
                with_app(|a| {
                    a.wibu_picked(picked);
                    a.cfg.save();
                    present(a);
                    schedule(a);
                });
            }
            LRESULT(0)
        }
        wibu::WM_WIBU_READY => {
            if let Some(loaded) = wibu::take_loaded() {
                with_app(|a| {
                    a.wibu_loaded(loaded);
                    present(a);
                    schedule(a);
                });
            }
            LRESULT(0)
        }
        privacy::WM_PRIVACY => {
            with_app(|a| {
                a.privacy_apply();
                a.last = Instant::now();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        keys::WM_LOCKKEY => {
            let vk = wp.0 as u32;
            if let Some(kind) = keys::kind_of(vk) {
                // bit toggle (bit 0) = keadaan tombol kunci saat ini
                let on = (GetKeyState(vk as i32) & 1) != 0;
                with_app(|a| {
                    a.push_toast(ToastKind::Hud { kind, on });
                    a.last = Instant::now();
                    present(a);
                    schedule(a);
                });
            }
            LRESULT(0)
        }
        notif::WM_NOTIF => {
            with_app(|a| {
                let evs: Vec<_> = a.notif_slot.lock().unwrap_or_else(|p| p.into_inner()).drain(..).collect();
                a.notif_apply(evs);
                a.last = Instant::now();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        bt::WM_BT => {
            with_app(|a| {
                let kinds: Vec<_> = a.bt_shared.lock().unwrap_or_else(|p| p.into_inner()).drain(..).collect();
                for k in kinds {
                    a.push_toast(k);
                }
                a.last = Instant::now();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        sys::WM_TRAY => {
            match (lp.0 & 0xffff) as u32 {
                WM_RBUTTONUP | WM_CONTEXTMENU => {
                    let (mut minimal, mut notif_on, mut update) = (false, true, None);
                    with_app(|a| {
                        a.menu_open = true;
                        minimal = a.minimal;
                        notif_on = a.notif_on;
                        update = a.update.as_ref().map(|r| r.version.clone());
                    });
                    let cmd = sys::tray_menu(hwnd, sys::autostart_enabled(), minimal, notif_on, update.as_deref());
                    run_command(hwnd, cmd);
                }
                WM_LBUTTONUP => {
                    // Klik kiri pada ikon tray: tampilkan notch sebentar.
                    with_app(|a| {
                        a.peek_until = Some(Instant::now() + Duration::from_secs(4));
                        a.last = Instant::now();
                        present(a);
                        schedule(a);
                    });
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            // Pesan daya dikirim sinkron dan bisa tiba saat state aplikasi sedang dipegang (with_app lalu diam-diam
            // dilewati). Karena itu hanya dicatat di sini dan diteruskan lewat pesan yang di-post.
            if let Some(pct) = sys::brightness_from(wp, lp) {
                let _ = PostMessageW(hwnd, sys::WM_BRIGHTNESS, WPARAM(pct as usize), LPARAM(0));
            }
            // PBT_APMPOWERSTATUSCHANGE: charger dicolok/dicabut atau baterai berubah.
            if wp.0 == 0xA {
                let _ = PostMessageW(hwnd, sys::WM_POWERCHECK, WPARAM(0), LPARAM(0));
            }
            if let Some(off) = sys::display_off_from(wp, lp) {
                notif::set_paused(off);
                let _ = PostMessageW(hwnd, sys::WM_DISPLAY, WPARAM(off as usize), LPARAM(0));
            }
            LRESULT(1)
        }
        sys::WM_POWERCHECK => {
            with_app(|a| {
                a.power_check();
                a.last = Instant::now();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        sys::WM_DISPLAY => {
            let off = wp.0 != 0;
            with_app(|a| {
                a.display_off = off;
                if off {
                    let _ = KillTimer(hwnd, 2);
                    schedule(a); // want = 0 -> timer animasi mati
                } else {
                    SetTimer(hwnd, 2, 100, None);
                    a.last = Instant::now();
                    present(a);
                    schedule(a);
                }
            });
            LRESULT(0)
        }
        WM_DPICHANGED => {
            // Scaling monitor diubah (atau window pindah monitor): skala dihitung ulang, ukuran saran Windows diabaikan.
            with_app(|a| {
                a.place(true);
                a.last = Instant::now();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            // monitor dicolok/dicabut atau resolusi berubah: hitung ulang tempat notch
            with_app(|a| {
                a.place(true);
                a.last = Instant::now();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        WM_HOTKEY => {
            let id = wp.0 as i32;
            if id == HOTKEY_ID {
                with_app(|a| toggle_idle_phase(a));
            } else if id == hotkeys::MIC_ID {
                with_app(|a| {
                    a.mic_hotkey();
                    a.last = Instant::now();
                    present(a);
                    schedule(a);
                });
            } else if let Some(n) = hotkeys::slide_of(id) {
                with_app(|a| {
                    if a.slide_hotkey(n) {
                        a.last = Instant::now();
                        present(a);
                        schedule(a);
                    }
                });
            }
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            with_app(|a| {
                // with the slide off the clipboard is not even read
                let text = if a.clip_enabled() { clipboard::read(a.hwnd) } else { None };
                if a.on_clipboard(text, Instant::now()) && a.expanded() && a.is_slide(SlideId::Clipboard) {
                    present(a);
                }
            });
            LRESULT(0)
        }
        WM_DROPFILES => {
            let hdrop = HDROP(wp.0 as *mut c_void);
            let n = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
            let mut paths: Vec<String> = Vec::new();
            for i in 0..n {
                let len = DragQueryFileW(hdrop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                let got = DragQueryFileW(hdrop, i, Some(&mut buf)) as usize;
                paths.push(String::from_utf16_lossy(&buf[..got.min(buf.len())]));
            }
            DragFinish(hdrop);
            with_app(|a| {
                a.shelf_add(paths);
                if !a.editing {
                    a.show_id(SlideId::Ledge);
                }
                a.peek_until = Some(Instant::now() + Duration::from_secs(4));
                a.last = Instant::now();
                present(a);
                schedule(a);
            });
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(3),
        WM_DESTROY => {
            sys::tray_remove(hwnd);
            with_app(|a| {
                if a.note_mode {
                    save_note(&a.note);
                }
            });
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ if msg == sys::taskbar_created_msg() => {
            sys::tray_add(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Jalankan perintah dari context menu notch atau tray (id menu di `sys::CMD_*`).
unsafe fn run_command(hwnd: HWND, cmd: i32) {
    let mut show_about = false;
    with_app(|a| {
        a.menu_open = false;
        match cmd {
            sys::CMD_AUTOSTART => sys::set_autostart(!sys::autostart_enabled()),
            sys::CMD_MINIMAL => {
                a.minimal = !a.minimal;
                a.save_settings();
            }
            sys::CMD_ABOUT => show_about = true,
            sys::CMD_SETTINGS => a.open_settings(),
            sys::CMD_UPDATE_OPEN => {
                if let Some(r) = &a.update {
                    sys::open_url(&r.url);
                }
            }
            sys::CMD_UPDATE_CHECK => a.update_check_now(),
            sys::CMD_NOTIF => {
                a.notif_on = !a.notif_on;
                notif::set_enabled(a.notif_on);
                a.save_settings();
                if a.notif_on {
                    if !a.notif_started {
                        a.notif_started = true;
                        notif::start(hwnd.0 as isize, a.notif_slot.clone());
                    }
                } else {
                    a.notifs.clear();
                }
            }
            sys::CMD_NOTIF_ACCESS => sys::open_notification_settings(),
            _ => {}
        }
        a.last = Instant::now();
        present(a);
        schedule(a);
    });
    if show_about {
        sys::about(hwnd);
    }
    if cmd == sys::CMD_EXIT {
        let _ = DestroyWindow(hwnd);
    }
}
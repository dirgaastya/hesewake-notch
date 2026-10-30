//! Input mouse: klik per slide dan context menu.

use super::*;

// ---------------------------------------------------------------- input
pub(crate) unsafe fn click(x: f32, y: f32) {
    with_app(|a| {
        if a.toast_click(x, y) {
            a.last = Instant::now();
            present(a);
            schedule(a);
            return;
        }
        if a.lock_click(x, y) {
            a.last = Instant::now();
            present(a);
            schedule(a);
            return;
        }
        if !a.ready() {
            return;
        }
        let near = |c: (f32, f32)| ((x - c.0).powi(2) + (y - c.1).powi(2)).sqrt() < 16.0;
        let lm = a.lyric_screen();
        if !lm && (near(NAV_L) || near(NAV_R)) {
            if a.editing {
                return;
            }
            a.step(if near(NAV_L) { -1 } else { 1 });
            a.last = Instant::now();
            present(a);
            schedule(a);
            return;
        }
        if y >= EXP_H - 22.0 && !a.editing && !lm {
            for i in 0..a.n_slides() {
                if (x - dot_x(i, a.n_slides())).abs() < 8.0 {
                    a.show(i);
                    a.last = Instant::now();
                    schedule(a);
                    return;
                }
            }
        }
        match a.cur_id() {
            SlideId::Music if a.lyric_mode => {
                a.lyric_click(x, y);
                a.last = Instant::now();
                present(a);
                schedule(a);
            }
            SlideId::Music => {
                let d = |px: f32, py: f32| ((x - px).powi(2) + (y - py).powi(2)).sqrt();
                if a.out_click(x, y) {
                    a.last = Instant::now();
                    present(a);
                    schedule(a);
                } else if a.media.sources > 1 && a.lyric_text().is_none() && in_rect(x, y, LX, 6.0, 190.0, 24.0) {
                    let _ = a.tx.send(Cmd::CycleSource);
                } else if d(MCX + 92.0, MCY) < 14.0 {
                    a.lyric_toggle_mode();
                    a.last = Instant::now();
                    present(a);
                    schedule(a);
                } else if d(MCX - 56.0, MCY) < 17.0 {
                    let _ = a.tx.send(Cmd::Prev);
                } else if d(MCX, MCY) < 19.0 {
                    let _ = a.tx.send(Cmd::Toggle);
                } else if d(MCX + 56.0, MCY) < 17.0 {
                    let _ = a.tx.send(Cmd::Next);
                } else if a.media.dur > 0 && in_rect(x, y, MX, 88.0, MW, 16.0) {
                    let _ = a.tx.send(Cmd::Seek(((x - MX) / MW).clamp(0.0, 1.0)));
                }
            }
            SlideId::Water => {
                a.water_click(x, y);
                present(a);
                schedule(a);
            }
            SlideId::Todo => {
                if a.editing {
                    return;
                }
                if in_rect(x, y, LX, 116.0, 92.0, 28.0) {
                    a.begin_edit(None);
                    schedule(a);
                } else if in_rect(x, y, LX + 98.0, 116.0, 92.0, 28.0) {
                    a.edit_mode = !a.edit_mode;
                } else {
                    let (rx, rw, y0, max) = rows_geom(false);
                    let order = a.todo_order();
                    for vi in 0..max {
                        let idx = a.todo_scroll + vi;
                        if idx >= order.len() {
                            break;
                        }
                        let ri = order[idx];
                        let ry = y0 + vi as f32 * ROW_P;
                        if in_rect(x, y, rx, ry, rw, ROW_H) {
                            if a.edit_mode {
                                if x > rx + rw - 24.0 {
                                    a.todos.remove(ri);
                                    save_todos(&a.todos);
                                    a.fix_scroll();
                                } else {
                                    a.begin_edit(Some(ri));
                                    schedule(a);
                                }
                            } else {
                                a.todos[ri].0 = !a.todos[ri].0;
                                save_todos(&a.todos);
                            }
                            break;
                        }
                    }
                }
                present(a);
            }
            SlideId::Ledge if !a.dls.is_empty() => {
                a.dl_ledge_click(x, y);
                present(a);
            }
            SlideId::Ledge => {
                let mut hit: Option<(usize, bool)> = None;
                for i in 0..a.shelf.len() {
                    let tx = LX + i as f32 * (SH_W + SH_GAP);
                    if in_rect(x, y, tx, SH_Y, SH_W, SH_H) {
                        let (bx, by) = (tx + SH_W - 10.0, SH_Y + 10.0);
                        hit = Some((i, ((x - bx).powi(2) + (y - by).powi(2)).sqrt() < 11.0));
                        break;
                    }
                }
                if let Some((i, close)) = hit {
                    // Remove on "x", or when the file/folder no longer exists; otherwise open it.
                    if close || !std::path::Path::new(&a.shelf[i].path).exists() {
                        a.shelf.remove(i);
                        save_shelf(&a.shelf);
                    } else {
                        open_path(&a.shelf[i].path);
                    }
                }
                present(a);
            }
            SlideId::Notifications => {
                a.notif_click(x, y);
                present(a);
                schedule(a);
            }
            SlideId::Scrap => {
                let in_card = in_rect(x, y, LX, NOTE_Y, CW, NOTE_H);
                if a.note_mode {
                    if !in_card {
                        a.finish_edit(false, true);
                    }
                } else if !a.note.is_empty() && in_rect(x, y, RX - 56.0, 8.0, 56.0, 24.0) {
                    a.note.clear();
                    save_note(&a.note);
                } else if in_card {
                    a.begin_note();
                    schedule(a);
                }
                present(a);
            }
            SlideId::System => {}
            SlideId::Prayer => {
                if a.prayer_click(x, y) {
                    present(a);
                    schedule(a);
                }
            }
            SlideId::Agenda => {
                let (now, off) = (notif::now_unix() as i64, calendar::local_offset());
                if let Some(calendar::AgendaHit::Join(i)) = a.agenda_hit(x, y, now, off) {
                    if let Some(link) = a.agenda(now, off).get(i).and_then(|e| e.link.clone()) {
                        sys::open_url(&link);
                    }
                }
            }
            SlideId::Clipboard => {
                if let Some(text) = a.clip_hit(x, y).and_then(|hit| a.clip_apply(hit)) {
                    note::clip_set(HWND(a.hwnd as _), &text);
                }
                a.last = Instant::now();
                present(a);
                schedule(a);
            }
            SlideId::Timer => {
                a.timer_click(x, y);
                present(a);
                schedule(a);
            }
            SlideId::Settings => {
                a.settings_click(x, y);
                present(a);
                schedule(a);
            }
        }
    });
}

pub(crate) unsafe fn context_menu(hwnd: HWND, autostart: bool, minimal: bool, notif_on: bool, lock: Lock) -> i32 {
    let menu = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return 0,
    };
    let chk = |on: bool| if on { MF_STRING | MF_CHECKED } else { MF_STRING };
    if lock != Lock::Off {
        let _ = AppendMenuW(menu, MF_STRING, sys::CMD_UNLOCK as usize, &HSTRING::from(tr("Unlock notch")));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    }
    let _ = AppendMenuW(menu, chk(autostart), sys::CMD_AUTOSTART as usize, &HSTRING::from(tr("Start with Windows")));
    let _ = AppendMenuW(menu, chk(minimal), sys::CMD_MINIMAL as usize, &HSTRING::from(tr("Minimal notch")));
    let _ = AppendMenuW(menu, chk(notif_on), sys::CMD_NOTIF as usize, &HSTRING::from(tr("Windows notifications")));
    let _ = AppendMenuW(menu, MF_STRING, sys::CMD_NOTIF_ACCESS as usize, &HSTRING::from(tr("Notification access...")));
    let settings = if lock == Lock::Min { MF_STRING | MF_GRAYED } else { MF_STRING };
    let _ = AppendMenuW(menu, settings, sys::CMD_SETTINGS as usize, &HSTRING::from(tr("Settings")));
    let _ = AppendMenuW(menu, MF_STRING, sys::CMD_ABOUT as usize, &HSTRING::from(tr("About Hesewake Notch")));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, 1, &HSTRING::from(tr("Exit")));

    sys::track(hwnd, menu)
}

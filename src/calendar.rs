//! Calendar: upcoming meetings from the calendar's secret ICS link (Google "Secret address in iCal format", Outlook
//! "Publish calendar"). A countdown in the notch in the last 10 minutes, a toast with a Join button when a meeting
//! starts, and the Agenda slide. The link works like a password, so it is kept in Windows Credential Manager, not in a
//! file.

use super::*;
use std::sync::{Mutex, OnceLock};
use std::sync::mpsc::{RecvTimeoutError, Sender};
use windows::Win32::Security::Credentials::{CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC};

/// The countdown shows in the last 10 minutes before a meeting.
pub(crate) const LEAD_SECS: i64 = 10 * 60;
pub(crate) const WM_CALENDAR: u32 = WM_APP + 11;
const DAY: i64 = 86_400;
/// How often the feed is fetched again.
const REFRESH: Duration = Duration::from_secs(10 * 60);
/// Bigger feeds (years of history) are refused rather than parsed.
const MAX_BYTES: usize = 10_000_000;
/// "Remove" must be pressed again within this time.
const CONFIRM_SECS: f32 = 3.0;
/// Where the link is kept in Windows Credential Manager (Control Panel > Credential Manager > Windows Credentials).
const LINK_TARGET: &str = "HesewakeNotch/calendar-link";

/// Status of the calendar feed, shown in Settings and on the Agenda slide.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum CalStatus {
    /// No link set.
    Off,
    Loading,
    Ready,
    /// The last fetch failed (offline, wrong link, not a calendar).
    Failed,
}

/// The link as stored: trimmed, `webcal://` turned into `https://`. None if it is not an https/webcal link.
pub(crate) fn normalize_link(s: &str) -> Option<String> {
    let s = s.trim();
    let s = match s.get(..9) {
        Some(p) if p.eq_ignore_ascii_case("webcal://") => format!("https://{}", &s[9..]),
        _ => s.to_string(),
    };
    let rest = s.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let ok = host.contains('.') && !host.starts_with('.') && s.len() <= 2048 && !s.chars().any(|c| c.is_whitespace() || c.is_control());
    ok.then_some(s)
}

/// The PC's offset from UTC in seconds (WIB = 25200).
pub(crate) fn local_offset() -> i32 {
    (prayer::local_utc_offset() * 3600.0).round() as i32
}

// ---------------------------------------------------------------- the link, in Windows Credential Manager

/// Save `link` under `target` in Windows Credential Manager (a generic credential for this Windows user, kept on this
/// PC), or remove it. True on success; removing a link that isn't there also counts.
fn store_link(target: &str, link: Option<&str>) -> bool {
    let mut name: Vec<u16> = target.encode_utf16().chain([0]).collect();
    unsafe {
        match link {
            Some(l) => {
                let mut blob = l.as_bytes().to_vec();
                let mut user: Vec<u16> = "Hesewake Notch".encode_utf16().chain([0]).collect();
                let cred = CREDENTIALW {
                    Type: CRED_TYPE_GENERIC,
                    TargetName: PWSTR(name.as_mut_ptr()),
                    CredentialBlobSize: blob.len() as u32,
                    CredentialBlob: blob.as_mut_ptr(),
                    Persist: CRED_PERSIST_LOCAL_MACHINE,
                    UserName: PWSTR(user.as_mut_ptr()),
                    ..Default::default()
                };
                CredWriteW(&cred, 0).is_ok()
            }
            None => CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, 0).is_ok() || read_link(target).is_none(),
        }
    }
}

/// The link saved under `target` in Windows Credential Manager.
fn read_link(target: &str) -> Option<String> {
    let name: Vec<u16> = target.encode_utf16().chain([0]).collect();
    let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, 0, &mut cred).ok()?;
        let c = &*cred;
        let bytes = if c.CredentialBlob.is_null() { Vec::new() } else { std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec() };
        CredFree(cred as *const c_void);
        String::from_utf8(bytes).ok().filter(|s| !s.is_empty())
    }
}

fn save_link(link: Option<&str>) {
    if !store_link(LINK_TARGET, link) {
        perf::note("calendar: saving the link in Windows Credential Manager failed");
    }
}

/// The saved link, if any.
pub(crate) fn load_link() -> Option<String> {
    if cfg!(test) {
        return None;
    }
    read_link(LINK_TARGET)
}

// ---------------------------------------------------------------- fetch thread

static CMD: OnceLock<Mutex<Sender<Option<String>>>> = OnceLock::new();
/// Latest result for the UI thread: (link it came from, status, events).
static SLOT: Mutex<Option<(String, CalStatus, Vec<ics::Event>)>> = Mutex::new(None);

/// Start fetching `link` (if any) now and every 10 minutes.
pub(crate) fn start(hwnd: isize, link: Option<String>) {
    if cfg!(test) {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    if CMD.set(Mutex::new(tx)).is_err() {
        return;
    }
    std::thread::spawn(move || {
        let mut link = link;
        loop {
            if let Some(l) = &link {
                let now = notif::now_unix() as i64;
                let (status, events) = match weather::http_get(l, MAX_BYTES).filter(|b| b.contains("BEGIN:VCALENDAR")) {
                    Some(body) => (CalStatus::Ready, ics::events_between(&body, now - DAY, now + 2 * DAY, local_offset())),
                    None => {
                        perf::note("calendar: fetching the ICS link failed");
                        (CalStatus::Failed, Vec::new())
                    }
                };
                *SLOT.lock().unwrap_or_else(|e| e.into_inner()) = Some((l.clone(), status, events));
                unsafe {
                    let _ = PostMessageW(HWND(hwnd as *mut c_void), WM_CALENDAR, WPARAM(0), LPARAM(0));
                }
            }
            match rx.recv_timeout(REFRESH) {
                Ok(new) => link = new,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}

/// Save the new link (or remove it) and fetch right away.
fn set_link(link: Option<String>) {
    if cfg!(test) {
        return;
    }
    save_link(link.as_deref());
    if let Some(tx) = CMD.get() {
        let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(link);
    }
}

/// The fetch thread's latest result (taken once).
pub(crate) fn take() -> Option<(String, CalStatus, Vec<ics::Event>)> {
    SLOT.lock().unwrap_or_else(|e| e.into_inner()).take()
}

// ---------------------------------------------------------------- Agenda slide layout

/// Agenda slide layout: rows under the title, three visible at a time.
pub(crate) const ROW_Y0: f32 = 44.0;
pub(crate) const ROW_H: f32 = 30.0;
pub(crate) const ROW_P: f32 = 35.0;
pub(crate) const ROWS: usize = 3;
/// Rows leave room for the scrollbar on the right.
pub(crate) const ROW_W: f32 = CW - 10.0;
const JOIN_W: f32 = 52.0;
const JOIN_H: f32 = 22.0;

/// The Join button of visible row `vi`: x, y, w, h.
pub(crate) fn join_rect(vi: usize) -> (f32, f32, f32, f32) {
    let y = ROW_Y0 + vi as f32 * ROW_P;
    (LX + ROW_W - 6.0 - JOIN_W, y + (ROW_H - JOIN_H) / 2.0, JOIN_W, JOIN_H)
}

/// The countdown shown in the notch: whichever comes first.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Upcoming {
    /// Prayer index and minutes left.
    Prayer(usize, u16),
    /// Meeting title and minutes left.
    Meeting(String, i64),
    /// Running downloads: total bytes per second (downloads.rs). Shown when no prayer or meeting is near.
    Download(u64),
}

/// "09:30" for unix time `t` at `offset` seconds east of UTC.
pub(crate) fn hhmm(t: i64, offset: i32) -> String {
    let m = (t + offset as i64).rem_euclid(DAY) / 60;
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// 0 = Sunday.
fn weekday(day: i64) -> usize {
    (day + 4).rem_euclid(7) as usize
}

fn icon_calendar(g: &mut Gfx, cx: f32, cy: f32, s: f32, p: &Paint) {
    let (w, h) = (s * 1.05, s * 0.95);
    let (x, y) = (cx - w / 2.0, cy - h / 2.0 + s * 0.06);
    g.fill(&rrect(x, y, w, h, s * 0.18), p);
    g.fill(&rrect(x + s * 0.12, y + s * 0.3, w - s * 0.24, h - s * 0.42, s * 0.08), &paint(10, 12, 20, 0.85));
    for dx in [-0.25f32, 0.25] {
        g.fill(&rrect(cx + dx * s - s * 0.06, y - s * 0.14, s * 0.12, s * 0.3, s * 0.06), p);
    }
}

/// The clipboard's text for the Calendar row's Paste button.
pub(crate) fn clipboard_text(hwnd: isize) -> Option<String> {
    #[cfg(test)]
    {
        let _ = hwnd;
        TEST_CLIPBOARD.with(|c| c.borrow_mut().take())
    }
    #[cfg(not(test))]
    unsafe {
        note::clip_get(HWND(hwnd as *mut c_void))
    }
}

// Tests: what the clipboard holds when the Calendar row's button is pressed.
#[cfg(test)]
thread_local! {
    pub(crate) static TEST_CLIPBOARD: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// What a click on the Agenda slide hit.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum AgendaHit {
    /// The Join button of row `i` (index into `agenda()`).
    Join(usize),
}

impl App {
    /// The next meeting starting within `LEAD_SECS` (not all-day), with whole minutes left (rounded up).
    pub(crate) fn meeting_countdown(&self, now: i64) -> Option<(&ics::Event, i64)> {
        self.cal
            .iter()
            .filter(|e| !e.all_day && e.start > now && e.start - now <= LEAD_SECS)
            .min_by_key(|e| e.start)
            .map(|e| (e, (e.start - now + 59) / 60))
    }

    /// Called every minute: one toast per meeting when it starts. True if a toast was started.
    pub(crate) fn meeting_minute(&mut self, now: i64) -> bool {
        let due: Vec<ics::Event> = self
            .cal
            .iter()
            .filter(|e| !e.all_day && e.start <= now && now - e.start < 120)
            .filter(|e| !self.cal_fired.iter().any(|(s, t)| *s == e.start && *t == e.summary))
            .cloned()
            .collect();
        for e in &due {
            self.cal_fired.push((e.start, e.summary.clone()));
            self.push_toast(ToastKind::Meeting { title: e.summary.clone(), at: e.start, link: e.link.clone() });
        }
        let n = self.cal_fired.len();
        if n > 32 {
            self.cal_fired.drain(..n - 32);
        }
        !due.is_empty()
    }

    /// Today's events that haven't ended (all-day ones first), for the Agenda slide.
    pub(crate) fn agenda(&self, now: i64, offset: i32) -> Vec<&ics::Event> {
        let day = (now + offset as i64).div_euclid(DAY);
        let (d0, d1) = (day * DAY - offset as i64, (day + 1) * DAY - offset as i64);
        let mut v: Vec<&ics::Event> = self.cal.iter().filter(|e| e.end > now && e.start < d1 && e.end > d0).collect();
        v.sort_by_key(|e| (!e.all_day, e.start));
        v
    }

    pub(crate) fn agenda_hit(&self, x: f32, y: f32, now: i64, offset: i32) -> Option<AgendaHit> {
        let rows = self.agenda(now, offset);
        (0..ROWS).find_map(|vi| {
            let i = self.agenda_scroll + vi;
            let (bx, by, bw, bh) = join_rect(vi);
            (rows.get(i)?.link.is_some() && in_rect(x, y, bx, by, bw, bh)).then_some(AgendaHit::Join(i))
        })
    }

    /// The Calendar row's button: paste a link from the clipboard text, or remove the link (press twice).
    pub(crate) fn cal_button(&mut self, clipboard: Option<String>) {
        if self.cal_link.is_some() {
            if self.cal_confirm.is_some_and(|t| t.elapsed().as_secs_f32() < CONFIRM_SECS) {
                self.cal_link = None;
                self.cal_status = CalStatus::Off;
                self.cal.clear();
                self.cal_confirm = None;
                set_link(None);
            } else {
                self.cal_confirm = Some(Instant::now());
            }
            return;
        }
        match clipboard.as_deref().and_then(normalize_link) {
            Some(link) => {
                self.cal_link = Some(link.clone());
                self.cal_status = CalStatus::Loading;
                self.cal_paste_err = None;
                set_link(Some(link));
            }
            None => self.cal_paste_err = Some(Instant::now()),
        }
    }

    /// New data from the fetch thread. A failed refresh keeps the events already known.
    pub(crate) fn cal_update(&mut self, status: CalStatus, events: Vec<ics::Event>) {
        self.cal_status = status;
        if status != CalStatus::Failed {
            self.cal = events;
        }
        self.agenda_scroll = self.agenda_scroll.min(self.cal.len().saturating_sub(ROWS));
    }

    /// The countdown to show: prayer or meeting, whichever starts first.
    pub(crate) fn upcoming(&self) -> Option<Upcoming> {
        let prayer = self.prayer_countdown();
        let meeting = if self.thin() { None } else { self.meeting_countdown(notif::now_unix() as i64) };
        match (prayer, meeting) {
            (Some((_, pm)), Some((e, mm))) if mm < pm as i64 => Some(Upcoming::Meeting(e.summary.clone(), mm)),
            (Some((i, pm)), _) => Some(Upcoming::Prayer(i, pm)),
            (None, Some((e, mm))) => Some(Upcoming::Meeting(e.summary.clone(), mm)),
            (None, None) => self.dl_speed().filter(|_| !self.thin()).map(Upcoming::Download),
        }
    }

    pub(crate) fn countdown_on(&self) -> bool {
        self.upcoming().is_some()
    }

    /// The countdown label and its colour, fitted to `prayer::LABEL_W` ("Standup 8m", "Ashar 12m").
    pub(crate) fn countdown_text(&self, g: &mut Gfx) -> Option<(String, [u8; 3])> {
        match self.upcoming()? {
            Upcoming::Prayer(i, m) => Some((format!("{} {}m", prayer::short_name(i, self.cfg.ramadan), m), prayer::GOLD)),
            Upcoming::Meeting(title, m) => {
                let suffix = format!(" {m}m");
                let room = prayer::LABEL_W - g.width(&suffix, 12.0);
                let title = if g.width(&title, 12.0) > room { ellipsize(g, &title, 12.0, room) } else { title };
                Some((title + &suffix, g.acc.bright))
            }
            Upcoming::Download(bps) => Some((format!("\u{2193} {}", downloads::fmt_speed(bps)), g.acc.bright)),
        }
    }

    /// Sub-label of the Calendar row in Settings.
    pub(crate) fn cal_sub(&self) -> &'static str {
        if self.cal_link.is_none() {
            return if self.cal_paste_failed() { tr("No ICS link copied") } else { tr("Copy an ICS link first") };
        }
        if self.cal_confirming() {
            return tr("Press again to remove");
        }
        match self.cal_status {
            CalStatus::Loading => tr("Loading\u{2026}"),
            CalStatus::Ready => tr("Synced"),
            CalStatus::Failed => tr("Couldn't load it"),
            CalStatus::Off => tr("Copy an ICS link first"),
        }
    }

    /// The Calendar row's button: Paste, Remove, or Sure? (second press removes).
    pub(crate) fn cal_button_text(&self) -> &'static str {
        if self.cal_link.is_none() {
            tr("Paste")
        } else if self.cal_confirming() {
            tr("Sure?")
        } else {
            tr("Remove")
        }
    }

    pub(crate) fn agenda_scrollable(&self) -> bool {
        self.agenda(notif::now_unix() as i64, local_offset()).len() > ROWS
    }

    /// Wheel over the Agenda slide. True if it scrolled.
    pub(crate) fn agenda_wheel(&mut self, down: bool) -> bool {
        let max = self.agenda(notif::now_unix() as i64, local_offset()).len().saturating_sub(ROWS);
        let old = self.agenda_scroll;
        self.agenda_scroll = if down { (old + 1).min(max) } else { old.saturating_sub(1) };
        self.agenda_scroll != old
    }

    pub(crate) fn slide_agenda(&self, g: &mut Gfx, ox: f32, al: f32) {
        let (now, off) = (notif::now_unix() as i64, local_offset());
        let x = LX + ox;
        g.txtb("Agenda", x, 12.0, 15.0, [255, 255, 255], al, 120.0);
        let day = (now + off as i64).div_euclid(DAY);
        let (_, m, d) = ics::civil_from_days(day);
        let date = format!("{} {} {}", clock::day_short(weekday(day)), d, clock::month(m as usize - 1));
        let dw = g.width(&date, 13.0);
        g.txt(&date, RX + ox - dw, 14.0, 13.0, [235, 235, 235], al, 160.0);

        let rows = self.agenda(now, off);
        if rows.is_empty() {
            let (l1, l2): (&str, String) = if self.cal_link.is_none() {
                (tr("Link your calendar in Settings \u{203a} Calendar"), tr("Paste its secret ICS address there").to_string())
            } else if self.cal_status == CalStatus::Loading {
                (tr("Loading your calendar\u{2026}"), String::new())
            } else if self.cal_status == CalStatus::Failed && self.cal.is_empty() {
                (tr("Couldn't load your calendar"), tr("Check the link in Settings \u{203a} Calendar").to_string())
            } else {
                let next = self.cal.iter().filter(|e| e.start >= now).min_by_key(|e| e.start).map(|e| {
                    let eday = (e.start + off as i64).div_euclid(DAY);
                    let time = if e.all_day { tr("All day").to_string() } else { hhmm(e.start, off) };
                    fill(tr("Next: {} {} \u{b7} {}"), &[&clock::day_short(weekday(eday)), &time, &e.summary])
                });
                (tr("Nothing else today"), next.unwrap_or_default())
            };
            let cx = x + CW / 2.0;
            g.txt_c(l1, cx, 66.0, 13.0, TEXT_DIM, al, CW, false);
            if !l2.is_empty() {
                let l2 = if g.width(&l2, 11.0) > CW { ellipsize(g, &l2, 11.0, CW) } else { l2 };
                g.txt_c(&l2, cx, 88.0, 11.0, TEXT_FAINT, al, CW, false);
            }
            return;
        }
        const TIME_W: f32 = 60.0;
        let cur = self.cursor_canvas();
        for (vi, e) in rows.iter().skip(self.agenda_scroll).take(ROWS).enumerate() {
            let y = ROW_Y0 + vi as f32 * ROW_P;
            let running = !e.all_day && e.start <= now;
            let bg = if running { g.sacc(0.22 * al) } else { paint(255, 255, 255, 0.08 * al) };
            g.fill(&rrect(x, y, ROW_W, ROW_H, 10.0), &bg);
            let (time, tcol) = if e.all_day {
                (tr("All day").to_string(), TEXT_DIM)
            } else if running {
                (tr("Now").to_string(), g.acc.bright)
            } else {
                (hhmm(e.start, off), [255, 255, 255])
            };
            g.txtb(&time, x + 12.0, y + ty(ROW_H, 12.0), 12.0, tcol, al, TIME_W - 4.0);
            let join = e.link.is_some();
            let maxw = ROW_W - 12.0 - TIME_W - if join { JOIN_W + 16.0 } else { 12.0 };
            let title = if g.width(&e.summary, 13.0) > maxw { ellipsize(g, &e.summary, 13.0, maxw) } else { e.summary.clone() };
            g.txt(&title, x + 12.0 + TIME_W, y + ty(ROW_H, 13.0), 13.0, [242, 242, 242], al, maxw + 2.0);
            if join {
                let (bx, by, bw, bh) = join_rect(vi);
                let over = cur.is_some_and(|(cx, cy)| in_rect(cx, cy, bx, by, bw, bh));
                let a = g.acc.deep;
                g.pill(bx + ox, by, bw, bh, a, if over { 1.0 } else { 0.85 } * al, 1.0);
                g.txt_c(tr("Join"), bx + ox + bw / 2.0, by + ty(bh, 12.0), 12.0, [255, 255, 255], al, bw, true);
            }
        }
        let n = rows.len();
        if n > ROWS {
            let th = ROWS as f32 * ROW_P - (ROW_P - ROW_H);
            let thumb = (th * ROWS as f32 / n as f32).max(14.0);
            let ty0 = ROW_Y0 + (th - thumb) * self.agenda_scroll as f32 / (n - ROWS) as f32;
            g.fill(&rrect(RX - 4.0 + ox, ROW_Y0, 3.0, th, 1.5), &paint(255, 255, 255, 0.10 * al));
            g.fill(&rrect(RX - 4.0 + ox, ty0, 3.0, thumb, 1.5), &paint(255, 255, 255, 0.55 * al));
        }
    }

    /// Meeting toast: calendar icon, title, "Starts now · 09:30", and a Join button when there is a link.
    pub(crate) fn draw_meeting_toast(&self, g: &mut Gfx, (w, h): (f32, f32), al: f32, t: f32, (title, at, link): (&str, i64, bool)) {
        let (x0, cy) = (CX - w / 2.0, h / 2.0);
        let acc = g.acc.bright;
        let sc = 0.6 + 0.4 * toast::pop(t).clamp(0.0, 1.2);
        let (icx, r) = (x0 + 34.0, 18.0 * sc);
        g.circle(icx, cy, r, &paint(acc[0], acc[1], acc[2], 0.22 * al));
        icon_calendar(g, icx, cy, 15.0 * sc, &paint(acc[0], acc[1], acc[2], al));

        let k = toast::ss(0.12, 0.4, t);
        let tx = x0 + 66.0 + (1.0 - k) * 6.0;
        let right = if link { 70.0 + 16.0 + 10.0 } else { 16.0 };
        let maxw = x0 + w - right - tx;
        let title = if g.width_ex(title, 14.0, true) > maxw { ellipsize(g, title, 14.0, maxw) } else { title.to_string() };
        g.txtb(&title, tx, cy - 19.0, 14.0, [255, 255, 255], al * k, maxw + 2.0);
        let sub = fill(tr("Starts now \u{b7} {}"), &[&hhmm(at, local_offset())]);
        g.txt(&sub, tx, cy + 2.0, 12.0, TEXT_DIM, al * k, maxw);
        if link {
            let (bx, by, bw, bh) = toast::meeting_join_rect(w, h);
            let over = self.cursor_canvas().is_some_and(|(x, y)| in_rect(x, y, bx, by, bw, bh));
            g.pill(bx, by, bw, bh, g.acc.deep, if over { 1.0 } else { 0.85 } * al * k, 1.0);
            g.txt_c(tr("Join"), bx + bw / 2.0, by + ty(bh, 13.0), 13.0, [255, 255, 255], al * k, bw, true);
        }
    }

    /// The Calendar row is waiting for a second press to remove the link.
    /// The Calendar row is waiting for a second press to remove the link.
    pub(crate) fn cal_confirming(&self) -> bool {
        self.cal_confirm.is_some_and(|t| t.elapsed().as_secs_f32() < CONFIRM_SECS)
    }

    /// The Calendar row is saying the clipboard had no link.
    pub(crate) fn cal_paste_failed(&self) -> bool {
        self.cal_paste_err.is_some_and(|t| t.elapsed().as_secs_f32() < CONFIRM_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_cleaned_and_checked() {
        let g = "https://calendar.google.com/calendar/ical/me%40gmail.com/private-abc123/basic.ics";
        assert_eq!(normalize_link(&format!("  {g}\r\n")).as_deref(), Some(g));
        assert_eq!(normalize_link("webcal://outlook.office365.com/owa/calendar/x/reachcalendar.ics").as_deref(), Some("https://outlook.office365.com/owa/calendar/x/reachcalendar.ics"));
        for bad in ["", "hello", "http://example.com/a.ics", "https://", "https://exa mple.com/a.ics", "file:///C:/a.ics", "ftp://x/y.ics"] {
            assert_eq!(normalize_link(bad), None, "{bad:?}");
        }
        assert_eq!(normalize_link(&format!("https://x.com/{}", "a".repeat(3000))), None, "absurdly long");
    }

    /// End to end with a real Google Calendar feed (the public Indonesian holidays calendar): WinINet fetches it and
    /// the parser finds this year's holidays. Runs only on CI (`CI` is set by GitHub Actions) because it needs the
    /// network.
    #[test]
    fn real_google_feed_is_fetched_and_parsed() {
        if std::env::var_os("CI").is_none() {
            eprintln!("skipped: needs the network, runs on CI only");
            return;
        }
        let url = normalize_link("webcal://calendar.google.com/calendar/ical/en.indonesian%23holiday%40group.v.calendar.google.com/public/basic.ics").unwrap();
        let body = weather::http_get(&url, MAX_BYTES).expect("feed downloaded");
        assert!(body.contains("BEGIN:VCALENDAR"), "it is a calendar");
        let now = notif::now_unix() as i64;
        let year = ics::events_between(&body, now - 183 * DAY, now + 183 * DAY, 7 * 3600);
        assert!(year.len() >= 5, "holidays within a year: {}", year.len());
        assert!(year.iter().all(|e| e.all_day && e.end - e.start >= DAY && !e.summary.is_empty()), "all-day events with names");
        eprintln!("{} holidays, e.g. {:?}", year.len(), year.first().map(|e| &e.summary));
    }

    /// The link lives in Windows Credential Manager (decided in #14), not in a file. Uses its own test entry and
    /// removes it again.
    #[test]
    fn link_is_kept_in_credential_manager() {
        let target = "HesewakeNotch/calendar-link-test";
        let link = "https://calendar.google.com/calendar/ical/x/private-secret/basic.ics";
        assert!(store_link(target, Some(link)));
        assert_eq!(read_link(target).as_deref(), Some(link));
        assert!(store_link(target, Some("https://x.com/other.ics")), "replacing works");
        assert_eq!(read_link(target).as_deref(), Some("https://x.com/other.ics"));
        assert!(store_link(target, None));
        assert_eq!(read_link(target), None, "removed");
        assert!(store_link(target, None), "removing twice is fine");
    }
}

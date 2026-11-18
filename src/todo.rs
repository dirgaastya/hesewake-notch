//! Fitur To-do: daftar tugas, editor teks, slide To-do.

use super::*;

const LP: f32 = LX + 204.0;
pub(crate) const ROW_H: f32 = 30.0;
pub(crate) const ROW_P: f32 = 35.0;

pub(crate) fn rows_geom(editing: bool) -> (f32, f32, f32, usize) {
    if editing {
        (LX, CW - 10.0, 78.0, 2)
    } else {
        (LP, RX - 10.0 - LP, 44.0, 3)
    }
}

/// One task. `due` is when it is due (unix seconds), set by typing "tomorrow 14:00" and the like (duedate.rs).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Todo {
    pub(crate) done: bool,
    pub(crate) text: String,
    pub(crate) due: Option<i64>,
}

impl Todo {
    pub(crate) fn new(done: bool, text: &str) -> Todo {
        Todo { done, text: text.to_string(), due: None }
    }
}

/// In todo.txt a task is `1|text` or `0|text`; a due time follows the text as a tab and `@seconds`. An older version
/// that reads such a line shows the extra characters as part of the text and loses nothing else.
const DUE_MARK: &str = "\t@";

pub(crate) fn parse_todos(s: &str) -> Vec<Todo> {
    s.lines()
        .filter_map(|l| {
            let (d, rest) = l.split_once('|')?;
            let (text, due) = match rest.rsplit_once(DUE_MARK) {
                Some((t, n)) => match n.parse::<i64>() {
                    Ok(v) => (t, Some(v)),
                    Err(_) => (rest, None),
                },
                None => (rest, None),
            };
            Some(Todo { done: d == "1", text: text.to_string(), due })
        })
        .collect()
}

pub(crate) fn format_todos(t: &[Todo]) -> String {
    t.iter()
        .map(|x| format!("{}|{}{}\n", if x.done { 1 } else { 0 }, x.text, x.due.map(|d| format!("{DUE_MARK}{d}")).unwrap_or_default()))
        .collect()
}

pub(crate) fn load_todos() -> Vec<Todo> {
    match std::fs::read_to_string(data_path("todo.txt")) {
        Ok(s) => parse_todos(&s),
        Err(_) => {
            let d = vec![Todo::new(false, tr("Tap Add Task to create a task"))];
            save_todos(&d);
            d
        }
    }
}

pub(crate) fn save_todos(t: &[Todo]) {
    write_data("todo.txt", format_todos(t));
}

/// How long after its due time a task still raises a toast (a PC that was asleep at the time catches up).
const REMIND_WINDOW: i64 = 2 * 3600;

impl App {
    /// Open tasks first, the soonest due first (tasks without a due time after those, in the order they were added), then
    /// the done ones.
    pub(crate) fn todo_order(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.todos.len()).filter(|&i| !self.todos[i].done).collect();
        v.sort_by_key(|&i| (self.todos[i].due.is_none(), self.todos[i].due.unwrap_or(0)));
        v.extend((0..self.todos.len()).filter(|&i| self.todos[i].done));
        v
    }

    /// Called every minute: one toast when a task comes due (a single summary toast when several come due together).
    /// True if a toast was started.
    pub(crate) fn todo_minute(&mut self, now: i64) -> bool {
        let mut due: Vec<(i64, String)> = self
            .todos
            .iter()
            .filter(|t| !t.done)
            .filter_map(|t| t.due.map(|d| (d, t.text.clone())))
            .filter(|(d, _)| *d <= now && now - *d < REMIND_WINDOW)
            .filter(|k| !self.todo_fired.contains(k))
            .collect();
        if due.is_empty() {
            return false;
        }
        due.sort();
        let (first_at, first_text) = due[0].clone();
        let kind = ToastKind::Todo { text: first_text, due: first_at, more: due.len() as u32 - 1 };
        // held by a quiet time: not marked as given, so it comes when the quiet time is over (within the window)
        if self.quiet_blocks(&kind) {
            return false;
        }
        self.todo_fired.extend(due.iter().cloned());
        let n = self.todo_fired.len();
        if n > 64 {
            self.todo_fired.drain(..n - 64);
        }
        self.push_toast(kind);
        true
    }

    /// Tombol dan baris di slide To-do yang bisa diklik (kursor tangan).
    pub(crate) fn todo_hand(&self, x: f32, y: f32) -> bool {
        if self.editing {
            return false;
        }
        if in_rect(x, y, LX, 116.0, 92.0, 28.0) || in_rect(x, y, LX + 98.0, 116.0, 92.0, 28.0) {
            return true;
        }
        let (rx, rw, y0, max) = rows_geom(false);
        let shown = self.todos.len().saturating_sub(self.todo_scroll).min(max);
        (0..shown).any(|vi| in_rect(x, y, rx, y0 + vi as f32 * ROW_P, rw, ROW_H))
    }

    pub(crate) fn todo_animating(&self) -> bool {
        self.expanded() && self.is_slide(SlideId::Todo) && (self.todo_pop.is_some_and(|p| p.1.elapsed().as_secs_f32() < 0.75) || self.todo_edit_end.is_some_and(|t| t.elapsed().as_secs_f32() < 0.25) || self.todo_all.is_some_and(|t| t.elapsed().as_secs_f32() < 0.95) || (self.editing && self.edit_start.elapsed().as_secs_f32() < 0.25) || self.todo_cnt.get().2.is_some_and(|t| t.elapsed().as_secs_f32() < 0.35) || (self.edit_mode && !self.todos.is_empty()) || self.todo_ring.get() >= 0.0 && {
            let tot = self.todos.len();
            let f = if tot > 0 { self.todos.iter().filter(|t| t.done).count() as f32 / tot as f32 } else { 0.0 };
            (self.todo_ring.get() - f).abs() > 0.002
        })
    }

    pub(crate) fn fix_scroll(&mut self) {
        let max = self.todos.len().saturating_sub(3);
        self.todo_scroll = self.todo_scroll.min(max);
    }

    pub(crate) unsafe fn begin_edit(&mut self, idx: Option<usize>) {
        self.editing = true;
        self.edit_idx = idx;
        // a task with a due time shows it after the text, so editing keeps it (and the phrase can be changed or removed)
        self.input = idx
            .map(|i| match self.todos[i].due {
                Some(due) => format!("{} {}", self.todos[i].text, duedate::format_due(due, calendar::local_offset())),
                None => self.todos[i].text.clone(),
            })
            .unwrap_or_default();
        self.caret_on = true;
        self.edit_start = Instant::now();
        self.prev_fg = GetForegroundWindow().0 as isize;
        let hwnd = HWND(self.hwnd as *mut c_void);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);
    }

    pub(crate) unsafe fn finish_edit(&mut self, commit: bool, restore: bool) {
        if self.note_mode {
            // The note is edited in place, so Esc / click away / focus loss all keep the text.
            save_note(&self.note);
            self.nv.saved = Some(Instant::now());
            self.note_mode = false;
            self.editing = false;
            if restore && self.prev_fg != 0 {
                let _ = SetForegroundWindow(HWND(self.prev_fg as *mut c_void));
            }
            return;
        }
        if commit {
            let (title, due) = duedate::split_due(&self.input, notif::now_unix() as i64, calendar::local_offset());
            // the text is only rewritten when a due phrase was cut off it
            let t = if due.is_some() { title } else { self.input.trim().to_string() };
            if !t.is_empty() {
                match self.edit_idx {
                    Some(i) if i < self.todos.len() => {
                        self.todos[i].text = t;
                        self.todos[i].due = due;
                    }
                    _ => self.todos.push(Todo { done: false, text: t, due }),
                }
                save_todos(&self.todos);
            }
        }
        self.editing = false;
        self.todo_edit_end = Some(Instant::now());
        self.edit_idx = None;
        self.input.clear();
        if restore && self.prev_fg != 0 {
            let _ = SetForegroundWindow(HWND(self.prev_fg as *mut c_void));
        }
    }

    fn todo_row(&self, g: &mut Gfx, x: f32, y: f32, w: f32, ri: usize, al: f32) {
        let (dn, s) = (self.todos[ri].done, &self.todos[ri].text);
        if dn {
            g.fill(&rrect(x, y, w, ROW_H, 10.0), &g.sacc(0.20 * al));
        } else {
            let hov = self.cursor_canvas().is_some_and(|(hx, hy)| in_rect(hx, hy, x, y, w, ROW_H));
            g.fill(&rrect(x, y, w, ROW_H, 10.0), &paint(255, 255, 255, if hov { 0.13 } else { 0.08 } * al));
        }
        let (cx, cy) = (x + 17.0, y + ROW_H / 2.0);
        // Baru dicentang: lingkaran memantul (pop), tanda centang tergambar bertahap, coretan memanjang.
        let pop = self.todo_pop.filter(|p| p.0 == ri).map(|p| p.1.elapsed().as_secs_f32()).filter(|&e| e < 0.7);
        if dn {
            if let Some(e) = pop {
                // konfeti kecil dari kotak centang: posisi dihitung langsung dari waktu (lintasan parabola)
                let cols = [g.acc.main, g.acc.bright, g.acc.pale, [255, 255, 255]];
                for i in 0..9 {
                    let ang = i as f32 * std::f32::consts::TAU / 9.0 + ri as f32;
                    let sp = 45.0 + ((i * 37) % 55) as f32;
                    let (px, py) = (cx + ang.cos() * sp * e, cy + ang.sin() * sp * e - 40.0 * e + 0.5 * 420.0 * e * e);
                    let a = (1.0 - e / 0.7).clamp(0.0, 1.0) * al;
                    let c = cols[i % 4];
                    g.circle(px, py, 1.6, &paint(c[0], c[1], c[2], a));
                }
            }
            let r = pop.map_or(8.0, |e| 8.0 * (1.0 - (-10.0 * e).exp() * (20.0 * e).cos()).max(0.0));
            g.circle(cx, cy, r, &g.sacc(al));
            let wh = paint(255, 255, 255, al);
            let pk = pop.map_or(1.0, |e| ((e - 0.08) / 0.2).clamp(0.0, 1.0));
            let len = pk * (3.67 + 6.1);
            if pk > 0.0 && r > 2.4 {
                let (p0, p1, p2) = ((cx - 3.6, cy + 0.4), (cx - 1.0, cy + 3.0), (cx + 3.8, cy - 2.8));
                if len <= 3.67 {
                    let k = len / 3.67;
                    g.line(p0.0, p0.1, p0.0 + (p1.0 - p0.0) * k, p0.1 + (p1.1 - p0.1) * k, 1.9, &wh);
                } else {
                    g.line(p0.0, p0.1, p1.0, p1.1, 1.9, &wh);
                    let k = (len - 3.67) / 6.1;
                    g.line(p1.0, p1.1, p1.0 + (p2.0 - p1.0) * k, p1.1 + (p2.1 - p1.1) * k, 1.9, &wh);
                }
            }
        } else {
            g.circle(cx, cy, 8.0, &paint(255, 255, 255, 0.32 * al));
            g.circle(cx, cy, 6.7, &paint(22, 22, 24, al));
        }
        let col = if dn { [172, 174, 180] } else { [242, 242, 242] };
        let tx = x + 33.0;
        let right = x + w - 8.0 - if self.edit_mode { 24.0 } else { 0.0 };
        // the due badge sits at the right end of an open task
        let badge = self.todos[ri].due.filter(|_| !dn).map(|due| self.due_badge(g, due));
        let badge_w = badge.as_ref().map_or(0.0, |b| g.width(&b.0, 10.5) + 12.0);
        let maxw = w - 33.0 - 10.0 - if self.edit_mode { 24.0 } else { 0.0 } - if badge_w > 0.0 { badge_w + 4.0 } else { 0.0 };
        if let Some((label, c)) = &badge {
            g.fill(&rrect(right - badge_w, y + (ROW_H - 17.0) / 2.0, badge_w, 17.0, 8.5), &paint(c[0], c[1], c[2], 0.20 * al));
            g.txt(label, right - badge_w + 6.0, y + (ROW_H - 17.0) / 2.0 + ty(17.0, 10.5), 10.5, *c, al, badge_w);
        }
        // with a badge the title has little room: cut it with "…" so it is clear that it goes on
        let shown = if badge_w > 0.0 && g.width(s, 13.0) > maxw { gfx::ellipsize(g, s, 13.0, maxw) } else { s.to_string() };
        g.txt(&shown, tx, y + ty(ROW_H, 13.0), 13.0, col, al, maxw);
        if dn {
            let k = pop.map_or(1.0, |e| (e / 0.3).clamp(0.0, 1.0));
            let tw = g.width(s, 13.0).min(maxw) * k;
            g.rect(tx, cy, tw, 1.3, &paint(175, 175, 175, 0.9 * al));
        }
        if self.edit_mode {
            // × bergoyang pelan di mode edit
            let (bx, by) = (x + w - 17.0, cy + (self.t * 14.0 + ri as f32).sin() * 0.9);
            g.circle(bx, by, 8.0, &g.sacc(0.9 * al));
            let wh = paint(255, 255, 255, al);
            g.line(bx - 2.8, by - 2.8, bx + 2.8, by + 2.8, 1.6, &wh);
            g.line(bx - 2.8, by + 2.8, bx + 2.8, by - 2.8, 1.6, &wh);
        }
    }

    /// The text and colour of the badge for a task due at `due`.
    fn due_badge(&self, g: &Gfx, due: i64) -> (String, [u8; 3]) {
        let (now, off) = (notif::now_unix() as i64, calendar::local_offset());
        match duedate::when(due, now, off) {
            duedate::When::Overdue => (tr("Late").to_string(), [235, 87, 87]),
            duedate::When::Today(m) => (format!("{:02}:{:02}", m / 60, m % 60), g.acc.bright),
            duedate::When::Tomorrow => (tr("Tomorrow").to_string(), [200, 204, 214]),
            duedate::When::Weekday(i) => (clock::day_short(i).to_string(), [200, 204, 214]),
            duedate::When::Date(d, m) => (format!("{} {}", d, clock::month(m)), [200, 204, 214]),
        }
    }

    fn pill(&self, g: &mut Gfx, x: f32, y: f32, label: &str, edit_icon: bool, active: bool, al: f32) {
        let hov = self.cursor_canvas().is_some_and(|(cx, cy)| in_rect(cx, cy, x, y, 92.0, 28.0));
        let bg = if active { g.sacc(0.95 * al) } else { paint(255, 255, 255, if hov { 0.19 } else { 0.11 } * al) };
        g.fill(&rrect(x, y, 92.0, 28.0, 14.0), &bg);
        let (ix, iy) = (x + 16.0, y + 14.0);
        let wh = paint(255, 255, 255, al);
        if edit_icon {
            g.line(ix - 4.0, iy + 4.5, ix + 4.0, iy - 4.0, 2.4, &wh);
            g.poly(&[(ix - 5.5, iy + 6.0), (ix - 5.0, iy + 2.5), (ix - 2.5, iy + 5.0)], &wh);
        } else {
            g.circle(ix, iy, 7.0, &wh);
            let bk = paint(0, 0, 0, al);
            g.rect(ix - 3.5, iy - 0.8, 7.0, 1.6, &bk);
            g.rect(ix - 0.8, iy - 3.5, 1.6, 7.0, &bk);
        }
        g.txt(label, x + 30.0, y + ty(28.0, 12.0), 12.0, [255, 255, 255], al, 60.0);
    }

    pub(crate) fn slide_todo(&self, g: &mut Gfx, ox: f32, al: f32) {
        let x = LX + ox;
        // two-colour title; "|" marks where the accent part starts
        let (t1, t2) = tr("Todo|List").split_once('|').unwrap_or(("", ""));
        g.txtb(t1, x, 12.0, 15.0, [255, 255, 255], al, 80.0);
        let tw = g.width_ex(t1, 15.0, true);
        g.txtb(t2, x + tw, 12.0, 15.0, g.acc.main, al, 80.0);
        let st = now_tm();
        let time = format!("{:02}:{:02}", st.wHour, st.wMinute);
        let date = format!(
            "{} {} {} {}",
            clock::day_short(st.wDayOfWeek as usize),
            st.wDay,
            clock::month((st.wMonth as usize).max(1) - 1),
            st.wYear
        );
        let timew = g.width_ex(&time, 13.0, true);
        let datew = g.width(&date, 13.0);
        let tx = RX + ox - timew;
        let dx = tx - 12.0 - datew;
        g.txt(&date, dx, 14.0, 13.0, [235, 235, 235], al, 200.0);
        g.txtb(&time, tx, 14.0, 13.0, [255, 255, 255], al, 80.0);
        let done = self.todos.iter().filter(|t| t.done).count();
        let total = self.todos.len();
        let frac = if total > 0 { done as f32 / total as f32 } else { 0.0 };
        let all_done = total > 0 && done == total;
        let ring_fg = if all_done { paint(60, 200, 110, al) } else { g.sacc(al) };
        let shown = if self.todo_ring.get() < 0.0 { frac } else { self.todo_ring.get() + (frac - self.todo_ring.get()) * 0.2 };
        self.todo_ring.set(if (shown - frac).abs() < 0.002 { frac } else { shown });
        g.ring(dx - 16.0, 23.0, 8.0, shown, 3.0, &paint(255, 255, 255, 0.16 * al), &ring_fg);

        let (rx, rw, y0, max) = rows_geom(self.editing);
        let ek = (self.edit_start.elapsed().as_secs_f32() / 0.2).clamp(0.0, 1.0);
        let ek = ek * ek * (3.0 - 2.0 * ek);
        if self.editing {
            // bilah input turun dari atas sambil memudar (0,2 dtk)
            let (eal, eo) = (al * ek, (1.0 - ek) * -8.0);
            g.fill(&rrect(LX - 1.0 + ox, 39.0 + eo, CW + 2.0, 32.0, 11.0), &g.sacc(0.55 * eal));
            g.fill(&rrect(LX + ox, 40.0 + eo, CW, 30.0, 10.0), &paint(26, 26, 29, eal));
            let shown: &str = if self.input.is_empty() { tr("Type a task. Add \"fri 14:00\" for a reminder") } else { &self.input };
            let col = if self.input.is_empty() { TEXT_FAINT } else { [255, 255, 255] };
            g.txt(shown, LX + 12.0 + ox, 40.0 + eo + ty(30.0, 13.0), 13.0, col, eal, CW - 30.0);
            if self.caret_on {
                let cw = if self.input.is_empty() { 0.0 } else { g.width(&self.input, 13.0).min(CW - 30.0) };
                g.rect(LX + 12.0 + ox + cw + 1.0, 46.0 + eo, 1.5, 18.0, &paint(255, 255, 255, eal));
            }
        }
        // panel kiri (angka, keterangan, tombol): memudar dan bergeser ke kiri saat input dibuka, kembali saat selesai
        let pa = if self.editing {
            1.0 - ek
        } else {
            self.todo_edit_end.map_or(1.0, |t| {
                let k = (t.elapsed().as_secs_f32() / 0.2).clamp(0.0, 1.0);
                k * k * (3.0 - 2.0 * k)
            })
        };
        if pa > 0.02 {
            let (px, pal) = (x - (1.0 - pa) * 16.0, al * pa);
            // angka selesai berguling ke atas saat berubah (pemakaian pertama hanya menyamakan nilai)
            let (mut prev, mut cur_n, mut ct) = self.todo_cnt.get();
            if prev == usize::MAX {
                (prev, cur_n, ct) = (done, done, None);
                self.todo_cnt.set((prev, cur_n, ct));
            } else if cur_n != done {
                (prev, cur_n, ct) = (cur_n, done, Some(Instant::now()));
                self.todo_cnt.set((prev, cur_n, ct));
            }
            let roll = ct.map_or(1.0, |t| (t.elapsed().as_secs_f32() / 0.3).clamp(0.0, 1.0));
            if roll < 1.0 {
                let e = 1.0 - (1.0 - roll).powi(3);
                g.txtb(&format!("{}/{}", prev, total), px, 36.0 - 18.0 * e, 42.0, [255, 255, 255], pal * (1.0 - e), 190.0);
                g.txtb(&format!("{}/{}", done, total), px, 36.0 + 18.0 * (1.0 - e), 42.0, [255, 255, 255], pal * e, 190.0);
            } else {
                g.txtb(&format!("{}/{}", done, total), px, 36.0, 42.0, [255, 255, 255], pal, 190.0);
            }
            if all_done {
                g.txtb(tr("All done, nice work!"), px, 92.0, 12.0, [60, 200, 110], pal, 190.0);
            } else {
                g.txt(tr("Tasks done today"), px, 92.0, 12.0, TEXT_DIM, pal, 190.0);
            }
            self.pill(g, px, 116.0, tr("Add Task"), false, false, pal);
            self.pill(g, px + 98.0, 116.0, if self.edit_mode { tr("Done") } else { tr("Edit Task") }, true, self.edit_mode, pal);
            if self.todos.is_empty() && !self.editing {
                crate::shelf::dashed(g, &rrect(rx + ox, 40.0, rw, 56.0, 12.0), &paint(255, 255, 255, 0.22 * pal), 1.2, 6.0, 0.0);
                g.txt_c(tr("No tasks yet"), rx + ox + rw / 2.0, 56.0, 13.0, TEXT_FAINT, pal, rw, false);
                g.txt_c(tr("Add Task"), rx + ox + rw / 2.0, 74.0, 12.0, g.acc.bright, pal, rw, true);
            }
        }
        let order = self.todo_order();
        for (vi, &ri) in order.iter().skip(self.todo_scroll).take(max).enumerate() {
            self.todo_row(g, rx + ox, y0 + vi as f32 * ROW_P, rw, ri, al);
        }
        // semua tugas selesai: konfeti besar dari angka hitungan (posisi dihitung dari waktu, tanpa state tambahan)
        if let Some(e) = self.todo_all.map(|t| t.elapsed().as_secs_f32()).filter(|&e| e < 0.9) {
            let cols = [g.acc.main, g.acc.bright, g.acc.pale, [60, 200, 110], [255, 255, 255]];
            for i in 0..26 {
                let ang = i as f32 * std::f32::consts::TAU / 26.0 + 0.3;
                let sp = 70.0 + ((i * 53) % 110) as f32;
                let (px, py) = (x + 30.0 + ang.cos() * sp * e, 60.0 + ang.sin() * sp * e - 50.0 * e + 0.5 * 420.0 * e * e);
                let c = cols[i % 5];
                g.circle(px, py, 1.9, &paint(c[0], c[1], c[2], (1.0 - e / 0.9) * al));
            }
        }
        let n = order.len();
        if n > max {
            let th = max as f32 * ROW_P - (ROW_P - ROW_H);
            let thumb = (th * max as f32 / n as f32).max(14.0);
            let ty0 = y0 + (th - thumb) * self.todo_scroll as f32 / (n - max) as f32;
            g.fill(&rrect(RX - 4.0 + ox, y0, 3.0, th, 1.5), &paint(255, 255, 255, 0.10 * al));
            g.fill(&rrect(RX - 4.0 + ox, ty0, 3.0, thumb, 1.5), &paint(255, 255, 255, 0.55 * al));
        }
    }
}

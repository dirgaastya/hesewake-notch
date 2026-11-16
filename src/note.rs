//! Fitur Scrap: notepad mini. Scrollable, kursor + seleksi, maks 3.333 karakter, slide Scrap.

use super::*;

use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::Foundation::GlobalFree;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

const NOTE_NAME: &str = "Scrap";

/// Isi baris di mode baca Scrap.
enum ReadItem {
    Word(String),
    Chip { label: String, domain: String, k: f32, loading: bool, url: String },
}

pub(crate) const NOTE_Y: f32 = 38.0;
pub(crate) const NOTE_H: f32 = 100.0;
const NOTE_SIZE: f32 = 13.0;
const NOTE_LINE: f32 = 17.0;
const NOTE_LINES: usize = 5; // visible lines; the notch never grows, the text scrolls instead
const NOTE_TOP: f32 = 12.0; // first line offset inside the card
const NOTE_W: f32 = CW - 28.0;
pub(crate) const NOTE_MAX_CHARS: usize = 3333;

const CF_UNICODETEXT: u32 = 13;
const WHEEL_STEP: i32 = 40; // wheel delta per line (one 120 notch = 3 lines, trackpads accumulate)

// Virtual-key codes handled in WM_KEYDOWN.
const K_PGUP: u32 = 0x21;
const K_PGDN: u32 = 0x22;
const K_END: u32 = 0x23;
const K_HOME: u32 = 0x24;
const K_LEFT: u32 = 0x25;
const K_UP: u32 = 0x26;
const K_RIGHT: u32 = 0x27;
const K_DOWN: u32 = 0x28;
const K_DELETE: u32 = 0x2E;

/// Caret / selection / scroll state of the note editor (char indices, not bytes).
#[derive(Default)]
pub(crate) struct NoteView {
    pub(crate) cur: usize,
    pub(crate) anchor: usize,
    pub(crate) scroll: usize,
    pub(crate) sel_drag: bool,
    wheel: i32,
    /// Teks yang baru dimasukkan (awal, akhir, waktu): memudar masuk selama 0,14 dtk.
    pub(crate) fresh: Option<(usize, usize, Instant)>,
    /// Saat teks panjang ditempel: kartu berkedip aksen sekali.
    pub(crate) flash: Option<Instant>,
    /// Saat catatan baru disimpan (Esc / klik di luar): titik hijau "Saved" memudar.
    pub(crate) saved: Option<Instant>,
    /// Mulai mengedit: bingkai aksen memudar masuk.
    pub(crate) focus: Option<Instant>,
    /// Posisi caret yang ditampilkan (meluncur halus ke posisi sebenarnya).
    glide: std::cell::Cell<Option<(f32, f32)>>,
    /// Tombol Salin / Hapus di header (muncul saat kursor di kartu): alpha yang ditampilkan, kapan disalin, kapan Hapus diarmkan.
    ha: std::cell::Cell<f32>,
    pub(crate) copied: Option<Instant>,
    pub(crate) clear_arm: Option<Instant>,
}

/// Geometri tombol header Scrap: (pusat x tombol Salin, area tombol Hapus: x, lebar).
pub(crate) fn note_btn_geo(armed: bool) -> (f32, f32, f32) {
    if armed { (RX - 72.0, RX - 58.0, 58.0) } else { (RX - 37.0, RX - 22.0, 22.0) }
}

impl App {
    pub(crate) fn note_armed(&self) -> bool {
        self.nv.clear_arm.is_some_and(|t| t.elapsed().as_secs_f32() < 2.5)
    }

    /// 1 = Salin, 2 = Hapus (atau konfirmasi), 0 = bukan tombol. Hanya saat membaca dan catatan tidak kosong.
    pub(crate) fn note_btn_at(&self, x: f32, y: f32) -> u8 {
        if self.note_mode || self.note.is_empty() || !(7.0..=31.0).contains(&y) {
            return 0;
        }
        let (cx, bx, bw) = note_btn_geo(self.note_armed());
        if (x - cx).abs() <= 11.0 {
            1
        } else if x >= bx && x <= bx + bw {
            2
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------- persistence
pub(crate) fn load_note() -> String {
    let s = std::fs::read_to_string(data_path("notepad.txt")).unwrap_or_default();
    s.replace("\r\n", "\n").chars().take(NOTE_MAX_CHARS).collect()
}

pub(crate) fn save_note(s: &str) {
    write_data("notepad.txt", s.to_string());
}

// ---------------------------------------------------------------- layout helpers
fn char_w(g: &mut Gfx, ch: char) -> f32 {
    let mut b = [0u8; 4];
    g.width(ch.encode_utf8(&mut b), NOTE_SIZE)
}

/// Tokens "http(s)://..." (up to whitespace) as char ranges; with link titles on they are shown as chips.
fn url_spans(chars: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let a = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let tok: String = chars[a..i].iter().collect();
        if (tok.starts_with("http://") || tok.starts_with("https://")) && tok.len() > 8 {
            out.push((a, i));
        }
    }
    out
}

/// The address of a chip, without trailing punctuation (same rule as the read view).
fn url_text(chars: &[char], a: usize, b: usize) -> String {
    let w: String = chars[a..b].iter().collect();
    w.trim_end_matches(|c: char| ".,;:!?)]}'\"".contains(c)).to_string()
}

/// Width of every char. A chip counts as one unit: the first char has the chip's width, the rest none, so wrapping,
/// the caret and hit-testing never land inside a link.
fn widths(g: &mut Gfx, chars: &[char], chips: bool) -> Vec<f32> {
    let mut ws: Vec<f32> = chars.iter().map(|&c| char_w(g, c)).collect();
    if chips {
        for (a, b) in url_spans(chars) {
            let (_, w) = chip_item(g, &url_text(chars, a, b));
            ws[a] = w;
            for x in ws.iter_mut().take(b).skip(a + 1) {
                *x = 0.0;
            }
        }
    }
    ws
}

fn prefix(ws: &[f32]) -> Vec<f32> {
    let mut p = Vec::with_capacity(ws.len() + 1);
    let mut acc = 0.0f32;
    p.push(0.0);
    for w in ws {
        acc += w;
        p.push(acc);
    }
    p
}

/// A caret index that falls inside a link moves to its end (`fwd`) or its start.
fn snap(spans: &[(usize, usize)], i: usize, fwd: bool) -> usize {
    match spans.iter().find(|&&(a, b)| i > a && i < b) {
        Some(&(a, b)) => {
            if fwd {
                b
            } else {
                a
            }
        }
        None => i,
    }
}

/// Chip for a link: what it shows, and its width. Shared by the read view and the editor.
fn chip_item(g: &mut Gfx, url: &str) -> (ReadItem, f32) {
    let loading = matches!(linktitle::state(url), Some(linktitle::LinkState::Loading));
    let domain = linktitle::domain(url);
    let label = match linktitle::state(url) {
        Some(linktitle::LinkState::Title(t)) => t,
        _ => domain.clone(),
    };
    let label = if g.width_ex(&label, 11.0, true) > 150.0 { ellipsize(g, &label, 11.0, 150.0) } else { label };
    // judul baru datang: lebar chip melebar bertahap dan label berganti dengan fade (0,3 dtk)
    let titled = matches!(linktitle::state(url), Some(linktitle::LinkState::Title(_)));
    let k = if titled {
        let e = (linktitle::age(url).unwrap_or(1.0) / 0.3).clamp(0.0, 1.0);
        e * e * (3.0 - 2.0 * e)
    } else {
        0.0
    };
    let wd = 6.0 + 10.0 + 4.0 + g.width_ex(&domain, 11.0, true) + 7.0;
    let wt = 6.0 + 10.0 + 4.0 + g.width_ex(&label, 11.0, true) + 7.0;
    let w = if titled { wd + (wt - wd) * k } else { wd };
    (ReadItem::Chip { label, domain, k, loading, url: url.to_string() }, w)
}

/// Wrapped lines as char ranges `[start, end)`. Unlike `wrap_lines` no character is dropped
/// (spaces stay at the end of the line they wrap on, '\n' sits between two ranges), so
/// caret / selection / hit-testing map exactly onto the text.
fn layout(g: &mut Gfx, s: &str, chips: bool) -> Vec<(usize, usize)> {
    let chars: Vec<char> = s.chars().collect();
    let allw = widths(g, &chars, chips);
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut base = 0usize;
    for para in chars.split(|&c| c == '\n') {
        let n = para.len();
        let ws: &[f32] = &allw[base..base + n];
        let mut start = 0usize;
        let mut w = 0.0f32;
        let mut brk: Option<usize> = None; // index just after the last space
        let mut i = 0usize;
        while i < n {
            let c = para[i];
            if c != ' ' && w + ws[i] > NOTE_W && i > start {
                let b = match brk {
                    Some(b) if b > start => b,
                    _ => i,
                };
                out.push((base + start, base + b));
                start = b;
                w = ws[start..i].iter().sum();
                brk = None;
                continue;
            }
            w += ws[i];
            if c == ' ' {
                brk = Some(i + 1);
            }
            i += 1;
        }
        out.push((base + start, base + n));
        base += n + 1;
    }
    out
}

/// Index of the visual line that holds the caret.
fn line_of(lines: &[(usize, usize)], cur: usize) -> usize {
    lines.iter().rposition(|&(s, _)| s <= cur).unwrap_or(0)
}

/// Caret position for "End" / clamping on line `li` (stays on this line even when it soft-wraps).
fn line_end(lines: &[(usize, usize)], li: usize) -> usize {
    let (s, e) = lines[li];
    if li + 1 < lines.len() && lines[li + 1].0 == e && e > s {
        e - 1
    } else {
        e
    }
}

/// Char index on line `li` closest to pixel offset `px` (relative to the text origin).
fn index_at(ws: &[f32], spans: &[(usize, usize)], lines: &[(usize, usize)], li: usize, px: f32) -> usize {
    let s = lines[li].0;
    let e = line_end(lines, li);
    let mut acc = 0.0f32;
    let mut idx = s;
    while idx < e {
        let w = ws[idx];
        if acc + w / 2.0 > px {
            break;
        }
        acc += w;
        idx += 1;
    }
    snap(spans, idx, true)
}

fn word_left(v: &[char], mut i: usize) -> usize {
    while i > 0 && v[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !v[i - 1].is_whitespace() {
        i -= 1;
    }
    i
}

fn word_right(v: &[char], mut i: usize) -> usize {
    while i < v.len() && !v[i].is_whitespace() {
        i += 1;
    }
    while i < v.len() && v[i].is_whitespace() {
        i += 1;
    }
    i
}

/// Ctrl+Backspace: trailing spaces, then either one newline or the previous word.
fn word_back(v: &[char], from: usize) -> usize {
    let mut j = from;
    while j > 0 && v[j - 1] == ' ' {
        j -= 1;
    }
    if j > 0 && v[j - 1] == '\n' {
        j - 1
    } else {
        while j > 0 && v[j - 1] != ' ' && v[j - 1] != '\n' {
            j -= 1;
        }
        j
    }
}

fn fmt_num(n: usize) -> String {
    let s = n.to_string();
    let mut o = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            o.push(',');
        }
        o.push(c);
    }
    o
}

// ---------------------------------------------------------------- clipboard
pub(crate) unsafe fn clip_set(hwnd: HWND, text: &str) {
    if OpenClipboard(hwnd).is_err() {
        return;
    }
    let _ = EmptyClipboard();
    let wide: Vec<u16> = text.replace('\n', "\r\n").encode_utf16().chain(std::iter::once(0)).collect();
    if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
        let p = GlobalLock(h) as *mut u16;
        if p.is_null() {
            let _ = GlobalFree(h);
        } else {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(h);
            if SetClipboardData(CF_UNICODETEXT, HANDLE(h.0)).is_err() {
                let _ = GlobalFree(h);
            }
        }
    }
    let _ = CloseClipboard();
}

pub(crate) unsafe fn clip_get(hwnd: HWND) -> Option<String> {
    if OpenClipboard(hwnd).is_err() {
        return None;
    }
    let out = read_open_clipboard();
    let _ = CloseClipboard();
    out
}

/// The text on the clipboard, which the caller has already opened.
pub(crate) unsafe fn read_open_clipboard() -> Option<String> {
    let h = GetClipboardData(CF_UNICODETEXT).ok()?;
    let hg = HGLOBAL(h.0);
    let p = GlobalLock(hg) as *const u16;
    if p.is_null() {
        return None;
    }
    let mut len = 0usize;
    while len < (1 << 20) && *p.add(len) != 0 {
        len += 1;
    }
    let out = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
    let _ = GlobalUnlock(hg);
    Some(out)
}

// ---------------------------------------------------------------- editing
impl App {
    /// Keep caret/anchor valid (the note can be changed from elsewhere, e.g. "Clear").
    fn clamp_nv(&mut self) {
        let n = self.note.chars().count();
        self.nv.cur = self.nv.cur.min(n);
        self.nv.anchor = self.nv.anchor.min(n);
    }

    fn sel_range(&self) -> (usize, usize) {
        (self.nv.cur.min(self.nv.anchor), self.nv.cur.max(self.nv.anchor))
    }

    fn sel_text(&mut self) -> Option<String> {
        self.clamp_nv();
        let (lo, hi) = self.sel_range();
        if hi > lo {
            Some(self.note.chars().skip(lo).take(hi - lo).collect())
        } else {
            None
        }
    }

    pub(crate) unsafe fn begin_note(&mut self) {
        self.editing = true;
        self.note_mode = true;
        self.caret_on = true;
        self.edit_start = Instant::now();
        self.nv.focus = Some(Instant::now());
        let n = self.note.chars().count();
        self.nv.cur = n;
        self.nv.anchor = n;
        self.nv.sel_drag = false;
        self.note_ensure_visible();
        self.prev_fg = GetForegroundWindow().0 as isize;
        let hwnd = HWND(self.hwnd as *mut c_void);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);
    }

    /// Scroll so the caret line is inside the visible window.
    pub(crate) fn note_ensure_visible(&mut self) {
        let cur = self.nv.cur.min(self.note.chars().count());
        let chips = self.cfg.link_titles;
        let Some(g) = self.gfx.as_mut() else { return; };
        let lines = layout(g, &self.note, chips);
        let li = line_of(&lines, cur);
        let max = lines.len().saturating_sub(NOTE_LINES);
        let mut s = self.nv.scroll.min(max);
        if li < s {
            s = li;
        } else if li >= s + NOTE_LINES {
            s = li + 1 - NOTE_LINES;
        }
        self.nv.scroll = s.min(max);
    }

    /// Common tail of every editing action: keep caret visible, restart blink, repaint.
    unsafe fn note_refresh(&mut self) {
        self.note_ensure_visible();
        self.edit_start = Instant::now();
        self.caret_on = true;
        self.last = Instant::now();
        present(self);
        schedule(self);
    }

    /// Insert text at the caret (replacing the selection). Input beyond the limit is cut off.
    fn note_insert(&mut self, text: &str) -> bool {
        let mut ins: Vec<char> = Vec::new();
        let mut prev_cr = false;
        for ch in text.chars() {
            match ch {
                '\r' => {
                    ins.push('\n');
                    prev_cr = true;
                    continue;
                }
                '\n' => {
                    if !prev_cr {
                        ins.push('\n');
                    }
                }
                '\t' => ins.push(' '),
                c if (c as u32) >= 32 && (c as u32) != 127 => ins.push(c),
                _ => {}
            }
            prev_cr = false;
        }
        if ins.is_empty() {
            return false;
        }
        self.clamp_nv();
        let (lo, hi) = self.sel_range();
        let mut v: Vec<char> = self.note.chars().collect();
        if self.cfg.link_titles {
            // a link stays its own word: a space goes between it and text typed or pasted against it
            let spans = url_spans(&v);
            let starts_url = {
                let t: String = ins.iter().take(8).collect();
                t.starts_with("http://") || t.starts_with("https://")
            };
            let at_link_end = lo > 0 && spans.iter().any(|&(_, b)| b == lo);
            if lo > 0 && !v[lo - 1].is_whitespace() && (starts_url || (at_link_end && ins.len() > 1)) && !ins[0].is_whitespace() {
                ins.insert(0, ' ');
            }
            let before_link = hi < v.len() && spans.iter().any(|&(a, _)| a == hi);
            let ends_url = ins.split(|c| c.is_whitespace()).next_back().is_some_and(|t| {
                let t: String = t.iter().collect();
                t.starts_with("http://") || t.starts_with("https://")
            });
            let last_ws = ins.last().is_some_and(|c| c.is_whitespace());
            if hi < v.len() && !v[hi].is_whitespace() && !last_ws && (ends_url || before_link) {
                ins.push(' ');
            }
        }
        let room = NOTE_MAX_CHARS.saturating_sub(v.len() - (hi - lo));
        if room == 0 {
            return false;
        }
        ins.truncate(room); // pasted text is truncated to the remaining allowance
        let p = lo + ins.len();
        v.splice(lo..hi, ins);
        let added = p - lo;
        self.note = v.into_iter().collect();
        self.nv.cur = p;
        self.nv.anchor = p;
        self.nv.fresh = Some((lo, p, Instant::now()));
        if added > 1 {
            self.nv.flash = Some(Instant::now());
        }
        true
    }

    fn note_delete(&mut self, forward: bool, word: bool) -> bool {
        self.clamp_nv();
        let (lo, hi) = self.sel_range();
        let mut v: Vec<char> = self.note.chars().collect();
        let spans = if self.cfg.link_titles { url_spans(&v) } else { Vec::new() };
        let (a, b) = if hi > lo {
            (lo, hi)
        } else if forward {
            if hi >= v.len() {
                return false;
            }
            (hi, snap(&spans, hi + 1, true))
        } else {
            if lo == 0 {
                return false;
            }
            (if word { word_back(&v, lo) } else { snap(&spans, lo - 1, false) }, lo)
        };
        v.drain(a..b);
        self.note = v.into_iter().collect();
        self.nv.cur = a;
        self.nv.anchor = a;
        true
    }

    /// WM_CHAR while the note is being edited.
    pub(crate) unsafe fn note_char(&mut self, c: u32) {
        let hwnd = HWND(self.hwnd as *mut c_void);
        match c {
            27 => self.finish_edit(false, true),
            1 => {
                // Ctrl+A
                self.nv.anchor = 0;
                self.nv.cur = self.note.chars().count();
            }
            3 => {
                // Ctrl+C
                if let Some(t) = self.sel_text() {
                    clip_set(hwnd, &t);
                }
            }
            24 => {
                // Ctrl+X
                if let Some(t) = self.sel_text() {
                    clip_set(hwnd, &t);
                    self.note_delete(false, false);
                }
            }
            22 => {
                // Ctrl+V
                if let Some(t) = clip_get(hwnd) {
                    self.note_insert(&t);
                }
            }
            13 => {
                self.note_insert("\n");
            }
            8 => {
                self.note_delete(false, false);
            }
            127 => {
                self.note_delete(false, true); // Ctrl+Backspace
            }
            c if c >= 32 => {
                if let Some(ch) = char::from_u32(c) {
                    let mut b = [0u8; 4];
                    self.note_insert(ch.encode_utf8(&mut b));
                }
            }
            _ => {}
        }
        self.note_refresh();
    }

    /// Target caret index for Home/End/Up/Down/PageUp/PageDown.
    fn note_nav(&mut self, vk: u32, ctrl: bool, v: &[char]) -> usize {
        let n = v.len();
        let cur = self.nv.cur.min(n);
        let chips = self.cfg.link_titles;
        let Some(g) = self.gfx.as_mut() else { return cur; };
        let lines = layout(g, &self.note, chips);
        let ws = widths(g, v, chips);
        let pre = prefix(&ws);
        let spans = if chips { url_spans(v) } else { Vec::new() };
        let li = line_of(&lines, cur);
        match vk {
            K_HOME => {
                if ctrl {
                    0
                } else {
                    lines[li].0
                }
            }
            K_END => {
                if ctrl {
                    n
                } else {
                    line_end(&lines, li)
                }
            }
            _ => {
                let up = vk == K_UP || vk == K_PGUP;
                let step = if vk == K_UP || vk == K_DOWN { 1 } else { NOTE_LINES };
                if up && li == 0 {
                    return 0;
                }
                if !up && li + 1 >= lines.len() {
                    return n;
                }
                let tl = if up { li.saturating_sub(step) } else { (li + step).min(lines.len() - 1) };
                let px = pre[cur] - pre[lines[li].0];
                index_at(&ws, &spans, &lines, tl, px)
            }
        }
    }

    /// WM_KEYDOWN while editing the note. Returns true when the key was consumed.
    pub(crate) unsafe fn note_key(&mut self, vk: u32, shift: bool, ctrl: bool) -> bool {
        self.clamp_nv();
        if vk == K_DELETE {
            self.note_delete(true, false);
            self.note_refresh();
            return true;
        }
        let v: Vec<char> = self.note.chars().collect();
        let n = v.len();
        let (lo, hi) = self.sel_range();
        let cur = self.nv.cur;
        let spans = if self.cfg.link_titles { url_spans(&v) } else { Vec::new() };
        let target = match vk {
            K_LEFT => {
                if !shift && hi > lo {
                    lo
                } else if ctrl {
                    word_left(&v, cur)
                } else {
                    snap(&spans, cur.saturating_sub(1), false)
                }
            }
            K_RIGHT => {
                if !shift && hi > lo {
                    hi
                } else if ctrl {
                    word_right(&v, cur)
                } else {
                    snap(&spans, (cur + 1).min(n), true)
                }
            }
            K_UP | K_DOWN | K_PGUP | K_PGDN | K_HOME | K_END => self.note_nav(vk, ctrl, &v),
            _ => return false,
        };
        self.nv.cur = target;
        if !shift {
            self.nv.anchor = target;
        }
        self.note_refresh();
        true
    }

    // ------------------------------------------------------------ mouse
    /// Char index under a point (client coordinates).
    fn note_hit(&mut self, x: f32, y: f32) -> usize {
        let cur = self.nv.cur;
        let chars: Vec<char> = self.note.chars().collect();
        let chips = self.cfg.link_titles;
        let Some(g) = self.gfx.as_mut() else { return cur; };
        let lines = layout(g, &self.note, chips);
        let ws = widths(g, &chars, chips);
        let spans = if chips { url_spans(&chars) } else { Vec::new() };
        let max = lines.len().saturating_sub(NOTE_LINES);
        let scroll = self.nv.scroll.min(max) as i32;
        let rel = ((y - (NOTE_Y + NOTE_TOP)) / NOTE_LINE).floor() as i32;
        let li = (scroll + rel).clamp(0, lines.len() as i32 - 1) as usize;
        index_at(&ws, &spans, &lines, li, x - (LX + 14.0))
    }

    /// Mouse press inside the note card while editing: place caret, start drag-selection.
    pub(crate) unsafe fn note_mouse_down(&mut self, x: f32, y: f32, shift: bool) -> bool {
        if !(self.ready() && self.editing && self.note_mode && self.is_slide(SlideId::Scrap) && in_rect(x, y, LX, NOTE_Y, CW, NOTE_H)) {
            return false;
        }
        self.clamp_nv();
        let idx = self.note_hit(x, y);
        self.nv.cur = idx;
        if !shift {
            self.nv.anchor = idx;
        }
        self.nv.sel_drag = true;
        self.note_refresh();
        true
    }

    pub(crate) unsafe fn note_mouse_drag(&mut self, x: f32, y: f32) {
        let idx = self.note_hit(x, y);
        if idx != self.nv.cur {
            self.nv.cur = idx;
            self.note_refresh();
        }
    }

    /// Mouse wheel / trackpad over the Scrap slide. Returns false when the text fits (no scrolling needed).
    pub(crate) fn note_wheel(&mut self, d: i32) -> bool {
        let chips = self.cfg.link_titles;
        let Some(g) = self.gfx.as_mut() else { return false; };
        let total = layout(g, &self.note, chips).len();
        if total <= NOTE_LINES {
            self.nv.wheel = 0;
            return false;
        }
        let max = total - NOTE_LINES;
        self.nv.wheel += d;
        let steps = self.nv.wheel / WHEEL_STEP;
        if steps != 0 {
            self.nv.wheel -= steps * WHEEL_STEP;
            let s = self.nv.scroll.min(max) as i32 - steps;
            self.nv.scroll = s.clamp(0, max as i32) as usize;
        }
        true
    }

    // ------------------------------------------------------------ drawing
    /// Tata letak mode baca: kata dan chip tautan per baris, dengan posisi x dan lebarnya.
    fn note_read_layout(g: &mut Gfx, text: &str) -> Vec<Vec<(f32, ReadItem, f32)>> {
        let sp = g.width(" ", NOTE_SIZE);
        let mut lines: Vec<Vec<(f32, ReadItem, f32)>> = Vec::new();
        for para in text.split('\n') {
            let mut line: Vec<(f32, ReadItem, f32)> = Vec::new();
            let mut x = 0.0f32;
            for word in para.split(' ').filter(|w| !w.is_empty()) {
                let is_url = word.starts_with("http://") || word.starts_with("https://");
                let (item, w) = if is_url {
                    let url = word.trim_end_matches(|c: char| ".,;:!?)]}'\"".contains(c)).to_string();
                    chip_item(g, &url)
                } else {
                    let w = g.width(word, NOTE_SIZE);
                    (ReadItem::Word(word.to_string()), w)
                };
                if x > 0.0 && x + w > NOTE_W {
                    lines.push(std::mem::take(&mut line));
                    x = 0.0;
                }
                line.push((x, item, w));
                x += w + sp;
            }
            lines.push(line);
        }
        lines
    }

    /// Tautan di bawah (x, y) pada chip mode baca (None bila tidak ada, atau sedang mengedit / fitur dimatikan).
    /// Penyorot ketik/tempel, caret, dan chip tautan perlu frame terus-menerus selama diedit atau selama chip beranimasi.
    pub(crate) fn note_animating(&self) -> bool {
        if !(self.expanded() && self.is_slide(SlideId::Scrap)) {
            return false;
        }
        if self.note_mode {
            return true;
        }
        let rec = |t: Option<Instant>, d: f32| t.is_some_and(|t| t.elapsed().as_secs_f32() < d);
        let armed_or_copied = self.note_armed() || rec(self.nv.copied, 1.35) || rec(self.nv.clear_arm, 2.6);
        let hover_btn = !self.note.is_empty() && (self.nv.ha.get() > 0.02 || self.cursor_canvas().is_some_and(|(cx, cy)| in_rect(cx, cy, LX, 6.0, CW, NOTE_Y + NOTE_H - 6.0)));
        if armed_or_copied || hover_btn || rec(self.nv.flash, 0.55) || rec(self.nv.saved, 2.05) {
            return true;
        }
        self.cfg.link_titles && linktitle::urls_in(&self.note).iter().any(|u| match linktitle::state(u) {
            Some(linktitle::LinkState::Loading) => true,
            Some(_) => linktitle::age(u).is_some_and(|a| a < 0.35),
            None => false,
        })
    }

    pub(crate) fn note_link_at(&mut self, x: f32, y: f32) -> Option<String> {
        if self.note_mode || !self.cfg.link_titles || linktitle::urls_in(&self.note).is_empty() {
            return None;
        }
        let mut g = self.gfx.take()?;
        let lines = Self::note_read_layout(&mut g, &self.note);
        self.gfx = Some(g);
        let scroll = self.nv.scroll.min(lines.len().saturating_sub(NOTE_LINES));
        let tx = LX + 14.0;
        for li in scroll..(scroll + NOTE_LINES).min(lines.len()) {
            let ly = NOTE_Y + NOTE_TOP + (li - scroll) as f32 * NOTE_LINE;
            for (lx, item, w) in &lines[li] {
                if let ReadItem::Chip { url, .. } = item {
                    if in_rect(x, y, tx + lx, ly, *w, 16.0) {
                        return Some(url.clone());
                    }
                }
            }
        }
        None
    }

    /// Tautan sebagai chip: bola dunia kecil dan judul halaman (atau domainnya selagi memuat).
    #[allow(clippy::too_many_arguments)]
    fn draw_chip(&self, g: &mut Gfx, cx0: f32, cy0: f32, w: f32, label: &str, domain: &str, k: f32, loading: bool, al: f32) {
        let (m, b) = (g.acc.main, g.acc.bright);
        g.fill(&rrect(cx0, cy0, w, 16.0, 8.0), &paint(m[0], m[1], m[2], 0.30 * al));
        g.stroke(&rrect(cx0 + 0.5, cy0 + 0.5, w - 1.0, 15.0, 7.5), &paint(b[0], b[1], b[2], 0.5 * al), 1.0);
        // ikon tautan: bola dunia kecil
        let (ix, iy) = (cx0 + 11.0, cy0 + 8.0);
        let p = paint(255, 255, 255, 0.9 * al);
        g.ring(ix, iy, 3.8, 1.0, 1.1, &p, &p);
        g.line(ix - 3.8, iy, ix + 3.8, iy, 1.0, &p);
        g.line(ix, iy - 3.8, ix, iy + 3.8, 1.0, &p);
        let a = if loading { 0.7 } else { 1.0 };
        let ly = cy0 + ty(16.0, 11.0);
        if k < 1.0 && k > 0.0 {
            g.txtb(domain, cx0 + 20.0, ly - k * 3.0, 11.0, [255, 255, 255], al * (1.0 - k), w - 24.0);
            g.txtb(label, cx0 + 20.0, ly + (1.0 - k) * 3.0, 11.0, [255, 255, 255], al * k, w - 24.0);
        } else {
            g.txtb(label, cx0 + 20.0, ly, 11.0, [255, 255, 255], al * a, w - 24.0);
        }
        if loading {
            // kilau yang menyapu chip selagi judul dimuat
            let sx = cx0 + (((self.t * 0.9) % 1.4) - 0.2) * w;
            let (l, r) = (sx.max(cx0 + 2.0), (sx + w * 0.4).min(cx0 + w - 2.0));
            if r > l {
                g.fill(&rrect(l, cy0 + 2.0, r - l, 12.0, 6.0), &paint(255, 255, 255, 0.16 * al));
            }
        }
    }

    fn note_read_view(&self, g: &mut Gfx, ox: f32, al: f32) {
        let tx = LX + ox + 14.0;
        let lines = Self::note_read_layout(g, &self.note);
        let total = lines.len();
        let max_scroll = total.saturating_sub(NOTE_LINES);
        let scroll = self.nv.scroll.min(max_scroll);
        for li in scroll..(scroll + NOTE_LINES).min(total) {
            let y = NOTE_Y + NOTE_TOP + (li - scroll) as f32 * NOTE_LINE;
            // baris paling atas / bawah memudar bila masih ada teks di luar jendela
            let edge = (scroll > 0 && li == scroll) || (li + 1 == scroll + NOTE_LINES && li + 1 < total);
            let al = if edge { al * 0.5 } else { al };
            let pr = self.nv.saved.map_or(1.0, |t| (t.elapsed().as_secs_f32() / 0.25).clamp(0.0, 1.0));
            for (lx, item, w) in &lines[li] {
                match item {
                    ReadItem::Word(t) => g.txt(t, tx + lx, y, NOTE_SIZE, [255, 228, 225], al, *w + 4.0),
                    ReadItem::Chip { label, domain, k, loading, .. } => {
                        // chip muncul dengan naik dan memudar masuk setelah catatan disimpan
                        self.draw_chip(g, tx + lx, y + 0.5 + (1.0 - pr) * 3.0, *w, label, domain, *k, *loading, al * pr);
                    }
                }
            }
        }
        if total > NOTE_LINES {
            let top = NOTE_Y + 10.0;
            let th = NOTE_H - 20.0;
            let bh = (th * NOTE_LINES as f32 / total as f32).clamp(14.0, th);
            let pos = if max_scroll == 0 { 0.0 } else { scroll as f32 / max_scroll as f32 };
            g.fill(&rrect(LX + ox + CW - 8.0, top + (th - bh) * pos, 2.5, bh, 1.25), &paint(255, 255, 255, 0.22 * al));
        }
    }

    pub(crate) fn slide_note(&self, g: &mut Gfx, ox: f32, al: f32) {
        let x = LX + ox;
        g.txtb(NOTE_NAME, x, 12.0, 15.0, [255, 255, 255], al, 100.0);

        let editing = self.note_mode;
        let chars: Vec<char> = self.note.chars().collect();
        let n = chars.len();

        let (hint, hcol) = if editing { (tr("Esc to save"), TEXT_FAINT) } else { ("", g.acc.main) };
        let hw = if hint.is_empty() { 0.0 } else { g.width(hint, 12.0) };
        if !hint.is_empty() {
            g.txt(hint, RX + ox - hw, 14.0, 12.0, hcol, al, 120.0);
        }
        // Penghitung karakter: cincin kecil (kuning di 90%, merah di batas); angkanya muncul saat mengetik atau hampir penuh.
        if editing || n > 0 {
            let frac = n as f32 / NOTE_MAX_CHARS as f32;
            let col = if n >= NOTE_MAX_CHARS { ACCENT } else if frac >= 0.9 { [255, 190, 60] } else { g.acc.main };
            let rcx = RX + ox - if hw > 0.0 { hw + 16.0 } else { 98.0 };
            g.ring(rcx, 21.0, 7.0, frac.max(0.03), 2.4, &paint(255, 255, 255, 0.14 * al), &paint(col[0], col[1], col[2], al));
            if editing || frac >= 0.9 {
                let label = format!("{} / {}", fmt_num(n), fmt_num(NOTE_MAX_CHARS));
                let lw = g.width(&label, 11.0);
                g.txt(&label, rcx - 12.0 - lw, 16.0, 11.0, col, al, 140.0);
            }
        }

        // tombol Salin dan Hapus (dua langkah) muncul saat kursor di kartu atau header
        if !editing && n > 0 {
            let armed = self.note_armed();
            let over = self.cursor_canvas().is_some_and(|(cx, cy)| in_rect(cx, cy, LX, 6.0, CW, NOTE_Y + NOTE_H - 6.0));
            let tgt = if over || armed { 1.0 } else { 0.0 };
            let ha = self.nv.ha.get() + (tgt - self.nv.ha.get()) * 0.25;
            self.nv.ha.set(ha);
            if ha > 0.02 {
                let (ccx, bx, bw) = note_btn_geo(armed);
                let a = al * ha;
                let ink = |al2: f32| paint(255, 255, 255, al2 * a);
                g.circle(ccx + ox, 19.0, 11.0, &paint(10, 16, 34, 0.92 * a));
                g.circle(ccx + ox, 19.0, 11.0, &paint(255, 255, 255, 0.14 * a));
                if let Some(e) = self.nv.copied.map(|t| t.elapsed().as_secs_f32()).filter(|&e| e < 1.3) {
                    let pp = 1.0 + 0.3 * (-e * 9.0).exp() * (e * 28.0).sin();
                    let gp = paint(60, 200, 110, a);
                    g.line(ccx + ox - 4.5 * pp, 19.0, ccx + ox - 1.2 * pp, 19.0 + 3.5 * pp, 2.0, &gp);
                    g.line(ccx + ox - 1.2 * pp, 19.0 + 3.5 * pp, ccx + ox + 5.0 * pp, 19.0 - 3.5 * pp, 2.0, &gp);
                } else {
                    g.stroke(&rrect(ccx + ox - 1.0, 15.5, 7.0, 8.0, 2.0), &ink(1.0), 1.4);
                    g.stroke(&rrect(ccx + ox - 6.0, 12.0, 7.0, 8.0, 2.0), &ink(1.0), 1.4);
                }
                if armed {
                    let aa = self.nv.clear_arm.map_or(1.0, |t| t.elapsed().as_secs_f32());
                    let sc = (0.7 + 0.3 * (aa / 0.15).clamp(0.0, 1.0)).min(1.0);
                    let (pw, ph) = (bw * sc, 22.0 * sc);
                    g.fill(&rrect(bx + ox + (bw - pw) / 2.0, 19.0 - ph / 2.0, pw, ph, ph / 2.0), &paint(225, 70, 70, a));
                    g.txt_c(tr("Delete?"), bx + ox + bw / 2.0, 19.0 - 7.0, 11.0, [255, 255, 255], a, bw, true);
                } else {
                    let tcx = bx + 11.0 + ox;
                    g.circle(tcx, 19.0, 11.0, &paint(10, 16, 34, 0.92 * a));
                    g.circle(tcx, 19.0, 11.0, &paint(255, 255, 255, 0.14 * a));
                    g.line(tcx - 5.0, 15.0, tcx + 5.0, 15.0, 1.5, &ink(1.0));
                    g.stroke(&rrect(tcx - 3.6, 15.0, 7.2, 8.5, 1.5), &ink(1.0), 1.4);
                }
            }
        }

        // sticky-note card: deep red body, a brighter "tape" strip on top
        g.fill(&rrect(x, NOTE_Y, CW, NOTE_H, 12.0), &g.sacc((if editing { 0.30 } else { 0.18 }) * al));
        let ss = |e: f32| {
            let t = e.clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        if editing {
            // bingkai fokus memudar masuk
            let k = self.nv.focus.map_or(1.0, |t| ss(t.elapsed().as_secs_f32() / 0.2));
            let (b, m) = (g.acc.bright, g.acc.main);
            g.stroke(&rrect(x + 0.75, NOTE_Y + 0.75, CW - 1.5, NOTE_H - 1.5, 11.5), &paint(b[0], b[1], b[2], 0.85 * k * al), 1.5);
            g.stroke(&rrect(x - 1.5, NOTE_Y - 1.5, CW + 3.0, NOTE_H + 3.0, 13.5), &paint(m[0], m[1], m[2], 0.18 * k * al), 3.0);
        }
        if let Some(e) = self.nv.flash.map(|t| t.elapsed().as_secs_f32()).filter(|&e| e < 0.5) {
            let b = g.acc.bright;
            g.fill(&rrect(x, NOTE_Y, CW, NOTE_H, 12.0), &paint(b[0], b[1], b[2], 0.30 * (1.0 - e / 0.5) * al));
        }
        if let Some(e) = self.nv.saved.map(|t| t.elapsed().as_secs_f32()).filter(|&e| e < 2.0) {
            let a = if e < 0.15 { e / 0.15 } else if e < 1.4 { 1.0 } else { 1.0 - (e - 1.4) / 0.6 };
            let tw = g.width_ex(NOTE_NAME, 15.0, true);
            let pp = 1.0 + 0.4 * (-e * 9.0).exp() * (e * 30.0).sin();
            g.circle(x + tw + 14.0, 20.0, 3.0 * pp, &paint(60, 200, 110, a * al));
            g.txt(tr("Saved"), x + tw + 22.0, 13.0, 11.0, [150, 152, 160], a * al, 60.0);
        }

        let tx = x + 14.0;
        if n == 0 {
            if !editing {
                g.txt(tr("Click to write a note"), tx, NOTE_Y + NOTE_TOP, NOTE_SIZE, [160, 172, 200], al, NOTE_W);
            }
            if editing && self.caret_on {
                g.rect(tx, NOTE_Y + NOTE_TOP + 1.0, 1.5, 15.0, &paint(255, 255, 255, al));
            }
            return;
        }

        // Mode baca: tautan jadi chip berisi judul halaman (atau domainnya selagi memuat); mode edit menampilkan teks asli.
        if !editing && self.cfg.link_titles && !linktitle::urls_in(&self.note).is_empty() {
            self.note_read_view(g, ox, al);
            return;
        }
        // with link titles on, links stay chips while editing and behave as one character
        let chips = self.cfg.link_titles;
        let all_spans = if chips { url_spans(&chars) } else { Vec::new() };
        let ws = widths(g, &chars, chips);
        let pre = prefix(&ws);
        let lines = layout(g, &self.note, chips);
        let total = lines.len();
        let max_scroll = total.saturating_sub(NOTE_LINES);
        let scroll = self.nv.scroll.min(max_scroll);
        let cur = self.nv.cur.min(n);
        let anc = self.nv.anchor.min(n);
        let (slo, shi) = (cur.min(anc), cur.max(anc));

        for li in scroll..(scroll + NOTE_LINES).min(total) {
            let (s, e) = lines[li];
            let y = NOTE_Y + NOTE_TOP + (li - scroll) as f32 * NOTE_LINE;

            // selection highlight behind the text
            if editing && shi > slo {
                let a = slo.max(s);
                let b = shi.min(e);
                let past_end = shi > e && slo <= e; // selection continues past this line's end
                if b > a || past_end {
                    let xa = pre[a.min(e)] - pre[s];
                    let mut xb = pre[b.max(a).min(e)] - pre[s];
                    if past_end {
                        xb += 5.0;
                    }
                    if xb > xa {
                        g.rect(tx + xa, y - 1.0, xb - xa, NOTE_LINE, &g.sacc(0.55 * al));
                    }
                }
            }

            // tautan (http/https) diwarnai aksen dan digarisbawahi; sisanya tetap
            let line = &chars[s..e];
            let mut spans: Vec<(usize, usize)> = Vec::new();
            let mut i = 0;
            if chips {
                for &(a, b) in all_spans.iter().filter(|&&(a, b)| a >= s && b <= e + 1 && a < e) {
                    spans.push((a - s, (b - s).min(line.len())));
                }
                i = line.len();
            }
            while i < line.len() {
                let rest: String = line[i..line.len().min(i + 8)].iter().collect();
                if rest.starts_with("http://") || rest.starts_with("https://") {
                    let mut j = i;
                    while j < line.len() && !line[j].is_whitespace() {
                        j += 1;
                    }
                    spans.push((i, j));
                    i = j;
                } else {
                    i += 1;
                }
            }
            let mut c0 = 0;
            // teks yang baru diketik/ditempel memudar masuk sambil naik sedikit (0,14 dtk)
            let fresh = self.nv.fresh.filter(|f| f.2.elapsed().as_secs_f32() < 0.14);
            let fk = fresh.map_or(1.0, |f| {
                let k = (f.2.elapsed().as_secs_f32() / 0.14).clamp(0.0, 1.0);
                k * k * (3.0 - 2.0 * k)
            });
            let draw_piece = |g: &mut Gfx, a: usize, b: usize, url: bool, am: f32, dy: f32| {
                if b <= a {
                    return;
                }
                let sx = tx + pre[s + a] - pre[s];
                let seg: String = line[a..b].iter().collect();
                if url {
                    let c = g.acc.pale;
                    g.txt(&seg, sx, y + dy, NOTE_SIZE, c, al * am, NOTE_W + 4.0);
                    let w = pre[s + b] - pre[s + a];
                    g.rect(sx, y + dy + NOTE_SIZE * 1.12, w.min(NOTE_W), 1.0, &paint(c[0], c[1], c[2], 0.7 * al * am));
                } else {
                    g.txt(&seg, sx, y + dy, NOTE_SIZE, [255, 228, 225], al * am, NOTE_W + 4.0);
                }
            };
            let draw_seg = |g: &mut Gfx, a: usize, b: usize, url: bool| {
                if b <= a {
                    return;
                }
                match fresh {
                    Some((fs, fe, _)) => {
                        let (la, lb) = (fs.saturating_sub(s).clamp(a, b), fe.saturating_sub(s).clamp(a, b));
                        if lb > la {
                            draw_piece(g, a, la, url, 1.0, 0.0);
                            draw_piece(g, la, lb, url, fk, (1.0 - fk) * 3.0);
                            draw_piece(g, lb, b, url, 1.0, 0.0);
                        } else {
                            draw_piece(g, a, b, url, 1.0, 0.0);
                        }
                    }
                    None => draw_piece(g, a, b, url, 1.0, 0.0),
                }
            };
            for &(a, b) in &spans {
                draw_seg(g, c0, a, false);
                if chips {
                    let url = url_text(&chars, s + a, s + b);
                    if let (ReadItem::Chip { label, domain, k, loading, .. }, w) = chip_item(g, &url) {
                        self.draw_chip(g, tx + pre[s + a] - pre[s], y + 0.5, w, &label, &domain, k, loading, al);
                    }
                } else {
                    draw_seg(g, a, b, true);
                }
                c0 = b;
            }
            draw_seg(g, c0, line.len(), false);
        }

        // caret
        if editing && slo == shi {
            let li = line_of(&lines, cur);
            if li >= scroll && li < scroll + NOTE_LINES {
                let cx = pre[cur] - pre[lines[li].0];
                let y = NOTE_Y + NOTE_TOP + 1.0 + (li - scroll) as f32 * NOTE_LINE;
                let target = (tx + cx + 0.5, y);
                // caret meluncur halus ke posisi barunya, berkedip lembut (tetap terang sebentar setelah mengetik)
                let (gx, gy) = match self.nv.glide.get() {
                    Some((px, py)) if (px - target.0).abs() < 60.0 && (py - target.1).abs() < 40.0 => (px + (target.0 - px) * 0.45, py + (target.1 - py) * 0.45),
                    _ => target,
                };
                self.nv.glide.set(Some((gx, gy)));
                let e = self.edit_start.elapsed().as_secs_f32();
                let ka = if e < 0.5 { 1.0 } else { 0.5 + 0.5 * ((e - 0.5) * std::f32::consts::TAU / 1.1).cos() };
                g.rect(gx, gy, 1.5, 15.0, &paint(255, 255, 255, ka * al));
            }
        } else {
            self.nv.glide.set(None);
        }

        // minimal scrollbar: a thin thumb on the card's right edge, only when the text overflows
        if total > NOTE_LINES {
            let top = NOTE_Y + 10.0;
            let th = NOTE_H - 20.0;
            let bh = (th * NOTE_LINES as f32 / total as f32).clamp(14.0, th);
            let pos = if max_scroll == 0 { 0.0 } else { scroll as f32 / max_scroll as f32 };
            let by = top + (th - bh) * pos;
            g.fill(&rrect(x + CW - 8.0, by, 2.5, bh, 1.25), &paint(255, 255, 255, (if editing { 0.34 } else { 0.22 }) * al));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn links_are_whole_tokens() {
        let v = chars("see https://a.example/x?y=1 and http://b.example done");
        assert_eq!(url_spans(&v), vec![(4, 27), (32, 48)]);
        assert!(url_spans(&chars("http:// abchttps://x.example")).is_empty());
    }

    #[test]
    fn the_caret_never_stops_inside_a_link() {
        let v = chars("a https://x.example b");
        let spans = url_spans(&v);
        assert_eq!(spans, vec![(2, 19)]);
        assert_eq!(snap(&spans, 10, true), 19);
        assert_eq!(snap(&spans, 10, false), 2);
        assert_eq!(snap(&spans, 19, true), 19);
        assert_eq!(snap(&spans, 2, false), 2);
        assert_eq!(snap(&spans, 19 - 1, false), 2);
        assert_eq!(snap(&spans, 2 + 1, true), 19);
    }
}

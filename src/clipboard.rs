//! Clipboard history: the last few texts copied in any app, shown on the Clipboard slide. Click one to copy it again.
//! Text only, kept in memory (never written to disk), and entries that ask not to be recorded (password managers) are
//! skipped.

use super::*;
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

/// Most entries kept; the oldest is dropped first.
pub(crate) const MAX_ITEMS: usize = 10;
/// Longer copies (e.g. a whole file) are not kept.
pub(crate) const MAX_CHARS: usize = 100_000;
/// Standard clipboard format for UTF-16 text.
const CF_UNICODETEXT: u32 = 13;

pub(crate) struct Clip {
    pub(crate) text: String,
    /// When it was copied (last time, if it was copied more than once).
    pub(crate) at: Instant,
}

/// Newest first.
#[derive(Default)]
pub(crate) struct History {
    items: Vec<Clip>,
}

impl History {
    pub(crate) fn items(&self) -> &[Clip] {
        &self.items
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Add `text` as the newest entry. Blank or very long text is ignored; text already in the list moves to the top.
    /// True if the list changed.
    pub(crate) fn push(&mut self, text: &str, now: Instant) -> bool {
        if text.trim().is_empty() || text.chars().count() > MAX_CHARS {
            return false;
        }
        match self.items.iter().position(|c| c.text == text) {
            Some(0) => return false,
            Some(i) => {
                let mut c = self.items.remove(i);
                c.at = now;
                self.items.insert(0, c);
            }
            None => {
                self.items.insert(0, Clip { text: text.to_string(), at: now });
                self.items.truncate(MAX_ITEMS);
            }
        }
        true
    }

    pub(crate) fn remove(&mut self, i: usize) {
        if i < self.items.len() {
            self.items.remove(i);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.items.clear();
    }
}

/// One row of text: every run of whitespace (newlines, tabs) becomes one space.
pub(crate) fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How long ago, short: "now", "5m", "2h", "3d".
pub(crate) fn ago(secs: u64) -> String {
    match secs {
        0..=59 => tr("now").into(),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86399 => fill(tr("{}h"), &[&(secs / 3600)]),
        _ => fill(tr("{}d"), &[&(secs / 86400)]),
    }
}

/// May this clipboard content be recorded? `exclude` / `viewer_ignore`: the
/// "ExcludeClipboardContentFromMonitorProcessing" / "Clipboard Viewer Ignore" formats are present; `can_include`:
/// None if "CanIncludeInClipboardHistory" is absent, else its DWORD (0 = do not keep), or `Some(None)` if it is
/// present but could not be read. Password managers set these.
pub(crate) fn allowed(exclude: bool, viewer_ignore: bool, can_include: Option<Option<u32>>) -> bool {
    // when unsure (the value can't be read), don't keep it
    !exclude && !viewer_ignore && !matches!(can_include, Some(None | Some(0)))
}

// ---------------------------------------------------------------- Win32

/// Get `WM_CLIPBOARDUPDATE` on `hwnd` whenever any app changes the clipboard.
pub(crate) fn listen(hwnd: isize) {
    if cfg!(test) {
        return;
    }
    if unsafe { AddClipboardFormatListener(HWND(hwnd as _)) }.is_err() {
        perf::note("clipboard: AddClipboardFormatListener failed, the Clipboard slide stays empty");
    }
}

/// The id of registered format `name` if the clipboard currently holds it.
fn format_present(name: PCWSTR) -> Option<u32> {
    let f = unsafe { RegisterClipboardFormatW(name) };
    (f != 0 && unsafe { IsClipboardFormatAvailable(f) }.is_ok()).then_some(f)
}

/// The DWORD stored in clipboard format `f` (the clipboard must be open).
unsafe fn read_dword(f: u32) -> Option<u32> {
    let h = GetClipboardData(f).ok()?;
    let hg = HGLOBAL(h.0);
    if GlobalSize(hg) < 4 {
        return None;
    }
    let p = GlobalLock(hg) as *const u32;
    if p.is_null() {
        return None;
    }
    let v = p.read_unaligned();
    let _ = GlobalUnlock(hg);
    Some(v)
}

/// OpenClipboard fails while another app still holds it, which is common right after it changed; retry briefly.
unsafe fn open(hwnd: HWND) -> bool {
    for _ in 0..4 {
        if OpenClipboard(hwnd).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// The clipboard's text, if there is some and it may be recorded. Every check runs while the clipboard is open, so
/// the checks and the text come from the same copy.
pub(crate) fn read(hwnd: isize) -> Option<String> {
    let hwnd = HWND(hwnd as _);
    unsafe {
        if !open(hwnd) {
            return None;
        }
        let exclude = format_present(w!("ExcludeClipboardContentFromMonitorProcessing")).is_some();
        let viewer_ignore = format_present(w!("Clipboard Viewer Ignore")).is_some();
        let can_include = format_present(w!("CanIncludeInClipboardHistory")).map(|f| read_dword(f));
        let has_text = IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok();
        let text = if has_text && allowed(exclude, viewer_ignore, can_include) { note::read_open_clipboard() } else { None };
        let _ = CloseClipboard();
        text
    }
}

// ---------------------------------------------------------------- slide

/// Slide layout: rows start under the title, three visible at a time.
pub(crate) const ROW_Y0: f32 = 44.0;
pub(crate) const ROW_H: f32 = 30.0;
pub(crate) const ROW_P: f32 = 35.0;
pub(crate) const ROWS: usize = 3;
/// The "Clear" button at the top right: x, y, w, h.
pub(crate) const CLEAR_BTN: (f32, f32, f32, f32) = (RX - 56.0, 9.0, 56.0, 24.0);
/// How long a copied-back row shows "Copied".
const FLASH_SECS: f32 = 1.2;
/// Width of the × area at a row's right end.
const X_W: f32 = 28.0;

/// Row width for `n` entries (narrower when the scrollbar shows).
pub(crate) fn row_w(n: usize) -> f32 {
    if n > ROWS {
        CW - 10.0
    } else {
        CW
    }
}

/// An entry as shown in its row: one line, cut with "…" only when it does not fit in `maxw`.
pub(crate) fn row_text(g: &mut Gfx, text: &str, maxw: f32) -> String {
    // a row fits far fewer than 300 characters; don't measure a 100,000-character copy every frame
    let head: String = text.chars().take(300).collect();
    let line = one_line(&head);
    if g.width(&line, 13.0) <= maxw {
        line
    } else {
        gfx::ellipsize(g, &line, 13.0, maxw)
    }
}

/// What a click on the Clipboard slide hit.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum ClipHit {
    /// Copy entry `i` back to the clipboard.
    Copy(usize),
    /// The × on entry `i`.
    Remove(usize),
    Clear,
}

impl App {
    fn clip_max_scroll(&self) -> usize {
        self.clips.len().saturating_sub(ROWS)
    }

    pub(crate) fn clip_hit(&self, x: f32, y: f32) -> Option<ClipHit> {
        if self.clips.is_empty() {
            return None;
        }
        let (cx, cy, cw, ch) = CLEAR_BTN;
        if in_rect(x, y, cx, cy, cw, ch) {
            return Some(ClipHit::Clear);
        }
        let w = row_w(self.clips.len());
        for vi in 0..ROWS {
            let i = self.clip_scroll + vi;
            if i >= self.clips.len() {
                break;
            }
            if in_rect(x, y, LX, ROW_Y0 + vi as f32 * ROW_P, w, ROW_H) {
                return Some(if x > LX + w - X_W { ClipHit::Remove(i) } else { ClipHit::Copy(i) });
            }
        }
        None
    }

    /// Apply a click. Returns the text to put on the clipboard (the caller does that, so this stays testable).
    pub(crate) fn clip_apply(&mut self, hit: ClipHit) -> Option<String> {
        let out = match hit {
            ClipHit::Copy(i) => {
                let text = self.clips.items().get(i)?.text.clone();
                self.clips.push(&text, Instant::now());
                self.clip_scroll = 0;
                self.clip_flash = Some(Instant::now());
                Some(text)
            }
            ClipHit::Remove(i) => {
                if let Some(c) = self.clips.items().get(i) {
                    self.clip_gone.retain(|g| g.2.elapsed().as_secs_f32() < 0.3);
                    self.clip_gone.push((c.text.clone(), i.saturating_sub(self.clip_scroll), Instant::now()));
                }
                self.clips.remove(i);
                None
            }
            ClipHit::Clear => {
                // dua langkah: klik pertama meminta konfirmasi ("Sure?"), klik kedua dalam 2,5 detik menghapus
                if self.clip_clear_t.is_some_and(|t| t.elapsed().as_secs_f32() < 2.5) {
                    self.clips.clear();
                    self.clip_clear_t = None;
                } else {
                    self.clip_clear_t = Some(Instant::now());
                }
                None
            }
        };
        self.clip_scroll = self.clip_scroll.min(self.clip_max_scroll());
        out
    }

    /// Copies are only recorded while the Clipboard slide is turned on.
    pub(crate) fn clip_enabled(&self) -> bool {
        self.cfg.slides.iter().any(|&(s, on)| s == SlideId::Clipboard && on)
    }

    /// The clipboard changed to `text` (None: no text, or not allowed to be recorded). True if the list changed.
    pub(crate) fn on_clipboard(&mut self, text: Option<String>, now: Instant) -> bool {
        if !self.clip_enabled() {
            self.clips.clear();
            self.clip_scroll = 0;
            return false;
        }
        text.is_some_and(|t| self.clips.push(&t, now))
    }

    pub(crate) fn clip_wheel(&mut self, down: bool) -> bool {
        let old = self.clip_scroll;
        self.clip_scroll = if down { (old + 1).min(self.clip_max_scroll()) } else { old.saturating_sub(1) };
        self.clip_scroll != old
    }

    fn clip_flashing(&self) -> bool {
        self.clip_flash.is_some_and(|t| t.elapsed().as_secs_f32() < FLASH_SECS)
    }

    pub(crate) fn clip_animating(&self) -> bool {
        self.expanded()
            && self.is_slide(SlideId::Clipboard)
            && (self.clip_flashing()
                || self.clip_clear_t.is_some_and(|t| t.elapsed().as_secs_f32() < 2.6)
                || self.clip_gone.iter().any(|g| g.2.elapsed().as_secs_f32() < 0.3)
                || self.clips.items().first().is_some_and(|c| c.at.elapsed().as_secs_f32() < 0.35))
    }

    pub(crate) fn slide_clipboard(&self, g: &mut Gfx, ox: f32, al: f32) {
        let cur = self.cursor_canvas();
        let x = LX + ox;
        g.txtb("Clip", x, 12.0, 15.0, [255, 255, 255], al, 80.0);
        let tw = g.width_ex("Clip", 15.0, true);
        g.txtb("board", x + tw, 12.0, 15.0, g.acc.main, al, 80.0);
        let n = self.clips.len();
        if n == 0 {
            let cx = x + CW / 2.0;
            g.txt_c(tr("Copy some text and it shows up here"), cx, 66.0, 13.0, TEXT_DIM, al, CW, false);
            g.txt_c(tr("Kept in memory only, gone when the app closes"), cx, 88.0, 11.0, TEXT_FAINT, al, CW, false);
            return;
        }
        let (bx, by, bw, bh) = CLEAR_BTN;
        let armed = self.clip_clear_t.is_some_and(|t| t.elapsed().as_secs_f32() < 2.5);
        if armed {
            g.fill(&rrect(bx + ox, by, bw, bh, bh / 2.0), &paint(235, 87, 87, 0.30 * al));
        } else {
            g.fill(&rrect(bx + ox, by, bw, bh, bh / 2.0), &paint(255, 255, 255, 0.11 * al));
        }
        g.txt_c(if armed { tr("Sure?") } else { tr("Clear") }, bx + ox + bw / 2.0, by + ty(bh, 12.0), 12.0, [255, 255, 255], al, bw, armed);
        let count = fill(tr("{} of {}"), &[&n, &MAX_ITEMS]);
        let cw = g.width(&count, 11.0);
        g.txt(&count, bx + ox - 10.0 - cw, 15.0, 11.0, TEXT_FAINT, al, 80.0);

        let w = row_w(n);
        // baris yang baru dihapus: bergeser ke kanan sambil memudar di posisi lamanya
        for (text, vi, t) in &self.clip_gone {
            let k = (t.elapsed().as_secs_f32() / 0.25).clamp(0.0, 1.0);
            if k < 1.0 && *vi < ROWS {
                let a = al * (1.0 - k);
                let y = ROW_Y0 + *vi as f32 * ROW_P;
                g.fill(&rrect(x + k * 24.0, y, w, ROW_H, 10.0), &paint(255, 255, 255, 0.08 * a));
                let line = row_text(g, text.lines().next().unwrap_or(""), w - 60.0);
                g.txt(&line, x + 38.0 + k * 24.0, y + ty(ROW_H, 13.0), 13.0, [242, 242, 242], a, w - 58.0);
            }
        }
        let flashing = self.clip_flashing();
        for (vi, c) in self.clips.items().iter().skip(self.clip_scroll).take(ROWS).enumerate() {
            let i = self.clip_scroll + vi;
            // item yang baru disalin meluncur masuk dari atas sambil memudar
            let born = (i == 0).then(|| (c.at.elapsed().as_secs_f32() / 0.3).clamp(0.0, 1.0)).unwrap_or(1.0);
            let born = born * born * (3.0 - 2.0 * born);
            let al = al * born;
            let shift: f32 = self
                .clip_gone
                .iter()
                .filter(|(_, gvi, _)| vi >= *gvi)
                .map(|(_, _, t)| (1.0 - (t.elapsed().as_secs_f32() / 0.25).clamp(0.0, 1.0)) * ROW_P)
                .sum();
            let y = ROW_Y0 + vi as f32 * ROW_P - (1.0 - born) * 10.0 + shift;
            let copied = i == 0 && flashing;
            let hover = cur.is_some_and(|(hx, hy)| in_rect(hx, hy, x, y, w, ROW_H));
            let bg = if copied { g.sacc(0.30 * al) } else { paint(255, 255, 255, if hover { 0.13 } else { 0.08 } * al) };
            g.fill(&rrect(x, y, w, ROW_H, 10.0), &bg);
            let (right, rcol) = if copied {
                (tr("Copied").to_string(), g.acc.bright)
            } else if hover {
                (tr("Click to copy").to_string(), g.acc.bright)
            } else {
                (ago(c.at.elapsed().as_secs()), TEXT_FAINT)
            };
            let rw = g.width(&right, 11.0);
            g.txt(&right, x + w - X_W - rw, y + ty(ROW_H, 11.0), 11.0, rcol, al, 60.0);
            let kind = clip_kind(&c.text);
            clip_tile(g, kind, x + 8.0, y + 4.0, al);
            let tx = x + 38.0;
            let trimmed = c.text.trim();
            let extra = trimmed.lines().count().saturating_sub(1);
            let first = trimmed.lines().next().unwrap_or("");
            let maxw = w - 38.0 - X_W - rw - 10.0 - if extra > 0 { 30.0 } else { 0.0 };
            let ry = y + ty(ROW_H, 13.0);
            let used = if let ClipKind::Url = kind {
                // alamat: domain tebal, sisanya redup
                let s = first.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.");
                let (dom, rest) = match s.find('/') {
                    Some(i) => (&s[..i], &s[i..]),
                    None => (s, ""),
                };
                let dw = g.width_ex(dom, 13.0, true).min(maxw);
                g.txtb(dom, tx, ry, 13.0, [255, 255, 255], al, maxw + 2.0);
                if dw < maxw - 8.0 && !rest.is_empty() {
                    let r = row_text(g, rest, maxw - dw - 2.0);
                    g.txt(&r, tx + dw + 2.0, ry, 13.0, [150, 155, 168], al, maxw - dw);
                    dw + 2.0 + g.width(&r, 13.0)
                } else {
                    dw
                }
            } else {
                let line = row_text(g, first, maxw);
                g.txt(&line, tx, ry, 13.0, [242, 242, 242], al, maxw + 2.0);
                g.width(&line, 13.0).min(maxw)
            };
            if extra > 0 {
                let bx = tx + used + 6.0;
                g.fill(&rrect(bx, y + 8.0, 24.0, 14.0, 7.0), &paint(255, 255, 255, 0.16 * al));
                g.txt_c(&format!("+{extra}"), bx + 12.0, y + 8.0 + ty(14.0, 10.0), 10.0, [225, 225, 232], al, 24.0, true);
            }
            let (cx, cy) = (x + w - X_W / 2.0, y + ROW_H / 2.0);
            let xp = paint(255, 255, 255, 0.45 * al);
            g.line(cx - 3.0, cy - 3.0, cx + 3.0, cy + 3.0, 1.5, &xp);
            g.line(cx - 3.0, cy + 3.0, cx + 3.0, cy - 3.0, 1.5, &xp);
        }
        if n > ROWS {
            let th = ROWS as f32 * ROW_P - (ROW_P - ROW_H);
            let thumb = (th * ROWS as f32 / n as f32).max(14.0);
            let ty0 = ROW_Y0 + (th - thumb) * self.clip_scroll as f32 / (n - ROWS) as f32;
            g.fill(&rrect(RX - 4.0 + ox, ROW_Y0, 3.0, th, 1.5), &paint(255, 255, 255, 0.10 * al));
            g.fill(&rrect(RX - 4.0 + ox, ty0, 3.0, thumb, 1.5), &paint(255, 255, 255, 0.55 * al));
        }
    }
}

/// Jenis isi salinan untuk petak di kiri baris.
#[derive(Clone, Copy)]
enum ClipKind {
    Url,
    Email,
    Color([u8; 3]),
    Code,
    Text,
}

fn clip_kind(text: &str) -> ClipKind {
    let s = text.trim();
    let one = !s.contains(char::is_whitespace);
    if (s.starts_with("http://") || s.starts_with("https://")) && one {
        return ClipKind::Url;
    }
    if one {
        if let Some((a, b)) = s.split_once('@') {
            if !a.is_empty() && b.contains('.') {
                return ClipKind::Email;
            }
        }
    }
    if let Some(h) = s.strip_prefix('#') {
        if (h.len() == 3 || h.len() == 6) && h.chars().all(|c| c.is_ascii_hexdigit()) {
            let v = |i: usize| -> u8 {
                if h.len() == 3 {
                    u8::from_str_radix(&h[i..i + 1].repeat(2), 16).unwrap_or(0)
                } else {
                    u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap_or(0)
                }
            };
            return ClipKind::Color([v(0), v(1), v(2)]);
        }
    }
    let starts = ["npm ", "git ", "cargo ", "cd ", "ls ", "python ", "node ", "$ "];
    if starts.iter().any(|p| s.starts_with(p)) || s.contains('{') || s.contains('}') || s.contains(';') || s.contains("=>") {
        return ClipKind::Code;
    }
    ClipKind::Text
}

fn clip_tile(g: &mut Gfx, kind: ClipKind, x: f32, y: f32, al: f32) {
    let acc = g.acc.main;
    let (tc, a) = match kind {
        ClipKind::Url => (acc, 0.28),
        ClipKind::Email => ([50, 200, 210], 0.28),
        ClipKind::Code => ([255, 150, 60], 0.28),
        ClipKind::Text => ([150, 155, 170], 0.28),
        ClipKind::Color(_) => ([255, 255, 255], 0.10),
    };
    g.fill(&rrect(x, y, 22.0, 22.0, 7.0), &paint(tc[0], tc[1], tc[2], a * al));
    let (cx, cy) = (x + 11.0, y + 11.0);
    let w = [255, 255, 255];
    match kind {
        ClipKind::Url => {
            let p = paint(255, 255, 255, 0.92 * al);
            g.ring(cx, cy, 5.2, 1.0, 1.3, &p, &p);
            g.line(cx - 5.2, cy, cx + 5.2, cy, 1.1, &p);
            g.line(cx, cy - 5.2, cx, cy + 5.2, 1.1, &p);
        }
        ClipKind::Email => g.txt_c("@", cx, cy - 8.5, 13.0, w, al, 20.0, true),
        ClipKind::Code => g.txt_c(">_", cx, cy - 7.3, 11.0, w, al, 20.0, true),
        ClipKind::Text => g.txt_c("Aa", cx, cy - 7.3, 11.0, w, al, 20.0, true),
        ClipKind::Color(c) => {
            g.circle(cx, cy, 7.0, &paint(c[0], c[1], c[2], al));
            let p = paint(255, 255, 255, 0.4 * al);
            g.ring(cx, cy, 7.0, 1.0, 1.0, &p, &p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{EmptyClipboard, SetClipboardData};
    use windows::Win32::System::Memory::{GlobalAlloc, GMEM_MOVEABLE};

    fn at(h: &History) -> Vec<&str> {
        h.items().iter().map(|c| c.text.as_str()).collect()
    }

    /// Put `text` on the clipboard, plus extra registered formats holding a DWORD (like a password manager does).
    unsafe fn put(hwnd: HWND, text: &str, extra: &[(PCWSTR, u32)]) {
        unsafe fn set(fmt: u32, bytes: &[u8]) {
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).expect("GlobalAlloc");
            let p = GlobalLock(h) as *mut u8;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            let _ = GlobalUnlock(h);
            SetClipboardData(fmt, HANDLE(h.0)).expect("SetClipboardData");
        }
        assert!(open(hwnd), "OpenClipboard");
        EmptyClipboard().expect("EmptyClipboard");
        let wide: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        set(CF_UNICODETEXT, &wide);
        for (name, v) in extra {
            set(RegisterClipboardFormatW(*name), &v.to_le_bytes());
        }
        let _ = CloseClipboard();
    }

    /// Wait up to `ms` for a WM_CLIPBOARDUPDATE on `hwnd`.
    fn next_update(hwnd: HWND, ms: u64) -> bool {
        let end = Instant::now() + Duration::from_millis(ms);
        let mut msg = MSG::default();
        while Instant::now() < end {
            if unsafe { PeekMessageW(&mut msg, hwnd, WM_CLIPBOARDUPDATE, WM_CLIPBOARDUPDATE, PM_REMOVE) }.as_bool() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// End to end on a real Windows desktop: the listener gets WM_CLIPBOARDUPDATE, `read` returns the copied text,
    /// and copies marked by a password manager are skipped. Runs only on CI (`CI` is set by GitHub Actions) because it
    /// replaces the clipboard contents.
    #[test]
    fn real_clipboard_updates_arrive_and_password_copies_are_skipped() {
        if std::env::var_os("CI").is_none() {
            eprintln!("skipped: changes the real clipboard, runs on CI only");
            return;
        }
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("clipboard test"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                None,
                None,
                None,
            )
            .expect("message-only window");
            AddClipboardFormatListener(hwnd).expect("AddClipboardFormatListener");
            let h = hwnd.0 as isize;

            put(hwnd, "hello from CI\r\nline 2", &[]);
            assert!(next_update(hwnd, 2000), "WM_CLIPBOARDUPDATE arrives");
            assert_eq!(read(h).as_deref(), Some("hello from CI\r\nline 2"));
            eprintln!("plain text: recorded");

            put(hwnd, "secret 1", &[(w!("ExcludeClipboardContentFromMonitorProcessing"), 0)]);
            assert!(next_update(hwnd, 2000));
            assert_eq!(read(h), None, "ExcludeClipboardContentFromMonitorProcessing");

            put(hwnd, "secret 2", &[(w!("CanIncludeInClipboardHistory"), 0)]);
            assert!(next_update(hwnd, 2000));
            assert_eq!(read(h), None, "CanIncludeInClipboardHistory = 0");

            put(hwnd, "fine", &[(w!("CanIncludeInClipboardHistory"), 1)]);
            assert!(next_update(hwnd, 2000));
            assert_eq!(read(h).as_deref(), Some("fine"), "CanIncludeInClipboardHistory = 1");
            eprintln!("password-manager copies: skipped");

            let _ = DestroyWindow(hwnd);
        }
    }

    #[test]
    fn newest_first() {
        let mut h = History::default();
        let t = Instant::now();
        assert!(h.push("one", t));
        assert!(h.push("two", t));
        assert_eq!(at(&h), ["two", "one"]);
    }

    #[test]
    fn copying_the_same_text_again_moves_it_to_the_top() {
        let mut h = History::default();
        let t = Instant::now();
        h.push("one", t);
        h.push("two", t);
        let later = t + Duration::from_secs(30);
        assert!(h.push("one", later));
        assert_eq!(at(&h), ["one", "two"], "no duplicate, moved to the top");
        assert_eq!(h.items()[0].at, later, "its time is refreshed");
        assert!(!h.push("one", later), "already on top: nothing changes");
    }

    #[test]
    fn keeps_only_the_newest_entries() {
        let mut h = History::default();
        let t = Instant::now();
        for i in 0..MAX_ITEMS + 3 {
            h.push(&format!("item {i}"), t);
        }
        assert_eq!(h.len(), MAX_ITEMS);
        assert_eq!(h.items()[0].text, format!("item {}", MAX_ITEMS + 2));
        assert_eq!(h.items()[MAX_ITEMS - 1].text, "item 3", "the oldest ones were dropped");
    }

    #[test]
    fn blank_or_huge_text_is_not_kept() {
        let mut h = History::default();
        let t = Instant::now();
        assert!(!h.push("", t));
        assert!(!h.push(" \r\n\t ", t));
        assert!(!h.push(&"x".repeat(MAX_CHARS + 1), t));
        assert!(h.is_empty());
        assert!(h.push(&"x".repeat(MAX_CHARS), t), "exactly the limit is fine");
    }

    #[test]
    fn keeps_the_text_exactly_as_copied() {
        let mut h = History::default();
        h.push("  line 1\r\nline 2  ", Instant::now());
        assert_eq!(h.items()[0].text, "  line 1\r\nline 2  ", "copying it back must give the same text");
    }

    #[test]
    fn remove_and_clear() {
        let mut h = History::default();
        let t = Instant::now();
        for s in ["a", "b", "c"] {
            h.push(s, t);
        }
        h.remove(1);
        assert_eq!(at(&h), ["c", "a"]);
        h.remove(5); // out of range: ignored
        assert_eq!(h.len(), 2);
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn rows_show_one_line() {
        assert_eq!(one_line("  fn main() {\r\n\tprintln!();\n}  "), "fn main() { println!(); }");
        assert_eq!(one_line("plain"), "plain");
    }

    #[test]
    fn ago_is_short() {
        assert_eq!(ago(0), "now");
        assert_eq!(ago(59), "now");
        assert_eq!(ago(60), "1m");
        assert_eq!(ago(59 * 60 + 59), "59m");
        assert_eq!(ago(3600), "1h");
        assert_eq!(ago(23 * 3600 + 3599), "23h");
        assert_eq!(ago(2 * 86400), "2d");
    }

    #[test]
    fn only_entries_that_do_not_fit_are_cut() {
        let mut g = Gfx::new(1.0);
        assert_eq!(row_text(&mut g, "hello\r\nworld", 200.0), "hello world");
        let long = row_text(&mut g, &"long text ".repeat(40), 200.0);
        assert!(long.ends_with('\u{2026}'), "{long:?}");
        assert!(g.width(&long, 13.0) <= 200.0);
    }

    #[test]
    fn password_manager_copies_are_skipped() {
        assert!(allowed(false, false, None));
        assert!(allowed(false, false, Some(Some(1))));
        assert!(!allowed(true, false, None), "ExcludeClipboardContentFromMonitorProcessing");
        assert!(!allowed(false, true, None), "Clipboard Viewer Ignore");
        assert!(!allowed(false, false, Some(Some(0))), "CanIncludeInClipboardHistory = 0");
        assert!(!allowed(false, false, Some(None)), "CanIncludeInClipboardHistory present but unreadable: do not keep");
    }
}

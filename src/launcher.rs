//! Quick launcher (Ctrl+Alt+Space): the notch opens with a line to type in. Type a few letters, pick with the arrows, Enter
//! opens, Esc closes. Results: installed apps (Start Menu shortcuts), notch commands, a calculator and a web search.
//!
//! The ranking, the usage memory (frecency) and the calculator are plain functions without any window, so they are
//! tested directly. The window side (keys, drawing, focus) is at the end.

use super::*;
use crate::slides::SlideId;
use crate::textfield::TextField;
use std::collections::HashMap;

/// Most results kept for one query, and how many fit on the canvas at once.
const MAX_HITS: usize = 8;
pub(crate) const VISIBLE: usize = 4;
/// Longest query that is typed.
const MAX_QUERY: usize = 80;
/// Usage entries kept in launcher.txt.
const MAX_FREQ: usize = 200;

// ---------------------------------------------------------------- what can be run
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Cmd {
    Settings,
    Dnd,
    Mic,
    Update,
    Slide(SlideId),
}

/// What Enter does with a result.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    /// An app's shortcut (or any path).
    Open(String),
    Url(String),
    /// Put the text on the clipboard.
    Copy(String),
    Cmd(Cmd),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Hit {
    pub(crate) title: String,
    /// The kind, shown at the right of the row.
    pub(crate) sub: &'static str,
    pub(crate) act: Act,
}

/// Something that can be found by name: an app or a command.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub(crate) title: String,
    pub(crate) act: Act,
    pub(crate) sub: &'static str,
}

// ---------------------------------------------------------------- ranking
/// How well `q` (lowercase, not empty) matches `name`: exact, prefix, start of a word, inside, or letters in order.
pub(crate) fn score(q: &str, name: &str) -> Option<i32> {
    let n = name.to_lowercase();
    if n == q {
        return Some(1000);
    }
    if n.starts_with(q) {
        return Some(800);
    }
    if n.split(|c: char| !c.is_alphanumeric()).any(|w| !w.is_empty() && w.starts_with(q)) {
        return Some(600);
    }
    if n.contains(q) {
        return Some(400);
    }
    // letters in order, not next to each other: "vsc" finds "Visual Studio Code"
    let mut it = n.chars();
    q.chars().all(|c| it.any(|x| x == c)).then_some(200)
}

/// How often and how recently things were opened: lowercase title -> (times, last unix second).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Frecency(HashMap<String, (u32, u64)>);

impl Frecency {
    pub(crate) fn parse(s: &str) -> Frecency {
        let mut m = HashMap::new();
        for line in s.lines() {
            let mut p = line.splitn(3, '\t');
            let (Some(c), Some(t), Some(name)) = (p.next(), p.next(), p.next()) else { continue };
            if let (Ok(c), Ok(t)) = (c.parse::<u32>(), t.parse::<u64>()) {
                if !name.is_empty() && m.len() < MAX_FREQ {
                    m.insert(name.to_lowercase(), (c, t));
                }
            }
        }
        Frecency(m)
    }

    pub(crate) fn serialize(&self) -> String {
        let mut v: Vec<_> = self.0.iter().collect();
        v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
        v.iter().map(|(n, (c, t))| format!("{c}\t{t}\t{n}\n")).collect()
    }

    pub(crate) fn load() -> Frecency {
        if cfg!(test) {
            return Frecency::default(); // tests never read the user's own file
        }
        std::fs::read_to_string(data::data_path("launcher.txt")).map(|s| Frecency::parse(&s)).unwrap_or_default()
    }

    pub(crate) fn save(&self) {
        write_data("launcher.txt", self.serialize());
    }

    pub(crate) fn count(&self, title: &str) -> u32 {
        self.0.get(&title.to_lowercase()).map_or(0, |e| e.0)
    }

    pub(crate) fn record(&mut self, title: &str, now: u64) {
        let key = title.to_lowercase();
        let e = self.0.entry(key).or_insert((0, now));
        e.0 = e.0.saturating_add(1);
        e.1 = now;
        if self.0.len() > MAX_FREQ {
            // forget the least used one
            if let Some(k) = self.0.iter().min_by_key(|(_, v)| (v.0, v.1)).map(|(k, _)| k.clone()) {
                self.0.remove(&k);
            }
        }
    }

    /// Added to a match score: what is opened often comes first among equal matches.
    fn bonus(&self, title: &str) -> i32 {
        self.count(title).min(20) as i32 * 8
    }
}

/// Percent-encode a search text for a URL query.
pub(crate) fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A typed web address (no spaces, a dot inside, or http(s)://): the link to open.
pub(crate) fn as_url(q: &str) -> Option<String> {
    let q = q.trim();
    if q.is_empty() || q.contains(char::is_whitespace) {
        return None;
    }
    let lower = q.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return (q.len() > 8).then(|| q.to_string());
    }
    let host = lower.split(['/', '?', '#']).next().unwrap_or("");
    let ok = host.contains('.') && !host.starts_with('.') && !host.ends_with('.') && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    ok.then(|| format!("https://{q}"))
}

/// The results for `query`: a calculation, then the best matches of `items`, then the web.
/// `web` and `open` are the row titles for the two web rows (already translated).
pub(crate) fn search(query: &str, items: &[Item], freq: &Frecency, web: &str, open: &str, copy: &'static str, web_sub: &'static str) -> Vec<Hit> {
    let q = query.trim();
    let mut hits = Vec::new();
    if q.is_empty() {
        let mut used: Vec<(&Item, u32)> = items.iter().map(|i| (i, freq.count(&i.title))).filter(|(_, c)| *c > 0).collect();
        used.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.title.cmp(&b.0.title)));
        hits.extend(used.into_iter().take(MAX_HITS).map(|(i, _)| Hit { title: i.title.clone(), sub: i.sub, act: i.act.clone() }));
        return hits;
    }
    if let Some(v) = calc(q) {
        hits.push(Hit { title: format!("= {v}"), sub: copy, act: Act::Copy(v) });
    }
    let ql = q.to_lowercase();
    let mut ranked: Vec<(i32, &Item)> = items.iter().filter_map(|i| score(&ql, &i.title).map(|s| (s + freq.bonus(&i.title), i))).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.title.chars().count().cmp(&b.1.title.chars().count())).then(a.1.title.cmp(&b.1.title)));
    hits.extend(ranked.into_iter().take(MAX_HITS).map(|(_, i)| Hit { title: i.title.clone(), sub: i.sub, act: i.act.clone() }));
    if let Some(u) = as_url(q) {
        hits.push(Hit { title: format!("{open} {q}"), sub: web_sub, act: Act::Url(u) });
    }
    hits.push(Hit { title: web.replace("{}", q), sub: web_sub, act: Act::Url(format!("https://www.google.com/search?q={}", url_encode(q))) });
    // the web row must stay even when many apps match
    if hits.len() > MAX_HITS {
        let tail = hits.split_off(hits.len() - 1);
        hits.truncate(MAX_HITS - 1);
        hits.extend(tail);
    }
    hits
}

// ---------------------------------------------------------------- calculator
/// `+ - * / % ^`, parentheses, decimals (a dot or a comma). None if it is not a calculation or cannot be worked out
/// (division by zero, too long, too deep, not finite). A lone number is not a calculation.
pub(crate) fn calc(input: &str) -> Option<String> {
    let s: Vec<char> = input.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() || s.len() > 100 || !s.iter().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if !s.iter().any(|c| matches!(c, '+' | '*' | '/' | '%' | '^' | '(')) && !s.iter().skip(1).any(|c| *c == '-') {
        return None;
    }
    let mut p = Parser { s, i: 0, depth: 0 };
    let v = p.expr()?;
    if p.i != p.s.len() || !v.is_finite() {
        return None;
    }
    Some(format_num(v))
}

fn format_num(v: f64) -> String {
    if v.abs() >= 1e15 || (v != 0.0 && v.abs() < 1e-9) {
        return format!("{v:e}");
    }
    let t = format!("{v:.10}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t.is_empty() || t == "-0" { "0".into() } else { t.to_string() }
}

struct Parser {
    s: Vec<char>,
    i: usize,
    depth: u32,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(c @ ('+' | '-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = if c == '+' { v + r } else { v - r };
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        while let Some(c @ ('*' | '/' | '%')) = self.peek() {
            self.i += 1;
            let r = self.power()?;
            v = match c {
                '*' => v * r,
                '/' if r == 0.0 => return None,
                '/' => v / r,
                _ if r == 0.0 => return None,
                _ => v % r,
            };
        }
        Some(v)
    }

    fn power(&mut self) -> Option<f64> {
        let b = self.unary()?;
        if self.peek() == Some('^') {
            self.i += 1;
            self.depth += 1;
            if self.depth > 32 {
                return None;
            }
            let e = self.power()?;
            self.depth -= 1;
            return Some(b.powf(e));
        }
        Some(b)
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek()? {
            '-' => {
                self.i += 1;
                self.depth += 1;
                if self.depth > 32 {
                    return None;
                }
                let v = self.unary()?;
                self.depth -= 1;
                Some(-v)
            }
            '+' => {
                self.i += 1;
                self.unary()
            }
            '(' => {
                self.i += 1;
                self.depth += 1;
                if self.depth > 32 {
                    return None;
                }
                let v = self.expr()?;
                self.depth -= 1;
                if self.peek() != Some(')') {
                    return None;
                }
                self.i += 1;
                Some(v)
            }
            c if c.is_ascii_digit() || c == '.' || c == ',' => {
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.' || c == ',') {
                    self.i += 1;
                }
                let t: String = self.s[start..self.i].iter().map(|c| if *c == ',' { '.' } else { *c }).collect();
                t.parse::<f64>().ok()
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- installed apps
/// (name, shortcut path) of the apps found in the Start Menu; filled by a background thread.
static APPS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static SCANNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    static TEST_APPS: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(crate) fn set_test_apps(v: Vec<(String, String)>) {
    TEST_APPS.with(|t| *t.borrow_mut() = v);
}

pub(crate) fn apps() -> Vec<(String, String)> {
    #[cfg(test)]
    return TEST_APPS.with(|t| t.borrow().clone());
    #[cfg(not(test))]
    APPS.lock().map(|a| a.clone()).unwrap_or_default()
}

/// Read the Start Menu shortcuts once, off the UI thread.
pub(crate) fn scan_async() {
    if cfg!(test) || SCANNED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("launcher-scan".into()).spawn(|| {
        let mut files = Vec::new();
        for var in ["ProgramData", "APPDATA"] {
            if let Ok(base) = std::env::var(var) {
                notifopen::walk(&std::path::Path::new(&base).join(r"Microsoft\Windows\Start Menu\Programs"), 6, &mut files);
            }
        }
        let mut v = names_of(&files);
        v.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        if let Ok(mut a) = APPS.lock() {
            *a = v;
        }
    });
}

/// App names from shortcut files: the file name, one entry per name, uninstallers left out.
pub(crate) fn names_of(files: &[std::path::PathBuf]) -> Vec<(String, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for f in files {
        let Some(name) = f.file_stem().and_then(|s| s.to_str()) else { continue };
        let low = name.to_lowercase();
        if low.contains("uninstall") || low.starts_with("readme") || !seen.insert(low) {
            continue;
        }
        out.push((name.to_string(), f.to_string_lossy().into_owned()));
    }
    out
}

// ---------------------------------------------------------------- side effects (recorded in tests)
#[cfg(test)]
thread_local! {
    static DONE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Tests: what the launcher opened or copied on this thread (and forget it).
#[cfg(test)]
pub(crate) fn done() -> Vec<String> {
    DONE.with(|o| std::mem::take(&mut *o.borrow_mut()))
}

fn open_target(path: &str) {
    #[cfg(test)]
    DONE.with(|o| o.borrow_mut().push(format!("open:{path}")));
    #[cfg(not(test))]
    shelf::open_path(path);
}

fn copy_text(hwnd: isize, text: &str) {
    #[cfg(test)]
    {
        let _ = hwnd;
        DONE.with(|o| o.borrow_mut().push(format!("copy:{text}")));
    }
    #[cfg(not(test))]
    unsafe { note::clip_set(HWND(hwnd as _), text) };
}

// ---------------------------------------------------------------- the open launcher
pub(crate) struct Launcher {
    pub(crate) field: TextField,
    pub(crate) hits: Vec<Hit>,
    pub(crate) sel: usize,
    /// The window that had the keyboard before, to give it back on Esc.
    pub(crate) prev_fg: isize,
}

/// Canvas layout.
pub(crate) const IN_Y: f32 = 12.0;
pub(crate) const IN_H: f32 = 32.0;
pub(crate) const ROW_Y: f32 = 54.0;
pub(crate) const ROW_H: f32 = 24.0;
pub(crate) const ROW_P: f32 = 26.0;

impl Launcher {
    /// First visible row, so that the selected one is on the canvas.
    pub(crate) fn first(&self) -> usize {
        self.sel.saturating_sub(VISIBLE - 1).min(self.hits.len().saturating_sub(VISIBLE))
    }
}

impl App {
    /// Everything that can be found by name now: apps, then commands.
    fn launcher_items(&self) -> Vec<Item> {
        let mut v: Vec<Item> = apps().into_iter().map(|(n, p)| Item { title: n, act: Act::Open(p), sub: tr("App") }).collect();
        let cmd = |t: &str, c: Cmd| Item { title: t.to_string(), act: Act::Cmd(c), sub: tr("Command") };
        v.push(cmd(tr("Settings"), Cmd::Settings));
        v.push(cmd(tr("Do Not Disturb"), Cmd::Dnd));
        v.push(cmd(tr("Mute microphone"), Cmd::Mic));
        v.push(cmd(tr("Check for updates"), Cmd::Update));
        for s in &self.order {
            if !matches!(s, SlideId::Ext(_)) {
                v.push(cmd(s.label(), Cmd::Slide(*s)));
            }
        }
        v
    }

    fn launcher_refresh(&mut self) {
        let Some(q) = self.launcher.as_ref().map(|l| l.field.text()) else { return };
        let hits = search(&q, &self.launcher_items(), &self.launcher_freq, tr("Search the web for {}"), tr("Open"), tr("Copy"), tr("Web"));
        if let Some(l) = self.launcher.as_mut() {
            l.hits = hits;
            l.sel = 0;
        }
    }

    pub(crate) fn launcher_open_now(&mut self) -> bool {
        scan_async();
        let prev_fg = if cfg!(test) { 0 } else { unsafe { GetForegroundWindow() }.0 as isize };
        self.launcher = Some(Launcher { field: TextField::new("", MAX_QUERY), hits: Vec::new(), sel: 0, prev_fg });
        self.launcher_refresh();
        if !cfg!(test) {
            unsafe {
                let hwnd = HWND(self.hwnd as *mut c_void);
                let _ = SetForegroundWindow(hwnd);
                let _ = SetFocus(hwnd);
            }
        }
        true
    }

    /// Close it. `restore`: give the keyboard back to the window that had it (Esc, the hotkey again).
    pub(crate) fn launcher_close(&mut self, restore: bool) {
        let Some(l) = self.launcher.take() else { return };
        if restore && !cfg!(test) && l.prev_fg != 0 && l.prev_fg != self.hwnd {
            unsafe {
                let _ = SetForegroundWindow(HWND(l.prev_fg as *mut c_void));
            }
        }
    }

    /// The hotkey: open, or close if it is already open.
    pub(crate) fn launcher_toggle(&mut self) {
        if !self.cfg.launcher {
            return;
        }
        if self.launcher.is_some() {
            self.launcher_close(true);
        } else {
            self.launcher_open_now();
        }
    }

    /// A typed character. Enter runs the selected result, Esc closes.
    pub(crate) fn launcher_char(&mut self, c: u32) {
        match c {
            27 => self.launcher_close(true),
            13 => self.launcher_run(),
            8 => {
                if let Some(l) = self.launcher.as_mut() {
                    l.field.backspace();
                }
                self.launcher_refresh();
            }
            c if c >= 32 && c != 127 => {
                let Some(ch) = char::from_u32(c) else { return };
                if self.launcher.as_mut().is_some_and(|l| l.field.insert(ch)) {
                    self.launcher_refresh();
                }
            }
            _ => {}
        }
    }

    /// Arrows, Home, End, Delete, Tab. True if the key was used.
    pub(crate) fn launcher_key(&mut self, vk: u32) -> bool {
        let Some(l) = self.launcher.as_mut() else { return false };
        let last = l.hits.len().saturating_sub(1);
        match vk {
            0x26 => l.sel = l.sel.saturating_sub(1),
            0x28 | 0x09 => l.sel = (l.sel + 1).min(last),
            0x25 => l.field.left(),
            0x27 => l.field.right(),
            0x24 => l.field.home(),
            0x23 => l.field.end(),
            0x2E => {
                l.field.delete();
                self.launcher_refresh();
            }
            _ => return false,
        }
        true
    }

    /// A click: on a result it runs it.
    pub(crate) fn launcher_click(&mut self, x: f32, y: f32) {
        let Some(l) = self.launcher.as_mut() else { return };
        let first = l.first();
        for vi in 0..VISIBLE.min(l.hits.len().saturating_sub(first)) {
            if in_rect(x, y, LX, ROW_Y + vi as f32 * ROW_P, CW, ROW_H) {
                l.sel = first + vi;
                self.launcher_run();
                return;
            }
        }
    }

    /// Do what the selected result says, remember it, and close.
    fn launcher_run(&mut self) {
        let Some(hit) = self.launcher.as_ref().and_then(|l| l.hits.get(l.sel).cloned()) else { return };
        let used = matches!(hit.act, Act::Open(_) | Act::Cmd(_));
        if used {
            self.launcher_freq.record(&hit.title, notif::now_unix() as u64);
            self.launcher_freq.save();
        }
        self.launcher_close(false);
        match hit.act {
            Act::Open(p) => open_target(&p),
            Act::Url(u) => sys::open_url(&u),
            Act::Copy(t) => copy_text(self.hwnd, &t),
            Act::Cmd(Cmd::Settings) => self.open_settings_window(),
            Act::Cmd(Cmd::Dnd) => self.dnd_hotkey(),
            Act::Cmd(Cmd::Mic) => self.mic_hotkey(),
            Act::Cmd(Cmd::Update) => self.update_check_now(),
            Act::Cmd(Cmd::Slide(s)) => {
                self.show_id(s);
                self.peek_until = Some(Instant::now() + Duration::from_secs(8));
            }
        }
    }

    /// Draw the input and the results in place of the slides.
    pub(crate) fn draw_launcher(&self, g: &mut Gfx, al: f32) {
        let Some(l) = self.launcher.as_ref() else { return };
        let cur = self.cursor_canvas();
        g.fill(&rrect(LX, IN_Y, CW, IN_H, 12.0), &paint(255, 255, 255, 0.12 * al));
        let text = l.field.text();
        let tx = LX + 14.0;
        if text.is_empty() {
            g.txt(tr("Search apps, commands, math"), tx, IN_Y + ty(IN_H, 14.0), 14.0, TEXT_FAINT, al, CW - 28.0);
            g.fill(&rrect(tx, IN_Y + 8.0, 1.5, IN_H - 16.0, 0.7), &g.sacc(al));
        } else {
            // long text scrolls: show its end
            let mut shown = text.clone();
            while shown.chars().count() > 1 && g.width(&shown, 14.0) > CW - 34.0 {
                shown.remove(0);
            }
            g.txt(&shown, tx, IN_Y + ty(IN_H, 14.0), 14.0, [255, 255, 255], al, CW - 28.0);
            let before = l.field.before_cursor();
            let cut = before.chars().count().min(shown.chars().count());
            let head: String = shown.chars().take(cut).collect();
            let cx = tx + g.width(&head, 14.0);
            g.fill(&rrect(cx, IN_Y + 8.0, 1.5, IN_H - 16.0, 0.7), &g.sacc(al));
        }
        if l.hits.is_empty() {
            g.txt_c(tr("Type to search"), LX + CW / 2.0, ROW_Y + 30.0, 12.0, TEXT_FAINT, al, CW, false);
            return;
        }
        let first = l.first();
        for (vi, h) in l.hits.iter().skip(first).take(VISIBLE).enumerate() {
            let i = first + vi;
            let y = ROW_Y + vi as f32 * ROW_P;
            let hover = cur.is_some_and(|(hx, hy)| in_rect(hx, hy, LX, y, CW, ROW_H));
            let bg = if i == l.sel {
                g.sacc(0.28 * al)
            } else {
                paint(255, 255, 255, if hover { 0.13 } else { 0.07 } * al)
            };
            g.fill(&rrect(LX, y, CW, ROW_H, 9.0), &bg);
            let sw = g.width(h.sub, 11.0);
            g.txt(h.sub, LX + CW - 10.0 - sw, y + ty(ROW_H, 11.0), 11.0, if i == l.sel { g.acc.bright } else { TEXT_FAINT }, al, 80.0);
            let title = clipboard::row_text(g, &h.title, CW - 34.0 - sw);
            g.txt(&title, LX + 10.0, y + ty(ROW_H, 13.0), 13.0, [242, 242, 242], al, CW - 30.0 - sw);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(t: &str) -> Item {
        Item { title: t.into(), act: Act::Open(format!("{t}.lnk")), sub: "App" }
    }

    #[test]
    fn match_kinds_rank_in_order() {
        assert!(score("code", "Code") > score("code", "Code Editor"));
        assert!(score("code", "Code Editor") > score("code", "Visual Studio Code"));
        assert!(score("code", "Visual Studio Code") > score("code", "Barcode Reader"));
        assert!(score("code", "Barcode Reader") > score("vsc", "Visual Studio Code"), "inside beats letters in order");
        assert_eq!(score("vsc", "Visual Studio Code"), Some(200));
        assert_eq!(score("xyz", "Visual Studio Code"), None);
    }

    #[test]
    fn search_orders_by_match_then_by_use() {
        let items = [item("Notepad"), item("Notes"), item("Another Note")];
        let none = Frecency::default();
        let h = search("note", &items, &none, "Search {}", "Open", "Copy", "Web");
        let titles: Vec<_> = h.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(titles, ["Notes", "Notepad", "Another Note", "Search note"], "shorter first among prefixes, the web row last");
        let mut used = Frecency::default();
        used.record("Notepad", 10);
        let h = search("note", &items, &used, "Search {}", "Open", "Copy", "Web");
        assert_eq!(h[0].title, "Notepad", "what is opened often comes first");
    }

    #[test]
    fn empty_query_lists_what_was_used() {
        let items = [item("A"), item("B"), item("C")];
        assert!(search("", &items, &Frecency::default(), "S {}", "O", "C", "W").is_empty());
        let mut f = Frecency::default();
        f.record("C", 1);
        f.record("C", 2);
        f.record("A", 3);
        let h = search("  ", &items, &f, "S {}", "O", "C", "W");
        assert_eq!(h.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), ["C", "A"]);
    }

    #[test]
    fn the_web_row_is_always_there_even_with_many_matches() {
        let items: Vec<Item> = (0..30).map(|i| item(&format!("App {i}"))).collect();
        let h = search("app", &items, &Frecency::default(), "Search {}", "Open", "Copy", "Web");
        assert_eq!(h.len(), MAX_HITS);
        assert_eq!(h.last().unwrap().act, Act::Url("https://www.google.com/search?q=app".into()));
    }

    #[test]
    fn web_text_is_encoded_and_addresses_are_opened_as_they_are() {
        assert_eq!(url_encode("a b&c=é"), "a+b%26c%3D%C3%A9");
        assert_eq!(as_url("example.com/x?y=1").as_deref(), Some("https://example.com/x?y=1"));
        assert_eq!(as_url("https://a.b").as_deref(), Some("https://a.b"));
        assert_eq!(as_url("hello world.com"), None);
        assert_eq!(as_url("notepad"), None);
        assert_eq!(as_url("v1.2"), Some("https://v1.2".into()), "a dot inside is enough");
        assert_eq!(as_url("http://"), None);
        assert_eq!(as_url(".com"), None);
        assert_eq!(as_url("a b.c"), None);
        let h = search("example.com", &[], &Frecency::default(), "Search {}", "Open", "Copy", "Web");
        assert_eq!(h[0].act, Act::Url("https://example.com".into()));
        assert_eq!(h[0].title, "Open example.com");
    }

    #[test]
    fn calculator() {
        assert_eq!(calc("2+3*4").as_deref(), Some("14"));
        assert_eq!(calc("(2+3)*4").as_deref(), Some("20"));
        assert_eq!(calc("10/4").as_deref(), Some("2.5"));
        assert_eq!(calc("2^3^2").as_deref(), Some("512"), "power goes right to left");
        assert_eq!(calc("-3 + 5").as_deref(), Some("2"));
        assert_eq!(calc("7 % 4").as_deref(), Some("3"));
        assert_eq!(calc("1,5*2").as_deref(), Some("3"), "a comma is a decimal point");
        assert_eq!(calc("0.1+0.2").as_deref(), Some("0.3"));
        assert_eq!(calc("1/3").as_deref(), Some("0.3333333333"));
        assert_eq!(calc("2*-3").as_deref(), Some("-6"));
    }

    #[test]
    fn calculator_refuses_what_is_not_a_calculation() {
        for bad in ["", "5", "-5", "abc", "2+", "(2+3", "2+3)", "1/0", "5%0", "2**3", "1e5+1", "calc 1+1", "()", "2 3"] {
            assert_eq!(calc(bad), None, "{bad:?}");
        }
        assert_eq!(calc(&"1+".repeat(60)), None, "too long");
        assert_eq!(calc(&format!("{}1{}", "(".repeat(40), ")".repeat(40))), None, "too deep");
        assert_eq!(calc(&format!("{}2", "-".repeat(50))), None, "too many signs");
        assert_eq!(calc("9^9^9^9"), None, "a result that is not a finite number is refused");
    }

    #[test]
    fn a_calculation_is_the_first_result() {
        let h = search("2+2", &[item("2+2 game")], &Frecency::default(), "Search {}", "Open", "Copy", "Web");
        assert_eq!(h[0].title, "= 4");
        assert_eq!(h[0].act, Act::Copy("4".into()));
        assert!(h.len() >= 2);
    }

    #[test]
    fn frecency_roundtrips_and_ignores_broken_lines() {
        let mut f = Frecency::default();
        f.record("Notepad", 100);
        f.record("Notepad", 200);
        f.record("Calc", 300);
        let back = Frecency::parse(&f.serialize());
        assert_eq!(back, f);
        assert_eq!(back.count("notepad"), 2);
        let odd = Frecency::parse("x\ty\tz\n3\t4\n\n5\t6\tName with\ttab\n");
        assert_eq!(odd.count("name with\ttab"), 5);
        assert_eq!(odd.0.len(), 1);
    }

    #[test]
    fn frecency_stays_bounded() {
        let mut f = Frecency::default();
        for i in 0..(MAX_FREQ + 50) {
            f.record(&format!("app {i}"), i as u64);
        }
        assert!(f.0.len() <= MAX_FREQ);
    }

    #[test]
    fn shortcut_names_are_unique_and_without_uninstallers() {
        let files: Vec<std::path::PathBuf> = ["C:\\a\\Notepad.lnk", "C:\\b\\notepad.lnk", "C:\\a\\Uninstall Foo.lnk", "C:\\a\\Readme.lnk", "C:\\a\\Paint.lnk"]
            .iter()
            .map(std::path::PathBuf::from)
            .collect();
        let v = names_of(&files);
        assert_eq!(v.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["Notepad", "Paint"]);
    }
}

//! Quick launcher (Ctrl+Alt+Space): a window in the middle of the screen with a line to type in. Type a few letters, pick with the arrows, Enter
//! opens, Esc closes. Results: installed apps (Start Menu shortcuts), notch commands, a calculator and a web search.
//!
//! The ranking, the usage memory (frecency) and the calculator are plain functions without any window, so they are
//! tested directly. The window (keys, drawing, focus) is at the end.

use super::*;
use crate::slides::SlideId;
use crate::textfield::TextField;
use std::collections::HashMap;

/// Most results kept for one query, and how many fit on the canvas at once.
const MAX_HITS: usize = 8;
pub(crate) const VISIBLE: usize = 6;
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

fn app_icon(path: &str) -> Option<Pixmap> {
    if cfg!(test) {
        let mut pm = Pixmap::new(48, 48)?;
        pm.fill(Color::from_rgba8(230, 120, 60, 255));
        return Some(pm);
    }
    shelf::load_file_icon(path)
}

/// Points of an arc of the circle (cx, cy, r) from angle `from` to `to`, going the way that passes `through`.
fn arc_pts(cx: f32, cy: f32, r: f32, from: f32, to: f32, through: f32, n: usize) -> Vec<(f32, f32)> {
    let tau = std::f32::consts::TAU;
    let ccw = (to - from).rem_euclid(tau);
    let sweep = if (through - from).rem_euclid(tau) <= ccw { ccw } else { ccw - tau };
    (0..=n).map(|i| from + sweep * i as f32 / n as f32).map(|a| (cx + r * a.cos(), cy + r * a.sin())).collect()
}

/// A crescent moon as one shape (a circle with a bite out of it), so it needs no background color behind it.
fn crescent(g: &mut Gfx, cx: f32, cy: f32, s: f32, p: &Paint) {
    let (ra, rb) = (s * 0.42, s * 0.36);
    let (bx, by) = (s * 0.22, -s * 0.12);
    let d = (bx * bx + by * by).sqrt();
    let (ux, uy) = (bx / d, by / d);
    let a = (ra * ra - rb * rb + d * d) / (2.0 * d);
    let h = (ra * ra - a * a).max(0.0).sqrt();
    // the two points where the circles cross
    let (p1, p2) = ((a * ux - h * uy, a * uy + h * ux), (a * ux + h * uy, a * uy - h * ux));
    let toward_a = (-uy).atan2(-ux); // the side of A away from the bite, and of B facing A
    let mut pts = arc_pts(cx, cy, ra, p1.1.atan2(p1.0), p2.1.atan2(p2.0), toward_a, 24);
    let inner = arc_pts(cx + bx, cy + by, rb, (p2.1 - by).atan2(p2.0 - bx), (p1.1 - by).atan2(p1.0 - bx), toward_a, 24);
    pts.extend(inner);
    g.poly(&pts, p);
}

/// The small picture of a result that is not an app: what it does, drawn with lines.
fn row_icon(g: &mut Gfx, act: &Act, cx: f32, cy: f32, s: f32, p: &Paint) {
    let lw = (s * 0.14).max(1.3);
    match act {
        Act::Cmd(Cmd::Settings) => g.icon_gear(cx, cy, s, p),
        Act::Cmd(Cmd::Dnd) => crescent(g, cx, cy, s, p),
        Act::Cmd(Cmd::Mic) => g.icon_mic_off(cx, cy, s, p),
        Act::Cmd(Cmd::Slide(id)) => g.slide_icon(*id, cx, cy, s, p),
        Act::Cmd(Cmd::Update) => {
            g.line(cx, cy - s * 0.45, cx, cy + s * 0.2, lw, p);
            g.line(cx - s * 0.3, cy - s * 0.1, cx, cy + s * 0.22, lw, p);
            g.line(cx + s * 0.3, cy - s * 0.1, cx, cy + s * 0.22, lw, p);
            g.line(cx - s * 0.4, cy + s * 0.5, cx + s * 0.4, cy + s * 0.5, lw, p);
        }
        // an equals sign
        Act::Copy(_) => {
            g.line(cx - s * 0.4, cy - s * 0.17, cx + s * 0.4, cy - s * 0.17, lw, p);
            g.line(cx - s * 0.4, cy + s * 0.17, cx + s * 0.4, cy + s * 0.17, lw, p);
        }
        // an arrow going out: open in the browser
        Act::Url(_) | Act::Open(_) => {
            g.line(cx - s * 0.4, cy + s * 0.4, cx + s * 0.4, cy - s * 0.4, lw, p);
            g.line(cx - s * 0.05, cy - s * 0.4, cx + s * 0.4, cy - s * 0.4, lw, p);
            g.line(cx + s * 0.4, cy - s * 0.4, cx + s * 0.4, cy + s * 0.05, lw, p);
        }
    }
}

// ---------------------------------------------------------------- the open launcher
/// Which results the chips under the input keep.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Filter {
    All,
    Apps,
    Commands,
    /// Calculations and web searches.
    Web,
}

impl Filter {
    pub(crate) const EACH: [Filter; 4] = [Filter::All, Filter::Apps, Filter::Commands, Filter::Web];

    fn label(self) -> &'static str {
        match self {
            Filter::All => tr("All"),
            Filter::Apps => tr("Apps"),
            Filter::Commands => tr("Commands"),
            Filter::Web => tr("Web"),
        }
    }

    pub(crate) fn keeps(self, act: &Act) -> bool {
        matches!(
            (self, act),
            (Filter::All, _) | (Filter::Apps, Act::Open(_)) | (Filter::Commands, Act::Cmd(_)) | (Filter::Web, Act::Url(_) | Act::Copy(_))
        )
    }

    /// The next chip (or the one before).
    fn step(self, back: bool) -> Filter {
        let i = Filter::EACH.iter().position(|f| *f == self).unwrap_or(0);
        Filter::EACH[(i + if back { Filter::EACH.len() - 1 } else { 1 }) % Filter::EACH.len()]
    }
}

pub(crate) struct Launcher {
    pub(crate) field: TextField,
    pub(crate) hits: Vec<Hit>,
    pub(crate) sel: usize,
    /// The window that had the keyboard before, to give it back on Esc.
    pub(crate) prev_fg: isize,
    pub(crate) filter: Filter,
    /// The row under the pointer.
    pub(crate) hover: Option<usize>,
    pub(crate) hwnd: isize,
    pub(crate) gfx: Option<Gfx>,
    buf: Vec<u8>,
    pub(crate) scale: f32,
    /// Logical height of the window now (it grows with the results).
    h: f32,
    /// (x, width) of each filter chip, from the last drawing.
    chips: Vec<(f32, f32)>,
    tracking: bool,
}

/// Window layout, in logical px.
pub(crate) const WIN_W: f32 = 640.0;
const PAD: f32 = 14.0;
pub(crate) const BAR_H: f32 = 64.0;
pub(crate) const CHIP_Y: f32 = 74.0;
const CHIP_H: f32 = 26.0;
pub(crate) const ROWS_Y: f32 = 112.0;
pub(crate) const ROW_H: f32 = 48.0;
pub(crate) const ROW_P: f32 = 52.0;

fn height_for(chips: bool, rows: usize) -> f32 {
    if chips {
        ROWS_Y + rows.clamp(1, VISIBLE) as f32 * ROW_P + 8.0
    } else {
        BAR_H
    }
}

fn row_rect(vi: usize) -> (f32, f32, f32, f32) {
    (PAD, ROWS_Y + vi as f32 * ROW_P, WIN_W - 2.0 * PAD, ROW_H)
}

impl Launcher {
    /// First visible row, so that the selected one is in the window.
    pub(crate) fn first(&self) -> usize {
        self.sel.saturating_sub(VISIBLE - 1).min(self.hits.len().saturating_sub(VISIBLE))
    }

    #[cfg(test)]
    pub(crate) fn chip_rects(&self) -> &[(f32, f32)] {
        &self.chips
    }

    /// Chips and results show once there is something typed or something to offer.
    fn wants_chips(&self) -> bool {
        !self.hits.is_empty() || !self.field.text().trim().is_empty()
    }
}

impl App {
    /// Everything that can be found by name now: apps, then commands.
    fn launcher_items(&self) -> Vec<Item> {
        let mut v: Vec<Item> = apps().into_iter().map(|(n, p)| Item { title: n, act: Act::Open(p), sub: tr("Application") }).collect();
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
        let Some((q, filter)) = self.launcher.as_ref().map(|l| (l.field.text(), l.filter)) else { return };
        let mut items = self.launcher_items();
        items.retain(|i| filter.keeps(&i.act));
        let mut hits = search(&q, &items, &self.launcher_freq, tr("Search the web for {}"), tr("Open"), tr("Calculator"), tr("Web search"));
        hits.retain(|h| filter.keeps(&h.act));
        // icons of the apps in the list: read once from Windows (a plain colored tile in tests)
        for h in &hits {
            if let Act::Open(p) = &h.act {
                self.launcher_icons.entry(p.clone()).or_insert_with(|| app_icon(p));
            }
        }
        if let Some(l) = self.launcher.as_mut() {
            l.hits = hits;
            l.sel = 0;
            l.hover = None;
        }
        self.launcher_fit();
    }

    /// Make the window as tall as its content needs, and draw it again.
    fn launcher_fit(&mut self) {
        let Some(l) = self.launcher.as_mut() else { return };
        let h = height_for(l.wants_chips(), l.hits.len());
        if (h - l.h).abs() > 0.5 {
            l.h = h;
            l.gfx = None;
            if l.hwnd != 0 {
                unsafe {
                    let _ = SetWindowPos(
                        HWND(l.hwnd as *mut c_void),
                        None,
                        0,
                        0,
                        (WIN_W * l.scale).round() as i32,
                        (h * l.scale).round() as i32,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
        }
        self.launcher_redraw();
    }

    fn launcher_redraw(&self) {
        if let Some(l) = self.launcher.as_ref().filter(|l| l.hwnd != 0) {
            unsafe {
                let _ = InvalidateRect(HWND(l.hwnd as *mut c_void), None, false);
            }
        }
    }

    pub(crate) fn launcher_open_now(&mut self) -> bool {
        scan_async();
        let prev_fg = if cfg!(test) { 0 } else { unsafe { GetForegroundWindow() }.0 as isize };
        let dpi = if cfg!(test) { 0 } else { monitors::dpi_of(&self.mon) };
        let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
        self.launcher = Some(Launcher {
            field: TextField::new("", MAX_QUERY),
            hits: Vec::new(),
            sel: 0,
            prev_fg,
            filter: Filter::All,
            hover: None,
            hwnd: 0,
            gfx: None,
            buf: Vec::new(),
            scale,
            h: 0.0,
            chips: Vec::new(),
            tracking: false,
        });
        self.launcher_refresh();
        if !cfg!(test) {
            self.launcher_create();
        }
        true
    }

    /// The window: centered on the notch's monitor, a little above the middle.
    fn launcher_create(&mut self) {
        let Some(l) = self.launcher.as_mut() else { return };
        unsafe {
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let work = if GetMonitorInfoW(MonitorFromRect(&self.mon, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() { mi.rcWork } else { self.mon };
            let (w, h) = ((WIN_W * l.scale).round() as i32, (l.h * l.scale).round() as i32);
            let x = work.left + (work.right - work.left - w) / 2;
            let y = work.top + ((work.bottom - work.top) as f32 * 0.2) as i32;
            let Ok(hinst) = GetModuleHandleW(None) else { return };
            register_class(hinst.into());
            let Ok(hwnd) = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("HesewakeLauncher"),
                w!("Hesewake Launcher"),
                // WS_CAPTION keeps the shadow; the caption itself is removed in WM_NCCALCSIZE (as in the Settings window)
                WS_POPUP | WS_CAPTION,
                x,
                y,
                w,
                h,
                None,
                None,
                hinst,
                None,
            ) else {
                return;
            };
            use windows::Win32::Graphics::Dwm::{
                DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWM_WINDOW_CORNER_PREFERENCE,
            };
            let round = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &round as *const _ as *const c_void,
                std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );
            let dark: i32 = 1;
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as *const c_void, 4);
            l.hwnd = hwnd.0 as isize;
            let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(hwnd);
        }
    }

    /// Close it. `restore`: give the keyboard back to the window that had it (Esc, the hotkey again).
    pub(crate) fn launcher_close(&mut self, restore: bool) {
        let Some(l) = self.launcher.take() else { return };
        unsafe {
            if l.hwnd != 0 {
                // asked, not destroyed here: this may run inside the window's own message
                let _ = PostMessageW(HWND(l.hwnd as *mut c_void), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            if restore && !cfg!(test) && l.prev_fg != 0 && l.prev_fg != self.hwnd {
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
            // Ctrl+Backspace arrives as 127
            127 => {
                if let Some(l) = self.launcher.as_mut() {
                    l.field.backspace_word();
                }
                self.launcher_refresh();
            }
            c if c >= 32 => {
                let Some(ch) = char::from_u32(c) else { return };
                if self.launcher.as_mut().is_some_and(|l| l.field.insert(ch)) {
                    self.launcher_refresh();
                }
            }
            _ => {}
        }
    }

    /// Arrows, Home, End, Delete, Tab (with Ctrl: by word; Tab moves between the chips). True if the key was used.
    pub(crate) fn launcher_key(&mut self, vk: u32, ctrl: bool, shift: bool) -> bool {
        let Some(l) = self.launcher.as_mut() else { return false };
        let last = l.hits.len().saturating_sub(1);
        match vk {
            0x26 => l.sel = l.sel.saturating_sub(1),
            0x28 => l.sel = (l.sel + 1).min(last),
            0x09 => {
                l.filter = l.filter.step(shift);
                self.launcher_refresh();
                return true;
            }
            0x25 if ctrl => l.field.word_left(),
            0x27 if ctrl => l.field.word_right(),
            0x25 => l.field.left(),
            0x27 => l.field.right(),
            0x24 => l.field.home(),
            0x23 => l.field.end(),
            0x2E => {
                if ctrl {
                    l.field.delete_word();
                } else {
                    l.field.delete();
                }
                self.launcher_refresh();
            }
            _ => return false,
        }
        self.launcher_redraw();
        true
    }

    /// The wheel moves the selection.
    pub(crate) fn launcher_wheel(&mut self, delta: i32) {
        if let Some(l) = self.launcher.as_mut() {
            let last = l.hits.len().saturating_sub(1);
            l.sel = if delta > 0 { l.sel.saturating_sub(1) } else { (l.sel + 1).min(last) };
        }
        self.launcher_redraw();
    }

    /// Which row is at (x, y) in the window?
    fn launcher_row_at(&self, x: f32, y: f32) -> Option<usize> {
        let l = self.launcher.as_ref()?;
        let first = l.first();
        (0..VISIBLE.min(l.hits.len().saturating_sub(first))).find(|&vi| {
            let r = row_rect(vi);
            in_rect(x, y, r.0, r.1, r.2, r.3)
        })
        .map(|vi| first + vi)
    }

    fn launcher_chip_at(&self, x: f32, y: f32) -> Option<usize> {
        let l = self.launcher.as_ref()?;
        l.chips.iter().position(|&(cx, cw)| in_rect(x, y, cx, CHIP_Y, cw, CHIP_H))
    }

    /// The pointer moved over (x, y): true if the highlighted row changed.
    pub(crate) fn launcher_hover(&mut self, x: f32, y: f32) -> bool {
        let row = self.launcher_row_at(x, y);
        match self.launcher.as_mut() {
            Some(l) if l.hover != row => {
                l.hover = row;
                true
            }
            _ => false,
        }
    }

    /// 0 = arrow, 1 = hand (something to click), 2 = I-beam (the input).
    pub(crate) fn launcher_cursor(&self, x: f32, y: f32) -> u8 {
        if self.launcher_row_at(x, y).is_some() || self.launcher_chip_at(x, y).is_some() {
            1
        } else if y < BAR_H {
            2
        } else {
            0
        }
    }

    /// A click: on a chip it filters, on a result it runs it.
    pub(crate) fn launcher_click(&mut self, x: f32, y: f32) {
        if let Some(i) = self.launcher_chip_at(x, y) {
            if let Some(l) = self.launcher.as_mut() {
                l.filter = Filter::EACH[i];
            }
            self.launcher_refresh();
            return;
        }
        if let Some(i) = self.launcher_row_at(x, y) {
            if let Some(l) = self.launcher.as_mut() {
                l.sel = i;
            }
            self.launcher_run();
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
}

// ---------------------------------------------------------------- drawing
const BG: [u8; 3] = [27, 28, 34];

/// Draw the launcher into its canvas (`Launcher::gfx`) and remember where the chips are.
pub(crate) fn launcher_render(a: &mut App) {
    let acc = accent::get(a.cfg.accent);
    let Some(l) = a.launcher.as_mut() else { return };
    let h = if l.h > 0.0 { l.h } else { BAR_H };
    let mut g = match l.gfx.take() {
        Some(g) if g.s == l.scale && g.pm.height() == (h * l.scale).ceil() as u32 => g,
        _ => Gfx::sized(l.scale, WIN_W, h),
    };
    g.acc = acc;
    g.pm.fill(Color::from_rgba8(BG[0], BG[1], BG[2], 255));

    // the input: a magnifier, then the text (its end, when it is long) and the caret
    let (mx, my) = (PAD + 22.0, BAR_H / 2.0 - 1.0);
    g.ring(mx, my, 8.0, 0.0, 2.2, &paint(255, 255, 255, 0.75), &paint(0, 0, 0, 0.0));
    g.line(mx + 5.8, my + 5.8, mx + 11.5, my + 11.5, 2.2, &paint(255, 255, 255, 0.75));
    let tx = PAD + 46.0;
    let room = WIN_W - tx - PAD - 8.0;
    let text = l.field.text();
    let ty20 = ty(BAR_H, 20.0);
    let caret_x;
    if text.is_empty() {
        g.txt(tr("Search apps, commands, math"), tx, ty20, 20.0, TEXT_FAINT, 1.0, room);
        caret_x = tx;
    } else {
        let mut shown = text.clone();
        while shown.chars().count() > 1 && g.width(&shown, 20.0) > room {
            shown.remove(0);
        }
        g.txt(&shown, tx, ty20, 20.0, [255, 255, 255], 1.0, room + 8.0);
        let cut = l.field.before_cursor().chars().count().min(shown.chars().count());
        let head: String = shown.chars().take(cut).collect();
        caret_x = tx + g.width(&head, 20.0);
    }
    g.fill(&rrect(caret_x, 18.0, 1.6, BAR_H - 36.0, 0.8), &g.sacc(1.0));

    l.chips.clear();
    if l.wants_chips() {
        g.rect(PAD, BAR_H, WIN_W - 2.0 * PAD, 1.0, &paint(255, 255, 255, 0.08));
        let mut x = PAD;
        for f in Filter::EACH {
            let w = g.width(f.label(), 12.5) + 26.0;
            let on = f == l.filter;
            g.fill(&rrect(x, CHIP_Y, w, CHIP_H, CHIP_H / 2.0), &if on { g.sacc(0.38) } else { paint(255, 255, 255, 0.08) });
            g.txt_c(f.label(), x + w / 2.0, CHIP_Y + ty(CHIP_H, 12.5), 12.5, if on { [255, 255, 255] } else { TEXT_DIM }, 1.0, w, on);
            l.chips.push((x, w));
            x += w + 8.0;
        }
        if l.hits.is_empty() {
            g.txt_c(tr("No results"), WIN_W / 2.0, ROWS_Y + ty(ROW_H, 13.0), 13.0, TEXT_FAINT, 1.0, WIN_W, false);
        }
    }

    let first = l.first();
    for (vi, hit) in l.hits.iter().skip(first).take(VISIBLE).enumerate() {
        let i = first + vi;
        let (rx, ry, rw, rh) = row_rect(vi);
        let selected = i == l.sel;
        if selected {
            g.fill(&rrect(rx, ry, rw, rh, 12.0), &g.sacc(0.26));
        } else if l.hover == Some(i) {
            g.fill(&rrect(rx, ry, rw, rh, 12.0), &paint(255, 255, 255, 0.07));
        }
        // the picture: the app's own icon, or a tile with a drawn one
        let (ix, iy, isz) = (rx + 10.0, ry + (rh - 32.0) / 2.0, 32.0);
        match &hit.act {
            Act::Open(p) if a.launcher_icons.get(p).is_some_and(|i| i.is_some()) => {
                let pm = a.launcher_icons[p].as_ref().unwrap();
                let k = isz / pm.width() as f32;
                let mut pt = Paint::default();
                pt.shader = Pattern::new(pm.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, 1.0, Transform::from_row(k, 0.0, 0.0, k, ix, iy));
                pt.anti_alias = true;
                g.fill(&rrect(ix, iy, isz, isz, 7.0), &pt);
            }
            act => {
                g.fill(&rrect(ix, iy, isz, isz, 9.0), &g.sacc(0.32));
                row_icon(&mut g, act, ix + isz / 2.0, iy + isz / 2.0, 18.0, &paint(255, 255, 255, 1.0));
            }
        }
        // what it does (only on the selected row), then the title and the kind under it
        let mut right = rx + rw - 12.0;
        if selected {
            let word = if matches!(hit.act, Act::Copy(_)) { tr("Copy") } else { tr("Open") };
            let pw = g.width(word, 11.5) + 18.0;
            g.fill(&rrect(right - pw, ry + (rh - 22.0) / 2.0, pw, 22.0, 11.0), &paint(255, 255, 255, 0.16));
            g.txt_c(word, right - pw / 2.0, ry + (rh - 22.0) / 2.0 + ty(22.0, 11.5), 11.5, [255, 255, 255], 1.0, pw, false);
            right -= pw + 10.0;
        }
        let tx = ix + isz + 12.0;
        let maxw = right - tx;
        let mut title = hit.title.replace(['\n', '\r', '\t'], " ");
        if g.width(&title, 15.0) > maxw {
            title = gfx::ellipsize(&mut g, &title, 15.0, maxw);
        }
        g.txt(&title, tx, ry + 6.0, 15.0, [242, 242, 242], 1.0, maxw);
        g.txt(hit.sub, tx, ry + 27.0, 12.0, TEXT_FAINT, 1.0, maxw);
    }
    l.gfx = Some(g);
}

/// Copy the canvas into the window (RGBA -> BGRA; the canvas is opaque).
unsafe fn blit(l: &mut Launcher, hdc: HDC) {
    let Some(g) = l.gfx.as_ref() else { return };
    let (w, h) = (g.pm.width(), g.pm.height());
    l.buf.resize(g.pm.data().len(), 0);
    for (d, s) in l.buf.chunks_exact_mut(4).zip(g.pm.data().chunks_exact(4)) {
        d[0] = s[2];
        d[1] = s[1];
        d[2] = s[0];
        d[3] = 255;
    }
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    SetDIBitsToDevice(hdc, 0, 0, w, h, 0, 0, 0, h, l.buf.as_ptr() as *const c_void, &bmi, DIB_RGB_COLORS);
}

// ---------------------------------------------------------------- window procedure
const WM_MOUSELEAVE: u32 = 0x02A3;

unsafe fn register_class(hinst: HINSTANCE) {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: w!("HesewakeLauncher"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match std::panic::catch_unwind(|| wndproc_inner(hwnd, msg, wp, lp)) {
        Ok(r) => r,
        Err(_) => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Pointer position in the window, in logical px.
fn logical(lp: LPARAM, scale: f32) -> (f32, f32) {
    ((lp.0 & 0xffff) as i16 as f32 / scale, ((lp.0 >> 16) & 0xffff) as i16 as f32 / scale)
}

unsafe fn wndproc_inner(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_CONTROL, VK_SHIFT};
    let me = hwnd.0 as isize;
    // the scale of this window's launcher (1.0 if it is gone)
    let scale_of = || {
        let mut s = 1.0;
        with_app(|a| {
            if let Some(l) = a.launcher.as_ref().filter(|l| l.hwnd == me) {
                s = l.scale;
            }
        });
        s
    };
    match msg {
        WM_NCCALCSIZE if wp.0 != 0 => LRESULT(0),
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            with_app(|a| {
                if a.launcher.as_ref().is_some_and(|l| l.hwnd == me) {
                    launcher_render(a);
                    if let Some(l) = a.launcher.as_mut() {
                        blit(l, hdc);
                    }
                }
            });
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if (lp.0 & 0xffff) as u32 == HTCLIENT {
                let mut p = POINT::default();
                if GetCursorPos(&mut p).is_ok() {
                    let _ = ScreenToClient(hwnd, &mut p);
                    let s = scale_of();
                    let mut kind = 0;
                    with_app(|a| kind = a.launcher_cursor(p.x as f32 / s, p.y as f32 / s));
                    if kind != 0 {
                        if let Ok(c) = LoadCursorW(None, if kind == 2 { IDC_IBEAM } else { IDC_HAND }) {
                            SetCursor(c);
                            return LRESULT(1);
                        }
                    }
                }
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_MOUSEMOVE => {
            let s = scale_of();
            let (x, y) = logical(lp, s);
            with_app(|a| {
                if a.launcher.as_ref().is_some_and(|l| l.hwnd == me && !l.tracking) {
                    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let on = TrackMouseEvent(&mut tme).is_ok();
                    if let Some(l) = a.launcher.as_mut() {
                        l.tracking = on;
                    }
                }
                if a.launcher_hover(x, y) {
                    a.launcher_redraw();
                }
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_app(|a| {
                if let Some(l) = a.launcher.as_mut().filter(|l| l.hwnd == me) {
                    l.tracking = false;
                    if l.hover.take().is_some() {
                        a.launcher_redraw();
                    }
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let s = scale_of();
            let (x, y) = logical(lp, s);
            with_app(|a| {
                a.launcher_click(x, y);
                a.launcher_redraw();
            });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let d = ((wp.0 >> 16) & 0xffff) as i16 as i32;
            with_app(|a| a.launcher_wheel(d));
            LRESULT(0)
        }
        WM_CHAR => {
            let c = wp.0 as u32;
            with_app(|a| {
                if a.launcher.as_ref().is_some_and(|l| l.hwnd == me) {
                    a.launcher_char(c);
                }
            });
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wp.0 as u32;
            let ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
            let shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
            let mut used = false;
            with_app(|a| {
                if a.launcher.as_ref().is_some_and(|l| l.hwnd == me) {
                    used = a.launcher_key(vk, ctrl, shift);
                }
            });
            if used {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        // the keyboard went elsewhere (a click outside, Alt+Tab): there is nothing left to type into
        WM_ACTIVATE => {
            if (wp.0 & 0xffff) == 0 {
                with_app(|a| {
                    if a.launcher.as_ref().is_some_and(|l| l.hwnd == me) {
                        a.launcher_close(false);
                    }
                });
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
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

//! The slide of an extension (docs/extensions.md): draws the page the extension sent, the same way for every extension,
//! and turns clicks into messages back. The extension decides what to show, never how it looks. The hit boxes are
//! noted while drawing (like the Settings window does), so a click always lands on what was drawn.

#![allow(dead_code)] // wired up in the next steps of the extension host

use super::*;
use crate::extproto::{self as proto, Button, DetailMsg, Row, SlideMsg, State, Stepper};
use std::cell::RefCell;

const ROW_P: f32 = 26.0;
const ROW_H: f32 = 24.0;
const Y_HEAD: f32 = 10.0;
const HEAD_H: f32 = 22.0;
const Y_TABS: f32 = 38.0;
const TAB_H: f32 = 20.0;
const Y_ROWS: f32 = 38.0;
const Y_ROWS_TABS: f32 = 62.0;
const Y_FOOT: f32 = 148.0;
const Y_BODY: f32 = 42.0;
const LINE_P: f32 = 15.0;
const Y_BUTTONS: f32 = 118.0;
const BODY_BOTTOM: f32 = 114.0;
/// How long "Copied" stays up.
pub(crate) const COPIED_SECS: f32 = 1.6;

/// Something clickable on the page.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ExtHit {
    Tab(String),
    /// A header action, by position in the message.
    Action(usize),
    Row(String),
    Back,
    Step(i32),
    /// A button of the detail page, by position in the message.
    Button(usize),
    Retry,
    Setup,
}

/// What the notch remembers about the page it shows for one extension.
#[derive(Default)]
pub(crate) struct ExtView {
    /// Where things were drawn the last time (canvas coordinates, no slide offset).
    pub(crate) hits: RefCell<Vec<(f32, f32, f32, f32, ExtHit)>>,
    /// Rows scrolled off the top of the list, and lines scrolled off the top of a detail page.
    pub(crate) scroll: usize,
    pub(crate) dscroll: usize,
    /// The tab the user just clicked, until the extension answers with its own.
    pub(crate) tab_sel: Option<String>,
    pub(crate) copied: Option<Instant>,
    /// The picture of the page's hero, decoded once: what it was made from (a hash of its bytes) and the square.
    pub(crate) hero_pm: RefCell<Option<(u64, Pixmap)>>,
}

enum Item<'a> {
    Head(&'a str),
    Row(&'a Row),
}

fn items(s: &SlideMsg) -> Vec<Item<'_>> {
    let mut v = Vec::new();
    for sec in &s.sections {
        if !sec.title.is_empty() {
            v.push(Item::Head(&sec.title));
        }
        v.extend(sec.rows.iter().map(Item::Row));
    }
    v
}

/// How many list items fit under the header (one fewer line of them when there are tabs).
/// The rows of a page in the order they are drawn, sections one after the other.
pub(crate) fn rows_of(s: &SlideMsg) -> Vec<&Row> {
    s.sections.iter().flat_map(|sec| sec.rows.iter()).collect()
}

fn visible_items(tabs: bool) -> usize {
    if tabs {
        3
    } else {
        4
    }
}

fn scrollbar(g: &mut Gfx, ox: f32, y0: f32, shown: usize, total: usize, first: usize, al: f32) {
    if total <= shown {
        return;
    }
    let th = shown as f32 * ROW_P - (ROW_P - ROW_H);
    let thumb = (th * shown as f32 / total as f32).max(14.0);
    let ty0 = y0 + (th - thumb) * first as f32 / (total - shown) as f32;
    g.fill(&rrect(RX - 4.0 + ox, y0, 3.0, th, 1.5), &paint(255, 255, 255, 0.10 * al));
    g.fill(&rrect(RX - 4.0 + ox, ty0, 3.0, thumb, 1.5), &paint(255, 255, 255, 0.55 * al));
}

/// Where the page puts a hit box, if it is being drawn at rest (not while sliding in or out).
struct Hits<'a> {
    list: &'a RefCell<Vec<(f32, f32, f32, f32, ExtHit)>>,
    record: bool,
}

impl Hits<'_> {
    fn add(&self, r: (f32, f32, f32, f32), h: ExtHit) {
        if self.record {
            self.list.borrow_mut().push((r.0, r.1, r.2, r.3, h));
        }
    }
}

fn over(cur: Option<(f32, f32)>, r: (f32, f32, f32, f32)) -> bool {
    cur.is_some_and(|(cx, cy)| in_rect(cx, cy, r.0, r.1, r.2, r.3))
}

/// A small button: a pill with a centered label.
fn button(g: &mut Gfx, hits: &Hits, cur: Option<(f32, f32)>, r: (f32, f32, f32, f32), label: &str, hit: Option<ExtHit>, ox: f32, al: f32, strong: bool) {
    let hot = hit.is_some() && over(cur, (r.0, r.1, r.2, r.3));
    let a = if hit.is_none() { 0.05 } else if hot { 0.22 } else { 0.12 };
    if strong && hit.is_some() {
        let c = g.acc.deep;
        g.pill(r.0 + ox, r.1, r.2, r.3, c, if hot { 1.0 } else { 0.85 } * al, 1.0);
    } else {
        g.pill(r.0 + ox, r.1, r.2, r.3, [255, 255, 255], a * al, 1.0);
    }
    let col = if hit.is_some() { [255, 255, 255] } else { TEXT_FAINT };
    g.txt_c(label, r.0 + ox + r.2 / 2.0, r.1 + ty(r.3, 12.0), 12.0, col, al, r.2 - 4.0, true);
    if let Some(h) = hit {
        hits.add(r, h);
    }
}

/// The header: Back, the title (two colours at a `|`), the stepper and the actions. Returns nothing; the page below
/// starts at a fixed height.
fn header(g: &mut Gfx, hits: &Hits, cur: Option<(f32, f32)>, title: &str, back: bool, stepper: Option<&Stepper>, actions: &[Button], ox: f32, al: f32) {
    let x = LX;
    let mut lx = x;
    if back {
        button(g, hits, cur, (lx, Y_HEAD, 58.0, HEAD_H), tr("Back"), Some(ExtHit::Back), ox, al, false);
        lx += 66.0;
    }
    let mut rx = RX;
    for (i, a) in actions.iter().enumerate().rev() {
        let w = (g.width_ex(&a.label, 12.0, true) + 22.0).max(46.0);
        button(g, hits, cur, (rx - w, Y_HEAD, w, HEAD_H), &a.label, Some(ExtHit::Action(i)), ox, al, false);
        rx -= w + 6.0;
    }
    if let Some(st) = stepper {
        let lw = g.width_ex(&st.label, 12.0, true);
        let (next, prev) = ((rx - 24.0, Y_HEAD, 24.0, HEAD_H), (rx - 24.0 - lw - 18.0, Y_HEAD, 24.0, HEAD_H));
        button(g, hits, cur, next, "\u{203a}", st.next.then_some(ExtHit::Step(1)), ox, al, false);
        g.txt_c(&st.label, rx - 24.0 - 9.0 - lw / 2.0 + ox, Y_HEAD + ty(HEAD_H, 12.0), 12.0, [235, 235, 235], al, lw + 4.0, true);
        button(g, hits, cur, prev, "\u{2039}", st.prev.then_some(ExtHit::Step(-1)), ox, al, false);
        rx = prev.0 - 8.0;
    }
    let maxw = (rx - lx - 8.0).max(40.0);
    let (t1, t2) = title.split_once('|').unwrap_or((title, ""));
    let y = 12.0;
    let t1 = if g.width_ex(t1, 15.0, true) > maxw { ellipsize(g, t1, 15.0, maxw) } else { t1.to_string() };
    g.txtb(&t1, lx + ox, y, 15.0, [255, 255, 255], al, maxw);
    if !t2.is_empty() {
        let w1 = g.width_ex(&t1, 15.0, true);
        if w1 < maxw {
            let bright = g.acc.bright;
            g.txtb(t2, lx + w1 + ox, y, 15.0, bright, al, maxw - w1);
        }
    }
}

/// A message in the middle of the page, and an optional button under it.
fn centered(g: &mut Gfx, hits: &Hits, cur: Option<(f32, f32)>, line: &str, sub: &str, btn: Option<(&str, ExtHit)>, ox: f32, al: f32) {
    let cx = LX + CW / 2.0 + ox;
    let shown = if g.width(line, 13.0) > CW { ellipsize(g, line, 13.0, CW) } else { line.to_string() };
    g.txt_c(&shown, cx, 66.0, 13.0, TEXT_DIM, al, CW, false);
    if !sub.is_empty() {
        let sub = if g.width(sub, 11.0) > CW { ellipsize(g, sub, 11.0, CW) } else { sub.to_string() };
        g.txt_c(&sub, cx, 86.0, 11.0, TEXT_FAINT, al, CW, false);
    }
    if let Some((label, hit)) = btn {
        let w = (g.width_ex(label, 12.0, true) + 28.0).max(80.0);
        button(g, hits, cur, (LX + (CW - w) / 2.0, 106.0, w, 24.0), label, Some(hit), ox, al, true);
    }
}

/// The lines of a detail page, in order, with their look.
struct Ln {
    text: String,
    size: f32,
    col: [u8; 3],
    bold: bool,
    /// Space left before this line.
    gap: f32,
}

fn detail_lines(g: &mut Gfx, d: &DetailMsg) -> Vec<Ln> {
    let w = CW - 14.0;
    let mut v = Vec::new();
    // the meta pairs run on as one line (or two), not one line each: the room is for the description
    if !d.meta.is_empty() {
        let joined = d.meta.iter().map(|(k, val)| format!("{k}: {val}")).collect::<Vec<_>>().join("  \u{b7}  ");
        v.extend(wrap_lines(g, &joined, 12.0, w).into_iter().map(|l| Ln { text: l, size: 12.0, col: TEXT_DIM, bold: false, gap: 0.0 }));
    }
    for (i, l) in wrap_lines(g, &d.body, 12.0, w).into_iter().enumerate() {
        v.push(Ln { text: l, size: 12.0, col: [242, 242, 242], bold: false, gap: if i == 0 && !d.meta.is_empty() { 4.0 } else { 0.0 } });
    }
    for n in &d.notes {
        let head = if n.when.is_empty() { n.who.clone() } else { format!("{} \u{b7} {}", n.who, n.when) };
        v.push(Ln { text: head, size: 11.0, col: TEXT_FAINT, bold: false, gap: 6.0 });
        v.extend(wrap_lines(g, &n.text, 12.0, w).into_iter().map(|l| Ln { text: l, size: 12.0, col: [220, 220, 226], bold: false, gap: 0.0 }));
    }
    v
}

fn detail_room(buttons: bool) -> usize {
    (((if buttons { BODY_BOTTOM } else { Y_FOOT - 8.0 }) - Y_BODY) / LINE_P) as usize
}

impl App {
    /// The extension of the slide being shown, if it is one.
    pub(crate) fn cur_ext(&self) -> Option<usize> {
        match self.cur_id() {
            SlideId::Ext(i) => Some(i as usize),
            _ => None,
        }
    }

    pub(crate) fn slide_ext(&self, g: &mut Gfx, i: usize, ox: f32, al: f32) {
        let Some(e) = self.ext.list.get(i) else { return };
        let hits = Hits { list: &e.view.hits, record: ox.abs() < 0.5 };
        if hits.record {
            e.view.hits.borrow_mut().clear();
        }
        let cur = self.cursor_canvas();
        let name = e.manifest.as_ref().map_or("", |m| m.name.as_str());
        let sacc = Some(i);
        let _ = sacc;
        // not there yet: the notch speaks for the extension
        if !e.connected {
            header(g, &hits, cur, name, false, None, &[], ox, al);
            let (line, sub) = match (&e.problem, e.child_running()) {
                (Some(p), _) => (tr("Not running").to_string(), p.clone()),
                (None, true) => (tr("Starting\u{2026}").to_string(), String::new()),
                (None, false) => (tr("Not connected").to_string(), String::new()),
            };
            centered(g, &hits, cur, &line, &sub, None, ox, al);
            return;
        }
        if let Some(d) = &e.detail {
            self.draw_detail(g, e, d, &hits, cur, ox, al);
            return;
        }
        let Some(s) = &e.slide else {
            header(g, &hits, cur, name, false, None, &[], ox, al);
            centered(g, &hits, cur, tr("Loading\u{2026}"), "", None, ox, al);
            return;
        };
        let title = if s.title.is_empty() { name } else { s.title.as_str() };
        header(g, &hits, cur, title, s.back, s.stepper.as_ref(), &s.actions, ox, al);
        match s.state {
            State::Loading => return centered(g, &hits, cur, tr("Loading\u{2026}"), "", None, ox, al),
            State::Empty => return centered(g, &hits, cur, if s.message.is_empty() { tr("Nothing here") } else { &s.message }, "", None, ox, al),
            State::Error => return centered(g, &hits, cur, &s.message, "", Some((tr("Retry"), ExtHit::Retry)), ox, al),
            State::Setup => {
                let label = if s.setup_label.is_empty() { tr("Open settings") } else { &s.setup_label };
                return centered(g, &hits, cur, &s.message, "", Some((label, ExtHit::Setup)), ox, al);
            }
            State::Ready => {}
        }
        let tabs = !s.tabs.is_empty();
        if tabs {
            let sel = e.view.tab_sel.as_deref().unwrap_or(&s.tab);
            let mut x = LX;
            for t in &s.tabs {
                let label = match t.count {
                    Some(n) => format!("{} {}", t.label, n),
                    None => t.label.clone(),
                };
                let w = g.width_ex(&label, 11.0, true) + 18.0;
                if x + w > RX {
                    break;
                }
                let r = (x, Y_TABS, w, TAB_H);
                let on = t.id == sel;
                let hot = over(cur, r);
                if on {
                    let c = g.acc.deep;
                    g.pill(r.0 + ox, r.1, r.2, r.3, c, 0.9 * al, 1.0);
                } else {
                    g.pill(r.0 + ox, r.1, r.2, r.3, [255, 255, 255], if hot { 0.2 } else { 0.1 } * al, 1.0);
                }
                g.txt_c(&label, r.0 + ox + w / 2.0, r.1 + ty(TAB_H, 11.0), 11.0, if on { [255, 255, 255] } else { [215, 215, 222] }, al, w, true);
                hits.add(r, ExtHit::Tab(t.id.clone()));
                x += w + 6.0;
            }
        }
        let list = items(s);
        let (n, y0) = (visible_items(tabs), if tabs { Y_ROWS_TABS } else { Y_ROWS });
        let first = e.view.scroll.min(list.len().saturating_sub(n));
        let w = CW - if list.len() > n { 8.0 } else { 0.0 };
        for (vi, it) in list.iter().skip(first).take(n).enumerate() {
            let y = y0 + vi as f32 * ROW_P;
            match it {
                Item::Head(t) => g.txtb(&t.to_uppercase(), LX + 4.0 + ox, y + ty(ROW_H, 10.5), 10.5, TEXT_FAINT, al, w),
                Item::Row(r) => {
                    let rect = (LX, y, w, ROW_H);
                    let hot = over(cur, rect);
                    g.fill(&rrect(rect.0 + ox, y, w, ROW_H, 10.0), &paint(255, 255, 255, if hot { 0.14 } else { 0.08 } * al));
                    let mut tx = LX + 12.0;
                    if r.mark {
                        let c = g.acc.bright;
                        g.circle(tx + 3.0 + ox, y + ROW_H / 2.0, 3.0, &paint(c[0], c[1], c[2], al));
                        tx += 12.0;
                    }
                    let sub_w = if r.sub.is_empty() { 0.0 } else { g.width(&r.sub, 11.0) + 10.0 };
                    if sub_w > 0.0 {
                        g.txt(&r.sub, LX + w - 12.0 - sub_w + 10.0 + ox, y + ty(ROW_H, 11.0), 11.0, TEXT_FAINT, al, sub_w);
                    }
                    let maxw = w - (tx - LX) - 12.0 - sub_w;
                    let text = if g.width(&r.text, 13.0) > maxw { ellipsize(g, &r.text, 13.0, maxw) } else { r.text.clone() };
                    g.txt(&text, tx + ox, y + ty(ROW_H, 13.0), 13.0, [242, 242, 242], al, maxw + 2.0);
                    hits.add(rect, ExtHit::Row(r.id.clone()));
                }
            }
        }
        scrollbar(g, ox, y0, n, list.len(), first, al);
        self.draw_foot(g, e, &s.foot, ox, al);
    }

    fn draw_foot(&self, g: &mut Gfx, e: &crate::extapp::Ext, foot: &str, ox: f32, al: f32) {
        if !foot.is_empty() {
            g.txt(foot, LX + 4.0 + ox, Y_FOOT, 11.0, TEXT_FAINT, al, CW - 90.0);
        }
        if e.view.copied.is_some_and(|t| t.elapsed().as_secs_f32() < COPIED_SECS) {
            let c = g.acc.bright;
            g.txt_c(tr("Copied"), RX - 36.0 + ox, Y_FOOT, 11.0, c, al, 70.0, true);
        }
    }

    fn draw_detail(&self, g: &mut Gfx, e: &crate::extapp::Ext, d: &DetailMsg, hits: &Hits, cur: Option<(f32, f32)>, ox: f32, al: f32) {
        let title = if d.title.is_empty() { e.manifest.as_ref().map_or("", |m| m.name.as_str()) } else { d.title.as_str() };
        header(g, hits, cur, title, true, None, &[], ox, al);
        match d.state {
            State::Loading => return centered(g, hits, cur, tr("Loading\u{2026}"), "", None, ox, al),
            State::Empty | State::Error | State::Setup => {
                let btn = (d.state == State::Error).then_some((tr("Retry"), ExtHit::Retry));
                return centered(g, hits, cur, &d.message, "", btn, ox, al);
            }
            State::Ready => {}
        }
        let lines = detail_lines(g, d);
        let room = detail_room(!d.buttons.is_empty());
        let first = e.view.dscroll.min(lines.len().saturating_sub(room));
        let mut y = Y_BODY;
        let mut shown = 0;
        for l in lines.iter().skip(first) {
            if shown >= room {
                break;
            }
            y += if shown == 0 { 0.0 } else { l.gap };
            if l.bold {
                g.txtb(&l.text, LX + 4.0 + ox, y, l.size, l.col, al, CW - 14.0);
            } else {
                g.txt(&l.text, LX + 4.0 + ox, y, l.size, l.col, al, CW - 14.0);
            }
            y += LINE_P;
            shown += 1;
        }
        if lines.len() > room {
            let th = room as f32 * LINE_P;
            let thumb = (th * room as f32 / lines.len() as f32).max(14.0);
            let ty0 = Y_BODY + (th - thumb) * first as f32 / (lines.len() - room) as f32;
            g.fill(&rrect(RX - 4.0 + ox, Y_BODY, 3.0, th, 1.5), &paint(255, 255, 255, 0.10 * al));
            g.fill(&rrect(RX - 4.0 + ox, ty0, 3.0, thumb, 1.5), &paint(255, 255, 255, 0.55 * al));
        }
        let mut rx = RX;
        for (i, b) in d.buttons.iter().enumerate().rev() {
            let w = (g.width_ex(&b.label, 12.0, true) + 24.0).max(56.0);
            button(g, hits, cur, (rx - w, Y_BUTTONS, w, 24.0), &b.label, Some(ExtHit::Button(i)), ox, al, i == 0);
            rx -= w + 6.0;
        }
        self.draw_foot(g, e, &d.foot, ox, al);
    }

    /// Send a message to the extension of slide `i`.
    fn ext_send(&mut self, i: usize, frame: Vec<u8>) {
        let Some(id) = self.ext.list.get(i).map(|e| e.id().to_string()) else { return };
        self.ext_send_to(&id, frame);
    }

    /// A click on the extension slide. True if it was used.
    pub(crate) fn ext_click(&mut self, x: f32, y: f32) -> bool {
        let Some(i) = self.cur_ext() else { return false };
        let Some(e) = self.ext.list.get(i) else { return false };
        let hit = e.view.hits.borrow().iter().rev().find(|h| in_rect(x, y, h.0, h.1, h.2, h.3)).map(|h| h.4.clone());
        let Some(hit) = hit else { return false };
        match hit {
            ExtHit::Tab(id) => {
                self.ext.list[i].view.tab_sel = Some(id.clone());
                self.ext.list[i].view.scroll = 0;
                self.ext_send(i, proto::tab(&id));
            }
            ExtHit::Row(id) => self.ext_send(i, proto::row(&id)),
            ExtHit::Back => {
                // leave a detail page on the spot; the extension is told either way
                self.ext.list[i].detail = None;
                self.ext.list[i].view.dscroll = 0;
                self.ext_send(i, proto::back());
            }
            ExtHit::Step(d) => self.ext_send(i, proto::step(d)),
            ExtHit::Retry => self.ext_send(i, proto::action("retry")),
            ExtHit::Setup => self.ext_send(i, proto::action("setup")),
            ExtHit::Action(n) => {
                let b = self.ext.list[i].slide.as_ref().and_then(|s| s.actions.get(n)).cloned();
                if let Some(b) = b {
                    self.ext_press(i, &b);
                }
            }
            ExtHit::Button(n) => {
                let b = self.ext.list[i].detail.as_ref().and_then(|d| d.buttons.get(n)).cloned();
                if let Some(b) = b {
                    self.ext_press(i, &b);
                }
            }
        }
        true
    }

    #[cfg(not(test))]
    fn ext_clip(&mut self, text: &str) {
        unsafe { note::clip_set(HWND(self.hwnd as *mut c_void), text) };
    }

    /// Tests do not touch the real clipboard.
    #[cfg(test)]
    fn ext_clip(&mut self, text: &str) {
        self.ext.copied_text = Some(text.to_string());
    }

    /// A button was pressed: open its link, copy its text, or tell the extension. Only ever as the result of this click.
    pub(crate) fn ext_press(&mut self, i: usize, b: &Button) {
        if let Some(url) = b.link.as_deref().and_then(proto::safe_link) {
            sys::open_url(&url);
        } else if let Some(text) = &b.copy {
            self.ext_clip(text);
            self.ext.list[i].view.copied = Some(Instant::now());
        } else {
            self.ext_send(i, proto::action(&b.id));
        }
    }

    /// The page has more than fits, so the wheel scrolls it instead of changing slide.
    pub(crate) fn ext_scrollable(&mut self) -> bool {
        self.ext_scroll_room().is_some_and(|(total, room)| total > room)
    }

    /// (items or lines in all, how many show) of the page on the extension slide.
    fn ext_scroll_room(&mut self) -> Option<(usize, usize)> {
        let i = self.cur_ext()?;
        let e = self.ext.list.get(i)?;
        if let Some(d) = &e.detail {
            let g = self.gfx.as_mut()?;
            Some((detail_lines(g, d).len(), detail_room(!d.buttons.is_empty())))
        } else {
            let s = e.slide.as_ref().filter(|s| s.state == State::Ready)?;
            Some((items(s).len(), visible_items(!s.tabs.is_empty())))
        }
    }

    /// The wheel over the extension slide. True if it scrolled (the slide does not change).
    pub(crate) fn ext_wheel(&mut self, down: bool) -> bool {
        let Some((total, room)) = self.ext_scroll_room() else { return false };
        let Some(i) = self.cur_ext() else { return false };
        let Some(e) = self.ext.list.get_mut(i) else { return false };
        let scroll = if e.detail.is_some() { &mut e.view.dscroll } else { &mut e.view.scroll };
        let max = total.saturating_sub(room);
        let old = *scroll;
        *scroll = if down { (old + 1).min(max) } else { old.saturating_sub(1) };
        *scroll != old
    }
}

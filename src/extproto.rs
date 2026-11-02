//! Extension protocol (docs/extensions.md): the manifest, the frames on the pipe, and everything an extension sends,
//! checked and clipped to the limits. Data in, data out, no Win32 here, so every rule is tested without a pipe.
//! An extension is untrusted: nothing here panics on bad input, and anything over a limit is clipped, not rejected.

#![allow(dead_code)] // wired up in the next steps of the extension host

use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub(crate) const PROTO: u32 = 1;
/// Biggest frame in either direction.
pub(crate) const MAX_FRAME: usize = 64 * 1024;

pub(crate) const MAX_TABS: usize = 8;
pub(crate) const MAX_ACTIONS: usize = 3;
pub(crate) const MAX_ROWS: usize = 100;
pub(crate) const MAX_SECTIONS: usize = 4;
pub(crate) const MAX_META: usize = 6;
pub(crate) const MAX_NOTES: usize = 5;
pub(crate) const MAX_BUTTONS: usize = 4;

/// The biggest `matrix` (modules on a side): a QR code of version 20 is 97.
pub(crate) const MAX_MATRIX: usize = 100;

/// The biggest picture of a `hero`, decoded (the notch draws it as a small square).
pub(crate) const MAX_HERO_IMAGE: usize = 48 * 1024;
/// The longest line of a `hero`.
pub(crate) const LEN_HERO: usize = 80;
/// The longest text a page's `input` can hold.
pub(crate) const MAX_INPUT: usize = 40;

pub(crate) const LEN_ID: usize = 64;
pub(crate) const LEN_TAB: usize = 12;
pub(crate) const LEN_LABEL: usize = 16;
pub(crate) const LEN_ROW: usize = 120;
pub(crate) const LEN_SUB: usize = 40;
pub(crate) const LEN_META_VALUE: usize = 80;
pub(crate) const LEN_NOTE: usize = 400;
pub(crate) const LEN_BODY: usize = 4000;
pub(crate) const LEN_TOAST_TITLE: usize = 80;
pub(crate) const LEN_TOAST_SUB: usize = 60;
pub(crate) const LEN_STATUS: usize = 60;
/// Titles, meta keys, authors, dates, section and stepper labels.
pub(crate) const LEN_SHORT: usize = 24;
/// Messages, detail titles and footers.
pub(crate) const LEN_LINE: usize = 120;
pub(crate) const MAX_COPY: usize = 8000;
pub(crate) const MAX_LINK: usize = 500;

/// Messages an extension may send per second; the rest are dropped.
pub(crate) const MAX_PER_SEC: u32 = 10;
/// Least time between two toasts of one extension.
pub(crate) const TOAST_GAP: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------- manifest
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Manifest {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) exe: String,
    pub(crate) args: Vec<String>,
    /// Has a slide in the carousel (default); false = runs in the background.
    pub(crate) slide: bool,
    /// The Settings button of *Settings > Extensions* opens the extension's pages in a panel of the Settings window (true),
    /// or asks the extension to open its own window with `config` (false, the default).
    pub(crate) panel: bool,
    /// May the program start other programs? No unless the manifest says `"processes": "allow"` (see extjob.rs).
    pub(crate) processes: bool,
}

pub(crate) fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len()) && !id.starts_with('-') && !id.ends_with('-') && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `C:\dir\file.exe` or `C:/dir/file.exe`: a drive, no `..`, no control characters, ends in `.exe`.
fn valid_exe(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 8
        && b.len() <= 260
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b[2] == b'\\' || b[2] == b'/')
        && !p.chars().any(char::is_control)
        && !p.split(['\\', '/']).any(|seg| seg == "..")
        && p.to_ascii_lowercase().ends_with(".exe")
}

impl Manifest {
    /// Parse `extension.json` found in the folder named `folder`. The error is a short reason for the Settings page.
    pub(crate) fn parse(folder: &str, json: &str) -> Result<Manifest, &'static str> {
        let v: Value = serde_json::from_str(json.trim_start_matches('\u{feff}')).map_err(|_| "not valid JSON")?;
        let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("");
        let id = text("id");
        if !valid_id(id) {
            return Err("bad id");
        }
        if id != folder {
            return Err("id differs from the folder name");
        }
        let name = clip(v.get("name"), 32, false);
        if name.is_empty() {
            return Err("no name");
        }
        if v.get("proto").and_then(Value::as_u64) != Some(PROTO as u64) {
            return Err("unsupported proto");
        }
        let exe = text("exe");
        if !valid_exe(exe) {
            return Err("exe must be an absolute path to an .exe");
        }
        let args: Vec<String> = match v.get("args") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) if a.len() <= 8 => {
                let mut out = Vec::new();
                for x in a {
                    match x.as_str() {
                        Some(s) if s.chars().count() <= 200 && !s.chars().any(char::is_control) => out.push(s.to_string()),
                        _ => return Err("bad args"),
                    }
                }
                out
            }
            _ => return Err("bad args"),
        };
        let panel = match v.get("settings") {
            None | Some(Value::Null) => false,
            Some(Value::String(s)) if s == "window" => false,
            Some(Value::String(s)) if s == "panel" => true,
            _ => return Err("settings must be \"window\" or \"panel\""),
        };
        let processes = match v.get("processes") {
            None | Some(Value::Null) => false,
            Some(Value::String(s)) if s == "none" => false,
            Some(Value::String(s)) if s == "allow" => true,
            _ => return Err("processes must be \"none\" or \"allow\""),
        };
        Ok(Manifest {
            id: id.to_string(),
            name,
            version: clip(v.get("version"), 24, false),
            exe: exe.to_string(),
            args,
            slide: v.get("slide").and_then(Value::as_bool).unwrap_or(true),
            panel,
            processes,
        })
    }
}

// ---------------------------------------------------------------- frames
/// A frame announced more than `MAX_FRAME` bytes (or none): close the pipe.
#[derive(Debug, PartialEq)]
pub(crate) struct TooBig;

/// Bytes read from the pipe, cut into frames (u32 little-endian length, then the payload).
#[derive(Default)]
pub(crate) struct FrameBuf {
    buf: Vec<u8>,
}

impl FrameBuf {
    pub(crate) fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// The next whole payload, `Ok(None)` while one is still arriving.
    pub(crate) fn next(&mut self) -> Result<Option<Vec<u8>>, TooBig> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let n = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        if n == 0 || n > MAX_FRAME {
            return Err(TooBig);
        }
        if self.buf.len() < 4 + n {
            return Ok(None);
        }
        let payload = self.buf[4..4 + n].to_vec();
        self.buf.drain(..4 + n);
        Ok(Some(payload))
    }
}

/// One frame for the wire. None if it would be over the limit (the notch never sends one that big).
pub(crate) fn frame(v: &Value) -> Option<Vec<u8>> {
    let body = v.to_string().into_bytes();
    if body.len() > MAX_FRAME {
        return None;
    }
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&body);
    Some(out)
}

// ---------------------------------------------------------------- notch -> extension
fn msg(v: Value) -> Vec<u8> {
    frame(&v).unwrap_or_default()
}

pub(crate) fn welcome(app: &str, lang: &str, accent: [u8; 3]) -> Vec<u8> {
    msg(json!({"t": "welcome", "proto": PROTO, "app": app, "lang": lang, "accent": accent, "limits": {"rows": MAX_ROWS, "text": LEN_ROW}}))
}

/// `reason`: `disabled`, `unknown`, `proto` or `busy`.
pub(crate) fn reject(reason: &str) -> Vec<u8> {
    msg(json!({"t": "reject", "reason": reason}))
}

pub(crate) fn visible(on: bool) -> Vec<u8> {
    msg(json!({"t": "visible", "on": on}))
}

pub(crate) fn tab(id: &str) -> Vec<u8> {
    msg(json!({"t": "tab", "id": id}))
}

pub(crate) fn row(id: &str) -> Vec<u8> {
    msg(json!({"t": "row", "id": id}))
}

pub(crate) fn action(id: &str) -> Vec<u8> {
    msg(json!({"t": "action", "id": id}))
}

pub(crate) fn step(dir: i32) -> Vec<u8> {
    msg(json!({"t": "step", "dir": dir.signum()}))
}

pub(crate) fn back() -> Vec<u8> {
    msg(json!({"t": "back"}))
}

pub(crate) fn config() -> Vec<u8> {
    msg(json!({"t": "config"}))
}

/// What the user typed in a page's `input`: its id and the text (one line, at most `MAX_INPUT` characters).
pub(crate) fn input(id: &str, value: &str) -> Vec<u8> {
    msg(json!({"t": "input", "id": id, "value": clip(Some(&json!(value)), MAX_INPUT, false)}))
}

pub(crate) fn lang(l: &str) -> Vec<u8> {
    msg(json!({"t": "lang", "lang": l}))
}

pub(crate) fn quit() -> Vec<u8> {
    msg(json!({"t": "quit"}))
}

// ---------------------------------------------------------------- extension -> notch
/// A string clipped to `max` characters: control characters and invisible direction marks are dropped, a newline
/// becomes a space (or stays one when `multiline`), and the ends are trimmed. Missing or not a string = empty.
pub(crate) fn clip(v: Option<&Value>, max: usize, multiline: bool) -> String {
    let mut out = String::new();
    let mut n = 0;
    for c in v.and_then(Value::as_str).unwrap_or("").chars() {
        if n >= max {
            break;
        }
        let c = match c {
            '\t' => ' ',
            '\n' if !multiline => ' ',
            c => c,
        };
        let spoof = matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}');
        if spoof || (c.is_control() && c != '\n') {
            continue;
        }
        out.push(c);
        n += 1;
    }
    out.trim().to_string()
}

/// An `http`/`https` link of at most `MAX_LINK` characters, without spaces or credentials. None otherwise.
pub(crate) fn safe_link(s: &str) -> Option<String> {
    let rest = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://"))?;
    if s.chars().count() > MAX_LINK || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    (!host.is_empty() && !host.contains('@')).then(|| s.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum State {
    Ready,
    Loading,
    Empty,
    Error,
    Setup,
}

fn state_of(v: &Value) -> State {
    match v.get("state").and_then(Value::as_str) {
        Some("loading") => State::Loading,
        Some("empty") => State::Empty,
        Some("error") => State::Error,
        Some("setup") => State::Setup,
        _ => State::Ready,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tab {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) count: Option<u32>,
}

/// A header action or a detail button. With `link` or `copy` the notch does it on the click.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Button {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) link: Option<String>,
    pub(crate) copy: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) sub: String,
    pub(crate) mark: bool,
    pub(crate) tone: Tone,
}

/// The colour of a row's `sub` text and the dot before it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Tone {
    #[default]
    None,
    Good,
    Warn,
    Muted,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Section {
    pub(crate) title: String,
    pub(crate) rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Stepper {
    pub(crate) label: String,
    pub(crate) prev: bool,
    pub(crate) next: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SlideMsg {
    pub(crate) rev: u64,
    pub(crate) title: String,
    pub(crate) state: State,
    pub(crate) message: String,
    pub(crate) setup_label: String,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) tab: String,
    pub(crate) actions: Vec<Button>,
    /// Rows as sections: one section without a title when the extension sent plain `rows`.
    pub(crate) sections: Vec<Section>,
    pub(crate) stepper: Option<Stepper>,
    pub(crate) back: bool,
    /// A card at the top of the page with a picture (the Settings panel).
    pub(crate) hero: Option<Hero>,
    pub(crate) foot: String,
}

/// `{ "title": "Song", "sub": "Artist", "text": "Phone 1 - Playing", "image": { "mime": "image/jpeg", "data": "<base64>" } }`
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Hero {
    pub(crate) title: String,
    pub(crate) sub: String,
    pub(crate) text: String,
    /// The picture's bytes (PNG or JPEG), decoded from base64; None if it was missing or not allowed.
    pub(crate) image: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Note {
    pub(crate) who: String,
    pub(crate) when: String,
    pub(crate) text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DetailMsg {
    pub(crate) rev: u64,
    pub(crate) for_id: String,
    pub(crate) state: State,
    pub(crate) message: String,
    pub(crate) title: String,
    pub(crate) meta: Vec<(String, String)>,
    pub(crate) body: String,
    pub(crate) notes: Vec<Note>,
    pub(crate) buttons: Vec<Button>,
    pub(crate) matrix: Option<Matrix>,
    pub(crate) input: Option<InputSpec>,
    pub(crate) foot: String,
}

/// A line of text the user can edit on a page (the Settings panel): `{ "id": "rename", "label": "Name", "value": "Phone 1",
/// "max": 32, "button": "Save" }`. What is typed goes back as an `input` message.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InputSpec {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) value: String,
    pub(crate) max: usize,
    pub(crate) button: String,
}

/// A small square picture of black and white modules (a QR code), `size` x `size`, row by row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Matrix {
    pub(crate) size: usize,
    pub(crate) dark: Vec<bool>,
}

impl Matrix {
    pub(crate) fn at(&self, x: usize, y: usize) -> bool {
        self.dark.get(y * self.size + x).copied().unwrap_or(false)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FromExt {
    Hello { proto: u32, id: String, name: String, version: String },
    Slide(SlideMsg),
    Detail(DetailMsg),
    Toast { title: String, sub: String, count: u32 },
    Badge(bool),
    Status(String),
    Bye,
}

/// What one frame turned out to be.
#[derive(Debug, PartialEq)]
pub(crate) enum Parsed {
    Msg(FromExt),
    /// A type this version does not know: ignored on purpose, so newer extensions keep working.
    Ignored,
    /// Not JSON, no `t`, or a required field is missing: counts towards closing the connection.
    Bad,
}

fn items<'a>(v: &'a Value, key: &str, max: usize) -> impl Iterator<Item = &'a Value> {
    v.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().take(max)
}

fn id_of(v: &Value, key: &str) -> String {
    clip(v.get(key), LEN_ID, false)
}

fn parse_buttons(v: &Value, key: &str, max: usize) -> Vec<Button> {
    items(v, key, max)
        .filter_map(|b| {
            let (id, label) = (id_of(b, "id"), clip(b.get("label"), LEN_LABEL, false));
            if id.is_empty() || label.is_empty() {
                return None;
            }
            let link = b.get("link").and_then(Value::as_str).and_then(safe_link);
            let copy = Some(clip(b.get("copy"), MAX_COPY, true)).filter(|c| !c.is_empty());
            Some(Button { id, label, link, copy })
        })
        .collect()
}

fn parse_rows(v: &Value, key: &str, room: usize) -> Vec<Row> {
    items(v, key, room)
        .filter_map(|r| {
            let id = id_of(r, "id");
            (!id.is_empty()).then(|| Row {
                id,
                text: clip(r.get("text"), LEN_ROW, false),
                sub: clip(r.get("sub"), LEN_SUB, false),
                mark: r.get("mark").and_then(Value::as_bool).unwrap_or(false),
                tone: match r.get("tone").and_then(Value::as_str) {
                    Some("good") => Tone::Good,
                    Some("warn") => Tone::Warn,
                    Some("muted") => Tone::Muted,
                    _ => Tone::None,
                },
            })
        })
        .collect()
}

/// Standard base64 (`+`, `/`, padding optional). None for anything else, and for stray bits.
pub(crate) fn b64_decode_std(s: &str) -> Option<Vec<u8>> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let s = s.trim_end_matches('=');
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        acc = acc << 6 | A.iter().position(|&a| a == c)? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    (acc == 0).then_some(out)
}

fn parse_hero(v: &Value) -> Option<Hero> {
    let h = v.get("hero").filter(|h| h.is_object())?;
    let image = h.get("image").filter(|i| i.is_object()).and_then(|i| {
        let png_or_jpeg = matches!(i.get("mime").and_then(Value::as_str), Some("image/png" | "image/jpeg"));
        let data = i.get("data").and_then(Value::as_str)?;
        // base64 is a third bigger than what it holds: refuse before decoding something huge
        (png_or_jpeg && data.len() <= MAX_HERO_IMAGE / 3 * 4 + 8).then(|| b64_decode_std(data)).flatten().filter(|b| !b.is_empty() && b.len() <= MAX_HERO_IMAGE)
    });
    Some(Hero { title: clip(h.get("title"), LEN_HERO, false), sub: clip(h.get("sub"), LEN_HERO, false), text: clip(h.get("text"), LEN_HERO, false), image })
}

fn parse_slide(v: &Value) -> Option<SlideMsg> {
    let sections = if v.get("sections").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) {
        let mut left = MAX_ROWS;
        let mut out = Vec::new();
        for s in items(v, "sections", MAX_SECTIONS) {
            let rows = parse_rows(s, "rows", left);
            left -= rows.len();
            out.push(Section { title: clip(s.get("title"), LEN_SHORT, false), rows });
        }
        out
    } else {
        vec![Section { title: String::new(), rows: parse_rows(v, "rows", MAX_ROWS) }]
    };
    let stepper = v.get("stepper").filter(|s| s.is_object()).map(|s| Stepper {
        label: clip(s.get("label"), LEN_SHORT, false),
        prev: s.get("prev").and_then(Value::as_bool).unwrap_or(false),
        next: s.get("next").and_then(Value::as_bool).unwrap_or(false),
    });
    Some(SlideMsg {
        rev: v.get("rev")?.as_u64()?,
        title: clip(v.get("title"), LEN_SHORT, false),
        state: state_of(v),
        message: clip(v.get("message"), LEN_LINE, false),
        setup_label: clip(v.get("setup_label"), LEN_LABEL, false),
        tabs: items(v, "tabs", MAX_TABS)
            .filter_map(|t| {
                let (id, label) = (id_of(t, "id"), clip(t.get("label"), LEN_TAB, false));
                (!id.is_empty() && !label.is_empty()).then(|| Tab { id, label, count: t.get("count").and_then(Value::as_u64).map(|n| n.min(u32::MAX as u64) as u32) })
            })
            .collect(),
        tab: id_of(v, "tab"),
        actions: parse_buttons(v, "actions", MAX_ACTIONS),
        sections,
        stepper,
        back: v.get("back").and_then(Value::as_bool).unwrap_or(false),
        hero: parse_hero(v),
        foot: clip(v.get("foot"), LEN_LINE, false),
    })
}

fn parse_input(v: &Value) -> Option<InputSpec> {
    let i = v.get("input").filter(|i| i.is_object())?;
    let id = id_of(i, "id");
    if id.is_empty() {
        return None;
    }
    let max = i.get("max").and_then(Value::as_u64).map_or(MAX_INPUT, |m| (m as usize).clamp(1, MAX_INPUT));
    Some(InputSpec { id, label: clip(i.get("label"), LEN_LABEL, false), value: clip(i.get("value"), max, false), max, button: clip(i.get("button"), LEN_LABEL, false) })
}

/// `{"size": 3, "rows": ["101", "010", "101"]}`. Anything that is not exactly that is no matrix (the rest of the page stays).
fn parse_matrix(v: &Value) -> Option<Matrix> {
    let m = v.get("matrix")?;
    let size = usize::try_from(m.get("size")?.as_u64()?).ok().filter(|s| (1..=MAX_MATRIX).contains(s))?;
    let rows = m.get("rows")?.as_array().filter(|r| r.len() == size)?;
    let mut dark = Vec::with_capacity(size * size);
    for r in rows {
        let r = r.as_str().filter(|r| r.len() == size)?;
        for c in r.bytes() {
            match c {
                b'0' => dark.push(false),
                b'1' => dark.push(true),
                _ => return None,
            }
        }
    }
    Some(Matrix { size, dark })
}

fn parse_detail(v: &Value) -> Option<DetailMsg> {
    Some(DetailMsg {
        rev: v.get("rev")?.as_u64()?,
        for_id: id_of(v, "for"),
        state: state_of(v),
        message: clip(v.get("message"), LEN_LINE, false),
        title: clip(v.get("title"), LEN_LINE, false),
        meta: items(v, "meta", MAX_META)
            .filter_map(|m| {
                let k = clip(m.get("k"), LEN_SHORT, false);
                (!k.is_empty()).then(|| (k, clip(m.get("v"), LEN_META_VALUE, false)))
            })
            .collect(),
        body: clip(v.get("body"), LEN_BODY, true),
        notes: items(v, "notes", MAX_NOTES)
            .map(|n| Note { who: clip(n.get("who"), LEN_SHORT, false), when: clip(n.get("when"), LEN_SHORT, false), text: clip(n.get("text"), LEN_NOTE, true) })
            .collect(),
        buttons: parse_buttons(v, "buttons", MAX_BUTTONS),
        matrix: parse_matrix(v),
        input: parse_input(v),
        foot: clip(v.get("foot"), LEN_LINE, false),
    })
}

/// Check one frame's payload.
pub(crate) fn parse(payload: &[u8]) -> Parsed {
    let Ok(v) = serde_json::from_slice::<Value>(payload) else { return Parsed::Bad };
    let Some(t) = v.get("t").and_then(Value::as_str) else { return Parsed::Bad };
    let m = match t {
        "hello" => v.get("proto").and_then(Value::as_u64).map(|p| FromExt::Hello {
            proto: p.min(u32::MAX as u64) as u32,
            id: id_of(&v, "id"),
            name: clip(v.get("name"), 32, false),
            version: clip(v.get("version"), 24, false),
        }),
        "slide" => parse_slide(&v).map(FromExt::Slide),
        "detail" => parse_detail(&v).map(FromExt::Detail),
        "toast" => {
            let title = clip(v.get("title"), LEN_TOAST_TITLE, false);
            (!title.is_empty()).then(|| FromExt::Toast {
                title,
                sub: clip(v.get("sub"), LEN_TOAST_SUB, false),
                count: v.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 99) as u32,
            })
        }
        "badge" => v.get("on").and_then(Value::as_bool).map(FromExt::Badge),
        "status" => Some(FromExt::Status(clip(v.get("text"), LEN_STATUS, false))),
        "bye" => Some(FromExt::Bye),
        _ => return Parsed::Ignored,
    };
    m.map_or(Parsed::Bad, Parsed::Msg)
}

/// Version both sides speak, or the `reject` reason. Only version 1 exists so far.
pub(crate) fn negotiate(ext_proto: u32) -> Result<u32, &'static str> {
    if ext_proto == PROTO {
        Ok(PROTO)
    } else {
        Err("proto")
    }
}

/// Keeps the newest `rev` seen on a connection (shared by `slide` and `detail`): an older or repeated one is dropped.
pub(crate) fn newer(last: &mut Option<u64>, rev: u64) -> bool {
    if last.map_or(true, |l| rev > l) {
        *last = Some(rev);
        true
    } else {
        false
    }
}

/// At most `MAX_PER_SEC` messages per second.
pub(crate) struct RateLimit {
    start: Instant,
    n: u32,
}

impl RateLimit {
    pub(crate) fn new(now: Instant) -> RateLimit {
        RateLimit { start: now, n: 0 }
    }

    pub(crate) fn allow(&mut self, now: Instant) -> bool {
        if now.saturating_duration_since(self.start) >= Duration::from_secs(1) {
            self.start = now;
            self.n = 0;
        }
        self.n += 1;
        self.n <= MAX_PER_SEC
    }
}

/// One toast per `TOAST_GAP` per extension.
#[derive(Default)]
pub(crate) struct ToastGate {
    last: Option<Instant>,
}

impl ToastGate {
    pub(crate) fn allow(&mut self, now: Instant) -> bool {
        if self.last.map_or(true, |t| now.saturating_duration_since(t) >= TOAST_GAP) {
            self.last = Some(now);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(m: &str) -> Manifest {
        Manifest::parse("example-issues", m).unwrap()
    }

    const GOOD: &str = r#"{"id":"example-issues","name":"Issues","version":"1.0.0","exe":"C:\\Tools\\issues.exe","args":["--notch"],"proto":1}"#;

    #[test]
    fn manifest_good_and_defaults() {
        let m = ok(GOOD);
        assert_eq!((m.id.as_str(), m.name.as_str(), m.exe.as_str()), ("example-issues", "Issues", "C:\\Tools\\issues.exe"));
        assert_eq!(m.args, vec!["--notch".to_string()]);
        assert!(m.slide, "a slide unless said otherwise");
        let bg = ok(r#"{"id":"example-issues","name":"Issues","exe":"C:/Tools/issues.exe","slide":false,"proto":1}"#);
        assert!(!bg.slide && bg.args.is_empty() && bg.version.is_empty());
        assert!(Manifest::parse("example-issues", &format!("\u{feff}{GOOD}")).is_ok(), "a BOM from Notepad is fine");
    }

    #[test]
    fn manifest_rejects_what_it_should() {
        let with = |from: &str, to: &str| GOOD.replace(from, to);
        let bad = |m: String| Manifest::parse("example-issues", &m).is_err();
        assert!(Manifest::parse("other", GOOD).is_err(), "folder and id must match");
        assert!(bad("nope".into()));
        assert!(bad(with("example-issues", "Bad_Id")));
        assert!(bad(with("\"name\":\"Issues\"", "\"name\":\"\"")));
        assert!(bad(with("\"proto\":1", "\"proto\":2")));
        assert!(bad(with("\"proto\":1", "\"x\":0")), "proto is required");
        assert!(bad(with("C:\\\\Tools\\\\issues.exe", "issues.exe")), "relative path");
        assert!(bad(with("C:\\\\Tools\\\\issues.exe", "C:\\\\Tools\\\\..\\\\issues.exe")), "no ..");
        assert!(bad(with("C:\\\\Tools\\\\issues.exe", "C:\\\\Tools\\\\issues.bat")), "an .exe only");
        assert!(bad(with("[\"--notch\"]", "[1]")), "args are strings");
        assert!(bad(with("[\"--notch\"]", "[\"a\",\"b\",\"c\",\"d\",\"e\",\"f\",\"g\",\"h\",\"i\"]")), "at most 8 args");
        assert!(valid_id("a") && valid_id("a-b-1") && !valid_id("-a") && !valid_id("a-") && !valid_id("") && !valid_id(&"a".repeat(33)));
    }

    #[test]
    fn frames_split_join_and_limit() {
        let one = frame(&json!({"t": "bye"})).unwrap();
        let two = frame(&json!({"t": "badge", "on": true})).unwrap();
        let mut fb = FrameBuf::default();
        fb.push(&one[..3]);
        assert_eq!(fb.next(), Ok(None), "not even the length yet");
        fb.push(&one[3..one.len() - 1]);
        assert_eq!(fb.next(), Ok(None), "payload still arriving");
        let mut rest = one[one.len() - 1..].to_vec();
        rest.extend_from_slice(&two);
        fb.push(&rest);
        assert_eq!(parse(&fb.next().unwrap().unwrap()), Parsed::Msg(FromExt::Bye));
        assert_eq!(parse(&fb.next().unwrap().unwrap()), Parsed::Msg(FromExt::Badge(true)));
        assert_eq!(fb.next(), Ok(None));
        let mut big = FrameBuf::default();
        big.push(&((MAX_FRAME + 1) as u32).to_le_bytes());
        assert_eq!(big.next(), Err(TooBig));
        let mut zero = FrameBuf::default();
        zero.push(&0u32.to_le_bytes());
        assert_eq!(zero.next(), Err(TooBig), "an empty frame is not valid either");
        assert!(frame(&json!({"x": "a".repeat(MAX_FRAME)})).is_none(), "the notch never sends an oversize frame");
    }

    #[test]
    fn clip_cleans_and_cuts() {
        let s = |v: &str, max, ml| clip(Some(&json!(v)), max, ml);
        assert_eq!(s("  hello\u{7}\u{0} world\n!\t", 50, false), "hello world !");
        assert_eq!(s("a\nb\r\nc", 50, true), "a\nb\nc", "newlines stay when asked, carriage returns never");
        assert_eq!(s("abcdef", 3, false), "abc");
        assert_eq!(s("\u{202e}evil\u{200b}", 50, false), "evil", "direction marks and zero-width characters go");
        assert_eq!(s("ééé", 2, false), "éé", "characters, not bytes");
        assert_eq!(clip(None, 5, false), "");
        assert_eq!(clip(Some(&json!(5)), 5, false), "", "not a string");
    }

    #[test]
    fn links_are_http_only_without_credentials() {
        assert!(safe_link("https://tracker.example/issues/128").is_some());
        assert!(safe_link("http://tracker.example/a?b=c#d").is_some());
        for bad in ["javascript:alert(1)", "file:///C:/x", "ftp://x.example", "https://", "https://user:pw@x.example/", "https://x.example@evil.example/", "https://x.example/a b", "https://x.example/\u{7}", "//x.example", ""] {
            assert!(safe_link(bad).is_none(), "{bad:?}");
        }
        assert!(safe_link(&format!("https://x.example/{}", "a".repeat(MAX_LINK))).is_none(), "too long");
    }

    fn slide(j: Value) -> SlideMsg {
        match parse(j.to_string().as_bytes()) {
            Parsed::Msg(FromExt::Slide(s)) => s,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn slide_with_rows_tabs_and_actions() {
        let s = slide(json!({
            "t": "slide", "rev": 12, "title": "Is|sues", "state": "ready",
            "tabs": [{"id": "all", "label": "All", "count": 7}, {"id": "", "label": "no id"}, {"id": "x", "label": ""}],
            "tab": "all",
            "actions": [{"id": "refresh", "label": "Refresh"}, {"id": "copy", "label": "Copy", "copy": "a\nb"}, {"label": "no id"}],
            "rows": [{"id": "128", "text": "#128 Fix crash", "sub": "To do", "mark": true}, {"text": "no id"}, {"id": "127", "text": "#127"}],
            "foot": "Updated just now"
        }));
        assert_eq!((s.rev, s.title.as_str(), s.state, s.tab.as_str()), (12, "Is|sues", State::Ready, "all"));
        assert_eq!(s.tabs, vec![Tab { id: "all".into(), label: "All".into(), count: Some(7) }]);
        assert_eq!(s.actions.len(), 2);
        assert_eq!(s.actions[1].copy.as_deref(), Some("a\nb"));
        assert_eq!(s.sections.len(), 1);
        let rows = &s.sections[0].rows;
        assert_eq!(rows.len(), 2, "a row without an id is dropped");
        assert!(rows[0].mark && !rows[1].mark);
        assert_eq!((rows[0].sub.as_str(), s.foot.as_str()), ("To do", "Updated just now"));
        assert!(s.stepper.is_none() && !s.back);
    }

    #[test]
    fn slide_limits_are_clipped_not_rejected() {
        let many = |n: usize| (0..n).map(|i| json!({"id": i.to_string(), "text": "x".repeat(500), "sub": "y".repeat(500)})).collect::<Vec<_>>();
        let s = slide(json!({"t": "slide", "rev": 1, "title": "t".repeat(100), "rows": many(300),
            "tabs": (0..20).map(|i| json!({"id": i.to_string(), "label": "L".repeat(50)})).collect::<Vec<_>>(),
            "actions": (0..9).map(|i| json!({"id": i.to_string(), "label": "A".repeat(50)})).collect::<Vec<_>>()}));
        assert_eq!(s.sections[0].rows.len(), MAX_ROWS);
        assert_eq!((s.sections[0].rows[0].text.chars().count(), s.sections[0].rows[0].sub.chars().count()), (LEN_ROW, LEN_SUB));
        assert_eq!(s.title.chars().count(), LEN_SHORT);
        assert_eq!((s.tabs.len(), s.tabs[0].label.chars().count()), (MAX_TABS, LEN_TAB));
        assert_eq!((s.actions.len(), s.actions[0].label.chars().count()), (MAX_ACTIONS, LEN_LABEL));
    }

    #[test]
    fn sections_share_the_row_budget_and_the_stepper_parses() {
        let rows = |n: usize| (0..n).map(|i| json!({"id": i.to_string(), "text": "r"})).collect::<Vec<_>>();
        let s = slide(json!({"t": "slide", "rev": 2, "back": true, "stepper": {"label": "2026-10-31", "prev": true, "next": false},
            "sections": [{"title": "Worked on", "rows": rows(70)}, {"title": "Queue", "rows": rows(70)}, {"title": "C", "rows": rows(5)}, {"title": "D"}, {"title": "E", "rows": rows(1)}]}));
        assert_eq!(s.sections.len(), MAX_SECTIONS, "four sections at most");
        let total: usize = s.sections.iter().map(|x| x.rows.len()).sum();
        assert_eq!(total, MAX_ROWS, "100 rows in all");
        assert_eq!(s.sections[0].rows.len(), 70);
        assert_eq!(s.sections[1].rows.len(), 30);
        assert_eq!(s.sections[0].title, "Worked on");
        assert!(s.back);
        assert_eq!(s.stepper, Some(Stepper { label: "2026-10-31".into(), prev: true, next: false }));
    }

    #[test]
    fn slide_states() {
        for (name, want) in [("ready", State::Ready), ("loading", State::Loading), ("empty", State::Empty), ("error", State::Error), ("setup", State::Setup), ("???", State::Ready)] {
            assert_eq!(slide(json!({"t": "slide", "rev": 1, "state": name})).state, want, "{name}");
        }
        let s = slide(json!({"t": "slide", "rev": 1, "state": "setup", "message": "Not set up", "setup_label": "Open settings"}));
        assert_eq!((s.message.as_str(), s.setup_label.as_str()), ("Not set up", "Open settings"));
    }

    #[test]
    fn detail_page() {
        let d = match parse(
            json!({
                "t": "detail", "rev": 13, "for": "128", "title": "#128 Fix crash",
                "meta": [{"k": "Project", "v": "Installer"}, {"k": "", "v": "dropped"}],
                "body": "line1\nline2", "notes": [{"who": "Dewi", "when": "2026-10-31 14:02", "text": "Reproduced."}],
                "buttons": [{"id": "open", "label": "Open", "link": "https://tracker.example/issues/128"}, {"id": "bad", "label": "Bad", "link": "javascript:x"}],
                "foot": "2 attachments"
            })
            .to_string()
            .as_bytes(),
        ) {
            Parsed::Msg(FromExt::Detail(d)) => d,
            other => panic!("{other:?}"),
        };
        assert_eq!((d.rev, d.for_id.as_str(), d.state), (13, "128", State::Ready));
        assert_eq!(d.meta, vec![("Project".to_string(), "Installer".to_string())]);
        assert_eq!(d.body, "line1\nline2");
        assert_eq!(d.notes.len(), 1);
        assert_eq!(d.buttons[0].link.as_deref(), Some("https://tracker.example/issues/128"));
        assert_eq!(d.buttons[1].link, None, "a link that is not http(s) is dropped, the button stays");
    }

    #[test]
    fn small_messages() {
        let p = |j: Value| parse(j.to_string().as_bytes());
        assert_eq!(p(json!({"t": "hello", "proto": 1, "id": "example-issues", "name": "Issues", "version": "1.0.0"})), Parsed::Msg(FromExt::Hello { proto: 1, id: "example-issues".into(), name: "Issues".into(), version: "1.0.0".into() }));
        assert_eq!(p(json!({"t": "hello", "id": "x"})), Parsed::Bad, "proto is required");
        assert_eq!(p(json!({"t": "toast", "title": "#1 New", "sub": "New · To do", "count": 500})), Parsed::Msg(FromExt::Toast { title: "#1 New".into(), sub: "New · To do".into(), count: 99 }));
        assert_eq!(p(json!({"t": "toast", "sub": "no title"})), Parsed::Bad);
        assert_eq!(p(json!({"t": "toast", "title": "t"})), Parsed::Msg(FromExt::Toast { title: "t".into(), sub: String::new(), count: 1 }));
        assert_eq!(p(json!({"t": "badge"})), Parsed::Bad);
        assert_eq!(p(json!({"t": "status", "text": "Phone connected"})), Parsed::Msg(FromExt::Status("Phone connected".into())));
        assert_eq!(p(json!({"t": "status", "text": "s".repeat(200)})), Parsed::Msg(FromExt::Status("s".repeat(LEN_STATUS))));
        assert_eq!(p(json!({"t": "bye"})), Parsed::Msg(FromExt::Bye));
    }

    #[test]
    fn unknown_and_broken_frames() {
        assert_eq!(parse(br#"{"t":"hologram","x":1}"#), Parsed::Ignored, "a newer extension's message type");
        assert_eq!(parse(b"not json"), Parsed::Bad);
        assert_eq!(parse(b"[1,2]"), Parsed::Bad);
        assert_eq!(parse(br#"{"no":"type"}"#), Parsed::Bad);
        assert_eq!(parse(br#"{"t":"slide"}"#), Parsed::Bad, "slide needs a rev");
        assert_eq!(parse(&[0xff, 0xfe, 0x00]), Parsed::Bad);
        let deep = format!("{}1{}", "[".repeat(10_000), "]".repeat(10_000));
        assert_eq!(parse(deep.as_bytes()), Parsed::Bad, "deep nesting is refused, not a stack overflow");
        let extra = slide(json!({"t": "slide", "rev": 1, "future_field": {"a": 1}}));
        assert_eq!(extra.rev, 1, "unknown fields are ignored");
    }

    #[test]
    fn notch_messages_are_well_formed_frames() {
        let read = |bytes: Vec<u8>| {
            let mut fb = FrameBuf::default();
            fb.push(&bytes);
            serde_json::from_slice::<Value>(&fb.next().unwrap().unwrap()).unwrap()
        };
        assert_eq!(read(welcome("0.13.0", "en", [1, 2, 3])), json!({"t": "welcome", "proto": 1, "app": "0.13.0", "lang": "en", "accent": [1, 2, 3], "limits": {"rows": 100, "text": 120}}));
        assert_eq!(read(reject("busy")), json!({"t": "reject", "reason": "busy"}));
        assert_eq!(read(visible(true)), json!({"t": "visible", "on": true}));
        assert_eq!(read(step(-5)), json!({"t": "step", "dir": -1}), "only the direction");
        assert_eq!(read(row("128")), json!({"t": "row", "id": "128"}));
        assert_eq!(read(tab("todo")), json!({"t": "tab", "id": "todo"}));
        assert_eq!(read(action("refresh")), json!({"t": "action", "id": "refresh"}));
        for (m, t) in [(back(), "back"), (config(), "config"), (quit(), "quit")] {
            assert_eq!(read(m), json!({ "t": t }));
        }
        assert_eq!(read(lang("id")), json!({"t": "lang", "lang": "id"}));
    }

    #[test]
    fn version_negotiation_and_rev_order() {
        assert_eq!(negotiate(1), Ok(1));
        assert_eq!(negotiate(0), Err("proto"));
        assert_eq!(negotiate(2), Err("proto"));
        let mut last = None;
        assert!(newer(&mut last, 5));
        assert!(!newer(&mut last, 5), "a repeat is dropped");
        assert!(!newer(&mut last, 4), "an older one too");
        assert!(newer(&mut last, 6));
        assert!(newer(&mut None, 0), "the first one counts, whatever it is");
    }

    #[test]
    fn rate_limits() {
        let t0 = Instant::now();
        let mut r = RateLimit::new(t0);
        assert!((0..MAX_PER_SEC).all(|_| r.allow(t0)), "ten in a second");
        assert!(!r.allow(t0 + Duration::from_millis(900)), "the eleventh is dropped");
        assert!(r.allow(t0 + Duration::from_millis(1000)), "a new second");
        let mut g = ToastGate::default();
        assert!(g.allow(t0));
        assert!(!g.allow(t0 + Duration::from_secs(4)));
        assert!(g.allow(t0 + Duration::from_secs(5)));
        assert!(!g.allow(t0 + Duration::from_secs(6)), "the gap counts from the last one shown");
    }
}

#[cfg(test)]
mod panel_tests {
    use super::*;

    const BASE: &str = r#"{"id":"media-bridge","name":"Media Bridge","exe":"C:\\Tools\\mb.exe","proto":1"#;

    fn manifest(extra: &str) -> Result<Manifest, &'static str> {
        Manifest::parse("media-bridge", &format!("{BASE}{extra}}}"))
    }

    #[test]
    fn the_settings_button_says_where_the_settings_are() {
        assert!(!manifest("").unwrap().panel, "its own window unless said otherwise");
        assert!(!manifest(r#","settings":"window""#).unwrap().panel);
        assert!(manifest(r#","settings":"panel""#).unwrap().panel);
        assert!(!manifest(r#","settings":null"#).unwrap().panel);
        assert!(manifest(r#","settings":"tab""#).is_err(), "anything else is a mistake");
        assert!(manifest(r#","settings":true"#).is_err());
        let both = manifest(r#","slide":false,"settings":"panel""#).unwrap();
        assert!(!both.slide && both.panel, "an extension without a slide has its settings in the panel");
    }

    fn detail(matrix: serde_json::Value) -> Option<Matrix> {
        match parse(serde_json::json!({"t": "detail", "rev": 1, "matrix": matrix}).to_string().as_bytes()) {
            Parsed::Msg(FromExt::Detail(d)) => d.matrix,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_matrix_is_squares_of_ones_and_zeros() {
        let m = detail(serde_json::json!({"size": 3, "rows": ["101", "010", "111"]})).unwrap();
        assert_eq!(m.size, 3);
        assert_eq!([m.at(0, 0), m.at(1, 0), m.at(1, 1), m.at(0, 1), m.at(2, 2)], [true, false, true, false, true]);
        assert!(!m.at(3, 0) && !m.at(0, 3), "outside is white");
        let big = vec!["1".repeat(MAX_MATRIX); MAX_MATRIX];
        assert!(detail(serde_json::json!({"size": MAX_MATRIX, "rows": big})).is_some(), "the biggest one");
    }

    #[test]
    fn a_matrix_that_is_not_one_is_dropped_and_the_page_stays() {
        for bad in [
            serde_json::json!({"size": 0, "rows": []}),
            serde_json::json!({"size": 101, "rows": vec!["1".repeat(101); 101]}),
            serde_json::json!({"size": 3, "rows": ["101", "010"]}),
            serde_json::json!({"size": 3, "rows": ["101", "010", "11"]}),
            serde_json::json!({"size": 3, "rows": ["101", "0x0", "111"]}),
            serde_json::json!({"size": 3, "rows": ["101", "010", 7]}),
            serde_json::json!({"size": "3", "rows": ["101", "010", "111"]}),
            serde_json::json!({"rows": ["1"]}),
            serde_json::json!("101"),
            serde_json::json!(null),
        ] {
            assert_eq!(detail(bad.clone()), None, "{bad}");
        }
        // the page around it is still read
        match parse(serde_json::json!({"t": "detail", "rev": 1, "title": "Pair", "matrix": {"size": 2, "rows": ["1"]}}).to_string().as_bytes()) {
            Parsed::Msg(FromExt::Detail(d)) => assert_eq!((d.title.as_str(), d.matrix), ("Pair", None)),
            other => panic!("{other:?}"),
        }
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;

    fn input(i: serde_json::Value) -> Option<InputSpec> {
        match parse(serde_json::json!({"t": "detail", "rev": 1, "input": i}).to_string().as_bytes()) {
            Parsed::Msg(FromExt::Detail(d)) => d.input,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_text_box_is_read_with_its_limits() {
        let i = input(serde_json::json!({"id": "rename", "label": "Name", "value": "Phone 1", "max": 32, "button": "Save"})).unwrap();
        assert_eq!((i.id.as_str(), i.label.as_str(), i.value.as_str(), i.max, i.button.as_str()), ("rename", "Name", "Phone 1", 32, "Save"));
        let d = input(serde_json::json!({"id": "x"})).unwrap();
        assert_eq!((d.max, d.value.as_str(), d.button.as_str()), (MAX_INPUT, "", ""), "defaults");
        assert_eq!(input(serde_json::json!({"id": "x", "max": 0})).unwrap().max, 1);
        assert_eq!(input(serde_json::json!({"id": "x", "max": 999})).unwrap().max, MAX_INPUT, "never more than 40");
        assert_eq!(input(serde_json::json!({"id": "x", "max": 5, "value": "abcdefgh"})).unwrap().value, "abcde", "the value is cut to the limit");
        assert_eq!(input(serde_json::json!({"id": "x", "value": "a\u{7}b\nc"})).unwrap().value, "ab c", "one line, no control characters");
    }

    #[test]
    fn a_text_box_without_an_id_is_no_text_box() {
        assert_eq!(input(serde_json::json!({"label": "Name"})), None);
        assert_eq!(input(serde_json::json!({"id": ""})), None);
        assert_eq!(input(serde_json::json!("rename")), None);
        assert_eq!(input(serde_json::json!(null)), None);
    }

    #[test]
    fn what_is_typed_goes_back_clean_and_short() {
        let read = |b: Vec<u8>| serde_json::from_slice::<serde_json::Value>(&b[4..]).unwrap();
        assert_eq!(read(super::input("rename", "Pixel 8")), serde_json::json!({"t": "input", "id": "rename", "value": "Pixel 8"}));
        let long = read(super::input("rename", &format!("{}\n\u{7}x", "a".repeat(60))));
        assert_eq!(long["value"].as_str().unwrap().chars().count(), MAX_INPUT);
        assert!(!long["value"].as_str().unwrap().contains('\n'));
    }
}

#[cfg(test)]
mod hero_tests {
    use super::*;

    fn b64(data: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for c in data.chunks(3) {
            let n = c.iter().enumerate().fold(0u32, |acc, (i, &b)| acc | (b as u32) << (16 - 8 * i));
            for i in 0..c.len() + 1 {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
            out.extend(std::iter::repeat('=').take(3 - c.len()));
        }
        out
    }

    fn slide(extra: serde_json::Value) -> SlideMsg {
        match parse(serde_json::json!({"t": "slide", "rev": 1, "hero": extra}).to_string().as_bytes()) {
            Parsed::Msg(FromExt::Slide(s)) => s,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn standard_base64_is_read_with_and_without_padding() {
        let bytes = [0xfb, 0xff, 0xbe, 0xfb, 0xef];
        assert_eq!(b64(&bytes), "+/++++8=");
        assert_eq!(b64_decode_std("+/++++8="), Some(bytes.to_vec()));
        assert_eq!(b64_decode_std("+/++++8"), Some(bytes.to_vec()));
        assert_eq!(b64_decode_std("-_----8="), None, "base64url is not standard base64");
        assert_eq!(b64_decode_std("A"), None);
        assert_eq!(b64_decode_std("AB"), None, "stray bits");
        for n in 0..30 {
            let d: Vec<u8> = (0..n as u8).map(|i| i.wrapping_mul(53)).collect();
            assert_eq!(b64_decode_std(&b64(&d)), Some(d));
        }
    }

    #[test]
    fn a_hero_with_a_picture() {
        let img = vec![7u8; 5000];
        let h = slide(serde_json::json!({"title": "Cupid", "sub": "FIFTY FIFTY", "text": "Phone 1 \u{b7} Playing", "image": {"mime": "image/jpeg", "data": b64(&img)}})).hero.unwrap();
        assert_eq!((h.title.as_str(), h.sub.as_str(), h.text.as_str()), ("Cupid", "FIFTY FIFTY", "Phone 1 \u{b7} Playing"));
        assert_eq!(h.image, Some(img));
    }

    #[test]
    fn a_picture_that_is_not_allowed_is_left_out_and_the_words_stay() {
        let ok = b64(&[1u8; 100]);
        for (name, image) in [
            ("a gif", serde_json::json!({"mime": "image/gif", "data": ok})),
            ("no mime", serde_json::json!({"data": ok})),
            ("no data", serde_json::json!({"mime": "image/png"})),
            ("bad base64", serde_json::json!({"mime": "image/png", "data": "!!!"})),
            ("empty", serde_json::json!({"mime": "image/png", "data": ""})),
            ("too big", serde_json::json!({"mime": "image/png", "data": b64(&vec![1u8; MAX_HERO_IMAGE + 1])})),
            ("huge", serde_json::json!({"mime": "image/png", "data": "A".repeat(MAX_HERO_IMAGE * 2)})),
            ("not an object", serde_json::json!("image/png")),
        ] {
            let h = slide(serde_json::json!({"title": "Cupid", "image": image})).hero.unwrap();
            assert_eq!((h.title.as_str(), h.image.is_none()), ("Cupid", true), "{name}");
        }
        assert!(slide(serde_json::json!({"title": "x", "image": {"mime": "image/png", "data": b64(&vec![1u8; MAX_HERO_IMAGE])}})).hero.unwrap().image.is_some(), "the biggest allowed");
    }

    #[test]
    fn the_words_of_a_hero_are_clipped_and_a_hero_that_is_not_one_is_none() {
        let h = slide(serde_json::json!({"title": "t".repeat(300), "sub": "a\u{7}b\nc"})).hero.unwrap();
        assert_eq!(h.title.chars().count(), LEN_HERO);
        assert_eq!(h.sub, "ab c");
        for bad in [serde_json::json!("hero"), serde_json::json!(7), serde_json::json!(null)] {
            assert!(slide(bad).hero.is_none());
        }
    }

    #[test]
    fn rows_have_a_tone() {
        let rows = |tone: serde_json::Value| match parse(serde_json::json!({"t": "slide", "rev": 1, "rows": [{"id": "a", "text": "A", "tone": tone}]}).to_string().as_bytes()) {
            Parsed::Msg(FromExt::Slide(s)) => s.sections[0].rows[0].tone,
            other => panic!("{other:?}"),
        };
        assert_eq!(rows(serde_json::json!("good")), Tone::Good);
        assert_eq!(rows(serde_json::json!("warn")), Tone::Warn);
        assert_eq!(rows(serde_json::json!("muted")), Tone::Muted);
        assert_eq!(rows(serde_json::json!("purple")), Tone::None, "an unknown tone is none");
        assert_eq!(rows(serde_json::json!(5)), Tone::None);
    }
}

//! Extensions, app side: the manifests found on disk, which of them the user turned on, the programs the notch started,
//! and what each extension last showed. `ext.rs` is the pipe, `extproto.rs` the rules; this ties them to the notch.
//! The notch starts an enabled extension itself (no window, no shell) and starts it again, with a growing delay, when it
//! ends on its own. Turning it off sends `quit`, and ends the program if it is still there three seconds later.

#![allow(dead_code)] // wired up in the next steps of the extension host

use super::*;
use crate::ext::{self, Evt, Host};
use crate::extproto::{DetailMsg, FromExt, Manifest, SlideMsg};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// After `quit`, a program still running this much later is ended.
const QUIT_GRACE: Duration = Duration::from_secs(3);
/// A connection that held this long counts as healthy: the restart delays start over.
const HEALTHY: Duration = Duration::from_secs(60);

/// Delay before the next start of a program that keeps ending: 5 s, 15 s, 60 s, then every 5 minutes.
pub(crate) fn backoff(fails: u32) -> Duration {
    Duration::from_secs(match fails {
        0 | 1 => 5,
        2 => 15,
        3 => 60,
        _ => 300,
    })
}

/// One extension: its manifest (or why it is no good) and what the notch knows about it while it runs.
pub(crate) struct Ext {
    /// Name of the folder under `extensions`.
    pub(crate) folder: String,
    pub(crate) manifest: std::result::Result<Manifest, String>,
    child: Option<Child>,
    fails: u32,
    retry_at: Option<Instant>,
    connected_at: Option<Instant>,
    stop_at: Option<Instant>,
    /// It said `bye`: it ended on purpose, so it is not started again until the user turns it off and on.
    parked: bool,
    pub(crate) connected: bool,
    /// The last `status` text it sent.
    pub(crate) status: String,
    /// Why it is not running: could not start, ended, sent broken data.
    pub(crate) problem: Option<String>,
    pub(crate) slide: Option<SlideMsg>,
    pub(crate) detail: Option<DetailMsg>,
    pub(crate) badge: bool,
    /// What the notch remembers about the page it shows (scroll, tab, hit boxes).
    pub(crate) view: crate::extslide::ExtView,
}

/// What the Settings page says about an extension.
#[derive(Debug, PartialEq)]
pub(crate) enum Status {
    /// The manifest is no good.
    Invalid(String),
    Off,
    Starting,
    /// Connected, with the extension's own status text (may be empty).
    Connected(String),
    /// Not running: why, and seconds until the next try (None = no try planned).
    Problem(String, Option<u64>),
    /// Ended on purpose.
    Parked,
}

impl Ext {
    fn new(folder: String, manifest: std::result::Result<Manifest, String>) -> Ext {
        Ext {
            folder,
            manifest,
            child: None,
            fails: 0,
            retry_at: None,
            connected_at: None,
            stop_at: None,
            parked: false,
            connected: false,
            status: String::new(),
            problem: None,
            slide: None,
            detail: None,
            badge: false,
            view: Default::default(),
        }
    }

    /// The notch started a program for it and it has not ended.
    pub(crate) fn child_running(&self) -> bool {
        self.child.is_some()
    }

    pub(crate) fn id(&self) -> &str {
        self.manifest.as_ref().map_or(self.folder.as_str(), |m| m.id.as_str())
    }

    pub(crate) fn status(&self, enabled: bool, now: Instant) -> Status {
        if let Err(e) = &self.manifest {
            return Status::Invalid(e.clone());
        }
        if !enabled && !self.connected {
            return Status::Off;
        }
        if self.connected {
            Status::Connected(self.status.clone())
        } else if self.parked {
            Status::Parked
        } else if self.child.is_some() {
            Status::Starting
        } else if let Some(p) = &self.problem {
            Status::Problem(p.clone(), self.retry_at.map(|t| t.saturating_duration_since(now).as_secs()))
        } else {
            Status::Starting
        }
    }

    /// Forget what the connection showed.
    fn clear_view(&mut self) {
        self.slide = None;
        self.detail = None;
        self.badge = false;
        self.status.clear();
    }

    fn end_child(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// What the UI should do after events were applied.
#[derive(Debug, PartialEq)]
pub(crate) enum Out {
    Redraw,
    /// An extension asked for a toast (not muted, not too soon).
    Toast { id: String, name: String, title: String, sub: String, count: u32 },
}

#[derive(Default)]
pub(crate) struct ExtState {
    pub(crate) list: Vec<Ext>,
    serving: bool,
    /// Tests: what the notch would have sent to extensions, and the last text it would have copied.
    #[cfg(test)]
    pub(crate) sent: Vec<(String, Vec<u8>)>,
    #[cfg(test)]
    pub(crate) copied_text: Option<String>,
}

fn start_error(e: &std::io::Error) -> String {
    match (e.kind(), e.raw_os_error()) {
        // 4551: blocked by Application Control; 1260: by group policy
        (_, Some(4551)) | (_, Some(1260)) => "blocked by a policy".to_string(),
        // 2: no such file, 3: no such folder, 267: the folder to start in is missing
        (_, Some(2)) | (_, Some(3)) | (_, Some(267)) | (std::io::ErrorKind::NotFound, _) => "file not found".to_string(),
        (std::io::ErrorKind::PermissionDenied, _) => "access denied".to_string(),
        _ => "could not start".to_string(),
    }
}

fn spawn(m: &Manifest) -> std::result::Result<Child, String> {
    let mut c = Command::new(&m.exe);
    c.args(&m.args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW);
    if let Some(dir) = Path::new(&m.exe).parent() {
        c.current_dir(dir);
    }
    c.spawn().map_err(|e| start_error(&e))
}

impl ExtState {
    /// Tests: an extension that is already known and valid (with a slide if `slide`).
    #[cfg(test)]
    pub(crate) fn test_add(&mut self, id: &str, name: &str, slide: bool) {
        let m = Manifest { id: id.into(), name: name.into(), version: "1".into(), exe: r"C:\x\x.exe".into(), args: vec![], slide };
        self.list.push(Ext::new(id.into(), Ok(m)));
    }

    pub(crate) fn find(&self, id: &str) -> Option<&Ext> {
        self.list.iter().find(|e| e.id() == id)
    }

    pub(crate) fn find_mut(&mut self, id: &str) -> Option<&mut Ext> {
        self.list.iter_mut().find(|e| e.id() == id)
    }

    /// Read every `<dir>/<id>/extension.json`. Extensions already known keep what the notch knows about them.
    pub(crate) fn scan(&mut self, dir: &Path) {
        let mut old: Vec<Ext> = std::mem::take(&mut self.list);
        let mut found: Vec<Ext> = Vec::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let folder = entry.file_name().to_string_lossy().into_owned();
            let manifest = match std::fs::read_to_string(entry.path().join("extension.json")) {
                Ok(text) if text.len() <= 16 * 1024 => Manifest::parse(&folder, &text).map_err(str::to_string),
                Ok(_) => Err("manifest too big".to_string()),
                Err(_) => Err("no extension.json".to_string()),
            };
            match old.iter().position(|o| o.folder == folder) {
                Some(i) => {
                    let mut e = old.swap_remove(i);
                    e.manifest = manifest;
                    found.push(e);
                }
                None => found.push(Ext::new(folder, manifest)),
            }
        }
        for mut gone in old {
            gone.end_child();
        }
        found.sort_by(|a, b| a.folder.cmp(&b.folder));
        self.list = found;
    }

    /// (id, exe, enabled) of every valid manifest, for `Host::set_allowed`.
    pub(crate) fn allowed(&self, on: &[String]) -> Vec<(String, String, bool)> {
        self.list.iter().filter_map(|e| e.manifest.as_ref().ok()).map(|m| (m.id.clone(), m.exe.clone(), on.contains(&m.id))).collect()
    }

    /// Start, restart and stop programs to match what is turned on. Called every half second or so.
    pub(crate) fn tick(&mut self, now: Instant, on: &[String], host: &Host) {
        for e in &mut self.list {
            let Ok(m) = e.manifest.clone() else {
                continue;
            };
            let enabled = on.contains(&m.id);
            if let Some(c) = e.child.as_mut() {
                if let Ok(Some(code)) = c.try_wait() {
                    e.child = None;
                    if enabled && !e.parked && e.stop_at.is_none() {
                        e.fails += 1;
                        e.problem = Some(format!("ended (code {})", code.code().unwrap_or(-1)));
                        e.retry_at = Some(now + backoff(e.fails));
                    }
                }
            }
            if !enabled {
                if e.child.is_some() || e.connected {
                    match e.stop_at {
                        None => {
                            host.disconnect(&m.id);
                            e.stop_at = Some(now + QUIT_GRACE);
                        }
                        Some(t) if now >= t => e.end_child(),
                        Some(_) => {}
                    }
                } else {
                    (e.fails, e.retry_at, e.parked, e.problem, e.stop_at) = (0, None, false, None, None);
                }
                continue;
            }
            e.stop_at = None;
            if e.connected {
                if e.connected_at.is_some_and(|t| now.saturating_duration_since(t) >= HEALTHY) {
                    e.fails = 0;
                }
                continue;
            }
            if e.child.is_none() && !e.parked && e.retry_at.map_or(true, |t| now >= t) {
                match spawn(&m) {
                    Ok(c) => {
                        e.child = Some(c);
                        e.problem = None;
                        e.retry_at = None;
                    }
                    Err(why) => {
                        e.fails += 1;
                        e.problem = Some(why);
                        e.retry_at = Some(now + backoff(e.fails));
                    }
                }
            }
        }
    }

    /// Stop one extension and start it again on the next `tick` (if it is on).
    pub(crate) fn restart(&mut self, id: &str, host: &Host) {
        host.disconnect(id);
        if let Some(e) = self.find_mut(id) {
            e.end_child();
            (e.fails, e.retry_at, e.parked, e.problem, e.stop_at) = (0, None, false, None, None);
        }
    }

    /// Take in what the pipe host collected.
    pub(crate) fn apply(&mut self, events: Vec<Evt>, muted: &[String], now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        for ev in events {
            match ev {
                Evt::Connected { id } => {
                    if let Some(e) = self.find_mut(&id) {
                        e.connected = true;
                        e.connected_at = Some(now);
                        e.parked = false;
                        e.problem = None;
                        e.clear_view();
                    }
                }
                Evt::Gone { id, why } => {
                    if let Some(e) = self.find_mut(&id) {
                        e.connected = false;
                        e.clear_view();
                        if why == "bad data" {
                            e.problem = Some("sent broken data".to_string());
                        }
                    }
                }
                Evt::Msg { id, msg } => {
                    let Some(e) = self.find_mut(&id) else { continue };
                    match msg {
                        FromExt::Slide(s) => {
                            e.slide = Some(s);
                            e.detail = None;
                            // the extension answers the tab the user clicked with its own
                            e.view.tab_sel = None;
                        }
                        FromExt::Detail(d) => {
                            if e.detail.as_ref().map_or(true, |old| old.for_id != d.for_id) {
                                e.view.dscroll = 0;
                            }
                            e.detail = Some(d);
                        }
                        FromExt::Badge(on) => e.badge = on,
                        FromExt::Status(t) => e.status = t,
                        FromExt::Bye => e.parked = true,
                        FromExt::Toast { title, sub, count } => {
                            if !muted.contains(&id) {
                                let name = e.manifest.as_ref().map(|m| m.name.clone()).unwrap_or_default();
                                out.push(Out::Toast { id, name, title, sub, count });
                            }
                            continue;
                        }
                        FromExt::Hello { .. } => continue,
                    }
                    out.push(Out::Redraw);
                }
            }
        }
        out
    }
}

impl App {
    /// Find the extensions and, if there are any, open the pipe for them.
    pub(crate) fn ext_start(&mut self) {
        let dir = data_path("extensions");
        self.ext.scan(&dir);
        self.ext_sync();
        self.ext_serve();
        if self.order != self.slide_order() {
            self.apply_slide_cfg();
        }
    }

    /// Open the pipe once there is something to serve.
    fn ext_serve(&mut self) {
        if self.ext.serving || !self.ext.list.iter().any(|e| e.manifest.is_ok()) {
            return;
        }
        let Some(name) = ext::pipe_name() else { return };
        match ext::host().serve(&name, self.hwnd) {
            Ok(()) => self.ext.serving = true,
            Err(why) => perf::note(&format!("extensions: no pipe: {why}")),
        }
    }

    /// Tell the pipe host which programs may connect, and what a welcome says.
    pub(crate) fn ext_sync(&mut self) {
        let host = ext::host();
        host.set_allowed(self.ext.allowed(&self.cfg.ext_on));
        let lang = if self.cfg.lang == 1 { "id" } else { "en" };
        host.set_info(lang, accent::ACCENTS[self.cfg.accent.min(accent::ACCENTS.len() - 1)].main);
    }

    /// About every half second: start, restart and stop the programs. True if the page needs a redraw (the "Copied" mark
    /// went away).
    pub(crate) fn ext_tick(&mut self) -> bool {
        let on = self.cfg.ext_on.clone();
        self.ext.tick(Instant::now(), &on, ext::host());
        let mut redraw = false;
        for e in &mut self.ext.list {
            if e.view.copied.is_some_and(|t| t.elapsed().as_secs_f32() >= extslide::COPIED_SECS) {
                e.view.copied = None;
                redraw = true;
            }
        }
        redraw && self.cur_ext().is_some()
    }

    /// The carousel: the slides the user turned on, then the slides of the extensions that are on (at most four).
    pub(crate) fn slide_order(&self) -> Vec<SlideId> {
        let mut v = slides::visible_order(&self.slide_cfg());
        let on = |e: &Ext| e.manifest.as_ref().is_ok_and(|m| m.slide && self.cfg.ext_on.contains(&m.id));
        v.extend(self.ext.list.iter().enumerate().filter(|(_, e)| on(e)).take(4).map(|(i, _)| SlideId::Ext(i as u8)));
        v
    }

    /// An extension slide has something new: the dot of slide `i` gets a marker.
    pub(crate) fn ext_dot_badge(&self, i: usize) -> bool {
        matches!(self.order.get(i), Some(SlideId::Ext(k)) if self.ext.list.get(*k as usize).is_some_and(|e| e.badge))
    }

    /// `WM_EXT`: the pipe host has something for us. True if a redraw is needed.
    pub(crate) fn ext_events(&mut self) -> bool {
        let events = ext::host().take_events();
        if events.is_empty() {
            return false;
        }
        let muted = self.cfg.ext_mute.clone();
        let out = self.ext.apply(events, &muted, Instant::now());
        !out.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn whoami() -> String {
        format!(r"{}\System32\whoami.exe", std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
    }

    fn manifest(id: &str, exe: &str) -> String {
        json!({"id": id, "name": "Test", "exe": exe, "proto": 1}).to_string()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hesewake-ext-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(dir: &Path, folder: &str, text: &str) {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
        std::fs::write(dir.join(folder).join("extension.json"), text).unwrap();
    }

    fn state_with(id: &str, exe: &str) -> ExtState {
        let dir = scratch(id);
        put(&dir, id, &manifest(id, exe));
        let mut s = ExtState::default();
        s.scan(&dir);
        s
    }

    #[test]
    fn restart_delays_grow_and_cap() {
        let secs: Vec<u64> = (0..7).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(secs, vec![5, 5, 15, 60, 300, 300, 300]);
    }

    #[test]
    fn scan_reads_manifests_and_says_what_is_wrong() {
        let dir = scratch("scan");
        put(&dir, "good-one", &manifest("good-one", &whoami()));
        put(&dir, "wrong-id", &manifest("other", &whoami()));
        put(&dir, "broken", "{ nope");
        std::fs::create_dir_all(dir.join("empty")).unwrap();
        std::fs::write(dir.join("a-file.txt"), "x").unwrap();
        let mut s = ExtState::default();
        s.scan(&dir);
        let folders: Vec<&str> = s.list.iter().map(|e| e.folder.as_str()).collect();
        assert_eq!(folders, vec!["broken", "empty", "good-one", "wrong-id"], "folders only, sorted");
        assert!(s.find("good-one").unwrap().manifest.is_ok());
        assert_eq!(s.list[0].manifest.as_ref().unwrap_err(), "not valid JSON");
        assert_eq!(s.list[1].manifest.as_ref().unwrap_err(), "no extension.json");
        assert_eq!(s.list[3].manifest.as_ref().unwrap_err(), "id differs from the folder name");
        assert_eq!(s.allowed(&["good-one".into()]), vec![("good-one".to_string(), whoami(), true)], "only valid manifests are allowed in");
        assert_eq!(s.allowed(&[])[0].2, false);
        let mut none = ExtState::default();
        none.scan(&dir.join("does-not-exist"));
        assert!(none.list.is_empty(), "no folder, no extensions");
    }

    #[test]
    fn a_rescan_keeps_what_the_notch_knows() {
        let dir = scratch("rescan");
        put(&dir, "keep-me", &manifest("keep-me", &whoami()));
        let mut s = ExtState::default();
        s.scan(&dir);
        s.find_mut("keep-me").unwrap().status = "Syncing".into();
        s.scan(&dir);
        assert_eq!(s.find("keep-me").unwrap().status, "Syncing");
    }

    #[test]
    fn an_enabled_program_is_started_and_started_again_after_a_delay() {
        let mut s = state_with("starter", &whoami());
        let host = Host::new("t");
        let on = vec!["starter".to_string()];
        let t0 = Instant::now();
        s.tick(t0, &on, &host);
        assert!(s.list[0].child.is_some(), "started");
        assert_eq!(s.list[0].status(true, t0), Status::Starting);
        // it ends by itself (whoami prints and exits)
        let mut now = t0;
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            now = Instant::now();
            s.tick(now, &on, &host);
            if s.list[0].child.is_none() {
                break;
            }
        }
        let e = &s.list[0];
        assert!(e.child.is_none() && e.fails == 1, "it ended, and that was counted");
        assert!(matches!(e.status(true, now), Status::Problem(ref p, Some(n)) if p.starts_with("ended") && n <= 5), "{:?}", e.status(true, now));
        s.tick(now + Duration::from_secs(1), &on, &host);
        assert!(s.list[0].child.is_none(), "not before the delay");
        s.tick(now + Duration::from_secs(6), &on, &host);
        assert!(s.list[0].child.is_some(), "after it");
        s.list[0].end_child();
    }

    #[test]
    fn a_program_that_cannot_start_is_reported() {
        let mut s = state_with("lost", r"C:\hesewake-no-such-folder\nothing.exe");
        let host = Host::new("t");
        let now = Instant::now();
        s.tick(now, &["lost".to_string()], &host);
        let e = &s.list[0];
        assert_eq!((e.fails, e.problem.as_deref()), (1, Some("file not found")));
        assert_eq!(e.status(true, now), Status::Problem("file not found".into(), Some(5)));
        s.tick(now + Duration::from_secs(1), &["lost".to_string()], &host);
        assert_eq!(s.list[0].fails, 1, "no second try before the delay");
        assert_eq!(s.list[0].status(false, now), Status::Off, "off wins over an old problem");
    }

    #[test]
    fn switching_off_resets_and_ends_the_program() {
        let mut s = state_with("goner", &whoami());
        let host = Host::new("t");
        let (on, off) = (vec!["goner".to_string()], Vec::<String>::new());
        let t0 = Instant::now();
        s.tick(t0, &on, &host);
        s.list[0].fails = 3;
        s.tick(t0, &off, &host);
        assert!(s.list[0].stop_at.is_some() || s.list[0].child.is_none(), "asked to stop (or it was gone already)");
        s.tick(t0 + Duration::from_secs(4), &off, &host);
        s.tick(t0 + Duration::from_secs(5), &off, &host);
        let e = &s.list[0];
        assert!(e.child.is_none() && e.fails == 0 && e.retry_at.is_none() && e.problem.is_none(), "back to a clean slate");
    }

    #[test]
    fn events_update_what_an_extension_shows() {
        let mut s = state_with("shower", &whoami());
        let now = Instant::now();
        let id = || "shower".to_string();
        let slide = |rev| FromExt::Slide(crate::extproto::SlideMsg { rev, title: "T".into(), state: crate::extproto::State::Ready, message: String::new(), setup_label: String::new(), tabs: vec![], tab: String::new(), actions: vec![], sections: vec![], stepper: None, back: false, foot: String::new() });
        let out = s.apply(vec![Evt::Connected { id: id() }, Evt::Msg { id: id(), msg: slide(1) }, Evt::Msg { id: id(), msg: FromExt::Badge(true) }, Evt::Msg { id: id(), msg: FromExt::Status("Syncing".into()) }], &[], now);
        assert!(out.contains(&Out::Redraw));
        let e = s.find("shower").unwrap();
        assert!(e.connected && e.slide.is_some() && e.badge);
        assert_eq!(e.status(true, now), Status::Connected("Syncing".into()));
        let toast = FromExt::Toast { title: "#1".into(), sub: "New".into(), count: 1 };
        let out = s.apply(vec![Evt::Msg { id: id(), msg: toast.clone() }], &[], now);
        assert_eq!(out, vec![Out::Toast { id: id(), name: "Test".into(), title: "#1".into(), sub: "New".into(), count: 1 }]);
        assert!(s.apply(vec![Evt::Msg { id: id(), msg: toast }], &[id()], now).is_empty(), "muted: no toast");
        s.apply(vec![Evt::Msg { id: id(), msg: FromExt::Bye }, Evt::Gone { id: id(), why: "bye" }], &[], now);
        let e = s.find("shower").unwrap();
        assert!(!e.connected && e.slide.is_none() && !e.badge && e.status.is_empty(), "the view goes with the connection");
        assert_eq!(e.status(true, now), Status::Parked, "it ended on purpose: not started again by itself");
        s.apply(vec![Evt::Gone { id: id(), why: "bad data" }], &[], now);
        assert_eq!(s.find("shower").unwrap().problem.as_deref(), Some("sent broken data"));
        assert!(s.apply(vec![Evt::Msg { id: "stranger".into(), msg: FromExt::Bye }], &[], now).is_empty(), "an unknown id is ignored");
    }
}

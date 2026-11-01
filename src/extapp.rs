//! Extensions, app side: the manifests found on disk, which of them the user turned on, the programs the notch started,
//! and what each extension last showed. `ext.rs` is the pipe, `extproto.rs` the rules; this ties them to the notch.
//! The notch starts an enabled extension itself (no window, no shell) and starts it again, with a growing delay, when it
//! ends on its own. Turning it off sends `quit`, and ends the program if it is still there three seconds later.

#![allow(dead_code)] // wired up in the next steps of the extension host

use super::*;
use crate::ext::{self, Evt, Host};
use crate::extproto::{self as proto, DetailMsg, FromExt, Manifest, SlideMsg};
use std::os::windows::io::AsRawHandle;
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
    /// The limits the program runs under; letting go of it ends the program.
    job: Option<crate::extjob::Job>,
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
            job: None,
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
        self.job = None;
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
    /// Tests: the folder everything is read from and removed in. Without one a test never touches the user's own folder.
    #[cfg(test)]
    pub(crate) test_root: Option<std::path::PathBuf>,
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

/// Start the program, suspended, put it under its limits (a Job Object: dies with the notch, no other programs unless the
/// manifest allows them) and only then let it run. If the limits cannot be set the program is not started.
fn spawn(m: &Manifest) -> std::result::Result<(Child, crate::extjob::Job), String> {
    let mut c = Command::new(&m.exe);
    c.args(&m.args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW | crate::extjob::CREATE_SUSPENDED);
    if let Some(dir) = Path::new(&m.exe).parent() {
        c.current_dir(dir);
    }
    // the full name of the pipe, so the extension need not work it out from the user's SID
    if let Some(pipe) = ext::pipe_name() {
        c.env("HESEWAKE_NOTCH_PIPE", pipe);
    }
    let mut child = c.spawn().map_err(|e| start_error(&e))?;
    let handle = windows::Win32::Foundation::HANDLE(child.as_raw_handle());
    let job = crate::extjob::Job::new(m.processes);
    match job {
        Some(job) if job.assign(handle) && crate::extjob::resume(handle) => Ok((child, job)),
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            Err("could not limit the program".to_string())
        }
    }
}

impl ExtState {
    /// Tests: an extension that is already known and valid (with a slide if `slide`).
    #[cfg(test)]
    pub(crate) fn test_add(&mut self, id: &str, name: &str, slide: bool) {
        let m = Manifest { id: id.into(), name: name.into(), version: "1".into(), exe: r"C:\x\x.exe".into(), args: vec![], slide, panel: false, processes: false };
        self.list.push(Ext::new(id.into(), Ok(m)));
    }

    /// Tests: a folder whose manifest is no good.
    #[cfg(test)]
    pub(crate) fn test_add_bad(&mut self, folder: &str, why: &str) {
        self.list.push(Ext::new(folder.into(), Err(why.into())));
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
                    e.job = None;
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
                    Ok((c, job)) => {
                        e.child = Some(c);
                        e.job = Some(job);
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

    /// Take extension `i` out of the notch: stop it and delete its manifest folder under `root` (what an installer put
    /// there goes with it; a program installed somewhere else stays). The folder must be a plain directory directly under
    /// `root`: a name with a separator or `..`, or a folder that is really a link to somewhere else, is refused and nothing
    /// is deleted. Returns the extension's id.
    pub(crate) fn remove(&mut self, i: usize, root: &Path, host: &Host) -> std::result::Result<String, &'static str> {
        let e = self.list.get(i).ok_or("no such extension")?;
        let folder = e.folder.clone();
        if folder.is_empty() || folder == "." || folder == ".." || folder.contains(['/', '\\', ':']) {
            return Err("not a plain folder name");
        }
        let dir = root.join(&folder);
        let meta = std::fs::symlink_metadata(&dir).map_err(|_| "the folder is gone")?;
        // 0x400: a reparse point, which is what a junction or a link is on Windows
        if !meta.is_dir() || meta.file_type().is_symlink() || std::os::windows::fs::MetadataExt::file_attributes(&meta) & 0x400 != 0 {
            return Err("the folder is a link, not a folder");
        }
        let id = e.id().to_string();
        host.disconnect(&id);
        self.list[i].end_child();
        std::fs::remove_dir_all(&dir).map_err(|_| "the folder could not be deleted")?;
        self.list.remove(i);
        Ok(id)
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

/// What the Settings page says about an extension, in words.
pub(crate) fn status_text(e: &Ext, enabled: bool, now: Instant) -> String {
    match e.status(enabled, now) {
        Status::Invalid(why) => fill(tr("Invalid: {}"), &[&why]),
        Status::Off => tr("Off").to_string(),
        Status::Starting => tr("Starting\u{2026}").to_string(),
        Status::Connected(s) if s.is_empty() => tr("Connected").to_string(),
        Status::Connected(s) => s,
        Status::Problem(why, None) => fill(tr("Not running: {}"), &[&why]),
        Status::Problem(why, Some(n)) => format!("{} \u{b7} {}", fill(tr("Not running: {}"), &[&why]), fill(tr("Next try in {} s"), &[&n])),
        Status::Parked => tr("Stopped by the extension").to_string(),
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

    pub(crate) fn ext_id(&self, i: usize) -> Option<String> {
        self.ext.list.get(i).and_then(|e| e.manifest.as_ref().ok()).map(|m| m.id.clone())
    }

    /// Send a message to the extension `id`. (Tests keep it instead of using the pipe.)
    pub(crate) fn ext_send_to(&mut self, id: &str, frame: Vec<u8>) {
        #[cfg(test)]
        self.ext.sent.push((id.to_string(), frame));
        #[cfg(not(test))]
        let _ = ext::host().send(id, frame);
    }

    /// Settings > Extensions: switch extension `i` on or off. Its slide and its program follow at once.
    pub(crate) fn ext_set_on(&mut self, i: usize) {
        let Some(id) = self.ext_id(i) else { return };
        match self.cfg.ext_on.iter().position(|x| *x == id) {
            Some(p) => {
                self.cfg.ext_on.remove(p);
            }
            None if self.cfg.ext_on.len() < 16 => self.cfg.ext_on.push(id),
            None => return,
        }
        self.save_settings();
        self.ext_sync();
        self.apply_slide_cfg();
        self.ext_tick();
    }

    pub(crate) fn ext_toggle_mute(&mut self, i: usize) {
        let Some(id) = self.ext_id(i) else { return };
        match self.cfg.ext_mute.iter().position(|x| *x == id) {
            Some(p) => {
                self.cfg.ext_mute.remove(p);
            }
            None if self.cfg.ext_mute.len() < 16 => self.cfg.ext_mute.push(id),
            None => return,
        }
        self.save_settings();
    }

    /// The Settings button of an extension: its pages in a panel of the Settings window if its manifest says `panel`, else
    /// a message that makes it open its own window. Only while it is connected.
    pub(crate) fn ext_open_config(&mut self, i: usize) {
        let Some(id) = self.ext_id(i).filter(|_| self.ext.list[i].connected) else { return };
        if self.ext.list[i].manifest.as_ref().is_ok_and(|m| m.panel) {
            self.ext_panel_close();
            self.sw.panel = Some(i);
            self.sw.to_top();
            // it is looked at now, like a slide that comes into view
            self.ext_send_to(&id, proto::visible(true));
        } else {
            self.ext_send_to(&id, proto::config());
        }
    }

    /// Leave the panel (back to the list, another page of the window, or the window closed).
    pub(crate) fn ext_panel_close(&mut self) {
        if let Some(i) = self.sw.panel.take() {
            self.sw.to_top();
            if let Some(id) = self.ext_id(i) {
                self.ext_send_to(&id, proto::visible(false));
            }
        }
    }

    /// Back out of a detail page: gone at once, and the extension is told.
    pub(crate) fn ext_panel_detail_back(&mut self) {
        let Some(i) = self.sw.panel else { return };
        if let (Some(id), Some(e)) = (self.ext_id(i), self.ext.list.get_mut(i)) {
            e.detail = None;
            e.view.dscroll = 0;
            self.ext_send_to(&id, proto::back());
        }
    }

    pub(crate) fn ext_panel_row(&mut self, n: usize) {
        let Some(i) = self.sw.panel else { return };
        let row = self.ext.list.get(i).and_then(|e| e.slide.as_ref()).and_then(|s| crate::extslide::rows_of(s).get(n).map(|r| r.id.clone()));
        if let (Some(ext), Some(row)) = (self.ext_id(i), row) {
            self.ext_send_to(&ext, proto::row(&row));
        }
    }

    pub(crate) fn ext_panel_act(&mut self, n: usize) {
        let Some(i) = self.sw.panel else { return };
        if let Some(b) = self.ext.list.get(i).and_then(|e| e.slide.as_ref()).and_then(|s| s.actions.get(n)).cloned() {
            self.ext_press(i, &b);
        }
    }

    pub(crate) fn ext_panel_button(&mut self, n: usize) {
        let Some(i) = self.sw.panel else { return };
        if let Some(b) = self.ext.list.get(i).and_then(|e| e.detail.as_ref()).and_then(|d| d.buttons.get(n)).cloned() {
            self.ext_press(i, &b);
        }
    }

    pub(crate) fn ext_panel_tab(&mut self, n: usize) {
        let Some(i) = self.sw.panel else { return };
        let tab = self.ext.list.get(i).and_then(|e| e.slide.as_ref()).and_then(|s| s.tabs.get(n)).map(|t| t.id.clone());
        if let (Some(ext), Some(tab)) = (self.ext_id(i), tab) {
            self.ext.list[i].view.tab_sel = Some(tab.clone());
            self.ext_send_to(&ext, proto::tab(&tab));
        }
    }

    pub(crate) fn ext_panel_step(&mut self, dir: i32) {
        if let Some(id) = self.sw.panel.and_then(|i| self.ext_id(i)) {
            self.ext_send_to(&id, proto::step(dir));
        }
    }

    /// Retry or the setup button of a page: the extension is told which.
    pub(crate) fn ext_panel_action(&mut self, id: &str) {
        if let Some(ext) = self.sw.panel.and_then(|i| self.ext_id(i)) {
            self.ext_send_to(&ext, proto::action(id));
        }
    }

    pub(crate) fn ext_restart(&mut self, i: usize) {
        if let Some(id) = self.ext_id(i) {
            self.ext.restart(&id, ext::host());
            self.ext_tick();
        }
    }

    /// Show the folder of extension `i` (or the folder of all of them) in Explorer.
    pub(crate) fn ext_open_folder(&mut self, i: Option<usize>) {
        let root = data_path("extensions");
        if !cfg!(test) {
            let _ = std::fs::create_dir_all(&root);
        }
        let path = match i.and_then(|i| self.ext.list.get(i)) {
            Some(e) => root.join(&e.folder),
            None => root,
        };
        sys::open_url(&path.to_string_lossy());
    }

    /// The Remove button, in two steps: the first press asks "Sure?", a second one within three seconds does it.
    pub(crate) fn ext_remove(&mut self, i: usize) {
        let root = self.ext_root();
        self.ext_remove_in(i, &root);
    }

    /// The folder the extensions' manifests are in. (A test has one of its own, never the user's.)
    pub(crate) fn ext_root(&self) -> std::path::PathBuf {
        #[cfg(test)]
        return self.ext.test_root.clone().unwrap_or_else(|| std::env::temp_dir().join("hesewake-test-no-extensions"));
        #[cfg(not(test))]
        data_path("extensions")
    }

    pub(crate) fn ext_remove_in(&mut self, i: usize, root: &Path) {
        let sure = self.sw.remove_confirm.is_some_and(|(n, t)| n == i && t.elapsed().as_secs_f32() < 3.0);
        if !sure {
            self.sw.remove_confirm = Some((i, Instant::now()));
            return;
        }
        self.sw.remove_confirm = None;
        self.ext_panel_close();
        let id = match self.ext.remove(i, root, ext::host()) {
            Ok(id) => id,
            Err(why) => {
                perf::note(&format!("extensions: not removed: {why}"));
                return;
            }
        };
        self.cfg.ext_on.retain(|x| *x != id);
        self.cfg.ext_mute.retain(|x| *x != id);
        self.save_settings();
        self.ext_sync();
        self.apply_slide_cfg();
    }

    /// Look at the extensions folder again (a new extension was installed).
    pub(crate) fn ext_rescan(&mut self) {
        let root = self.ext_root();
        self.ext_rescan_in(&root);
    }

    pub(crate) fn ext_rescan_in(&mut self, dir: &Path) {
        self.ext.scan(dir);
        self.ext_sync();
        self.ext_serve();
        self.apply_slide_cfg();
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
        // the Settings window shows the pages of an extension in panel mode, and its status
        if self.sw.hwnd != 0 {
            self.sw_kick();
        }
        self.ext_out(out)
    }

    /// Act on what `ExtState::apply` found: toasts (not over a full-screen app). True if the screen should be redrawn.
    pub(crate) fn ext_out(&mut self, out: Vec<Out>) -> bool {
        let changed = !out.is_empty();
        for o in out {
            if let Out::Toast { id, title, sub, count, .. } = o {
                if !sys::user_fullscreen() {
                    self.push_toast(toast::ToastKind::Ext { id, title, sub, count });
                }
            }
        }
        changed
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
        let slide = |rev| FromExt::Slide(crate::extproto::SlideMsg { rev, title: "T".into(), state: crate::extproto::State::Ready, message: String::new(), setup_label: String::new(), tabs: vec![], tab: String::new(), actions: vec![], sections: vec![], stepper: None, back: false, hero: None, foot: String::new() });
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

#[cfg(test)]
mod job_tests {
    use super::*;
    use serde_json::json;

    fn system32(exe: &str) -> String {
        format!(r"{}\System32\{exe}", std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
    }

    /// An extension whose program is `exe` with `args`, found by a scan of a folder of its own.
    fn state(id: &str, exe: &str, args: &[&str], extra: serde_json::Value) -> ExtState {
        let dir = std::env::temp_dir().join(format!("hesewake-job-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(id)).unwrap();
        let mut m = json!({"id": id, "name": id, "exe": exe, "args": args, "proto": 1});
        for (k, v) in extra.as_object().into_iter().flatten() {
            m[k] = v.clone();
        }
        std::fs::write(dir.join(id).join("extension.json"), m.to_string()).unwrap();
        let mut s = ExtState::default();
        s.scan(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        s
    }

    /// Start it and wait until it has ended; what `tick` made of the end.
    fn run_to_the_end(s: &mut ExtState, id: &str) -> String {
        let (host, on) = (Host::new("t"), vec![id.to_string()]);
        s.tick(Instant::now(), &on, &host);
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            s.tick(Instant::now(), &on, &host);
            if s.list[0].child.is_none() {
                break;
            }
        }
        s.list[0].problem.clone().unwrap_or_default()
    }

    #[test]
    fn a_program_cannot_start_other_programs_by_default() {
        // cmd.exe starts whoami.exe: with the limit, that second program does not get to run
        let mut s = state("limited", &system32("cmd.exe"), &["/c", "whoami"], json!({}));
        assert!(s.list[0].manifest.as_ref().is_ok_and(|m| !m.processes), "none unless said otherwise");
        let ended = run_to_the_end(&mut s, "limited");
        assert!(ended.starts_with("ended") && !ended.contains("code 0)"), "the program it tried to start was stopped: {ended}");
        // the same program with the permission: whoami runs, cmd ends well
        let mut s = state("allowed", &system32("cmd.exe"), &["/c", "whoami"], json!({"processes": "allow"}));
        assert!(s.list[0].manifest.as_ref().is_ok_and(|m| m.processes));
        assert_eq!(run_to_the_end(&mut s, "allowed"), "ended (code 0)", "allowed: it works");
    }

    #[test]
    fn a_program_ends_when_the_job_is_let_go() {
        let mut s = state("mortal", &system32("PING.EXE"), &["-n", "30", "127.0.0.1"], json!({}));
        let (host, on) = (Host::new("t"), vec!["mortal".to_string()]);
        s.tick(Instant::now(), &on, &host);
        let e = &mut s.list[0];
        assert!(e.child.is_some() && e.job.is_some(), "it runs, under a job");
        std::thread::sleep(Duration::from_millis(200));
        assert!(e.child.as_mut().unwrap().try_wait().unwrap().is_none(), "still going after a moment");
        e.job = None; // the notch lets go (or ends)
        let mut gone = false;
        for _ in 0..60 {
            if e.child.as_mut().unwrap().try_wait().unwrap().is_some() {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(gone, "the job took the program with it");
    }

    #[test]
    fn ending_a_program_lets_go_of_its_job_too() {
        let mut s = state("tidy", &system32("PING.EXE"), &["-n", "30", "127.0.0.1"], json!({}));
        let (host, on) = (Host::new("t"), vec!["tidy".to_string()]);
        s.tick(Instant::now(), &on, &host);
        s.list[0].end_child();
        assert!(s.list[0].child.is_none() && s.list[0].job.is_none());
    }

    #[test]
    fn the_manifest_says_what_may_be_started() {
        let m = |extra: &str| Manifest::parse("p", &format!(r#"{{"id":"p","name":"P","exe":"C:\\Tools\\p.exe","proto":1{extra}}}"#));
        assert!(!m("").unwrap().processes);
        assert!(!m(r#","processes":"none""#).unwrap().processes);
        assert!(m(r#","processes":"allow""#).unwrap().processes);
        assert!(m(r#","processes":"yes""#).is_err() && m(r#","processes":true"#).is_err(), "anything else is a mistake");
    }
}

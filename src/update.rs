//! Update check: asks GitHub for the latest release (at startup and every 24 hours, or from the menu) and tells the
//! person once per version when a newer one is out. Nothing is downloaded or installed.

use serde_json::Value;

/// The latest published release (drafts and pre-releases are never returned by this endpoint).
pub(crate) const LATEST_URL: &str = "https://api.github.com/repos/dirgaastya/hesewake-notch/releases/latest";
/// Only release pages of this repository are opened.
const RELEASES_PREFIX: &str = "https://github.com/dirgaastya/hesewake-notch/releases/";
/// Largest API answer read.
pub(crate) const MAX_BYTES: usize = 512 * 1024;
/// Release files are only downloaded from here.
const DOWNLOAD_PREFIX: &str = "https://github.com/dirgaastya/hesewake-notch/releases/download/";
/// An installer outside these sizes is not trusted, whatever the release says.
const MIN_INSTALLER: u64 = 500_000;
const MAX_INSTALLER: u64 = 100_000_000;
/// Largest checksum file read.
#[cfg_attr(test, allow(dead_code))] // the download itself is not run in tests
const MAX_SUMS: usize = 64 * 1024;

/// The installer of a release, and where its SHA-256 is published.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Installer {
    /// `HesewakeNotch-Setup-0.10.2.exe`
    pub(crate) file: String,
    pub(crate) url: String,
    pub(crate) size: u64,
    /// The release's `SHA256SUMS` file.
    pub(crate) sums_url: String,
}

/// A release newer than the running version.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Release {
    /// Without the leading `v`, e.g. `0.9.0`.
    pub(crate) version: String,
    /// The release page.
    pub(crate) url: String,
    /// The installer and its checksum file, when the release has both (older releases have no checksum file: then the
    /// toast just opens the release page).
    pub(crate) installer: Option<Installer>,
}

/// The installer asset and the `SHA256SUMS` asset of `version`, both from this repository's release downloads.
fn find_installer(assets: &[Value], version: &str) -> Option<Installer> {
    let file = format!("HesewakeNotch-Setup-{version}.exe");
    let asset = |name: &str| assets.iter().find(|a| a.get("name").and_then(Value::as_str) == Some(name));
    let url_of = |a: &Value| a.get("browser_download_url").and_then(Value::as_str).filter(|u| u.starts_with(DOWNLOAD_PREFIX)).map(str::to_string);
    let (inst, sums) = (asset(&file)?, asset("SHA256SUMS")?);
    let size = inst.get("size").and_then(Value::as_u64).filter(|s| (MIN_INSTALLER..=MAX_INSTALLER).contains(s))?;
    Some(Installer { file, url: url_of(inst)?, size, sums_url: url_of(sums)? })
}

/// The SHA-256 (lowercase hex) listed for `file` in a `SHA256SUMS` text (`<hash>  <name>` or `<hash> *<name>`).
pub(crate) fn expected_sha(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, name) = l.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        (name == file && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hash.to_ascii_lowercase())
    })
}

/// `v1.2.3`, `1.2.3`, `1.2.3-alpha.4` → (1, 2, 3, pre-release?). None if it isn't a version.
pub(crate) fn parse_version(s: &str) -> Option<(u64, u64, u64, bool)> {
    let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
    let s = s.split('+').next().unwrap_or("");
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, !p.is_empty()),
        None => (s, false),
    };
    let mut parts = core.split('.');
    let mut num = || parts.next().filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())).and_then(|p| p.parse::<u64>().ok());
    let (a, b, c) = (num()?, num()?, num()?);
    if parts.next().is_some() {
        return None;
    }
    Some((a, b, c, pre))
}

/// Is `latest` newer than `current`? A pre-release `latest` never is.
pub(crate) fn is_newer(latest: &str, current: &str) -> bool {
    let (Some(l), Some(c)) = (parse_version(latest), parse_version(current)) else { return false };
    if l.3 {
        return false;
    }
    let (lv, cv) = ((l.0, l.1, l.2), (c.0, c.1, c.2));
    lv > cv || (lv == cv && c.3)
}

/// The release in the API's answer, if it is a proper, published release of this repository.
pub(crate) fn parse_release(json: &str) -> Option<Release> {
    let v: Value = serde_json::from_str(json).ok()?;
    let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    if flag("draft") || flag("prerelease") {
        return None;
    }
    let tag = v.get("tag_name").and_then(Value::as_str)?;
    let url = v.get("html_url").and_then(Value::as_str).filter(|u| u.starts_with(RELEASES_PREFIX))?;
    let (_, _, _, pre) = parse_version(tag)?;
    if pre {
        return None;
    }
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    let installer = v.get("assets").and_then(Value::as_array).and_then(|a| find_installer(a, &version));
    Some(Release { version, url: url.to_string(), installer })
}

// ---------------------------------------------------------------- in the app

use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use windows::Foundation::AsyncOperationProgressHandler;
use windows::Web::Http::HttpProgress;

pub(crate) const WM_UPDATE: u32 = WM_APP + 12;
/// The installer download made progress, finished or failed.
pub(crate) const WM_UPDATE_DL: u32 = WM_APP + 14;
/// Automatic checks run at most this often.
pub(crate) const INTERVAL: Duration = Duration::from_secs(24 * 3600);
pub(crate) const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// What an update toast says.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Note {
    Available(Release),
    UpToDate,
    Failed,
    /// The installer is being downloaded (percent).
    Downloading(u8),
    /// The download or its checksum failed: the toast offers the release page instead.
    DownloadFailed(Release),
}

/// Result of the last check: (asked from the menu, the latest release or None when the check failed).
static SLOT: Mutex<Option<(bool, Option<Release>)>> = Mutex::new(None);

#[cfg(test)]
thread_local! {
    static FETCHES: std::cell::RefCell<Vec<bool>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Ask GitHub on a background thread; the answer comes back as `WM_UPDATE`.
pub(crate) fn fetch(hwnd: isize, manual: bool) {
    #[cfg(test)]
    {
        let _ = hwnd;
        FETCHES.with(|f| f.borrow_mut().push(manual));
    }
    #[cfg(not(test))]
    std::thread::spawn(move || {
        let latest = weather::http_get(LATEST_URL, MAX_BYTES).and_then(|body| parse_release(&body));
        if let Ok(mut slot) = SLOT.lock() {
            *slot = Some((manual, latest));
        }
        unsafe {
            let _ = PostMessageW(HWND(hwnd as *mut c_void), WM_UPDATE, WPARAM(0), LPARAM(0));
        }
    });
}

pub(crate) fn take() -> Option<(bool, Option<Release>)> {
    SLOT.lock().ok()?.take()
}

/// The checks started in this test thread (true = from the menu), cleared on read.
#[cfg(test)]
pub(crate) fn fetches() -> Vec<bool> {
    FETCHES.with(|f| std::mem::take(&mut *f.borrow_mut()))
}

/// Progress of the installer download (percent) and its outcome; the UI thread reads them on `WM_UPDATE_DL`.
static DL_PCT: AtomicU8 = AtomicU8::new(0);
static DL_BUSY: AtomicBool = AtomicBool::new(false);
static DL_RESULT: Mutex<Option<std::result::Result<PathBuf, String>>> = Mutex::new(None);

#[cfg(test)]
thread_local! {
    static INSTALLS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Tests: the installers `update_install` was asked to download on this thread (and forget them).
#[cfg(test)]
pub(crate) fn installs() -> Vec<String> {
    INSTALLS.with(|f| std::mem::take(&mut *f.borrow_mut()))
}

/// Download the installer, check its size and SHA-256 against the release's `SHA256SUMS`, and save it in the temp
/// folder. Blocking: call from a worker thread.
#[cfg_attr(test, allow(dead_code))]
fn download_installer(hwnd: isize, inst: &Installer) -> std::result::Result<PathBuf, String> {
    let sums = weather::http_get(&inst.sums_url, MAX_SUMS).ok_or("checksum file unavailable")?;
    let expected = expected_sha(&sums, &inst.file).ok_or("no checksum for the installer")?;
    let client = HttpClient::new().map_err(|e| e.to_string())?;
    let uri = Uri::CreateUri(&HSTRING::from(inst.url.as_str())).map_err(|e| e.to_string())?;
    let op = client.GetBufferAsync(&uri).map_err(|e| e.to_string())?;
    let size = inst.size;
    let _ = op.SetProgress(&AsyncOperationProgressHandler::new(move |_, p: &HttpProgress| {
        let pct = (p.BytesReceived.saturating_mul(100) / size).min(99) as u8;
        if DL_PCT.swap(pct, Ordering::Relaxed) != pct {
            unsafe {
                let _ = PostMessageW(HWND(hwnd as *mut c_void), WM_UPDATE_DL, WPARAM(0), LPARAM(0));
            }
        }
        Ok(())
    }));
    let buf = op.get().map_err(|e| e.to_string())?;
    let len = buf.Length().map_err(|e| e.to_string())? as u64;
    if len != size {
        return Err(format!("unexpected size ({len} bytes)"));
    }
    let mut bytes = vec![0u8; len as usize];
    DataReader::FromBuffer(&buf).and_then(|r| r.ReadBytes(&mut bytes)).map_err(|e| e.to_string())?;
    if dict::sha256(&bytes).map(|d| dict::hex(&d)).as_deref() != Some(expected.as_str()) {
        return Err("checksum does not match".into());
    }
    let dir = std::env::temp_dir().join("HesewakeNotch-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (path, part) = (dir.join(&inst.file), dir.join(format!("{}.part", inst.file)));
    std::fs::write(&part, &bytes).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// The Download button of the "available" toast.
pub(crate) fn button_rect(w: f32, h: f32) -> (f32, f32, f32, f32) {
    let (bw, bh) = (88.0, 28.0);
    (CX + w / 2.0 - 16.0 - bw, h / 2.0 - bh / 2.0, bw, bh)
}

impl App {
    /// Called once a minute: start an automatic check when one is due. True if a check started.
    pub(crate) fn update_tick(&mut self, now: Instant) -> bool {
        if !self.cfg.update_check || self.update_busy {
            return false;
        }
        if self.update_last.is_some_and(|t| now.saturating_duration_since(t) < INTERVAL) {
            return false;
        }
        self.update_last = Some(now);
        self.update_busy = true;
        fetch(self.hwnd, false);
        true
    }

    /// Start downloading the installer of `r` (the Install button). The toast turns into a progress toast.
    pub(crate) fn update_install(&mut self, r: &Release) {
        let Some(inst) = r.installer.clone() else { return };
        if DL_BUSY.swap(true, Ordering::Relaxed) {
            return;
        }
        DL_PCT.store(0, Ordering::Relaxed);
        // kept so a failed download can offer this release's page
        self.update.get_or_insert_with(|| r.clone());
        self.set_update_note(Note::Downloading(0));
        #[cfg(test)]
        INSTALLS.with(|f| f.borrow_mut().push(inst.file.clone()));
        #[cfg(not(test))]
        {
            let hwnd = self.hwnd;
            std::thread::spawn(move || {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }
                let r = download_installer(hwnd, &inst);
                if let Err(e) = &r {
                    perf::note(&format!("update: download failed: {e}"));
                }
                if let Ok(mut slot) = DL_RESULT.lock() {
                    *slot = Some(r);
                }
                DL_BUSY.store(false, Ordering::Relaxed);
                unsafe {
                    let _ = PostMessageW(HWND(hwnd as *mut c_void), WM_UPDATE_DL, WPARAM(0), LPARAM(0));
                }
            });
        }
    }

    /// `WM_UPDATE_DL`: show the progress, run the installer when it is ready, or fall back to the release page.
    pub(crate) fn on_update_dl(&mut self) {
        let done = DL_RESULT.lock().ok().and_then(|mut s| s.take());
        match done {
            Some(Ok(path)) => {
                sys::open_url(&path.to_string_lossy());
                self.end_toast();
            }
            Some(Err(_)) => match self.update.clone() {
                Some(r) => self.set_update_note(Note::DownloadFailed(r)),
                None => self.end_toast(),
            },
            None => self.set_update_note(Note::Downloading(DL_PCT.load(Ordering::Relaxed))),
        }
        self.last = Instant::now();
    }

    /// Replace the update toast's note in place (no new pop-in) and keep it up while it still has work to show.
    pub(crate) fn set_update_note(&mut self, note: Note) {
        let keep = if matches!(note, Note::Downloading(_)) { 20.0 } else { 12.0 };
        match &mut self.toast {
            Some(t) if matches!(t.kind, ToastKind::Update(_)) => {
                t.kind = ToastKind::Update(note);
                t.dur = t.dur.max(t.t0.elapsed().as_secs_f32() + keep);
            }
            _ => self.push_toast(ToastKind::Update(note)),
        }
    }

    /// "Check for updates" from the menu: always answers with a toast.
    pub(crate) fn update_check_now(&mut self) {
        self.update_last = Some(Instant::now());
        self.update_busy = true;
        fetch(self.hwnd, true);
    }

    pub(crate) fn on_update(&mut self, manual: bool, latest: Option<Release>) {
        self.update_busy = false;
        match latest {
            Some(r) if is_newer(&r.version, CURRENT) => {
                self.update = Some(r.clone());
                if manual || self.cfg.update_seen != r.version {
                    self.cfg.update_seen = r.version.clone();
                    self.save_settings();
                    self.push_toast(ToastKind::Update(Note::Available(r)));
                }
            }
            Some(_) => {
                self.update = None;
                if manual {
                    self.push_toast(ToastKind::Update(Note::UpToDate));
                }
            }
            None => {
                if manual {
                    self.push_toast(ToastKind::Update(Note::Failed));
                }
            }
        }
        self.last = Instant::now();
    }

    pub(crate) fn draw_update_toast(&self, g: &mut Gfx, (w, h): (f32, f32), al: f32, t: f32, note: &Note) {
        let (x0, cy) = (CX - w / 2.0, h / 2.0);
        let (rgb, title, sub, button) = match note {
            Note::Available(r) => (g.acc.bright, fill(tr("Version {} is available"), &[&r.version]), tr("Release notes and installer").to_string(), true),
            Note::UpToDate => ([60, 200, 110], tr("You're up to date").to_string(), fill(tr("Version {}"), &[&CURRENT]), false),
            Note::Failed => (TEXT_DIM, tr("Couldn't check for updates").to_string(), tr("Check your internet connection").to_string(), false),
            Note::Downloading(p) => (g.acc.bright, tr("Downloading update").to_string(), fill(tr("Please wait \u{b7} {}%"), &[p]), false),
            Note::DownloadFailed(_) => ([255, 170, 60], tr("Couldn't download the update").to_string(), tr("Open the release page instead").to_string(), true),
        };
        let label = match note {
            Note::Available(r) if r.installer.is_some() => tr("Install"),
            _ => tr("Download"),
        };
        let sc = 0.6 + 0.4 * toast::pop(t).clamp(0.0, 1.2);
        let (icx, r) = (x0 + 34.0, 18.0 * sc);
        g.circle(icx, cy, r, &paint(rgb[0], rgb[1], rgb[2], 0.22 * al));
        icon_download(g, icx, cy, 15.0 * sc, &paint(rgb[0], rgb[1], rgb[2], al));

        let k = toast::ss(0.12, 0.4, t);
        let tx = x0 + 66.0 + (1.0 - k) * 6.0;
        let right = if button { button_rect(w, h).2 + 16.0 + 10.0 } else { 16.0 };
        let maxw = x0 + w - right - tx;
        let title = if g.width_ex(&title, 14.0, true) > maxw { ellipsize(g, &title, 14.0, maxw) } else { title };
        g.txtb(&title, tx, cy - 19.0, 14.0, [255, 255, 255], al * k, maxw + 2.0);
        let sub = if g.width(&sub, 12.0) > maxw { ellipsize(g, &sub, 12.0, maxw) } else { sub };
        g.txt(&sub, tx, cy + 2.0, 12.0, TEXT_DIM, al * k, maxw);
        if button {
            let (bx, by, bw, bh) = button_rect(w, h);
            let over = self.cursor_canvas().is_some_and(|(x, y)| in_rect(x, y, bx, by, bw, bh));
            g.pill(bx, by, bw, bh, g.acc.deep, if over { 1.0 } else { 0.85 } * al * k, 1.0);
            g.txt_c(label, bx + bw / 2.0, by + ty(bh, 13.0), 13.0, [255, 255, 255], al * k, bw, true);
        }
        if let Note::Downloading(p) = note {
            let (bx, by, bw) = (tx, cy + 20.0, maxw);
            g.fill(&rrect(bx, by, bw, 3.0, 1.5), &paint(255, 255, 255, 0.16 * al * k));
            g.fill(&rrect(bx, by, (bw * *p as f32 / 100.0).max(3.0), 3.0, 1.5), &paint(rgb[0], rgb[1], rgb[2], al * k));
        }
    }
}

/// A download arrow over a tray line, centred on (cx, cy), `s` tall.
fn icon_download(g: &mut Gfx, cx: f32, cy: f32, s: f32, p: &Paint<'static>) {
    let lw = (s * 0.13).max(1.4);
    g.line(cx, cy - 0.5 * s, cx, cy + 0.18 * s, lw, p);
    g.line(cx - 0.3 * s, cy - 0.1 * s, cx, cy + 0.2 * s, lw, p);
    g.line(cx + 0.3 * s, cy - 0.1 * s, cx, cy + 0.2 * s, lw, p);
    g.rect(cx - 0.45 * s, cy + 0.42 * s - lw / 2.0, 0.9 * s, lw, p);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_like_semver() {
        assert_eq!(parse_version("v0.8.0"), Some((0, 8, 0, false)));
        assert_eq!(parse_version("1.2.3-alpha.4"), Some((1, 2, 3, true)));
        assert_eq!(parse_version("V2.0.1+build5"), Some((2, 0, 1, false)));
        for bad in ["", "v", "1.2", "1.2.x", "a.b.c", "1.2.3.4", "-1.2.3"] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
        assert!(is_newer("v0.8.1", "0.8.0"));
        assert!(is_newer("0.9.0", "0.8.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("v0.8.0", "0.8.0"), "same version");
        assert!(!is_newer("0.7.9", "0.8.0"), "older");
        assert!(!is_newer("0.9.0-beta.1", "0.8.0"), "pre-releases are ignored");
        assert!(is_newer("0.8.0", "0.8.0-alpha.1"), "the release after its own pre-release");
        assert!(!is_newer("garbage", "0.8.0"));
        assert!(!is_newer("0.9.0", "garbage"), "can't tell: say nothing");
        assert!(!is_newer("0.10.0", "0.10.0"));
        assert!(is_newer("0.10.0", "0.9.0"), "numbers, not text");
    }

    /// The real endpoint (GitHub Actions only, it needs the network): the latest release is found and readable.
    #[test]
    fn real_latest_release_is_read() {
        if std::env::var_os("CI").is_none() {
            return;
        }
        let body = crate::weather::http_get(LATEST_URL, MAX_BYTES).expect("GitHub answers");
        let r = parse_release(&body).expect("a published release");
        assert!(parse_version(&r.version).is_some(), "{r:?}");
    }

    #[test]
    fn releases_are_read_and_checked() {
        let json = r#"{"tag_name":"v0.9.0","html_url":"https://github.com/dirgaastya/hesewake-notch/releases/tag/v0.9.0","draft":false,"prerelease":false}"#;
        assert_eq!(parse_release(json), Some(Release { version: "0.9.0".into(), url: "https://github.com/dirgaastya/hesewake-notch/releases/tag/v0.9.0".into(), installer: None }));
        let bad = [
            "",
            "{",
            r#"{"message":"Not Found","documentation_url":"https://docs.github.com"}"#,
            r#"{"tag_name":"v0.9.0","html_url":"https://evil.example/releases/tag/v0.9.0"}"#,
            r#"{"tag_name":"v0.9.0","html_url":"https://github.com/dirgaastya/hesewake-notch/releases.evil/x"}"#,
            r#"{"tag_name":"v0.9.0","html_url":"http://github.com/dirgaastya/hesewake-notch/releases/tag/v0.9.0"}"#,
            r#"{"tag_name":"nightly","html_url":"https://github.com/dirgaastya/hesewake-notch/releases/tag/nightly"}"#,
            r#"{"tag_name":"v0.9.0","html_url":"https://github.com/dirgaastya/hesewake-notch/releases/tag/v0.9.0","draft":true}"#,
            r#"{"tag_name":"v0.9.0","html_url":"https://github.com/dirgaastya/hesewake-notch/releases/tag/v0.9.0","prerelease":true}"#,
        ];
        for b in bad {
            assert_eq!(parse_release(b), None, "{b}");
        }
    }

    fn release_json(assets: &str) -> String {
        format!(
            r#"{{"tag_name":"v0.10.2","html_url":"https://github.com/dirgaastya/hesewake-notch/releases/tag/v0.10.2","draft":false,"prerelease":false,"assets":[{assets}]}}"#
        )
    }

    const SETUP: &str = r#"{"name":"HesewakeNotch-Setup-0.10.2.exe","size":3300000,"browser_download_url":"https://github.com/dirgaastya/hesewake-notch/releases/download/v0.10.2/HesewakeNotch-Setup-0.10.2.exe"}"#;
    const SUMS: &str = r#"{"name":"SHA256SUMS","size":100,"browser_download_url":"https://github.com/dirgaastya/hesewake-notch/releases/download/v0.10.2/SHA256SUMS"}"#;

    #[test]
    fn installer_needs_the_setup_file_and_its_checksum_from_this_repository() {
        let r = parse_release(&release_json(&format!("{SETUP},{SUMS}"))).expect("a release");
        let i = r.installer.expect("installer found");
        assert_eq!((i.file.as_str(), i.size), ("HesewakeNotch-Setup-0.10.2.exe", 3_300_000));
        assert!(i.url.ends_with("/HesewakeNotch-Setup-0.10.2.exe") && i.sums_url.ends_with("/SHA256SUMS"));
        // no checksum file (older releases), or no installer: the toast opens the release page instead
        assert_eq!(parse_release(&release_json(SETUP)).unwrap().installer, None);
        assert_eq!(parse_release(&release_json(SUMS)).unwrap().installer, None);
        assert_eq!(parse_release(&release_json("")).unwrap().installer, None);
        // another repository, another host, or a wrong name
        let evil = SETUP.replace("dirgaastya/hesewake-notch", "evil/hesewake-notch");
        assert_eq!(parse_release(&release_json(&format!("{evil},{SUMS}"))).unwrap().installer, None);
        let evil = SUMS.replace("https://github.com", "https://evil.example");
        assert_eq!(parse_release(&release_json(&format!("{SETUP},{evil}"))).unwrap().installer, None);
        let other = SETUP.replace("Setup-0.10.2", "Setup-0.10.1");
        assert_eq!(parse_release(&release_json(&format!("{other},{SUMS}"))).unwrap().installer, None);
        // sizes that cannot be the installer
        for size in ["100", "999999999999"] {
            let odd = SETUP.replace("3300000", size);
            assert_eq!(parse_release(&release_json(&format!("{odd},{SUMS}"))).unwrap().installer, None, "{size}");
        }
    }

    #[test]
    fn checksum_is_read_from_sha256sums() {
        let h = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let f = "HesewakeNotch-Setup-0.10.2.exe";
        assert_eq!(expected_sha(&format!("{h}  {f}\n"), f).as_deref(), Some(h));
        assert_eq!(expected_sha(&format!("{h} *{f}"), f).as_deref(), Some(h), "binary-mode marker");
        assert_eq!(expected_sha(&format!("{}  {f}", h.to_uppercase()), f).as_deref(), Some(h), "case does not matter");
        assert_eq!(expected_sha(&format!("0000  other.exe\n{h}  {f}\r\n"), f).as_deref(), Some(h), "the right line");
        for bad in ["", &format!("{h}  other.exe"), &format!("{}  {f}", &h[..63]), &format!("{}z  {f}", &h[..63]), f] {
            assert_eq!(expected_sha(bad, f), None, "{bad:?}");
        }
    }
}

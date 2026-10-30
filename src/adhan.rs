//! Optional adhan sound when a prayer time starts (issue #23). Off by default. The app ships no recording: the user
//! picks an .mp3 or .wav file, and without a file a short Windows sound plays instead. Files play through MCI (no
//! extra dependencies), so a long adhan keeps playing in the background until it ends or the user presses Stop.

use super::*;
use std::sync::Mutex;

/// Message to the window: the file dialog closed (`take_picked`).
pub(crate) const WM_ADHAN_FILE: u32 = WM_APP + 17;

/// Settings value of the Adhan sound row.
pub(crate) const OFF: u8 = 0;
pub(crate) const SYSTEM: u8 = 1;
pub(crate) const FILE: u8 = 2;
pub(crate) const MODES: u8 = 3;

/// "Off" / "System sound" / "File".
pub(crate) fn mode_label(m: u8) -> &'static str {
    match m {
        SYSTEM => tr("System sound"),
        FILE => tr("File"),
        _ => tr("Off"),
    }
}

/// Next or previous mode for the stepper.
pub(crate) fn step(m: u8, up: bool) -> u8 {
    if up { (m + 1) % MODES } else { (m + MODES - 1) % MODES }
}

/// File name without the folder, for the Settings row.
pub(crate) fn file_label(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// How long the system sound is treated as playing.
const SYSTEM_SECS: f32 = 2.0;

/// Windows playback (MCI for files, PlaySound for the system sound).
#[cfg(not(test))]
mod backend {
    use super::*;
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
    use windows::Win32::Media::Multimedia::mciSendStringW;

    /// Used when MCI cannot tell the length of a file.
    const UNKNOWN_SECS: u64 = 300;
    const ALIAS: &str = "hesewake_adhan";

    fn mci(cmd: &str) -> Option<String> {
        let mut out = [0u16; 128];
        let err = unsafe { mciSendStringW(&HSTRING::from(cmd), Some(&mut out), None) };
        (err == 0).then(|| String::from_utf16_lossy(&out[..out.iter().position(|&c| c == 0).unwrap_or(0)]))
    }

    pub(super) fn play_file(file: &str) -> Option<Duration> {
        let _ = mci(&format!("close {ALIAS}"));
        mci(&format!("open \"{file}\" type mpegvideo alias {ALIAS}"))?;
        let _ = mci(&format!("set {ALIAS} time format milliseconds"));
        let len = mci(&format!("status {ALIAS} length")).and_then(|s| s.trim().parse::<u64>().ok());
        mci(&format!("play {ALIAS}"))?;
        Some(Duration::from_millis(len.unwrap_or(UNKNOWN_SECS * 1000)))
    }

    pub(super) fn play_system() {
        unsafe {
            let _ = PlaySoundW(w!("SystemAsterisk"), None, SND_ALIAS | SND_ASYNC | SND_NODEFAULT);
        }
    }

    pub(super) fn stop() {
        let _ = mci(&format!("close {ALIAS}"));
    }
}

/// Tests: records what would have played.
#[cfg(test)]
mod backend {
    use super::*;

    thread_local! {
        pub(super) static PLAYED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    pub(super) fn play_file(file: &str) -> Option<Duration> {
        PLAYED.with(|p| p.borrow_mut().push(format!("file:{file}")));
        Some(Duration::from_secs(180))
    }

    pub(super) fn play_system() {
        PLAYED.with(|p| p.borrow_mut().push("system".into()));
    }

    pub(super) fn stop() {
        PLAYED.with(|p| p.borrow_mut().push("stop".into()));
    }
}

/// Tests: what was played or stopped on this thread ("system", "file:<path>", "stop"), and forget it.
#[cfg(test)]
pub(crate) fn played() -> Vec<String> {
    backend::PLAYED.with(|p| std::mem::take(&mut *p.borrow_mut()))
}

/// Play the sound for `mode`. Returns how long it plays, or None when nothing plays.
pub(crate) fn play(mode: u8, file: &str) -> Option<Duration> {
    if mode == OFF {
        return None;
    }
    if mode == FILE && !file.is_empty() && std::path::Path::new(file).is_file() {
        return backend::play_file(file);
    }
    // the system sound, or "File" without a usable file
    backend::play_system();
    Some(Duration::from_secs_f32(SYSTEM_SECS))
}

/// Stop a playing file.
pub(crate) fn stop() {
    backend::stop();
}

/// The file the user picked (Some(None) = the dialog was cancelled).
static PICKED: filepick::Picked = Mutex::new(None);

/// Show the Open dialog for an .mp3 / .wav file; `WM_ADHAN_FILE` reports back.
pub(crate) fn pick_file(hwnd: isize) {
    filepick::open(hwnd, tr("Adhan file"), "Audio (*.mp3, *.wav)", "*.mp3;*.wav", &PICKED, WM_ADHAN_FILE);
}

/// The answer of the file dialog: Some(Some(path)), Some(None) when cancelled, None when nothing is waiting.
pub(crate) fn take_picked() -> Option<Option<String>> {
    PICKED.lock().ok()?.take()
}

impl App {
    /// A prayer time started: play the adhan if the user turned it on. Silent while a fullscreen app is in front.
    pub(crate) fn adhan_start(&mut self) {
        if self.cfg.adhan == OFF || self.busy {
            return;
        }
        if let Some(len) = play(self.cfg.adhan, &self.cfg.adhan_file) {
            self.adhan_until = Some(Instant::now() + len);
        }
    }

    pub(crate) fn adhan_playing(&self) -> bool {
        self.adhan_until.is_some_and(|t| Instant::now() < t)
    }

    pub(crate) fn adhan_stop(&mut self) {
        if self.adhan_playing() {
            stop();
        }
        self.adhan_until = None;
    }

    /// The file dialog answered: keep the file, or go back to the previous mode when cancelled without a file.
    pub(crate) fn adhan_picked(&mut self, picked: Option<String>) {
        match picked {
            Some(path) => {
                self.cfg.adhan_file = path;
                self.cfg.adhan = FILE;
            }
            None if self.cfg.adhan_file.is_empty() => self.cfg.adhan = SYSTEM,
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_step_around() {
        assert_eq!(step(OFF, true), SYSTEM);
        assert_eq!(step(FILE, true), OFF);
        assert_eq!(step(OFF, false), FILE);
        assert_eq!(file_label("C:\\Users\\me\\Music\\adzan makkah.mp3"), "adzan makkah.mp3");
    }

    #[test]
    fn off_plays_nothing_and_a_missing_file_falls_back_to_the_system_sound() {
        let _ = played();
        assert_eq!(play(OFF, "C:\\x.mp3"), None);
        assert!(played().is_empty());
        assert!(play(FILE, "C:\\does\\not\\exist.mp3").is_some());
        assert_eq!(played(), vec!["system".to_string()], "no file: the short system sound");
        assert!(play(SYSTEM, "").is_some());
        assert_eq!(played(), vec!["system".to_string()]);
    }

    #[test]
    fn a_picked_file_plays() {
        let f = std::env::temp_dir().join(format!("hesewake-adhan-{}.mp3", std::process::id()));
        std::fs::write(&f, b"x").unwrap();
        let path = f.to_string_lossy().into_owned();
        let _ = played();
        assert!(play(FILE, &path).is_some());
        assert_eq!(played(), vec![format!("file:{path}")]);
        let _ = std::fs::remove_file(&f);
    }
}

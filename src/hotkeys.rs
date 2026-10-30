//! Global hotkeys Ctrl+Alt+1..9 and 0: open the notch on the Nth enabled slide (the order in Settings > Slides).
//! Pressing the same hotkey again while that slide is showing closes the notch.

use super::*;
use crate::slides::SlideId;
use windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey;

/// Number of slide hotkeys (Ctrl+Alt+1 .. Ctrl+Alt+9, then Ctrl+Alt+0 for the 10th).
pub(crate) const SLIDE_KEYS: usize = 10;
/// Hotkey ids 10..=19; id 1 is Ctrl+Alt+H (eye.rs).
const FIRST_ID: i32 = 10;
/// Hotkey id of Ctrl+Alt+M, the microphone mute (mic.rs).
pub(crate) const MIC_ID: i32 = 2;
/// How long the notch stays open after a slide hotkey (longer while hovered).
const PEEK_SECS: u64 = 8;

/// The number key for slide position `i`: 1..9, then 0 (like the keyboard's number row).
pub(crate) fn key_of(i: usize) -> char {
    if i < 9 {
        char::from(b'1' + i as u8)
    } else {
        '0'
    }
}

/// Slide position for a `WM_HOTKEY` id, if it is one of the slide hotkeys.
pub(crate) fn slide_of(id: i32) -> Option<usize> {
    (FIRST_ID..FIRST_ID + SLIDE_KEYS as i32).contains(&id).then(|| (id - FIRST_ID) as usize)
}

/// Register (or release) Ctrl+Alt+1..9 and 0. A key another app already holds is skipped and noted in perf.log.
pub(crate) fn set_slide_hotkeys(hwnd: isize, on: bool) {
    if cfg!(test) {
        return; // unit tests must not grab global hotkeys (the CI-only test below calls `register` itself)
    }
    for n in register(HWND(hwnd as _), on) {
        perf::note(&format!("hotkey: Ctrl+Alt+{n} is already taken by another app; that slide hotkey is off"));
    }
}

/// Register (or release) Ctrl+Alt+M, the microphone mute. A key another app already holds is noted in perf.log.
pub(crate) fn set_mic_hotkey(hwnd: isize, on: bool) {
    if cfg!(test) {
        return; // unit tests must not grab global hotkeys (the CI-only test below calls `register_mic` itself)
    }
    if !register_mic(HWND(hwnd as _), on) {
        perf::note("hotkey: Ctrl+Alt+M is already taken by another app; the mic mute hotkey is off");
    }
}

fn ctrl_alt() -> HOT_KEY_MODIFIERS {
    HOT_KEY_MODIFIERS(MOD_ALT.0 | MOD_CONTROL.0 | MOD_NOREPEAT.0)
}

/// Release Ctrl+Alt+M for `hwnd`, then register it again if `on`. False if registering failed.
fn register_mic(hwnd: HWND, on: bool) -> bool {
    let _ = unsafe { UnregisterHotKey(hwnd, MIC_ID) };
    !on || unsafe { RegisterHotKey(hwnd, MIC_ID, ctrl_alt(), 'M' as u32) }.is_ok()
}

/// Release Ctrl+Alt+1..9 and 0 for `hwnd`, then register them again if `on`. Returns the key numbers that failed.
fn register(hwnd: HWND, on: bool) -> Vec<usize> {
    let mods = ctrl_alt();
    let mut failed = Vec::new();
    for i in 0..SLIDE_KEYS {
        let id = FIRST_ID + i as i32;
        let _ = unsafe { UnregisterHotKey(hwnd, id) };
        if on && unsafe { RegisterHotKey(hwnd, id, mods, key_of(i) as u32) }.is_err() {
            failed.push(i + 1);
        }
    }
    failed
}

#[derive(Debug, PartialEq)]
enum Action {
    Show,
    Close,
    Ignore,
}

/// What hotkey `n` should do, given the carousel order, whether the notch is open, and the current slide.
fn action(order: &[SlideId], n: usize, expanded: bool, cur: usize) -> Action {
    match order.get(n) {
        None | Some(SlideId::Settings) => Action::Ignore,
        Some(_) if expanded && cur == n => Action::Close,
        Some(_) => Action::Show,
    }
}

/// Hotkey number (1..=10; 10 is the 0 key) of entry `i` in the slide config, shown in Settings > Slides; None if it has none.
pub(crate) fn number(cfg: &[(SlideId, bool)], i: usize) -> Option<usize> {
    if !cfg.get(i)?.1 {
        return None;
    }
    let pos = cfg[..i].iter().filter(|(_, on)| *on).count();
    (pos < SLIDE_KEYS).then_some(pos + 1)
}

impl App {
    /// Handle slide hotkey `n` (0-based). True if the notch changed.
    pub(crate) fn slide_hotkey(&mut self, n: usize) -> bool {
        if self.lock == Lock::Min {
            return false;
        }
        match action(&self.order, n, self.expanded(), self.cur()) {
            Action::Ignore => false,
            Action::Close => {
                self.peek_until = None;
                self.reminding = false;
                true
            }
            Action::Show => {
                self.show(n);
                self.peek_until = Some(Instant::now() + Duration::from_secs(PEEK_SECS));
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
        VK_MENU,
    };
    use SlideId::*;

    /// Press and release Ctrl+Alt+`digit` through the OS input queue, like a real keyboard.
    fn press_ctrl_alt(digit: u8) {
        press_ctrl_alt_key(b'0' + digit);
    }

    /// Press and release Ctrl+Alt+`vk` (a digit or capital letter) through the OS input queue.
    fn press_ctrl_alt_key(vk: u8) {
        let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() },
            },
        };
        let d = VIRTUAL_KEY(vk as u16);
        let seq = [key(VK_CONTROL, false), key(VK_MENU, false), key(d, false), key(d, true), key(VK_MENU, true), key(VK_CONTROL, true)];
        let sent = unsafe { SendInput(&seq, std::mem::size_of::<INPUT>() as i32) };
        assert_eq!(sent as usize, seq.len(), "SendInput was blocked (no interactive desktop?)");
    }

    /// Wait up to `ms` for a WM_HOTKEY on this thread's queue and return its id.
    fn next_hotkey(ms: u64) -> Option<i32> {
        let end = Instant::now() + Duration::from_millis(ms);
        let mut msg = MSG::default();
        while Instant::now() < end {
            if unsafe { PeekMessageW(&mut msg, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE) }.as_bool() {
                return Some(msg.wParam.0 as i32);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    /// End to end on a real Windows desktop: the keys really register, a real Ctrl+Alt+3 keystroke arrives as
    /// the hotkey for slide 3, and switching the setting off really releases the keys. Runs only on CI
    /// (`CI` is set by GitHub Actions) so a local `cargo test` never types into the developer's desktop.
    #[test]
    fn real_keystroke_reaches_the_slide_hotkey() {
        if std::env::var_os("CI").is_none() {
            eprintln!("skipped: sends real keystrokes, runs on CI only");
            return;
        }
        let mut msg = MSG::default();
        unsafe {
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE); // make sure this thread has a message queue
        }
        // null HWND: WM_HOTKEY is posted to this test thread's queue
        let none = HWND::default();
        assert_eq!(register(none, true), Vec::<usize>::new(), "every Ctrl+Alt+1..8 registers");

        press_ctrl_alt(3);
        let id = next_hotkey(2000).expect("Ctrl+Alt+3 produced a WM_HOTKEY");
        eprintln!("Ctrl+Alt+3 -> WM_HOTKEY id {id}");
        assert_eq!(slide_of(id), Some(2), "Ctrl+Alt+3 maps to the third slide");
        assert_eq!(action(&[Music, Todo, Timer], 2, false, 0), Action::Show);

        press_ctrl_alt(9);
        assert_eq!(next_hotkey(2000).and_then(slide_of), Some(8), "Ctrl+Alt+9 maps to the ninth slide");
        press_ctrl_alt(0);
        assert_eq!(next_hotkey(2000).and_then(slide_of), Some(9), "Ctrl+Alt+0 maps to the tenth slide");

        assert_eq!(register(none, false), Vec::<usize>::new());
        press_ctrl_alt(3);
        assert_eq!(next_hotkey(500), None, "with the setting off the keys are released");
        eprintln!("after release: no WM_HOTKEY");

        assert!(register_mic(none, true), "Ctrl+Alt+M registers");
        press_ctrl_alt_key(b'M');
        assert_eq!(next_hotkey(2000), Some(MIC_ID), "Ctrl+Alt+M is the mic mute hotkey");
        assert!(register_mic(none, false));
        press_ctrl_alt_key(b'M');
        assert_eq!(next_hotkey(500), None, "with the setting off Ctrl+Alt+M is released");
        eprintln!("Ctrl+Alt+M -> mic mute, released when off");
    }

    #[test]
    fn ids_map_to_slide_positions() {
        assert_eq!(slide_of(1), None, "Ctrl+Alt+H is not a slide hotkey");
        assert_eq!(slide_of(FIRST_ID), Some(0));
        assert_eq!(slide_of(FIRST_ID + 8), Some(8), "Ctrl+Alt+9");
        assert_eq!(slide_of(FIRST_ID + 9), Some(9), "Ctrl+Alt+0");
        assert_eq!(slide_of(FIRST_ID + 10), None);
        assert_eq!((key_of(0), key_of(8), key_of(9)), ('1', '9', '0'), "the tenth slide is on 0, like the number row");
    }

    #[test]
    fn hotkey_shows_then_closes_its_slide() {
        let order = [Music, Todo, Timer];
        assert_eq!(action(&order, 1, false, 0), Action::Show, "closed notch: open on that slide");
        assert_eq!(action(&order, 1, true, 0), Action::Show, "open on another slide: switch");
        assert_eq!(action(&order, 1, true, 1), Action::Close, "same hotkey again: close");
        assert_eq!(action(&order, 1, false, 1), Action::Show, "closed but last on that slide: open");
        assert_eq!(action(&order, 3, false, 0), Action::Ignore, "no 4th slide");
        assert_eq!(action(&[Music, Todo, Settings], 2, false, 0), Action::Ignore, "Settings has no hotkey");
    }

    #[test]
    fn numbers_count_only_enabled_slides() {
        let cfg = vec![(Music, true), (Water, false), (Todo, true), (Ledge, true)];
        assert_eq!(number(&cfg, 0), Some(1));
        assert_eq!(number(&cfg, 1), None, "disabled slide");
        assert_eq!(number(&cfg, 2), Some(2));
        assert_eq!(number(&cfg, 3), Some(3));
        assert_eq!(number(&cfg, 9), None);
        let many: Vec<_> = SlideId::USER.iter().map(|&s| (s, true)).chain([(Music, true)]).collect();
        assert_eq!(number(&many, 9), Some(10));
        assert_eq!(number(&many, 10), None, "only 10 hotkeys");
    }
}

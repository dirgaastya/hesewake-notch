//! Every setting the user can change (`Rid`), its current value, and what changing it does. The Settings window
//! (settingswin.rs) only draws and dispatches; a new setting gets a `Rid` here and a row on one of the window's pages.

use super::*;
use crate::settings::{LEAD_MAX, LEAD_MIN, LEAD_STEP};

/// Seconds a "press again" button (remove the dictionary) waits for the second press.
pub(crate) const CONFIRM_SECS: f32 = 3.0;

/// One setting.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Rid {
    Minimal,
    ThinMaximized,
    Monitor,
    LyricTiming,
    NotchSize,
    IdleDisplay,
    Notifications,
    ContactPhotos,
    Autostart,
    SlideHotkeys,
    Language,
    /// Button: paste the calendar link from the clipboard, or remove it (press twice).
    Calendar,
    HudVolume,
    HudBrightness,
    LockKeys,
    PrivacyDot,
    UpdateCheck,
    MicMute,
    Weather,
    City,
    Prayer,
    Accent,
    Romaji,
    /// Button: get the Japanese dictionary, or delete the downloaded one (press twice).
    Dictionary,
    Downloads,
    ImsakTerbit,
    Ramadan,
    /// Choice: adhan::OFF / SYSTEM / FILE.
    Adhan,
    /// Button: pick (or change) the adhan file.
    AdhanFile,
}

impl Rid {
    /// Every setting (a test checks each one has a row in the Settings window).
    #[cfg(test)]
    pub(crate) const ALL: [Rid; 29] = [
        Rid::Minimal,
        Rid::ThinMaximized,
        Rid::Monitor,
        Rid::LyricTiming,
        Rid::NotchSize,
        Rid::IdleDisplay,
        Rid::Notifications,
        Rid::ContactPhotos,
        Rid::Autostart,
        Rid::SlideHotkeys,
        Rid::Language,
        Rid::Calendar,
        Rid::HudVolume,
        Rid::HudBrightness,
        Rid::LockKeys,
        Rid::PrivacyDot,
        Rid::UpdateCheck,
        Rid::MicMute,
        Rid::Weather,
        Rid::City,
        Rid::Prayer,
        Rid::Accent,
        Rid::Romaji,
        Rid::Dictionary,
        Rid::Downloads,
        Rid::ImsakTerbit,
        Rid::Ramadan,
        Rid::Adhan,
        Rid::AdhanFile,
    ];
}

impl App {
    /// On/off state of a toggle setting.
    pub(crate) fn opt_on(&self, r: Rid) -> bool {
        match r {
            Rid::Minimal => self.minimal,
            Rid::ThinMaximized => self.cfg.busy_on_maximized,
            Rid::Notifications => self.notif_on,
            Rid::ContactPhotos => self.cfg.notif_avatar,
            Rid::Autostart => self.sw.autostart,
            Rid::SlideHotkeys => self.cfg.slide_hotkeys,
            Rid::HudVolume => self.cfg.hud_volume,
            Rid::HudBrightness => self.cfg.hud_brightness,
            Rid::LockKeys => self.cfg.lock_toast,
            Rid::PrivacyDot => self.cfg.privacy_dot,
            Rid::UpdateCheck => self.cfg.update_check,
            Rid::MicMute => self.cfg.mic_hotkey,
            Rid::Weather => self.cfg.weather_on,
            Rid::Prayer => self.cfg.prayer_on,
            Rid::Romaji => self.cfg.romaji_lyrics,
            Rid::Downloads => self.cfg.downloads,
            Rid::ImsakTerbit => self.cfg.imsak_terbit,
            Rid::Ramadan => self.cfg.ramadan,
            _ => false,
        }
    }

    /// Current choice of a choice setting (index into its list of choices).
    pub(crate) fn opt_choice(&self, r: Rid) -> usize {
        match r {
            Rid::Monitor => self.cfg.monitor.min(monitors::mode_count() - 1),
            Rid::NotchSize => self.cfg.notch_size,
            Rid::IdleDisplay => self.cfg.idle_display,
            Rid::Language => self.cfg.lang,
            Rid::City => self.cfg.weather_city,
            Rid::Accent => self.cfg.accent,
            Rid::Adhan => self.cfg.adhan as usize,
            _ => 0,
        }
    }

    /// The dictionary's Delete button was pressed once and waits for the second press.
    pub(crate) fn dict_confirming(&self) -> bool {
        self.sw.dict_confirm.is_some_and(|t| t.elapsed().as_secs_f32() < CONFIRM_SECS)
    }

    /// Flip a toggle, or press a button setting. Saves.
    pub(crate) fn opt_press(&mut self, r: Rid) {
        match r {
            Rid::Minimal => self.minimal = !self.minimal,
            Rid::ThinMaximized => self.cfg.busy_on_maximized = !self.cfg.busy_on_maximized,
            Rid::Notifications => {
                self.notif_on = !self.notif_on;
                notif::set_enabled(self.notif_on);
                if self.notif_on {
                    if !self.notif_started {
                        self.notif_started = true;
                        notif::start(self.hwnd, self.notif_slot.clone());
                    }
                } else {
                    self.notifs.clear();
                }
            }
            Rid::ContactPhotos => {
                self.cfg.notif_avatar = !self.cfg.notif_avatar;
                notif::set_avatar(self.cfg.notif_avatar);
            }
            Rid::Autostart => {
                sys::set_autostart(!self.sw.autostart);
                self.sw.autostart = sys::autostart_enabled();
            }
            Rid::SlideHotkeys => {
                self.cfg.slide_hotkeys = !self.cfg.slide_hotkeys;
                hotkeys::set_slide_hotkeys(self.hwnd, self.cfg.slide_hotkeys);
            }
            Rid::Calendar => {
                let clip = calendar::clipboard_text(self.hwnd);
                self.cal_button(clip);
            }
            Rid::HudVolume => self.cfg.hud_volume = !self.cfg.hud_volume,
            Rid::HudBrightness => self.cfg.hud_brightness = !self.cfg.hud_brightness,
            Rid::LockKeys => {
                self.cfg.lock_toast = !self.cfg.lock_toast;
                keys::set_enabled(self.hwnd, self.cfg.lock_toast);
            }
            Rid::PrivacyDot => {
                self.cfg.privacy_dot = !self.cfg.privacy_dot;
                privacy::set_enabled(self.cfg.privacy_dot);
                self.priv_t = Instant::now();
            }
            Rid::UpdateCheck => self.cfg.update_check = !self.cfg.update_check,
            Rid::MicMute => {
                self.cfg.mic_hotkey = !self.cfg.mic_hotkey;
                hotkeys::set_mic_hotkey(self.hwnd, self.cfg.mic_hotkey);
                self.priv_t = Instant::now();
            }
            Rid::Weather => {
                self.cfg.weather_on = !self.cfg.weather_on;
                self.weather_apply();
            }
            Rid::Prayer => {
                self.cfg.prayer_on = !self.cfg.prayer_on;
                self.prayer_refresh();
                self.apply_slide_cfg(); // the Prayer slide comes and goes with it
                if !self.cfg.prayer_on {
                    self.adhan_stop();
                }
            }
            Rid::ImsakTerbit => self.cfg.imsak_terbit = !self.cfg.imsak_terbit,
            Rid::Ramadan => self.cfg.ramadan = !self.cfg.ramadan,
            Rid::Downloads => {
                self.cfg.downloads = !self.cfg.downloads;
                downloads::set_enabled(self.hwnd, self.cfg.downloads);
                if !self.cfg.downloads {
                    self.dl_apply(Vec::new(), Vec::new());
                }
            }
            Rid::AdhanFile => adhan::pick_file(self.hwnd),
            Rid::Romaji => {
                self.cfg.romaji_lyrics = !self.cfg.romaji_lyrics;
                romaji::set_enabled(self.cfg.romaji_lyrics);
                self.romaji_refresh();
            }
            Rid::Dictionary => {
                if dict::removable().is_none() {
                    dict::start(self.hwnd);
                } else if !self.dict_confirming() {
                    self.sw.dict_confirm = Some(Instant::now());
                } else {
                    self.sw.dict_confirm = None;
                    dict::remove();
                }
            }
            Rid::Monitor | Rid::LyricTiming | Rid::NotchSize | Rid::IdleDisplay | Rid::Language | Rid::City | Rid::Accent | Rid::Adhan => {}
        }
        self.save_settings();
    }

    /// Move a choice setting one step back or forward (the lyric timing by `LEAD_STEP`). Saves.
    pub(crate) fn opt_step(&mut self, r: Rid, plus: bool) {
        let i = self.opt_choice(r);
        match r {
            Rid::LyricTiming => {
                let d = if plus { LEAD_STEP } else { -LEAD_STEP };
                self.cfg.lyric_lead_ms = (self.cfg.lyric_lead_ms + d).clamp(LEAD_MIN, LEAD_MAX);
                self.save_settings();
            }
            Rid::Monitor => self.opt_pick(r, monitors::step_mode(i, plus)),
            Rid::NotchSize => self.opt_pick(r, if plus { (i + 1).min(scale::SIZES.len() - 1) } else { i.saturating_sub(1) }),
            Rid::IdleDisplay => {
                self.opt_pick(r, if plus { (i + 1).min(settings::IDLE_DISPLAY_LABELS.len() - 1) } else { i.saturating_sub(1) })
            }
            Rid::Language => self.opt_pick(r, i18n::step(i, plus)),
            Rid::City => {
                let n = weather::CITIES.len();
                self.opt_pick(r, if plus { (i + 1) % n } else { (i + n - 1) % n })
            }
            Rid::Accent => self.opt_pick(r, accent::step(i, plus)),
            Rid::Adhan => self.opt_pick(r, adhan::step(i as u8, plus) as usize),
            _ => {}
        }
    }

    /// Set a choice setting to choice `i` directly. Saves.
    pub(crate) fn opt_pick(&mut self, r: Rid, i: usize) {
        match r {
            Rid::Monitor => {
                self.cfg.monitor = i.min(monitors::mode_count() - 1);
                self.place(true);
            }
            Rid::NotchSize => {
                self.cfg.notch_size = i.min(scale::SIZES.len() - 1);
                self.place(true);
            }
            Rid::IdleDisplay => {
                self.cfg.idle_display = i.min(settings::IDLE_DISPLAY_LABELS.len() - 1);
                self.cur_phase = self.idle_phase();
                self.eye = EyeAnim::new();
            }
            Rid::Language => {
                self.cfg.lang = i.min(i18n::LANGS.len() - 1);
                i18n::set(self.cfg.lang);
            }
            Rid::City => {
                self.cfg.weather_city = i.min(weather::CITIES.len() - 1);
                self.weather_apply();
                self.prayer_refresh();
            }
            Rid::Accent => self.cfg.accent = i.min(accent::ACCENTS.len() - 1),
            Rid::Adhan => {
                self.cfg.adhan = (i as u8).min(adhan::MODES - 1);
                // "File" without a file yet: ask for one (the answer comes back as adhan::WM_ADHAN_FILE)
                if self.cfg.adhan == adhan::FILE && self.cfg.adhan_file.is_empty() {
                    adhan::pick_file(self.hwnd);
                }
                if self.cfg.adhan == adhan::OFF {
                    self.adhan_stop();
                }
            }
            _ => return,
        }
        self.save_settings();
    }

    pub(crate) fn save_settings(&mut self) {
        self.cfg.minimal = self.minimal;
        self.cfg.notifications = self.notif_on;
        self.cfg.save();
    }

    /// The slide config with the Prayer slide off while prayer times are off (see `slides::effective`).
    pub(crate) fn slide_cfg(&self) -> slides::SlideCfg {
        slides::effective(&self.cfg.slides, self.cfg.prayer_on)
    }

    /// Rebuild the carousel after the slide list changed, staying on the slide being shown (or the nearest one).
    pub(crate) fn apply_slide_cfg(&mut self) {
        let cur = self.cur_id();
        self.order = slides::visible_order(&self.slide_cfg());
        let idx = self.order.iter().position(|&s| s == cur).unwrap_or(0).min(self.order.len() - 1);
        self.slide = idx as i32;
        self.sp = idx as f32;
        self.sv = 0.0;
    }

    /// Show or hide slide `i` of the slide list; at least one slide stays on. Saves.
    pub(crate) fn slide_toggle(&mut self, i: usize) {
        let enabled = self.cfg.slides.iter().filter(|(_, on)| *on).count();
        if i < self.cfg.slides.len() && !(self.cfg.slides[i].1 && enabled <= 1) {
            self.cfg.slides[i].1 = !self.cfg.slides[i].1;
            self.apply_slide_cfg();
            self.save_settings();
        }
    }

    /// Swap slides `i` and `j` in the carousel order. Saves.
    pub(crate) fn slide_swap(&mut self, i: usize, j: usize) {
        if i < self.cfg.slides.len() && j < self.cfg.slides.len() && i != j {
            self.cfg.slides.swap(i, j);
            self.apply_slide_cfg();
            self.save_settings();
        }
    }

    /// Hand the weather settings (on/off, city) to the thread that fetches the weather.
    pub(crate) fn weather_apply(&self) {
        let (_, lat, lon) = weather::CITIES[self.cfg.weather_city];
        weather::configure(self.cfg.weather_on, lat, lon);
    }
}

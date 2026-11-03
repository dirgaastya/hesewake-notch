//! Settings window: an ordinary floating window with a sidebar of categories, opened from the tray / right-click menu
//! and the gear in the icon dock. Drawn with the same Gfx (tiny-skia) code as the notch, then copied into the window
//! on WM_PAINT. Every change goes through opts.rs, so it applies exactly like it did on the Settings slide.
//!
//! The window runs on the notch's thread and reaches the app through `with_app`. Messages that Windows sends while
//! the app is locked (window creation, destroying it from a click) must not need the app, so those are handled
//! without it and the Win32 calls that trigger them are made after `with_app` returns.

use crate::extproto::{DetailMsg, Hero, SlideMsg, Tone};
use crate::textfield::TextField;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;
use crate::icons::OutlineIcon;
use crate::opts::Rid;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWM_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_ESCAPE};

/// Not in the enabled `windows` features (it lives with the common controls).
const WM_MOUSELEAVE: u32 = 0x02A3;

// ---------------------------------------------------------------- layout (logical px)
const WIN_W: f32 = 820.0;
const WIN_H: f32 = 560.0;
/// One row of an extension's page in the panel.
const PANEL_ROW_H: f32 = 46.0;
const SIDE_W: f32 = 214.0;
const BAR_H: f32 = 46.0;
const NAV_Y: f32 = 62.0;
const NAV_STEP: f32 = 36.0;
const NAV_H: f32 = 34.0;
/// Content column.
const X0: f32 = SIDE_W + 36.0;
const X1: f32 = WIN_W - 36.0;
const ROW_H: f32 = 58.0;
const SLIDE_ROW_H: f32 = 50.0;

const PANEL: [u8; 3] = [12, 12, 15];
const CARD: [u8; 3] = [20, 20, 24];
const WELL: [u8; 3] = [8, 8, 10];

/// Window scale, readable without the app (WM_NCHITTEST arrives while the app may be locked).
static SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0

fn scale() -> f32 {
    f32::from_bits(SCALE.load(Ordering::Relaxed))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Sec {
    General,
    Look,
    Slides,
    Screens,
    Music,
    Notifs,
    Alerts,
    Weather,
    Calendar,
    Extensions,
    About,
}

pub(crate) const SECS: [Sec; 11] = [
    Sec::General,
    Sec::Look,
    Sec::Slides,
    Sec::Screens,
    Sec::Music,
    Sec::Notifs,
    Sec::Alerts,
    Sec::Weather,
    Sec::Calendar,
    Sec::Extensions,
    Sec::About,
];

impl Sec {
    fn name(self) -> &'static str {
        match self {
            Sec::General => tr("General"),
            Sec::Look => tr("Appearance"),
            Sec::Slides => tr("Slides"),
            Sec::Screens => tr("Screens"),
            Sec::Music => tr("Music & lyrics"),
            Sec::Notifs => tr("Notifications"),
            Sec::Alerts => tr("HUD & alerts"),
            Sec::Weather => tr("Weather & prayer"),
            Sec::Calendar => tr("Calendar"),
            Sec::Extensions => tr("Extensions"),
            Sec::About => tr("About"),
        }
    }

    /// Two-tone title like the slides ("Todo|List"): the part after `|` is drawn in the accent color.
    fn title(self) -> &'static str {
        match self {
            Sec::General => tr("Gene|ral"),
            Sec::Look => tr("Look & |Feel"),
            Sec::Slides => tr("Slide |Order"),
            Sec::Screens => tr("Scr|eens"),
            Sec::Music => tr("Music & |Lyrics"),
            Sec::Notifs => tr("Notifi|cations"),
            Sec::Alerts => tr("HUD & |Alerts"),
            Sec::Weather => tr("Weather & |Prayer"),
            Sec::Calendar => tr("Calen|dar"),
            Sec::Extensions => tr("Ext|ensions"),
            Sec::About => tr("Ab|out"),
        }
    }

    fn desc(self) -> &'static str {
        match self {
            Sec::General => tr("How Hesewake starts and which language it speaks."),
            Sec::Look => tr("Accent color, size and how the notch looks while idle."),
            Sec::Slides => tr("Show, hide and reorder the slides in the carousel."),
            Sec::Screens => tr("Where the notch lives and when it steps aside."),
            Sec::Music => tr("Synced lyrics from your media player."),
            Sec::Notifs => tr("Windows notifications, shown in the notch."),
            Sec::Alerts => tr("Small popups for things that change around you."),
            Sec::Weather => tr("Calculated on this PC, no account needed."),
            Sec::Calendar => tr("Your next meeting from a secret ICS link. No sign-in."),
            Sec::Extensions => tr("Programs that add slides and alerts to the notch."),
            Sec::About => tr("A personal fork of Poch by Commonly Works (MIT)."),
        }
    }

    fn icon(self, g: &mut Gfx, cx: f32, cy: f32, s: f32, p: &Paint) {
        match self {
            Sec::General => g.icon_gear(cx, cy, s, p),
            Sec::Look => g.icon_outline(OutlineIcon::Palette, cx, cy, s, p),
            Sec::Slides => g.icon_outline(OutlineIcon::Layers, cx, cy, s, p),
            Sec::Screens => g.icon_outline(OutlineIcon::Monitor, cx, cy, s, p),
            Sec::Music => g.slide_icon(SlideId::Music, cx, cy, s, p),
            Sec::Notifs => g.slide_icon(SlideId::Notifications, cx, cy, s, p),
            Sec::Alerts => g.icon_outline(OutlineIcon::Speaker, cx, cy, s, p),
            Sec::Weather => g.icon_outline(OutlineIcon::Cloud, cx, cy, s, p),
            Sec::Calendar => g.slide_icon(SlideId::Agenda, cx, cy, s, p),
            Sec::Extensions => g.slide_icon(SlideId::Ext(0), cx, cy, s, p),
            Sec::About => g.icon_outline(OutlineIcon::Info, cx, cy, s, p),
        }
    }
}

/// Something clickable in the window.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Hit {
    Nav(usize),
    Close,
    Minimize,
    /// Flip a toggle or press a button.
    Press(Rid),
    Step(Rid, bool),
    Pick(Rid, usize),
    SlideToggle(usize),
    /// Move slide `i` up (true) or down in the order.
    SlideMove(usize, bool),
    /// Extensions page: switch one on or off, mute its toasts, open its settings, restart it, open its folder.
    ExtOn(usize),
    ExtMute(usize),
    ExtConfig(usize),
    ExtRestart(usize),
    ExtFolder(usize),
    /// Open the folder all extensions live in, or look for them again.
    ExtRoot,
    ExtScan,
    /// Take an extension out of the notch (two presses).
    ExtRemove(usize),
    /// The panel of an extension in panel mode: back to the list, back out of a detail page, a row, a header action, a
    /// detail button, a tab, a stepper arrow, Retry and the setup button.
    PanelBack,
    PanelDetailBack,
    PanelRow(usize),
    PanelAct(usize),
    PanelButton(usize),
    PanelTab(usize),
    PanelStep(i32),
    PanelRetry,
    PanelSetup,
    /// The text box of a page, and the button next to it.
    PanelInput,
    PanelInputSave,
    /// About > Updates: check now, or install / download the release found.
    Update,
}

/// What the About > Updates button does (`App::sw_update_action`).
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum UpdateAction {
    Check,
    Install(update::Release),
    /// A release without an installer: open its page.
    Open(String),
}

/// Animated toggle knobs: one per setting, and one per slide in the slide list.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Knob {
    Row(Rid),
    Slide(SlideId),
    /// An extension's on/off switch, and its Toasts switch (by place in the list).
    Ext(u8),
    ExtMute(u8),
}

/// The text box of an extension's page while the panel shows one.
pub(crate) struct FieldState {
    /// Which page it belongs to: the extension, the page (`for`) and the input's id.
    key: (usize, String, String),
    /// What the extension said the text is; the box goes back to it when the user gives up.
    original: String,
    max: usize,
    tf: TextField,
    focused: bool,
    /// When the caret last moved: it blinks from there.
    since: Instant,
    /// The first half of a character that came as two UTF-16 units.
    high: Option<u16>,
}

pub(crate) struct Win {
    pub(crate) hwnd: isize,
    pub(crate) sec: usize,
    pub(crate) scale: f32,
    gfx: Option<Gfx>,
    buf: Vec<u8>,
    hits: Vec<(f32, f32, f32, f32, Hit)>,
    hover: Option<Hit>,
    cut: Vec<String>,
    content_h: f32,
    scroll: f32,
    scroll_to: f32,
    /// Sidebar highlight position and velocity (springs to the selected item).
    pill: (f32, f32),
    knobs: HashMap<Knob, (f32, f32)>,
    /// Segmented controls: the thumb's position as a fractional choice index, and its velocity.
    segs: HashMap<Rid, (f32, f32)>,
    last: Instant,
    tracking: bool,
    /// Autostart as read from the registry when the window opened (not read on every frame).
    pub(crate) autostart: bool,
    /// The dictionary's Delete was pressed once: a second press within `CONFIRM_SECS` deletes it.
    pub(crate) dict_confirm: Option<Instant>,
    /// The extension whose pages the Extensions page shows instead of the list (its Settings in panel mode).
    pub(crate) panel: Option<usize>,
    /// Remove was pressed once for this extension, and when: a second press soon after does it.
    pub(crate) remove_confirm: Option<(usize, Instant)>,
    /// The text box the panel's page shows, if it shows one.
    pub(crate) field: Option<FieldState>,
    /// The update state last drawn (checking, release found, download progress): redraw when it changes.
    update_seen: (bool, Option<String>, Option<u8>),
}

impl Win {
    pub(crate) fn new() -> Win {
        Win {
            hwnd: 0,
            sec: 0,
            scale: 1.0,
            gfx: None,
            buf: Vec::new(),
            hits: Vec::new(),
            hover: None,
            cut: Vec::new(),
            content_h: 0.0,
            scroll: 0.0,
            scroll_to: 0.0,
            pill: (NAV_Y, 0.0),
            knobs: HashMap::new(),
            segs: HashMap::new(),
            last: Instant::now(),
            tracking: false,
            autostart: false,
            dict_confirm: None,
            panel: None,
            remove_confirm: None,
            field: None,
            update_seen: (false, None, None),
        }
    }

    /// Scroll back to the top of the page.
    pub(crate) fn to_top(&mut self) {
        self.scroll = 0.0;
        self.scroll_to = 0.0;
    }

    fn knob(&self, k: Knob, on: bool) -> f32 {
        self.knobs.get(&k).map_or(if on { 1.0 } else { 0.0 }, |v| v.0)
    }

    fn seg(&self, o: Rid, cur: usize) -> f32 {
        self.segs.get(&o).map_or(cur as f32, |v| v.0)
    }
}

fn nav_y(i: usize) -> f32 {
    if SECS[i] == Sec::About {
        WIN_H - 14.0 - NAV_H
    } else {
        NAV_Y + i as f32 * NAV_STEP
    }
}

fn spring(x: &mut f32, v: &mut f32, target: f32, dt: f32, k: f32, c: f32) {
    let n = ((dt / 0.008).ceil() as i32).max(1);
    let sdt = dt / n as f32;
    for _ in 0..n {
        *v += (k * (target - *x) - c * *v) * sdt;
        *x += *v * sdt;
    }
    if (target - *x).abs() < 0.002 && v.abs() < 0.02 {
        *x = target;
        *v = 0.0;
    }
}

// ---------------------------------------------------------------- what each category holds
enum Ctl {
    Toggle,
    /// Segmented control; `wide` puts it on its own line under the label, buttons sharing the width.
    Seg(Vec<String>, bool),
    Swatches,
    Stepper(String),
    /// Pill button; `danger` for the ones that remove something.
    Button(String, bool),
}

struct Row {
    id: Rid,
    label: &'static str,
    sub: std::borrow::Cow<'static, str>,
    ctl: Ctl,
}

impl Row {
    fn height(&self) -> f32 {
        if matches!(self.ctl, Ctl::Seg(_, true)) {
            ROW_H + 40.0
        } else {
            ROW_H
        }
    }
}

fn row(id: Rid, label: &'static str, sub: impl Into<std::borrow::Cow<'static, str>>, ctl: Ctl) -> Row {
    Row { id, label, sub: sub.into(), ctl }
}

impl App {
    /// The groups of rows on page `sec`: (header, rows). The labels are the ones the Settings slide used.
    fn sw_groups(&self, sec: Sec) -> Vec<(&'static str, Vec<Row>)> {
        let toggle = |o, l, s| row(o, l, s, Ctl::Toggle);
        match sec {
            Sec::General => vec![
                (tr("Startup"), vec![toggle(Rid::Autostart, tr("Autostart"), tr("Start with Windows"))]),
                (
                    tr("Language"),
                    vec![row(Rid::Language, tr("Language"), tr("Menus and slides"), Ctl::Seg(i18n::LANGS.iter().map(|s| s.to_string()).collect(), false))],
                ),
                ("Keyboard", vec![toggle(Rid::SlideHotkeys, tr("Slide hotkeys"), tr("Ctrl+Alt + slide no."))]),
                (tr("Updates"), vec![toggle(Rid::UpdateCheck, tr("Update notice"), tr("When a new version is out"))]),
            ],
            Sec::Look => vec![
                (tr("Color"), vec![row(Rid::Accent, tr("Accent"), accent::label(self.cfg.accent), Ctl::Swatches)]),
                (
                    "Notch",
                    vec![
                        row(Rid::NotchSize, tr("Notch size"), "Auto = Windows", Ctl::Seg((0..scale::SIZES.len()).map(scale::size_label).collect(), true)),
                        row(
                            Rid::IdleDisplay,
                            tr("Idle display"),
                            tr("Cycle: clock/eyes"),
                            Ctl::Seg((0..settings::IDLE_DISPLAY_LABELS.len()).map(|i| settings::idle_display_label(i).to_string()).collect(), false),
                        ),
                        toggle(Rid::Minimal, tr("Minimal notch"), tr("Thin line while idle")),
                        toggle(Rid::Glow, tr("Background glow"), tr("Off: plain black")),
                    ],
                ),
                ("Wibu", {
                    let mut rows = vec![toggle(Rid::Wibu, tr("Wibu mode"), tr("Image behind the notch"))];
                    // with Wibu mode on: a button to pick (or change) the image, and how dark it is
                    if self.cfg.wibu {
                        let sub = if self.wibu.failed { tr("Can't read the image") } else { tr("JPG, PNG or WebP") };
                        let b = if self.cfg.wibu_file.is_empty() { tr("Choose") } else { tr("Change") };
                        rows.push(row(Rid::WibuFile, tr("Wibu image"), sub, Ctl::Button(b.into(), false)));
                        rows.push(row(Rid::WibuDim, tr("Image dim"), tr("For readable text"), Ctl::Stepper(format!("{}%", self.cfg.wibu_dim))));
                    }
                    rows
                }),
            ],
            Sec::Screens => vec![
                ("Monitor", vec![row(Rid::Monitor, "Monitor", tr("Follow = cursor"), Ctl::Stepper(monitors::mode_label(self.opt_choice(Rid::Monitor))))]),
                (tr("Busy mode"), vec![toggle(Rid::ThinMaximized, tr("Thin on maximize"), tr("Off: only on fullscreen"))]),
            ],
            Sec::Music => {
                let mut rows = vec![
                    toggle(Rid::NotchSong, tr("Song in the notch"), tr("Off: idle, lyrics only from the lyrics button")),
                    row(Rid::LyricTiming, tr("Lyric timing"), tr("Show earlier by"), Ctl::Stepper(format!("{:+} ms", self.cfg.lyric_lead_ms))),
                    toggle(Rid::Romaji, tr("Romanize lyrics"), tr("Japanese and Korean")),
                ];
                // The Japanese dictionary (Korean needs none): a download button while it's missing, Delete (press twice)
                // for a downloaded one, and no row when it comes from another folder (see dict.rs).
                if dict::removable().is_some() {
                    let b = if self.dict_confirming() { tr("Sure?") } else { tr("Delete") };
                    rows.push(row(Rid::Dictionary, tr("Japanese dictionary"), tr("47 MB on disk"), Ctl::Button(b.into(), true)));
                } else if !romaji::available() {
                    rows.push(match dict::state() {
                        dict::Dl::Running(p) => row(Rid::Dictionary, tr("Japanese dictionary"), tr("Downloading dictionary"), Ctl::Button(format!("{p}%"), false)),
                        dict::Dl::Failed => row(Rid::Dictionary, tr("Japanese dictionary"), tr("Download failed"), Ctl::Button(tr("Retry").into(), false)),
                        dict::Dl::Idle => row(Rid::Dictionary, tr("Japanese dictionary"), tr("Needs 10 MB dictionary"), Ctl::Button(tr("Get").into(), false)),
                    });
                }
                vec![(tr("Lyrics"), rows)]
            }
            Sec::Notifs => vec![(
                tr("Notifications"),
                vec![
                    toggle(Rid::Notifications, tr("Notifications"), tr("Windows toasts + bell")),
                    toggle(Rid::ContactPhotos, tr("Contact photos"), tr("In notifications")),
                ],
            )],
            Sec::Alerts => vec![
                (
                    "HUD",
                    vec![toggle(Rid::HudVolume, tr("Volume HUD"), tr("Keys and mixer")), toggle(Rid::HudBrightness, tr("Brightness HUD"), tr("Fn keys / slider"))],
                ),
                (
                    tr("Indicators"),
                    vec![
                        toggle(Rid::LockKeys, tr("Lock keys"), "Caps / Num / Scroll"),
                        toggle(Rid::PrivacyDot, tr("Mic / camera dot"), tr("When an app uses them")),
                        toggle(Rid::MicMute, tr("Mic mute key"), tr("Ctrl+Alt+M, all apps")),
                    ],
                ),
                (tr("Downloads"), vec![toggle(Rid::Downloads, tr("Download progress"), tr("Downloads folder"))]),
            ],
            Sec::Weather => vec![
                (tr("Location"), vec![row(Rid::City, tr("City"), tr("Location"), Ctl::Stepper(weather::CITIES[self.cfg.weather_city].0.to_string()))]),
                (
                    tr("In the notch"),
                    vec![toggle(Rid::Weather, tr("Weather"), tr("Left of the clock"))],
                ),
                (tr("Prayer"), {
                    let mut rows = vec![
                        toggle(Rid::Prayer, tr("Prayer times"), tr("Countdown + toast")),
                        toggle(Rid::ImsakTerbit, tr("Imsak & sunrise"), tr("In the prayer list")),
                        toggle(Rid::Ramadan, tr("Ramadan mode"), tr("Imsak and iftar")),
                        row(
                            Rid::Adhan,
                            tr("Adhan sound"),
                            tr("At prayer times"),
                            Ctl::Seg((0..adhan::MODES).map(|m| adhan::mode_label(m).to_string()).collect(), false),
                        ),
                    ];
                    // with "File": the picked file, and a button to pick (or change) it
                    if self.cfg.adhan == adhan::FILE {
                        let (sub, b) = if self.cfg.adhan_file.is_empty() {
                            (tr("mp3 or wav").into(), tr("Choose"))
                        } else {
                            (adhan::file_label(&self.cfg.adhan_file).to_string(), tr("Change"))
                        };
                        rows.push(row(Rid::AdhanFile, tr("Adhan file"), sub, Ctl::Button(b.into(), false)));
                    }
                    rows
                }),
            ],
            Sec::Calendar => vec![(
                tr("Calendar link"),
                vec![row(Rid::Calendar, tr("Calendar"), self.cal_sub(), Ctl::Button(self.cal_button_text().into(), self.cal_link.is_some()))],
            )],
            Sec::Slides | Sec::Extensions | Sec::About => Vec::new(),
        }
    }

    fn sw_target(&self, k: Knob) -> f32 {
        let on = match k {
            Knob::Row(o) => self.opt_on(o),
            Knob::Slide(id) => self.cfg.slides.iter().any(|&(s, on)| s == id && on),
            Knob::Ext(i) => self.ext.list.get(i as usize).is_some_and(|e| self.cfg.ext_on.iter().any(|x| x == e.id())),
            Knob::ExtMute(i) => self.ext.list.get(i as usize).is_some_and(|e| !self.cfg.ext_mute.iter().any(|x| x == e.id())),
        };
        if on {
            1.0
        } else {
            0.0
        }
    }

    fn sw_animating(&self) -> bool {
        let w = &self.sw;
        (w.pill.0 - nav_y(w.sec)).abs() > 0.01
            || w.pill.1.abs() > 0.01
            || (w.scroll - w.scroll_to).abs() > 0.3
            || w.knobs.iter().any(|(&k, &(x, v))| (x - self.sw_target(k)).abs() > 0.001 || v.abs() > 0.01)
            || w.segs.iter().any(|(&o, &(x, v))| (x - self.opt_choice(o) as f32).abs() > 0.001 || v.abs() > 0.01)
    }

    /// Things that change by themselves and need a redraw now and then: the dictionary download and the
    /// "press again" buttons timing out.
    fn sw_slow_redraw(&mut self) -> bool {
        let recent = |t: Option<Instant>| t.is_some_and(|t| t.elapsed().as_secs_f32() < 4.0);
        let upd = (self.update_busy, self.update.as_ref().map(|r| r.version.clone()), self.update_downloading());
        let upd_changed = upd != self.sw.update_seen;
        self.sw.update_seen = upd;
        upd_changed
            || self.update_busy
            || self.sw.remove_confirm.is_some_and(|(_, t)| t.elapsed().as_secs_f32() < 4.0)
            || self.sw.field.as_ref().is_some_and(|f| f.focused)
            || matches!(dict::state(), dict::Dl::Running(_))
            || recent(self.sw.dict_confirm)
            || recent(self.cal_confirm)
            || recent(self.cal_paste_err)
    }

    /// The installer download's progress while the update toast shows it.
    fn update_downloading(&self) -> Option<u8> {
        match self.toast.as_ref().map(|t| &t.kind) {
            Some(ToastKind::Update(update::Note::Downloading(p))) => Some(*p),
            _ => None,
        }
    }

    /// What the About > Updates button does now: install (or download) the release already found, else check. None
    /// while a check or a download runs (no button then).
    pub(crate) fn sw_update_action(&self) -> Option<UpdateAction> {
        if self.update_busy || self.update_downloading().is_some() {
            return None;
        }
        Some(match &self.update {
            Some(r) if r.installer.is_some() => UpdateAction::Install(r.clone()),
            Some(r) => UpdateAction::Open(r.url.clone()),
            None => UpdateAction::Check,
        })
    }

    /// About > Updates button. The answer shows as the usual update toast.
    fn sw_update_button(&mut self) {
        match self.sw_update_action() {
            Some(UpdateAction::Install(r)) => self.update_install(&r),
            Some(UpdateAction::Open(url)) => sys::open_url(&url),
            Some(UpdateAction::Check) => self.update_check_now(),
            None => {}
        }
    }

    fn sw_step(&mut self) {
        let now = Instant::now();
        let dt = (now - self.sw.last).as_secs_f32().min(0.05);
        self.sw.last = now;
        let pill_to = nav_y(self.sw.sec);
        let (mut y, mut v) = self.sw.pill;
        spring(&mut y, &mut v, pill_to, dt, 420.0, 36.0);
        self.sw.pill = (y, v);
        self.sw.scroll += (self.sw.scroll_to - self.sw.scroll) * (1.0 - (-dt * 18.0).exp());
        if (self.sw.scroll - self.sw.scroll_to).abs() < 0.3 {
            self.sw.scroll = self.sw.scroll_to;
        }
        let knobs: Vec<(Knob, f32)> = self.sw.knobs.keys().map(|&k| (k, self.sw_target(k))).collect();
        for (k, t) in knobs {
            if let Some((x, v)) = self.sw.knobs.get_mut(&k) {
                spring(x, v, t, dt, 380.0, 30.0);
            }
        }
        let segs: Vec<(Rid, f32)> = self.sw.segs.keys().map(|&o| (o, self.opt_choice(o) as f32)).collect();
        for (o, t) in segs {
            if let Some((x, v)) = self.sw.segs.get_mut(&o) {
                spring(x, v, t, dt, 420.0, 34.0);
            }
        }
    }

    /// Open the Settings window, or bring it to the front if it is already open.
    pub(crate) fn open_settings_window(&mut self) {
        unsafe {
            let h = HWND(self.sw.hwnd as *mut c_void);
            if self.sw.hwnd != 0 && IsWindow(h).as_bool() {
                if IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                let _ = SetForegroundWindow(h);
                return;
            }
            self.sw.autostart = sys::autostart_enabled();
            let dpi = monitors::dpi_of(&self.mon);
            let s = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            self.sw.scale = s;
            SCALE.store(s.to_bits(), Ordering::Relaxed);
            let (w, h) = ((WIN_W * s).round() as i32, (WIN_H * s).round() as i32);
            // centred on the work area of the notch's monitor
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let work = if GetMonitorInfoW(MonitorFromRect(&self.mon, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() { mi.rcWork } else { self.mon };
            let x = work.left + (work.right - work.left - w) / 2;
            let y = work.top + (work.bottom - work.top - h) / 2;
            let Ok(hinst) = GetModuleHandleW(None) else { return };
            register_class(hinst.into());
            let Ok(hwnd) = CreateWindowExW(
                WS_EX_APPWINDOW,
                w!("HesewakeSettings"),
                w!("Hesewake Notch Settings"),
                // WS_CAPTION keeps the DWM shadow and snap; the caption itself is removed in WM_NCCALCSIZE.
                WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
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
            let round = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &round as *const _ as *const c_void,
                std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );
            let dark: i32 = 1;
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as *const c_void, 4);
            self.sw.hwnd = hwnd.0 as isize;
            self.sw.gfx = None;
            self.sw.scroll = 0.0;
            self.sw.scroll_to = 0.0;
            self.sw.pill = (nav_y(self.sw.sec), 0.0);
            self.sw.knobs.clear();
            self.sw.segs.clear();
            SetTimer(hwnd, 2, 500, None);
            // Windows only asks for the frame again (WM_NCCALCSIZE, where the caption is dropped) once it is told
            // the frame changed; without this the system title bar stays.
            let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }

    /// Tests: show page `i` with the sidebar highlight already on it.
    #[cfg(test)]
    pub(crate) fn sw_show_page(&mut self, i: usize) {
        self.sw.sec = i.min(SECS.len() - 1);
        self.sw.pill = (nav_y(self.sw.sec), 0.0);
    }

    /// Redraw the window (if open), and start the animation timer if something moves.
    pub(crate) fn sw_kick(&mut self) {
        if self.sw.hwnd == 0 {
            return; // no window (tests); a null HWND would invalidate every window
        }
        let h = HWND(self.sw.hwnd as *mut c_void);
        unsafe {
            if self.sw_animating() {
                self.sw.last = Instant::now();
                SetTimer(h, 1, 16, None);
            }
            let _ = InvalidateRect(h, None, false);
        }
    }

    pub(crate) fn sw_click(&mut self, hit: Hit) {
        // a click anywhere but in the text box leaves it
        if !matches!(hit, Hit::PanelInput | Hit::PanelInputSave) {
            self.sw_field_focus(false);
        }
        // Remember where the knob / thumb is before the change, so it springs from there.
        match hit {
            Hit::Press(o) => {
                let t = self.sw_target(Knob::Row(o));
                self.sw.knobs.entry(Knob::Row(o)).or_insert((t, 0.0));
            }
            Hit::Pick(o, _) => {
                let c = self.opt_choice(o) as f32;
                self.sw.segs.entry(o).or_insert((c, 0.0));
            }
            Hit::SlideToggle(i) => {
                if let Some(&(id, _)) = self.cfg.slides.get(i) {
                    let t = self.sw_target(Knob::Slide(id));
                    self.sw.knobs.entry(Knob::Slide(id)).or_insert((t, 0.0));
                }
            }
            Hit::ExtOn(i) | Hit::ExtMute(i) => {
                let k = if matches!(hit, Hit::ExtOn(_)) { Knob::Ext(i as u8) } else { Knob::ExtMute(i as u8) };
                let t = self.sw_target(k);
                self.sw.knobs.entry(k).or_insert((t, 0.0));
            }
            _ => {}
        }
        match hit {
            Hit::Nav(i) => {
                if self.sw.sec != i {
                    self.ext_panel_close();
                    self.sw.sec = i;
                    self.sw.scroll = 0.0;
                    self.sw.scroll_to = 0.0;
                }
            }
            Hit::Press(o) => self.opt_press(o),
            Hit::Step(o, plus) => self.opt_step(o, plus),
            Hit::Pick(o, i) => self.opt_pick(o, i),
            Hit::SlideToggle(i) => self.slide_toggle(i),
            Hit::SlideMove(i, up) => {
                if up && i > 0 {
                    self.slide_swap(i - 1, i);
                } else if !up {
                    self.slide_swap(i, i + 1);
                }
            }
            Hit::ExtOn(i) => self.ext_set_on(i),
            Hit::ExtMute(i) => self.ext_toggle_mute(i),
            Hit::ExtConfig(i) => self.ext_open_config(i),
            Hit::ExtRestart(i) => self.ext_restart(i),
            Hit::ExtFolder(i) => self.ext_open_folder(Some(i)),
            Hit::ExtRoot => self.ext_open_folder(None),
            Hit::ExtScan => self.ext_rescan(),
            Hit::ExtRemove(i) => self.ext_remove(i),
            Hit::PanelBack => self.ext_panel_close(),
            Hit::PanelDetailBack => self.ext_panel_detail_back(),
            Hit::PanelRow(n) => self.ext_panel_row(n),
            Hit::PanelAct(n) => self.ext_panel_act(n),
            Hit::PanelButton(n) => self.ext_panel_button(n),
            Hit::PanelTab(n) => self.ext_panel_tab(n),
            Hit::PanelStep(d) => self.ext_panel_step(d),
            Hit::PanelRetry => self.ext_panel_action("retry"),
            Hit::PanelSetup => self.ext_panel_action("setup"),
            Hit::PanelInput => self.sw_field_focus(true),
            Hit::PanelInputSave => self.sw_field_commit(),
            Hit::Update => self.sw_update_button(),
            Hit::Close | Hit::Minimize => {}
        }
        // the notch shows the change too (accent, size, language, slides...)
        self.last = Instant::now();
        unsafe {
            present(self);
            schedule(self);
        }
        self.sw_kick();
    }

    pub(crate) fn sw_hit(&self, x: f32, y: f32) -> Option<Hit> {
        self.sw.hits.iter().rev().find(|&&(hx, hy, hw, hh, _)| in_rect(x, y, hx, hy, hw, hh)).map(|h| h.4)
    }
}

unsafe fn register_class(hinst: HINSTANCE) {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: w!("HesewakeSettings"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            // the app icon from the exe's resources (winresource puts it at id 1)
            hIcon: LoadIconW(hinst, PCWSTR(1 as *const u16)).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
}

// ---------------------------------------------------------------- window procedure
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match std::panic::catch_unwind(|| wndproc_inner(hwnd, msg, wp, lp)) {
        Ok(r) => r,
        Err(_) => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn lp_xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xffff) as i16 as i32, ((lp.0 >> 16) & 0xffff) as i16 as i32)
}

/// Window buttons, top right.
fn close_rect() -> (f32, f32, f32, f32) {
    (WIN_W - 42.0, 8.0, 32.0, 30.0)
}
fn min_rect() -> (f32, f32, f32, f32) {
    (WIN_W - 78.0, 8.0, 32.0, 30.0)
}

unsafe fn wndproc_inner(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    enum After {
        Nothing,
        Close,
        Minimize,
    }
    match msg {
        // No non-client area: the whole window is ours to draw (the caption style only keeps shadow and snap).
        WM_NCCALCSIZE if wp.0 != 0 => LRESULT(0),
        WM_NCHITTEST => {
            let (sx, sy) = lp_xy(lp);
            let mut pt = POINT { x: sx, y: sy };
            let _ = ScreenToClient(hwnd, &mut pt);
            let s = scale();
            let (x, y) = (pt.x as f32 / s, pt.y as f32 / s);
            let on_button = [close_rect(), min_rect()].iter().any(|r| in_rect(x, y, r.0, r.1, r.2, r.3));
            // the top bar and the brand corner drag the window
            if (y < BAR_H && x > SIDE_W && !on_button) || (y < 48.0 && x < SIDE_W) {
                LRESULT(HTCAPTION as isize)
            } else {
                LRESULT(HTCLIENT as isize)
            }
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            with_app(|a| {
                render(a);
                blit(a, hdc);
            });
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_TIMER => {
            let mut stop = false;
            with_app(|a| {
                if wp.0 == 1 {
                    a.sw_step();
                    stop = !a.sw_animating();
                    let _ = InvalidateRect(hwnd, None, false);
                } else if a.sw_slow_redraw() {
                    let _ = InvalidateRect(hwnd, None, false);
                }
            });
            if stop {
                let _ = KillTimer(hwnd, 1);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (px, py) = lp_xy(lp);
            let s = scale();
            with_app(|a| {
                if !a.sw.tracking {
                    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    a.sw.tracking = TrackMouseEvent(&mut tme).is_ok();
                }
                let h = a.sw_hit(px as f32 / s, py as f32 / s);
                if h != a.sw.hover {
                    a.sw.hover = h;
                    let _ = InvalidateRect(hwnd, None, false);
                }
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_app(|a| {
                a.sw.tracking = false;
                if a.sw.hover.take().is_some() {
                    let _ = InvalidateRect(hwnd, None, false);
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (px, py) = lp_xy(lp);
            let s = scale();
            let mut after = After::Nothing;
            with_app(|a| match a.sw_hit(px as f32 / s, py as f32 / s) {
                Some(Hit::Close) => after = After::Close,
                Some(Hit::Minimize) => after = After::Minimize,
                Some(h) => a.sw_click(h),
                None => a.sw_field_focus(false),
            });
            match after {
                After::Close => {
                    let _ = DestroyWindow(hwnd);
                }
                After::Minimize => {
                    let _ = ShowWindow(hwnd, SW_MINIMIZE);
                }
                After::Nothing => {}
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
            with_app(|a| {
                let max = (a.sw.content_h - WIN_H + 28.0).max(0.0);
                a.sw.scroll_to = (a.sw.scroll_to - delta * 64.0).clamp(0.0, max);
                a.sw_kick();
            });
            LRESULT(0)
        }
        WM_CHAR => {
            let mut used = false;
            with_app(|a| used = a.sw_field_char(wp.0 as u16));
            if used {
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let (mut used, ctrl) = (false, windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11) < 0);
            with_app(|a| used = a.sw_field_key(wp.0 as u16, ctrl));
            if used {
                let _ = InvalidateRect(hwnd, None, false);
            } else if wp.0 == VK_ESCAPE.0 as usize {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_ACTIVATE => {
            // settings may have changed from the tray menu meanwhile
            let _ = InvalidateRect(hwnd, None, false);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_DPICHANGED => {
            let s = (wp.0 & 0xffff) as f32 / 96.0;
            SCALE.store(s.to_bits(), Ordering::Relaxed);
            with_app(|a| {
                a.sw.scale = s;
                a.sw.gfx = None;
            });
            let r = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            LRESULT(0)
        }
        WM_DESTROY => {
            with_app(|a| {
                a.ext_panel_close();
                a.sw.hwnd = 0;
                a.sw.gfx = None;
                a.sw.buf = Vec::new();
                a.sw.hover = None;
                a.sw.tracking = false;
            });
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Copy the canvas into the window (RGBA -> BGRA; the canvas is opaque).
unsafe fn blit(a: &mut App, hdc: HDC) {
    let Some(g) = a.sw.gfx.as_ref() else { return };
    let (w, h) = (g.pm.width(), g.pm.height());
    let src = g.pm.data();
    a.sw.buf.resize(src.len(), 0);
    for (d, s) in a.sw.buf.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
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
    SetDIBitsToDevice(hdc, 0, 0, w, h, 0, 0, 0, h, a.sw.buf.as_ptr() as *const c_void, &bmi, DIB_RGB_COLORS);
}

// ---------------------------------------------------------------- drawing
/// Draw the window into its canvas and remember where the clickable parts are.
pub(crate) fn render(a: &mut App) {
    a.sw_sync_field();
    let s = a.sw.scale;
    let mut g = match a.sw.gfx.take() {
        Some(g) if g.s == s => g,
        _ => Gfx::sized(s, WIN_W, WIN_H),
    };
    g.acc = accent::get(a.cfg.accent);
    g.pm.fill(Color::from_rgba8(PANEL[0], PANEL[1], PANEL[2], 255));
    let mut d = Draw { hits: Vec::new(), hover: a.sw.hover, cut: Vec::new() };
    let content_h = d.page(a, &mut g);
    d.chrome(a, &mut g);
    a.sw.gfx = Some(g);
    a.sw.hits = d.hits;
    a.sw.cut = d.cut;
    a.sw.content_h = content_h;
}

#[cfg(test)]
pub(crate) fn canvas(a: &App) -> Option<&Pixmap> {
    a.sw.gfx.as_ref().map(|g| &g.pm)
}

/// Tests: the centre of `hit` in the last drawn frame, and the page height.
#[cfg(test)]
pub(crate) fn hit_center(a: &App, hit: Hit) -> Option<(f32, f32)> {
    a.sw.hits.iter().find(|h| h.4 == hit).map(|&(x, y, w, h, _)| (x + w / 2.0, y + h / 2.0))
}

#[cfg(test)]
pub(crate) fn content_h(a: &App) -> f32 {
    a.sw.content_h
}

/// Tests: texts cut on the last frame.
#[cfg(test)]
pub(crate) fn cut(a: &App) -> &[String] {
    &a.sw.cut
}

/// Tests: page `i`'s rows, as (setting, label).
#[cfg(test)]
pub(crate) fn rows_on(a: &App, i: usize) -> Vec<(Rid, &'static str)> {
    a.sw_groups(SECS[i]).into_iter().flat_map(|(_, rows)| rows).map(|r| (r.id, r.label)).collect()
}

/// Tests: the description shown under setting `r` on page `i`.
#[cfg(test)]
pub(crate) fn row_sub(a: &App, i: usize, r: Rid) -> Option<String> {
    a.sw_groups(SECS[i]).into_iter().flat_map(|(_, rows)| rows).find(|row| row.id == r).map(|row| row.sub.into_owned())
}

/// Tests: scroll the page to its end (as far as the wheel goes).
#[cfg(test)]
pub(crate) fn scroll_to_end(a: &mut App) {
    let max = (a.sw.content_h - WIN_H + 28.0).max(0.0);
    a.sw.scroll = max;
    a.sw.scroll_to = max;
}

struct Draw {
    hits: Vec<(f32, f32, f32, f32, Hit)>,
    hover: Option<Hit>,
    /// Texts that did not fit their room on this frame (tests check none is cut).
    cut: Vec<String>,
}

fn rgb(c: [u8; 3], a: f32) -> Paint<'static> {
    paint(c[0], c[1], c[2], a)
}

impl Draw {
    fn hit(&mut self, x: f32, y: f32, w: f32, h: f32, hit: Hit) {
        // only the part inside the page can be clicked: not under the top bar, not below the window
        let (top, bottom) = (y.max(BAR_H), (y + h).min(WIN_H));
        if bottom > top {
            self.hits.push((x, top, w, bottom - top, hit));
        }
    }

    /// Note `s` when it is wider than `maxw` (it gets cut on screen).
    fn check(&mut self, g: &mut Gfx, s: &str, size: f32, bold: bool, maxw: f32) {
        if g.width_ex(s, size, bold) > maxw {
            self.cut.push(s.to_string());
        }
    }

    fn is_hover(&self, h: Hit) -> bool {
        self.hover == Some(h)
    }

    /// Sidebar and top bar: drawn last, over the scrolled page.
    fn chrome(&mut self, a: &App, g: &mut Gfx) {
        // fade the page out under the top bar
        g.rect(SIDE_W, 0.0, WIN_W - SIDE_W, BAR_H, &rgb(PANEL, 1.0));
        let stops = vec![
            GradientStop::new(0.0, Color::from_rgba8(PANEL[0], PANEL[1], PANEL[2], 255)),
            GradientStop::new(1.0, Color::from_rgba8(PANEL[0], PANEL[1], PANEL[2], 0)),
        ];
        if let Some(sh) = LinearGradient::new(Point::from_xy(0.0, BAR_H), Point::from_xy(0.0, BAR_H + 16.0), stops, SpreadMode::Pad, Transform::identity()) {
            let mut p = Paint::default();
            p.shader = sh;
            g.rect(SIDE_W, BAR_H, WIN_W - SIDE_W, 16.0, &p);
        }

        // window buttons
        let (cx, cy, cw, ch) = close_rect();
        if self.is_hover(Hit::Close) {
            g.fill(&rrect(cx, cy, cw, ch, 9.0), &paint(232, 17, 35, 1.0));
        }
        let col = if self.is_hover(Hit::Close) { paint(255, 255, 255, 1.0) } else { rgb(TEXT_FAINT, 1.0) };
        let (mx, my) = (cx + cw / 2.0, cy + ch / 2.0);
        g.line(mx - 5.0, my - 5.0, mx + 5.0, my + 5.0, 1.6, &col);
        g.line(mx + 5.0, my - 5.0, mx - 5.0, my + 5.0, 1.6, &col);
        self.hits.push((cx, cy, cw, ch, Hit::Close));
        let (nx, ny, nw, nh) = min_rect();
        if self.is_hover(Hit::Minimize) {
            g.fill(&rrect(nx, ny, nw, nh, 9.0), &paint(255, 255, 255, 0.08));
        }
        let col = if self.is_hover(Hit::Minimize) { paint(255, 255, 255, 1.0) } else { rgb(TEXT_FAINT, 1.0) };
        g.line(nx + nw / 2.0 - 6.0, ny + nh / 2.0, nx + nw / 2.0 + 6.0, ny + nh / 2.0, 1.6, &col);
        self.hits.push((nx, ny, nw, nh, Hit::Minimize));

        self.sidebar(a, g);
    }

    fn sidebar(&mut self, a: &App, g: &mut Gfx) {
        g.rect(0.0, 0.0, SIDE_W, WIN_H, &paint(0, 0, 0, 1.0));
        g.rect(SIDE_W, 0.0, 1.0, WIN_H, &paint(255, 255, 255, 0.07));
        // brand: the app icon, the name and the version
        let (bx, by) = (22.0, 18.0);
        a.icon_at(g, bx, by, 22.0, 6.0, 1.0);
        g.txtb("Hesewake", bx + 32.0, by + ty(22.0, 14.0), 14.0, [255, 255, 255], 1.0, 110.0);
        let ver = env!("CARGO_PKG_VERSION");
        let vw = g.width(ver, 11.0);
        g.txt(ver, SIDE_W - 18.0 - vw, by + ty(22.0, 11.0), 11.0, TEXT_FAINT, 1.0, 60.0);

        // the highlight springs to the selected item
        g.fill(&rrect(10.0, a.sw.pill.0, SIDE_W - 20.0, NAV_H, 10.0), &paint(255, 255, 255, 0.075));
        g.rect(10.0, WIN_H - 14.0 - NAV_H - 9.0, SIDE_W - 20.0, 1.0, &paint(255, 255, 255, 0.07));
        for (i, &sec) in SECS.iter().enumerate() {
            let y = nav_y(i);
            let on = i == a.sw.sec;
            let hot = self.is_hover(Hit::Nav(i));
            let icol = if on { g.sacc(1.0) } else if hot { paint(255, 255, 255, 0.9) } else { rgb(TEXT_FAINT, 1.0) };
            sec.icon(g, 32.0, y + NAV_H / 2.0, 15.0, &icol);
            let tcol = if on || hot { [255, 255, 255] } else { TEXT_DIM };
            let ty0 = y + ty(NAV_H, 13.0);
            self.check(g, sec.name(), 13.0, true, SIDE_W - 66.0);
            if on {
                g.txtb(sec.name(), 50.0, ty0, 13.0, tcol, 1.0, SIDE_W - 66.0);
            } else {
                g.txt(sec.name(), 50.0, ty0, 13.0, tcol, 1.0, SIDE_W - 66.0);
            }
            self.hits.push((10.0, y, SIDE_W - 20.0, NAV_H, Hit::Nav(i)));
        }
    }

    /// The scrolled page. Returns its height.
    fn page(&mut self, a: &App, g: &mut Gfx) -> f32 {
        // the accent glow at the bottom, like the slides have
        let gl = g.acc.glow;
        let stops = vec![
            GradientStop::new(0.0, Color::from_rgba8(gl[0], gl[1], gl[2], 0)),
            GradientStop::new(1.0, Color::from_rgba8(gl[0], gl[1], gl[2], 46)),
        ];
        if let Some(sh) = LinearGradient::new(Point::from_xy(0.0, WIN_H * 0.55), Point::from_xy(0.0, WIN_H), stops, SpreadMode::Pad, Transform::identity()) {
            let mut p = Paint::default();
            p.shader = sh;
            g.rect(SIDE_W, WIN_H * 0.55, WIN_W - SIDE_W, WIN_H * 0.45, &p);
        }

        let sec = SECS[a.sw.sec.min(SECS.len() - 1)];
        let off = -a.sw.scroll;
        let (t1, t2) = sec.title().split_once('|').unwrap_or((sec.title(), ""));
        let y = 56.0 + off;
        g.txtb(t1, X0, y, 26.0, [255, 255, 255], 1.0, X1 - X0);
        let w1 = g.width_ex(t1, 26.0, true);
        g.txtb(t2, X0 + w1, y, 26.0, g.acc.main, 1.0, X1 - X0 - w1);
        g.txt(sec.desc(), X0, y + 40.0, 13.0, TEXT_FAINT, 1.0, X1 - X0);
        let mut y = y + 78.0;

        match sec {
            Sec::Slides => y = self.slides(a, g, y),
            Sec::Extensions => y = self.extensions(a, g, y),
            Sec::About => {
                y = self.about(a, g, y);
                y = self.updates(a, g, y);
            }
            _ => {
                for (head, rows) in a.sw_groups(sec) {
                    y = self.group(a, g, head, &rows, y);
                }
            }
        }
        y - off + 8.0
    }

    fn card(g: &mut Gfx, y: f32, h: f32) {
        let p = rrect(X0, y, X1 - X0, h, 14.0);
        g.fill(&p, &rgb(CARD, 1.0));
        g.stroke(&p, &paint(255, 255, 255, 0.07), 1.0);
    }

    fn header(g: &mut Gfx, head: &str, y: f32) {
        g.txtb(&head.to_uppercase(), X0 + 2.0, y, 10.5, TEXT_FAINT, 1.0, X1 - X0);
    }

    fn group(&mut self, a: &App, g: &mut Gfx, head: &str, rows: &[Row], y: f32) -> f32 {
        Self::header(g, head, y);
        let y = y + 22.0;
        let h: f32 = rows.iter().map(Row::height).sum();
        Self::card(g, y, h);
        let mut ry = y;
        for (i, r) in rows.iter().enumerate() {
            if i > 0 {
                g.rect(X0 + 16.0, ry, X1 - X0 - 32.0, 1.0, &paint(255, 255, 255, 0.07));
            }
            self.row(a, g, r, ry);
            ry += r.height();
        }
        y + h + 22.0
    }

    fn row(&mut self, a: &App, g: &mut Gfx, r: &Row, y: f32) {
        let (rx, rw) = (X0, X1 - X0);
        // the control, right-aligned; its left edge limits the text
        let right = rx + rw - 16.0;
        let cy = y + ROW_H / 2.0;
        let ctl_x = match &r.ctl {
            Ctl::Toggle => {
                let kx = right - 46.0;
                g.toggle(kx, cy - 13.0, 46.0, 26.0, a.sw.knob(Knob::Row(r.id), a.opt_on(r.id)), 0.0, 1.0);
                // the whole row flips the toggle
                self.hit(rx, y, rw, ROW_H, Hit::Press(r.id));
                kx
            }
            Ctl::Seg(opts, wide) => {
                let cur = a.opt_choice(r.id);
                let pos = a.sw.seg(r.id, cur);
                if *wide {
                    let (sx, sw) = (rx + 16.0, rw - 32.0);
                    let bw = sw / opts.len() as f32;
                    let rects: Vec<(f32, f32)> = (0..opts.len()).map(|i| (sx + i as f32 * bw, bw)).collect();
                    self.seg(g, r.id, opts, &rects, cur, pos, y + ROW_H - 6.0);
                    right
                } else {
                    let ws: Vec<f32> = opts.iter().map(|o| g.width_ex(o, 12.5, true).ceil() + 26.0).collect();
                    let total: f32 = ws.iter().sum::<f32>() + 6.0;
                    let mut x = right - total + 3.0;
                    let rects: Vec<(f32, f32)> = ws
                        .iter()
                        .map(|&w| {
                            let r = (x, w);
                            x += w;
                            r
                        })
                        .collect();
                    self.seg(g, r.id, opts, &rects, cur, pos, cy - 15.0);
                    right - total
                }
            }
            Ctl::Swatches => {
                let n = accent::ACCENTS.len();
                let x0 = right - (n as f32 - 1.0) * 32.0 - 11.0;
                for i in 0..n {
                    let c = accent::get(i).main;
                    let x = x0 + i as f32 * 32.0;
                    let hot = self.is_hover(Hit::Pick(r.id, i));
                    if i == a.opt_choice(r.id) {
                        if let Some(p) = PathBuilder::from_circle(x, cy, 15.0) {
                            g.stroke(&p, &rgb(c, 1.0), 2.0);
                        }
                    }
                    g.circle(x, cy, if hot { 12.5 } else { 11.0 }, &rgb(c, 1.0));
                    self.hit(x - 16.0, cy - 16.0, 32.0, 32.0, Hit::Pick(r.id, i));
                }
                x0 - 16.0
            }
            Ctl::Stepper(v) => {
                let vw = g.width_ex(v, 13.0, true).max(64.0) + 16.0;
                let px = right - 14.0;
                let mx = px - 28.0 - vw;
                for (x, plus) in [(mx, false), (px, true)] {
                    let hot = self.is_hover(Hit::Step(r.id, plus));
                    g.circle(x, cy, 14.0, &paint(255, 255, 255, if hot { 0.14 } else { 0.07 }));
                    let c = paint(255, 255, 255, 0.9);
                    let d = if plus { 1.0 } else { -1.0 };
                    g.line(x - 2.0 * d, cy - 4.5, x + 2.5 * d, cy, 1.6, &c);
                    g.line(x + 2.5 * d, cy, x - 2.0 * d, cy + 4.5, 1.6, &c);
                    self.hit(x - 16.0, cy - 16.0, 32.0, 32.0, Hit::Step(r.id, plus));
                }
                g.txt_c(v, (mx + px) / 2.0, cy + ty(0.0, 13.0), 13.0, [255, 255, 255], 1.0, vw, true);
                mx - 16.0
            }
            Ctl::Button(label, danger) => {
                let bw = g.width_ex(label, 12.5, true).ceil() + 30.0;
                let bx = right - bw;
                let hot = self.is_hover(Hit::Press(r.id));
                let (fill, text) = if *danger { ([255, 69, 58], [255, 165, 155]) } else { (g.acc.bright, g.acc.pale) };
                g.fill(&rrect(bx, cy - 15.0, bw, 30.0, 15.0), &rgb(fill, if hot { 0.32 } else { 0.2 }));
                g.txt_c(label, bx + bw / 2.0, cy + ty(0.0, 12.5), 12.5, text, 1.0, bw, true);
                self.hit(bx, cy - 15.0, bw, 30.0, Hit::Press(r.id));
                bx
            }
        };
        let tw = (ctl_x - rx - 32.0).max(40.0);
        self.check(g, r.label, 14.0, true, tw);
        self.check(g, &r.sub, 12.0, false, tw);
        g.txtb(r.label, rx + 16.0, y + 11.0, 14.0, [255, 255, 255], 1.0, tw);
        let sub = fit(g, &r.sub, 12.0, tw);
        g.txt(&sub, rx + 16.0, y + 31.0, 12.0, TEXT_FAINT, 1.0, tw + 2.0);
    }

    /// Segmented control: buttons at `rects` (x, width), a thumb that slides to the chosen one.
    fn seg(&mut self, g: &mut Gfx, o: Rid, opts: &[String], rects: &[(f32, f32)], cur: usize, pos: f32, y: f32) {
        let (x0, x1) = (rects[0].0 - 3.0, rects[rects.len() - 1].0 + rects[rects.len() - 1].1 + 3.0);
        let bg = rrect(x0, y, x1 - x0, 30.0, 10.0);
        g.fill(&bg, &rgb(WELL, 1.0));
        g.stroke(&bg, &paint(255, 255, 255, 0.07), 1.0);
        // thumb between the two buttons around `pos`
        let pos = pos.clamp(0.0, (rects.len() - 1) as f32);
        let (i0, f) = (pos.floor() as usize, pos.fract());
        let i1 = (i0 + 1).min(rects.len() - 1);
        let tx = rects[i0].0 + (rects[i1].0 - rects[i0].0) * f;
        let tw = rects[i0].1 + (rects[i1].1 - rects[i0].1) * f;
        let th = rrect(tx, y + 3.0, tw, 24.0, 7.0);
        g.fill(&th, &g.sacc(0.24));
        g.stroke(&th, &g.sacc(0.4), 1.0);
        for (i, (label, &(bx, bw))) in opts.iter().zip(rects).enumerate() {
            let hot = self.is_hover(Hit::Pick(o, i));
            let col = if i == cur { g.acc.pale } else if hot { [255, 255, 255] } else { TEXT_DIM };
            self.check(g, label, 12.5, true, bw - 4.0);
            g.txt_c(label, bx + bw / 2.0, y + 3.0 + ty(24.0, 12.5), 12.5, col, 1.0, bw - 4.0, i == cur);
            self.hit(bx, y, bw, 30.0, Hit::Pick(o, i));
        }
    }

    fn slides(&mut self, a: &App, g: &mut Gfx, y: f32) -> f32 {
        Self::header(g, "Carousel", y);
        let y = y + 22.0;
        let n = a.cfg.slides.len();
        let h = n as f32 * SLIDE_ROW_H + 8.0;
        Self::card(g, y, h);
        for (i, &(id, on)) in a.cfg.slides.iter().enumerate() {
            let ry = y + 4.0 + i as f32 * SLIDE_ROW_H;
            let cy = ry + SLIDE_ROW_H / 2.0;
            if i > 0 {
                g.rect(X0 + 16.0, ry, X1 - X0 - 32.0, 1.0, &paint(255, 255, 255, 0.05));
            }
            // icon in a small well
            let ib = rrect(X0 + 14.0, cy - 16.0, 32.0, 32.0, 9.0);
            g.fill(&ib, &rgb(WELL, 1.0));
            g.stroke(&ib, &paint(255, 255, 255, 0.07), 1.0);
            g.slide_icon(id, X0 + 30.0, cy, 14.0, &if on { g.sacc(1.0) } else { rgb(TEXT_FAINT, 1.0) });
            // right side: toggle, down, up, hotkey
            let right = X1 - 16.0;
            let kx = right - 46.0;
            g.toggle(kx, cy - 13.0, 46.0, 26.0, a.sw.knob(Knob::Slide(id), on), 0.0, 1.0);
            self.hit(kx - 4.0, cy - 17.0, 54.0, 34.0, Hit::SlideToggle(i));
            for (bx, up, ok) in [(kx - 22.0, false, i + 1 < n), (kx - 52.0, true, i > 0)] {
                let hit = Hit::SlideMove(i, up);
                let hot = ok && self.is_hover(hit);
                if hot {
                    g.circle(bx, cy, 13.0, &paint(255, 255, 255, 0.1));
                }
                let c = paint(255, 255, 255, if !ok { 0.15 } else if hot { 1.0 } else { 0.6 });
                let d = if up { -1.0 } else { 1.0 };
                g.line(bx - 4.5, cy - 2.0 * d, bx, cy + 2.5 * d, 1.6, &c);
                g.line(bx, cy + 2.5 * d, bx + 4.5, cy - 2.0 * d, 1.6, &c);
                if ok {
                    self.hit(bx - 14.0, cy - 15.0, 28.0, 30.0, hit);
                }
            }
            let mut text_r = kx - 70.0;
            // the same number as the slide hotkey (hotkeys.rs)
            if let Some(key) = hotkeys::number(&a.slide_cfg(), i).filter(|_| a.cfg.slide_hotkeys) {
                let k = format!("Ctrl+Alt+{}", key % 10);
                let kw = g.width_ex(&k, 10.5, true).ceil() + 14.0;
                let kx0 = kx - 74.0 - kw;
                let kr = rrect(kx0, cy - 10.0, kw, 20.0, 6.0);
                g.stroke(&kr, &paint(255, 255, 255, 0.14), 1.0);
                g.txt_c(&k, kx0 + kw / 2.0, cy + ty(0.0, 10.5), 10.5, TEXT_DIM, 1.0, kw, true);
                text_r = kx0 - 8.0;
            }
            let tw = (text_r - X0 - 60.0).max(40.0);
            self.check(g, id.label(), 13.5, true, tw);
            self.check(g, slide_sub(id), 11.5, false, tw);
            let name_col = if on { [255, 255, 255] } else { TEXT_FAINT };
            g.txtb(id.label(), X0 + 60.0, cy - 17.0, 13.5, name_col, 1.0, tw);
            let sub = fit(g, slide_sub(id), 11.5, tw);
            g.txt(&sub, X0 + 60.0, cy + 2.0, 11.5, TEXT_FAINT, 1.0, tw + 2.0);
        }
        y + h + 22.0
    }

    fn extensions(&mut self, a: &App, g: &mut Gfx, y: f32) -> f32 {
        if let Some(i) = a.sw.panel.filter(|&i| i < a.ext.list.len()) {
            return self.ext_panel(a, g, y, i);
        }
        Self::header(g, tr("Installed"), y);
        let mut y = y + 22.0;
        let now = Instant::now();
        if a.ext.list.is_empty() {
            let h = 92.0;
            Self::card(g, y, h);
            let w = X1 - X0 - 40.0;
            let mut ly = y + 16.0;
            self.check(g, tr("No extensions found"), 13.5, true, w);
            g.txtb(tr("No extensions found"), X0 + 20.0, ly, 13.5, [255, 255, 255], 1.0, w);
            ly += 24.0;
            for line in wrap_lines(g, tr("An extension is a small program that adds a slide or alerts to the notch. Its installer puts a folder in the extensions folder."), 11.5, w) {
                g.txt(&line, X0 + 20.0, ly, 11.5, TEXT_FAINT, 1.0, w + 2.0);
                ly += 16.0;
            }
            y += h + 12.0;
        }
        for (i, e) in a.ext.list.iter().enumerate() {
            let enabled = a.cfg.ext_on.iter().any(|x| x == e.id());
            let valid = e.manifest.is_ok();
            let h = if valid { 104.0 } else { 62.0 };
            Self::card(g, y, h);
            let cy = y + 31.0;
            let ib = rrect(X0 + 14.0, cy - 16.0, 32.0, 32.0, 9.0);
            g.fill(&ib, &rgb(WELL, 1.0));
            g.stroke(&ib, &paint(255, 255, 255, 0.07), 1.0);
            g.slide_icon(SlideId::Ext(i as u8), X0 + 30.0, cy, 14.0, &if enabled { g.sacc(1.0) } else { rgb(TEXT_FAINT, 1.0) });
            let right = X1 - 16.0;
            let tw = (right - 46.0 - 14.0 - X0 - 60.0 - if valid { 0.0 } else { 30.0 }).max(40.0);
            let name = match &e.manifest {
                Ok(m) if m.version.is_empty() => m.name.clone(),
                Ok(m) => format!("{}  {}", m.name, m.version),
                Err(_) => e.folder.clone(),
            };
            let status = crate::extapp::status_text(e, enabled, now);
            self.check(g, &name, 13.5, true, tw);
            self.check(g, &status, 11.5, false, tw);
            // a manifest that is no good can only be taken out
            let sure = a.sw.remove_confirm.is_some_and(|(n, t)| n == i && t.elapsed().as_secs_f32() < 3.0);
            let remove = if sure { tr("Sure?") } else { tr("Remove") };
            if !valid {
                self.small_button(g, X1 - 16.0, cy - 13.0, remove, Hit::ExtRemove(i), true);
            }
            g.txtb(&name, X0 + 60.0, cy - 17.0, 13.5, if enabled || !valid { [255, 255, 255] } else { TEXT_FAINT }, 1.0, tw);
            let status = fit(g, &status, 11.5, tw);
            g.txt(&status, X0 + 60.0, cy + 2.0, 11.5, TEXT_FAINT, 1.0, tw + 2.0);
            if valid {
                let kx = right - 46.0;
                g.toggle(kx, cy - 13.0, 46.0, 26.0, a.sw.knob(Knob::Ext(i as u8), enabled), 0.0, 1.0);
                self.hit(kx - 4.0, cy - 17.0, 54.0, 34.0, Hit::ExtOn(i));
                // second row: Toasts, Settings, Restart, Open folder
                let by = y + 66.0;
                g.rect(X0 + 16.0, by - 8.0, X1 - X0 - 32.0, 1.0, &paint(255, 255, 255, 0.05));
                let mut rx = right;
                rx = self.small_button(g, rx, by, tr("Open folder"), Hit::ExtFolder(i), true);
                rx = self.small_button(g, rx - 6.0, by, tr("Restart"), Hit::ExtRestart(i), enabled);
                rx = self.small_button(g, rx - 6.0, by, tr("Settings"), Hit::ExtConfig(i), e.connected);
                rx = self.small_button(g, rx - 6.0, by, remove, Hit::ExtRemove(i), true);
                // Toasts: a label and a small switch
                let muted = a.cfg.ext_mute.iter().any(|x| x == e.id());
                let sx = rx - 6.0 - 38.0;
                g.toggle(sx, by + 2.0, 38.0, 22.0, a.sw.knob(Knob::ExtMute(i as u8), !muted), 0.0, 1.0);
                self.hit(sx - 4.0, by - 2.0, 46.0, 30.0, Hit::ExtMute(i));
                let lw = g.width(tr("Toasts"), 12.0);
                self.check(g, tr("Toasts"), 12.0, false, lw + 4.0);
                g.txt(tr("Toasts"), sx - 8.0 - lw, by + ty(26.0, 12.0), 12.0, TEXT_DIM, 1.0, lw + 4.0);
            }
            y += h + 12.0;
        }
        // the folder everything is read from
        let by = y + 2.0;
        let rx = self.small_button(g, X1, by, tr("Open extensions folder"), Hit::ExtRoot, true);
        self.small_button(g, rx - 6.0, by, tr("Rescan"), Hit::ExtScan, true);
        by + 26.0 + 22.0
    }

    /// A button whose left edge is at `x`; returns its right edge.
    fn left_button(&mut self, g: &mut Gfx, x: f32, y: f32, label: &str, hit: Hit) -> f32 {
        let w = (g.width_ex(label, 12.0, true) + 24.0).max(56.0);
        self.small_button(g, x + w, y, label, hit, true);
        x + w
    }

    /// The Settings button of an extension in panel mode: its pages, drawn here in the style of this window. The extension
    /// sends the same `slide` and `detail` pages as for the notch; clicks go back to it as the same messages.
    fn ext_panel(&mut self, a: &App, g: &mut Gfx, y: f32, i: usize) -> f32 {
        let Some(e) = a.ext.list.get(i) else { return y };
        let name = e.manifest.as_ref().map_or(e.folder.clone(), |m| m.name.clone());
        let mut x = self.left_button(g, X0, y, tr("Extensions"), Hit::PanelBack);
        if e.connected && e.detail.is_some() {
            x = self.left_button(g, x + 6.0, y, tr("Back"), Hit::PanelDetailBack);
        }
        self.check(g, &name, 15.0, true, X1 - x - 20.0);
        g.txtb(&name, x + 14.0, y + ty(26.0, 15.0), 15.0, [255, 255, 255], 1.0, X1 - x - 20.0);
        let y = y + 46.0;
        if !e.connected {
            return self.panel_message(g, y, &match (&e.problem, e.child_running()) {
                (Some(p), _) => fill(tr("Not running: {}"), &[p]),
                (None, true) => tr("Starting\u{2026}").to_string(),
                (None, false) => tr("Not connected").to_string(),
            }, None);
        }
        if let Some(d) = &e.detail {
            return self.panel_detail(g, y, d, a.sw.field.as_ref());
        }
        let Some(s) = &e.slide else { return self.panel_message(g, y, tr("Loading\u{2026}"), None) };
        self.panel_slide(g, y, s, e.view.tab_sel.as_deref(), &e.view)
    }

    /// One line of text (and a button under it), in a card.
    fn panel_message(&mut self, g: &mut Gfx, y: f32, line: &str, button: Option<(&str, Hit)>) -> f32 {
        let h = if button.is_some() { 110.0 } else { 70.0 };
        Self::card(g, y, h);
        let w = X1 - X0 - 40.0;
        let shown = fit(g, line, 13.0, w);
        self.check(g, line, 13.0, false, w);
        g.txt_c(&shown, (X0 + X1) / 2.0, y + 24.0, 13.0, TEXT_DIM, 1.0, w, false);
        if let Some((label, hit)) = button {
            let bw = (g.width_ex(label, 12.0, true) + 28.0).max(80.0);
            self.small_button(g, (X0 + X1) / 2.0 + bw / 2.0, y + 58.0, label, hit, true);
        }
        y + h + 22.0
    }

    /// The page's hero: the picture (the middle of it, as a square), a title, a line under it and a small line.
    fn panel_hero(&mut self, g: &mut Gfx, y: f32, h: &Hero, view: &crate::extslide::ExtView) -> f32 {
        const SIDE: f32 = 96.0;
        let card_h = SIDE + 20.0;
        Self::card(g, y, card_h);
        let (px, py) = (X0 + 14.0, y + 10.0);
        // decoded once for as long as the bytes are the same
        let key = h.image.as_ref().map(|b| {
            use std::hash::{Hash, Hasher};
            let mut s = std::collections::hash_map::DefaultHasher::new();
            b.hash(&mut s);
            s.finish()
        });
        {
            let mut c = view.hero_pm.borrow_mut();
            match (key, c.as_ref().map(|(k, _)| *k)) {
                (None, _) => *c = None,
                (Some(k), have) if have != Some(k) => *c = h.image.as_deref().and_then(|b| hero_pixmap(b, 192)).map(|p| (k, p)),
                _ => {}
            }
        }
        match view.hero_pm.borrow().as_ref() {
            Some((_, pm)) => draw_picture(g, pm, px, py, SIDE, 10.0),
            None => {
                // no picture: a tile with the extension's own mark
                g.fill(&rrect(px, py, SIDE, SIDE, 10.0), &rgb(WELL, 1.0));
                g.stroke(&rrect(px, py, SIDE, SIDE, 10.0), &paint(255, 255, 255, 0.07), 1.0);
                g.slide_icon(SlideId::Ext(0), px + SIDE / 2.0, py + SIDE / 2.0, 30.0, &rgb(TEXT_FAINT, 1.0));
            }
        }
        let tx = px + SIDE + 18.0;
        let tw = X1 - 18.0 - tx;
        for (text, size, bold, dy, col) in [(&h.title, 16.0, true, 8.0, [255, 255, 255]), (&h.sub, 12.5, false, 34.0, TEXT_DIM), (&h.text, 11.5, false, 58.0, TEXT_FAINT)] {
            if text.is_empty() {
                continue;
            }
            let shown = fit(g, text, size, tw);
            self.check(g, text, size, bold, tw);
            if bold {
                g.txtb(&shown, tx, py + dy, size, col, 1.0, tw + 2.0);
            } else {
                g.txt(&shown, tx, py + dy, size, col, 1.0, tw + 2.0);
            }
        }
        y + card_h + 14.0
    }

    fn panel_slide(&mut self, g: &mut Gfx, y: f32, s: &SlideMsg, tab_sel: Option<&str>, view: &crate::extslide::ExtView) -> f32 {
        use crate::extproto::State;
        let mut y = y;
        // actions and the stepper, on the right
        let mut rx = X1;
        for (n, b) in s.actions.iter().enumerate().rev() {
            rx = self.small_button(g, rx, y - 46.0, &b.label, Hit::PanelAct(n), true) - 6.0;
        }
        if let Some(st) = &s.stepper {
            let lw = g.width_ex(&st.label, 12.0, true);
            let next = self.small_button(g, rx, y - 46.0, "\u{203a}", Hit::PanelStep(1), st.next);
            g.txt_c(&st.label, next - 8.0 - lw / 2.0, y - 46.0 + ty(26.0, 12.0), 12.0, [235, 235, 235], 1.0, lw + 4.0, true);
            self.small_button(g, next - 8.0 - lw - 6.0, y - 46.0, "\u{2039}", Hit::PanelStep(-1), st.prev);
        }
        if let (Some(h), State::Ready) = (&s.hero, s.state) {
            y = self.panel_hero(g, y, h, view);
        }
        match s.state {
            State::Loading => return self.panel_message(g, y, tr("Loading\u{2026}"), None),
            State::Empty => return self.panel_message(g, y, if s.message.is_empty() { tr("Nothing here") } else { &s.message }, None),
            State::Error => return self.panel_message(g, y, &s.message, Some((tr("Retry"), Hit::PanelRetry))),
            State::Setup => {
                let label = if s.setup_label.is_empty() { tr("Open settings") } else { &s.setup_label };
                return self.panel_message(g, y, &s.message, Some((label, Hit::PanelSetup)));
            }
            State::Ready => {}
        }
        if !s.tabs.is_empty() {
            let sel = tab_sel.unwrap_or(&s.tab);
            let mut x = X0;
            for (n, t) in s.tabs.iter().enumerate() {
                let label = match t.count {
                    Some(c) => format!("{} {}", t.label, c),
                    None => t.label.clone(),
                };
                let w = (g.width_ex(&label, 12.0, true) + 24.0).max(48.0);
                if x + w > X1 {
                    break;
                }
                let on = t.id == sel;
                let hit = Hit::PanelTab(n);
                let hot = self.is_hover(hit);
                if on {
                    let c = g.acc.deep;
                    g.pill(x, y, w, 26.0, c, 0.9, 1.0);
                } else {
                    g.pill(x, y, w, 26.0, [255, 255, 255], if hot { 0.2 } else { 0.1 }, 1.0);
                }
                self.check(g, &label, 12.0, true, w - 8.0);
                g.txt_c(&label, x + w / 2.0, y + ty(26.0, 12.0), 12.0, if on { [255, 255, 255] } else { [215, 215, 222] }, 1.0, w - 4.0, true);
                self.hit(x, y, w, 26.0, hit);
                x += w + 6.0;
            }
            y += 38.0;
        }
        let mut flat = 0usize;
        for sec in &s.sections {
            if !sec.title.is_empty() {
                Self::header(g, &sec.title, y);
                y += 22.0;
            }
            if sec.rows.is_empty() {
                continue;
            }
            let h = sec.rows.len() as f32 * PANEL_ROW_H + 8.0;
            Self::card(g, y, h);
            for (k, r) in sec.rows.iter().enumerate() {
                let ry = y + 4.0 + k as f32 * PANEL_ROW_H;
                let cy = ry + PANEL_ROW_H / 2.0;
                let hit = Hit::PanelRow(flat);
                flat += 1;
                if k > 0 {
                    g.rect(X0 + 16.0, ry, X1 - X0 - 32.0, 1.0, &paint(255, 255, 255, 0.05));
                }
                if self.is_hover(hit) {
                    g.fill(&rrect(X0 + 6.0, ry + 2.0, X1 - X0 - 12.0, PANEL_ROW_H - 4.0, 8.0), &paint(255, 255, 255, 0.06));
                }
                let mut tx = X0 + 18.0;
                if r.mark {
                    let c = g.acc.bright;
                    g.circle(tx + 3.0, cy, 3.5, &paint(c[0], c[1], c[2], 1.0));
                    tx += 14.0;
                }
                let dot = if r.tone != Tone::None && !r.sub.is_empty() { 12.0 } else { 0.0 };
                let sub_w = if r.sub.is_empty() { 0.0 } else { g.width(&r.sub, 11.5) + 8.0 + dot };
                if sub_w > 0.0 {
                    let col = match r.tone {
                        Tone::Good => [100, 215, 140],
                        Tone::Warn => [255, 195, 80],
                        _ => TEXT_FAINT,
                    };
                    if dot > 0.0 {
                        g.circle(X1 - 18.0 - sub_w + 8.0 + 3.5, cy, 3.5, &paint(col[0], col[1], col[2], 1.0));
                    }
                    g.txt(&r.sub, X1 - 18.0 - sub_w + 8.0 + dot, cy - 7.0, 11.5, col, 1.0, sub_w);
                }
                let tw = X1 - 18.0 - sub_w - tx - 8.0;
                self.check(g, &r.text, 13.5, true, tw);
                g.txtb(&r.text, tx, cy - 8.0, 13.5, [255, 255, 255], 1.0, tw);
                self.hit(X0 + 6.0, ry, X1 - X0 - 12.0, PANEL_ROW_H, hit);
            }
            y += h + 14.0;
        }
        if !s.foot.is_empty() {
            self.check(g, &s.foot, 11.5, false, X1 - X0 - 8.0);
            g.txt(&s.foot, X0 + 4.0, y, 11.5, TEXT_FAINT, 1.0, X1 - X0 - 8.0);
            y += 24.0;
        }
        y + 8.0
    }

    fn panel_detail(&mut self, g: &mut Gfx, y: f32, d: &DetailMsg, field: Option<&FieldState>) -> f32 {
        use crate::extproto::State;
        match d.state {
            State::Loading => return self.panel_message(g, y, tr("Loading\u{2026}"), None),
            State::Empty | State::Setup => return self.panel_message(g, y, &d.message, None),
            State::Error => return self.panel_message(g, y, &d.message, Some((tr("Retry"), Hit::PanelRetry))),
            State::Ready => {}
        }
        // text on the left, the matrix (a QR code) on the right when there is one
        let qr = d.matrix.as_ref().map(|m| {
            // large enough for a phone camera: about 250 px on a side, in whole modules (a version 9 code gets 4 px per module)
            let px = (260 / (m.size + 8)).max(2) as f32;
            (m, px, (m.size + 8) as f32 * px)
        });
        let text_w = X1 - X0 - 40.0 - qr.map_or(0.0, |q| q.2 + 20.0);
        let mut lines: Vec<(String, f32, [u8; 3], bool, f32)> = Vec::new();
        if !d.title.is_empty() {
            let t = fit(g, &d.title, 15.0, text_w);
            lines.push((t, 15.0, [255, 255, 255], true, 0.0));
        }
        if !d.meta.is_empty() {
            let joined = d.meta.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("   \u{b7}   ");
            lines.extend(wrap_lines(g, &joined, 12.0, text_w).into_iter().enumerate().map(|(n, l)| (l, 12.0, TEXT_DIM, false, if n == 0 { 6.0 } else { 0.0 })));
        }
        for (n, l) in wrap_lines(g, &d.body, 12.5, text_w).into_iter().enumerate() {
            lines.push((l, 12.5, [235, 235, 238], false, if n == 0 { 10.0 } else { 0.0 }));
        }
        for n in &d.notes {
            let head = if n.when.is_empty() { n.who.clone() } else { format!("{} \u{b7} {}", n.who, n.when) };
            lines.push((head, 11.0, TEXT_FAINT, false, 10.0));
            lines.extend(wrap_lines(g, &n.text, 12.0, text_w).into_iter().map(|l| (l, 12.0, [220, 220, 226], false, 0.0)));
        }
        let text_h: f32 = 20.0 + lines.iter().map(|l| 17.0 + l.4).sum::<f32>();
        let buttons_h = if d.buttons.is_empty() { 0.0 } else { 46.0 };
        let input_h = if d.input.is_some() { 64.0 } else { 0.0 };
        let h = (text_h + input_h + buttons_h).max(qr.map_or(0.0, |q| q.2 + 36.0 + buttons_h)).max(80.0);
        Self::card(g, y, h);
        let mut ly = y + 18.0;
        for (t, size, col, bold, gap) in &lines {
            ly += gap;
            self.check(g, t, *size, *bold, text_w + 2.0);
            if *bold {
                g.txtb(t, X0 + 20.0, ly, *size, *col, 1.0, text_w + 2.0);
            } else {
                g.txt(t, X0 + 20.0, ly, *size, *col, 1.0, text_w + 2.0);
            }
            ly += 17.0;
        }
        if let (Some(inp), Some(f)) = (&d.input, field) {
            let save = if inp.button.is_empty() { tr("Save") } else { inp.button.as_str() };
            let save_w = (g.width_ex(save, 12.0, true) + 24.0).max(56.0);
            let (bx, by) = (X0 + 20.0, ly + 6.0 + 16.0);
            let bw = text_w - save_w - 10.0;
            if !inp.label.is_empty() {
                self.check(g, &inp.label, 11.5, false, text_w);
                g.txt(&inp.label, bx, ly + 6.0, 11.5, TEXT_FAINT, 1.0, text_w);
            }
            let frame = rrect(bx, by, bw, 30.0, 8.0);
            g.fill(&frame, &paint(255, 255, 255, if f.focused { 0.12 } else { 0.07 }));
            let edge = if f.focused { let m = g.acc.main; paint(m[0], m[1], m[2], 0.9) } else { paint(255, 255, 255, 0.14) };
            g.stroke(&frame, &edge, 1.0);
            // the text slides left when the caret would be outside the box
            let (inner, before) = (bw - 20.0, g.width(&f.tf.before_cursor(), 13.0));
            let off = (before - inner + 2.0).max(0.0);
            g.txt_clip(&f.tf.text(), bx + 10.0 - off, by + ty(30.0, 13.0), 13.0, [245, 245, 248], 1.0, bx + 8.0, bx + bw - 8.0, 0.0);
            if f.focused && (f.since.elapsed().as_millis() / 500) % 2 == 0 {
                g.rect(bx + 10.0 - off + before, by + 6.0, 1.5, 18.0, &paint(255, 255, 255, 0.95));
            }
            self.hit(bx, by, bw, 30.0, Hit::PanelInput);
            self.small_button(g, bx + bw + 10.0 + save_w, by + 2.0, save, Hit::PanelInputSave, true);
        }
        if let Some((m, px, side)) = qr {
            let (qx, qy) = (X1 - 20.0 - side, y + 18.0);
            g.fill(&rrect(qx, qy, side, side, 6.0), &paint(255, 255, 255, 1.0));
            for r in 0..m.size {
                let mut c = 0;
                while c < m.size {
                    if !m.at(c, r) {
                        c += 1;
                        continue;
                    }
                    let start = c;
                    while c < m.size && m.at(c, r) {
                        c += 1;
                    }
                    g.rect(qx + (4 + start) as f32 * px, qy + (4 + r) as f32 * px, (c - start) as f32 * px, px, &paint(0, 0, 0, 1.0));
                }
            }
        }
        if !d.buttons.is_empty() {
            let by = y + h - 38.0;
            let mut rx = X1 - 16.0;
            for (n, b) in d.buttons.iter().enumerate().rev() {
                rx = self.small_button(g, rx, by, &b.label, Hit::PanelButton(n), true) - 6.0;
            }
        }
        let mut y = y + h + 14.0;
        if !d.foot.is_empty() {
            g.txt(&d.foot, X0 + 4.0, y, 11.5, TEXT_FAINT, 1.0, X1 - X0 - 8.0);
            y += 24.0;
        }
        y + 8.0
    }

    /// A small pill button whose right edge is at `rx`; returns its left edge. A disabled one is drawn dim and does not
    /// answer clicks.
    fn small_button(&mut self, g: &mut Gfx, rx: f32, y: f32, label: &str, hit: Hit, enabled: bool) -> f32 {
        let w = (g.width_ex(label, 12.0, true) + 24.0).max(56.0);
        let x = rx - w;
        self.check(g, label, 12.0, true, w - 8.0);
        let hot = enabled && self.is_hover(hit);
        g.pill(x, y, w, 26.0, [255, 255, 255], if !enabled { 0.05 } else if hot { 0.22 } else { 0.12 }, 1.0);
        g.txt_c(label, x + w / 2.0, y + ty(26.0, 12.0), 12.0, if enabled { [255, 255, 255] } else { TEXT_FAINT }, 1.0, w - 4.0, true);
        if enabled {
            self.hit(x, y, w, 26.0, hit);
        }
        x
    }

    fn about(&mut self, a: &App, g: &mut Gfx, y: f32) -> f32 {
        let data = wrap_lines(g, tr("All your data stays on this PC."), 12.5, 380.0);
        let credits: Vec<&str> = tr("Based on Poch by Commonly Works\n(github.com/klawcodes/poch), MIT License.\nLyrics: LRCLIB (lrclib.net) and lrc.red.\nRomaji: Lindera (IPADIC) and WanaKana.\nFont: Instrument Sans.").split('\n').collect();
        let h = 142.0 + 18.0 * data.len() as f32 + 12.0 + 17.0 * credits.len() as f32 + 12.0 + 18.0 + 22.0;
        Self::card(g, y, h);
        let cx = (X0 + X1) / 2.0;
        // the app's own icon (icon.ico), as in the exe and the tray
        a.icon_at(g, cx - 28.0, y + 22.0, 56.0, 14.0, 1.0);
        g.txt_c("Hesewake Notch", cx, y + 90.0, 18.0, [255, 255, 255], 1.0, X1 - X0, true);
        let ver = i18n::fill(tr("Version {}"), &[&env!("CARGO_PKG_VERSION")]);
        g.txt_c(&ver, cx, y + 114.0, 12.5, TEXT_FAINT, 1.0, X1 - X0, false);
        let mut ly = y + 142.0;
        for line in data {
            g.txt_c(&line, cx, ly, 12.5, TEXT_DIM, 1.0, 400.0, false);
            ly += 18.0;
        }
        ly += 12.0;
        // credits the licenses ask for: Poch (MIT) and the lyrics sources (lrc.red wants it in the About)
        for line in credits {
            g.txt_c(line, cx, ly, 12.0, TEXT_FAINT, 1.0, X1 - X0 - 20.0, false);
            ly += 17.0;
        }
        g.txt_c("github.com/dirgaastya/hesewake-notch", cx, ly + 12.0, 12.0, g.acc.bright, 1.0, 400.0, false);
        y + h + 22.0
    }
}

impl Draw {
    /// About > Updates: the running version or the release found, and a button to check, install or download.
    fn updates(&mut self, a: &App, g: &mut Gfx, y: f32) -> f32 {
        Self::header(g, tr("Updates"), y);
        let y = y + 22.0;
        Self::card(g, y, ROW_H);
        let (label, sub) = if let Some(p) = a.update_downloading() {
            (tr("Downloading update").to_string(), i18n::fill(tr("Please wait \u{b7} {}%"), &[&p]))
        } else if let Some(r) = &a.update {
            (i18n::fill(tr("Version {} is available"), &[&r.version]), tr("Release notes and installer").to_string())
        } else {
            let sub = if a.update_busy { tr("Checking\u{2026}") } else { tr("Checks GitHub for a newer release") };
            (i18n::fill(tr("Version {}"), &[&update::CURRENT]), sub.to_string())
        };
        let button = a.sw_update_action().map(|act| match act {
            UpdateAction::Install(_) => tr("Install"),
            UpdateAction::Open(_) => tr("Download"),
            UpdateAction::Check => tr("Check for updates"),
        });
        let right = X1 - 16.0;
        let cy = y + ROW_H / 2.0;
        let mut text_r = right;
        if let Some(b) = button {
            let bw = g.width_ex(b, 12.5, true).ceil() + 30.0;
            let bx = right - bw;
            let hot = self.is_hover(Hit::Update);
            let (fill, text) = (g.acc.bright, g.acc.pale);
            g.fill(&rrect(bx, cy - 15.0, bw, 30.0, 15.0), &rgb(fill, if hot { 0.32 } else { 0.2 }));
            g.txt_c(b, bx + bw / 2.0, cy + ty(0.0, 12.5), 12.5, text, 1.0, bw, true);
            self.hit(bx, cy - 15.0, bw, 30.0, Hit::Update);
            text_r = bx;
        }
        let tw = (text_r - X0 - 32.0).max(40.0);
        self.check(g, &label, 14.0, true, tw);
        self.check(g, &sub, 12.0, false, tw);
        g.txtb(&label, X0 + 16.0, y + 11.0, 14.0, [255, 255, 255], 1.0, tw);
        g.txt(&sub, X0 + 16.0, y + 31.0, 12.0, TEXT_FAINT, 1.0, tw + 2.0);
        y + ROW_H + 22.0
    }
}

/// `s` as is when it fits in `maxw`, otherwise cut with an ellipsis.
fn fit(g: &mut Gfx, s: &str, size: f32, maxw: f32) -> String {
    if g.width(s, size) <= maxw {
        s.to_string()
    } else {
        ellipsize(g, s, size, maxw)
    }
}

/// One line about each slide, under its name in the slide list.
fn slide_sub(id: SlideId) -> &'static str {
    match id {
        SlideId::Music => tr("Now playing, clock and lyrics"),
        SlideId::Water => tr("Daily water tracker"),
        SlideId::Todo => tr("Checklist"),
        SlideId::Ledge => tr("Shelf for 5 files"),
        SlideId::Scrap => tr("Mini notepad"),
        SlideId::Notifications => tr("Windows notifications"),
        SlideId::System => tr("CPU, memory, network"),
        SlideId::Timer => tr("Stopwatch, countdown, Pomodoro"),
        SlideId::Clipboard => tr("Last 10 copied texts"),
        SlideId::Agenda => tr("What is left today"),
        SlideId::Prayer => tr("Today's prayer times"),
        SlideId::Ext(_) => "",
    }
}

impl App {
    /// Make the text box match the page the panel shows: made when a page with an `input` appears, dropped when it goes.
    pub(crate) fn sw_sync_field(&mut self) {
        let want = self.sw.panel.and_then(|i| {
            let e = self.ext.list.get(i).filter(|e| e.connected)?;
            let d = e.detail.as_ref()?;
            Some((i, d.for_id.clone(), d.input.as_ref()?.clone()))
        });
        let Some((i, page, inp)) = want else {
            self.sw.field = None;
            return;
        };
        let key = (i, page, inp.id.clone());
        match self.sw.field.as_mut() {
            Some(f) if f.key == key => {
                // the extension says the text is something else now (it took the new name): follow, unless it is being typed
                if !f.focused && f.original != inp.value {
                    f.original = inp.value.clone();
                    f.tf = TextField::new(&inp.value, inp.max);
                }
            }
            _ => self.sw.field = Some(FieldState { key, original: inp.value.clone(), max: inp.max, tf: TextField::new(&inp.value, inp.max), focused: false, since: Instant::now(), high: None }),
        }
    }

    /// The box gets the keyboard (a click on it) or lets go of it (a click elsewhere). What was typed stays.
    pub(crate) fn sw_field_focus(&mut self, on: bool) {
        if let Some(f) = self.sw.field.as_mut() {
            if on && !f.focused {
                f.tf.end();
            }
            f.focused = on;
            f.since = Instant::now();
        }
    }

    /// A character typed (WM_CHAR, a UTF-16 unit). True if the box took it.
    pub(crate) fn sw_field_char(&mut self, unit: u16) -> bool {
        let Some(f) = self.sw.field.as_mut().filter(|f| f.focused) else { return false };
        let c = match unit {
            0xD800..=0xDBFF => {
                f.high = Some(unit);
                return true;
            }
            0xDC00..=0xDFFF => f.high.take().and_then(|h| char::decode_utf16([h, unit]).next()?.ok()),
            u => {
                f.high = None;
                char::from_u32(u as u32)
            }
        };
        if c.is_some_and(|c| f.tf.insert(c)) {
            f.since = Instant::now();
        }
        true
    }

    /// A key pressed (WM_KEYDOWN). True if the box took it.
    pub(crate) fn sw_field_key(&mut self, vk: u16, ctrl: bool) -> bool {
        let hwnd = self.sw.hwnd;
        let Some(f) = self.sw.field.as_mut().filter(|f| f.focused) else { return false };
        f.since = Instant::now();
        let mut commit = false;
        match vk {
            0x08 => f.tf.backspace(),
            0x2E => f.tf.delete(),
            0x25 => f.tf.left(),
            0x27 => f.tf.right(),
            0x24 => f.tf.home(),
            0x23 => f.tf.end(),
            0x0D => commit = true,
            // Esc: give up, back to what the extension says
            0x1B => {
                f.tf = TextField::new(&f.original, f.max);
                f.focused = false;
            }
            0x56 if ctrl => {
                if let Some(t) = calendar::clipboard_text(hwnd) {
                    f.tf.insert_str(&t);
                }
            }
            _ => {}
        }
        if commit {
            self.sw_field_commit();
        }
        true
    }

    /// Enter or the button: what is typed goes to the extension, which answers with the page it makes of it.
    pub(crate) fn sw_field_commit(&mut self) {
        let Some(f) = self.sw.field.as_mut() else { return };
        f.focused = false;
        let (i, id, text) = (f.key.0, f.key.2.clone(), f.tf.text());
        if let Some(ext) = self.ext_id(i) {
            self.ext_send_to(&ext, crate::extproto::input(&id, &text));
        }
    }
}

/// Tests look at the box without reaching into it.
#[cfg(test)]
impl FieldState {
    pub(crate) fn tf_text(&self) -> String {
        self.tf.text()
    }

    pub(crate) fn is_focused(&self) -> bool {
        self.focused
    }
}


/// A square of `side` pixels from the middle of a picture (PNG or JPEG), for drawing. None if it is not one we can read.
fn hero_pixmap(bytes: &[u8], side: u32) -> Option<Pixmap> {
    let img = decode_limited(bytes)?.resize_to_fill(side, side, image::imageops::FilterType::Triangle);
    pixmap_from_rgba(&img.to_rgba8().into_raw(), side)
}

/// `pm` as a rounded square of `size` pixels at (x, y).
fn draw_picture(g: &mut Gfx, pm: &Pixmap, x: f32, y: f32, size: f32, r: f32) {
    let k = size / pm.width() as f32;
    let mut p = Paint::default();
    p.shader = Pattern::new(pm.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, 1.0, Transform::from_row(k, 0.0, 0.0, k, x, y));
    p.anti_alias = true;
    g.fill(&rrect(x, y, size, size, r), &p);
}

//! Slide Settings: empat tab (Slides, General, Alerts, Widgets); baris yang lebih dari tiga baris di-scroll dengan roda. Toggle berpegas, urutan slide bisa digeser dengan tombol
//! naik/turun (dua sel bertukar tempat dengan animasi), pengaturan tersimpan langsung ke `settings.txt`.

use super::*;
use crate::settings::{LEAD_MAX, LEAD_MIN, LEAD_STEP};
use crate::slides::SlideId;

const TAB_Y: f32 = 8.0;
const TAB_H: f32 = 24.0;
/// Settings tab names, in order.
fn tab_labels() -> [&'static str; 4] {
    [tr("Slides"), tr("General"), tr("Alerts"), tr("Widgets")]
}
/// Padding inside a tab chip, left + right.
const TAB_PAD: f32 = 14.0;
const TAB_GAP: f32 = 6.0;

const GRID_Y0: f32 = 42.0;
/// Two columns of cells fill LX..RX exactly (2 x 198 + 8 = 404), like the other slides.
pub(crate) const CELL_W: f32 = 198.0;
const CELL_H: f32 = 34.0;
const GRID_GAP_X: f32 = 8.0;
const GRID_GAP_Y: f32 = 4.0;
const GREY: [u8; 3] = TEXT_DIM;
const MAX_ROWS: usize = 16;

pub(crate) struct SettingsUi {
    pub(crate) tab: u8,
    tab_t0: Instant,
    tab_dir: f32,
    knob: [f32; MAX_ROWS],
    kvel: [f32; MAX_ROWS],
    entered: bool,
    enter_t: Instant,
    /// Dua indeks slide yang baru bertukar tempat (animasi geser).
    swap: Option<(usize, usize, Instant)>,
    /// Status autostart di-cache (registry dibaca saat tab Features dibuka, bukan tiap frame).
    autostart: bool,
    /// Tombol Delete kamus romaji ditekan sekali: tekan lagi dalam `CONFIRM_SECS` untuk benar-benar menghapus.
    dict_confirm: Option<Instant>,
    /// Settings > Slides: rows scrolled out of view at the top.
    pub(crate) slides_scroll: usize,
    /// Settings > General / Alerts / Widgets: rows scrolled out of view at the top.
    pub(crate) rows_scroll: usize,
}

impl SettingsUi {
    pub(crate) fn new() -> SettingsUi {
        let old = Instant::now() - Duration::from_secs(10);
        SettingsUi { tab: 0, tab_t0: old, tab_dir: 1.0, knob: [0.0; MAX_ROWS], kvel: [0.0; MAX_ROWS], entered: false, enter_t: old, swap: None, autostart: false, dict_confirm: None, slides_scroll: 0, rows_scroll: 0 }
    }

    fn dict_confirming(&self) -> bool {
        self.dict_confirm.map_or(false, |t| t.elapsed().as_secs_f32() < CONFIRM_SECS)
    }
}

const CONFIRM_SECS: f32 = 3.0;

fn ss(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Tab Slides: 3 kolom x 3 baris (sel sempit agar 7+ slide muat).
pub(crate) const SC_W: f32 = 117.0;
const SC_GAP: f32 = 6.5;

/// The Settings tabs: (x, width) of each, sized to their names (which depend on the language) and right-aligned.
pub(crate) fn tab_rects(g: &mut Gfx) -> Vec<(f32, f32)> {
    let ws: Vec<f32> = tab_labels().iter().map(|l| g.width_ex(l, 12.0, true).ceil() + TAB_PAD).collect();
    let mut x = RX - ws.iter().sum::<f32>() - TAB_GAP * (ws.len() - 1) as f32;
    ws.into_iter()
        .map(|w| {
            let r = (x, w);
            x += w + TAB_GAP;
            r
        })
        .collect()
}

/// Room for a slide's name in its Settings > Slides cell, left of the up/down chevrons.
pub(crate) const SLIDE_NAME_W: f32 = 48.0;

/// Settings > Slides lists the slides in carousel order, 3 per row; 3 rows show at a time and the rest scroll.
const SC_COLS: usize = 3;
const SC_ROWS: usize = 3;

fn slide_rows(n: usize) -> usize {
    n.div_ceil(SC_COLS)
}

/// Where slide cell `i` is with `scroll` rows scrolled away (may be outside the grid).
fn slide_cell_pos(i: usize, scroll: usize) -> (f32, f32) {
    let (col, row) = (i % SC_COLS, i / SC_COLS);
    (LX + col as f32 * (SC_W + SC_GAP), GRID_Y0 + (row as f32 - scroll as f32) * (CELL_H + GRID_GAP_Y))
}

/// Slide cell `i`'s position if its row is in view.
pub(crate) fn slide_cells_visible(i: usize, scroll: usize) -> Option<(f32, f32)> {
    let row = i / SC_COLS;
    (row >= scroll && row < scroll + SC_ROWS).then(|| slide_cell_pos(i, scroll))
}

impl App {
    /// Wheel over Settings > Slides. True if it scrolled.
    pub(crate) fn settings_slides_wheel(&mut self, down: bool) -> bool {
        let max = slide_rows(self.cfg.slides.len()).saturating_sub(SC_ROWS);
        let old = self.sui.slides_scroll;
        self.sui.slides_scroll = if down { (old + 1).min(max) } else { old.saturating_sub(1) };
        self.sui.slides_scroll != old
    }

    /// Scroll so slide cell `i` is in view (after it moved with the arrows).
    fn slides_keep_visible(&mut self, i: usize) {
        let row = i / SC_COLS;
        if row < self.sui.slides_scroll {
            self.sui.slides_scroll = row;
        } else if row >= self.sui.slides_scroll + SC_ROWS {
            self.sui.slides_scroll = row + 1 - SC_ROWS;
        }
    }

    /// Settings > Slides has more rows than fit (the wheel scrolls it).
    pub(crate) fn slides_scrollable(&self) -> bool {
        slide_rows(self.cfg.slides.len()) > SC_ROWS
    }
}

/// Rows tabs hold two cells per row, left to right; three rows show at a time and the rest scroll.
const ROW_COLS: usize = 2;
const ROW_VIS: usize = 3;

fn row_count(n: usize) -> usize {
    n.div_ceil(ROW_COLS)
}

/// Where row `i` is with `scroll` rows scrolled away (may be outside the grid).
fn row_cell_pos(i: usize, scroll: usize) -> (f32, f32) {
    let (col, row) = (i % ROW_COLS, i / ROW_COLS);
    (LX + col as f32 * (CELL_W + GRID_GAP_X), GRID_Y0 + (row as f32 - scroll as f32) * (CELL_H + GRID_GAP_Y))
}

/// Where row `i` is when it is visible with `scroll` rows scrolled away.
pub(crate) fn row_cell_visible(i: usize, scroll: usize) -> Option<(f32, f32)> {
    let row = i / ROW_COLS;
    (row >= scroll && row < scroll + ROW_VIS).then(|| row_cell_pos(i, scroll))
}

/// Cell of row `i` with nothing scrolled (the first six rows).
#[cfg(test)]
pub(crate) fn cell_pos(i: usize) -> (f32, f32) {
    row_cell_pos(i, 0)
}

/// What a settings row changes; clicks are dispatched on this, so rows can move between tabs freely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    Calendar,
    HudVolume,
    HudBrightness,
    LockKeys,
    PrivacyDot,
    UpdateCheck,
    MicMute,
    Downloads,
    Weather,
    City,
    Prayer,
    ImsakTerbit,
    Ramadan,
    Adhan,
    AdhanFile,
    Accent,
    Romaji,
    Dictionary,
}

/// Satu baris pengaturan pada tab General, Alerts, atau Widgets.
pub(crate) enum RowKind {
    Toggle(bool),
    /// Teks nilai + tombol - dan +.
    Stepper(String),
    /// Tombol pil berlabel di kanan sel (mis. unduh kamus romaji).
    Button(String),
}

pub(crate) struct Row {
    pub(crate) id: Rid,
    pub(crate) label: &'static str,
    pub(crate) sub: &'static str,
    pub(crate) kind: RowKind,
}

/// Max width of a cell's label and sub-label: stop before the control on the right so they never overlap it.
pub(crate) fn text_w(kind: &RowKind) -> f32 {
    match kind {
        // the toggle starts at CELL_W - 44
        RowKind::Toggle(_) => CELL_W - 44.0 - 12.0 - 8.0,
        // the - button starts at CELL_W - 91
        RowKind::Stepper(_) => CELL_W - 91.0 - 12.0 - 4.0,
        // the button is 50 px wide, 10 px from the right edge
        RowKind::Button(_) => CELL_W - 60.0 - 12.0 - 4.0,
    }
}

/// Text that fits in `maxw`; anything longer is cut with an ellipsis (a safety net for new strings).
fn fit(g: &mut Gfx, s: &str, size: f32, bold: bool, maxw: f32) -> String {
    if g.width_ex(s, size, bold) <= maxw {
        return s.to_string();
    }
    g.text.as_mut().map_or_else(|| s.to_string(), |t| t.ellipsize(s, size, bold, maxw))
}

/// Centre of a stepper's "+" button, from the cell's left edge.
pub(crate) const STEP_PLUS_X: f32 = CELL_W - 18.0;
/// Narrowest stepper, centre of "-" to centre of "+" (the old fixed size; enough for short values like "Auto").
pub(crate) const STEP_MIN_SPAN: f32 = 64.0;
/// From a button's centre to the value text: the button radius (9 px) plus a 3 px gap. With this, the default
/// city ("Jakarta") still fits the old fixed stepper, so the default look does not change.
const STEP_SIDE: f32 = 12.0;
/// Value text sizes, tried largest first.
const STEP_SIZES: [f32; 3] = [11.0, 10.0, 9.0];

/// Where a stepper's "-" button goes and how its value is drawn.
pub(crate) struct StepperFit {
    /// Centre of the "-" button, from the cell's left edge.
    pub(crate) minus_x: f32,
    pub(crate) size: f32,
    pub(crate) value: String,
}

/// Lay out a stepper so long values (e.g. "Bandar Lampung") are not cut off. The "-" button moves left, but never
/// into the row's own label and sub-label; if that is not enough the value shrinks to 10 / 9 px, and only then is it
/// cut with an ellipsis.
pub(crate) fn stepper_fit(g: &mut Gfx, r: &Row, value: &str) -> StepperFit {
    let label_w = g.width_ex(r.label, 12.0, true).max(g.width(r.sub, 11.0));
    // the "-" button's left edge stays 4 px right of the longer label
    let max_span = (STEP_PLUS_X - (12.0 + label_w + 4.0 + 9.0)).max(STEP_MIN_SPAN);
    for size in STEP_SIZES {
        let span = (g.width_ex(value, size, true) + 2.0 * STEP_SIDE).max(STEP_MIN_SPAN);
        if span <= max_span {
            return StepperFit { minus_x: STEP_PLUS_X - span, size, value: value.to_string() };
        }
    }
    let size = STEP_SIZES[STEP_SIZES.len() - 1];
    StepperFit { minus_x: STEP_PLUS_X - max_span, size, value: fit(g, value, size, true, max_span - 2.0 * STEP_SIDE) }
}

impl App {
    /// Buka slide Settings (dari item menu): ditambahkan ke carousel lalu langsung ditampilkan.
    pub(crate) fn open_settings(&mut self) {
        if !self.settings_open {
            self.settings_prev = Some(self.cur_id());
            self.settings_open = true;
            self.order = slides::visible_order(&self.slide_cfg(), true);
        }
        self.show_id(SlideId::Settings);
        self.peek_until = Some(Instant::now() + Duration::from_secs(8));
    }

    /// Keluarkan Settings dari carousel bila pengguna sudah pindah ke slide lain atau notch menutup. Dipanggil saat
    /// carousel diam. True bila ada perubahan.
    pub(crate) fn settings_autoclose(&mut self) -> bool {
        if !self.settings_open {
            return false;
        }
        let on_settings = self.is_slide(SlideId::Settings);
        let collapsed = !self.expanded() && self.h < 90.0;
        if on_settings && !collapsed {
            return false;
        }
        // tetap di slide yang sedang dilihat; bila notch menutup di Settings, kembali ke slide asal
        let keep = if on_settings { self.settings_prev } else { Some(self.cur_id()) };
        self.settings_open = false;
        self.order = slides::visible_order(&self.slide_cfg(), false);
        let idx = keep.and_then(|id| self.order.iter().position(|&s| s == id)).unwrap_or(0);
        self.slide = idx as i32;
        self.sp = idx as f32;
        self.sv = 0.0;
        true
    }

    /// Terapkan pengaturan cuaca (nyala/mati, kota) ke thread pengambil data.
    fn weather_apply(&self) {
        let (_, lat, lon) = weather::CITIES[self.cfg.weather_city];
        weather::configure(self.cfg.weather_on, lat, lon);
    }

    fn settings_visible(&self) -> bool {
        self.expanded() && self.is_slide(SlideId::Settings) && self.h > 100.0
    }

    /// Daftar baris tab General (1), Alerts (2) atau Widgets (3).
    pub(crate) fn settings_rows(&self, tab: u8) -> Vec<Row> {
        match tab {
            1 => vec![
                Row { id: Rid::Language, label: tr("Language"), sub: tr("Menus and slides"), kind: RowKind::Stepper(i18n::LANGS[self.cfg.lang.min(i18n::LANGS.len() - 1)].to_string()) },
                Row { id: Rid::Accent, label: tr("Accent"), sub: tr("Highlight color"), kind: RowKind::Stepper(accent::label(self.cfg.accent).to_string()) },
                Row { id: Rid::Autostart, label: tr("Autostart"), sub: tr("Start with Windows"), kind: RowKind::Toggle(self.sui.autostart) },
                Row { id: Rid::SlideHotkeys, label: tr("Slide hotkeys"), sub: tr("Ctrl+Alt + slide no."), kind: RowKind::Toggle(self.cfg.slide_hotkeys) },
                Row { id: Rid::Monitor, label: "Monitor", sub: tr("Follow = cursor"), kind: RowKind::Stepper(monitors::mode_label(self.cfg.monitor.min(monitors::mode_count() - 1))) },
                Row { id: Rid::NotchSize, label: tr("Notch size"), sub: "Auto = Windows", kind: RowKind::Stepper(scale::size_label(self.cfg.notch_size)) },
                Row { id: Rid::Minimal, label: tr("Minimal notch"), sub: tr("Thin line while idle"), kind: RowKind::Toggle(self.minimal) },
                Row { id: Rid::ThinMaximized, label: tr("Thin on maximize"), sub: tr("Off: only on fullscreen"), kind: RowKind::Toggle(self.cfg.busy_on_maximized) },
                Row { id: Rid::IdleDisplay, label: tr("Idle display"), sub: tr("Cycle: clock/eyes"), kind: RowKind::Stepper(settings::idle_display_label(self.cfg.idle_display).to_string()) },
            ],
            2 => vec![
                Row { id: Rid::Notifications, label: tr("Notifications"), sub: tr("Windows toasts + bell"), kind: RowKind::Toggle(self.notif_on) },
                Row { id: Rid::ContactPhotos, label: tr("Contact photos"), sub: tr("In notifications"), kind: RowKind::Toggle(self.cfg.notif_avatar) },
                Row { id: Rid::HudVolume, label: tr("Volume HUD"), sub: tr("Keys and mixer"), kind: RowKind::Toggle(self.cfg.hud_volume) },
                Row { id: Rid::HudBrightness, label: tr("Brightness HUD"), sub: tr("Fn keys / slider"), kind: RowKind::Toggle(self.cfg.hud_brightness) },
                Row { id: Rid::LockKeys, label: tr("Lock keys"), sub: "Caps / Num / Scroll", kind: RowKind::Toggle(self.cfg.lock_toast) },
                Row { id: Rid::PrivacyDot, label: tr("Mic / camera dot"), sub: tr("When an app uses them"), kind: RowKind::Toggle(self.cfg.privacy_dot) },
                Row { id: Rid::UpdateCheck, label: tr("Update notice"), sub: tr("When a new version is out"), kind: RowKind::Toggle(self.cfg.update_check) },
                Row { id: Rid::MicMute, label: tr("Mic mute key"), sub: tr("Ctrl+Alt+M, all apps"), kind: RowKind::Toggle(self.cfg.mic_hotkey) },
                Row { id: Rid::Downloads, label: tr("Download progress"), sub: tr("Downloads folder"), kind: RowKind::Toggle(self.cfg.downloads) },
            ],
            3 => vec![
                Row { id: Rid::Weather, label: tr("Weather"), sub: tr("Left of the clock"), kind: RowKind::Toggle(self.cfg.weather_on) },
                Row { id: Rid::City, label: tr("City"), sub: tr("Location"), kind: RowKind::Stepper(weather::CITIES[self.cfg.weather_city].0.to_string()) },
                Row { id: Rid::Prayer, label: tr("Prayer times"), sub: tr("Countdown + toast"), kind: RowKind::Toggle(self.cfg.prayer_on) },
                Row { id: Rid::ImsakTerbit, label: tr("Imsak & sunrise"), sub: tr("In the prayer list"), kind: RowKind::Toggle(self.cfg.imsak_terbit) },
                Row { id: Rid::Ramadan, label: tr("Ramadan mode"), sub: tr("Imsak and iftar"), kind: RowKind::Toggle(self.cfg.ramadan) },
                Row { id: Rid::Calendar, label: tr("Calendar"), sub: self.cal_sub(), kind: RowKind::Button(self.cal_button_text().into()) },
                Row { id: Rid::LyricTiming, label: tr("Lyric timing"), sub: tr("Show earlier by"), kind: RowKind::Stepper(format!("{:+} ms", self.cfg.lyric_lead_ms)) },
                Row { id: Rid::Romaji, label: tr("Romanize lyrics"), sub: tr("Japanese and Korean"), kind: RowKind::Toggle(self.cfg.romaji_lyrics) },
            ]
            .into_iter()
            // the adhan sound, and with "File" a row to pick (or change) the file next to it
            .chain([Row { id: Rid::Adhan, label: tr("Adhan sound"), sub: tr("At prayer times"), kind: RowKind::Stepper(self.adhan_value()) }])
            .chain((self.cfg.adhan == adhan::FILE).then(|| Row {
                id: Rid::AdhanFile,
                label: tr("Adhan file"),
                sub: tr("mp3 or wav"),
                kind: RowKind::Button(if self.cfg.adhan_file.is_empty() { tr("Choose") } else { tr("Change") }.into()),
            }))
            // Kamus Jepang (hanya Jepang yang butuh; Korea tanpa kamus): tombol unduh bila belum ada, Delete (tekan dua kali) bila
            // hasil unduhan, tidak ada baris bila kamus datang dari folder lain (lihat dict.rs).
            .chain(if dict::removable().is_some() {
                Some(Row {
                    id: Rid::Dictionary,
                    label: tr("Japanese dictionary"),
                    sub: tr("47 MB on disk"),
                    kind: RowKind::Button(if self.sui.dict_confirming() { tr("Sure?") } else { tr("Delete") }.into()),
                })
            } else if !romaji::available() {
                Some(match dict::state() {
                    dict::Dl::Running(p) => Row { id: Rid::Dictionary, label: tr("Japanese dictionary"), sub: tr("Downloading dictionary"), kind: RowKind::Button(format!("{p}%")) },
                    dict::Dl::Failed => Row { id: Rid::Dictionary, label: tr("Japanese dictionary"), sub: tr("Download failed"), kind: RowKind::Button(tr("Retry").into()) },
                    dict::Dl::Idle => Row { id: Rid::Dictionary, label: tr("Japanese dictionary"), sub: tr("Needs 10 MB dictionary"), kind: RowKind::Button(tr("Get").into()) },
                })
            } else {
                None
            })
            .collect(),
            _ => Vec::new(),
        }
    }

    /// Wheel over Settings > General / Alerts / Widgets. True if it scrolled.
    pub(crate) fn settings_rows_wheel(&mut self, down: bool) -> bool {
        let max = row_count(self.settings_rows(self.sui.tab).len()).saturating_sub(ROW_VIS);
        let old = self.sui.rows_scroll;
        self.sui.rows_scroll = if down { (old + 1).min(max) } else { old.saturating_sub(1) };
        self.sui.rows_scroll != old
    }

    /// This rows tab has more rows than fit (the wheel scrolls it).
    pub(crate) fn rows_scrollable(&self) -> bool {
        self.sui.tab != 0 && row_count(self.settings_rows(self.sui.tab).len()) > ROW_VIS
    }

    fn settings_row_target(&self, i: usize) -> f32 {
        match self.sui.tab {
            0 => {
                if self.cfg.slides.get(i).map_or(false, |(_, on)| *on) { 1.0 } else { 0.0 }
            }
            t => match self.settings_rows(t).get(i).map(|r| &r.kind) {
                Some(RowKind::Toggle(true)) => 1.0,
                _ => 0.0,
            },
        }
    }

    pub(crate) fn save_settings(&mut self) {
        self.cfg.minimal = self.minimal;
        self.cfg.notifications = self.notif_on;
        self.cfg.save();
    }

    /// Value of the Adhan sound stepper: the mode, or the picked file's name with "File".
    fn adhan_value(&self) -> String {
        if self.cfg.adhan == adhan::FILE && !self.cfg.adhan_file.is_empty() {
            let name = adhan::file_label(&self.cfg.adhan_file);
            if name.chars().count() > 16 {
                return name.chars().take(15).collect::<String>() + "\u{2026}";
            }
            return name.to_string();
        }
        adhan::mode_label(self.cfg.adhan).to_string()
    }

    /// The slide config with the Prayer slide off while prayer times are off (see `slides::effective`).
    pub(crate) fn slide_cfg(&self) -> slides::SlideCfg {
        slides::effective(&self.cfg.slides, self.cfg.prayer_on)
    }

    /// Terapkan perubahan konfigurasi slide: bangun ulang urutan dan tetap berada di slide Settings.
    pub(crate) fn apply_slide_cfg(&mut self) {
        let cur = self.cur_id();
        self.order = slides::visible_order(&self.slide_cfg(), self.settings_open);
        let idx = self.order.iter().position(|&s| s == cur).unwrap_or(self.order.len() - 1);
        self.slide = idx as i32;
        self.sp = idx as f32;
        self.sv = 0.0;
    }

    pub(crate) fn settings_set_tab(&mut self, t: u8) {
        if self.sui.tab != t {
            self.sui.tab_dir = if t > self.sui.tab { 1.0 } else { -1.0 };
            self.sui.tab = t;
            self.sui.rows_scroll = 0;
            self.sui.tab_t0 = Instant::now();
            if t == 1 {
                self.sui.autostart = sys::autostart_enabled();
            }
        }
    }

    /// Satu langkah animasi (dipanggil tiap frame dari `tick`).
    pub(crate) fn settings_ui_step(&mut self, dt: f32) {
        if !self.settings_visible() {
            self.sui.entered = false;
            self.sui.swap = None;
            // snap knob ke keadaan sebenarnya supaya animasi masuk berikutnya mulai dari nilai yang benar
            for i in 0..MAX_ROWS {
                self.sui.knob[i] = self.settings_row_target(i);
                self.sui.kvel[i] = 0.0;
            }
            return;
        }
        if !self.sui.entered {
            self.sui.entered = true;
            self.sui.enter_t = Instant::now();
        }
        let n = ((dt / 0.008).ceil() as i32).max(1);
        let sdt = dt / n as f32;
        for i in 0..MAX_ROWS {
            let target = self.settings_row_target(i);
            let (mut x, mut v) = (self.sui.knob[i], self.sui.kvel[i]);
            for _ in 0..n {
                v += (380.0 * (target - x) - 30.0 * v) * sdt;
                x += v * sdt;
            }
            self.sui.knob[i] = x;
            self.sui.kvel[i] = v;
        }
        if self.sui.swap.map_or(false, |(_, _, t)| t.elapsed().as_secs_f32() > 0.28) {
            self.sui.swap = None;
        }
    }

    pub(crate) fn settings_animating(&self) -> bool {
        if !self.settings_visible() {
            return false;
        }
        (0..MAX_ROWS).any(|i| (self.sui.knob[i] - self.settings_row_target(i)).abs() > 0.01 || self.sui.kvel[i].abs() > 0.05)
            || self.sui.swap.is_some()
            || self.sui.tab_t0.elapsed().as_secs_f32() < 0.3
            || self.sui.enter_t.elapsed().as_secs_f32() < 0.5
            // label "Sure?" kembali ke "Delete" tepat waktu
            || self.sui.dict_confirm.map_or(false, |t| t.elapsed().as_secs_f32() < CONFIRM_SECS + 0.1)
            // the Calendar row's "Sure?" / "No ICS link copied" go back on time
            || self.cal_confirm.is_some_and(|t| t.elapsed().as_secs_f32() < CONFIRM_SECS + 0.1)
            || self.cal_paste_err.is_some_and(|t| t.elapsed().as_secs_f32() < CONFIRM_SECS + 0.1)
    }

    // ------------------------------------------------------------ klik
    pub(crate) fn settings_click(&mut self, x: f32, y: f32) {
        // tab
        let tabs = self.gfx.as_mut().map(tab_rects).unwrap_or_default();
        for (i, (tx, w)) in tabs.into_iter().enumerate() {
            if in_rect(x, y, tx, TAB_Y, w, TAB_H) {
                self.settings_set_tab(i as u8);
                return;
            }
        }
        match self.sui.tab {
            0 => self.settings_click_slides(x, y),
            t => self.settings_click_rows(t, x, y),
        }
    }

    fn settings_click_slides(&mut self, x: f32, y: f32) {
        for i in 0..self.cfg.slides.len() {
            let Some((cx, cy)) = slide_cells_visible(i, self.sui.slides_scroll) else { continue };
            if !in_rect(x, y, cx, cy, SC_W, CELL_H) {
                continue;
            }
            // dari kanan ke kiri di dalam sel: toggle, turun, naik
            let (tgx, dnx, upx) = (cx + SC_W - 34.0, cx + SC_W - 50.0, cx + SC_W - 66.0);
            if x >= tgx {
                let enabled = self.cfg.slides.iter().filter(|(_, on)| *on).count();
                // minimal satu slide harus tetap aktif
                if !(self.cfg.slides[i].1 && enabled <= 1) {
                    self.cfg.slides[i].1 = !self.cfg.slides[i].1;
                    self.slides_changed();
                }
            } else if x >= dnx && i + 1 < self.cfg.slides.len() {
                self.cfg.slides.swap(i, i + 1);
                self.sui.swap = Some((i, i + 1, Instant::now()));
                self.slides_keep_visible(i + 1);
                self.slides_changed();
            } else if x >= upx && i > 0 {
                self.cfg.slides.swap(i, i - 1);
                self.sui.swap = Some((i - 1, i, Instant::now()));
                self.slides_keep_visible(i - 1);
                self.slides_changed();
            }
            return;
        }
    }

    fn slides_changed(&mut self) {
        self.apply_slide_cfg();
        self.save_settings();
    }

    pub(crate) fn settings_click_rows(&mut self, tab: u8, x: f32, y: f32) {
        let rows = self.settings_rows(tab);
        for (i, r) in rows.iter().enumerate() {
            let Some((cx, cy)) = row_cell_visible(i, self.sui.rows_scroll) else { continue };
            if !in_rect(x, y, cx, cy, CELL_W, CELL_H) {
                continue;
            }
            // right part of the cell: the toggle (44 px), or the stepper ("-" on the left, "+" on the right). A long
            // value moves the "-" button left, so use the same layout as the drawing.
            let (right, plus) = match &r.kind {
                RowKind::Stepper(v) => {
                    let mx = self.gfx.as_mut().map_or(STEP_PLUS_X - STEP_MIN_SPAN, |g| stepper_fit(g, r, v).minus_x);
                    (x > cx + mx - 10.0, x > cx + (mx + STEP_PLUS_X) / 2.0)
                }
                RowKind::Toggle(_) | RowKind::Button(_) => (x > cx + CELL_W - 92.0, x > cx + CELL_W - 46.0),
            };
            match r.id {
                Rid::Minimal => self.minimal = !self.minimal,
                Rid::ThinMaximized => self.cfg.busy_on_maximized = !self.cfg.busy_on_maximized,
                Rid::Monitor => {
                    if right {
                        self.cfg.monitor = monitors::step_mode(self.cfg.monitor, plus);
                        self.place(true);
                    }
                }
                Rid::LyricTiming => {
                    if right {
                        let d = if plus { LEAD_STEP } else { -LEAD_STEP };
                        self.cfg.lyric_lead_ms = (self.cfg.lyric_lead_ms + d).clamp(LEAD_MIN, LEAD_MAX);
                    }
                }
                Rid::NotchSize => {
                    if right {
                        let last = scale::SIZES.len() - 1;
                        self.cfg.notch_size = if plus { (self.cfg.notch_size + 1).min(last) } else { self.cfg.notch_size.saturating_sub(1) };
                        self.place(true);
                    }
                }
                Rid::IdleDisplay => {
                    if right {
                        let last = settings::IDLE_DISPLAY_LABELS.len() - 1;
                        self.cfg.idle_display = if plus { (self.cfg.idle_display + 1).min(last) } else { self.cfg.idle_display.saturating_sub(1) };
                        self.cur_phase = self.idle_phase();
                        self.eye = EyeAnim::new();
                    }
                }
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
                    sys::set_autostart(!self.sui.autostart);
                    self.sui.autostart = sys::autostart_enabled();
                }
                Rid::SlideHotkeys => {
                    self.cfg.slide_hotkeys = !self.cfg.slide_hotkeys;
                    hotkeys::set_slide_hotkeys(self.hwnd, self.cfg.slide_hotkeys);
                }
                Rid::Language => {
                    if right {
                        self.cfg.lang = i18n::step(self.cfg.lang, plus);
                        i18n::set(self.cfg.lang);
                    }
                }
                Rid::Calendar => {
                    if right {
                        let clip = calendar::clipboard_text(self.hwnd);
                        self.cal_button(clip);
                    }
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
                Rid::Downloads => {
                    self.cfg.downloads = !self.cfg.downloads;
                    downloads::set_enabled(self.hwnd, self.cfg.downloads);
                    if !self.cfg.downloads {
                        self.dl_apply(Vec::new(), Vec::new());
                    }
                }
                Rid::MicMute => {
                    self.cfg.mic_hotkey = !self.cfg.mic_hotkey;
                    hotkeys::set_mic_hotkey(self.hwnd, self.cfg.mic_hotkey);
                    self.priv_t = Instant::now();
                }
                Rid::Weather => {
                    self.cfg.weather_on = !self.cfg.weather_on;
                    self.weather_apply();
                }
                Rid::City => {
                    if right {
                        let n = weather::CITIES.len();
                        self.cfg.weather_city = if plus { (self.cfg.weather_city + 1) % n } else { (self.cfg.weather_city + n - 1) % n };
                        self.weather_apply();
                        self.prayer_refresh();
                    }
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
                Rid::Adhan => {
                    if right {
                        self.cfg.adhan = adhan::step(self.cfg.adhan, plus);
                        if self.cfg.adhan == adhan::FILE && self.cfg.adhan_file.is_empty() {
                            adhan::pick_file(self.hwnd);
                        }
                        if self.cfg.adhan == adhan::OFF {
                            self.adhan_stop();
                        }
                    }
                }
                Rid::AdhanFile => {
                    if right {
                        adhan::pick_file(self.hwnd);
                    }
                }
                Rid::Accent => {
                    if right {
                        self.cfg.accent = accent::step(self.cfg.accent, plus);
                    }
                }
                Rid::Romaji => {
                    self.cfg.romaji_lyrics = !self.cfg.romaji_lyrics;
                    romaji::set_enabled(self.cfg.romaji_lyrics);
                    self.romaji_refresh();
                }
                Rid::Dictionary => {
                    if right {
                        if !dict::removable().is_some() {
                            dict::start(self.hwnd);
                        } else if !self.sui.dict_confirming() {
                            self.sui.dict_confirm = Some(Instant::now());
                        } else {
                            self.sui.dict_confirm = None;
                            dict::remove();
                        }
                    }
                }
            }
            self.save_settings();
            return;
        }
    }

    // ------------------------------------------------------------ gambar
    pub(crate) fn slide_settings(&self, g: &mut Gfx, ox: f32, al: f32) {
        g.txtb(tr("Settings"), LX + ox, 10.0, 15.0, [255, 255, 255], al, 100.0);
        for (i, ((tx, w), label)) in tab_rects(g).into_iter().zip(tab_labels()).enumerate() {
            g.chip(tx + ox, TAB_Y, w, TAB_H, label, i as u8 == self.sui.tab, al);
        }

        // konten tab: geser 16 px + fade saat ganti tab; baris masuk bertahap saat slide dibuka
        let k = ss(0.0, 0.26, self.sui.tab_t0.elapsed().as_secs_f32());
        let (vx, va) = (ox + (1.0 - k) * 16.0 * self.sui.tab_dir, al * k);
        let el = self.sui.enter_t.elapsed().as_secs_f32();
        match self.sui.tab {
            0 => self.draw_slides_tab(g, vx, va, el),
            t => self.draw_rows_tab(g, t, vx, va, el),
        }
    }

    fn draw_cell_bg(g: &mut Gfx, x: f32, y: f32, a: f32) {
        g.fill(&rrect(x, y, CELL_W, CELL_H, 11.0), &paint(255, 255, 255, 0.07 * a));
    }

    fn draw_slides_tab(&self, g: &mut Gfx, ox: f32, al: f32, el: f32) {
        let n = self.cfg.slides.len();
        for i in 0..n {
            let (id, on) = self.cfg.slides[i];
            let sc = self.sui.slides_scroll;
            let (mut x, mut y) = slide_cell_pos(i, sc);
            // animasi tukar tempat: sel yang baru pindah meluncur dari posisi lamanya
            if let Some((a, b, t)) = self.sui.swap {
                let e = ss(0.0, 0.26, t.elapsed().as_secs_f32());
                let from = if i == a { Some(slide_cell_pos(b, sc)) } else if i == b { Some(slide_cell_pos(a, sc)) } else { None };
                if let Some((fx, fy)) = from {
                    x = fx + (x - fx) * e;
                    y = fy + (y - fy) * e;
                }
            }
            // only rows in view (a cell sliding in from a hidden row appears once it is inside)
            if y < GRID_Y0 - 0.5 || y > GRID_Y0 + (SC_ROWS - 1) as f32 * (CELL_H + GRID_GAP_Y) + 0.5 {
                continue;
            }
            let enter = ss(0.0, 0.3, el - i.saturating_sub(sc * SC_COLS) as f32 * 0.04);
            let a = al * enter;
            let (x, y) = (x + ox, y + (1.0 - enter) * 8.0);
            g.fill(&rrect(x, y, SC_W, CELL_H, 11.0), &paint(255, 255, 255, 0.07 * a));
            let lab = if on { [255, 255, 255] } else { [160, 164, 176] };
            // with hotkeys on, a small keycap under the name shows the slide's Ctrl+Alt number
            match hotkeys::number(&self.slide_cfg(), i).filter(|_| self.cfg.slide_hotkeys) {
                Some(k) => {
                    g.txtb(id.short_label(), x + 9.0, y + 4.0, 11.0, lab, a, SLIDE_NAME_W);
                    let (kx, ky, kw, kh) = (x + 9.0, y + 19.0, 13.0, 11.0);
                    g.fill(&rrect(kx, ky, kw, kh, 3.0), &paint(255, 255, 255, 0.14 * a));
                    g.txt_c(&(k % 10).to_string(), kx + kw / 2.0, ky + ty(kh, 9.0), 9.0, GREY, a, kw, true);
                }
                None => g.txtb(id.short_label(), x + 9.0, y + ty(CELL_H, 11.0), 11.0, lab, a, SLIDE_NAME_W),
            }
            let dim = |ok: bool| paint(220, 220, 228, if ok { 0.9 } else { 0.25 } * a);
            // chevron naik / turun
            let (ux, dx, cy) = (x + SC_W - 58.0, x + SC_W - 42.0, y + CELL_H / 2.0);
            let (up, dn) = (dim(i > 0), dim(i + 1 < n));
            g.line(ux - 3.5, cy + 1.8, ux, cy - 1.8, 1.5, &up);
            g.line(ux, cy - 1.8, ux + 3.5, cy + 1.8, 1.5, &up);
            g.line(dx - 3.5, cy - 1.8, dx, cy + 1.8, 1.5, &dn);
            g.line(dx, cy + 1.8, dx + 3.5, cy - 1.8, 1.5, &dn);
            g.toggle(x + SC_W - 33.0, y + (CELL_H - 18.0) / 2.0, 28.0, 18.0, self.sui.knob[i], 0.0, a);
        }
        if self.slides_scrollable() {
            let rows = slide_rows(n);
            let th = SC_ROWS as f32 * (CELL_H + GRID_GAP_Y) - GRID_GAP_Y;
            let thumb = (th * SC_ROWS as f32 / rows as f32).max(14.0);
            let ty0 = GRID_Y0 + (th - thumb) * self.sui.slides_scroll as f32 / (rows - SC_ROWS) as f32;
            g.fill(&rrect(RX - 4.0 + ox, GRID_Y0, 3.0, th, 1.5), &paint(255, 255, 255, 0.10 * al));
            g.fill(&rrect(RX - 4.0 + ox, ty0, 3.0, thumb, 1.5), &paint(255, 255, 255, 0.55 * al));
        }
    }

    fn draw_rows_tab(&self, g: &mut Gfx, tab: u8, ox: f32, al: f32, el: f32) {
        let rows = self.settings_rows(tab);
        for (i, r) in rows.iter().enumerate() {
            let Some((x, y)) = row_cell_visible(i, self.sui.rows_scroll) else { continue };
            let enter = ss(0.0, 0.3, el - i as f32 * 0.04);
            let a = al * enter;
            let (x, y) = (x + ox, y + (1.0 - enter) * 8.0);
            Self::draw_cell_bg(g, x, y, a);
            let tw = text_w(&r.kind);
            let (label, sub) = (fit(g, r.label, 12.0, true, tw), fit(g, r.sub, 11.0, false, tw));
            g.txtb(&label, x + 12.0, y + 4.0, 12.0, [255, 255, 255], a, tw + 2.0);
            g.txt(&sub, x + 12.0, y + 19.0, 11.0, GREY, a, tw + 2.0);
            match &r.kind {
                RowKind::Toggle(_) => g.toggle(x + CELL_W - 44.0, y + (CELL_H - 20.0) / 2.0, 34.0, 20.0, self.sui.knob[i], 0.0, a),
                RowKind::Stepper(v) => {
                    let f = stepper_fit(g, r, v);
                    let cy = y + CELL_H / 2.0;
                    let (mx, px) = (x + f.minus_x, x + STEP_PLUS_X);
                    g.fill(&rrect(mx - 9.0, cy - 9.0, 18.0, 18.0, 9.0), &paint(255, 255, 255, 0.12 * a));
                    g.fill(&rrect(px - 9.0, cy - 9.0, 18.0, 18.0, 9.0), &paint(255, 255, 255, 0.12 * a));
                    let w = paint(235, 235, 240, a);
                    g.line(mx - 3.5, cy, mx + 3.5, cy, 1.6, &w);
                    g.line(px - 3.5, cy, px + 3.5, cy, 1.6, &w);
                    g.line(px, cy - 3.5, px, cy + 3.5, 1.6, &w);
                    let (vy, vw) = (cy - f.size * 6.0 / 11.0, px - mx - 18.0);
                    g.txt_c(&f.value, (mx + px) / 2.0, vy, f.size, g.acc.bright, a, vw, true);
                }
                RowKind::Button(v) => {
                    let (bw, cy) = (50.0, y + CELL_H / 2.0);
                    let bx = x + CELL_W - 10.0 - bw;
                    g.fill(&rrect(bx, cy - 10.0, bw, 20.0, 10.0), &paint(g.acc.bright[0], g.acc.bright[1], g.acc.bright[2], 0.25 * a));
                    g.txt_c(v, bx + bw / 2.0, cy - 6.0, 11.0, [190, 225, 255], a, bw, true);
                }
            }
        }
        let n = row_count(rows.len());
        if n > ROW_VIS {
            let th = ROW_VIS as f32 * (CELL_H + GRID_GAP_Y) - GRID_GAP_Y;
            let thumb = (th * ROW_VIS as f32 / n as f32).max(14.0);
            let ty0 = GRID_Y0 + (th - thumb) * self.sui.rows_scroll as f32 / (n - ROW_VIS) as f32;
            g.fill(&rrect(RX - 4.0 + ox, GRID_Y0, 3.0, th, 1.5), &paint(255, 255, 255, 0.10 * al));
            g.fill(&rrect(RX - 4.0 + ox, ty0, 3.0, thumb, 1.5), &paint(255, 255, 255, 0.55 * al));
        }
    }
}

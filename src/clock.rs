//! Fitur Clock: slide jam, jam pada notch kecil, helper waktu.

use super::*;

pub(crate) const CLOCK_FADE_IN: f32 = 0.35;

/// Short day name, 0 = Sunday.
pub(crate) fn day_short(i: usize) -> &'static str {
    [tr("Sun"), tr("Mon"), tr("Tue"), tr("Wed"), tr("Thu"), tr("Fri"), tr("Sat")][i % 7]
}

/// Short month name, 0 = January.
pub(crate) fn month(i: usize) -> &'static str {
    ["Jan", "Feb", "Mar", "Apr", tr("May"), "Jun", "Jul", tr("Aug"), "Sep", tr("Oct"), "Nov", tr("Dec")][i % 12]
}

pub(crate) fn now_tm() -> SYSTEMTIME {
    unsafe { GetLocalTime() }
}

pub(crate) fn today() -> String {
    let st = now_tm();
    format!("{:04}-{:02}-{:02}", st.wYear, st.wMonth, st.wDay)
}

impl App {
    pub(crate) fn clock_text(&self, g: &mut Gfx, w: f32, h: f32, al: f32) {
        let st = now_tm();
        let s = format!("{:02}:{:02}", st.wHour, st.wMinute);
        g.txt_c(&s, self.idle_clock_cx(w), ty(h, 14.0), 14.0, [255, 255, 255], al, w, true);
    }

    pub(crate) fn draw_clock_collapsed(&self, g: &mut Gfx, w: f32, h: f32, ca: f32) {
        let fade = ((self.idle_secs() % IDLE_CYCLE_SECS) / CLOCK_FADE_IN).clamp(0.0, 1.0);
        self.clock_text(g, w, h, ca * fade);
    }
}

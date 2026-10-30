//! Notch lock: keep the notch open or small until the person unlocks it. The padlock in the expanded notch offers the
//! two modes; while locked small, hovering only reveals a padlock in the small notch to unlock with.

use super::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Lock {
    Off,
    /// Stays expanded, except while a toast shows.
    Open,
    /// Stays small: nothing opens the notch.
    Min,
}

impl Lock {
    pub(crate) fn from_u8(v: u8) -> Lock {
        match v {
            1 => Lock::Open,
            2 => Lock::Min,
            _ => Lock::Off,
        }
    }

    pub(crate) fn as_u8(self) -> u8 {
        match self {
            Lock::Off => 0,
            Lock::Open => 1,
            Lock::Min => 2,
        }
    }
}

/// Padlock in the empty left gutter of the expanded notch, above the arrow button.
pub(crate) const LOCK_BTN: (f32, f32) = (CL + 22.0, 24.0);
const BTN_R: f32 = 14.0;
/// Width the padlock takes in the small notch (added through `priv_extra`).
pub(crate) const CHIP_W: f32 = 26.0;
const MENU: (f32, f32, f32, f32) = (CL + 10.0, 40.0, 132.0, 60.0);
const ROW_HEIGHT: f32 = 26.0;
const ROW_PAD: f32 = 4.0;

/// Top edge of popover row `i`.
fn row_y(i: usize) -> f32 {
    MENU.1 + ROW_PAD + i as f32 * ROW_HEIGHT
}

impl App {
    pub(crate) fn lock_set(&mut self, lock: Lock) {
        self.lock = lock;
        self.lock_menu = false;
        self.peek_until = None;
        self.reminding = false;
        self.out_close();
        self.save_settings();
    }

    /// Centre of the padlock in the small notch, left of the privacy marks.
    fn chip_pos(&self) -> (f32, f32) {
        (CX + self.w / 2.0 - self.priv_marks_extra() - CHIP_W / 2.0, self.h / 2.0)
    }

    /// Handle a click on the padlock or its popover. True if the click was used.
    pub(crate) fn lock_click(&mut self, x: f32, y: f32) -> bool {
        let near = |c: (f32, f32)| ((x - c.0).powi(2) + (y - c.1).powi(2)).sqrt() < BTN_R;
        if self.lock_chip() {
            if near(self.chip_pos()) {
                self.lock_set(Lock::Off);
                return true;
            }
            return false;
        }
        if !self.ready() || self.editing {
            return false;
        }
        if near(LOCK_BTN) {
            if self.lock == Lock::Open {
                self.lock_set(Lock::Off);
            } else {
                self.lock_menu = !self.lock_menu;
            }
            return true;
        }
        if !self.lock_menu {
            return false;
        }
        self.lock_menu = false;
        if !in_rect(x, y, MENU.0, MENU.1, MENU.2, MENU.3) {
            return false; // a click elsewhere closes the popover and still reaches the slide
        }
        if in_rect(x, y, MENU.0, row_y(0), MENU.2, ROW_HEIGHT) {
            self.lock_set(Lock::Open);
        } else if in_rect(x, y, MENU.0, row_y(1), MENU.2, ROW_HEIGHT) {
            self.lock_set(Lock::Min);
        }
        true
    }

    /// Padlock button and its popover in the expanded notch. `p` = how far the notch has opened.
    pub(crate) fn draw_lock(&self, g: &mut Gfx, p: f32) {
        let (x, y) = LOCK_BTN;
        let locked = self.lock == Lock::Open;
        if locked || self.lock_menu {
            g.circle(x, y, 11.0, &paint(255, 255, 255, 0.12 * p));
        }
        if locked {
            let ic = g.sacc(p);
            g.icon_lock(x, y, 13.0, false, &ic);
        } else {
            g.icon_lock(x, y, 13.0, true, &paint(255, 255, 255, 0.6 * p));
        }
        if !self.lock_menu {
            return;
        }
        let (mx, my, mw, mh) = MENU;
        let bg = rrect(mx, my, mw, mh, 10.0);
        g.fill(&bg, &paint(30, 30, 34, p));
        g.stroke(&bg, &paint(255, 255, 255, 0.14 * p), 1.0);
        for (i, (label, open)) in [(tr("Lock expanded"), true), (tr("Lock minimize"), false)].into_iter().enumerate() {
            let ry = row_y(i);
            let ic = paint(255, 255, 255, 0.85 * p);
            if open {
                g.icon_lock(mx + 17.0, ry + ROW_HEIGHT / 2.0, 12.0, false, &ic);
            } else {
                g.line(mx + 11.0, ry + ROW_HEIGHT / 2.0, mx + 23.0, ry + ROW_HEIGHT / 2.0, 2.0, &ic);
            }
            g.txt(label, mx + 32.0, ry + ty(ROW_HEIGHT, 12.0), 12.0, [255, 255, 255], p, mw - 38.0);
        }
    }

    /// Padlock in the small notch while locked small and hovered.
    pub(crate) fn draw_lock_chip(&self, g: &mut Gfx, ca: f32) {
        if self.lock_chip() {
            let (x, y) = self.chip_pos();
            let ic = g.sacc(ca);
            g.icon_lock(x, y, 13.0, false, &ic);
        }
    }
}

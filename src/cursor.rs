//! Kursor mouse di atas notch: tangan di tombol/baris yang bisa diklik, I-beam di kolom ketik, panah di tempat lain.

use super::*;

impl App {
    /// 0 = panah, 1 = tangan (bisa diklik), 2 = I-beam (teks). `(x, y)` dalam koordinat kanvas logis.
    pub(crate) fn cursor_kind(&self, x: f32, y: f32) -> u8 {
        if !self.expanded() {
            return 0;
        }
        let near = |c: (f32, f32)| ((x - c.0).powi(2) + (y - c.1).powi(2)).sqrt() <= 14.0;
        if self.n_slides() > 1 && (near(nav::NAV_L) || near(nav::NAV_R)) {
            return 1;
        }
        let hand = if self.is_slide(SlideId::Music) {
            self.music_hand(x, y)
        } else if self.is_slide(SlideId::Water) {
            self.water_hand(x, y)
        } else if self.is_slide(SlideId::Todo) {
            if self.editing && in_rect(x, y, LX, 40.0, CW, 30.0) {
                return 2;
            }
            self.todo_hand(x, y)
        } else if self.is_slide(SlideId::Timer) {
            self.timer_hand(x, y)
        } else if self.is_slide(SlideId::Clipboard) {
            self.clip_hit(x, y).is_some() || in_rect(x, y, clipboard::CLEAR_BTN.0, clipboard::CLEAR_BTN.1, clipboard::CLEAR_BTN.2, clipboard::CLEAR_BTN.3)
        } else if self.is_slide(SlideId::Ledge) {
            self.shelf_hand(x, y)
        } else if self.is_slide(SlideId::Scrap) {
            if in_rect(x, y, LX, NOTE_Y, CW, NOTE_H) {
                return if self.note_mode { 2 } else { 1 };
            }
            self.note_btn_at(x, y) != 0
        } else if self.is_slide(SlideId::Notifications) {
            self.notif_hand(x, y)
        } else if self.is_slide(SlideId::Agenda) {
            self.agenda_hit(x, y, notif::now_unix() as i64, calendar::local_offset()).is_some()
        } else if self.is_slide(SlideId::Prayer) {
            self.adhan_playing() && {
                let (bx, by, bw, bh) = prayer::slide_stop_rect();
                in_rect(x, y, bx, by, bw, bh)
            }
        } else if let Some(i) = self.cur_ext() {
            self.ext_hand(i, x, y)
        } else {
            false
        };
        hand as u8
    }
}

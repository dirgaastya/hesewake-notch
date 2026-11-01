//! Accent colors selectable in Settings > Appearance. Warning red, "done" green, water/Bluetooth blue and the
//! Pomodoro tomato stay as they are, because those colors carry their own meaning.

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Accent {
    pub(crate) name: &'static str,
    /// Main color: slide titles, bar fills, buttons, nav dots.
    pub(crate) main: [u8; 3],
    /// A little brighter, for small text and thin lines (stepper values, graphs, timer).
    pub(crate) bright: [u8; 3],
    /// Glow at the bottom of slides (dimmer than `main`).
    pub(crate) glow: [u8; 3],
    /// Track of a toggle that is on, and the bell icon on its knob.
    pub(crate) deep: [u8; 3],
    /// Text of a selected chip (pale).
    pub(crate) pale: [u8; 3],
    /// Dark tint behind white text or as a faint fill: the Timer Start button, the idle timer pill.
    pub(crate) dark: [u8; 3],
}

/// Index 0 = the default blue (the same colors as before accents were selectable).
pub(crate) const ACCENTS: [Accent; 6] = [
    Accent { name: "Blue", main: [90, 150, 255], bright: [90, 170, 255], glow: [90, 120, 200], deep: [40, 132, 245], pale: [190, 225, 255], dark: [40, 90, 170] },
    Accent { name: "Purple", main: [165, 120, 255], bright: [180, 145, 255], glow: [125, 95, 205], deep: [130, 80, 240], pale: [225, 210, 255], dark: [100, 65, 175] },
    Accent { name: "Pink", main: [255, 105, 180], bright: [255, 135, 200], glow: [195, 85, 150], deep: [230, 65, 150], pale: [255, 210, 232], dark: [160, 45, 110] },
    Accent { name: "Orange", main: [255, 150, 60], bright: [255, 172, 90], glow: [200, 120, 60], deep: [235, 120, 30], pale: [255, 225, 192], dark: [165, 85, 25] },
    Accent { name: "Green", main: [70, 200, 120], bright: [95, 215, 140], glow: [55, 155, 100], deep: [35, 170, 95], pale: [195, 240, 210], dark: [30, 120, 70] },
    Accent { name: "Teal", main: [50, 200, 210], bright: [85, 215, 225], glow: [40, 145, 170], deep: [20, 165, 180], pale: [190, 240, 245], dark: [20, 110, 130] },
];

/// Accent `i`; an out-of-range index falls back to the default blue.
pub(crate) fn get(i: usize) -> Accent {
    ACCENTS.get(i).copied().unwrap_or(ACCENTS[0])
}

/// Accent `i`'s name in the UI language.
pub(crate) fn label(i: usize) -> &'static str {
    use crate::i18n::tr;
    [tr("Blue"), tr("Purple"), "Pink", tr("Orange"), tr("Green"), tr("Teal")].get(i).copied().unwrap_or(ACCENTS[0].name)
}

/// Next/previous index, wrapping around.
pub(crate) fn step(i: usize, forward: bool) -> usize {
    let n = ACCENTS.len();
    if forward { (i + 1) % n } else { (i % n + n - 1) % n }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blue_is_the_previous_hardcoded_palette() {
        let b = get(0);
        assert_eq!(b.main, [90, 150, 255]);
        assert_eq!(b.bright, [90, 170, 255]);
        assert_eq!(b.glow, [90, 120, 200]);
        assert_eq!(b.deep, [40, 132, 245]);
        assert_eq!(b.pale, [190, 225, 255]);
        assert_eq!(b.dark, [40, 90, 170]);
    }

    #[test]
    fn get_falls_back_and_step_wraps() {
        assert_eq!(get(99), ACCENTS[0]);
        assert_eq!(step(0, false), ACCENTS.len() - 1);
        assert_eq!(step(ACCENTS.len() - 1, true), 0);
        assert_eq!(step(2, true), 3);
    }

    #[test]
    fn accent_text_stays_readable_on_black() {
        // WCAG contrast against the (black) notch background is at least 4.5:1 for colors used as text.
        fn lum(c: [u8; 3]) -> f64 {
            let ch = |v: u8| {
                let s = v as f64 / 255.0;
                if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2])
        }
        for a in ACCENTS {
            for c in [a.main, a.bright, a.pale] {
                let ratio = (lum(c) + 0.05) / 0.05;
                assert!(ratio >= 4.5, "{} {:?} contrast {:.2}", a.name, c, ratio);
            }
        }
        let names: std::collections::HashSet<_> = ACCENTS.iter().map(|a| a.name).collect();
        assert_eq!(names.len(), ACCENTS.len());
    }
}

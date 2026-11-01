//! A line of text that is being edited: the characters, the caret and the longest it may be. No window and no keyboard
//! here, so every key's effect is tested directly; the Settings window feeds it what the user types (an extension's page
//! can ask for one, see `input` in docs/extensions.md).

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextField {
    chars: Vec<char>,
    cursor: usize,
    max: usize,
}

impl TextField {
    /// Starts with `text` (control characters dropped, cut to `max` characters) and the caret at the end.
    pub(crate) fn new(text: &str, max: usize) -> TextField {
        let chars: Vec<char> = text.chars().filter(|c| !c.is_control()).take(max).collect();
        let cursor = chars.len();
        TextField { chars, cursor, max }
    }

    pub(crate) fn text(&self) -> String {
        self.chars.iter().collect()
    }

    #[cfg(test)]
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    /// The text before the caret, to measure where to draw it.
    pub(crate) fn before_cursor(&self) -> String {
        self.chars[..self.cursor].iter().collect()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Type one character. False if it was refused (a control character, or the line is as long as it may be).
    pub(crate) fn insert(&mut self, c: char) -> bool {
        if c.is_control() || self.chars.len() >= self.max {
            return false;
        }
        self.chars.insert(self.cursor, c);
        self.cursor += 1;
        true
    }

    /// Paste: what fits, one line (a new line becomes a space).
    pub(crate) fn insert_str(&mut self, s: &str) {
        for c in s.chars().map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c }) {
            if !self.insert(c) {
                break;
            }
        }
    }

    pub(crate) fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    pub(crate) fn delete(&mut self) {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    pub(crate) fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub(crate) fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.chars.len());
    }

    pub(crate) fn home(&mut self) {
        self.cursor = 0;
    }

    pub(crate) fn end(&mut self) {
        self.cursor = self.chars.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str, max: usize) -> TextField {
        let mut t = TextField::new("", max);
        s.chars().for_each(|c| {
            t.insert(c);
        });
        t
    }

    #[test]
    fn typing_goes_in_at_the_caret() {
        let mut t = typed("Phne", 32);
        t.left();
        t.left();
        t.insert('o');
        assert_eq!((t.text().as_str(), t.cursor()), ("Phone", 3));
        t.end();
        t.insert('!');
        assert_eq!(t.text(), "Phone!");
    }

    #[test]
    fn backspace_and_delete_at_the_ends_do_nothing_wrong() {
        let mut t = typed("ab", 8);
        t.home();
        t.backspace();
        assert_eq!(t.text(), "ab", "nothing before the caret");
        t.delete();
        assert_eq!(t.text(), "b");
        t.end();
        t.delete();
        assert_eq!(t.text(), "b", "nothing after the caret");
        t.backspace();
        assert_eq!((t.text().as_str(), t.is_empty()), ("", true));
        t.backspace();
        t.delete();
        assert_eq!(t.cursor(), 0);
    }

    #[test]
    fn the_caret_stays_between_the_ends() {
        let mut t = typed("abc", 8);
        t.right();
        assert_eq!(t.cursor(), 3);
        t.home();
        t.left();
        assert_eq!(t.cursor(), 0);
        t.right();
        t.right();
        assert_eq!((t.cursor(), t.before_cursor().as_str()), (2, "ab"));
    }

    #[test]
    fn it_is_never_longer_than_allowed_and_refuses_control_characters() {
        let mut t = typed("abcd", 4);
        assert!(!t.insert('e'), "full");
        assert!(!TextField::new("", 4).insert('\u{7}'));
        assert!(!TextField::new("", 4).insert('\n'));
        assert_eq!(t.text(), "abcd");
        let start = TextField::new("a\u{7}bc\u{0}defgh", 5);
        assert_eq!(start.text(), "abcde", "control characters dropped, then cut to the limit");
        t.backspace();
        assert!(t.insert('x'), "room again");
    }

    #[test]
    fn it_counts_characters_not_bytes() {
        let mut t = TextField::new("", 3);
        "\u{e9}\u{e9}\u{e9}\u{e9}".chars().for_each(|c| {
            t.insert(c);
        });
        assert_eq!(t.text().chars().count(), 3);
        t.home();
        t.right();
        assert_eq!(t.before_cursor(), "\u{e9}");
    }

    #[test]
    fn pasting_makes_one_line_and_stops_at_the_limit() {
        let mut t = typed("ab", 8);
        t.left();
        t.insert_str("X\nY\tZ");
        assert_eq!(t.text(), "aX Y Zb");
        let mut full = TextField::new("12345", 6);
        full.home();
        full.insert_str("abcdef");
        assert_eq!(full.text().chars().count(), 6);
        assert_eq!(full.text(), "a12345");
    }
}

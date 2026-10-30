//! Single-line editor used by dialogs and the command line (`edit.cpp`).

use crate::clipboard;
use crate::draw;
use gm_term::{Buffer, KeyChord, KeyCode, Mods, Style};
use gm_text::width::char_width;

#[derive(Clone, Debug, Default)]
pub struct LineEdit {
    chars: Vec<char>,
    /// Cursor position in chars.
    pub cursor: usize,
    scroll: usize,
    /// Selection as (anchor, cursor) in chars.
    sel: Option<(usize, usize)>,
    /// Far's "unchanged" state: the first typed character replaces the text.
    pub unchanged: bool,
    pub overtype: bool,
    pub password: bool,
    pub readonly: bool,
    /// Characters that delimit words for Ctrl+Left/Right.
    pub word_div: String,
}

impl LineEdit {
    pub fn new(text: &str) -> LineEdit {
        let chars: Vec<char> = text.chars().collect();
        LineEdit { cursor: chars.len(), chars, word_div: " \t~!%^&*()+|{}:\"<>?`-=\\[];',./".into(), ..Default::default() }
    }

    /// Initial text shown in the "unchanged" color, replaced by typing.
    pub fn with_unchanged(text: &str) -> LineEdit {
        let mut e = LineEdit::new(text);
        e.unchanged = !text.is_empty();
        e
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn set_text(&mut self, s: &str) {
        self.chars = s.chars().collect();
        self.cursor = self.chars.len();
        self.sel = None;
        self.scroll = 0;
        self.unchanged = false;
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        self.sel.map(|(a, b)| (a.min(b), a.max(b))).filter(|(a, b)| a != b)
    }

    pub fn select_all(&mut self) {
        self.sel = Some((0, self.chars.len()));
        self.cursor = self.chars.len();
    }

    pub fn insert_str(&mut self, s: &str) {
        if self.readonly {
            return;
        }
        self.delete_selection();
        for c in s.chars().filter(|c| *c != '\r' && *c != '\n') {
            self.chars.insert(self.cursor, c);
            self.cursor += 1;
        }
    }

    fn delete_selection(&mut self) -> bool {
        if let Some((a, b)) = self.selection() {
            self.chars.drain(a..b);
            self.cursor = a;
            self.sel = None;
            true
        } else {
            self.sel = None;
            false
        }
    }

    fn is_div(&self, c: char) -> bool {
        c.is_whitespace() || self.word_div.contains(c)
    }

    fn word_left(&self) -> usize {
        let mut i = self.cursor;
        while i > 0 && self.is_div(self.chars[i - 1]) {
            i -= 1;
        }
        while i > 0 && !self.is_div(self.chars[i - 1]) {
            i -= 1;
        }
        i
    }

    fn word_right(&self) -> usize {
        let mut i = self.cursor;
        let n = self.chars.len();
        while i < n && !self.is_div(self.chars[i]) {
            i += 1;
        }
        while i < n && self.is_div(self.chars[i]) {
            i += 1;
        }
        i
    }

    fn move_to(&mut self, pos: usize, select: bool) {
        if select {
            let anchor = match self.sel {
                Some((a, _)) => a,
                None => self.cursor,
            };
            self.sel = Some((anchor, pos));
        } else {
            self.sel = None;
        }
        self.cursor = pos;
    }

    /// Handles editing keys. Returns true if the key was consumed.
    pub fn handle_key(&mut self, k: &KeyChord) -> bool {
        let was_unchanged = self.unchanged;
        let select = k.mods.shift();
        let ctrl = k.mods.ctrl();
        let n = self.chars.len();
        match (k.code, k.mods) {
            (KeyCode::Char(c), m) if !m.ctrl() && !m.alt() => {
                if self.readonly {
                    return true;
                }
                if was_unchanged {
                    self.chars.clear();
                    self.cursor = 0;
                    self.sel = None;
                }
                self.delete_selection();
                if self.overtype && self.cursor < self.chars.len() {
                    self.chars[self.cursor] = c;
                } else {
                    self.chars.insert(self.cursor, c);
                }
                self.cursor += 1;
            }
            (KeyCode::Left, _) if !k.mods.alt() => {
                let p = if ctrl { self.word_left() } else { self.cursor.saturating_sub(1) };
                self.move_to(p, select);
            }
            (KeyCode::Right, _) if !k.mods.alt() => {
                let p = if ctrl { self.word_right() } else { (self.cursor + 1).min(n) };
                self.move_to(p, select);
            }
            (KeyCode::Home, m) if !m.ctrl() && !m.alt() => self.move_to(0, select),
            (KeyCode::End, m) if !m.ctrl() && !m.alt() => self.move_to(n, select),
            (KeyCode::Backspace, m) if !m.alt() && !m.shift() => {
                if self.readonly {
                    return true;
                }
                if was_unchanged {
                    self.set_text("");
                } else if !self.delete_selection() && self.cursor > 0 {
                    let from = if ctrl { self.word_left() } else { self.cursor - 1 };
                    self.chars.drain(from..self.cursor);
                    self.cursor = from;
                }
            }
            (KeyCode::Delete, Mods::NONE) | (KeyCode::Delete, Mods::CTRL) => {
                if self.readonly {
                    return true;
                }
                if was_unchanged {
                    self.set_text("");
                } else if !self.delete_selection() && self.cursor < n {
                    let to = if ctrl { self.word_right() } else { self.cursor + 1 };
                    self.chars.drain(self.cursor..to.min(n));
                }
            }
            // Ctrl+Y: clear; Ctrl+K: delete to end of line.
            (KeyCode::Char('y'), Mods::CTRL) if !self.readonly => self.set_text(""),
            (KeyCode::Char('k'), Mods::CTRL) if !self.readonly => {
                self.chars.truncate(self.cursor);
                self.sel = None;
            }
            (KeyCode::Char('a'), Mods::CTRL) => self.select_all(),
            // Copy: Ctrl+Ins / Ctrl+C.
            (KeyCode::Insert, Mods::CTRL) | (KeyCode::Char('c'), Mods::CTRL) => {
                let t = match self.selection() {
                    Some((a, b)) => self.chars[a..b].iter().collect(),
                    None => self.text(),
                };
                if !self.password {
                    clipboard::set(&t);
                }
            }
            // Cut: Shift+Del / Ctrl+X.
            (KeyCode::Delete, Mods::SHIFT) | (KeyCode::Char('x'), Mods::CTRL) => {
                if let Some((a, b)) = self.selection() {
                    let t: String = self.chars[a..b].iter().collect();
                    if !self.password {
                        clipboard::set(&t);
                    }
                    if !self.readonly {
                        self.delete_selection();
                    }
                }
            }
            // Paste: Shift+Ins / Ctrl+V.
            (KeyCode::Insert, Mods::SHIFT) | (KeyCode::Char('v'), Mods::CTRL) => {
                if was_unchanged {
                    self.set_text("");
                }
                let t = clipboard::get();
                self.insert_str(&t);
            }
            (KeyCode::Insert, Mods::NONE) => self.overtype = !self.overtype,
            _ => return false,
        }
        self.unchanged = false;
        true
    }

    pub fn paste(&mut self, s: &str) {
        if self.unchanged {
            self.set_text("");
        }
        self.insert_str(s);
    }

    /// Renders into `width` cells; returns the cursor x.
    pub fn render(&mut self, buf: &mut Buffer, x: u16, y: u16, width: usize, style: Style, sel_style: Style, unchanged_style: Style) -> u16 {
        if width == 0 {
            return x;
        }
        // Keep the cursor visible.
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        }
        loop {
            let w: usize = self.chars[self.scroll..self.cursor].iter().map(|&c| self.cw(c)).sum();
            if w < width || self.scroll >= self.cursor {
                break;
            }
            self.scroll += 1;
        }
        let base = if self.unchanged { unchanged_style } else { style };
        let sel = self.selection();
        let mut col = 0usize;
        let mut cursor_x = x;
        for i in self.scroll..=self.chars.len() {
            if i == self.cursor {
                cursor_x = x + col as u16;
            }
            if i == self.chars.len() {
                break;
            }
            let c = if self.password { '*' } else { self.chars[i] };
            let w = self.cw(c);
            if col + w > width {
                break;
            }
            let st = match sel {
                Some((a, b)) if i >= a && i < b => sel_style,
                _ => base,
            };
            draw::put(buf, x + col as u16, y, &c.to_string(), st, w);
            col += w;
        }
        for i in col..width {
            draw::set(buf, x + i as u16, y, ' ', base);
        }
        cursor_x
    }

    fn cw(&self, c: char) -> usize {
        if self.password { 1 } else { char_width(c) }
    }

    /// Moves the cursor to a clicked column.
    pub fn click(&mut self, col: usize) {
        let mut w = 0;
        let mut i = self.scroll;
        while i < self.chars.len() && w + self.cw(self.chars[i]) <= col {
            w += self.cw(self.chars[i]);
            i += 1;
        }
        self.cursor = i;
        self.sel = None;
        self.unchanged = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode, m: Mods) -> KeyChord {
        KeyChord::new(code, m)
    }

    #[test]
    fn editing() {
        let mut e = LineEdit::new("hello world");
        assert!(e.handle_key(&k(KeyCode::Left, Mods::CTRL)));
        assert_eq!(e.cursor, 6);
        e.handle_key(&k(KeyCode::Backspace, Mods::CTRL));
        assert_eq!(e.text(), "world");
        e.handle_key(&k(KeyCode::End, Mods::SHIFT));
        e.handle_key(&k(KeyCode::Char('X'), Mods::NONE));
        assert_eq!(e.text(), "X");
        let mut u = LineEdit::with_unchanged("old");
        u.handle_key(&k(KeyCode::Char('n'), Mods::NONE));
        assert_eq!(u.text(), "n");
        let mut u = LineEdit::with_unchanged("old");
        u.handle_key(&k(KeyCode::End, Mods::NONE));
        u.handle_key(&k(KeyCode::Char('!'), Mods::NONE));
        assert_eq!(u.text(), "old!");
    }
}

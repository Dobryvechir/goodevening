//! Text buffer: a rope with grouped undo/redo and encoding-aware load/save.

use gm_text::{Codepage, Eol, codepage};
use ropey::Rope;
use std::io;
use std::path::Path;

#[derive(Clone, Debug)]
struct Change {
    at: usize,
    removed: String,
    inserted: String,
    before: (usize, usize),
    after: (usize, usize),
    group: u64,
}

pub struct TextBuffer {
    pub rope: Rope,
    pub cp: Codepage,
    pub bom: bool,
    pub eol: Eol,
    undo: Vec<Change>,
    redo: Vec<Change>,
    group: u64,
    /// Undo depth at the last save (to compute `modified`).
    saved_at: usize,
    saved_group: Option<u64>,
    /// Some characters could not be represented in the code page.
    pub lossy: bool,
}

impl Default for TextBuffer {
    fn default() -> Self {
        TextBuffer::from_str("", Eol::native())
    }
}

impl TextBuffer {
    pub fn from_str(s: &str, eol: Eol) -> TextBuffer {
        TextBuffer {
            rope: Rope::from_str(s),
            cp: Codepage::Utf8,
            bom: false,
            eol,
            undo: Vec::new(),
            redo: Vec::new(),
            group: 1,
            saved_at: 0,
            saved_group: None,
            lossy: false,
        }
    }

    /// Loads bytes, detecting BOM, code page and line endings.
    pub fn from_bytes(bytes: &[u8], detect: bool, default_cp: Codepage) -> TextBuffer {
        let (cp, skip, bom) = match codepage::detect_bom(bytes) {
            Some((cp, n)) => (cp, n, true),
            None if detect => (codepage::detect(&bytes[..bytes.len().min(256 * 1024)]), 0, false),
            None => (default_cp, 0, false),
        };
        let text = cp.decode(&bytes[skip..]);
        let mut b = TextBuffer::from_str(&text, Eol::detect(&text));
        b.cp = cp;
        b.bom = bom;
        b
    }

    pub fn load(path: &Path, detect: bool, default_cp: Codepage) -> io::Result<TextBuffer> {
        let bytes = std::fs::read(path)?;
        Ok(TextBuffer::from_bytes(&bytes, detect, default_cp))
    }

    /// Re-decodes the original bytes in another code page (Shift+F8).
    pub fn recode(&mut self, bytes: &[u8], cp: Codepage) {
        let skip = if self.bom { codepage::detect_bom(bytes).map(|b| b.1).unwrap_or(0) } else { 0 };
        let text = cp.decode(&bytes[skip..]);
        self.rope = Rope::from_str(&text);
        self.cp = cp;
        self.undo.clear();
        self.redo.clear();
        self.saved_at = 0;
        self.saved_group = None;
    }

    /// Encoded file content, optionally converting all line breaks.
    pub fn encode(&self, convert_eol: Option<Eol>) -> (Vec<u8>, bool) {
        let mut text = self.rope.to_string();
        if let Some(e) = convert_eol {
            text = text.replace("\r\n", "\n").replace('\r', "\n");
            if e != Eol::Lf {
                text = text.replace('\n', e.as_str());
            }
        }
        let mut out = Vec::with_capacity(text.len() + 4);
        if self.bom {
            out.extend_from_slice(self.cp.bom());
        }
        let body = self.cp.encode(&text);
        let lossy = self.cp.decode(&body) != text;
        out.extend_from_slice(&body);
        (out, lossy)
    }

    /// Saves safely: the data is first written to a temporary file (to fail
    /// early on a full disk), then written in place so that hard links,
    /// ownership and permissions of the original file are kept.
    pub fn save(&mut self, path: &Path, convert_eol: Option<Eol>) -> io::Result<()> {
        let (bytes, lossy) = self.encode(convert_eol);
        self.lossy = lossy;
        let tmp = path.with_file_name(format!(".{}.gmtmp", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        std::fs::write(&tmp, &bytes)?;
        let r = std::fs::write(path, &bytes);
        let _ = std::fs::remove_file(&tmp);
        r?;
        if let Some(e) = convert_eol {
            self.eol = e;
            let s = self.rope.to_string().replace("\r\n", "\n").replace('\r', "\n");
            let s = if e == Eol::Lf { s } else { s.replace('\n', e.as_str()) };
            self.rope = Rope::from_str(&s);
            self.undo.clear();
            self.redo.clear();
        }
        self.mark_saved();
        Ok(())
    }

    pub fn mark_saved(&mut self) {
        self.saved_at = self.undo.len();
        self.saved_group = self.undo.last().map(|c| c.group);
    }

    pub fn modified(&self) -> bool {
        self.undo.len() != self.saved_at || self.undo.last().map(|c| c.group) != self.saved_group
    }

    pub fn line_count(&self) -> usize {
        // A trailing newline does not start a visible extra line in Far
        // terms, but the cursor may go there, so keep ropey's count.
        self.rope.len_lines()
    }

    /// Length of line `i` in chars, without its line break.
    pub fn line_len(&self, i: usize) -> usize {
        if i >= self.rope.len_lines() {
            return 0;
        }
        let l = self.rope.line(i);
        let mut n = l.len_chars();
        if n > 0 && l.char(n - 1) == '\n' {
            n -= 1;
        }
        if n > 0 && l.char(n - 1) == '\r' {
            n -= 1;
        }
        n
    }

    pub fn line_text(&self, i: usize) -> String {
        if i >= self.rope.len_lines() {
            return String::new();
        }
        let l = self.rope.line(i);
        l.slice(..self.line_len(i)).to_string()
    }

    pub fn char_idx(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.rope.len_lines().saturating_sub(1));
        self.rope.line_to_char(line) + col.min(self.line_len(line))
    }

    pub fn pos_of(&self, idx: usize) -> (usize, usize) {
        let idx = idx.min(self.rope.len_chars());
        let line = self.rope.char_to_line(idx);
        (line, idx - self.rope.line_to_char(line))
    }

    /// Starts a new undo group (each user command is one group).
    pub fn new_group(&mut self) {
        self.group += 1;
    }

    /// Replaces chars `[from, to)` with `text`, recording undo.
    pub fn replace(&mut self, from: usize, to: usize, text: &str, before: (usize, usize)) -> (usize, usize) {
        let to = to.min(self.rope.len_chars());
        let from = from.min(to);
        let removed = self.rope.slice(from..to).to_string();
        self.rope.remove(from..to);
        self.rope.insert(from, text);
        let after = self.pos_of(from + text.chars().count());
        // Merge consecutive single-char typing into one change.
        if let Some(last) = self.undo.last_mut()
            && last.group == self.group
            && removed.is_empty()
            && last.at + last.inserted.chars().count() == from
        {
            last.inserted.push_str(text);
            last.after = after;
            self.redo.clear();
            return after;
        }
        self.undo.push(Change { at: from, removed, inserted: text.to_string(), before, after, group: self.group });
        self.redo.clear();
        after
    }

    /// Undoes one group; returns the cursor to restore.
    pub fn undo(&mut self) -> Option<(usize, usize)> {
        let g = self.undo.last()?.group;
        let mut cursor = None;
        while self.undo.last().is_some_and(|c| c.group == g) {
            let c = self.undo.pop()?;
            let n = c.inserted.chars().count();
            self.rope.remove(c.at..c.at + n);
            self.rope.insert(c.at, &c.removed);
            cursor = Some(c.before);
            self.redo.push(c);
        }
        self.group += 1;
        cursor
    }

    pub fn redo(&mut self) -> Option<(usize, usize)> {
        let g = self.redo.last()?.group;
        let mut cursor = None;
        while self.redo.last().is_some_and(|c| c.group == g) {
            let c = self.redo.pop()?;
            let n = c.removed.chars().count();
            self.rope.remove(c.at..c.at + n);
            self.rope.insert(c.at, &c.inserted);
            cursor = Some(c.after);
            self.undo.push(c);
        }
        self.group += 1;
        cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_groups() {
        let mut b = TextBuffer::from_str("hello\nworld\n", Eol::Lf);
        b.new_group();
        for (i, c) in "abc".chars().enumerate() {
            b.replace(i, i, &c.to_string(), (0, i));
        }
        assert_eq!(b.line_text(0), "abchello");
        assert!(b.modified());
        b.new_group();
        let s = b.char_idx(1, 0);
        b.replace(s, s + 5, "WORLD", (1, 0));
        assert_eq!(b.undo(), Some((1, 0)));
        assert_eq!(b.line_text(1), "world");
        assert_eq!(b.undo(), Some((0, 0)));
        assert_eq!(b.line_text(0), "hello");
        assert!(!b.modified());
        b.redo();
        assert_eq!(b.line_text(0), "abchello");
    }

    #[test]
    fn encodings_and_eol() {
        let bytes = Codepage::Utf16Le.bom().iter().copied().chain(Codepage::Utf16Le.encode("a\r\nb")).collect::<Vec<u8>>();
        let b = TextBuffer::from_bytes(&bytes, true, Codepage::Utf8);
        assert_eq!(b.cp, Codepage::Utf16Le);
        assert!(b.bom);
        assert_eq!(b.eol, Eol::CrLf);
        assert_eq!(b.line_len(0), 1);
        let (out, lossy) = b.encode(None);
        assert_eq!(out, bytes);
        assert!(!lossy);
        let (unix, _) = b.encode(Some(Eol::Lf));
        assert_eq!(Codepage::Utf16Le.decode(&unix[2..]), "a\nb");
    }
}

//! Syntax colors of the editor's "Colors" mode (F5): the same colors as the
//! viewer's, kept up to date while the text is edited.
//!
//! Texts up to [`SYNC_LIMIT`] chars are recolored right after every change.
//! Bigger texts are recolored at most once per [`RECOLOR_EVERY`] while
//! typing (and when idle); in between, edits are applied to the old colors
//! so they stay in place (inserted text takes the color before it).

use crate::buffer::TextBuffer;
use gm_term::Color;
use gm_viewer::syntax::{Lang, TEXT, colorize_chars};
use std::time::{Duration, Instant};

/// Texts recolored completely after every change (about 25 ms per million chars).
const SYNC_LIMIT: usize = 1 << 20;
/// Texts bigger than this are not colored (as in the viewer).
pub const MAX_CHARS: usize = 32 << 20;
/// How often big texts are recolored while being edited.
const RECOLOR_EVERY: Duration = Duration::from_secs(1);

pub struct SyntaxColors {
    pub lang: Lang,
    fg: Vec<Color>,
    mark: Vec<bool>,
    /// Number of problems (red background) at the last full recoloring.
    pub problems: usize,
    /// Buffer version the colors belong to.
    version: u64,
    /// The colors were computed for exactly that text (not patched).
    exact: bool,
    computed: Instant,
}

impl SyntaxColors {
    pub fn new(lang: Lang, buf: &mut TextBuffer) -> SyntaxColors {
        let mut c = SyntaxColors { lang, fg: Vec::new(), mark: Vec::new(), problems: 0, version: 0, exact: false, computed: Instant::now() };
        c.recolor(buf);
        c
    }

    fn recolor(&mut self, buf: &mut TextBuffer) {
        buf.take_changes();
        let c = colorize_chars(&buf.rope.to_string(), self.lang);
        (self.fg, self.mark, self.problems) = (c.fg, c.mark, c.problems);
        self.version = buf.version();
        self.exact = true;
        self.computed = Instant::now();
    }

    /// Brings the colors up to date with `buf`; `idle` (no input for a
    /// while) finishes a pending recoloring of a big text. Returns true if
    /// the colors changed.
    pub fn update(&mut self, buf: &mut TextBuffer, idle: bool) -> bool {
        let due = idle || self.computed.elapsed() >= RECOLOR_EVERY;
        if self.version == buf.version() {
            if !self.exact && due {
                self.recolor(buf);
                return true;
            }
            return false;
        }
        if buf.rope.len_chars() <= SYNC_LIMIT || due {
            self.recolor(buf);
            return true;
        }
        match buf.take_changes() {
            Some(changes) => {
                for (at, removed, inserted) in changes {
                    let at = at.min(self.fg.len());
                    let end = (at + removed).min(self.fg.len());
                    let fill = if at > 0 { self.fg[at - 1] } else { TEXT };
                    self.fg.splice(at..end, std::iter::repeat_n(fill, inserted));
                    self.mark.splice(at..end, std::iter::repeat_n(false, inserted));
                }
                self.version = buf.version();
                self.exact = false;
            }
            None => self.recolor(buf),
        }
        true
    }

    /// Color and problem flag of the char at index `i`.
    pub fn at(&self, i: usize) -> (Color, bool) {
        (self.fg.get(i).copied().unwrap_or(TEXT), self.mark.get(i).copied().unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gm_text::Eol;

    #[test]
    fn recolors_after_changes() {
        let mut b = TextBuffer::from_str("let s = 1;\n", Eol::Lf);
        let mut c = SyntaxColors::new(Lang::Rust, &mut b);
        let (kw, _) = c.at(0);
        assert_eq!(c.problems, 0);
        assert!(!c.update(&mut b, false));
        // Insert an unclosed bracket: recolored at once (small text).
        b.replace(8, 8, "(", (0, 8));
        assert!(c.update(&mut b, false));
        assert_eq!(c.problems, 1);
        assert!(c.at(8).1, "the unclosed bracket is marked");
        assert_eq!(c.at(0).0, kw);
        b.undo();
        c.update(&mut b, false);
        assert_eq!(c.problems, 0);
    }

    #[test]
    fn patches_big_texts_until_idle() {
        let mut b = TextBuffer::from_str("x", Eol::Lf);
        let mut c = SyntaxColors::new(Lang::Rust, &mut b);
        // Pretend the last full coloring just happened on a big text.
        let mut big = "let a = \"s\";\n".repeat(SYNC_LIMIT / 13 + 1);
        big.insert(0, ' ');
        b = TextBuffer::from_str(&big, Eol::Lf);
        c.recolor(&mut b);
        let string = c.at(9).0;
        b.replace(10, 10, "zz", (0, 10));
        assert!(c.update(&mut b, false));
        assert!(!c.exact);
        assert_eq!(c.at(10).0, string, "inserted text takes the color before it");
        assert_eq!(c.fg.len(), b.rope.len_chars());
        assert!(c.update(&mut b, true));
        assert!(c.exact);
    }
}

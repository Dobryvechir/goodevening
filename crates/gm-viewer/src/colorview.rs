//! The viewer's "Colors" mode (F5): syntax-colored text on a black
//! background, scrolled by lines (and wrapped rows within a line).

use crate::syntax::{self, BACKGROUND, Cell, Lang, MARK, TEXT};
use gm_term::{Buffer, Modifier, Rect, Style};
use gm_text::width::char_width;
use gm_ui::draw;

pub struct ColorView {
    lines: Vec<Vec<Cell>>,
    /// Number of problems shown on a red background.
    pub problems: usize,
    top: usize,
    /// Wrapped row of the top line shown first.
    sub: usize,
    pub left: usize,
    pub wrap: bool,
    height: usize,
    width: usize,
}

impl ColorView {
    pub fn new(text: &str, lang: Lang, tab: usize, top_line: usize, wrap: bool) -> ColorView {
        let c = syntax::colorize(text, lang, tab);
        let top = top_line.min(c.lines.len().saturating_sub(1));
        ColorView { lines: c.lines, problems: c.problems, top, sub: 0, left: 0, wrap, height: 20, width: 80 }
    }

    /// The first visible line (0-based).
    pub fn top_line(&self) -> usize {
        self.top
    }

    /// Start indices (into the line's cells) of each screen row of `line`.
    fn row_starts(&self, line: usize) -> Vec<usize> {
        let mut starts = vec![0];
        if !self.wrap || self.width == 0 {
            return starts;
        }
        let mut w = 0;
        for (i, c) in self.lines[line].iter().enumerate() {
            let cw = char_width(c.ch).max(1);
            if w > 0 && w + cw > self.width {
                starts.push(i);
                w = 0;
            }
            w += cw;
        }
        starts
    }

    fn rows(&self, line: usize) -> usize {
        self.row_starts(line).len()
    }

    fn next(&self, (l, s): (usize, usize)) -> Option<(usize, usize)> {
        if s + 1 < self.rows(l) {
            Some((l, s + 1))
        } else if l + 1 < self.lines.len() {
            Some((l + 1, 0))
        } else {
            None
        }
    }

    fn prev(&self, (l, s): (usize, usize)) -> Option<(usize, usize)> {
        if s > 0 {
            Some((l, s - 1))
        } else if l > 0 {
            Some((l - 1, self.rows(l - 1) - 1))
        } else {
            None
        }
    }

    /// Whether everything from the top row to the end fits on the screen.
    fn end_visible(&self) -> bool {
        let mut p = (self.top, self.sub);
        for _ in 1..self.height.max(1) {
            match self.next(p) {
                Some(n) => p = n,
                None => return true,
            }
        }
        self.next(p).is_none()
    }

    pub fn down(&mut self, n: usize) {
        for _ in 0..n {
            if self.end_visible() {
                break;
            }
            match self.next((self.top, self.sub)) {
                Some((l, s)) => (self.top, self.sub) = (l, s),
                None => break,
            }
        }
    }

    pub fn up(&mut self, n: usize) {
        for _ in 0..n {
            match self.prev((self.top, self.sub)) {
                Some((l, s)) => (self.top, self.sub) = (l, s),
                None => break,
            }
        }
    }

    pub fn page_down(&mut self) {
        self.down(self.height.saturating_sub(1).max(1));
    }

    pub fn page_up(&mut self) {
        self.up(self.height.saturating_sub(1).max(1));
    }

    pub fn go_home(&mut self) {
        (self.top, self.sub, self.left) = (0, 0, 0);
    }

    pub fn go_end(&mut self) {
        let l = self.lines.len() - 1;
        (self.top, self.sub) = (l, self.rows(l) - 1);
        self.up(self.height.saturating_sub(1));
    }

    pub fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
        self.sub = 0;
        self.left = 0;
    }

    pub fn percent(&self) -> u64 {
        if self.lines.len() <= 1 || self.end_visible() { 100 } else { (self.top * 100 / self.lines.len()) as u64 }
    }

    pub fn render(&mut self, buf: &mut Buffer, r: Rect, show_arrows: bool) {
        self.height = r.height as usize;
        self.width = r.width as usize;
        self.sub = self.sub.min(self.rows(self.top) - 1);
        let base = Style::default().fg(TEXT).bg(BACKGROUND);
        let arrows = Style::default().fg(syntax::TEXT).bg(BACKGROUND).add_modifier(Modifier::BOLD);
        draw::fill(buf, r, ' ', base);
        let mut pos = Some((self.top, self.sub));
        for row in 0..r.height {
            let Some((l, s)) = pos else { break };
            let starts = self.row_starts(l);
            let cells = &self.lines[l];
            let (a, b) = (starts[s], starts.get(s + 1).copied().unwrap_or(cells.len()));
            let y = r.y + row;
            let skip = if self.wrap { 0 } else { self.left };
            let (mut col, mut x) = (0usize, 0usize);
            for cell in &cells[a..b] {
                let w = char_width(cell.ch).max(1);
                if col < skip {
                    col += w;
                    continue;
                }
                if x + w > r.width as usize {
                    break;
                }
                let st = if cell.mark {
                    Style::default().fg(syntax::MARK_TEXT).bg(MARK).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(cell.fg).bg(BACKGROUND)
                };
                let ch = if cell.ch.is_control() { draw::control_glyph(cell.ch) } else { cell.ch };
                draw::put(buf, r.x + x as u16, y, &ch.to_string(), st, w);
                x += w;
                col += w;
            }
            if show_arrows && !self.wrap {
                if skip > 0 && !cells.is_empty() {
                    draw::set(buf, r.x, y, '«', arrows);
                }
                let total: usize = cells.iter().map(|c| char_width(c.ch).max(1)).sum();
                if total > skip + r.width as usize {
                    draw::set(buf, r.right() - 1, y, '»', arrows);
                }
            }
            pos = self.next((l, s));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolling() {
        let text: String = (0..50).map(|i| format!("let x{i} = {i};\n")).collect();
        let mut v = ColorView::new(&text, Lang::JavaScript, 4, 10, false);
        v.height = 10;
        v.width = 40;
        assert_eq!(v.top_line(), 10);
        v.down(5);
        assert_eq!(v.top_line(), 15);
        v.go_end();
        assert_eq!(v.top_line(), 41);
        v.down(3);
        assert_eq!(v.top_line(), 41);
        v.go_home();
        v.up(1);
        assert_eq!(v.top_line(), 0);
    }

    #[test]
    fn wrapped_rows() {
        let mut v = ColorView::new("abcdefghij\nx\n", Lang::Python, 4, 0, true);
        v.width = 4;
        v.height = 2;
        assert_eq!(v.row_starts(0), vec![0, 4, 8]);
        v.down(1);
        assert_eq!((v.top, v.sub), (0, 1));
    }
}

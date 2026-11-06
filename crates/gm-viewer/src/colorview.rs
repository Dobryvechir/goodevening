//! The viewer's "Colors" mode (F5): syntax-colored text on a black
//! background, scrolled by lines (and wrapped rows within a line).

use crate::syntax::{self, BACKGROUND, Cell, Lang, MARK, SELECTION, TEXT};
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
    /// Mouse selection: (anchor, caret) as (line, cell index).
    pub sel: Option<(Pos, Pos)>,
    /// Where the content was last rendered (for mouse hit-testing).
    view: Rect,
}

/// A position between cells: (line, cell index).
pub type Pos = (usize, usize);

impl ColorView {
    pub fn new(text: &str, lang: Lang, tab: usize, top_line: usize, wrap: bool) -> ColorView {
        let c = syntax::colorize(text, lang, tab);
        let top = top_line.min(c.lines.len().saturating_sub(1));
        ColorView { lines: c.lines, problems: c.problems, top, sub: 0, left: 0, wrap, height: 20, width: 80, sel: None, view: Rect::new(0, 0, 0, 0) }
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

    /// The selection as ordered positions, if not empty.
    fn selection(&self) -> Option<(Pos, Pos)> {
        let (a, b) = self.sel?;
        let (a, b) = (a.min(b), a.max(b));
        (a < b).then_some((a, b))
    }

    /// The selected text as shown (tabs restored, hidden Markdown tags left out).
    pub fn selected_text(&self) -> Option<String> {
        let ((l1, i1), (l2, i2)) = self.selection()?;
        let eol = if cfg!(windows) { "\r\n" } else { "\n" };
        let mut out = String::new();
        for l in l1..=l2.min(self.lines.len() - 1) {
            let cells = &self.lines[l];
            let a = if l == l1 { i1.min(cells.len()) } else { 0 };
            let b = if l == l2 { i2.min(cells.len()) } else { cells.len() };
            for c in &cells[a..b.max(a)] {
                match c.tab {
                    0 => out.push(c.ch),
                    1 => out.push('\t'),
                    _ => {}
                }
            }
            if l < l2 {
                out.push_str(eol);
            }
        }
        Some(out)
    }

    /// Whether screen cell (x, y) is in the rendered content area.
    pub fn inside(&self, x: u16, y: u16) -> bool {
        let r = self.view;
        x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
    }

    /// Rendered content area (for mouse handling).
    pub fn view(&self) -> Rect {
        self.view
    }

    /// The position of screen cell (x, y) (past the end of a row: its end);
    /// `y` outside the content area is clamped to the first/last row.
    pub fn pos_at(&self, x: u16, y: u16) -> Pos {
        let r = self.view;
        let row = y.clamp(r.y, r.bottom().max(r.y + 1) - 1) - r.y;
        let target = if self.wrap { 0 } else { self.left } + x.saturating_sub(r.x) as usize;
        let mut p = (self.top, self.sub.min(self.rows(self.top) - 1));
        for _ in 0..row {
            match self.next(p) {
                Some(n) => p = n,
                None => {
                    let l = self.lines.len() - 1;
                    return (l, self.lines[l].len());
                }
            }
        }
        let (l, s) = p;
        let starts = self.row_starts(l);
        let cells = &self.lines[l];
        let (a, b) = (starts[s], starts.get(s + 1).copied().unwrap_or(cells.len()));
        let mut col = 0;
        for (i, c) in cells[a..b].iter().enumerate() {
            col += char_width(c.ch).max(1);
            if target < col {
                return (l, a + i);
            }
        }
        (l, b)
    }

    pub fn render(&mut self, buf: &mut Buffer, r: Rect, show_arrows: bool) {
        self.height = r.height as usize;
        self.width = r.width as usize;
        self.view = r;
        let sel = self.selection();
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
            for (i, cell) in cells[a..b].iter().enumerate() {
                let selected = sel.is_some_and(|(p, q)| (l, a + i) >= p && (l, a + i) < q);
                let w = char_width(cell.ch).max(1);
                if col < skip {
                    col += w;
                    continue;
                }
                if x + w > r.width as usize {
                    break;
                }
                let st = if selected {
                    Style::default().fg(cell.fg).bg(SELECTION)
                } else if cell.mark {
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
    fn mouse_selection() {
        let mut v = ColorView::new("ab	c
xyz
", Lang::Python, 4, 0, false);
        v.view = Rect::new(0, 1, 20, 5);
        v.height = 5;
        assert_eq!(v.pos_at(1, 1), (0, 1));
        assert_eq!(v.pos_at(15, 1), (0, 5)); // past the end of "ab<tab>c"
        assert_eq!(v.pos_at(1, 2), (1, 1));
        assert_eq!(v.pos_at(1, 9), (2, 0)); // below the text
        v.sel = Some(((1, 2), (0, 1)));
        let eol = if cfg!(windows) { "\r\n" } else { "\n" };
        assert_eq!(v.selected_text(), Some(format!("b\tc{eol}xy")));
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

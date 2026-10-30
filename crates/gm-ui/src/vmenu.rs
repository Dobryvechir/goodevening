//! Vertical menu (`vmenu.cpp`): hotkeys, separators, check marks, scrolling,
//! filtering and keys passed back to the caller.

use crate::draw;
use crate::ui::{Flow, Modal, Ui};
use gm_term::{Buffer, Event, KeyChord, KeyCode, Mods, MouseButton, MouseEventKind, PaletteId, Rect, style};
use gm_text::width::str_width;

#[derive(Clone, Debug, Default)]
pub struct MenuItem {
    /// Label; `&` marks the hotkey.
    pub text: String,
    /// Right-aligned hint (a key name, a size…).
    pub hint: String,
    pub separator: bool,
    pub disabled: bool,
    pub checked: bool,
    /// Grayed but selectable (e.g. history items for missing files).
    pub gray: bool,
    pub data: usize,
}

impl MenuItem {
    pub fn new(text: impl Into<String>) -> MenuItem {
        MenuItem { text: text.into(), ..Default::default() }
    }
    pub fn with_hint(text: impl Into<String>, hint: impl Into<String>) -> MenuItem {
        MenuItem { text: text.into(), hint: hint.into(), ..Default::default() }
    }
    pub fn separator() -> MenuItem {
        MenuItem { separator: true, ..Default::default() }
    }
    pub fn data(mut self, d: usize) -> MenuItem {
        self.data = d;
        self
    }
    pub fn checked(mut self, c: bool) -> MenuItem {
        self.checked = c;
        self
    }
    pub fn disabled(mut self, d: bool) -> MenuItem {
        self.disabled = d;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuResult {
    Selected(usize),
    Cancel,
    /// A key listed in `pass_keys` was pressed on item `usize`.
    Key(KeyChord, usize),
    /// Left/Right in a menu bar submenu.
    Left,
    Right,
}

pub struct VMenu {
    pub title: String,
    pub bottom: String,
    pub items: Vec<MenuItem>,
    pub selected: usize,
    top: usize,
    /// Explicit position (x, y); centered when `None`.
    pub pos: Option<(u16, u16)>,
    pub max_height: Option<u16>,
    pub min_width: u16,
    /// Keys returned to the caller instead of being handled.
    pub pass_keys: Vec<KeyChord>,
    /// Left/Right return `MenuResult::Left/Right` (menu bar).
    pub bar_mode: bool,
    /// Typing filters the items (Far's Ctrl+Alt+F filter, always on for history lists).
    pub filterable: bool,
    filter: String,
    visible: Vec<usize>,
    /// Dialog colors (combo box / list) instead of menu colors.
    pub dialog_colors: bool,
    pub wrap: bool,
    pub help_topic: String,
    result: MenuResult,
    rect: Rect,
}

impl VMenu {
    pub fn new(title: impl Into<String>, items: Vec<MenuItem>) -> VMenu {
        let mut m = VMenu {
            title: title.into(),
            bottom: String::new(),
            items,
            selected: 0,
            top: 0,
            pos: None,
            max_height: None,
            min_width: 0,
            pass_keys: Vec::new(),
            bar_mode: false,
            filterable: false,
            filter: String::new(),
            visible: Vec::new(),
            dialog_colors: false,
            wrap: true,
            help_topic: String::new(),
            result: MenuResult::Cancel,
            rect: Rect::default(),
        };
        m.refilter();
        m.selected = m.first_selectable(0, true).unwrap_or(0);
        m
    }

    pub fn select(&mut self, i: usize) {
        if i < self.items.len() {
            self.selected = i;
        }
    }

    fn refilter(&mut self) {
        let f = self.filter.to_lowercase();
        self.visible =
            (0..self.items.len()).filter(|&i| f.is_empty() || gm_i18n::strip_hotkey(&self.items[i].text).to_lowercase().contains(&f)).collect();
        if !self.visible.contains(&self.selected)
            && let Some(&first) = self.visible.iter().find(|&&i| self.selectable(i))
        {
            self.selected = first;
        }
    }

    fn selectable(&self, i: usize) -> bool {
        self.items.get(i).is_some_and(|it| !it.separator && !it.disabled)
    }

    fn first_selectable(&self, from_vis: usize, forward: bool) -> Option<usize> {
        let v = &self.visible;
        if forward {
            v.iter().skip(from_vis).find(|&&i| self.selectable(i)).copied()
        } else {
            v.iter().take(from_vis + 1).rev().find(|&&i| self.selectable(i)).copied()
        }
    }

    fn vis_pos(&self) -> usize {
        self.visible.iter().position(|&i| i == self.selected).unwrap_or(0)
    }

    fn step(&mut self, delta: isize) {
        let n = self.visible.len();
        if n == 0 {
            return;
        }
        let mut p = self.vis_pos() as isize;
        for _ in 0..n {
            p += delta.signum();
            if p < 0 || p >= n as isize {
                if !self.wrap || delta.abs() > 1 {
                    p = p.clamp(0, n as isize - 1);
                    if let Some(s) = self.first_selectable(p as usize, delta < 0).or(self.first_selectable(p as usize, delta > 0)) {
                        self.selected = s;
                    }
                    return;
                }
                p = if p < 0 { n as isize - 1 } else { 0 };
            }
            if self.selectable(self.visible[p as usize]) {
                self.selected = self.visible[p as usize];
                return;
            }
        }
    }

    fn page(&mut self, down: bool) {
        let h = self.rect.height.saturating_sub(2).max(1) as isize;
        let n = self.visible.len() as isize;
        let p = (self.vis_pos() as isize + if down { h - 1 } else { -(h - 1) }).clamp(0, n - 1);
        if let Some(s) = self.first_selectable(p as usize, down).or_else(|| self.first_selectable(p as usize, !down)) {
            self.selected = s;
        }
    }

    pub fn run(&mut self, ui: &mut Ui) -> MenuResult {
        self.result = MenuResult::Cancel;
        let saved = std::mem::take(&mut ui.help_topic);
        ui.help_topic = if self.help_topic.is_empty() { saved.clone() } else { self.help_topic.clone() };
        let _ = ui.run_modal(self);
        ui.help_topic = saved;
        self.result
    }

    fn layout(&self, screen: Rect) -> Rect {
        let mut w = self.min_width as usize;
        w = w.max(str_width(&self.title) + 6).max(str_width(&self.bottom) + 6);
        for it in &self.items {
            let hint = if it.hint.is_empty() { 0 } else { str_width(&it.hint) + 2 };
            w = w.max(draw::label_width(&it.text) + hint + 6);
        }
        if self.filterable && !self.filter.is_empty() {
            w = w.max(str_width(&self.filter) + 12);
        }
        let w = (w as u16).min(screen.width.saturating_sub(4)).max(10);
        let maxh = self.max_height.unwrap_or(screen.height.saturating_sub(4)).min(screen.height.saturating_sub(2));
        let h = (self.visible.len() as u16 + 2).min(maxh).max(3);
        match self.pos {
            Some((x, y)) => {
                let x = x.min(screen.width.saturating_sub(w + 2));
                let y = y.min(screen.height.saturating_sub(h + 1));
                Rect::new(x, y, w, h)
            }
            None => draw::centered(screen, w, h),
        }
    }
}

impl Modal for VMenu {
    fn render(&mut self, buf: &mut Buffer, screen: Rect) -> Option<(u16, u16)> {
        let r = self.layout(screen);
        self.rect = r;
        let (text, sel, hl, sel_hl, boxs, title, gray, sel_gray, dis) = if self.dialog_colors {
            (
                style(PaletteId::DialogComboText),
                style(PaletteId::DialogComboTextSelected),
                style(PaletteId::DialogComboHighlight),
                style(PaletteId::DialogComboHighlightSelected),
                style(PaletteId::DialogComboBox),
                style(PaletteId::DialogComboTitle),
                style(PaletteId::DialogComboGrayText),
                style(PaletteId::DialogComboGrayTextSelected),
                style(PaletteId::DialogComboDisabled),
            )
        } else {
            (
                style(PaletteId::MenuText),
                style(PaletteId::MenuTextSelected),
                style(PaletteId::MenuHighlight),
                style(PaletteId::MenuHighlightSelected),
                style(PaletteId::MenuBox),
                style(PaletteId::MenuTitle),
                style(PaletteId::MenuGrayText),
                style(PaletteId::MenuGrayTextSelected),
                style(PaletteId::MenuTextDisabled),
            )
        };
        draw::fill(buf, r, ' ', text);
        draw::frame(buf, r, true, boxs, Some((&self.title, title)));
        let bottom = if self.filterable && !self.filter.is_empty() { format!("[{}]", self.filter) } else { self.bottom.clone() };
        draw::title_at(buf, r, r.bottom() - 1, &bottom, title);
        draw::shadow(buf, r);
        let rows = r.height.saturating_sub(2) as usize;
        let vp = self.vis_pos();
        if vp < self.top {
            self.top = vp;
        }
        if vp >= self.top + rows {
            self.top = vp + 1 - rows;
        }
        if self.top + rows > self.visible.len() {
            self.top = self.visible.len().saturating_sub(rows);
        }
        let inner = (r.width - 2) as usize;
        for row in 0..rows {
            let Some(&idx) = self.visible.get(self.top + row) else { break };
            let it = &self.items[idx];
            let y = r.y + 1 + row as u16;
            if it.separator {
                draw::separator(buf, r.x, y, r.width, true, boxs);
                if !it.text.is_empty() {
                    draw::title_at(buf, r, y, &it.text, boxs);
                }
                continue;
            }
            let is_sel = idx == self.selected;
            let (st, hst) = match (is_sel, it.disabled, it.gray) {
                (_, true, _) => (dis, dis),
                (true, _, true) => (sel_gray, sel_hl),
                (true, _, _) => (sel, sel_hl),
                (false, _, true) => (gray, hl),
                _ => (text, hl),
            };
            draw::fill(buf, Rect::new(r.x + 1, y, r.width - 2, 1), ' ', st);
            let mark = if it.checked { '√' } else { ' ' };
            draw::set(buf, r.x + 1, y, mark, st);
            let hint_w = if it.hint.is_empty() { 0 } else { str_width(&it.hint) + 1 };
            let label_max = inner.saturating_sub(2 + hint_w);
            draw::hotkey_text(buf, r.x + 2, y, &it.text, st, hst, label_max);
            if hint_w > 0 {
                let hx = r.right() - 1 - hint_w as u16;
                draw::put(buf, hx, y, &it.hint, st, hint_w);
            }
        }
        if self.visible.len() > rows {
            draw::scrollbar(buf, r.right() - 1, r.y + 1, rows as u16, self.top, self.visible.len(), rows, boxs);
        }
        None
    }

    fn handle(&mut self, ev: &Event, _ui: &mut Ui) -> Flow {
        match ev {
            Event::Key(k) => {
                if self.pass_keys.contains(k) {
                    self.result = MenuResult::Key(*k, self.selected);
                    return Flow::Done;
                }
                match (k.code, k.mods) {
                    (KeyCode::Esc, _) | (KeyCode::F(10), Mods::NONE) => {
                        if self.filterable && !self.filter.is_empty() && k.code == KeyCode::Esc {
                            self.filter.clear();
                            self.refilter();
                            return Flow::Continue;
                        }
                        self.result = MenuResult::Cancel;
                        return Flow::Done;
                    }
                    (KeyCode::Enter, _) => {
                        if self.selectable(self.selected) && self.visible.contains(&self.selected) {
                            self.result = MenuResult::Selected(self.selected);
                            return Flow::Done;
                        }
                    }
                    (KeyCode::Up, _) => self.step(-1),
                    (KeyCode::Down, _) => self.step(1),
                    (KeyCode::Home, _) | (KeyCode::PageUp, Mods::CTRL) => {
                        if let Some(s) = self.first_selectable(0, true) {
                            self.selected = s;
                        }
                    }
                    (KeyCode::End, _) | (KeyCode::PageDown, Mods::CTRL) => {
                        if let Some(s) = self.first_selectable(self.visible.len().saturating_sub(1), false) {
                            self.selected = s;
                        }
                    }
                    (KeyCode::PageUp, _) => self.page(false),
                    (KeyCode::PageDown, _) => self.page(true),
                    (KeyCode::Left, _) if self.bar_mode => {
                        self.result = MenuResult::Left;
                        return Flow::Done;
                    }
                    (KeyCode::Right, _) if self.bar_mode => {
                        self.result = MenuResult::Right;
                        return Flow::Done;
                    }
                    (KeyCode::Backspace, _) if self.filterable => {
                        self.filter.pop();
                        self.refilter();
                    }
                    (KeyCode::Char(c), m) if !m.ctrl() => {
                        if self.filterable {
                            self.filter.push(c);
                            self.refilter();
                            return Flow::Continue;
                        }
                        let lc = c.to_lowercase().next().unwrap_or(c);
                        let hit = self.items.iter().enumerate().find(|(i, it)| self.selectable(*i) && draw::hotkey_of(&it.text) == Some(lc));
                        if let Some((i, _)) = hit {
                            self.selected = i;
                            self.result = MenuResult::Selected(i);
                            return Flow::Done;
                        }
                    }
                    _ => {}
                }
            }
            Event::Mouse(m) => {
                let r = self.rect;
                let inside = m.column >= r.x && m.column < r.right() && m.row > r.y && m.row + 1 < r.bottom();
                match m.kind {
                    MouseEventKind::ScrollUp => self.step(-1),
                    MouseEventKind::ScrollDown => self.step(1),
                    MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                        if inside {
                            let row = (m.row - r.y - 1) as usize + self.top;
                            if let Some(&idx) = self.visible.get(row)
                                && self.selectable(idx)
                            {
                                self.selected = idx;
                                if matches!(m.kind, MouseEventKind::Up(_)) {
                                    self.result = MenuResult::Selected(idx);
                                    return Flow::Done;
                                }
                            }
                        } else if matches!(m.kind, MouseEventKind::Down(_)) {
                            self.result = MenuResult::Cancel;
                            return Flow::Done;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        self.result = MenuResult::Cancel;
                        return Flow::Done;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Flow::Continue
    }
}

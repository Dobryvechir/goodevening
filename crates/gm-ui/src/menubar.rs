//! Horizontal menu bar (F9, `hmenu.cpp`).

use crate::draw;
use crate::ui::Ui;
use crate::vmenu::{MenuItem, MenuResult, VMenu};
use gm_term::{Buffer, PaletteId, Rect, style};
use gm_text::width::str_width;

pub struct MenuBar {
    /// (title with `&` hotkey, items)
    pub menus: Vec<(String, Vec<MenuItem>)>,
    pub selected: usize,
    /// Last selected item per submenu (Far remembers it).
    pub last_items: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuBarResult {
    /// (menu index, item index)
    Selected(usize, usize),
    Cancel,
}

impl MenuBar {
    pub fn new(menus: Vec<(String, Vec<MenuItem>)>) -> MenuBar {
        let n = menus.len();
        MenuBar { menus, selected: 0, last_items: vec![0; n] }
    }

    /// Title positions (x, width) for rendering and mouse hit tests.
    fn positions(&self) -> Vec<(u16, u16)> {
        let mut x = 2u16;
        self.menus
            .iter()
            .map(|(t, _)| {
                let w = draw::label_width(t) as u16 + 4;
                let p = (x, w);
                x += w;
                p
            })
            .collect()
    }

    /// Draws the bar on row 0. `active` highlights the selected title.
    pub fn render(&self, buf: &mut Buffer, width: u16, active: bool) {
        let text = style(PaletteId::HMenuText);
        let hl = style(PaletteId::HMenuHighlight);
        let sel = style(PaletteId::HMenuTextSelected);
        let sel_hl = style(PaletteId::HMenuHighlightSelected);
        draw::fill(buf, Rect::new(0, 0, width, 1), ' ', text);
        for (i, ((t, _), (x, w))) in self.menus.iter().zip(self.positions()).enumerate() {
            let (st, h) = if active && i == self.selected { (sel, sel_hl) } else { (text, hl) };
            draw::fill(buf, Rect::new(x, 0, w, 1), ' ', st);
            draw::hotkey_text(buf, x + 2, 0, t, st, h, w as usize);
        }
    }

    pub fn run(&mut self, ui: &mut Ui) -> MenuBarResult {
        loop {
            let bg = ui.term.last_frame().clone();
            let width = ui.screen().width;
            ui.draw_over(&bg, |buf, _| self.render(buf, width, true));
            let pos = self.positions();
            let (x, _) = pos[self.selected];
            let items = self.menus[self.selected].1.clone();
            let w = items.iter().map(|i| draw::label_width(&i.text) + str_width(&i.hint) + 8).max().unwrap_or(10);
            let mut m = VMenu::new("", items);
            m.bar_mode = true;
            m.pos = Some((x, 1));
            m.min_width = w as u16;
            m.select(self.last_items[self.selected]);
            let r = m.run(ui);
            // Restore the screen without the bar highlight for the next round.
            ui.draw_over(&bg, |_, _| {});
            match r {
                MenuResult::Selected(i) => {
                    self.last_items[self.selected] = i;
                    return MenuBarResult::Selected(self.selected, i);
                }
                MenuResult::Left => self.selected = (self.selected + self.menus.len() - 1) % self.menus.len(),
                MenuResult::Right => self.selected = (self.selected + 1) % self.menus.len(),
                _ => return MenuBarResult::Cancel,
            }
        }
    }
}

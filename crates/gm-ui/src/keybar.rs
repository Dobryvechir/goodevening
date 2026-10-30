//! The bottom key bar (`keybar.cpp`): F1…F12 labels per modifier set.

use crate::draw;
use gm_term::{Buffer, Mods, PaletteId, Rect, style};

#[derive(Clone, Debug, Default)]
pub struct KeyBarLabels {
    pub normal: [String; 12],
    pub shift: [String; 12],
    pub alt: [String; 12],
    pub ctrl: [String; 12],
    pub ctrl_shift: [String; 12],
    pub alt_shift: [String; 12],
    pub ctrl_alt: [String; 12],
}

impl KeyBarLabels {
    pub fn for_mods(&self, m: Mods) -> &[String; 12] {
        match (m.ctrl(), m.alt(), m.shift()) {
            (false, false, true) => &self.shift,
            (false, true, false) => &self.alt,
            (true, false, false) => &self.ctrl,
            (true, false, true) => &self.ctrl_shift,
            (false, true, true) => &self.alt_shift,
            (true, true, _) => &self.ctrl_alt,
            _ => &self.normal,
        }
    }

    pub fn set(row: &mut [String; 12], labels: &[&str]) {
        for (i, l) in labels.iter().enumerate().take(12) {
            row[i] = (*l).to_string();
        }
    }
}

pub struct KeyBar;

impl KeyBar {
    /// Draws the 12 labels evenly across `width` on row `y`.
    pub fn render(buf: &mut Buffer, y: u16, width: u16, labels: &[String; 12]) {
        let num = style(PaletteId::KeybarNum);
        let text = style(PaletteId::KeybarText);
        let bg = style(PaletteId::KeybarBackground);
        draw::fill(buf, Rect::new(0, y, width, 1), ' ', bg);
        let w = width as usize;
        for (i, label) in labels.iter().enumerate() {
            let start = i * w / 12;
            let end = (i + 1) * w / 12;
            if end <= start {
                continue;
            }
            let n = (i + 1).to_string();
            let nw = draw::put(buf, start as u16, y, &n, num, end - start);
            let lw = end - start - nw;
            if lw > 0 {
                draw::put_fit(buf, (start + nw) as u16, y, label, text, lw);
            }
        }
    }

    /// Which key (1–12) is under column `x`.
    pub fn hit(x: u16, width: u16) -> Option<u8> {
        if width == 0 {
            return None;
        }
        Some(((x as usize * 12 / width as usize) + 1).min(12) as u8)
    }
}

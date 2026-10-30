//! Message boxes (`message.cpp`).

use crate::dialog::DialogBuilder;
use crate::draw;
use crate::ui::Ui;
use gm_term::{Buffer, PaletteId, Rect, style};
use gm_text::width::str_width;

fn build(title: &str, text: &str, buttons: &[&str], warn: bool, screen_w: u16) -> crate::Dialog {
    let lines: Vec<&str> = text.lines().collect();
    let btn_w: u16 = buttons.iter().map(|b| draw::label_width(b) as u16 + 5).sum();
    let max_w = screen_w.saturating_sub(4).max(30);
    let mut w = lines.iter().map(|l| str_width(l) as u16 + 10).max().unwrap_or(30);
    w = w.max(btn_w + 8).max(str_width(title) as u16 + 12).clamp(30, max_w);
    let mut b = DialogBuilder::new(title, w);
    if warn {
        b = b.warn();
    }
    for l in &lines {
        // Long lines are wrapped.
        let inner = (w - 10) as usize;
        for chunk in wrap(l, inner) {
            b.text_center(chunk);
        }
    }
    if !buttons.is_empty() {
        b.separator();
        b.buttons(buttons, 0);
    }
    b.build()
}

/// Splits `s` into lines of at most `width` cells at word boundaries.
pub fn wrap(s: &str, width: usize) -> Vec<String> {
    if str_width(s) <= width || width == 0 {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split(' ') {
        let candidate = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if str_width(&candidate) <= width {
            cur = candidate;
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            let mut w = word.to_string();
            while str_width(&w) > width {
                let (p, _) = gm_text::width::prefix_by_width(&w, width);
                let p = p.to_string();
                out.push(p.clone());
                w = w[p.len()..].to_string();
            }
            cur = w;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Shows a message; returns the index of the chosen button (None on Esc).
pub fn message(ui: &mut Ui, title: &str, text: &str, buttons: &[&str]) -> Option<usize> {
    let w = ui.screen().width;
    let mut d = build(title, text, buttons, false, w);
    let first = d.items.iter().position(|i| matches!(i.kind, crate::ItemKind::Button { .. }))?;
    d.run(ui).map(|i| i - first)
}

/// Same as `message` with the red warning colors.
pub fn message_warn(ui: &mut Ui, title: &str, text: &str, buttons: &[&str]) -> Option<usize> {
    let w = ui.screen().width;
    let mut d = build(title, text, buttons, true, w);
    let first = d.items.iter().position(|i| matches!(i.kind, crate::ItemKind::Button { .. }))?;
    d.run(ui).map(|i| i - first)
}

/// Error box with an OK button.
pub fn error(ui: &mut Ui, title: &str, text: &str) {
    message_warn(ui, title, text, &[gm_i18n::msg(gm_i18n::MsgId::MOk)]);
}

/// Draws a non-interactive box (progress / "please wait") over `bg`.
pub fn draw_box(ui: &mut Ui, bg: &Buffer, title: &str, lines: &[String]) {
    ui.draw_over(bg, |buf, screen| {
        let w = lines.iter().map(|l| str_width(l) as u16 + 10).max().unwrap_or(30).clamp(30, screen.width.saturating_sub(4));
        let h = lines.len() as u16 + 4;
        let r = draw::centered(screen, w, h);
        let text = style(PaletteId::DialogText);
        draw::fill(buf, r, ' ', text);
        draw::frame(
            buf,
            Rect::new(r.x + 3, r.y + 1, r.width - 6, r.height - 2),
            true,
            style(PaletteId::DialogBox),
            Some((title, style(PaletteId::DialogBoxTitle))),
        );
        draw::shadow(buf, r);
        for (i, l) in lines.iter().enumerate() {
            draw::put(buf, r.x + 5, r.y + 2 + i as u16, l, text, (w - 10) as usize);
        }
    });
}

/// A text progress bar of `width` cells: `█████░░░░░`.
pub fn bar(done: u64, total: u64, width: usize) -> String {
    let filled = if total == 0 { width } else { ((done as u128 * width as u128) / total as u128) as usize }.min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapping() {
        assert_eq!(wrap("aaa bbb ccc", 7), vec!["aaa bbb", "ccc"]);
        assert_eq!(bar(1, 2, 4), "██░░");
    }
}

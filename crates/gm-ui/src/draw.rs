//! Drawing primitives: width-aware text, boxes, shadows, hotkey labels.

use gm_term::{Buffer, Color, Rect, Style};
use gm_text::width::{char_width, str_width};

pub const SINGLE: [char; 6] = ['┌', '┐', '└', '┘', '─', '│'];
pub const DOUBLE: [char; 6] = ['╔', '╗', '╚', '╝', '═', '║'];

/// Writes `s` at (x, y), clipped to `max` cells and to the buffer.
/// Returns the number of cells written.
pub fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, style: Style, max: usize) -> usize {
    let area = buf.area;
    if y < area.y || y >= area.bottom() || x >= area.right() {
        return 0;
    }
    let limit = max.min((area.right() - x) as usize);
    let mut col = 0usize;
    for c in s.chars() {
        let c = if c.is_control() { control_glyph(c) } else { c };
        let w = char_width(c);
        if w == 0 {
            // Combining mark: append to the previous cell.
            if col > 0 {
                let cell = &mut buf[(x + col as u16 - 1, y)];
                let mut sym = cell.symbol().to_string();
                sym.push(c);
                cell.set_symbol(&sym);
            }
            continue;
        }
        if col + w > limit {
            break;
        }
        let cell = &mut buf[(x + col as u16, y)];
        cell.set_char(c).set_style(style);
        if w == 2 && col + 1 < limit {
            buf[(x + col as u16 + 1, y)].set_symbol("").set_style(style);
        }
        col += w;
    }
    col
}

/// Like `put` but pads with spaces to exactly `width` cells.
pub fn put_fit(buf: &mut Buffer, x: u16, y: u16, s: &str, style: Style, width: usize) {
    let n = put(buf, x, y, s, style, width);
    for i in n..width {
        if x as usize + i < buf.area.right() as usize {
            set(buf, x + i as u16, y, ' ', style);
        }
    }
}

/// Glyphs Far uses for control characters in viewers/panels.
pub fn control_glyph(c: char) -> char {
    match c {
        '\0' => ' ',
        '\t' => ' ',
        '\x07' => '•',
        '\x08' => '◘',
        '\x0b' => '♂',
        '\x0c' => '♀',
        '\x1b' => '←',
        c if (c as u32) < 32 => char::from_u32(0x2400 + c as u32).unwrap_or('?'),
        '\x7f' => '⌂',
        c => c,
    }
}

pub fn set(buf: &mut Buffer, x: u16, y: u16, c: char, style: Style) {
    let a = buf.area;
    if x >= a.x && x < a.right() && y >= a.y && y < a.bottom() {
        buf[(x, y)].set_char(c).set_style(style);
    }
}

pub fn fill(buf: &mut Buffer, r: Rect, c: char, style: Style) {
    let r = r.intersection(buf.area);
    for y in r.top()..r.bottom() {
        for x in r.left()..r.right() {
            buf[(x, y)].set_char(c).set_style(style);
        }
    }
}

/// Draws a single or double box with an optional centered title.
pub fn frame(buf: &mut Buffer, r: Rect, double: bool, style: Style, title: Option<(&str, Style)>) {
    if r.width < 2 || r.height < 2 {
        return;
    }
    let g = if double { DOUBLE } else { SINGLE };
    let (l, t, rr, b) = (r.left(), r.top(), r.right() - 1, r.bottom() - 1);
    set(buf, l, t, g[0], style);
    set(buf, rr, t, g[1], style);
    set(buf, l, b, g[2], style);
    set(buf, rr, b, g[3], style);
    for x in l + 1..rr {
        set(buf, x, t, g[4], style);
        set(buf, x, b, g[4], style);
    }
    for y in t + 1..b {
        set(buf, l, y, g[5], style);
        set(buf, rr, y, g[5], style);
    }
    if let Some((text, ts)) = title {
        title_at(buf, r, t, text, ts);
    }
}

/// Centered " title " on row `y` of rect `r`.
pub fn title_at(buf: &mut Buffer, r: Rect, y: u16, text: &str, style: Style) {
    if text.is_empty() || r.width < 6 {
        return;
    }
    let max = (r.width - 4) as usize;
    let t = gm_text::truncate_right(&format!(" {text} "), max);
    let w = str_width(&t) as u16;
    let x = r.x + (r.width - w) / 2;
    put(buf, x, y, &t, style, max);
}

/// Horizontal separator inside a box: `╟────╢` (double box) or `├────┤`.
pub fn separator(buf: &mut Buffer, x: u16, y: u16, width: u16, double_box: bool, style: Style) {
    if width < 2 {
        return;
    }
    let (l, r) = if double_box { ('╟', '╢') } else { ('├', '┤') };
    set(buf, x, y, l, style);
    for i in 1..width - 1 {
        set(buf, x + i, y, '─', style);
    }
    set(buf, x + width - 1, y, r, style);
}

/// Far's shadow: two columns to the right and one row below become dark.
pub fn shadow(buf: &mut Buffer, r: Rect) {
    let dark = Style::default().fg(Color::DarkGray).bg(Color::Black);
    let a = buf.area;
    let mut darken = |x: u16, y: u16| {
        if x < a.right() && y < a.bottom() {
            let cell = &mut buf[(x, y)];
            cell.set_style(dark);
        }
    };
    for y in r.top() + 1..=r.bottom() {
        darken(r.right(), y);
        darken(r.right() + 1, y);
    }
    for x in r.left() + 2..r.right() + 2 {
        darken(x, r.bottom());
    }
}

/// Text with an `&` hotkey: the marked char is drawn with `hl`.
/// Returns cells written.
pub fn hotkey_text(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style, hl: Style, max: usize) -> usize {
    let mut col = 0;
    let mut chars = text.chars().peekable();
    let mut marked = false;
    while let Some(c) = chars.next() {
        if col >= max {
            break;
        }
        let (ch, st) = if c == '&' && !marked {
            match chars.peek() {
                Some('&') => {
                    chars.next();
                    ('&', style)
                }
                Some(&n) => {
                    chars.next();
                    marked = true;
                    (n, hl)
                }
                None => break,
            }
        } else {
            (c, style)
        };
        col += put(buf, x + col as u16, y, &ch.to_string(), st, max - col);
    }
    col
}

/// The hotkey character of an `&`-marked label, lowercased.
pub fn hotkey_of(text: &str) -> Option<char> {
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if c == '&' {
            match it.next() {
                Some('&') => continue,
                Some(n) => return n.to_lowercase().next(),
                None => return None,
            }
        }
    }
    None
}

/// Display width of a label without its `&` marker.
pub fn label_width(text: &str) -> usize {
    str_width(&gm_i18n::strip_hotkey(text))
}

/// Vertical scroll bar (`▲░█▼`) at column `x`.
pub fn scrollbar(buf: &mut Buffer, x: u16, y: u16, height: u16, pos: usize, total: usize, visible: usize, style: Style) {
    if height < 3 || total <= visible {
        return;
    }
    set(buf, x, y, '▲', style);
    set(buf, x, y + height - 1, '▼', style);
    let track = (height - 2) as usize;
    let thumb = ((pos as u64 * (track.saturating_sub(1)) as u64) / (total - visible).max(1) as u64) as usize;
    for i in 0..track {
        set(buf, x, y + 1 + i as u16, if i == thumb.min(track - 1) { '█' } else { '░' }, style);
    }
}

/// Centers a `w`×`h` rect on the screen.
pub fn centered(screen: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(screen.width);
    let h = h.min(screen.height);
    Rect::new(screen.x + (screen.width - w) / 2, screen.y + (screen.height - h) / 2, w, h)
}

/// Thousands-separated number, as in Far's size columns.
pub fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Compact size for narrow columns: `1234567` → `1206 K`.
pub fn short_size(n: u64, width: usize) -> String {
    let full = group_digits(n);
    if full.len() <= width {
        return full;
    }
    let plain = n.to_string();
    if plain.len() <= width {
        return plain;
    }
    let units = ["K", "M", "G", "T", "P"];
    let mut v = n;
    for u in units {
        v /= 1024;
        let s = format!("{v} {u}");
        if s.len() <= width {
            return s;
        }
    }
    format!("{v}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(group_digits(1234567), "1 234 567");
        assert_eq!(short_size(123_456_789, 7), "117 M");
        assert_eq!(hotkey_of("&Copy"), Some('c'));
        assert_eq!(hotkey_of("A && B"), None);
        assert_eq!(label_width("&Copy"), 4);
    }

    #[test]
    fn put_wide_and_clip() {
        let mut b = Buffer::empty(Rect::new(0, 0, 5, 1));
        let n = put(&mut b, 0, 0, "a日本語", Style::default(), 5);
        assert_eq!(n, 5);
        assert_eq!(b[(1, 0)].symbol(), "日");
        let n = put(&mut b, 3, 0, "xyz", Style::default(), 10);
        assert_eq!(n, 2);
    }
}

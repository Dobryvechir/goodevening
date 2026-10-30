//! Display width of strings in terminal cells (replaces `char_width.cpp`).

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Width of a single char; control characters count as one cell
/// because they are shown as replacement glyphs.
pub fn char_width(c: char) -> usize {
    if c.is_control() { 1 } else { c.width().unwrap_or(1) }
}

pub fn str_width(s: &str) -> usize {
    if s.chars().any(char::is_control) { s.chars().map(char_width).sum() } else { s.width() }
}

/// Longest prefix of `s` fitting in `max` cells, and its width.
pub fn prefix_by_width(s: &str, max: usize) -> (&str, usize) {
    let mut w = 0;
    for (i, g) in s.grapheme_indices(true) {
        let gw = str_width(g);
        if w + gw > max {
            return (&s[..i], w);
        }
        w += gw;
    }
    (s, w)
}

/// Pads or truncates `s` to exactly `width` cells.
pub fn fit(s: &str, width: usize) -> String {
    let (p, w) = prefix_by_width(s, width);
    let mut out = String::with_capacity(p.len() + width - w);
    out.push_str(p);
    out.extend(std::iter::repeat_n(' ', width - w));
    out
}

/// Truncates on the right with `…` if wider than `width`.
pub fn truncate_right(s: &str, width: usize) -> String {
    if str_width(s) <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let (p, _) = prefix_by_width(s, width - 1);
    format!("{p}…")
}

/// Truncates on the left with `…` (used for long paths, like Far's `TruncPathStr`).
pub fn truncate_left(s: &str, width: usize) -> String {
    let total = str_width(s);
    if total <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut w = 0;
    let mut start = s.len();
    for (i, g) in s.grapheme_indices(true).rev() {
        let gw = str_width(g);
        if w + gw > width - 1 {
            break;
        }
        w += gw;
        start = i;
    }
    format!("…{}", &s[start..])
}

/// Centers `s` in `width` cells.
pub fn center(s: &str, width: usize) -> String {
    let w = str_width(s).min(width);
    let left = (width - w) / 2;
    let mut out = " ".repeat(left);
    out.push_str(&fit(s, width - left));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(str_width("abc"), 3);
        assert_eq!(str_width("日本"), 4);
        assert_eq!(fit("日本語", 5), "日本 ");
        assert_eq!(truncate_right("abcdef", 4), "abc…");
        assert_eq!(truncate_left("/very/long/path", 6), "…/path");
        assert_eq!(center("ab", 6), "  ab  ");
    }
}

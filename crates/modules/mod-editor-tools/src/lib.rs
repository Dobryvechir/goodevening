//! Editor tools (replaces the `editcase`, `brackets`, `autowrap`, `align`
//! and `drawline` plugins), shown in the editor's F11 menu.
#![forbid(unsafe_code)]

use gm_core::Module;
use gm_core::gm_ui::Ui;
use gm_core::gm_ui::dialog::DialogBuilder;
use gm_editor::{Editor, EditorTool};

#[derive(Clone, Copy)]
enum Case {
    Lower,
    Upper,
    Title,
    Toggle,
}

fn convert(s: &str, c: Case) -> String {
    match c {
        Case::Lower => s.to_lowercase(),
        Case::Upper => s.to_uppercase(),
        Case::Toggle => {
            s.chars().map(|ch| if ch.is_uppercase() { ch.to_lowercase().collect::<String>() } else { ch.to_uppercase().collect() }).collect()
        }
        Case::Title => {
            let mut out = String::with_capacity(s.len());
            let mut start = true;
            for ch in s.chars() {
                if ch.is_alphanumeric() {
                    if start {
                        out.extend(ch.to_uppercase());
                    } else {
                        out.extend(ch.to_lowercase());
                    }
                    start = false;
                } else {
                    out.push(ch);
                    start = true;
                }
            }
            out
        }
    }
}

struct CaseTool(Case, &'static str);

impl EditorTool for CaseTool {
    fn name(&self) -> String {
        self.1.into()
    }
    fn run(&self, ed: &mut Editor, _ui: &mut Ui) {
        if let Some(t) = ed.selected_text() {
            let (l, c) = ed.cursor();
            ed.replace_selection(&convert(&t, self.0));
            ed.set_cursor(l, c);
        } else {
            // No block: the word under the cursor.
            let (l, c) = ed.cursor();
            let line: Vec<char> = ed.line_text(l).chars().collect();
            let is_w = |ch: char| ch.is_alphanumeric() || ch == '_';
            let mut a = c.min(line.len());
            while a > 0 && is_w(line[a - 1]) {
                a -= 1;
            }
            let mut b = c.min(line.len());
            while b < line.len() && is_w(line[b]) {
                b += 1;
            }
            let word: String = line[a..b].iter().collect();
            let new: String = line[..a].iter().collect::<String>() + &convert(&word, self.0) + &line[b..].iter().collect::<String>();
            ed.set_line_text(l, &new);
            ed.set_cursor(l, c);
        }
    }
}

struct Brackets;

impl EditorTool for Brackets {
    fn name(&self) -> String {
        "Go to matching &bracket".into()
    }
    fn run(&self, ed: &mut Editor, _ui: &mut Ui) {
        let (l, c) = ed.cursor();
        if let Some((nl, nc)) = matching(ed, l, c) {
            ed.set_cursor(nl, nc);
        }
    }
}

/// Finds the bracket matching the one at (or just before) the cursor.
pub fn matching(ed: &Editor, line: usize, col: usize) -> Option<(usize, usize)> {
    const PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];
    let text: Vec<char> = ed.line_text(line).chars().collect();
    let (col, ch) = [col, col.wrapping_sub(1)]
        .into_iter()
        .find_map(|c| text.get(c).map(|&ch| (c, ch)).filter(|(_, ch)| PAIRS.iter().any(|(a, b)| a == ch || b == ch)))?;
    let (open, close, forward) = PAIRS.iter().find_map(|&(a, b)| {
        if ch == a {
            Some((a, b, true))
        } else if ch == b {
            Some((a, b, false))
        } else {
            None
        }
    })?;
    let mut depth = 0i32;
    let total = ed.line_count();
    let mut l = line;
    let mut c = col as isize;
    loop {
        let t: Vec<char> = ed.line_text(l).chars().collect();
        while c >= 0 && (c as usize) < t.len() {
            let x = t[c as usize];
            if x == open {
                depth += if forward { 1 } else { -1 };
            } else if x == close {
                depth += if forward { -1 } else { 1 };
            }
            if depth == 0 {
                return Some((l, c as usize));
            }
            c += if forward { 1 } else { -1 };
        }
        if forward {
            l += 1;
            if l >= total {
                return None;
            }
            c = 0;
        } else {
            if l == 0 {
                return None;
            }
            l -= 1;
            c = ed.line_text(l).chars().count() as isize - 1;
        }
    }
}

struct Wrap;

impl EditorTool for Wrap {
    fn name(&self) -> String {
        "&Wrap paragraph to width…".into()
    }
    fn run(&self, ed: &mut Editor, ui: &mut Ui) {
        let mut b = DialogBuilder::new("Wrap", 40);
        b.text("Right margin:");
        let e = b.edit("72", Some("WrapWidth"));
        b.separator();
        b.ok_cancel();
        let mut d = b.build();
        if d.run(ui).is_none() {
            return;
        }
        let width: usize = d.text(e).trim().parse().unwrap_or(72).max(10);
        let (first, last) = ed.selection_lines();
        let (first, last) = if first == last { paragraph(ed, first) } else { (first, last) };
        let indent: String = ed.line_text(first).chars().take_while(|c| c.is_whitespace()).collect();
        let words: Vec<String> = (first..=last).flat_map(|i| ed.line_text(i).split_whitespace().map(String::from).collect::<Vec<_>>()).collect();
        let lines = wrap_words(&words, width, &indent);
        let eol = ed.buf.eol.as_str().to_string();
        let a = ed.buf.char_idx(first, 0);
        let b_idx = ed.buf.char_idx(last, usize::MAX);
        let (l, c) = ed.cursor();
        ed.buf.replace(a, b_idx, &lines.join(&eol), (l, c));
        ed.set_cursor(first, 0);
    }
}

fn paragraph(ed: &Editor, line: usize) -> (usize, usize) {
    let blank = |i: usize| ed.line_text(i).trim().is_empty();
    let mut a = line;
    while a > 0 && !blank(a - 1) {
        a -= 1;
    }
    let mut b = line;
    while b + 1 < ed.line_count() && !blank(b + 1) {
        b += 1;
    }
    (a, b)
}

pub fn wrap_words(words: &[String], width: usize, indent: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = indent.to_string();
    for w in words {
        if cur.trim().is_empty() {
            cur.push_str(w);
        } else if gm_core::gm_text::str_width(&cur) + 1 + gm_core::gm_text::str_width(w) > width {
            out.push(std::mem::replace(&mut cur, format!("{indent}{w}")));
        } else {
            cur.push(' ');
            cur.push_str(w);
        }
    }
    if !cur.trim().is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

struct AlignRight;

impl EditorTool for AlignRight {
    fn name(&self) -> String {
        "&Align lines (left / center / right)…".into()
    }
    fn run(&self, ed: &mut Editor, ui: &mut Ui) {
        let mut b = DialogBuilder::new("Align", 44);
        b.text("Width:");
        let e = b.edit("72", Some("AlignWidth"));
        let l = b.radio("&Left", true, true);
        let c = b.radio("&Center", false, false);
        let r = b.radio("&Right", false, false);
        b.separator();
        b.ok_cancel();
        let mut d = b.build();
        if d.run(ui).is_none() {
            return;
        }
        let width: usize = d.text(e).trim().parse().unwrap_or(72);
        let _ = l;
        let (first, last) = ed.selection_lines();
        for i in first..=last {
            let t = ed.line_text(i).trim().to_string();
            let w = gm_core::gm_text::str_width(&t);
            let pad = width.saturating_sub(w);
            let new = if d.checked(r) {
                format!("{}{t}", " ".repeat(pad))
            } else if d.checked(c) {
                format!("{}{t}", " ".repeat(pad / 2))
            } else {
                t
            };
            ed.set_line_text(i, &new);
        }
    }
}

struct Frame;

impl EditorTool for Frame {
    fn name(&self) -> String {
        "Draw a &frame around the lines".into()
    }
    fn run(&self, ed: &mut Editor, _ui: &mut Ui) {
        let (first, last) = ed.selection_lines();
        let lines: Vec<String> = (first..=last).map(|i| ed.line_text(i)).collect();
        let w = lines.iter().map(|l| gm_core::gm_text::str_width(l)).max().unwrap_or(0);
        let mut out = vec![format!("┌{}┐", "─".repeat(w + 2))];
        for l in &lines {
            out.push(format!("│ {} │", gm_core::gm_text::fit(l, w)));
        }
        out.push(format!("└{}┘", "─".repeat(w + 2)));
        let eol = ed.buf.eol.as_str().to_string();
        let a = ed.buf.char_idx(first, 0);
        let b = ed.buf.char_idx(last, usize::MAX);
        let (l, c) = ed.cursor();
        ed.buf.replace(a, b, &out.join(&eol), (l, c));
        ed.set_cursor(first, 0);
    }
}

#[derive(Default)]
pub struct EditorTools;

impl Module for EditorTools {
    fn id(&self) -> &'static str {
        "editor_tools"
    }
    fn title(&self) -> String {
        "Editor tools".into()
    }
    fn editor_tools(&self) -> Vec<Box<dyn EditorTool>> {
        vec![
            Box::new(CaseTool(Case::Lower, "&Lower case")),
            Box::new(CaseTool(Case::Upper, "&Upper case")),
            Box::new(CaseTool(Case::Title, "&Title case")),
            Box::new(CaseTool(Case::Toggle, "Tog&gle case")),
            Box::new(Brackets),
            Box::new(Wrap),
            Box::new(AlignRight),
            Box::new(Frame),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases_and_wrap() {
        assert_eq!(convert("hello wORLD", Case::Title), "Hello World");
        assert_eq!(convert("aBc", Case::Toggle), "AbC");
        let words: Vec<String> = "one two three four".split(' ').map(String::from).collect();
        assert_eq!(wrap_words(&words, 9, ""), ["one two", "three", "four"]);
    }

    #[test]
    fn bracket_matching() {
        let p = std::env::temp_dir().join(format!("gm-br-{}.txt", std::process::id()));
        std::fs::write(&p, "f(a, [b],\n  {c})\n").unwrap();
        let ed = Editor::open(&p, gm_editor::EditorOptions::default(), std::rc::Rc::new(Vec::new())).unwrap();
        assert_eq!(matching(&ed, 0, 1), Some((1, 5)));
        assert_eq!(matching(&ed, 1, 5), Some((0, 1)));
        assert_eq!(matching(&ed, 0, 5), Some((0, 7)));
        std::fs::remove_file(&p).unwrap();
    }
}

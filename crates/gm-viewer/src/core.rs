//! Viewer engine: data source, line/row layout, rendering and search.

use gm_platform::Mmap;
use gm_term::{Buffer, PaletteId, Rect, Style, style};
use gm_text::Codepage;
use gm_text::codepage;
use gm_text::width::char_width;
use gm_ui::draw;
use gm_ui::search::{SearchState, parse_hex};
use regex::bytes::{Regex, RegexBuilder};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Longest line scanned before it is split (protects against binary files).
const MAX_LINE: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Text,
    Hex,
    Dump,
}

enum Data {
    Map(Mmap),
    Bytes(Vec<u8>),
}

impl Data {
    fn bytes(&self) -> &[u8] {
        match self {
            Data::Map(m) => m,
            Data::Bytes(b) => b,
        }
    }
}

struct Row {
    end: u64,
    /// (char, byte offset)
    chars: Vec<(char, u64)>,
}

pub struct ViewerCore {
    pub path: PathBuf,
    data: Data,
    file_len_seen: u64,
    pub cp: Codepage,
    table: Option<Box<[char; 256]>>,
    bom: u64,
    pub top: u64,
    pub left: usize,
    pub mode: Mode,
    pub wrap: bool,
    pub word_wrap: bool,
    pub tab: usize,
    pub found: Option<(u64, u64)>,
    /// Mouse selection: (anchor, caret) byte offsets.
    pub sel: Option<(u64, u64)>,
    /// Where the content was last rendered (for mouse hit-testing).
    view: Rect,
    pub search: SearchState,
    pub bookmarks: [Option<u64>; 10],
    pub at_end: bool,
    /// Follow growing files (like `tail -f`).
    pub tail: bool,
    pub hex_upper: bool,
    height: usize,
    width: usize,
}

impl ViewerCore {
    pub fn open(path: &Path, detect: bool, default_cp: Codepage) -> io::Result<ViewerCore> {
        let data = load(path)?;
        let mut v = ViewerCore::from_data(path.to_path_buf(), data, detect, default_cp);
        v.file_len_seen = v.len();
        Ok(v)
    }

    /// Views in-memory content (archive members, process details).
    pub fn from_bytes(name: PathBuf, bytes: Vec<u8>, detect: bool, default_cp: Codepage) -> ViewerCore {
        ViewerCore::from_data(name, Data::Bytes(bytes), detect, default_cp)
    }

    fn from_data(path: PathBuf, data: Data, detect: bool, default_cp: Codepage) -> ViewerCore {
        let sample = &data.bytes()[..data.bytes().len().min(64 * 1024)];
        let (cp, bom) = match codepage::detect_bom(sample) {
            Some((cp, n)) => (cp, n as u64),
            None if detect => (codepage::detect(sample), 0),
            None => (default_cp, 0),
        };
        let mut v = ViewerCore {
            path,
            data,
            file_len_seen: 0,
            cp,
            table: None,
            bom,
            top: bom,
            left: 0,
            mode: Mode::Text,
            wrap: true,
            word_wrap: false,
            tab: 8,
            found: None,
            sel: None,
            view: Rect::new(0, 0, 0, 0),
            search: SearchState::default(),
            bookmarks: [None; 10],
            at_end: false,
            tail: false,
            hex_upper: true,
            height: 20,
            width: 80,
        };
        v.set_codepage(cp);
        v
    }

    pub fn len(&self) -> u64 {
        self.data.bytes().len() as u64
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn set_codepage(&mut self, cp: Codepage) {
        self.cp = cp;
        self.table = single_byte_table(cp);
        let unit = cp.unit() as u64;
        if self.top % unit != self.bom % unit {
            self.top -= self.top % unit;
        }
    }

    fn unit(&self) -> u64 {
        self.cp.unit() as u64
    }

    /// Re-reads the file if it changed size; returns true if it did.
    pub fn refresh(&mut self) -> bool {
        let Ok(meta) = std::fs::metadata(&self.path) else { return false };
        if meta.len() == self.file_len_seen {
            return false;
        }
        if let Ok(d) = load(&self.path) {
            self.data = d;
            self.file_len_seen = self.len();
            if self.top > self.len() {
                self.top = self.bom;
            }
            if self.tail {
                self.go_end();
            }
            return true;
        }
        false
    }

    // ----- decoding -----

    fn unit_at(&self, off: u64) -> Option<u32> {
        let b = self.data.bytes();
        let o = off as usize;
        match self.cp {
            Codepage::Utf16Le => Some(u32::from(u16::from_le_bytes([*b.get(o)?, *b.get(o + 1)?]))),
            Codepage::Utf16Be => Some(u32::from(u16::from_be_bytes([*b.get(o)?, *b.get(o + 1)?]))),
            _ => b.get(o).map(|&x| u32::from(x)),
        }
    }

    /// Decodes one char at `off`: (char, byte length).
    fn char_at(&self, off: u64) -> Option<(char, u64)> {
        let b = self.data.bytes();
        let o = off as usize;
        if o >= b.len() {
            return None;
        }
        match self.cp {
            Codepage::Utf8 => {
                let first = b[o];
                let n = match first {
                    0x00..=0x7F => 1,
                    0xC0..=0xDF => 2,
                    0xE0..=0xEF => 3,
                    0xF0..=0xF7 => 4,
                    _ => return Some(('\u{FFFD}', 1)),
                };
                let end = (o + n).min(b.len());
                match std::str::from_utf8(&b[o..end]) {
                    Ok(s) => s.chars().next().map(|c| (c, n as u64)),
                    Err(_) => Some(('\u{FFFD}', 1)),
                }
            }
            Codepage::Utf16Le | Codepage::Utf16Be => {
                let u = self.unit_at(off)? as u16;
                if (0xD800..0xDC00).contains(&u) {
                    if let Some(l) = self.unit_at(off + 2) {
                        let l = l as u16;
                        if let Some(Ok(c)) = char::decode_utf16([u, l]).next() {
                            return Some((c, 4));
                        }
                    }
                    return Some(('\u{FFFD}', 2));
                }
                Some((char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}'), 2))
            }
            Codepage::Web(e) if self.table.is_none() => {
                // Multi-byte legacy encodings: decode a small window.
                let end = (o + 4).min(b.len());
                let (s, _) = e.decode_without_bom_handling(&b[o..end]);
                let c = s.chars().next().unwrap_or('\u{FFFD}');
                let n = e.encode(&c.to_string()).0.len().clamp(1, end - o);
                Some((c, n as u64))
            }
            _ => {
                let t = self.table.as_ref()?;
                Some((t[b[o] as usize], 1))
            }
        }
    }

    fn is_cr(&self, off: u64) -> bool {
        self.unit_at(off) == Some(13)
    }

    fn is_lf(&self, off: u64) -> bool {
        self.unit_at(off) == Some(10)
    }

    /// Offset just after the line break that ends the line at/after `off`,
    /// and the end of the line content.
    fn line_end(&self, off: u64) -> (u64, u64) {
        let u = self.unit();
        let len = self.len();
        let limit = (off + MAX_LINE).min(len);
        let mut p = off;
        while p < limit {
            if self.is_lf(p) {
                return (p, p + u);
            }
            if self.is_cr(p) {
                return if self.is_lf(p + u) { (p, p + 2 * u) } else { (p, p + u) };
            }
            p += u;
        }
        (p, p)
    }

    /// Start of the line containing byte `p`.
    fn line_start(&self, p: u64) -> u64 {
        let u = self.unit();
        let floor = p.saturating_sub(MAX_LINE).max(self.bom);
        let mut q = p;
        while q > floor {
            let prev = q - u;
            if self.is_lf(prev) || (self.is_cr(prev) && !self.is_lf(q)) {
                return q;
            }
            q = prev;
        }
        q
    }

    fn row_at(&self, off: u64, width: usize) -> Row {
        let (content_end, next) = self.line_end(off);
        let mut chars = Vec::new();
        let mut p = off;
        let mut col = 0usize;
        while p < content_end {
            let Some((c, n)) = self.char_at(p) else { break };
            let w = if c == '\t' { self.tab - col % self.tab.max(1) } else { char_width(c) };
            if self.wrap && col + w > width && !chars.is_empty() {
                // Word wrap: break after the last space if there is one.
                if self.word_wrap
                    && let Some(i) = chars.iter().rposition(|&(ch, _): &(char, u64)| ch == ' ')
                    && i + 1 < chars.len()
                {
                    let end = chars[i + 1].1;
                    chars.truncate(i + 1);
                    return Row { end, chars };
                }
                return Row { end: p, chars };
            }
            if c == '\t' {
                for _ in 0..w {
                    chars.push((' ', p));
                }
            } else {
                chars.push((c, p));
            }
            col += w;
            p += n;
        }
        Row { end: if p >= content_end { next } else { p }, chars }
    }

    fn next_row(&self, off: u64) -> u64 {
        match self.mode {
            Mode::Text => {
                let r = self.row_at(off, self.width);
                if r.end <= off { self.len() } else { r.end }
            }
            Mode::Hex => off + 16,
            Mode::Dump => off + self.width as u64,
        }
    }

    fn prev_row(&self, off: u64) -> u64 {
        if off <= self.bom {
            return self.bom;
        }
        match self.mode {
            Mode::Hex => off.saturating_sub(16),
            Mode::Dump => off.saturating_sub(self.width as u64),
            Mode::Text => {
                let ls = self.line_start(off - self.unit());
                if !self.wrap {
                    return ls;
                }
                let mut s = ls;
                loop {
                    let r = self.row_at(s, self.width);
                    if r.end >= off || r.end <= s {
                        return s;
                    }
                    s = r.end;
                }
            }
        }
    }

    // ----- navigation -----

    pub fn down(&mut self, n: usize) {
        for _ in 0..n {
            if self.at_end {
                break;
            }
            let nt = self.next_row(self.top);
            if nt >= self.len() {
                break;
            }
            self.top = nt;
            self.at_end = self.eof_visible();
        }
    }

    pub fn up(&mut self, n: usize) {
        for _ in 0..n {
            self.top = self.prev_row(self.top);
        }
        self.at_end = self.eof_visible();
    }

    fn eof_visible(&self) -> bool {
        let mut off = self.top;
        for _ in 0..self.height {
            if off >= self.len() {
                return true;
            }
            off = self.next_row(off);
        }
        off >= self.len()
    }

    pub fn go_home(&mut self) {
        self.top = self.bom;
        self.left = 0;
        self.at_end = self.eof_visible();
    }

    pub fn go_end(&mut self) {
        self.top = match self.mode {
            Mode::Hex => self.len().saturating_sub(1) & !15,
            _ => self.len(),
        };
        self.up(self.height.saturating_sub(1).max(1));
        if self.mode == Mode::Text && self.top < self.len() && !self.is_empty() {
            // `up` from EOF lands on the last row; make the last row the bottom one.
        }
        self.at_end = true;
    }

    pub fn page_down(&mut self) {
        self.down(self.height.saturating_sub(1).max(1));
    }

    pub fn page_up(&mut self) {
        self.up(self.height.saturating_sub(1).max(1));
    }

    /// Moves to an absolute byte offset (aligned to a row start).
    pub fn goto_offset(&mut self, off: u64) {
        let off = off.min(self.len());
        self.top = match self.mode {
            Mode::Hex => off & !15,
            Mode::Dump => off - off % self.width.max(1) as u64,
            Mode::Text => self.line_start(off - off % self.unit()),
        };
        self.at_end = self.eof_visible();
    }

    pub fn percent(&self) -> u64 {
        if self.is_empty() || self.at_end { 100 } else { self.top * 100 / self.len() }
    }

    pub fn toggle_hex(&mut self) {
        self.mode = if self.mode == Mode::Hex { Mode::Text } else { Mode::Hex };
        let t = self.top;
        self.goto_offset(t);
    }

    pub fn cycle_mode(&mut self) {
        self.mode = match self.mode {
            Mode::Text => Mode::Hex,
            Mode::Hex => Mode::Dump,
            Mode::Dump => Mode::Text,
        };
        let t = self.top;
        self.goto_offset(t);
    }

    // ----- rendering -----

    /// Renders the content into `r`.
    pub fn render(&mut self, buf: &mut Buffer, r: Rect, show_arrows: bool) {
        self.height = r.height as usize;
        self.width = r.width as usize;
        self.view = r;
        let text = style(PaletteId::ViewerText);
        let sel = style(PaletteId::ViewerTextSelected);
        let arrows = style(PaletteId::ViewerArrows);
        draw::fill(buf, r, ' ', text);
        match self.mode {
            Mode::Text => self.render_text(buf, r, text, sel, arrows, show_arrows),
            Mode::Hex => self.render_hex(buf, r, text, sel),
            Mode::Dump => self.render_dump(buf, r, text, sel),
        }
        self.at_end = self.eof_visible();
    }

    fn in_found(&self, off: u64) -> bool {
        self.found.is_some_and(|(a, b)| off >= a && off < b) || self.selection().is_some_and(|(a, b)| off >= a && off < b)
    }

    // ----- mouse selection -----

    /// The selected byte range `a..b`, if not empty.
    pub fn selection(&self) -> Option<(u64, u64)> {
        let (a, b) = self.sel?;
        let (a, b) = (a.min(b), a.max(b).min(self.len()));
        (a < b).then_some((a, b))
    }

    /// The selected text (the mouse selection, else the search result),
    /// decoded with the current code page.
    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection().or(self.found.filter(|&(a, b)| a < b && b <= self.len()))?;
        Some(self.cp.decode(&self.data.bytes()[a as usize..b as usize]).into_owned())
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

    /// Byte offset of the character at screen cell (x, y) in text mode
    /// (past the end of a row: the end of its text). `y` outside the
    /// content area is clamped to the first/last row. Only the visible
    /// rows are laid out, as for rendering.
    pub fn offset_at(&self, x: u16, y: u16) -> Option<u64> {
        let r = self.view;
        if self.mode != Mode::Text || r.width == 0 || r.height == 0 {
            return None;
        }
        let row = y.clamp(r.y, r.bottom() - 1) - r.y;
        let target = if self.wrap { 0 } else { self.left } + x.saturating_sub(r.x) as usize;
        let len = self.len();
        let mut off = self.top;
        for _ in 0..row {
            let next = self.row_at(off, self.width).end;
            if off >= len || next <= off {
                return Some(len);
            }
            off = next;
        }
        if off >= len {
            return Some(len);
        }
        let rr = self.row_at(off, self.width);
        let mut col = 0;
        for &(c, o) in &rr.chars {
            col += char_width(c).max(1);
            if target < col {
                return Some(o);
            }
        }
        Some(match rr.chars.last() {
            Some(&(_, o)) => o + self.char_at(o).map_or(1, |(_, n)| n),
            None => off,
        })
    }

    fn render_text(&self, buf: &mut Buffer, r: Rect, text: Style, sel: Style, arrows: Style, show_arrows: bool) {
        let mut off = self.top;
        for row in 0..r.height {
            if off >= self.len() {
                break;
            }
            let rr = self.row_at(off, r.width as usize);
            let y = r.y + row;
            let skip = if self.wrap { 0 } else { self.left };
            let mut col = 0usize;
            let mut x = 0usize;
            for &(c, o) in &rr.chars {
                let w = char_width(c).max(1);
                if col < skip {
                    col += w;
                    continue;
                }
                if x + w > r.width as usize {
                    break;
                }
                let st = if self.in_found(o) { sel } else { text };
                let ch = if c.is_control() { draw::control_glyph(c) } else { c };
                draw::put(buf, r.x + x as u16, y, &ch.to_string(), st, w);
                x += w;
                col += w;
            }
            if show_arrows && !self.wrap {
                if skip > 0 && !rr.chars.is_empty() {
                    draw::set(buf, r.x, y, '«', arrows);
                }
                let total: usize = rr.chars.iter().map(|&(c, _)| char_width(c).max(1)).sum();
                if total > skip + r.width as usize {
                    draw::set(buf, r.right() - 1, y, '»', arrows);
                }
            }
            let next = rr.end;
            if next <= off {
                break;
            }
            off = next;
        }
    }

    fn render_hex(&self, buf: &mut Buffer, r: Rect, text: Style, sel: Style) {
        let b = self.data.bytes();
        let mut off = self.top & !15;
        for row in 0..r.height {
            if off as usize >= b.len() {
                break;
            }
            let y = r.y + row;
            let head = if self.hex_upper { format!("{off:010X}: ") } else { format!("{off:010x}: ") };
            let mut x = r.x + draw::put(buf, r.x, y, &head, text, r.width as usize) as u16;
            for i in 0..16u64 {
                let o = off + i;
                if i == 8 {
                    x += draw::put(buf, x, y, "│ ", text, 2) as u16;
                }
                let s = match b.get(o as usize) {
                    Some(v) if self.hex_upper => format!("{v:02X} "),
                    Some(v) => format!("{v:02x} "),
                    None => "   ".into(),
                };
                let st = if self.in_found(o) { sel } else { text };
                x += draw::put(buf, x, y, &s, st, 3) as u16;
            }
            x += 1;
            for i in 0..16u64 {
                let o = off + i;
                let Some(&v) = b.get(o as usize) else { break };
                let c = self.table.as_ref().map(|t| t[v as usize]).unwrap_or(if v.is_ascii_graphic() { v as char } else { '.' });
                let c = if c.is_control() { draw::control_glyph(c) } else { c };
                let st = if self.in_found(o) { sel } else { text };
                if x >= r.right() {
                    break;
                }
                x += draw::put(buf, x, y, &c.to_string(), st, 1).max(1) as u16;
            }
            off += 16;
        }
    }

    fn render_dump(&self, buf: &mut Buffer, r: Rect, text: Style, sel: Style) {
        let w = r.width as u64;
        let mut off = self.top;
        for row in 0..r.height {
            let y = r.y + row;
            let mut x = 0u16;
            let mut p = off;
            while (x as u64) < w && p < self.len() {
                let Some((c, n)) = self.char_at(p) else { break };
                let c = if c.is_control() { draw::control_glyph(c) } else { c };
                let st = if self.in_found(p) { sel } else { text };
                x += draw::put(buf, r.x + x, y, &c.to_string(), st, (w - x as u64) as usize).max(1) as u16;
                p += n;
            }
            if p >= self.len() {
                break;
            }
            off = p;
        }
    }

    // ----- search -----

    fn build_regex(&self) -> Result<Regex, String> {
        let st = &self.search;
        if st.hex {
            let bytes = parse_hex(&st.spec.text).ok_or("invalid hex string")?;
            let pat: String = bytes.iter().map(|b| format!("\\x{b:02X}")).collect();
            return RegexBuilder::new(&pat).unicode(false).build().map_err(|e| e.to_string());
        }
        let ci = !st.spec.case_sensitive;
        if self.cp == Codepage::Utf8 {
            let mut spec = st.spec.clone();
            if !spec.regex {
                spec.text = regex::escape(&spec.text);
            }
            let (body, flag_ci) = if spec.regex { gm_text::regex_adapter::parse_far_regex(&spec.text) } else { (spec.text.clone(), false) };
            let body = if spec.whole_words { format!(r"\b(?:{body})\b") } else { body };
            return RegexBuilder::new(&body).case_insensitive(ci || flag_ci).multi_line(true).build().map_err(|e| e.to_string());
        }
        if st.spec.regex && self.cp.unit() == 1 {
            // Regex over single-byte text: ASCII semantics.
            let (body, flag_ci) = gm_text::regex_adapter::parse_far_regex(&st.spec.text);
            return RegexBuilder::new(&body).unicode(false).case_insensitive(ci || flag_ci).build().map_err(|e| e.to_string());
        }
        if st.spec.regex {
            return Err("regular expressions are supported for UTF-8 and single-byte code pages".into());
        }
        // Literal text in any code page: per-char alternation of encodings.
        let mut pat = String::new();
        for c in st.spec.text.chars() {
            let mut variants: Vec<String> = vec![c.to_string()];
            if ci {
                variants.push(c.to_lowercase().collect());
                variants.push(c.to_uppercase().collect());
            }
            variants.sort();
            variants.dedup();
            let alts: Vec<String> = variants.iter().map(|v| self.cp.encode(v).iter().map(|b| format!("\\x{b:02X}")).collect()).collect();
            pat.push_str(&format!("(?:{})", alts.join("|")));
        }
        RegexBuilder::new(&pat).unicode(false).build().map_err(|e| e.to_string())
    }

    fn word_ok(&self, a: u64, b: u64) -> bool {
        if !self.search.spec.whole_words || self.cp == Codepage::Utf8 || self.search.hex {
            return true;
        }
        let is_word = |o: Option<u32>| o.is_some_and(|u| char::from_u32(u).is_some_and(|c| c.is_alphanumeric() || c == '_'));
        let before = if a >= self.unit() { self.unit_at(a - self.unit()) } else { None };
        !is_word(before) && !is_word(self.unit_at(b))
    }

    /// Searches forward/backward from the current position. Returns
    /// Ok(true) if found, Ok(false) if not found before the end/start.
    pub fn find(&mut self, backward: bool, wrap_around: bool) -> Result<bool, String> {
        let re = self.build_regex()?;
        let data = self.data.bytes();
        let start = match self.found {
            Some((a, b)) => {
                if backward {
                    a
                } else {
                    b.max(a + 1)
                }
            }
            None => self.top,
        } as usize;
        let hit = if backward {
            let mut best = None;
            let mut window_end = start;
            while window_end > 0 && best.is_none() {
                let ws = window_end.saturating_sub(4 << 20);
                for m in re.find_iter(&data[ws..window_end]) {
                    let (a, b) = ((ws + m.start()) as u64, (ws + m.end()) as u64);
                    if self.word_ok(a, b) && (a % self.unit() == 0) {
                        best = Some((a, b));
                    }
                }
                window_end = ws;
            }
            if best.is_none() && wrap_around {
                re.find_iter(&data[start..]).last().map(|m| ((start + m.start()) as u64, (start + m.end()) as u64))
            } else {
                best
            }
        } else {
            let mut found = None;
            let mut pos = start.min(data.len());
            while let Some(m) = re.find_at(data, pos) {
                let (a, b) = (m.start() as u64, m.end() as u64);
                if self.word_ok(a, b) && a % self.unit() == self.bom % self.unit() {
                    found = Some((a, b));
                    break;
                }
                pos = m.start() + 1;
                if pos >= data.len() {
                    break;
                }
            }
            if found.is_none() && wrap_around {
                found = re.find(&data[..start.min(data.len())]).map(|m| (m.start() as u64, m.end() as u64));
            }
            found
        };
        match hit {
            Some((a, b)) => {
                self.found = Some((a, b.max(a + 1)));
                self.reveal(a);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Scrolls so that byte `off` is visible.
    fn reveal(&mut self, off: u64) {
        // Already on screen?
        let mut o = self.top;
        for _ in 0..self.height {
            let n = self.next_row(o);
            if off >= o && off < n.max(o + 1) {
                self.adjust_left(o, off);
                return;
            }
            if n >= self.len() {
                break;
            }
            o = n;
        }
        self.goto_offset(off);
        if self.mode == Mode::Text && self.wrap {
            // Move down to the row that contains `off`.
            while self.next_row(self.top) <= off && self.next_row(self.top) < self.len() {
                self.top = self.next_row(self.top);
            }
        }
        let t = self.top;
        self.adjust_left(t, off);
    }

    fn adjust_left(&mut self, row_start: u64, off: u64) {
        if self.mode != Mode::Text || self.wrap {
            return;
        }
        let rr = self.row_at(row_start, usize::MAX / 4);
        let col: usize = rr.chars.iter().take_while(|&&(_, o)| o < off).map(|&(c, _)| char_width(c).max(1)).sum();
        if col < self.left || col >= self.left + self.width.saturating_sub(4) {
            self.left = col.saturating_sub(self.width / 3);
        }
    }

    /// The whole content decoded with the current code page (BOM excluded).
    pub fn decoded_text(&self) -> String {
        let b = self.data.bytes();
        self.cp.decode(&b[(self.bom as usize).min(b.len())..]).into_owned()
    }

    /// 0-based number of the line containing byte `off` (counted by LFs).
    pub fn line_of_offset(&self, off: u64) -> usize {
        let u = self.unit();
        let mut n = 0;
        let mut p = self.bom;
        while p < off.min(self.len()) {
            if self.is_lf(p) {
                n += 1;
            }
            p += u;
        }
        n
    }

    /// Byte offset of the start of 0-based line `line`.
    pub fn offset_of_line(&self, line: usize) -> u64 {
        let u = self.unit();
        let mut n = 0;
        let mut p = self.bom;
        while n < line && p < self.len() {
            if self.is_lf(p) {
                n += 1;
            }
            p += u;
        }
        p.min(self.len())
    }

    /// Bytes of the whole content (for "view as text" of providers).
    pub fn bytes(&self) -> &[u8] {
        self.data.bytes()
    }

    /// Text of the first lines (used by the quick view of the info panel).
    pub fn display_name(&self) -> String {
        self.path.display().to_string()
    }
}

fn load(path: &Path) -> io::Result<Data> {
    let file = File::open(path)?;
    let meta = file.metadata()?;
    if meta.is_dir() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "is a directory"));
    }
    if meta.is_file()
        && meta.len() > 0
        && let Ok(m) = gm_platform::map_file(&file)
    {
        return Ok(Data::Map(m));
    }
    // Pipes, /proc files and other special files: read up to 64 MiB.
    let mut v = Vec::new();
    file.take(64 << 20).read_to_end(&mut v)?;
    Ok(Data::Bytes(v))
}

fn single_byte_table(cp: Codepage) -> Option<Box<[char; 256]>> {
    let single = match cp {
        Codepage::Oem(_) => true,
        Codepage::Web(e) => e.is_single_byte(),
        _ => false,
    };
    if !single {
        return None;
    }
    let mut t = Box::new(['\0'; 256]);
    for (i, slot) in t.iter_mut().enumerate() {
        *slot = cp.decode(&[i as u8]).chars().next().unwrap_or('\u{FFFD}');
    }
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core(text: &str) -> ViewerCore {
        let mut v = ViewerCore::from_bytes("t".into(), text.as_bytes().to_vec(), true, Codepage::Utf8);
        v.height = 3;
        v.width = 10;
        v
    }

    #[test]
    fn mouse_selection() {
        let mut v = core("abc\ndefgh\n\tx\n");
        v.view = Rect::new(2, 1, 10, 3);
        assert_eq!(v.offset_at(2, 1), Some(0));
        assert_eq!(v.offset_at(4, 2), Some(6)); // 'f'
        assert_eq!(v.offset_at(9, 1), Some(3)); // past "abc": its end
        assert_eq!(v.offset_at(5, 3), Some(10)); // inside the tab
        assert_eq!(v.offset_at(0, 0), Some(0)); // clamped to the first row
        v.sel = Some((6, 1));
        assert_eq!(v.selected_text().as_deref(), Some("bc\nde"));
        v.sel = Some((3, 3));
        assert_eq!(v.selected_text(), None);
    }

    #[test]
    fn rows_and_scroll() {
        let mut v = core("line1\nline2\r\nline3\nline4\nline5\n");
        v.wrap = false;
        v.down(1);
        assert_eq!(v.top, 6);
        v.down(1);
        assert_eq!(v.top, 13);
        v.up(1);
        assert_eq!(v.top, 6);
        v.go_end();
        assert!(v.at_end);
        v.go_home();
        assert_eq!(v.top, 0);
    }

    #[test]
    fn wrapping() {
        let mut v = core("abcdefghijklmnopqrstuvwxyz\nx\n");
        v.wrap = true;
        assert_eq!(v.next_row(0), 10);
        assert_eq!(v.prev_row(20), 10);
        assert_eq!(v.prev_row(27), 20);
    }

    #[test]
    fn search_text_and_hex() {
        let mut v = core("hello World\nsecond world\n");
        v.search.spec.text = "world".into();
        assert!(v.find(false, false).unwrap());
        assert_eq!(v.found, Some((6, 11)));
        assert!(v.find(false, false).unwrap());
        assert_eq!(v.found, Some((19, 24)));
        assert!(v.find(true, false).unwrap());
        assert_eq!(v.found, Some((6, 11)));
        v.search.hex = true;
        v.search.spec.text = "73 65".into();
        v.found = None;
        assert!(v.find(false, false).unwrap());
        assert_eq!(v.found, Some((12, 14)));
    }

    #[test]
    fn line_offsets() {
        let v = core("ab\ncd\r\nef\n");
        assert_eq!(v.line_of_offset(0), 0);
        assert_eq!(v.line_of_offset(3), 1);
        assert_eq!(v.line_of_offset(7), 2);
        assert_eq!(v.offset_of_line(1), 3);
        assert_eq!(v.offset_of_line(2), 7);
        assert_eq!(v.offset_of_line(9), 10);
        assert_eq!(v.decoded_text().lines().count(), 3);
    }

    #[test]
    fn utf16_search() {
        let bytes = Codepage::Utf16Le.encode("abc ПРИВЕТ def");
        let mut v = ViewerCore::from_bytes("t".into(), bytes, false, Codepage::Utf16Le);
        v.search.spec.text = "привет".into();
        assert!(v.find(false, false).unwrap());
        assert_eq!(v.found, Some((8, 20)));
    }
}

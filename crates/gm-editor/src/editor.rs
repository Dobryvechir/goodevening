//! The editor window.

use crate::buffer::TextBuffer;
use gm_i18n::{MsgId, msg};
use gm_term::{Buffer, Event, KeyCode, Mods, MouseButton, MouseEventKind, PaletteId, Rect, style};
use gm_text::width::char_width;
use gm_text::{Codepage, Eol};
use gm_ui::dialog::DialogBuilder;
use gm_ui::search::{SearchAction, SearchState, search_dialog};
use gm_ui::{KeyBar, KeyBarLabels, MenuItem, MenuResult, OpenRequest, Ui, VMenu, WinAction, Window, WindowKind, clipboard, draw, message};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

/// A built-in editor command (former editor plugins), shown in the F11 menu.
pub trait EditorTool {
    fn name(&self) -> String;
    fn run(&self, ed: &mut Editor, ui: &mut Ui);
}

#[derive(Clone, Debug)]
pub struct EditorOptions {
    pub tab_size: usize,
    pub expand_tabs: bool,
    pub auto_indent: bool,
    pub persistent_blocks: bool,
    pub del_removes_blocks: bool,
    pub line_numbers: bool,
    pub show_whitespace: bool,
    pub keybar: bool,
    pub auto_detect: bool,
    pub default_cp: Codepage,
    pub new_file_eol: Eol,
    pub word_div: String,
}

impl Default for EditorOptions {
    fn default() -> Self {
        EditorOptions {
            tab_size: 8,
            expand_tabs: false,
            auto_indent: false,
            persistent_blocks: false,
            del_removes_blocks: true,
            line_numbers: false,
            show_whitespace: false,
            keybar: true,
            auto_detect: true,
            default_cp: Codepage::Utf8,
            new_file_eol: Eol::native(),
            word_div: "~!%^&*()+|{}:\"<>?`-=\\[];',./".into(),
        }
    }
}

pub struct Editor {
    pub path: PathBuf,
    pub buf: TextBuffer,
    pub opts: EditorOptions,
    pub line: usize,
    pub col: usize,
    want_vis: Option<usize>,
    pub top: usize,
    pub left: usize,
    anchor: Option<(usize, usize)>,
    vertical: bool,
    pub search: SearchState,
    bookmarks: [Option<(usize, usize)>; 10],
    height: usize,
    width: usize,
    mtime: Option<SystemTime>,
    external_asked: bool,
    pub tools: Rc<Vec<Box<dyn EditorTool>>>,
    labels: KeyBarLabels,
    pub is_new: bool,
    pub overtype: bool,
    body: Rect,
    gutter: u16,
    pub readonly: bool,
}

fn labels() -> KeyBarLabels {
    let mut l = KeyBarLabels::default();
    let row = |ids: [MsgId; 12]| -> [String; 12] { ids.map(|i| msg(i).to_string()) };
    l.normal = row([
        MsgId::MEditF1,
        MsgId::MEditF2,
        MsgId::MEditF3,
        MsgId::MEditF4,
        MsgId::MEditF5,
        MsgId::MEditF6,
        MsgId::MEditF7,
        MsgId::MEditF8,
        MsgId::MEditF9,
        MsgId::MEditF10,
        MsgId::MEditF11,
        MsgId::MEditF12,
    ]);
    l.normal[10] = "Tools".into();
    l.shift = row([
        MsgId::MEditShiftF1,
        MsgId::MEditShiftF2,
        MsgId::MEditShiftF3,
        MsgId::MEditShiftF4,
        MsgId::MEditShiftF5,
        MsgId::MEditShiftF6,
        MsgId::MEditShiftF7,
        MsgId::MEditShiftF8,
        MsgId::MEditShiftF9,
        MsgId::MEditShiftF10,
        MsgId::MEditShiftF11,
        MsgId::MEditShiftF12,
    ]);
    l.alt = row([
        MsgId::MEditAltF1,
        MsgId::MEditAltF2,
        MsgId::MEditAltF3,
        MsgId::MEditAltF4,
        MsgId::MEditAltF5,
        MsgId::MEditAltF6,
        MsgId::MEditAltF7,
        MsgId::MEditAltF8,
        MsgId::MEditAltF9,
        MsgId::MEditAltF10,
        MsgId::MEditAltF11,
        MsgId::MEditAltF12,
    ]);
    l.alt[8] = String::new();
    l
}

impl Editor {
    /// Opens a file; a missing file starts a new, empty buffer.
    pub fn open(path: &Path, opts: EditorOptions, tools: Rc<Vec<Box<dyn EditorTool>>>) -> std::io::Result<Editor> {
        let (buf, is_new, mtime, readonly) = if path.exists() {
            let meta = std::fs::metadata(path)?;
            if meta.is_dir() {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, msg(MsgId::MEditCanNotEditDirectory)));
            }
            let b = TextBuffer::load(path, opts.auto_detect, opts.default_cp)?;
            (b, false, meta.modified().ok(), meta.permissions().readonly())
        } else {
            let mut b = TextBuffer::from_str("", opts.new_file_eol);
            b.cp = opts.default_cp;
            (b, true, None, false)
        };
        Ok(Editor {
            path: path.to_path_buf(),
            buf,
            opts,
            line: 0,
            col: 0,
            want_vis: None,
            top: 0,
            left: 0,
            anchor: None,
            vertical: false,
            search: SearchState::default(),
            bookmarks: [None; 10],
            height: 20,
            width: 80,
            mtime,
            external_asked: false,
            tools,
            labels: labels(),
            is_new,
            overtype: false,
            body: Rect::default(),
            gutter: 0,
            readonly,
        })
    }

    // ----- API used by editor tools -----

    pub fn cursor(&self) -> (usize, usize) {
        (self.line, self.col)
    }

    pub fn set_cursor(&mut self, line: usize, col: usize) {
        self.line = line.min(self.buf.line_count().saturating_sub(1));
        self.col = col.min(self.buf.line_len(self.line));
        self.want_vis = None;
    }

    pub fn line_count(&self) -> usize {
        self.buf.line_count()
    }

    pub fn line_text(&self, i: usize) -> String {
        self.buf.line_text(i)
    }

    pub fn begin_group(&mut self) {
        self.buf.new_group();
    }

    pub fn set_line_text(&mut self, i: usize, s: &str) {
        let a = self.buf.char_idx(i, 0);
        let b = a + self.buf.line_len(i);
        self.buf.replace(a, b, s, (self.line, self.col));
        if self.line == i {
            self.col = self.col.min(self.buf.line_len(i));
        }
    }

    pub fn insert_text(&mut self, s: &str) {
        let at = self.buf.char_idx(self.line, self.col);
        let (l, c) = self.buf.replace(at, at, s, (self.line, self.col));
        self.line = l;
        self.col = c;
    }

    /// The selected text (vertical blocks are joined with the file's EOL).
    pub fn selected_text(&self) -> Option<String> {
        if let Some((l1, l2, v1, v2)) = self.block_rect() {
            let mut out = Vec::new();
            for l in l1..=l2 {
                let (a, b) = (self.col_of_vis(l, v1), self.col_of_vis(l, v2));
                let t: String = self.buf.line_text(l).chars().skip(a).take(b - a).collect();
                out.push(t);
            }
            return Some(out.join(self.buf.eol.as_str()));
        }
        let (a, b) = self.sel_range()?;
        Some(self.buf.rope.slice(a..b).to_string())
    }

    /// First and last line touched by the selection, or the cursor line.
    pub fn selection_lines(&self) -> (usize, usize) {
        match self.anchor {
            Some((al, _)) => (al.min(self.line), al.max(self.line)),
            None => (self.line, self.line),
        }
    }

    pub fn has_selection(&self) -> bool {
        self.anchor.is_some_and(|a| a != (self.line, self.col))
    }

    pub fn replace_selection(&mut self, text: &str) {
        self.delete_selection();
        self.insert_text(text);
    }

    pub fn word_at_cursor(&self) -> String {
        let t: Vec<char> = self.buf.line_text(self.line).chars().collect();
        let is_w = |c: char| c.is_alphanumeric() || c == '_';
        let mut a = self.col.min(t.len());
        while a > 0 && is_w(t[a - 1]) {
            a -= 1;
        }
        let mut b = self.col.min(t.len());
        while b < t.len() && is_w(t[b]) {
            b += 1;
        }
        t[a..b].iter().collect()
    }

    // ----- geometry -----

    fn vis_of(&self, line: usize, col: usize) -> usize {
        let mut v = 0;
        for c in self.buf.line_text(line).chars().take(col) {
            v += self.cell_width(c, v);
        }
        v
    }

    fn col_of_vis(&self, line: usize, vis: usize) -> usize {
        let mut v = 0;
        for (i, c) in self.buf.line_text(line).chars().enumerate() {
            let w = self.cell_width(c, v);
            if v + w > vis {
                return i;
            }
            v += w;
        }
        self.buf.line_len(line)
    }

    fn cell_width(&self, c: char, at: usize) -> usize {
        if c == '\t' {
            let t = self.opts.tab_size.max(1);
            t - at % t
        } else {
            char_width(c).max(1)
        }
    }

    // ----- selection -----

    fn sel_range(&self) -> Option<(usize, usize)> {
        if self.vertical {
            return None;
        }
        let (al, ac) = self.anchor?;
        let a = self.buf.char_idx(al, ac);
        let b = self.buf.char_idx(self.line, self.col);
        (a != b).then(|| (a.min(b), a.max(b)))
    }

    fn block_rect(&self) -> Option<(usize, usize, usize, usize)> {
        if !self.vertical {
            return None;
        }
        let (al, ac) = self.anchor?;
        let av = self.vis_of(al, ac);
        let cv = self.want_vis.unwrap_or_else(|| self.vis_of(self.line, self.col));
        let (v1, v2) = (av.min(cv), av.max(cv));
        (v1 != v2).then(|| (al.min(self.line), al.max(self.line), v1, v2))
    }

    fn delete_selection(&mut self) -> bool {
        if let Some((l1, l2, v1, v2)) = self.block_rect() {
            for l in (l1..=l2).rev() {
                let (a, b) = (self.col_of_vis(l, v1), self.col_of_vis(l, v2));
                let s = self.buf.char_idx(l, 0);
                self.buf.replace(s + a, s + b, "", (self.line, self.col));
            }
            self.line = l1;
            self.col = self.col_of_vis(l1, v1);
            self.anchor = None;
            self.vertical = false;
            return true;
        }
        if let Some((a, b)) = self.sel_range() {
            self.buf.replace(a, b, "", (self.line, self.col));
            let (l, c) = self.buf.pos_of(a);
            self.line = l;
            self.col = c;
            self.anchor = None;
            return true;
        }
        self.anchor = None;
        false
    }

    fn select_motion(&mut self, select: bool, vertical: bool) {
        if select {
            if self.anchor.is_none() || self.vertical != vertical {
                self.anchor = Some((self.line, self.col));
                self.vertical = vertical;
            }
        } else if !self.opts.persistent_blocks {
            self.anchor = None;
            self.vertical = false;
        }
    }

    // ----- editing commands -----

    fn type_char(&mut self, c: char) {
        if self.readonly {
            return;
        }
        if self.has_selection() && !self.opts.persistent_blocks {
            self.buf.new_group();
            self.delete_selection();
        }
        let at = self.buf.char_idx(self.line, self.col);
        let end = if self.overtype && self.col < self.buf.line_len(self.line) { at + 1 } else { at };
        let (l, cc) = self.buf.replace(at, end, &c.to_string(), (self.line, self.col));
        self.line = l;
        self.col = cc;
        self.want_vis = None;
    }

    fn newline(&mut self) {
        if self.readonly {
            return;
        }
        self.buf.new_group();
        let indent: String = if self.opts.auto_indent {
            self.buf.line_text(self.line).chars().take_while(|c| *c == ' ' || *c == '\t').collect()
        } else {
            String::new()
        };
        let eol = self.buf.eol.as_str().to_string();
        self.insert_text(&format!("{eol}{indent}"));
        self.buf.new_group();
    }

    fn backspace(&mut self, word: bool) {
        if self.readonly {
            return;
        }
        self.buf.new_group();
        if self.opts.del_removes_blocks && self.has_selection() && !word {
            self.delete_selection();
            return;
        }
        let at = self.buf.char_idx(self.line, self.col);
        if at == 0 {
            return;
        }
        let from = if word { self.buf.char_idx(self.line, self.word_left()) } else { self.prev_char_idx(at) };
        let from = if from == at { self.prev_char_idx(at) } else { from };
        self.buf.replace(from, at, "", (self.line, self.col));
        let (l, c) = self.buf.pos_of(from);
        self.line = l;
        self.col = c;
    }

    /// Previous char index, treating CRLF as one unit.
    fn prev_char_idx(&self, at: usize) -> usize {
        if at >= 2 && self.buf.rope.char(at - 1) == '\n' && self.buf.rope.char(at - 2) == '\r' { at - 2 } else { at - 1 }
    }

    fn delete(&mut self, word: bool) {
        if self.readonly {
            return;
        }
        self.buf.new_group();
        if self.opts.del_removes_blocks && self.has_selection() {
            self.delete_selection();
            return;
        }
        let at = self.buf.char_idx(self.line, self.col);
        let total = self.buf.rope.len_chars();
        if at >= total {
            return;
        }
        let to = if word {
            self.buf.char_idx(self.line, self.word_right()).max(at + 1)
        } else if self.col >= self.buf.line_len(self.line) {
            // Join with the next line: remove the whole line break.
            let next = self.buf.rope.line_to_char((self.line + 1).min(self.buf.line_count()));
            next.max(at + 1)
        } else {
            at + 1
        };
        self.buf.replace(at, to.min(total), "", (self.line, self.col));
    }

    fn delete_line(&mut self) {
        if self.readonly {
            return;
        }
        self.buf.new_group();
        let a = self.buf.rope.line_to_char(self.line);
        let b = if self.line + 1 < self.buf.line_count() { self.buf.rope.line_to_char(self.line + 1) } else { self.buf.rope.len_chars() };
        self.buf.replace(a, b, "", (self.line, self.col));
        self.line = self.line.min(self.buf.line_count().saturating_sub(1));
        self.col = 0;
        self.anchor = None;
    }

    fn is_div(&self, c: char) -> bool {
        c.is_whitespace() || self.opts.word_div.contains(c)
    }

    fn word_left(&self) -> usize {
        let t: Vec<char> = self.buf.line_text(self.line).chars().collect();
        let mut i = self.col.min(t.len());
        while i > 0 && self.is_div(t[i - 1]) {
            i -= 1;
        }
        while i > 0 && !self.is_div(t[i - 1]) {
            i -= 1;
        }
        i
    }

    fn word_right(&self) -> usize {
        let t: Vec<char> = self.buf.line_text(self.line).chars().collect();
        let mut i = self.col;
        while i < t.len() && !self.is_div(t[i]) {
            i += 1;
        }
        while i < t.len() && self.is_div(t[i]) {
            i += 1;
        }
        i
    }

    fn copy(&self) {
        if let Some(t) = self.selected_text() {
            clipboard::set(&t);
        }
    }

    fn paste(&mut self) {
        if self.readonly {
            return;
        }
        let t = clipboard::get();
        if t.is_empty() {
            return;
        }
        self.buf.new_group();
        if self.has_selection() && !self.opts.persistent_blocks {
            self.delete_selection();
        }
        self.insert_text(&t);
        self.buf.new_group();
    }

    // ----- movement -----

    fn move_vert(&mut self, delta: isize) {
        let vis = *self.want_vis.get_or_insert_with(|| {
            let mut v = 0;
            for c in self.buf.line_text(self.line).chars().take(self.col) {
                v += if c == '\t' { self.opts.tab_size.max(1) - v % self.opts.tab_size.max(1) } else { char_width(c).max(1) };
            }
            v
        });
        let max = self.buf.line_count().saturating_sub(1) as isize;
        self.line = (self.line as isize + delta).clamp(0, max) as usize;
        self.col = self.col_of_vis(self.line, vis);
    }

    fn ensure_visible(&mut self) {
        if self.line < self.top {
            self.top = self.line;
        }
        let h = self.height.max(1);
        if self.line >= self.top + h {
            self.top = self.line + 1 - h;
        }
        let v = self.vis_of(self.line, self.col);
        let w = self.width.max(1);
        if v < self.left {
            self.left = v;
        }
        if v >= self.left + w {
            self.left = v + 1 - w;
        }
    }

    // ----- search -----

    fn find(&mut self, backward: bool) -> Result<bool, String> {
        let re = self.search.spec.compile().map_err(|e| e.to_string())?;
        let text = self.buf.rope.to_string();
        let cur = self.buf.char_idx(self.line, self.col);
        let cur_b = self.buf.rope.char_to_byte(cur);
        let hit = if backward {
            let sel_start = self.sel_range().map(|(a, _)| self.buf.rope.char_to_byte(a)).unwrap_or(cur_b);
            re.find_iter(&text[..sel_start]).last().map(|m| (m.start(), m.end()))
        } else {
            re.find_at(&text, cur_b).map(|m| (m.start(), m.end()))
        };
        match hit {
            Some((a, b)) => {
                let (ca, cb) = (self.buf.rope.byte_to_char(a), self.buf.rope.byte_to_char(b));
                let (al, ac) = self.buf.pos_of(ca);
                let (bl, bc) = self.buf.pos_of(cb);
                self.anchor = Some((al, ac));
                self.vertical = false;
                self.line = bl;
                self.col = bc;
                if backward {
                    self.anchor = Some((bl, bc));
                    self.line = al;
                    self.col = ac;
                }
                self.want_vis = None;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn search_cmd(&mut self, ui: &mut Ui, backward: bool) {
        match self.find(backward) {
            Ok(true) => {}
            Ok(false) => {
                message(
                    ui,
                    msg(MsgId::MSearchReplaceSearchTitle),
                    &format!("{}\n\"{}\"", msg(MsgId::MEditNotFound), self.search.spec.text),
                    &[msg(MsgId::MOk)],
                );
            }
            Err(e) => gm_ui::message::error(ui, msg(MsgId::MError), &e),
        }
    }

    fn replace_cmd(&mut self, ui: &mut Ui, all_mode: bool) {
        let mut all = all_mode;
        let mut count = 0;
        self.buf.new_group();
        while let Ok(true) = self.find(false) {
            if !all {
                self.ensure_visible();
                let bg = ui.term.last_frame().clone();
                let mut tmp = bg.clone();
                let area = tmp.area;
                self.render(&mut tmp, area);
                ui.draw_over(&tmp, |_, _| {});
                let r = message(
                    ui,
                    msg(MsgId::MSearchReplaceReplaceTitle),
                    &format!(
                        "{}\n\"{}\"\n{}\n\"{}\"",
                        msg(MsgId::MEditAskReplace),
                        self.selected_text().unwrap_or_default(),
                        msg(MsgId::MEditAskReplaceWith),
                        self.search.replace
                    ),
                    &[msg(MsgId::MEditReplace), msg(MsgId::MEditReplaceAll), msg(MsgId::MEditSkip), msg(MsgId::MCancel)],
                );
                match r {
                    Some(0) => {}
                    Some(1) => all = true,
                    Some(2) => {
                        self.anchor = None;
                        continue;
                    }
                    _ => break,
                }
            }
            let repl = self.search.replace.clone();
            let with = if self.search.spec.regex {
                let sel = self.selected_text().unwrap_or_default();
                match self.search.spec.compile() {
                    Ok(re) => re.replace(&sel, repl.as_str()).into_owned(),
                    Err(_) => repl,
                }
            } else {
                repl
            };
            if let Some((a, b)) = self.sel_range() {
                let (l, c) = self.buf.replace(a, b, &with, (self.line, self.col));
                self.line = l;
                self.col = c;
                self.anchor = None;
                count += 1;
            } else {
                break;
            }
            if count > 1_000_000 {
                break;
            }
        }
        self.buf.new_group();
        if count == 0 {
            message(
                ui,
                msg(MsgId::MSearchReplaceSearchTitle),
                &format!("{}\n\"{}\"", msg(MsgId::MEditNotFound), self.search.spec.text),
                &[msg(MsgId::MOk)],
            );
        }
    }

    // ----- file commands -----

    /// Saves; returns false on error (already reported).
    pub fn save(&mut self, ui: &mut Ui, convert: Option<Eol>) -> bool {
        let path = self.path.clone();
        match self.buf.save(&path, convert) {
            Ok(()) => {
                self.is_new = false;
                self.mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.external_asked = false;
                if self.buf.lossy {
                    message(ui, msg(MsgId::MWarning), msg(MsgId::MEditDataLostWarn), &[msg(MsgId::MOk)]);
                }
                true
            }
            Err(e) => {
                gm_ui::message::error(ui, msg(MsgId::MEditTitle), &format!("{}\n{}\n{e}", msg(MsgId::MEditCannotSave), path.display()));
                false
            }
        }
    }

    fn save_as(&mut self, ui: &mut Ui) -> bool {
        let mut b = DialogBuilder::new(msg(MsgId::MEditTitle), 70);
        b.text(msg(MsgId::MEditSaveAs));
        let name = b.edit(&self.path.display().to_string(), Some("NewEdit"));
        b.text(msg(MsgId::MEditCodePage));
        let cps = Codepage::menu_list();
        let names: Vec<String> = cps.iter().map(Codepage::name).collect();
        let combo = b.combo(&self.buf.cp.name(), names, true);
        let bom = b.checkbox("Add &BOM", self.buf.bom);
        b.separator_text(msg(MsgId::MEditSaveAsFormatTitle));
        let r0 = b.radio(msg(MsgId::MEditSaveOriginal), true, true);
        let r1 = b.radio(msg(MsgId::MEditSaveDOS), false, false);
        let r2 = b.radio(msg(MsgId::MEditSaveUnix), false, false);
        let r3 = b.radio(msg(MsgId::MEditSaveMac), false, false);
        b.separator();
        b.ok_cancel();
        let mut d = b.build();
        if d.run(ui).is_none() {
            return false;
        }
        let new = PathBuf::from(d.text(name).trim());
        if new.as_os_str().is_empty() {
            return false;
        }
        let cp_name = d.text(combo);
        if let Some(cp) = cps.iter().find(|c| c.name() == cp_name) {
            self.buf.cp = *cp;
        }
        self.buf.bom = d.checked(bom);
        let eol = if d.checked(r1) {
            Some(Eol::CrLf)
        } else if d.checked(r2) {
            Some(Eol::Lf)
        } else if d.checked(r3) {
            Some(Eol::Cr)
        } else {
            let _ = r0;
            None
        };
        self.path = new;
        self.save(ui, eol)
    }

    fn goto_dialog(&mut self, ui: &mut Ui) {
        let mut b = DialogBuilder::new(msg(MsgId::MGoTo), 50);
        let e = b.edit("", Some("LineNumber"));
        b.text("line[:column]");
        b.separator();
        b.ok_cancel();
        let mut d = b.build();
        if d.run(ui).is_none() {
            return;
        }
        let t = d.text(e);
        let mut parts = t.split([':', ',', ' ']).filter(|s| !s.is_empty());
        let total = self.buf.line_count();
        if let Some(Ok(l)) = parts.next().map(|s| s.trim().parse::<i64>()) {
            let l = if l < 0 { total as i64 + l } else { l - 1 }.max(0) as usize;
            let c = parts.next().and_then(|s| s.trim().parse::<usize>().ok()).map(|c| c.saturating_sub(1)).unwrap_or(0);
            self.anchor = None;
            self.set_cursor(l, 0);
            self.col = self.col_of_vis(self.line, c);
            self.top = self.line.saturating_sub(self.height / 2);
        }
    }

    fn codepage_menu(&mut self, ui: &mut Ui) {
        let list = Codepage::menu_list();
        let items = list.iter().map(|c| MenuItem::new(c.name()).checked(*c == self.buf.cp)).collect();
        let mut m = VMenu::new(msg(MsgId::MGetCodePageTitle), items);
        if let Some(p) = list.iter().position(|c| *c == self.buf.cp) {
            m.select(p);
        }
        if let MenuResult::Selected(i) = m.run(ui) {
            self.set_codepage(ui, list[i]);
        }
    }

    fn set_codepage(&mut self, ui: &mut Ui, cp: Codepage) {
        if self.buf.modified() || self.is_new {
            // Re-decoding needs the original bytes; with changes, only the save encoding changes.
            self.buf.cp = cp;
            return;
        }
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                self.buf.recode(&bytes, cp);
                self.set_cursor(self.line, self.col);
            }
            Err(e) => gm_ui::message::error(ui, msg(MsgId::MError), &e.to_string()),
        }
    }

    fn tools_menu(&mut self, ui: &mut Ui) {
        if self.tools.is_empty() {
            return;
        }
        let tools = self.tools.clone();
        let items = tools.iter().map(|t| MenuItem::new(t.name())).collect();
        let mut m = VMenu::new("Tools", items);
        if let MenuResult::Selected(i) = m.run(ui) {
            self.buf.new_group();
            tools[i].run(self, ui);
            self.buf.new_group();
            self.set_cursor(self.line, self.col);
        }
    }

    /// Asks to save when modified. Returns false to keep editing.
    pub fn confirm_close(&mut self, ui: &mut Ui) -> bool {
        if !self.buf.modified() {
            return true;
        }
        let r = message(
            ui,
            msg(MsgId::MEditTitle),
            &format!("{}\n{}", self.path.display(), msg(MsgId::MEditAskSave)),
            &[msg(MsgId::MHYes), msg(MsgId::MHNo), msg(MsgId::MCancel)],
        );
        match r {
            Some(0) => {
                if self.is_new && !self.path.parent().is_some_and(Path::exists) {
                    return self.save_as(ui);
                }
                self.save(ui, None)
            }
            Some(1) => true,
            _ => false,
        }
    }

    pub fn position(&self) -> (usize, usize, usize, usize) {
        (self.line, self.col, self.top, self.left)
    }

    pub fn restore_position(&mut self, line: usize, col: usize, top: usize, left: usize) {
        self.set_cursor(line, col);
        self.top = top.min(self.line);
        self.left = left;
    }

    fn render_body(&mut self, buf: &mut Buffer, r: Rect) {
        let text = style(PaletteId::EditorText);
        let sel = style(PaletteId::EditorTextSelected);
        let nums = style(PaletteId::EditorLineNumbers);
        draw::fill(buf, r, ' ', text);
        let total = self.buf.line_count();
        self.gutter = if self.opts.line_numbers { (total.to_string().len() + 1) as u16 } else { 0 };
        let tx = r.x + self.gutter;
        let tw = r.width.saturating_sub(self.gutter) as usize;
        self.width = tw;
        self.height = r.height as usize;
        self.body = r;
        self.ensure_visible();
        let range = self.sel_range();
        let block = self.block_rect();
        for row in 0..r.height as usize {
            let ln = self.top + row;
            if ln >= total {
                break;
            }
            let y = r.y + row as u16;
            if self.gutter > 0 {
                draw::put_fit(buf, r.x, y, &format!("{:>w$} ", ln + 1, w = self.gutter as usize - 1), nums, self.gutter as usize);
            }
            let line_start = self.buf.rope.line_to_char(ln);
            let mut vis = 0usize;
            for (ci, c) in self.buf.line_text(ln).chars().enumerate() {
                let w = self.cell_width(c, vis);
                let selected = match (range, block) {
                    (Some((a, b)), _) => (line_start + ci) >= a && (line_start + ci) < b,
                    (_, Some((l1, l2, v1, v2))) => ln >= l1 && ln <= l2 && vis >= v1 && vis < v2,
                    _ => false,
                };
                let st = if selected { sel } else { text };
                if vis + w > self.left && vis < self.left + tw {
                    let x = tx + (vis.saturating_sub(self.left)) as u16;
                    if c == '\t' {
                        for i in 0..w {
                            let g = if self.opts.show_whitespace && i == 0 { '→' } else { ' ' };
                            if vis + i >= self.left && vis + i < self.left + tw {
                                draw::set(buf, tx + (vis + i - self.left) as u16, y, g, st);
                            }
                        }
                    } else {
                        let g = if c == ' ' && self.opts.show_whitespace { '·' } else { c };
                        draw::put(buf, x, y, &g.to_string(), st, tw.saturating_sub(vis - self.left.min(vis)));
                    }
                }
                vis += w;
            }
            // Selection that includes the line break.
            if let Some((a, b)) = range {
                let eol_idx = line_start + self.buf.line_len(ln);
                if eol_idx >= a && eol_idx < b && ln + 1 < total && vis >= self.left && vis < self.left + tw {
                    draw::set(buf, tx + (vis - self.left) as u16, y, ' ', sel);
                }
            }
        }
    }
}

impl Window for Editor {
    fn title(&self) -> String {
        self.path.display().to_string()
    }

    fn kind(&self) -> WindowKind {
        WindowKind::Editor
    }

    fn modified(&self) -> bool {
        self.buf.modified()
    }

    fn file(&self) -> Option<PathBuf> {
        Some(self.path.clone())
    }

    fn help_topic(&self) -> &str {
        "Editor"
    }

    fn render(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        let status = style(PaletteId::EditorStatus);
        draw::fill(buf, Rect::new(area.x, area.y, area.width, 1), ' ', status);
        let ch = self.buf.line_text(self.line).chars().nth(self.col).map(|c| c as u32).unwrap_or(0);
        let info = format!(
            "{}{} {} {} {}/{} {} {} {} {}",
            if self.buf.modified() { '*' } else { ' ' },
            if self.readonly { "-" } else { " " },
            self.buf.cp.name(),
            msg(MsgId::MEditStatusLine),
            self.line + 1,
            self.buf.line_count(),
            msg(MsgId::MEditStatusCol),
            self.vis_of(self.line, self.col) + 1,
            msg(MsgId::MEditStatusChar),
            ch
        );
        let info_w = gm_text::str_width(&info) + 8;
        let name_w = (area.width as usize).saturating_sub(info_w + 1);
        draw::put(buf, area.x, area.y, &gm_text::truncate_left(&self.title(), name_w), status, name_w);
        draw::put(buf, area.right().saturating_sub(info_w as u16), area.y, &info, status, info_w);
        let kb = u16::from(self.opts.keybar);
        let body = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(1 + kb));
        self.render_body(buf, body);
        if self.opts.keybar {
            self.labels.normal[7] = match self.buf.cp {
                Codepage::Oem(_) => msg(MsgId::MEditF8).to_string(),
                _ => msg(MsgId::MEditF8DOS).to_string(),
            };
            self.labels.ctrl[2] = msg(if self.opts.line_numbers { MsgId::MEditCtrlF3Hide } else { MsgId::MEditCtrlF3 }).to_string();
            KeyBar::render(buf, area.bottom() - 1, area.width, self.labels.for_mods(Mods::NONE));
        }
        let vis = self.vis_of(self.line, self.col);
        if self.line >= self.top && vis >= self.left {
            Some((body.x + self.gutter + (vis - self.left) as u16, body.y + (self.line - self.top) as u16))
        } else {
            None
        }
    }

    fn handle(&mut self, ev: &Event, ui: &mut Ui) -> WinAction {
        let k = match ev {
            Event::Key(k) => *k,
            Event::Paste(s) => {
                if !self.readonly {
                    self.buf.new_group();
                    self.delete_selection();
                    let s = s.replace("\r\n", "\n").replace('\n', self.buf.eol.as_str());
                    self.insert_text(&s);
                    self.buf.new_group();
                }
                return WinAction::Redraw;
            }
            Event::Mouse(m) => {
                let r = self.body;
                let inside = m.column >= r.x + self.gutter && m.column < r.right() && m.row >= r.y && m.row < r.bottom();
                match m.kind {
                    MouseEventKind::ScrollDown => {
                        self.top = (self.top + 3).min(self.buf.line_count().saturating_sub(1));
                        if self.line < self.top {
                            self.line = self.top;
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        self.top = self.top.saturating_sub(3);
                        if self.line >= self.top + self.height {
                            self.line = self.top + self.height - 1;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left) if inside => {
                        let line = (self.top + (m.row - r.y) as usize).min(self.buf.line_count().saturating_sub(1));
                        let vis = self.left + (m.column - r.x - self.gutter) as usize;
                        if matches!(m.kind, MouseEventKind::Drag(_)) {
                            self.select_motion(true, false);
                        } else {
                            self.anchor = None;
                        }
                        self.line = line;
                        self.col = self.col_of_vis(line, vis);
                        self.want_vis = None;
                    }
                    _ => {}
                }
                return WinAction::Redraw;
            }
            _ => return WinAction::None,
        };
        let shift = k.mods.shift();
        let vertical = k.mods.alt() && shift;
        let ctrl = k.mods.ctrl();
        let is_move = matches!(
            k.code,
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End | KeyCode::PageUp | KeyCode::PageDown
        );
        if is_move && !(ctrl && !shift && matches!(k.code, KeyCode::Up | KeyCode::Down)) {
            self.select_motion(shift, vertical);
            let page = self.height.saturating_sub(1).max(1) as isize;
            match k.code {
                KeyCode::Left => {
                    if ctrl {
                        if self.col == 0 && self.line > 0 {
                            self.line -= 1;
                            self.col = self.buf.line_len(self.line);
                        } else {
                            self.col = self.word_left();
                        }
                    } else if self.col > 0 {
                        self.col -= 1;
                    } else if self.line > 0 && !vertical {
                        self.line -= 1;
                        self.col = self.buf.line_len(self.line);
                    }
                    self.want_vis = None;
                }
                KeyCode::Right => {
                    let len = self.buf.line_len(self.line);
                    if ctrl {
                        if self.col >= len && self.line + 1 < self.buf.line_count() {
                            self.line += 1;
                            self.col = 0;
                        } else {
                            self.col = self.word_right();
                        }
                    } else if self.col < len {
                        self.col += 1;
                    } else if self.line + 1 < self.buf.line_count() && !vertical {
                        self.line += 1;
                        self.col = 0;
                    }
                    self.want_vis = None;
                }
                KeyCode::Up => self.move_vert(-1),
                KeyCode::Down => self.move_vert(1),
                KeyCode::PageUp if ctrl => self.set_cursor(0, 0),
                KeyCode::PageDown if ctrl => {
                    let l = self.buf.line_count().saturating_sub(1);
                    self.set_cursor(l, usize::MAX);
                }
                KeyCode::PageUp => {
                    self.move_vert(-page);
                    self.top = self.top.saturating_sub(page as usize);
                }
                KeyCode::PageDown => {
                    self.move_vert(page);
                    self.top += page as usize;
                    self.top = self.top.min(self.buf.line_count().saturating_sub(1));
                }
                KeyCode::Home if ctrl => self.set_cursor(0, 0),
                KeyCode::End if ctrl => {
                    let l = self.buf.line_count().saturating_sub(1);
                    self.set_cursor(l, usize::MAX);
                }
                KeyCode::Home => {
                    // Smart home: first non-blank, then column 0.
                    let first = self.buf.line_text(self.line).chars().take_while(|c| c.is_whitespace()).count();
                    self.col = if self.col == first { 0 } else { first };
                    self.want_vis = None;
                }
                KeyCode::End => {
                    self.col = self.buf.line_len(self.line);
                    self.want_vis = None;
                }
                _ => {}
            }
            if !vertical {
                self.want_vis =
                    if matches!(k.code, KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown) { self.want_vis } else { None };
            }
            return WinAction::Redraw;
        }
        match (k.code, k.mods) {
            (KeyCode::Up, Mods::CTRL) => {
                self.top = self.top.saturating_sub(1);
                if self.line >= self.top + self.height {
                    self.move_vert(-1);
                }
            }
            (KeyCode::Down, Mods::CTRL) => {
                if self.top + 1 < self.buf.line_count() {
                    self.top += 1;
                }
                if self.line < self.top {
                    self.move_vert(1);
                }
            }
            (KeyCode::Esc, _) | (KeyCode::F(10), Mods::NONE) | (KeyCode::F(4), Mods::NONE) => {
                if self.confirm_close(ui) {
                    return WinAction::Close;
                }
            }
            (KeyCode::F(10), Mods::SHIFT) => {
                if !self.buf.modified() || self.save(ui, None) {
                    return WinAction::Close;
                }
            }
            (KeyCode::F(2), Mods::NONE) => {
                if self.is_new && !self.path.parent().is_some_and(Path::exists) {
                    self.save_as(ui);
                } else {
                    self.save(ui, None);
                }
            }
            (KeyCode::F(2), Mods::SHIFT) => {
                self.save_as(ui);
            }
            (KeyCode::F(6), Mods::NONE) => {
                if self.confirm_close(ui) {
                    return WinAction::Replace(OpenRequest::View(self.path.clone()));
                }
            }
            (KeyCode::F(7), Mods::NONE) => {
                let mut st = self.search.clone();
                let word = self.word_at_cursor();
                if let Some(a) = search_dialog(ui, &mut st, false, false, &word) {
                    self.search = st;
                    self.search_cmd(ui, a == SearchAction::FindPrev);
                }
            }
            (KeyCode::F(7), Mods::CTRL) if !self.readonly => {
                let mut st = self.search.clone();
                let word = self.word_at_cursor();
                if let Some(a) = search_dialog(ui, &mut st, true, false, &word) {
                    self.search = st;
                    match a {
                        SearchAction::FindPrev => self.search_cmd(ui, true),
                        SearchAction::ReplaceAll => self.replace_cmd(ui, true),
                        _ => self.replace_cmd(ui, false),
                    }
                }
            }
            (KeyCode::F(7), Mods::SHIFT) if !self.search.spec.text.is_empty() => self.search_cmd(ui, false),
            (KeyCode::F(7), Mods::ALT) if !self.search.spec.text.is_empty() => self.search_cmd(ui, true),
            (KeyCode::F(8), Mods::NONE) => {
                let next = match self.buf.cp {
                    Codepage::Oem(_) => Codepage::ansi(),
                    _ => Codepage::oem(),
                };
                self.set_codepage(ui, next);
            }
            (KeyCode::F(8), Mods::SHIFT) => self.codepage_menu(ui),
            (KeyCode::F(8), Mods::ALT) => self.goto_dialog(ui),
            (KeyCode::F(3), Mods::CTRL) => self.opts.line_numbers = !self.opts.line_numbers,
            (KeyCode::F(11), Mods::NONE) => self.tools_menu(ui),
            (KeyCode::F(12), Mods::NONE) => return WinAction::ScreensList,
            (KeyCode::F(1), Mods::NONE) => return WinAction::Open(OpenRequest::Help("Editor".into())),
            (KeyCode::Tab, Mods::CTRL) => return WinAction::NextWindow,
            (KeyCode::Tab, Mods::CTRL_SHIFT) => return WinAction::PrevWindow,
            (KeyCode::Char('o'), Mods::CTRL) => return WinAction::Open(OpenRequest::UserScreen),
            (KeyCode::Enter, Mods::NONE) => self.newline(),
            (KeyCode::Tab, Mods::NONE) => {
                if self.opts.expand_tabs {
                    let v = self.vis_of(self.line, self.col);
                    let n = self.opts.tab_size.max(1) - v % self.opts.tab_size.max(1);
                    self.buf.new_group();
                    self.insert_text(&" ".repeat(n));
                } else {
                    self.type_char('\t');
                }
            }
            (KeyCode::Backspace, Mods::NONE) => self.backspace(false),
            (KeyCode::Backspace, Mods::CTRL) => self.backspace(true),
            (KeyCode::Backspace, Mods::ALT) | (KeyCode::Char('z'), Mods::CTRL) => {
                if let Some((l, c)) = self.buf.undo() {
                    self.anchor = None;
                    self.set_cursor(l, c);
                }
            }
            (KeyCode::Char('z'), Mods::CTRL_SHIFT) | (KeyCode::Backspace, Mods::CTRL_ALT) => {
                if let Some((l, c)) = self.buf.redo() {
                    self.anchor = None;
                    self.set_cursor(l, c);
                }
            }
            (KeyCode::Delete, Mods::NONE) => self.delete(false),
            (KeyCode::Delete, Mods::CTRL) => self.delete(true),
            (KeyCode::Char('y'), Mods::CTRL) => self.delete_line(),
            (KeyCode::Char('k'), Mods::CTRL) if !self.readonly => {
                self.buf.new_group();
                let a = self.buf.char_idx(self.line, self.col);
                let b = self.buf.char_idx(self.line, usize::MAX);
                self.buf.replace(a, b, "", (self.line, self.col));
            }
            (KeyCode::Char('a'), Mods::CTRL) => {
                self.anchor = Some((0, 0));
                self.vertical = false;
                let l = self.buf.line_count().saturating_sub(1);
                self.set_cursor(l, usize::MAX);
            }
            (KeyCode::Char('u'), Mods::CTRL) => {
                self.anchor = None;
                self.vertical = false;
            }
            (KeyCode::Insert, Mods::CTRL) | (KeyCode::Char('c'), Mods::CTRL) => self.copy(),
            (KeyCode::Delete, Mods::SHIFT) | (KeyCode::Char('x'), Mods::CTRL) => {
                self.copy();
                self.buf.new_group();
                self.delete_selection();
            }
            (KeyCode::Insert, Mods::SHIFT) | (KeyCode::Char('v'), Mods::CTRL) => self.paste(),
            (KeyCode::Insert, Mods::NONE) => {
                self.overtype = !self.overtype;
                ui.term.block_cursor = self.overtype;
            }
            // Ctrl+P: copy block to cursor; Ctrl+M: move block to cursor.
            (KeyCode::Char('p'), Mods::CTRL) | (KeyCode::Char('m'), Mods::CTRL) if self.sel_range().is_some() => {
                let text = self.selected_text().unwrap_or_default();
                let mv = k.code == KeyCode::Char('m');
                self.buf.new_group();
                let at = self.buf.char_idx(self.line, self.col);
                let (a, b) = self.sel_range().unwrap_or((at, at));
                if at >= a && at <= b {
                    return WinAction::Redraw;
                }
                let (l, c) = (self.line, self.col);
                let ins = if mv && at > b { at - (b - a) } else { at };
                if mv {
                    self.buf.replace(a, b, "", (l, c));
                }
                self.buf.replace(ins, ins, &text, (l, c));
                let (sl, sc) = self.buf.pos_of(ins);
                let (el, ec) = self.buf.pos_of(ins + text.chars().count());
                self.anchor = Some((sl, sc));
                self.line = el;
                self.col = ec;
            }
            (KeyCode::Char(d @ '0'..='9'), m) if m == Mods::CTRL_SHIFT || m == Mods::ALT_SHIFT => {
                self.bookmarks[(d as u8 - b'0') as usize] = Some((self.line, self.col));
            }
            (KeyCode::Char(d @ '0'..='9'), m) if m == Mods::CTRL || m == Mods::ALT => {
                if let Some((l, c)) = self.bookmarks[(d as u8 - b'0') as usize] {
                    self.set_cursor(l, c);
                }
            }
            (KeyCode::Char(c), m) if !m.ctrl() && !m.alt() => {
                if self.anchor.is_some() && !self.opts.persistent_blocks && !self.has_selection() {
                    self.anchor = None;
                }
                self.type_char(c);
            }
            _ => return WinAction::None,
        }
        WinAction::Redraw
    }

    fn can_close(&mut self, ui: &mut Ui) -> bool {
        self.confirm_close(ui)
    }

    fn tick(&mut self, ui: &mut Ui) -> bool {
        if self.external_asked || self.is_new {
            return false;
        }
        let now = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        if now.is_some() && self.mtime.is_some() && now != self.mtime {
            self.external_asked = true;
            if !self.buf.modified() {
                let r = message(
                    ui,
                    msg(MsgId::MEditTitle),
                    &format!("{}\n{}", self.path.display(), msg(MsgId::MEditAskSaveExt).replace(" Save?", " Reload?")),
                    &[msg(MsgId::MHYes), msg(MsgId::MHNo)],
                );
                if r == Some(0)
                    && let Ok(b) = TextBuffer::load(&self.path, self.opts.auto_detect, self.opts.default_cp)
                {
                    self.buf = b;
                    self.mtime = now;
                    self.external_asked = false;
                    self.set_cursor(self.line, self.col);
                }
            }
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gm_term::KeyChord;

    fn ed(text: &str) -> Editor {
        let p = std::env::temp_dir().join(format!("gm-ed-{}-{}.txt", std::process::id(), text.len()));
        std::fs::write(&p, text).unwrap();
        Editor::open(&p, EditorOptions::default(), Rc::new(Vec::new())).unwrap()
    }

    #[test]
    fn selection_and_delete() {
        let mut e = ed("hello\nworld\n");
        e.anchor = Some((0, 1));
        e.line = 1;
        e.col = 2;
        assert_eq!(e.selected_text().unwrap(), "ello\nwo");
        e.buf.new_group();
        e.delete_selection();
        assert_eq!(e.line_text(0), "hrld");
        e.buf.undo();
        assert_eq!(e.line_text(1), "world");
        // Vertical block.
        e.anchor = Some((0, 1));
        e.vertical = true;
        e.line = 1;
        e.col = 3;
        e.want_vis = None;
        assert_eq!(e.selected_text().unwrap(), "el\nor");
        let _ = std::fs::remove_file(&e.path);
    }

    #[test]
    fn search_and_vis() {
        let mut e = ed("a\tb foo\nbar foo\n");
        assert_eq!(e.vis_of(0, 2), 8);
        assert_eq!(e.col_of_vis(0, 5), 1);
        e.search.spec.text = "foo".into();
        assert!(e.find(false).unwrap());
        assert_eq!(e.cursor(), (0, 7));
        assert!(e.find(false).unwrap());
        assert_eq!(e.cursor(), (1, 7));
        let _ = KeyChord::plain(KeyCode::Null);
        let _ = std::fs::remove_file(&e.path);
    }
}

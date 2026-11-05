//! The file list panel model: provider stack, entries, cursor, selection.

use crate::columns::{ColumnSet, view_mode};
use crate::highlight::Highlighter;
use crate::sort::{SortMode, sort_entries};
use crate::{PanelOptions, diz};
use gm_text::FileMask;
use gm_vfs::{CancelToken, ChangeDir, Entry, LocalFs, VfsProvider};
use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;

pub enum EnterResult {
    /// The panel changed directory.
    Changed,
    /// Enter on a file: the core decides (execute, association, archive…).
    File(String),
    Nothing,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Geometry {
    pub x: u16,
    pub rows: usize,
    pub cols: usize,
    pub col_width: u16,
    pub first_row_y: u16,
}

pub struct FilePanel {
    pub stack: Vec<Box<dyn VfsProvider>>,
    pub entries: Vec<Entry>,
    pub selected: Vec<bool>,
    pub cursor: usize,
    pub top: usize,
    pub mode: u8,
    pub columns: ColumnSet,
    pub sort: SortMode,
    pub reverse: bool,
    pub opts: PanelOptions,
    pub filter: Option<(String, FileMask)>,
    pub highlighter: Rc<Highlighter>,
    /// Alt+letters quick search prefix.
    pub fast_find: Option<String>,
    prev_selection: Vec<String>,
    pub error: Option<String>,
    /// `error` was set by the last listing of a remote provider and has not
    /// been reported in a message box yet.
    fresh_error: bool,
    pub free: Option<(u64, u64)>,
    pub(crate) geom: Geometry,
    /// Archive names of the provider stack, for titles.
    pub stack_names: Vec<String>,
}

impl FilePanel {
    pub fn new(dir: PathBuf, opts: PanelOptions, highlighter: Rc<Highlighter>) -> FilePanel {
        let mut p = FilePanel {
            stack: vec![Box::new(LocalFs::new(dir))],
            entries: Vec::new(),
            selected: Vec::new(),
            cursor: 0,
            top: 0,
            mode: 2,
            columns: view_mode(2),
            sort: SortMode::Name,
            reverse: false,
            opts,
            filter: None,
            highlighter,
            fast_find: None,
            prev_selection: Vec::new(),
            error: None,
            fresh_error: false,
            free: None,
            geom: Geometry::default(),
            stack_names: Vec::new(),
        };
        p.reload();
        p
    }

    pub fn provider(&self) -> &dyn VfsProvider {
        self.stack.last().expect("provider stack is never empty").as_ref()
    }

    pub fn provider_mut(&mut self) -> &mut dyn VfsProvider {
        self.stack.last_mut().expect("provider stack is never empty").as_mut()
    }

    pub fn is_local(&self) -> bool {
        self.stack.len() == 1
    }

    /// The real directory when the panel shows the local file system.
    pub fn local_dir(&self) -> Option<PathBuf> {
        if self.is_local() { self.provider().local_dir() } else { None }
    }

    /// The local directory of the bottom provider (also when inside an archive).
    pub fn base_dir(&self) -> PathBuf {
        self.stack[0].local_dir().unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn display_dir(&self) -> String {
        if self.is_local() {
            return self.provider().current_dir();
        }
        let p = self.provider();
        let fmt = p.format();
        let inner = p.current_dir();
        let arc = self.stack_names.last().cloned().unwrap_or_default();
        if fmt.is_empty() {
            p.title()
        } else if arc.is_empty() {
            format!("{fmt}:{inner}")
        } else {
            format!("{fmt}:{arc}{inner}")
        }
    }

    pub fn set_mode(&mut self, mode: u8) {
        self.mode = mode;
        self.columns = view_mode(mode);
        if let Some(custom) = self.provider().columns() {
            // Providers with custom columns (process list) get N + C0..Cn.
            let mut cols = vec![crate::columns::Column { kind: crate::columns::ColumnKind::Name, width: 0 }];
            for (i, c) in custom.iter().enumerate() {
                cols.push(crate::columns::Column { kind: crate::columns::ColumnKind::Custom(i as u8), width: c.width });
            }
            self.columns.columns = cols;
            self.columns.multi = false;
        }
    }

    pub fn set_sort(&mut self, mode: SortMode) {
        if self.sort == mode {
            self.reverse = !self.reverse;
        } else {
            self.sort = mode;
            self.reverse = false;
        }
        self.resort();
    }

    pub fn resort(&mut self) {
        let cur = self.current_name();
        let sel: HashSet<String> = self.selected_names().into_iter().collect();
        sort_entries(&mut self.entries, self.sort, self.reverse, self.opts.natural_sort, None);
        self.selected = self.entries.iter().map(|e| sel.contains(&e.name)).collect();
        if let Some(c) = cur {
            self.set_cursor_name(&c);
        }
    }

    /// Re-reads the directory, keeping cursor and selection by name.
    pub fn reload(&mut self) {
        let cur = self.current_name();
        let sel: HashSet<String> = self.selected_names().into_iter().collect();
        let old_cursor = self.cursor;
        self.load_entries();
        self.selected = self.entries.iter().map(|e| sel.contains(&e.name)).collect();
        match cur {
            Some(c) if self.set_cursor_name(&c) => {}
            _ => self.cursor = old_cursor.min(self.entries.len().saturating_sub(1)),
        }
    }

    fn load_entries(&mut self) {
        let cancel = CancelToken::new();
        let mut list = match self.provider_mut().list(&cancel) {
            Ok(v) => {
                self.error = None;
                v
            }
            Err(e) => {
                self.error = Some(e.to_string());
                self.fresh_error = self.provider().report_errors();
                Vec::new()
            }
        };
        let show_hidden = self.opts.show_hidden;
        let filter = self.filter.as_ref().map(|f| f.1.clone());
        list.retain(|e| (show_hidden || !e.hidden) && (e.is_dir || filter.as_ref().is_none_or(|m| m.matches(&e.name))));
        let has_parent = match self.local_dir() {
            Some(d) => d.parent().is_some() || self.opts.show_dotdot_in_root,
            None => true,
        };
        if has_parent {
            list.insert(0, Entry::dotdot());
        }
        if let Some(dir) = self.local_dir() {
            let d = diz::read(&dir);
            if !d.is_empty() {
                for e in &mut list {
                    if let Some(s) = d.get(&e.name.to_lowercase()) {
                        e.description = Some(s.clone());
                    }
                }
            }
            self.free = if self.opts.free_space { gm_platform::drives::space(&dir) } else { None };
        } else {
            self.free = None;
        }
        sort_entries(&mut list, self.sort, self.reverse, self.opts.natural_sort, None);
        self.entries = list;
        self.selected = vec![false; self.entries.len()];
        if self.cursor >= self.entries.len() {
            self.cursor = self.entries.len().saturating_sub(1);
        }
    }

    /// The listing error of a remote provider, once (for a message box).
    pub fn take_fresh_error(&mut self) -> Option<String> {
        if !std::mem::take(&mut self.fresh_error) {
            return None;
        }
        self.error.clone()
    }

    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    pub fn current_name(&self) -> Option<String> {
        self.current().map(|e| e.name.clone())
    }

    /// Full local path of the item under the cursor.
    pub fn current_path(&self) -> Option<PathBuf> {
        let e = self.current()?;
        if e.is_dotdot() {
            return None;
        }
        match self.local_dir() {
            Some(d) => Some(d.join(&e.name)),
            None => self.provider().real_path(&e.name),
        }
    }

    pub fn set_cursor_name(&mut self, name: &str) -> bool {
        let ci = gm_platform::case_insensitive_names();
        let pos = self
            .entries
            .iter()
            .position(|e| e.name == name)
            .or_else(|| if ci { self.entries.iter().position(|e| e.name.eq_ignore_ascii_case(name)) } else { None });
        match pos {
            Some(p) => {
                self.cursor = p;
                true
            }
            None => false,
        }
    }

    pub fn selected_names(&self) -> Vec<String> {
        self.entries.iter().zip(&self.selected).filter(|(_, s)| **s).map(|(e, _)| e.name.clone()).collect()
    }

    pub fn selected_count(&self) -> usize {
        self.selected.iter().filter(|s| **s).count()
    }

    /// Selected items, or the item under the cursor (never "..").
    pub fn targets(&self) -> Vec<String> {
        let s = self.selected_names();
        if !s.is_empty() {
            return s;
        }
        match self.current() {
            Some(e) if !e.is_dotdot() => vec![e.name.clone()],
            _ => Vec::new(),
        }
    }

    pub fn target_entries(&self) -> Vec<&Entry> {
        let any = self.selected.iter().any(|s| *s);
        if any {
            self.entries.iter().zip(&self.selected).filter(|(_, s)| **s).map(|(e, _)| e).collect()
        } else {
            self.current().filter(|e| !e.is_dotdot()).into_iter().collect()
        }
    }

    /// Local paths of `targets()`.
    pub fn target_paths(&self) -> Vec<PathBuf> {
        match self.local_dir() {
            Some(d) => self.targets().iter().map(|n| d.join(n)).collect(),
            None => Vec::new(),
        }
    }

    pub fn clear_selection(&mut self) {
        let names = self.selected_names();
        if !names.is_empty() {
            self.prev_selection = names;
        }
        self.selected.iter_mut().for_each(|s| *s = false);
    }

    fn selectable(&self, i: usize) -> bool {
        self.entries.get(i).is_some_and(|e| !e.is_dotdot())
    }

    /// Ins: toggle and move down.
    pub fn toggle_current(&mut self) {
        if self.selectable(self.cursor) {
            self.selected[self.cursor] = !self.selected[self.cursor];
        }
        self.move_by(1);
    }

    pub fn set_selected(&mut self, i: usize, on: bool) {
        if self.selectable(i) {
            self.selected[i] = on;
        }
    }

    /// Gray +/-: select or deselect by mask.
    pub fn select_mask(&mut self, mask: &FileMask, on: bool) {
        for i in 0..self.entries.len() {
            let e = &self.entries[i];
            if e.is_dotdot() || (e.is_dir && !self.opts.select_folders && on) {
                continue;
            }
            if mask.matches(&e.name) {
                self.selected[i] = on;
            }
        }
    }

    /// Gray *: invert selection (files only unless folders are selectable).
    pub fn invert_selection(&mut self) {
        for i in 0..self.entries.len() {
            let e = &self.entries[i];
            if e.is_dotdot() || (e.is_dir && !self.opts.select_folders) {
                continue;
            }
            self.selected[i] = !self.selected[i];
        }
    }

    /// Ctrl+M: restore the previous selection.
    pub fn restore_selection(&mut self) {
        let prev: HashSet<String> = self.prev_selection.iter().cloned().collect();
        for (i, e) in self.entries.iter().enumerate() {
            self.selected[i] = prev.contains(&e.name);
        }
    }

    /// Selects files with the same extension / name as the current one.
    pub fn select_same(&mut self, by_ext: bool, on: bool) {
        let Some(cur) = self.current().cloned() else { return };
        let stem = |n: &str| n.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or_else(|| n.to_string());
        for i in 0..self.entries.len() {
            let e = &self.entries[i];
            if e.is_dotdot() || e.is_dir != cur.is_dir {
                continue;
            }
            let hit = if by_ext { e.ext().eq_ignore_ascii_case(cur.ext()) } else { stem(&e.name).eq_ignore_ascii_case(&stem(&cur.name)) };
            if hit {
                self.selected[i] = on;
            }
        }
    }

    pub fn selected_size(&self) -> u64 {
        self.entries.iter().zip(&self.selected).filter(|(_, s)| **s).map(|(e, _)| e.size).sum()
    }

    // ----- navigation -----

    pub fn move_by(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let n = self.entries.len() as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, n - 1) as usize;
    }

    pub fn page(&mut self, down: bool) {
        let per = (self.geom.rows * self.geom.cols.max(1)).max(1) as isize;
        let step = if self.columns.multi { per } else { per - 1 };
        self.move_by(if down { step.max(1) } else { -step.max(1) });
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.entries.len().saturating_sub(1);
    }

    /// Left/Right: previous/next column in multi-column modes, page otherwise.
    pub fn column_move(&mut self, right: bool) {
        if self.columns.multi {
            let rows = self.geom.rows.max(1) as isize;
            self.move_by(if right { rows } else { -rows });
        } else {
            self.page(right);
        }
    }

    /// Changes directory. `to` is a child name, `..` or an absolute path.
    pub fn cd(&mut self, to: &str) -> io::Result<()> {
        let from_name = if to == ".." {
            if self.is_local() {
                self.local_dir().and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
            } else {
                let cur = self.provider().current_dir();
                cur.trim_end_matches('/').rsplit('/').next().map(String::from).filter(|s| !s.is_empty())
            }
        } else {
            None
        };
        match self.provider_mut().change_dir(to)? {
            ChangeDir::Changed => {
                self.after_cd();
                if let Some(n) = from_name {
                    self.set_cursor_name(&n);
                }
            }
            ChangeDir::Exit => {
                self.pop_provider();
            }
            ChangeDir::Local(dir) => {
                self.set_local_dir(dir)?;
            }
        }
        Ok(())
    }

    fn after_cd(&mut self) {
        self.filter = None;
        self.fast_find = None;
        self.cursor = 0;
        self.top = 0;
        self.load_entries();
        self.set_mode(self.mode);
    }

    /// Enters a provider (archive, temporary panel…) on top of the stack.
    pub fn push_provider(&mut self, p: Box<dyn VfsProvider>, name: String) {
        self.stack.push(p);
        self.stack_names.push(name);
        self.after_cd();
    }

    /// Leaves the top provider; the cursor goes to the item it was opened from.
    pub fn pop_provider(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        self.stack.pop();
        let name = self.stack_names.pop().unwrap_or_default();
        self.after_cd();
        let short = name.rsplit(['/', '\\']).next().unwrap_or(&name).to_string();
        self.set_cursor_name(&short);
    }

    /// Replaces the whole stack with a local directory.
    pub fn set_local_dir(&mut self, dir: PathBuf) -> io::Result<()> {
        std::fs::read_dir(&dir)?;
        self.stack.truncate(1);
        self.stack_names.clear();
        self.stack[0] = Box::new(LocalFs::new(dir));
        self.after_cd();
        Ok(())
    }

    pub fn enter(&mut self) -> io::Result<EnterResult> {
        let Some(e) = self.current().cloned() else { return Ok(EnterResult::Nothing) };
        if e.is_dir {
            self.cd(&e.name)?;
            Ok(EnterResult::Changed)
        } else {
            Ok(EnterResult::File(e.name))
        }
    }

    /// Ctrl+\: go to the root.
    pub fn go_root(&mut self) -> io::Result<()> {
        if !self.is_local() {
            while !self.is_local() {
                self.pop_provider();
            }
            return Ok(());
        }
        if let Some(d) = self.local_dir() {
            let root = gm_platform::drives::volume_root(&d);
            let root = if cfg!(windows) { root } else { d.ancestors().last().map(PathBuf::from).unwrap_or(root) };
            self.set_local_dir(root)?;
        }
        Ok(())
    }

    // ----- fast find (Alt+letters) -----

    pub fn fast_find_push(&mut self, c: char) -> bool {
        let mut prefix = self.fast_find.clone().unwrap_or_default();
        prefix.push(c);
        if self.fast_find_seek(&prefix, 0) {
            self.fast_find = Some(prefix);
            true
        } else {
            false
        }
    }

    pub fn fast_find_back(&mut self) {
        if let Some(p) = &mut self.fast_find {
            p.pop();
        }
    }

    /// Ctrl+Enter in fast find: next match.
    pub fn fast_find_next(&mut self) {
        if let Some(p) = self.fast_find.clone() {
            let start = self.cursor + 1;
            if !self.fast_find_seek(&p, start) {
                self.fast_find_seek(&p, 0);
            }
        }
    }

    fn fast_find_seek(&mut self, prefix: &str, from: usize) -> bool {
        let lp = prefix.to_lowercase();
        let wild = lp.contains(['*', '?']);
        let mask = if wild { FileMask::new(&format!("{lp}*")).ok() } else { None };
        let hit = self.entries.iter().enumerate().skip(from).find(|(_, e)| match &mask {
            Some(m) => m.matches(&e.name),
            None => e.name.to_lowercase().starts_with(&lp),
        });
        match hit {
            Some((i, _)) => {
                self.cursor = i;
                true
            }
            None => false,
        }
    }

    /// Index of the entry under a screen position (mouse).
    pub fn hit(&self, x: u16, y: u16) -> Option<usize> {
        let g = self.geom;
        if y < g.first_row_y || x < g.x {
            return None;
        }
        let row = (y - g.first_row_y) as usize;
        if row >= g.rows {
            return None;
        }
        let col = if g.col_width > 0 { ((x - g.x) / g.col_width) as usize } else { 0 };
        if col >= g.cols.max(1) {
            return None;
        }
        let idx = self.top + col * g.rows + row;
        (idx < self.entries.len()).then_some(idx)
    }

    /// Rows visible after the last render (for scrolling with the mouse wheel).
    pub fn visible_rows(&self) -> usize {
        self.geom.rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> (FilePanel, PathBuf) {
        let d = std::env::temp_dir().join(format!("gm-panel-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        for n in ["b.txt", "a.rs", "c.zip"] {
            std::fs::write(d.join(n), n).unwrap();
        }
        let p = FilePanel::new(d.clone(), PanelOptions::default(), Rc::new(Highlighter::default()));
        (p, d)
    }

    #[test]
    fn listing_selection_and_cd() {
        let (mut p, d) = panel();
        let names: Vec<&str> = p.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["..", "sub", "a.rs", "b.txt", "c.zip"]);
        p.select_mask(&FileMask::new("*.txt,*.rs").unwrap(), true);
        assert_eq!(p.selected_names(), ["a.rs", "b.txt"]);
        p.invert_selection();
        assert_eq!(p.selected_names(), ["c.zip"]);
        p.clear_selection();
        p.restore_selection();
        assert_eq!(p.targets(), ["c.zip"]);
        p.set_cursor_name("sub");
        p.enter().unwrap();
        assert_eq!(p.local_dir().unwrap(), d.join("sub"));
        p.cd("..").unwrap();
        assert_eq!(p.current_name().as_deref(), Some("sub"));
        assert!(p.fast_find_push('b'));
        assert_eq!(p.current_name().as_deref(), Some("b.txt"));
        p.set_sort(SortMode::Ext);
        assert_eq!(p.entries[2].name, "a.rs");
        std::fs::remove_dir_all(&d).unwrap();
    }
}

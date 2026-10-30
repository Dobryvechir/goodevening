//! Folder tree panel (`treelist.cpp`), built lazily: the path to the
//! current folder is expanded, other branches load when opened.

use gm_i18n::{MsgId, msg};
use gm_term::{Buffer, PaletteId, Rect, style};
use gm_ui::draw;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
struct Node {
    path: PathBuf,
    depth: usize,
    expanded: bool,
    last: bool,
}

#[derive(Default)]
pub struct TreePanel {
    nodes: Vec<Node>,
    pub cursor: usize,
    top: usize,
    rows: usize,
}

fn subdirs(p: &Path, hidden: bool) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(p)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter(|e| hidden || !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| gm_text::sort::cmp_natural(&a.to_string_lossy(), &b.to_string_lossy()));
    v
}

impl TreePanel {
    /// Builds the tree from the volume root down to `current`.
    pub fn build(&mut self, current: &Path, show_hidden: bool) {
        let root = if cfg!(windows) { gm_platform::drives::volume_root(current) } else { PathBuf::from("/") };
        self.nodes = vec![Node { path: root.clone(), depth: 0, expanded: true, last: true }];
        let mut i = 0;
        while i < self.nodes.len() {
            let n = self.nodes[i].clone();
            if n.expanded || current.starts_with(&n.path) {
                self.nodes[i].expanded = true;
                let kids = subdirs(&n.path, show_hidden);
                let count = kids.len();
                let new: Vec<Node> = kids
                    .into_iter()
                    .enumerate()
                    .map(|(k, p)| Node { expanded: current.starts_with(&p) && p != *current, path: p, depth: n.depth + 1, last: k + 1 == count })
                    .collect();
                self.nodes.splice(i + 1..i + 1, new);
            }
            i += 1;
        }
        self.cursor = self.nodes.iter().position(|n| n.path == current).unwrap_or(0);
    }

    pub fn current(&self) -> Option<&Path> {
        self.nodes.get(self.cursor).map(|n| n.path.as_path())
    }

    pub fn move_by(&mut self, d: isize) {
        let n = self.nodes.len() as isize;
        if n > 0 {
            self.cursor = (self.cursor as isize + d).clamp(0, n - 1) as usize;
        }
    }

    pub fn page(&mut self, down: bool) {
        let r = self.rows.max(2) as isize - 1;
        self.move_by(if down { r } else { -r });
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.nodes.len().saturating_sub(1);
    }

    /// Right / "+": expand the folder under the cursor.
    pub fn expand(&mut self, show_hidden: bool) {
        let Some(n) = self.nodes.get(self.cursor).cloned() else { return };
        if n.expanded {
            return;
        }
        self.nodes[self.cursor].expanded = true;
        let kids = subdirs(&n.path, show_hidden);
        let count = kids.len();
        let new: Vec<Node> =
            kids.into_iter().enumerate().map(|(k, p)| Node { path: p, depth: n.depth + 1, expanded: false, last: k + 1 == count }).collect();
        self.nodes.splice(self.cursor + 1..self.cursor + 1, new);
    }

    /// Left / "-": collapse, or move to the parent when already collapsed.
    pub fn collapse(&mut self) {
        let Some(n) = self.nodes.get(self.cursor).cloned() else { return };
        if n.expanded && self.cursor > 0 {
            let end = (self.cursor + 1..self.nodes.len()).find(|&j| self.nodes[j].depth <= n.depth).unwrap_or(self.nodes.len());
            self.nodes.drain(self.cursor + 1..end);
            self.nodes[self.cursor].expanded = false;
        } else if let Some(p) = (0..self.cursor).rev().find(|&j| self.nodes[j].depth < n.depth) {
            self.cursor = p;
        }
    }

    pub fn render(&mut self, buf: &mut Buffer, r: Rect, active: bool) {
        let text = style(PaletteId::PanelText);
        let boxs = style(PaletteId::PanelBox);
        let cur = style(PaletteId::PanelCursor);
        draw::fill(buf, r, ' ', text);
        draw::frame(buf, r, true, boxs, None);
        let title = style(if active { PaletteId::PanelTitleSelected } else { PaletteId::PanelTitle });
        draw::title_at(buf, r, r.y, msg(MsgId::MTreeTitle), title);
        let rows = r.height.saturating_sub(4) as usize;
        self.rows = rows;
        if self.cursor < self.top {
            self.top = self.cursor;
        }
        if self.cursor >= self.top + rows {
            self.top = self.cursor + 1 - rows;
        }
        // Precompute which depths still have siblings below (for │ guides).
        for row in 0..rows {
            let i = self.top + row;
            let Some(n) = self.nodes.get(i) else { break };
            let mut prefix = String::new();
            for d in 1..n.depth {
                // Is there a later sibling at depth d along this branch?
                let more = self.nodes[i + 1..].iter().take_while(|m| m.depth >= d).any(|m| m.depth == d);
                prefix.push_str(if more { "│ " } else { "  " });
            }
            if n.depth > 0 {
                prefix.push_str(if n.last { "└─" } else { "├─" });
            }
            let name = if n.depth == 0 {
                n.path.display().to_string()
            } else {
                n.path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
            };
            let y = r.y + 1 + row as u16;
            let px = draw::put(buf, r.x + 1, y, &prefix, text, (r.width - 2) as usize) as u16;
            let st = if active && i == self.cursor { cur } else { text };
            draw::put(buf, r.x + 1 + px, y, &name, st, (r.width - 2).saturating_sub(px) as usize);
        }
        draw::separator(buf, r.x, r.bottom() - 3, r.width, true, boxs);
        if let Some(p) = self.current() {
            draw::put_fit(
                buf,
                r.x + 1,
                r.bottom() - 2,
                &gm_text::truncate_left(&p.display().to_string(), (r.width - 2) as usize),
                text,
                (r.width - 2) as usize,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builds_to_current() {
        let cur = std::env::current_dir().unwrap();
        let mut t = TreePanel::default();
        t.build(&cur, true);
        assert_eq!(t.current(), Some(cur.as_path()));
        t.collapse();
        t.home();
        assert!(t.current().is_some());
    }
}

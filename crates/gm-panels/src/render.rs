//! Drawing the file list panel in Far's layout.

use crate::columns::ColumnKind;
use crate::filepanel::{FilePanel, Geometry};
use crate::format;
use gm_i18n::{MsgId, msg};
use gm_term::{Buffer, PaletteId, Rect, Style, palette, style};
use gm_text::width::str_width;
use gm_text::{fit, truncate_left};
use gm_ui::draw;
use gm_vfs::Entry;

fn size_text(e: &Entry, w: usize, packed: bool) -> String {
    let br = msg(MsgId::MListBrackets);
    let (l, r) = (br.chars().next().unwrap_or('<'), br.chars().nth(1).unwrap_or('>'));
    if e.is_dotdot() {
        return format!("{l}{}{r}", msg(MsgId::MListUp));
    }
    if e.is_dir {
        let word = if e.is_link { msg(MsgId::MListSymlink) } else { msg(MsgId::MListFolder) };
        return format!("{l}{word}{r}");
    }
    let n = if packed { e.packed.unwrap_or(e.size) } else { e.size };
    draw::short_size(n, w)
}

fn cell_text(e: &Entry, kind: ColumnKind, w: usize) -> (String, bool) {
    // (text, right-aligned)
    match kind {
        ColumnKind::Name => (e.name.clone(), false),
        ColumnKind::Size => (size_text(e, w, false), true),
        ColumnKind::Packed => (size_text(e, w, true), true),
        ColumnKind::Date => (if e.is_dotdot() { String::new() } else { format::date(e.modified) }, false),
        ColumnKind::Time => (if e.is_dotdot() { String::new() } else { format::time(e.modified) }, false),
        ColumnKind::Modified => (format::datetime(e.modified, w), false),
        ColumnKind::Created => (format::datetime(e.created, w), false),
        ColumnKind::Accessed => (format::datetime(e.accessed, w), false),
        ColumnKind::Attributes => (if e.is_dotdot() { String::new() } else { e.attrs.format(e.is_dir, e.is_link) }, false),
        ColumnKind::Description => (e.description.clone().unwrap_or_default(), false),
        ColumnKind::Owner => (e.owner.clone(), false),
        ColumnKind::Links => (e.links.map(|n| n.to_string()).unwrap_or_default(), true),
        ColumnKind::Custom(i) => (e.custom.get(i as usize).cloned().unwrap_or_default(), true),
    }
}

fn column_title(kind: ColumnKind, custom: &Option<Vec<gm_vfs::CustomColumn>>) -> String {
    match kind {
        ColumnKind::Name => msg(MsgId::MColumnName).into(),
        ColumnKind::Size | ColumnKind::Packed => msg(MsgId::MColumnSize).into(),
        ColumnKind::Date => msg(MsgId::MColumnDate).into(),
        ColumnKind::Time => msg(MsgId::MColumnTime).into(),
        ColumnKind::Modified => msg(MsgId::MColumnWrited).into(),
        ColumnKind::Created => msg(MsgId::MColumnCreated).into(),
        ColumnKind::Accessed => msg(MsgId::MColumnAccessed).into(),
        ColumnKind::Attributes => msg(MsgId::MColumnAttr).into(),
        ColumnKind::Description => msg(MsgId::MColumnDescription).into(),
        ColumnKind::Owner => msg(MsgId::MColumnOwner).into(),
        ColumnKind::Links => msg(MsgId::MColumnMumLinks).into(),
        ColumnKind::Custom(i) => custom.as_ref().and_then(|c| c.get(i as usize)).map(|c| c.title.clone()).unwrap_or_default(),
    }
}

/// Replaces the background of `fg_style` with the one from `bg_of`.
fn with_fg(base: Style, fg: Option<gm_term::C16>) -> Style {
    match fg {
        Some(c) => base.fg(c.to_color(palette::color_mode())),
        None => base,
    }
}

impl FilePanel {
    pub fn render(&mut self, buf: &mut Buffer, r: Rect, active: bool) {
        if r.width < 8 || r.height < 5 {
            return;
        }
        let text = style(PaletteId::PanelText);
        let boxs = style(PaletteId::PanelBox);
        draw::fill(buf, r, ' ', text);
        draw::frame(buf, r, true, boxs, None);

        let status_rows: u16 = if self.opts.status_line { 2 } else { 0 };
        let title_rows: u16 = u16::from(self.opts.column_titles);
        let inner_x = r.x + 1;
        let inner_w = (r.width - 2) as usize;
        let first_row_y = r.y + 1 + title_rows;
        let rows = r.height.saturating_sub(2 + title_rows + status_rows) as usize;

        // Column layout: multi-column modes repeat the name column.
        let widths = self.columns.layout(inner_w);
        let multi = self.columns.multi;
        let cols = if multi { widths.len() } else { 1 };
        let mut xs = Vec::with_capacity(widths.len());
        let mut x = inner_x;
        for w in &widths {
            xs.push(x);
            x += *w as u16 + 1;
        }
        self.geom = Geometry {
            x: inner_x,
            rows,
            cols,
            col_width: if multi { (widths.first().copied().unwrap_or(1) + 1) as u16 } else { inner_w as u16 },
            first_row_y,
        };

        // Keep the cursor visible.
        let per_page = (rows * cols).max(1);
        if self.cursor < self.top {
            self.top = if multi { self.cursor - self.cursor % rows.max(1) } else { self.cursor };
        }
        if self.cursor >= self.top + per_page {
            self.top = if multi { (self.cursor / rows.max(1) + 1).saturating_sub(cols) * rows.max(1) } else { self.cursor + 1 - per_page };
        }
        if !multi && self.top + per_page > self.entries.len() {
            self.top = self.entries.len().saturating_sub(per_page);
        }

        // Column separators (joined with the frame).
        let sep_bottom = if self.opts.status_line { r.bottom() - 3 } else { r.bottom() - 1 };
        for (i, &cx) in xs.iter().enumerate().skip(1) {
            let sx = cx - 1;
            let _ = i;
            draw::set(buf, sx, r.y, '╤', boxs);
            for y in r.y + 1..sep_bottom {
                draw::set(buf, sx, y, '│', boxs);
            }
            draw::set(buf, sx, sep_bottom, if self.opts.status_line { '┴' } else { '╧' }, boxs);
        }

        // Column titles.
        if self.opts.column_titles {
            let ts = style(PaletteId::PanelTitleColumn);
            let custom = self.provider().columns();
            for (i, w) in widths.iter().enumerate() {
                let t = column_title(self.columns.columns[i].kind, &custom);
                let t = gm_text::width::center(&gm_text::truncate_right(&t, *w), *w);
                draw::put(buf, xs[i], r.y + 1, &t, ts, *w);
            }
        }

        // Entries.
        let cursor_style = style(PaletteId::PanelCursor);
        let sel_style = style(PaletteId::PanelTextSelected);
        let sel_cursor = style(PaletteId::PanelCursorSelected);
        for c in 0..cols {
            for row in 0..rows {
                let idx = self.top + c * rows + row;
                let Some(e) = self.entries.get(idx) else { continue };
                let y = first_row_y + row as u16;
                let is_cursor = active && idx == self.cursor;
                let is_sel = self.selected.get(idx).copied().unwrap_or(false);
                let hl = if self.opts.highlight { self.highlighter.get(e) } else { Default::default() };
                let st = match (is_cursor, is_sel) {
                    (true, true) => sel_cursor,
                    (true, false) => with_fg(cursor_style, hl.cursor_color),
                    (false, true) => sel_style,
                    (false, false) => with_fg(text, hl.color),
                };
                if multi {
                    let w = widths[c];
                    let name = name_cell(e, w, hl.mark);
                    draw::put_fit(buf, xs[c], y, &name, st, w);
                } else {
                    // One row with all columns; the cursor bar spans every column.
                    for (ci, w) in widths.iter().enumerate() {
                        let kind = self.columns.columns[ci].kind;
                        let (t, right) = cell_text(e, kind, *w);
                        let t = if kind == ColumnKind::Name { name_cell(e, *w, hl.mark) } else { t };
                        let s = if right && str_width(&t) < *w { format!("{t:>width$}", width = *w) } else { gm_text::truncate_right(&t, *w) };
                        draw::put_fit(buf, xs[ci], y, &s, st, *w);
                        if is_cursor && ci + 1 < widths.len() {
                            draw::set(buf, xs[ci] + *w as u16, y, '│', with_fg(cursor_style, None).fg(boxs.fg.unwrap_or_default()));
                        }
                    }
                }
            }
        }

        // Status line.
        if self.opts.status_line {
            draw::separator(buf, r.x, r.bottom() - 3, r.width, true, boxs);
            for (i, &cx) in xs.iter().enumerate().skip(1) {
                let _ = i;
                draw::set(buf, cx - 1, r.bottom() - 3, '┴', boxs);
            }
            let y = r.bottom() - 2;
            if let Some(err) = &self.error {
                draw::put_fit(buf, inner_x, y, err, style(PaletteId::PanelTextInfo), inner_w);
            } else if let Some(e) = self.current() {
                let right = if e.is_dir {
                    format!("{} {}", size_text(e, 10, false), format::date(e.modified))
                } else {
                    format!("{} {} {}", draw::short_size(e.size, 13), format::date(e.modified), format::time(e.modified))
                };
                let rw = str_width(&right);
                let mut name = e.name.clone();
                if let Some(t) = &e.link_target {
                    name = format!("{name} → {t}");
                }
                if let Some(d) = &e.description
                    && self.columns.columns.iter().all(|c| c.kind != ColumnKind::Description)
                {
                    name = format!("{name}  {d}");
                }
                let nw = inner_w.saturating_sub(rw + 1);
                draw::put_fit(buf, inner_x, y, &fit(&name, nw), text, nw);
                draw::put(buf, inner_x + nw as u16 + 1, y, &right, text, rw);
            }
        }

        // Title: path, highlighted when active.
        let title_style = style(if active { PaletteId::PanelTitleSelected } else { PaletteId::PanelTitle });
        let title = truncate_left(&self.display_dir(), (r.width as usize).saturating_sub(6));
        draw::title_at(buf, r, r.y, &title, title_style);
        // Sort mode letter in the top-left corner.
        let letter = if self.reverse { self.sort.letter().to_ascii_uppercase() } else { self.sort.letter() };
        draw::set(buf, r.x + 1, r.y, letter, style(PaletteId::PanelTitleColumn));
        if let Some((f, _)) = &self.filter {
            draw::set(buf, r.x + 2, r.y, '*', style(PaletteId::PanelTitleColumn));
            let _ = f;
        }

        // Totals on the bottom border.
        let selected = self.selected_count();
        let (info, st) = if selected > 0 {
            (
                format!(" {} ", gm_i18n::fmt(MsgId::MListFileSize, &[&draw::group_digits(self.selected_size()), &selected, &self.selected_dirs()])),
                style(PaletteId::PanelInfoSelected),
            )
        } else {
            let files = self.entries.iter().filter(|e| !e.is_dir).count();
            let dirs = self.entries.iter().filter(|e| e.is_dir && !e.is_dotdot()).count();
            let bytes: u64 = self.entries.iter().map(|e| e.size).sum();
            (format!(" {} ", gm_i18n::fmt(MsgId::MListFileSize, &[&draw::group_digits(bytes), &files, &dirs])), style(PaletteId::PanelInfoTotal))
        };
        if self.opts.total_info {
            draw::title_at(buf, r, r.bottom() - 1, info.trim(), st);
        }
        if self.opts.scrollbar && !multi {
            draw::scrollbar(buf, r.right() - 1, first_row_y, rows as u16, self.top, self.entries.len(), rows, style(PaletteId::PanelScrollbar));
        }
        // Fast find box.
        if active && let Some(ff) = &self.fast_find {
            let w = 24.min(r.width - 2);
            let fr = Rect::new(r.x + (r.width - w) / 2, r.bottom().saturating_sub(3), w, 3);
            let ds = style(PaletteId::DialogText);
            draw::fill(buf, fr, ' ', ds);
            draw::frame(buf, fr, true, style(PaletteId::DialogBox), None);
            draw::put_fit(buf, fr.x + 2, fr.y + 1, ff, style(PaletteId::DialogEdit), (w - 4) as usize);
        }
    }

    fn selected_dirs(&self) -> usize {
        self.entries.iter().zip(&self.selected).filter(|(e, s)| **s && e.is_dir).count()
    }

    /// Cursor position of the fast-find box (for the hardware cursor).
    pub fn fast_find_cursor(&self, r: Rect) -> Option<(u16, u16)> {
        let ff = self.fast_find.as_ref()?;
        let w = 24.min(r.width - 2);
        Some((r.x + (r.width - w) / 2 + 2 + str_width(ff).min((w - 5) as usize) as u16, r.bottom().saturating_sub(2)))
    }
}

/// Name cell with Far's `}` marker for truncated names.
fn name_cell(e: &Entry, w: usize, mark: Option<char>) -> String {
    let mut name = match mark {
        Some(m) => format!("{m}{}", e.name),
        None => e.name.clone(),
    };
    if str_width(&name) > w && w > 1 {
        let (p, _) = gm_text::width::prefix_by_width(&name, w - 1);
        name = format!("{p}}}");
    }
    name
}

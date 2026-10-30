//! Find File dialog and the streaming results window.

use crate::engine::{Area, FindMsg, FindParams, start};
use crossbeam_channel::Receiver;
use gm_i18n::{MsgId, fmt, msg};
use gm_term::{Buffer, Event, KeyCode, Mods, MouseButton, MouseEventKind, PaletteId, Rect, style};
use gm_text::Codepage;
use gm_ui::dialog::DialogBuilder;
use gm_ui::{Flow, Modal, Ui, draw};
use gm_vfs::CancelToken;
use std::path::PathBuf;

/// Shows the Alt+F7 dialog. Returns the parameters, or None on cancel.
pub fn find_dialog(ui: &mut Ui, last: &FindParams, have_selection: bool) -> Option<FindParams> {
    let mut b = DialogBuilder::new(msg(MsgId::MFindFileTitle), 76).help("FindFile");
    b.text(msg(MsgId::MFindFileMasks));
    let masks = b.edit(&last.masks, Some("Masks"));
    b.separator_text(msg(MsgId::MFindFileContaining));
    let text = b.edit(&last.text.text, Some("SearchText"));
    let r_text = b.radio(msg(MsgId::MFindFileText), !last.hex, true);
    let r_hex = b.radio(msg(MsgId::MFindFileHex), last.hex, false);
    b.text(msg(MsgId::MFindFileCodePage));
    let mut cps = vec![gm_i18n::plain(MsgId::MFindFileAllCodePages)];
    let list = Codepage::menu_list();
    cps.extend(list.iter().map(Codepage::name));
    let cur_cp = last.codepage.map(|c| c.name()).unwrap_or_else(|| cps[0].clone());
    let cp = b.combo(&cur_cp, cps.clone(), true);
    let case = b.checkbox(msg(MsgId::MFindFileCase), last.text.case_sensitive);
    let words = b.checkbox_right(msg(MsgId::MFindFileWholeWords), last.text.whole_words);
    let regex = b.checkbox(gm_i18n::msg(MsgId::MSearchReplaceRegexp), last.text.regex);
    let not = b.checkbox_right(msg(MsgId::MFindFileNotContaining), last.not_containing);
    let folders = b.checkbox(msg(MsgId::MFindFileFolders), last.folders);
    let links = b.checkbox_right(msg(MsgId::MFindFileSymLinks), last.follow_links);
    b.separator_text(msg(MsgId::MFindFileSearchArea));
    let areas = [
        (Area::AllDrives, MsgId::MFindFileSearchAllDisks),
        (Area::Path, MsgId::MFindFileSearchInPATH),
        (Area::Root, MsgId::MFindFileSearchFromRootFolder),
        (Area::Current, MsgId::MFindFileSearchFromCurrent),
        (Area::CurrentOnly, MsgId::MFindFileSearchInCurrent),
        (Area::Selected, MsgId::MFindFileSearchInSelected),
    ];
    let mut area_items = Vec::new();
    for (i, (a, m)) in areas.iter().enumerate() {
        let id = b.radio(msg(*m), *a == last.area, i == 0);
        area_items.push((id, *a));
    }
    if !have_selection && let Some((id, _)) = area_items.last() {
        let _ = id;
    }
    b.separator();
    b.buttons(&[msg(MsgId::MFindFileFind), msg(MsgId::MFindFileCancel)], 0);
    let mut d = b.build();
    let first_btn = d.items.len() - 2;
    let r = d.run(ui)?;
    if r == first_btn + 1 {
        return None;
    }
    let _ = r_text;
    let cp_name = d.text(cp);
    let codepage = list.iter().find(|c| c.name() == cp_name).copied();
    let mut p = FindParams {
        masks: d.text(masks),
        hex: d.checked(r_hex),
        codepage,
        not_containing: d.checked(not),
        folders: d.checked(folders),
        follow_links: d.checked(links),
        area: area_items.iter().find(|(id, _)| d.checked(*id)).map(|(_, a)| *a).unwrap_or(Area::Current),
        ..Default::default()
    };
    p.text.text = d.text(text);
    p.text.case_sensitive = d.checked(case);
    p.text.whole_words = d.checked(words);
    p.text.regex = d.checked(regex);
    Some(p)
}

#[derive(Clone, Debug)]
pub enum FindOutcome {
    None,
    GoTo(PathBuf),
    View(PathBuf),
    Edit(PathBuf),
    Panel(Vec<PathBuf>),
    NewSearch,
}

struct Results {
    rx: Receiver<FindMsg>,
    cancel: CancelToken,
    items: Vec<(PathBuf, bool)>,
    sel: usize,
    top: usize,
    done: bool,
    scanning: String,
    error: Option<String>,
    outcome: FindOutcome,
    rect: Rect,
    rows: usize,
    focus_btn: usize,
    title_what: String,
}

impl Results {
    fn drain(&mut self) -> bool {
        let mut changed = false;
        for _ in 0..10_000 {
            match self.rx.try_recv() {
                Ok(FindMsg::Found { path, is_dir }) => {
                    self.items.push((path, is_dir));
                    changed = true;
                }
                Ok(FindMsg::Scanning(p)) => {
                    self.scanning = p.display().to_string();
                    changed = true;
                }
                Ok(FindMsg::Done) => {
                    self.done = true;
                    changed = true;
                }
                Ok(FindMsg::Error(e)) => {
                    self.error = Some(e);
                    changed = true;
                }
                Err(_) => break,
            }
        }
        changed
    }

    fn current(&self) -> Option<PathBuf> {
        self.items.get(self.sel).map(|i| i.0.clone())
    }

    fn buttons(&self) -> [&'static str; 5] {
        [
            msg(MsgId::MFindFileNewSearch),
            msg(MsgId::MFindFileGoTo),
            msg(MsgId::MFindFileView),
            msg(MsgId::MFindFilePanel),
            if self.done { msg(MsgId::MFindFileCancel) } else { msg(MsgId::MFindFileStop) },
        ]
    }

    fn press(&mut self, i: usize) -> Flow {
        match i {
            0 => {
                self.cancel.cancel();
                self.outcome = FindOutcome::NewSearch;
                Flow::Done
            }
            1 => match self.current() {
                Some(p) => {
                    self.cancel.cancel();
                    self.outcome = FindOutcome::GoTo(p);
                    Flow::Done
                }
                None => Flow::Continue,
            },
            2 => match self.items.get(self.sel) {
                Some((p, false)) => {
                    self.outcome = FindOutcome::View(p.clone());
                    self.cancel.cancel();
                    Flow::Done
                }
                _ => Flow::Continue,
            },
            3 => {
                self.cancel.cancel();
                self.outcome = FindOutcome::Panel(self.items.iter().map(|i| i.0.clone()).collect());
                Flow::Done
            }
            _ => {
                if self.done {
                    Flow::Done
                } else {
                    self.cancel.cancel();
                    Flow::Continue
                }
            }
        }
    }
}

impl Modal for Results {
    fn render(&mut self, buf: &mut Buffer, screen: Rect) -> Option<(u16, u16)> {
        self.drain();
        let r = Rect::new(screen.x, screen.y + 1, screen.width, screen.height.saturating_sub(2));
        self.rect = r;
        let text = style(PaletteId::DialogText);
        let list = style(PaletteId::DialogListText);
        let lsel = style(PaletteId::DialogListTextSelected);
        draw::fill(buf, r, ' ', text);
        let frame = Rect::new(r.x + 3, r.y + 1, r.width.saturating_sub(6), r.height.saturating_sub(2));
        draw::frame(
            buf,
            frame,
            true,
            style(PaletteId::DialogBox),
            Some((&format!("{}{}", msg(MsgId::MFindFileTitle), self.title_what), style(PaletteId::DialogBoxTitle))),
        );
        draw::shadow(buf, r);
        let lx = frame.x + 1;
        let lw = frame.width.saturating_sub(2) as usize;
        let rows = frame.height.saturating_sub(6) as usize;
        self.rows = rows;
        if self.sel < self.top {
            self.top = self.sel;
        }
        if self.sel >= self.top + rows.max(1) {
            self.top = self.sel + 1 - rows.max(1);
        }
        for row in 0..rows {
            let i = self.top + row;
            let y = frame.y + 1 + row as u16;
            let Some((p, is_dir)) = self.items.get(i) else {
                draw::fill(buf, Rect::new(lx, y, lw as u16, 1), ' ', list);
                continue;
            };
            let s = if i == self.sel { lsel } else { list };
            let name = p.display().to_string();
            let line = if *is_dir { format!("{name}{}", std::path::MAIN_SEPARATOR) } else { name };
            draw::put_fit(buf, lx, y, &gm_text::truncate_left(&line, lw.saturating_sub(1)), s, lw);
        }
        let sep_y = frame.y + 1 + rows as u16;
        draw::separator(buf, frame.x, sep_y, frame.width, true, style(PaletteId::DialogBox));
        let files = self.items.iter().filter(|i| !i.1).count();
        let dirs = self.items.len() - files;
        let status = if let Some(e) = &self.error {
            e.clone()
        } else if self.done {
            fmt(MsgId::MFindFileDone, &[&files, &dirs])
        } else {
            format!("{} {}", fmt(MsgId::MFindFileFound, &[&files, &dirs]), self.scanning)
        };
        draw::put_fit(buf, lx + 1, sep_y + 1, &gm_text::truncate_left(&status, lw.saturating_sub(2)), text, lw.saturating_sub(2));
        draw::separator(buf, frame.x, sep_y + 2, frame.width, true, style(PaletteId::DialogBox));
        // Buttons.
        let labels = self.buttons();
        let total: usize = labels.iter().map(|l| draw::label_width(l) + 5).sum();
        let mut x = r.x + (r.width.saturating_sub(total as u16)) / 2;
        let by = sep_y + 3;
        for (i, l) in labels.iter().enumerate() {
            let (bs, bh) = if i == self.focus_btn {
                (style(PaletteId::DialogButtonSelected), style(PaletteId::DialogButtonHighlightSelected))
            } else {
                (style(PaletteId::DialogButton), style(PaletteId::DialogButtonHighlight))
            };
            x += draw::put(buf, x, by, "[ ", bs, 2) as u16;
            x += draw::hotkey_text(buf, x, by, l, bs, bh, 40) as u16;
            x += draw::put(buf, x, by, " ]", bs, 2) as u16 + 1;
        }
        None
    }

    fn handle(&mut self, ev: &Event, _ui: &mut Ui) -> Flow {
        self.drain();
        match ev {
            Event::Key(k) => match (k.code, k.mods) {
                (KeyCode::Esc, _) => {
                    if self.done {
                        return Flow::Done;
                    }
                    self.cancel.cancel();
                }
                (KeyCode::Up, _) => self.sel = self.sel.saturating_sub(1),
                (KeyCode::Down, _) => self.sel = (self.sel + 1).min(self.items.len().saturating_sub(1)),
                (KeyCode::PageUp, _) => self.sel = self.sel.saturating_sub(self.rows.max(1)),
                (KeyCode::PageDown, _) => self.sel = (self.sel + self.rows.max(1)).min(self.items.len().saturating_sub(1)),
                (KeyCode::Home, Mods::CTRL) | (KeyCode::Home, Mods::NONE) => self.sel = 0,
                (KeyCode::End, _) => self.sel = self.items.len().saturating_sub(1),
                (KeyCode::Tab, Mods::NONE) | (KeyCode::Right, Mods::NONE) => self.focus_btn = (self.focus_btn + 1) % 5,
                (KeyCode::Tab, Mods::SHIFT) | (KeyCode::Left, Mods::NONE) => self.focus_btn = (self.focus_btn + 4) % 5,
                (KeyCode::Enter, _) => return self.press(if self.focus_btn == 0 { 1 } else { self.focus_btn }),
                (KeyCode::F(3), Mods::NONE) => return self.press(2),
                (KeyCode::F(4), Mods::NONE) => {
                    if let Some((p, false)) = self.items.get(self.sel) {
                        self.outcome = FindOutcome::Edit(p.clone());
                        self.cancel.cancel();
                        return Flow::Done;
                    }
                }
                (KeyCode::Char(c), m) if m.alt() || !m.ctrl() => {
                    let lc = c.to_lowercase().next().unwrap_or(c);
                    if let Some(i) = self.buttons().iter().position(|l| draw::hotkey_of(l) == Some(lc)) {
                        return self.press(i);
                    }
                }
                _ => {}
            },
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollDown => self.sel = (self.sel + 1).min(self.items.len().saturating_sub(1)),
                MouseEventKind::ScrollUp => self.sel = self.sel.saturating_sub(1),
                MouseEventKind::Down(MouseButton::Left) => {
                    let row = m.row as i32 - (self.rect.y as i32 + 2);
                    if row >= 0 && (row as usize) < self.rows {
                        let i = self.top + row as usize;
                        if i < self.items.len() {
                            if i == self.sel {
                                return self.press(1);
                            }
                            self.sel = i;
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Flow::Continue
    }

    fn tick(&mut self, _ui: &mut Ui) -> bool {
        self.drain() || !self.done
    }
}

/// Runs the search and shows results until the user picks an action.
pub fn results_window(ui: &mut Ui, params: FindParams, roots: Vec<PathBuf>) -> FindOutcome {
    let what = if params.text.text.is_empty() { String::new() } else { format!(": \"{}\"", params.text.text) };
    let (rx, cancel) = start(params, roots);
    let mut r = Results {
        rx,
        cancel,
        items: Vec::new(),
        sel: 0,
        top: 0,
        done: false,
        scanning: String::new(),
        error: None,
        outcome: FindOutcome::None,
        rect: Rect::default(),
        rows: 1,
        focus_btn: 1,
        title_what: what,
    };
    let _ = ui.run_modal(&mut r);
    r.cancel.cancel();
    r.outcome
}

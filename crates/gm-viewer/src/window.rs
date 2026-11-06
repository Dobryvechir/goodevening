//! The viewer as a manager window: status line, key bar and key handling.

use crate::colorview::ColorView;
use crate::core::{Mode, ViewerCore};
use crate::syntax::{Lang, lang_for};
use gm_i18n::{MsgId, msg};
use gm_term::{Buffer, Event, KeyChord, KeyCode, Mods, MouseButton, MouseEvent, MouseEventKind, PaletteId, Rect, style};
use gm_text::Codepage;
use gm_ui::dialog::DialogBuilder;
use gm_ui::search::{SearchAction, search_dialog};
use gm_ui::{KeyBar, KeyBarLabels, MenuItem, MenuResult, OpenRequest, Ui, VMenu, WinAction, Window, WindowKind, clipboard, draw, message};

#[derive(Clone, Debug)]
pub struct ViewerOptions {
    pub wrap: bool,
    pub word_wrap: bool,
    pub tab_size: usize,
    pub show_arrows: bool,
    pub keybar: bool,
    pub auto_detect: bool,
    pub default_cp: Codepage,
    pub hex_upper: bool,
}

impl Default for ViewerOptions {
    fn default() -> Self {
        ViewerOptions {
            wrap: true,
            word_wrap: false,
            tab_size: 8,
            show_arrows: true,
            keybar: true,
            auto_detect: true,
            default_cp: Codepage::Utf8,
            hex_upper: true,
        }
    }
}

pub struct ViewerWindow {
    pub core: ViewerCore,
    opts: ViewerOptions,
    labels: KeyBarLabels,
    /// The file is a temporary copy (archive member) removed on close.
    pub temp_file: bool,
    pub title_override: Option<String>,
    /// Language of the file for the "Colors" mode (F5), if supported.
    lang: Option<Lang>,
    /// The syntax-colored view while "Colors" mode is on.
    colors: Option<ColorView>,
    /// A mouse selection is being dragged.
    dragging: bool,
}

/// Files larger than this are not shown in "Colors" mode.
const MAX_COLORS_SIZE: u64 = 32 << 20;

fn labels() -> KeyBarLabels {
    let mut l = KeyBarLabels::default();
    let row = |ids: [MsgId; 12]| -> [String; 12] { ids.map(|i| msg(i).to_string()) };
    l.normal = row([
        MsgId::MViewF1,
        MsgId::MViewF2,
        MsgId::MViewF3,
        MsgId::MViewF4,
        MsgId::MViewF5,
        MsgId::MViewF6,
        MsgId::MViewF7,
        MsgId::MViewF8,
        MsgId::MViewF9,
        MsgId::MViewF10,
        MsgId::MViewF11,
        MsgId::MViewF12,
    ]);
    l.normal[10] = String::new(); // no plugins
    l.shift = row([
        MsgId::MViewShiftF1,
        MsgId::MViewShiftF2,
        MsgId::MViewShiftF3,
        MsgId::MViewShiftF4,
        MsgId::MViewShiftF5,
        MsgId::MViewShiftF6,
        MsgId::MViewShiftF7,
        MsgId::MViewShiftF8,
        MsgId::MViewShiftF9,
        MsgId::MViewShiftF10,
        MsgId::MViewShiftF11,
        MsgId::MViewShiftF12,
    ]);
    l.alt = row([
        MsgId::MViewAltF1,
        MsgId::MViewAltF2,
        MsgId::MViewAltF3,
        MsgId::MViewAltF4,
        MsgId::MViewAltF5,
        MsgId::MViewAltF6,
        MsgId::MViewAltF7,
        MsgId::MViewAltF8,
        MsgId::MViewAltF9,
        MsgId::MViewAltF10,
        MsgId::MViewAltF11,
        MsgId::MViewAltF12,
    ]);
    l.alt[8] = String::new();
    l.ctrl = row([
        MsgId::MViewCtrlF1,
        MsgId::MViewCtrlF2,
        MsgId::MViewCtrlF3,
        MsgId::MViewCtrlF4,
        MsgId::MViewCtrlF5,
        MsgId::MViewCtrlF6,
        MsgId::MViewCtrlF7,
        MsgId::MViewCtrlF8,
        MsgId::MViewCtrlF9,
        MsgId::MViewCtrlF10,
        MsgId::MViewCtrlF11,
        MsgId::MViewCtrlF12,
    ]);
    l
}

impl ViewerWindow {
    pub fn new(mut core: ViewerCore, opts: ViewerOptions) -> ViewerWindow {
        core.wrap = opts.wrap;
        core.word_wrap = opts.word_wrap;
        core.tab = opts.tab_size.max(1);
        core.hex_upper = opts.hex_upper;
        let lang = lang_for(&core.path);
        ViewerWindow { core, opts, labels: labels(), temp_file: false, title_override: None, lang, colors: None, dragging: false }
    }

    /// Turns the syntax-colored "Colors" mode (F5) on or off, keeping the
    /// top line. Does nothing for unsupported file types.
    pub fn set_colors(&mut self, on: bool) {
        if on {
            let Some(lang) = self.lang else { return };
            if self.colors.is_some() || self.core.len() > MAX_COLORS_SIZE {
                return;
            }
            let text = self.core.decoded_text();
            let line = self.core.line_of_offset(self.core.top);
            self.colors = Some(ColorView::new(&text, lang, self.core.tab, line, self.core.wrap));
        } else if let Some(cv) = self.colors.take() {
            self.sync_from(&cv);
        }
    }

    /// Mouse: wheel scrolls; left button press and drag select text.
    fn handle_mouse(&mut self, m: &MouseEvent) {
        let (x, y) = (m.column, m.row);
        match m.kind {
            MouseEventKind::ScrollDown => match &mut self.colors {
                Some(cv) => cv.down(3),
                None => self.core.down(3),
            },
            MouseEventKind::ScrollUp => match &mut self.colors {
                Some(cv) => cv.up(3),
                None => self.core.up(3),
            },
            MouseEventKind::Down(MouseButton::Left) => {
                self.dragging = false;
                match &mut self.colors {
                    Some(cv) => {
                        cv.sel = None;
                        if cv.inside(x, y) {
                            let p = cv.pos_at(x, y);
                            cv.sel = Some((p, p));
                            self.dragging = true;
                        }
                    }
                    None => {
                        let c = &mut self.core;
                        c.sel = None;
                        if c.inside(x, y)
                            && let Some(o) = c.offset_at(x, y)
                        {
                            c.sel = Some((o, o));
                            self.dragging = true;
                        }
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => match &mut self.colors {
                // Dragging above or below the text scrolls it.
                Some(cv) => {
                    let v = cv.view();
                    if y < v.y {
                        cv.up(1);
                    } else if y >= v.bottom() {
                        cv.down(1);
                    }
                    let p = cv.pos_at(x, y);
                    if let Some((a, _)) = cv.sel {
                        cv.sel = Some((a, p));
                    }
                }
                None => {
                    let c = &mut self.core;
                    let v = c.view();
                    if y < v.y {
                        c.up(1);
                    } else if y >= v.bottom() {
                        c.down(1);
                    }
                    if let (Some((a, _)), Some(o)) = (c.sel, c.offset_at(x, y)) {
                        c.sel = Some((a, o));
                    }
                }
            },
            MouseEventKind::Up(MouseButton::Left) => self.dragging = false,
            _ => {}
        }
    }

    /// Copies the mouse selection to the clipboard.
    fn copy(&self) {
        let text = match &self.colors {
            Some(cv) => cv.selected_text(),
            None => self.core.selected_text(),
        };
        if let Some(t) = text {
            clipboard::set(&t);
        }
    }

    /// Moves the plain view to the colored view's position.
    fn sync_from(&mut self, cv: &ColorView) {
        let off = self.core.offset_of_line(cv.top_line());
        self.core.goto_offset(off);
        self.core.left = cv.left;
    }

    /// Keys of the "Colors" mode; returns true if handled.
    fn handle_colors(&mut self, code: KeyCode, mods: Mods) -> bool {
        let Some(cv) = self.colors.as_mut() else { return false };
        match (code, mods) {
            (KeyCode::Down, Mods::NONE) => cv.down(1),
            (KeyCode::Up, Mods::NONE) => cv.up(1),
            (KeyCode::PageDown, Mods::NONE) | (KeyCode::Char(' '), Mods::NONE) => cv.page_down(),
            (KeyCode::PageUp, Mods::NONE) => cv.page_up(),
            (KeyCode::Home, _) | (KeyCode::PageUp, Mods::CTRL) => cv.go_home(),
            (KeyCode::End, _) | (KeyCode::PageDown, Mods::CTRL) => cv.go_end(),
            (KeyCode::Right, Mods::NONE) if !cv.wrap => cv.left += 1,
            (KeyCode::Left, Mods::NONE) if !cv.wrap => cv.left = cv.left.saturating_sub(1),
            (KeyCode::Right, Mods::CTRL) if !cv.wrap => cv.left += 20,
            (KeyCode::Left, Mods::CTRL) if !cv.wrap => cv.left = cv.left.saturating_sub(20),
            (KeyCode::F(2), Mods::NONE) => {
                let w = !cv.wrap;
                cv.set_wrap(w);
                self.core.wrap = w;
                self.core.left = 0;
            }
            _ => return false,
        }
        true
    }

    fn update_labels(&mut self) {
        self.labels.normal[1] = msg(if self.core.wrap { MsgId::MViewF2Unwrap } else { MsgId::MViewF2 }).to_string();
        self.labels.normal[4] = if self.lang.is_some() { "Colors".to_string() } else { String::new() };
        self.labels.normal[3] = msg(if self.core.mode == Mode::Hex { MsgId::MViewF4Text } else { MsgId::MViewF4 }).to_string();
        self.labels.normal[7] = match self.core.cp {
            Codepage::Oem(_) => msg(MsgId::MViewF8).to_string(),
            _ => msg(MsgId::MViewF8DOS).to_string(),
        };
    }

    fn do_search(&mut self, ui: &mut Ui, backward: bool) {
        match self.core.find(backward, false) {
            Ok(true) => {}
            Ok(false) => {
                let (head, q) =
                    if backward { (MsgId::MViewSearchBod, MsgId::MViewSearchFromEnd) } else { (MsgId::MViewSearchEod, MsgId::MViewSearchFromBegin) };
                let text = format!("{}\n{}", msg(head), msg(q));
                if message(ui, msg(MsgId::MSearchReplaceSearchTitle), &text, &[msg(MsgId::MYes), msg(MsgId::MNo)]) == Some(0) {
                    let saved = self.core.found;
                    self.core.found = None;
                    self.core.top = 0;
                    if !matches!(self.core.find(backward, true), Ok(true)) {
                        self.core.found = saved;
                        let nf = if self.core.search.hex { MsgId::MViewSearchCannotFindHex } else { MsgId::MViewSearchCannotFind };
                        message(
                            ui,
                            msg(MsgId::MSearchReplaceSearchTitle),
                            &format!("{}\n\"{}\"", msg(nf), self.core.search.spec.text),
                            &[msg(MsgId::MOk)],
                        );
                    }
                }
            }
            Err(e) => {
                gm_ui::message::error(ui, msg(MsgId::MError), &e);
            }
        }
    }

    fn goto_dialog(&mut self, ui: &mut Ui) {
        let mut b = DialogBuilder::new(msg(MsgId::MGoTo), 50);
        let e = b.edit("", Some("ViewerOffset"));
        b.text("123, 0x1F, 50%");
        b.separator();
        b.ok_cancel();
        let mut d = b.build();
        if d.run(ui).is_none() {
            return;
        }
        let s = d.text(e);
        let s = s.trim();
        let off = if let Some(p) = s.strip_suffix('%') {
            p.trim().parse::<u64>().ok().map(|p| self.core.len() * p.min(100) / 100)
        } else if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix('$')).or_else(|| s.strip_suffix('h')) {
            u64::from_str_radix(h, 16).ok()
        } else {
            s.parse::<u64>().ok()
        };
        if let Some(o) = off {
            self.core.goto_offset(o);
        }
    }

    fn codepage_menu(&mut self, ui: &mut Ui) {
        let list = Codepage::menu_list();
        let items = list.iter().map(|c| MenuItem::new(c.name()).checked(*c == self.core.cp)).collect();
        let mut m = VMenu::new(msg(MsgId::MGetCodePageTitle), items);
        if let Some(p) = list.iter().position(|c| *c == self.core.cp) {
            m.select(p);
        }
        if let MenuResult::Selected(i) = m.run(ui) {
            self.core.set_codepage(list[i]);
        }
    }
}

impl Window for ViewerWindow {
    fn title(&self) -> String {
        self.title_override.clone().unwrap_or_else(|| self.core.display_name())
    }

    fn kind(&self) -> WindowKind {
        WindowKind::Viewer
    }

    fn file(&self) -> Option<std::path::PathBuf> {
        Some(self.core.path.clone())
    }

    fn help_topic(&self) -> &str {
        "Viewer"
    }

    fn render(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        self.update_labels();
        let status = style(PaletteId::ViewerStatus);
        draw::fill(buf, Rect::new(area.x, area.y, area.width, 1), ' ', status);
        let (mode, left, percent) = match &self.colors {
            Some(cv) if cv.problems > 0 => (format!("Colors !{}  ", cv.problems), cv.left, cv.percent()),
            Some(cv) => ("Colors  ".to_string(), cv.left, cv.percent()),
            None => (String::new(), self.core.left, self.core.percent()),
        };
        let info = format!(
            "{mode}{}  {}  {}  {percent}%",
            self.core.cp.name(),
            if self.core.wrap { String::new() } else { format!("{} {left}", msg(MsgId::MEditStatusCol)) },
            draw::group_digits(self.core.len()),
        );
        let info_w = gm_text::str_width(&info) + 9; // room for the clock
        let name_w = (area.width as usize).saturating_sub(info_w + 1);
        draw::put(buf, area.x, area.y, &gm_text::truncate_left(&self.title(), name_w), status, name_w);
        let x = area.right().saturating_sub(info_w as u16);
        draw::put(buf, x, area.y, &info, status, info_w);
        let kb = u16::from(self.opts.keybar);
        let body = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(1 + kb));
        match &mut self.colors {
            Some(cv) => cv.render(buf, body, self.opts.show_arrows),
            None => self.core.render(buf, body, self.opts.show_arrows),
        }
        if self.opts.keybar {
            KeyBar::render(buf, area.bottom() - 1, area.width, self.labels.for_mods(Mods::NONE));
        }
        None
    }

    fn handle(&mut self, ev: &Event, ui: &mut Ui) -> WinAction {
        let k = match ev {
            Event::Key(k) => *k,
            Event::Mouse(m) => {
                self.handle_mouse(m);
                return WinAction::Redraw;
            }
            _ => return WinAction::None,
        };
        if matches!((k.code, k.mods), (KeyCode::Char('c'), Mods::CTRL) | (KeyCode::Insert, Mods::CTRL)) {
            self.copy();
            return WinAction::None;
        }
        if (k.code, k.mods) == (KeyCode::Char('u'), Mods::CTRL) {
            self.core.sel = None;
            self.core.found = None;
            if let Some(cv) = self.colors.as_mut() {
                cv.sel = None;
            }
            return WinAction::Redraw;
        }
        if (k.code, k.mods) == (KeyCode::F(5), Mods::NONE) {
            if self.lang.is_some() {
                let on = self.colors.is_none();
                self.set_colors(on);
            }
            return WinAction::Redraw;
        }
        if self.colors.is_some() {
            if self.handle_colors(k.code, k.mods) {
                return WinAction::Redraw;
            }
            match (k.code, k.mods) {
                // Hex, search, code pages and tail mode work on the plain view.
                (KeyCode::F(4) | KeyCode::F(7) | KeyCode::F(8), _) | (KeyCode::Char('f'), Mods::CTRL) => self.set_colors(false),
                // Window commands: keep the plain position in sync (saved on close).
                (KeyCode::Esc, _) | (KeyCode::F(1 | 3 | 6 | 10 | 12), Mods::NONE) | (KeyCode::Tab, _) | (KeyCode::Char('o'), Mods::CTRL) => {
                    if let Some(cv) = self.colors.take() {
                        self.sync_from(&cv);
                        self.colors = Some(cv);
                    }
                }
                _ => return WinAction::None,
            }
        }
        let c = &mut self.core;
        match (k.code, k.mods) {
            (KeyCode::Esc, _) | (KeyCode::F(10), Mods::NONE) | (KeyCode::F(3), Mods::NONE) => return WinAction::Close,
            (KeyCode::Down, Mods::NONE) => c.down(1),
            (KeyCode::Up, Mods::NONE) => c.up(1),
            (KeyCode::PageDown, Mods::NONE) | (KeyCode::Char(' '), Mods::NONE) => c.page_down(),
            (KeyCode::PageUp, Mods::NONE) => c.page_up(),
            (KeyCode::Home, _) | (KeyCode::PageUp, Mods::CTRL) => c.go_home(),
            (KeyCode::End, _) | (KeyCode::PageDown, Mods::CTRL) => c.go_end(),
            (KeyCode::Right, Mods::NONE) if !c.wrap => c.left += 1,
            (KeyCode::Left, Mods::NONE) if !c.wrap => c.left = c.left.saturating_sub(1),
            (KeyCode::Right, Mods::CTRL) if !c.wrap => c.left += 20,
            (KeyCode::Left, Mods::CTRL) if !c.wrap => c.left = c.left.saturating_sub(20),
            (KeyCode::F(2), Mods::NONE) => {
                c.wrap = !c.wrap;
                c.left = 0;
            }
            (KeyCode::F(2), Mods::SHIFT) => {
                c.word_wrap = !c.word_wrap;
                c.wrap = true;
            }
            (KeyCode::F(4), Mods::NONE) => c.toggle_hex(),
            (KeyCode::F(4), Mods::SHIFT) => c.cycle_mode(),
            (KeyCode::F(6), Mods::NONE) if !self.temp_file => {
                return WinAction::Replace(OpenRequest::Edit { path: c.path.clone(), line: None });
            }
            (KeyCode::F(7), Mods::NONE) => {
                let mut st = c.search.clone();
                if let Some(a) = search_dialog(ui, &mut st, false, true, "") {
                    self.core.search = st;
                    self.core.found = None;
                    self.do_search(ui, a == SearchAction::FindPrev);
                }
            }
            (KeyCode::F(7), Mods::SHIFT) if !c.search.spec.text.is_empty() => self.do_search(ui, false),
            (KeyCode::F(7), Mods::ALT) if !c.search.spec.text.is_empty() => self.do_search(ui, true),
            (KeyCode::F(8), Mods::NONE) => {
                let next = match c.cp {
                    Codepage::Oem(_) => Codepage::ansi(),
                    _ => Codepage::oem(),
                };
                c.set_codepage(next);
            }
            (KeyCode::F(8), Mods::SHIFT) => self.codepage_menu(ui),
            (KeyCode::F(8), Mods::ALT) => self.goto_dialog(ui),
            (KeyCode::Char('f'), Mods::CTRL) => c.tail = !c.tail,
            (KeyCode::Char(d @ '0'..='9'), m) if m == Mods::CTRL_SHIFT || m == Mods::ALT_SHIFT => {
                c.bookmarks[(d as u8 - b'0') as usize] = Some(c.top);
            }
            (KeyCode::Char(d @ '0'..='9'), m) if m == Mods::CTRL || m == Mods::ALT => {
                if let Some(o) = c.bookmarks[(d as u8 - b'0') as usize] {
                    c.goto_offset(o);
                }
            }
            (KeyCode::F(12), Mods::NONE) => return WinAction::ScreensList,
            (KeyCode::Tab, Mods::CTRL) => return WinAction::NextWindow,
            (KeyCode::Tab, Mods::CTRL_SHIFT) => return WinAction::PrevWindow,
            (KeyCode::Char('o'), Mods::CTRL) => return WinAction::Open(OpenRequest::UserScreen),
            (KeyCode::F(1), Mods::NONE) => return WinAction::Open(OpenRequest::Help("Viewer".into())),
            _ => {
                let _ = KeyChord::plain(KeyCode::Null);
                return WinAction::None;
            }
        }
        WinAction::Redraw
    }

    fn tick(&mut self, _ui: &mut Ui) -> bool {
        let changed = self.core.refresh();
        if changed && let Some(cv) = self.colors.take() {
            // Re-color the new content at the same place.
            self.sync_from(&cv);
            self.set_colors(true);
        }
        changed
    }
}

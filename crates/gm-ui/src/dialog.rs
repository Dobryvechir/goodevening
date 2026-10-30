//! Dialog engine (`dialog.cpp`) and the auto-layout builder (`FarDlgBuilder.cpp`).

use crate::draw;
use crate::edit::LineEdit;
use crate::ui::{Flow, Modal, Ui};
use crate::vmenu::{MenuItem, MenuResult, VMenu};
use gm_term::{Buffer, Event, KeyChord, KeyCode, Mods, MouseButton, MouseEventKind, PaletteId, Rect, style};
use gm_text::width::str_width;

pub enum ItemKind {
    Text { text: String, center: bool },
    Separator { text: String },
    Edit { edit: LineEdit, history: Option<String> },
    Combo { edit: LineEdit, items: Vec<String>, list_only: bool },
    CheckBox { text: String, state: u8, three: bool },
    Radio { text: String, on: bool, group: bool },
    Button { text: String, default: bool },
    List { items: Vec<String>, sel: usize, top: usize },
}

pub struct Item {
    pub kind: ItemKind,
    /// Position relative to the dialog's top-left corner.
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub disabled: bool,
}

impl Item {
    fn focusable(&self) -> bool {
        !self.disabled && !matches!(self.kind, ItemKind::Text { .. } | ItemKind::Separator { .. })
    }

    fn label(&self) -> Option<&str> {
        match &self.kind {
            ItemKind::Text { text, .. } | ItemKind::CheckBox { text, .. } | ItemKind::Radio { text, .. } | ItemKind::Button { text, .. } => {
                Some(text)
            }
            _ => None,
        }
    }

    fn is_text_input(&self) -> bool {
        matches!(self.kind, ItemKind::Edit { .. } | ItemKind::Combo { .. } | ItemKind::List { .. })
    }
}

/// What a key hook decided.
pub enum HookResult {
    /// Not handled; the dialog processes the key.
    Ignore,
    Handled,
    Close(Option<usize>),
}

pub type Hook<'a> = &'a mut dyn FnMut(&mut [Item], usize, &KeyChord, &mut Ui) -> HookResult;

pub struct Dialog {
    pub title: String,
    pub w: u16,
    pub h: u16,
    pub items: Vec<Item>,
    pub focus: usize,
    pub warn: bool,
    pub help_topic: String,
    /// Explicit top-left corner; centered when `None`.
    pub pos: Option<(u16, u16)>,
    result: Option<usize>,
    origin: (u16, u16),
}

impl Dialog {
    pub fn new(title: impl Into<String>, w: u16, h: u16, items: Vec<Item>) -> Dialog {
        let mut d =
            Dialog { title: title.into(), w, h, items, focus: 0, warn: false, help_topic: String::new(), pos: None, result: None, origin: (0, 0) };
        d.focus = d.items.iter().position(Item::focusable).unwrap_or(0);
        d
    }

    /// Runs the dialog; returns the index of the button (or item) that closed it.
    pub fn run(&mut self, ui: &mut Ui) -> Option<usize> {
        self.run_with(ui, &mut |_, _, _, _| HookResult::Ignore)
    }

    pub fn run_with(&mut self, ui: &mut Ui, hook: Hook) -> Option<usize> {
        self.result = None;
        let saved = std::mem::take(&mut ui.help_topic);
        ui.help_topic = if self.help_topic.is_empty() { saved.clone() } else { self.help_topic.clone() };
        let mut running = Running { dlg: self, hook };
        let _ = ui.run_modal(&mut running);
        ui.help_topic = saved;
        // Remember edit-field histories.
        if self.result.is_some() {
            for it in &self.items {
                if let ItemKind::Edit { edit, history: Some(h) } = &it.kind {
                    ui.history.add(h, &edit.text());
                }
            }
        }
        self.result
    }

    pub fn text(&self, i: usize) -> String {
        match &self.items[i].kind {
            ItemKind::Edit { edit, .. } | ItemKind::Combo { edit, .. } => edit.text(),
            ItemKind::Text { text, .. } => text.clone(),
            ItemKind::List { items, sel, .. } => items.get(*sel).cloned().unwrap_or_default(),
            _ => String::new(),
        }
    }

    pub fn set_text(&mut self, i: usize, s: &str) {
        match &mut self.items[i].kind {
            ItemKind::Edit { edit, .. } | ItemKind::Combo { edit, .. } => edit.set_text(s),
            ItemKind::Text { text, .. } => *text = s.to_string(),
            _ => {}
        }
    }

    pub fn checked(&self, i: usize) -> bool {
        match &self.items[i].kind {
            ItemKind::CheckBox { state, .. } => *state == 1,
            ItemKind::Radio { on, .. } => *on,
            _ => false,
        }
    }

    /// 0 = off, 1 = on, 2 = undefined (3-state checkboxes).
    pub fn state(&self, i: usize) -> u8 {
        match &self.items[i].kind {
            ItemKind::CheckBox { state, .. } => *state,
            ItemKind::Radio { on, .. } => u8::from(*on),
            _ => 0,
        }
    }

    pub fn list_sel(&self, i: usize) -> usize {
        match &self.items[i].kind {
            ItemKind::List { sel, .. } => *sel,
            ItemKind::Combo { edit, items, .. } => items.iter().position(|s| *s == edit.text()).unwrap_or(0),
            _ => 0,
        }
    }

    fn rect(&self, screen: Rect) -> Rect {
        match self.pos {
            Some((x, y)) => Rect::new(x, y, self.w.min(screen.width), self.h.min(screen.height)),
            None => draw::centered(screen, self.w, self.h),
        }
    }

    fn move_focus(&mut self, forward: bool) {
        let n = self.items.len();
        let mut i = self.focus;
        for _ in 0..n {
            i = if forward { (i + 1) % n } else { (i + n - 1) % n };
            if self.items[i].focusable() {
                self.focus = i;
                return;
            }
        }
    }

    fn set_radio(&mut self, i: usize) {
        // Find the group bounds.
        let mut start = i;
        while start > 0 {
            if let ItemKind::Radio { group: true, .. } = self.items[start].kind {
                break;
            }
            if !matches!(self.items[start - 1].kind, ItemKind::Radio { .. }) {
                break;
            }
            start -= 1;
        }
        let mut j = start;
        while j < self.items.len() {
            match &mut self.items[j].kind {
                ItemKind::Radio { on, group, .. } => {
                    if j != start && *group {
                        break;
                    }
                    *on = j == i;
                }
                _ => break,
            }
            j += 1;
        }
    }

    /// Activates item `i` (Space, hotkey, click). Returns true to close.
    fn activate(&mut self, i: usize) -> bool {
        self.focus = i;
        match &mut self.items[i].kind {
            ItemKind::CheckBox { state, three, .. } => {
                *state = if *three { (*state + 1) % 3 } else { 1 - (*state).min(1) };
            }
            ItemKind::Radio { .. } => self.set_radio(i),
            ItemKind::Button { .. } => {
                self.result = Some(i);
                return true;
            }
            _ => {}
        }
        false
    }

    fn default_button(&self) -> Option<usize> {
        self.items.iter().position(|it| matches!(it.kind, ItemKind::Button { default: true, .. }))
    }

    fn open_dropdown(&mut self, ui: &mut Ui) {
        let (ox, oy) = self.origin;
        let it = &self.items[self.focus];
        let (x, y, w) = (ox + it.x, oy + it.y + 1, it.w);
        let (entries, current): (Vec<String>, String) = match &it.kind {
            ItemKind::Combo { items, edit, .. } => (items.clone(), edit.text()),
            ItemKind::Edit { history: Some(h), edit } => (ui.history.list(h), edit.text()),
            _ => return,
        };
        if entries.is_empty() {
            return;
        }
        let mut m = VMenu::new("", entries.iter().map(MenuItem::new).collect());
        m.pos = Some((x, y));
        m.min_width = w + 2;
        m.dialog_colors = true;
        m.max_height = Some(12);
        if let Some(p) = entries.iter().position(|e| *e == current) {
            m.select(p);
        }
        let is_history = matches!(it.kind, ItemKind::Edit { .. });
        if is_history {
            m.pass_keys = vec![KeyChord::shift(KeyCode::Delete)];
        }
        loop {
            match m.run(ui) {
                MenuResult::Selected(i) => {
                    if let ItemKind::Combo { edit, .. } | ItemKind::Edit { edit, .. } = &mut self.items[self.focus].kind {
                        edit.set_text(&entries[i]);
                    }
                    return;
                }
                MenuResult::Key(_, i) => {
                    if let ItemKind::Edit { history: Some(h), .. } = &self.items[self.focus].kind {
                        ui.history.delete(h, &entries[i]);
                        m.items.remove(i);
                        if m.items.is_empty() {
                            return;
                        }
                    }
                }
                _ => return,
            }
        }
    }

    fn handle_key(&mut self, k: &KeyChord, ui: &mut Ui) -> Flow {
        let focus_is_input = self.items.get(self.focus).is_some_and(Item::is_text_input);
        match (k.code, k.mods) {
            (KeyCode::Esc, _) | (KeyCode::F(10), Mods::NONE) => {
                self.result = None;
                return Flow::Done;
            }
            (KeyCode::Tab, Mods::NONE) => self.move_focus(true),
            (KeyCode::Tab, Mods::SHIFT) => self.move_focus(false),
            (KeyCode::Enter, m) if !m.alt() => {
                if let ItemKind::Button { .. } = self.items[self.focus].kind {
                    self.result = Some(self.focus);
                } else {
                    self.result = Some(self.default_button().unwrap_or(self.focus));
                }
                return Flow::Done;
            }
            (KeyCode::Down, m) | (KeyCode::Up, m) if (m.ctrl() || m.alt()) && focus_is_input => {
                if k.code == KeyCode::Down {
                    self.open_dropdown(ui);
                }
            }
            (KeyCode::Char(' '), Mods::NONE) if !focus_is_input => {
                if self.activate(self.focus) {
                    return Flow::Done;
                }
            }
            _ => {
                // Item-specific keys.
                let focus = self.focus;
                let it = &mut self.items[focus];
                let consumed = match &mut it.kind {
                    ItemKind::Edit { edit, .. } => edit.handle_key(k),
                    ItemKind::Combo { edit, items, list_only } => {
                        if *list_only {
                            let cur = items.iter().position(|s| *s == edit.text()).unwrap_or(0);
                            match k.code {
                                KeyCode::Left | KeyCode::Right if k.mods.0 == 0 => {
                                    let n = items.len().max(1);
                                    let next = if k.code == KeyCode::Right { (cur + 1) % n } else { (cur + n - 1) % n };
                                    if let Some(s) = items.get(next) {
                                        edit.set_text(s);
                                    }
                                    true
                                }
                                _ => false,
                            }
                        } else {
                            edit.handle_key(k)
                        }
                    }
                    ItemKind::List { items, sel, .. } => match k.code {
                        KeyCode::Up if *sel > 0 => {
                            *sel -= 1;
                            true
                        }
                        KeyCode::Down if *sel + 1 < items.len() => {
                            *sel += 1;
                            true
                        }
                        KeyCode::Home => {
                            *sel = 0;
                            true
                        }
                        KeyCode::End => {
                            *sel = items.len().saturating_sub(1);
                            true
                        }
                        KeyCode::PageUp => {
                            *sel = sel.saturating_sub(it.h as usize);
                            true
                        }
                        KeyCode::PageDown => {
                            *sel = (*sel + it.h as usize).min(items.len().saturating_sub(1));
                            true
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if consumed {
                    return Flow::Continue;
                }
                match (k.code, k.mods) {
                    (KeyCode::Down, Mods::NONE) => self.move_focus(true),
                    (KeyCode::Up, Mods::NONE) => self.move_focus(false),
                    (KeyCode::Right, Mods::NONE) | (KeyCode::Left, Mods::NONE) if matches!(self.items[self.focus].kind, ItemKind::Button { .. }) => {
                        self.move_focus(k.code == KeyCode::Right)
                    }
                    (KeyCode::Char(c), m) if m.alt() || (!focus_is_input && !m.ctrl()) => {
                        let lc = c.to_lowercase().next().unwrap_or(c);
                        let hit = self.items.iter().position(|it| !it.disabled && it.label().and_then(draw::hotkey_of) == Some(lc));
                        if let Some(i) = hit {
                            if let ItemKind::Text { .. } = self.items[i].kind {
                                // A label's hotkey focuses the next control.
                                if let Some(n) = (i + 1..self.items.len()).find(|&j| self.items[j].focusable()) {
                                    self.focus = n;
                                }
                            } else if self.activate(i) {
                                return Flow::Done;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Flow::Continue
    }

    fn handle_mouse(&mut self, m: &gm_term::MouseEvent, ui: &mut Ui) -> Flow {
        let (ox, oy) = self.origin;
        if !matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
            if let MouseEventKind::ScrollDown | MouseEventKind::ScrollUp = m.kind
                && let ItemKind::List { items, sel, .. } = &mut self.items[self.focus].kind
            {
                if m.kind == MouseEventKind::ScrollDown && *sel + 1 < items.len() {
                    *sel += 1;
                } else if m.kind == MouseEventKind::ScrollUp && *sel > 0 {
                    *sel -= 1;
                }
            }
            return Flow::Continue;
        }
        if m.column < ox || m.row < oy || m.column >= ox + self.w || m.row >= oy + self.h {
            return Flow::Continue;
        }
        let (cx, cy) = (m.column - ox, m.row - oy);
        let hit = self.items.iter().position(|it| {
            let w = match &it.kind {
                ItemKind::CheckBox { text, .. } | ItemKind::Radio { text, .. } => draw::label_width(text) as u16 + 4,
                ItemKind::Button { text, .. } => draw::label_width(text) as u16 + 4,
                ItemKind::Edit { .. } | ItemKind::Combo { .. } => it.w + 1,
                _ => it.w,
            };
            it.focusable() && cy >= it.y && cy < it.y + it.h.max(1) && cx >= it.x && cx < it.x + w
        });
        if let Some(i) = hit {
            self.focus = i;
            let it = &mut self.items[i];
            match &mut it.kind {
                ItemKind::Edit { edit, history } => {
                    if cx == it.x + it.w && history.is_some() {
                        self.open_dropdown(ui);
                    } else {
                        edit.click((cx - it.x) as usize);
                    }
                }
                ItemKind::Combo { .. } => self.open_dropdown(ui),
                ItemKind::List { items, sel, top } => {
                    let row = (cy - it.y) as usize + *top;
                    if row < items.len() {
                        *sel = row;
                    }
                }
                _ => {
                    if self.activate(i) {
                        return Flow::Done;
                    }
                }
            }
        }
        Flow::Continue
    }

    fn render_dialog(&mut self, buf: &mut Buffer, screen: Rect) -> Option<(u16, u16)> {
        let r = self.rect(screen);
        self.origin = (r.x, r.y);
        let w = self.warn;
        let p = |normal: PaletteId, warn: PaletteId| style(if w { warn } else { normal });
        let text = p(PaletteId::DialogText, PaletteId::WarnDialogText);
        let hl = p(PaletteId::DialogTextHighlight, PaletteId::WarnDialogTextHighlight);
        let boxs = p(PaletteId::DialogBox, PaletteId::WarnDialogBox);
        let title = p(PaletteId::DialogBoxTitle, PaletteId::WarnDialogBoxTitle);
        let disabled = p(PaletteId::DialogDisabled, PaletteId::WarnDialogDisabled);
        draw::fill(buf, r, ' ', text);
        let frame = Rect::new(r.x + 3, r.y + 1, r.width.saturating_sub(6), r.height.saturating_sub(2));
        draw::frame(buf, frame, true, boxs, Some((&self.title, title)));
        draw::shadow(buf, r);
        let mut cursor = None;
        let focus = self.focus;
        for (idx, it) in self.items.iter_mut().enumerate() {
            let (x, y) = (r.x + it.x, r.y + it.y);
            let focused = idx == focus;
            let (st, hst) = if it.disabled { (disabled, disabled) } else { (text, hl) };
            match &mut it.kind {
                ItemKind::Text { text: t, center } => {
                    let tx = if *center { r.x + (r.width.saturating_sub(draw::label_width(t) as u16)) / 2 } else { x };
                    draw::hotkey_text(buf, tx, y, t, st, hst, (r.right().saturating_sub(tx + 4)) as usize);
                }
                ItemKind::Separator { text: t } => {
                    draw::separator(buf, frame.x, y, frame.width, true, boxs);
                    if !t.is_empty() {
                        draw::title_at(buf, frame, y, &gm_i18n::strip_hotkey(t), boxs);
                    }
                }
                ItemKind::Edit { edit, history } => {
                    let es = p(PaletteId::DialogEdit, PaletteId::WarnDialogEdit);
                    let sel = p(PaletteId::DialogEditSelected, PaletteId::WarnDialogEditSelected);
                    let un = p(PaletteId::DialogEditUnchanged, PaletteId::WarnDialogEditUnchanged);
                    let cx = edit.render(buf, x, y, it.w as usize, if it.disabled { style(PaletteId::DialogEditDisabled) } else { es }, sel, un);
                    if history.is_some() {
                        draw::set(buf, x + it.w, y, '↓', text);
                    }
                    if focused {
                        cursor = Some((cx, y));
                    }
                }
                ItemKind::Combo { edit, list_only, .. } => {
                    let es = style(PaletteId::DialogEdit);
                    let sel = style(PaletteId::DialogEditSelected);
                    let cx = edit.render(buf, x, y, it.w as usize, es, sel, es);
                    draw::set(buf, x + it.w, y, '↓', text);
                    if focused {
                        cursor = Some((if *list_only { x } else { cx }, y));
                    }
                }
                ItemKind::CheckBox { text: t, state, .. } => {
                    let mark = match state {
                        1 => 'x',
                        2 => '?',
                        _ => ' ',
                    };
                    draw::put(buf, x, y, &format!("[{mark}] "), st, 4);
                    draw::hotkey_text(buf, x + 4, y, t, st, hst, (r.right().saturating_sub(x + 8)) as usize);
                    if focused {
                        cursor = Some((x + 1, y));
                    }
                }
                ItemKind::Radio { text: t, on, .. } => {
                    draw::put(buf, x, y, if *on { "(•) " } else { "( ) " }, st, 4);
                    draw::hotkey_text(buf, x + 4, y, t, st, hst, (r.right().saturating_sub(x + 8)) as usize);
                    if focused {
                        cursor = Some((x + 1, y));
                    }
                }
                ItemKind::Button { text: t, default } => {
                    let (bs, bh) = match (focused, it.disabled) {
                        (_, true) => (disabled, disabled),
                        (true, _) => (
                            p(PaletteId::DialogButtonSelected, PaletteId::WarnDialogButtonSelected),
                            p(PaletteId::DialogButtonHighlightSelected, PaletteId::WarnDialogButtonHighlightSelected),
                        ),
                        _ => (
                            p(PaletteId::DialogButton, PaletteId::WarnDialogButton),
                            p(PaletteId::DialogButtonHighlight, PaletteId::WarnDialogButtonHighlight),
                        ),
                    };
                    let (l, rr) = if *default { ("{ ", " }") } else { ("[ ", " ]") };
                    let mut cx = x;
                    cx += draw::put(buf, cx, y, l, bs, 2) as u16;
                    cx += draw::hotkey_text(buf, cx, y, t, bs, bh, 60) as u16;
                    draw::put(buf, cx, y, rr, bs, 2);
                    if focused {
                        cursor = Some((x + 2, y));
                    }
                }
                ItemKind::List { items, sel, top } => {
                    let ls = style(PaletteId::DialogListText);
                    let lsel = style(PaletteId::DialogListTextSelected);
                    let h = it.h as usize;
                    if *sel < *top {
                        *top = *sel;
                    }
                    if *sel >= *top + h {
                        *top = *sel + 1 - h;
                    }
                    for row in 0..h {
                        let i = *top + row;
                        let s = if i == *sel && focused { lsel } else { ls };
                        let line = items.get(i).map(String::as_str).unwrap_or("");
                        draw::put_fit(buf, x, y + row as u16, &format!(" {line}"), s, it.w as usize);
                    }
                    draw::scrollbar(buf, x + it.w, y, it.h, *top, items.len(), h, ls);
                }
            }
        }
        if self.items.get(focus).is_some_and(|it| matches!(it.kind, ItemKind::List { .. })) { None } else { cursor }
    }
}

struct Running<'a, 'b> {
    dlg: &'a mut Dialog,
    hook: Hook<'b>,
}

impl Modal for Running<'_, '_> {
    fn render(&mut self, buf: &mut Buffer, screen: Rect) -> Option<(u16, u16)> {
        self.dlg.render_dialog(buf, screen)
    }

    fn handle(&mut self, ev: &Event, ui: &mut Ui) -> Flow {
        match ev {
            Event::Key(k) => {
                match (self.hook)(&mut self.dlg.items, self.dlg.focus, k, ui) {
                    HookResult::Handled => return Flow::Continue,
                    HookResult::Close(r) => {
                        self.dlg.result = r;
                        return Flow::Done;
                    }
                    HookResult::Ignore => {}
                }
                self.dlg.handle_key(k, ui)
            }
            Event::Paste(s) => {
                if let Some(Item { kind: ItemKind::Edit { edit, .. } | ItemKind::Combo { edit, .. }, .. }) = self.dlg.items.get_mut(self.dlg.focus) {
                    edit.paste(s);
                }
                Flow::Continue
            }
            Event::Mouse(m) => self.dlg.handle_mouse(m, ui),
            _ => Flow::Continue,
        }
    }
}

/// Auto-layout builder: items stack vertically inside a double frame.
pub struct DialogBuilder {
    title: String,
    width: u16,
    items: Vec<Item>,
    y: u16,
    warn: bool,
    help: String,
}

impl DialogBuilder {
    /// `width` is the total dialog width (the frame is 3 cells inside it).
    pub fn new(title: impl Into<String>, width: u16) -> DialogBuilder {
        DialogBuilder { title: title.into(), width: width.max(20), items: Vec::new(), y: 2, warn: false, help: String::new() }
    }

    pub fn warn(mut self) -> DialogBuilder {
        self.warn = true;
        self
    }

    pub fn help(mut self, topic: &str) -> DialogBuilder {
        self.help = topic.into();
        self
    }

    fn inner(&self) -> u16 {
        self.width - 10
    }

    fn push(&mut self, kind: ItemKind, x: u16, w: u16, h: u16) -> usize {
        self.items.push(Item { kind, x, y: self.y, w, h, disabled: false });
        self.y += h.max(1);
        self.items.len() - 1
    }

    pub fn text(&mut self, s: impl Into<String>) -> usize {
        let w = self.inner();
        self.push(ItemKind::Text { text: s.into(), center: false }, 5, w, 1)
    }

    pub fn text_center(&mut self, s: impl Into<String>) -> usize {
        let w = self.inner();
        self.push(ItemKind::Text { text: s.into(), center: true }, 5, w, 1)
    }

    pub fn edit(&mut self, init: &str, history: Option<&str>) -> usize {
        let w = self.inner() - u16::from(history.is_some());
        let edit = if history.is_some() { LineEdit::with_unchanged(init) } else { LineEdit::new(init) };
        self.push(ItemKind::Edit { edit, history: history.map(String::from) }, 5, w, 1)
    }

    pub fn password(&mut self, init: &str) -> usize {
        let w = self.inner();
        let mut edit = LineEdit::new(init);
        edit.password = true;
        self.push(ItemKind::Edit { edit, history: None }, 5, w, 1)
    }

    pub fn combo(&mut self, init: &str, items: Vec<String>, list_only: bool) -> usize {
        let w = self.inner() - 1;
        self.push(ItemKind::Combo { edit: LineEdit::new(init), items, list_only }, 5, w, 1)
    }

    pub fn checkbox(&mut self, label: impl Into<String>, on: bool) -> usize {
        let w = self.inner();
        self.push(ItemKind::CheckBox { text: label.into(), state: u8::from(on), three: false }, 5, w, 1)
    }

    pub fn checkbox3(&mut self, label: impl Into<String>, state: u8) -> usize {
        let w = self.inner();
        self.push(ItemKind::CheckBox { text: label.into(), state, three: true }, 5, w, 1)
    }

    pub fn radio(&mut self, label: impl Into<String>, on: bool, first_in_group: bool) -> usize {
        let w = self.inner();
        self.push(ItemKind::Radio { text: label.into(), on, group: first_in_group }, 5, w, 1)
    }

    pub fn list(&mut self, items: Vec<String>, sel: usize, height: u16) -> usize {
        let w = self.inner() - 1;
        self.push(ItemKind::List { items, sel, top: 0 }, 5, w, height)
    }

    pub fn separator(&mut self) -> usize {
        self.separator_text("")
    }

    pub fn separator_text(&mut self, text: &str) -> usize {
        let w = self.width - 6;
        self.push(ItemKind::Separator { text: text.into() }, 3, w, 1)
    }

    /// Two-column helper: adds a checkbox in the right half of the previous row.
    pub fn checkbox_right(&mut self, label: impl Into<String>, on: bool) -> usize {
        self.y -= 1;
        let x = 5 + self.inner() / 2;
        let w = self.inner() / 2;
        self.push(ItemKind::CheckBox { text: label.into(), state: u8::from(on), three: false }, x, w, 1)
    }

    /// A centered row of buttons; returns the index of the first one.
    pub fn buttons(&mut self, labels: &[&str], default: usize) -> usize {
        let total: u16 = labels.iter().map(|l| draw::label_width(l) as u16 + 4).sum::<u16>() + labels.len() as u16 - 1;
        let mut x = (self.width.saturating_sub(total)) / 2;
        let first = self.items.len();
        for (i, l) in labels.iter().enumerate() {
            let w = draw::label_width(l) as u16 + 4;
            self.items.push(Item {
                kind: ItemKind::Button { text: (*l).to_string(), default: i == default },
                x,
                y: self.y,
                w,
                h: 1,
                disabled: false,
            });
            x += w + 1;
        }
        self.y += 1;
        first
    }

    pub fn ok_cancel(&mut self) -> usize {
        self.buttons(&[gm_i18n::msg(gm_i18n::MsgId::MOk), gm_i18n::msg(gm_i18n::MsgId::MCancel)], 0)
    }

    pub fn build(self) -> Dialog {
        let h = self.y + 2;
        let mut d = Dialog::new(self.title, self.width, h, self.items);
        d.warn = self.warn;
        d.help_topic = self.help;
        d
    }

    /// Computes a width that fits all text items (between `min` and `max`).
    pub fn fit_width(lines: &[&str], min: u16, max: u16) -> u16 {
        let w = lines.iter().map(|l| str_width(l) as u16 + 10).max().unwrap_or(min);
        w.clamp(min, max)
    }
}

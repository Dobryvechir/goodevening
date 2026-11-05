//! The application: window manager, event loop and the panels screen
//! (replaces `manager.cpp`, `filepanels.cpp`, `ctrlobj.cpp`, `cmdline.cpp`).

use crate::keymap::Keymap;
use crate::module::Module;
use gm_config::{FilePos, Settings, StateDb};
use gm_editor::{Editor, EditorOptions, EditorTool};
use gm_i18n::{MsgId, msg};
use gm_panels::qview::QuickView;
use gm_panels::tree::TreePanel;
use gm_panels::{FilePanel, Highlighter, PanelKind, PanelOptions, SortMode, format};
use gm_term::{Buffer, Event, KeyChord, KeyCode, Mods, MouseButton, MouseEventKind, PaletteId, Rect, palette, style};
use gm_text::{Codepage, Eol};
use gm_ui::{HistoryStore, KeyBar, KeyBarLabels, LineEdit, OpenRequest, Ui, WinAction, Window, WindowKind, draw};
use gm_viewer::{ViewerCore, ViewerOptions, ViewerWindow};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub struct PanelSlot {
    pub files: FilePanel,
    pub kind: PanelKind,
    pub tree: TreePanel,
    pub qview: QuickView,
    pub visible: bool,
    pub rect: Rect,
}

/// `HistoryStore` backed by `state.db`.
pub struct DbHistory(pub Rc<StateDb>);

impl HistoryStore for DbHistory {
    fn list(&self, kind: &str) -> Vec<String> {
        self.0.history(&format!("dlg:{kind}")).into_iter().map(|h| h.0).collect()
    }
    fn add(&self, kind: &str, item: &str) {
        self.0.add_history(&format!("dlg:{kind}"), item, "");
    }
    fn delete(&self, kind: &str, item: &str) {
        self.0.delete_history_item(&format!("dlg:{kind}"), item);
    }
    fn clear(&self, kind: &str) {
        self.0.clear_history(&format!("dlg:{kind}"));
    }
}

pub struct App {
    pub ui: Ui,
    pub settings: Settings,
    pub settings_path: PathBuf,
    pub db: Rc<StateDb>,
    pub left: PanelSlot,
    pub right: PanelSlot,
    pub active_left: bool,
    pub cmdline: LineEdit,
    pub(crate) cmd_hist_pos: Option<usize>,
    pub windows: Vec<Box<dyn Window>>,
    pub current: Option<usize>,
    pub modules: Vec<Box<dyn Module>>,
    pub(crate) editor_tools: Rc<Vec<Box<dyn EditorTool>>>,
    pub(crate) keymap: Keymap,
    pub(crate) highlighter: Rc<Highlighter>,
    watcher: Option<notify::RecommendedWatcher>,
    watch_rx: crossbeam_channel::Receiver<()>,
    watch_tx: crossbeam_channel::Sender<()>,
    watched: [Option<PathBuf>; 2],
    fs_dirty: Option<Instant>,
    pub quit: bool,
    pub(crate) dir_stack: Vec<PathBuf>,
    pub(crate) menu_last: (usize, Vec<usize>),
    last_click: Option<(Instant, u16, u16)>,
    pub(crate) find_params: gm_search::FindParams,
    pub pending_messages: Vec<(String, String)>,
    /// Server execution mode (`gm --client=<file>`): the queue file that
    /// receives the command lines gm would otherwise run itself.
    pub server_queue: Option<PathBuf>,
    labels: KeyBarLabels,
    cmd_rect: Rect,
    /// Decoded copies of encoded files (F9 → Commands → Encoded archives).
    /// Last, so it is dropped after the windows and panels that use them:
    /// changes are encoded back and the copies deleted.
    pub encoded: crate::encoded::Sessions,
    /// Local copies of remote (SFTP) files opened with F3/F4; dropped last
    /// for the same reason.
    pub remote: crate::remote::RemoteFiles,
}

fn panel_labels() -> KeyBarLabels {
    let row = |ids: [MsgId; 12]| -> [String; 12] { ids.map(|i| msg(i).to_string()) };
    let mut l = KeyBarLabels::default();
    l.normal = row([
        MsgId::MF1,
        MsgId::MF2,
        MsgId::MF3,
        MsgId::MF4,
        MsgId::MF5,
        MsgId::MF6,
        MsgId::MF7,
        MsgId::MF8,
        MsgId::MF9,
        MsgId::MF10,
        MsgId::MF11,
        MsgId::MF12,
    ]);
    l.normal[10] = String::new();
    l.shift = row([
        MsgId::MShiftF1,
        MsgId::MShiftF2,
        MsgId::MShiftF3,
        MsgId::MShiftF4,
        MsgId::MShiftF5,
        MsgId::MShiftF6,
        MsgId::MShiftF7,
        MsgId::MShiftF8,
        MsgId::MShiftF9,
        MsgId::MShiftF10,
        MsgId::MShiftF11,
        MsgId::MShiftF12,
    ]);
    l.alt = row([
        MsgId::MAltF1,
        MsgId::MAltF2,
        MsgId::MAltF3,
        MsgId::MAltF4,
        MsgId::MAltF5,
        MsgId::MAltF6,
        MsgId::MAltF7,
        MsgId::MAltF8,
        MsgId::MAltF9,
        MsgId::MAltF10,
        MsgId::MAltF11,
        MsgId::MAltF12,
    ]);
    l.ctrl = row([
        MsgId::MCtrlF1,
        MsgId::MCtrlF2,
        MsgId::MCtrlF3,
        MsgId::MCtrlF4,
        MsgId::MCtrlF5,
        MsgId::MCtrlF6,
        MsgId::MCtrlF7,
        MsgId::MCtrlF8,
        MsgId::MCtrlF9,
        MsgId::MCtrlF10,
        MsgId::MCtrlF11,
        MsgId::MCtrlF12,
    ]);
    l
}

pub fn panel_options(s: &Settings) -> PanelOptions {
    let p = &s.panels;
    PanelOptions {
        show_hidden: p.show_hidden,
        highlight: p.highlight,
        select_folders: p.select_folders,
        natural_sort: p.natural_sort,
        show_dotdot_in_root: p.show_dotdot_in_root,
        column_titles: p.show_column_titles,
        status_line: p.show_status_line,
        total_info: p.show_total,
        free_space: p.show_free_space,
        scrollbar: p.show_scrollbar,
    }
}

impl App {
    pub fn new(
        ui: Ui,
        settings: Settings,
        settings_path: PathBuf,
        db: Rc<StateDb>,
        modules: Vec<Box<dyn Module>>,
        keymap: Keymap,
        start_dirs: (Option<PathBuf>, Option<PathBuf>),
    ) -> App {
        let highlighter = Rc::new(Highlighter::new(&settings.highlight, &settings.mask_groups));
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let pick = |explicit: Option<PathBuf>, saved: &str| -> PathBuf {
            explicit.or_else(|| (!saved.is_empty()).then(|| PathBuf::from(saved)).filter(|p| p.is_dir())).unwrap_or_else(|| cwd.clone())
        };
        let ldir = pick(start_dirs.0, &settings.left.path);
        let rdir = pick(start_dirs.1, &settings.right.path);
        let opts = panel_options(&settings);
        let mk = |dir: PathBuf, st: &gm_config::PanelState| {
            let mut f = FilePanel::new(dir, opts.clone(), highlighter.clone());
            f.sort = SortMode::from_key(&st.sort);
            f.reverse = st.reverse;
            f.set_mode(st.mode);
            f.resort();
            PanelSlot {
                files: f,
                kind: PanelKind::from_name(&st.kind),
                tree: TreePanel::default(),
                qview: QuickView::default(),
                visible: st.visible,
                rect: Rect::default(),
            }
        };
        let left = mk(ldir, &settings.left);
        let right = mk(rdir, &settings.right);
        let editor_tools: Vec<Box<dyn EditorTool>> = modules.iter().flat_map(|m| m.editor_tools()).collect();
        let (watch_tx, watch_rx) = crossbeam_channel::unbounded();
        let mut app = App {
            ui,
            settings,
            settings_path,
            db,
            left,
            right,
            active_left: true,
            cmdline: LineEdit::new(""),
            cmd_hist_pos: None,
            windows: Vec::new(),
            current: None,
            modules,
            editor_tools: Rc::new(editor_tools),
            keymap,
            highlighter,
            watcher: None,
            watch_rx,
            watch_tx,
            watched: [None, None],
            fs_dirty: None,
            quit: false,
            dir_stack: Vec::new(),
            menu_last: (0, vec![0; 5]),
            last_click: None,
            find_params: gm_search::FindParams::default(),
            pending_messages: Vec::new(),
            server_queue: None,
            labels: panel_labels(),
            cmd_rect: Rect::default(),
            encoded: Default::default(),
            remote: Default::default(),
        };
        // A panel can only be a tree/info/quick view if the other one is a file list.
        if app.left.kind != PanelKind::Files && app.right.kind != PanelKind::Files {
            app.left.kind = PanelKind::Files;
        }
        if app.left.kind != PanelKind::Files {
            app.active_left = false;
        }
        for slot in [&mut app.left, &mut app.right] {
            if slot.kind == PanelKind::Tree
                && let Some(d) = slot.files.local_dir()
            {
                slot.tree.build(&d, opts.show_hidden);
            }
        }
        if let Some(d) = app.active().files.local_dir() {
            let _ = std::env::set_current_dir(d);
        }
        app.setup_watcher();
        app
    }

    // ----- panel access -----

    pub fn active(&mut self) -> &mut PanelSlot {
        if self.active_left { &mut self.left } else { &mut self.right }
    }

    pub fn passive(&mut self) -> &mut PanelSlot {
        if self.active_left { &mut self.right } else { &mut self.left }
    }

    pub fn sides(&mut self) -> (&mut PanelSlot, &mut PanelSlot) {
        if self.active_left { (&mut self.left, &mut self.right) } else { (&mut self.right, &mut self.left) }
    }

    /// The local folder commands run in (the active panel, or the archive's folder).
    pub fn work_dir(&mut self) -> PathBuf {
        let a = self.active();
        a.files.local_dir().unwrap_or_else(|| a.files.base_dir())
    }

    /// Call after the active or passive panel changed folder.
    pub fn dir_changed(&mut self) {
        let dirs: Vec<PathBuf> = [&self.left, &self.right].iter().filter_map(|s| s.files.local_dir()).collect();
        if self.settings.system.save_folder_history
            && let Some(d) = self.active().files.local_dir()
        {
            self.db.add_history("folder", &d.display().to_string(), "");
        }
        if let Some(d) = self.active().files.local_dir() {
            let _ = std::env::set_current_dir(d);
        }
        let _ = dirs;
        self.setup_watcher();
        let opts = self.settings.panels.show_hidden;
        let (a, p) = self.sides();
        if p.kind == PanelKind::Tree
            && let Some(d) = a.files.local_dir()
        {
            p.tree.build(&d, opts);
        }
    }

    fn setup_watcher(&mut self) {
        if !self.settings.panels.auto_refresh {
            self.watcher = None;
            return;
        }
        let want = [self.left.files.local_dir(), self.right.files.local_dir()];
        if want == self.watched && self.watcher.is_some() {
            return;
        }
        use notify::Watcher;
        let tx = self.watch_tx.clone();
        let w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(e) = res
                && !matches!(e.kind, notify::EventKind::Access(_))
            {
                let _ = tx.send(());
            }
        });
        let Ok(mut w) = w else {
            self.watcher = None;
            return;
        };
        for d in want.iter().flatten() {
            let _ = w.watch(d, notify::RecursiveMode::NonRecursive);
        }
        self.watched = want;
        self.watcher = Some(w);
    }

    fn process_fs_events(&mut self) {
        let mut got = false;
        while self.watch_rx.try_recv().is_ok() {
            got = true;
        }
        if got && self.fs_dirty.is_none() {
            self.fs_dirty = Some(Instant::now());
        }
        if let Some(t) = self.fs_dirty
            && t.elapsed() >= Duration::from_millis(300)
        {
            self.fs_dirty = None;
            self.left.files.reload();
            self.right.files.reload();
        }
    }

    pub fn refresh_labels(&mut self) {
        self.labels = panel_labels();
    }

    pub fn refresh_panels(&mut self) {
        self.left.files.reload();
        self.right.files.reload();
    }

    // ----- main loop -----

    pub fn run(&mut self) -> std::io::Result<()> {
        for e in std::mem::take(&mut self.keymap.errors) {
            self.pending_messages.push(("keymap.toml".into(), e));
        }
        while !self.quit {
            for (t, m) in std::mem::take(&mut self.pending_messages) {
                self.render()?;
                gm_ui::message::error(&mut self.ui, &t, &m);
            }
            self.process_fs_events();
            self.render()?;
            let busy = self.left.qview.busy() || self.right.qview.busy() || self.fs_dirty.is_some();
            let timeout = if busy { Duration::from_millis(150) } else { Duration::from_millis(1000) };
            match self.ui.term.next_event(timeout)? {
                Some(ev) => self.dispatch(ev),
                None => self.tick(),
            }
        }
        self.save_state();
        Ok(())
    }

    fn tick(&mut self) {
        if let Some(i) = self.current
            && let Some(w) = self.windows.get_mut(i)
        {
            w.tick(&mut self.ui);
        }
        // Decoded copies written by other programs, and copies other
        // programs have released.
        crate::encoded::sync(self);
        crate::encoded::release_unused(self);
        crate::remote::sync(self);
        crate::remote::release_unused(self);
    }

    pub fn render(&mut self) -> std::io::Result<()> {
        let area = self.ui.term.area();
        let mut buf = Buffer::empty(area);
        let cursor = match self.current {
            Some(i) if i < self.windows.len() => {
                let c = self.windows[i].render(&mut buf, area);
                if self.settings.interface.console_title {
                    let t = format!("{} - gm", self.windows[i].title());
                    self.ui.term.set_title(&t);
                }
                c
            }
            _ => {
                self.current = None;
                let c = self.render_panels(&mut buf, area);
                if self.settings.interface.console_title {
                    let t = format!("{} - gm", self.active().files.display_dir());
                    self.ui.term.set_title(&t);
                }
                c
            }
        };
        if self.settings.interface.clock && area.width > 20 {
            let c = format::clock();
            let st = style(match self.current.map(|i| self.windows[i].kind()) {
                Some(WindowKind::Editor) => PaletteId::EditorClock,
                Some(WindowKind::Viewer) => PaletteId::ViewerClock,
                _ => PaletteId::Clock,
            });
            draw::put(&mut buf, area.right() - c.len() as u16, area.y, &c, st, c.len());
        }
        if self.settings.interface.show_screen_number && !self.windows.is_empty() && self.current.is_none() {
            let s = format!("[{}]", self.windows.len());
            draw::put(&mut buf, area.x, area.y, &s, style(PaletteId::PanelScreensNumber), s.len());
        }
        self.ui.term.draw(|b| {
            *b = buf;
            cursor
        })
    }

    pub(crate) fn panel_layout(&self, area: Rect) -> (Rect, Rect, Rect, u16) {
        let top = u16::from(self.settings.interface.menubar_always);
        let kb = u16::from(self.settings.interface.keybar);
        let reduce = self.settings.panels.height_reduce;
        let ph = area.height.saturating_sub(top + 1 + kb + reduce).max(5.min(area.height));
        let pct = self.settings.panels.left_width_percent.clamp(10, 90);
        let lw = (area.width as u32 * pct as u32 / 100) as u16;
        let left = Rect::new(area.x, area.y + top, lw, ph);
        let right = Rect::new(area.x + lw, area.y + top, area.width - lw, ph);
        let cmd_y = area.bottom().saturating_sub(1 + kb);
        let cmd = Rect::new(area.x, cmd_y, area.width, 1);
        (left, right, cmd, kb)
    }

    fn render_panels(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        let (lr, rr, cmd, kb) = self.panel_layout(area);
        self.left.rect = lr;
        self.right.rect = rr;
        self.cmd_rect = cmd;
        let al = self.active_left;
        let mut cursor = None;
        {
            let (left, right) = (&mut self.left, &mut self.right);
            render_slot(buf, left, right, al, &mut cursor);
            render_slot(buf, right, left, !al, &mut cursor);
        }
        // Command line: prompt + editor.
        let prompt = format!("{}>", self.active().files.display_dir());
        let prompt = gm_text::truncate_left(&prompt, (area.width / 2) as usize);
        let cs = style(PaletteId::CommandLine);
        draw::fill(buf, cmd, ' ', cs);
        let pw = draw::put(buf, cmd.x, cmd.y, &prompt, style(PaletteId::CommandLinePrefix), prompt.len()) as u16;
        let cx = self.cmdline.render(buf, cmd.x + pw, cmd.y, (cmd.width - pw) as usize, cs, style(PaletteId::CommandLineSelected), cs);
        if cursor.is_none() {
            cursor = Some((cx, cmd.y));
        }
        if kb > 0 {
            KeyBar::render(buf, area.bottom() - 1, area.width, self.labels.for_mods(Mods::NONE));
        }
        if self.settings.interface.menubar_always {
            crate::menus::bar(self).render(buf, area.width, false);
        }
        cursor
    }

    // ----- event dispatch -----

    /// Handles one input event (public for scripted UI tests).
    pub fn handle_event(&mut self, ev: Event) {
        self.dispatch(ev);
    }

    fn dispatch(&mut self, ev: Event) {
        self.dispatch_event(ev);
        // A decoded copy saved by the editor or rewritten by an archive
        // panel is encoded back over its original; copies that are no longer
        // shown (window closed, archive left) are deleted.
        crate::encoded::sync(self);
        crate::encoded::release_unused(self);
        // Local copies of remote files: saves are uploaded, closed ones deleted.
        crate::remote::sync(self);
        crate::remote::release_unused(self);
        // Errors of remote panels (lost connection…) pop up in a red box.
        self.report_panel_errors();
    }

    fn report_panel_errors(&mut self) {
        for left in [true, false] {
            let slot = if left { &mut self.left } else { &mut self.right };
            if let Some(e) = slot.files.take_fresh_error() {
                let title = slot.files.provider().title();
                gm_ui::message::error(&mut self.ui, &title, &e);
            }
        }
    }

    fn dispatch_event(&mut self, ev: Event) {
        if let Some(i) = self.current {
            // Global window switching keys first.
            if let Event::Key(k) = &ev
                && k.code == KeyCode::F(12)
                && k.mods == Mods::NONE
            {
                crate::menus::screens(self);
                return;
            }
            let act = match self.windows.get_mut(i) {
                Some(w) => w.handle(&ev, &mut self.ui),
                None => WinAction::Panels,
            };
            self.window_action(i, act);
            return;
        }
        match ev {
            Event::Key(k) => self.panels_key(k),
            Event::Paste(s) => self.cmdline.paste(&s),
            Event::Mouse(m) => self.panels_mouse(m),
            Event::Focus(true) => {
                if self.settings.panels.auto_refresh {
                    self.refresh_panels();
                }
            }
            _ => {}
        }
    }

    fn window_action(&mut self, i: usize, act: WinAction) {
        match act {
            WinAction::None | WinAction::Redraw => {}
            WinAction::Close => self.close_window(i),
            WinAction::Replace(req) => {
                self.close_window(i);
                self.open(req);
            }
            WinAction::Open(req) => self.open(req),
            WinAction::ScreensList => crate::menus::screens(self),
            WinAction::NextWindow => self.cycle(true),
            WinAction::PrevWindow => self.cycle(false),
            WinAction::Panels => self.current = None,
            WinAction::Quit => self.quit = true,
        }
    }

    pub fn cycle(&mut self, forward: bool) {
        let n = self.windows.len() + 1;
        let cur = self.current.map(|i| i + 1).unwrap_or(0);
        let next = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
        self.current = if next == 0 { None } else { Some(next - 1) };
    }

    pub fn close_window(&mut self, i: usize) {
        if i >= self.windows.len() {
            return;
        }
        // Positions are saved by `PositionSaver` when the window closes itself.
        self.windows.remove(i);
        // Return to the previous window, or to the panels.
        self.current = if i == 0 || self.windows.is_empty() { None } else { Some((i - 1).min(self.windows.len() - 1)) };
        self.refresh_panels();
    }

    /// Opens a viewer, editor, help or the user screen.
    pub fn open(&mut self, req: OpenRequest) {
        match req {
            OpenRequest::View(p) => self.open_viewer(&p),
            OpenRequest::Edit { path, line } => self.open_editor(&path, line),
            OpenRequest::Help(t) => gm_help::show(&mut self.ui, &t),
            OpenRequest::UserScreen => self.user_screen(),
        }
    }

    pub fn viewer_options(&self) -> ViewerOptions {
        let v = &self.settings.viewer;
        ViewerOptions {
            wrap: v.wrap,
            word_wrap: v.word_wrap,
            tab_size: v.tab_size,
            show_arrows: v.show_arrows,
            keybar: self.settings.interface.keybar,
            auto_detect: v.auto_detect_codepage,
            default_cp: Codepage::from_id(v.default_codepage).unwrap_or(Codepage::Utf8),
            hex_upper: v.hex_uppercase,
        }
    }

    pub fn editor_options(&self) -> EditorOptions {
        let e = &self.settings.editor;
        EditorOptions {
            tab_size: e.tab_size,
            expand_tabs: e.expand_tabs,
            auto_indent: e.auto_indent,
            persistent_blocks: e.persistent_blocks,
            del_removes_blocks: e.del_removes_blocks,
            line_numbers: e.show_line_numbers,
            show_whitespace: e.show_whitespace,
            keybar: self.settings.interface.keybar,
            auto_detect: e.auto_detect_codepage,
            default_cp: Codepage::from_id(e.default_codepage).unwrap_or(Codepage::Utf8),
            new_file_eol: match e.new_file_eol.as_str() {
                "crlf" => Eol::CrLf,
                "lf" => Eol::Lf,
                "cr" => Eol::Cr,
                _ => Eol::native(),
            },
            word_div: self.settings.system.word_div.clone(),
        }
    }

    fn existing_window(&self, path: &Path, kind: WindowKind) -> Option<usize> {
        self.windows.iter().position(|w| w.kind() == kind && w.file().as_deref() == Some(path))
    }

    pub fn open_viewer(&mut self, path: &Path) {
        if let Some(i) = self.existing_window(path, WindowKind::Viewer) {
            self.current = Some(i);
            return;
        }
        let opts = self.viewer_options();
        match ViewerCore::open(path, opts.auto_detect, opts.default_cp) {
            Ok(mut core) => {
                let key = format!("view:{}", path.display());
                if self.settings.viewer.save_position
                    && let Some(p) = self.db.load_pos(&key)
                {
                    if let Some(cp) = Codepage::from_id(p.codepage) {
                        core.set_codepage(cp);
                    }
                    core.goto_offset(p.top);
                    core.left = p.left as usize;
                }
                if self.settings.system.save_view_history {
                    self.db.add_history("view", &path.display().to_string(), "v");
                }
                let w = PositionSaver::Viewer(ViewerWindow::new(core, opts), self.db.clone(), key, self.settings.viewer.save_position);
                self.windows.push(Box::new(w));
                self.current = Some(self.windows.len() - 1);
            }
            Err(e) => gm_ui::message::error(
                &mut self.ui,
                msg(MsgId::MViewerTitle),
                &format!("{}\n{}\n{e}", msg(MsgId::MViewerCannotOpenFile), path.display()),
            ),
        }
    }

    /// Views in-memory text (module output, archive members).
    pub fn view_text(&mut self, title: &str, bytes: Vec<u8>) {
        let opts = self.viewer_options();
        let core = ViewerCore::from_bytes(PathBuf::from(title), bytes, true, opts.default_cp);
        let mut w = ViewerWindow::new(core, opts);
        w.temp_file = true;
        w.title_override = Some(title.to_string());
        self.windows.push(Box::new(w));
        self.current = Some(self.windows.len() - 1);
    }

    pub fn open_editor(&mut self, path: &Path, line: Option<usize>) {
        if let Some(i) = self.existing_window(path, WindowKind::Editor) {
            self.current = Some(i);
            return;
        }
        let opts = self.editor_options();
        match Editor::open(path, opts, self.editor_tools.clone()) {
            Ok(mut ed) => {
                let key = format!("edit:{}", path.display());
                if ed.readonly && self.settings.editor.warn_readonly {
                    let r = gm_ui::message(
                        &mut self.ui,
                        msg(MsgId::MEditTitle),
                        &format!("{}\n{}\n{}", path.display(), msg(MsgId::MEditRO), msg(MsgId::MEditROOpen)),
                        &[msg(MsgId::MHYes), msg(MsgId::MHNo)],
                    );
                    if r != Some(0) {
                        return;
                    }
                }
                ed.readonly = self.settings.editor.lock_readonly && ed.readonly;
                if let Some(l) = line {
                    ed.set_cursor(l, 0);
                } else if self.settings.editor.save_position
                    && let Some(p) = self.db.load_pos(&key)
                {
                    ed.restore_position(p.line as usize, p.col as usize, p.top as usize, p.left as usize);
                }
                if self.settings.system.save_view_history {
                    self.db.add_history("view", &path.display().to_string(), "e");
                }
                let w = PositionSaver::Editor(ed, self.db.clone(), key, self.settings.editor.save_position);
                self.windows.push(Box::new(w));
                self.current = Some(self.windows.len() - 1);
            }
            Err(e) => {
                gm_ui::message::error(&mut self.ui, msg(MsgId::MEditTitle), &format!("{}\n{}\n{e}", msg(MsgId::MEditCannotOpen), path.display()))
            }
        }
    }

    /// Ctrl+O: shows the terminal's own screen until a key is pressed.
    pub fn user_screen(&mut self) {
        if self.ui.term.suspend().is_ok() {
            let _ = self.ui.term.wait_key_suspended();
            let _ = self.ui.term.resume();
        }
    }

    fn panels_key(&mut self, k: KeyChord) {
        // Fast find (Alt+letters) mode.
        if self.active().kind == PanelKind::Files && self.active().files.fast_find.is_some() {
            match (k.code, k.mods) {
                (KeyCode::Char(c), m) if !m.ctrl() => {
                    self.active().files.fast_find_push(c);
                    return;
                }
                (KeyCode::Backspace, _) => {
                    self.active().files.fast_find_back();
                    return;
                }
                (KeyCode::Enter, Mods::CTRL) | (KeyCode::Char('j'), Mods::CTRL) => {
                    self.active().files.fast_find_next();
                    return;
                }
                (KeyCode::Esc, _) => {
                    self.active().files.fast_find = None;
                    return;
                }
                _ => self.active().files.fast_find = None,
            }
        }
        if let (KeyCode::Char(c), m) = (k.code, k.mods) {
            // Alt+letter starts fast find unless the key is bound to a command.
            if m.alt() && !m.ctrl() && self.active().kind == PanelKind::Files && self.keymap.panel_command(&k).is_none() {
                self.active().files.fast_find = Some(String::new());
                self.active().files.fast_find_push(c);
                return;
            }
        }
        // Tree panel navigation.
        if self.active().kind == PanelKind::Tree && self.tree_key(&k) {
            return;
        }
        let cmd_empty = self.cmdline.is_empty();
        // Command-line editing keys.
        match (k.code, k.mods) {
            (KeyCode::Enter, Mods::NONE) if !cmd_empty => {
                let line = self.cmdline.text();
                self.cmdline.set_text("");
                self.cmd_hist_pos = None;
                crate::exec::execute_line(self, &line);
                return;
            }
            (KeyCode::Esc, _) if !cmd_empty => {
                self.cmdline.set_text("");
                self.cmd_hist_pos = None;
                return;
            }
            (KeyCode::Char('e'), Mods::CTRL) | (KeyCode::Char('x'), Mods::CTRL) => {
                let hist = self.db.history("cmd");
                if hist.is_empty() {
                    return;
                }
                let pos = match (self.cmd_hist_pos, k.code == KeyCode::Char('e')) {
                    (None, true) => 0,
                    (None, false) => return,
                    (Some(p), true) => (p + 1).min(hist.len() - 1),
                    (Some(0), false) => {
                        self.cmd_hist_pos = None;
                        self.cmdline.set_text("");
                        return;
                    }
                    (Some(p), false) => p - 1,
                };
                self.cmd_hist_pos = Some(pos);
                self.cmdline.set_text(&hist[pos].0);
                return;
            }
            (KeyCode::Char('y'), Mods::CTRL) => {
                self.cmdline.set_text("");
                return;
            }
            (KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End | KeyCode::Backspace | KeyCode::Delete, m)
                if !(cmd_empty || m.alt() || (k.code == KeyCode::Delete && m.shift())) =>
            {
                self.cmdline.handle_key(&k);
                return;
            }
            (KeyCode::Char('+' | '-' | '*'), Mods::NONE) if !cmd_empty => {
                self.cmdline.handle_key(&k);
                return;
            }
            (KeyCode::Insert, Mods::SHIFT) | (KeyCode::Char('v'), Mods::CTRL) => {
                self.cmdline.handle_key(&KeyChord::shift(KeyCode::Insert));
                return;
            }
            _ => {}
        }
        if let Some(c) = self.keymap.panel_command(&k) {
            crate::commands::run(self, c);
            return;
        }
        // Panel navigation.
        let shift = k.mods.shift();
        let a = &mut self.active().files;
        match (k.code, k.mods.ctrl(), k.mods.alt()) {
            (KeyCode::Up, false, false) => {
                if shift {
                    let c = a.cursor;
                    let on = !a.selected.get(c).copied().unwrap_or(false);
                    a.set_selected(c, on);
                }
                a.move_by(-1);
            }
            (KeyCode::Down, false, false) => {
                if shift {
                    let c = a.cursor;
                    let on = !a.selected.get(c).copied().unwrap_or(false);
                    a.set_selected(c, on);
                }
                a.move_by(1);
            }
            (KeyCode::PageUp, false, false) => a.page(false),
            (KeyCode::PageDown, false, false) => a.page(true),
            (KeyCode::Home, false, false) => a.home(),
            (KeyCode::End, false, false) => a.end(),
            (KeyCode::Left, false, false) => a.column_move(false),
            (KeyCode::Right, false, false) => a.column_move(true),
            (KeyCode::Insert, false, false) => a.toggle_current(),
            (KeyCode::Delete, false, false) => crate::commands::run(self, crate::keymap::Command::Delete),
            (KeyCode::Enter, false, false) => crate::commands::enter(self, false),
            (KeyCode::Char(_), false, false) | (KeyCode::Char(_), true, true) => {
                self.cmdline.handle_key(&k);
            }
            (KeyCode::Left, true, false) => {
                self.settings.panels.left_width_percent = self.settings.panels.left_width_percent.saturating_sub(2).max(10);
            }
            (KeyCode::Right, true, false) => {
                self.settings.panels.left_width_percent = (self.settings.panels.left_width_percent + 2).min(90);
            }
            (KeyCode::Home, true, false) | (KeyCode::End, true, false) => {
                self.cmdline.handle_key(&KeyChord::plain(k.code));
            }
            _ => {
                self.cmdline.handle_key(&k);
            }
        }
    }

    fn tree_key(&mut self, k: &KeyChord) -> bool {
        let hidden = self.settings.panels.show_hidden;
        let t = &mut self.active().tree;
        let before = t.current().map(Path::to_path_buf);
        match (k.code, k.mods) {
            (KeyCode::Up, Mods::NONE) => t.move_by(-1),
            (KeyCode::Down, Mods::NONE) => t.move_by(1),
            (KeyCode::PageUp, Mods::NONE) => t.page(false),
            (KeyCode::PageDown, Mods::NONE) => t.page(true),
            (KeyCode::Home, Mods::NONE) => t.home(),
            (KeyCode::End, Mods::NONE) => t.end(),
            (KeyCode::Right, Mods::NONE) | (KeyCode::Char('+'), Mods::NONE) => t.expand(hidden),
            (KeyCode::Left, Mods::NONE) | (KeyCode::Char('-'), Mods::NONE) => t.collapse(),
            (KeyCode::Enter, Mods::NONE) => {
                // Enter: the tree panel becomes a file panel in that folder.
                if let Some(p) = t.current().map(Path::to_path_buf) {
                    let slot = self.active();
                    slot.kind = PanelKind::Files;
                    let _ = slot.files.set_local_dir(p);
                    self.dir_changed();
                }
                return true;
            }
            _ => return false,
        }
        let after = self.active().tree.current().map(Path::to_path_buf);
        if after != before
            && let Some(p) = after
        {
            // The other (file) panel follows the tree cursor.
            if self.passive().kind == PanelKind::Files {
                let _ = self.passive().files.set_local_dir(p.clone());
            }
            let _ = self.active().files.set_local_dir(p);
            self.setup_watcher();
        }
        true
    }

    fn panels_mouse(&mut self, m: gm_term::MouseEvent) {
        let area = self.ui.term.area();
        let kb = self.settings.interface.keybar;
        if kb && m.row == area.bottom() - 1 {
            if let MouseEventKind::Down(MouseButton::Left) = m.kind
                && let Some(n) = KeyBar::hit(m.column, area.width)
            {
                self.panels_key(KeyChord::plain(KeyCode::F(n)));
            }
            return;
        }
        if m.row == area.y && matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) && self.settings.interface.menubar_always {
            crate::commands::run(self, crate::keymap::Command::Menu);
            return;
        }
        let in_left = self.left.visible && m.column < self.left.rect.right() && m.row < self.left.rect.bottom();
        let in_right = self.right.visible && m.column >= self.right.rect.x && m.row < self.right.rect.bottom();
        if !in_left && !in_right {
            return;
        }
        match m.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let slot = if in_left { &mut self.left } else { &mut self.right };
                let d = if m.kind == MouseEventKind::ScrollDown { 1 } else { -1 };
                slot.files.move_by(d);
                slot.tree.move_by(d);
            }
            MouseEventKind::Down(btn) => {
                if ((in_left && !self.active_left) || (in_right && self.active_left))
                    && (if in_left { &self.left } else { &self.right }).kind != PanelKind::Info
                {
                    self.active_left = in_left;
                    if let Some(d) = self.active().files.local_dir() {
                        let _ = std::env::set_current_dir(d);
                    }
                }
                let slot = if in_left { &mut self.left } else { &mut self.right };
                if slot.kind != PanelKind::Files {
                    return;
                }
                if let Some(idx) = slot.files.hit(m.column, m.row) {
                    slot.files.cursor = idx;
                    if btn == MouseButton::Right {
                        let on = !slot.files.selected.get(idx).copied().unwrap_or(false);
                        slot.files.set_selected(idx, on);
                        return;
                    }
                    let now = Instant::now();
                    let double =
                        self.last_click.is_some_and(|(t, x, y)| now.duration_since(t) < Duration::from_millis(400) && x == m.column && y == m.row);
                    self.last_click = Some((now, m.column, m.row));
                    if double && btn == MouseButton::Left {
                        self.last_click = None;
                        crate::commands::enter(self, false);
                    }
                }
            }
            _ => {}
        }
    }

    // ----- persistence -----

    pub fn save_state(&mut self) {
        let save = |slot: &PanelSlot, st: &mut gm_config::PanelState| {
            st.path = slot.files.base_dir().display().to_string();
            st.mode = slot.files.mode;
            st.sort = slot.files.sort.key().to_string();
            st.reverse = slot.files.reverse;
            st.visible = slot.visible;
            st.kind = slot.kind.name().to_string();
        };
        save(&self.left, &mut self.settings.left);
        save(&self.right, &mut self.settings.right);
        self.settings.colors = palette::changed_overrides();
        self.settings.language = gm_i18n::current_language().to_string();
        if self.settings.system.auto_save_setup {
            let _ = self.settings.save(&self.settings_path);
        }
    }

    pub fn save_setup(&mut self) {
        let auto = self.settings.system.auto_save_setup;
        self.settings.system.auto_save_setup = true;
        self.save_state();
        self.settings.system.auto_save_setup = auto;
        let _ = self.settings.save(&self.settings_path);
    }

    /// Re-applies settings after an options dialog.
    pub fn apply_settings(&mut self) {
        let opts = panel_options(&self.settings);
        self.highlighter = Rc::new(Highlighter::new(&self.settings.highlight, &self.settings.mask_groups));
        for slot in [&mut self.left, &mut self.right] {
            slot.files.opts = opts.clone();
            slot.files.highlighter = self.highlighter.clone();
            slot.files.reload();
        }
        self.ui.term.alt_keymap = self.settings.interface.alt_keymap && !cfg!(windows);
        palette::set_color_mode(palette::detect_color_mode(&self.settings.interface.colors));
        self.setup_watcher();
    }
}

fn render_slot(buf: &mut Buffer, slot: &mut PanelSlot, other: &mut PanelSlot, active: bool, cursor: &mut Option<(u16, u16)>) {
    if !slot.visible {
        return;
    }
    let r = slot.rect;
    match slot.kind {
        PanelKind::Files => {
            slot.files.render(buf, r, active);
            if active && let Some(c) = slot.files.fast_find_cursor(r) {
                *cursor = Some(c);
            }
        }
        PanelKind::Tree => slot.tree.render(buf, r, active),
        PanelKind::Info => gm_panels::info::render(buf, r, &other.files, active),
        PanelKind::QuickView => {
            slot.qview.update(&mut other.files);
            slot.qview.render(buf, r, active);
        }
    }
}

/// Wraps viewers/editors to remember their position when closed.
enum PositionSaver {
    Viewer(ViewerWindow, Rc<StateDb>, String, bool),
    Editor(Editor, Rc<StateDb>, String, bool),
}

impl PositionSaver {
    fn inner(&mut self) -> &mut dyn Window {
        match self {
            PositionSaver::Viewer(w, ..) => w,
            PositionSaver::Editor(e, ..) => e,
        }
    }

    fn inner_ref(&self) -> &dyn Window {
        match self {
            PositionSaver::Viewer(w, ..) => w,
            PositionSaver::Editor(e, ..) => e,
        }
    }

    fn save(&self) {
        match self {
            PositionSaver::Viewer(w, db, key, true) => {
                db.save_pos(key, &FilePos { top: w.core.top, left: w.core.left as u64, codepage: w.core.cp.id(), ..Default::default() })
            }
            PositionSaver::Editor(e, db, key, true) => {
                let (line, col, top, left) = e.position();
                db.save_pos(
                    key,
                    &FilePos {
                        line: line as u64,
                        col: col as u64,
                        top: top as u64,
                        left: left as u64,
                        codepage: e.buf.cp.id(),
                        bookmarks: String::new(),
                    },
                );
            }
            _ => {}
        }
    }
}

impl Window for PositionSaver {
    fn title(&self) -> String {
        self.inner_ref().title()
    }
    fn kind(&self) -> WindowKind {
        self.inner_ref().kind()
    }
    fn render(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        self.inner().render(buf, area)
    }
    fn handle(&mut self, ev: &Event, ui: &mut Ui) -> WinAction {
        let a = self.inner().handle(ev, ui);
        if matches!(a, WinAction::Close | WinAction::Replace(_)) {
            self.save();
        }
        a
    }
    fn can_close(&mut self, ui: &mut Ui) -> bool {
        let ok = self.inner().can_close(ui);
        if ok {
            self.save();
        }
        ok
    }
    fn tick(&mut self, ui: &mut Ui) -> bool {
        self.inner().tick(ui)
    }
    fn modified(&self) -> bool {
        self.inner_ref().modified()
    }
    fn file(&self) -> Option<PathBuf> {
        self.inner_ref().file()
    }
    fn help_topic(&self) -> &str {
        self.inner_ref().help_topic()
    }
}

//! Terminal session: raw mode, alternate screen, mouse, keyboard
//! enhancement, buffered rendering and suspend/resume around commands.

use crate::keys::KeyChord;
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, KeyCode, KeyboardEnhancementFlags, MouseEvent,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute, queue};
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use std::collections::VecDeque;
use std::io::{self, Stdout, Write};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Key(KeyChord),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Paste(String),
    /// Window focus changes (used to refresh panels).
    Focus(bool),
}

pub struct Term {
    /// `None` in headless mode (tests): frames go to `last`, input comes from `queue`.
    terminal: Option<ratatui::Terminal<CrosstermBackend<Stdout>>>,
    queue: VecDeque<Event>,
    /// Headless mode: every rendered frame as text, when enabled.
    pub recorded: Option<Vec<String>>,
    last: Buffer,
    enhanced_keys: bool,
    mouse: bool,
    /// Esc followed by a digit acts as F1…F10 (for terminals without F-keys).
    pub alt_keymap: bool,
    pending: Option<Event>,
    cursor: Option<(u16, u16)>,
    /// Hardware cursor shape: true = block (insert off), false = underline.
    pub block_cursor: bool,
}

/// Restores the terminal; safe to call more than once.
pub fn restore_terminal() {
    let mut out = io::stdout();
    let _ = execute!(out, PopKeyboardEnhancementFlags);
    let _ = execute!(out, DisableBracketedPaste, DisableMouseCapture, LeaveAlternateScreen, cursor::Show);
    let _ = terminal::disable_raw_mode();
}

/// Installs a panic hook that restores the terminal and writes a crash report.
pub fn install_panic_hook(report_dir: std::path::PathBuf) {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        let msg = format!("Goodevening Manager {} crashed.\n{info}\n\n{}\n", env!("CARGO_PKG_VERSION"), std::backtrace::Backtrace::force_capture());
        let file = report_dir.join("gm-crash.log");
        if std::fs::write(&file, &msg).is_ok() {
            eprintln!("A crash report was written to {}", file.display());
        }
        prev(info);
    }));
}

impl Term {
    pub fn new(mouse: bool) -> io::Result<Term> {
        terminal::enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, EnableBracketedPaste)?;
        if mouse {
            execute!(out, EnableMouseCapture)?;
        }
        // The kitty keyboard protocol reports Ctrl+Enter, Ctrl+Shift+letters etc.
        let enhanced_keys = matches!(terminal::supports_keyboard_enhancement(), Ok(true));
        if enhanced_keys {
            let _ = execute!(
                out,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS)
            );
        }
        let terminal = ratatui::Terminal::new(CrosstermBackend::new(io::stdout()))?;
        let size = terminal.size()?;
        Ok(Term {
            terminal: Some(terminal),
            queue: VecDeque::new(),
            recorded: None,
            last: Buffer::empty(Rect::new(0, 0, size.width, size.height)),
            enhanced_keys,
            mouse,
            alt_keymap: !cfg!(windows),
            pending: None,
            cursor: None,
            block_cursor: false,
        })
    }

    /// A terminal without a TTY for scripted UI tests. When the scripted
    /// input runs out, `next_event` returns Esc so every modal loop ends.
    pub fn headless(width: u16, height: u16) -> Term {
        Term {
            terminal: None,
            queue: VecDeque::new(),
            recorded: None,
            last: Buffer::empty(Rect::new(0, 0, width, height)),
            enhanced_keys: false,
            mouse: false,
            alt_keymap: false,
            pending: None,
            cursor: None,
            block_cursor: false,
        }
    }

    pub fn is_headless(&self) -> bool {
        self.terminal.is_none()
    }

    /// Queues scripted input (headless mode).
    pub fn push_event(&mut self, ev: Event) {
        self.queue.push_back(ev);
    }

    /// The last frame as plain text, one line per row (for snapshot tests).
    pub fn screen_text(&self) -> String {
        let b = &self.last;
        let mut out = String::new();
        for y in 0..b.area.height {
            let mut line = String::new();
            for x in 0..b.area.width {
                line.push_str(b[(x, y)].symbol());
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }

    pub fn cursor_position(&self) -> Option<(u16, u16)> {
        self.cursor
    }

    pub fn size(&self) -> (u16, u16) {
        let r = self.last.area;
        (r.width, r.height)
    }

    pub fn area(&self) -> Rect {
        self.last.area
    }

    pub fn has_enhanced_keys(&self) -> bool {
        self.enhanced_keys
    }

    /// Renders a frame: `f` fills the whole buffer and returns the cursor
    /// position (or `None` to hide it); only changed cells are written.
    pub fn draw(&mut self, f: impl FnOnce(&mut Buffer) -> Option<(u16, u16)>) -> io::Result<()> {
        let block = self.block_cursor;
        let mut copy = None;
        let mut cursor = None;
        let Some(terminal) = self.terminal.as_mut() else {
            let mut buf = Buffer::empty(self.last.area);
            self.cursor = f(&mut buf);
            self.last = buf;
            if self.recorded.is_some() {
                let t = self.screen_text();
                if let Some(r) = self.recorded.as_mut() {
                    r.push(t);
                }
            }
            return Ok(());
        };
        terminal.draw(|frame| {
            let buf = frame.buffer_mut();
            cursor = f(buf);
            copy = Some(buf.clone());
            if let Some((x, y)) = cursor {
                frame.set_cursor_position(Position::new(x, y));
            }
        })?;
        self.cursor = cursor;
        if cursor.is_some() {
            let style = if block { cursor::SetCursorStyle::SteadyBlock } else { cursor::SetCursorStyle::SteadyUnderScore };
            let _ = execute!(io::stdout(), style);
        }
        if let Some(b) = copy {
            self.last = b;
        }
        Ok(())
    }

    /// The last rendered frame (background for modal dialogs).
    pub fn last_frame(&self) -> &Buffer {
        &self.last
    }

    /// Forces a full repaint on the next draw.
    pub fn invalidate(&mut self) {
        if let Some(t) = self.terminal.as_mut() {
            let _ = t.clear();
        }
    }

    /// Waits up to `timeout` for an input event.
    pub fn next_event(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        if let Some(e) = self.pending.take() {
            return Ok(Some(e));
        }
        if self.terminal.is_none() {
            // A non-blocking poll sees only scripted input; a blocking wait
            // on an empty script gets Esc so that modal loops end.
            return Ok(match self.queue.pop_front() {
                Some(e) => Some(e),
                None if timeout.is_zero() => None,
                None => Some(Event::Key(KeyChord::plain(KeyCode::Esc))),
            });
        }
        // A resize the terminal did not report (callers wait at most about
        // a second between calls, so the screen is repaired soon).
        if let Some(e) = self.sync_size() {
            return Ok(Some(e));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if !event::poll(left)? {
                return Ok(self.sync_size());
            }
            if let Some(e) = self.translate(event::read()?) {
                if self.alt_keymap
                    && let Event::Key(k) = &e
                    && k.code == KeyCode::Esc
                    && k.mods.0 == 0
                    && event::poll(Duration::from_millis(250))?
                {
                    let next = self.translate(event::read()?);
                    if let Some(Event::Key(n)) = &next
                        && let KeyCode::Char(d @ '0'..='9') = n.code
                    {
                        let f = if d == '0' { 10 } else { d as u8 - b'0' };
                        return Ok(Some(Event::Key(KeyChord::new(KeyCode::F(f), n.mods))));
                    }
                    self.pending = next;
                }
                return Ok(Some(e));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }

    fn translate(&mut self, ev: event::Event) -> Option<Event> {
        match ev {
            event::Event::Key(k) => KeyChord::from_event(&k).map(Event::Key),
            event::Event::Mouse(m) => Some(Event::Mouse(m)),
            event::Event::Resize(w, h) => Some(self.resized(w, h)),
            event::Event::Paste(s) => Some(Event::Paste(s)),
            event::Event::FocusGained => Some(Event::Focus(true)),
            event::Event::FocusLost => Some(Event::Focus(false)),
        }
    }

    /// Adopts a new terminal size: `(w, h)` from a resize event, replaced by
    /// the real visible size when it can be read.
    fn resized(&mut self, mut w: u16, mut h: u16) -> Event {
        if let Some(t) = self.terminal.as_mut() {
            let _ = t.autoresize();
            // The terminal may have reflowed (re-wrapped) what was on screen,
            // or output written for the old size may have landed after the
            // resize (Windows Terminal resizes asynchronously). The previous
            // frame no longer matches the screen, so repaint everything, not
            // only the diff.
            let _ = t.clear();
            // Windows reports the buffer size in the event; the visible
            // window is what is drawn.
            if let Ok(s) = t.size() {
                (w, h) = (s.width, s.height);
            }
        }
        self.last = Buffer::empty(Rect::new(0, 0, w, h));
        Event::Resize(w, h)
    }

    /// Compares the real terminal size with the size of the last frame and,
    /// when they differ, adopts the new size as a resize event would.
    /// Terminals do not always report a resize: Windows Terminal loses it
    /// when gm moves its own window (F9 → Commands → Terminal position /
    /// Multiple terminals). Call it after changing the window size.
    pub fn sync_size(&mut self) -> Option<Event> {
        let s = self.terminal.as_ref()?.size().ok()?;
        if (s.width, s.height) == self.size() || s.width == 0 || s.height == 0 {
            return None;
        }
        Some(self.resized(s.width, s.height))
    }

    /// Leaves the alternate screen and raw mode (to run a command or show
    /// the user screen). Call [`Term::resume`] afterwards.
    pub fn suspend(&mut self) -> io::Result<()> {
        if self.terminal.is_none() {
            return Ok(());
        }
        let mut out = io::stdout();
        if self.enhanced_keys {
            let _ = execute!(out, PopKeyboardEnhancementFlags);
        }
        if self.mouse {
            let _ = execute!(out, DisableMouseCapture);
        }
        execute!(out, DisableBracketedPaste, LeaveAlternateScreen, cursor::Show, cursor::SetCursorStyle::DefaultUserShape)?;
        terminal::disable_raw_mode()?;
        out.flush()
    }

    pub fn resume(&mut self) -> io::Result<()> {
        let Some(term) = self.terminal.as_mut() else { return Ok(()) };
        terminal::enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, EnableBracketedPaste)?;
        if self.mouse {
            execute!(out, EnableMouseCapture)?;
        }
        if self.enhanced_keys {
            let _ = execute!(
                out,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS)
            );
        }
        let _ = term.autoresize();
        let s = term.size()?;
        self.last = Buffer::empty(Rect::new(0, 0, s.width, s.height));
        term.clear()
    }

    /// Waits for any key in cooked mode (after a command finished, Ctrl+O view).
    pub fn wait_key_suspended(&mut self) -> io::Result<()> {
        if self.terminal.is_none() {
            self.queue.pop_front();
            return Ok(());
        }
        terminal::enable_raw_mode()?;
        loop {
            if let event::Event::Key(k) = event::read()?
                && k.kind != event::KeyEventKind::Release
            {
                break;
            }
        }
        terminal::disable_raw_mode()
    }

    /// Sets the terminal window title (OSC 0).
    pub fn set_title(&mut self, title: &str) {
        if self.terminal.is_none() {
            return;
        }
        let _ = queue!(io::stdout(), terminal::SetTitle(title));
    }

    /// Terminal taskbar progress (OSC 9;4, Windows Terminal / ConEmu). `None` clears it.
    pub fn set_progress(&mut self, percent: Option<u8>) {
        if self.terminal.is_none() {
            return;
        }
        let seq = match percent {
            Some(p) => format!("\x1b]9;4;1;{}\x07", p.min(100)),
            None => "\x1b]9;4;0;0\x07".to_string(),
        };
        let mut out = io::stdout();
        let _ = out.write_all(seq.as_bytes());
        let _ = out.flush();
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        if self.terminal.is_some() {
            restore_terminal();
        }
    }
}

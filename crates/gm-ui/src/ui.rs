//! The UI context: terminal, history store and the nested modal loop.

use gm_term::{Buffer, Event, KeyChord, KeyCode, Rect, Term};
use std::io;
use std::time::Duration;

/// Storage for edit-field histories (implemented by gm-config's `StateDb`).
pub trait HistoryStore {
    fn list(&self, kind: &str) -> Vec<String>;
    fn add(&self, kind: &str, item: &str);
    fn delete(&self, _kind: &str, _item: &str) {}
    fn clear(&self, _kind: &str) {}
}

pub struct NoHistory;

impl HistoryStore for NoHistory {
    fn list(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
    fn add(&self, _: &str, _: &str) {}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Done,
}

/// A modal window drawn over a snapshot of the screen.
pub trait Modal {
    /// Draws over `buf` (which already holds the background). Returns the cursor position.
    fn render(&mut self, buf: &mut Buffer, screen: Rect) -> Option<(u16, u16)>;
    fn handle(&mut self, ev: &Event, ui: &mut Ui) -> Flow;
    /// Called about twice a second when idle; true = redraw.
    fn tick(&mut self, _ui: &mut Ui) -> bool {
        false
    }
}

pub struct Ui {
    pub term: Term,
    pub history: Box<dyn HistoryStore>,
    /// Opens context help for a topic (set by gm-core to the gm-help viewer).
    pub help: Option<fn(&mut Ui, &str)>,
    /// Help topic of the innermost window/dialog (F1).
    pub help_topic: String,
    /// Background frames for nested modals.
    backgrounds: Vec<Buffer>,
    /// Snapshot used by `draw_over` between modal loops.
    pub insert_mode: bool,
}

impl Ui {
    pub fn new(term: Term, history: Box<dyn HistoryStore>) -> Ui {
        Ui { term, history, help: None, help_topic: String::new(), backgrounds: Vec::new(), insert_mode: true }
    }

    pub fn screen(&self) -> Rect {
        self.term.area()
    }

    /// Runs a modal window until it returns `Flow::Done`.
    pub fn run_modal(&mut self, m: &mut dyn Modal) -> io::Result<()> {
        self.backgrounds.push(self.term.last_frame().clone());
        let r = self.modal_loop(m);
        self.backgrounds.pop();
        r
    }

    fn modal_loop(&mut self, m: &mut dyn Modal) -> io::Result<()> {
        let mut dirty = true;
        loop {
            if dirty {
                self.paint(m)?;
            }
            match self.term.next_event(Duration::from_millis(500))? {
                Some(Event::Resize(..)) => dirty = true,
                Some(Event::Key(k)) if k.code == KeyCode::F(1) && k.mods.0 == 0 && self.help.is_some() => {
                    let topic = self.help_topic.clone();
                    if let Some(h) = self.help {
                        h(self, &topic);
                    }
                    dirty = true;
                }
                Some(ev) => {
                    if m.handle(&ev, self) == Flow::Done {
                        return Ok(());
                    }
                    dirty = true;
                }
                None => dirty = m.tick(self),
            }
        }
    }

    fn paint(&mut self, m: &mut dyn Modal) -> io::Result<()> {
        let screen = self.term.area();
        let mut bg = self.backgrounds.last().cloned().unwrap_or_else(|| Buffer::empty(screen));
        if bg.area != screen {
            bg.resize(screen);
        }
        self.term.draw(|buf| {
            *buf = bg;
            m.render(buf, screen)
        })
    }

    /// Draws something over the current screen once, without an event loop
    /// (progress windows during long operations).
    pub fn draw_over(&mut self, bg: &Buffer, f: impl FnOnce(&mut Buffer, Rect)) {
        let screen = self.term.area();
        let mut b = bg.clone();
        if b.area != screen {
            b.resize(screen);
        }
        let _ = self.term.draw(|buf| {
            *buf = b;
            f(buf, screen);
            None
        });
    }

    /// Non-blocking check for a key press (Esc during long operations).
    pub fn poll_key(&mut self) -> Option<KeyChord> {
        match self.term.next_event(Duration::ZERO) {
            Ok(Some(Event::Key(k))) => Some(k),
            _ => None,
        }
    }

    /// Shows help for `topic` if a help viewer is installed.
    pub fn show_help(&mut self, topic: &str) {
        if let Some(h) = self.help {
            h(self, topic);
        }
    }
}

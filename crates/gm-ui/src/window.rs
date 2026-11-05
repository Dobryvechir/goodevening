//! Non-modal windows switched by the manager (panels, viewers, editors).

use crate::ui::Ui;
use gm_term::{Buffer, Event, Rect};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    Panels,
    Viewer,
    Editor,
    Help,
}

/// Requests a window makes to the manager.
#[derive(Clone, Debug)]
pub enum OpenRequest {
    View(PathBuf),
    /// The viewer with syntax colors turned on (F5 "Colors" in the editor).
    ViewColors(PathBuf),
    Edit {
        path: PathBuf,
        line: Option<usize>,
    },
    Help(String),
    /// Show the user screen (Ctrl+O).
    UserScreen,
}

#[derive(Clone, Debug)]
pub enum WinAction {
    None,
    Redraw,
    Close,
    /// Close and open a different window in its place (F6 viewer ↔ editor).
    Replace(OpenRequest),
    Open(OpenRequest),
    /// F12 screens list.
    ScreensList,
    /// Ctrl+Tab / Ctrl+Shift+Tab.
    NextWindow,
    PrevWindow,
    /// Go to the panels window.
    Panels,
    Quit,
}

pub trait Window {
    fn title(&self) -> String;
    fn kind(&self) -> WindowKind;
    /// Renders into `area` (full screen). Returns the cursor position.
    fn render(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)>;
    fn handle(&mut self, ev: &Event, ui: &mut Ui) -> WinAction;
    /// Asked before closing (save changes?). False keeps the window open.
    fn can_close(&mut self, _ui: &mut Ui) -> bool {
        true
    }
    /// Periodic tick (clock, tail mode). True = redraw.
    fn tick(&mut self, _ui: &mut Ui) -> bool {
        false
    }
    /// True if the window has unsaved changes (shown with `*` in F12 list).
    fn modified(&self) -> bool {
        false
    }
    /// The file this window shows, for the F12 list and dedup on open.
    fn file(&self) -> Option<PathBuf> {
        None
    }
    fn help_topic(&self) -> &str {
        "Contents"
    }
}

/// File extensions the viewer can show in "Colors" mode (F5).
pub const COLOR_EXTENSIONS: &[&str] = &["html", "htm", "cs", "js", "ts", "tsx", "php", "java", "py", "py3", "json", "xml", "go", "dart", "rs", "md"];

/// Whether `path` has an extension supported by the viewer's "Colors" mode.
pub fn colors_supported(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| COLOR_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

//! Compile-time modules: the replacement for Far's plugin API.
//!
//! A module talks to the core only through these typed traits. There is
//! no dynamic loading and no ABI; each module is a crate behind a Cargo
//! feature of `gm-app`.

use gm_config::Settings;
use gm_editor::EditorTool;
use gm_panels::FilePanel;
use gm_ui::Ui;
use gm_vfs::VfsProvider;
use std::path::{Path, PathBuf};

/// A command shown in the F11 "Commands" menu.
#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub id: &'static str,
    pub title: String,
}

/// An extra entry in the Alt+F1/Alt+F2 drive menu.
#[derive(Clone, Debug)]
pub struct DriveItem {
    pub id: usize,
    pub title: String,
}

/// Something a module asks the core to do after it returns.
#[derive(Clone, Debug)]
pub enum Action {
    View(PathBuf),
    /// Shows text in the viewer (process details, comparison report…).
    ViewText {
        title: String,
        text: String,
    },
    Edit(PathBuf),
    /// Runs a command line as if typed by the user.
    Execute(String),
    /// Sets the active panel to a local folder.
    Cd(PathBuf),
    Message {
        title: String,
        text: String,
    },
    /// Re-reads both panels.
    Refresh,
}

/// Access to the core state for module code.
pub struct Ctx<'a> {
    pub ui: &'a mut Ui,
    pub active: &'a mut FilePanel,
    pub passive: &'a mut FilePanel,
    pub settings: &'a Settings,
    pub actions: Vec<Action>,
}

impl Ctx<'_> {
    pub fn push(&mut self, a: Action) {
        self.actions.push(a);
    }
}

pub trait Module {
    /// Stable id: "archive", "temp_panel", "process_panel"…
    fn id(&self) -> &'static str;
    fn title(&self) -> String;
    /// F11 menu commands.
    fn commands(&self) -> Vec<CommandSpec> {
        Vec::new()
    }
    fn run_command(&self, _id: &str, _ctx: &mut Ctx) {}
    /// Command-line prefixes handled by this module (`tmp:`, `proc:`…), without the colon.
    fn prefixes(&self) -> &[&'static str] {
        &[]
    }
    /// Handles `prefix:arg` typed on the command line. Returns true if handled.
    fn open_prefix(&self, _ctx: &mut Ctx, _prefix: &str, _arg: &str) -> bool {
        false
    }
    /// Opens a file as a panel (archives). `head` is the first bytes of the file.
    fn open_file(&self, _path: &Path, _head: &[u8]) -> Option<Box<dyn VfsProvider>> {
        None
    }
    fn drive_menu_items(&self) -> Vec<DriveItem> {
        Vec::new()
    }
    fn open_drive_item(&self, _id: usize, _ctx: &mut Ctx) {}
    /// Editor commands for the editor's F11 menu.
    fn editor_tools(&self) -> Vec<Box<dyn EditorTool>> {
        Vec::new()
    }
    /// Builds a panel from a list of paths (temporary panel; Find File → Panel).
    fn list_panel(&self, _items: Vec<PathBuf>) -> Option<Box<dyn VfsProvider>> {
        None
    }
}

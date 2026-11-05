//! Far-style widgets on top of a ratatui `Buffer`: frames, menus, the
//! key bar, line editors, the dialog engine and message boxes (replaces
//! `dialog.cpp`, `vmenu*.cpp`, `hmenu.cpp`, `keybar.cpp`, `message.cpp`,
//! `edit.cpp` / `dlgedit.cpp`).
#![forbid(unsafe_code)]

pub mod clipboard;
pub mod dialog;
pub mod draw;
pub mod edit;
pub mod keybar;
pub mod menubar;
pub mod message;
pub mod search;
pub mod ui;
pub mod vmenu;
pub mod window;

pub use dialog::{Dialog, DialogBuilder, ItemKind};
pub use edit::LineEdit;
pub use keybar::{KeyBar, KeyBarLabels};
pub use menubar::{MenuBar, MenuBarResult};
pub use message::{message, message_warn};
pub use ui::{Flow, HistoryStore, Modal, NoHistory, Ui};
pub use vmenu::{MenuItem, MenuResult, VMenu};
pub use window::{OpenRequest, WinAction, Window, WindowKind, colors_supported};

pub use gm_i18n::{MsgId, msg, plain};

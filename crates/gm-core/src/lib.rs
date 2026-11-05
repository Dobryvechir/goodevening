//! gm core: the window manager, event loop, panel commands, command line,
//! menus, options and the compile-time module registry.
#![forbid(unsafe_code)]

pub mod app;
pub mod codec;
pub mod commands;
pub mod encoded;
pub mod exec;
pub mod fileops;
pub mod keymap;
pub mod menus;
pub mod module;
pub mod options;
pub mod queue;
pub mod usermenu;

pub use app::{App, DbHistory};
pub use keymap::{Command, Keymap};
pub use module::{Action, CommandSpec, Ctx, DriveItem, Module};

// Re-exports for module crates.
pub use gm_config;
pub use gm_editor;
pub use gm_i18n;
pub use gm_panels;
pub use gm_term;
pub use gm_text;
pub use gm_ui;
pub use gm_vfs;

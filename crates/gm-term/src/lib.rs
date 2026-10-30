//! Terminal layer: crossterm I/O, ratatui buffer diffing, key decoding and
//! the color palette (replaces `console.cpp`, `scrbuf.cpp`, `keyboard.cpp`
//! and `palette.cpp`).
#![forbid(unsafe_code)]

pub mod keys;
pub mod palette;
mod palette_gen;
pub mod term;

pub use keys::{KeyChord, Mods};
pub use palette::{C16, PaletteId, style};
pub use ratatui::buffer::{Buffer, Cell};
pub use ratatui::layout::{Position, Rect};
pub use ratatui::style::{Color, Modifier, Style};
pub use term::{Event, Term};

pub use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};

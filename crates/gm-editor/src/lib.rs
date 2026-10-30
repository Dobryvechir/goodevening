//! The internal editor (F4) (replaces `editor.cpp`, `fileedit.cpp`,
//! `edit.cpp`, `eol.cpp`): rope buffer, undo/redo, stream and vertical
//! blocks, search/replace, code pages and line-ending preservation.
#![forbid(unsafe_code)]

pub mod buffer;
mod editor;

pub use buffer::TextBuffer;
pub use editor::{Editor, EditorOptions, EditorTool};

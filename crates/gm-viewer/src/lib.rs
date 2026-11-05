//! The internal viewer (F3) and the quick view panel core
//! (replaces `viewer.cpp`, `fileview.cpp`, `map_file.cpp`).
//!
//! Files are memory-mapped and never read whole into memory. All
//! positions are byte offsets, so code pages with any unit size work.
#![forbid(unsafe_code)]

mod colorview;
mod core;
pub mod syntax;
mod window;

pub use core::{Mode, ViewerCore};
pub use window::{ViewerOptions, ViewerWindow};

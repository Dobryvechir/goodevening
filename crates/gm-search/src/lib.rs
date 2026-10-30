//! Find File (Alt+F7, `findfile.cpp`): a background search with streamed
//! results, masks, content search in several code pages, and the results
//! window with Go to / View / Edit / Panel actions.
#![forbid(unsafe_code)]

mod engine;
mod ui;

pub use engine::{Area, FindMsg, FindParams, start};
pub use ui::{FindOutcome, find_dialog, results_window};

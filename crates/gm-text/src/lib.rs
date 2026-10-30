//! Text utilities shared by every gm crate: display width, code pages,
//! line endings, Far-style file masks, natural sort and regex syntax.
#![forbid(unsafe_code)]

pub mod codepage;
pub mod eol;
pub mod masks;
pub mod regex_adapter;
pub mod sort;
pub mod width;

pub use codepage::Codepage;
pub use eol::Eol;
pub use masks::FileMask;
pub use width::{fit, str_width, truncate_left, truncate_right};

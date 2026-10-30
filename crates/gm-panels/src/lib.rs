//! Panels (replaces `filelist.cpp`, `panel.cpp`, `treelist.cpp`,
//! `infolist.cpp`, `qview.cpp`, `hilight.cpp`, `dizlist.cpp`).
#![forbid(unsafe_code)]

pub mod columns;
pub mod diz;
pub mod filepanel;
pub mod format;
pub mod highlight;
pub mod info;
pub mod qview;
mod render;
pub mod sort;
pub mod tree;

pub use columns::{Column, ColumnKind, ColumnSet, view_mode};
pub use filepanel::{EnterResult, FilePanel};
pub use highlight::Highlighter;
pub use sort::SortMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    Files,
    Tree,
    Info,
    QuickView,
}

impl PanelKind {
    pub fn name(&self) -> &'static str {
        match self {
            PanelKind::Files => "files",
            PanelKind::Tree => "tree",
            PanelKind::Info => "info",
            PanelKind::QuickView => "qview",
        }
    }
    pub fn from_name(s: &str) -> PanelKind {
        match s {
            "tree" => PanelKind::Tree,
            "info" => PanelKind::Info,
            "qview" => PanelKind::QuickView,
            _ => PanelKind::Files,
        }
    }
}

/// Display options shared by both panels (from settings).
#[derive(Clone, Debug)]
pub struct PanelOptions {
    pub show_hidden: bool,
    pub highlight: bool,
    pub select_folders: bool,
    pub natural_sort: bool,
    pub show_dotdot_in_root: bool,
    pub column_titles: bool,
    pub status_line: bool,
    pub total_info: bool,
    pub free_space: bool,
    pub scrollbar: bool,
}

impl Default for PanelOptions {
    fn default() -> Self {
        PanelOptions {
            show_hidden: true,
            highlight: true,
            select_folders: false,
            natural_sort: true,
            show_dotdot_in_root: false,
            column_titles: true,
            status_line: true,
            total_info: true,
            free_space: true,
            scrollbar: false,
        }
    }
}

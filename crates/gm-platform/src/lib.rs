//! Operating-system adapters. Every `cfg(windows)` / `cfg(unix)` difference
//! lives here, behind one API, so UI crates never use `cfg` themselves.
//!
//! This is the only gm crate allowed to contain `unsafe` code (Win32 calls).

pub mod attrs;
pub mod drives;
pub mod shell;
pub mod users;

pub use attrs::Attrs;
pub use drives::{DriveInfo, DriveKind};

/// True when file names on this platform are compared case-insensitively.
pub fn case_insensitive_names() -> bool {
    cfg!(windows) || cfg!(target_os = "macos")
}

/// Platform name used in titles and the About box.
pub fn os_name() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "Unix"
    }
}

pub use memmap2::Mmap;

/// Read-only memory map of a file (for the viewer and Find File).
pub fn map_file(file: &std::fs::File) -> std::io::Result<memmap2::Mmap> {
    // SAFETY: the map is read-only. If another process truncates the file
    // while it is mapped, reads may fault; the viewer re-checks the file
    // length on every tick and remaps, which is the same contract as Far's
    // `map_file.cpp`.
    unsafe { memmap2::Mmap::map(file) }
}

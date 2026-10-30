//! Configuration and persistent state (replaces `config.cpp`, `configdb.cpp`,
//! `history.cpp` storage and `poscache.cpp`).
//!
//! * `settings.toml` – the typed [`Settings`] struct (edited by F9 → Options).
//! * `state.db` – SQLite with histories, file positions and folder shortcuts.
//!   WAL mode and a busy timeout keep several gm instances safe.
#![forbid(unsafe_code)]

pub mod settings;
pub mod state;

pub use settings::*;
pub use state::{FilePos, StateDb};

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static PROFILE: OnceLock<PathBuf> = OnceLock::new();

/// Sets the profile directory (call once at startup; `-p` makes it portable).
pub fn init_profile(portable: bool, explicit: Option<PathBuf>) -> &'static Path {
    PROFILE.get_or_init(|| {
        let dir = if let Some(d) = explicit {
            d
        } else if portable {
            std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("gm-profile"))).unwrap_or_else(|| PathBuf::from("gm-profile"))
        } else {
            directories::BaseDirs::new().map(|b| b.config_dir().join("gm")).unwrap_or_else(|| PathBuf::from(".gm"))
        };
        let _ = std::fs::create_dir_all(&dir);
        dir
    })
}

/// The profile directory (`%APPDATA%\gm`, `~/.config/gm`, `~/Library/Application Support/gm`).
pub fn profile_dir() -> &'static Path {
    init_profile(false, None)
}

pub fn settings_path() -> PathBuf {
    profile_dir().join("settings.toml")
}

pub fn keymap_path() -> PathBuf {
    profile_dir().join("keymap.toml")
}

pub fn global_user_menu_path() -> PathBuf {
    profile_dir().join("GmMenu.ini")
}

//! Virtual file system: the trait every panel data source implements
//! (local disk, archives, temporary panel, process list, network…).
//!
//! Panels keep a stack of providers (`LocalFs → archive.zip → nested.tar`).
#![forbid(unsafe_code)]

pub mod local;

use gm_platform::Attrs;
use std::any::Any;
use std::io::{self, Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

pub use local::LocalFs;

/// One item in a panel.
#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub size: u64,
    /// Packed size (archives) or allocation size.
    pub packed: Option<u64>,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub attrs: Attrs,
    pub hidden: bool,
    pub system: bool,
    pub executable: bool,
    pub owner: String,
    pub links: Option<u64>,
    pub link_target: Option<String>,
    pub description: Option<String>,
    /// Values for provider-specific columns (`C0`, `C1`…).
    pub custom: Vec<String>,
    /// Provider-specific identifier (process id, archive index…).
    pub id: u64,
}

impl Entry {
    pub fn dotdot() -> Entry {
        Entry { name: "..".into(), is_dir: true, ..Default::default() }
    }

    pub fn is_dotdot(&self) -> bool {
        self.name == ".."
    }

    /// Extension without the dot, for sorting by extension.
    pub fn ext(&self) -> &str {
        if self.is_dir {
            return "";
        }
        match self.name.rfind('.') {
            Some(0) | None => "",
            Some(i) => &self.name[i + 1..],
        }
    }
}

/// Capability flags of a provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Caps(pub u32);

impl Caps {
    pub const READ: Caps = Caps(1);
    pub const WRITE: Caps = Caps(2);
    pub const DELETE: Caps = Caps(4);
    pub const MKDIR: Caps = Caps(8);
    pub const RENAME: Caps = Caps(16);
    pub const EXEC: Caps = Caps(32);
    pub const ATTRS: Caps = Caps(64);
    pub const ALL: Caps = Caps(127);

    pub fn has(self, c: Caps) -> bool {
        self.0 & c.0 == c.0
    }
}

impl std::ops::BitOr for Caps {
    type Output = Caps;
    fn bitor(self, rhs: Caps) -> Caps {
        Caps(self.0 | rhs.0)
    }
}

/// Cooperative cancellation for long operations.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> CancelToken {
        CancelToken::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Result of `change_dir`.
pub enum ChangeDir {
    /// The provider is now in a new directory.
    Changed,
    /// ".." at the provider root: the panel pops this provider.
    Exit,
    /// Leave the provider and show this local folder (temporary/network panels).
    Local(PathBuf),
}

/// A provider-specific column: title and preferred width (0 = flexible).
#[derive(Clone, Debug)]
pub struct CustomColumn {
    pub title: String,
    pub width: usize,
}

pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// Progress callback for transfers: (current item, bytes done in item) → keep going?
pub type Progress<'a> = &'a mut dyn FnMut(&str, u64) -> bool;

pub trait VfsProvider: Send {
    /// Panel title (shown in the top border).
    fn title(&self) -> String;
    /// Current directory as shown to the user.
    fn current_dir(&self) -> String;
    /// The real directory on disk, for the local provider only.
    fn local_dir(&self) -> Option<PathBuf> {
        None
    }
    /// The file on disk this provider shows the contents of (an archive).
    fn source_file(&self) -> Option<PathBuf> {
        None
    }
    /// Short format id ("zip", "tmp", "proc") for the panel title prefix.
    fn format(&self) -> &str {
        ""
    }
    fn caps(&self) -> Caps;
    fn list(&mut self, cancel: &CancelToken) -> io::Result<Vec<Entry>>;
    /// `to` is a child name, `..`, or an absolute provider path.
    fn change_dir(&mut self, to: &str) -> io::Result<ChangeDir>;
    fn open_read(&mut self, name: &str) -> io::Result<Box<dyn ReadSeek>>;
    /// Copies items out of the provider into a local directory.
    fn get_files(&mut self, names: &[String], dest: &Path, progress: Progress) -> io::Result<()>;
    /// Adds local files into the provider's current directory.
    fn put_files(&mut self, _src: &[PathBuf], _progress: Progress) -> io::Result<()> {
        Err(unsupported())
    }
    fn delete(&mut self, _names: &[String]) -> io::Result<()> {
        Err(unsupported())
    }
    fn mkdir(&mut self, _name: &str) -> io::Result<()> {
        Err(unsupported())
    }
    fn columns(&self) -> Option<Vec<CustomColumn>> {
        None
    }
    /// Extra lines for the info panel (Ctrl+L).
    fn info_lines(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    /// Text shown by F3 on an item when the provider has no file content
    /// (e.g. process details). `None` means use `open_read`.
    fn describe(&mut self, _name: &str) -> Option<String> {
        None
    }
    /// The real file behind an item, when there is one (temporary panel).
    fn real_path(&self, _name: &str) -> Option<PathBuf> {
        None
    }
    fn as_any(&mut self) -> &mut dyn Any;
}

pub fn unsupported() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "operation not supported by this panel")
}

/// Joins a provider path with `/` separators.
pub fn join_vpath(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir == "/" { format!("/{name}").replace("//", "/") } else { format!("{}/{name}", dir.trim_end_matches('/')) }
}

/// Parent of a provider path (`/a/b` → `/a`, `/a` → `/`).
pub fn parent_vpath(dir: &str) -> String {
    let t = dir.trim_end_matches('/');
    match t.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => t[..i].to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vpaths() {
        assert_eq!(join_vpath("/", "a"), "/a");
        assert_eq!(join_vpath("/a", "b"), "/a/b");
        assert_eq!(parent_vpath("/a/b"), "/a");
        assert_eq!(parent_vpath("/a"), "/");
        let e = Entry { name: "x.tar.gz".into(), ..Default::default() };
        assert_eq!(e.ext(), "gz");
        assert_eq!(Entry { name: ".bashrc".into(), ..Default::default() }.ext(), "");
    }
}

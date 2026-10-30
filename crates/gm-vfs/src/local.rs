//! The local file system provider.

use crate::{CancelToken, Caps, ChangeDir, Entry, Progress, ReadSeek, VfsProvider};
use gm_platform::{Attrs, attrs, users};
use std::any::Any;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct LocalFs {
    dir: PathBuf,
    /// Fill the owner column (costly on some systems).
    pub want_owners: bool,
}

impl LocalFs {
    pub fn new(dir: PathBuf) -> LocalFs {
        LocalFs { dir, want_owners: cfg!(unix) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn set_dir(&mut self, dir: PathBuf) {
        self.dir = dir;
    }
}

/// Builds an `Entry` for a local path (does not follow the final symlink for `is_link`).
pub fn entry_for(path: &Path, name: String, want_owner: bool) -> io::Result<Entry> {
    let lmeta = fs::symlink_metadata(path)?;
    let is_link = lmeta.file_type().is_symlink();
    let meta = if is_link { fs::metadata(path).unwrap_or_else(|_| lmeta.clone()) } else { lmeta };
    let attrs = Attrs::from_metadata(&meta);
    let is_dir = meta.is_dir();
    Ok(Entry {
        hidden: attrs::is_hidden(&name, &attrs),
        system: attrs::is_system(&attrs),
        executable: attrs::is_executable(&name, &attrs, is_dir),
        owner: if want_owner { users::owner(&meta) } else { String::new() },
        links: attrs::hard_links(&meta),
        link_target: if is_link { fs::read_link(path).ok().map(|p| p.display().to_string()) } else { None },
        name,
        is_dir,
        is_link,
        size: if is_dir { 0 } else { meta.len() },
        packed: None,
        modified: meta.modified().ok(),
        created: meta.created().ok(),
        accessed: meta.accessed().ok(),
        attrs,
        description: None,
        custom: Vec::new(),
        id: 0,
    })
}

impl VfsProvider for LocalFs {
    fn title(&self) -> String {
        self.dir.display().to_string()
    }

    fn current_dir(&self) -> String {
        self.dir.display().to_string()
    }

    fn local_dir(&self) -> Option<PathBuf> {
        Some(self.dir.clone())
    }

    fn caps(&self) -> Caps {
        Caps::ALL
    }

    fn list(&mut self, cancel: &CancelToken) -> io::Result<Vec<Entry>> {
        let mut out = Vec::new();
        for de in fs::read_dir(&self.dir)? {
            if cancel.is_cancelled() {
                break;
            }
            let Ok(de) = de else { continue };
            let name = de.file_name().to_string_lossy().into_owned();
            match entry_for(&de.path(), name.clone(), self.want_owners) {
                Ok(e) => out.push(e),
                // Broken entries (permission denied on stat) are still listed.
                Err(_) => out.push(Entry { name, ..Default::default() }),
            }
        }
        Ok(out)
    }

    fn change_dir(&mut self, to: &str) -> io::Result<ChangeDir> {
        let target = if to == ".." {
            match self.dir.parent() {
                Some(p) => p.to_path_buf(),
                None => return Ok(ChangeDir::Changed),
            }
        } else {
            let p = Path::new(to);
            if p.is_absolute() { p.to_path_buf() } else { self.dir.join(p) }
        };
        let target = normalize(&target);
        fs::read_dir(&target)?;
        self.dir = target;
        Ok(ChangeDir::Changed)
    }

    fn open_read(&mut self, name: &str) -> io::Result<Box<dyn ReadSeek>> {
        Ok(Box::new(fs::File::open(self.dir.join(name))?))
    }

    fn get_files(&mut self, names: &[String], dest: &Path, progress: Progress) -> io::Result<()> {
        for n in names {
            if !progress(n, 0) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            fs::copy(self.dir.join(n), dest.join(n))?;
        }
        Ok(())
    }

    fn delete(&mut self, names: &[String]) -> io::Result<()> {
        for n in names {
            let p = self.dir.join(n);
            if fs::symlink_metadata(&p)?.is_dir() { fs::remove_dir_all(&p)? } else { fs::remove_file(&p)? }
        }
        Ok(())
    }

    fn mkdir(&mut self, name: &str) -> io::Result<()> {
        fs::create_dir_all(self.dir.join(name))
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Lexically resolves `.` and `..` components without touching the disk
/// (so symlinked directories keep their logical path, as in Far).
pub fn normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() { PathBuf::from(".") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_and_cd() {
        let tmp = std::env::temp_dir().join(format!("gm-vfs-test-{}", std::process::id()));
        fs::create_dir_all(tmp.join("sub")).unwrap();
        fs::write(tmp.join("a.txt"), "hello").unwrap();
        let mut l = LocalFs::new(tmp.clone());
        let items = l.list(&CancelToken::new()).unwrap();
        assert!(items.iter().any(|e| e.name == "a.txt" && e.size == 5 && !e.is_dir));
        assert!(items.iter().any(|e| e.name == "sub" && e.is_dir));
        l.change_dir("sub").unwrap();
        assert_eq!(l.dir(), tmp.join("sub"));
        l.change_dir("..").unwrap();
        assert_eq!(l.dir(), tmp);
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn normalize_paths() {
        assert_eq!(normalize(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
    }
}

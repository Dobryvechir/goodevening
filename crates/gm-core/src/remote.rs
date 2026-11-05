//! Files of remote panels (SSH + SFTP) opened with F3/F4.
//!
//! The remote file is downloaded into a private temporary folder and the
//! local copy is opened in the viewer or the editor. Every time the copy
//! changes (the editor saves it) it is uploaded over the remote file. When
//! no window shows the copy any more, it is deleted with its folder.
//! Failures are reported in a red message box; a copy whose last upload
//! failed is kept (and its location reported) so no change is lost.

use crate::app::App;
use gm_i18n::{MsgId, msg};
use gm_vfs::RemoteFile;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

fn stamp(p: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(p).ok()?;
    m.is_file().then(|| Stamp { len: m.len(), modified: m.modified().ok() })
}

struct Session {
    file: Box<dyn RemoteFile>,
    label: String,
    /// The local copy (inside `dir`).
    local: PathBuf,
    dir: PathBuf,
    stamp: Option<Stamp>,
    /// The last upload failed: the copy holds the only version of the changes.
    upload_failed: bool,
}

/// The local copies of remote files.
#[derive(Default)]
pub struct RemoteFiles {
    list: Vec<Session>,
}

impl RemoteFiles {
    fn base_dir() -> PathBuf {
        std::env::temp_dir().join(format!("gm-remote-{}", std::process::id()))
    }

    fn new_dir() -> std::io::Result<PathBuf> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let base = Self::base_dir();
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        for _ in 0..100 {
            match b.create(&base) {
                Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e),
                _ => {}
            }
            let dir = base.join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
            match b.create(&dir) {
                Ok(()) => return Ok(dir),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::NotFound) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::other(format!("cannot create a folder in {}", base.display())))
    }

    /// The local copies that are open (for tests).
    pub fn local_files(&self) -> Vec<PathBuf> {
        self.list.iter().map(|s| s.local.clone()).collect()
    }
}

impl Drop for RemoteFiles {
    fn drop(&mut self) {
        for s in &self.list {
            if !s.upload_failed {
                let _ = std::fs::remove_dir_all(&s.dir);
            }
        }
        let _ = std::fs::remove_dir(Self::base_dir());
    }
}

fn busy(app: &mut App, title: &str, line: String) {
    let bg = app.ui.term.last_frame().clone();
    gm_ui::message::draw_box(&mut app.ui, &bg, title, &[line]);
}

/// F3/F4 on a remote file: downloads it and opens the local copy.
pub fn open(app: &mut App, mut file: Box<dyn RemoteFile>, edit: bool) {
    let label = file.label();
    let title = if edit { msg(MsgId::MEditTitle).to_string() } else { msg(MsgId::MViewerTitle).to_string() };
    // Already open: show the same copy again (it may hold unsaved work).
    // Generated text (database output) is produced afresh every time.
    if let Some(s) = app.remote.list.iter().find(|s| s.label == label && s.file.write_back()) {
        let p = s.local.clone();
        if edit {
            app.open_editor(&p, None)
        } else {
            app.open_viewer(&p)
        }
        return;
    }
    let dir = match RemoteFiles::new_dir() {
        Ok(d) => d,
        Err(e) => {
            gm_ui::message::error(&mut app.ui, &title, &format!("{label}\n{e}"));
            return;
        }
    };
    let name = file.name();
    let name = if name.is_empty() || name.contains(['/', '\\']) { "file".to_string() } else { name };
    let local = dir.join(&name);
    let verb = if file.write_back() { "Downloading" } else { "Reading" };
    busy(app, &title, format!("{verb} {label}"));
    if let Err(e) = file.download(&local) {
        let _ = std::fs::remove_dir_all(&dir);
        gm_ui::message::error(&mut app.ui, &title, &format!("Cannot download\n{label}\n{e}"));
        return;
    }
    let st = stamp(&local);
    app.remote.list.push(Session { file, label, local: local.clone(), dir, stamp: st, upload_failed: false });
    if edit {
        app.open_editor(&local, None)
    } else {
        app.open_viewer(&local)
    }
    // The window could not be opened: nothing uses the copy.
    release_unused(app);
}

/// Uploads every local copy that changed since it was downloaded or uploaded.
pub fn sync(app: &mut App) {
    if app.remote.list.is_empty() {
        return;
    }
    let mut errors = Vec::new();
    let changed: Vec<usize> = app
        .remote
        .list
        .iter()
        .enumerate()
        .filter(|(_, s)| s.file.write_back() && stamp(&s.local).is_some_and(|now| Some(now) != s.stamp))
        .map(|(i, _)| i)
        .collect();
    for i in changed {
        let label = app.remote.list[i].label.clone();
        busy(app, msg(MsgId::MEditTitle), format!("Uploading {label}"));
        let s = &mut app.remote.list[i];
        // Remembered even on failure: the next save tries again.
        s.stamp = stamp(&s.local);
        match s.file.upload(&s.local) {
            Ok(()) => s.upload_failed = false,
            Err(e) => {
                s.upload_failed = true;
                errors.push(format!("Cannot upload\n{label}\n{e}"));
            }
        }
    }
    for e in errors {
        gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &e);
    }
}

/// Deletes the local copies that no viewer or editor shows any more.
pub fn release_unused(app: &mut App) {
    if app.remote.list.is_empty() {
        return;
    }
    let in_use: Vec<PathBuf> = app.windows.iter().filter_map(|w| w.file()).collect();
    let mut kept = Vec::new();
    app.remote.list.retain(|s| {
        if in_use.iter().any(|p| p == &s.local) {
            return true;
        }
        if s.upload_failed {
            kept.push(format!("{}\n{}", s.label, s.local.display()));
        } else {
            let _ = std::fs::remove_dir_all(&s.dir);
        }
        false
    });
    let _ = std::fs::remove_dir(RemoteFiles::base_dir());
    for k in kept {
        gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &format!("The changes could not be uploaded to\n{k}\nThe local copy is kept."));
    }
}

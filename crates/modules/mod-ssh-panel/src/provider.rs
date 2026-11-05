//! The remote panel: SFTP for listing and file transfers, SSH for commands.

use crate::conn::{Conn, err, rt};
use gm_core::gm_vfs::{CancelToken, Caps, ChangeDir, Entry, Progress, ReadSeek, RemoteFile, VfsProvider, join_vpath, parent_vpath};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use std::any::Any;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const CHUNK: usize = 256 * 1024;

/// Quotes a word for the remote POSIX shell.
pub fn sh_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./+,:@%=".contains(c)) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "Cancelled")
}

fn base_name(p: &str) -> String {
    p.trim_end_matches('/').rsplit('/').next().unwrap_or(p).to_string()
}

fn to_time(t: Option<u32>) -> Option<SystemTime> {
    t.map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s as u64))
}

fn from_time(t: SystemTime) -> Option<u32> {
    t.duration_since(SystemTime::UNIX_EPOCH).ok().map(|d| d.as_secs().min(u32::MAX as u64) as u32)
}

async fn download_file(sftp: &SftpSession, remote: &str, local: &Path, label: &str, progress: Progress<'_>) -> io::Result<()> {
    let mut f = sftp.open(remote).await.map_err(|e| err(format!("{remote}: {e}")))?;
    let mut out = std::fs::File::create(local).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", local.display())))?;
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    if !progress(label, 0) {
        return Err(cancelled());
    }
    loop {
        let n = f.read(&mut buf).await.map_err(|e| err(format!("{remote}: {e}")))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        done += n as u64;
        if !progress(label, done) {
            drop(out);
            let _ = std::fs::remove_file(local);
            return Err(cancelled());
        }
    }
    drop(out);
    if let Ok(md) = f.metadata().await
        && let Some(m) = to_time(md.mtime)
        && let Ok(o) = std::fs::File::options().write(true).open(local)
    {
        let _ = o.set_modified(m);
    }
    let _ = f.close().await;
    Ok(())
}

async fn upload_file(sftp: &SftpSession, local: &Path, remote: &str, label: &str, progress: Progress<'_>) -> io::Result<()> {
    let mut src = std::fs::File::open(local).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", local.display())))?;
    let mut f =
        sftp.open_with_flags(remote, OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE).await.map_err(|e| err(format!("{remote}: {e}")))?;
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    if !progress(label, 0) {
        return Err(cancelled());
    }
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).await.map_err(|e| err(format!("{remote}: {e}")))?;
        done += n as u64;
        if !progress(label, done) {
            let _ = f.close().await;
            return Err(cancelled());
        }
    }
    f.flush().await.map_err(|e| err(format!("{remote}: {e}")))?;
    f.close().await.map_err(|e| err(format!("{remote}: {e}")))?;
    // Keep the modification time, like `scp -p`.
    if let Ok(m) = std::fs::metadata(local) {
        let mut a = FileAttributes::empty();
        a.mtime = m.modified().ok().and_then(from_time);
        a.atime = m.accessed().ok().and_then(from_time).or(a.mtime);
        if a.mtime.is_some() {
            let _ = sftp.set_metadata(remote, a).await;
        }
    }
    Ok(())
}

/// Creates a remote folder unless it exists.
async fn ensure_dir(sftp: &SftpSession, path: &str) -> io::Result<()> {
    match sftp.metadata(path).await {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(err(format!("{path}: not a folder"))),
        Err(_) => sftp.create_dir(path).await.map_err(|e| err(format!("{path}: {e}"))),
    }
}

/// Deletes a remote file or folder tree.
async fn remove_tree(sftp: &SftpSession, path: &str) -> io::Result<()> {
    let md = sftp.symlink_metadata(path).await.map_err(|e| err(format!("{path}: {e}")))?;
    if !md.is_dir() {
        return sftp.remove_file(path).await.map_err(|e| err(format!("{path}: {e}")));
    }
    // Depth-first: collect folders, delete files as they are found.
    let mut dirs = vec![path.to_string()];
    let mut i = 0;
    while i < dirs.len() {
        let d = dirs[i].clone();
        for e in sftp.read_dir(&d).await.map_err(|e| err(format!("{d}: {e}")))? {
            let name = e.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let p = join_vpath(&d, &name);
            if e.metadata().is_dir() {
                dirs.push(p);
            } else {
                sftp.remove_file(&p).await.map_err(|e| err(format!("{p}: {e}")))?;
            }
        }
        i += 1;
    }
    for d in dirs.iter().rev() {
        sftp.remove_dir(d).await.map_err(|e| err(format!("{d}: {e}")))?;
    }
    Ok(())
}

pub struct SshProvider {
    conn: Arc<Conn>,
    dir: String,
}

impl SshProvider {
    pub fn new(conn: Arc<Conn>) -> io::Result<SshProvider> {
        let start = conn.target.dir.clone();
        let mut p = SshProvider { dir: conn.home.clone(), conn };
        if !start.is_empty() {
            p.set_dir(&start)?;
        }
        Ok(p)
    }

    fn check(&self) -> io::Result<()> {
        if self.conn.is_closed() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                format!("The connection to {} is closed.\nOpen it again from the Alt+F1/Alt+F2 menu.", self.conn.target.label()),
            ));
        }
        Ok(())
    }

    /// A provider path for `p`: absolute, `~`-relative or relative to the current folder.
    fn resolve(&self, p: &str) -> String {
        if p.starts_with('/') {
            p.to_string()
        } else if p == "~" {
            self.conn.home.clone()
        } else if let Some(r) = p.strip_prefix("~/") {
            join_vpath(&self.conn.home, r)
        } else {
            join_vpath(&self.dir, p)
        }
    }

    fn set_dir(&mut self, p: &str) -> io::Result<()> {
        self.check()?;
        let target = self.resolve(p);
        let sftp = &self.conn.sftp;
        let real = rt().block_on(async {
            let real = sftp.canonicalize(target.as_str()).await.map_err(|e| err(format!("{target}: {e}")))?;
            let md = sftp.metadata(real.as_str()).await.map_err(|e| err(format!("{real}: {e}")))?;
            if !md.is_dir() {
                return Err(io::Error::new(io::ErrorKind::NotADirectory, format!("{real}: not a folder")));
            }
            Ok(real)
        })?;
        self.dir = real;
        Ok(())
    }
}

impl VfsProvider for SshProvider {
    fn title(&self) -> String {
        self.conn.target.label()
    }
    fn current_dir(&self) -> String {
        self.dir.clone()
    }
    fn format(&self) -> &str {
        "ssh"
    }
    fn caps(&self) -> Caps {
        Caps::READ | Caps::WRITE | Caps::DELETE | Caps::MKDIR | Caps::RENAME | Caps::EXEC
    }

    fn list(&mut self, _cancel: &CancelToken) -> io::Result<Vec<Entry>> {
        self.check()?;
        let sftp = &self.conn.sftp;
        let dir = self.dir.clone();
        rt().block_on(async {
            let rd = sftp.read_dir(dir.as_str()).await.map_err(|e| err(format!("{dir}: {e}")))?;
            let mut v = Vec::new();
            for de in rd {
                let name = de.file_name();
                if name == "." || name == ".." || name.is_empty() {
                    continue;
                }
                let lmd = de.metadata();
                let is_link = lmd.is_symlink();
                // Links show what they point to (a link to a folder is a folder).
                let md = if is_link { sftp.metadata(join_vpath(&dir, &name)).await.unwrap_or(lmd.clone()) } else { lmd.clone() };
                let is_dir = md.is_dir();
                let mode = md.permissions.unwrap_or(0);
                v.push(Entry {
                    is_dir,
                    is_link,
                    size: if is_dir { 0 } else { md.size.unwrap_or(0) },
                    modified: to_time(md.mtime),
                    accessed: to_time(md.atime),
                    attrs: gm_platform::Attrs { win: 0, mode },
                    hidden: name.starts_with('.'),
                    executable: !is_dir && mode & 0o111 != 0,
                    owner: lmd.user.clone().or_else(|| lmd.uid.map(|u| u.to_string())).unwrap_or_default(),
                    name,
                    ..Default::default()
                });
            }
            Ok(v)
        })
    }

    fn change_dir(&mut self, to: &str) -> io::Result<ChangeDir> {
        if to == ".." {
            if self.dir == "/" {
                return Ok(ChangeDir::Exit);
            }
            self.dir = parent_vpath(&self.dir);
            return Ok(ChangeDir::Changed);
        }
        self.set_dir(to)?;
        Ok(ChangeDir::Changed)
    }

    fn open_read(&mut self, name: &str) -> io::Result<Box<dyn ReadSeek>> {
        self.check()?;
        let path = self.resolve(name);
        let sftp = &self.conn.sftp;
        let data = rt().block_on(async {
            let f = sftp.open(path.as_str()).await.map_err(|e| err(format!("{path}: {e}")))?;
            let mut v = Vec::new();
            // Quick view needs the beginning only.
            f.take(16 << 20).read_to_end(&mut v).await?;
            Ok::<_, io::Error>(v)
        })?;
        Ok(Box::new(io::Cursor::new(data)))
    }

    fn get_files(&mut self, names: &[String], dest: &Path, progress: Progress) -> io::Result<()> {
        self.check()?;
        let sftp = &self.conn.sftp;
        let dir = self.dir.clone();
        rt().block_on(async {
            let mut work: Vec<(String, PathBuf)> = names.iter().map(|n| (join_vpath(&dir, n), dest.join(n))).collect();
            while let Some((remote, local)) = work.pop() {
                let md = sftp.metadata(remote.as_str()).await.map_err(|e| err(format!("{remote}: {e}")))?;
                if md.is_dir() {
                    std::fs::create_dir_all(&local).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", local.display())))?;
                    for e in sftp.read_dir(remote.as_str()).await.map_err(|e| err(format!("{remote}: {e}")))? {
                        let n = e.file_name();
                        if n != "." && n != ".." {
                            work.push((join_vpath(&remote, &n), local.join(&n)));
                        }
                    }
                } else {
                    download_file(sftp, &remote, &local, &remote, progress).await?;
                }
            }
            Ok(())
        })
    }

    fn put_files(&mut self, src: &[PathBuf], progress: Progress) -> io::Result<()> {
        self.check()?;
        let sftp = &self.conn.sftp;
        let dir = self.dir.clone();
        rt().block_on(async {
            let mut work: Vec<(PathBuf, String)> = Vec::new();
            for s in src {
                let n = s.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| err(format!("{}: no file name", s.display())))?;
                work.push((s.clone(), join_vpath(&dir, &n)));
            }
            while let Some((local, remote)) = work.pop() {
                let label = local.display().to_string();
                if local.is_dir() {
                    ensure_dir(sftp, &remote).await?;
                    for e in std::fs::read_dir(&local).map_err(|e| io::Error::new(e.kind(), format!("{label}: {e}")))? {
                        let e = e?;
                        let n = e.file_name().to_string_lossy().into_owned();
                        work.push((e.path(), join_vpath(&remote, &n)));
                    }
                } else {
                    upload_file(sftp, &local, &remote, &label, progress).await?;
                }
            }
            Ok(())
        })
    }

    fn delete(&mut self, names: &[String]) -> io::Result<()> {
        self.check()?;
        let sftp = &self.conn.sftp;
        let dir = self.dir.clone();
        rt().block_on(async {
            for n in names {
                remove_tree(sftp, &join_vpath(&dir, n)).await?;
            }
            Ok(())
        })
    }

    fn mkdir(&mut self, name: &str) -> io::Result<()> {
        self.check()?;
        let sftp = &self.conn.sftp;
        let target = self.resolve(name);
        rt().block_on(async {
            // Every missing level, like `mkdir -p`.
            let mut cur = String::new();
            for part in target.split('/').filter(|p| !p.is_empty()) {
                cur = format!("{cur}/{part}");
                ensure_dir(sftp, &cur).await?;
            }
            Ok(())
        })
    }

    fn rename(&mut self, from: &str, to: &str) -> io::Result<()> {
        self.check()?;
        let sftp = &self.conn.sftp;
        let src = self.resolve(from);
        let mut dst = self.resolve(to.trim());
        rt().block_on(async {
            // Into an existing folder: keep the name.
            if to.ends_with('/') || sftp.metadata(dst.as_str()).await.is_ok_and(|m| m.is_dir()) {
                dst = join_vpath(&dst, &base_name(&src));
            }
            sftp.rename(src.as_str(), dst.as_str()).await.map_err(|e| err(format!("{src} → {dst}: {e}")))
        })
    }

    fn remote_file(&mut self, name: &str) -> Option<Box<dyn RemoteFile>> {
        Some(Box::new(SftpFile { conn: self.conn.clone(), path: self.resolve(name) }))
    }

    fn can_execute(&self) -> bool {
        true
    }

    fn execute(&mut self, line: &str) -> io::Result<()> {
        self.check()?;
        crate::shell::run(&self.conn, &self.dir, line)
    }

    fn run_line(&self, name: &str) -> Option<String> {
        Some(format!("./{}", sh_quote(name)))
    }

    fn info_lines(&self) -> Vec<(String, String)> {
        let t = &self.conn.target;
        vec![
            ("Host".into(), if t.host == t.alias { t.host.clone() } else { format!("{} ({})", t.alias, t.host) }),
            ("Port".into(), t.port.to_string()),
            ("User".into(), t.user.clone()),
            ("Home".into(), self.conn.home.clone()),
        ]
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// A remote file edited through a local copy.
struct SftpFile {
    conn: Arc<Conn>,
    path: String,
}

impl SftpFile {
    fn check(&self) -> io::Result<()> {
        if self.conn.is_closed() {
            return Err(io::Error::new(io::ErrorKind::NotConnected, format!("The connection to {} is closed", self.conn.target.label())));
        }
        Ok(())
    }
}

impl RemoteFile for SftpFile {
    fn label(&self) -> String {
        format!("{}:{}", self.conn.target.label(), self.path)
    }
    fn name(&self) -> String {
        base_name(&self.path)
    }
    fn download(&mut self, dest: &Path) -> io::Result<()> {
        self.check()?;
        rt().block_on(download_file(&self.conn.sftp, &self.path, dest, &self.path, &mut |_, _| true))
    }
    fn upload(&mut self, src: &Path) -> io::Result<()> {
        self.check()?;
        rt().block_on(upload_file(&self.conn.sftp, src, &self.path, &self.path, &mut |_, _| true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("run.sh"), "run.sh");
        assert_eq!(sh_quote("my file"), "'my file'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote(""), "''");
        assert_eq!(base_name("/a/b/c.txt"), "c.txt");
    }
}

//! The archive VFS provider.

use gm_core::gm_vfs::{CancelToken, Caps, ChangeDir, Entry, Progress, ReadSeek, VfsProvider, unsupported};
use std::any::Any;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    Gz,
    Bz2,
    Xz,
}

impl Kind {
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Zip => "zip",
            Kind::Tar => "tar",
            Kind::TarGz => "tgz",
            Kind::TarBz2 => "tbz",
            Kind::TarXz => "txz",
            Kind::Gz => "gz",
            Kind::Bz2 => "bz2",
            Kind::Xz => "xz",
        }
    }
}

/// Detects the archive type by signature (and the name for compressed tars).
pub fn detect(path: &Path, head: &[u8]) -> Option<Kind> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let is_tar_name = |exts: &[&str]| exts.iter().any(|e| name.ends_with(e));
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        return Some(Kind::Zip);
    }
    if head.len() > 262 && &head[257..262] == b"ustar" {
        return Some(Kind::Tar);
    }
    if head.starts_with(&[0x1f, 0x8b]) {
        return Some(if is_tar_name(&[".tar.gz", ".tgz"]) { Kind::TarGz } else { Kind::Gz });
    }
    if head.starts_with(b"BZh") {
        return Some(if is_tar_name(&[".tar.bz2", ".tbz", ".tbz2"]) { Kind::TarBz2 } else { Kind::Bz2 });
    }
    if head.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
        return Some(if is_tar_name(&[".tar.xz", ".txz"]) { Kind::TarXz } else { Kind::Xz });
    }
    None
}

#[derive(Clone, Debug)]
struct Item {
    /// Normalized path inside the archive, `/`-separated, no leading slash.
    path: String,
    is_dir: bool,
    size: u64,
    packed: u64,
    mtime: Option<SystemTime>,
    index: usize,
}

pub struct ArchiveProvider {
    file: PathBuf,
    kind: Kind,
    items: Vec<Item>,
    /// Current folder inside the archive ("" = root).
    cur: String,
}

/// Normalizes an entry name and rejects unsafe ones ("..", absolute paths).
fn sanitize(name: &str) -> Option<String> {
    let n = name.replace('\\', "/");
    let mut parts = Vec::new();
    for c in Path::new(&n).components() {
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn zip_time(t: zip::DateTime) -> Option<SystemTime> {
    let dt =
        jiff::civil::DateTime::new(t.year() as i16, t.month() as i8, t.day() as i8, t.hour() as i8, t.minute() as i8, t.second() as i8, 0).ok()?;
    let z = dt.to_zoned(jiff::tz::TimeZone::system()).ok()?;
    Some(SystemTime::from(z.timestamp()))
}

fn tar_reader(path: &Path, kind: Kind) -> io::Result<Box<dyn Read>> {
    let f = io::BufReader::new(File::open(path)?);
    Ok(match kind {
        Kind::Tar => Box::new(f),
        Kind::TarGz | Kind::Gz => Box::new(flate2::read::GzDecoder::new(f)),
        Kind::TarBz2 | Kind::Bz2 => Box::new(bzip2::read::BzDecoder::new(f)),
        Kind::TarXz | Kind::Xz => Box::new(xz2::read::XzDecoder::new(f)),
        Kind::Zip => unreachable!(),
    })
}

pub fn create_empty_zip(path: &Path) -> io::Result<()> {
    let f = File::create(path)?;
    zip::ZipWriter::new(f).finish().map_err(io::Error::other)?;
    Ok(())
}

impl ArchiveProvider {
    pub fn open(path: &Path) -> io::Result<ArchiveProvider> {
        let mut head = vec![0u8; 512];
        let n = File::open(path)?.read(&mut head)?;
        head.truncate(n);
        let kind = detect(path, &head).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown archive format"))?;
        let mut p = ArchiveProvider { file: path.to_path_buf(), kind, items: Vec::new(), cur: String::new() };
        p.index()?;
        Ok(p)
    }

    fn index(&mut self) -> io::Result<()> {
        let mut items = Vec::new();
        match self.kind {
            Kind::Zip => {
                let mut z = zip::ZipArchive::new(File::open(&self.file)?).map_err(io::Error::other)?;
                for i in 0..z.len() {
                    let f = z.by_index_raw(i).map_err(io::Error::other)?;
                    let Some(path) = sanitize(f.name()) else { continue };
                    items.push(Item {
                        path,
                        is_dir: f.is_dir(),
                        size: f.size(),
                        packed: f.compressed_size(),
                        mtime: f.last_modified().and_then(zip_time),
                        index: i,
                    });
                }
            }
            Kind::Gz | Kind::Bz2 | Kind::Xz => {
                // A single compressed file: one member named like the archive without its extension.
                let name = self.file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "data".into());
                let meta = std::fs::metadata(&self.file)?;
                items.push(Item { path: name, is_dir: false, size: 0, packed: meta.len(), mtime: meta.modified().ok(), index: 0 });
            }
            _ => {
                let mut a = tar::Archive::new(tar_reader(&self.file, self.kind)?);
                for (i, e) in a.entries()?.enumerate() {
                    let e = e?;
                    let raw = e.path()?.to_string_lossy().into_owned();
                    let Some(path) = sanitize(&raw) else { continue };
                    let h = e.header();
                    let is_dir = h.entry_type().is_dir();
                    if !(is_dir || h.entry_type().is_file()) {
                        continue;
                    }
                    let mtime = h.mtime().ok().map(|t| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(t));
                    items.push(Item { path, is_dir, size: e.size(), packed: e.size(), mtime, index: i });
                }
            }
        }
        // Add implicit parent folders.
        let mut dirs: BTreeMap<String, ()> = BTreeMap::new();
        for it in &items {
            let mut p = it.path.as_str();
            while let Some((parent, _)) = p.rsplit_once('/') {
                dirs.insert(parent.to_string(), ());
                p = parent;
            }
        }
        for d in dirs.keys() {
            if !items.iter().any(|i| i.path == *d) {
                items.push(Item { path: d.clone(), is_dir: true, size: 0, packed: 0, mtime: None, index: usize::MAX });
            }
        }
        self.items = items;
        Ok(())
    }

    fn full(&self, name: &str) -> String {
        if self.cur.is_empty() { name.to_string() } else { format!("{}/{name}", self.cur) }
    }

    fn parent_of(p: &str) -> &str {
        p.rsplit_once('/').map(|(a, _)| a).unwrap_or("")
    }

    fn read_member(&self, item: &Item) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        match self.kind {
            Kind::Zip => {
                let mut z = zip::ZipArchive::new(File::open(&self.file)?).map_err(io::Error::other)?;
                let mut f = z.by_index(item.index).map_err(io::Error::other)?;
                f.read_to_end(&mut out)?;
            }
            Kind::Gz | Kind::Bz2 | Kind::Xz => {
                tar_reader(&self.file, self.kind)?.read_to_end(&mut out)?;
            }
            _ => {
                let mut a = tar::Archive::new(tar_reader(&self.file, self.kind)?);
                for (i, e) in a.entries()?.enumerate() {
                    let mut e = e?;
                    if i == item.index {
                        e.read_to_end(&mut out)?;
                        break;
                    }
                }
            }
        }
        Ok(out)
    }

    /// Extracts the items whose paths are in `wanted` (or under them) into `dest`,
    /// stripping `strip` from the front of each path.
    fn extract(&self, wanted: &[String], strip: &str, dest: &Path, progress: Progress) -> io::Result<()> {
        let selected = |p: &str| wanted.iter().any(|w| p == w || p.starts_with(&format!("{w}/")));
        let target = |p: &str| -> PathBuf {
            let rel = if strip.is_empty() { p } else { p.strip_prefix(&format!("{strip}/")).unwrap_or(p) };
            let mut t = dest.to_path_buf();
            for part in rel.split('/') {
                t.push(part);
            }
            t
        };
        for it in self.items.iter().filter(|i| i.is_dir && selected(&i.path)) {
            std::fs::create_dir_all(target(&it.path))?;
        }
        let cancelled = || io::Error::new(io::ErrorKind::Interrupted, "cancelled");
        match self.kind {
            Kind::Zip => {
                let mut z = zip::ZipArchive::new(File::open(&self.file)?).map_err(io::Error::other)?;
                for it in self.items.iter().filter(|i| !i.is_dir && i.index != usize::MAX && selected(&i.path)) {
                    if !progress(&it.path, 0) {
                        return Err(cancelled());
                    }
                    let t = target(&it.path);
                    if let Some(p) = t.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    let mut f = z.by_index(it.index).map_err(io::Error::other)?;
                    let mut out = File::create(&t)?;
                    io::copy(&mut f, &mut out)?;
                    if let Some(m) = it.mtime {
                        let _ = out.set_modified(m);
                    }
                }
            }
            Kind::Gz | Kind::Bz2 | Kind::Xz => {
                let it = &self.items[0];
                if selected(&it.path) {
                    let t = target(&it.path);
                    let mut out = File::create(&t)?;
                    io::copy(&mut tar_reader(&self.file, self.kind)?, &mut out)?;
                }
            }
            _ => {
                let mut a = tar::Archive::new(tar_reader(&self.file, self.kind)?);
                let by_index: std::collections::HashMap<usize, &Item> = self.items.iter().filter(|i| !i.is_dir).map(|i| (i.index, i)).collect();
                for (i, e) in a.entries()?.enumerate() {
                    let mut e = e?;
                    let Some(it) = by_index.get(&i) else { continue };
                    if !selected(&it.path) {
                        continue;
                    }
                    if !progress(&it.path, 0) {
                        return Err(cancelled());
                    }
                    let t = target(&it.path);
                    if let Some(p) = t.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    let mut out = File::create(&t)?;
                    io::copy(&mut e, &mut out)?;
                    if let Some(m) = it.mtime {
                        let _ = out.set_modified(m);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn extract_all(&mut self, dest: &Path) -> io::Result<()> {
        let roots: Vec<String> = self.items.iter().filter(|i| !i.path.contains('/')).map(|i| i.path.clone()).collect();
        self.extract(&roots, "", dest, &mut |_, _| true)
    }

    fn writable(&self) -> bool {
        self.kind == Kind::Zip
    }

    /// Rewrites the zip without the given paths (and everything under them).
    fn rewrite_without(&mut self, remove: &[String]) -> io::Result<()> {
        let tmp = self.file.with_extension("gmtmp");
        {
            let mut src = zip::ZipArchive::new(File::open(&self.file)?).map_err(io::Error::other)?;
            let mut w = zip::ZipWriter::new(File::create(&tmp)?);
            for i in 0..src.len() {
                let f = src.by_index_raw(i).map_err(io::Error::other)?;
                let name = sanitize(f.name()).unwrap_or_default();
                if remove.iter().any(|r| name == *r || name.starts_with(&format!("{r}/"))) {
                    continue;
                }
                w.raw_copy_file(f).map_err(io::Error::other)?;
            }
            w.finish().map_err(io::Error::other)?;
        }
        std::fs::rename(&tmp, &self.file)?;
        self.index()
    }
}

impl VfsProvider for ArchiveProvider {
    fn title(&self) -> String {
        format!("{}:{}/{}", self.kind.name(), self.file.display(), self.cur)
    }

    fn current_dir(&self) -> String {
        format!("/{}", self.cur)
    }

    fn format(&self) -> &str {
        self.kind.name()
    }

    fn caps(&self) -> Caps {
        if self.writable() { Caps::READ | Caps::WRITE | Caps::DELETE | Caps::MKDIR } else { Caps::READ }
    }

    fn list(&mut self, _cancel: &CancelToken) -> io::Result<Vec<Entry>> {
        let cur = self.cur.clone();
        Ok(self
            .items
            .iter()
            .filter(|i| Self::parent_of(&i.path) == cur && i.path != cur)
            .map(|i| Entry {
                name: i.path.rsplit('/').next().unwrap_or(&i.path).to_string(),
                is_dir: i.is_dir,
                size: i.size,
                packed: Some(i.packed),
                modified: i.mtime,
                id: i.index as u64,
                ..Default::default()
            })
            .collect())
    }

    fn change_dir(&mut self, to: &str) -> io::Result<ChangeDir> {
        if to == ".." {
            if self.cur.is_empty() {
                return Ok(ChangeDir::Exit);
            }
            self.cur = Self::parent_of(&self.cur).to_string();
            return Ok(ChangeDir::Changed);
        }
        let target = if let Some(abs) = to.strip_prefix('/') { abs.trim_end_matches('/').to_string() } else { self.full(to) };
        if target.is_empty() || self.items.iter().any(|i| i.is_dir && i.path == target) {
            self.cur = target;
            Ok(ChangeDir::Changed)
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, format!("{to}: no such folder in the archive")))
        }
    }

    fn open_read(&mut self, name: &str) -> io::Result<Box<dyn ReadSeek>> {
        let full = self.full(name);
        let item = self
            .items
            .iter()
            .find(|i| i.path == full && !i.is_dir)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, name.to_string()))?;
        Ok(Box::new(Cursor::new(self.read_member(&item)?)))
    }

    fn get_files(&mut self, names: &[String], dest: &Path, progress: Progress) -> io::Result<()> {
        let wanted: Vec<String> = names.iter().map(|n| self.full(n)).collect();
        let strip = self.cur.clone();
        self.extract(&wanted, &strip, dest, progress)
    }

    fn put_files(&mut self, src: &[PathBuf], progress: Progress) -> io::Result<()> {
        if !self.writable() {
            return Err(unsupported());
        }
        let existing: Vec<String> = self.items.iter().map(|i| i.path.clone()).collect();
        // Replace members that are added again.
        let mut replace = Vec::new();
        for s in src {
            if let Some(n) = s.file_name() {
                let p = self.full(&n.to_string_lossy());
                if existing.contains(&p) {
                    replace.push(p);
                }
            }
        }
        if !replace.is_empty() {
            self.rewrite_without(&replace)?;
        }
        let f = std::fs::OpenOptions::new().read(true).write(true).open(&self.file)?;
        let mut w = zip::ZipWriter::new_append(f).map_err(io::Error::other)?;
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let base = self.cur.clone();
        let mut stack: Vec<(PathBuf, String)> = src
            .iter()
            .filter_map(|p| {
                p.file_name()
                    .map(|n| (p.clone(), if base.is_empty() { n.to_string_lossy().into_owned() } else { format!("{base}/{}", n.to_string_lossy()) }))
            })
            .collect();
        while let Some((p, inner)) = stack.pop() {
            if !progress(&inner, 0) {
                break;
            }
            let meta = std::fs::metadata(&p)?;
            if meta.is_dir() {
                w.add_directory(inner.clone(), opts).map_err(io::Error::other)?;
                for e in std::fs::read_dir(&p)?.flatten() {
                    stack.push((e.path(), format!("{inner}/{}", e.file_name().to_string_lossy())));
                }
            } else {
                w.start_file(inner, opts).map_err(io::Error::other)?;
                let mut f = File::open(&p)?;
                io::copy(&mut f, &mut w)?;
            }
        }
        w.finish().map_err(io::Error::other)?;
        self.index()
    }

    fn delete(&mut self, names: &[String]) -> io::Result<()> {
        if !self.writable() {
            return Err(unsupported());
        }
        let full: Vec<String> = names.iter().map(|n| self.full(n)).collect();
        self.rewrite_without(&full)
    }

    fn mkdir(&mut self, name: &str) -> io::Result<()> {
        if !self.writable() {
            return Err(unsupported());
        }
        let Some(clean) = sanitize(name) else { return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid folder name")) };
        let f = std::fs::OpenOptions::new().read(true).write(true).open(&self.file)?;
        let mut w = zip::ZipWriter::new_append(f).map_err(io::Error::other)?;
        w.add_directory(self.full(&clean), zip::write::SimpleFileOptions::default()).map_err(io::Error::other)?;
        w.finish().map_err(io::Error::other)?;
        self.index()
    }

    fn info_lines(&self) -> Vec<(String, String)> {
        let files = self.items.iter().filter(|i| !i.is_dir).count();
        let size: u64 = self.items.iter().map(|i| i.size).sum();
        let packed: u64 = self.items.iter().map(|i| i.packed).sum();
        vec![
            ("Archive".into(), self.file.display().to_string()),
            ("Format".into(), self.kind.name().into()),
            ("Files".into(), files.to_string()),
            ("Size".into(), size.to_string()),
            ("Packed".into(), packed.to_string()),
        ]
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gm-arc-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn zip_roundtrip() {
        let d = tmp("zip");
        std::fs::create_dir_all(d.join("src/sub")).unwrap();
        std::fs::write(d.join("src/a.txt"), "alpha").unwrap();
        std::fs::write(d.join("src/sub/b.txt"), "beta").unwrap();
        let z = d.join("t.zip");
        create_empty_zip(&z).unwrap();
        let mut p = ArchiveProvider::open(&z).unwrap();
        p.put_files(&[d.join("src")], &mut |_, _| true).unwrap();
        let root = p.list(&CancelToken::new()).unwrap();
        assert_eq!(root.len(), 1);
        assert!(root[0].is_dir);
        p.change_dir("src").unwrap();
        let mut names: Vec<String> = p.list(&CancelToken::new()).unwrap().into_iter().map(|e| e.name).collect();
        names.sort();
        assert_eq!(names, ["a.txt", "sub"]);
        let mut s = String::new();
        p.open_read("a.txt").unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "alpha");
        let out = d.join("out");
        std::fs::create_dir(&out).unwrap();
        p.get_files(&["sub".into()], &out, &mut |_, _| true).unwrap();
        assert_eq!(std::fs::read_to_string(out.join("sub/b.txt")).unwrap(), "beta");
        p.delete(&["a.txt".into()]).unwrap();
        assert_eq!(p.list(&CancelToken::new()).unwrap().len(), 1);
        p.mkdir("newdir").unwrap();
        assert_eq!(p.list(&CancelToken::new()).unwrap().len(), 2);
        assert!(matches!(p.change_dir("..").unwrap(), ChangeDir::Changed));
        assert!(matches!(p.change_dir("..").unwrap(), ChangeDir::Exit));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn tar_gz_and_sanitize() {
        let d = tmp("tgz");
        let path = d.join("x.tar.gz");
        {
            let f = File::create(&path).unwrap();
            let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
            let mut b = tar::Builder::new(gz);
            let data = b"hello tar";
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, "dir/file.txt", &data[..]).unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let mut p = ArchiveProvider::open(&path).unwrap();
        assert_eq!(p.kind, Kind::TarGz);
        p.change_dir("dir").unwrap();
        let mut s = String::new();
        p.open_read("file.txt").unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "hello tar");
        assert_eq!(sanitize("../../etc/passwd"), None);
        assert_eq!(sanitize("/abs/x"), None);
        assert_eq!(sanitize("a\\b/./c"), Some("a/b/c".into()));
        std::fs::remove_dir_all(&d).unwrap();
    }
}

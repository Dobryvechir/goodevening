//! File operations: copy, move, delete (trash/permanent/wipe), mkdir,
//! rename, links and folder size (replaces `copy.cpp`, `delete.cpp`,
//! `mkdir.cpp`, `flink.cpp`, `dirinfo.cpp`).
//!
//! Every operation is split into **plan** (walk the tree, compute totals)
//! and **execute**. The UI is reached only through [`OpUi`], so the engine
//! can be tested without a terminal and never panics on I/O errors.
#![forbid(unsafe_code)]

use std::fs::{self, File, Metadata};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct ProgressInfo {
    pub current_src: PathBuf,
    pub current_dst: PathBuf,
    pub item_done: u64,
    pub item_total: u64,
    pub total_done: u64,
    pub total_bytes: u64,
    pub files_done: u64,
    pub files_total: u64,
    pub started: Option<Instant>,
}

impl ProgressInfo {
    /// Bytes per second since the operation started.
    pub fn speed(&self) -> u64 {
        match self.started {
            Some(t) => {
                let s = t.elapsed().as_secs_f64();
                if s > 0.2 { (self.total_done as f64 / s) as u64 } else { 0 }
            }
            None => 0,
        }
    }

    /// Estimated seconds remaining.
    pub fn eta(&self) -> Option<u64> {
        let sp = self.speed();
        (sp > 0).then(|| self.total_bytes.saturating_sub(self.total_done) / sp)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictAction {
    Overwrite,
    Skip,
    Rename(String),
    Append,
    OnlyNewer,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictChoice {
    pub action: ConflictAction,
    /// Apply to all further conflicts.
    pub remember: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorChoice {
    Retry,
    Skip,
    SkipAll,
    Cancel,
}

/// Callbacks into the user interface.
pub trait OpUi {
    /// Returns false to cancel.
    fn progress(&mut self, p: &ProgressInfo) -> bool;
    fn conflict(&mut self, src: &Path, src_meta: &Metadata, dst: &Path, dst_meta: &Metadata) -> ConflictChoice;
    fn error(&mut self, what: &str, path: &Path, err: &io::Error) -> ErrorChoice;
}

/// Answers every question automatically (tests, scripted use).
pub struct AutoUi {
    pub on_conflict: ConflictAction,
    pub on_error: ErrorChoice,
}

impl OpUi for AutoUi {
    fn progress(&mut self, _p: &ProgressInfo) -> bool {
        true
    }
    fn conflict(&mut self, _: &Path, _: &Metadata, _: &Path, _: &Metadata) -> ConflictChoice {
        ConflictChoice { action: self.on_conflict.clone(), remember: true }
    }
    fn error(&mut self, _: &str, _: &Path, _: &io::Error) -> ErrorChoice {
        self.on_error
    }
}

#[derive(Clone, Debug)]
pub struct CopyOptions {
    pub move_files: bool,
    pub preserve_times: bool,
    pub preserve_permissions: bool,
    /// Copy symbolic links as links instead of their targets.
    pub copy_links_as_links: bool,
    /// Default conflict action; `None` asks through `OpUi`.
    pub on_conflict: Option<ConflictAction>,
}

impl Default for CopyOptions {
    fn default() -> Self {
        CopyOptions { move_files: false, preserve_times: true, preserve_permissions: true, copy_links_as_links: cfg!(unix), on_conflict: None }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub skipped: u64,
    pub cancelled: bool,
}

#[derive(Debug)]
struct PlanItem {
    src: PathBuf,
    dst: PathBuf,
    is_dir: bool,
    is_link: bool,
    size: u64,
}

#[derive(Debug, Default)]
pub struct DirSize {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
}

/// Recursively sums a directory (F3 on a folder). `keep_going` is polled.
pub fn dir_size(path: &Path, keep_going: &mut dyn FnMut(&DirSize) -> bool) -> DirSize {
    let mut acc = DirSize::default();
    let mut stack = vec![path.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                acc.dirs += 1;
                stack.push(e.path());
            } else {
                acc.files += 1;
                acc.bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
        if !keep_going(&acc) {
            break;
        }
    }
    acc
}

/// Resolves the destination for each source like Far: copy *into* `dest`
/// when it is an existing directory, ends with a separator or there are
/// several sources; otherwise `dest` is the new name of the single source.
pub fn resolve_targets(sources: &[PathBuf], dest: &Path) -> Vec<(PathBuf, PathBuf)> {
    let ends_sep = dest.as_os_str().to_string_lossy().ends_with(['/', '\\']);
    let into = dest.is_dir() || ends_sep || sources.len() > 1;
    sources
        .iter()
        .map(|s| {
            let target = if into { dest.join(s.file_name().unwrap_or(s.as_os_str())) } else { dest.to_path_buf() };
            (s.clone(), target)
        })
        .collect()
}

fn plan(pairs: &[(PathBuf, PathBuf)], follow_links: bool) -> io::Result<Vec<PlanItem>> {
    let mut items = Vec::new();
    for (src, dst) in pairs {
        if dst.starts_with(src) && dst != src {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("cannot copy {} into itself", src.display())));
        }
        walk(src, dst, follow_links, &mut items)?;
    }
    Ok(items)
}

fn walk(src: &Path, dst: &Path, follow_links: bool, out: &mut Vec<PlanItem>) -> io::Result<()> {
    let lmeta = fs::symlink_metadata(src)?;
    let is_link = lmeta.file_type().is_symlink();
    let meta = if is_link && follow_links { fs::metadata(src)? } else { lmeta };
    if is_link && !follow_links {
        out.push(PlanItem { src: src.into(), dst: dst.into(), is_dir: false, is_link: true, size: 0 });
        return Ok(());
    }
    if meta.is_dir() {
        out.push(PlanItem { src: src.into(), dst: dst.into(), is_dir: true, is_link: false, size: 0 });
        let mut children: Vec<_> = fs::read_dir(src)?.flatten().map(|e| e.file_name()).collect();
        children.sort();
        for name in children {
            walk(&src.join(&name), &dst.join(&name), follow_links, out)?;
        }
    } else {
        out.push(PlanItem { src: src.into(), dst: dst.into(), is_dir: false, is_link: false, size: meta.len() });
    }
    Ok(())
}

/// Copies or moves `sources` to `dest`.
pub fn copy(sources: &[PathBuf], dest: &Path, opts: &CopyOptions, ui: &mut dyn OpUi) -> io::Result<Summary> {
    let pairs = resolve_targets(sources, dest);
    let mut summary = Summary::default();

    // Move: a rename is enough when source and target share a volume.
    let mut remaining = Vec::new();
    if opts.move_files {
        for (s, d) in pairs {
            if same_file(&s, &d) {
                continue;
            }
            if !d.exists() && fs::rename(&s, &d).is_ok() {
                summary.files += 1;
                continue;
            }
            remaining.push((s, d));
        }
    } else {
        remaining = pairs;
    }
    if remaining.is_empty() {
        return Ok(summary);
    }

    let items = plan(&remaining, !opts.copy_links_as_links)?;
    let mut p = ProgressInfo {
        total_bytes: items.iter().map(|i| i.size).sum(),
        files_total: items.iter().filter(|i| !i.is_dir).count() as u64,
        started: Some(Instant::now()),
        ..Default::default()
    };
    let mut remembered = opts.on_conflict.clone();
    let mut skip_all_errors = false;
    let mut copied_ok: Vec<&PlanItem> = Vec::new();

    'items: for item in &items {
        p.current_src = item.src.clone();
        p.current_dst = item.dst.clone();
        p.item_done = 0;
        p.item_total = item.size;
        if !ui.progress(&p) {
            summary.cancelled = true;
            break;
        }
        if item.is_dir {
            loop {
                match fs::create_dir_all(&item.dst) {
                    Ok(()) => {
                        summary.dirs += 1;
                        copied_ok.push(item);
                        continue 'items;
                    }
                    Err(e) => match handle_error(ui, "create folder", &item.dst, &e, &mut skip_all_errors) {
                        ErrorChoice::Retry => continue,
                        ErrorChoice::Cancel => {
                            summary.cancelled = true;
                            break 'items;
                        }
                        _ => {
                            summary.skipped += 1;
                            continue 'items;
                        }
                    },
                }
            }
        }
        if same_file(&item.src, &item.dst) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "cannot copy a file onto itself"));
        }
        // Conflict resolution.
        let mut target = item.dst.clone();
        let mut append = false;
        if let Ok(dmeta) = fs::symlink_metadata(&target) {
            let smeta = fs::symlink_metadata(&item.src)?;
            let choice = match &remembered {
                Some(a) => ConflictChoice { action: a.clone(), remember: true },
                None => ui.conflict(&item.src, &smeta, &target, &dmeta),
            };
            if choice.remember {
                remembered = Some(match &choice.action {
                    ConflictAction::Rename(_) => ConflictAction::Rename(String::new()),
                    a => a.clone(),
                });
            }
            match choice.action {
                ConflictAction::Overwrite => {}
                ConflictAction::Skip => {
                    summary.skipped += 1;
                    p.total_done += item.size;
                    continue;
                }
                ConflictAction::OnlyNewer => {
                    let newer = match (smeta.modified(), dmeta.modified()) {
                        (Ok(s), Ok(d)) => s > d,
                        _ => true,
                    };
                    if !newer {
                        summary.skipped += 1;
                        p.total_done += item.size;
                        continue;
                    }
                }
                ConflictAction::Append => append = true,
                ConflictAction::Rename(name) => {
                    target = if name.is_empty() { unique_name(&target) } else { target.with_file_name(name) };
                }
                ConflictAction::Cancel => {
                    summary.cancelled = true;
                    break;
                }
            }
        }
        loop {
            let r = if item.is_link { copy_link(&item.src, &target) } else { copy_file(item, &target, append, opts, ui, &mut p) };
            match r {
                Ok(true) => {
                    summary.files += 1;
                    summary.bytes += item.size;
                    p.files_done += 1;
                    copied_ok.push(item);
                    break;
                }
                Ok(false) => {
                    let _ = fs::remove_file(&target);
                    summary.cancelled = true;
                    break 'items;
                }
                Err(e) => match handle_error(ui, "copy", &item.src, &e, &mut skip_all_errors) {
                    ErrorChoice::Retry => continue,
                    ErrorChoice::Cancel => {
                        summary.cancelled = true;
                        break 'items;
                    }
                    _ => {
                        summary.skipped += 1;
                        break;
                    }
                },
            }
        }
    }

    // Move = copy + delete of the sources that were copied completely.
    if opts.move_files && !summary.cancelled {
        for item in copied_ok.iter().rev() {
            if item.is_dir {
                let _ = fs::remove_dir(&item.src);
            } else {
                let _ = fs::remove_file(&item.src);
            }
        }
    }
    Ok(summary)
}

fn handle_error(ui: &mut dyn OpUi, what: &str, path: &Path, e: &io::Error, skip_all: &mut bool) -> ErrorChoice {
    if *skip_all {
        return ErrorChoice::Skip;
    }
    let c = ui.error(what, path, e);
    if c == ErrorChoice::SkipAll {
        *skip_all = true;
    }
    c
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// `name.ext` → `name(1).ext`, `name(2).ext`… (first that does not exist).
pub fn unique_name(p: &Path) -> PathBuf {
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for i in 1.. {
        let c = p.with_file_name(format!("{stem}({i}){ext}"));
        if !c.exists() {
            return c;
        }
    }
    unreachable!()
}

fn copy_link(src: &Path, dst: &Path) -> io::Result<bool> {
    let target = fs::read_link(src)?;
    if fs::symlink_metadata(dst).is_ok() {
        fs::remove_file(dst)?;
    }
    let dir = fs::metadata(src).map(|m| m.is_dir()).unwrap_or(false);
    gm_platform::attrs::symlink(&target, dst, dir)?;
    Ok(true)
}

/// Buffered copy with progress. Returns Ok(false) if cancelled.
fn copy_file(item: &PlanItem, dst: &Path, append: bool, opts: &CopyOptions, ui: &mut dyn OpUi, p: &mut ProgressInfo) -> io::Result<bool> {
    let smeta = fs::metadata(&item.src)?;
    if let Ok(dm) = fs::metadata(dst)
        && dm.permissions().readonly()
        && !append
    {
        gm_platform::attrs::set_readonly(dst, false)?;
    }
    let mut input = File::open(&item.src)?;
    let mut output = fs::OpenOptions::new().write(true).create(true).append(append).truncate(!append).open(dst)?;
    let mut buf = vec![0u8; 1 << 20];
    let base = p.total_done;
    let mut done = 0u64;
    let mut last = Instant::now();
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        output.write_all(&buf[..n])?;
        done += n as u64;
        p.item_done = done;
        p.total_done = base + done;
        if last.elapsed().as_millis() >= 100 {
            last = Instant::now();
            if !ui.progress(p) {
                return Ok(false);
            }
        }
    }
    output.flush()?;
    if opts.preserve_times {
        let mut times = fs::FileTimes::new();
        if let Ok(m) = smeta.modified() {
            times = times.set_modified(m);
        }
        if let Ok(a) = smeta.accessed() {
            times = times.set_accessed(a);
        }
        let _ = output.set_times(times);
    }
    drop(output);
    if opts.preserve_permissions {
        let _ = fs::set_permissions(dst, smeta.permissions());
    }
    p.total_done = base + item.size.max(done);
    Ok(true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteMode {
    Trash,
    Permanent,
    /// Overwrite file contents with zeros before deleting (Alt+Del).
    Wipe,
}

/// Deletes paths. Errors are reported through `ui`.
pub fn delete(paths: &[PathBuf], mode: DeleteMode, ui: &mut dyn OpUi) -> io::Result<Summary> {
    let mut summary = Summary::default();
    if mode == DeleteMode::Trash {
        match trash::delete_all(paths) {
            Ok(()) => {
                summary.files = paths.len() as u64;
                return Ok(summary);
            }
            Err(e) => {
                let err = io::Error::other(e.to_string());
                match ui.error("move to trash", paths.first().map(|p| p.as_path()).unwrap_or(Path::new("")), &err) {
                    ErrorChoice::Retry | ErrorChoice::Skip | ErrorChoice::SkipAll => {} // fall through to permanent
                    ErrorChoice::Cancel => {
                        summary.cancelled = true;
                        return Ok(summary);
                    }
                }
            }
        }
    }
    let mut items = Vec::new();
    for p in paths {
        walk(p, p, false, &mut items)?;
    }
    let mut pi = ProgressInfo { files_total: items.iter().filter(|i| !i.is_dir).count() as u64, started: Some(Instant::now()), ..Default::default() };
    let mut skip_all = false;
    // Children first: reverse plan order.
    for item in items.iter().rev() {
        pi.current_src = item.src.clone();
        if !ui.progress(&pi) {
            summary.cancelled = true;
            break;
        }
        loop {
            let r = if item.is_dir {
                fs::remove_dir(&item.src)
            } else {
                if mode == DeleteMode::Wipe && !item.is_link {
                    let _ = wipe(&item.src);
                }
                remove_file_force(&item.src)
            };
            match r {
                Ok(()) => {
                    if item.is_dir {
                        summary.dirs += 1;
                    } else {
                        summary.files += 1;
                        pi.files_done += 1;
                    }
                    break;
                }
                Err(e) => match handle_error(ui, "delete", &item.src, &e, &mut skip_all) {
                    ErrorChoice::Retry => continue,
                    ErrorChoice::Cancel => {
                        summary.cancelled = true;
                        return Ok(summary);
                    }
                    _ => {
                        summary.skipped += 1;
                        break;
                    }
                },
            }
        }
    }
    Ok(summary)
}

fn remove_file_force(p: &Path) -> io::Result<()> {
    match fs::remove_file(p) {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied && cfg!(windows) => {
            gm_platform::attrs::set_readonly(p, false)?;
            fs::remove_file(p)
        }
        r => r,
    }
}

fn wipe(p: &Path) -> io::Result<()> {
    let len = fs::metadata(p)?.len();
    let mut f = fs::OpenOptions::new().write(true).open(p)?;
    let zeros = vec![0u8; 1 << 16];
    let mut left = len;
    while left > 0 {
        let n = left.min(zeros.len() as u64) as usize;
        f.write_all(&zeros[..n])?;
        left -= n as u64;
    }
    f.sync_all()?;
    drop(f);
    // Hide the original name, too.
    let renamed = p.with_file_name(format!("~gm-wipe-{}", std::process::id()));
    fs::rename(p, &renamed)?;
    fs::OpenOptions::new().write(true).truncate(true).open(&renamed)?;
    fs::rename(&renamed, p)
}

/// Splits Far's "a;b;\"c;d\"" multi-name syntax for F7.
pub fn split_names(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    for c in spec.chars() {
        match c {
            '"' => q = !q,
            ';' | ',' if !q => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Creates one or more (nested) folders; returns the first created path.
pub fn make_dirs(base: &Path, spec: &str, multiple: bool) -> io::Result<PathBuf> {
    let names = if multiple { split_names(spec) } else { vec![spec.trim().to_string()] };
    let mut first = None;
    for n in names {
        let p = base.join(&n);
        fs::create_dir_all(&p)?;
        first.get_or_insert(p);
    }
    first.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty folder name"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Hard,
    Symbolic,
}

pub fn make_link(target: &Path, link: &Path, kind: LinkKind) -> io::Result<()> {
    match kind {
        LinkKind::Hard => fs::hard_link(target, link),
        LinkKind::Symbolic => gm_platform::attrs::symlink(target, link, target.is_dir()),
    }
}

/// Renames in place (Shift+F6 / F6 on a single item with a bare name).
pub fn rename(src: &Path, new_name: &str) -> io::Result<PathBuf> {
    let dst = match src.parent() {
        Some(p) => p.join(new_name),
        None => PathBuf::from(new_name),
    };
    let case_only = dst.to_string_lossy().to_lowercase() == src.to_string_lossy().to_lowercase();
    if dst.exists() && !case_only {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} already exists", dst.display())));
    }
    fs::rename(src, &dst)?;
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("gm-fs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn ui() -> AutoUi {
        AutoUi { on_conflict: ConflictAction::Overwrite, on_error: ErrorChoice::Cancel }
    }

    #[test]
    fn copy_tree_and_move() {
        let t = tmp("copy");
        fs::create_dir_all(t.join("src/sub")).unwrap();
        fs::write(t.join("src/a.txt"), "aaa").unwrap();
        fs::write(t.join("src/sub/b.txt"), "bb").unwrap();
        fs::create_dir(t.join("dst")).unwrap();
        let s = copy(&[t.join("src")], &t.join("dst"), &CopyOptions::default(), &mut ui()).unwrap();
        assert_eq!((s.files, s.dirs, s.bytes), (2, 2, 5));
        assert_eq!(fs::read_to_string(t.join("dst/src/sub/b.txt")).unwrap(), "bb");

        // Copy into itself is refused.
        assert!(copy(&[t.join("src")], &t.join("src/sub"), &CopyOptions::default(), &mut ui()).is_err());

        // Rename-copy of a single file.
        copy(&[t.join("src/a.txt")], &t.join("dst/renamed.txt"), &CopyOptions::default(), &mut ui()).unwrap();
        assert!(t.join("dst/renamed.txt").is_file());

        let mv = CopyOptions { move_files: true, ..Default::default() };
        copy(&[t.join("src")], &t.join("moved"), &mv, &mut ui()).unwrap();
        assert!(!t.join("src").exists() && t.join("moved/sub/b.txt").exists());
        fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn conflicts() {
        let t = tmp("conflict");
        fs::write(t.join("a"), "new").unwrap();
        fs::create_dir(t.join("d")).unwrap();
        fs::write(t.join("d/a"), "old").unwrap();
        let mut skip = AutoUi { on_conflict: ConflictAction::Skip, on_error: ErrorChoice::Cancel };
        let s = copy(&[t.join("a")], &t.join("d"), &CopyOptions::default(), &mut skip).unwrap();
        assert_eq!(s.skipped, 1);
        assert_eq!(fs::read_to_string(t.join("d/a")).unwrap(), "old");
        let mut app = AutoUi { on_conflict: ConflictAction::Append, on_error: ErrorChoice::Cancel };
        copy(&[t.join("a")], &t.join("d"), &CopyOptions::default(), &mut app).unwrap();
        assert_eq!(fs::read_to_string(t.join("d/a")).unwrap(), "oldnew");
        let mut ren = AutoUi { on_conflict: ConflictAction::Rename(String::new()), on_error: ErrorChoice::Cancel };
        copy(&[t.join("a")], &t.join("d"), &CopyOptions::default(), &mut ren).unwrap();
        assert!(t.join("d/a(1)").exists());
        fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn delete_mkdir_rename() {
        let t = tmp("del");
        let first = make_dirs(&t, "x;y/z;\"p;q\"", true).unwrap();
        assert_eq!(first, t.join("x"));
        assert!(t.join("y/z").is_dir() && t.join("p;q").is_dir());
        fs::write(t.join("y/z/f"), "data").unwrap();
        let s = delete(&[t.join("y")], DeleteMode::Wipe, &mut ui()).unwrap();
        assert_eq!((s.files, s.dirs), (1, 2));
        assert!(!t.join("y").exists());
        let r = rename(&t.join("x"), "x2").unwrap();
        assert!(r.is_dir());
        let ds = dir_size(&t, &mut |_| true);
        assert_eq!(ds.dirs, 2);
        fs::remove_dir_all(&t).unwrap();
    }
}

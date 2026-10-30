//! Temporary panel (replaces the `tmppanel` plugin): a panel holding an
//! arbitrary list of files and folders from anywhere (Find File → Panel,
//! `tmp:` prefix, F5 from another panel adds items).
#![forbid(unsafe_code)]

use gm_core::gm_vfs::local::entry_for;
use gm_core::gm_vfs::{CancelToken, Caps, ChangeDir, Entry, Progress, ReadSeek, VfsProvider};
use gm_core::{CommandSpec, Ctx, Module};
use std::any::Any;
use std::io;
use std::path::{Path, PathBuf};

pub struct TempPanelProvider {
    items: Vec<PathBuf>,
}

impl TempPanelProvider {
    pub fn new(items: Vec<PathBuf>) -> TempPanelProvider {
        let mut t = TempPanelProvider { items: Vec::new() };
        for i in items {
            t.add(i);
        }
        t
    }

    fn add(&mut self, p: PathBuf) {
        if !self.items.contains(&p) {
            self.items.push(p);
        }
    }

    fn find(&self, name: &str) -> Option<&PathBuf> {
        self.items.iter().find(|p| p.display().to_string() == name)
    }
}

impl VfsProvider for TempPanelProvider {
    fn title(&self) -> String {
        "Temporary panel".into()
    }

    fn current_dir(&self) -> String {
        "/".into()
    }

    fn format(&self) -> &str {
        "tmp"
    }

    fn caps(&self) -> Caps {
        Caps::READ | Caps::WRITE | Caps::DELETE
    }

    fn list(&mut self, _cancel: &CancelToken) -> io::Result<Vec<Entry>> {
        Ok(self
            .items
            .iter()
            .map(|p| entry_for(p, p.display().to_string(), false).unwrap_or_else(|_| Entry { name: p.display().to_string(), ..Default::default() }))
            .collect())
    }

    fn change_dir(&mut self, to: &str) -> io::Result<ChangeDir> {
        if to == ".." {
            return Ok(ChangeDir::Exit);
        }
        match self.find(to) {
            Some(p) if p.is_dir() => Ok(ChangeDir::Local(p.clone())),
            _ => Err(io::Error::new(io::ErrorKind::NotFound, to.to_string())),
        }
    }

    fn open_read(&mut self, name: &str) -> io::Result<Box<dyn ReadSeek>> {
        let p = self.find(name).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, name.to_string()))?;
        Ok(Box::new(std::fs::File::open(p)?))
    }

    fn get_files(&mut self, names: &[String], dest: &Path, progress: Progress) -> io::Result<()> {
        for n in names {
            if !progress(n, 0) {
                break;
            }
            if let Some(p) = self.find(n).cloned() {
                if p.is_dir() {
                    copy_dir(&p, &dest.join(p.file_name().unwrap_or_default()))?;
                } else {
                    std::fs::copy(&p, dest.join(p.file_name().unwrap_or_default()))?;
                }
            }
        }
        Ok(())
    }

    /// F5 into the temporary panel adds references, not copies.
    fn put_files(&mut self, src: &[PathBuf], _progress: Progress) -> io::Result<()> {
        for s in src {
            self.add(s.clone());
        }
        Ok(())
    }

    /// F8 removes items from the panel; the files stay on disk.
    fn delete(&mut self, names: &[String]) -> io::Result<()> {
        self.items.retain(|p| !names.contains(&p.display().to_string()));
        Ok(())
    }

    fn real_path(&self, name: &str) -> Option<PathBuf> {
        self.find(name).cloned()
    }

    fn info_lines(&self) -> Vec<(String, String)> {
        vec![("Items".into(), self.items.len().to_string())]
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

fn copy_dir(src: &Path, dst: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)?.flatten() {
        let p = e.path();
        let t = dst.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &t)?;
        } else {
            std::fs::copy(&p, &t)?;
        }
    }
    Ok(())
}

#[derive(Default)]
pub struct TempPanel;

impl Module for TempPanel {
    fn id(&self) -> &'static str {
        "temp_panel"
    }

    fn title(&self) -> String {
        "Temporary panel".into()
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![CommandSpec { id: "temp_panel", title: "&Temporary panel".into() }]
    }

    fn run_command(&self, _id: &str, ctx: &mut Ctx) {
        ctx.active.push_provider(Box::new(TempPanelProvider::new(Vec::new())), String::new());
    }

    fn prefixes(&self) -> &[&'static str] {
        &["tmp"]
    }

    /// `tmp:` opens an empty panel; `tmp:list.txt` loads paths from a list file.
    fn open_prefix(&self, ctx: &mut Ctx, _prefix: &str, arg: &str) -> bool {
        let mut items = Vec::new();
        if !arg.is_empty() {
            let p = ctx.active.base_dir().join(arg.trim_matches('"'));
            if let Ok(text) = std::fs::read_to_string(&p) {
                items = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect();
            }
        }
        ctx.active.push_provider(Box::new(TempPanelProvider::new(items)), String::new());
        true
    }

    fn list_panel(&self, items: Vec<PathBuf>) -> Option<Box<dyn VfsProvider>> {
        Some(Box::new(TempPanelProvider::new(items)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn add_remove() {
        let cur = std::env::current_dir().unwrap();
        let mut t = TempPanelProvider::new(vec![cur.clone(), cur.clone()]);
        assert_eq!(t.list(&CancelToken::new()).unwrap().len(), 1);
        assert!(matches!(t.change_dir(&cur.display().to_string()).unwrap(), ChangeDir::Local(_)));
        t.delete(&[cur.display().to_string()]).unwrap();
        assert!(t.list(&CancelToken::new()).unwrap().is_empty());
    }
}

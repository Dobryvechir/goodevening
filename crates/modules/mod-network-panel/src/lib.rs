//! Network panel (replaces the `network` plugin): connected network drives
//! on Windows (WNet) and network mounts (NFS/SMB/SSHFS) on Linux and macOS.
//! Enter opens the share in the panel. Also accepts `net:\\server\share`.
#![forbid(unsafe_code)]

use gm_core::gm_vfs::{CancelToken, Caps, ChangeDir, Entry, Progress, ReadSeek, VfsProvider, unsupported};
use gm_core::{Action, CommandSpec, Ctx, DriveItem, Module};
use std::any::Any;
use std::io;
use std::path::{Path, PathBuf};

pub struct NetworkProvider {
    items: Vec<(String, String)>,
}

impl NetworkProvider {
    pub fn new() -> NetworkProvider {
        NetworkProvider { items: gm_platform::drives::network_resources() }
    }

    fn path_of(&self, name: &str) -> Option<PathBuf> {
        let (local, remote) = self.items.iter().find(|(l, r)| Self::label(l, r) == name)?;
        Some(PathBuf::from(if !local.is_empty() {
            if cfg!(windows) && local.len() == 2 { format!("{local}\\") } else { local.clone() }
        } else {
            remote.clone()
        }))
    }

    fn label(local: &str, remote: &str) -> String {
        if local.is_empty() { remote.to_string() } else { format!("{local}  {remote}") }
    }
}

impl Default for NetworkProvider {
    fn default() -> Self {
        NetworkProvider::new()
    }
}

impl VfsProvider for NetworkProvider {
    fn title(&self) -> String {
        "Network".into()
    }
    fn current_dir(&self) -> String {
        "/".into()
    }
    fn format(&self) -> &str {
        "net"
    }
    fn caps(&self) -> Caps {
        Caps::READ
    }
    fn list(&mut self, _cancel: &CancelToken) -> io::Result<Vec<Entry>> {
        self.items = gm_platform::drives::network_resources();
        Ok(self.items.iter().map(|(l, r)| Entry { name: Self::label(l, r), is_dir: true, ..Default::default() }).collect())
    }
    fn change_dir(&mut self, to: &str) -> io::Result<ChangeDir> {
        if to == ".." {
            return Ok(ChangeDir::Exit);
        }
        self.path_of(to).map(ChangeDir::Local).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, to.to_string()))
    }
    fn open_read(&mut self, _name: &str) -> io::Result<Box<dyn ReadSeek>> {
        Err(unsupported())
    }
    fn get_files(&mut self, _names: &[String], _dest: &Path, _progress: Progress) -> io::Result<()> {
        Err(unsupported())
    }
    fn real_path(&self, name: &str) -> Option<PathBuf> {
        self.path_of(name)
    }
    fn info_lines(&self) -> Vec<(String, String)> {
        vec![("Network resources".into(), self.items.len().to_string())]
    }
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Default)]
pub struct NetworkPanel;

impl Module for NetworkPanel {
    fn id(&self) -> &'static str {
        "network_panel"
    }
    fn title(&self) -> String {
        "Network".into()
    }
    fn commands(&self) -> Vec<CommandSpec> {
        vec![CommandSpec { id: "network", title: "&Network".into() }]
    }
    fn run_command(&self, _id: &str, ctx: &mut Ctx) {
        ctx.active.push_provider(Box::new(NetworkProvider::new()), String::new());
    }
    fn prefixes(&self) -> &[&'static str] {
        &["net"]
    }
    fn open_prefix(&self, ctx: &mut Ctx, _prefix: &str, arg: &str) -> bool {
        if arg.trim().is_empty() {
            ctx.active.push_provider(Box::new(NetworkProvider::new()), String::new());
        } else {
            ctx.push(Action::Cd(PathBuf::from(arg.trim())));
        }
        true
    }
    fn drive_menu_items(&self) -> Vec<DriveItem> {
        vec![DriveItem { id: 0, title: "Network".into() }]
    }
    fn open_drive_item(&self, _id: usize, ctx: &mut Ctx) {
        ctx.active.push_provider(Box::new(NetworkProvider::new()), String::new());
    }
}

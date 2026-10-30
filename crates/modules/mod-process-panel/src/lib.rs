//! Process list (replaces the `proclist` plugin): `proc:` or
//! F9 → Commands → Task list. F8 terminates processes, F3 shows details.
#![forbid(unsafe_code)]

use gm_core::gm_vfs::{CancelToken, Caps, ChangeDir, CustomColumn, Entry, Progress, ReadSeek, VfsProvider, unsupported};
use gm_core::{CommandSpec, Ctx, Module};
use std::any::Any;
use std::io;
use std::path::Path;
use sysinfo::{Pid, ProcessesToUpdate, System};

pub struct ProcessProvider {
    sys: System,
}

impl ProcessProvider {
    pub fn new() -> ProcessProvider {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        ProcessProvider { sys }
    }

    fn pid_of(name: &str) -> Option<Pid> {
        let open = name.rfind('[')?;
        name[open + 1..].trim_end_matches(']').trim().parse::<usize>().ok().map(Pid::from)
    }
}

impl Default for ProcessProvider {
    fn default() -> Self {
        ProcessProvider::new()
    }
}

impl VfsProvider for ProcessProvider {
    fn title(&self) -> String {
        "Process list".into()
    }

    fn current_dir(&self) -> String {
        "/".into()
    }

    fn format(&self) -> &str {
        "proc"
    }

    fn caps(&self) -> Caps {
        Caps::READ | Caps::DELETE
    }

    fn list(&mut self, _cancel: &CancelToken) -> io::Result<Vec<Entry>> {
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        let mut v: Vec<Entry> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, p)| Entry {
                name: format!("{} [{}]", p.name().to_string_lossy(), pid),
                size: p.memory(),
                custom: vec![pid.to_string(), format!("{:.1}", p.cpu_usage()), format!("{}", p.memory() / 1024)],
                id: pid.as_u32() as u64,
                ..Default::default()
            })
            .collect();
        v.sort_by_key(|e| e.id);
        Ok(v)
    }

    fn change_dir(&mut self, to: &str) -> io::Result<ChangeDir> {
        if to == ".." { Ok(ChangeDir::Exit) } else { Err(unsupported()) }
    }

    fn open_read(&mut self, name: &str) -> io::Result<Box<dyn ReadSeek>> {
        let text = self.describe(name).unwrap_or_default();
        Ok(Box::new(io::Cursor::new(text.into_bytes())))
    }

    fn get_files(&mut self, names: &[String], dest: &Path, _progress: Progress) -> io::Result<()> {
        for n in names {
            if let Some(t) = self.describe(n) {
                let file = n.replace(['[', ']', ' ', '/', '\\', ':'], "_");
                std::fs::write(dest.join(format!("{file}.txt")), t)?;
            }
        }
        Ok(())
    }

    /// F8: terminate.
    fn delete(&mut self, names: &[String]) -> io::Result<()> {
        let mut failed = Vec::new();
        for n in names {
            let ok = Self::pid_of(n).and_then(|pid| self.sys.process(pid)).map(|p| p.kill()).unwrap_or(false);
            if !ok {
                failed.push(n.clone());
            }
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("Cannot kill the specified task: {}", failed.join(", "))))
        }
    }

    fn columns(&self) -> Option<Vec<CustomColumn>> {
        Some(vec![
            CustomColumn { title: "PID".into(), width: 7 },
            CustomColumn { title: "CPU%".into(), width: 6 },
            CustomColumn { title: "Mem K".into(), width: 9 },
        ])
    }

    fn describe(&mut self, name: &str) -> Option<String> {
        let pid = Self::pid_of(name)?;
        let p = self.sys.process(pid)?;
        let cmd: Vec<String> = p.cmd().iter().map(|s| s.to_string_lossy().into_owned()).collect();
        Some(format!(
            "Name:        {}\nPID:         {}\nParent PID:  {}\nStatus:      {}\nExecutable:  {}\nCommand:     {}\nWork folder: {}\nMemory:      {} bytes\nVirtual:     {} bytes\nCPU:         {:.1}%\nStarted:     {} s after the epoch\nRun time:    {} s\n",
            p.name().to_string_lossy(),
            pid,
            p.parent().map(|x| x.to_string()).unwrap_or_default(),
            p.status(),
            p.exe().map(|e| e.display().to_string()).unwrap_or_default(),
            cmd.join(" "),
            p.cwd().map(|e| e.display().to_string()).unwrap_or_default(),
            p.memory(),
            p.virtual_memory(),
            p.cpu_usage(),
            p.start_time(),
            p.run_time()
        ))
    }

    fn info_lines(&self) -> Vec<(String, String)> {
        vec![("Processes".into(), self.sys.processes().len().to_string())]
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Default)]
pub struct ProcessPanel;

impl Module for ProcessPanel {
    fn id(&self) -> &'static str {
        "process_panel"
    }

    fn title(&self) -> String {
        "Process list".into()
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![CommandSpec { id: "process_list", title: "&Process list".into() }]
    }

    fn run_command(&self, _id: &str, ctx: &mut Ctx) {
        ctx.active.push_provider(Box::new(ProcessProvider::new()), String::new());
    }

    fn prefixes(&self) -> &[&'static str] {
        &["proc"]
    }

    fn open_prefix(&self, ctx: &mut Ctx, _prefix: &str, _arg: &str) -> bool {
        ctx.active.push_provider(Box::new(ProcessProvider::new()), String::new());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lists_self() {
        let mut p = ProcessProvider::new();
        let me = std::process::id() as u64;
        let items = p.list(&CancelToken::new()).unwrap();
        let e = items.iter().find(|e| e.id == me).expect("own process listed");
        assert!(p.describe(&e.name).unwrap().contains("PID"));
    }
}

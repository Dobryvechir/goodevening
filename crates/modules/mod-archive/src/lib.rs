//! Archive module (replaces the `arclite` plugin): browse archives as
//! panels, extract (F5 / Shift+F2), add (F5 into a zip / Shift+F1),
//! delete and create folders inside zip files.
//!
//! Formats: zip (read/write); tar, tar.gz/tgz, tar.bz2/tbz, tar.xz/txz,
//! gz, bz2, xz (read-only). Entry names are sanitized, so a hostile
//! archive cannot write outside the extraction folder ("zip slip").
#![forbid(unsafe_code)]

mod provider;

use gm_core::gm_i18n::{MsgId, msg};
use gm_core::gm_ui::dialog::DialogBuilder;
use gm_core::gm_vfs::VfsProvider;
use gm_core::{Action, CommandSpec, Ctx, Module};
use provider::{ArchiveProvider, Kind};
use std::path::{Path, PathBuf};

pub use provider::detect;

#[derive(Default)]
pub struct Archive;

impl Module for Archive {
    fn id(&self) -> &'static str {
        "archive"
    }

    fn title(&self) -> String {
        "Archives".into()
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![
            CommandSpec { id: "add_to_archive", title: "Add to &archive (zip)".into() },
            CommandSpec { id: "extract_archive", title: "E&xtract archive".into() },
        ]
    }

    fn prefixes(&self) -> &[&'static str] {
        &["arc"]
    }

    fn open_prefix(&self, ctx: &mut Ctx, _prefix: &str, arg: &str) -> bool {
        let p = PathBuf::from(arg.trim().trim_matches('"'));
        let p = if p.is_absolute() { p } else { ctx.active.base_dir().join(p) };
        match ArchiveProvider::open(&p) {
            Ok(prov) => {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                ctx.active.push_provider(Box::new(prov), name);
            }
            Err(e) => ctx.push(Action::Message { title: msg(MsgId::MError).into(), text: format!("{}\n{e}", p.display()) }),
        }
        true
    }

    fn open_file(&self, path: &Path, head: &[u8]) -> Option<Box<dyn VfsProvider>> {
        detect(path, head)?;
        ArchiveProvider::open(path).ok().map(|p| Box::new(p) as Box<dyn VfsProvider>)
    }

    fn run_command(&self, id: &str, ctx: &mut Ctx) {
        match id {
            "add_to_archive" => add_to_archive(ctx),
            "extract_archive" => extract_archive(ctx),
            _ => {}
        }
    }
}

fn add_to_archive(ctx: &mut Ctx) {
    let Some(dir) = ctx.active.local_dir() else { return };
    let names = ctx.active.targets();
    if names.is_empty() {
        return;
    }
    let default = if names.len() == 1 {
        let n = &names[0];
        let stem = n.rsplit_once('.').map(|(a, _)| a).filter(|a| !a.is_empty()).unwrap_or(n);
        format!("{stem}.zip")
    } else {
        format!("{}.zip", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "archive".into()))
    };
    let mut b = DialogBuilder::new("Add to archive", 70);
    b.text("Archive (zip):");
    let e = b.edit(&default, Some("ArcName"));
    b.separator();
    b.ok_cancel();
    let mut d = b.build();
    if d.run(ctx.ui).is_none() {
        return;
    }
    let target = PathBuf::from(d.text(e).trim());
    let target = if target.is_absolute() { target } else { dir.join(target) };
    let sources: Vec<PathBuf> = names.iter().map(|n| dir.join(n)).collect();
    let r = (|| -> std::io::Result<()> {
        if !target.exists() {
            provider::create_empty_zip(&target)?;
        }
        let mut p = ArchiveProvider::open(&target)?;
        p.put_files(&sources, &mut |_, _| true)
    })();
    match r {
        Ok(()) => {
            ctx.active.reload();
            let n = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            ctx.active.set_cursor_name(&n);
        }
        Err(err) => ctx.push(Action::Message { title: msg(MsgId::MError).into(), text: err.to_string() }),
    }
}

fn extract_archive(ctx: &mut Ctx) {
    let Some(path) = ctx.active.current_path() else { return };
    let mut head = vec![0u8; 512];
    let n = std::fs::File::open(&path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    head.truncate(n);
    if detect(&path, &head).is_none() {
        return;
    }
    let dest = ctx.passive.local_dir().unwrap_or_else(|| path.parent().map(Path::to_path_buf).unwrap_or_default());
    let mut b = DialogBuilder::new("Extract", 70);
    b.text(format!("Extract {} to:", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    let e = b.edit(&dest.display().to_string(), Some("ExtractTo"));
    b.separator();
    b.ok_cancel();
    let mut d = b.build();
    if d.run(ctx.ui).is_none() {
        return;
    }
    let dest = PathBuf::from(d.text(e).trim());
    let r = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&dest)?;
        let mut p = ArchiveProvider::open(&path)?;
        p.extract_all(&dest)
    })();
    match r {
        Ok(()) => ctx.push(Action::Refresh),
        Err(err) => ctx.push(Action::Message { title: msg(MsgId::MError).into(), text: err.to_string() }),
    }
}

#[allow(dead_code)]
fn kind_name(k: Kind) -> &'static str {
    k.name()
}

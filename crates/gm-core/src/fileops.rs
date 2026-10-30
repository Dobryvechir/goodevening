//! File operation dialogs and the progress/conflict UI (`copy.cpp`,
//! `copy_progress.cpp`, `delete.cpp`, `mkdir.cpp`, `setattr.cpp`).

use crate::app::App;
use gm_fs::{ConflictAction, ConflictChoice, CopyOptions, DeleteMode, ErrorChoice, OpUi, ProgressInfo};
use gm_i18n::{MsgId, fmt, msg, plain};
use gm_term::{Buffer, KeyCode};
use gm_ui::dialog::DialogBuilder;
use gm_ui::message::{bar, draw_box};
use gm_ui::{Ui, draw, message, message_warn};
use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

/// Progress window + interactive conflict/error questions.
pub struct ProgressUi<'a> {
    pub ui: &'a mut Ui,
    bg: Buffer,
    title: String,
    verb: String,
    last: Option<Instant>,
    confirm_interrupt: bool,
}

impl<'a> ProgressUi<'a> {
    pub fn new(ui: &'a mut Ui, title: &str, verb: &str, confirm_interrupt: bool) -> ProgressUi<'a> {
        let bg = ui.term.last_frame().clone();
        ProgressUi { ui, bg, title: title.to_string(), verb: verb.to_string(), last: None, confirm_interrupt }
    }
}

fn fmt_time(t: Option<SystemTime>) -> String {
    format!("{} {}", gm_panels::format::date(t), gm_panels::format::time(t))
}

impl OpUi for ProgressUi<'_> {
    fn progress(&mut self, p: &ProgressInfo) -> bool {
        if self.last.is_none_or(|l| l.elapsed().as_millis() >= 100) {
            self.last = Some(Instant::now());
            let w = 50usize;
            let mut lines = vec![self.verb.clone(), gm_text::truncate_left(&p.current_src.display().to_string(), w)];
            if !p.current_dst.as_os_str().is_empty() {
                lines.push(msg(MsgId::MCopyTo).to_string());
                lines.push(gm_text::truncate_left(&p.current_dst.display().to_string(), w));
            }
            if p.item_total > 0 {
                lines.push(bar(p.item_done, p.item_total, w));
            }
            if p.total_bytes > 0 {
                lines.push(format!("{} {}", msg(MsgId::MCopyDlgTotal), draw::group_digits(p.total_bytes)));
                lines.push(bar(p.total_done, p.total_bytes, w));
            }
            let mut stat = format!("{} {}/{}", msg(MsgId::MCopyFilesTotalInfo), p.files_done, p.files_total);
            let sp = p.speed();
            if sp > 0 {
                stat.push_str(&format!("   {}/s", draw::short_size(sp, 8)));
                if let Some(eta) = p.eta() {
                    stat.push_str(&format!("   {:02}:{:02}:{:02}", eta / 3600, eta / 60 % 60, eta % 60));
                }
            }
            lines.push(stat);
            draw_box(self.ui, &self.bg, &self.title, &lines);
            if p.total_bytes > 0 {
                self.ui.term.set_progress(Some((p.total_done * 100 / p.total_bytes.max(1)) as u8));
            }
        }
        if let Some(k) = self.ui.poll_key()
            && k.code == KeyCode::Esc
        {
            if !self.confirm_interrupt {
                return false;
            }
            let text = format!("{}\n{}", msg(MsgId::MKeyESCWasPressed), msg(MsgId::MDoYouWantToCancel));
            let r = message_warn(self.ui, &self.title, &text, &[msg(MsgId::MYes), msg(MsgId::MNo)]);
            self.last = None;
            if r == Some(0) {
                return false;
            }
        }
        true
    }

    fn conflict(&mut self, src: &Path, sm: &Metadata, dst: &Path, dm: &Metadata) -> ConflictChoice {
        let mut b = DialogBuilder::new(msg(MsgId::MWarning), 72).warn();
        b.text_center(msg(MsgId::MCopyFileExist));
        b.text_center(gm_text::truncate_left(&dst.display().to_string(), 60));
        b.separator();
        b.text(format!("{:<10}{:>16}  {}", plain(MsgId::MCopySource), draw::group_digits(sm.len()), fmt_time(sm.modified().ok())));
        b.text(format!("{:<10}{:>16}  {}", plain(MsgId::MCopyDest), draw::group_digits(dm.len()), fmt_time(dm.modified().ok())));
        b.separator();
        let remember = b.checkbox(msg(MsgId::MCopyRememberChoice), false);
        b.separator();
        let first = b.buttons(
            &[
                msg(MsgId::MCopyOverwrite),
                msg(MsgId::MCopySkip),
                msg(MsgId::MCopyRename),
                msg(MsgId::MCopyAppend),
                msg(MsgId::MCopyOnlyNewerFiles),
                msg(MsgId::MCopyCancel),
            ],
            0,
        );
        let mut d = b.build();
        let r = d.run(self.ui);
        self.last = None;
        let remember = d.checked(remember);
        let action = match r.map(|i| i.saturating_sub(first)) {
            Some(0) => ConflictAction::Overwrite,
            Some(1) => ConflictAction::Skip,
            Some(2) => {
                let name = dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let mut rb = DialogBuilder::new(msg(MsgId::MCopyRenameTitle), 60);
                rb.text(msg(MsgId::MCopyRenameText));
                let e = rb.edit(&gm_fs::unique_name(dst).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name), None);
                rb.separator();
                rb.ok_cancel();
                let mut rd = rb.build();
                match rd.run(self.ui) {
                    Some(_) => ConflictAction::Rename(rd.text(e)),
                    None => ConflictAction::Skip,
                }
            }
            Some(3) => ConflictAction::Append,
            Some(4) => ConflictAction::OnlyNewer,
            _ => ConflictAction::Cancel,
        };
        let _ = src;
        ConflictChoice { remember: remember && !matches!(action, ConflictAction::Rename(_)), action }
    }

    fn error(&mut self, what: &str, path: &Path, err: &io::Error) -> ErrorChoice {
        let text = format!("{what}\n{}\n{err}", path.display());
        let r = message_warn(
            self.ui,
            msg(MsgId::MError),
            &text,
            &[msg(MsgId::MDeleteRetry), msg(MsgId::MCopySkip), msg(MsgId::MCopySkipAll), msg(MsgId::MCopyCancel)],
        );
        self.last = None;
        match r {
            Some(0) => ErrorChoice::Retry,
            Some(1) => ErrorChoice::Skip,
            Some(2) => ErrorChoice::SkipAll,
            _ => ErrorChoice::Cancel,
        }
    }
}

impl Drop for ProgressUi<'_> {
    fn drop(&mut self) {
        self.ui.term.set_progress(None);
    }
}

fn describe_targets(names: &[String]) -> String {
    match names {
        [one] => format!("\"{one}\""),
        many => format!("{} {}", many.len(), plain(MsgId::MAskDeleteObjects)),
    }
}

/// F5 / F6 / Shift+F5 / Shift+F6.
pub fn copy_move(app: &mut App, mv: bool, in_place: bool) {
    let names = app.active().files.targets();
    if names.is_empty() {
        return;
    }
    let src_local = app.active().files.local_dir();
    let dst_local = app.passive().files.local_dir();
    let default_dest = if in_place {
        if names.len() == 1 { names[0].clone() } else { String::new() }
    } else {
        match &dst_local {
            Some(d) => {
                let mut s = d.display().to_string();
                if !s.ends_with(std::path::MAIN_SEPARATOR) {
                    s.push(std::path::MAIN_SEPARATOR);
                }
                s
            }
            None => app.passive().files.display_dir(),
        }
    };
    let what = describe_targets(&names);
    let (title, label) = if mv {
        (
            msg(MsgId::MMoveDlgTitle),
            if names.len() == 1 {
                fmt(MsgId::MMoveFile, &[&what, &msg(MsgId::MCopyTo)])
            } else {
                format!("{} {}:", plain(MsgId::MMoveDlgTitle), what)
            },
        )
    } else {
        (
            msg(MsgId::MCopyDlgTitle),
            if names.len() == 1 {
                fmt(MsgId::MCopyFile, &[&what, &msg(MsgId::MCopyTo)])
            } else {
                format!("{} {} {}:", plain(MsgId::MCopyDlgTitle), what, msg(MsgId::MCopyTo))
            },
        )
    };
    let mut b = DialogBuilder::new(title, 76).help("CopyFiles");
    b.text(label);
    let dest = b.edit(&default_dest, Some("Copy"));
    b.separator();
    let existing = vec![
        "Ask".to_string(),
        plain(MsgId::MCopyOverwrite),
        plain(MsgId::MCopySkip),
        plain(MsgId::MCopyRename),
        plain(MsgId::MCopyAppend),
        plain(MsgId::MCopyOnlyNewerFiles),
    ];
    b.text("Already existing files:");
    let ex = b.combo(&existing[0], existing.clone(), true);
    let times = b.checkbox("Preserve &timestamps", app.settings.system.preserve_times);
    let links = b.checkbox("Copy symbolic links as &links", app.settings.system.copy_links_as_links);
    b.separator();
    let verb = if mv { msg(MsgId::MCopyDlgRename) } else { msg(MsgId::MCopyDlgCopy) };
    let first = b.buttons(&[verb, msg(MsgId::MCopyDlgCancel)], 0);
    let mut d = b.build();
    let r = d.run(&mut app.ui);
    if r != Some(first) && r != Some(dest) {
        return;
    }
    let dest_s = d.text(dest);
    if dest_s.trim().is_empty() {
        return;
    }
    let on_conflict = match d.list_sel(ex) {
        1 => Some(ConflictAction::Overwrite),
        2 => Some(ConflictAction::Skip),
        3 => Some(ConflictAction::Rename(String::new())),
        4 => Some(ConflictAction::Append),
        5 => Some(ConflictAction::OnlyNewer),
        _ => None,
    };
    let opts = CopyOptions {
        move_files: mv,
        preserve_times: d.checked(times),
        preserve_permissions: true,
        copy_links_as_links: d.checked(links),
        on_conflict,
    };
    let confirm_interrupt = app.settings.confirm.interrupt;
    let title = if mv { plain(MsgId::MMoveDlgTitle) } else { plain(MsgId::MCopyDlgTitle) };

    match (src_local, dst_local) {
        (Some(src_dir), _) if !in_place && dest_s == app.passive().files.display_dir() && app.passive().files.local_dir().is_none() => {
            // Local → provider (add to archive / temp panel).
            let paths: Vec<PathBuf> = names.iter().map(|n| src_dir.join(n)).collect();
            let r = app.passive().files.provider_mut().put_files(&paths, &mut |_, _| true);
            if let Err(e) = r {
                gm_ui::message::error(&mut app.ui, &title, &e.to_string());
            }
        }
        (Some(src_dir), _) => {
            let sources: Vec<PathBuf> = names.iter().map(|n| src_dir.join(n)).collect();
            let dp = PathBuf::from(dest_s.trim());
            let dp = if dp.is_absolute() { dp } else { src_dir.join(dp) };
            let verb = if mv { plain(MsgId::MMoveDlgTitle) } else { plain(MsgId::MCopyDlgTitle) };
            let res = {
                let mut pui = ProgressUi::new(&mut app.ui, &title, &verb, confirm_interrupt);
                gm_fs::copy(&sources, &dp, &opts, &mut pui)
            };
            match res {
                Ok(_) => app.active().files.clear_selection(),
                Err(e) => gm_ui::message::error(&mut app.ui, &title, &e.to_string()),
            }
        }
        (None, _) => {
            // Provider → local (extract).
            let dp = PathBuf::from(dest_s.trim());
            if let Err(e) = std::fs::create_dir_all(&dp) {
                gm_ui::message::error(&mut app.ui, &title, &e.to_string());
                return;
            }
            let bg = app.ui.term.last_frame().clone();
            let mut count = 0u64;
            let r = {
                let ui = &mut app.ui;
                let files = if app.active_left { &mut app.left.files } else { &mut app.right.files };
                files.provider_mut().get_files(&names, &dp, &mut |n, _| {
                    count += 1;
                    if count % 16 == 1 {
                        draw_box(ui, &bg, &title, &[n.to_string()]);
                    }
                    !matches!(ui.poll_key(), Some(k) if k.code == KeyCode::Esc)
                })
            };
            match r {
                Ok(()) => {
                    if mv {
                        let _ = app.active().files.provider_mut().delete(&names);
                    }
                    app.active().files.clear_selection();
                }
                Err(e) => gm_ui::message::error(&mut app.ui, &title, &e.to_string()),
            }
        }
    }
    app.refresh_panels();
    if in_place && names.len() == 1 {
        let n = Path::new(dest_s.trim()).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        app.active().files.set_cursor_name(&n);
    }
}

/// F8 / Shift+Del / Alt+Del.
pub fn delete(app: &mut App, mut mode: DeleteMode) {
    let names = app.active().files.targets();
    if names.is_empty() {
        return;
    }
    if mode == DeleteMode::Trash && !app.settings.system.delete_to_trash {
        mode = DeleteMode::Permanent;
    }
    let what = match names.as_slice() {
        [one] => {
            let is_dir = app.active().files.entries.iter().any(|e| e.name == *one && e.is_dir);
            format!("{} \"{one}\"", plain(if is_dir { MsgId::MAskDeleteFolder } else { MsgId::MAskDeleteFile }))
        }
        many => format!("{} {}", many.len(), plain(MsgId::MAskDeleteObjects)),
    };
    let (title, q, btn) = match mode {
        DeleteMode::Trash => (msg(MsgId::MDeleteTitle), fmt(MsgId::MAskDeleteRecycle, &[&what]), msg(MsgId::MDeleteRecycle)),
        DeleteMode::Permanent => (msg(MsgId::MDeleteTitle), fmt(MsgId::MAskDelete, &[&what]), msg(MsgId::MDeleteFileDelete)),
        DeleteMode::Wipe => (msg(MsgId::MDeleteWipeTitle), fmt(MsgId::MAskWipe, &[&what]), msg(MsgId::MDeleteWipe)),
    };
    if app.settings.confirm.delete && message_warn(&mut app.ui, title, &q, &[btn, msg(MsgId::MCancel)]) != Some(0) {
        return;
    }
    let Some(dir) = app.active().files.local_dir() else {
        if let Err(e) = app.active().files.provider_mut().delete(&names) {
            gm_ui::message::error(&mut app.ui, title, &e.to_string());
        }
        app.refresh_panels();
        return;
    };
    // Non-empty folders get a second confirmation.
    if app.settings.confirm.delete_nonempty_folders && mode != DeleteMode::Trash {
        for n in &names {
            let p = dir.join(n);
            if p.is_dir() && std::fs::read_dir(&p).map(|mut r| r.next().is_some()).unwrap_or(false) {
                let r = message_warn(
                    &mut app.ui,
                    msg(MsgId::MDeleteFolderTitle),
                    &format!("{}\n{}", msg(MsgId::MDeleteFolderConfirm), p.display()),
                    &[msg(MsgId::MDeleteFileDelete), msg(MsgId::MDeleteFileAll), msg(MsgId::MDeleteFileCancel)],
                );
                match r {
                    Some(0) => {}
                    Some(1) => break,
                    _ => return,
                }
            }
        }
    }
    let paths: Vec<PathBuf> = names.iter().map(|n| dir.join(n)).collect();
    let confirm_interrupt = app.settings.confirm.interrupt;
    let res = {
        let mut pui = ProgressUi::new(&mut app.ui, msg(MsgId::MDeletingTitle), msg(MsgId::MDeleting), confirm_interrupt);
        gm_fs::delete(&paths, mode, &mut pui)
    };
    if let Err(e) = res {
        gm_ui::message::error(&mut app.ui, title, &e.to_string());
    }
    app.active().files.clear_selection();
    app.refresh_panels();
}

/// F7.
pub fn make_folder(app: &mut App) {
    let mut b = DialogBuilder::new(msg(MsgId::MMakeFolderTitle), 72).help("MakeFolder");
    b.text(msg(MsgId::MCreateFolder));
    let e = b.edit("", Some("NewFolder"));
    let multi = b.checkbox(msg(MsgId::MMultiMakeDir), false);
    b.separator();
    b.ok_cancel();
    let mut d = b.build();
    if d.run(&mut app.ui).is_none() {
        return;
    }
    let spec = d.text(e);
    if spec.trim().is_empty() {
        return;
    }
    let multiple = d.checked(multi);
    match app.active().files.local_dir() {
        Some(dir) => match gm_fs::make_dirs(&dir, &spec, multiple) {
            Ok(first) => {
                app.active().files.reload();
                if let Ok(rel) = first.strip_prefix(&dir)
                    && let Some(c) = rel.components().next()
                {
                    app.active().files.set_cursor_name(&c.as_os_str().to_string_lossy());
                }
            }
            Err(err) => gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &format!("{}\n{spec}\n{err}", msg(MsgId::MCannotCreateFolder))),
        },
        None => {
            if let Err(err) = app.active().files.provider_mut().mkdir(spec.trim()) {
                gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &err.to_string());
            }
            app.active().files.reload();
        }
    }
}

/// Alt+F6.
pub fn link(app: &mut App) {
    let Some(src) = app.active().files.current_path() else { return };
    let Some(pdir) = app.passive().files.local_dir() else { return };
    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut b = DialogBuilder::new(msg(MsgId::MLinkDlgTitle), 72);
    b.text(fmt(MsgId::MLinkFile, &[&format!("\"{name}\""), &msg(MsgId::MCopyTo)]));
    let e = b.edit(&pdir.join(&name).display().to_string(), Some("Link"));
    b.separator();
    b.text(msg(MsgId::MLinkType));
    let hard = b.radio(msg(MsgId::MLinkTypeHardlink), !src.is_dir(), true);
    let sym = b.radio(msg(MsgId::MLinkTypeSymlink), src.is_dir(), false);
    b.separator();
    b.ok_cancel();
    let mut d = b.build();
    if d.run(&mut app.ui).is_none() {
        return;
    }
    let _ = sym;
    let kind = if d.checked(hard) { gm_fs::LinkKind::Hard } else { gm_fs::LinkKind::Symbolic };
    if let Err(err) = gm_fs::make_link(&src, Path::new(&d.text(e)), kind) {
        gm_ui::message::error(&mut app.ui, msg(MsgId::MCannotLink), &err.to_string());
    }
    app.refresh_panels();
}

/// Ctrl+A: Windows attributes or Unix permissions.
pub fn attributes(app: &mut App) {
    let Some(dir) = app.active().files.local_dir() else { return };
    let names = app.active().files.targets();
    if names.is_empty() {
        return;
    }
    let paths: Vec<PathBuf> = names.iter().map(|n| dir.join(n)).collect();
    let metas: Vec<gm_platform::Attrs> =
        paths.iter().filter_map(|p| std::fs::symlink_metadata(p).ok()).map(|m| gm_platform::Attrs::from_metadata(&m)).collect();
    let three = paths.len() > 1;
    // State of one bit across all files: 0 off, 1 on, 2 mixed.
    let state = |f: &dyn Fn(&gm_platform::Attrs) -> bool| -> u8 {
        let on = metas.iter().filter(|a| f(a)).count();
        if on == 0 {
            0
        } else if on == metas.len() {
            1
        } else {
            2
        }
    };
    let mut b = DialogBuilder::new(msg(MsgId::MSetAttrTitle), 60).help("FileAttrDlg");
    b.text_center(msg(MsgId::MSetAttrFor));
    b.text_center(if paths.len() == 1 { names[0].clone() } else { plain(MsgId::MSetAttrSelectedObjects) });
    b.separator();
    let mut items: Vec<(usize, u32)> = Vec::new();
    if cfg!(windows) {
        use gm_platform::attrs::*;
        for (m, bit) in [
            (MsgId::MSetAttrReadOnly, FILE_ATTRIBUTE_READONLY),
            (MsgId::MSetAttrArchive, FILE_ATTRIBUTE_ARCHIVE),
            (MsgId::MSetAttrHidden, FILE_ATTRIBUTE_HIDDEN),
            (MsgId::MSetAttrSystem, FILE_ATTRIBUTE_SYSTEM),
            (MsgId::MSetAttrNotIndexed, FILE_ATTRIBUTE_NOT_CONTENT_INDEXED),
            (MsgId::MSetAttrTemporary, FILE_ATTRIBUTE_TEMPORARY),
            (MsgId::MSetAttrOffline, FILE_ATTRIBUTE_OFFLINE),
        ] {
            let s = state(&|a| a.win & bit != 0);
            let id = if three { b.checkbox3(msg(m), s) } else { b.checkbox(msg(m), s == 1) };
            items.push((id, bit));
        }
    } else {
        for (who, shift) in [("Owner", 6u32), ("Group", 3), ("Others", 0)] {
            b.text(who);
            for (label, bit) in [("&read", 4u32), ("&write", 2), ("e&xecute", 1)] {
                let mask = bit << shift;
                let s = state(&|a| a.mode & mask != 0);
                let id = if three { b.checkbox3(format!("  {label}"), s) } else { b.checkbox(format!("  {label}"), s == 1) };
                items.push((id, mask));
            }
        }
    }
    b.separator();
    b.ok_cancel();
    let mut d = b.build();
    if d.run(&mut app.ui).is_none() {
        return;
    }
    let mut errors = Vec::new();
    for (p, a) in paths.iter().zip(&metas) {
        let r = if cfg!(windows) {
            let mut v = a.win & !gm_platform::attrs::FILE_ATTRIBUTE_DIRECTORY;
            for (id, bit) in &items {
                match d.state(*id) {
                    0 => v &= !bit,
                    1 => v |= bit,
                    _ => {}
                }
            }
            gm_platform::attrs::set_win_attrs(p, if v == 0 { 0x80 } else { v })
        } else {
            let mut m = a.mode & 0o7777;
            for (id, bit) in &items {
                match d.state(*id) {
                    0 => m &= !bit,
                    1 => m |= bit,
                    _ => {}
                }
            }
            gm_platform::attrs::set_mode(p, m)
        };
        if let Err(e) = r {
            errors.push(format!("{}: {e}", p.display()));
        }
    }
    if !errors.is_empty() {
        gm_ui::message::error(&mut app.ui, msg(MsgId::MSetAttrTitle), &errors.join("\n"));
    }
    app.refresh_panels();
}

/// F3 on a folder: count its content (Esc stops).
pub fn folder_size(app: &mut App, p: &Path) {
    let bg = app.ui.term.last_frame().clone();
    let title = plain(MsgId::MDirInfoViewTitle);
    let mut stopped = false;
    let mut last = Instant::now();
    let s = {
        let ui = &mut app.ui;
        gm_fs::dir_size(p, &mut |acc| {
            if last.elapsed().as_millis() > 100 {
                last = Instant::now();
                draw_box(
                    ui,
                    &bg,
                    &title,
                    &[
                        p.display().to_string(),
                        format!("{} {}  {} {}", msg(MsgId::MQuickViewFiles), acc.files, msg(MsgId::MQuickViewBytes), draw::group_digits(acc.bytes)),
                    ],
                );
                if matches!(ui.poll_key(), Some(k) if k.code == KeyCode::Esc) {
                    stopped = true;
                    return false;
                }
            }
            true
        })
    };
    let text = format!(
        "{}\n{} {}\n{} {}\n{} {}{}",
        p.display(),
        msg(MsgId::MQuickViewFolders),
        draw::group_digits(s.dirs),
        msg(MsgId::MQuickViewFiles),
        draw::group_digits(s.files),
        msg(MsgId::MQuickViewBytes),
        draw::group_digits(s.bytes),
        if stopped { "\n…" } else { "" }
    );
    message(&mut app.ui, &title, &text, &[msg(MsgId::MOk)]);
}

//! Execution of panel commands.

use crate::app::App;
use crate::keymap::Command;
use crate::module::{Action, Ctx};
use gm_i18n::{MsgId, msg, plain};
use gm_panels::{EnterResult, PanelKind};
use gm_text::FileMask;
use gm_ui::dialog::DialogBuilder;
use gm_ui::{OpenRequest, clipboard, message};
use std::path::{Path, PathBuf};

pub fn run(app: &mut App, c: Command) {
    use Command::*;
    match c {
        Help => gm_help::show(&mut app.ui, "Panels"),
        UserMenu => crate::usermenu::show(app),
        View => view_or_edit(app, false, false),
        ViewAlt => view_or_edit(app, false, true),
        Edit => view_or_edit(app, true, false),
        EditAlt => view_or_edit(app, true, true),
        EditNew => edit_new(app),
        Copy => crate::fileops::copy_move(app, false, false),
        Move => crate::fileops::copy_move(app, true, false),
        CopyInPlace => crate::fileops::copy_move(app, false, true),
        RenameInPlace => crate::fileops::copy_move(app, true, true),
        MakeFolder => crate::fileops::make_folder(app),
        Delete => crate::fileops::delete(app, gm_fs::DeleteMode::Trash),
        DeletePermanent => crate::fileops::delete(app, gm_fs::DeleteMode::Permanent),
        Wipe => crate::fileops::delete(app, gm_fs::DeleteMode::Wipe),
        Link => crate::fileops::link(app),
        Attributes => crate::fileops::attributes(app),
        Menu => crate::menus::main_menu(app),
        Quit => {
            if !app.settings.confirm.exit
                || message(&mut app.ui, msg(MsgId::MQuit), msg(MsgId::MAskQuit), &[msg(MsgId::MYes), msg(MsgId::MNo)]) == Some(0)
            {
                // Editors with changes ask before closing.
                let mut i = 0;
                while i < app.windows.len() {
                    app.current = Some(i);
                    let _ = app.render();
                    if !app.windows[i].can_close(&mut app.ui) {
                        return;
                    }
                    i += 1;
                }
                app.quit = true;
            }
        }
        Screens => crate::menus::screens(app),
        DriveLeft => crate::menus::drive_menu(app, true),
        DriveRight => crate::menus::drive_menu(app, false),
        FindFile => crate::menus::find_file(app, false),
        FindFolder => crate::menus::find_file(app, true),
        CommandHistory => crate::menus::command_history(app),
        ViewHistory => crate::menus::view_history(app),
        FolderHistory => crate::menus::folder_history(app),
        History => crate::menus::command_history(app),
        Log => crate::exec::enter_logged(app, crate::exec::LogMode::Show),
        LogQuiet => crate::exec::enter_logged(app, crate::exec::LogMode::Quiet),
        SwitchPanel => {
            let other_kind = app.passive().kind;
            if app.passive().visible && other_kind != PanelKind::Info && other_kind != PanelKind::QuickView {
                app.active_left = !app.active_left;
                if let Some(d) = app.active().files.local_dir() {
                    let _ = std::env::set_current_dir(d);
                }
            }
        }
        SwapPanels => {
            std::mem::swap(&mut app.left, &mut app.right);
            app.active_left = !app.active_left;
        }
        UserScreen => app.user_screen(),
        HideLeft => toggle_visible(app, true),
        HideRight => toggle_visible(app, false),
        HideOther => {
            let left = !app.active_left;
            toggle_visible(app, left);
        }
        Refresh => {
            app.refresh_panels();
            let hidden = app.settings.panels.show_hidden;
            for s in [&mut app.left, &mut app.right] {
                if s.kind == PanelKind::Tree
                    && let Some(d) = s.files.local_dir()
                {
                    s.tree.build(&d, hidden);
                }
            }
        }
        ToggleHidden => {
            app.settings.panels.show_hidden = !app.settings.panels.show_hidden;
            app.apply_settings();
        }
        InfoPanel => toggle_kind(app, PanelKind::Info),
        QuickView => toggle_kind(app, PanelKind::QuickView),
        TreePanel => toggle_kind(app, PanelKind::Tree),
        Sort(m) => app.active().files.set_sort(m),
        SortMenu => crate::menus::sort_menu(app),
        ViewMode(n) => {
            let s = app.active();
            s.kind = PanelKind::Files;
            s.files.set_mode(n);
        }
        Describe => describe(app),
        Filter => filter(app),
        Parent => {
            if app.active().files.cd("..").is_ok() {
                app.dir_changed();
            }
        }
        EnterInPlace => enter(app, true),
        Root => {
            if app.active().files.go_root().is_ok() {
                app.dir_changed();
            }
        }
        SelectGroup => select_group(app, true),
        UnselectGroup => select_group(app, false),
        InvertSelection => app.active().files.invert_selection(),
        RestoreSelection => app.active().files.restore_selection(),
        SelectAll => {
            if let Ok(m) = FileMask::new("*") {
                app.active().files.select_mask(&m, true);
            }
        }
        UnselectAll => app.active().files.clear_selection(),
        SelectSameExt => app.active().files.select_same(true, true),
        UnselectSameExt => app.active().files.select_same(true, false),
        InsertName | InsertPath if insert_provider_name(app) => {}
        InsertName => {
            if let Some(n) = app.active().files.current_name() {
                let n = if n == ".." { n } else { gm_platform::shell::quote(&n) };
                app.cmdline.insert_str(&format!("{n} "));
            }
        }
        InsertPath => {
            let p = app.active().files.current_path().or_else(|| app.active().files.local_dir());
            if let Some(p) = p {
                app.cmdline.insert_str(&format!("{} ", gm_platform::shell::quote(&p.display().to_string())));
            }
        }
        InsertLeftDir | InsertRightDir | InsertActiveDir | InsertPassiveDir => {
            let slot = match c {
                InsertLeftDir => &mut app.left,
                InsertRightDir => &mut app.right,
                InsertActiveDir => app.active(),
                _ => app.passive(),
            };
            let d = slot.files.display_dir();
            let sep = if d.ends_with(['/', '\\']) { "" } else { std::path::MAIN_SEPARATOR_STR };
            let s = gm_platform::shell::quote(&format!("{d}{sep}"));
            app.cmdline.insert_str(&s);
        }
        CopyNames | CopyPaths => {
            let names = app.active().files.targets();
            let dir = app.active().files.local_dir();
            let text: Vec<String> = names
                .iter()
                .map(|n| match (&dir, c) {
                    (Some(d), CopyPaths) => d.join(n).display().to_string(),
                    _ => n.clone(),
                })
                .collect();
            clipboard::set(&text.join(if cfg!(windows) { "\r\n" } else { "\n" }));
        }
        ShortcutGo(n) => {
            if let Some(p) = app.db.shortcut(n) {
                go_to_dir(app, Path::new(&p));
            }
        }
        ShortcutSet(n) => {
            let d = app.active().files.base_dir();
            app.db.set_shortcut(n, &d.display().to_string());
        }
        ShortcutsMenu => crate::menus::shortcuts_menu(app),
        ToggleKeyBar => app.settings.interface.keybar = !app.settings.interface.keybar,
        PanelsTaller => app.settings.panels.height_reduce = app.settings.panels.height_reduce.saturating_sub(1),
        PanelsShorter => {
            let max = app.ui.screen().height.saturating_sub(8);
            app.settings.panels.height_reduce = (app.settings.panels.height_reduce + 1).min(max);
        }
        LeftNarrower => app.settings.panels.left_width_percent = app.settings.panels.left_width_percent.saturating_sub(2).max(10),
        LeftWider => app.settings.panels.left_width_percent = (app.settings.panels.left_width_percent + 2).min(90),
        OpenWithSystem => {
            if let Some(p) = app.active().files.current_path()
                && let Err(e) = crate::exec::open_with_system(app, &p)
            {
                gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &e.to_string());
            }
        }
        AddToArchive => module_command(app, "add_to_archive"),
        ExtractArchive => module_command(app, "extract_archive"),
        CompareFolders => module_command(app, "compare_folders"),
        SaveSetup => app.save_setup(),
        ApplyCommand => crate::usermenu::apply_command(app),
    }
}

fn toggle_visible(app: &mut App, left: bool) {
    let slot = if left { &mut app.left } else { &mut app.right };
    slot.visible = !slot.visible;
    if !slot.visible && app.active_left == left {
        let other_visible = if left { app.right.visible } else { app.left.visible };
        if other_visible {
            app.active_left = !left;
        }
    } else if slot.visible && !(if left { app.right.visible } else { app.left.visible }) {
        app.active_left = left;
    }
}

/// Ctrl+L / Ctrl+Q / Ctrl+T: Info and Quick view replace the *passive*
/// panel; the tree replaces the active one (like Far).
fn toggle_kind(app: &mut App, kind: PanelKind) {
    let hidden = app.settings.panels.show_hidden;
    if kind == PanelKind::Tree {
        let s = app.active();
        if s.kind == PanelKind::Tree {
            s.kind = PanelKind::Files;
        } else if let Some(d) = s.files.local_dir() {
            s.kind = PanelKind::Tree;
            s.tree.build(&d, hidden);
        }
        return;
    }
    let p = app.passive();
    p.kind = if p.kind == kind { PanelKind::Files } else { kind };
    p.visible = true;
    if p.kind == PanelKind::Files {
        p.files.reload();
    }
}

pub fn module_command(app: &mut App, id: &str) {
    let Some(mi) = app.modules.iter().position(|m| m.commands().iter().any(|c| c.id == id)) else { return };
    run_module(app, mi, id);
}

pub fn run_module(app: &mut App, mi: usize, id: &str) {
    let actions = {
        let modules = std::mem::take(&mut app.modules);
        let actions;
        {
            let (a, p) = if app.active_left { (&mut app.left, &mut app.right) } else { (&mut app.right, &mut app.left) };
            let mut ctx = Ctx { ui: &mut app.ui, active: &mut a.files, passive: &mut p.files, settings: &app.settings, actions: Vec::new() };
            modules[mi].run_command(id, &mut ctx);
            actions = ctx.actions;
        }
        app.modules = modules;
        actions
    };
    apply_actions(app, actions);
    app.dir_changed();
}

pub fn apply_actions(app: &mut App, actions: Vec<Action>) {
    for a in actions {
        match a {
            Action::View(p) => app.open_viewer(&p),
            Action::ViewText { title, text } => app.view_text(&title, text.into_bytes()),
            Action::Edit(p) => app.open_editor(&p, None),
            Action::Execute(line) => crate::exec::execute_line(app, &line),
            Action::Open(p) => {
                if let Err(e) = crate::exec::open_with_system(app, &p) {
                    gm_ui::message::error(&mut app.ui, msg(MsgId::MCannotExecute), &format!("{}\n{e}", p.display()));
                }
            }
            Action::Cd(p) => go_to_dir(app, &p),
            Action::Message { title, text } => {
                message(&mut app.ui, &title, &text, &[msg(MsgId::MOk)]);
            }
            Action::Refresh => app.refresh_panels(),
        }
    }
}

/// Sets the active panel to a local folder (or the folder of a file, with the cursor on it).
pub fn go_to_dir(app: &mut App, p: &Path) {
    let (dir, name) = if p.is_dir() {
        (p.to_path_buf(), None)
    } else {
        (p.parent().map(Path::to_path_buf).unwrap_or_default(), p.file_name().map(|n| n.to_string_lossy().into_owned()))
    };
    let s = app.active();
    s.kind = PanelKind::Files;
    match s.files.set_local_dir(dir.clone()) {
        Ok(()) => {
            if let Some(n) = name {
                s.files.set_cursor_name(&n);
            }
            app.dir_changed();
        }
        Err(e) => gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &format!("{}\n{e}", dir.display())),
    }
}

fn find_association<'a>(app: &'a App, name: &str) -> Option<&'a gm_config::Association> {
    let groups: std::collections::HashMap<String, String> = app.settings.mask_groups.iter().map(|(k, v)| (k.to_lowercase(), v.clone())).collect();
    app.settings.associations.iter().find(|a| FileMask::with_groups(&a.masks, &groups).map(|m| m.matches(name)).unwrap_or(false))
}

/// Enter (or Ctrl+PgDn with `in_place`) on the panel item.
/// Ctrl+F / Ctrl+Enter on an item of a provider that names its items
/// itself (a database table). Returns false if it does not.
fn insert_provider_name(app: &mut App) -> bool {
    let files = &app.active().files;
    if files.is_local() {
        return false;
    }
    let Some(n) = files.current_name().filter(|n| n != "..") else { return false };
    let Some(text) = files.provider().command_line_name(&n) else { return false };
    app.cmdline.insert_str(&format!("{text} "));
    true
}

pub fn enter(app: &mut App, in_place: bool) {
    let Some(e) = app.active().files.current().cloned() else { return };
    if e.is_dir {
        let r = app.active().files.enter();
        match r {
            Ok(_) => app.dir_changed(),
            Err(err) => gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &format!("{}\n{err}", e.name)),
        }
        return;
    }
    // Inside a provider without real files: view the member.
    if !app.active().files.is_local() && app.active().files.current_path().is_none() {
        // A remote shell (SSH): run the file there.
        let line = app.active().files.provider().run_line(&e.name);
        if let Some(line) = line {
            crate::exec::execute_line(app, &line);
            return;
        }
        if let Some(t) = app.active().files.provider_mut().describe(&e.name) {
            app.view_text(&e.name, t.into_bytes());
            return;
        }
        // Generated text (a database table): like F3.
        if let Some(f) = app.active().files.provider_mut().item_file(&e.name, false) {
            crate::remote::open(app, f, false);
            return;
        }
        // Nested archives: extract to a temp folder and open as a panel.
        let tmp = std::env::temp_dir().join(format!("gm-nested-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let ok = app.active().files.provider_mut().get_files(std::slice::from_ref(&e.name), &tmp, &mut |_, _| true).is_ok();
        if ok && open_as_panel(app, &tmp.join(&e.name)) {
            return;
        }
        view_or_edit(app, false, false);
        return;
    }
    let Some(path) = app.active().files.current_path() else { return };
    // An encoded file: open its decoded copy instead (an archive panel or
    // the system's default program).
    match crate::encoded::decode_for_open(app, &path) {
        crate::encoded::Decoded::NotEncoded => {}
        crate::encoded::Decoded::Failed => return,
        crate::encoded::Decoded::Ready(decoded) => {
            open_decoded(app, &decoded, &e.name);
            return;
        }
    }
    if open_as_panel(app, &path) {
        return;
    }
    let name = e.name.clone();
    if let Some(cmd) = find_association(app, &name).map(|a| if in_place && !a.alt_exec.is_empty() { a.alt_exec.clone() } else { a.exec.clone() })
        && !cmd.is_empty()
    {
        if let Some(line) = crate::usermenu::expand(app, &cmd) {
            crate::exec::execute_line(app, &line);
        }
        return;
    }
    let is_local = app.active().files.is_local();
    if !is_local || matches!(app.active().files.enter(), Ok(EnterResult::File(_))) {
        if e.executable {
            let q = gm_platform::shell::quote(&if is_local { name.clone() } else { path.display().to_string() });
            let line = if cfg!(windows) || !is_local { q } else { format!("./{q}") };
            crate::exec::execute_line(app, &line);
        } else if let Err(err) = crate::exec::open_with_system(app, &path) {
            gm_ui::message::error(&mut app.ui, msg(MsgId::MCannotExecute), &format!("{}\n{err}", path.display()));
        }
    }
}

/// Enter on the encoded file `encoded_name`, after decoding it into `decoded`.
/// The archive panel remembers `encoded_name`, so leaving it puts the cursor
/// back on the encoded file.
fn open_decoded(app: &mut App, decoded: &Path, encoded_name: &str) {
    if open_as_panel_named(app, decoded, encoded_name.to_string()) {
        return;
    }
    // Not an archive: the system's default program for the original extension.
    match crate::exec::open_with_system(app, decoded) {
        // It starts asynchronously: give it time to open the copy before
        // gm tries to delete it.
        Ok(()) => crate::encoded::opened_externally(app, decoded),
        Err(err) => gm_ui::message::error(&mut app.ui, msg(MsgId::MCannotExecute), &format!("{}\n{err}", decoded.display())),
    }
}

/// Asks the modules to open a file as a panel (archives). Returns true if one did.
pub fn open_as_panel(app: &mut App, path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    open_as_panel_named(app, path, name)
}

/// `open_as_panel`, with `name` as the panel's name for the file.
fn open_as_panel_named(app: &mut App, path: &Path, name: String) -> bool {
    let mut head = vec![0u8; 512];
    let n = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    head.truncate(n);
    for m in &app.modules {
        if let Some(p) = m.open_file(path, &head) {
            app.active().files.push_provider(p, name);
            return true;
        }
    }
    false
}

fn view_or_edit(app: &mut App, edit: bool, alt: bool) {
    let Some(e) = app.active().files.current().cloned() else { return };
    if e.is_dotdot() {
        return;
    }
    if !app.active().files.is_local() && app.active().files.current_path().is_none() {
        // Remote files (SFTP): a local copy, uploaded back when the editor
        // saves it; generated text (database panel): a temporary file.
        if !e.is_dir
            && let Some(f) = app.active().files.provider_mut().item_file(&e.name, edit)
        {
            crate::remote::open(app, f, edit);
            return;
        }
        // Provider items: describe() text, or the member's content.
        if let Some(t) = app.active().files.provider_mut().describe(&e.name) {
            app.view_text(&e.name, t.into_bytes());
            return;
        }
        if e.is_dir {
            return;
        }
        let read = app.active().files.provider_mut().open_read(&e.name);
        match read {
            Ok(mut r) => {
                let mut v = Vec::new();
                let _ = std::io::Read::read_to_end(&mut std::io::Read::take(&mut r, 256 << 20), &mut v);
                if edit {
                    message(
                        &mut app.ui,
                        msg(MsgId::MEditTitle),
                        "Editing files inside this panel is not supported yet.\nThe file is opened in the viewer.",
                        &[msg(MsgId::MOk)],
                    );
                }
                app.view_text(&e.name, v);
            }
            Err(err) => gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &err.to_string()),
        }
        return;
    }
    let Some(path) = app.active().files.current_path() else { return };
    if e.is_dir {
        if !edit {
            crate::fileops::folder_size(app, &path);
        }
        return;
    }
    // F3/F4 on an encoded file: view or edit its decoded copy; saving it
    // encodes the original again.
    if !alt {
        match crate::encoded::decode_for_open(app, &path) {
            crate::encoded::Decoded::NotEncoded => {}
            crate::encoded::Decoded::Failed => return,
            crate::encoded::Decoded::Ready(decoded) => {
                if edit {
                    app.open_editor(&decoded, None)
                } else {
                    app.open_viewer(&decoded)
                }
                return;
            }
        }
    }
    // External viewer/editor from file associations (Alt+F3/Alt+F4 use the "alt" commands).
    let cmd = find_association(app, &e.name).map(|a| match (edit, alt) {
        (false, false) => a.view.clone(),
        (false, true) => a.alt_view.clone(),
        (true, false) => a.edit.clone(),
        (true, true) => a.alt_edit.clone(),
    });
    if let Some(c) = cmd.filter(|c| !c.is_empty()) {
        if let Some(line) = crate::usermenu::expand(app, &c) {
            crate::exec::execute_line(app, &line);
        }
        return;
    }
    if edit { app.open_editor(&path, None) } else { app.open_viewer(&path) }
}

fn edit_new(app: &mut App) {
    let mut b = DialogBuilder::new(msg(MsgId::MEditTitle), 70);
    b.text(msg(MsgId::MEditOpenCreateLabel));
    let init = app.active().files.current().filter(|e| !e.is_dir).map(|e| e.name.clone()).unwrap_or_default();
    let e = b.edit(&init, Some("NewEdit"));
    b.separator();
    b.ok_cancel();
    let mut d = b.build();
    if d.run(&mut app.ui).is_none() {
        return;
    }
    let name = d.text(e);
    if name.trim().is_empty() {
        return;
    }
    let p = PathBuf::from(name.trim());
    let path = if p.is_absolute() { p } else { app.work_dir().join(p) };
    app.open(OpenRequest::Edit { path, line: None });
}

fn describe(app: &mut App) {
    let Some(dir) = app.active().files.local_dir() else { return };
    let names = app.active().files.targets();
    for n in names {
        let cur = app.active().files.entries.iter().find(|e| e.name == n).and_then(|e| e.description.clone()).unwrap_or_default();
        let mut b = DialogBuilder::new(plain(MsgId::MMenuDescribe), 72);
        b.text(format!("{}:", n));
        let e = b.edit(&cur, Some("DizText"));
        b.separator();
        b.ok_cancel();
        let mut d = b.build();
        if d.run(&mut app.ui).is_none() {
            break;
        }
        if let Err(err) = gm_panels::diz::set(&dir, &n, &d.text(e)) {
            gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &err.to_string());
            break;
        }
    }
    app.active().files.reload();
}

fn filter(app: &mut App) {
    let cur = app.active().files.filter.as_ref().map(|f| f.0.clone()).unwrap_or_default();
    let mut b = DialogBuilder::new(plain(MsgId::MMenuFilter), 60);
    b.text(msg(MsgId::MFindFileMasks));
    let e = b.edit(&cur, Some("Masks"));
    b.text("(empty = no filter)");
    b.separator();
    b.ok_cancel();
    let mut d = b.build();
    if d.run(&mut app.ui).is_none() {
        return;
    }
    let s = d.text(e);
    let groups: std::collections::HashMap<String, String> = app.settings.mask_groups.iter().map(|(k, v)| (k.to_lowercase(), v.clone())).collect();
    let f = &mut app.active().files;
    if s.trim().is_empty() {
        f.filter = None;
    } else {
        match FileMask::with_groups(&s, &groups) {
            Ok(m) => f.filter = Some((s, m)),
            Err(e) => {
                gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &e.to_string());
                return;
            }
        }
    }
    app.active().files.reload();
}

fn select_group(app: &mut App, on: bool) {
    let title = if on { msg(MsgId::MSelectTitle) } else { msg(MsgId::MUnselectTitle) };
    let mut b = DialogBuilder::new(title, 50);
    let e = b.edit("*.*", Some("Masks"));
    b.separator();
    b.ok_cancel();
    let mut d = b.build();
    if d.run(&mut app.ui).is_none() {
        return;
    }
    let groups: std::collections::HashMap<String, String> = app.settings.mask_groups.iter().map(|(k, v)| (k.to_lowercase(), v.clone())).collect();
    match FileMask::with_groups(&d.text(e), &groups) {
        Ok(m) => app.active().files.select_mask(&m, on),
        Err(err) => gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &err.to_string()),
    }
}

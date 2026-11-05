//! Menus: F9 bar, drives, histories, screens list, module commands, sort.

use crate::app::App;
use crate::keymap::Command;
use crate::module::Ctx;
use gm_i18n::{MsgId, msg, plain};
use gm_panels::{PanelKind, SortMode};
use gm_term::{KeyChord, KeyCode};
use gm_ui::{MenuBar, MenuBarResult, MenuItem, MenuResult, OpenRequest, VMenu, WindowKind, draw, message};
use std::path::{Path, PathBuf};

fn item(app: &App, m: MsgId, c: Command) -> MenuItem {
    MenuItem::with_hint(msg(m), app.keymap.key_for(c).unwrap_or_default())
}

fn panel_menu(app: &App, left: bool) -> Vec<MenuItem> {
    let slot = if left { &app.left } else { &app.right };
    let modes = [
        (MsgId::MMenuBriefView, 1u8),
        (MsgId::MMenuMediumView, 2),
        (MsgId::MMenuFullView, 3),
        (MsgId::MMenuWideView, 4),
        (MsgId::MMenuDetailedView, 5),
        (MsgId::MMenuDizView, 6),
        (MsgId::MMenuLongDizView, 7),
        (MsgId::MMenuOwnersView, 8),
        (MsgId::MMenuLinksView, 9),
        (MsgId::MMenuAlternativeView, 0),
    ];
    let mut v: Vec<MenuItem> = modes
        .iter()
        .map(|(m, n)| MenuItem::with_hint(msg(*m), format!("Ctrl+{n}")).checked(slot.kind == PanelKind::Files && slot.files.mode == *n))
        .collect();
    v.push(MenuItem::separator());
    v.push(item(app, MsgId::MMenuInfoPanel, Command::InfoPanel).checked(slot.kind == PanelKind::Info));
    v.push(item(app, MsgId::MMenuTreePanel, Command::TreePanel).checked(slot.kind == PanelKind::Tree));
    v.push(item(app, MsgId::MMenuQuickView, Command::QuickView).checked(slot.kind == PanelKind::QuickView));
    v.push(MenuItem::separator());
    v.push(item(app, MsgId::MMenuSortModes, Command::SortMenu));
    v.push(MenuItem::with_hint(msg(MsgId::MMenuTogglePanel), if left { "Ctrl+F1" } else { "Ctrl+F2" }));
    v.push(item(app, MsgId::MMenuReread, Command::Refresh));
    v.push(MenuItem::with_hint(msg(MsgId::MMenuChangeDrive), if left { "Alt+F1" } else { "Alt+F2" }));
    v
}

pub fn bar(app: &App) -> MenuBar {
    let files = vec![
        item(app, MsgId::MMenuView, Command::View),
        item(app, MsgId::MMenuEdit, Command::Edit),
        item(app, MsgId::MMenuCopy, Command::Copy),
        item(app, MsgId::MMenuMove, Command::Move),
        item(app, MsgId::MMenuLink, Command::Link),
        item(app, MsgId::MMenuCreateFolder, Command::MakeFolder),
        item(app, MsgId::MMenuDelete, Command::Delete),
        item(app, MsgId::MMenuWipe, Command::Wipe),
        MenuItem::separator(),
        item(app, MsgId::MMenuAdd, Command::AddToArchive),
        item(app, MsgId::MMenuExtract, Command::ExtractArchive),
        MenuItem::separator(),
        item(app, MsgId::MMenuAttributes, Command::Attributes),
        item(app, MsgId::MMenuApplyCommand, Command::ApplyCommand),
        item(app, MsgId::MMenuDescribe, Command::Describe),
        MenuItem::separator(),
        item(app, MsgId::MMenuSelectGroup, Command::SelectGroup),
        item(app, MsgId::MMenuUnselectGroup, Command::UnselectGroup),
        item(app, MsgId::MMenuInvertSelection, Command::InvertSelection),
        item(app, MsgId::MMenuRestoreSelection, Command::RestoreSelection),
    ];
    let commands = vec![
        item(app, MsgId::MMenuFindFile, Command::FindFile),
        item(app, MsgId::MMenuHistory, Command::CommandHistory),
        item(app, MsgId::MMenuFindFolder, Command::FindFolder),
        item(app, MsgId::MMenuViewHistory, Command::ViewHistory),
        item(app, MsgId::MMenuFoldersHistory, Command::FolderHistory),
        MenuItem::separator(),
        item(app, MsgId::MMenuSwapPanels, Command::SwapPanels),
        item(app, MsgId::MMenuTogglePanels, Command::UserScreen),
        MenuItem::new(msg(MsgId::MMenuTerminalPosition)),
        MenuItem::new(msg(MsgId::MMenuMultipleTerminals)),
        MenuItem::new(msg(MsgId::MMenuExecOtherTerminal)),
        item(app, MsgId::MMenuCompareFolders, Command::CompareFolders),
        MenuItem::separator(),
        MenuItem::new(msg(MsgId::MMenuUserMenu)),
        MenuItem::new(msg(MsgId::MMenuFileAssociations)),
        item(app, MsgId::MMenuFolderShortcuts, Command::ShortcutsMenu),
        item(app, MsgId::MMenuFilter, Command::Filter),
        MenuItem::separator(),
        MenuItem::new(msg(MsgId::MMenuEncodedArchives)),
        item(app, MsgId::MMenuWindowsList, Command::Screens),
        MenuItem::new(msg(MsgId::MMenuProcessList)),
    ];
    let options = vec![
        MenuItem::new(msg(MsgId::MMenuSystemSettings)),
        MenuItem::new(msg(MsgId::MMenuPanelSettings)),
        MenuItem::new(msg(MsgId::MMenuInterface)),
        MenuItem::new(msg(MsgId::MMenuLanguages)),
        MenuItem::new(msg(MsgId::MMenuConfirmation)),
        MenuItem::new(msg(MsgId::MMenuMaskGroups)),
        MenuItem::new(msg(MsgId::MMenuViewer)),
        MenuItem::new(msg(MsgId::MMenuEditor)),
        MenuItem::new(msg(MsgId::MMenuColors)),
        MenuItem::new(msg(MsgId::MMenuFilesHighlighting)),
        MenuItem::separator(),
        MenuItem::new("gm:&config"),
        item(app, MsgId::MMenuSaveSetup, Command::SaveSetup),
    ];
    MenuBar::new(vec![
        (msg(MsgId::MMenuLeftTitle).to_string(), panel_menu(app, true)),
        (msg(MsgId::MMenuFilesTitle).to_string(), files),
        (msg(MsgId::MMenuCommandsTitle).to_string(), commands),
        (msg(MsgId::MMenuOptionsTitle).to_string(), options),
        (msg(MsgId::MMenuRightTitle).to_string(), panel_menu(app, false)),
    ])
}

/// F9 → Commands → Terminal position: moves and resizes the console window
/// like `gm --pos`. Does nothing where the system does not allow it.
pub fn terminal_position(app: &mut App) {
    const ITEMS: [(MsgId, &str); 9] = [
        (MsgId::MTermPosFull, "f"),
        (MsgId::MTermPosLeft, "l"),
        (MsgId::MTermPosRight, "r"),
        (MsgId::MTermPosTop, "t"),
        (MsgId::MTermPosBottom, "b"),
        (MsgId::MTermPosLeftTop, "lt"),
        (MsgId::MTermPosLeftBottom, "lb"),
        (MsgId::MTermPosRightTop, "rt"),
        (MsgId::MTermPosRightBottom, "rb"),
    ];
    let items = ITEMS.iter().map(|(name, _)| MenuItem::new(msg(*name))).collect();
    let mut m = VMenu::new(plain(MsgId::MMenuTerminalPosition), items);
    let MenuResult::Selected(i) = m.run(&mut app.ui) else { return };
    if let Some((_, code)) = ITEMS.get(i) {
        move_console(app, code);
    }
}

/// Moves and resizes this console window like `gm --pos <code>`.
fn move_console(app: &mut App, code: &str) {
    if let Some(pos) = gm_platform::window::WindowPos::parse(code) {
        gm_platform::window::place_console(pos);
        // Windows Terminal does not report this resize as an event; adopt
        // the new size now and redraw fully.
        app.ui.term.sync_size();
        app.ui.term.invalidate();
    }
}

/// The `--profile` and `--lang` arguments that give a new gm the same
/// settings and interface language as this one.
fn same_profile_args() -> Vec<std::ffi::OsString> {
    vec![
        "--profile".into(),
        gm_config::profile_dir().as_os_str().to_owned(),
        "--lang".into(),
        gm_i18n::current_language().into(),
    ]
}

/// F9 → Commands → Execute in other terminal.
///
/// In server execution mode (`gm --client=<file>`, or started from here
/// before) the menu only offers to disconnect: gm then runs commands itself
/// again. Otherwise it offers a terminal at the left, right, bottom or top:
/// gm creates a new queue file with a unique name in the settings folder,
/// moves this window to the opposite half, starts
/// `gm --pos <side> --lpath <folder> --server=<queue>` in a new terminal
/// window and switches itself to server execution mode with that queue,
/// as `--client=<queue>` would.
pub fn execute_in_other_terminal(app: &mut App) {
    let title = plain(MsgId::MMenuExecOtherTerminal);
    if app.server_queue.is_some() {
        let mut m = VMenu::new(title.clone(), vec![MenuItem::new(msg(MsgId::MExecTermDisconnect))]);
        if let MenuResult::Selected(0) = m.run(&mut app.ui)
            && let Some(path) = app.server_queue.take()
            // The server runs what is queued before it, then deletes the
            // queue file and exits.
            && let Err(e) = crate::queue::push(&path, crate::queue::EXIT_COMMAND)
        {
            message(&mut app.ui, &title, &format!("{}\n{e}", path.display()), &[msg(MsgId::MOk)]);
        }
        return;
    }
    // (item, where the new terminal goes, where this gm moves)
    const ITEMS: [(MsgId, &str, &str); 4] = [
        (MsgId::MExecTermLeft, "l", "r"),
        (MsgId::MExecTermRight, "r", "l"),
        (MsgId::MExecTermBottom, "b", "t"),
        (MsgId::MExecTermTop, "t", "b"),
    ];
    let items = ITEMS.iter().map(|(name, _, _)| MenuItem::new(msg(*name))).collect();
    let mut m = VMenu::new(title.clone(), items);
    let MenuResult::Selected(i) = m.run(&mut app.ui) else { return };
    let Some((_, other, this)) = ITEMS.get(i) else { return };
    let dir = app.work_dir();
    let name = match crate::queue::create_unique() {
        Ok(n) => n,
        Err(e) => {
            message(&mut app.ui, &title, &format!("{}\n{e}", gm_config::profile_dir().display()), &[msg(MsgId::MOk)]);
            return;
        }
    };
    let path = crate::queue::queue_path(&name);
    // Move this window first: where gm finds its window as the active one,
    // the new gm must not see this one while placing itself.
    move_console(app, this);
    let mut args: Vec<std::ffi::OsString> = vec!["--pos".into(), (*other).into()];
    // A panel that shows no local folder leaves the server in its own folder.
    if dir.is_absolute() && dir.is_dir() {
        args.extend(["--lpath".into(), dir.into_os_string()]);
    }
    args.extend(same_profile_args());
    args.push(format!("--server={name}").into());
    match gm_platform::instance::open_in_new_terminal(&args) {
        Ok(()) => app.server_queue = Some(path),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            message(&mut app.ui, &title, &format!("{}\n{e}", msg(MsgId::MMultiTermError)), &[msg(MsgId::MOk)]);
        }
    }
    app.ui.term.invalidate();
}

/// F9 → Commands → Multiple terminals: moves this console window to the
/// first part of the screen (as Terminal position does) and starts one more
/// independent gm for every other part, each in a new terminal window with
/// `--pos <part> --lpath <left folder> --rpath <right folder>`.
pub fn multiple_terminals(app: &mut App) {
    const ITEMS: [(MsgId, &[&str]); 3] = [
        (MsgId::MMultiTermLeftRight, &["l", "r"]),
        (MsgId::MMultiTermTopBottom, &["t", "b"]),
        (MsgId::MMultiTermFour, &["lt", "rt", "lb", "rb"]),
    ];
    let items = ITEMS.iter().map(|(name, _)| MenuItem::new(msg(*name))).collect();
    let mut m = VMenu::new(plain(MsgId::MMenuMultipleTerminals), items);
    let MenuResult::Selected(i) = m.run(&mut app.ui) else { return };
    let Some((_, parts)) = ITEMS.get(i) else { return };
    move_console(app, parts[0]);
    let mut common: Vec<std::ffi::OsString> = Vec::new();
    // Panels that show no local folder (e.g. a network panel) are left to
    // the new gm's saved settings.
    for (opt, dir) in [("--lpath", app.left.files.base_dir()), ("--rpath", app.right.files.base_dir())] {
        if dir.is_absolute() && dir.is_dir() {
            common.extend([opt.into(), dir.into_os_string()]);
        }
    }
    // The same settings and language as this gm.
    common.extend(same_profile_args());
    for (n, part) in parts[1..].iter().enumerate() {
        if n > 0 {
            // Let the previous terminal open its window first: where gm finds
            // its window as the active one, the starts must not overlap.
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        let mut args: Vec<std::ffi::OsString> = vec!["--pos".into(), (*part).into()];
        args.extend(common.iter().cloned());
        if let Err(e) = gm_platform::instance::open_in_new_terminal(&args) {
            message(&mut app.ui, &plain(MsgId::MMenuMultipleTerminals), &format!("{}
{e}", msg(MsgId::MMultiTermError)), &[msg(MsgId::MOk)]);
            break;
        }
    }
    app.ui.term.invalidate();
}

pub fn main_menu(app: &mut App) {
    let mut b = bar(app);
    b.selected = if app.active_left { app.menu_last.0.min(4) } else { 4 };
    if app.menu_last.0 != 0 && app.menu_last.0 != 4 {
        b.selected = app.menu_last.0;
    }
    b.last_items = app.menu_last.1.clone();
    let r = b.run(&mut app.ui);
    app.menu_last = (b.selected, b.last_items.clone());
    let MenuBarResult::Selected(menu, it) = r else { return };
    match menu {
        0 | 4 => {
            let left = menu == 0;
            let was = app.active_left;
            let mode = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 0];
            if it < 10 {
                let s = if left { &mut app.left } else { &mut app.right };
                s.kind = PanelKind::Files;
                s.files.set_mode(mode[it]);
                return;
            }
            // Panel-specific commands act on that panel.
            if app.left.kind == PanelKind::Files || app.right.kind == PanelKind::Files {
                app.active_left = left;
            }
            match it {
                11 => crate::commands::run(app, Command::InfoPanel),
                12 => crate::commands::run(app, Command::TreePanel),
                13 => crate::commands::run(app, Command::QuickView),
                15 => crate::commands::run(app, Command::SortMenu),
                16 => crate::commands::run(app, if left { Command::HideLeft } else { Command::HideRight }),
                17 => crate::commands::run(app, Command::Refresh),
                18 => drive_menu(app, left),
                _ => {}
            }
            if matches!(it, 11 | 13) {
                app.active_left = was;
            }
        }
        1 => {
            let cmds = [
                Some(Command::View),
                Some(Command::Edit),
                Some(Command::Copy),
                Some(Command::Move),
                Some(Command::Link),
                Some(Command::MakeFolder),
                Some(Command::Delete),
                Some(Command::Wipe),
                None,
                Some(Command::AddToArchive),
                Some(Command::ExtractArchive),
                None,
                Some(Command::Attributes),
                Some(Command::ApplyCommand),
                Some(Command::Describe),
                None,
                Some(Command::SelectGroup),
                Some(Command::UnselectGroup),
                Some(Command::InvertSelection),
                Some(Command::RestoreSelection),
            ];
            if let Some(Some(c)) = cmds.get(it) {
                crate::commands::run(app, *c);
            }
        }
        2 => match it {
            0 => find_file(app, false),
            1 => command_history(app),
            2 => find_file(app, true),
            3 => view_history(app),
            4 => folder_history(app),
            6 => crate::commands::run(app, Command::SwapPanels),
            7 => app.user_screen(),
            8 => terminal_position(app),
            9 => multiple_terminals(app),
            10 => execute_in_other_terminal(app),
            11 => crate::commands::run(app, Command::CompareFolders),
            13 => crate::usermenu::edit_menu_file(app),
            14 => crate::options::associations(app),
            15 => shortcuts_menu(app),
            16 => crate::commands::run(app, Command::Filter),
            18 => crate::encoded::configure(app),
            19 => screens(app),
            20 => crate::exec::execute_line(app, "proc:"),
            _ => {}
        },
        3 => match it {
            0 => crate::options::system(app),
            1 => crate::options::panels(app),
            2 => crate::options::interface(app),
            3 => crate::options::languages(app),
            4 => crate::options::confirmations(app),
            5 => crate::options::mask_groups(app),
            6 => crate::options::viewer(app),
            7 => crate::options::editor(app),
            8 => crate::options::colors(app),
            9 => crate::options::highlighting(app),
            11 => crate::options::config_editor(app),
            12 => app.save_setup(),
            _ => {}
        },
        _ => {}
    }
}

/// F12: list of open windows.
pub fn screens(app: &mut App) {
    let mut items = vec![MenuItem::with_hint(format!("&0 {}", "Panels"), "")];
    for (i, w) in app.windows.iter().enumerate() {
        let kind = match w.kind() {
            WindowKind::Viewer => plain(MsgId::MHistoryView),
            WindowKind::Editor => plain(MsgId::MHistoryEdit),
            _ => String::new(),
        };
        let hk = if i < 9 { format!("&{} ", i + 1) } else { "  ".into() };
        let mark = if w.modified() { "*" } else { " " };
        items.push(MenuItem::new(format!("{hk}{kind:<6}{mark}{}", gm_text::truncate_left(&w.title(), 60))));
    }
    let mut m = VMenu::new("Screens", items);
    m.select(app.current.map(|i| i + 1).unwrap_or(0));
    m.pass_keys = vec![KeyChord::plain(KeyCode::Delete)];
    match m.run(&mut app.ui) {
        MenuResult::Selected(0) => app.current = None,
        MenuResult::Selected(i) => app.current = Some(i - 1),
        MenuResult::Key(_, i) if i > 0 => {
            app.current = Some(i - 1);
            if app.windows[i - 1].can_close(&mut app.ui) {
                app.close_window(i - 1);
            }
        }
        _ => {}
    }
}

/// Alt+F1 / Alt+F2.
pub fn drive_menu(app: &mut App, left: bool) {
    let drives = gm_platform::drives::list();
    let mut items = Vec::new();
    for d in &drives {
        let kind = d.kind.label();
        let size = match (d.total, d.free) {
            (Some(t), Some(f)) => format!("{:>8} {:>8}", draw::short_size(t, 8), draw::short_size(f, 8)),
            _ => String::new(),
        };
        let hk = if cfg!(windows) && d.name.len() == 2 { format!("&{}", d.name) } else { d.name.clone() };
        items.push(MenuItem::with_hint(format!("{hk:<20} {kind:<10} {}", d.label), size));
    }
    let mut extra = Vec::new();
    for (mi, m) in app.modules.iter().enumerate() {
        for di in m.drive_menu_items() {
            extra.push((mi, di.id));
            if extra.len() == 1 {
                items.push(MenuItem::separator());
            }
            items.push(MenuItem::new(di.title));
        }
    }
    let mut m = VMenu::new(msg(MsgId::MChangeDriveTitle), items);
    let cur = if left { app.left.files.base_dir() } else { app.right.files.base_dir() };
    if let Some(p) = drives.iter().enumerate().filter(|(_, d)| cur.starts_with(&d.path)).max_by_key(|(_, d)| d.path.as_os_str().len()).map(|(i, _)| i)
    {
        m.select(p);
    }
    let MenuResult::Selected(i) = m.run(&mut app.ui) else { return };
    let was = app.active_left;
    app.active_left = left;
    if i < drives.len() {
        let target = drives[i].path.clone();
        // Far remembers the last folder per drive; go there if we were on that drive.
        crate::commands::go_to_dir(app, &target);
    } else {
        let k = i - drives.len() - 1;
        if let Some((mi, id)) = extra.get(k).copied() {
            let modules = std::mem::take(&mut app.modules);
            let actions = {
                let (a, p) = if app.active_left { (&mut app.left, &mut app.right) } else { (&mut app.right, &mut app.left) };
                let mut ctx = Ctx { ui: &mut app.ui, active: &mut a.files, passive: &mut p.files, settings: &app.settings, actions: Vec::new() };
                modules[mi].open_drive_item(id, &mut ctx);
                ctx.actions
            };
            app.modules = modules;
            crate::commands::apply_actions(app, actions);
        }
    }
    let _ = was;
}

fn history_menu(app: &mut App, kind: &str, title: &str) -> Option<(String, String, bool)> {
    loop {
        let h = app.db.history(kind);
        if h.is_empty() {
            return None;
        }
        let items = h
            .iter()
            .map(|(t, extra, locked)| {
                let hint = match extra.as_str() {
                    "v" => plain(MsgId::MHistoryView),
                    "e" => plain(MsgId::MHistoryEdit),
                    _ => String::new(),
                };
                let mut it = MenuItem::with_hint(t.replace('&', "&&"), hint);
                it.checked = *locked;
                it
            })
            .collect();
        let mut m = VMenu::new(title, items);
        m.filterable = true;
        m.max_height = Some(app.ui.screen().height.saturating_sub(6));
        m.pass_keys = vec![
            KeyChord::shift(KeyCode::Delete),
            KeyChord::plain(KeyCode::Delete),
            KeyChord::plain(KeyCode::Insert),
            KeyChord::ctrl(KeyCode::Enter),
            KeyChord::shift(KeyCode::Enter),
        ];
        match m.run(&mut app.ui) {
            MenuResult::Selected(i) => return Some((h[i].0.clone(), h[i].1.clone(), false)),
            MenuResult::Key(k, i) => match (k.code, k.mods.0) {
                (KeyCode::Delete, 4) => app.db.delete_history_item(kind, &h[i].0),
                (KeyCode::Delete, _) => {
                    if message(&mut app.ui, title, msg(MsgId::MHistoryClear), &[msg(MsgId::MYes), msg(MsgId::MNo)]) == Some(0) {
                        app.db.clear_history(kind);
                    }
                }
                (KeyCode::Insert, _) => app.db.toggle_lock(kind, &h[i].0),
                (KeyCode::Enter, _) => return Some((h[i].0.clone(), h[i].1.clone(), true)),
                _ => {}
            },
            _ => return None,
        }
    }
}

/// Alt+F8: Enter runs, Ctrl+Enter / Shift+Enter puts the command into the command line.
pub fn command_history(app: &mut App) {
    if let Some((line, _, to_cmdline)) = history_menu(app, "cmd", msg(MsgId::MHistoryTitle)) {
        if to_cmdline {
            app.cmdline.set_text(&line);
        } else {
            crate::exec::execute_line(app, &line);
        }
    }
}

/// Alt+F11.
pub fn view_history(app: &mut App) {
    if let Some((path, extra, _)) = history_menu(app, "view", msg(MsgId::MViewHistoryTitle)) {
        let p = PathBuf::from(path);
        if extra == "e" {
            app.open(OpenRequest::Edit { path: p, line: None });
        } else {
            app.open(OpenRequest::View(p));
        }
    }
}

/// Alt+F12.
pub fn folder_history(app: &mut App) {
    if let Some((path, _, _)) = history_menu(app, "folder", msg(MsgId::MFolderHistoryTitle)) {
        crate::commands::go_to_dir(app, Path::new(&path));
    }
}

/// Ctrl+F12.
pub fn sort_menu(app: &mut App) {
    let cur = (app.active().files.sort, app.active().files.reverse);
    let names = [
        (SortMode::Name, MsgId::MMenuSortByName),
        (SortMode::Ext, MsgId::MMenuSortByExt),
        (SortMode::Modified, MsgId::MMenuSortByWrite),
        (SortMode::Size, MsgId::MMenuSortBySize),
        (SortMode::Unsorted, MsgId::MMenuUnsorted),
        (SortMode::Created, MsgId::MMenuSortByCreation),
        (SortMode::Accessed, MsgId::MMenuSortByAccess),
        (SortMode::Description, MsgId::MMenuSortByDiz),
        (SortMode::Owner, MsgId::MMenuSortByOwner),
    ];
    let mut items: Vec<MenuItem> = names
        .iter()
        .map(|(m, id)| {
            let mark = if cur.0 == *m { if cur.1 { "▼ " } else { "▲ " } } else { "  " };
            MenuItem::with_hint(format!("{mark}{}", msg(*id)), format!("Ctrl+F{}", m.fkey())).checked(cur.0 == *m)
        })
        .collect();
    items.push(MenuItem::separator());
    items.push(MenuItem::new("&Reverse order").checked(cur.1));
    let mut m = VMenu::new(msg(MsgId::MMenuSortTitle), items);
    m.select(names.iter().position(|(s, _)| *s == cur.0).unwrap_or(0));
    match m.run(&mut app.ui) {
        MenuResult::Selected(i) if i < names.len() => app.active().files.set_sort(names[i].0),
        MenuResult::Selected(_) => {
            let f = &mut app.active().files;
            f.reverse = !f.reverse;
            f.resort();
        }
        _ => {}
    }
}

/// Folder shortcuts (Ctrl+Shift+0…9 to set, Ctrl+Alt+0…9 to go).
pub fn shortcuts_menu(app: &mut App) {
    let list = app.db.shortcuts();
    let items = (0..10u8)
        .map(|n| {
            let p = list.iter().find(|(s, _)| *s == n).map(|(_, p)| p.clone()).unwrap_or_default();
            MenuItem::with_hint(format!("&{n}  {p}"), format!("Ctrl+Alt+{n}"))
        })
        .collect();
    let mut m = VMenu::new(msg(MsgId::MFolderShortcutsTitle), items);
    m.pass_keys = vec![KeyChord::plain(KeyCode::Insert), KeyChord::plain(KeyCode::Delete)];
    match m.run(&mut app.ui) {
        MenuResult::Selected(i) => {
            if let Some((_, p)) = list.iter().find(|(s, _)| *s as usize == i) {
                crate::commands::go_to_dir(app, Path::new(p));
            }
        }
        MenuResult::Key(k, i) => {
            if k.code == KeyCode::Insert {
                let d = app.active().files.base_dir();
                app.db.set_shortcut(i as u8, &d.display().to_string());
            } else {
                app.db.set_shortcut(i as u8, "");
            }
        }
        _ => {}
    }
}

/// Alt+F7 / Alt+F10.
pub fn find_file(app: &mut App, folders_only: bool) {
    let have_sel = app.active().files.selected_count() > 0;
    let mut params = app.find_params.clone();
    if folders_only {
        params.folders = true;
        params.text.text.clear();
    }
    loop {
        let Some(p) = gm_search::find_dialog(&mut app.ui, &params, have_sel) else { return };
        params = p;
        app.find_params = params.clone();
        let base = app.work_dir();
        let roots: Vec<PathBuf> = match params.area {
            gm_search::Area::AllDrives => gm_platform::drives::list()
                .into_iter()
                .filter(|d| matches!(d.kind, gm_platform::DriveKind::Fixed | gm_platform::DriveKind::Root))
                .map(|d| d.path)
                .collect(),
            gm_search::Area::Path => std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default(),
            gm_search::Area::Root => vec![gm_platform::drives::volume_root(&base)],
            gm_search::Area::Selected => {
                let s: Vec<PathBuf> = app.active().files.target_paths().into_iter().filter(|p| p.is_dir()).collect();
                if s.is_empty() { vec![base] } else { s }
            }
            _ => vec![base],
        };
        match gm_search::results_window(&mut app.ui, params.clone(), roots) {
            gm_search::FindOutcome::NewSearch => continue,
            gm_search::FindOutcome::GoTo(p) => crate::commands::go_to_dir(app, &p),
            gm_search::FindOutcome::View(p) => app.open_viewer(&p),
            gm_search::FindOutcome::Edit(p) => app.open_editor(&p, None),
            gm_search::FindOutcome::Panel(items) => {
                if let Some(prov) = app.modules.iter().find_map(|m| m.list_panel(items.clone())) {
                    let s = app.active();
                    s.kind = PanelKind::Files;
                    s.files.push_provider(prov, String::new());
                }
            }
            gm_search::FindOutcome::None => {}
        }
        return;
    }
}

pub fn about(app: &mut App) {
    let modules: Vec<String> = app.modules.iter().map(|m| m.title()).collect();
    let text = format!(
        "Goodevening Manager {}\nA Rust-based cross-platform app for {}\n inspired by Midnight Commander and Far Manager\nCopyright © 2026 Danyil Dobryvechir\n\nModules: {}",
        env!("CARGO_PKG_VERSION"),
        gm_platform::os_name(),
        if modules.is_empty() { "-".into() } else { modules.join(", ") }
    );
    message(&mut app.ui, "gm:about", &text, &[msg(MsgId::MOk)]);
}

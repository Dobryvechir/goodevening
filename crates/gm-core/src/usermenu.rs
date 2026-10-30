//! User menu (F2, `usermenu.cpp`) and metasymbols (`fnparse.cpp`).
//!
//! `GmMenu.ini` format (same as Far's `FarMenu.ini`):
//!
//! ```text
//! A:  Archive the current file
//!     tar czf !.tgz !.!
//! S:  Submenu {
//!     1:  First item
//!         echo !.!
//! }
//! ```

use crate::app::App;
use gm_i18n::{MsgId, msg, plain};
use gm_term::{KeyChord, KeyCode};
use gm_ui::dialog::DialogBuilder;
use gm_ui::{MenuItem, MenuResult, VMenu};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct MenuEntry {
    pub hotkey: String,
    pub label: String,
    pub commands: Vec<String>,
    pub submenu: Option<Vec<MenuEntry>>,
}

pub fn parse(text: &str) -> Vec<MenuEntry> {
    fn level(lines: &mut std::iter::Peekable<std::str::Lines>) -> Vec<MenuEntry> {
        let mut out: Vec<MenuEntry> = Vec::new();
        while let Some(&line) = lines.peek() {
            let t = line.trim_end();
            if t.trim() == "}" {
                lines.next();
                return out;
            }
            lines.next();
            if t.trim().is_empty() || t.trim_start().starts_with(';') {
                continue;
            }
            if !t.starts_with([' ', '\t']) {
                let (hk, label) = match t.split_once(':') {
                    Some((h, l)) => (h.trim().to_string(), l.trim().to_string()),
                    None => (String::new(), t.trim().to_string()),
                };
                if let Some(l) = label.strip_suffix('{') {
                    let sub = level(lines);
                    out.push(MenuEntry { hotkey: hk, label: l.trim().to_string(), commands: Vec::new(), submenu: Some(sub) });
                } else {
                    out.push(MenuEntry { hotkey: hk, label, commands: Vec::new(), submenu: None });
                }
            } else if let Some(last) = out.last_mut() {
                let c = t.trim();
                if c == "}" {
                    return out;
                }
                last.commands.push(c.to_string());
            }
        }
        out
    }
    let mut it = text.lines().peekable();
    level(&mut it)
}

fn local_menu_path(app: &mut App) -> PathBuf {
    app.work_dir().join("GmMenu.ini")
}

pub fn show(app: &mut App) {
    let local = local_menu_path(app);
    let (path, title) = if local.is_file() { (local, "Local menu") } else { (gm_config::global_user_menu_path(), "Main menu") };
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let entries = parse(&text);
    if entries.is_empty() {
        let r = gm_ui::message(
            &mut app.ui,
            msg(MsgId::MMenuUserMenu),
            &format!("{}\n\nThe user menu is empty. Edit it now?", path.display()),
            &[msg(MsgId::MYes), msg(MsgId::MNo)],
        );
        if r == Some(0) {
            seed_menu(&path);
            app.open_editor(&path, None);
        }
        return;
    }
    run_menu(app, &entries, title, &path);
}

fn seed_menu(path: &Path) {
    if path.exists() {
        return;
    }
    let sample = if cfg!(windows) {
        "D:  Show the folder size\n    dir /s !.!\nE:  Open with the associated program\n    start \"\" \"!.!\"\n"
    } else {
        "D:  Disk usage of the current item\n    du -sh !.!\nE:  Open with the associated program\n    xdg-open !.!\n"
    };
    let _ = std::fs::write(path, sample);
}

fn run_menu(app: &mut App, entries: &[MenuEntry], title: &str, path: &Path) {
    let items = entries
        .iter()
        .map(|e| {
            let hk = if e.hotkey.chars().count() == 1 { format!("&{}", e.hotkey) } else { e.hotkey.clone() };
            let label = if e.submenu.is_some() { format!("{} »", e.label) } else { e.label.clone() };
            MenuItem::new(format!("{hk:<4} {label}"))
        })
        .collect();
    let mut m = VMenu::new(title, items);
    m.bottom = "F4=edit".into();
    m.pass_keys = vec![KeyChord::plain(KeyCode::F(4))];
    // F-key hotkeys ("F1:") select directly.
    for (i, e) in entries.iter().enumerate() {
        if let Some(k) = KeyChord::parse(&e.hotkey).filter(|k| matches!(k.code, KeyCode::F(_))) {
            m.pass_keys.push(k);
            let _ = i;
        }
    }
    match m.run(&mut app.ui) {
        MenuResult::Selected(i) => run_entry(app, &entries[i], path),
        MenuResult::Key(k, _) if k.code == KeyCode::F(4) && k.mods.0 == 0 => {
            seed_menu(path);
            app.open_editor(path, None);
        }
        MenuResult::Key(k, _) => {
            if let Some(e) = entries.iter().find(|e| KeyChord::parse(&e.hotkey) == Some(k)) {
                let e = e.clone();
                run_entry(app, &e, path);
            }
        }
        _ => {}
    }
}

fn run_entry(app: &mut App, e: &MenuEntry, path: &Path) {
    if let Some(sub) = &e.submenu {
        let sub = sub.clone();
        run_menu(app, &sub, &e.label, path);
        return;
    }
    for c in &e.commands {
        let Some(line) = expand(app, c) else { return };
        crate::exec::execute_line(app, &line);
    }
}

/// F9 → Commands → Edit user menu.
pub fn edit_menu_file(app: &mut App) {
    let items = vec![MenuItem::new("&Main menu"), MenuItem::new("&Local menu")];
    let mut m = VMenu::new(plain(MsgId::MMenuUserMenu), items);
    let path = match m.run(&mut app.ui) {
        MenuResult::Selected(0) => gm_config::global_user_menu_path(),
        MenuResult::Selected(_) => local_menu_path(app),
        _ => return,
    };
    seed_menu(&path);
    app.open_editor(&path, None);
}

/// Ctrl+G: apply a command (with metasymbols) to every selected file.
pub fn apply_command(app: &mut App) {
    let mut b = DialogBuilder::new(plain(MsgId::MMenuApplyCommand), 72);
    b.text("Command (!.! = file name):");
    let e = b.edit("", Some("ApplyCmd"));
    b.separator();
    b.ok_cancel();
    let mut d = b.build();
    if d.run(&mut app.ui).is_none() {
        return;
    }
    let template = d.text(e);
    if template.trim().is_empty() {
        return;
    }
    let names = app.active().files.targets();
    for n in names {
        app.active().files.set_cursor_name(&n);
        let Some(line) = expand_for(app, &template, Some(&n)) else { return };
        crate::exec::execute_line(app, &line);
    }
}

/// Expands Far metasymbols in `template` for the current item.
/// Returns `None` if the user cancelled a `!?prompt?default!` input.
pub fn expand(app: &mut App, template: &str) -> Option<String> {
    expand_for(app, template, None)
}

fn expand_for(app: &mut App, template: &str, name: Option<&str>) -> Option<String> {
    let mut out = String::new();
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    let mut passive = false;
    let quote = gm_platform::shell::quote;
    while i < chars.len() {
        if chars[i] != '!' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let rest: String = chars[i + 1..].iter().collect();
        let (slot_name, slot_dir, slot_sel) = {
            let s = if passive { app.passive() } else { app.active() };
            let n = name.map(String::from).filter(|_| !passive).or_else(|| s.files.current_name()).unwrap_or_default();
            (n, s.files.local_dir().unwrap_or_else(|| s.files.base_dir()), s.files.targets())
        };
        let stem = match slot_name.rfind('.') {
            Some(p) if p > 0 => slot_name[..p].to_string(),
            _ => slot_name.clone(),
        };
        let ext = match slot_name.rfind('.') {
            Some(p) if p > 0 => slot_name[p + 1..].to_string(),
            _ => String::new(),
        };
        let mut consumed = 1;
        if rest.starts_with('!') {
            out.push('!');
            consumed = 2;
        } else if rest.starts_with(".!") {
            out.push_str(&slot_name);
            consumed = 3;
        } else if rest.starts_with("~") {
            out.push_str(&stem);
            consumed = 2;
        } else if rest.starts_with("`") {
            out.push_str(&ext);
            consumed = 2;
        } else if rest.starts_with('\\') || rest.starts_with('/') {
            let mut d = slot_dir.display().to_string();
            if !d.ends_with(std::path::MAIN_SEPARATOR) {
                d.push(std::path::MAIN_SEPARATOR);
            }
            out.push_str(&d);
            consumed = 2;
        } else if rest.starts_with("=\\") || rest.starts_with("=/") {
            out.push_str(&slot_dir.display().to_string());
            consumed = 3;
        } else if rest.starts_with(':') {
            let root = gm_platform::drives::volume_root(&slot_dir).display().to_string();
            out.push_str(root.trim_end_matches(['\\', '/']));
            consumed = 2;
        } else if rest.starts_with('&') {
            let list: Vec<String> = slot_sel.iter().map(|n| quote(n)).collect();
            out.push_str(&list.join(" "));
            consumed = 2;
        } else if rest.starts_with("@!") {
            // List file with the selected names, one per line.
            let p = std::env::temp_dir().join(format!("gm-list-{}.txt", std::process::id()));
            let _ = std::fs::write(&p, slot_sel.join(if cfg!(windows) { "\r\n" } else { "\n" }));
            out.push_str(&p.display().to_string());
            consumed = 3;
        } else if rest.starts_with('#') {
            passive = true;
            consumed = 2;
            i += consumed;
            continue;
        } else if rest.starts_with('^') {
            passive = false;
            consumed = 2;
            i += consumed;
            continue;
        } else if let Some(body) = rest.strip_prefix('?') {
            // !?Title?Default!
            if let Some(q) = body.find('?')
                && let Some(end) = body[q + 1..].find('!')
            {
                let title = &body[..q];
                let def = &body[q + 1..q + 1 + end];
                let mut b = DialogBuilder::new(msg(MsgId::MMenuUserMenu), 60);
                b.text(title.to_string());
                let e = b.edit(def, Some("UserVar"));
                b.separator();
                b.ok_cancel();
                let mut d = b.build();
                d.run(&mut app.ui)?;
                out.push_str(&d.text(e));
                consumed = 1 + 1 + q + 1 + end + 1;
            }
        } else {
            // Bare "!" = name without extension.
            out.push_str(&stem);
        }
        i += consumed;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_menu() {
        // As in Far, submenu items start at column 0; commands are indented.
        let m = parse("A:  First\n    echo 1\n    echo 2\nS: Sub {\n1: Inner\n     ls\n}\nF5: Five\n    x\n");
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].commands, ["echo 1", "echo 2"]);
        let sub = m[1].submenu.as_ref().unwrap();
        assert_eq!(sub[0].label, "Inner");
        assert_eq!(sub[0].commands, ["ls"]);
        assert_eq!(m[2].hotkey, "F5");
    }
}

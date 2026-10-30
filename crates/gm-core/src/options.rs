//! F9 → Options dialogs and the gm:config editor.

use crate::app::App;
use gm_config::{Association, HighlightRule};
use gm_i18n::{MsgId, msg, plain};
use gm_term::{C16, KeyChord, KeyCode, palette};
use gm_ui::dialog::DialogBuilder;
use gm_ui::{MenuItem, MenuResult, Ui, VMenu, message};

/// A dialog of checkboxes and labelled edit fields.
fn form(ui: &mut Ui, title: &str, help: &str, checks: &[(&str, bool)], edits: &[(&str, String)]) -> Option<(Vec<bool>, Vec<String>)> {
    let mut b = DialogBuilder::new(title, 66).help(help);
    let ids: Vec<usize> = checks.iter().map(|(l, v)| b.checkbox(*l, *v)).collect();
    let mut eids = Vec::new();
    if !edits.is_empty() {
        b.separator();
        for (l, v) in edits {
            b.text(*l);
            eids.push(b.edit(v, None));
        }
    }
    b.separator();
    b.ok_cancel();
    let mut d = b.build();
    let first_btn = d.items.len() - 2;
    let r = d.run(ui)?;
    if r == first_btn + 1 {
        return None;
    }
    Some((ids.iter().map(|i| d.checked(*i)).collect(), eids.iter().map(|i| d.text(*i)).collect()))
}

pub fn system(app: &mut App) {
    let s = &app.settings.system;
    let Some((c, e)) = form(
        &mut app.ui,
        msg(MsgId::MConfigSystemTitle),
        "SystemSettings",
        &[
            (msg(MsgId::MConfigRecycleBin), s.delete_to_trash),
            ("Copy symbolic links as &links", s.copy_links_as_links),
            ("&Preserve timestamps when copying", s.preserve_times),
            (msg(MsgId::MConfigSaveHistory), s.save_history),
            (msg(MsgId::MConfigSaveFoldersHistory), s.save_folder_history),
            (msg(MsgId::MConfigSaveViewHistory), s.save_view_history),
            (msg(MsgId::MConfigAutoSave), s.auto_save_setup),
        ],
        &[("History size:", s.history_size.to_string())],
    ) else {
        return;
    };
    let s = &mut app.settings.system;
    s.delete_to_trash = c[0];
    s.copy_links_as_links = c[1];
    s.preserve_times = c[2];
    s.save_history = c[3];
    s.save_folder_history = c[4];
    s.save_view_history = c[5];
    s.auto_save_setup = c[6];
    if let Ok(n) = e[0].trim().parse() {
        s.history_size = n;
    }
    app.apply_settings();
}

pub fn panels(app: &mut App) {
    let p = &app.settings.panels;
    let Some((c, _)) = form(
        &mut app.ui,
        msg(MsgId::MConfigPanelTitle),
        "PanelSettings",
        &[
            (msg(MsgId::MConfigHidden), p.show_hidden),
            (msg(MsgId::MConfigHighlight), p.highlight),
            (msg(MsgId::MConfigSelectFolders), p.select_folders),
            (msg(MsgId::MConfigSortingDigitsAsNumbers), p.natural_sort),
            (msg(MsgId::MConfigShowDotsInRoot), p.show_dotdot_in_root),
            (msg(MsgId::MConfigShowColumns), p.show_column_titles),
            (msg(MsgId::MConfigShowStatus), p.show_status_line),
            (msg(MsgId::MConfigShowTotal), p.show_total),
            (msg(MsgId::MConfigShowFree), p.show_free_space),
            (msg(MsgId::MConfigShowScrollbar), p.show_scrollbar),
            ("&Automatic panel update", p.auto_refresh),
        ],
        &[],
    ) else {
        return;
    };
    let p = &mut app.settings.panels;
    [
        p.show_hidden,
        p.highlight,
        p.select_folders,
        p.natural_sort,
        p.show_dotdot_in_root,
        p.show_column_titles,
        p.show_status_line,
        p.show_total,
        p.show_free_space,
        p.show_scrollbar,
        p.auto_refresh,
    ] = [c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], c[9], c[10]];
    app.apply_settings();
}

pub fn interface(app: &mut App) {
    let i = &app.settings.interface;
    let Some((c, e)) = form(
        &mut app.ui,
        msg(MsgId::MConfigInterfaceTitle),
        "InterfSettings",
        &[
            (msg(MsgId::MConfigClock), i.clock),
            (msg(MsgId::MConfigMouse), i.mouse),
            (msg(MsgId::MConfigKeyBar), i.keybar),
            (msg(MsgId::MConfigMenuBar), i.menubar_always),
            (msg(MsgId::MConfigShowScreensNumber), i.show_screen_number),
            ("Set the terminal &title", i.console_title),
            ("Esc+digit as F-keys (alternate &keymap)", i.alt_keymap),
        ],
        &[("Colors (auto, 16, truecolor):", i.colors.clone())],
    ) else {
        return;
    };
    let i = &mut app.settings.interface;
    [i.clock, i.mouse, i.keybar, i.menubar_always, i.show_screen_number, i.console_title, i.alt_keymap] = [c[0], c[1], c[2], c[3], c[4], c[5], c[6]];
    i.colors = e[0].trim().to_string();
    app.apply_settings();
}

pub fn confirmations(app: &mut App) {
    let c = &app.settings.confirm;
    let Some((v, _)) = form(
        &mut app.ui,
        msg(MsgId::MSetConfirmTitle),
        "ConfirmDlg",
        &[
            (msg(MsgId::MSetConfirmCopy), c.copy),
            (msg(MsgId::MSetConfirmMove), c.move_),
            (msg(MsgId::MSetConfirmRO), c.overwrite),
            (msg(MsgId::MSetConfirmDrag), c.drag),
            (msg(MsgId::MSetConfirmDelete), c.delete),
            (msg(MsgId::MSetConfirmDeleteFolders), c.delete_nonempty_folders),
            (msg(MsgId::MSetConfirmEsc), c.interrupt),
            ("E&xit", c.exit),
        ],
        &[],
    ) else {
        return;
    };
    let c = &mut app.settings.confirm;
    [c.copy, c.move_, c.overwrite, c.drag, c.delete, c.delete_nonempty_folders, c.interrupt, c.exit] =
        [v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]];
}

pub fn viewer(app: &mut App) {
    let v = &app.settings.viewer;
    let Some((c, e)) = form(
        &mut app.ui,
        msg(MsgId::MViewConfigTitle),
        "ViewerSettings",
        &[
            (msg(MsgId::MViewConfigSavePos), v.save_position),
            (msg(MsgId::MViewAutoDetectCodePage), v.auto_detect_codepage),
            (msg(MsgId::MViewConfigArrows), v.show_arrows),
            (msg(MsgId::MViewConfigScrollbar), v.show_scrollbar),
            ("&Wrap long lines", v.wrap),
            ("Wrap by w&ords", v.word_wrap),
        ],
        &[(msg(MsgId::MViewConfigTabSize), v.tab_size.to_string()), ("Default code page (65001 = UTF-8):", v.default_codepage.to_string())],
    ) else {
        return;
    };
    let v = &mut app.settings.viewer;
    [v.save_position, v.auto_detect_codepage, v.show_arrows, v.show_scrollbar, v.wrap, v.word_wrap] = [c[0], c[1], c[2], c[3], c[4], c[5]];
    if let Ok(n) = e[0].trim().parse::<usize>() {
        v.tab_size = n.clamp(1, 32);
    }
    if let Ok(n) = e[1].trim().parse::<u32>()
        && gm_text::Codepage::from_id(n).is_some()
    {
        v.default_codepage = n;
    }
}

pub fn editor(app: &mut App) {
    let v = &app.settings.editor;
    let Some((c, e)) = form(
        &mut app.ui,
        msg(MsgId::MEditConfigTitle),
        "EditorSettings",
        &[
            (msg(MsgId::MEditConfigExpandTabs), v.expand_tabs),
            (msg(MsgId::MEditConfigPersistentBlocks), v.persistent_blocks),
            (msg(MsgId::MEditConfigDelRemovesBlocks), v.del_removes_blocks),
            (msg(MsgId::MEditConfigAutoIndent), v.auto_indent),
            (msg(MsgId::MEditConfigSavePos), v.save_position),
            (msg(MsgId::MEditConfigLineNumbers), v.show_line_numbers),
            ("Show &whitespace", v.show_whitespace),
            ("Auto-detect code &page", v.auto_detect_codepage),
            ("Warn when opening &read-only files", v.warn_readonly),
            ("&Lock read-only files", v.lock_readonly),
        ],
        &[
            (msg(MsgId::MEditConfigTabSize), v.tab_size.to_string()),
            ("Default code page (65001 = UTF-8):", v.default_codepage.to_string()),
            ("Line breaks for new files (native, crlf, lf, cr):", v.new_file_eol.clone()),
        ],
    ) else {
        return;
    };
    let v = &mut app.settings.editor;
    [
        v.expand_tabs,
        v.persistent_blocks,
        v.del_removes_blocks,
        v.auto_indent,
        v.save_position,
        v.show_line_numbers,
        v.show_whitespace,
        v.auto_detect_codepage,
        v.warn_readonly,
        v.lock_readonly,
    ] = [c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], c[9]];
    if let Ok(n) = e[0].trim().parse::<usize>() {
        v.tab_size = n.clamp(1, 32);
    }
    if let Ok(n) = e[1].trim().parse::<u32>()
        && gm_text::Codepage::from_id(n).is_some()
    {
        v.default_codepage = n;
    }
    let eol = e[2].trim().to_lowercase();
    if ["native", "crlf", "lf", "cr"].contains(&eol.as_str()) {
        v.new_file_eol = eol;
    }
}

pub fn languages(app: &mut App) {
    let langs = gm_i18n::languages();
    let cur = gm_i18n::current_language();
    let items = langs.iter().map(|(n, d)| MenuItem::with_hint(*d, *n).checked(*n == cur)).collect();
    let mut m = VMenu::new(plain(MsgId::MMenuLanguages), items);
    if let Some(p) = langs.iter().position(|(n, _)| *n == cur) {
        m.select(p);
    }
    if let MenuResult::Selected(i) = m.run(&mut app.ui) {
        gm_i18n::set_language(langs[i].0);
        app.settings.language = langs[i].0.to_string();
        app.refresh_labels();
    }
}

pub fn mask_groups(app: &mut App) {
    loop {
        let groups: Vec<(String, String)> = app.settings.mask_groups.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let items = groups.iter().map(|(k, v)| MenuItem::new(format!("<{k}>  {}", gm_text::truncate_right(v, 50)))).collect();
        let mut m = VMenu::new(plain(MsgId::MMenuMaskGroups), items);
        m.bottom = "Enter Ins Del".into();
        m.pass_keys = vec![KeyChord::plain(KeyCode::Insert), KeyChord::plain(KeyCode::Delete)];
        let (edit, idx) = match m.run(&mut app.ui) {
            MenuResult::Selected(i) => (true, Some(i)),
            MenuResult::Key(k, i) if k.code == KeyCode::Delete => {
                if let Some((n, _)) = groups.get(i) {
                    app.settings.mask_groups.remove(n);
                }
                continue;
            }
            MenuResult::Key(_, _) => (true, None),
            _ => break,
        };
        if edit {
            let (name, masks) = idx.and_then(|i| groups.get(i).cloned()).unwrap_or_default();
            let Some((_, e)) =
                form(&mut app.ui, &plain(MsgId::MMenuMaskGroups), "MaskGroupsSettings", &[], &[("Name:", name.clone()), ("Masks:", masks)])
            else {
                continue;
            };
            if !name.is_empty() {
                app.settings.mask_groups.remove(&name);
            }
            if !e[0].trim().is_empty() {
                app.settings.mask_groups.insert(e[0].trim().to_lowercase(), e[1].trim().to_string());
            }
        }
    }
    app.apply_settings();
}

fn pick_color(ui: &mut Ui, title: &str, current: C16) -> Option<C16> {
    let items = palette::NAMES.iter().map(|(n, c)| MenuItem::new(*n).checked(*c == current)).collect();
    let mut m = VMenu::new(title, items);
    if let Some(p) = palette::NAMES.iter().position(|(_, c)| *c == current) {
        m.select(p);
    }
    match m.run(ui) {
        MenuResult::Selected(i) => Some(palette::NAMES[i].1),
        _ => None,
    }
}

pub fn colors(app: &mut App) {
    let groups = ["Panel", "Dialog", "WarnDialog", "Menu", "HMenu", "Keybar", "CommandLine", "Clock", "Viewer", "Editor", "Help"];
    loop {
        let mut items: Vec<MenuItem> = groups.iter().map(|g| MenuItem::new(*g)).collect();
        items.push(MenuItem::separator());
        items.push(MenuItem::new("&Reset to defaults"));
        let mut m = VMenu::new(plain(MsgId::MMenuColors), items);
        let g = match m.run(&mut app.ui) {
            MenuResult::Selected(i) if i < groups.len() => groups[i],
            MenuResult::Selected(_) => {
                palette::reset_defaults();
                continue;
            }
            _ => break,
        };
        loop {
            let elems: Vec<(usize, &str)> =
                palette::DEFAULTS.iter().enumerate().filter(|(_, (n, _, _))| n.split('.').next() == Some(g)).map(|(i, (n, _, _))| (i, *n)).collect();
            let items = elems
                .iter()
                .map(|(i, n)| {
                    let (f, b) = palette::colors(palette::ALL[*i]);
                    MenuItem::with_hint(*n, format!("{} on {}", f.name(), b.name()))
                })
                .collect();
            let mut em = VMenu::new(g, items);
            let MenuResult::Selected(k) = em.run(&mut app.ui) else { break };
            let id = palette::ALL[elems[k].0];
            let (f, b) = palette::colors(id);
            let Some(nf) = pick_color(&mut app.ui, "Foreground", f) else { continue };
            let Some(nb) = pick_color(&mut app.ui, "Background", b) else { continue };
            palette::set_colors(id, nf, nb);
        }
    }
    app.settings.colors = palette::changed_overrides();
}

pub fn highlighting(app: &mut App) {
    loop {
        let rules = app.settings.highlight.clone();
        let items = rules
            .iter()
            .map(|r| {
                let mut flags = String::new();
                for (f, c) in [(r.directories, 'D'), (r.hidden, 'H'), (r.system, 'S'), (r.executable, 'X'), (r.links, 'L')] {
                    flags.push(match f {
                        Some(true) => c,
                        Some(false) => '-',
                        None => '.',
                    });
                }
                MenuItem::with_hint(format!("{flags}  {}", gm_text::truncate_right(&r.masks, 40)), r.color.clone())
            })
            .collect();
        let mut m = VMenu::new(msg(MsgId::MHighlightTitle), items);
        m.bottom = "Enter Ins Del F5=defaults".into();
        m.pass_keys = vec![KeyChord::plain(KeyCode::Insert), KeyChord::plain(KeyCode::Delete), KeyChord::plain(KeyCode::F(5))];
        let idx = match m.run(&mut app.ui) {
            MenuResult::Selected(i) => Some(i),
            MenuResult::Key(k, i) => match k.code {
                KeyCode::Delete => {
                    if i < app.settings.highlight.len() {
                        app.settings.highlight.remove(i);
                    }
                    continue;
                }
                KeyCode::F(5) => {
                    if message(&mut app.ui, msg(MsgId::MHighlightTitle), msg(MsgId::MHighlightAskRestore), &[msg(MsgId::MYes), msg(MsgId::MNo)])
                        == Some(0)
                    {
                        app.settings.highlight = gm_config::default_highlight();
                    }
                    continue;
                }
                _ => None,
            },
            _ => break,
        };
        let r = idx.and_then(|i| rules.get(i).cloned()).unwrap_or_default();
        let tri = |v: Option<bool>| match v {
            Some(true) => 1,
            Some(false) => 0,
            None => 2,
        };
        let mut b = DialogBuilder::new(msg(MsgId::MHighlightEditTitle), 66);
        b.text(msg(MsgId::MFileAssocMasks));
        let masks = b.edit(&r.masks, Some("Masks"));
        b.separator_text("Attributes ([?] = any)");
        let d = b.checkbox3("&Directory", tri(r.directories));
        let h = b.checkbox3("&Hidden", tri(r.hidden));
        let s = b.checkbox3("&System", tri(r.system));
        let x = b.checkbox3("E&xecutable", tri(r.executable));
        let l = b.checkbox3("Symbolic &link", tri(r.links));
        b.separator();
        let names: Vec<String> = palette::NAMES.iter().map(|(n, _)| n.to_string()).collect();
        b.text("Color:");
        let col = b.combo(if r.color.is_empty() { "Default" } else { &r.color }, names.clone(), true);
        b.text("Color under cursor:");
        let ccol = b.combo(if r.cursor_color.is_empty() { "Default" } else { &r.cursor_color }, names, true);
        b.text(msg(MsgId::MHighlightMark));
        let mark = b.edit(&r.mark, None);
        b.separator();
        b.ok_cancel();
        let mut dlg = b.build();
        let first_btn = dlg.items.len() - 2;
        match dlg.run(&mut app.ui) {
            Some(i) if i != first_btn + 1 => {}
            _ => continue,
        }
        let from = |v: u8| match v {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        };
        let rule = HighlightRule {
            masks: dlg.text(masks),
            directories: from(dlg.state(d)),
            hidden: from(dlg.state(h)),
            system: from(dlg.state(s)),
            executable: from(dlg.state(x)),
            links: from(dlg.state(l)),
            color: dlg.text(col),
            cursor_color: dlg.text(ccol),
            mark: dlg.text(mark).chars().take(1).collect(),
        };
        match idx {
            Some(i) => app.settings.highlight[i] = rule,
            None => app.settings.highlight.push(rule),
        }
    }
    app.apply_settings();
}

/// F9 → Commands → File associations.
pub fn associations(app: &mut App) {
    loop {
        let list = app.settings.associations.clone();
        let items = list
            .iter()
            .map(|a| MenuItem::new(if a.description.is_empty() { a.masks.clone() } else { format!("{} ({})", a.description, a.masks) }))
            .collect();
        let mut m = VMenu::new(msg(MsgId::MFileAssocTitle), items);
        m.bottom = "Enter Ins Del".into();
        m.min_width = 40;
        m.pass_keys = vec![KeyChord::plain(KeyCode::Insert), KeyChord::plain(KeyCode::Delete)];
        let idx = match m.run(&mut app.ui) {
            MenuResult::Selected(i) => Some(i),
            MenuResult::Key(k, i) if k.code == KeyCode::Delete => {
                if i < app.settings.associations.len() {
                    app.settings.associations.remove(i);
                }
                continue;
            }
            MenuResult::Key(_, _) => None,
            _ => break,
        };
        let a = idx.and_then(|i| list.get(i).cloned()).unwrap_or_default();
        let Some((_, e)) = form(
            &mut app.ui,
            msg(MsgId::MFileAssocTitle),
            "FileAssoc",
            &[],
            &[
                (msg(MsgId::MFileAssocMasks), a.masks),
                (msg(MsgId::MFileAssocDescr), a.description),
                (msg(MsgId::MFileAssocExec), a.exec),
                (msg(MsgId::MFileAssocAltExec), a.alt_exec),
                (msg(MsgId::MFileAssocView), a.view),
                (msg(MsgId::MFileAssocAltView), a.alt_view),
                (msg(MsgId::MFileAssocEdit), a.edit),
                (msg(MsgId::MFileAssocAltEdit), a.alt_edit),
            ],
        ) else {
            continue;
        };
        if e[0].trim().is_empty() {
            continue;
        }
        let na = Association {
            masks: e[0].clone(),
            description: e[1].clone(),
            exec: e[2].clone(),
            alt_exec: e[3].clone(),
            view: e[4].clone(),
            alt_view: e[5].clone(),
            edit: e[6].clone(),
            alt_edit: e[7].clone(),
        };
        match idx {
            Some(i) => app.settings.associations[i] = na,
            None => app.settings.associations.push(na),
        }
    }
}

/// gm:config: every setting, editable in place.
pub fn config_editor(app: &mut App) {
    let mut sel = 0;
    loop {
        let flat = app.settings.flatten();
        let items = flat
            .iter()
            .map(|(k, v)| {
                let (kind, val) = match v {
                    toml::Value::Boolean(b) => ("boolean", b.to_string()),
                    toml::Value::Integer(i) => ("integer", i.to_string()),
                    toml::Value::String(s) => ("string", s.clone()),
                    other => ("", other.to_string()),
                };
                MenuItem::with_hint(format!("{k:<42} {kind:<8}"), gm_text::truncate_right(&val, 30)).data(0)
            })
            .collect();
        let mut m = VMenu::new(format!("gm:config — {}", plain(MsgId::MConfigEditor)), items);
        m.filterable = true;
        m.select(sel);
        m.max_height = Some(app.ui.screen().height.saturating_sub(4));
        let MenuResult::Selected(i) = m.run(&mut app.ui) else { break };
        sel = i;
        let (key, val) = flat[i].clone();
        let r = match val {
            toml::Value::Boolean(b) => app.settings.set_flat(&key, if b { "false" } else { "true" }),
            other => {
                let cur = match &other {
                    toml::Value::String(s) => s.clone(),
                    o => o.to_string(),
                };
                let mut b = DialogBuilder::new(&key, 70);
                let e = b.edit(&cur, None);
                b.separator();
                b.ok_cancel();
                let mut d = b.build();
                if d.run(&mut app.ui).is_none() {
                    continue;
                }
                app.settings.set_flat(&key, &d.text(e))
            }
        };
        if let Err(e) = r {
            gm_ui::message::error(&mut app.ui, "gm:config", &e);
        }
        app.apply_settings();
    }
}

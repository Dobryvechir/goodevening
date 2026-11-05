//! Databases panel: Alt+F1/Alt+F2 → "Databases" lists the saved
//! configurations (Enter opens, Ins adds, F4 edits, Del deletes). Each has
//! a host, a database name, a user, an optional password and a driver
//! (MariaDB >= 10.0 or PostgreSQL).
//!
//! Opening a configuration connects; without a password, or with a wrong
//! one, the password is asked. The panel then lists the user tables and
//! views (sorted like files); see [`provider`] for what the keys do there.
//! Leaving the panel closes the connection.
#![forbid(unsafe_code)]

mod config;
mod conn;
mod provider;
mod sql;

use config::{Driver, SavedDb};
use gm_core::gm_term::{KeyChord, KeyCode};
use gm_core::gm_ui::dialog::DialogBuilder;
use gm_core::gm_ui::message::{draw_box, error};
use gm_core::gm_ui::{MenuItem, MenuResult, Ui, VMenu, message_warn};
use gm_core::{CommandSpec, Ctx, DriveItem, Module};

const TITLE: &str = "Databases";

#[derive(Default)]
pub struct DbPanel;

impl Module for DbPanel {
    fn id(&self) -> &'static str {
        "db_panel"
    }
    fn title(&self) -> String {
        TITLE.into()
    }
    fn commands(&self) -> Vec<CommandSpec> {
        vec![CommandSpec { id: "databases", title: "&Databases".into() }]
    }
    fn run_command(&self, _id: &str, ctx: &mut Ctx) {
        configurations_menu(ctx);
    }
    fn drive_menu_items(&self) -> Vec<DriveItem> {
        vec![DriveItem { id: 0, title: TITLE.into() }]
    }
    fn open_drive_item(&self, _id: usize, ctx: &mut Ctx) {
        configurations_menu(ctx);
    }
}

fn busy(ui: &mut Ui, line: String) {
    let bg = ui.term.last_frame().clone();
    draw_box(ui, &bg, TITLE, &[line]);
}

fn amp(s: &str) -> String {
    s.replace('&', "&&")
}

/// The saved configurations: Enter opens, Ins (or the last item) adds, F4 edits, Del deletes.
fn configurations_menu(ctx: &mut Ctx) {
    let path = config::store_path();
    let mut list = match config::load(&path) {
        Ok(l) => l,
        Err(e) => {
            error(ctx.ui, TITLE, &format!("Cannot read the saved configurations\n{e}"));
            return;
        }
    };
    let mut sel = 0;
    loop {
        let mut items: Vec<MenuItem> = list
            .iter()
            .map(|c| {
                let host = if c.host.trim().is_empty() { "localhost" } else { c.host.trim() };
                let pw = if c.password.is_empty() { "" } else { " ●" };
                MenuItem::with_hint(amp(&format!("{host}/{}", c.db)), amp(&format!("{}  {}{pw}", c.driver.short(), c.user)))
            })
            .collect();
        if !items.is_empty() {
            items.push(MenuItem::separator());
        }
        let add_index = items.len();
        items.push(MenuItem::new("Add a new configuration…"));
        let mut m = VMenu::new(TITLE, items);
        m.bottom = "Enter Ins F4 Del".into();
        m.min_width = 56;
        m.pass_keys = vec![KeyChord::plain(KeyCode::Insert), KeyChord::plain(KeyCode::F(4)), KeyChord::plain(KeyCode::Delete)];
        m.select(sel.min(add_index));
        let edit = match m.run(ctx.ui) {
            MenuResult::Selected(i) if i < list.len() => {
                if open(ctx, &list[i]) {
                    return;
                }
                sel = i;
                continue;
            }
            MenuResult::Selected(_) => None,
            MenuResult::Key(k, i) if k.code == KeyCode::Delete => {
                if i < list.len() {
                    let q = format!("Delete the configuration\n{}/{}?", list[i].host, list[i].db);
                    if message_warn(ctx.ui, TITLE, &q, &["Delete", "Cancel"]) == Some(0) {
                        list.remove(i);
                        persist(ctx.ui, &path, &list);
                    }
                }
                sel = i;
                continue;
            }
            MenuResult::Key(k, i) if k.code == KeyCode::F(4) && i < list.len() => Some(i),
            MenuResult::Key(_, _) => None,
            _ => return,
        };
        let init = edit.and_then(|i| list.get(i).cloned()).unwrap_or_default();
        let Some(c) = edit_dialog(ctx.ui, &init) else {
            sel = edit.unwrap_or(add_index);
            continue;
        };
        match edit {
            Some(i) => {
                list[i] = c;
                sel = i;
            }
            None => {
                list.push(c);
                sel = list.len() - 1;
            }
        }
        persist(ctx.ui, &path, &list);
    }
}

fn persist(ui: &mut Ui, path: &std::path::Path, list: &[SavedDb]) {
    if let Err(e) = config::save(path, list) {
        error(ui, TITLE, &format!("Cannot save the configurations\n{e}"));
    }
}

/// The add/edit dialog; None if cancelled.
fn edit_dialog(ui: &mut Ui, init: &SavedDb) -> Option<SavedDb> {
    let mut cur = init.clone();
    loop {
        let mut b = DialogBuilder::new(TITLE, 64);
        b.text("&Host: host[:port] (empty: localhost)");
        let host = b.edit(&cur.host, Some("DbHost"));
        b.text("D&b name (PostgreSQL: empty is the user name)");
        let db = b.edit(&cur.db, Some("DbName"));
        b.text(format!("&User (empty: \"{}\")", config::local_user()));
        let user = b.edit(&cur.user, Some("DbUser"));
        b.text("&Password (optional; asked when needed)");
        let pw = b.password(&cur.password);
        b.text("&Driver");
        let driver = b.combo(cur.driver.label(), Driver::ALL.iter().map(|d| d.label().to_string()).collect(), true);
        b.separator();
        b.ok_cancel();
        let mut d = b.build();
        let cancel = d.items.len() - 1;
        let r = d.run(ui)?;
        if r == cancel {
            return None;
        }
        cur = SavedDb {
            host: d.text(host).trim().to_string(),
            db: d.text(db).trim().to_string(),
            user: d.text(user).trim().to_string(),
            password: d.text(pw),
            driver: Driver::from_label(&d.text(driver)).unwrap_or(cur.driver),
        };
        match config::resolve(&cur) {
            Ok(_) => return Some(cur),
            Err(e) => error(ui, TITLE, &e),
        }
    }
}

fn ask_password(ui: &mut Ui, prompt: &str, retry: Option<&str>) -> Option<String> {
    let mut b = DialogBuilder::new(TITLE, 64);
    if let Some(r) = retry {
        for l in r.lines() {
            b.text(l);
        }
    }
    b.text(prompt);
    let e = b.password("");
    b.separator();
    b.ok_cancel();
    let mut d = b.build();
    let cancel = d.items.len() - 1;
    let r = d.run(ui)?;
    if r == cancel {
        return None;
    }
    Some(d.text(e))
}

/// Connects and shows the tables and views in the active panel. Returns
/// false if it did not happen (cancelled or failed; the reason was shown).
fn open(ctx: &mut Ctx, c: &SavedDb) -> bool {
    let mut params = match config::resolve(c) {
        Ok(p) => p,
        Err(e) => {
            error(ctx.ui, TITLE, &e);
            return false;
        }
    };
    let label = params.label();
    loop {
        busy(ctx.ui, format!("Connecting to {label}…"));
        match conn::connect(&params) {
            Ok(db) => {
                let p = provider::DbProvider::new(params, db);
                ctx.active.push_provider(Box::new(p), format!("{label}:"));
                return true;
            }
            Err(e) if e.password => {
                // No password, or a wrong one: ask (the saved one is not changed).
                let retry = if params.password.is_empty() && !e.message.contains("ERROR") { None } else { Some(e.message.as_str()) };
                let Some(pw) = ask_password(ctx.ui, &format!("Password of {} for {label}:", params.user), retry) else {
                    return false;
                };
                params.password = pw;
            }
            Err(e) => {
                error(ctx.ui, TITLE, &format!("Cannot connect to {label}\n{}", e.message));
                return false;
            }
        }
    }
}

/// End-to-end test against a real server. Runs only when `GM_DB_TEST` is
/// `driver|host[:port]|db|user|password` (driver: `mariadb` or `postgresql`).
/// It creates and drops the tables `gm_e2e_parent` and `gm_e2e_child` and
/// the view `gm_e2e_view`.
#[cfg(test)]
mod e2e {
    use super::*;
    use gm_core::gm_vfs::{CancelToken, VfsProvider};

    #[test]
    fn database_round_trip() {
        let Ok(spec) = std::env::var("GM_DB_TEST") else { return };
        let f: Vec<&str> = spec.splitn(5, '|').collect();
        let driver = if f[0] == "mariadb" { Driver::MariaDb } else { Driver::PostgreSql };
        let saved = SavedDb { host: f[1].into(), db: f[2].into(), user: f[3].into(), password: String::new(), driver };
        let mut params = config::resolve(&saved).unwrap();

        // Without the password the server asks for one (unless it trusts us).
        if let Err(e) = conn::connect(&params) {
            assert!(e.password, "{}", e.message);
        }
        params.password = "certainly wrong".into();
        if let Err(e) = conn::connect(&params) {
            assert!(e.password, "{}", e.message);
        }
        params.password = f[4].into();
        let db = conn::connect(&params).map_err(|e| e.message).unwrap();
        let mut p = provider::DbProvider::new(params, db);

        let setup = "DROP VIEW IF EXISTS gm_e2e_view; DROP TABLE IF EXISTS gm_e2e_child; DROP TABLE IF EXISTS gm_e2e_parent;\n\
             CREATE TABLE gm_e2e_parent (id INT PRIMARY KEY, name VARCHAR(20) NOT NULL);\n\
             CREATE TABLE gm_e2e_child (id INT PRIMARY KEY, parent_id INT REFERENCES gm_e2e_parent(id), note VARCHAR(50));\n\
             CREATE INDEX gm_e2e_child_note ON gm_e2e_child (note);\n\
             INSERT INTO gm_e2e_parent VALUES (1, 'one;two'), (2, 'two');\n\
             CREATE VIEW gm_e2e_view AS SELECT id, name FROM gm_e2e_parent WHERE id > 1;";
        let dir = std::env::temp_dir().join(format!("gm-db-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("setup.sql");
        std::fs::write(&script, setup).unwrap();
        p.put_files(std::slice::from_ref(&script), &mut |_, _| true).unwrap();
        let mut res = p.take_result_file().unwrap();
        res.download(&dir.join("res.txt")).unwrap();
        let res_text = std::fs::read_to_string(dir.join("res.txt")).unwrap();
        assert!(res_text.contains("OK, 2 rows affected"), "{res_text}");

        let names: Vec<String> = p.list(&CancelToken::new()).unwrap().into_iter().map(|e| e.name).collect();
        for n in ["gm_e2e_parent", "gm_e2e_child", "gm_e2e_view"] {
            assert!(names.contains(&n.to_string()), "{names:?}");
        }

        // F3: the structure; F4: the rows.
        let read = |f: &mut Box<dyn gm_core::gm_vfs::RemoteFile>, name: &str| {
            let path = dir.join(name);
            f.download(&path).unwrap();
            std::fs::read_to_string(&path).unwrap()
        };
        let s = read(&mut p.item_file("gm_e2e_child", false).unwrap(), "s.txt");
        assert!(s.contains("parent_id") && s.contains("gm_e2e_child_note") && s.contains("gm_e2e_parent"), "{s}");
        let s = read(&mut p.item_file("gm_e2e_parent", false).unwrap(), "p.txt");
        assert!(s.contains("Referenced by") && s.contains("gm_e2e_child"), "{s}");
        let s = read(&mut p.item_file("gm_e2e_view", false).unwrap(), "v.txt");
        assert!(s.contains("Script") && s.to_lowercase().contains("where"), "{s}");
        let mut rows = p.item_file("gm_e2e_parent", true).unwrap();
        assert!(!rows.write_back());
        let s = read(&mut rows, "r.txt");
        assert!(s.contains("one;two") && s.contains("2 rows"), "{s}");

        // A click counts the rows.
        assert!(p.clicked("gm_e2e_parent").unwrap());
        let e = p.list(&CancelToken::new()).unwrap().into_iter().find(|e| e.name == "gm_e2e_parent").unwrap();
        assert_eq!(e.custom[1], "2");
        assert_eq!(p.command_line_name("gm_e2e_parent").unwrap(), "gm_e2e_parent");

        // The command line, with an error that stops the execution.
        let mut f = p.execute_to_file("select count(*) as n from gm_e2e_child; select * from no_such_table; select 1").unwrap().unwrap();
        let s = read(&mut f, "c.txt");
        assert!(s.contains("| n ") && s.contains("Execution stopped") && !s.contains("SQL> select 1"), "{s}");

        // F5: the first rows into a folder.
        p.get_files(&["gm_e2e_parent".to_string()], &dir, &mut |_, _| true).unwrap();
        assert!(std::fs::read_to_string(dir.join("gm_e2e_parent.txt")).unwrap().contains("one;two"));
        assert!(!p.delete_after_move());

        // The server closes the connection: the next operation connects again.
        let kill = match driver {
            Driver::PostgreSql => "select pg_terminate_backend(pg_backend_pid())",
            Driver::MariaDb => "kill connection connection_id()",
        };
        let _ = p.execute_to_file(kill);
        assert!(p.clicked("gm_e2e_child").unwrap());

        // F8.
        assert!(p.delete_question(&["gm_e2e_view".to_string()]).unwrap().contains("Drop the view"));
        p.delete(&["gm_e2e_view".into(), "gm_e2e_child".into(), "gm_e2e_parent".into()]).unwrap();
        let names: Vec<String> = p.list(&CancelToken::new()).unwrap().into_iter().map(|e| e.name).collect();
        assert!(!names.iter().any(|n| n.starts_with("gm_e2e_")), "{names:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

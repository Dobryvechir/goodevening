//! SSH + SFTP panel: Alt+F1/Alt+F2 → "SSH + SFTP" lists the saved
//! connections (Enter opens, Ins adds, F4 edits, Del deletes). Each has a
//! machine name (`[user@]host[:port][/remote/folder]`), an optional user
//! and an optional password.
//!
//! Connecting follows the rules of the `ssh` program: `~/.ssh/config`,
//! the local user name when none is given, `known_hosts` (an unknown host
//! is confirmed by the user, a changed key refuses the connection), and
//! without a password the ssh-agent and the key files in `~/.ssh`. When
//! nothing works, or the password is wrong, a password dialog opens.
//!
//! The remote folder is then shown in the panel like a local one: copy and
//! move use SFTP, F3/F4 work on a local copy that is uploaded again when
//! the editor saves it, command lines and Enter on a file run on the
//! remote machine through SSH. Problems are reported in red message boxes.
//! Also `ssh:[user@]host[/folder]` on the command line.
#![forbid(unsafe_code)]

mod config;
mod conn;
mod provider;
mod shell;

use config::SavedConn;
use conn::{HostKey, KeyResult};
use gm_core::gm_term::{KeyChord, KeyCode};
use gm_core::gm_ui::dialog::DialogBuilder;
use gm_core::gm_ui::message::{draw_box, error};
use gm_core::gm_ui::{MenuItem, MenuResult, Ui, VMenu, message, message_warn};
use gm_core::{CommandSpec, Ctx, DriveItem, Module};
use std::sync::Arc;

const TITLE: &str = "SSH + SFTP";

#[derive(Default)]
pub struct SshPanel;

impl Module for SshPanel {
    fn id(&self) -> &'static str {
        "ssh_panel"
    }
    fn title(&self) -> String {
        TITLE.into()
    }
    fn commands(&self) -> Vec<CommandSpec> {
        vec![CommandSpec { id: "ssh", title: "&SSH + SFTP".into() }]
    }
    fn run_command(&self, _id: &str, ctx: &mut Ctx) {
        connections_menu(ctx);
    }
    fn prefixes(&self) -> &[&'static str] {
        &["ssh", "sftp"]
    }
    fn open_prefix(&self, ctx: &mut Ctx, _prefix: &str, arg: &str) -> bool {
        let arg = arg.trim().trim_start_matches("//");
        if arg.is_empty() {
            connections_menu(ctx);
        } else {
            open(ctx, &SavedConn { machine: arg.to_string(), ..Default::default() });
        }
        true
    }
    fn drive_menu_items(&self) -> Vec<DriveItem> {
        vec![DriveItem { id: 0, title: TITLE.into() }]
    }
    fn open_drive_item(&self, _id: usize, ctx: &mut Ctx) {
        connections_menu(ctx);
    }
}

fn busy(ui: &mut Ui, line: String) {
    let bg = ui.term.last_frame().clone();
    draw_box(ui, &bg, TITLE, &[line]);
}

/// The saved connections: Enter opens, Ins (or the last item) adds, F4 edits, Del deletes.
fn connections_menu(ctx: &mut Ctx) {
    let path = config::store_path();
    let mut list = match config::load(&path) {
        Ok(l) => l,
        Err(e) => {
            error(ctx.ui, TITLE, &format!("Cannot read the saved connections\n{e}"));
            return;
        }
    };
    let mut sel = 0;
    loop {
        let mut items: Vec<MenuItem> = list
            .iter()
            .map(|c| {
                let user = if c.user.is_empty() { String::new() } else { c.user.clone() };
                let pw = if c.password.is_empty() { "" } else { " ●" };
                MenuItem::with_hint(c.machine.replace('&', "&&"), format!("{user}{pw}").replace('&', "&&"))
            })
            .collect();
        if !items.is_empty() {
            items.push(MenuItem::separator());
        }
        let add_index = items.len();
        items.push(MenuItem::new("Add a new connection…"));
        let mut m = VMenu::new(TITLE, items);
        m.bottom = "Enter Ins F4 Del".into();
        m.min_width = 50;
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
                    let q = format!("Delete the connection\n{}?", list[i].machine);
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

fn persist(ui: &mut Ui, path: &std::path::Path, list: &[SavedConn]) {
    if let Err(e) = config::save(path, list) {
        error(ui, TITLE, &format!("Cannot save the connections\n{e}"));
    }
}

/// The add/edit dialog; None if cancelled.
fn edit_dialog(ui: &mut Ui, init: &SavedConn) -> Option<SavedConn> {
    let mut cur = init.clone();
    loop {
        let mut b = DialogBuilder::new(TITLE, 70);
        b.text("&Machine name: host[:port][/remote/folder]");
        let machine = b.edit(&cur.machine, Some("SshMachine"));
        b.text(format!("&User (optional; empty: ~/.ssh/config or \"{}\")", config::local_user()));
        let user = b.edit(&cur.user, Some("SshUser"));
        b.text("&Password (optional; empty: SSH keys, then ask)");
        let pw = b.password(&cur.password);
        b.separator();
        b.ok_cancel();
        let mut d = b.build();
        let cancel = d.items.len() - 1;
        let r = d.run(ui)?;
        if r == cancel {
            return None;
        }
        cur = SavedConn { machine: d.text(machine).trim().to_string(), user: d.text(user).trim().to_string(), password: d.text(pw) };
        match config::parse_machine(&cur.machine) {
            Ok(_) => return Some(cur),
            Err(e) => error(ui, TITLE, &e),
        }
    }
}

fn ask_secret(ui: &mut Ui, title: &str, prompt: &str, retry: Option<&str>) -> Option<String> {
    let mut b = DialogBuilder::new(title, 64);
    if let Some(r) = retry {
        b.text(r);
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

/// Connects, authenticates and shows the remote folder in the active panel.
/// Returns false if it did not happen (cancelled or failed; the reason was shown).
fn open(ctx: &mut Ctx, c: &SavedConn) -> bool {
    let target = match config::resolve(c) {
        Ok(t) => t,
        Err(e) => {
            error(ctx.ui, TITLE, &e);
            return false;
        }
    };
    let label = target.label();
    let fail = |ui: &mut Ui, what: &str, e: &dyn std::fmt::Display| {
        error(ui, TITLE, &format!("{what} {label}\n{e}"));
        false
    };
    busy(ctx.ui, format!("Connecting to {label}…"));
    let mut p = match conn::connect(&target) {
        Ok(p) => p,
        Err(e) => return fail(ctx.ui, "Cannot connect to", &e),
    };
    // An unknown server: ask like ssh does before sending any credentials.
    if let HostKey::Unknown(key) = &p.host_key {
        let who = if target.alias == target.host { target.host.clone() } else { format!("{} ({})", target.alias, target.host) };
        let q = format!(
            "The authenticity of host '{who}' can't be established.\nKey fingerprint: {}\nAre you sure you want to continue connecting?",
            conn::fingerprint(key)
        );
        if message(ctx.ui, TITLE, &q, &["Yes", "No"]) != Some(0) {
            p.disconnect();
            return false;
        }
        if let Err(e) = conn::learn_host_key(&target.host, target.port, key) {
            error(ctx.ui, TITLE, &format!("Cannot add the host to known_hosts\n{e}"));
        }
    }
    let user = target.user.clone();
    busy(ctx.ui, format!("Authenticating as {label}…"));
    let mut ok = false;
    let mut retry_text: Option<&str> = None;
    if !c.password.is_empty() {
        match p.try_password(&user, &c.password) {
            Ok(true) => ok = true,
            Ok(false) => retry_text = Some("Permission denied, please try again."),
            Err(e) if !p.is_closed() => return fail(ctx.ui, "Cannot log in to", &e),
            Err(_) => {}
        }
    } else {
        ok = p.try_agent(&user);
        for f in &target.identity_files {
            if ok || p.is_closed() {
                break;
            }
            let mut passphrase: Option<String> = None;
            let mut wrong = false;
            loop {
                match p.try_key_file(&user, f, passphrase.as_deref()) {
                    KeyResult::Accepted => {
                        ok = true;
                        break;
                    }
                    KeyResult::Encrypted => {
                        let prompt = format!("Enter passphrase for key '{}':", f.display());
                        let retry = if wrong { Some("Wrong passphrase, please try again.") } else { None };
                        match ask_secret(ctx.ui, TITLE, &prompt, retry) {
                            Some(s) => passphrase = Some(s),
                            None => break,
                        }
                        wrong = true;
                        busy(ctx.ui, format!("Authenticating as {label}…"));
                    }
                    KeyResult::Rejected | KeyResult::Missing | KeyResult::Unusable => break,
                }
            }
        }
    }
    // Nothing worked: ask for the password until it is right or cancelled.
    while !ok {
        if p.is_closed() {
            // Too many failed attempts close the connection: connect again.
            busy(ctx.ui, format!("Connecting to {label}…"));
            p = match conn::connect(&target) {
                Ok(p) => p,
                Err(e) => return fail(ctx.ui, "Cannot connect to", &e),
            };
        }
        let Some(pw) = ask_secret(ctx.ui, TITLE, &format!("{label}'s password:"), retry_text) else {
            p.disconnect();
            return false;
        };
        busy(ctx.ui, format!("Authenticating as {label}…"));
        match p.try_password(&user, &pw) {
            Ok(true) => ok = true,
            Ok(false) => retry_text = Some("Permission denied, please try again."),
            Err(e) if !p.is_closed() => return fail(ctx.ui, "Cannot log in to", &e),
            Err(_) => retry_text = Some("The server closed the connection, please try again."),
        }
    }
    busy(ctx.ui, format!("Starting SFTP on {label}…"));
    let conn = match p.finish(target) {
        Ok(c) => Arc::new(c),
        Err(e) => return fail(ctx.ui, "Cannot start SFTP on", &e),
    };
    let provider = match provider::SshProvider::new(conn) {
        Ok(p) => p,
        Err(e) => return fail(ctx.ui, "Cannot open the folder on", &e),
    };
    ctx.active.push_provider(Box::new(provider), format!("{label}:"));
    true
}

/// End-to-end test against a real server. Runs only when `GM_SSH_TEST` is
/// `host:port:user:private_key_file` (the key must be accepted by the server).
#[cfg(test)]
mod e2e {
    use super::*;
    use gm_core::gm_vfs::{CancelToken, ChangeDir, VfsProvider};

    #[test]
    fn sftp_round_trip() {
        let Ok(spec) = std::env::var("GM_SSH_TEST") else { return };
        let mut parts = spec.splitn(4, ':');
        let (host, port, user, key) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
        let c = SavedConn { machine: format!("{host}:{port}"), user: user.into(), password: String::new() };
        let t = config::resolve(&c).unwrap();
        assert_eq!((t.host.as_str(), t.port, t.user.as_str()), (host, port.parse().unwrap(), user));

        let mut p = conn::connect(&t).unwrap();
        assert!(!matches!(p.host_key, HostKey::Changed { .. }));
        // A missing key file is skipped; a wrong password is refused.
        assert!(matches!(p.try_key_file(user, std::path::Path::new("Z:/no/such/key"), None), KeyResult::Missing));
        assert!(matches!(p.try_password(user, "wrong"), Ok(false) | Err(_)));
        if p.is_closed() {
            p = conn::connect(&t).unwrap();
        }
        assert!(matches!(p.try_key_file(user, std::path::Path::new(key), None), KeyResult::Accepted));
        let conn = Arc::new(p.finish(t).unwrap());
        let mut pr = provider::SshProvider::new(conn.clone()).unwrap();
        let home = pr.current_dir();

        // A scratch folder on the server.
        let scratch = format!("gm-ssh-e2e-{}", std::process::id());
        pr.mkdir(&format!("{scratch}/sub")).unwrap();
        assert!(matches!(pr.change_dir(&scratch).unwrap(), ChangeDir::Changed));
        assert!(pr.current_dir().ends_with(&scratch));

        // Upload a local tree.
        let local = std::env::temp_dir().join(format!("gm-ssh-e2e-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&local);
        std::fs::create_dir_all(local.join("tree/inner")).unwrap();
        std::fs::write(local.join("tree/a.txt"), "alpha").unwrap();
        let big: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(local.join("tree/inner/big.bin"), &big).unwrap();
        let mut calls = 0;
        pr.put_files(&[local.join("tree")], &mut |_, _| {
            calls += 1;
            true
        })
        .unwrap();
        assert!(calls > 2);
        let names: Vec<String> = pr.list(&CancelToken::new()).unwrap().into_iter().map(|e| e.name).collect();
        assert!(names.contains(&"tree".to_string()) && names.contains(&"sub".to_string()), "{names:?}");

        // Rename (Shift+F6), then download the tree again.
        pr.rename("tree", "tree2").unwrap();
        let back = local.join("back");
        std::fs::create_dir_all(&back).unwrap();
        pr.get_files(&["tree2".to_string()], &back, &mut |_, _| true).unwrap();
        assert_eq!(std::fs::read_to_string(back.join("tree2/a.txt")).unwrap(), "alpha");
        assert_eq!(std::fs::read(back.join("tree2/inner/big.bin")).unwrap(), big);

        // Edit through a local copy: download, change, upload.
        pr.change_dir("tree2").unwrap();
        let mut f = pr.remote_file("a.txt").unwrap();
        assert_eq!(f.name(), "a.txt");
        let copy = local.join("copy.txt");
        f.download(&copy).unwrap();
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "alpha");
        std::fs::write(&copy, "changed text").unwrap();
        f.upload(&copy).unwrap();
        let mut r = pr.open_read("a.txt").unwrap();
        let mut s = String::new();
        std::io::Read::read_to_string(&mut r, &mut s).unwrap();
        assert_eq!(s, "changed text");
        assert!(pr.change_dir("a.txt").is_err());

        // `cd ~` and absolute paths, then delete everything.
        pr.change_dir("~").unwrap();
        assert_eq!(pr.current_dir(), home);
        pr.delete(std::slice::from_ref(&scratch)).unwrap();
        assert!(!pr.list(&CancelToken::new()).unwrap().iter().any(|e| e.name == scratch));
        let _ = std::fs::remove_dir_all(&local);

        // A command through SSH (no terminal here, so through a plain channel).
        let out = conn::rt().block_on(async {
            let mut ch = conn.handle.channel_open_session().await.unwrap();
            // Like shell::run: a pseudo-terminal, then `cd <folder> && <line>`.
            ch.request_pty(false, "xterm-256color", 80, 25, 0, 0, &[]).await.unwrap();
            let line = format!("cd {} && echo gm-ok", provider::sh_quote(&home));
            ch.exec(true, line.as_bytes()).await.unwrap();
            let mut out = Vec::new();
            while let Some(m) = ch.wait().await {
                if let russh::ChannelMsg::Data { data } = m {
                    out.extend_from_slice(&data);
                }
            }
            String::from_utf8_lossy(&out).into_owned()
        });
        assert!(out.contains("gm-ok"), "{out}");
    }
}

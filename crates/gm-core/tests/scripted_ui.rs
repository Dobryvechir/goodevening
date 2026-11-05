//! Scripted UI tests: key sequences are fed into a headless `App`, then
//! the rendered screen and the file system are checked.

use gm_config::{Settings, StateDb};
use gm_core::{App, DbHistory, Keymap, Module};
use gm_term::{Event, KeyChord, KeyCode, Mods, Term};
use gm_ui::Ui;
use std::path::{Path, PathBuf};
use std::rc::Rc;

struct Fixture {
    app: App,
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn fixture(name: &str, modules: Vec<Box<dyn Module>>) -> Fixture {
    let root = std::env::temp_dir().join(format!("gm-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let left = root.join("left");
    let right = root.join("right");
    std::fs::create_dir_all(left.join("sub")).unwrap();
    std::fs::create_dir_all(&right).unwrap();
    std::fs::write(left.join("alpha.txt"), "Alpha content\nsecond line\n").unwrap();
    std::fs::write(left.join("beta.rs"), "fn main() {}\n").unwrap();
    let mut settings = Settings::default();
    settings.panels.auto_refresh = false;
    settings.interface.clock = false;
    settings.system.delete_to_trash = false;
    settings.system.auto_save_setup = false;
    let db = Rc::new(StateDb::in_memory());
    let ui = Ui::new(Term::headless(110, 32), Box::new(DbHistory(db.clone())));
    let keymap = Keymap::load(&root.join("no-keymap.toml"));
    let app = App::new(ui, settings, root.join("settings.toml"), db, modules, keymap, (Some(left), Some(right)));
    Fixture { app, root }
}

fn key(code: KeyCode, mods: Mods) -> Event {
    Event::Key(KeyChord::new(code, mods))
}

fn plain(code: KeyCode) -> Event {
    key(code, Mods::NONE)
}

fn text(s: &str) -> Vec<Event> {
    s.chars().map(|c| plain(KeyCode::Char(c))).collect()
}

impl Fixture {
    /// Queues `script` as input for the modal loops, then sends `trigger`.
    fn run(&mut self, trigger: Event, script: Vec<Event>) {
        for e in script {
            self.app.ui.term.push_event(e);
        }
        self.app.handle_event(trigger);
        self.app.render().unwrap();
    }

    fn screen(&mut self) -> String {
        self.app.render().unwrap();
        self.app.ui.term.screen_text()
    }

    fn left(&self) -> PathBuf {
        self.root.join("left")
    }

    fn right(&self) -> PathBuf {
        self.root.join("right")
    }

    fn cursor_to(&mut self, name: &str) {
        assert!(self.app.left.files.set_cursor_name(name), "{name} not in panel");
    }
}

#[test]
fn panels_render_like_far() {
    let mut f = fixture("render", vec![]);
    let s = f.screen();
    assert!(s.contains("alpha.txt"), "{s}");
    assert!(s.contains("beta.rs"));
    assert!(s.contains("sub"));
    assert!(s.contains("Name"), "column titles: {s}");
    assert!(s.contains("╔") && s.contains("╝"), "double frame");
    assert!(s.contains("1Help"), "key bar: {s}");
    assert!(s.contains("left>"), "prompt: {s}");
    println!("{s}");
}

#[test]
fn make_folder_copy_and_delete() {
    let mut f = fixture("ops", vec![]);
    // F7: create a folder.
    let mut script = text("newdir");
    script.push(plain(KeyCode::Enter));
    f.run(plain(KeyCode::F(7)), script);
    assert!(f.left().join("newdir").is_dir());
    assert_eq!(f.app.left.files.current_name().as_deref(), Some("newdir"));

    // F5: copy alpha.txt to the right panel (accept the default target).
    f.cursor_to("alpha.txt");
    f.run(plain(KeyCode::F(5)), vec![plain(KeyCode::Enter)]);
    assert_eq!(std::fs::read_to_string(f.right().join("alpha.txt")).unwrap(), "Alpha content\nsecond line\n");
    assert!(f.screen().contains("alpha.txt"));

    // Shift+Del: delete permanently after confirmation.
    f.cursor_to("beta.rs");
    f.run(key(KeyCode::Delete, Mods::SHIFT), vec![plain(KeyCode::Enter)]);
    assert!(!f.left().join("beta.rs").exists());
}

#[test]
fn move_with_selection() {
    let mut f = fixture("move", vec![]);
    f.cursor_to("alpha.txt");
    f.app.handle_event(plain(KeyCode::Insert));
    f.cursor_to("beta.rs");
    f.app.handle_event(plain(KeyCode::Insert));
    assert_eq!(f.app.left.files.selected_count(), 2);
    f.run(plain(KeyCode::F(6)), vec![plain(KeyCode::Enter)]);
    assert!(f.right().join("alpha.txt").exists() && f.right().join("beta.rs").exists());
    assert!(!f.left().join("alpha.txt").exists());
}

#[test]
fn viewer_and_editor() {
    let mut f = fixture("view", vec![]);
    f.cursor_to("alpha.txt");
    f.app.handle_event(plain(KeyCode::F(3)));
    let s = f.screen();
    assert!(s.contains("Alpha content"), "{s}");
    assert!(s.contains("UTF-8"), "status line: {s}");
    f.app.handle_event(plain(KeyCode::Esc));
    assert!(f.app.current.is_none());

    // F4: type at the start, save with F2, quit with Esc.
    f.app.handle_event(plain(KeyCode::F(4)));
    for e in text(">> ") {
        f.app.handle_event(e);
    }
    let s = f.screen();
    assert!(s.contains(">> Alpha content"), "{s}");
    assert!(s.contains('*'), "modified mark");
    f.app.handle_event(plain(KeyCode::F(2)));
    f.app.handle_event(plain(KeyCode::Esc));
    assert!(f.app.current.is_none());
    assert_eq!(std::fs::read_to_string(f.left().join("alpha.txt")).unwrap(), ">> Alpha content\nsecond line\n");
}

#[test]
fn editor_asks_before_losing_changes() {
    let mut f = fixture("ask", vec![]);
    f.cursor_to("beta.rs");
    f.app.handle_event(plain(KeyCode::F(4)));
    f.app.handle_event(plain(KeyCode::Char('x')));
    // Esc → "Save?" → choose "No" (Right moves to the second button).
    f.run(plain(KeyCode::Esc), vec![plain(KeyCode::Right), plain(KeyCode::Enter)]);
    assert!(f.app.current.is_none());
    assert_eq!(std::fs::read_to_string(f.left().join("beta.rs")).unwrap(), "fn main() {}\n");
}

#[test]
fn command_line_cd_and_history() {
    let mut f = fixture("cd", vec![]);
    for e in text("cd sub") {
        f.app.handle_event(e);
    }
    assert!(f.screen().contains("cd sub"));
    f.app.handle_event(plain(KeyCode::Enter));
    assert_eq!(f.app.left.files.local_dir().unwrap(), f.left().join("sub"));
    f.app.handle_event(key(KeyCode::PageUp, Mods::CTRL));
    assert_eq!(f.app.left.files.local_dir().unwrap(), f.left());
    assert_eq!(f.app.left.files.current_name().as_deref(), Some("sub"));
    // Ctrl+E recalls the last command.
    f.app.handle_event(key(KeyCode::Char('e'), Mods::CTRL));
    assert_eq!(f.app.cmdline.text(), "cd sub");
}

#[test]
fn menus_and_panels() {
    let mut f = fixture("menu", vec![]);
    // F9 opens the menu bar; the queued Esc closes it.
    f.run(plain(KeyCode::F(9)), vec![]);
    // Tab switches panels; Ctrl+L shows the info panel in the passive one.
    f.app.handle_event(plain(KeyCode::Tab));
    assert!(!f.app.active_left);
    f.app.handle_event(plain(KeyCode::Tab));
    f.app.handle_event(key(KeyCode::Char('l'), Mods::CTRL));
    let s = f.screen();
    assert!(s.contains("Information"), "{s}");
    f.app.handle_event(key(KeyCode::Char('l'), Mods::CTRL));
    // Ctrl+F6: sort by size (the largest file first).
    f.app.handle_event(key(KeyCode::F(6), Mods::CTRL));
    let names: Vec<String> = f.app.left.files.entries.iter().filter(|e| !e.is_dir).map(|e| e.name.clone()).collect();
    assert_eq!(names, ["alpha.txt", "beta.rs"]);
    // Ctrl+1: brief mode, three columns.
    f.app.handle_event(key(KeyCode::Char('1'), Mods::CTRL));
    assert_eq!(f.app.left.files.mode, 1);
}

#[test]
fn terminal_position_menu() {
    let mut f = fixture("termpos", vec![]);
    f.app.ui.term.recorded = Some(Vec::new());
    // F9 → Right, Right (Commands) → T (Terminal position); Esc closes the
    // submenu without moving the window. The resize forces a frame inside it.
    let script = vec![plain(KeyCode::Right), plain(KeyCode::Right), plain(KeyCode::Char('t')), Event::Resize(110, 32), plain(KeyCode::Esc)];
    f.run(plain(KeyCode::F(9)), script);
    let frames = f.app.ui.term.recorded.replace(Vec::new()).unwrap_or_default();
    let menu = frames.iter().find(|fr| fr.contains("Terminal position") && fr.contains("Right bottom"));
    let menu = menu.unwrap_or_else(|| panic!("no Terminal position menu in {} frames", frames.len()));
    for item in ["Full screen", "Left half", "Right half", "Top half", "Bottom half", "Left top", "Left bottom", "Right top"] {
        assert!(menu.contains(item), "{item} missing:
{menu}");
    }
}

#[test]
fn multiple_terminals_menu() {
    let mut f = fixture("multiterm", vec![]);
    f.app.ui.term.recorded = Some(Vec::new());
    // F9 → Right, Right (Commands) → E (Multiple terminals); Esc closes the
    // submenu without moving the window or starting anything.
    let script = vec![plain(KeyCode::Right), plain(KeyCode::Right), plain(KeyCode::Char('e')), Event::Resize(110, 32), plain(KeyCode::Esc)];
    f.run(plain(KeyCode::F(9)), script);
    let frames = f.app.ui.term.recorded.replace(Vec::new()).unwrap_or_default();
    let menu = frames.iter().find(|fr| fr.contains("Multiple terminals") && fr.contains("4 terminals"));
    let menu = menu.unwrap_or_else(|| panic!("no Multiple terminals menu in {} frames", frames.len()));
    for item in ["Left and right terminals", "Top and bottom terminals"] {
        assert!(menu.contains(item), "{item} missing:\n{menu}");
    }
}

#[test]
fn execute_in_other_terminal_menu() {
    let mut f = fixture("exectermmenu", vec![]);
    f.app.ui.term.recorded = Some(Vec::new());
    // F9 → Right, Right (Commands) → X (Execute in other terminal); Esc
    // closes the submenu without starting anything.
    let script = vec![plain(KeyCode::Right), plain(KeyCode::Right), plain(KeyCode::Char('x')), Event::Resize(110, 32), plain(KeyCode::Esc)];
    f.run(plain(KeyCode::F(9)), script);
    let frames = f.app.ui.term.recorded.replace(Vec::new()).unwrap_or_default();
    let menu = frames.iter().find(|fr| fr.contains("Execute in other terminal") && fr.contains("Terminal at left"));
    let menu = menu.unwrap_or_else(|| panic!("no Execute in other terminal menu in {} frames", frames.len()));
    for item in ["Terminal at right", "Terminal at bottom", "Terminal at top"] {
        assert!(menu.contains(item), "{item} missing:\n{menu}");
    }
    assert!(f.app.server_queue.is_none());
}

#[test]
fn execute_in_other_terminal_disconnect() {
    let mut f = fixture("exectermdisc", vec![]);
    let queue = std::env::temp_dir().join(format!("gm-test-queue-{}.txt", std::process::id()));
    std::fs::write(&queue, "echo before\n").unwrap();
    f.app.server_queue = Some(queue.clone());
    // F9 → Commands → X; the only item is "Disconnect other terminal",
    // Enter chooses it: it queues exit_gm_server for the server and leaves
    // server execution mode.
    let script = vec![plain(KeyCode::Right), plain(KeyCode::Right), plain(KeyCode::Char('x')), plain(KeyCode::Enter)];
    f.run(plain(KeyCode::F(9)), script);
    assert!(f.app.server_queue.is_none());
    assert_eq!(std::fs::read_to_string(&queue).unwrap(), "echo before\nexit_gm_server\n");
    let _ = std::fs::remove_file(&queue);
}

#[test]
fn find_file_goto() {
    let mut f = fixture("find", vec![]);
    std::fs::write(f.left().join("sub/needle.txt"), "hay").unwrap();
    // Alt+F7: mask "needle*", Enter starts; in the results Enter = "Go to".
    let mut script = vec![key(KeyCode::Char('y'), Mods::CTRL)];
    script.extend(text("needle*"));
    script.push(plain(KeyCode::Enter));
    // Give the worker time: the results window drains on every event.
    for _ in 0..50 {
        script.push(Event::Resize(110, 32));
    }
    script.push(plain(KeyCode::Enter));
    f.run(key(KeyCode::F(7), Mods::ALT), script);
    assert_eq!(f.app.left.files.local_dir().unwrap(), f.left().join("sub"));
    assert_eq!(f.app.left.files.current_name().as_deref(), Some("needle.txt"));
}

#[test]
fn archive_as_panel() {
    let mut f = fixture("zip", vec![Box::new(mod_archive::Archive)]);
    // Shift+F1 on alpha.txt creates alpha.zip; Enter opens it as a panel.
    f.cursor_to("alpha.txt");
    f.run(key(KeyCode::F(1), Mods::SHIFT), vec![plain(KeyCode::Enter)]);
    assert!(f.left().join("alpha.zip").is_file());
    f.cursor_to("alpha.zip");
    f.app.handle_event(plain(KeyCode::Enter));
    let s = f.screen();
    assert!(s.contains("zip:"), "{s}");
    assert!(f.app.left.files.entries.iter().any(|e| e.name == "alpha.txt"));
    // F5 extracts into the right panel.
    f.app.left.files.set_cursor_name("alpha.txt");
    f.run(plain(KeyCode::F(5)), vec![plain(KeyCode::Enter)]);
    assert!(f.right().join("alpha.txt").is_file());
    // ".." leaves the archive with the cursor on it.
    f.app.left.files.home();
    f.app.handle_event(plain(KeyCode::Enter));
    assert!(f.app.left.files.is_local());
    assert_eq!(f.app.left.files.current_name().as_deref(), Some("alpha.zip"));
}

#[allow(dead_code)]
fn exists(p: &Path) -> bool {
    p.exists()
}

/// Prints the main screens and dialogs (run with --nocapture to look at them).
#[test]
fn gallery() {
    let mut f = fixture("gallery", vec![Box::new(mod_archive::Archive)]);
    f.app.ui.term.recorded = Some(Vec::new());
    let shots: Vec<(&str, Event)> = vec![
        ("F7 make folder", plain(KeyCode::F(7))),
        ("F5 copy", plain(KeyCode::F(5))),
        ("F8 delete", plain(KeyCode::F(8))),
        ("F9 menu", plain(KeyCode::F(9))),
        ("Alt+F1 drives", key(KeyCode::F(1), Mods::ALT)),
        ("Alt+F7 find", key(KeyCode::F(7), Mods::ALT)),
        ("F1 help", plain(KeyCode::F(1))),
    ];
    for (title, ev) in shots {
        // Resize forces one frame inside the modal before the final Esc.
        f.run(ev, vec![Event::Resize(110, 32)]);
        let frames = f.app.ui.term.recorded.replace(Vec::new()).unwrap_or_default();
        if let Some(fr) = frames.iter().rev().nth(1).or(frames.last()) {
            println!("===== {title} =====\n{fr}");
        }
    }
    f.cursor_to("alpha.txt");
    f.app.handle_event(plain(KeyCode::F(3)));
    println!("===== F3 viewer =====\n{}", f.screen());
    f.app.handle_event(plain(KeyCode::Esc));
    f.app.handle_event(plain(KeyCode::F(4)));
    println!("===== F4 editor =====\n{}", f.screen());
    f.app.handle_event(plain(KeyCode::Esc));
    f.app.handle_event(key(KeyCode::Char('l'), Mods::CTRL));
    println!("===== Ctrl+L info =====\n{}", f.screen());
    f.app.handle_event(key(KeyCode::Char('l'), Mods::CTRL));
    f.app.handle_event(key(KeyCode::Char('3'), Mods::CTRL));
    println!("===== Ctrl+3 full =====\n{}", f.screen());
}

// ----- F9 → Commands → Encoded archives -----

/// A key file with all three operations, as `codec.py` reads it.
const ENC_KEY: &str = "hjhiyuy798797*0003000000010002hjhljhlh jhkjjhj+abcdef45hjhjkhkhkjk$#@!%^456577979ab4jkjljk ipipp+5678adgjggjg";

fn codec_file(mode: gm_core::codec::Mode, key: &Path, password: Option<&str>, input: &Path, output: &Path) {
    let ops = gm_core::codec::load_key(key, password.map(str::as_bytes)).unwrap();
    gm_core::codec::transform_file(mode, &ops, input, output).unwrap();
}

/// Writes `plain` encoded as `gm --e` / `gm --pe` would into `out`.
fn encode_bytes(f: &Fixture, key: &Path, password: Option<&str>, plain: &[u8], out: &Path) {
    let tmp = f.root.join("plain.tmp");
    std::fs::write(&tmp, plain).unwrap();
    codec_file(gm_core::codec::Mode::Encode, key, password, &tmp, out);
    std::fs::remove_file(&tmp).unwrap();
}

/// Decodes `file` as `gm --d` / `gm --pd` would.
fn decode_bytes(f: &Fixture, key: &Path, password: Option<&str>, file: &Path) -> Vec<u8> {
    let tmp = f.root.join("decoded.tmp");
    codec_file(gm_core::codec::Mode::Decode, key, password, file, &tmp);
    let v = std::fs::read(&tmp).unwrap();
    std::fs::remove_file(&tmp).unwrap();
    v
}

/// Adds an encoded archive configuration with a plain key file (or one
/// masked with `password`, as `--pe`/`--pd` expect).
fn add_config(f: &mut Fixture, new_ext: &str, orig_ext: &str, password: Option<&str>) -> PathBuf {
    let key = f.root.join(format!("{new_ext}.key"));
    let mut bytes = ENC_KEY.as_bytes().to_vec();
    if let Some(p) = password {
        gm_core::codec::unmask_key(&mut bytes, p.as_bytes());
    }
    std::fs::write(&key, bytes).unwrap();
    f.app.settings.encoded_archives.push(gm_config::EncodedArchive {
        new_extension: new_ext.into(),
        original_extension: orig_ext.into(),
        password_protected: password.is_some(),
        key_file: key.display().to_string(),
    });
    key
}

#[test]
fn encoded_archives_menu_adds_configuration() {
    let mut f = fixture("encmenu", vec![]);
    let key = f.root.join("my.key");
    std::fs::write(&key, ENC_KEY).unwrap();
    f.app.ui.term.recorded = Some(Vec::new());
    // F9 → Commands → V (Encoded archives) → Ins: new extension, original
    // extension, key file, Space on "Password protected", Enter = OK; Esc
    // closes the list.
    let mut script = vec![plain(KeyCode::Right), plain(KeyCode::Right), plain(KeyCode::Char('v')), plain(KeyCode::Insert)];
    script.extend(text(".zipx"));
    script.push(plain(KeyCode::Tab));
    script.extend(text("zip"));
    script.push(plain(KeyCode::Tab));
    script.extend(text(&key.display().to_string()));
    script.push(plain(KeyCode::Tab));
    script.push(plain(KeyCode::Char(' ')));
    script.push(plain(KeyCode::Enter));
    script.push(Event::Resize(110, 32));
    script.push(plain(KeyCode::Esc));
    f.run(plain(KeyCode::F(9)), script);
    let frames = f.app.ui.term.recorded.replace(Vec::new()).unwrap_or_default();
    // The item is right above "Screens list".
    let menu = frames.iter().find(|fr| fr.contains("Encoded archives") && fr.contains("Screens list")).expect("no Commands menu");
    let lines: Vec<&str> = menu.lines().collect();
    let enc = lines.iter().position(|l| l.contains("Encoded archives")).unwrap();
    assert!(lines[enc + 1].contains("Screens list"), "{menu}");
    // The list shows the new configuration.
    assert!(frames.iter().any(|fr| fr.contains("*.zipx") && fr.contains("*.zip ") && fr.contains("Password protected")), "list not shown");
    assert_eq!(
        f.app.settings.encoded_archives,
        vec![gm_config::EncodedArchive {
            new_extension: "zipx".into(),
            original_extension: "zip".into(),
            password_protected: true,
            key_file: key.display().to_string(),
        }]
    );
    // Stored in settings.toml right away.
    let saved = std::fs::read_to_string(f.root.join("settings.toml")).unwrap();
    assert!(saved.contains("[[encoded_archives]]") && saved.contains("newExtension = \"zipx\""), "{saved}");
    assert!(saved.contains("originalExtension = \"zip\"") && saved.contains("passwordProtected = true"), "{saved}");
}

#[test]
fn encoded_archives_rejects_long_extension() {
    let mut f = fixture("enclong", vec![]);
    let key = f.root.join("my.key");
    std::fs::write(&key, ENC_KEY).unwrap();
    f.app.ui.term.recorded = Some(Vec::new());
    let mut script = vec![plain(KeyCode::Right), plain(KeyCode::Right), plain(KeyCode::Char('v')), plain(KeyCode::Insert)];
    script.extend(text("toolong12"));
    script.push(plain(KeyCode::Tab));
    script.extend(text("txt"));
    script.push(plain(KeyCode::Tab));
    script.extend(text(&key.display().to_string()));
    script.push(plain(KeyCode::Enter));
    // The error message, then the dialog again: cancel it and the list.
    script.extend([plain(KeyCode::Enter), plain(KeyCode::Esc), plain(KeyCode::Esc)]);
    f.run(plain(KeyCode::F(9)), script);
    let frames = f.app.ui.term.recorded.replace(Vec::new()).unwrap_or_default();
    assert!(frames.iter().any(|fr| fr.contains("1 to 7 characters")), "no error shown");
    assert!(f.app.settings.encoded_archives.is_empty());
}

#[test]
fn encoded_file_edit_and_save_encodes_back() {
    let mut f = fixture("encedit", vec![]);
    let key = add_config(&mut f, "enc", "txt", None);
    let file = f.left().join("notes.enc");
    encode_bytes(&f, &key, None, b"secret line\n", &file);
    f.app.left.files.reload();
    f.cursor_to("notes.enc");
    // F3 shows the decoded text; closing the viewer deletes the decoded copy.
    f.app.handle_event(plain(KeyCode::F(3)));
    assert!(f.screen().contains("secret line"));
    let viewed = f.app.encoded.decoded_files();
    assert_eq!(viewed.len(), 1);
    assert!(viewed[0].is_file());
    f.app.handle_event(plain(KeyCode::Esc));
    assert!(!viewed[0].exists(), "decoded copy left after the viewer");
    assert!(!viewed[0].parent().unwrap().exists(), "decoded folder left after the viewer");
    assert!(f.app.encoded.decoded_files().is_empty());
    // F4 edits the decoded copy notes.txt; F2 saves: notes.enc is encoded again.
    f.app.handle_event(plain(KeyCode::F(4)));
    let decoded = f.app.encoded.decoded_files();
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].file_name().unwrap(), "notes.txt");
    for e in text(">> ") {
        f.app.handle_event(e);
    }
    f.app.handle_event(plain(KeyCode::F(2)));
    assert_eq!(decode_bytes(&f, &key, None, &file), b">> secret line\n");
    assert_ne!(std::fs::read(&file).unwrap(), b">> secret line\n", "stored encoded");
    assert!(decoded[0].is_file(), "deleted while the editor is open");
    // Closing the editor deletes the decoded copy right away.
    f.app.handle_event(plain(KeyCode::Esc));
    assert!(f.app.current.is_none());
    assert!(!decoded[0].exists(), "decoded copy left after the editor");
    assert!(f.app.encoded.decoded_files().is_empty());
    assert_eq!(decode_bytes(&f, &key, None, &file), b">> secret line\n");
}

#[test]
fn encoded_file_unsaved_editor_changes_are_encoded_on_exit() {
    let mut f = fixture("encexit", vec![]);
    let key = add_config(&mut f, "enc", "txt", None);
    let file = f.left().join("todo.enc");
    encode_bytes(&f, &key, None, b"one\n", &file);
    f.app.left.files.reload();
    f.cursor_to("todo.enc");
    f.app.handle_event(plain(KeyCode::F(4)));
    for e in text("zero ") {
        f.app.handle_event(e);
    }
    // Esc on a modified file asks; Enter = Save: encoded, then deleted.
    f.run(plain(KeyCode::Esc), vec![plain(KeyCode::Enter)]);
    assert!(f.app.current.is_none());
    assert!(f.app.encoded.decoded_files().is_empty());
    assert_eq!(decode_bytes(&f, &key, None, &file), b"zero one\n");
}

/// Opens a file so that nobody else may delete it, like a program that
/// keeps it open (Windows; elsewhere an open file can always be deleted).
/// Others may still read it (the viewer has it open).
#[cfg(windows)]
fn lock(p: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(p).unwrap()
}

#[cfg(windows)]
#[test]
fn encoded_file_locked_copy_is_kept_silently() {
    let mut f = fixture("enclock", vec![]);
    let key = add_config(&mut f, "enc", "txt", None);
    encode_bytes(&f, &key, None, b"locked\n", &f.left().join("l.enc"));
    f.app.left.files.reload();
    f.cursor_to("l.enc");
    f.app.handle_event(plain(KeyCode::F(3)));
    let decoded = f.app.encoded.decoded_files();
    // Closing the viewer while another program holds the copy: nothing is
    // shown, the copy stays.
    let held = lock(&decoded[0]);
    f.app.ui.term.recorded = Some(Vec::new());
    f.app.handle_event(plain(KeyCode::Esc));
    let frames = f.app.ui.term.recorded.replace(Vec::new()).unwrap_or_default();
    assert!(f.app.current.is_none());
    assert!(frames.iter().all(|fr| !fr.contains("Error") && !fr.contains("Encoded archives")), "a message was shown");
    assert!(decoded[0].is_file(), "locked copy deleted");
    assert_eq!(f.app.encoded.decoded_files(), decoded);
    // Once the program lets it go, the next event deletes it.
    drop(held);
    f.app.handle_event(Event::Resize(110, 32));
    assert!(!decoded[0].exists(), "copy left after the lock was released");
    assert!(f.app.encoded.decoded_files().is_empty());
}

#[test]
fn encoded_file_opened_externally_waits_for_grace_period() {
    let mut f = fixture("encext", vec![]);
    let key = add_config(&mut f, "enc", "txt", None);
    encode_bytes(&f, &key, None, b"ext\n", &f.left().join("x.enc"));
    f.app.left.files.reload();
    f.cursor_to("x.enc");
    // Decode it (F3), then mark it as handed to another program, as Enter
    // does for a file that is not an archive (without starting one here).
    f.app.handle_event(plain(KeyCode::F(3)));
    let decoded = f.app.encoded.decoded_files();
    gm_core::encoded::opened_externally(&mut f.app, &decoded[0]);
    f.app.handle_event(plain(KeyCode::Esc));
    assert!(decoded[0].is_file(), "deleted before the program could open it");
    // After the grace period it goes on the next event.
    f.app.encoded.external_grace = std::time::Duration::ZERO;
    f.app.handle_event(Event::Resize(110, 32));
    assert!(!decoded[0].exists());
}

#[test]
fn encoded_file_with_password_asks_once() {
    let mut f = fixture("encpw", vec![]);
    let key = add_config(&mut f, "penc", "txt", Some("пароль"));
    let file = f.left().join("diary.penc");
    encode_bytes(&f, &key, Some("пароль"), b"dear diary\n", &file);
    f.app.left.files.reload();
    f.cursor_to("diary.penc");
    f.app.ui.term.recorded = Some(Vec::new());
    // F3: the password dialog (input hidden), then the viewer.
    let mut script = text("пароль");
    script.push(Event::Resize(110, 32));
    script.push(plain(KeyCode::Enter));
    f.run(plain(KeyCode::F(3)), script);
    let frames = f.app.ui.term.recorded.replace(Vec::new()).unwrap_or_default();
    let dlg = frames.iter().find(|fr| fr.contains("Password for diary.penc")).expect("no password dialog");
    assert!(!dlg.contains("пароль"), "password shown:\n{dlg}");
    assert!(f.screen().contains("dear diary"));
    // Closing the viewer deletes the copy and forgets the password.
    f.app.handle_event(plain(KeyCode::Esc));
    assert!(f.app.encoded.decoded_files().is_empty());
    // F4 asks again (a new copy); saving then encodes with that password
    // without asking.
    let mut script = text("пароль");
    script.push(plain(KeyCode::Enter));
    f.run(plain(KeyCode::F(4)), script);
    assert!(f.screen().contains("dear diary"), "editor expected");
    for e in text("my ") {
        f.app.handle_event(e);
    }
    f.app.ui.term.recorded = Some(Vec::new());
    f.app.handle_event(plain(KeyCode::F(2)));
    let frames = f.app.ui.term.recorded.replace(Vec::new()).unwrap_or_default();
    assert!(frames.iter().all(|fr| !fr.contains("Password for")), "asked again when saving");
    assert_eq!(decode_bytes(&f, &key, Some("пароль"), &file), b"my dear diary\n");
    f.app.handle_event(plain(KeyCode::Esc));
    assert!(f.app.encoded.decoded_files().is_empty());
    // Nothing about the password is stored in the settings.
    let state = format!("{:?}", f.app.settings);
    assert!(!state.contains("пароль"));
}

#[test]
fn encoded_file_cancelled_password_opens_nothing() {
    let mut f = fixture("encpwcancel", vec![]);
    let key = add_config(&mut f, "penc", "txt", Some("pw"));
    encode_bytes(&f, &key, Some("pw"), b"x", &f.left().join("a.penc"));
    f.app.left.files.reload();
    f.cursor_to("a.penc");
    f.run(plain(KeyCode::F(4)), vec![plain(KeyCode::Esc)]);
    assert!(f.app.current.is_none());
    assert!(f.app.encoded.decoded_files().is_empty());
}

#[test]
fn encoded_zip_enter_and_add_file_encodes_back() {
    let mut f = fixture("enczip", vec![Box::new(mod_archive::Archive)]);
    let key_file = add_config(&mut f, "zipx", "zip", None);
    // Make alpha.zip (Shift+F1), encode it into alpha.zipx.
    f.cursor_to("alpha.txt");
    f.run(key(KeyCode::F(1), Mods::SHIFT), vec![plain(KeyCode::Enter)]);
    let zip = f.left().join("alpha.zip");
    let zipx = f.left().join("alpha.zipx");
    codec_file(gm_core::codec::Mode::Encode, &key_file, None, &zip, &zipx);
    std::fs::remove_file(&zip).unwrap();
    f.app.left.files.reload();
    // Enter opens the decoded alpha.zip as an archive panel.
    f.cursor_to("alpha.zipx");
    f.app.handle_event(plain(KeyCode::Enter));
    assert!(f.screen().contains("zip:"));
    assert!(f.app.left.files.entries.iter().any(|e| e.name == "alpha.txt"));
    // F5 from the right panel copies gamma.txt into the archive: the
    // rewritten zip is encoded back into alpha.zipx.
    std::fs::write(f.right().join("gamma.txt"), "gamma").unwrap();
    f.app.handle_event(plain(KeyCode::Tab));
    f.app.right.files.reload();
    assert!(f.app.right.files.set_cursor_name("gamma.txt"));
    f.run(plain(KeyCode::F(5)), vec![plain(KeyCode::Enter)]);
    let decoded = decode_bytes(&f, &key_file, None, &zipx);
    assert!(decoded.starts_with(b"PK"), "not a zip after decoding");
    let has = |name: &[u8]| decoded.windows(name.len()).any(|w| w == name);
    assert!(has(b"gamma.txt") && has(b"alpha.txt"), "zip members missing");
    assert!(!f.left().join("alpha.zip").exists(), "no plain copy next to the original");
    // While the archive panel is open, its decoded zip stays.
    let copies = f.app.encoded.decoded_files();
    assert_eq!(copies.len(), 1);
    assert!(copies[0].is_file());
    // ".." leaves the archive: the decoded zip and its folder are deleted.
    f.app.handle_event(plain(KeyCode::Tab));
    f.app.left.files.home();
    f.app.handle_event(plain(KeyCode::Enter));
    assert!(f.app.left.files.is_local());
    assert_eq!(f.app.left.files.current_name().as_deref(), Some("alpha.zipx"));
    assert!(!copies[0].exists(), "decoded zip left after leaving the archive");
    assert!(!copies[0].parent().unwrap().exists(), "decoded folder left");
    assert!(f.app.encoded.decoded_files().is_empty());
    // The saved archive is still complete.
    let decoded = decode_bytes(&f, &key_file, None, &zipx);
    let has = |name: &[u8]| decoded.windows(name.len()).any(|w| w == name);
    assert!(has(b"gamma.txt") && has(b"alpha.txt"), "zip members missing after leaving");
    // Opening it again works (a fresh copy).
    f.app.handle_event(plain(KeyCode::Enter));
    assert!(f.app.left.files.entries.iter().any(|e| e.name == "gamma.txt"));
}

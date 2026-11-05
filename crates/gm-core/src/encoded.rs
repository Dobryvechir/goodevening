//! F9 → Commands → Encoded archives.
//!
//! A configuration says that files named `*.<newExtension>` are encoded with
//! a key file (see [`crate::codec`]). F3, F4 and Enter on such a file decode
//! it (`gm --d`, or `gm --pd` after asking for a password) into
//! `*.<originalExtension>` in a private temporary folder and open that copy:
//! the viewer, the editor, or (Enter) an archive panel / the associated
//! program.
//!
//! Every change of a decoded copy (the editor saves it, the archive panel
//! rewrites the .zip, another program writes it) is encoded back over the
//! original file (`gm --e` / `gm --pe`) with the same password. The password
//! is only kept in memory, never written anywhere.
//!
//! As soon as gm no longer uses a decoded copy (its viewer or editor is
//! closed, its archive panel is left), the copy is encoded back if needed
//! and deleted with its folder. A copy opened with another program is only
//! tried after a grace period (the program is started asynchronously and
//! must get the chance to open it); a copy that cannot be deleted because
//! it is locked is silently kept and tried again later. Whatever is left is
//! deleted when gm exits.

use crate::app::App;
use crate::codec::{self, Mode};
use gm_config::{EncodedArchive, Settings};
use gm_i18n::{MsgId, msg, plain};
use gm_term::{KeyChord, KeyCode};
use gm_ui::dialog::DialogBuilder;
use gm_ui::{MenuItem, MenuResult, Ui, VMenu};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// What a file looks like on disk, to notice changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

fn stamp(p: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(p).ok()?;
    m.is_file().then(|| Stamp { len: m.len(), modified: m.modified().ok() })
}

/// One decoded file.
struct Session {
    /// The encoded original.
    encoded: PathBuf,
    /// The decoded copy (inside `dir`).
    plain: PathBuf,
    /// The private temporary folder of this session.
    dir: PathBuf,
    key_file: PathBuf,
    /// Kept in memory only, for encoding back without asking again.
    password: Option<Vec<u8>>,
    plain_stamp: Option<Stamp>,
    encoded_stamp: Option<Stamp>,
    /// The last encode failed: keep the decoded copy, it holds the only
    /// version of the changes.
    encode_failed: bool,
    /// Opened with another program at this time (not by gm itself).
    external_since: Option<Instant>,
}

impl Session {
    fn ops(&self) -> Result<Vec<codec::Op>, String> {
        codec::load_key(&self.key_file, self.password.as_deref())
    }

    /// Decodes the original into the decoded copy.
    fn decode(&mut self) -> Result<(), String> {
        let ops = self.ops()?;
        codec::transform_file(Mode::Decode, &ops, &self.encoded, &self.plain)?;
        self.plain_stamp = stamp(&self.plain);
        self.encoded_stamp = stamp(&self.encoded);
        Ok(())
    }

    /// Encodes the decoded copy back over the original (via a temporary file
    /// next to it, so a failure never leaves a half-written original).
    fn encode(&mut self) -> Result<(), String> {
        let ops = self.ops()?;
        let mut tmp_name = self.encoded.file_name().unwrap_or_default().to_os_string();
        tmp_name.push(".gm-tmp");
        let tmp = self.encoded.with_file_name(tmp_name);
        let r = codec::transform_file(Mode::Encode, &ops, &self.plain, &tmp)
            .and_then(|()| std::fs::rename(&tmp, &self.encoded).map_err(|e| format!("{}: {e}", self.encoded.display())));
        if r.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        self.encode_failed = r.is_err();
        r?;
        self.encoded_stamp = stamp(&self.encoded);
        Ok(())
    }

    /// Encodes back if the decoded copy changed since it was last decoded or encoded.
    fn sync(&mut self) -> Option<Result<(), String>> {
        let now = stamp(&self.plain)?;
        if Some(now) == self.plain_stamp {
            return None;
        }
        // Remembered even on failure: the next save tries again.
        self.plain_stamp = Some(now);
        Some(self.encode())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(p) = &mut self.password {
            p.iter_mut().for_each(|b| *b = 0);
        }
    }
}

/// The decoded files of this gm.
pub struct Sessions {
    list: Vec<Session>,
    /// How long a copy opened with another program is left alone before gm
    /// tries to delete it (10 s).
    pub external_grace: Duration,
}

impl Default for Sessions {
    fn default() -> Self {
        Sessions { list: Vec::new(), external_grace: Duration::from_secs(10) }
    }
}

impl Sessions {
    fn base_dir() -> PathBuf {
        std::env::temp_dir().join(format!("gm-encoded-{}", std::process::id()))
    }

    /// A new private folder for one decoded file. The folder is created
    /// exclusively (not recursively), so no two sessions ever share one.
    fn new_dir(&mut self) -> std::io::Result<PathBuf> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let base = Self::base_dir();
        // Only the owner may read the decoded copies (mode is set on Unix).
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        // The empty base folder is removed when the last copy goes, so it may
        // vanish between the two steps: then it is created again.
        for _ in 0..100 {
            match b.create(&base) {
                Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e),
                _ => {}
            }
            let dir = base.join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
            match b.create(&dir) {
                Ok(()) => return Ok(dir),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::NotFound) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::other(format!("cannot create a folder in {}", base.display())))
    }

    fn find(&self, encoded: &Path) -> Option<usize> {
        let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let want = canon(encoded);
        self.list.iter().position(|s| canon(&s.encoded) == want)
    }

    /// The decoded copies that are open (for tests and the info line).
    pub fn decoded_files(&self) -> Vec<PathBuf> {
        self.list.iter().map(|s| s.plain.clone()).collect()
    }

    /// Encodes back every decoded copy that changed; returns the errors.
    fn sync(&mut self) -> Vec<(PathBuf, String)> {
        self.list
            .iter_mut()
            .filter_map(|s| match s.sync()? {
                Ok(()) => None,
                Err(e) => Some((s.encoded.clone(), e)),
            })
            .collect()
    }
}

impl Sessions {
    /// Deletes the decoded copies that nothing in gm uses any more (`in_use`
    /// lists the files of open windows and archive panels). A copy is kept
    /// when its last encode failed, during the grace period of another
    /// program, or when it is locked (then silently, to try again later).
    fn release_unused(&mut self, in_use: &[PathBuf]) {
        let grace = self.external_grace;
        self.list.retain_mut(|s| {
            if s.encode_failed || in_use.iter().any(|p| same_file(p, &s.plain)) {
                return true;
            }
            if s.external_since.is_some_and(|t| t.elapsed() < grace) {
                return true;
            }
            // Changes made since the last event are encoded first.
            if let Some(Err(_)) = s.sync() {
                return true;
            }
            match std::fs::remove_file(&s.plain) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                // Locked (open in another program): try again later.
                Err(_) => return true,
            }
            let _ = std::fs::remove_dir_all(&s.dir);
            false
        });
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

impl Drop for Sessions {
    /// Last chance to encode back changes, then the decoded copies go away.
    fn drop(&mut self) {
        let _ = self.sync();
        for s in self.list.drain(..) {
            let _ = std::fs::remove_dir_all(&s.dir);
        }
        let _ = std::fs::remove_dir(Self::base_dir());
    }
}

/// The configuration for a file name: the name must end with exactly
/// `.<newExtension>` (the longest one wins) and have something before it.
pub fn config_for<'a>(list: &'a [EncodedArchive], name: &str) -> Option<&'a EncodedArchive> {
    list.iter()
        .filter(|c| !c.new_extension.is_empty())
        .filter(|c| name.len() > c.new_extension.len() + 1 && name.ends_with(&format!(".{}", c.new_extension)))
        .max_by_key(|c| c.new_extension.len())
}

/// `report.txt.enc` with newExtension `enc` and originalExtension `txt` → `report.txt.txt`;
/// `report.enc` → `report.txt`. An empty originalExtension leaves only the stem.
pub fn decoded_name(name: &str, c: &EncodedArchive) -> String {
    let stem = &name[..name.len() - c.new_extension.len() - 1];
    if c.original_extension.is_empty() { stem.to_string() } else { format!("{stem}.{}", c.original_extension) }
}

/// The key file of a configuration; relative paths are in the settings folder.
pub fn key_path(c: &EncodedArchive) -> PathBuf {
    let p = PathBuf::from(c.key_file.trim());
    if p.is_absolute() { p } else { gm_config::profile_dir().join(p) }
}

/// The result of [`decode_for_open`].
pub enum Decoded {
    /// Not an encoded file: open it as usual.
    NotEncoded,
    /// Encoded, but cancelled or failed (the user was told).
    Failed,
    /// The decoded copy to open instead.
    Ready(PathBuf),
}

/// F3/F4/Enter on `path`: decodes it when its extension is configured.
pub fn decode_for_open(app: &mut App, path: &Path) -> Decoded {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let Some(cfg) = config_for(&app.settings.encoded_archives, &name).cloned() else { return Decoded::NotEncoded };
    let title = plain(MsgId::MEncArcTitle);
    // Opened before: reuse the decoded copy (and the password), decoding again
    // only if the original was changed by someone else meanwhile.
    if let Some(i) = app.encoded.find(path) {
        let s = &mut app.encoded.list[i];
        if stamp(&s.plain).is_some() && stamp(&s.encoded) == s.encoded_stamp {
            return Decoded::Ready(s.plain.clone());
        }
        return match s.decode() {
            Ok(()) => Decoded::Ready(s.plain.clone()),
            Err(e) => {
                gm_ui::message::error(&mut app.ui, &title, &format!("{}\n{}\n{e}", msg(MsgId::MEncArcCannotDecode), path.display()));
                Decoded::Failed
            }
        };
    }
    let password = if cfg.password_protected {
        match ask_password(&mut app.ui, &name) {
            Some(p) => Some(p),
            None => return Decoded::Failed,
        }
    } else {
        None
    };
    let dir = match app.encoded.new_dir() {
        Ok(d) => d,
        Err(e) => {
            gm_ui::message::error(&mut app.ui, &title, &format!("{}\n{e}", msg(MsgId::MEncArcCannotDecode)));
            return Decoded::Failed;
        }
    };
    let mut s = Session {
        encoded: path.to_path_buf(),
        plain: dir.join(decoded_name(&name, &cfg)),
        dir,
        key_file: key_path(&cfg),
        password,
        plain_stamp: None,
        encoded_stamp: None,
        encode_failed: false,
        external_since: None,
    };
    match s.decode() {
        Ok(()) => {
            let plain = s.plain.clone();
            app.encoded.list.push(s);
            Decoded::Ready(plain)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&s.dir);
            gm_ui::message::error(&mut app.ui, &title, &format!("{}\n{}\n{e}", msg(MsgId::MEncArcCannotDecode), path.display()));
            Decoded::Failed
        }
    }
}

/// Encodes back the decoded copies that changed (called after every event).
pub fn sync(app: &mut App) {
    if app.encoded.list.is_empty() {
        return;
    }
    for (path, e) in app.encoded.sync() {
        gm_ui::message::error(&mut app.ui, &plain(MsgId::MEncArcTitle), &format!("{}\n{}\n{e}", msg(MsgId::MEncArcCannotEncode), path.display()));
    }
}

/// The decoded copy `plain` was handed to another program: it is not
/// deleted during the grace period.
pub fn opened_externally(app: &mut App, plain: &Path) {
    if let Some(s) = app.encoded.list.iter_mut().find(|s| s.plain == plain) {
        s.external_since = Some(Instant::now());
    }
}

/// Encodes back and deletes the decoded copies gm no longer shows: no
/// viewer or editor window and no archive panel uses them (called after
/// every event and on idle ticks).
pub fn release_unused(app: &mut App) {
    if app.encoded.list.is_empty() {
        return;
    }
    let mut in_use: Vec<PathBuf> = app.windows.iter().filter_map(|w| w.file()).collect();
    for slot in [&app.left, &app.right] {
        in_use.extend(slot.files.stack.iter().filter_map(|p| p.source_file()));
    }
    app.encoded.release_unused(&in_use);
    let _ = std::fs::remove_dir(Sessions::base_dir());
}

/// Asks for a non-empty password (hidden input); None if cancelled.
fn ask_password(ui: &mut Ui, name: &str) -> Option<Vec<u8>> {
    loop {
        let mut b = DialogBuilder::new(plain(MsgId::MEncArcTitle), 60);
        b.text(format!("{} {name}:", msg(MsgId::MEncArcPasswordFor)));
        let e = b.password("");
        b.separator();
        b.ok_cancel();
        let mut d = b.build();
        let cancel = d.items.len() - 1;
        let r = d.run(ui)?;
        if r == cancel {
            return None;
        }
        let p = d.text(e);
        if !p.is_empty() {
            return Some(p.into_bytes());
        }
    }
}

/// Checks an edited configuration against the others.
pub fn validate(c: &EncodedArchive, others: &[EncodedArchive]) -> Result<(), MsgId> {
    let ok = |s: &str, min: usize| {
        let n = s.chars().count();
        n >= min && n <= EncodedArchive::MAX_EXTENSION && !s.contains(['/', '\\']) && !s.chars().any(char::is_whitespace)
    };
    if !ok(&c.new_extension, 1) || !ok(&c.original_extension, 0) {
        return Err(MsgId::MEncArcBadExtension);
    }
    if others.iter().any(|o| o.new_extension == c.new_extension) {
        return Err(MsgId::MEncArcDuplicate);
    }
    if c.key_file.trim().is_empty() || !key_path(c).is_file() {
        return Err(MsgId::MEncArcNoKeyFile);
    }
    Ok(())
}

/// Trims spaces and a leading dot from an extension field.
fn clean_ext(s: &str) -> String {
    let s = s.trim();
    s.strip_prefix('.').unwrap_or(s).to_string()
}

/// The add/edit dialog; None if cancelled.
fn edit_dialog(ui: &mut Ui, init: &EncodedArchive, others: &[EncodedArchive]) -> Option<EncodedArchive> {
    let title = plain(MsgId::MEncArcTitle);
    let mut cur = init.clone();
    loop {
        let mut b = DialogBuilder::new(title.clone(), 66).help("EncodedArchives");
        b.text(msg(MsgId::MEncArcNewExt));
        let new_ext = b.edit(&cur.new_extension, None);
        b.text(msg(MsgId::MEncArcOrigExt));
        let orig_ext = b.edit(&cur.original_extension, None);
        b.text(msg(MsgId::MEncArcKeyFile));
        let key = b.edit(&cur.key_file, Some("EncodedArchiveKey"));
        b.separator();
        let pw = b.checkbox(msg(MsgId::MEncArcPasswordProtected), cur.password_protected);
        b.separator();
        b.ok_cancel();
        let mut d = b.build();
        let cancel = d.items.len() - 1;
        let r = d.run(ui)?;
        if r == cancel {
            return None;
        }
        cur = EncodedArchive {
            new_extension: clean_ext(&d.text(new_ext)),
            original_extension: clean_ext(&d.text(orig_ext)),
            password_protected: d.checked(pw),
            key_file: d.text(key).trim().to_string(),
        };
        match validate(&cur, others) {
            Ok(()) => return Some(cur),
            Err(m) => {
                let text = if m == MsgId::MEncArcNoKeyFile { format!("{}\n{}", msg(m), key_path(&cur).display()) } else { msg(m).to_string() };
                gm_ui::message::error(ui, &title, &text);
            }
        }
    }
}

/// Saves only the encoded archive list into settings.toml, leaving the other
/// settings as they are on disk.
fn persist(app: &mut App) {
    let (mut on_disk, broken) = Settings::load(&app.settings_path);
    if let Some(e) = broken {
        // Never overwrite a settings file gm could not read.
        gm_ui::message::error(&mut app.ui, &plain(MsgId::MEncArcTitle), &e);
        return;
    }
    on_disk.encoded_archives = app.settings.encoded_archives.clone();
    if let Err(e) = on_disk.save(&app.settings_path) {
        gm_ui::message::error(&mut app.ui, &plain(MsgId::MEncArcTitle), &format!("{}\n{e}", app.settings_path.display()));
    }
}

fn describe(c: &EncodedArchive) -> String {
    let orig = if c.original_extension.is_empty() { "*".to_string() } else { format!("*.{}", c.original_extension) };
    format!("*.{:<9}→ {:<10} {}", c.new_extension, orig, c.key_file).replace('&', "&&")
}

/// F9 → Commands → Encoded archives: the configured list; Enter edits,
/// Ins or the last item adds, Del deletes.
pub fn configure(app: &mut App) {
    let mut sel = 0;
    loop {
        let list = app.settings.encoded_archives.clone();
        let mut items: Vec<MenuItem> = list
            .iter()
            .map(|c| MenuItem::with_hint(describe(c), if c.password_protected { plain(MsgId::MEncArcPasswordProtected) } else { String::new() }))
            .collect();
        if !items.is_empty() {
            items.push(MenuItem::separator());
        }
        let add_index = items.len();
        items.push(MenuItem::new(msg(MsgId::MEncArcAddNew)));
        let mut m = VMenu::new(plain(MsgId::MEncArcTitle), items);
        m.bottom = "Enter Ins Del".into();
        m.min_width = 50;
        m.pass_keys = vec![KeyChord::plain(KeyCode::Insert), KeyChord::plain(KeyCode::Delete)];
        m.select(sel.min(add_index));
        let edit = match m.run(&mut app.ui) {
            MenuResult::Selected(i) if i < list.len() => Some(i),
            MenuResult::Selected(_) => None,
            MenuResult::Key(k, i) if k.code == KeyCode::Delete => {
                if i < list.len() {
                    app.settings.encoded_archives.remove(i);
                    persist(app);
                }
                sel = i;
                continue;
            }
            MenuResult::Key(_, _) => None,
            _ => return,
        };
        let init = edit.and_then(|i| list.get(i).cloned()).unwrap_or_default();
        let others: Vec<EncodedArchive> = list.iter().enumerate().filter(|(i, _)| Some(*i) != edit).map(|(_, c)| c.clone()).collect();
        let Some(c) = edit_dialog(&mut app.ui, &init, &others) else {
            sel = edit.unwrap_or(add_index);
            continue;
        };
        match edit {
            Some(i) => {
                app.settings.encoded_archives[i] = c;
                sel = i;
            }
            None => {
                app.settings.encoded_archives.push(c);
                sel = app.settings.encoded_archives.len() - 1;
            }
        }
        persist(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(new: &str, orig: &str) -> EncodedArchive {
        EncodedArchive { new_extension: new.into(), original_extension: orig.into(), ..Default::default() }
    }

    #[test]
    fn matching_is_exact_and_longest_wins() {
        let list = vec![cfg("enc", "txt"), cfg("zip.enc", "zip"), cfg("ZX", "zip")];
        assert_eq!(config_for(&list, "a.enc").unwrap().original_extension, "txt");
        assert_eq!(config_for(&list, "a.zip.enc").unwrap().new_extension, "zip.enc");
        assert!(config_for(&list, "a.ENC").is_none());
        assert!(config_for(&list, "a.zx").is_none());
        assert!(config_for(&list, ".enc").is_none());
        assert!(config_for(&list, "aenc").is_none());
        assert!(config_for(&list, "a.xenc").is_none());
    }

    #[test]
    fn decoded_names() {
        assert_eq!(decoded_name("report.enc", &cfg("enc", "txt")), "report.txt");
        assert_eq!(decoded_name("data.zip.enc", &cfg("zip.enc", "zip")), "data.zip");
        assert_eq!(decoded_name("notes.enc", &cfg("enc", "")), "notes");
    }

    #[test]
    fn validation() {
        let dir = std::env::temp_dir().join(format!("gm-encval-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("key.txt");
        std::fs::write(&key, "^01").unwrap();
        let mk = |new: &str, orig: &str| EncodedArchive { key_file: key.display().to_string(), ..cfg(new, orig) };
        assert_eq!(validate(&mk("enc", "txt"), &[]), Ok(()));
        assert_eq!(validate(&mk("1234567", "1234567"), &[]), Ok(()));
        assert_eq!(validate(&mk("enc", ""), &[]), Ok(()));
        assert_eq!(validate(&mk("12345678", "txt"), &[]), Err(MsgId::MEncArcBadExtension));
        assert_eq!(validate(&mk("enc", "12345678"), &[]), Err(MsgId::MEncArcBadExtension));
        assert_eq!(validate(&mk("", "txt"), &[]), Err(MsgId::MEncArcBadExtension));
        assert_eq!(validate(&mk("e/c", "txt"), &[]), Err(MsgId::MEncArcBadExtension));
        assert_eq!(validate(&mk("enc", "txt"), &[cfg("enc", "zip")]), Err(MsgId::MEncArcDuplicate));
        let mut missing = mk("enc", "txt");
        missing.key_file = dir.join("nope.txt").display().to_string();
        assert_eq!(validate(&missing, &[]), Err(MsgId::MEncArcNoKeyFile));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

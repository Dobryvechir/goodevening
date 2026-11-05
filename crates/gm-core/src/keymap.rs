//! Panel commands and the key map. Defaults reproduce Far; users can
//! override them in `keymap.toml`:
//!
//! ```toml
//! [panels]
//! "Ctrl+F3" = "SortByName"
//! "Alt+Q" = "Quit"
//! ```

use gm_panels::SortMode;
use gm_term::KeyChord;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    UserMenu,
    View,
    ViewAlt,
    Edit,
    EditAlt,
    EditNew,
    Copy,
    Move,
    CopyInPlace,
    RenameInPlace,
    MakeFolder,
    Delete,
    DeletePermanent,
    Wipe,
    Menu,
    Quit,
    Screens,
    DriveLeft,
    DriveRight,
    FindFile,
    FindFolder,
    CommandHistory,
    ViewHistory,
    FolderHistory,
    Link,
    SwitchPanel,
    SwapPanels,
    UserScreen,
    HideLeft,
    HideRight,
    HideOther,
    Refresh,
    ToggleHidden,
    InfoPanel,
    QuickView,
    TreePanel,
    Sort(SortMode),
    SortMenu,
    ViewMode(u8),
    Attributes,
    Describe,
    Filter,
    Parent,
    EnterInPlace,
    Root,
    SelectGroup,
    UnselectGroup,
    InvertSelection,
    RestoreSelection,
    SelectAll,
    UnselectAll,
    SelectSameExt,
    UnselectSameExt,
    InsertName,
    InsertPath,
    InsertLeftDir,
    InsertRightDir,
    InsertActiveDir,
    InsertPassiveDir,
    CopyNames,
    CopyPaths,
    ShortcutGo(u8),
    ShortcutSet(u8),
    ShortcutsMenu,
    ToggleKeyBar,
    PanelsTaller,
    PanelsShorter,
    LeftNarrower,
    LeftWider,
    OpenWithSystem,
    AddToArchive,
    ExtractArchive,
    SaveSetup,
    ApplyCommand,
    CompareFolders,
    History,
    /// F11: like Enter, with the program's output logged to a file.
    Log,
    /// Shift+F11: like Log, but only errors are shown on the screen.
    LogQuiet,
}

/// Names used in `keymap.toml`.
pub fn command_names() -> Vec<(String, Command)> {
    use Command::*;
    let mut v: Vec<(String, Command)> = [
        ("Help", Help),
        ("UserMenu", UserMenu),
        ("View", View),
        ("ViewAlt", ViewAlt),
        ("Edit", Edit),
        ("EditAlt", EditAlt),
        ("EditNew", EditNew),
        ("Copy", Copy),
        ("Move", Move),
        ("CopyInPlace", CopyInPlace),
        ("RenameInPlace", RenameInPlace),
        ("MakeFolder", MakeFolder),
        ("Delete", Delete),
        ("DeletePermanent", DeletePermanent),
        ("Wipe", Wipe),
        ("Menu", Menu),
        ("Quit", Quit),
        ("Screens", Screens),
        ("DriveLeft", DriveLeft),
        ("DriveRight", DriveRight),
        ("FindFile", FindFile),
        ("FindFolder", FindFolder),
        ("CommandHistory", CommandHistory),
        ("ViewHistory", ViewHistory),
        ("FolderHistory", FolderHistory),
        ("Link", Link),
        ("SwitchPanel", SwitchPanel),
        ("SwapPanels", SwapPanels),
        ("UserScreen", UserScreen),
        ("HideLeft", HideLeft),
        ("HideRight", HideRight),
        ("HideOther", HideOther),
        ("Refresh", Refresh),
        ("ToggleHidden", ToggleHidden),
        ("InfoPanel", InfoPanel),
        ("QuickView", QuickView),
        ("TreePanel", TreePanel),
        ("SortMenu", SortMenu),
        ("Attributes", Attributes),
        ("Describe", Describe),
        ("Filter", Filter),
        ("Parent", Parent),
        ("EnterInPlace", EnterInPlace),
        ("Root", Root),
        ("SelectGroup", SelectGroup),
        ("UnselectGroup", UnselectGroup),
        ("InvertSelection", InvertSelection),
        ("RestoreSelection", RestoreSelection),
        ("SelectAll", SelectAll),
        ("UnselectAll", UnselectAll),
        ("SelectSameExt", SelectSameExt),
        ("UnselectSameExt", UnselectSameExt),
        ("InsertName", InsertName),
        ("InsertPath", InsertPath),
        ("InsertLeftDir", InsertLeftDir),
        ("InsertRightDir", InsertRightDir),
        ("InsertActiveDir", InsertActiveDir),
        ("InsertPassiveDir", InsertPassiveDir),
        ("CopyNames", CopyNames),
        ("CopyPaths", CopyPaths),
        ("ShortcutsMenu", ShortcutsMenu),
        ("ToggleKeyBar", ToggleKeyBar),
        ("PanelsTaller", PanelsTaller),
        ("PanelsShorter", PanelsShorter),
        ("LeftNarrower", LeftNarrower),
        ("LeftWider", LeftWider),
        ("OpenWithSystem", OpenWithSystem),
        ("AddToArchive", AddToArchive),
        ("ExtractArchive", ExtractArchive),
        ("SaveSetup", SaveSetup),
        ("ApplyCommand", ApplyCommand),
        ("CompareFolders", CompareFolders),
        ("History", History),
        ("Log", Log),
        ("LogQuiet", LogQuiet),
    ]
    .into_iter()
    .map(|(n, c)| (n.to_string(), c))
    .collect();
    for m in SortMode::ALL {
        let name = m.key();
        let cap = name[..1].to_uppercase() + &name[1..];
        v.push((format!("SortBy{cap}"), Sort(m)));
    }
    for n in 0..=9u8 {
        v.push((format!("ViewMode{n}"), ViewMode(n)));
        v.push((format!("ShortcutGo{n}"), ShortcutGo(n)));
        v.push((format!("ShortcutSet{n}"), ShortcutSet(n)));
    }
    v
}

/// Far's default panel keys.
fn defaults() -> Vec<(&'static str, Command)> {
    use Command::*;
    let mut v = vec![
        ("F1", Help),
        ("F2", UserMenu),
        ("F3", View),
        ("Alt+F3", ViewAlt),
        ("F4", Edit),
        ("Alt+F4", EditAlt),
        ("Shift+F4", EditNew),
        ("F5", Copy),
        ("F6", Move),
        ("Shift+F5", CopyInPlace),
        ("Shift+F6", RenameInPlace),
        ("F7", MakeFolder),
        ("F8", Delete),
        ("Shift+F8", DeletePermanent),
        ("Shift+Del", DeletePermanent),
        ("Alt+Del", Wipe),
        ("F9", Menu),
        ("F10", Quit),
        ("Alt+X", Quit),
        ("F12", Screens),
        ("Alt+F1", DriveLeft),
        ("Alt+F2", DriveRight),
        ("Alt+F7", FindFile),
        ("Alt+F10", FindFolder),
        ("Alt+F8", CommandHistory),
        ("Alt+F11", ViewHistory),
        ("Alt+F12", FolderHistory),
        ("Alt+F6", Link),
        ("Tab", SwitchPanel),
        ("Ctrl+U", SwapPanels),
        ("Ctrl+O", UserScreen),
        ("Ctrl+F1", HideLeft),
        ("Ctrl+F2", HideRight),
        ("Ctrl+P", HideOther),
        ("Ctrl+R", Refresh),
        ("Ctrl+H", ToggleHidden),
        ("Ctrl+L", InfoPanel),
        ("Ctrl+Q", QuickView),
        ("Ctrl+T", TreePanel),
        ("Ctrl+F12", SortMenu),
        ("Ctrl+A", Attributes),
        ("Ctrl+Z", Describe),
        ("Ctrl+I", Filter),
        ("Ctrl+PgUp", Parent),
        ("Ctrl+PgDn", EnterInPlace),
        ("Ctrl+\\", Root),
        ("+", SelectGroup),
        ("-", UnselectGroup),
        ("*", InvertSelection),
        ("Ctrl+M", RestoreSelection),
        ("Shift++", SelectAll),
        ("Shift+-", UnselectAll),
        ("Ctrl++", SelectSameExt),
        ("Ctrl+-", UnselectSameExt),
        ("Ctrl+Enter", InsertName),
        ("Ctrl+J", InsertName),
        ("Ctrl+F", InsertPath),
        ("Ctrl+[", InsertLeftDir),
        ("Ctrl+]", InsertRightDir),
        ("Ctrl+Shift+[", InsertActiveDir),
        ("Ctrl+Shift+]", InsertPassiveDir),
        ("Ctrl+Ins", CopyNames),
        ("Ctrl+Shift+Ins", CopyPaths),
        ("Ctrl+Shift+C", CopyPaths),
        ("Ctrl+B", ToggleKeyBar),
        ("Ctrl+Up", PanelsShorter),
        ("Ctrl+Down", PanelsTaller),
        ("Shift+Enter", OpenWithSystem),
        ("Shift+F1", AddToArchive),
        ("Shift+F2", ExtractArchive),
        ("Shift+F9", SaveSetup),
        ("Ctrl+G", ApplyCommand),
        ("Alt+F9", History),
        ("F11", Log),
        ("Shift+F11", LogQuiet),
    ];
    for m in SortMode::ALL {
        let k: &'static str = Box::leak(format!("Ctrl+F{}", m.fkey()).into_boxed_str());
        v.push((k, Sort(m)));
    }
    for n in 0..=9u8 {
        let mode: &'static str = Box::leak(format!("Ctrl+{n}").into_boxed_str());
        let go: &'static str = Box::leak(format!("Ctrl+Alt+{n}").into_boxed_str());
        let set: &'static str = Box::leak(format!("Ctrl+Shift+{n}").into_boxed_str());
        v.push((mode, ViewMode(n)));
        v.push((go, ShortcutGo(n)));
        v.push((set, ShortcutSet(n)));
    }
    v
}

pub struct Keymap {
    panels: HashMap<KeyChord, Command>,
    pub errors: Vec<String>,
}

impl Keymap {
    pub fn load(path: &Path) -> Keymap {
        let mut panels = HashMap::new();
        for (k, c) in defaults() {
            if let Some(kc) = KeyChord::parse(k) {
                panels.insert(kc, c);
            }
        }
        let mut errors = Vec::new();
        if let Ok(text) = std::fs::read_to_string(path) {
            match text.parse::<toml::Table>() {
                Ok(t) => {
                    let names = command_names();
                    if let Some(toml::Value::Table(p)) = t.get("panels") {
                        for (k, v) in p {
                            let Some(kc) = KeyChord::parse(k) else {
                                errors.push(format!("unknown key: {k}"));
                                continue;
                            };
                            match v.as_str() {
                                Some("") | Some("None") => {
                                    panels.remove(&kc);
                                }
                                Some(name) => match names.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                                    Some((_, c)) => {
                                        panels.insert(kc, *c);
                                    }
                                    None => errors.push(format!("unknown command: {name}")),
                                },
                                None => errors.push(format!("{k}: expected a command name")),
                            }
                        }
                    }
                }
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
        Keymap { panels, errors }
    }

    pub fn panel_command(&self, k: &KeyChord) -> Option<Command> {
        self.panels.get(k).copied()
    }

    /// The key bound to a command (for menu hints).
    pub fn key_for(&self, c: Command) -> Option<String> {
        let mut keys: Vec<String> = self.panels.iter().filter(|(_, v)| **v == c).map(|(k, _)| k.to_string()).collect();
        keys.sort_by_key(|k| k.len());
        keys.into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gm_term::KeyCode;

    #[test]
    fn defaults_and_overrides() {
        let p = std::env::temp_dir().join(format!("gm-keymap-{}.toml", std::process::id()));
        std::fs::write(&p, "[panels]\n\"Alt+Q\" = \"Quit\"\n\"F10\" = \"\"\n\"Ctrl+Y\" = \"Nope\"\n").unwrap();
        let k = Keymap::load(&p);
        assert_eq!(k.panel_command(&KeyChord::plain(KeyCode::F(5))), Some(Command::Copy));
        assert_eq!(k.panel_command(&KeyChord::alt(KeyCode::Char('q'))), Some(Command::Quit));
        assert_eq!(k.panel_command(&KeyChord::plain(KeyCode::F(10))), None);
        assert_eq!(k.panel_command(&KeyChord::ctrl(KeyCode::F(6))), Some(Command::Sort(SortMode::Size)));
        assert_eq!(k.errors.len(), 1);
        assert_eq!(k.key_for(Command::Copy).as_deref(), Some("F5"));
        std::fs::remove_file(&p).unwrap();
    }
}

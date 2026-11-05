//! The typed settings model, saved as `settings.toml`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub language: String,
    pub panels: PanelSettings,
    pub left: PanelState,
    pub right: PanelState,
    pub interface: InterfaceSettings,
    pub confirm: Confirmations,
    pub system: SystemSettings,
    pub viewer: ViewerSettings,
    pub editor: EditorSettings,
    /// Mask groups usable as `<name>` in masks.
    pub mask_groups: BTreeMap<String, String>,
    pub associations: Vec<Association>,
    /// F9 → Commands → Encoded archives.
    pub encoded_archives: Vec<EncodedArchive>,
    pub highlight: Vec<HighlightRule>,
    /// Palette overrides: `"Panel.Text" = "LightCyan on Blue"`.
    pub colors: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PanelSettings {
    pub show_hidden: bool,
    pub highlight: bool,
    pub select_folders: bool,
    pub natural_sort: bool,
    pub show_dotdot_in_root: bool,
    pub show_column_titles: bool,
    pub show_status_line: bool,
    pub show_total: bool,
    pub show_free_space: bool,
    pub show_scrollbar: bool,
    pub auto_refresh: bool,
    pub right_click_selects: bool,
    /// Ctrl+Up/Down panel height adjustment (0 = full height).
    pub height_reduce: u16,
    /// Left panel width share in percent (Ctrl+Left/Right).
    pub left_width_percent: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PanelState {
    pub path: String,
    /// View mode 0–9 (Ctrl+0…9): 1 = brief, 2 = medium, 3 = full, 4 = wide,
    /// 5 = detailed, 6 = descriptions, 7 = long descriptions, 8 = owners,
    /// 9 = links, 0 = alternative full.
    pub mode: u8,
    pub sort: String,
    pub reverse: bool,
    pub visible: bool,
    /// "files", "tree", "info" or "qview".
    pub kind: String,
    pub sort_groups: bool,
    pub selected_first: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct InterfaceSettings {
    pub clock: bool,
    pub keybar: bool,
    pub menubar_always: bool,
    pub show_screen_number: bool,
    pub mouse: bool,
    pub screensaver_minutes: u32,
    pub console_title: bool,
    pub alt_keymap: bool,
    /// "auto", "16", "256" or "truecolor".
    pub colors: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Confirmations {
    pub copy: bool,
    pub move_: bool,
    pub overwrite: bool,
    pub drag: bool,
    pub delete: bool,
    pub delete_nonempty_folders: bool,
    pub interrupt: bool,
    pub exit: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SystemSettings {
    pub delete_to_trash: bool,
    pub copy_links_as_links: bool,
    pub preserve_times: bool,
    pub history_size: usize,
    pub save_history: bool,
    pub save_folder_history: bool,
    pub save_view_history: bool,
    pub auto_save_setup: bool,
    pub word_div: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ViewerSettings {
    pub wrap: bool,
    pub word_wrap: bool,
    pub tab_size: usize,
    pub show_arrows: bool,
    pub show_scrollbar: bool,
    pub save_position: bool,
    pub auto_detect_codepage: bool,
    /// Code page id used when detection is off (65001 = UTF-8).
    pub default_codepage: u32,
    pub hex_uppercase: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EditorSettings {
    pub tab_size: usize,
    pub expand_tabs: bool,
    pub auto_indent: bool,
    pub persistent_blocks: bool,
    pub del_removes_blocks: bool,
    pub save_position: bool,
    pub show_line_numbers: bool,
    pub show_whitespace: bool,
    pub cursor_beyond_eol: bool,
    pub auto_detect_codepage: bool,
    pub default_codepage: u32,
    /// "native", "crlf", "lf" or "cr" for new files.
    pub new_file_eol: String,
    pub lock_readonly: bool,
    pub warn_readonly: bool,
}

/// File association (F9 → Commands → File associations).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Association {
    pub masks: String,
    pub description: String,
    /// Enter
    pub exec: String,
    /// Ctrl+PgDn
    pub alt_exec: String,
    /// F3
    pub view: String,
    /// Alt+F3
    pub alt_view: String,
    /// F4
    pub edit: String,
    /// Alt+F4
    pub alt_edit: String,
}

/// Encoded archive configuration (F9 → Commands → Encoded archives): files
/// named `*.<newExtension>` are decoded (`gm --d`, or `gm --pd` with a
/// password) into `*.<originalExtension>` when opened with F3, F4 or Enter,
/// and encoded back (`gm --e` / `gm --pe`) when the decoded copy is saved.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct EncodedArchive {
    /// Extension of the encoded files, without the dot (up to 7 characters).
    pub new_extension: String,
    /// Extension of the decoded file, without the dot (up to 7 characters).
    pub original_extension: String,
    /// Ask for a password that unmasks the key file (`--pd` / `--pe`).
    pub password_protected: bool,
    /// The key file with the operations; relative paths are taken from the
    /// settings folder.
    pub key_file: String,
}

impl EncodedArchive {
    /// Maximum length of `new_extension` / `original_extension`.
    pub const MAX_EXTENSION: usize = 7;
}

/// File highlighting rule (F9 → Options → Files highlighting).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct HighlightRule {
    pub masks: String,
    pub directories: Option<bool>,
    pub hidden: Option<bool>,
    pub system: Option<bool>,
    pub executable: Option<bool>,
    pub links: Option<bool>,
    /// Color name: "LightGreen", "White"…
    pub color: String,
    pub cursor_color: String,
    /// A mark character shown before the name.
    pub mark: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: "English".into(),
            panels: PanelSettings::default(),
            left: PanelState::default(),
            right: PanelState::default(),
            interface: InterfaceSettings::default(),
            confirm: Confirmations::default(),
            system: SystemSettings::default(),
            viewer: ViewerSettings::default(),
            editor: EditorSettings::default(),
            mask_groups: gm_default_groups(),
            associations: Vec::new(),
            encoded_archives: Vec::new(),
            highlight: default_highlight(),
            colors: BTreeMap::new(),
        }
    }
}

fn gm_default_groups() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("arc".into(), "*.rar,*.zip,*.[zj],*.[bg7x]z,*.[bg]zip,*.tar,*.t[agbx]z,*.ar[cj],*.r[0-9][0-9],*.a[0-9][0-9],*.bz2,*.cab,*.msi,*.jar,*.lha,*.lzh,*.ha,*.ac[bei],*.pa[ck],*.rk,*.cpio,*.rpm,*.zoo,*.hqx,*.sit,*.ice,*.uc2,*.ain,*.imp,*.777,*.ufa,*.boa,*.bs[2a],*.sea,*.hpk,*.ddi,*.x2,*.rkv,*.[lw]sz,*.h[ay]p,*.lim,*.sqz,*.chz,*.zst,*.iso,*.deb".into());
    m.insert("temp".into(), "*.bak,*.tmp,*.~*,*.swp,~*".into());
    m.insert("exec".into(), "*.exe,*.com,*.bat,*.cmd,*.vbs,*.vbe,*.js,*.jse,*.wsf,*.wsh,*.ps1,*.sh".into());
    m
}

/// Far's default highlighting: hidden/system dim, folders white,
/// executables green, archives magenta, temporary files brown.
pub fn default_highlight() -> Vec<HighlightRule> {
    let r = |masks: &str, color: &str, cursor: &str| HighlightRule {
        masks: masks.into(),
        color: color.into(),
        cursor_color: cursor.into(),
        ..Default::default()
    };
    vec![
        HighlightRule { hidden: Some(true), ..r("*", "Cyan", "DarkGray") },
        HighlightRule { system: Some(true), ..r("*", "Cyan", "DarkGray") },
        HighlightRule { directories: Some(true), ..r("*", "White", "White") },
        HighlightRule { executable: Some(true), ..r("*", "LightGreen", "LightGreen") },
        r("<exec>", "LightGreen", "LightGreen"),
        r("<arc>", "LightMagenta", "LightMagenta"),
        r("<temp>", "Brown", "Brown"),
    ]
}

impl Default for PanelSettings {
    fn default() -> Self {
        PanelSettings {
            show_hidden: true,
            highlight: true,
            select_folders: false,
            natural_sort: true,
            show_dotdot_in_root: false,
            show_column_titles: true,
            show_status_line: true,
            show_total: true,
            show_free_space: true,
            show_scrollbar: false,
            auto_refresh: true,
            right_click_selects: false,
            height_reduce: 0,
            left_width_percent: 50,
        }
    }
}

impl Default for PanelState {
    fn default() -> Self {
        PanelState {
            path: String::new(),
            mode: 2,
            sort: "name".into(),
            reverse: false,
            visible: true,
            kind: "files".into(),
            sort_groups: false,
            selected_first: false,
        }
    }
}

impl Default for InterfaceSettings {
    fn default() -> Self {
        InterfaceSettings {
            clock: true,
            keybar: true,
            menubar_always: false,
            show_screen_number: true,
            mouse: true,
            screensaver_minutes: 0,
            console_title: true,
            alt_keymap: true,
            colors: "auto".into(),
        }
    }
}

impl Default for Confirmations {
    fn default() -> Self {
        Confirmations {
            copy: false,
            move_: false,
            overwrite: true,
            drag: true,
            delete: true,
            delete_nonempty_folders: true,
            interrupt: true,
            exit: true,
        }
    }
}

impl Default for SystemSettings {
    fn default() -> Self {
        SystemSettings {
            delete_to_trash: true,
            copy_links_as_links: cfg!(unix),
            preserve_times: true,
            history_size: 1000,
            save_history: true,
            save_folder_history: true,
            save_view_history: true,
            auto_save_setup: true,
            word_div: "~!%^&*()+|{}:\"<>?`-=\\[];',./".into(),
        }
    }
}

impl Default for ViewerSettings {
    fn default() -> Self {
        ViewerSettings {
            wrap: true,
            word_wrap: false,
            tab_size: 8,
            show_arrows: true,
            show_scrollbar: false,
            save_position: true,
            auto_detect_codepage: true,
            default_codepage: 65001,
            hex_uppercase: true,
        }
    }
}

impl Default for EditorSettings {
    fn default() -> Self {
        EditorSettings {
            tab_size: 8,
            expand_tabs: false,
            auto_indent: false,
            persistent_blocks: false,
            del_removes_blocks: true,
            save_position: true,
            show_line_numbers: false,
            show_whitespace: false,
            cursor_beyond_eol: true,
            auto_detect_codepage: true,
            default_codepage: 65001,
            new_file_eol: "native".into(),
            lock_readonly: false,
            warn_readonly: true,
        }
    }
}

impl Settings {
    /// Loads settings; a missing file gives defaults, a broken one gives
    /// defaults plus the parse error (shown to the user once).
    pub fn load(path: &Path) -> (Settings, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(s) => (s, None),
                Err(e) => (Settings::default(), Some(format!("{}: {e}", path.display()))),
            },
            Err(_) => (Settings::default(), None),
        }
    }

    /// Writes atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    /// Flattened `section.key = value` view for the gm:config editor.
    pub fn flatten(&self) -> Vec<(String, toml::Value)> {
        let mut out = Vec::new();
        if let Ok(toml::Value::Table(t)) = toml::Value::try_from(self) {
            flatten_into("", &toml::Value::Table(t), &mut out);
        }
        out
    }

    /// Sets one flattened value (as printed by `flatten`), parsing `raw` to the existing type.
    pub fn set_flat(&mut self, key: &str, raw: &str) -> Result<(), String> {
        let mut root = toml::Value::try_from(&*self).map_err(|e| e.to_string())?;
        let mut cur = &mut root;
        let parts: Vec<&str> = key.split('.').collect();
        for p in &parts[..parts.len() - 1] {
            cur = cur.get_mut(*p).ok_or_else(|| format!("unknown key {key}"))?;
        }
        let last = parts[parts.len() - 1];
        let slot = cur.get_mut(last).ok_or_else(|| format!("unknown key {key}"))?;
        *slot = match slot {
            toml::Value::Boolean(_) => toml::Value::Boolean(matches!(raw.trim(), "true" | "1" | "yes" | "on")),
            toml::Value::Integer(_) => toml::Value::Integer(raw.trim().parse().map_err(|_| "expected a number".to_string())?),
            toml::Value::Float(_) => toml::Value::Float(raw.trim().parse().map_err(|_| "expected a number".to_string())?),
            _ => toml::Value::String(raw.to_string()),
        };
        *self = root.try_into().map_err(|e: toml::de::Error| e.to_string())?;
        Ok(())
    }
}

fn flatten_into(prefix: &str, v: &toml::Value, out: &mut Vec<(String, toml::Value)>) {
    match v {
        toml::Value::Table(t) => {
            for (k, v) in t {
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten_into(&key, v, out);
            }
        }
        toml::Value::Array(_) => {}
        other => out.push((prefix.to_string(), other.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_flat() {
        let mut s = Settings::default();
        s.left.mode = 3;
        let text = toml::to_string_pretty(&s).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back, s);
        // Unknown/missing keys fall back to defaults.
        let partial: Settings = toml::from_str("[editor]\ntab_size = 4\n").unwrap();
        assert_eq!(partial.editor.tab_size, 4);
        assert!(partial.editor.del_removes_blocks);
        s.set_flat("editor.tab_size", "2").unwrap();
        s.set_flat("panels.show_hidden", "false").unwrap();
        assert_eq!(s.editor.tab_size, 2);
        assert!(!s.panels.show_hidden);
        assert!(s.flatten().iter().any(|(k, _)| k == "viewer.wrap"));
        assert!(s.set_flat("editor.tab_size", "abc").is_err());
    }

    #[test]
    fn encoded_archives_roundtrip() {
        let mut s = Settings::default();
        s.encoded_archives.push(EncodedArchive {
            new_extension: "zipx".into(),
            original_extension: "zip".into(),
            password_protected: true,
            key_file: "key.txt".into(),
        });
        let text = toml::to_string_pretty(&s).unwrap();
        assert!(text.contains("[[encoded_archives]]"), "{text}");
        assert!(text.contains("newExtension = \"zipx\""), "{text}");
        assert!(text.contains("originalExtension = \"zip\""), "{text}");
        assert!(text.contains("passwordProtected = true"), "{text}");
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back, s);
    }
}

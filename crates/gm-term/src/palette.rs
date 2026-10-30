//! Far's 16-color palette and the named UI color elements.

pub use crate::palette_gen::{ALL, DEFAULTS, PALETTE_COUNT, PaletteId};
use ratatui::style::{Color, Style};
use std::cell::RefCell;
use std::collections::BTreeMap;

/// Console colors in Far's index order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum C16 {
    Black,
    Blue,
    Green,
    Cyan,
    Red,
    Magenta,
    Brown,
    Lightgray,
    Darkgray,
    Lightblue,
    Lightgreen,
    Lightcyan,
    Lightred,
    Lightmagenta,
    Yellow,
    White,
    /// Terminal default (used for the command line).
    Default,
}

pub const NAMES: [(&str, C16); 17] = [
    ("Black", C16::Black),
    ("Blue", C16::Blue),
    ("Green", C16::Green),
    ("Cyan", C16::Cyan),
    ("Red", C16::Red),
    ("Magenta", C16::Magenta),
    ("Brown", C16::Brown),
    ("LightGray", C16::Lightgray),
    ("DarkGray", C16::Darkgray),
    ("LightBlue", C16::Lightblue),
    ("LightGreen", C16::Lightgreen),
    ("LightCyan", C16::Lightcyan),
    ("LightRed", C16::Lightred),
    ("LightMagenta", C16::Lightmagenta),
    ("Yellow", C16::Yellow),
    ("White", C16::White),
    ("Default", C16::Default),
];

/// Classic Windows console RGB values (Far's default RGB palette).
const RGB: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (0, 0, 128),
    (0, 128, 0),
    (0, 128, 128),
    (128, 0, 0),
    (128, 0, 128),
    (128, 128, 0),
    (192, 192, 192),
    (128, 128, 128),
    (0, 0, 255),
    (0, 255, 0),
    (0, 255, 255),
    (255, 0, 0),
    (255, 0, 255),
    (255, 255, 0),
    (255, 255, 255),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    /// ANSI 16 colors: follows the terminal theme, like Far follows the console palette.
    Ansi16,
    /// Exact RGB values of Far's classic palette.
    TrueColor,
}

impl C16 {
    pub fn from_name(s: &str) -> Option<C16> {
        NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(s.trim())).map(|&(_, c)| c)
    }

    pub fn name(self) -> &'static str {
        NAMES.iter().find(|(_, c)| *c == self).map(|(n, _)| *n).unwrap_or("Default")
    }

    pub fn index(self) -> Option<usize> {
        NAMES.iter().position(|(_, c)| *c == self).filter(|&i| i < 16)
    }

    pub fn to_color(self, mode: ColorMode) -> Color {
        let Some(i) = self.index() else { return Color::Reset };
        if mode == ColorMode::TrueColor {
            let (r, g, b) = RGB[i];
            return Color::Rgb(r, g, b);
        }
        match self {
            C16::Black => Color::Black,
            C16::Blue => Color::Blue,
            C16::Green => Color::Green,
            C16::Cyan => Color::Cyan,
            C16::Red => Color::Red,
            C16::Magenta => Color::Magenta,
            C16::Brown => Color::Yellow,
            C16::Lightgray => Color::Gray,
            C16::Darkgray => Color::DarkGray,
            C16::Lightblue => Color::LightBlue,
            C16::Lightgreen => Color::LightGreen,
            C16::Lightcyan => Color::LightCyan,
            C16::Lightred => Color::LightRed,
            C16::Lightmagenta => Color::LightMagenta,
            C16::Yellow => Color::LightYellow,
            C16::White => Color::White,
            C16::Default => Color::Reset,
        }
    }
}

struct State {
    colors: Vec<(C16, C16)>,
    mode: ColorMode,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        colors: DEFAULTS.iter().map(|&(_, f, b)| (f, b)).collect(),
        mode: ColorMode::Ansi16,
    });
}

/// Style of a UI element in the current palette.
pub fn style(id: PaletteId) -> Style {
    STATE.with(|s| {
        let s = s.borrow();
        let (f, b) = s.colors[id as usize];
        Style::default().fg(f.to_color(s.mode)).bg(b.to_color(s.mode))
    })
}

/// Raw (fg, bg) of an element.
pub fn colors(id: PaletteId) -> (C16, C16) {
    STATE.with(|s| s.borrow().colors[id as usize])
}

pub fn set_colors(id: PaletteId, fg: C16, bg: C16) {
    STATE.with(|s| s.borrow_mut().colors[id as usize] = (fg, bg));
}

/// A style from explicit colors in the current mode.
pub fn style_of(fg: C16, bg: C16) -> Style {
    let mode = color_mode();
    Style::default().fg(fg.to_color(mode)).bg(bg.to_color(mode))
}

pub fn color_mode() -> ColorMode {
    STATE.with(|s| s.borrow().mode)
}

pub fn set_color_mode(mode: ColorMode) {
    STATE.with(|s| s.borrow_mut().mode = mode);
}

/// Picks the color mode from a setting ("auto", "16", "truecolor") and the environment.
pub fn detect_color_mode(setting: &str) -> ColorMode {
    match setting {
        "16" => ColorMode::Ansi16,
        "truecolor" | "24bit" | "rgb" => ColorMode::TrueColor,
        _ => {
            let ct = std::env::var("COLORTERM").unwrap_or_default();
            // Windows Terminal renders ANSI blue very differently from Far's
            // classic palette; it supports true color, so use exact values there.
            if ct == "truecolor" || ct == "24bit" || std::env::var_os("WT_SESSION").is_some() { ColorMode::TrueColor } else { ColorMode::Ansi16 }
        }
    }
}

pub fn reset_defaults() {
    STATE.with(|s| s.borrow_mut().colors = DEFAULTS.iter().map(|&(_, f, b)| (f, b)).collect());
}

/// Applies overrides like `"Panel.Text" = "LightCyan on Blue"`; returns unknown keys.
pub fn apply_overrides(map: &BTreeMap<String, String>) -> Vec<String> {
    let mut bad = Vec::new();
    for (k, v) in map {
        let Some(i) = DEFAULTS.iter().position(|(n, _, _)| n == k) else {
            bad.push(k.clone());
            continue;
        };
        match parse_pair(v) {
            Some((f, b)) => set_colors(ALL[i], f, b),
            None => bad.push(k.clone()),
        }
    }
    bad
}

/// Overrides that differ from the defaults (for saving).
pub fn changed_overrides() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for (i, &(name, df, db)) in DEFAULTS.iter().enumerate() {
        let (f, b) = colors(ALL[i]);
        if (f, b) != (df, db) {
            m.insert(name.to_string(), format!("{} on {}", f.name(), b.name()));
        }
    }
    m
}

pub fn parse_pair(v: &str) -> Option<(C16, C16)> {
    let (f, b) = v.split_once(" on ")?;
    Some((C16::from_name(f)?, C16::from_name(b)?))
}

pub fn element_name(id: PaletteId) -> &'static str {
    DEFAULTS[id as usize].0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_overrides() {
        assert_eq!(colors(PaletteId::PanelText), (C16::Lightcyan, C16::Blue));
        assert_eq!(colors(PaletteId::DialogText), (C16::Black, C16::Lightgray));
        let mut m = BTreeMap::new();
        m.insert("Panel.Text".to_string(), "White on Black".to_string());
        m.insert("Nope".to_string(), "x".to_string());
        assert_eq!(apply_overrides(&m), vec!["Nope".to_string()]);
        assert_eq!(colors(PaletteId::PanelText), (C16::White, C16::Black));
        assert_eq!(changed_overrides().len(), 1);
        reset_defaults();
        assert!(changed_overrides().is_empty());
    }
}

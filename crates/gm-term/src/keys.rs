//! Normalized key chords (key + Ctrl/Alt/Shift), parsing and display.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::fmt;

/// Modifier bit set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Mods(pub u8);

impl Mods {
    pub const NONE: Mods = Mods(0);
    pub const CTRL: Mods = Mods(1);
    pub const ALT: Mods = Mods(2);
    pub const SHIFT: Mods = Mods(4);
    pub const CTRL_SHIFT: Mods = Mods(5);
    pub const ALT_SHIFT: Mods = Mods(6);
    pub const CTRL_ALT: Mods = Mods(3);

    pub fn ctrl(self) -> bool {
        self.0 & 1 != 0
    }
    pub fn alt(self) -> bool {
        self.0 & 2 != 0
    }
    pub fn shift(self) -> bool {
        self.0 & 4 != 0
    }
}

impl std::ops::BitOr for Mods {
    type Output = Mods;
    fn bitor(self, r: Mods) -> Mods {
        Mods(self.0 | r.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub code: KeyCode,
    pub mods: Mods,
}

impl KeyChord {
    pub const fn new(code: KeyCode, mods: Mods) -> KeyChord {
        KeyChord { code, mods }
    }
    pub const fn plain(code: KeyCode) -> KeyChord {
        KeyChord { code, mods: Mods::NONE }
    }
    pub const fn ctrl(code: KeyCode) -> KeyChord {
        KeyChord { code, mods: Mods::CTRL }
    }
    pub const fn alt(code: KeyCode) -> KeyChord {
        KeyChord { code, mods: Mods::ALT }
    }
    pub const fn shift(code: KeyCode) -> KeyChord {
        KeyChord { code, mods: Mods::SHIFT }
    }

    /// A printable character typed without Ctrl/Alt, if any.
    pub fn text_char(&self) -> Option<char> {
        match self.code {
            KeyCode::Char(c) if !self.mods.ctrl() && !self.mods.alt() => Some(c),
            _ => None,
        }
    }

    /// Normalizes a crossterm event. Returns `None` for key releases.
    ///
    /// * Shifted printable characters drop the Shift flag (`Shift+a` → `A`).
    /// * With Ctrl/Alt, letters are lowercased and Shift is kept (`Ctrl+Shift+A`).
    /// * AltGr (reported as Ctrl+Alt on Windows) followed by a non-letter is a plain character.
    /// * `BackTab` becomes `Shift+Tab`.
    pub fn from_event(ev: &KeyEvent) -> Option<KeyChord> {
        if ev.kind == KeyEventKind::Release {
            return None;
        }
        let mut mods = Mods::NONE;
        if ev.modifiers.contains(KeyModifiers::CONTROL) {
            mods = mods | Mods::CTRL;
        }
        if ev.modifiers.contains(KeyModifiers::ALT) {
            mods = mods | Mods::ALT;
        }
        if ev.modifiers.contains(KeyModifiers::SHIFT) {
            mods = mods | Mods::SHIFT;
        }
        let code = match ev.code {
            KeyCode::BackTab => {
                mods = mods | Mods::SHIFT;
                KeyCode::Tab
            }
            KeyCode::Char(c) => {
                if mods.ctrl() && mods.alt() && !c.is_ascii_alphanumeric() {
                    // AltGr character on European layouts.
                    mods = Mods::NONE;
                    KeyCode::Char(c)
                } else if mods.ctrl() || mods.alt() {
                    if c.is_uppercase() {
                        mods = mods | Mods::SHIFT;
                    }
                    KeyCode::Char(c.to_lowercase().next().unwrap_or(c))
                } else {
                    mods = Mods::NONE;
                    KeyCode::Char(c)
                }
            }
            other => other,
        };
        Some(KeyChord { code, mods })
    }

    /// Parses "Ctrl+Shift+F3", "Alt+X", "Enter", "Ctrl+\\"…
    pub fn parse(s: &str) -> Option<KeyChord> {
        let s = s.trim();
        let mut mods = Mods::NONE;
        let mut rest = s;
        loop {
            let lower = rest.to_ascii_lowercase();
            let (m, len) = if lower.starts_with("ctrl+") {
                (Mods::CTRL, 5)
            } else if lower.starts_with("alt+") {
                (Mods::ALT, 4)
            } else if lower.starts_with("shift+") {
                (Mods::SHIFT, 6)
            } else if lower.starts_with("rctrl+") {
                (Mods::CTRL, 6)
            } else if lower.starts_with("ralt+") {
                (Mods::ALT, 5)
            } else {
                break;
            };
            if rest.len() == len {
                break;
            }
            mods = mods | m;
            rest = &rest[len..];
        }
        let code = match rest.to_ascii_lowercase().as_str() {
            "enter" | "return" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "bs" | "backspace" => KeyCode::Backspace,
            "del" | "delete" => KeyCode::Delete,
            "ins" | "insert" => KeyCode::Insert,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pgup" | "pageup" => KeyCode::PageUp,
            "pgdn" | "pagedown" => KeyCode::PageDown,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "space" => KeyCode::Char(' '),
            f if f.len() >= 2 && f.starts_with('f') && f[1..].parse::<u8>().is_ok() => KeyCode::F(f[1..].parse().ok()?),
            _ => {
                let mut it = rest.chars();
                let c = it.next()?;
                if it.next().is_some() {
                    return None;
                }
                // In key names the letter case is not significant: "Ctrl+L" is
                // Ctrl+l; Shift must be written explicitly ("Ctrl+Shift+L").
                if mods.ctrl() || mods.alt() { KeyCode::Char(c.to_lowercase().next()?) } else { KeyCode::Char(c) }
            }
        };
        Some(KeyChord { code, mods })
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mods.ctrl() {
            f.write_str("Ctrl+")?;
        }
        if self.mods.alt() {
            f.write_str("Alt+")?;
        }
        if self.mods.shift() {
            f.write_str("Shift+")?;
        }
        match self.code {
            KeyCode::Enter => f.write_str("Enter"),
            KeyCode::Esc => f.write_str("Esc"),
            KeyCode::Tab => f.write_str("Tab"),
            KeyCode::Backspace => f.write_str("BS"),
            KeyCode::Delete => f.write_str("Del"),
            KeyCode::Insert => f.write_str("Ins"),
            KeyCode::Home => f.write_str("Home"),
            KeyCode::End => f.write_str("End"),
            KeyCode::PageUp => f.write_str("PgUp"),
            KeyCode::PageDown => f.write_str("PgDn"),
            KeyCode::Up => f.write_str("Up"),
            KeyCode::Down => f.write_str("Down"),
            KeyCode::Left => f.write_str("Left"),
            KeyCode::Right => f.write_str("Right"),
            KeyCode::F(n) => write!(f, "F{n}"),
            KeyCode::Char(' ') => f.write_str("Space"),
            KeyCode::Char(c) => {
                if self.mods.ctrl() || self.mods.alt() {
                    write!(f, "{}", c.to_uppercase())
                } else {
                    write!(f, "{c}")
                }
            }
            other => write!(f, "{other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventState;

    fn ev(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent { code, modifiers: m, kind: KeyEventKind::Press, state: KeyEventState::NONE }
    }

    #[test]
    fn normalize() {
        let k = KeyChord::from_event(&ev(KeyCode::Char('A'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(k, KeyChord::plain(KeyCode::Char('A')));
        let k = KeyChord::from_event(&ev(KeyCode::Char('A'), KeyModifiers::CONTROL | KeyModifiers::SHIFT)).unwrap();
        assert_eq!(k, KeyChord::new(KeyCode::Char('a'), Mods::CTRL_SHIFT));
        let k = KeyChord::from_event(&ev(KeyCode::Char('@'), KeyModifiers::CONTROL | KeyModifiers::ALT)).unwrap();
        assert_eq!(k, KeyChord::plain(KeyCode::Char('@')));
        let k = KeyChord::from_event(&ev(KeyCode::BackTab, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(k, KeyChord::shift(KeyCode::Tab));
    }

    #[test]
    fn parse_display_roundtrip() {
        for s in ["Ctrl+F3", "Alt+Shift+F9", "Ctrl+Shift+A", "Enter", "Ctrl+\\", "Shift+Del", "Ctrl+PgUp", "Space"] {
            let k = KeyChord::parse(s).unwrap_or_else(|| panic!("{s}"));
            assert_eq!(k.to_string(), s);
        }
        assert_eq!(KeyChord::parse("Ctrl++"), Some(KeyChord::ctrl(KeyCode::Char('+'))));
        assert_eq!(KeyChord::parse("Ctrl+L"), Some(KeyChord::ctrl(KeyCode::Char('l'))));
    }
}

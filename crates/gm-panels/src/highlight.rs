//! File highlighting (`hilight.cpp`): masks + attributes → colors and marks.

use gm_config::HighlightRule;
use gm_term::C16;
use gm_text::FileMask;
use gm_vfs::Entry;
use std::collections::HashMap;

struct Compiled {
    mask: FileMask,
    rule: HighlightRule,
    color: Option<C16>,
    cursor: Option<C16>,
}

#[derive(Default)]
pub struct Highlighter {
    rules: Vec<Compiled>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Highlight {
    pub color: Option<C16>,
    pub cursor_color: Option<C16>,
    pub mark: Option<char>,
}

impl Highlighter {
    pub fn new(rules: &[HighlightRule], groups: &std::collections::BTreeMap<String, String>) -> Highlighter {
        let g: HashMap<String, String> = groups.iter().map(|(k, v)| (k.to_lowercase(), v.clone())).collect();
        let rules = rules
            .iter()
            .filter_map(|r| {
                let mask = FileMask::with_groups(if r.masks.is_empty() { "*" } else { &r.masks }, &g).ok()?;
                Some(Compiled { mask, color: C16::from_name(&r.color), cursor: C16::from_name(&r.cursor_color), rule: r.clone() })
            })
            .collect();
        Highlighter { rules }
    }

    /// First matching rule wins, as in Far's default (non-"continue") rules.
    pub fn get(&self, e: &Entry) -> Highlight {
        for c in &self.rules {
            let r = &c.rule;
            let ok = r.directories.is_none_or(|v| v == e.is_dir)
                && r.hidden.is_none_or(|v| v == e.hidden)
                && r.system.is_none_or(|v| v == e.system)
                && r.executable.is_none_or(|v| v == e.executable)
                && r.links.is_none_or(|v| v == e.is_link);
            if ok && c.mask.matches(&e.name) {
                return Highlight { color: c.color, cursor_color: c.cursor, mark: r.mark.chars().next() };
            }
        }
        Highlight::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let s = gm_config::Settings::default();
        let h = Highlighter::new(&s.highlight, &s.mask_groups);
        let dir = Entry { name: "src".into(), is_dir: true, ..Default::default() };
        assert_eq!(h.get(&dir).color, Some(C16::White));
        let arc = Entry { name: "x.zip".into(), ..Default::default() };
        assert_eq!(h.get(&arc).color, Some(C16::Lightmagenta));
        let hid = Entry { name: "x.zip".into(), hidden: true, ..Default::default() };
        assert_eq!(h.get(&hid).color, Some(C16::Cyan));
        let plain = Entry { name: "readme".into(), ..Default::default() };
        assert_eq!(h.get(&plain).color, None);
    }
}

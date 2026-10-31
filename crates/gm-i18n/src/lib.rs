//! Localized messages for Goodevening Manager.
//!
//! Message tables come from Far's `farlang.templ.m4` (converted once by
//! `tools/lng-convert`) and are embedded in the binary. A missing translation
//! falls back to English. The language can be switched at runtime.
#![forbid(unsafe_code)]

use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

include!(concat!(env!("OUT_DIR"), "/msg.rs"));

static TABLES: OnceLock<Vec<OnceLock<Vec<&'static str>>>> = OnceLock::new();
static CURRENT: AtomicUsize = AtomicUsize::new(usize::MAX);

fn english_index() -> usize {
    LANGS.iter().position(|l| l.0 == "English").unwrap_or(0)
}

fn table(i: usize) -> &'static [&'static str] {
    let tables = TABLES.get_or_init(|| LANGS.iter().map(|_| OnceLock::new()).collect());
    tables[i].get_or_init(|| parse_lng(LANGS[i].2))
}

/// Parses a `.lng` file: every line starting with `"` is the next message.
fn parse_lng(text: &'static str) -> Vec<&'static str> {
    text.lines()
        .filter(|l| l.starts_with('"'))
        .map(|l| {
            let inner = l.strip_prefix('"').unwrap_or(l);
            let inner = inner.strip_suffix('"').unwrap_or(inner);
            if inner.contains('\\') { &*Box::leak(unescape(inner).into_boxed_str()) } else { inner }
        })
        .collect()
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn current() -> usize {
    let c = CURRENT.load(Ordering::Relaxed);
    if c == usize::MAX { english_index() } else { c }
}

/// Returns the message in the current language (with `&` hotkey markers).
pub fn msg(id: MsgId) -> &'static str {
    let i = id as usize;
    let t = table(current());
    match t.get(i) {
        Some(s) if !s.is_empty() => s,
        _ => table(english_index()).get(i).copied().unwrap_or(""),
    }
}

/// Message with `&` hotkey markers removed.
pub fn plain(id: MsgId) -> String {
    strip_hotkey(msg(id))
}

/// Removes the first `&` marker (`&&` stays a literal ampersand).
pub fn strip_hotkey(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '&' {
            if it.peek() == Some(&'&') {
                out.push('&');
                it.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Formats a message with `{0}`, `{1}`… placeholders.
pub fn fmt(id: MsgId, args: &[&dyn Display]) -> String {
    format_with(msg(id), args)
}

pub fn format_with(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let key = &after[..close];
                let spec = key.split(':').next().unwrap_or("");
                if let Ok(n) = spec.parse::<usize>() {
                    if let Some(a) = args.get(n) {
                        out.push_str(&a.to_string());
                    }
                    rest = &after[close + 1..];
                } else {
                    out.push('{');
                    rest = after;
                }
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Available languages as (name, native description), sorted by the native
/// description in UTF-8 byte order (the order of the language menu).
pub fn languages() -> Vec<(&'static str, &'static str)> {
    let mut v: Vec<_> = LANGS.iter().map(|l| (l.0, l.1)).collect();
    v.sort_by(|a, b| a.1.as_bytes().cmp(b.1.as_bytes()));
    v
}

/// Switches the UI language by English name ("Russian", "Ukrainian"…).
pub fn set_language(name: &str) -> bool {
    match LANGS.iter().position(|l| l.0.eq_ignore_ascii_case(name)) {
        Some(i) => {
            CURRENT.store(i, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

pub fn current_language() -> &'static str {
    LANGS[current()].0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_default_and_switch() {
        assert_eq!(msg(MsgId::MYes), "Yes");
        assert!(set_language("Ukrainian"));
        assert_eq!(msg(MsgId::MYes), "Так");
        set_language("English");
        assert_eq!(plain(MsgId::MMenuLeftTitle), "Left");
    }

    #[test]
    fn placeholders() {
        assert_eq!(format_with("Copy {0} {1}:", &[&"3", &"files"]), "Copy 3 files:");
        assert_eq!(strip_hotkey("&Save && exit"), "Save & exit");
    }

    #[test]
    fn languages_sorted_by_native_name() {
        let names: Vec<&str> = languages().iter().map(|l| l.1).collect();
        let mut sorted = names.clone();
        sorted.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        assert_eq!(names, sorted);
        assert!(names.contains(&"Norsk"));
    }

    #[test]
    fn every_language_has_all_messages() {
        for i in 0..LANGS.len() {
            assert_eq!(table(i).len(), MSG_COUNT, "{}", LANGS[i].0);
        }
    }
}

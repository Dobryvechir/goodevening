//! Far-style file masks (replaces `filemasks.cpp`).
//!
//! Syntax: `*.cpp,*.h;"name with, comma"|tmp*,*.bak` — masks separated by
//! `,` or `;`, an optional `|` starts the exclude list, `/regex/i` is a
//! regular expression mask and `<group>` refers to a named mask group.
//! Matching is case-insensitive, as in Far. `*.*` also matches names without
//! an extension and `*.` matches only names without one.

use regex::Regex;
use std::collections::HashMap;

#[derive(Clone, Debug)]
enum Pattern {
    Wild(Vec<char>),
    Re(Regex),
}

#[derive(Clone, Debug, Default)]
pub struct FileMask {
    include: Vec<Pattern>,
    exclude: Vec<Pattern>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MaskError(pub String);

impl std::fmt::Display for MaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid file mask: {}", self.0)
    }
}

impl std::error::Error for MaskError {}

impl FileMask {
    pub fn new(spec: &str) -> Result<FileMask, MaskError> {
        Self::with_groups(spec, &HashMap::new())
    }

    /// `groups` maps names (without `<>`) to mask strings, e.g. `arc` → `*.zip,*.7z`.
    pub fn with_groups(spec: &str, groups: &HashMap<String, String>) -> Result<FileMask, MaskError> {
        let mut m = FileMask::default();
        let (inc, exc) = split_exclude(spec);
        parse_list(inc, groups, &mut m.include, 0)?;
        if let Some(exc) = exc {
            parse_list(exc, groups, &mut m.exclude, 0)?;
        }
        if m.include.is_empty() {
            if exc.is_some() {
                m.include.push(Pattern::Wild(vec!['*']));
            } else {
                return Err(MaskError(spec.to_string()));
            }
        }
        Ok(m)
    }

    pub fn all() -> FileMask {
        FileMask { include: vec![Pattern::Wild(vec!['*'])], exclude: vec![] }
    }

    pub fn matches(&self, name: &str) -> bool {
        let lower: Vec<char> = name.to_lowercase().chars().collect();
        let hit = |p: &Pattern| match p {
            Pattern::Wild(w) => wild_match(w, &lower),
            Pattern::Re(r) => r.is_match(name),
        };
        self.include.iter().any(hit) && !self.exclude.iter().any(hit)
    }
}

fn split_exclude(spec: &str) -> (&str, Option<&str>) {
    let mut in_quote = false;
    let mut in_re = false;
    for (i, c) in spec.char_indices() {
        match c {
            '"' if !in_re => in_quote = !in_quote,
            '/' if !in_quote => in_re = !in_re,
            '|' if !in_quote && !in_re => return (&spec[..i], Some(&spec[i + 1..])),
            _ => {}
        }
    }
    (spec, None)
}

fn parse_list(s: &str, groups: &HashMap<String, String>, out: &mut Vec<Pattern>, depth: usize) -> Result<(), MaskError> {
    if depth > 8 {
        return Err(MaskError("mask groups nested too deeply".into()));
    }
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => in_quote = !in_quote,
            '/' if !in_quote && cur.trim().is_empty() => {
                // Regex mask: read up to the closing slash plus flags.
                let mut re = String::new();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        re.push(c);
                        if let Some(n) = chars.next() {
                            re.push(n);
                        }
                    } else if c == '/' {
                        closed = true;
                        break;
                    } else {
                        re.push(c);
                    }
                }
                if !closed {
                    return Err(MaskError(format!("/{re}")));
                }
                let mut flags = String::new();
                while let Some(&f) = chars.peek() {
                    if f.is_ascii_alphabetic() {
                        flags.push(f);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let ci = flags.contains('i');
                let pat = if ci { format!("(?i){re}") } else { re.clone() };
                out.push(Pattern::Re(Regex::new(&pat).map_err(|e| MaskError(e.to_string()))?));
            }
            ',' | ';' if !in_quote => items.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    items.push(cur);
    for item in items {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if let Some(g) = item.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
            let spec = groups.get(&g.to_lowercase()).ok_or_else(|| MaskError(format!("unknown group <{g}>")))?;
            parse_list(spec, groups, out, depth + 1)?;
            continue;
        }
        let w: Vec<char> = item.to_lowercase().chars().collect();
        out.push(Pattern::Wild(w));
    }
    Ok(())
}

/// Wildcard match on lowercase chars with Far's extension rules.
fn wild_match(p: &[char], name: &[char]) -> bool {
    // "*.*" matches everything; "x*." matches names with no dot.
    if p.ends_with(&['*', '.', '*']) && !name.contains(&'.') {
        return wild(&p[..p.len() - 2], name);
    }
    if p.last() == Some(&'.') && p.len() > 1 {
        return !name.contains(&'.') && wild(&p[..p.len() - 1], name);
    }
    wild(p, name)
}

fn wild(p: &[char], s: &[char]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while si < s.len() {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    star = Some(pi);
                    mark = si;
                    pi += 1;
                    continue;
                }
                '?' => {
                    pi += 1;
                    si += 1;
                    continue;
                }
                '[' => {
                    if let Some((matched, len)) = class(&p[pi..], s[si]) {
                        if matched {
                            pi += len;
                            si += 1;
                            continue;
                        }
                    } else if s[si] == '[' {
                        pi += 1;
                        si += 1;
                        continue;
                    }
                }
                c if c == s[si] => {
                    pi += 1;
                    si += 1;
                    continue;
                }
                _ => {}
            }
        }
        match star {
            Some(sp) => {
                pi = sp + 1;
                mark += 1;
                si = mark;
            }
            None => return false,
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Matches a `[a-z0-9]` class; returns (matched, pattern length).
fn class(p: &[char], c: char) -> Option<(bool, usize)> {
    let end = p.iter().position(|&x| x == ']')?;
    let body = &p[1..end];
    let mut i = 0;
    let mut hit = false;
    while i < body.len() {
        if i + 2 < body.len() && body[i + 1] == '-' {
            if body[i] <= c && c <= body[i + 2] {
                hit = true;
            }
            i += 3;
        } else {
            if body[i] == c {
                hit = true;
            }
            i += 1;
        }
    }
    Some((hit, end + 1))
}

/// Default mask groups, as in Far's "File masks" settings.
pub fn default_groups() -> HashMap<String, String> {
    let mut g = HashMap::new();
    g.insert(
        "arc".into(),
        "*.rar,*.zip,*.[zj],*.[bg7x]z,*.[bg]zip,*.tar,*.t[agbx]z,*.ar[cj],*.r[0-9][0-9],*.a[0-9][0-9],*.bz2,*.cab,*.msi,*.jar,*.lha,*.lzh,*.ha,*.ac[bei],*.pa[ck],*.rk,*.cpio,*.rpm,*.zoo,*.hqx,*.sit,*.ice,*.uc2,*.ain,*.imp,*.777,*.ufa,*.boa,*.bs[2a],*.sea,*.hpk,*.ddi,*.x2,*.rkv,*.[lw]sz,*.h[ay]p,*.lim,*.sqz,*.chz,*.zst,*.iso,*.deb".into(),
    );
    g.insert("temp".into(), "*.bak,*.tmp,*.~*,*.swp,~*".into());
    g.insert("exec".into(), "*.exe,*.com,*.bat,*.cmd,*.vbs,*.vbe,*.js,*.jse,*.wsf,*.wsh,*.ps1,*.sh,*.py,*.pl,*.rb".into());
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let m = FileMask::new("*.cpp,*.h").unwrap();
        assert!(m.matches("main.CPP"));
        assert!(m.matches("x.h"));
        assert!(!m.matches("x.hpp"));
    }

    #[test]
    fn far_rules() {
        assert!(FileMask::new("*.*").unwrap().matches("Makefile"));
        assert!(FileMask::new("*.").unwrap().matches("Makefile"));
        assert!(!FileMask::new("*.").unwrap().matches("a.txt"));
        assert!(FileMask::new("file?.[0-9]").unwrap().matches("file1.7"));
    }

    #[test]
    fn exclude_and_regex() {
        let m = FileMask::new("*.*|*.bak,tmp*").unwrap();
        assert!(m.matches("a.txt"));
        assert!(!m.matches("a.bak"));
        assert!(!m.matches("tmp1"));
        let r = FileMask::new("/^ab\\d+$/i").unwrap();
        assert!(r.matches("AB12"));
        assert!(!r.matches("abc"));
        let only_ex = FileMask::new("|*.o").unwrap();
        assert!(only_ex.matches("a.c") && !only_ex.matches("a.o"));
    }

    #[test]
    fn groups() {
        let m = FileMask::with_groups("<arc>", &default_groups()).unwrap();
        assert!(m.matches("x.zip") && m.matches("y.tar.gz") && !m.matches("z.txt"));
        assert!(FileMask::new("\"a, b\"").unwrap().matches("a, b"));
    }
}

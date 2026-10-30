//! Search worker thread.

use crossbeam_channel::{Receiver, Sender, unbounded};
use gm_text::regex_adapter::SearchSpec;
use gm_text::{Codepage, FileMask};
use gm_vfs::CancelToken;
use regex::bytes::{Regex, RegexBuilder};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    AllDrives,
    Path,
    Root,
    Current,
    CurrentOnly,
    Selected,
}

#[derive(Clone, Debug)]
pub struct FindParams {
    pub masks: String,
    pub text: SearchSpec,
    pub hex: bool,
    /// `None` = all standard code pages (UTF-8, UTF-16LE/BE, ANSI, OEM).
    pub codepage: Option<Codepage>,
    pub not_containing: bool,
    pub folders: bool,
    pub follow_links: bool,
    pub area: Area,
    pub skip_hidden: bool,
}

impl Default for FindParams {
    fn default() -> Self {
        FindParams {
            masks: "*".into(),
            text: SearchSpec::default(),
            hex: false,
            codepage: None,
            not_containing: false,
            folders: true,
            follow_links: false,
            area: Area::Current,
            skip_hidden: false,
        }
    }
}

#[derive(Clone, Debug)]
pub enum FindMsg {
    Found {
        path: PathBuf,
        is_dir: bool,
    },
    /// The folder being scanned.
    Scanning(PathBuf),
    Done,
    Error(String),
}

/// Compiles the content matchers (one per code page).
fn matchers(p: &FindParams) -> Result<Vec<Regex>, String> {
    if p.text.text.is_empty() {
        return Ok(Vec::new());
    }
    if p.hex {
        let bytes = gm_ui::search::parse_hex(&p.text.text).ok_or("invalid hex string")?;
        let pat: String = bytes.iter().map(|b| format!("\\x{b:02X}")).collect();
        return Ok(vec![RegexBuilder::new(&pat).unicode(false).build().map_err(|e| e.to_string())?]);
    }
    let pages = match p.codepage {
        Some(c) => vec![c],
        None => {
            let mut v = vec![Codepage::Utf8, Codepage::Utf16Le, Codepage::Utf16Be, Codepage::oem()];
            if Codepage::ansi() != Codepage::Utf8 {
                v.push(Codepage::ansi());
            }
            v
        }
    };
    let ci = !p.text.case_sensitive;
    let mut out = Vec::new();
    for cp in pages {
        if cp == Codepage::Utf8 {
            let (body, fci) = if p.text.regex { gm_text::regex_adapter::parse_far_regex(&p.text.text) } else { (regex::escape(&p.text.text), false) };
            let body = if p.text.whole_words { format!(r"\b(?:{body})\b") } else { body };
            out.push(RegexBuilder::new(&body).case_insensitive(ci || fci).build().map_err(|e| e.to_string())?);
        } else if p.text.regex {
            if cp.unit() == 1 {
                let (body, fci) = gm_text::regex_adapter::parse_far_regex(&p.text.text);
                out.push(RegexBuilder::new(&body).unicode(false).case_insensitive(ci || fci).build().map_err(|e| e.to_string())?);
            }
        } else {
            let mut pat = String::new();
            for c in p.text.text.chars() {
                let mut vars = vec![c.to_string()];
                if ci {
                    vars.push(c.to_lowercase().collect());
                    vars.push(c.to_uppercase().collect());
                }
                vars.sort();
                vars.dedup();
                let alts: Vec<String> = vars.iter().map(|v| cp.encode(v).iter().map(|b| format!("\\x{b:02X}")).collect()).collect();
                pat.push_str(&format!("(?:{})", alts.join("|")));
            }
            out.push(RegexBuilder::new(&pat).unicode(false).build().map_err(|e| e.to_string())?);
        }
    }
    Ok(out)
}

fn content_matches(path: &std::path::Path, res: &[Regex]) -> bool {
    let Ok(f) = std::fs::File::open(path) else { return false };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len == 0 {
        return false;
    }
    match gm_platform::map_file(&f) {
        Ok(m) => res.iter().any(|r| r.is_match(&m)),
        Err(_) => {
            let mut v = Vec::new();
            use std::io::Read;
            let _ = f.take(64 << 20).read_to_end(&mut v);
            res.iter().any(|r| r.is_match(&v))
        }
    }
}

/// Starts the search over `roots` in a worker thread.
pub fn start(params: FindParams, roots: Vec<PathBuf>) -> (Receiver<FindMsg>, CancelToken) {
    let (tx, rx) = unbounded();
    let cancel = CancelToken::new();
    let c2 = cancel.clone();
    std::thread::spawn(move || run(params, roots, tx, c2));
    (rx, cancel)
}

fn run(p: FindParams, roots: Vec<PathBuf>, tx: Sender<FindMsg>, cancel: CancelToken) {
    let groups: std::collections::HashMap<String, String> = gm_text::masks::default_groups();
    let mask = match FileMask::with_groups(if p.masks.trim().is_empty() { "*" } else { &p.masks }, &groups) {
        Ok(m) => m,
        Err(e) => {
            let _ = tx.send(FindMsg::Error(e.to_string()));
            let _ = tx.send(FindMsg::Done);
            return;
        }
    };
    let res = match matchers(&p) {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(FindMsg::Error(e));
            let _ = tx.send(FindMsg::Done);
            return;
        }
    };
    let content = !res.is_empty();
    for root in roots {
        let max_depth = if p.area == Area::CurrentOnly { 1 } else { usize::MAX };
        let walker = walkdir::WalkDir::new(&root).follow_links(p.follow_links).max_depth(max_depth).min_depth(1);
        let mut last_dir = PathBuf::new();
        let mut it = walker.into_iter();
        while let Some(entry) = it.next() {
            if cancel.is_cancelled() {
                let _ = tx.send(FindMsg::Done);
                return;
            }
            let Ok(e) = entry else { continue };
            let name = e.file_name().to_string_lossy();
            let is_dir = e.file_type().is_dir();
            if p.skip_hidden && name.starts_with('.') {
                if is_dir {
                    it.skip_current_dir();
                }
                continue;
            }
            if let Some(parent) = e.path().parent()
                && parent != last_dir
            {
                last_dir = parent.to_path_buf();
                let _ = tx.send(FindMsg::Scanning(last_dir.clone()));
            }
            if !mask.matches(&name) {
                continue;
            }
            if is_dir {
                if p.folders && !content {
                    let _ = tx.send(FindMsg::Found { path: e.path().to_path_buf(), is_dir: true });
                }
                continue;
            }
            let ok = if content { content_matches(e.path(), &res) != p.not_containing } else { true };
            if ok {
                let _ = tx.send(FindMsg::Found { path: e.path().to_path_buf(), is_dir: false });
            }
        }
    }
    let _ = tx.send(FindMsg::Done);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_by_mask_and_content() {
        let d = std::env::temp_dir().join(format!("gm-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a.txt"), "Hello World").unwrap();
        std::fs::write(d.join("sub/b.txt"), Codepage::Utf16Le.encode("say HELLO")).unwrap();
        std::fs::write(d.join("sub/c.rs"), "nothing").unwrap();
        let mut p = FindParams { masks: "*.txt".into(), ..Default::default() };
        p.text.text = "hello".into();
        let (rx, _c) = start(p, vec![d.clone()]);
        let mut found: Vec<String> = Vec::new();
        for m in rx.iter() {
            match m {
                FindMsg::Found { path, .. } => found.push(path.file_name().unwrap().to_string_lossy().into_owned()),
                FindMsg::Done => break,
                _ => {}
            }
        }
        found.sort();
        assert_eq!(found, ["a.txt", "b.txt"]);
        std::fs::remove_dir_all(&d).unwrap();
    }
}

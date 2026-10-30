//! File descriptions in `descript.ion` / `files.bbs` (`dizlist.cpp`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const DIZ_NAMES: &[&str] = &["descript.ion", "files.bbs"];

/// Finds the description file of a directory.
pub fn find(dir: &Path) -> Option<PathBuf> {
    DIZ_NAMES.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

/// Parses `name description` / `"long name" description` lines.
/// Continuation lines starting with whitespace append to the previous entry.
pub fn parse(text: &str) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut last: Option<String> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(k) = &last
                && let Some(v) = map.get_mut(k)
            {
                v.push(' ');
                v.push_str(line.trim());
            }
            continue;
        }
        let (name, rest) = if let Some(r) = line.strip_prefix('"') {
            match r.find('"') {
                Some(i) => (&r[..i], &r[i + 1..]),
                None => continue,
            }
        } else {
            match line.find([' ', '\t']) {
                Some(i) => (&line[..i], &line[i..]),
                None => (line, ""),
            }
        };
        let key = name.to_lowercase();
        // Ignore the Ctrl+D (0x04) "extended description" tail used by some tools.
        let desc = rest.split('\u{4}').next().unwrap_or("").trim().to_string();
        map.insert(key.clone(), desc);
        last = Some(key);
    }
    map
}

pub fn read(dir: &Path) -> HashMap<String, String> {
    find(dir).and_then(|p| std::fs::read(p).ok()).map(|b| parse(&String::from_utf8_lossy(&b))).unwrap_or_default()
}

/// Sets or removes (empty `desc`) a description and rewrites the file.
pub fn set(dir: &Path, name: &str, desc: &str) -> std::io::Result<()> {
    let path = find(dir).unwrap_or_else(|| dir.join(DIZ_NAMES[0]));
    let text = std::fs::read(&path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let key = name.to_lowercase();
    let mut out = String::new();
    let mut skipping = false;
    for line in text.lines() {
        if skipping && line.starts_with([' ', '\t']) {
            continue;
        }
        skipping = false;
        let lname =
            if let Some(r) = line.strip_prefix('"') { r.split('"').next().unwrap_or("") } else { line.split([' ', '\t']).next().unwrap_or("") };
        if lname.to_lowercase() == key {
            skipping = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !desc.trim().is_empty() {
        let n = if name.contains(' ') { format!("\"{name}\"") } else { name.to_string() };
        out.push_str(&format!("{n} {}\n", desc.trim()));
    }
    if out.trim().is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    std::fs::write(&path, out)?;
    gm_platform::attrs::set_win_attrs(&path, gm_platform::attrs::FILE_ATTRIBUTE_HIDDEN | gm_platform::attrs::FILE_ATTRIBUTE_ARCHIVE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_set() {
        let m = parse("a.txt First file\n\"long name.doc\" Second\n  continued\nB.TXT\tThird\n");
        assert_eq!(m["a.txt"], "First file");
        assert_eq!(m["long name.doc"], "Second continued");
        assert_eq!(m["b.txt"], "Third");
        let d = std::env::temp_dir().join(format!("gm-diz-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        set(&d, "x y.txt", "hello").unwrap();
        set(&d, "z", "zed").unwrap();
        set(&d, "x y.txt", "").unwrap();
        let m = read(&d);
        assert_eq!(m.len(), 1);
        assert_eq!(m["z"], "zed");
        std::fs::remove_dir_all(&d).unwrap();
    }
}

//! One-time converter: `far/*.hlf.m4` → `assets/help/*.hlf`.
//!
//! Usage: `hlf-convert <far-source-dir> <out-dir>`
//!
//! Expands the few m4 macros the help files use, rebrands product names,
//! removes topics about dropped features (plugins, Lua macros) and turns
//! links to removed topics into plain emphasized text.

use regex::Regex;
use std::collections::HashSet;
use std::{env, fs, path::Path, process::ExitCode};

const VERSION: &str = concat!("Goodevening Manager ", env!("CARGO_PKG_VERSION"));
const YEAR: &str = "2026";

/// Topic-name prefixes describing features that do not exist in gm.
const DROPPED: &[&str] = &["Plugin", "Macro", "Lua", "KeyMacro", "MacroCmd", "PluginsReviews", "ErrLoadPlugin", "ErrLoadPluginModule"];

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: hlf-convert <far-source-dir> <out-dir>");
        return ExitCode::FAILURE;
    }
    let src = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    if let Err(e) = fs::create_dir_all(out) {
        eprintln!("hlf-convert: {e}");
        return ExitCode::FAILURE;
    }
    let mut ok = true;
    let Ok(dir) = fs::read_dir(src) else {
        eprintln!("hlf-convert: cannot read {}", src.display());
        return ExitCode::FAILURE;
    };
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".hlf.m4") else { continue };
        let target = out.join(format!("{}.hlf", stem.replacen("Far", "Gm", 1)));
        match fs::read_to_string(entry.path()) {
            Ok(text) => {
                let (converted, kept, dropped) = convert(&text);
                if let Err(e) = fs::write(&target, converted) {
                    eprintln!("hlf-convert: {}: {e}", target.display());
                    ok = false;
                } else {
                    println!("{name} → {} ({kept} topics, {dropped} dropped)", target.display());
                }
            }
            Err(e) => {
                eprintln!("hlf-convert: {name}: {e}");
                ok = false;
            }
        }
    }
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn is_dropped(topic: &str) -> bool {
    DROPPED.iter().any(|p| topic.starts_with(p))
}

fn convert(text: &str) -> (String, usize, usize) {
    let text = text.trim_start_matches('\u{feff}');
    let mut topics_out: Vec<String> = Vec::new();
    let mut dropped_names = HashSet::new();
    let mut kept = 0;
    let mut dropped = 0;
    let mut skipping = false;
    for line in text.lines() {
        if line.starts_with("m4_include") {
            continue;
        }
        let line = line.replace("M4_MACRO_GET(FULLVERSION)", VERSION).replace("M4_MACRO_GET(COPYRIGHTYEAR)", YEAR);
        if let Some(rest) = line.strip_prefix('@') {
            if !rest.is_empty() && !rest.contains('=') && !rest.starts_with('-') && !rest.starts_with('+') {
                skipping = is_dropped(rest);
                if skipping {
                    dropped += 1;
                    dropped_names.insert(rest.to_string());
                } else {
                    kept += 1;
                }
            } else if let Some((alias, target)) = rest.split_once('=')
                && (is_dropped(alias) || is_dropped(target))
            {
                continue;
            }
        }
        if !skipping {
            topics_out.push(rebrand(&line));
        }
    }
    // Links to dropped topics become emphasis: ~text~@Topic@ → #text#.
    let link = Regex::new(r"~([^~]*)~@([^@]*)@").unwrap();
    let mut out = String::new();
    for line in topics_out {
        let fixed = link.replace_all(&line, |c: &regex::Captures| {
            let target = &c[2];
            if is_dropped(target) || dropped_names.contains(target) { format!("#{}#", &c[1]) } else { c[0].to_string() }
        });
        out.push_str(&fixed);
        out.push('\n');
    }
    (out, kept, dropped)
}

fn rebrand(s: &str) -> String {
    let s = s
        .replace("Far Manager", "Goodevening Manager")
        .replace("FarMenu.ini", "GmMenu.ini")
        .replace("far:config", "gm:config")
        .replace("far:about", "gm:about")
        .replace("Far.exe", "gm")
        .replace("far.exe", "gm");
    let word = Regex::new(r"\bFar\b").unwrap();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for m in word.find_iter(&s) {
        out.push_str(&s[last..m.start()]);
        if s[m.end()..].starts_with(" Group") || s[..m.start()].ends_with('@') || s[..m.start()].ends_with("~@") {
            out.push_str("Far");
        } else {
            out.push_str("gm");
        }
        last = m.end();
    }
    out.push_str(&s[last..]);
    out
}

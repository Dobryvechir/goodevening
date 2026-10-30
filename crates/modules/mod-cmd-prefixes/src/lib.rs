//! Command-line prefixes (replaces the `farcmds` plugin):
//!
//! * `view:file`, `edit:file` – open in the internal viewer/editor
//! * `goto:path` – go to a folder (or to a file's folder, cursor on it)
//! * `clip:command` – run a command and put its output into the clipboard
//! * `whereis:name` – look a program up in `PATH`
//! * `link:target link` – create a symbolic link
//! * `run:command` – run a command and show its output in the viewer
//! * `load:` / `unload:` – accepted for compatibility; there are no plugins
#![forbid(unsafe_code)]

use gm_core::{Action, Ctx, Module};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct CmdPrefixes;

fn resolve(ctx: &Ctx, arg: &str) -> PathBuf {
    let p = PathBuf::from(arg.trim().trim_matches('"'));
    if p.is_absolute() { p } else { ctx.active.base_dir().join(p) }
}

fn capture(cmd: &str, dir: &Path) -> String {
    match gm_platform::shell::shell_command(cmd, dir).output() {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            s
        }
        Err(e) => e.to_string(),
    }
}

pub fn whereis(name: &str) -> Vec<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.COM;.BAT;.CMD".into()).split(';').map(|s| s.to_lowercase()).collect()
    } else {
        vec![String::new()]
    };
    let mut out = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for e in std::iter::once(String::new()).chain(exts.iter().cloned()) {
                let c = dir.join(format!("{name}{e}"));
                if c.is_file() && !out.contains(&c) {
                    out.push(c);
                }
            }
        }
    }
    out
}

impl Module for CmdPrefixes {
    fn id(&self) -> &'static str {
        "cmd_prefixes"
    }

    fn title(&self) -> String {
        "Command-line prefixes".into()
    }

    fn prefixes(&self) -> &[&'static str] {
        &["view", "edit", "goto", "clip", "whereis", "link", "run", "load", "unload"]
    }

    fn open_prefix(&self, ctx: &mut Ctx, prefix: &str, arg: &str) -> bool {
        let dir = ctx.active.base_dir();
        match prefix {
            "view" => {
                let p = resolve(ctx, arg);
                ctx.push(Action::View(p));
            }
            "edit" => {
                let p = resolve(ctx, arg);
                ctx.push(Action::Edit(p));
            }
            "goto" => {
                let p = resolve(ctx, &crate_expand(arg));
                ctx.push(Action::Cd(p));
            }
            "clip" => {
                let out = capture(arg, &dir);
                gm_core::gm_ui::clipboard::set(&out);
            }
            "run" => {
                let out = capture(arg, &dir);
                ctx.push(Action::ViewText { title: format!("run:{arg}"), text: out });
            }
            "whereis" => {
                let found = whereis(arg.trim());
                let text = if found.is_empty() {
                    format!("{} not found in PATH", arg.trim())
                } else {
                    found.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n")
                };
                ctx.push(Action::Message { title: "whereis".into(), text });
            }
            "link" => {
                let parts: Vec<&str> = arg.split_whitespace().collect();
                if let [target, link] = parts.as_slice() {
                    let t = resolve(ctx, target);
                    let l = resolve(ctx, link);
                    if let Err(e) = gm_platform::attrs::symlink(&t, &l, t.is_dir()) {
                        ctx.push(Action::Message { title: "link".into(), text: e.to_string() });
                    }
                    ctx.push(Action::Refresh);
                } else {
                    ctx.push(Action::Message { title: "link".into(), text: "Usage: link:target link_name".into() });
                }
            }
            "load" | "unload" => {
                ctx.push(Action::Message { title: prefix.into(), text: "Goodevening Manager has no plugins: all modules are built in.".into() });
            }
            _ => return false,
        }
        true
    }
}

fn crate_expand(s: &str) -> String {
    let s = s.trim();
    match s.strip_prefix('~') {
        Some(rest) => gm_platform::drives::home_dir().map(|h| format!("{}{rest}", h.display())).unwrap_or_else(|| s.to_string()),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn whereis_finds_shell() {
        let name = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(!super::whereis(name).is_empty());
    }
}

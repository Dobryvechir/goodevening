//! Command line execution (`execute.cpp`): internal commands, module
//! prefixes and running programs through the user's shell.
//!
//! Programs run with the terminal restored to normal mode (the "user
//! screen"); their output stays visible with Ctrl+O, like in Far.

use crate::app::App;
use crate::module::Ctx;
use gm_i18n::{MsgId, msg};
use gm_panels::PanelKind;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Expands `~`, `%VAR%` (Windows) and `$VAR` / `${VAR}` (Unix).
pub fn expand_env(s: &str) -> String {
    let mut s = s.to_string();
    if let Some(rest) = s.strip_prefix('~')
        && (rest.is_empty() || rest.starts_with(['/', '\\']))
        && let Some(h) = gm_platform::drives::home_dir()
    {
        s = format!("{}{rest}", h.display());
    }
    if cfg!(windows) {
        let mut out = String::new();
        let mut rest = s.as_str();
        while let Some(i) = rest.find('%') {
            out.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            match after.find('%') {
                Some(j) => {
                    let name = &after[..j];
                    match std::env::var(name) {
                        Ok(v) if !name.is_empty() => out.push_str(&v),
                        _ => {
                            out.push('%');
                            out.push_str(name);
                            out.push('%');
                        }
                    }
                    rest = &after[j + 1..];
                }
                None => {
                    out.push('%');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    } else {
        let mut out = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() {
                let (name, len) = if chars[i + 1] == '{' {
                    let end = chars[i + 2..].iter().position(|&c| c == '}').map(|p| p + i + 2);
                    match end {
                        Some(e) => (chars[i + 2..e].iter().collect::<String>(), e + 1 - i),
                        None => (String::new(), 1),
                    }
                } else {
                    let n: String = chars[i + 1..].iter().take_while(|c| c.is_alphanumeric() || **c == '_').collect();
                    let l = n.chars().count() + 1;
                    (n, l)
                };
                if !name.is_empty() {
                    out.push_str(&std::env::var(&name).unwrap_or_default());
                    i += len;
                    continue;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    }
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(s)
}

/// Changes the active panel folder (`cd` on the command line).
pub fn change_dir(app: &mut App, arg: &str) -> bool {
    let arg = expand_env(unquote(arg));
    let base = app.work_dir();
    let target = if arg.is_empty() {
        if cfg!(windows) {
            // "cd" alone prints the folder in cmd; in gm it is a no-op.
            return true;
        }
        gm_platform::drives::home_dir().unwrap_or(base)
    } else if arg == "-" {
        match app.dir_stack.last() {
            Some(p) => p.clone(),
            None => return true,
        }
    } else {
        let p = PathBuf::from(&arg);
        if p.is_absolute() {
            p
        } else if cfg!(windows) && (arg.starts_with('\\') || arg.starts_with('/')) {
            gm_platform::drives::volume_root(&base).join(&arg[1..])
        } else {
            base.join(p)
        }
    };
    let target = gm_vfs::local::normalize(&target);
    let slot = app.active();
    slot.kind = PanelKind::Files;
    match slot.files.set_local_dir(target.clone()) {
        Ok(()) => {
            app.dir_changed();
            true
        }
        Err(e) => {
            gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &format!("{}\n{e}", target.display()));
            false
        }
    }
}

/// Executes a command line typed by the user (or produced by menus).
pub fn execute_line(app: &mut App, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if app.settings.system.save_history {
        app.db.add_history("cmd", line, "");
    }
    // gm:config / gm:about
    let lower = line.to_lowercase();
    if lower == "gm:config" {
        crate::options::config_editor(app);
        return;
    }
    if lower == "gm:about" {
        crate::menus::about(app);
        return;
    }
    // Module prefixes: "prefix:argument" (a single letter before ':' is a drive).
    if let Some((prefix, arg)) = line.split_once(':')
        && prefix.len() > 1
        && prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        let p = prefix.to_lowercase();
        if let Some(mi) = app.modules.iter().position(|m| m.prefixes().iter().any(|x| *x == p)) {
            let modules = std::mem::take(&mut app.modules);
            let (handled, actions) = {
                let (a, b) = if app.active_left { (&mut app.left, &mut app.right) } else { (&mut app.right, &mut app.left) };
                let mut ctx = Ctx { ui: &mut app.ui, active: &mut a.files, passive: &mut b.files, settings: &app.settings, actions: Vec::new() };
                let h = modules[mi].open_prefix(&mut ctx, &p, arg.trim());
                (h, ctx.actions)
            };
            app.modules = modules;
            crate::commands::apply_actions(app, actions);
            if handled {
                app.dir_changed();
                return;
            }
        }
    }
    // Internal commands.
    let (word, rest) = gm_platform::shell::split_program(line);
    let w = word.to_lowercase();
    if w == "cd" || w == "chdir" {
        // cmd.exe's "/d" switch is implied.
        let arg = rest.strip_prefix("/d ").or_else(|| rest.strip_prefix("/D ")).unwrap_or(rest);
        change_dir(app, arg);
        return;
    }
    if w.starts_with("cd..") || w.starts_with("cd\\") || w.starts_with("cd/") {
        // "cd.." and "cd\" without a space.
        change_dir(app, &line[2..]);
        return;
    }
    if cfg!(windows) && line.len() == 2 && line.ends_with(':') && line.as_bytes()[0].is_ascii_alphabetic() {
        change_dir(app, &format!("{line}\\"));
        return;
    }
    match w.as_str() {
        "pushd" => {
            let cur = app.work_dir();
            if change_dir(app, rest) {
                app.dir_stack.push(cur);
            }
            return;
        }
        "popd" => {
            if let Some(d) = app.dir_stack.pop() {
                change_dir(app, &d.display().to_string());
            }
            return;
        }
        "exit" if rest.is_empty() => {
            app.quit = true;
            return;
        }
        "cls" | "clear" if rest.is_empty() => return,
        _ => {}
    }
    run_external(app, line);
}

/// Runs a program through the shell on the user screen.
pub fn run_external(app: &mut App, line: &str) {
    let dir = app.work_dir();
    if app.ui.term.suspend().is_err() {
        return;
    }
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{}>{line}", dir.display());
    let _ = out.flush();
    let r = gm_platform::shell::run_in_shell(line, &dir);
    let _ = out.flush();
    let _ = app.ui.term.resume();
    if let Err(e) = r {
        gm_ui::message::error(&mut app.ui, msg(MsgId::MError), &format!("{}\n{line}\n{e}", msg(MsgId::MCannotExecute)));
    }
    app.refresh_panels();
}

#[allow(dead_code)]
fn is_dir(p: &Path) -> bool {
    p.is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_expansion() {
        // SAFETY of set_var: tests in this module do not run other threads that read the env.
        let home = gm_platform::drives::home_dir().unwrap();
        assert_eq!(expand_env("~"), home.display().to_string());
        if cfg!(windows) {
            assert_eq!(expand_env("%NO_SUCH_VAR_GM%"), "%NO_SUCH_VAR_GM%");
            let p = std::env::var("PATH").unwrap();
            assert_eq!(expand_env("%PATH%"), p);
        } else {
            assert_eq!(expand_env("$NO_SUCH_VAR_GM/x"), "/x");
        }
        assert_eq!(unquote("\"a b\""), "a b");
    }
}

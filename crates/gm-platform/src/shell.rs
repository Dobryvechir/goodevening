//! Shell integration: running commands, opening files with the system handler.

use std::path::Path;
use std::process::{Command, ExitStatus};

/// The user's shell and the flag that runs one command string.
pub fn default_shell() -> (String, &'static str) {
    if cfg!(windows) {
        (std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()), "/C")
    } else {
        (std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()), "-c")
    }
}

/// Builds a command that runs `line` through the user's shell in `cwd`.
pub fn shell_command(line: &str, cwd: &Path) -> Command {
    let (shell, flag) = default_shell();
    let mut cmd = Command::new(shell);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // cmd.exe parses the raw command line itself; avoid Rust's quoting.
        cmd.arg(flag).raw_arg(line);
    }
    #[cfg(not(windows))]
    {
        cmd.arg(flag).arg(line);
    }
    cmd.current_dir(cwd);
    cmd
}

/// Runs `line` with inherited stdio and waits.
pub fn run_in_shell(line: &str, cwd: &Path) -> std::io::Result<ExitStatus> {
    shell_command(line, cwd).status()
}

/// Opens a file or URL with the associated application, without waiting.
pub fn open_with_system(path: &Path) -> std::io::Result<()> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]).arg(path);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(path);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(path);
        c
    };
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().map(|_| ())
}

/// Shows the system "Properties" dialog for a file (Windows only).
pub fn show_properties(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::{SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW, ShellExecuteExW};
        let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let verb: Vec<u16> = "properties".encode_utf16().chain(Some(0)).collect();
        // SAFETY: zeroed SHELLEXECUTEINFOW is a valid "empty" value for this POD struct.
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_INVOKEIDLIST;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.nShow = 1;
        // SAFETY: all pointers in `info` reference live NUL-terminated buffers.
        unsafe { ShellExecuteExW(&mut info) != 0 }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// Splits a command line into the program word and the rest (honoring quotes).
pub fn split_program(line: &str) -> (&str, &str) {
    let line = line.trim_start();
    if let Some(rest) = line.strip_prefix('"')
        && let Some(end) = rest.find('"')
    {
        return (&rest[..end], rest[end + 1..].trim_start());
    }
    match line.find(char::is_whitespace) {
        Some(i) => (&line[..i], line[i..].trim_start()),
        None => (line, ""),
    }
}

/// Quotes a path for the platform shell if it contains special characters.
pub fn quote(s: &str) -> String {
    let special = |c: char| c.is_whitespace() || "&()[]{}^=;!'+,`~|<>$\"*?#".contains(c);
    if !s.chars().any(special) {
        return s.to_string();
    }
    if cfg!(windows) { format!("\"{s}\"") } else { format!("'{}'", s.replace('\'', "'\\''")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split() {
        assert_eq!(split_program("\"my prog\" a b"), ("my prog", "a b"));
        assert_eq!(split_program("ls -la"), ("ls", "-la"));
        assert_eq!(quote("plain"), "plain");
    }
}

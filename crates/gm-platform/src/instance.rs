//! Starting another, independent gm in a new terminal window
//! (F9 → Commands → Multiple terminals).
//!
//! Best effort, like `window`: a new console on Windows, a new Windows
//! console from WSL, Terminal or iTerm2 on macOS, and the first terminal
//! emulator found on Linux/X11/Wayland.

use std::ffi::OsString;
use std::io;

/// Starts the running gm executable again, with `args`, in a new terminal
/// window. Returns once the new process (or the terminal hosting it) has
/// been started; it is not waited for.
pub fn open_in_new_terminal(args: &[OsString]) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    imp::open(exe.as_os_str(), args)
}

#[cfg(windows)]
mod imp {
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
    };

    /// `CreateProcessW` with `CREATE_NEW_CONSOLE` and no inherited handles,
    /// so the new gm gets its own console window (or a new Windows Terminal
    /// window when that is the default terminal application) with its own
    /// standard handles. `std::process::Command` would pass this console's
    /// handles to the child.
    pub fn open(exe: &OsStr, args: &[OsString]) -> io::Result<()> {
        let mut line: Vec<u16> = Vec::new();
        quote(exe, &mut line);
        for a in args {
            line.push(' ' as u16);
            quote(a, &mut line);
        }
        line.push(0);
        let app: Vec<u16> = exe.encode_wide().chain(std::iter::once(0)).collect();
        // SAFETY: all pointers refer to live, NUL-terminated buffers or
        // zero-initialized structures of the right size; the returned
        // handles are closed right away.
        unsafe {
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let ok = CreateProcessW(
                app.as_ptr(),
                line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
                std::ptr::null(),
                std::ptr::null(),
                &si,
                &mut pi,
            );
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
        }
        Ok(())
    }

    /// Appends one argument quoted by the rules `CommandLineToArgvW` and the
    /// C runtime use to split a command line.
    fn quote(arg: &OsStr, out: &mut Vec<u16>) {
        let s: Vec<u16> = arg.encode_wide().collect();
        let needs = s.is_empty() || s.iter().any(|&c| c == ' ' as u16 || c == '\t' as u16 || c == '"' as u16);
        if !needs {
            out.extend_from_slice(&s);
            return;
        }
        out.push('"' as u16);
        let mut backslashes = 0;
        for &c in &s {
            if c == '\\' as u16 {
                backslashes += 1;
                continue;
            }
            if c == '"' as u16 {
                // Backslashes before a quote are doubled, the quote escaped.
                out.extend(std::iter::repeat_n('\\' as u16, backslashes * 2 + 1));
            } else {
                out.extend(std::iter::repeat_n('\\' as u16, backslashes));
            }
            backslashes = 0;
            out.push(c);
        }
        // Backslashes before the closing quote are doubled.
        out.extend(std::iter::repeat_n('\\' as u16, backslashes * 2));
        out.push('"' as u16);
    }

    #[cfg(test)]
    mod tests {
        use super::quote;
        use std::ffi::OsStr;

        fn q(s: &str) -> String {
            let mut v = Vec::new();
            quote(OsStr::new(s), &mut v);
            String::from_utf16(&v).unwrap()
        }

        #[test]
        fn quoting() {
            assert_eq!(q(r"C:\gm\gm.exe"), r"C:\gm\gm.exe");
            assert_eq!(q(r"C:\Program Files\gm"), r#""C:\Program Files\gm""#);
            assert_eq!(q(r"C:\My dir\"), r#""C:\My dir\\""#);
            assert_eq!(q(r#"a"b"#), r#""a\"b""#);
            assert_eq!(q(""), r#""""#);
        }
    }
}

#[cfg(unix)]
mod imp {
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::process::{Command, Stdio};

    pub fn open(exe: &OsStr, args: &[OsString]) -> io::Result<()> {
        // The new terminal must not see this terminal's window id, or the new
        // gm would move this window instead of its own.
        if crate::window::is_wsl() && std::env::var_os("WINDOWID").is_none() {
            return wsl(exe, args);
        }
        #[cfg(target_os = "macos")]
        return mac(exe, args);
        #[cfg(not(target_os = "macos"))]
        return linux(exe, args);
    }

    /// Starts `cmd` detached from gm (stdio closed, without `WINDOWID`) and
    /// reaps it in the background.
    fn spawn(mut cmd: Command) -> io::Result<()> {
        let mut child = cmd.env_remove("WINDOWID").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        std::thread::spawn(move || child.wait());
        Ok(())
    }

    /// WSL inside a Windows console: `start` opens a new Windows console (or
    /// Windows Terminal window) running the same distribution.
    fn wsl(exe: &OsStr, args: &[OsString]) -> io::Result<()> {
        let mut last = io::Error::from(io::ErrorKind::NotFound);
        for cmd_exe in ["cmd.exe", "/mnt/c/Windows/System32/cmd.exe"] {
            let mut c = Command::new(cmd_exe);
            c.args(["/c", "start", "", "wsl.exe"]);
            if let Some(d) = std::env::var_os("WSL_DISTRO_NAME") {
                c.arg("-d").arg(d);
            }
            c.arg("-e").arg(exe).args(args);
            // cmd.exe cannot start in a Linux folder; it would warn about it.
            if std::path::Path::new("/mnt/c").is_dir() {
                c.current_dir("/mnt/c");
            }
            match spawn(c) {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// A command line for a POSIX shell.
    #[cfg(target_os = "macos")]
    fn shell_line(exe: &OsStr, args: &[OsString]) -> String {
        let q = |s: &OsStr| format!("'{}'", s.to_string_lossy().replace('\'', r"'\''"));
        let mut line = q(exe);
        for a in args {
            line.push(' ');
            line.push_str(&q(a));
        }
        line
    }

    /// macOS: a new iTerm2 window when gm runs in iTerm2, otherwise a new
    /// Terminal window. The command line goes in as an argument, so it needs
    /// no AppleScript quoting.
    #[cfg(target_os = "macos")]
    fn mac(exe: &OsStr, args: &[OsString]) -> io::Result<()> {
        let line = shell_line(exe, args);
        let iterm = std::env::var("TERM_PROGRAM").is_ok_and(|t| t == "iTerm.app");
        let body = if iterm {
            r#"tell application "iTerm2" to create window with default profile command (item 1 of argv)"#
        } else {
            r#"tell application "Terminal"
activate
do script ("exec " & item 1 of argv)
end tell"#
        };
        let mut c = Command::new("osascript");
        c.args(["-e", "on run argv"]);
        for l in body.lines() {
            c.args(["-e", l]);
        }
        c.args(["-e", "end run", &line]);
        let o = c.env_remove("WINDOWID").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
        if o.success() { Ok(()) } else { Err(io::Error::other("osascript failed")) }
    }

    /// Linux and other Unix desktops: `$TERMINAL`, the terminal gm runs in
    /// (from its environment variables), then common terminal emulators.
    #[cfg(not(target_os = "macos"))]
    fn linux(exe: &OsStr, args: &[OsString]) -> io::Result<()> {
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "no graphical session (DISPLAY/WAYLAND_DISPLAY)"));
        }
        let env = |k: &str| std::env::var_os(k).is_some();
        let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default().to_ascii_lowercase();
        // (program, options that come before the command to run)
        let mut list: Vec<(String, Vec<&str>)> = Vec::new();
        if let Ok(t) = std::env::var("TERMINAL")
            && !t.trim().is_empty()
        {
            list.push((t, vec!["-e"]));
        }
        let mut current = |p: &str, o: Vec<&'static str>| list.push((p.to_string(), o));
        if env("KITTY_WINDOW_ID") {
            current("kitty", vec![]);
        }
        if env("ALACRITTY_WINDOW_ID") || env("ALACRITTY_SOCKET") {
            current("alacritty", vec!["-e"]);
        }
        if env("WEZTERM_PANE") || term_program == "wezterm" {
            current("wezterm", vec!["start", "--"]);
        }
        if term_program == "ghostty" {
            current("ghostty", vec!["-e"]);
        }
        if env("KONSOLE_VERSION") {
            current("konsole", vec!["-e"]);
        }
        if env("GNOME_TERMINAL_SCREEN") || env("GNOME_TERMINAL_SERVICE") {
            current("gnome-terminal", vec!["--"]);
        }
        if std::env::var("TERM").is_ok_and(|t| t.starts_with("foot")) {
            current("foot", vec![]);
        }
        for (p, o) in [
            ("x-terminal-emulator", vec!["-e"]),
            ("gnome-terminal", vec!["--"]),
            ("konsole", vec!["-e"]),
            ("xfce4-terminal", vec!["-x"]),
            ("mate-terminal", vec!["-x"]),
            ("kitty", vec![]),
            ("alacritty", vec!["-e"]),
            ("wezterm", vec!["start", "--"]),
            ("ghostty", vec!["-e"]),
            ("foot", vec![]),
            ("xterm", vec!["-e"]),
        ] {
            current(p, o);
        }
        let mut last = io::Error::new(io::ErrorKind::NotFound, "no terminal emulator found");
        for (program, opts) in list {
            let mut c = Command::new(&program);
            c.args(opts).arg(exe).args(args);
            match spawn(c) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => last = e,
            }
        }
        Err(last)
    }
}

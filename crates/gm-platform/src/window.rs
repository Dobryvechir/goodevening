//! Moving and resizing the console window (`gm --pos`).
//!
//! Best effort everywhere: Win32 calls on Windows, PowerShell on WSL,
//! AppleScript on macOS, wmctrl/xdotool on X11 and xterm window operations
//! as the last resort. Whatever the system does not support is silently
//! ignored.

/// Where the console window is placed at startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowPos {
    /// Leave the window as the system opened it.
    Normal,
    /// Maximize the window.
    Full,
    /// Left, top, width and height in halves of the screen work area.
    Part { x: u8, y: u8, w: u8, h: u8 },
}

impl WindowPos {
    /// Parses the `--pos` value: `n`, `f`, `l`, `r`, `t`, `b`, `lt`/`tl`,
    /// `lb`/`bl`, `rt`/`tr`, `rb`/`br`.
    pub fn parse(s: &str) -> Option<Self> {
        let part = |x, y, w, h| Some(WindowPos::Part { x, y, w, h });
        match s.to_ascii_lowercase().as_str() {
            "n" => Some(WindowPos::Normal),
            "f" => Some(WindowPos::Full),
            "l" => part(0, 0, 1, 2),
            "r" => part(1, 0, 1, 2),
            "t" => part(0, 0, 2, 1),
            "b" => part(0, 1, 2, 1),
            "lt" | "tl" => part(0, 0, 1, 1),
            "lb" | "bl" => part(0, 1, 1, 1),
            "rt" | "tr" => part(1, 0, 1, 1),
            "rb" | "br" => part(1, 1, 1, 1),
            _ => None,
        }
    }

    /// The position in halves; `Full` is the whole area.
    fn halves(self) -> Option<(u8, u8, u8, u8)> {
        match self {
            WindowPos::Normal => None,
            WindowPos::Full => Some((0, 0, 2, 2)),
            WindowPos::Part { x, y, w, h } => Some((x, y, w, h)),
        }
    }
}

/// A screen rectangle: left, top, width, height.
type Rect = (i32, i32, i32, i32);

/// The part of `area` covered by `halves` (left, top, width, height in halves).
fn part_rect(area: Rect, halves: (u8, u8, u8, u8)) -> Rect {
    let span = |start: i32, len: i32, from: u8, n: u8| {
        let a = start + len * from as i32 / 2;
        let b = start + len * (from + n) as i32 / 2;
        (a, b - a)
    };
    let (x, w) = span(area.0, area.2, halves.0, halves.2);
    let (y, h) = span(area.1, area.3, halves.1, halves.3);
    (x, y, w, h)
}

/// Moves and resizes the console window hosting gm. Call it before the
/// terminal is switched to raw mode.
pub fn place_console(pos: WindowPos) {
    if pos != WindowPos::Normal {
        imp::place(pos);
    }
}

#[cfg(unix)]
pub(crate) use imp::is_wsl;

#[cfg(windows)]
mod imp {
    use super::{WindowPos, part_rect};
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
    use windows_sys::Win32::System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo, GetConsoleWindow, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GA_ROOTOWNER, GetAncestor, GetForegroundWindow, GetWindowRect, IsWindowVisible, IsZoomed, SW_MAXIMIZE, SW_RESTORE, SWP_NOACTIVATE,
        SWP_NOZORDER, SetWindowPos, ShowWindow,
    };

    /// The visible window showing the console: the classic console window,
    /// or the Windows Terminal window owning the pseudo-console window (which
    /// claims to be visible, so the owner is checked first).
    unsafe fn console_window() -> HWND {
        unsafe {
            let hwnd = GetConsoleWindow();
            if hwnd.is_null() {
                return hwnd;
            }
            let owner = GetAncestor(hwnd, GA_ROOTOWNER);
            if !owner.is_null() && owner != hwnd && IsWindowVisible(owner) != 0 {
                return owner;
            }
            if IsWindowVisible(hwnd) != 0 {
                return hwnd;
            }
            GetForegroundWindow()
        }
    }

    pub fn place(pos: WindowPos) {
        // SAFETY: plain Win32 calls on window handles; every out-pointer
        // refers to a local of the right type and size.
        unsafe {
            // Physical pixels everywhere, so DWM frame bounds and monitor
            // rectangles use the same units.
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let hwnd = console_window();
            if hwnd.is_null() {
                return;
            }
            let Some(halves) = pos.halves() else { return };
            let mut mi: MONITORINFO = std::mem::zeroed();
            mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi) == 0 {
                return;
            }
            if IsZoomed(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            let w = mi.rcWork;
            let (x, y, cx, cy) = part_rect((w.left, w.top, w.right - w.left, w.bottom - w.top), halves);
            // Windows 10+ windows have invisible resize borders; grow the
            // target by them so the visible frame fits the area exactly.
            let mut wr: RECT = std::mem::zeroed();
            let mut fr: RECT = std::mem::zeroed();
            let (mut l, mut t, mut r, mut b) = (0, 0, 0, 0);
            if GetWindowRect(hwnd, &mut wr) != 0
                && DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS as u32, &mut fr as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32)
                    == 0
            {
                (l, t, r, b) = (fr.left - wr.left, fr.top - wr.top, wr.right - fr.right, wr.bottom - fr.bottom);
            }
            let before = console_size();
            SetWindowPos(hwnd, std::ptr::null_mut(), x - l, y - t, cx + l + r, cy + t + b, SWP_NOZORDER | SWP_NOACTIVATE);
            // Windows Terminal may ignore a maximize request while it starts,
            // so `Full` first fills the work area and then also maximizes.
            if pos == WindowPos::Full {
                ShowWindow(hwnd, SW_MAXIMIZE);
            }
            wait_for_resize(before);
        }
    }

    /// The visible console size in cells (columns, rows).
    fn console_size() -> Option<(i16, i16)> {
        // SAFETY: the out-pointer refers to a local of the right type.
        unsafe {
            let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
            if GetConsoleScreenBufferInfo(GetStdHandle(STD_OUTPUT_HANDLE), &mut info) == 0 {
                return None;
            }
            let w = info.srWindow;
            Some((w.Right - w.Left + 1, w.Bottom - w.Top + 1))
        }
    }

    /// Windows Terminal resizes its pseudo-console asynchronously, after
    /// `SetWindowPos` returns. Drawing before that happens makes the
    /// terminal re-wrap the first frame into the new width (garbled text),
    /// so wait until the console size has changed and stopped changing.
    fn wait_for_resize(before: Option<(i16, i16)>) {
        use std::time::{Duration, Instant};
        let Some(mut last) = before else { return };
        let start = Instant::now();
        let mut changed = false;
        let mut stable_since = start;
        loop {
            std::thread::sleep(Duration::from_millis(15));
            let now = Instant::now();
            let Some(size) = console_size() else { return };
            if size != last {
                (last, changed, stable_since) = (size, true, now);
            }
            let waited = now - start;
            let stable = now - stable_since;
            // Done: the size changed and then held for a while; or it never
            // changed (classic console already resized, or nothing to do);
            // or the overall limit.
            if (changed && stable >= Duration::from_millis(150)) || (!changed && waited >= Duration::from_millis(600)) || waited >= Duration::from_secs(2) {
                return;
            }
        }
    }
}

#[cfg(unix)]
mod imp {
    #[cfg(not(target_os = "macos"))]
    use super::Rect;
    use super::{WindowPos, part_rect};
    use std::process::{Command, Stdio};

    pub fn place(pos: WindowPos) {
        if is_wsl() && std::env::var_os("WINDOWID").is_none() && wsl(pos) {
            return;
        }
        #[cfg(target_os = "macos")]
        if mac(pos) {
            return;
        }
        #[cfg(not(target_os = "macos"))]
        if x11(pos) {
            return;
        }
        xterm(pos);
    }

    /// Runs a helper program; its stdout when it succeeded.
    fn output(cmd: &str, args: &[&str]) -> Option<String> {
        let o = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    }

    fn ok(cmd: &str, args: &[&str]) -> bool {
        output(cmd, args).is_some()
    }

    pub(crate) fn is_wsl() -> bool {
        std::env::var_os("WSL_DISTRO_NAME").is_some()
            || std::env::var_os("WSL_INTEROP").is_some()
            || std::fs::read_to_string("/proc/sys/kernel/osrelease").is_ok_and(|s| s.to_ascii_lowercase().contains("microsoft"))
    }

    // ---- WSL: the console is a Windows window; let PowerShell move it ----

    const WSL_SCRIPT: &str = r#"$ErrorActionPreference = 'SilentlyContinue'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class GmPos {
    [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
    [StructLayout(LayoutKind.Sequential)] public struct MI { public int Size; public R Mon; public R Work; public int Flags; }
    [DllImport("kernel32.dll")] static extern IntPtr GetConsoleWindow();
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint f);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool IsZoomed(IntPtr h);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int c);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out R r);
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
    [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr h, uint f);
    [DllImport("user32.dll", EntryPoint = "GetMonitorInfoW")] static extern bool GetMonitorInfo(IntPtr m, ref MI mi);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out R r, int s);
    public static void Place(bool full, int hx, int hy, int hw, int hh) {
        try { SetThreadDpiAwarenessContext((IntPtr)(-4)); } catch {}
        IntPtr w = GetConsoleWindow();
        if (w != IntPtr.Zero) {
            IntPtr o = GetAncestor(w, 3);
            if (o != IntPtr.Zero && o != w && IsWindowVisible(o)) w = o;
            else if (!IsWindowVisible(w)) w = GetForegroundWindow();
        }
        if (w == IntPtr.Zero) w = GetForegroundWindow();
        if (w == IntPtr.Zero) return;
        MI mi = new MI(); mi.Size = Marshal.SizeOf(typeof(MI));
        if (!GetMonitorInfo(MonitorFromWindow(w, 2), ref mi)) return;
        if (IsZoomed(w)) ShowWindow(w, 9);
        R a = mi.Work;
        int x = a.L + (a.Rt - a.L) * hx / 2, x2 = a.L + (a.Rt - a.L) * (hx + hw) / 2;
        int y = a.T + (a.B - a.T) * hy / 2, y2 = a.T + (a.B - a.T) * (hy + hh) / 2;
        R wr, fr; int l = 0, t = 0, r = 0, b = 0;
        if (GetWindowRect(w, out wr) && DwmGetWindowAttribute(w, 9, out fr, 16) == 0) {
            l = fr.L - wr.L; t = fr.T - wr.T; r = wr.Rt - fr.Rt; b = wr.B - fr.B;
        }
        SetWindowPos(w, IntPtr.Zero, x - l, y - t, x2 - x + l + r, y2 - y + t + b, 0x14);
        if (full) ShowWindow(w, 3);
    }
}
'@
[GmPos]::Place(ARGS)
"#;

    fn wsl(pos: WindowPos) -> bool {
        let Some((x, y, w, h)) = pos.halves() else { return false };
        let full = if pos == WindowPos::Full { "$true" } else { "$false" };
        let script = WSL_SCRIPT.replace("ARGS", &format!("{full}, {x}, {y}, {w}, {h}"));
        let utf16: Vec<u8> = script.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        let encoded = base64(&utf16);
        for exe in ["powershell.exe", "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe"] {
            let child = Command::new(exe)
                .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            if let Ok(mut child) = child {
                // PowerShell compiles the helper for a second or two; do not
                // delay startup, the terminal reports the new size later.
                std::thread::spawn(move || child.wait());
                return true;
            }
        }
        false
    }

    fn base64(data: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
        for c in data.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for i in 0..4 {
                if i <= c.len() {
                    s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    s.push('=');
                }
            }
        }
        s
    }

    // ---- macOS: AppleScript ----

    #[cfg(target_os = "macos")]
    const MAC_SCRIPT: &str = r#"use framework "AppKit"
use scripting additions
on run argv
set pf to (item 1 of (current application's NSScreen's screens()))'s frame()
set vf to current application's NSScreen's mainScreen()'s visibleFrame()
set vx to item 1 of item 1 of vf
set vw to item 1 of item 2 of vf
set vh to item 2 of item 2 of vf
set vy to (item 2 of item 2 of pf) - (item 2 of item 1 of vf) - vh
set {a, b, c, d} to {(item 1 of argv) as integer, (item 2 of argv) as integer, (item 3 of argv) as integer, (item 4 of argv) as integer}
set x1 to round (vx + vw * a / 2)
set y1 to round (vy + vh * b / 2)
set x2 to round (vx + vw * (a + c) / 2)
set y2 to round (vy + vh * (b + d) / 2)
MOVE
end run"#;

    #[cfg(target_os = "macos")]
    fn mac(pos: WindowPos) -> bool {
        let Some((x, y, w, h)) = pos.halves() else { return false };
        let bundle = std::env::var("__CFBundleIdentifier").unwrap_or_default();
        let term = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let id = match term.as_str() {
            "Apple_Terminal" => "com.apple.Terminal",
            "iTerm.app" => "com.googlecode.iterm2",
            _ => bundle.as_str(),
        };
        if id.is_empty() || id.contains('"') {
            return false;
        }
        let mv = if id == "com.apple.Terminal" || id == "com.googlecode.iterm2" {
            format!(r#"tell application id "{id}" to set bounds of front window to {{x1, y1, x2, y2}}"#)
        } else {
            // Other terminals: System Events (needs the Accessibility permission).
            format!(
                r#"tell application "System Events" to tell (first process whose bundle identifier is "{id}") to tell front window to set {{position, size}} to {{{{x1, y1}}, {{x2 - x1, y2 - y1}}}}"#
            )
        };
        let script = MAC_SCRIPT.replace("MOVE", &mv);
        let mut args: Vec<String> = Vec::new();
        for line in script.lines() {
            args.push("-e".into());
            args.push(line.into());
        }
        args.extend([x, y, w, h].map(|v| v.to_string()));
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        ok("osascript", &args)
    }

    // ---- X11: wmctrl or xdotool ----

    #[cfg(not(target_os = "macos"))]
    fn x11(pos: WindowPos) -> bool {
        if std::env::var_os("DISPLAY").is_none() {
            return false;
        }
        let Some(id) = window_id() else { return false };
        let hex = format!("0x{id:x}");
        let has_wmctrl = ok("wmctrl", &["-m"]);
        let has_xdotool = ok("xdotool", &["version"]);
        if !has_wmctrl && !has_xdotool {
            return false;
        }
        if pos == WindowPos::Full {
            return if has_wmctrl {
                ok("wmctrl", &["-i", "-r", &hex, "-b", "add,maximized_vert,maximized_horz"])
            } else {
                ok("xdotool", &["windowstate", "--add", "MAXIMIZED_VERT,MAXIMIZED_HORZ", &id.to_string()])
            };
        }
        let Some(halves) = pos.halves() else { return false };
        let Some(area) = work_area(id) else { return false };
        let (x, y, w, h) = part_rect(area, halves);
        let (l, r, t, b) = frame_extents(&hex);
        let (cw, ch) = ((w - l - r).max(1).to_string(), (h - t - b).max(1).to_string());
        if has_wmctrl {
            let _ = ok("wmctrl", &["-i", "-r", &hex, "-b", "remove,maximized_vert,maximized_horz"]);
            ok("wmctrl", &["-i", "-r", &hex, "-e", &format!("1,{x},{y},{cw},{ch}")])
        } else {
            let id = id.to_string();
            let _ = ok("xdotool", &["windowstate", "--remove", "MAXIMIZED_VERT,MAXIMIZED_HORZ", &id]);
            ok("xdotool", &["windowsize", &id, &cw, &ch]) && ok("xdotool", &["windowmove", &id, &x.to_string(), &y.to_string()])
        }
    }

    /// The terminal's X window: `$WINDOWID`, else the active window (only
    /// under a pure X11 session, where it is the terminal that started gm).
    #[cfg(not(target_os = "macos"))]
    fn window_id() -> Option<u64> {
        if let Some(id) = std::env::var("WINDOWID").ok().and_then(|s| s.trim().parse().ok()) {
            return Some(id);
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_some() || is_wsl() {
            return None;
        }
        if let Some(s) = output("xprop", &["-root", "_NET_ACTIVE_WINDOW"]) {
            let hex = s.rsplit('#').next()?.trim().trim_start_matches("0x");
            return u64::from_str_radix(hex, 16).ok().filter(|&id| id != 0);
        }
        output("xdotool", &["getactivewindow"])?.trim().parse().ok()
    }

    /// The desktop work area (without panels), narrowed to the monitor that
    /// holds the window.
    #[cfg(not(target_os = "macos"))]
    fn work_area(id: u64) -> Option<Rect> {
        let mut area = output("wmctrl", &["-d"])
            .and_then(|s| {
                let line = s.lines().find(|l| l.split_whitespace().nth(1) == Some("*"))?.to_string();
                let wa = line.split("WA:").nth(1)?.split_whitespace().take(2).collect::<Vec<_>>();
                let (x, y) = wa.first()?.split_once(',')?;
                let (w, h) = wa.get(1)?.split_once('x')?;
                Some((x.parse().ok()?, y.parse().ok()?, w.parse().ok()?, h.parse().ok()?))
            })
            .or_else(|| {
                let s = output("xdotool", &["getdisplaygeometry"])?;
                let mut it = s.split_whitespace().map(|v| v.parse::<i32>());
                Some((0, 0, it.next()?.ok()?, it.next()?.ok()?))
            })?;
        if let (Some(mon), Some(win)) = (monitors(), window_geometry(id)) {
            let (cx, cy) = (win.0 + win.2 / 2, win.1 + win.3 / 2);
            if let Some(m) = mon.into_iter().find(|m| cx >= m.0 && cx < m.0 + m.2 && cy >= m.1 && cy < m.1 + m.3) {
                let (x1, y1) = (area.0.max(m.0), area.1.max(m.1));
                let (x2, y2) = ((area.0 + area.2).min(m.0 + m.2), (area.1 + area.3).min(m.1 + m.3));
                area = if x2 > x1 && y2 > y1 { (x1, y1, x2 - x1, y2 - y1) } else { m };
            }
        }
        Some(area)
    }

    /// Monitors from `xrandr --listactivemonitors` (` 0: +*DP-1 1920/527x1080/296+0+0  DP-1`).
    #[cfg(not(target_os = "macos"))]
    fn monitors() -> Option<Vec<Rect>> {
        let s = output("xrandr", &["--listactivemonitors"])?;
        let v: Vec<Rect> = s
            .lines()
            .skip(1)
            .filter_map(|l| {
                let g = l.split_whitespace().nth(2)?;
                let (w, rest) = g.split_once('x')?;
                let mut p = rest.split('+');
                let h = p.next()?;
                let num = |s: &str| s.split('/').next()?.parse::<i32>().ok();
                Some((num(p.next()?)?, num(p.next()?)?, num(w)?, num(h)?))
            })
            .collect();
        (v.len() > 1).then_some(v)
    }

    #[cfg(not(target_os = "macos"))]
    fn window_geometry(id: u64) -> Option<Rect> {
        let s = output("xwininfo", &["-id", &id.to_string()])?;
        let get = |key: &str| s.lines().find_map(|l| l.trim().strip_prefix(key)?.trim().parse::<i32>().ok());
        Some((get("Absolute upper-left X:")?, get("Absolute upper-left Y:")?, get("Width:")?, get("Height:")?))
    }

    /// Window manager decorations: left, right, top, bottom.
    #[cfg(not(target_os = "macos"))]
    fn frame_extents(hex: &str) -> (i32, i32, i32, i32) {
        output("xprop", &["-id", hex, "_NET_FRAME_EXTENTS"])
            .and_then(|s| {
                let v: Vec<i32> = s.split('=').nth(1)?.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                (v.len() == 4).then(|| (v[0], v[1], v[2], v[3]))
            })
            .unwrap_or((0, 0, 0, 0))
    }

    // ---- Any other terminal: xterm window operations (CSI … t) ----

    fn xterm(pos: WindowPos) {
        use std::io::{IsTerminal, Write};
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return;
        }
        let seq = match pos {
            WindowPos::Normal => return,
            WindowPos::Full => "\x1b[9;1t".to_string(),
            WindowPos::Part { .. } => {
                let Some((sw, sh)) = screen_pixels() else { return };
                let (x, y, w, h) = part_rect((0, 0, sw, sh), pos.halves().unwrap_or((0, 0, 2, 2)));
                format!("\x1b[9;0t\x1b[3;{x};{y}t\x1b[4;{h};{w}t")
            }
        };
        let mut out = std::io::stdout();
        let _ = out.write_all(seq.as_bytes());
        let _ = out.flush();
    }

    /// Asks the terminal for the screen size in pixels (`CSI 15 t`). A
    /// primary device attributes request follows, so terminals that ignore
    /// the first query answer quickly anyway.
    fn screen_pixels() -> Option<(i32, i32)> {
        use rustix::termios::{OptionalActions, tcgetattr, tcsetattr};
        use std::io::Write;
        use std::time::{Duration, Instant};
        let stdin = std::io::stdin();
        let saved = tcgetattr(&stdin).ok()?;
        let mut raw = saved.clone();
        raw.make_raw();
        tcsetattr(&stdin, OptionalActions::Now, &raw).ok()?;
        let mut reply = Vec::new();
        let mut out = std::io::stdout();
        if out.write_all(b"\x1b[15t\x1b[c").and_then(|_| out.flush()).is_ok() {
            let deadline = Instant::now() + Duration::from_millis(500);
            let mut buf = [0u8; 256];
            loop {
                let left = deadline.saturating_duration_since(Instant::now()).as_millis() as i32;
                if left <= 0 {
                    break;
                }
                let mut fds = [rustix::event::PollFd::new(&stdin, rustix::event::PollFlags::IN)];
                if rustix::event::poll(&mut fds, left).unwrap_or(0) == 0 {
                    break;
                }
                match rustix::io::read(&stdin, &mut buf) {
                    Ok(n) if n > 0 => reply.extend_from_slice(&buf[..n]),
                    _ => break,
                }
                // The device attributes reply (`CSI ? … c`) ends the exchange.
                if let Some(p) = reply.windows(3).position(|w| w == b"\x1b[?")
                    && reply[p..].contains(&b'c')
                {
                    break;
                }
            }
        }
        let _ = tcsetattr(&stdin, OptionalActions::Now, &saved);
        let s = String::from_utf8_lossy(&reply);
        let body = s.split("\x1b[5;").nth(1)?.split('t').next()?;
        let (h, w) = body.split_once(';')?;
        let (h, w) = (h.parse::<i32>().ok()?, w.parse::<i32>().ok()?);
        (h > 0 && w > 0).then_some((w, h))
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn base64_matches_rfc4648() {
            assert_eq!(super::base64(b""), "");
            assert_eq!(super::base64(b"f"), "Zg==");
            assert_eq!(super::base64(b"fo"), "Zm8=");
            assert_eq!(super::base64(b"foo"), "Zm9v");
            assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
        }
    }
}

#[cfg(not(any(windows, unix)))]
mod imp {
    pub fn place(_: super::WindowPos) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_positions() {
        assert_eq!(WindowPos::parse("n"), Some(WindowPos::Normal));
        assert_eq!(WindowPos::parse("F"), Some(WindowPos::Full));
        assert_eq!(WindowPos::parse("tl"), WindowPos::parse("lt"));
        assert_eq!(WindowPos::parse("br"), Some(WindowPos::Part { x: 1, y: 1, w: 1, h: 1 }));
        assert_eq!(WindowPos::parse("b"), Some(WindowPos::Part { x: 0, y: 1, w: 2, h: 1 }));
        assert_eq!(WindowPos::parse("x"), None);
        assert_eq!(WindowPos::parse("lr"), None);
    }

    #[test]
    fn parts_tile_the_area() {
        let area = (10, 20, 1921, 1081);
        let r = |s| part_rect(area, WindowPos::parse(s).unwrap().halves().unwrap());
        assert_eq!(r("f"), area);
        assert_eq!(r("l"), (10, 20, 960, 1081));
        assert_eq!(r("r"), (970, 20, 961, 1081));
        assert_eq!(r("t"), (10, 20, 1921, 540));
        assert_eq!(r("b"), (10, 560, 1921, 541));
        assert_eq!(r("rb"), (970, 560, 961, 541));
    }
}

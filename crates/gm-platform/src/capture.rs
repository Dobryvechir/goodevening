//! Running a command with its output logged (F11 / Shift+F11 in panels).
//!
//! The program keeps the terminal's input (stdin is inherited, so it can
//! talk to the user); its stdout and stderr go through pipes. Every chunk is
//! passed on to the terminal unchanged and also appended to a Markdown log
//! where ANSI colors become `<span style="color:…">` elements. Uncolored
//! error output is logged in red. The log is flushed every 0.5 s and closed
//! when the program ends.

use crate::shell::shell_command;
use gm_text::Codepage;
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often the log is flushed to disk while the program runs.
const FLUSH_EVERY: Duration = Duration::from_millis(500);
/// How long to wait for the pipes to close after the program has exited
/// (a background process it started may keep them open).
const DRAIN_WAIT: Duration = Duration::from_secs(1);

type Log = Arc<Mutex<Option<BufWriter<File>>>>;

/// Runs `line` through the shell in `cwd`, logging its output to `log`.
/// `prompt` is written as the first line of the log. With `quiet`, the
/// program's normal output is only logged; errors are still shown.
pub fn run_logged(line: &str, cwd: &Path, log: File, prompt: &str, quiet: bool) -> io::Result<ExitStatus> {
    let mut cmd = shell_command(line, cwd);
    // Output goes to pipes: ask programs to keep their colors and not to
    // buffer what they print (our pump shows it at once).
    for (k, v) in [("FORCE_COLOR", "1"), ("CLICOLOR_FORCE", "1"), ("PYTHONUNBUFFERED", "1")] {
        if std::env::var_os(k).is_none() {
            cmd.env(k, v);
        }
    }
    cmd.stdin(Stdio::inherit()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let log: Log = Arc::new(Mutex::new(Some(BufWriter::new(log))));
    write_log(&log, &format!("# Goodevening Manager log\n\n<pre>\n{}\n", escape_html(prompt)));

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            write_log(&log, &format!("<span style=\"color:red\">{}</span>\n</pre>\n", escape_html(&e.to_string())));
            close_log(&log);
            return Err(e);
        }
    };
    let _vt = console::VtGuard::enable();
    let cp = console::output_codepage();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let mut pumps = 0;
    if let Some(out) = child.stdout.take() {
        spawn_pump(out, false, !quiet, cp, log.clone(), done_tx.clone());
        pumps += 1;
    }
    if let Some(err) = child.stderr.take() {
        spawn_pump(err, true, true, cp, log.clone(), done_tx.clone());
        pumps += 1;
    }
    drop(done_tx);

    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let flusher = {
        let log = log.clone();
        std::thread::spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = stop_rx.recv_timeout(FLUSH_EVERY) {
                if let Some(w) = lock(&log).as_mut() {
                    let _ = w.flush();
                }
            }
        })
    };

    let status = child.wait();
    for _ in 0..pumps {
        if done_rx.recv_timeout(DRAIN_WAIT).is_err() {
            break;
        }
    }
    let _ = stop_tx.send(());
    let _ = flusher.join();
    let footer = match &status {
        Ok(s) => match s.code() {
            Some(c) => format!("</pre>\n\nExit code: {c}\n"),
            None => format!("</pre>\n\n{s}\n"),
        },
        Err(e) => format!("</pre>\n\n{}\n", escape_html(&e.to_string())),
    };
    write_log(&log, &footer);
    close_log(&log);
    status
}

fn lock(log: &Log) -> std::sync::MutexGuard<'_, Option<BufWriter<File>>> {
    log.lock().unwrap_or_else(|e| e.into_inner())
}

fn write_log(log: &Log, s: &str) {
    if let Some(w) = lock(log).as_mut() {
        let _ = w.write_all(s.as_bytes());
    }
}

/// Flushes and closes the log; pumps still running write nothing more.
fn close_log(log: &Log) {
    if let Some(mut w) = lock(log).take() {
        let _ = w.flush();
    }
}

fn spawn_pump(mut src: impl Read + Send + 'static, is_err: bool, show: bool, cp: Codepage, log: Log, done: mpsc::Sender<()>) {
    std::thread::spawn(move || {
        let mut conv = AnsiToHtml::new(cp, if is_err { Some("red") } else { None });
        let mut buf = [0u8; 8192];
        loop {
            let n = match src.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            if show {
                console::write(is_err, &buf[..n]);
            }
            let html = conv.feed(&buf[..n]);
            if !html.is_empty() {
                write_log(&log, &html);
            }
        }
        let html = conv.finish();
        if !html.is_empty() {
            write_log(&log, &html);
        }
        let _ = done.send(());
    });
}

/// Escapes text for HTML (inside `<pre>` in the Markdown log).
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
    out
}

/// The 16 standard terminal colors (Windows Terminal's "Campbell" scheme).
const BASIC: [&str; 16] = [
    "#0C0C0C", "#C50F1F", "#13A10E", "#C19C00", "#0037DA", "#881798", "#3A96DD", "#CCCCCC", "#767676", "#E74856", "#16C60C", "#F9F1A5", "#3B78FF",
    "#B4009E", "#61D6D6", "#F2F2F2",
];

fn color_256(n: u16) -> String {
    match n {
        0..=15 => BASIC[n as usize].to_string(),
        16..=231 => {
            let lv = [0u8, 95, 135, 175, 215, 255];
            let i = n - 16;
            format!("#{:02X}{:02X}{:02X}", lv[(i / 36) as usize], lv[(i / 6 % 6) as usize], lv[(i % 6) as usize])
        }
        _ => {
            let g = 8 + 10 * (n.min(255) - 232) as u8;
            format!("#{g:02X}{g:02X}{g:02X}")
        }
    }
}

/// Converts a byte stream with ANSI escape sequences into HTML with
/// colored spans. Sequences and characters split between chunks are kept
/// until the rest arrives.
pub struct AnsiToHtml {
    cp: Codepage,
    /// Color used when the output sets none (red for errors).
    default_fg: Option<&'static str>,
    fg: Option<String>,
    bg: Option<String>,
    pending: Vec<u8>,
    /// A `\r` at the end of the previous chunk.
    cr: bool,
}

impl AnsiToHtml {
    pub fn new(cp: Codepage, default_fg: Option<&'static str>) -> AnsiToHtml {
        AnsiToHtml { cp, default_fg, fg: None, bg: None, pending: Vec::new(), cr: false }
    }

    fn style(&self) -> Option<String> {
        let fg = self.fg.as_deref().or(self.default_fg);
        match (fg, &self.bg) {
            (None, None) => None,
            (Some(f), None) => Some(format!("color:{f}")),
            (None, Some(b)) => Some(format!("background-color:{b}")),
            (Some(f), Some(b)) => Some(format!("color:{f};background-color:{b}")),
        }
    }

    /// Converts the next chunk; returns the HTML for what is complete.
    pub fn feed(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let data = std::mem::take(&mut self.pending);
        let mut out = Out::default();
        let mut i = 0;
        let mut text_start = 0;
        while i < data.len() {
            if data[i] != 0x1b {
                i += 1;
                continue;
            }
            match sequence_len(&data[i..]) {
                None if data.len() - i < 256 => break,
                None => {
                    // Too long to be a real sequence: drop the ESC.
                    self.text(&mut out, &data[text_start..i]);
                    i += 1;
                    text_start = i;
                }
                Some(n) => {
                    self.text(&mut out, &data[text_start..i]);
                    let seq = &data[i..i + n];
                    if seq.len() >= 3 && seq[1] == b'[' && seq[n - 1] == b'm' {
                        self.sgr(&seq[2..n - 1]);
                    }
                    i += n;
                    text_start = i;
                }
            }
        }
        // Text up to `i`, except an incomplete UTF-8 character at its end.
        let mut end = i;
        if self.cp == Codepage::Utf8 && i == data.len() {
            end = text_start + complete_utf8_len(&data[text_start..i]);
        }
        self.text(&mut out, &data[text_start..end]);
        self.pending = data[end..].to_vec();
        out.finish()
    }

    /// Converts whatever is left at the end of the stream.
    pub fn finish(&mut self) -> String {
        let rest = std::mem::take(&mut self.pending);
        let mut out = Out::default();
        self.text(&mut out, &rest);
        out.finish()
    }

    fn text(&mut self, out: &mut Out, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let s = self.cp.decode(bytes);
        let mut t = String::with_capacity(s.len());
        for c in s.chars() {
            if self.cr {
                self.cr = false;
                if c != '\n' {
                    // A lone CR (a progress line being redrawn): start a new line.
                    t.push('\n');
                }
            }
            match c {
                '\r' => self.cr = true,
                '\n' | '\t' => t.push(c),
                c if c.is_control() => {}
                c => t.push(c),
            }
        }
        if !t.is_empty() {
            out.push(self.style(), &escape_html(&t));
        }
    }

    fn sgr(&mut self, params: &[u8]) {
        let p: Vec<u16> = String::from_utf8_lossy(params).split([';', ':']).map(|x| x.parse().unwrap_or(0)).collect();
        let mut k = 0;
        while k < p.len() {
            match p[k] {
                0 => {
                    self.fg = None;
                    self.bg = None;
                }
                n @ 30..=37 => self.fg = Some(BASIC[(n - 30) as usize].into()),
                n @ 90..=97 => self.fg = Some(BASIC[(n - 90 + 8) as usize].into()),
                39 => self.fg = None,
                n @ 40..=47 => self.bg = Some(BASIC[(n - 40) as usize].into()),
                n @ 100..=107 => self.bg = Some(BASIC[(n - 100 + 8) as usize].into()),
                49 => self.bg = None,
                n @ (38 | 48) => {
                    let c = match p.get(k + 1) {
                        Some(5) => {
                            let c = p.get(k + 2).map(|&x| color_256(x));
                            k += 2;
                            c
                        }
                        Some(2) => {
                            let c =
                                (p.len() > k + 4).then(|| format!("#{:02X}{:02X}{:02X}", p[k + 2].min(255), p[k + 3].min(255), p[k + 4].min(255)));
                            k += 4;
                            c
                        }
                        _ => None,
                    };
                    if let Some(c) = c {
                        if n == 38 {
                            self.fg = Some(c);
                        } else {
                            self.bg = Some(c);
                        }
                    }
                }
                _ => {}
            }
            k += 1;
        }
    }
}

/// HTML output with the currently open span.
#[derive(Default)]
struct Out {
    html: String,
    open: Option<String>,
}

impl Out {
    fn push(&mut self, style: Option<String>, text: &str) {
        if self.open != style {
            if self.open.is_some() {
                self.html.push_str("</span>");
            }
            if let Some(s) = &style {
                self.html.push_str(&format!("<span style=\"{s}\">"));
            }
            self.open = style;
        }
        self.html.push_str(text);
    }

    /// Closes the open span, so the chunk is complete HTML (the other
    /// stream's output may come next in the log).
    fn finish(mut self) -> String {
        if self.open.is_some() {
            self.html.push_str("</span>");
        }
        self.html
    }
}

/// Length of the escape sequence at the start of `b` (which starts with
/// ESC), or None if it is not complete yet.
fn sequence_len(b: &[u8]) -> Option<usize> {
    match *b.get(1)? {
        // CSI: parameters, then a final byte 0x40..=0x7E.
        b'[' => b[2..].iter().position(|c| (0x40..=0x7e).contains(c)).map(|p| p + 3),
        // OSC (window title…): ends with BEL or ESC \.
        b']' | b'P' | b'_' | b'^' => {
            let mut j = 2;
            while j < b.len() {
                if b[j] == 0x07 {
                    return Some(j + 1);
                }
                if b[j] == 0x1b && j + 1 < b.len() {
                    return Some(j + 2);
                }
                j += 1;
            }
            None
        }
        // Character set selection: ESC ( B.
        b'(' | b')' => b.get(2).map(|_| 3),
        _ => Some(2),
    }
}

/// Length of `b` without an incomplete UTF-8 character at its end.
fn complete_utf8_len(b: &[u8]) -> usize {
    let n = b.len();
    for back in 1..=n.min(3) {
        let c = b[n - back];
        if c & 0xC0 == 0x80 {
            continue; // continuation byte
        }
        let need = match c {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => 1,
        };
        return if need > back { n - back } else { n };
    }
    n
}

#[cfg(windows)]
mod console {
    use gm_text::Codepage;
    use windows_sys::Win32::Storage::FileSystem::WriteFile;
    use windows_sys::Win32::System::Console::{
        CONSOLE_MODE, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetConsoleOutputCP, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        SetConsoleMode,
    };

    /// Writes raw bytes to the console: they are in the console's code page,
    /// like those the program would have written itself.
    pub fn write(is_err: bool, bytes: &[u8]) {
        // SAFETY: GetStdHandle has no preconditions; WriteFile gets a live
        // buffer with its length and a valid out pointer.
        unsafe {
            let h = GetStdHandle(if is_err { STD_ERROR_HANDLE } else { STD_OUTPUT_HANDLE });
            let mut rest = bytes;
            while !rest.is_empty() {
                let mut written = 0u32;
                let len = rest.len().min(u32::MAX as usize) as u32;
                if WriteFile(h, rest.as_ptr(), len, &mut written, std::ptr::null_mut()) == 0 || written == 0 {
                    return;
                }
                rest = &rest[written as usize..];
            }
        }
    }

    pub fn output_codepage() -> Codepage {
        // SAFETY: no preconditions.
        let cp = unsafe { GetConsoleOutputCP() };
        if cp == 0 {
            return Codepage::ansi();
        }
        Codepage::from_id(cp).or_else(|| encoding_for(cp)).unwrap_or(Codepage::Utf8)
    }

    fn encoding_for(cp: u32) -> Option<Codepage> {
        let label = match cp {
            932 => "shift_jis",
            936 => "gbk",
            949 => "euc-kr",
            950 => "big5",
            _ => return None,
        };
        encoding_rs::Encoding::for_label(label.as_bytes()).map(Codepage::Web)
    }

    /// Lets the console interpret the ANSI colors programs print; restores
    /// the previous mode when dropped.
    pub struct VtGuard(Option<CONSOLE_MODE>);

    impl VtGuard {
        pub fn enable() -> VtGuard {
            // SAFETY: valid handle and out pointer.
            unsafe {
                let h = GetStdHandle(STD_OUTPUT_HANDLE);
                let mut mode: CONSOLE_MODE = 0;
                if GetConsoleMode(h, &mut mode) == 0 || mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0 {
                    return VtGuard(None);
                }
                if SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) == 0 {
                    return VtGuard(None);
                }
                VtGuard(Some(mode))
            }
        }
    }

    impl Drop for VtGuard {
        fn drop(&mut self) {
            if let Some(mode) = self.0 {
                // SAFETY: valid handle.
                unsafe {
                    SetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), mode);
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod console {
    use gm_text::Codepage;
    use std::io::Write;

    pub fn write(is_err: bool, bytes: &[u8]) {
        if is_err {
            let mut e = std::io::stderr().lock();
            let _ = e.write_all(bytes);
            let _ = e.flush();
        } else {
            let mut o = std::io::stdout().lock();
            let _ = o.write_all(bytes);
            let _ = o.flush();
        }
    }

    pub fn output_codepage() -> Codepage {
        Codepage::Utf8
    }

    pub struct VtGuard;

    impl VtGuard {
        pub fn enable() -> VtGuard {
            VtGuard
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(chunks: &[&[u8]], err: bool) -> String {
        let mut c = AnsiToHtml::new(Codepage::Utf8, if err { Some("red") } else { None });
        let mut s: String = chunks.iter().map(|ch| c.feed(ch)).collect();
        s.push_str(&c.finish());
        s
    }

    #[test]
    fn plain_text_is_escaped() {
        assert_eq!(conv(&[b"a <b> & c\r\n"], false), "a &lt;b&gt; &amp; c\n");
    }

    #[test]
    fn colors_become_spans() {
        assert_eq!(conv(&[b"x\x1b[31mred\x1b[0m y"], false), "x<span style=\"color:#C50F1F\">red</span> y");
        assert_eq!(
            conv(&[b"\x1b[38;5;196mA\x1b[38;2;1;2;3mB\x1b[39m"], false),
            "<span style=\"color:#FF0000\">A</span><span style=\"color:#010203\">B</span>"
        );
        assert_eq!(conv(&[b"\x1b[1;92;44mok"], false), "<span style=\"color:#16C60C;background-color:#0037DA\">ok</span>");
    }

    #[test]
    fn errors_default_to_red() {
        assert_eq!(conv(&[b"bad\n"], true), "<span style=\"color:red\">bad\n</span>");
        assert_eq!(conv(&[b"\x1b[33mwarn\x1b[0m!"], true), "<span style=\"color:#C19C00\">warn</span><span style=\"color:red\">!</span>");
    }

    #[test]
    fn split_sequences_and_characters() {
        assert_eq!(conv(&[b"a\x1b[3", b"2mb"], false), "a<span style=\"color:#13A10E\">b</span>");
        let s = "й".as_bytes();
        assert_eq!(conv(&[&s[..1], &s[1..]], false), "й");
    }

    #[test]
    fn other_sequences_are_dropped() {
        assert_eq!(conv(&[b"\x1b]0;title\x07a\x1b[2Kb\x1b(Bc"], false), "abc");
        assert_eq!(conv(&[b"10%\r20%\r\n"], false), "10%\n20%\n");
    }

    #[test]
    fn logged_run() {
        let dir = std::env::temp_dir().join(format!("gm-capture-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.md");
        let line = if cfg!(windows) { "echo hello& echo oops 1>&2" } else { "echo hello; echo oops >&2" };
        let st = run_logged(line, &dir, File::create(&path).unwrap(), "> test", true).unwrap();
        assert!(st.success());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.ends_with("Exit code: 0\n"), "{text}");
        assert!(text.contains("hello"), "{text}");
        assert!(text.contains("<span style=\"color:red\">oops"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

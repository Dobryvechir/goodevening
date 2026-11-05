//! Commands typed on the command line of a remote panel: run like
//! `ssh -t host 'cd <panel folder> && <command>'` — a pseudo-terminal of the
//! size of the console, keys sent as a terminal sends them, the output
//! written to the console as it comes.

use crate::conn::{Conn, err, rt};
use crate::provider::sh_quote;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use russh::ChannelMsg;
use std::io::{self, Write};
use std::time::Duration;

/// Bytes a terminal sends for a key. `app_cursor`: the remote program
/// switched the arrow keys to application mode (`ESC [ ? 1 h`).
pub fn key_bytes(k: &KeyEvent, app_cursor: bool) -> Vec<u8> {
    let m = k.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let shift = m.contains(KeyModifiers::SHIFT);
    // xterm modifier parameter: 1 + Shift 1 + Alt 2 + Ctrl 4.
    let modp = 1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8;
    let csi_letter = |c: char| -> Vec<u8> {
        if modp > 1 {
            format!("\x1b[1;{modp}{c}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    let csi_tilde = |n: u8| -> Vec<u8> { if modp > 1 { format!("\x1b[{n};{modp}~").into_bytes() } else { format!("\x1b[{n}~").into_bytes() } };
    let mut out = match k.code {
        KeyCode::Char(c) => {
            // AltGr arrives as Ctrl+Alt with the resulting character.
            let altgr = ctrl && alt && !c.is_ascii_alphabetic();
            if ctrl && !altgr {
                let b = match c {
                    'a'..='z' | 'A'..='Z' => (c.to_ascii_lowercase() as u8) & 0x1f,
                    ' ' | '@' | '2' => 0,
                    '[' | '3' => 0x1b,
                    '\\' | '4' => 0x1c,
                    ']' | '5' => 0x1d,
                    '^' | '6' => 0x1e,
                    '_' | '7' | '-' => 0x1f,
                    '?' | '8' => 0x7f,
                    _ => return c.to_string().into_bytes(),
                };
                let mut v = Vec::new();
                if alt {
                    v.push(0x1b);
                }
                v.push(b);
                return v;
            }
            let mut v = Vec::new();
            if alt && !altgr {
                v.push(0x1b);
            }
            v.extend(c.to_string().into_bytes());
            return v;
        }
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => {
            if ctrl {
                vec![0x08]
            } else {
                vec![0x7f]
            }
        }
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => csi_letter('A'),
        KeyCode::Down => csi_letter('B'),
        KeyCode::Right => csi_letter('C'),
        KeyCode::Left => csi_letter('D'),
        KeyCode::Home => csi_letter('H'),
        KeyCode::End => csi_letter('F'),
        KeyCode::Insert => csi_tilde(2),
        KeyCode::Delete => csi_tilde(3),
        KeyCode::PageUp => csi_tilde(5),
        KeyCode::PageDown => csi_tilde(6),
        KeyCode::F(n @ 1..=4) => {
            let c = (b'P' + n - 1) as char;
            if modp > 1 { format!("\x1b[1;{modp}{c}").into_bytes() } else { format!("\x1bO{c}").into_bytes() }
        }
        KeyCode::F(n @ 5..=12) => csi_tilde([15, 17, 18, 19, 20, 21, 23, 24][(n - 5) as usize]),
        _ => Vec::new(),
    };
    // Alt+Enter, Alt+Backspace…: ESC prefix.
    if alt && matches!(k.code, KeyCode::Enter | KeyCode::Backspace | KeyCode::Tab | KeyCode::Esc) {
        out.insert(0, 0x1b);
    }
    out
}

/// Notices DECCKM (application cursor keys) switches in the output.
fn track_cursor_mode(data: &[u8], app_cursor: &mut bool) {
    let on = b"\x1b[?1h";
    let off = b"\x1b[?1l";
    for i in 0..data.len() {
        if data[i..].starts_with(on) {
            *app_cursor = true;
        } else if data[i..].starts_with(off) {
            *app_cursor = false;
        }
    }
}

/// Puts the console in raw mode for the session and back afterwards.
struct RawMode;

impl RawMode {
    fn enable() -> io::Result<RawMode> {
        #[cfg(windows)]
        let _ = crossterm::ansi_support::supports_ansi();
        crossterm::terminal::enable_raw_mode()?;
        Ok(RawMode)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Runs `line` in `dir` on the remote machine, connected to the console.
pub fn run(conn: &Conn, dir: &str, line: &str) -> io::Result<()> {
    let command = format!("cd {} && {line}", sh_quote(dir));
    let term = std::env::var("TERM").ok().filter(|t| !t.is_empty() && t != "dumb").unwrap_or_else(|| "xterm-256color".into());
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 25));
    rt().block_on(async {
        let mut ch = conn.handle.channel_open_session().await.map_err(err)?;
        ch.request_pty(false, &term, cols as u32, rows as u32, 0, 0, &[]).await.map_err(err)?;
        ch.exec(true, command.as_bytes()).await.map_err(err)?;
        let _raw = RawMode::enable()?;
        let mut stdout = io::stdout();
        let mut stderr = io::stderr();
        let mut app_cursor = false;
        let mut exit: Option<u32> = None;
        loop {
            tokio::select! {
                msg = ch.wait() => match msg {
                    Some(ChannelMsg::Data { data }) => {
                        track_cursor_mode(&data, &mut app_cursor);
                        stdout.write_all(&data)?;
                        stdout.flush()?;
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        stderr.write_all(&data)?;
                        stderr.flush()?;
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => exit = Some(exit_status),
                    Some(ChannelMsg::ExitSignal { signal_name, .. }) => {
                        let _ = write!(stderr, "\r\n[{signal_name:?}]\r\n");
                    }
                    Some(ChannelMsg::Failure) => return Err(err("The server refused to run the command")),
                    Some(ChannelMsg::Close) | None => break,
                    _ => {}
                },
                _ = tokio::time::sleep(Duration::from_millis(15)) => {
                    let mut buf = Vec::new();
                    while event::poll(Duration::ZERO)? {
                        match event::read()? {
                            Event::Key(k) if k.kind != KeyEventKind::Release => buf.extend(key_bytes(&k, app_cursor)),
                            Event::Paste(s) => buf.extend(s.into_bytes()),
                            Event::Resize(c, r) => {
                                let _ = ch.window_change(c as u32, r as u32, 0, 0).await;
                            }
                            _ => {}
                        }
                    }
                    if !buf.is_empty() {
                        ch.data(&buf[..]).await.map_err(err)?;
                    }
                }
            }
        }
        let _ = exit;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    #[test]
    fn keys() {
        let n = KeyModifiers::NONE;
        assert_eq!(key_bytes(&k(KeyCode::Char('a'), n), false), b"a");
        assert_eq!(key_bytes(&k(KeyCode::Char('c'), KeyModifiers::CONTROL), false), vec![3]);
        assert_eq!(key_bytes(&k(KeyCode::Char('x'), KeyModifiers::ALT), false), b"\x1bx");
        assert_eq!(key_bytes(&k(KeyCode::Char('@'), KeyModifiers::CONTROL | KeyModifiers::ALT), false), b"@");
        assert_eq!(key_bytes(&k(KeyCode::Enter, n), false), b"\r");
        assert_eq!(key_bytes(&k(KeyCode::Up, n), false), b"\x1b[A");
        assert_eq!(key_bytes(&k(KeyCode::Up, n), true), b"\x1bOA");
        assert_eq!(key_bytes(&k(KeyCode::Right, KeyModifiers::CONTROL), false), b"\x1b[1;5C");
        assert_eq!(key_bytes(&k(KeyCode::Delete, n), false), b"\x1b[3~");
        assert_eq!(key_bytes(&k(KeyCode::F(1), n), false), b"\x1bOP");
        assert_eq!(key_bytes(&k(KeyCode::F(5), n), false), b"\x1b[15~");
        assert_eq!(key_bytes(&k(KeyCode::F(12), n), false), b"\x1b[24~");
        assert_eq!(key_bytes(&k(KeyCode::Backspace, n), false), vec![0x7f]);
        let mut app = false;
        track_cursor_mode(b"abc\x1b[?1hdef", &mut app);
        assert!(app);
        track_cursor_mode(b"\x1b[?1l", &mut app);
        assert!(!app);
    }
}

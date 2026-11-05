//! `gm --e|--d|--pe|--pd key_file input_file output_file`: the command line
//! front end of [`gm_core::codec`], a byte-exact port of `codec.py`
//! (`python codec.py e|d|pe|pd key_file input_file output_file`).
//!
//! `--pe` / `--pd` first ask for a password (at least 1 character) that
//! unmasks the key file: `key[i] ^= password[i % len(password)]`.

use gm_core::codec::{self, Mode};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

pub const USAGE: &str = "Usage: gm --e|--d|--pe|--pd key_file input_file output_file";

/// Maps a command line flag to (mode, asks for a password).
pub fn flag(arg: &str) -> Option<(Mode, bool)> {
    match arg {
        "--e" => Some((Mode::Encode, false)),
        "--d" => Some((Mode::Decode, false)),
        "--pe" => Some((Mode::Encode, true)),
        "--pd" => Some((Mode::Decode, true)),
        _ => None,
    }
}

/// Asks for a non-empty password like Python's `getpass`: hidden input on a
/// console; when stdin is redirected, one line is read from it instead.
fn ask_password() -> io::Result<Vec<u8>> {
    loop {
        let p = if io::stdin().is_terminal() { read_hidden("Password: ")? } else { read_line("Password: ")? };
        if !p.is_empty() {
            return Ok(p.into_bytes());
        }
        eprintln!("Password must be at least 1 character long.");
    }
}

fn read_line(prompt: &str) -> io::Result<String> {
    eprint!("{prompt}");
    let mut s = String::new();
    if io::stdin().lock().read_line(&mut s)? == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "no password given"));
    }
    while s.ends_with('\n') || s.ends_with('\r') {
        s.pop();
    }
    Ok(s)
}

fn read_hidden(prompt: &str) -> io::Result<String> {
    use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, read};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
    eprint!("{prompt}");
    io::stderr().flush()?;
    enable_raw_mode()?;
    let mut s = String::new();
    let r = loop {
        let ev = match read() {
            Ok(ev) => ev,
            Err(e) => break Err(e),
        };
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => match k.code {
                KeyCode::Enter => break Ok(()),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    break Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted"));
                }
                KeyCode::Backspace => {
                    s.pop();
                }
                KeyCode::Char(c) => s.push(c),
                _ => {}
            },
            Event::Paste(p) => s.push_str(&p),
            _ => {}
        }
    };
    let _ = disable_raw_mode();
    eprintln!();
    r.map(|()| s)
}

/// Runs the codec for `args` = everything after the program name; the first
/// one is the `--e|--d|--pe|--pd` flag.
pub fn main(args: &[String]) -> ExitCode {
    let Some((mode, with_password)) = args.first().and_then(|a| flag(a)) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if args.len() != 4 {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let (key_file, input_file, output_file) = (&args[1], &args[2], &args[3]);
    let password = if with_password {
        match ask_password() {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("gm: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    let ops = match codec::load_key(Path::new(key_file), password.as_deref()) {
        Ok(o) => o,
        Err(e) => {
            // "Key error: …" exactly as codec.py prints it.
            if e.starts_with("Key error") {
                eprintln!("{e}")
            } else {
                eprintln!("gm: {e}")
            }
            return ExitCode::FAILURE;
        }
    };
    match codec::transform_file(mode, &ops, Path::new(input_file), Path::new(output_file)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gm: {e}");
            ExitCode::FAILURE
        }
    }
}

//! Command queue shared by `gm --client=<file>` and `gm --server=<file>`.
//!
//! The queue is a plain text file in the profile folder (next to
//! `settings.toml`), one command line per line. Every access takes an
//! exclusive lock on the file, so several clients and a server can use the
//! same queue at the same time.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often the server checks the queue file.
const POLL: Duration = Duration::from_millis(200);

/// The queue file `name` inside the profile folder.
pub fn queue_path(name: &str) -> PathBuf {
    gm_config::profile_dir().join(name)
}

/// Opens the queue for reading and writing (creating it) and locks it.
/// The lock is released when the returned file is dropped.
fn open_locked(path: &Path) -> std::io::Result<File> {
    let f = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
    f.lock()?;
    Ok(f)
}

/// Appends one command line to the queue (the `--client` side).
pub fn push(path: &Path, line: &str) -> std::io::Result<()> {
    // A queued command is one line: line breaks inside it become spaces.
    let line: String = line.chars().map(|c| if c == '\r' || c == '\n' { ' ' } else { c }).collect();
    let mut f = open_locked(path)?;
    let mut old = Vec::new();
    f.read_to_end(&mut old)?;
    let mut out = Vec::new();
    if !old.is_empty() && !old.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend_from_slice(line.trim().as_bytes());
    out.push(b'\n');
    f.seek(SeekFrom::End(0))?;
    f.write_all(&out)?;
    f.flush()?;
    f.unlock()
}

/// Removes the first line from the queue and returns it (the `--server`
/// side). Returns `None` when the queue is empty.
pub fn pop(path: &Path) -> std::io::Result<Option<String>> {
    let mut f = open_locked(path)?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;
    if data.is_empty() {
        f.unlock()?;
        return Ok(None);
    }
    let (first, rest) = match data.iter().position(|&b| b == b'\n') {
        Some(i) => (&data[..i], &data[i + 1..]),
        None => (&data[..], &[][..]),
    };
    let line = String::from_utf8_lossy(first).trim_end_matches('\r').to_string();
    let rest = rest.to_vec();
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&rest)?;
    f.set_len(rest.len() as u64)?;
    f.flush()?;
    f.unlock()?;
    Ok(Some(line))
}

/// `gm --server=<file>`: waits for commands in the queue and runs them in
/// this console, showing each command and its output. Runs until killed.
pub fn serve(path: &Path) -> std::io::Result<()> {
    // Make sure the file exists so clients and the user can find it.
    drop(OpenOptions::new().append(true).create(true).open(path)?);
    println!("gm server: waiting for commands in {}", path.display());
    println!("(Ctrl+C stops the server)");
    loop {
        let has_data = std::fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false);
        if !has_data {
            if !path.exists() {
                drop(OpenOptions::new().append(true).create(true).open(path)?);
            }
            std::thread::sleep(POLL);
            continue;
        }
        let line = match pop(path)? {
            Some(l) if !l.trim().is_empty() => l,
            _ => continue,
        };
        run(&line);
    }
}

/// Runs one command line, echoing it first like a shell prompt.
fn run(line: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    println!();
    println!("{}> {line}", cwd.display());
    let _ = std::io::stdout().flush();
    match gm_platform::shell::run_in_shell(line, &cwd) {
        Ok(st) if !st.success() => match st.code() {
            Some(c) => println!("[exit code {c}]"),
            None => println!("[terminated: {st}]"),
        },
        Ok(_) => {}
        Err(e) => eprintln!("gm server: cannot run the command: {e}"),
    }
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("gm-queue-test-{tag}-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn push_and_pop_in_order() {
        let p = temp_file("order");
        push(&p, "echo one").unwrap();
        push(&p, "echo two\r\nthree").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "echo one\necho two  three\n");
        assert_eq!(pop(&p).unwrap().as_deref(), Some("echo one"));
        assert_eq!(pop(&p).unwrap().as_deref(), Some("echo two  three"));
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 0);
        assert_eq!(pop(&p).unwrap(), None);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn handles_crlf_and_missing_final_newline() {
        let p = temp_file("crlf");
        std::fs::write(&p, "first\r\nsecond").unwrap();
        push(&p, "third").unwrap();
        assert_eq!(pop(&p).unwrap().as_deref(), Some("first"));
        assert_eq!(pop(&p).unwrap().as_deref(), Some("second"));
        assert_eq!(pop(&p).unwrap().as_deref(), Some("third"));
        let _ = std::fs::remove_file(&p);
    }
}

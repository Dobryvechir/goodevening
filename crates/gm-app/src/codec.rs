//! `gm --e|--d|--pe|--pd key_file input_file output_file`: encode / decode a
//! file with operations parsed from a key file. A byte-exact port of
//! `codec.py` (`python codec.py e|d|pe|pd key_file input_file output_file`).
//!
//! `--pe` / `--pd` work like `--e` / `--d`, but first ask for a password (at
//! least 1 character) and de-obfuscate the key file bytes before parsing:
//! `key[i] = key[i] ^ password[i % len(password)]`, password as UTF-8.
//!
//! Key file parsing: scan characters; on `*`, `+` or `^` collect the following
//! hex digits until a non-hex character (an odd count is padded with a leading
//! `0`). Operations with no digits are skipped; no operations is an error.
//!
//! Operations (encoding applies them in key order, decoding applies the
//! inverses in reverse order):
//! - `^`  `data[i] ^= code[i % len]`
//! - `+`  `data[i] = data[i] + code[i % len]` (mod 256)
//! - `*`  code is N big-endian 2-byte numbers, a permutation of 0..N-1; each
//!   full block of N bytes becomes `new[j] = old[code[j]]`; a trailing partial
//!   block is left unchanged.
//!
//! The input is processed in chunks: `BUFFER_SIZE` without `*` operations,
//! otherwise a multiple of L = lcm of all `*` sizes (L itself if
//! L >= `BUFFER_SIZE`, else the largest multiple of L that fits), so
//! permutation blocks never straddle a chunk boundary. `^` and `+` use the
//! absolute file position.

use std::fs::File;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::process::ExitCode;

pub const BUFFER_SIZE: usize = 512 * 1024;

pub const USAGE: &str = "Usage: gm --e|--d|--pe|--pd key_file input_file output_file";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Xor(Vec<u8>),
    Add(Vec<u8>),
    Perm(Vec<usize>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Encode,
    Decode,
}

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

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Parses the key. Python decodes the key as UTF-8 with replacement; every
/// ASCII byte survives that decoding unchanged and all operators and hex
/// digits are ASCII, so scanning the raw bytes gives the same result.
pub fn parse_key(text: &[u8]) -> Result<Vec<Op>, String> {
    let mut ops = Vec::new();
    let (mut i, n) = (0, text.len());
    while i < n {
        let ch = text[i];
        i += 1;
        if !matches!(ch, b'*' | b'+' | b'^') {
            continue;
        }
        let start = i;
        while i < n && hex_val(text[i]).is_some() {
            i += 1;
        }
        let mut digits: Vec<u8> = text[start..i].iter().map(|&b| hex_val(b).unwrap()).collect();
        if digits.is_empty() {
            continue;
        }
        if digits.len() % 2 == 1 {
            digits.insert(0, 0);
        }
        let code: Vec<u8> = digits.chunks(2).map(|p| (p[0] << 4) | p[1]).collect();
        ops.push((ch, code));
    }
    if ops.is_empty() {
        return Err("key file contains no operations".into());
    }
    ops.into_iter().map(|(op, code)| prepare(op, code)).collect()
}

fn prepare(op: u8, code: Vec<u8>) -> Result<Op, String> {
    match op {
        b'^' => Ok(Op::Xor(code)),
        b'+' => Ok(Op::Add(code)),
        _ => {
            if code.len() % 2 == 1 {
                return Err(format!("'*' code has odd byte length {}: {}", code.len(), to_hex(&code)));
            }
            let perm: Vec<usize> = code.chunks(2).map(|p| ((p[0] as usize) << 8) | p[1] as usize).collect();
            let mut sorted = perm.clone();
            sorted.sort_unstable();
            if sorted.iter().enumerate().any(|(i, &v)| i != v) {
                let list = perm.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ");
                return Err(format!("'*' code is not a permutation of 0..{}: [{list}]", perm.len() as i64 - 1));
            }
            Ok(Op::Perm(perm))
        }
    }
}

pub fn unmask_key(data: &mut [u8], password: &[u8]) {
    let m = password.len();
    for (i, b) in data.iter_mut().enumerate() {
        *b ^= password[i % m];
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

pub fn buffer_size(ops: &[Op]) -> Result<usize, String> {
    let mut lcm: Option<u64> = None;
    for op in ops {
        if let Op::Perm(p) = op {
            let n = p.len() as u64;
            lcm = Some(match lcm {
                None => n,
                Some(l) => (l / gcd(l, n)).checked_mul(n).ok_or("permutation block size is too large")?,
            });
        }
    }
    match lcm {
        None => Ok(BUFFER_SIZE),
        Some(l) => {
            let l = usize::try_from(l).map_err(|_| "permutation block size is too large".to_string())?;
            Ok(if l >= BUFFER_SIZE { l } else { BUFFER_SIZE / l * l })
        }
    }
}

fn apply_xor(data: &mut [u8], code: &[u8], offset: u64) {
    let m = code.len();
    let mut k = (offset % m as u64) as usize;
    for b in data.iter_mut() {
        *b ^= code[k];
        k += 1;
        if k == m {
            k = 0;
        }
    }
}

fn apply_add(data: &mut [u8], code: &[u8], decode: bool, offset: u64) {
    let m = code.len();
    let mut k = (offset % m as u64) as usize;
    for b in data.iter_mut() {
        *b = if decode { b.wrapping_sub(code[k]) } else { b.wrapping_add(code[k]) };
        k += 1;
        if k == m {
            k = 0;
        }
    }
}

fn apply_perm(data: &mut [u8], perm: &[usize], inverse: bool) {
    let n = perm.len();
    let mut old = vec![0u8; n];
    for block in data.chunks_exact_mut(n) {
        old.copy_from_slice(block);
        for (j, &src) in perm.iter().enumerate() {
            if inverse {
                block[src] = old[j];
            } else {
                block[j] = old[src];
            }
        }
    }
}

/// Applies all operations in place to `data`, which starts at file position `offset`.
pub fn apply_ops(mode: Mode, ops: &[Op], data: &mut [u8], offset: u64) {
    let decode = mode == Mode::Decode;
    let mut run = |op: &Op| match op {
        Op::Xor(c) => apply_xor(data, c, offset),
        Op::Add(c) => apply_add(data, c, decode, offset),
        Op::Perm(p) => apply_perm(data, p, decode),
    };
    if decode { ops.iter().rev().for_each(&mut run) } else { ops.iter().for_each(&mut run) }
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match r.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

/// Streams `input` through the operations into `output`, chunk by chunk.
pub fn process(mode: Mode, ops: &[Op], input: &mut impl Read, output: &mut impl Write) -> Result<(), String> {
    let size = buffer_size(ops)?;
    let mut buf = vec![0u8; size];
    let mut offset = 0u64;
    loop {
        let n = read_full(input, &mut buf).map_err(|e| format!("cannot read the input file: {e}"))?;
        if n == 0 {
            break;
        }
        apply_ops(mode, ops, &mut buf[..n], offset);
        output.write_all(&buf[..n]).map_err(|e| format!("cannot write the output file: {e}"))?;
        offset += n as u64;
        if n < size {
            break;
        }
    }
    output.flush().map_err(|e| format!("cannot write the output file: {e}"))
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
    let mut key = match std::fs::read(key_file) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("gm: cannot read the key file {key_file}: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(p) = &password {
        unmask_key(&mut key, p);
    }
    let ops = match parse_key(&key) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Key error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = File::open(input_file)
        .map_err(|e| format!("cannot open the input file {input_file}: {e}"))
        .and_then(|fin| {
            let fout = File::create(output_file).map_err(|e| format!("cannot create the output file {output_file}: {e}"))?;
            process(mode, &ops, &mut io::BufReader::new(fin), &mut io::BufWriter::new(fout))
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gm: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    fn run(mode: Mode, key: &[u8], data: &[u8]) -> Vec<u8> {
        let ops = parse_key(key).unwrap();
        let mut out = Vec::new();
        process(mode, &ops, &mut &data[..], &mut out).unwrap();
        out
    }

    /// Deterministic pseudo-random bytes.
    fn bytes(n: usize, seed: u64) -> Vec<u8> {
        let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn parse_skips_empty_and_pads_odd() {
        let ops = parse_key(b"zz^ +abc *0000x^1").unwrap();
        assert_eq!(ops, vec![Op::Add(vec![0x0a, 0xbc]), Op::Perm(vec![0]), Op::Xor(vec![0x01])]);
    }

    #[test]
    fn parse_errors_match_python() {
        assert_eq!(parse_key(b"hello ^ + *").unwrap_err(), "key file contains no operations");
        assert_eq!(parse_key(b"").unwrap_err(), "key file contains no operations");
        assert_eq!(parse_key(b"*abcdef").unwrap_err(), "'*' code has odd byte length 3: abcdef");
        assert_eq!(parse_key(b"*00000000").unwrap_err(), "'*' code is not a permutation of 0..1: [0, 0]");
        assert_eq!(parse_key(b"*0002").unwrap_err(), "'*' code is not a permutation of 0..0: [2]");
    }

    #[test]
    fn non_utf8_key_bytes_are_ignored() {
        let ops = parse_key(b"\xff\xfe^0a\xc3\xa9+ff\x80").unwrap();
        assert_eq!(ops, vec![Op::Xor(vec![0x0a]), Op::Add(vec![0xff])]);
    }

    #[test]
    fn buffer_sizes() {
        assert_eq!(buffer_size(&parse_key(b"^01").unwrap()).unwrap(), BUFFER_SIZE);
        // lcm(3, 7) = 21 -> largest multiple of 21 within 512 KiB.
        let ops = parse_key(b"*000200000001 *0006000500040003000200010000").unwrap();
        assert_eq!(buffer_size(&ops).unwrap(), BUFFER_SIZE / 21 * 21);
    }

    /// Golden vectors produced by `python codec.py`.
    #[test]
    fn golden_vectors_from_python() {
        let data = b"Hello, World! 0123456789";
        let key = b"hjhiyuy798797*0003000000010002hjhljhlh jhkjjhj+abcdef45hjhjkhkhkjk$#@!%^456577979ab4jkjljk ipipp+5678adgjggjg";
        let enc = run(Mode::Encode, key, data);
        assert_eq!(enc, hex(GOLDEN_KEYTXT));
        assert_eq!(run(Mode::Decode, key, &enc), data);
        let key2 = b"^ff00+01*00010000^abcdef*000200000001";
        let enc2 = run(Mode::Encode, key2, data);
        assert_eq!(enc2, hex(GOLDEN_KEY2));
        assert_eq!(run(Mode::Decode, key2, &enc2), data);
    }

    const GOLDEN_KEYTXT: &str = "a8e8d07c1035b478250173b2ef032538f2f8bd9440ea3476";
    const GOLDEN_KEY2: &str = "82cd757e3fe09cf32d7b3aa8dd8a12217bf9d79d012761f7";

    #[test]
    fn round_trip_across_chunk_boundaries() {
        // lcm 21: chunks of 524286 bytes, so ^/+ offsets must carry over chunks.
        let key = b"^0102030405 +ff7f01 *000200000001 *0006000500040003000200010000 ^9a";
        let ops = parse_key(key).unwrap();
        for len in [0, 1, 20, 21, 22, 524_286, 524_287, BUFFER_SIZE * 2 + 13] {
            let data = bytes(len, len as u64);
            let enc = run(Mode::Encode, key, &data);
            // Chunked result equals processing the whole buffer at once.
            let mut whole = data.clone();
            apply_ops(Mode::Encode, &ops, &mut whole, 0);
            assert_eq!(enc, whole, "len {len}");
            assert_eq!(run(Mode::Decode, key, &enc), data, "len {len}");
        }
    }

    #[test]
    fn partial_permutation_block_unchanged() {
        let enc = run(Mode::Encode, b"*000200000001", b"abcdefgh");
        assert_eq!(enc, b"cabfdegh");
    }

    #[test]
    fn unmask_round_trip() {
        let mut k = b"^0a+0b".to_vec();
        let pw = "пароль".as_bytes();
        unmask_key(&mut k, pw);
        assert_ne!(k, b"^0a+0b");
        unmask_key(&mut k, pw);
        assert_eq!(k, b"^0a+0b");
    }
}

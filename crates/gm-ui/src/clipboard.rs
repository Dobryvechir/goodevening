//! System clipboard with an internal fallback and OSC 52 for remote sessions.

use std::io::Write;
use std::sync::Mutex;

static INTERNAL: Mutex<String> = Mutex::new(String::new());

pub fn set(text: &str) {
    if let Ok(mut i) = INTERNAL.lock() {
        *i = text.to_string();
    }
    let ok = arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())).is_ok();
    if !ok {
        // OSC 52: lets terminals over SSH put text into the local clipboard.
        let seq = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
        let mut out = std::io::stdout();
        let _ = out.write_all(seq.as_bytes());
        let _ = out.flush();
    }
}

pub fn get() -> String {
    if let Ok(t) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
        return t;
    }
    INTERNAL.lock().map(|s| s.clone()).unwrap_or_default()
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn b64() {
        assert_eq!(super::base64(b"hello"), "aGVsbG8=");
        assert_eq!(super::base64(b"ab"), "YWI=");
    }
}

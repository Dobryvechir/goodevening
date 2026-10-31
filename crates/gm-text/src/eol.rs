//! Line endings (replaces `eol.cpp`).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Eol {
    #[default]
    Lf,
    CrLf,
    Cr,
}

impl Eol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
            Eol::Cr => "\r",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::CrLf => "CRLF",
            Eol::Cr => "CR",
        }
    }

    pub fn native() -> Eol {
        if cfg!(windows) { Eol::CrLf } else { Eol::Lf }
    }

    /// The first line ending found in `text`, or `None` if it has none.
    pub fn first(text: &str) -> Option<Eol> {
        let b = text.as_bytes();
        let i = b.iter().position(|&c| c == b'\r' || c == b'\n')?;
        Some(match (b[i], b.get(i + 1)) {
            (b'\r', Some(b'\n')) => Eol::CrLf,
            (b'\r', _) => Eol::Cr,
            _ => Eol::Lf,
        })
    }

    /// The line ending to use for new lines in `text`: the first one found,
    /// or the native one (CR+LF on Windows, LF elsewhere) if there is none.
    pub fn detect_first(text: &str) -> Eol {
        Eol::first(text).unwrap_or_else(Eol::native)
    }

    /// Converts every line break in `text` (CR+LF, LF or CR) to `self`.
    pub fn convert(&self, text: &str) -> String {
        let s = text.replace("\r\n", "\n").replace('\r', "\n");
        if *self == Eol::Lf { s } else { s.replace('\n', self.as_str()) }
    }

    /// The most frequent line ending in `text` (native if there are none).
    pub fn detect(text: &str) -> Eol {
        let b = text.as_bytes();
        let (mut lf, mut crlf, mut cr) = (0usize, 0usize, 0usize);
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\r' if b.get(i + 1) == Some(&b'\n') => {
                    crlf += 1;
                    i += 1;
                }
                b'\r' => cr += 1,
                b'\n' => lf += 1,
                _ => {}
            }
            i += 1;
        }
        if lf == 0 && crlf == 0 && cr == 0 {
            Eol::native()
        } else if crlf >= lf && crlf >= cr {
            Eol::CrLf
        } else if lf >= cr {
            Eol::Lf
        } else {
            Eol::Cr
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detect() {
        assert_eq!(Eol::detect("a\r\nb\r\nc\n"), Eol::CrLf);
        assert_eq!(Eol::detect("a\nb\n"), Eol::Lf);
        assert_eq!(Eol::detect("a\rb\r"), Eol::Cr);
    }

    #[test]
    fn first_and_convert() {
        assert_eq!(Eol::first("a\nb\r\nc\r\n"), Some(Eol::Lf));
        assert_eq!(Eol::first("a\r\nb\nc\n"), Some(Eol::CrLf));
        assert_eq!(Eol::first("a\rb\n"), Some(Eol::Cr));
        assert_eq!(Eol::first("abc"), None);
        assert_eq!(Eol::detect_first(""), Eol::native());
        assert_eq!(Eol::detect_first("abc"), Eol::native());
        assert_eq!(Eol::CrLf.convert("a\nb\r\nc\rd"), "a\r\nb\r\nc\r\nd");
        assert_eq!(Eol::Lf.convert("a\nb\r\nc\rd"), "a\nb\nc\nd");
    }
}

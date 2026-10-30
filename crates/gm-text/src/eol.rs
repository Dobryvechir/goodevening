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
}

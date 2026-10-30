//! Code pages (replaces `codepage.cpp`, `encoding.cpp` and `uchardet`).

use encoding_rs::Encoding;
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codepage {
    Utf8,
    Utf16Le,
    Utf16Be,
    /// DOS/OEM code page (437, 850, 852, 866…).
    Oem(u16),
    /// Any encoding known to `encoding_rs` (Windows-125x, KOI8, ISO-8859-x…).
    Web(&'static Encoding),
}

/// OEM code pages offered in the code page menu.
pub const OEM_PAGES: &[u16] = &[437, 720, 737, 775, 850, 852, 855, 857, 858, 862, 866, 874];

impl Codepage {
    pub fn name(&self) -> String {
        match self {
            Codepage::Utf8 => "UTF-8".into(),
            Codepage::Utf16Le => "UTF-16LE".into(),
            Codepage::Utf16Be => "UTF-16BE".into(),
            Codepage::Oem(n) => format!("CP{n}"),
            Codepage::Web(e) => e.name().to_string(),
        }
    }

    /// Numeric id compatible with Windows code page numbers, used for persistence.
    pub fn id(&self) -> u32 {
        match self {
            Codepage::Utf8 => 65001,
            Codepage::Utf16Le => 1200,
            Codepage::Utf16Be => 1201,
            Codepage::Oem(n) => u32::from(*n),
            Codepage::Web(e) => windows_number(e).unwrap_or(65001),
        }
    }

    pub fn from_id(id: u32) -> Option<Codepage> {
        Some(match id {
            65001 => Codepage::Utf8,
            1200 => Codepage::Utf16Le,
            1201 => Codepage::Utf16Be,
            n if OEM_PAGES.contains(&(n as u16)) => Codepage::Oem(n as u16),
            1250..=1258 => Codepage::Web(Encoding::for_label(format!("windows-{id}").as_bytes())?),
            20866 => Codepage::Web(encoding_rs::KOI8_R),
            21866 => Codepage::Web(encoding_rs::KOI8_U),
            28591 => Codepage::Web(encoding_rs::WINDOWS_1252),
            n @ 28592..=28605 => Codepage::Web(Encoding::for_label(format!("iso-8859-{}", n - 28590).as_bytes())?),
            _ => return None,
        })
    }

    /// The system "ANSI" code page. On Unix the locale is almost always UTF-8.
    pub fn ansi() -> Codepage {
        if cfg!(windows) { Codepage::Web(encoding_rs::WINDOWS_1252) } else { Codepage::Utf8 }
    }

    pub fn oem() -> Codepage {
        Codepage::Oem(437)
    }

    /// Code pages listed in the Shift+F8 menu.
    pub fn menu_list() -> Vec<Codepage> {
        let mut v = vec![Codepage::Utf8, Codepage::Utf16Le, Codepage::Utf16Be];
        v.extend(OEM_PAGES.iter().map(|&n| Codepage::Oem(n)));
        for id in [1250, 1251, 1252, 1253, 1254, 1255, 1256, 1257, 1258, 20866, 21866] {
            if let Some(c) = Codepage::from_id(id) {
                v.push(c);
            }
        }
        v
    }

    /// Bytes per code unit (1 for single-byte and UTF-8).
    pub fn unit(&self) -> usize {
        match self {
            Codepage::Utf16Le | Codepage::Utf16Be => 2,
            _ => 1,
        }
    }

    pub fn decode<'a>(&self, bytes: &'a [u8]) -> Cow<'a, str> {
        match self {
            Codepage::Utf8 => String::from_utf8_lossy(bytes),
            Codepage::Utf16Le => Cow::Owned(encoding_rs::UTF_16LE.decode_without_bom_handling(bytes).0.into_owned()),
            Codepage::Utf16Be => Cow::Owned(encoding_rs::UTF_16BE.decode_without_bom_handling(bytes).0.into_owned()),
            Codepage::Oem(n) => match oem_table(*n) {
                Some(t) => Cow::Owned(t.decode_string_lossy(bytes)),
                None => String::from_utf8_lossy(bytes),
            },
            Codepage::Web(e) => e.decode_without_bom_handling(bytes).0,
        }
    }

    /// Encodes text; unmappable characters become `?`.
    pub fn encode(&self, s: &str) -> Vec<u8> {
        match self {
            Codepage::Utf8 => s.as_bytes().to_vec(),
            Codepage::Utf16Le => s.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            Codepage::Utf16Be => s.encode_utf16().flat_map(u16::to_be_bytes).collect(),
            Codepage::Oem(n) => match oem_cp::code_table::ENCODING_TABLE_CP_MAP.get(n) {
                Some(t) => oem_cp::encode_string_lossy(s, t),
                None => s.as_bytes().to_vec(),
            },
            Codepage::Web(e) => e.encode(s).0.into_owned(),
        }
    }

    pub fn bom(&self) -> &'static [u8] {
        match self {
            Codepage::Utf8 => b"\xEF\xBB\xBF",
            Codepage::Utf16Le => b"\xFF\xFE",
            Codepage::Utf16Be => b"\xFE\xFF",
            _ => b"",
        }
    }
}

fn windows_number(e: &'static Encoding) -> Option<u32> {
    let name = e.name();
    if let Some(n) = name.strip_prefix("windows-") {
        return n.parse().ok();
    }
    match name {
        "KOI8-R" => Some(20866),
        "KOI8-U" => Some(21866),
        _ => name.strip_prefix("ISO-8859-").and_then(|n| n.parse::<u32>().ok()).map(|n| 28590 + n),
    }
}

fn oem_table(n: u16) -> Option<&'static oem_cp::code_table_type::TableType> {
    oem_cp::code_table::DECODING_TABLE_CP_MAP.get(&n)
}

/// Detects BOM; returns the code page and BOM length.
pub fn detect_bom(bytes: &[u8]) -> Option<(Codepage, usize)> {
    if bytes.starts_with(b"\xEF\xBB\xBF") {
        Some((Codepage::Utf8, 3))
    } else if bytes.starts_with(b"\xFF\xFE") {
        Some((Codepage::Utf16Le, 2))
    } else if bytes.starts_with(b"\xFE\xFF") {
        Some((Codepage::Utf16Be, 2))
    } else {
        None
    }
}

/// Guesses the code page of a text sample (BOM, UTF-16 heuristics, valid
/// UTF-8, then `chardetng`).
pub fn detect(sample: &[u8]) -> Codepage {
    if let Some((cp, _)) = detect_bom(sample) {
        return cp;
    }
    if sample.len() >= 4 {
        let even_zeros = sample.iter().step_by(2).filter(|&&b| b == 0).count();
        let odd_zeros = sample.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
        let half = sample.len() / 2;
        if odd_zeros > half / 2 && even_zeros <= half / 16 {
            return Codepage::Utf16Le;
        }
        if even_zeros > half / 2 && odd_zeros <= half / 16 {
            return Codepage::Utf16Be;
        }
    }
    // A truncated multi-byte sequence at the end of the sample is not an error.
    match std::str::from_utf8(sample) {
        Ok(_) => return Codepage::Utf8,
        Err(e) if e.error_len().is_none() => return Codepage::Utf8,
        Err(_) => {}
    }
    let mut det = chardetng::EncodingDetector::new();
    det.feed(sample, true);
    let enc = det.guess(None, true);
    if enc == encoding_rs::UTF_8 { Codepage::Utf8 } else { Codepage::Web(enc) }
}

/// True if a sample looks like binary data (NUL bytes outside UTF-16).
pub fn looks_binary(sample: &[u8]) -> bool {
    !matches!(detect(sample), Codepage::Utf16Le | Codepage::Utf16Be) && sample.contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for cp in [Codepage::Utf8, Codepage::Utf16Le, Codepage::Oem(866), Codepage::from_id(1251).unwrap()] {
            let s = "Привет, world";
            assert_eq!(cp.decode(&cp.encode(s)), s, "{}", cp.name());
        }
        assert_eq!(Codepage::Oem(437).decode(&[0xC9, 0xCD, 0xBB]), "╔═╗");
    }

    #[test]
    fn detection() {
        assert_eq!(detect("hello мир".as_bytes()), Codepage::Utf8);
        assert_eq!(detect(&Codepage::Utf16Le.encode("hello world")), Codepage::Utf16Le);
        let cp1251 = Codepage::from_id(1251).unwrap().encode("Съешь же ещё этих мягких французских булок");
        assert_eq!(detect(&cp1251), Codepage::from_id(1251).unwrap());
    }
}

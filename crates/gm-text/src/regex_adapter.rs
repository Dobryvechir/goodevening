//! Adapter from Far's RegExp syntax (`/pattern/flags`) to the `regex` crate.

use regex::{Regex, RegexBuilder};

/// Search options shared by viewer, editor and Find File.
#[derive(Clone, Debug, Default)]
pub struct SearchSpec {
    pub text: String,
    pub case_sensitive: bool,
    pub whole_words: bool,
    pub regex: bool,
}

impl SearchSpec {
    /// Compiles the spec into a regex (plain text is escaped).
    pub fn compile(&self) -> Result<Regex, regex::Error> {
        let (body, mut ci) = if self.regex { parse_far_regex(&self.text) } else { (regex::escape(&self.text), false) };
        if !self.case_sensitive {
            ci = true;
        }
        let pat = if self.whole_words { format!(r"\b(?:{body})\b") } else { body };
        RegexBuilder::new(&pat).case_insensitive(ci).multi_line(true).build()
    }
}

/// Accepts `/pattern/flags` or a bare pattern; returns (pattern, case-insensitive).
pub fn parse_far_regex(s: &str) -> (String, bool) {
    if let Some(rest) = s.strip_prefix('/')
        && let Some(end) = rest.rfind('/')
    {
        let flags = &rest[end + 1..];
        if flags.chars().all(|c| "imsx".contains(c)) {
            let mut inline = String::new();
            for f in ['m', 's', 'x'] {
                if flags.contains(f) {
                    inline.push(f);
                }
            }
            let pat = &rest[..end];
            let pat = if inline.is_empty() { pat.to_string() } else { format!("(?{inline}){pat}") };
            return (pat, flags.contains('i'));
        }
    }
    (s.to_string(), false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn far_syntax() {
        assert_eq!(parse_far_regex("/a+b/i"), ("a+b".to_string(), true));
        let spec = SearchSpec { text: "foo".into(), whole_words: true, ..Default::default() };
        let r = spec.compile().unwrap();
        assert!(r.is_match("a FOO b") && !r.is_match("food"));
    }
}

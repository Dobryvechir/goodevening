//! Syntax colors for the viewer's "Colors" mode (F5).
//!
//! Source code gets the usual code-editor colors (close to VS Code's
//! "Dark+" theme) and a simple structure check: unbalanced brackets,
//! unterminated strings/comments and mismatched tags get a red background.
//!
//! Markdown is shown with its inline HTML tags hidden; the text inside a
//! tag takes the color from the tag's `style="color: …"` (or the `color`
//! attribute of `<font>`), and mismatched tags are shown on a red background.

use gm_term::Color;
use gm_text::width::char_width;
use std::path::Path;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

pub const BACKGROUND: Color = rgb(0, 0, 0);
/// Background of problem text, and the text color on it.
pub const MARK: Color = rgb(200, 0, 0);
pub const MARK_TEXT: Color = rgb(255, 255, 255);
pub const TEXT: Color = rgb(0xD4, 0xD4, 0xD4);
const KEYWORD: Color = rgb(0x56, 0x9C, 0xD6);
const CONTROL: Color = rgb(0xC5, 0x86, 0xC0);
const STRING: Color = rgb(0xCE, 0x91, 0x78);
const ESCAPE: Color = rgb(0xD7, 0xBA, 0x7D);
const NUMBER: Color = rgb(0xB5, 0xCE, 0xA8);
const COMMENT: Color = rgb(0x6A, 0x99, 0x55);
const FUNCTION: Color = rgb(0xDC, 0xDC, 0xAA);
const TYPE: Color = rgb(0x4E, 0xC9, 0xB0);
const VARIABLE: Color = rgb(0x9C, 0xDC, 0xFE);
const CONSTANT: Color = rgb(0x4F, 0xC1, 0xFF);
const REGEX: Color = rgb(0xD1, 0x69, 0x69);
const PUNCT: Color = rgb(0x80, 0x80, 0x80);
const BRACKETS: [Color; 3] = [rgb(0xFF, 0xD7, 0x00), rgb(0xDA, 0x70, 0xD6), rgb(0x17, 0x9F, 0xFF)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Html,
    Xml,
    Php,
    JavaScript,
    TypeScript,
    CSharp,
    Java,
    Go,
    Dart,
    Rust,
    Python,
    Json,
    Markdown,
}

/// The language of a file by its extension, if "Colors" supports it.
pub fn lang_for(path: &Path) -> Option<Lang> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "html" | "htm" => Lang::Html,
        "xml" => Lang::Xml,
        "php" => Lang::Php,
        "js" => Lang::JavaScript,
        "ts" | "tsx" => Lang::TypeScript,
        "cs" => Lang::CSharp,
        "java" => Lang::Java,
        "go" => Lang::Go,
        "dart" => Lang::Dart,
        "rs" => Lang::Rust,
        "py" | "py3" => Lang::Python,
        "json" => Lang::Json,
        "md" => Lang::Markdown,
        _ => return None,
    })
}

/// One screen cell of colored text. `mark` cells are part of a problem
/// (shown on a red background).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub mark: bool,
    /// Tab expansion: 1 = first cell of a tab, 2 = a following cell.
    pub tab: u8,
}

pub struct Colored {
    pub lines: Vec<Vec<Cell>>,
    /// Number of problems found (red marks).
    pub problems: usize,
}

/// Colors `text`; tabs are expanded to `tab` columns.
pub fn colorize(text: &str, lang: Lang, tab: usize) -> Colored {
    let mut h = Hl::new(text);
    let end = h.s.len();
    match lang {
        Lang::Html => lex_markup(&mut h, true, false),
        Lang::Php => lex_markup(&mut h, true, true),
        Lang::Xml => lex_markup(&mut h, false, false),
        Lang::Markdown => lex_markdown(&mut h),
        Lang::JavaScript => lex_code(&mut h, 0, end, &JS),
        Lang::TypeScript => lex_code(&mut h, 0, end, &TS),
        Lang::CSharp => lex_code(&mut h, 0, end, &CSHARP),
        Lang::Java => lex_code(&mut h, 0, end, &JAVA),
        Lang::Go => lex_code(&mut h, 0, end, &GO),
        Lang::Dart => lex_code(&mut h, 0, end, &DART),
        Lang::Rust => lex_code(&mut h, 0, end, &RUST),
        Lang::Python => lex_code(&mut h, 0, end, &PYTHON),
        Lang::Json => lex_code(&mut h, 0, end, &JSON),
    }
    h.finish(tab.max(1))
}

// ----- highlighter state -----

struct Hl {
    s: Vec<char>,
    fg: Vec<Color>,
    hidden: Vec<bool>,
    /// Problem ranges `a..b`, shown with a red background.
    marks: Vec<(usize, usize)>,
}

impl Hl {
    fn new(text: &str) -> Hl {
        let s: Vec<char> = text.chars().collect();
        let n = s.len();
        Hl { s, fg: vec![TEXT; n], hidden: vec![false; n], marks: Vec::new() }
    }

    fn at(&self, i: usize) -> char {
        self.s.get(i).copied().unwrap_or('\0')
    }

    fn paint(&mut self, a: usize, b: usize, c: Color) {
        let b = b.min(self.s.len());
        if a < b {
            self.fg[a..b].fill(c);
        }
    }

    fn hide(&mut self, a: usize, b: usize) {
        let b = b.min(self.s.len());
        if a < b {
            self.hidden[a..b].fill(true);
        }
    }

    /// Marks `a..b` (at least one character) as a problem: red background.
    fn mark(&mut self, a: usize, b: usize) {
        let n = self.s.len();
        if n == 0 {
            return;
        }
        let a = a.min(n - 1);
        self.marks.push((a, b.clamp(a + 1, n)));
    }

    fn starts(&self, i: usize, pat: &str) -> bool {
        pat.chars().enumerate().all(|(k, c)| self.s.get(i + k) == Some(&c))
    }

    fn starts_ci(&self, i: usize, pat: &str) -> bool {
        pat.chars().enumerate().all(|(k, c)| self.s.get(i + k).is_some_and(|x| x.eq_ignore_ascii_case(&c)))
    }

    /// First position in `from..end` where `pat` starts and fits before `end`.
    fn find(&self, from: usize, end: usize, pat: &str) -> Option<usize> {
        let n = pat.chars().count();
        (from..end).find(|&i| i + n <= end && self.starts(i, pat))
    }

    fn find_ci(&self, from: usize, end: usize, pat: &str) -> Option<usize> {
        let n = pat.chars().count();
        (from..end).find(|&i| i + n <= end && self.starts_ci(i, pat))
    }

    fn line_end(&self, from: usize, end: usize) -> usize {
        (from..end).find(|&i| self.s[i] == '\n').unwrap_or(end)
    }

    fn next_non_space(&self, from: usize, end: usize) -> char {
        (from..end).map(|i| self.s[i]).find(|c| !c.is_whitespace()).unwrap_or('\0')
    }

    fn finish(self, tab: usize) -> Colored {
        let mut marks = self.marks;
        marks.sort_unstable();
        marks.dedup_by_key(|m| m.0);
        let problems = marks.len();
        let mut bad = vec![false; self.s.len()];
        for &(a, b) in &marks {
            bad[a..b].fill(true);
        }
        let mut lines: Vec<Vec<Cell>> = vec![Vec::new()];
        let mut width = 0usize;
        for (i, &c) in self.s.iter().enumerate() {
            let line = lines.last_mut().expect("at least one line");
            let mark = bad[i];
            match c {
                '\n' => {
                    lines.push(Vec::new());
                    width = 0;
                }
                '\r' => {}
                // Hidden text (Markdown tags) is shown when it is the problem.
                _ if self.hidden[i] && !mark => {}
                '\t' => {
                    let n = tab - width % tab;
                    for k in 0..n {
                        line.push(Cell { ch: ' ', fg: self.fg[i], mark, tab: if k == 0 { 1 } else { 2 } });
                    }
                    width += n;
                }
                _ => {
                    line.push(Cell { ch: c, fg: self.fg[i], mark, tab: 0 });
                    width += char_width(c).max(1);
                }
            }
        }
        Colored { lines, problems }
    }
}

// ----- programming languages -----

#[derive(Clone, Copy, PartialEq, Eq)]
enum SingleQuote {
    /// `'` is not a quote (JSON).
    None,
    /// `'…'` is a string (JS, Python, PHP, Dart, CSS).
    Str,
    /// `'x'` is a character literal (C#, Java, Go).
    Char,
    /// `'x'` is a character literal, `'a` a lifetime (Rust).
    Rust,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Backtick {
    None,
    /// JS/TS template literal (escapes, multi-line).
    Template,
    /// Go raw string (no escapes, multi-line).
    Raw,
}

struct Spec {
    keywords: &'static [&'static str],
    control: &'static [&'static str],
    types: &'static [&'static str],
    line_comments: &'static [&'static str],
    block_comments: bool,
    single: SingleQuote,
    backtick: Backtick,
    /// Triple-quoted multi-line strings (Python, Dart, Java, C#).
    triple: &'static [&'static str],
    /// `#…` at line start is a preprocessor directive (C#).
    preproc: bool,
    /// `$` is part of identifiers (JS, TS, Dart).
    dollar_idents: bool,
    /// `$name` is a variable (PHP).
    php_vars: bool,
    /// `/…/` regex literals (JS, TS).
    regex: bool,
    /// `@name` annotations/decorators.
    decorators: bool,
    /// Object keys get the variable color (JSON).
    json_keys: bool,
    /// String prefixes like `r"…"`, `f'…'` (Python) / `r'…'` (Dart).
    prefixes: &'static str,
    /// `"…"` strings may span lines (Rust, PHP).
    multiline_strings: bool,
    rust: bool,
    csharp: bool,
}

const BASE: Spec = Spec {
    keywords: &[],
    control: &[],
    types: &[],
    line_comments: &["//"],
    block_comments: true,
    single: SingleQuote::Str,
    backtick: Backtick::None,
    triple: &[],
    preproc: false,
    dollar_idents: false,
    php_vars: false,
    regex: false,
    decorators: false,
    json_keys: false,
    prefixes: "",
    multiline_strings: false,
    rust: false,
    csharp: false,
};

#[rustfmt::skip]
const JS_KEYWORDS: &[&str] = &[
    "var", "let", "const", "function", "class", "extends", "new", "delete", "typeof", "instanceof", "in", "of", "void", "this",
    "super", "static", "get", "set", "async", "true", "false", "null", "undefined", "NaN", "Infinity", "debugger", "with",
];
#[rustfmt::skip]
const JS_CONTROL: &[&str] = &[
    "if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "return", "throw", "try", "catch", "finally", "import",
    "export", "from", "as", "yield", "await",
];

const JS: Spec = Spec { keywords: JS_KEYWORDS, control: JS_CONTROL, backtick: Backtick::Template, dollar_idents: true, regex: true, ..BASE };

#[rustfmt::skip]
const TS: Spec = Spec {
    keywords: &[
        "var", "let", "const", "function", "class", "extends", "new", "delete", "typeof", "instanceof", "in", "of", "void",
        "this", "super", "static", "get", "set", "async", "true", "false", "null", "undefined", "NaN", "Infinity", "interface",
        "type", "enum", "implements", "namespace", "module", "declare", "readonly", "private", "protected", "public", "abstract",
        "keyof", "infer", "is", "unique", "satisfies", "override", "debugger",
    ],
    control: JS_CONTROL,
    types: &["string", "number", "boolean", "any", "never", "unknown", "object", "symbol", "bigint"],
    backtick: Backtick::Template,
    dollar_idents: true,
    regex: true,
    decorators: true,
    ..BASE
};

#[rustfmt::skip]
const CSHARP: Spec = Spec {
    keywords: &[
        "abstract", "as", "base", "checked", "class", "const", "delegate", "enum", "event", "explicit", "extern", "false",
        "fixed", "implicit", "in", "interface", "internal", "is", "lock", "namespace", "new", "null", "operator", "out",
        "override", "params", "private", "protected", "public", "readonly", "ref", "sealed", "sizeof", "stackalloc", "static",
        "struct", "this", "true", "typeof", "unchecked", "unsafe", "using", "virtual", "volatile", "var", "async", "record",
        "init", "get", "set", "value", "nameof", "where", "partial", "required", "global",
    ],
    control: &[
        "if", "else", "switch", "case", "default", "break", "continue", "return", "throw", "try", "catch", "finally", "for", "foreach", "while",
        "do", "goto", "yield", "await", "when",
    ],
    types: &[
        "bool", "byte", "sbyte", "char", "decimal", "double", "float", "int", "uint", "long", "ulong", "short", "ushort", "object", "string", "void",
        "dynamic", "nint", "nuint",
    ],
    single: SingleQuote::Char,
    triple: &["\"\"\""],
    preproc: true,
    csharp: true,
    ..BASE
};

#[rustfmt::skip]
const JAVA: Spec = Spec {
    keywords: &[
        "abstract", "class", "const", "enum", "extends", "final", "implements", "import", "instanceof", "interface", "native",
        "new", "package", "private", "protected", "public", "static", "strictfp", "super", "synchronized", "this", "throws",
        "transient", "volatile", "var", "record", "sealed", "permits", "true", "false", "null",
    ],
    control: &[
        "if", "else", "switch", "case", "default", "break", "continue", "return", "throw", "try", "catch", "finally", "for", "while", "do", "assert",
        "yield",
    ],
    types: &["boolean", "byte", "char", "double", "float", "int", "long", "short", "void"],
    single: SingleQuote::Char,
    triple: &["\"\"\""],
    decorators: true,
    ..BASE
};

#[rustfmt::skip]
const GO: Spec = Spec {
    keywords: &[
        "package", "import", "func", "var", "const", "type", "struct", "interface", "map", "chan", "go", "defer", "true",
        "false", "nil", "iota",
    ],
    control: &["if", "else", "for", "range", "switch", "case", "default", "break", "continue", "return", "goto", "fallthrough", "select"],
    types: &[
        "int", "int8", "int16", "int32", "int64", "uint", "uint8", "uint16", "uint32", "uint64", "uintptr", "float32", "float64",
        "complex64", "complex128", "string", "bool", "byte", "rune", "error", "any",
    ],
    single: SingleQuote::Char,
    backtick: Backtick::Raw,
    ..BASE
};

#[rustfmt::skip]
const DART: Spec = Spec {
    keywords: &[
        "abstract", "as", "async", "class", "const", "covariant", "deferred", "dynamic", "enum", "export", "extends",
        "extension", "external", "factory", "final", "get", "implements", "import", "interface", "is", "late", "library",
        "mixin", "new", "null", "on", "operator", "part", "required", "sealed", "set", "static", "super", "this", "true",
        "false", "typedef", "var", "with", "base",
    ],
    control: &[
        "if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "return", "throw", "try", "catch", "finally",
        "rethrow", "yield", "assert", "in", "await",
    ],
    types: &["void", "int", "double", "num", "bool", "String", "List", "Map", "Set", "Future", "Stream", "Object", "Never"],
    triple: &["\"\"\"", "'''"],
    dollar_idents: true,
    decorators: true,
    prefixes: "r",
    ..BASE
};

#[rustfmt::skip]
const RUST: Spec = Spec {
    keywords: &[
        "as", "async", "const", "crate", "dyn", "enum", "extern", "false", "fn", "impl", "let", "mod", "move", "mut", "pub", "ref", "self", "Self",
        "static", "struct", "super", "trait", "true", "type", "union", "unsafe", "use", "where",
    ],
    control: &["if", "else", "for", "while", "loop", "match", "break", "continue", "return", "in", "yield", "await"],
    types: &["i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32", "f64", "bool", "char", "str"],
    single: SingleQuote::Rust,
    multiline_strings: true,
    rust: true,
    ..BASE
};

#[rustfmt::skip]
const PYTHON: Spec = Spec {
    keywords: &[
        "and", "as", "async", "class", "def", "del", "False", "global", "in", "is", "lambda", "None", "nonlocal", "not", "or", "True", "self", "cls",
    ],
    control: &[
        "if", "elif", "else", "for", "while", "break", "continue", "return", "try", "except", "finally", "raise", "with", "yield", "pass", "await",
        "import", "from", "assert", "match", "case",
    ],
    types: &["int", "float", "str", "bool", "list", "dict", "set", "tuple", "bytes", "object", "type", "complex", "frozenset"],
    line_comments: &["#"],
    block_comments: false,
    triple: &["\"\"\"", "'''"],
    decorators: true,
    prefixes: "rRbBuUfF",
    ..BASE
};

#[rustfmt::skip]
const PHP: Spec = Spec {
    keywords: &[
        "abstract", "and", "array", "as", "class", "clone", "const", "declare", "echo", "enum", "extends", "final", "fn",
        "function", "global", "implements", "include", "include_once", "instanceof", "insteadof", "interface", "isset", "list",
        "namespace", "new", "or", "print", "private", "protected", "public", "readonly", "require", "require_once", "static",
        "trait", "unset", "use", "var", "xor", "true", "false", "null", "TRUE", "FALSE", "NULL", "self", "parent", "empty",
    ],
    control: &[
        "if", "else", "elseif", "for", "foreach", "while", "do", "switch", "case", "default", "break", "continue", "return",
        "throw", "try", "catch", "finally", "goto", "yield", "match", "endif", "endforeach", "endwhile", "endfor", "endswitch",
    ],
    types: &["int", "float", "string", "bool", "void", "mixed", "object", "iterable", "callable"],
    line_comments: &["//", "#"],
    php_vars: true,
    multiline_strings: true,
    ..BASE
};

const JSON: Spec = Spec { keywords: &["true", "false", "null"], single: SingleQuote::None, json_keys: true, ..BASE };

const CSS: Spec = Spec { line_comments: &[], ..BASE };

/// What came before, for telling a regex literal from a division.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prev {
    Start,
    Operator,
    Value,
    /// A keyword after which an expression starts (`return /x/`).
    ExprKeyword,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn closing_of(c: char) -> char {
    match c {
        '(' => ')',
        '[' => ']',
        _ => '}',
    }
}

/// Scans a string whose body starts at `from` (the opening delimiter is at
/// `open`). Returns the position after the closing delimiter, or after the
/// scanned text if the string is unterminated (marked as a problem).
fn scan_string(h: &mut Hl, open: usize, from: usize, end: usize, close: &str, escapes: bool, multiline: bool, doubled: bool) -> usize {
    let n = close.chars().count();
    let mut esc = Vec::new();
    let mut j = from;
    let result = loop {
        if j >= end || (!multiline && h.s[j] == '\n') {
            h.mark(open, from);
            break j.min(end);
        }
        if escapes && h.s[j] == '\\' {
            esc.push(j);
            j += 2;
            continue;
        }
        if h.starts(j, close) {
            if doubled && h.starts(j + n, close) {
                j += 2 * n;
                continue;
            }
            break j + n;
        }
        j += 1;
    };
    h.paint(open, result, STRING);
    for e in esc {
        h.paint(e, (e + 2).min(result), ESCAPE);
    }
    result
}

fn lex_code(h: &mut Hl, start: usize, end: usize, sp: &Spec) {
    let mut stack: Vec<(char, usize)> = Vec::new();
    let mut prev = Prev::Start;
    let mut line_start = true;
    let mut i = start;
    while i < end {
        let c = h.s[i];
        if c == '\n' {
            line_start = true;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let at_line_start = line_start;
        line_start = false;

        // Preprocessor directives (C#).
        if sp.preproc && c == '#' && at_line_start {
            let e = h.line_end(i, end);
            h.paint(i, e, KEYWORD);
            i = e;
            continue;
        }
        // Comments.
        if sp.line_comments.iter().any(|p| h.starts(i, p)) {
            let e = h.line_end(i, end);
            h.paint(i, e, COMMENT);
            i = e;
            continue;
        }
        if sp.block_comments && h.starts(i, "/*") {
            let e = match h.find(i + 2, end, "*/") {
                Some(p) => p + 2,
                None => {
                    h.mark(i, i + 2);
                    end
                }
            };
            h.paint(i, e, COMMENT);
            i = e;
            continue;
        }
        // Strings.
        if let Some(t) = sp.triple.iter().find(|t| h.starts(i, t)) {
            i = scan_string(h, i, i + 3, end, t, !sp.csharp, true, false);
            prev = Prev::Value;
            continue;
        }
        if sp.csharp && (c == '@' || c == '$') {
            let mut k = i;
            while k < end && matches!(h.s[k], '@' | '$') {
                k += 1;
            }
            if h.at(k) == '"' {
                let verbatim = h.s[i..k].contains(&'@');
                i = if h.starts(k, "\"\"\"") {
                    scan_string(h, i, k + 3, end, "\"\"\"", false, true, false)
                } else {
                    scan_string(h, i, k + 1, end, "\"", !verbatim, verbatim, verbatim)
                };
                prev = Prev::Value;
                continue;
            }
        }
        if c == '"' {
            let open = i;
            i = scan_string(h, i, i + 1, end, "\"", true, sp.multiline_strings, false);
            if sp.json_keys && h.next_non_space(i, end) == ':' {
                h.paint(open, i, VARIABLE);
            }
            prev = Prev::Value;
            continue;
        }
        if c == '\'' && sp.single != SingleQuote::None {
            if sp.single == SingleQuote::Rust && h.at(i + 1) != '\\' && h.at(i + 2) != '\'' && is_ident_start(h.at(i + 1)) {
                // Lifetime or loop label.
                let mut j = i + 1;
                while j < end && is_ident_char(h.s[j]) {
                    j += 1;
                }
                h.paint(i, j, KEYWORD);
                i = j;
                prev = Prev::Value;
                continue;
            }
            i = scan_string(h, i, i + 1, end, "'", true, false, false);
            prev = Prev::Value;
            continue;
        }
        if c == '`' && sp.backtick != Backtick::None {
            i = scan_string(h, i, i + 1, end, "`", sp.backtick == Backtick::Template, true, false);
            prev = Prev::Value;
            continue;
        }
        // Regex literals (JS/TS).
        if sp.regex && c == '/' && matches!(prev, Prev::Start | Prev::Operator | Prev::ExprKeyword) {
            let mut j = i + 1;
            let mut class = false;
            let mut closed = None;
            while j < end && h.s[j] != '\n' {
                match h.s[j] {
                    '\\' => j += 1,
                    '[' => class = true,
                    ']' => class = false,
                    '/' if !class => {
                        closed = Some(j);
                        break;
                    }
                    _ => {}
                }
                j += 1;
            }
            if let Some(j) = closed {
                let mut e = j + 1;
                while e < end && h.s[e].is_ascii_alphabetic() {
                    e += 1;
                }
                h.paint(i, e, REGEX);
                i = e;
                prev = Prev::Value;
                continue;
            }
        }
        // Numbers.
        if c.is_ascii_digit() || (c == '.' && h.at(i + 1).is_ascii_digit() && prev != Prev::Value) {
            let mut j = i + 1;
            while j < end {
                let d = h.s[j];
                let exp_sign = matches!(d, '+' | '-') && matches!(h.s[j - 1], 'e' | 'E') && !h.s[i..j].contains(&'x');
                if d.is_ascii_alphanumeric() || d == '_' || (d == '.' && h.at(j + 1) != '.' && !h.at(j + 1).is_alphabetic()) || exp_sign {
                    j += 1;
                } else {
                    break;
                }
            }
            h.paint(i, j, NUMBER);
            i = j;
            prev = Prev::Value;
            continue;
        }
        // Decorators / annotations.
        if sp.decorators && c == '@' && is_ident_start(h.at(i + 1)) {
            let mut j = i + 1;
            while j < end && (is_ident_char(h.s[j]) || (h.s[j] == '.' && is_ident_start(h.at(j + 1)))) {
                j += 1;
            }
            h.paint(i, j, FUNCTION);
            i = j;
            prev = Prev::Value;
            continue;
        }
        // PHP variables.
        if sp.php_vars && c == '$' && is_ident_start(h.at(i + 1)) {
            let mut j = i + 1;
            while j < end && is_ident_char(h.s[j]) {
                j += 1;
            }
            h.paint(i, j, VARIABLE);
            i = j;
            prev = Prev::Value;
            continue;
        }
        // Identifiers and keywords.
        if is_ident_start(c) || (sp.dollar_idents && c == '$') {
            let mut j = i + 1;
            while j < end && (is_ident_char(h.s[j]) || (sp.dollar_idents && h.s[j] == '$')) {
                j += 1;
            }
            let word: String = h.s[i..j].iter().collect();
            let next = h.at(j);
            // String prefixes: r"…", b"…", f'…', Rust raw strings r#"…"#.
            if sp.rust && (word == "r" || word == "br") && (next == '"' || next == '#') {
                let mut k = j;
                while h.at(k) == '#' {
                    k += 1;
                }
                if h.at(k) == '"' {
                    let close = format!("\"{}", "#".repeat(k - j));
                    i = scan_string(h, i, k + 1, end, &close, false, true, false);
                    prev = Prev::Value;
                    continue;
                }
            }
            if sp.rust && word == "b" && (next == '"' || next == '\'') {
                let q = if next == '"' { "\"" } else { "'" };
                i = scan_string(h, i, j + 1, end, q, true, next == '"', false);
                prev = Prev::Value;
                continue;
            }
            if !sp.prefixes.is_empty()
                && word.len() <= 3
                && word.chars().all(|ch| sp.prefixes.contains(ch))
                && (next == '"' || (next == '\'' && sp.single == SingleQuote::Str))
            {
                let raw = word.contains(['r', 'R']);
                h.paint(i, j, KEYWORD);
                let quote = next.to_string();
                let triple = quote.repeat(3);
                i = if h.starts(j, &triple) {
                    scan_string(h, j, j + 3, end, &triple, !raw, true, false)
                } else {
                    scan_string(h, j, j + 1, end, &quote, !raw, false, false)
                };
                prev = Prev::Value;
                continue;
            }
            let w = word.as_str();
            let color = if sp.control.contains(&w) {
                CONTROL
            } else if sp.keywords.contains(&w) {
                KEYWORD
            } else if sp.types.contains(&w) {
                TYPE
            } else if sp.rust && next == '!' && h.at(j + 1) != '=' {
                j += 1;
                FUNCTION
            } else if sp.json_keys {
                TEXT
            } else if h.next_non_space(j, end) == '(' {
                FUNCTION
            } else if w.chars().next().is_some_and(char::is_uppercase) && w.chars().any(char::is_lowercase) {
                TYPE
            } else if w.chars().count() > 1 && w.chars().all(|ch| ch.is_uppercase() || ch.is_ascii_digit() || ch == '_') {
                CONSTANT
            } else {
                VARIABLE
            };
            h.paint(i, j, color);
            prev = if matches!(
                w,
                "return" | "typeof" | "case" | "do" | "else" | "in" | "of" | "new" | "delete" | "void" | "throw" | "yield" | "await" | "instanceof"
            ) {
                Prev::ExprKeyword
            } else {
                Prev::Value
            };
            i = j;
            continue;
        }
        // Brackets.
        match c {
            '(' | '[' | '{' => {
                h.paint(i, i + 1, BRACKETS[stack.len() % 3]);
                stack.push((c, i));
                prev = Prev::Operator;
            }
            ')' | ']' | '}' => {
                if let Some(pos) = stack.iter().rposition(|&(o, _)| closing_of(o) == c) {
                    for &(_, p) in &stack[pos + 1..] {
                        h.mark(p, p + 1);
                    }
                    stack.truncate(pos);
                    h.paint(i, i + 1, BRACKETS[pos % 3]);
                } else {
                    h.mark(i, i + 1);
                }
                prev = Prev::Value;
            }
            _ => prev = Prev::Operator,
        }
        i += 1;
    }
    for &(_, p) in &stack {
        h.mark(p, p + 1);
    }
}

// ----- HTML / XML / PHP -----

#[rustfmt::skip]
const VOID_TAGS: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr", "keygen"];

/// HTML elements whose end tag may be omitted.
#[rustfmt::skip]
const OPTIONAL_END: &[&str] = &[
    "html", "head", "body", "p", "li", "dt", "dd", "option", "optgroup", "thead", "tbody", "tfoot", "tr", "td", "th", "colgroup", "caption", "rt",
    "rp",
];

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.')
}

/// Pops `name` from the open-tag stack; marks tags left unclosed in
/// between, or the closing tag `at` if nothing matches.
/// Stack entries are `(name, (tag start, tag end), extra)`.
fn close_tag<T>(h: &mut Hl, stack: &mut Vec<(String, (usize, usize), T)>, name: &str, at: (usize, usize), html: bool) {
    if let Some(pos) = stack.iter().rposition(|(n, ..)| n == name) {
        for (n, (a, b), _) in stack.drain(pos + 1..) {
            if !(html && OPTIONAL_END.contains(&n.as_str())) {
                h.mark(a, b);
            }
        }
        stack.pop();
    } else {
        h.mark(at.0, at.1);
    }
}

fn mark_unclosed<T>(h: &mut Hl, stack: &[(String, (usize, usize), T)], html: bool) {
    for (n, (a, b), _) in stack {
        if !(html && OPTIONAL_END.contains(&n.as_str())) {
            h.mark(*a, *b);
        }
    }
}

/// Paints a PHP block starting at `i` (`<?php`, `<?=` or `<?`); returns its end.
fn php_block(h: &mut Hl, i: usize, end: usize) -> usize {
    let head = if h.starts_ci(i, "<?php") {
        5
    } else if h.starts(i, "<?=") {
        3
    } else {
        2
    };
    h.paint(i, i + head, KEYWORD);
    let close = h.find(i + head, end, "?>");
    lex_code(h, i + head, close.unwrap_or(end), &PHP);
    match close {
        Some(c) => {
            h.paint(c, c + 2, KEYWORD);
            c + 2
        }
        None => end,
    }
}

fn lex_markup(h: &mut Hl, html: bool, php: bool) {
    let end = h.s.len();
    let mut stack: Vec<(String, (usize, usize), ())> = Vec::new();
    let mut i = 0;
    while i < end {
        if php && h.starts(i, "<?") {
            i = php_block(h, i, end);
            continue;
        }
        if h.starts(i, "<!--") {
            let e = match h.find(i + 4, end, "-->") {
                Some(p) => p + 3,
                None => {
                    h.mark(i, i + 4);
                    end
                }
            };
            h.paint(i, e, COMMENT);
            i = e;
            continue;
        }
        if h.starts(i, "<![CDATA[") {
            h.paint(i, i + 9, PUNCT);
            i = match h.find(i + 9, end, "]]>") {
                Some(p) => {
                    h.paint(p, p + 3, PUNCT);
                    p + 3
                }
                None => {
                    h.mark(i, i + 9);
                    end
                }
            };
            continue;
        }
        if h.starts(i, "<!") || h.starts(i, "<?") {
            let e = match h.find(i + 2, end, ">") {
                Some(p) => p + 1,
                None => {
                    h.mark(i, i + 2);
                    end
                }
            };
            h.paint(i, e, PUNCT);
            h.paint(i + 2, e.saturating_sub(1).max(i + 2), KEYWORD);
            i = e;
            continue;
        }
        if h.at(i) == '<' && h.at(i + 1) == '/' && h.at(i + 2).is_alphabetic() {
            let ts = i;
            let mut j = i + 2;
            while j < end && is_name_char(h.s[j]) {
                j += 1;
            }
            let name: String = h.s[i + 2..j].iter().collect();
            h.paint(i, i + 2, PUNCT);
            h.paint(i + 2, j, KEYWORD);
            while j < end && h.s[j].is_whitespace() {
                j += 1;
            }
            if h.at(j) == '>' {
                h.paint(j, j + 1, PUNCT);
                j += 1;
            } else {
                h.mark(ts, j);
            }
            let name = if html { name.to_ascii_lowercase() } else { name };
            close_tag(h, &mut stack, &name, (ts, j), html);
            i = j;
            continue;
        }
        if h.at(i) == '<' && h.at(i + 1).is_alphabetic() {
            let ts = i;
            let mut j = i + 1;
            while j < end && is_name_char(h.s[j]) {
                j += 1;
            }
            let name: String = h.s[i + 1..j].iter().collect();
            let name_end = j;
            h.paint(i, i + 1, PUNCT);
            h.paint(i + 1, j, KEYWORD);
            let mut self_close = false;
            let mut closed = false;
            while j < end {
                let c = h.s[j];
                if c == '>' {
                    h.paint(j, j + 1, PUNCT);
                    j += 1;
                    closed = true;
                    break;
                }
                if c == '/' && h.at(j + 1) == '>' {
                    h.paint(j, j + 2, PUNCT);
                    j += 2;
                    self_close = true;
                    closed = true;
                    break;
                }
                if c == '<' {
                    if php && h.starts(j, "<?") {
                        j = php_block(h, j, end);
                        continue;
                    }
                    break;
                }
                if c.is_whitespace() {
                    j += 1;
                    continue;
                }
                if c == '"' || c == '\'' {
                    match h.find(j + 1, end, if c == '"' { "\"" } else { "'" }) {
                        Some(k) => {
                            h.paint(j, k + 1, STRING);
                            j = k + 1;
                        }
                        None => {
                            h.mark(j, j + 1);
                            h.paint(j, end, STRING);
                            j = end;
                        }
                    }
                    continue;
                }
                if c == '=' {
                    h.paint(j, j + 1, PUNCT);
                    j += 1;
                    let k = j;
                    while j < end && !h.s[j].is_whitespace() && !matches!(h.s[j], '>' | '<' | '"' | '\'') {
                        j += 1;
                    }
                    h.paint(k, j, STRING);
                    continue;
                }
                let k = j;
                while j < end && !h.s[j].is_whitespace() && !matches!(h.s[j], '=' | '>' | '<' | '"' | '\'' | '/') {
                    j += 1;
                }
                if j == k {
                    j += 1;
                }
                h.paint(k, j, VARIABLE);
            }
            if !closed {
                h.mark(ts, name_end);
                i = j;
                continue;
            }
            let lname = if html { name.to_ascii_lowercase() } else { name };
            let void = html && VOID_TAGS.contains(&lname.as_str());
            if !self_close && !void {
                if html && (lname == "script" || lname == "style") {
                    let stop = h.find_ci(j, end, &format!("</{lname}")).unwrap_or(end);
                    lex_code(h, j, stop, if lname == "script" { &JS } else { &CSS });
                    stack.push((lname, (ts, j), ()));
                    i = stop;
                    continue;
                }
                stack.push((lname, (ts, j), ()));
            }
            i = j;
            continue;
        }
        i += 1;
    }
    mark_unclosed(h, &stack, html);
}

// ----- Markdown -----

struct MdTag {
    end: usize,
    name: String,
    closing: bool,
    self_close: bool,
    attrs: String,
}

/// Parses an HTML tag at `i` inside Markdown; None if it is not a tag.
fn md_tag(h: &Hl, i: usize, end: usize) -> Option<MdTag> {
    let closing = h.at(i + 1) == '/';
    let ns = i + 1 + usize::from(closing);
    if !h.at(ns).is_ascii_alphabetic() {
        return None;
    }
    let mut j = ns;
    while j < end && (h.s[j].is_ascii_alphanumeric() || h.s[j] == '-') {
        j += 1;
    }
    let name: String = h.s[ns..j].iter().collect::<String>().to_ascii_lowercase();
    if !(h.at(j).is_whitespace() || h.at(j) == '>' || h.at(j) == '/') {
        return None; // e.g. an autolink <https://…>
    }
    let attrs_start = j;
    let mut quote: Option<char> = None;
    while j < end {
        let c = h.s[j];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => {
                let self_close = j > attrs_start && h.s[j - 1] == '/';
                let attrs: String = h.s[attrs_start..j].iter().collect();
                return Some(MdTag { end: j + 1, name, closing, self_close, attrs });
            }
            None if c == '<' => return None,
            None if c == '\n' && h.at(j + 1) == '\n' => return None,
            None => {}
        }
        j += 1;
    }
    None
}

/// The value of attribute `name` in a tag's attribute text.
fn attr_value(attrs: &str, name: &str) -> Option<String> {
    let s: Vec<char> = attrs.chars().collect();
    let mut i = 0;
    while i < s.len() {
        while i < s.len() && (s[i].is_whitespace() || s[i] == '/') {
            i += 1;
        }
        let a = i;
        while i < s.len() && !s[i].is_whitespace() && s[i] != '=' && s[i] != '/' {
            i += 1;
        }
        let key: String = s[a..i].iter().collect();
        while i < s.len() && s[i].is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < s.len() && s[i] == '=' {
            i += 1;
            while i < s.len() && s[i].is_whitespace() {
                i += 1;
            }
            if i < s.len() && (s[i] == '"' || s[i] == '\'') {
                let q = s[i];
                i += 1;
                let b = i;
                while i < s.len() && s[i] != q {
                    i += 1;
                }
                value = s[b..i].iter().collect();
                i += 1;
            } else {
                let b = i;
                while i < s.len() && !s[i].is_whitespace() {
                    i += 1;
                }
                value = s[b..i].iter().collect();
            }
        }
        if key.eq_ignore_ascii_case(name) {
            return Some(value);
        }
        if i == a {
            i += 1;
        }
    }
    None
}

/// The text color set by a tag: `style="color: …"` or `<font color="…">`.
fn tag_color(attrs: &str) -> Option<Color> {
    if let Some(style) = attr_value(attrs, "style") {
        for decl in style.split(';') {
            if let Some((prop, val)) = decl.split_once(':')
                && prop.trim().eq_ignore_ascii_case("color")
                && let Some(c) = parse_color(val)
            {
                return Some(c);
            }
        }
    }
    attr_value(attrs, "color").and_then(|v| parse_color(&v))
}

fn lex_markdown(h: &mut Hl) {
    let end = h.s.len();
    let mut stack: Vec<(String, (usize, usize), Option<Color>)> = Vec::new();
    let mut fence: Option<char> = None;
    let mut cur = TEXT;
    let mut bol = true;
    // Indented code block state: previous line was blank / indented code.
    let mut prev_blank = true;
    let mut indented_code = false;
    let mut i = 0;
    while i < end {
        let c = h.s[i];
        if bol {
            bol = false;
            let e = h.line_end(i, end);
            let blank = h.s[i..e].iter().all(|c| c.is_whitespace());
            let indented = h.starts(i, "    ") || h.at(i) == '\t';
            indented_code = fence.is_none() && !blank && indented && (prev_blank || indented_code);
            prev_blank = blank;
            if indented_code {
                h.paint(i, e, cur);
                i = e;
                continue;
            }
            let mut k = i;
            while k < end && k - i < 3 && h.s[k] == ' ' {
                k += 1;
            }
            if h.starts(k, "```") || h.starts(k, "~~~") {
                let f = h.s[k];
                match fence {
                    None => fence = Some(f),
                    Some(x) if x == f => fence = None,
                    Some(_) => {}
                }
                let e = h.line_end(i, end);
                h.paint(i, e, cur);
                i = e;
                continue;
            }
        }
        if c == '\n' {
            bol = true;
            i += 1;
            continue;
        }
        if fence.is_some() {
            h.paint(i, i + 1, cur);
            i += 1;
            continue;
        }
        if c == '`' {
            let mut n = 0;
            while h.at(i + n) == '`' {
                n += 1;
            }
            let run = "`".repeat(n);
            let e = h.line_end(i, end);
            let stop = h.find(i + n, e, &run).map_or(i + n, |p| p + n);
            h.paint(i, stop, cur);
            i = stop;
            continue;
        }
        if c == '\\' && h.at(i + 1) == '<' {
            h.hide(i, i + 1);
            h.paint(i + 1, i + 2, cur);
            i += 2;
            continue;
        }
        if h.starts(i, "<!--") {
            let e = match h.find(i + 4, end, "-->") {
                Some(p) => p + 3,
                None => {
                    h.mark(i, i + 4);
                    end
                }
            };
            h.hide(i, e);
            i = e;
            continue;
        }
        if c == '<'
            && let Some(t) = md_tag(h, i, end)
        {
            h.hide(i, t.end);
            if t.closing {
                close_tag(h, &mut stack, &t.name, (i, t.end), true);
            } else if !t.self_close && !VOID_TAGS.contains(&t.name.as_str()) {
                let color = tag_color(&t.attrs);
                stack.push((t.name, (i, t.end), color));
            }
            cur = stack.iter().rev().find_map(|&(_, _, c)| c).unwrap_or(TEXT);
            i = t.end;
            continue;
        }
        h.paint(i, i + 1, cur);
        i += 1;
    }
    mark_unclosed(h, &stack, true);
}

// ----- CSS colors -----

const NAMED_COLORS: &[(&str, (u8, u8, u8))] = &[
    ("black", (0, 0, 0)),
    ("white", (255, 255, 255)),
    ("red", (255, 0, 0)),
    ("green", (0, 128, 0)),
    ("blue", (0, 0, 255)),
    ("yellow", (255, 255, 0)),
    ("orange", (255, 165, 0)),
    ("purple", (128, 0, 128)),
    ("pink", (255, 192, 203)),
    ("brown", (165, 42, 42)),
    ("gray", (128, 128, 128)),
    ("grey", (128, 128, 128)),
    ("silver", (192, 192, 192)),
    ("maroon", (128, 0, 0)),
    ("olive", (128, 128, 0)),
    ("lime", (0, 255, 0)),
    ("aqua", (0, 255, 255)),
    ("cyan", (0, 255, 255)),
    ("teal", (0, 128, 128)),
    ("navy", (0, 0, 128)),
    ("fuchsia", (255, 0, 255)),
    ("magenta", (255, 0, 255)),
    ("gold", (255, 215, 0)),
    ("violet", (238, 130, 238)),
    ("indigo", (75, 0, 130)),
    ("coral", (255, 127, 80)),
    ("crimson", (220, 20, 60)),
    ("salmon", (250, 128, 114)),
    ("khaki", (240, 230, 140)),
    ("orchid", (218, 112, 214)),
    ("plum", (221, 160, 221)),
    ("tan", (210, 180, 140)),
    ("turquoise", (64, 224, 208)),
    ("darkred", (139, 0, 0)),
    ("darkgreen", (0, 100, 0)),
    ("darkblue", (0, 0, 139)),
    ("darkorange", (255, 140, 0)),
    ("darkviolet", (148, 0, 211)),
    ("darkcyan", (0, 139, 139)),
    ("darkgray", (169, 169, 169)),
    ("darkgrey", (169, 169, 169)),
    ("lightblue", (173, 216, 230)),
    ("lightgreen", (144, 238, 144)),
    ("lightgray", (211, 211, 211)),
    ("lightgrey", (211, 211, 211)),
    ("lightyellow", (255, 255, 224)),
    ("lightpink", (255, 182, 193)),
    ("lightcoral", (240, 128, 128)),
    ("skyblue", (135, 206, 235)),
    ("steelblue", (70, 130, 180)),
    ("royalblue", (65, 105, 225)),
    ("dodgerblue", (30, 144, 255)),
    ("midnightblue", (25, 25, 112)),
    ("tomato", (255, 99, 71)),
    ("chocolate", (210, 105, 30)),
    ("firebrick", (178, 34, 34)),
    ("forestgreen", (34, 139, 34)),
    ("seagreen", (46, 139, 87)),
    ("limegreen", (50, 205, 50)),
    ("springgreen", (0, 255, 127)),
    ("chartreuse", (127, 255, 0)),
    ("deeppink", (255, 20, 147)),
    ("hotpink", (255, 105, 180)),
    ("lavender", (230, 230, 250)),
    ("beige", (245, 245, 220)),
    ("ivory", (255, 255, 240)),
    ("wheat", (245, 222, 179)),
    ("sienna", (160, 82, 45)),
    ("peru", (205, 133, 63)),
    ("goldenrod", (218, 165, 32)),
    ("aquamarine", (127, 255, 212)),
    ("slategray", (112, 128, 144)),
    ("slategrey", (112, 128, 144)),
    ("mediumpurple", (147, 112, 219)),
    ("orangered", (255, 69, 0)),
    ("yellowgreen", (154, 205, 50)),
];

/// Parses a CSS color: a name, `#rgb`, `#rrggbb` (with optional alpha)
/// or `rgb()`/`rgba()`.
pub fn parse_color(v: &str) -> Option<Color> {
    let v = v.trim().to_ascii_lowercase();
    let v = v.trim_end_matches("!important").trim();
    if let Some(hex) = v.strip_prefix('#') {
        let d: Vec<u8> = hex.chars().map(|c| c.to_digit(16).map(|x| x as u8)).collect::<Option<_>>()?;
        return match d.len() {
            3 | 4 => Some(rgb(d[0] * 17, d[1] * 17, d[2] * 17)),
            6 | 8 => Some(rgb(d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5])),
            _ => None,
        };
    }
    if let Some(args) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")) {
        let args = args.strip_suffix(')')?;
        let parts: Vec<u8> = args
            .split([',', ' ', '/'])
            .filter(|s| !s.is_empty())
            .take(3)
            .map(|p| match p.strip_suffix('%') {
                Some(pc) => pc.parse::<f32>().ok().map(|x| (x.clamp(0.0, 100.0) * 2.55).round() as u8),
                None => p.parse::<f32>().ok().map(|x| x.clamp(0.0, 255.0).round() as u8),
            })
            .collect::<Option<_>>()?;
        return (parts.len() == 3).then(|| rgb(parts[0], parts[1], parts[2]));
    }
    NAMED_COLORS.iter().find(|(n, _)| *n == v).map(|&(_, (r, g, b))| rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(c: &Colored) -> Vec<String> {
        c.lines.iter().map(|l| l.iter().map(|c| c.ch).collect()).collect()
    }

    fn marked(c: &Colored, line: usize) -> String {
        c.lines[line].iter().filter(|x| x.mark).map(|x| x.ch).collect()
    }

    fn color_of(c: &Colored, line: usize, ch: char) -> Color {
        c.lines[line].iter().find(|x| x.ch == ch && !x.mark).unwrap().fg
    }

    #[test]
    fn balanced_code_has_no_marks() {
        let c = colorize("fn main() {\n    let v = vec![1, 2];\n    println!(\"{}\", v[0]); // ok\n}\n", Lang::Rust, 4);
        assert_eq!(c.problems, 0, "{:?}", text_of(&c));
        assert_eq!(c.lines.len(), 5);
        assert_eq!(c.lines[0][0].fg, KEYWORD);
    }

    #[test]
    fn unbalanced_brackets_and_strings_are_marked() {
        let c = colorize("function f() {\n  return (1 + 2;\n}\n", Lang::JavaScript, 4);
        assert_eq!(c.problems, 1);
        assert_eq!(text_of(&c)[1], "  return (1 + 2;");
        assert_eq!(marked(&c, 1), "(");
        let c = colorize("x = \"abc\ny = 1\n", Lang::Python, 4);
        assert_eq!(c.problems, 1);
        assert_eq!(text_of(&c)[0], "x = \"abc");
        assert_eq!(marked(&c, 0), "\"");
        let c = colorize("a = [1, 2)]\n", Lang::Python, 4);
        assert!(c.problems >= 1);
    }

    #[test]
    fn rust_lifetimes_and_chars() {
        let c = colorize("fn f<'a>(x: &'a str) -> char { 'x' }\n", Lang::Rust, 4);
        assert_eq!(c.problems, 0);
    }

    #[test]
    fn js_regex_is_not_a_string() {
        let c = colorize("let r = /it's \"[/]\"/g; let d = a / b / c;\n", Lang::JavaScript, 4);
        assert_eq!(c.problems, 0);
    }

    #[test]
    fn json_keys_and_values() {
        let c = colorize("{\"key\": \"value\", \"n\": 12, \"b\": true}", Lang::Json, 4);
        assert_eq!(c.problems, 0);
        assert_eq!(color_of(&c, 0, 'k'), VARIABLE);
        assert_eq!(color_of(&c, 0, 'v'), STRING);
        assert_eq!(color_of(&c, 0, '1'), NUMBER);
    }

    #[test]
    fn html_tags_are_checked() {
        let ok = colorize("<html><body><p>a<br>b<ul><li>x<li>y</ul></body></html>", Lang::Html, 4);
        assert_eq!(ok.problems, 0);
        let bad = colorize("<div><span>text</div>", Lang::Html, 4);
        assert_eq!(bad.problems, 1);
        assert_eq!(text_of(&bad)[0], "<div><span>text</div>");
        assert_eq!(marked(&bad, 0), "<span>");
        let script = colorize("<script>if (a < b) { x(); }</script>", Lang::Html, 4);
        assert_eq!(script.problems, 0);
        let xml = colorize("<?xml version=\"1.0\"?><a><B></b></a>", Lang::Xml, 4);
        assert!(xml.problems >= 1);
    }

    #[test]
    fn php_blocks() {
        let c = colorize("<p><?php if ($a) { echo \"<b>\"; } ?></p>", Lang::Php, 4);
        assert_eq!(c.problems, 0);
    }

    #[test]
    fn markdown_hides_tags_and_applies_colors() {
        let c = colorize("Hello <span style=\"color: red\">red <b>bold</b></span> end\n", Lang::Markdown, 4);
        assert_eq!(c.problems, 0);
        assert_eq!(text_of(&c)[0], "Hello red bold end");
        assert_eq!(color_of(&c, 0, 'H'), TEXT);
        assert_eq!(color_of(&c, 0, 'r'), rgb(255, 0, 0));
        assert_eq!(color_of(&c, 0, 'b'), rgb(255, 0, 0));
        assert_eq!(color_of(&c, 0, 'n'), TEXT);
        let c = colorize("<font color=\"#00ff00\">g</font> a < b <https://x.y>\n", Lang::Markdown, 4);
        assert_eq!(text_of(&c)[0], "g a < b <https://x.y>");
        assert_eq!(color_of(&c, 0, 'g'), rgb(0, 255, 0));
        let c = colorize("<div>unclosed\n", Lang::Markdown, 4);
        assert_eq!(c.problems, 1);
        assert_eq!(text_of(&c)[0], "<div>unclosed");
        assert_eq!(marked(&c, 0), "<div>");
    }

    #[test]
    fn multiline_strings_and_md_code() {
        assert_eq!(colorize("let s = \"a\nb\";\n", Lang::Rust, 4).problems, 0);
        assert_eq!(colorize("let s = \"a\nb;\n", Lang::JavaScript, 4).problems, 1);
        let c = colorize("text\n\n    gm <part> --x\n\n```\n<b>\n```\n", Lang::Markdown, 4);
        assert_eq!(c.problems, 0);
        assert_eq!(text_of(&c)[2], "    gm <part> --x");
        assert_eq!(text_of(&c)[5], "<b>");
    }

    #[test]
    fn css_colors() {
        assert_eq!(parse_color("#f00"), Some(rgb(255, 0, 0)));
        assert_eq!(parse_color(" #1E90FF "), Some(rgb(30, 144, 255)));
        assert_eq!(parse_color("rgb(1, 2, 3)"), Some(rgb(1, 2, 3)));
        assert_eq!(parse_color("Blue !important"), Some(rgb(0, 0, 255)));
        assert_eq!(parse_color("nonsense"), None);
    }
}

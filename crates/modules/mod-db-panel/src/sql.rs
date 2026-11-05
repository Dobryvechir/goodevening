//! SQL text helpers: splitting scripts into statements, quoting names,
//! and turning results into plain text.

use crate::config::Driver;
use unicode_width::UnicodeWidthStr;

/// Rows returned by a statement.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub columns: Vec<String>,
    /// `None` is SQL NULL.
    pub rows: Vec<Vec<Option<String>>>,
}

impl Table {
    /// The first column of the first row.
    pub fn scalar(&self) -> Option<&str> {
        self.rows.first()?.first()?.as_deref()
    }
}

/// What one statement produced (some statements produce several).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Rows(Table),
    Affected(u64),
}

/// At most this many rows of a statement are written to the output file.
pub const MAX_OUTPUT_ROWS: usize = 10_000;

/// Splits a script into statements on `;` outside of strings, quoted names
/// and comments. PostgreSQL: `$tag$…$tag$` bodies and nested `/* */`.
/// MariaDB: `#` comments, backslash escapes, `` `names` `` and the
/// `DELIMITER` command of the command line client. Statements made only of
/// comments and spaces are dropped.
pub fn split_statements(text: &str, driver: Driver) -> Vec<String> {
    let my = driver == Driver::MariaDb;
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut delim = ";".to_string();
    let mut start = 0;
    let mut i = 0;
    // Something other than spaces and comments since `start`.
    let mut content = false;
    let push = |out: &mut Vec<String>, s: &str, content: bool| {
        let s = s.trim();
        if content && !s.is_empty() {
            out.push(s.to_string());
        }
    };
    while i < b.len() {
        let c = b[i];
        // DELIMITER at the start of a statement (MariaDB client syntax).
        if my && !content && (i == 0 || b[i - 1] == b'\n' || b[i - 1].is_ascii_whitespace()) && starts_with_ci(&b[i..], b"delimiter") {
            let after = i + 9;
            if after < b.len() && (b[after] == b' ' || b[after] == b'\t') {
                let end = b[after..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| after + p);
                let d = text[after..end].trim();
                if !d.is_empty() {
                    delim = d.to_string();
                }
                i = end;
                start = end;
                continue;
            }
        }
        match c {
            b'\'' | b'"' => {
                content = true;
                i = skip_quoted(b, i, c, my);
            }
            b'`' if my => {
                content = true;
                i = skip_quoted(b, i, c, false);
            }
            b'-' if b.get(i + 1) == Some(&b'-') && (!my || b.get(i + 2).is_none_or(|c| c.is_ascii_whitespace())) => {
                i = b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| i + p);
            }
            b'#' if my => {
                i = b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| i + p);
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                // MariaDB's /*! … */ is executable: it counts as content.
                if my && b.get(i + 2) == Some(&b'!') {
                    content = true;
                }
                i = skip_block_comment(b, i, !my);
            }
            b'$' if !my && (i == 0 || !is_ident(b[i - 1])) => {
                content = true;
                i = skip_dollar(b, i);
            }
            _ if b[i..].starts_with(delim.as_bytes()) => {
                push(&mut out, &text[start..i], content);
                i += delim.len();
                start = i;
                content = false;
            }
            _ => {
                if !c.is_ascii_whitespace() {
                    content = true;
                }
                i += 1;
            }
        }
    }
    push(&mut out, &text[start.min(text.len())..], content);
    out
}

fn starts_with_ci(b: &[u8], word: &[u8]) -> bool {
    b.len() >= word.len() && b[..word.len()].eq_ignore_ascii_case(word)
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

/// The index after the closing quote (doubled quotes stay inside).
fn skip_quoted(b: &[u8], open: usize, q: u8, backslash: bool) -> usize {
    let mut i = open + 1;
    while i < b.len() {
        if backslash && b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == q {
            if b.get(i + 1) == Some(&q) {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    b.len()
}

fn skip_block_comment(b: &[u8], open: usize, nested: bool) -> usize {
    let mut depth = 0;
    let mut i = open;
    while i < b.len() {
        if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            if depth == 0 || nested {
                depth += 1;
            }
            i += 2;
        } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    b.len()
}

/// `$tag$ … $tag$`; a lone `$` (or `$1`) is just a character.
fn skip_dollar(b: &[u8], open: usize) -> usize {
    let mut j = open + 1;
    while j < b.len() && is_ident(b[j]) {
        j += 1;
    }
    if j >= b.len() || b[j] != b'$' || b.get(open + 1).is_some_and(|c| c.is_ascii_digit()) {
        return open + 1;
    }
    let tag = &b[open..=j];
    let mut i = j + 1;
    while i + tag.len() <= b.len() {
        if &b[i..i + tag.len()] == tag {
            return i + tag.len();
        }
        i += 1;
    }
    b.len()
}

/// Always quotes a name (for the SQL gm writes itself).
pub fn quote_ident(name: &str, driver: Driver) -> String {
    match driver {
        Driver::MariaDb => format!("`{}`", name.replace('`', "``")),
        Driver::PostgreSql => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// A string literal for PostgreSQL (`standard_conforming_strings` is on).
pub fn pg_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

const RESERVED: &[&str] = &[
    "all", "analyze", "and", "any", "array", "as", "asc", "between", "both", "by", "case", "cast", "check", "collate", "column", "constraint",
    "create", "cross", "current_date", "current_time", "current_user", "database", "default", "delete", "desc", "distinct", "do", "drop",
    "else", "end", "except", "exists", "false", "fetch", "for", "foreign", "from", "full", "grant", "group", "having", "in", "index", "inner",
    "insert", "intersect", "interval", "into", "is", "join", "key", "keys", "leading", "left", "like", "limit", "natural", "not", "null",
    "offset", "on", "only", "or", "order", "outer", "primary", "range", "references", "returning", "right", "rows", "select", "set", "some",
    "table", "then", "to", "trailing", "true", "union", "unique", "update", "user", "using", "values", "when", "where", "window", "with",
];

/// Quotes a name only when it has to be (for the command line).
pub fn maybe_quote(name: &str, driver: Driver) -> String {
    let plain = match driver {
        Driver::PostgreSql => {
            name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
                && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$')
        }
        Driver::MariaDb => {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$') && !name.chars().all(|c| c.is_ascii_digit())
        }
    };
    if plain && !RESERVED.contains(&name.to_ascii_lowercase().as_str()) { name.to_string() } else { quote_ident(name, driver) }
}

/// A cell for a text grid: one line, NULL spelled out.
fn cell(v: &Option<String>) -> String {
    match v {
        None => "NULL".into(),
        Some(s) => {
            if s.contains(['\r', '\n', '\t']) {
                s.replace('\\', "\\\\").replace('\r', "\\r").replace('\n', "\\n").replace('\t', "\\t")
            } else {
                s.clone()
            }
        }
    }
}

fn pad(s: &str, w: usize) -> String {
    let n = s.width();
    if n >= w { s.to_string() } else { format!("{s}{}", " ".repeat(w - n)) }
}

/// A table as text:
/// ```text
/// +----+-------+
/// | id | name  |
/// +----+-------+
/// | 1  | Alpha |
/// +----+-------+
/// ```
/// At most `max_rows` rows are written.
pub fn grid(t: &Table, max_rows: usize) -> String {
    let rows: Vec<Vec<String>> = t.rows.iter().take(max_rows).map(|r| r.iter().map(cell).collect()).collect();
    let mut w: Vec<usize> = t.columns.iter().map(|c| c.width()).collect();
    for r in &rows {
        for (i, c) in r.iter().enumerate() {
            if i < w.len() {
                w[i] = w[i].max(c.width());
            }
        }
    }
    let line = || {
        let mut s = String::from("+");
        for n in &w {
            s.push_str(&"-".repeat(n + 2));
            s.push('+');
        }
        s.push('\n');
        s
    };
    let row = |cells: &[String]| {
        let mut s = String::from("|");
        for (i, n) in w.iter().enumerate() {
            s.push(' ');
            s.push_str(&pad(cells.get(i).map(String::as_str).unwrap_or(""), *n));
            s.push_str(" |");
        }
        s.push('\n');
        s
    };
    let mut out = line();
    out.push_str(&row(&t.columns));
    out.push_str(&line());
    for r in &rows {
        out.push_str(&row(r));
    }
    if !rows.is_empty() {
        out.push_str(&line());
    }
    out
}

/// The text of a statement's outcomes: grids with row counts, or the
/// number of affected rows.
pub fn outcomes_text(list: &[Outcome]) -> String {
    let mut out = String::new();
    for o in list {
        match o {
            Outcome::Rows(t) => {
                out.push_str(&grid(t, MAX_OUTPUT_ROWS));
                let n = t.rows.len();
                out.push_str(&format!("{n} row{}", if n == 1 { "" } else { "s" }));
                if n > MAX_OUTPUT_ROWS {
                    out.push_str(&format!(" (the first {MAX_OUTPUT_ROWS} are shown)"));
                }
                out.push('\n');
            }
            Outcome::Affected(n) => out.push_str(&format!("OK, {n} row{} affected\n", if *n == 1 { "" } else { "s" })),
        }
    }
    if list.is_empty() {
        out.push_str("OK\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitting() {
        let pg = Driver::PostgreSql;
        assert_eq!(split_statements("select 1; select 2", pg), ["select 1", "select 2"]);
        assert_eq!(split_statements("select ';' ; -- x; y\n select \"a;b\";", pg), ["select ';'", "-- x; y\n select \"a;b\""]);
        assert_eq!(split_statements("-- only a comment;\n/* and; this */ ;", pg), Vec::<String>::new());
        let f = "create function f() returns int as $$ begin return 1; end; $$ language plpgsql; select f()";
        assert_eq!(split_statements(f, pg).len(), 2);
        assert_eq!(split_statements("select $1; select $a$;$a$", pg), ["select $1", "select $a$;$a$"]);
        assert_eq!(split_statements("/* a /* b; */ c; */ select 1", pg), ["/* a /* b; */ c; */ select 1"]);

        let my = Driver::MariaDb;
        assert_eq!(split_statements("select 'it\\'s;'; # c;\nselect `x;y`", my), ["select 'it\\'s;'", "# c;\nselect `x;y`"]);
        let s = "DELIMITER //\ncreate procedure p() begin select 1; select 2; end//\nDELIMITER ;\ncall p();";
        assert_eq!(split_statements(s, my), ["create procedure p() begin select 1; select 2; end", "call p()"]);
        // "--" needs a space after it in MariaDB.
        assert_eq!(split_statements("select 1--1;", my), ["select 1--1"]);
    }

    #[test]
    fn quoting() {
        assert_eq!(maybe_quote("users", Driver::PostgreSql), "users");
        assert_eq!(maybe_quote("Users", Driver::PostgreSql), "\"Users\"");
        assert_eq!(maybe_quote("user", Driver::PostgreSql), "\"user\"");
        assert_eq!(maybe_quote("Users", Driver::MariaDb), "Users");
        assert_eq!(maybe_quote("my table", Driver::MariaDb), "`my table`");
        assert_eq!(quote_ident("a`b", Driver::MariaDb), "`a``b`");
        assert_eq!(pg_literal("it's"), "'it''s'");
    }

    #[test]
    fn grids() {
        let t = Table { columns: vec!["id".into(), "name".into()], rows: vec![vec![Some("1".into()), None], vec![Some("22".into()), Some("a\nb".into())]] };
        let g = grid(&t, 100);
        assert_eq!(g, "+----+------+\n| id | name |\n+----+------+\n| 1  | NULL |\n| 22 | a\\nb |\n+----+------+\n");
        assert!(outcomes_text(&[Outcome::Rows(t), Outcome::Affected(1)]).ends_with("2 rows\nOK, 1 row affected\n"));
    }
}

//! The database panel: tables and views as panel items.
//!
//! F3 / Enter: the structure (columns, keys, indexes, foreign keys, or the
//! script of a view) in a temporary file; F4: the first 100 rows in a
//! temporary file in the editor; a click: `select count(*)` into the
//! "Rows" column; F5/F6 out: the first 100 rows as `<table>.txt` (moving
//! never drops anything); F5/F6 in: text files run as SQL scripts; F8:
//! `DROP`; the command line: SQL. The output files are only text: saving
//! them changes nothing in the database.
//!
//! The connection is checked before every operation and opened again when
//! the server has closed it; it is closed when the panel is left.

use crate::config::{Driver, Params};
use crate::conn::{self, Db};
use crate::sql::{self, Table, maybe_quote, pg_literal, quote_ident};
use gm_core::gm_vfs::{CancelToken, Caps, ChangeDir, CustomColumn, Entry, Progress, ReadSeek, RemoteFile, VfsProvider};
use std::any::Any;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

/// F4 and F5 read this many rows.
pub const ROW_LIMIT: usize = 100;
/// Scripts copied into the panel must be smaller than this.
pub const MAX_SCRIPT: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Table,
    View,
    MatView,
    Foreign,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Table => "table",
            Kind::View => "view",
            Kind::MatView => "m.view",
            Kind::Foreign => "foreign",
        }
    }
    fn is_view(self) -> bool {
        matches!(self, Kind::View | Kind::MatView)
    }
    fn noun(self) -> &'static str {
        match self {
            Kind::Table => "table",
            Kind::View => "view",
            Kind::MatView => "materialized view",
            Kind::Foreign => "foreign table",
        }
    }
}

#[derive(Clone, Debug)]
struct Obj {
    /// PostgreSQL schema; empty for MariaDB (the configured database).
    schema: String,
    name: String,
    kind: Kind,
}

pub struct DbProvider {
    params: Params,
    label: String,
    db: Option<Db>,
    server: String,
    /// Panel name → object.
    objects: HashMap<String, Obj>,
    /// Row counts from clicks.
    counts: HashMap<String, u64>,
    /// The output of the last scripts copied in.
    result: Option<Box<dyn RemoteFile>>,
    seq: u32,
}

fn other(e: impl Into<String>) -> io::Error {
    io::Error::other(e.into())
}

/// Generated text as a "remote" file: a temporary local copy, never written back.
struct TextFile {
    label: String,
    name: String,
    text: Result<String, String>,
}

impl RemoteFile for TextFile {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn name(&self) -> String {
        self.name.clone()
    }
    fn download(&mut self, dest: &Path) -> io::Result<()> {
        match &self.text {
            Ok(t) => std::fs::write(dest, t),
            Err(e) => Err(other(e.clone())),
        }
    }
    fn upload(&mut self, _src: &Path) -> io::Result<()> {
        Ok(())
    }
    fn write_back(&self) -> bool {
        false
    }
}

/// A name usable as a file name everywhere.
fn file_name(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let t = t.trim_end_matches(['.', ' ']).to_string();
    if t.is_empty() { "table".into() } else { t }
}

fn section(out: &mut String, title: &str, t: &Table) {
    out.push_str(title);
    out.push('\n');
    if t.rows.is_empty() {
        out.push_str("(none)\n\n");
    } else {
        out.push_str(&sql::grid(t, usize::MAX));
        out.push('\n');
    }
}

/// Only the rows whose column `col` is (or is not) `value`, without that column.
fn split_rows(t: &Table, col: usize, value: &str, keep: bool) -> Table {
    let pick = |cols: &[String]| cols.iter().enumerate().filter(|(i, _)| *i != col).map(|(_, c)| c.clone()).collect();
    Table {
        columns: pick(&t.columns),
        rows: t
            .rows
            .iter()
            .filter(|r| (r.get(col).and_then(|v| v.as_deref()) == Some(value)) == keep)
            .map(|r| r.iter().enumerate().filter(|(i, _)| *i != col).map(|(_, v)| v.clone()).collect())
            .collect(),
    }
}

impl DbProvider {
    pub fn new(params: Params, mut db: Db) -> DbProvider {
        let v = match params.driver {
            Driver::MariaDb => "SELECT version()",
            Driver::PostgreSql => "SELECT 'PostgreSQL ' || current_setting('server_version')",
        };
        let server = db.rows(v, &[]).ok().and_then(|t| t.scalar().map(str::to_string)).unwrap_or_default();
        let label = params.label();
        DbProvider { params, label, db: Some(db), server, objects: HashMap::new(), counts: HashMap::new(), result: None, seq: 0 }
    }

    /// The connection, opened again if the server closed it.
    fn db(&mut self) -> io::Result<&mut Db> {
        let alive = self.db.as_mut().is_some_and(|d| d.alive());
        if !alive {
            self.db = None;
            let d = conn::connect(&self.params).map_err(|e| other(format!("Cannot connect to {} again\n{}", self.label, e.message)))?;
            self.db = Some(d);
        }
        Ok(self.db.as_mut().expect("connected"))
    }

    fn driver(&self) -> Driver {
        self.params.driver
    }

    fn obj(&self, name: &str) -> io::Result<Obj> {
        self.objects.get(name).cloned().ok_or_else(|| other(format!("No table or view \"{name}\"")))
    }

    /// The quoted name for the SQL gm writes.
    fn qualified(&self, o: &Obj) -> String {
        let d = self.driver();
        if o.schema.is_empty() { quote_ident(&o.name, d) } else { format!("{}.{}", quote_ident(&o.schema, d), quote_ident(&o.name, d)) }
    }

    fn display_name(o: &Obj) -> String {
        if o.schema.is_empty() || o.schema == "public" { o.name.clone() } else { format!("{}.{}", o.schema, o.name) }
    }

    fn next_seq(&mut self) -> u32 {
        self.seq += 1;
        self.seq
    }

    /// `select * … limit 100` as text.
    fn rows_text(&mut self, name: &str) -> io::Result<String> {
        let o = self.obj(name)?;
        let q = format!("SELECT * FROM {} LIMIT {ROW_LIMIT}", self.qualified(&o));
        let t = self.db()?.rows(&q, &[]).map_err(other)?;
        let n = t.rows.len();
        Ok(format!("-- {q}\n-- {}\n{}{n} row{}\n", self.label, sql::grid(&t, ROW_LIMIT), if n == 1 { "" } else { "s" }))
    }

    /// Columns, keys, indexes and foreign keys of a table; the script of a view.
    fn structure_text(&mut self, name: &str) -> io::Result<String> {
        let o = self.obj(name)?;
        let mut out = format!("{} {}\n-- {}\n\n", o.kind.noun(), Self::display_name(&o), self.label);
        match self.driver() {
            Driver::PostgreSql => self.pg_structure(&o, &mut out)?,
            Driver::MariaDb => self.my_structure(&o, &mut out)?,
        }
        Ok(out)
    }

    fn pg_structure(&mut self, o: &Obj, out: &mut String) -> io::Result<()> {
        let rel = format!("{}::regclass", pg_literal(&self.qualified(o)));
        let db = self.db()?;
        let mut q = |s: String| db.rows(&s, &[]).map_err(other);
        let cols = q(format!(
            "SELECT a.attnum AS \"#\", a.attname AS \"Column\", format_type(a.atttypid, a.atttypmod) AS \"Type\", \
             CASE WHEN a.attnotnull THEN 'NO' ELSE 'YES' END AS \"Nullable\", pg_get_expr(d.adbin, d.adrelid) AS \"Default\", \
             col_description(a.attrelid, a.attnum) AS \"Comment\" \
             FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
             WHERE a.attrelid = {rel} AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum"
        ))?;
        section(out, "Columns", &cols);
        if o.kind.is_view() {
            let def = q(format!("SELECT pg_get_viewdef({rel}, true)"))?;
            let what = if o.kind == Kind::MatView { "MATERIALIZED VIEW" } else { "OR REPLACE VIEW" };
            out.push_str(&format!("Script\nCREATE {what} {} AS\n{}\n", self.qualified(o), def.scalar().unwrap_or("").trim_end()));
            return Ok(());
        }
        let cons = q(format!(
            "SELECT contype::text AS t, conname AS \"Name\", pg_get_constraintdef(oid, true) AS \"Definition\" \
             FROM pg_constraint WHERE conrelid = {rel} ORDER BY conname"
        ))?;
        section(out, "Primary key", &split_rows(&cons, 0, "p", true));
        let idx = q(format!(
            "SELECT c.relname AS \"Name\", pg_get_indexdef(i.indexrelid) AS \"Definition\" \
             FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid WHERE i.indrelid = {rel} ORDER BY i.indisprimary DESC, c.relname"
        ))?;
        section(out, "Indexes", &idx);
        section(out, "Foreign keys", &split_rows(&cons, 0, "f", true));
        let others = Table {
            columns: cons.columns[1..].to_vec(),
            rows: cons.rows.iter().filter(|r| !matches!(r[0].as_deref(), Some("p" | "f"))).map(|r| r[1..].to_vec()).collect(),
        };
        section(out, "Other constraints (unique, check, exclusion)", &others);
        let refs = q(format!(
            "SELECT conrelid::regclass::text AS \"Table\", conname AS \"Name\", pg_get_constraintdef(oid, true) AS \"Definition\" \
             FROM pg_constraint WHERE confrelid = {rel} AND contype = 'f' ORDER BY 1, 2"
        ))?;
        section(out, "Referenced by (foreign keys of other tables)", &refs);
        Ok(())
    }

    fn my_structure(&mut self, o: &Obj, out: &mut String) -> io::Result<()> {
        let qn = self.qualified(o);
        let name = o.name.clone();
        let db = self.db()?;
        let cols = db
            .rows(
                "SELECT ordinal_position AS `#`, column_name AS `Column`, column_type AS `Type`, is_nullable AS `Nullable`, \
                 column_default AS `Default`, extra AS `Extra`, column_comment AS `Comment` \
                 FROM information_schema.columns WHERE table_schema = DATABASE() AND table_name = ? ORDER BY ordinal_position",
                &[&name],
            )
            .map_err(other)?;
        section(out, "Columns", &cols);
        if o.kind.is_view() {
            let t = db.rows(&format!("SHOW CREATE VIEW {qn}"), &[]).map_err(other)?;
            let def = t.rows.first().and_then(|r| r.get(1)).cloned().flatten().unwrap_or_default();
            out.push_str(&format!("Script\n{def}\n"));
            return Ok(());
        }
        let idx = db
            .rows(
                "SELECT index_name AS `Name`, IF(non_unique = 0, 'YES', 'NO') AS `Unique`, index_type AS `Type`, \
                 GROUP_CONCAT(CONCAT(column_name, COALESCE(CONCAT('(', sub_part, ')'), '')) ORDER BY seq_in_index SEPARATOR ', ') AS `Columns` \
                 FROM information_schema.statistics WHERE table_schema = DATABASE() AND table_name = ? \
                 GROUP BY index_name, non_unique, index_type ORDER BY index_name = 'PRIMARY' DESC, index_name",
                &[&name],
            )
            .map_err(other)?;
        let pk = Table {
            columns: vec!["Columns".into()],
            rows: idx.rows.iter().filter(|r| r[0].as_deref() == Some("PRIMARY")).map(|r| vec![r[3].clone()]).collect(),
        };
        section(out, "Primary key", &pk);
        section(out, "Indexes", &idx);
        let fk_sql = |mine: bool| {
            format!(
                "SELECT k.constraint_name AS `Name`, {} \
                 GROUP_CONCAT(k.column_name ORDER BY k.ordinal_position SEPARATOR ', ') AS `Columns`, \
                 k.referenced_table_name AS `References`, \
                 GROUP_CONCAT(k.referenced_column_name ORDER BY k.ordinal_position SEPARATOR ', ') AS `Referenced columns`, \
                 r.update_rule AS `On update`, r.delete_rule AS `On delete` \
                 FROM information_schema.key_column_usage k JOIN information_schema.referential_constraints r \
                 ON r.constraint_schema = k.constraint_schema AND r.constraint_name = k.constraint_name AND r.table_name = k.table_name \
                 WHERE k.referenced_table_name IS NOT NULL AND {} \
                 GROUP BY k.constraint_name, k.table_name, k.referenced_table_name, r.update_rule, r.delete_rule ORDER BY k.table_name, k.constraint_name",
                if mine { "" } else { "k.table_name AS `Table`," },
                if mine {
                    "k.table_schema = DATABASE() AND k.table_name = ?"
                } else {
                    "k.referenced_table_schema = DATABASE() AND k.referenced_table_name = ?"
                }
            )
        };
        let fks = db.rows(&fk_sql(true), &[&name]).map_err(other)?;
        section(out, "Foreign keys", &fks);
        let refs = db.rows(&fk_sql(false), &[&name]).map_err(other)?;
        section(out, "Referenced by (foreign keys of other tables)", &refs);
        let t = db.rows(&format!("SHOW CREATE TABLE {qn}"), &[]).map_err(other)?;
        let def = t.rows.first().and_then(|r| r.get(1)).cloned().flatten().unwrap_or_default();
        out.push_str(&format!("Script\n{def};\n"));
        Ok(())
    }

    /// Runs the statements of `text`, adding each one and its result to
    /// `out`. Stops at the first error (also written to `out`).
    fn run_script(&mut self, text: &str, out: &mut String, progress: &mut dyn FnMut(&str) -> bool) -> io::Result<Result<(), String>> {
        let stmts = sql::split_statements(text, self.driver());
        if stmts.is_empty() {
            out.push_str("-- No statements\n");
        }
        for s in stmts {
            out.push_str(&format!("SQL> {s}\n"));
            if !progress(&s) {
                out.push_str("Cancelled.\n");
                return Ok(Err("Cancelled".into()));
            }
            match self.db()?.run(&s) {
                Ok(list) => {
                    out.push_str(&sql::outcomes_text(&list));
                    out.push('\n');
                }
                Err(e) => {
                    out.push_str(&format!("{e}\n\nExecution stopped.\n"));
                    return Ok(Err(e));
                }
            }
        }
        Ok(Ok(()))
    }

    /// Checks that every path is a text file smaller than 1 MB and reads them.
    fn read_scripts(src: &[PathBuf]) -> io::Result<Vec<(PathBuf, String)>> {
        let mut v = Vec::new();
        for p in src {
            let m = std::fs::metadata(p).map_err(|e| other(format!("{}\n{e}", p.display())))?;
            if !m.is_file() {
                return Err(other(format!("{}\nOnly text files with SQL statements can be copied here.", p.display())));
            }
            if m.len() >= MAX_SCRIPT {
                return Err(other(format!("{}\nThe file is too large ({} bytes): SQL scripts must be smaller than 1 MB.", p.display(), m.len())));
            }
            let bytes = std::fs::read(p).map_err(|e| other(format!("{}\n{e}", p.display())))?;
            let not_text = || other(format!("{}\nThis is not a text file.", p.display()));
            if bytes.contains(&0) {
                return Err(not_text());
            }
            let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
            let text = String::from_utf8(bytes.to_vec()).map_err(|_| not_text())?;
            v.push((p.clone(), text));
        }
        Ok(v)
    }

    fn text_file(&mut self, what: &str, name: String, text: Result<String, String>) -> Box<dyn RemoteFile> {
        Box::new(TextFile { label: format!("{}: {what}", self.label), name, text })
    }
}

impl VfsProvider for DbProvider {
    fn title(&self) -> String {
        self.label.clone()
    }
    fn current_dir(&self) -> String {
        String::new()
    }
    fn caps(&self) -> Caps {
        Caps::READ | Caps::WRITE | Caps::DELETE
    }
    fn list(&mut self, _cancel: &CancelToken) -> io::Result<Vec<Entry>> {
        let sql = match self.driver() {
            Driver::PostgreSql => {
                "SELECT n.nspname, c.relname, c.relkind::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
                 AND n.nspname !~ '^pg_(toast|temp)' AND NOT c.relispartition"
            }
            Driver::MariaDb => {
                "SELECT '', table_name, table_type FROM information_schema.tables \
                 WHERE table_schema = DATABASE() AND table_type NOT IN ('SEQUENCE', 'SYSTEM VIEW', 'TEMPORARY')"
            }
        };
        let t = self.db()?.rows(sql, &[]).map_err(other)?;
        self.objects.clear();
        let mut out = Vec::new();
        for r in t.rows {
            let get = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
            let kind = match get(2).as_str() {
                "v" | "VIEW" => Kind::View,
                "m" => Kind::MatView,
                "f" => Kind::Foreign,
                _ => Kind::Table,
            };
            let o = Obj { schema: get(0), name: get(1), kind };
            let name = Self::display_name(&o);
            let count = self.counts.get(&name).copied();
            out.push(Entry {
                name: name.clone(),
                size: count.unwrap_or(0),
                custom: vec![kind.label().into(), count.map(|c| c.to_string()).unwrap_or_default()],
                ..Default::default()
            });
            self.objects.insert(name, o);
        }
        self.counts.retain(|k, _| self.objects.contains_key(k));
        Ok(out)
    }
    fn change_dir(&mut self, to: &str) -> io::Result<ChangeDir> {
        if to == ".." || to == "/" {
            return Ok(ChangeDir::Exit);
        }
        Err(other(format!("{to}: tables and views have no folders")))
    }
    /// The structure text (quick view).
    fn open_read(&mut self, name: &str) -> io::Result<Box<dyn ReadSeek>> {
        let t = self.structure_text(name)?;
        Ok(Box::new(io::Cursor::new(t.into_bytes())))
    }
    /// F5/F6 to a folder: `<table>.txt` with the first 100 rows.
    fn get_files(&mut self, names: &[String], dest: &Path, progress: Progress) -> io::Result<()> {
        for n in names {
            if !progress(n, 0) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "Cancelled"));
            }
            let text = self.rows_text(n).map_err(|e| other(format!("{n}\n{e}")))?;
            let p = dest.join(format!("{}.txt", file_name(n)));
            std::fs::write(&p, text).map_err(|e| other(format!("{}\n{e}", p.display())))?;
        }
        Ok(())
    }
    /// F5/F6 into the panel: SQL scripts, run one statement after another.
    fn put_files(&mut self, src: &[PathBuf], progress: Progress) -> io::Result<()> {
        let scripts = Self::read_scripts(src)?;
        let mut out = format!("-- {}\n", self.label);
        let mut status = Ok(());
        let mut conn_err = None;
        for (p, text) in &scripts {
            out.push_str(&format!("\n-- File: {}\n\n", p.display()));
            let shown = p.display().to_string();
            match self.run_script(text, &mut out, &mut |_| progress(&shown, 0)) {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    status = Err(other(format!("{shown}\n{}", e.lines().next().unwrap_or(&e))));
                    break;
                }
                Err(e) => {
                    out.push_str(&format!("{e}\n\nExecution stopped.\n"));
                    conn_err = Some(e);
                    break;
                }
            }
        }
        let n = self.next_seq();
        let name = format!("script-results-{n}.txt");
        let f = self.text_file(&format!("script results #{n}"), name, Ok(out));
        self.result = Some(f);
        match conn_err {
            Some(e) => Err(e),
            None => status,
        }
    }
    /// F8: drops the tables and views.
    fn delete(&mut self, names: &[String]) -> io::Result<()> {
        for n in names {
            let o = self.obj(n)?;
            let what = match (self.driver(), o.kind) {
                (_, Kind::Table) => "TABLE",
                (_, Kind::View) => "VIEW",
                (Driver::PostgreSql, Kind::MatView) => "MATERIALIZED VIEW",
                (Driver::PostgreSql, Kind::Foreign) => "FOREIGN TABLE",
                (Driver::MariaDb, _) => "TABLE",
            };
            let q = format!("DROP {what} {}", self.qualified(&o));
            self.db()?.run(&q).map_err(|e| other(format!("{q}\n{e}")))?;
            self.counts.remove(n);
        }
        Ok(())
    }
    fn columns(&self) -> Option<Vec<CustomColumn>> {
        Some(vec![CustomColumn { title: "Type".into(), width: 7 }, CustomColumn { title: "Rows".into(), width: 11 }])
    }
    fn info_lines(&self) -> Vec<(String, String)> {
        vec![
            ("Driver".into(), self.params.driver.label().into()),
            ("Server".into(), self.server.clone()),
            ("Host".into(), format!("{}:{}", self.params.host, self.params.port)),
            ("Database".into(), self.params.db.clone()),
            ("User".into(), self.params.user.clone()),
            ("Tables and views".into(), self.objects.len().to_string()),
        ]
    }
    fn item_file(&mut self, name: &str, edit: bool) -> Option<Box<dyn RemoteFile>> {
        self.objects.get(name)?;
        let base = file_name(name);
        let n = self.next_seq();
        Some(if edit {
            let t = self.rows_text(name).map_err(|e| e.to_string());
            self.text_file(&format!("{name} (first {ROW_LIMIT} rows) #{n}"), format!("{base}.rows.txt"), t)
        } else {
            let t = self.structure_text(name).map_err(|e| e.to_string());
            self.text_file(&format!("{name} (structure) #{n}"), format!("{base}.structure.txt"), t)
        })
    }
    fn execute_to_file(&mut self, line: &str) -> Option<io::Result<Box<dyn RemoteFile>>> {
        let mut out = format!("-- {}\n\n", self.label);
        Some(match self.run_script(line, &mut out, &mut |_| true) {
            Err(e) => Err(e),
            Ok(_) => {
                let n = self.next_seq();
                Ok(self.text_file(&format!("SQL #{n}"), format!("sql-{n}.txt"), Ok(out)))
            }
        })
    }
    fn take_result_file(&mut self) -> Option<Box<dyn RemoteFile>> {
        self.result.take()
    }
    /// A click counts the rows.
    fn clicked(&mut self, name: &str) -> io::Result<bool> {
        let o = self.obj(name)?;
        let q = format!("SELECT count(*) FROM {}", self.qualified(&o));
        let t = self.db()?.rows(&q, &[]).map_err(other)?;
        let n = t.scalar().and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| other(format!("{q}\nNo number in the result")))?;
        self.counts.insert(name.to_string(), n);
        Ok(true)
    }
    fn command_line_name(&self, name: &str) -> Option<String> {
        let o = self.objects.get(name)?;
        let d = self.driver();
        let n = maybe_quote(&o.name, d);
        Some(if o.schema.is_empty() || o.schema == "public" { n } else { format!("{}.{n}", maybe_quote(&o.schema, d)) })
    }
    fn delete_after_move(&self) -> bool {
        false
    }
    fn delete_question(&self, names: &[String]) -> Option<String> {
        Some(match names {
            [one] => {
                let noun = self.objects.get(one).map_or("table", |o| o.kind.noun());
                format!("Drop the {noun}\n{one}\nin {}?\nThis cannot be undone.", self.label)
            }
            many => {
                let mut list = many.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
                if many.len() > 8 {
                    list.push_str(", …");
                }
                format!("Drop {} tables and views\n{list}\nin {}?\nThis cannot be undone.", many.len(), self.label)
            }
        })
    }
    fn report_errors(&self) -> bool {
        true
    }
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        assert_eq!(file_name("public.users"), "public.users");
        assert_eq!(file_name("a/b:c"), "a_b_c");
        assert_eq!(file_name(".."), "table");
    }

    #[test]
    fn scripts_are_checked() {
        let dir = std::env::temp_dir().join(format!("gm-db-scripts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.sql");
        std::fs::write(&ok, "\u{FEFF}select 1;").unwrap();
        let bin = dir.join("bin.sql");
        std::fs::write(&bin, b"select\0").unwrap();
        let big = dir.join("big.sql");
        std::fs::write(&big, vec![b' '; MAX_SCRIPT as usize]).unwrap();
        assert_eq!(DbProvider::read_scripts(std::slice::from_ref(&ok)).unwrap()[0].1, "select 1;");
        assert!(DbProvider::read_scripts(&[ok.clone(), bin]).unwrap_err().to_string().contains("not a text file"));
        assert!(DbProvider::read_scripts(&[big]).unwrap_err().to_string().contains("1 MB"));
        assert!(DbProvider::read_scripts(std::slice::from_ref(&dir)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

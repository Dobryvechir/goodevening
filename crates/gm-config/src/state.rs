//! `state.db`: histories, file positions and folder shortcuts in SQLite.

use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct StateDb {
    conn: Connection,
    pub max_history: usize,
}

/// Remembered viewer/editor position for a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilePos {
    pub line: u64,
    pub col: u64,
    pub top: u64,
    pub left: u64,
    pub codepage: u32,
    /// Bookmarks as "line:col" pairs separated by commas.
    pub bookmarks: String,
}

const SCHEMA: &[&str] = &[
    // v1
    "CREATE TABLE IF NOT EXISTS history (kind TEXT NOT NULL, item TEXT NOT NULL, extra TEXT NOT NULL DEFAULT '',
        time INTEGER NOT NULL, locked INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(kind, item));
     CREATE TABLE IF NOT EXISTS positions (path TEXT PRIMARY KEY, line INTEGER, col INTEGER, top INTEGER,
        left_col INTEGER, codepage INTEGER, bookmarks TEXT, time INTEGER);
     CREATE TABLE IF NOT EXISTS shortcuts (slot INTEGER PRIMARY KEY, path TEXT NOT NULL);",
];

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl StateDb {
    /// Opens (or creates) the database; falls back to an in-memory DB when
    /// the profile is not writable, so gm still works.
    pub fn open(path: &Path) -> StateDb {
        let conn = Connection::open(path)
            .and_then(Self::prepare)
            .or_else(|_| Connection::open_in_memory().and_then(Self::prepare))
            .expect("SQLite in-memory database");
        StateDb { conn, max_history: 1000 }
    }

    pub fn in_memory() -> StateDb {
        StateDb { conn: Self::prepare(Connection::open_in_memory().unwrap()).unwrap(), max_history: 1000 }
    }

    fn prepare(c: Connection) -> rusqlite::Result<Connection> {
        c.busy_timeout(std::time::Duration::from_secs(2))?;
        let _ = c.pragma_update(None, "journal_mode", "WAL");
        let version: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in SCHEMA.iter().enumerate().skip(version as usize) {
            c.execute_batch(sql)?;
            c.pragma_update(None, "user_version", (i + 1) as i64)?;
        }
        Ok(c)
    }

    /// Adds an item to a history (moves it to the top if present).
    pub fn add_history(&self, kind: &str, item: &str, extra: &str) {
        if item.trim().is_empty() {
            return;
        }
        let _ = self.conn.execute(
            "INSERT INTO history(kind, item, extra, time) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(kind, item) DO UPDATE SET time = excluded.time, extra = excluded.extra",
            params![kind, item, extra, now()],
        );
        let _ = self.conn.execute(
            "DELETE FROM history WHERE kind = ?1 AND locked = 0 AND item NOT IN
               (SELECT item FROM history WHERE kind = ?1 ORDER BY time DESC LIMIT ?2)",
            params![kind, self.max_history as i64],
        );
    }

    /// Items newest first: (item, extra, locked).
    pub fn history(&self, kind: &str) -> Vec<(String, String, bool)> {
        let Ok(mut st) = self.conn.prepare_cached("SELECT item, extra, locked FROM history WHERE kind = ?1 ORDER BY time DESC") else {
            return Vec::new();
        };
        st.query_map(params![kind], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0))).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    pub fn delete_history_item(&self, kind: &str, item: &str) {
        let _ = self.conn.execute("DELETE FROM history WHERE kind = ?1 AND item = ?2", params![kind, item]);
    }

    pub fn toggle_lock(&self, kind: &str, item: &str) {
        let _ = self.conn.execute("UPDATE history SET locked = 1 - locked WHERE kind = ?1 AND item = ?2", params![kind, item]);
    }

    pub fn clear_history(&self, kind: &str) {
        let _ = self.conn.execute("DELETE FROM history WHERE kind = ?1 AND locked = 0", params![kind]);
    }

    pub fn save_pos(&self, path: &str, p: &FilePos) {
        let _ = self.conn.execute(
            "INSERT OR REPLACE INTO positions(path, line, col, top, left_col, codepage, bookmarks, time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![path, p.line as i64, p.col as i64, p.top as i64, p.left as i64, p.codepage, p.bookmarks, now()],
        );
    }

    pub fn load_pos(&self, path: &str) -> Option<FilePos> {
        self.conn
            .query_row("SELECT line, col, top, left_col, codepage, bookmarks FROM positions WHERE path = ?1", params![path], |r| {
                Ok(FilePos {
                    line: r.get::<_, i64>(0)? as u64,
                    col: r.get::<_, i64>(1)? as u64,
                    top: r.get::<_, i64>(2)? as u64,
                    left: r.get::<_, i64>(3)? as u64,
                    codepage: r.get(4)?,
                    bookmarks: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                })
            })
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_shortcut(&self, slot: u8, path: &str) {
        let _ = self.conn.execute("INSERT OR REPLACE INTO shortcuts(slot, path) VALUES (?1, ?2)", params![slot, path]);
    }

    pub fn shortcut(&self, slot: u8) -> Option<String> {
        self.conn.query_row("SELECT path FROM shortcuts WHERE slot = ?1", params![slot], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn shortcuts(&self) -> Vec<(u8, String)> {
        let Ok(mut st) = self.conn.prepare("SELECT slot, path FROM shortcuts ORDER BY slot") else {
            return Vec::new();
        };
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_order_and_limit() {
        let mut db = StateDb::in_memory();
        db.max_history = 3;
        for c in ["a", "b", "c", "d"] {
            db.add_history("cmd", c, "");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        db.add_history("cmd", "b", "");
        let h: Vec<String> = db.history("cmd").into_iter().map(|x| x.0).collect();
        assert_eq!(h, ["b", "d", "c"]);
        db.clear_history("cmd");
        assert!(db.history("cmd").is_empty());
    }

    #[test]
    fn positions_and_shortcuts() {
        let db = StateDb::in_memory();
        let p = FilePos { line: 10, col: 2, top: 5, left: 0, codepage: 65001, bookmarks: "1:0".into() };
        db.save_pos("/x", &p);
        assert_eq!(db.load_pos("/x"), Some(p));
        db.set_shortcut(3, "/tmp");
        assert_eq!(db.shortcut(3).as_deref(), Some("/tmp"));
    }
}

//! One connection to MariaDB or PostgreSQL behind a common interface:
//! every value comes back as text.

use crate::config::{Driver, Params};
use crate::sql::{Outcome, Table};
use mysql::prelude::Queryable;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Why connecting failed.
#[derive(Debug)]
pub struct ConnectError {
    pub message: String,
    /// The server refused the password (or wants one): asking for it may help.
    pub password: bool,
}

pub enum Db {
    My(mysql::Conn),
    Pg(Box<postgres::Client>),
}

pub fn connect(p: &Params) -> Result<Db, ConnectError> {
    match p.driver {
        Driver::MariaDb => {
            let opts = mysql::OptsBuilder::new()
                .ip_or_hostname(Some(p.host.clone()))
                .tcp_port(p.port)
                .user(Some(p.user.clone()))
                .pass(Some(p.password.clone()).filter(|s| !s.is_empty()))
                .db_name(Some(p.db.clone()))
                .prefer_socket(false)
                .tcp_connect_timeout(Some(CONNECT_TIMEOUT));
            match mysql::Conn::new(opts) {
                Ok(c) => Ok(Db::My(c)),
                Err(e) => {
                    // ER_ACCESS_DENIED_ERROR, ER_ACCESS_DENIED_NO_PASSWORD_ERROR.
                    let password = matches!(&e, mysql::Error::MySqlError(m) if m.code == 1045 || m.code == 1698);
                    Err(ConnectError { message: my_err(&e), password })
                }
            }
        }
        Driver::PostgreSql => {
            let mut cfg = postgres::Config::new();
            cfg.host(&p.host).port(p.port).dbname(&p.db).user(&p.user).connect_timeout(CONNECT_TIMEOUT).application_name("gm");
            if !p.password.is_empty() {
                cfg.password(&p.password);
            }
            match cfg.connect(postgres::NoTls) {
                Ok(mut c) => {
                    // The literals gm writes double quotes only.
                    let _ = c.simple_query("SET standard_conforming_strings = on");
                    Ok(Db::Pg(Box::new(c)))
                }
                Err(e) => {
                    let password = e.code() == Some(&postgres::error::SqlState::INVALID_PASSWORD) || e.to_string().contains("password missing");
                    Err(ConnectError { message: pg_err(&e), password })
                }
            }
        }
    }
}

fn my_err(e: &mysql::Error) -> String {
    match e {
        mysql::Error::MySqlError(m) => format!("ERROR {} ({}): {}", m.code, m.state, m.message),
        e => e.to_string(),
    }
}

fn pg_err(e: &postgres::Error) -> String {
    let Some(d) = e.as_db_error() else {
        // "error connecting to server: …" and the like.
        let mut s = e.to_string();
        let mut src = std::error::Error::source(e);
        while let Some(x) = src {
            let t = x.to_string();
            if !s.contains(&t) {
                s.push_str(": ");
                s.push_str(&t);
            }
            src = x.source();
        }
        return s;
    };
    let mut s = format!("{}: {}", d.severity(), d.message());
    if let Some(x) = d.detail() {
        s.push_str(&format!("\nDETAIL: {x}"));
    }
    if let Some(x) = d.hint() {
        s.push_str(&format!("\nHINT: {x}"));
    }
    if let Some(postgres::error::ErrorPosition::Original(p)) = d.position() {
        s.push_str(&format!("\nPOSITION: {p}"));
    }
    s
}

fn my_value(v: mysql::Value) -> Option<String> {
    use mysql::Value as V;
    Some(match v {
        V::NULL => return None,
        V::Bytes(b) => match String::from_utf8(b) {
            Ok(s) => s,
            Err(e) => {
                let b = e.into_bytes();
                let mut s = String::from("0x");
                for x in b.iter().take(256) {
                    s.push_str(&format!("{x:02X}"));
                }
                if b.len() > 256 {
                    s.push('…');
                }
                s
            }
        },
        V::Int(i) => i.to_string(),
        V::UInt(u) => u.to_string(),
        V::Float(f) => f.to_string(),
        V::Double(d) => d.to_string(),
        V::Date(y, mo, d, h, mi, s, us) => {
            let mut t = format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}");
            if us > 0 {
                t.push_str(&format!(".{us:06}"));
            }
            t
        }
        V::Time(neg, d, h, mi, s, us) => {
            let mut t = format!("{}{:02}:{mi:02}:{s:02}", if neg { "-" } else { "" }, d * 24 + u32::from(h));
            if us > 0 {
                t.push_str(&format!(".{us:06}"));
            }
            t
        }
    })
}

/// Reads all result sets of a MariaDB query.
fn my_collect<P: mysql::prelude::Protocol>(mut r: mysql::QueryResult<'_, '_, '_, P>) -> Result<Vec<Outcome>, String> {
    let mut out = Vec::new();
    while let Some(set) = r.iter() {
        let columns: Vec<String> = set.columns().as_ref().iter().map(|c| c.name_str().into_owned()).collect();
        if columns.is_empty() {
            out.push(Outcome::Affected(set.affected_rows()));
            continue;
        }
        let mut t = Table { columns, rows: Vec::new() };
        for row in set {
            let row = row.map_err(|e| my_err(&e))?;
            t.rows.push(row.unwrap().into_iter().map(my_value).collect());
        }
        out.push(Outcome::Rows(t));
    }
    Ok(out)
}

impl Db {
    /// False when the server closed the connection (idle timeout, restart…).
    pub fn alive(&mut self) -> bool {
        match self {
            Db::My(c) => c.ping().is_ok(),
            Db::Pg(c) => !c.is_closed() && c.is_valid(Duration::from_secs(10)).is_ok(),
        }
    }

    /// Runs one statement as it is (text protocol).
    pub fn run(&mut self, sql: &str) -> Result<Vec<Outcome>, String> {
        match self {
            Db::My(c) => {
                let r = c.query_iter(sql).map_err(|e| my_err(&e))?;
                my_collect(r)
            }
            Db::Pg(c) => {
                use postgres::SimpleQueryMessage as M;
                let msgs = c.simple_query(sql).map_err(|e| pg_err(&e))?;
                let mut out = Vec::new();
                let mut cur: Option<Table> = None;
                for m in msgs {
                    match m {
                        M::RowDescription(cols) => {
                            cur = Some(Table { columns: cols.iter().map(|c| c.name().to_string()).collect(), rows: Vec::new() });
                        }
                        M::Row(r) => {
                            let t = cur.get_or_insert_with(|| Table { columns: r.columns().iter().map(|c| c.name().to_string()).collect(), rows: Vec::new() });
                            t.rows.push((0..r.len()).map(|i| r.get(i).map(str::to_string)).collect());
                        }
                        M::CommandComplete(n) => match cur.take() {
                            Some(t) => out.push(Outcome::Rows(t)),
                            None => out.push(Outcome::Affected(n)),
                        },
                        _ => {}
                    }
                }
                if let Some(t) = cur {
                    out.push(Outcome::Rows(t));
                }
                Ok(out)
            }
        }
    }

    /// The first result set of a query gm writes itself. MariaDB binds
    /// `params` to `?`; PostgreSQL queries carry their literals already.
    pub fn rows(&mut self, sql: &str, params: &[&str]) -> Result<Table, String> {
        let list = match self {
            Db::My(c) if !params.is_empty() => {
                let p = mysql::Params::Positional(params.iter().map(|s| mysql::Value::from(*s)).collect());
                let r = c.exec_iter(sql, p).map_err(|e| my_err(&e))?;
                my_collect(r)?
            }
            _ => self.run(sql)?,
        };
        Ok(list
            .into_iter()
            .find_map(|o| match o {
                Outcome::Rows(t) => Some(t),
                Outcome::Affected(_) => None,
            })
            .unwrap_or_default())
    }
}

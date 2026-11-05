//! Saved database configurations (`db.toml` in the gm profile).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The database servers gm can talk to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Driver {
    #[default]
    #[serde(rename = "mariadb")]
    MariaDb,
    #[serde(rename = "postgresql")]
    PostgreSql,
}

impl Driver {
    pub const ALL: [Driver; 2] = [Driver::MariaDb, Driver::PostgreSql];

    /// The name in the driver list of the dialog.
    pub fn label(self) -> &'static str {
        match self {
            Driver::MariaDb => "MariaDB (>= 10.0)",
            Driver::PostgreSql => "PostgreSQL",
        }
    }

    /// A short name for titles.
    pub fn short(self) -> &'static str {
        match self {
            Driver::MariaDb => "MariaDB",
            Driver::PostgreSql => "PostgreSQL",
        }
    }

    pub fn from_label(s: &str) -> Option<Driver> {
        Driver::ALL.into_iter().find(|d| d.label() == s)
    }

    pub fn default_port(self) -> u16 {
        match self {
            Driver::MariaDb => 3306,
            Driver::PostgreSql => 5432,
        }
    }
}

/// One saved configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedDb {
    /// `host[:port]` (`[ipv6]:port`).
    pub host: String,
    #[serde(default)]
    pub db: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user: String,
    /// Base64 in the file: this only keeps it from being read at a glance,
    /// it is not encryption. The file is private to the user on Unix.
    #[serde(default, skip_serializing_if = "String::is_empty", with = "obfuscated")]
    pub password: String,
    #[serde(default)]
    pub driver: Driver,
}

mod obfuscated {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &str, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&data_encoding::BASE64.encode(v.as_bytes()))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
        let s = String::deserialize(d)?;
        let bytes = data_encoding::BASE64.decode(s.as_bytes()).map_err(serde::de::Error::custom)?;
        String::from_utf8(bytes).map_err(serde::de::Error::custom)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    database: Vec<SavedDb>,
}

pub fn store_path() -> PathBuf {
    gm_config::profile_dir().join("db.toml")
}

pub fn load(path: &Path) -> Result<Vec<SavedDb>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => toml::from_str::<ConfigFile>(&s).map(|f| f.database).map_err(|e| format!("{}\n{e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}\n{e}", path.display())),
    }
}

pub fn save(path: &Path, list: &[SavedDb]) -> Result<(), String> {
    let text = toml::to_string(&ConfigFile { database: list.to_vec() }).map_err(|e| e.to_string())?;
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = path.with_extension("toml.tmp");
    let write = || -> std::io::Result<()> {
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        std::io::Write::write_all(&mut o.open(&tmp)?, text.as_bytes())?;
        std::fs::rename(&tmp, path)
    };
    write().map_err(|e| format!("{}\n{e}", path.display()))
}

/// The local login name: the user when none is given.
pub fn local_user() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default()
}

/// Everything needed to connect (and to connect again when the server
/// closed the connection).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Params {
    pub driver: Driver,
    pub host: String,
    pub port: u16,
    pub db: String,
    pub user: String,
    /// Empty: none (the server may then ask for one).
    pub password: String,
}

impl Params {
    /// `PostgreSQL user@host/db`.
    pub fn label(&self) -> String {
        let port = if self.port == self.driver.default_port() { String::new() } else { format!(":{}", self.port) };
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        format!("{} {}@{host}{port}/{}", self.driver.short(), self.user, self.db)
    }
}

/// Splits `host[:port]` / `[ipv6]:port`; an empty host is `localhost`.
pub fn parse_host(s: &str, default_port: u16) -> Result<(String, u16), String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(("localhost".into(), default_port));
    }
    let (host, port) = if let Some(r) = s.strip_prefix('[') {
        let end = r.find(']').ok_or_else(|| format!("Bad host name: {s}"))?;
        (&r[..end], r[end + 1..].strip_prefix(':'))
    } else if s.matches(':').count() > 1 {
        // A bare IPv6 address.
        (s, None)
    } else {
        match s.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (s, None),
        }
    };
    if host.is_empty() {
        return Err(format!("No host name in \"{s}\""));
    }
    let port = match port {
        Some(p) => p.trim().parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| format!("Bad port in \"{s}\""))?,
        None => default_port,
    };
    Ok((host.to_string(), port))
}

/// The connection parameters of a saved configuration.
pub fn resolve(c: &SavedDb) -> Result<Params, String> {
    let (host, port) = parse_host(&c.host, c.driver.default_port())?;
    let user = if c.user.trim().is_empty() { local_user() } else { c.user.trim().to_string() };
    if user.is_empty() {
        return Err("No user name".into());
    }
    let db = match (c.db.trim(), c.driver) {
        ("", Driver::MariaDb) => return Err("No database name".into()),
        // PostgreSQL's default database has the name of the user.
        ("", Driver::PostgreSql) => user.clone(),
        (d, _) => d.to_string(),
    };
    Ok(Params { driver: c.driver, host, port, db, user, password: c.password.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(parse_host("", 5432).unwrap(), ("localhost".into(), 5432));
        assert_eq!(parse_host("db.example.com", 3306).unwrap(), ("db.example.com".into(), 3306));
        assert_eq!(parse_host("db:3307", 3306).unwrap(), ("db".into(), 3307));
        assert_eq!(parse_host("[::1]:5433", 5432).unwrap(), ("::1".into(), 5433));
        assert_eq!(parse_host("::1", 5432).unwrap(), ("::1".into(), 5432));
        assert!(parse_host("db:x", 5432).is_err());
        assert!(parse_host(":5432", 5432).is_err());
    }

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("gm-db-cfg-{}", std::process::id()));
        let p = dir.join("db.toml");
        let list = vec![
            SavedDb { host: "h:1".into(), db: "d".into(), user: "u".into(), password: "p w".into(), driver: Driver::PostgreSql },
            SavedDb { host: "x".into(), db: "y".into(), ..Default::default() },
        ];
        save(&p, &list).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("postgresql") && !text.contains("p w"), "{text}");
        assert_eq!(load(&p).unwrap(), list);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolving() {
        let c = SavedDb { host: "h".into(), user: "u".into(), driver: Driver::PostgreSql, ..Default::default() };
        let p = resolve(&c).unwrap();
        assert_eq!((p.db.as_str(), p.port), ("u", 5432));
        assert_eq!(p.label(), "PostgreSQL u@h/u");
        let c = SavedDb { host: "h".into(), user: "u".into(), driver: Driver::MariaDb, ..Default::default() };
        assert!(resolve(&c).is_err());
    }
}

//! Saved connections (`ssh.toml` in the gm profile) and how a connection
//! is resolved the way the `ssh` program does it: `~/.ssh/config`
//! (`Host`, `HostName`, `User`, `Port`, `IdentityFile`), the local user
//! name when no user is given, and the default key files.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One saved connection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedConn {
    /// `[user@]host[:port][/remote/dir]` or `host:remote/dir` (scp style).
    pub machine: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user: String,
    /// Base64 in the file: this only keeps it from being read at a glance,
    /// it is not encryption. The file is private to the user on Unix.
    #[serde(default, skip_serializing_if = "String::is_empty", with = "obfuscated")]
    pub password: String,
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
    connection: Vec<SavedConn>,
}

pub fn store_path() -> PathBuf {
    gm_config::profile_dir().join("ssh.toml")
}

pub fn load(path: &Path) -> Result<Vec<SavedConn>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => toml::from_str::<ConfigFile>(&s).map(|f| f.connection).map_err(|e| format!("{}\n{e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}\n{e}", path.display())),
    }
}

pub fn save(path: &Path, list: &[SavedConn]) -> Result<(), String> {
    let text = toml::to_string(&ConfigFile { connection: list.to_vec() }).map_err(|e| e.to_string())?;
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

/// The parts of a machine string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Machine {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    /// The starting remote folder: absolute, relative to the home folder, or empty.
    pub dir: String,
}

/// Parses `[ssh://][user@]host[:port][/dir]`, `host:dir` and `[ipv6]:port/dir`.
pub fn parse_machine(s: &str) -> Result<Machine, String> {
    let s = s.trim();
    let s = s.strip_prefix("ssh://").or_else(|| s.strip_prefix("sftp://")).unwrap_or(s);
    let mut m = Machine::default();
    let first_slash = s.find('/').unwrap_or(s.len());
    let rest = match s[..first_slash].rfind('@') {
        Some(i) => {
            m.user = Some(s[..i].to_string()).filter(|u| !u.is_empty());
            &s[i + 1..]
        }
        None => s,
    };
    let after_host = if let Some(r) = rest.strip_prefix('[') {
        let end = r.find(']').ok_or_else(|| format!("Bad machine name: {s}"))?;
        m.host = r[..end].to_string();
        &r[end + 1..]
    } else {
        let end = rest.find([':', '/']).unwrap_or(rest.len());
        m.host = rest[..end].to_string();
        &rest[end..]
    };
    if m.host.is_empty() {
        return Err(format!("No host name in \"{s}\""));
    }
    if let Some(r) = after_host.strip_prefix(':') {
        let digits = r.find('/').unwrap_or(r.len());
        if digits > 0 && r[..digits].chars().all(|c| c.is_ascii_digit()) {
            m.port = Some(r[..digits].parse().map_err(|_| format!("Bad port in \"{s}\""))?);
            m.dir = r[digits..].to_string();
        } else {
            // scp style: host:dir (relative to the home folder unless absolute).
            m.dir = r.to_string();
        }
    } else {
        m.dir = after_host.to_string();
    }
    Ok(m)
}

/// The local user name (the default remote user of `ssh`).
pub fn local_user() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).or_else(|_| std::env::var("LOGNAME")).unwrap_or_default()
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

pub fn ssh_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".ssh"))
}

/// What the `ssh` program would use to connect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The name the user typed (the `Host` alias).
    pub alias: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub dir: String,
    /// `IdentityFile` entries first, then the default key files.
    pub identity_files: Vec<PathBuf>,
}

impl Target {
    pub fn label(&self) -> String {
        if self.port == 22 { format!("{}@{}", self.user, self.alias) } else { format!("{}@{}:{}", self.user, self.alias, self.port) }
    }
}

/// Settings found for a host in an ssh config file (first value wins).
#[derive(Default, Debug, PartialEq, Eq)]
pub struct HostConfig {
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_files: Vec<String>,
}

/// `*` and `?` wildcards, like ssh's `Host` patterns.
fn wildcard(pat: &str, s: &str) -> bool {
    let p: Vec<char> = pat.to_lowercase().chars().collect();
    let t: Vec<char> = s.to_lowercase().chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

fn host_matches(patterns: &[String], host: &str) -> bool {
    let mut matched = false;
    for p in patterns {
        if let Some(neg) = p.strip_prefix('!') {
            if wildcard(neg, host) {
                return false;
            }
        } else if wildcard(p, host) {
            matched = true;
        }
    }
    matched
}

/// Reads the settings for `host` from the text of an ssh config file.
pub fn parse_ssh_config(text: &str, host: &str) -> HostConfig {
    let mut c = HostConfig::default();
    // Lines before the first Host apply to every host.
    let mut active = true;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.find(|ch: char| ch.is_whitespace() || ch == '=') {
            Some(i) => (&line[..i], line[i..].trim_start_matches(|ch: char| ch.is_whitespace() || ch == '=').trim()),
            None => (line, ""),
        };
        let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value);
        match key.to_lowercase().as_str() {
            "host" => {
                let pats: Vec<String> = value.split_whitespace().map(String::from).collect();
                active = host_matches(&pats, host);
            }
            // Match blocks are not evaluated: they never apply.
            "match" => active = value.eq_ignore_ascii_case("all"),
            _ if !active => {}
            "hostname" if c.host_name.is_none() => c.host_name = Some(value.to_string()),
            "user" if c.user.is_none() => c.user = Some(value.to_string()),
            "port" if c.port.is_none() => c.port = value.parse().ok(),
            "identityfile" => c.identity_files.push(value.to_string()),
            _ => {}
        }
    }
    c
}

/// Expands `~` and the `%d %u %h %r %%` tokens of `IdentityFile`.
fn expand_identity(s: &str, host: &str, user: &str) -> PathBuf {
    let home = home_dir().unwrap_or_default();
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            match chars.next() {
                Some('d') => out.push_str(&home.display().to_string()),
                Some('u') => out.push_str(&local_user()),
                Some('h') => out.push_str(host),
                Some('r') => out.push_str(user),
                Some('%') => out.push('%'),
                Some(o) => {
                    out.push('%');
                    out.push(o);
                }
                None => out.push('%'),
            }
        } else {
            out.push(ch);
        }
    }
    match out.strip_prefix('~') {
        Some(r) if r.is_empty() || r.starts_with(['/', '\\']) => home.join(r.trim_start_matches(['/', '\\'])),
        _ => PathBuf::from(out),
    }
}

/// Resolves a saved connection like `ssh` would.
pub fn resolve(c: &SavedConn) -> Result<Target, String> {
    let m = parse_machine(&c.machine)?;
    let cfg_text = ssh_dir().and_then(|d| std::fs::read_to_string(d.join("config")).ok()).unwrap_or_default();
    Ok(resolve_with(&m, c.user.trim(), &cfg_text))
}

pub fn resolve_with(m: &Machine, user_field: &str, ssh_config: &str) -> Target {
    let hc = parse_ssh_config(ssh_config, &m.host);
    // The User field, then user@ in the machine name, then ~/.ssh/config, then the local user.
    let user = Some(user_field.to_string()).filter(|u| !u.is_empty()).or_else(|| m.user.clone()).or(hc.user.clone()).unwrap_or_else(local_user);
    let host = hc.host_name.clone().map(|h| h.replace("%h", &m.host)).unwrap_or_else(|| m.host.clone());
    let port = m.port.or(hc.port).unwrap_or(22);
    let mut identity_files: Vec<PathBuf> = hc.identity_files.iter().map(|f| expand_identity(f, &host, &user)).collect();
    if let Some(d) = ssh_dir() {
        for f in ["id_rsa", "id_ecdsa", "id_ed25519", "id_dsa"] {
            let p = d.join(f);
            if !identity_files.contains(&p) {
                identity_files.push(p);
            }
        }
    }
    Target { alias: m.host.clone(), host, port, user, dir: m.dir.clone(), identity_files }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_strings() {
        let m = parse_machine("example.com").unwrap();
        assert_eq!((m.user, m.host.as_str(), m.port, m.dir.as_str()), (None, "example.com", None, ""));
        let m = parse_machine("bob@example.com:2222/var/www").unwrap();
        assert_eq!((m.user.as_deref(), m.host.as_str(), m.port, m.dir.as_str()), (Some("bob"), "example.com", Some(2222), "/var/www"));
        let m = parse_machine("example.com/var/www").unwrap();
        assert_eq!((m.host.as_str(), m.port, m.dir.as_str()), ("example.com", None, "/var/www"));
        let m = parse_machine("example.com:projects/app").unwrap();
        assert_eq!((m.host.as_str(), m.port, m.dir.as_str()), ("example.com", None, "projects/app"));
        let m = parse_machine("example.com:/etc").unwrap();
        assert_eq!((m.port, m.dir.as_str()), (None, "/etc"));
        let m = parse_machine("ssh://[::1]:22/tmp").unwrap();
        assert_eq!((m.host.as_str(), m.port, m.dir.as_str()), ("::1", Some(22), "/tmp"));
        let m = parse_machine("host/home/a@b").unwrap();
        assert_eq!((m.user, m.host.as_str(), m.dir.as_str()), (None, "host", "/home/a@b"));
        assert!(parse_machine("").is_err());
        assert!(parse_machine("bob@").is_err());
    }

    #[test]
    fn ssh_config_rules() {
        let cfg = "User everyone\n\nHost web *.lan !bad.lan\n  HostName 10.0.0.%h\n  Port 2200\n  IdentityFile ~/.ssh/web_key\n\nHost *\n  User fallback\n  Port 22\n";
        let c = parse_ssh_config(cfg, "web");
        assert_eq!(c.host_name.as_deref(), Some("10.0.0.%h"));
        assert_eq!(c.port, Some(2200));
        assert_eq!(c.user.as_deref(), Some("everyone"));
        assert_eq!(c.identity_files, vec!["~/.ssh/web_key".to_string()]);
        let c = parse_ssh_config(cfg, "bad.lan");
        assert_eq!(c.port, Some(22));
        assert!(c.host_name.is_none());

        let m = parse_machine("web/srv").unwrap();
        let t = resolve_with(&m, "", "Host web\n HostName web.example.com\n User admin\n Port 2022\n");
        assert_eq!((t.host.as_str(), t.port, t.user.as_str(), t.dir.as_str()), ("web.example.com", 2022, "admin", "/srv"));
        // The User field wins over the config file and user@ in the machine name.
        let m = parse_machine("joe@web").unwrap();
        assert_eq!(resolve_with(&m, "ann", "Host web\n User admin\n").user, "ann");
        assert_eq!(resolve_with(&m, "", "Host web\n User admin\n").user, "joe");
        let m = parse_machine("other").unwrap();
        assert_eq!(resolve_with(&m, "", "Host web\n User admin\n").user, local_user());
        assert_eq!(resolve_with(&m, "", "").port, 22);
    }

    #[test]
    fn wildcards() {
        assert!(wildcard("*.example.com", "a.example.com"));
        assert!(wildcard("h?st", "HOST"));
        assert!(!wildcard("*.example.com", "example.com"));
        assert!(wildcard("*", ""));
    }

    #[test]
    fn saved_round_trip() {
        let dir = std::env::temp_dir().join(format!("gm-ssh-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("ssh.toml");
        let list = vec![
            SavedConn { machine: "a.example.com/srv".into(), user: "bob".into(), password: "pä$$ \"w\"".into() },
            SavedConn { machine: "b".into(), ..Default::default() },
        ];
        save(&p, &list).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("pä$$"));
        assert_eq!(load(&p).unwrap(), list);
        assert_eq!(load(&dir.join("missing.toml")).unwrap(), Vec::new());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

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
    /// Optional SSH tunnel, the value of `ssh -J` / `ProxyJump`: jump hosts
    /// `[user@]host[:port]` separated by commas, or `none`. When set, it
    /// replaces the `ProxyJump` of `~/.ssh/config` for `machine`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tunnel_machine: String,
    /// The user for the jump hosts of `tunnel_machine` without `user@`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tunnel_user: String,
    /// The password tried first on the jump hosts of `tunnel_machine`.
    #[serde(default, skip_serializing_if = "String::is_empty", with = "obfuscated")]
    pub tunnel_password: String,
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
    /// `ProxyCommand` with its tokens expanded: a local command whose
    /// stdin/stdout carry the connection instead of a TCP socket.
    pub proxy_command: Option<String>,
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
    /// `ProxyJump` (raw value; `none` included). Like ssh, only the first
    /// of `ProxyJump` and `ProxyCommand` is used.
    pub proxy_jump: Option<String>,
    pub proxy_command: Option<String>,
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
            "proxyjump" if c.proxy_jump.is_none() && c.proxy_command.is_none() => c.proxy_jump = Some(value.to_string()),
            "proxycommand" if c.proxy_jump.is_none() && c.proxy_command.is_none() => c.proxy_command = Some(value.to_string()),
            _ => {}
        }
    }
    c
}

/// The local machine name (`%l`).
fn local_host() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default()
}

/// The values of the `%` tokens of ssh's config.
struct Tokens<'a> {
    /// `%h`: the real host name (after `HostName`).
    host: &'a str,
    /// `%n`: the name as given (the `Host` alias).
    alias: &'a str,
    port: u16,
    /// `%r`: the remote user.
    user: &'a str,
}

/// Expands `%h %n %p %r %u %d %l %%`; other tokens are kept as they are.
fn expand_tokens(s: &str, t: &Tokens) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('h') => out.push_str(t.host),
            Some('n') => out.push_str(t.alias),
            Some('p') => out.push_str(&t.port.to_string()),
            Some('r') => out.push_str(t.user),
            Some('u') => out.push_str(&local_user()),
            Some('d') => out.push_str(&home_dir().unwrap_or_default().display().to_string()),
            Some('l') => out.push_str(&local_host()),
            Some('%') => out.push('%'),
            Some(o) => {
                out.push('%');
                out.push(o);
            }
            None => out.push('%'),
        }
    }
    out
}

/// Expands `~` and the `%` tokens of `IdentityFile`.
fn expand_identity(s: &str, t: &Tokens) -> PathBuf {
    let home = home_dir().unwrap_or_default();
    let out = expand_tokens(s, t);
    match out.strip_prefix('~') {
        Some(r) if r.is_empty() || r.starts_with(['/', '\\']) => home.join(r.trim_start_matches(['/', '\\'])),
        _ => PathBuf::from(out),
    }
}

fn read_ssh_config() -> String {
    ssh_dir().and_then(|d| std::fs::read_to_string(d.join("config")).ok()).unwrap_or_default()
}

/// Resolves a saved connection like `ssh` would (without its jump hosts).
#[cfg_attr(not(test), allow(dead_code))]
pub fn resolve(c: &SavedConn) -> Result<Target, String> {
    let m = parse_machine(&c.machine)?;
    Ok(resolve_with(&m, c.user.trim(), &read_ssh_config()))
}

/// One step of the way to a machine: a jump host or the machine itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hop {
    pub target: Target,
    /// Tried before the keys; empty: agent, keys, then ask.
    pub password: String,
}

/// More jump hosts than this means a `ProxyJump` loop in `~/.ssh/config`.
const MAX_HOPS: usize = 16;

/// Parses a `ProxyJump` / `ssh -J` value: `none`, or jump hosts
/// `[user@]host[:port]` or `ssh://[user@]host[:port]` separated by commas.
pub fn parse_jumps(s: &str) -> Result<Vec<Machine>, String> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    s.split(',')
        .map(|j| {
            let m = parse_machine(j)?;
            if m.dir.trim_end_matches('/').is_empty() { Ok(m) } else { Err(format!("Bad jump host \"{}\": use [user@]host[:port]", j.trim())) }
        })
        .collect()
}

/// The whole way to a saved connection, like `ssh` (with `-J` when the
/// tunnel field is set): the jump hosts in the order they are connected,
/// then the machine itself (always last).
pub fn route(c: &SavedConn) -> Result<Vec<Hop>, String> {
    route_with(c, &read_ssh_config())
}

pub fn route_with(c: &SavedConn, ssh_config: &str) -> Result<Vec<Hop>, String> {
    let m = parse_machine(&c.machine)?;
    let explicit = if c.tunnel_machine.trim().is_empty() { None } else { Some(parse_jumps(&c.tunnel_machine).map_err(|e| format!("Tunnel: {e}"))?) };
    let mut route = Vec::new();
    let explicit = explicit.as_ref().map(|j| (j.as_slice(), c.tunnel_user.trim(), c.tunnel_password.as_str()));
    add_hops(&m, c.user.trim(), &c.password, explicit, ssh_config, 0, &mut route)?;
    Ok(route)
}

/// Adds the way to `m` and then `m`. The jump hosts are `explicit` (with
/// their user and password) or the `ProxyJump` of `m` in the config. Like
/// `ssh -J a,b m`: `a` is reached the way its own config says, `b`
/// through `a`, and `m` through `b`.
fn add_hops(
    m: &Machine,
    user_field: &str,
    password: &str,
    explicit: Option<(&[Machine], &str, &str)>,
    ssh_config: &str,
    depth: usize,
    route: &mut Vec<Hop>,
) -> Result<(), String> {
    if depth > MAX_HOPS || route.len() > MAX_HOPS {
        return Err(format!("Too many jump hosts on the way to {} (a ProxyJump loop in ~/.ssh/config?)", m.host));
    }
    let from_config;
    let (jumps, jump_user, jump_password) = match explicit {
        Some(e) => e,
        None => {
            // A ProxyCommand (instead of ProxyJump) is kept in the target by resolve_with.
            let hc = parse_ssh_config(ssh_config, &m.host);
            from_config = parse_jumps(hc.proxy_jump.as_deref().unwrap_or("")).map_err(|e| format!("ProxyJump of {}: {e}", m.host))?;
            (from_config.as_slice(), "", "")
        }
    };
    // user@ in a jump host wins over the tunnel user.
    let user_of = |j: &Machine| if j.user.is_some() { "" } else { jump_user };
    if let Some((first, rest)) = jumps.split_first() {
        add_hops(first, user_of(first), jump_password, None, ssh_config, depth + 1, route)?;
        for j in rest {
            // Reached through the jump host before it: no ProxyCommand of its own.
            let target = Target { proxy_command: None, ..resolve_with(j, user_of(j), ssh_config) };
            route.push(Hop { target, password: jump_password.to_string() });
        }
    }
    let mut target = resolve_with(m, user_field, ssh_config);
    // Like ssh: a ProxyJump (also -J, even `-J none`) means no ProxyCommand.
    if explicit.is_some() || !jumps.is_empty() {
        target.proxy_command = None;
    }
    route.push(Hop { target, password: password.to_string() });
    Ok(())
}

pub fn resolve_with(m: &Machine, user_field: &str, ssh_config: &str) -> Target {
    let hc = parse_ssh_config(ssh_config, &m.host);
    // The User field, then user@ in the machine name, then ~/.ssh/config, then the local user.
    let user = Some(user_field.to_string()).filter(|u| !u.is_empty()).or_else(|| m.user.clone()).or(hc.user.clone()).unwrap_or_else(local_user);
    let host = hc.host_name.clone().map(|h| h.replace("%h", &m.host)).unwrap_or_else(|| m.host.clone());
    let port = m.port.or(hc.port).unwrap_or(22);
    let tokens = Tokens { host: &host, alias: &m.host, port, user: &user };
    let mut identity_files: Vec<PathBuf> = hc.identity_files.iter().map(|f| expand_identity(f, &tokens)).collect();
    let proxy_command =
        hc.proxy_command.as_deref().filter(|v| !v.trim().is_empty() && !v.eq_ignore_ascii_case("none")).map(|v| expand_tokens(v, &tokens));
    if let Some(d) = ssh_dir() {
        for f in ["id_rsa", "id_ecdsa", "id_ed25519", "id_dsa"] {
            let p = d.join(f);
            if !identity_files.contains(&p) {
                identity_files.push(p);
            }
        }
    }
    Target { alias: m.host.clone(), host, port, user, dir: m.dir.clone(), identity_files, proxy_command }
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
    fn jump_lists() {
        assert!(parse_jumps("").unwrap().is_empty());
        assert!(parse_jumps(" None ").unwrap().is_empty());
        let j = parse_jumps("a, bob@b:2200 ,ssh://ann@[::1]:22/").unwrap();
        let v: Vec<_> = j.iter().map(|m| (m.user.as_deref(), m.host.as_str(), m.port)).collect();
        assert_eq!(v, vec![(None, "a", None), (Some("bob"), "b", Some(2200)), (Some("ann"), "::1", Some(22))]);
        assert!(parse_jumps("a,").is_err());
        assert!(parse_jumps("a/srv").is_err());
        assert!(parse_jumps("a:folder").is_err());
    }

    fn hops(r: &[Hop]) -> Vec<(String, u16, String, String)> {
        r.iter().map(|h| (h.target.host.clone(), h.target.port, h.target.user.clone(), h.password.clone())).collect()
    }

    fn hop(host: &str, port: u16, user: &str, pw: &str) -> (String, u16, String, String) {
        (host.into(), port, user.into(), pw.into())
    }

    #[test]
    fn routes() {
        let conn = |machine: &str| SavedConn { machine: machine.into(), password: "pw".into(), ..Default::default() };
        // No ProxyJump: only the machine.
        assert_eq!(hops(&route_with(&conn("u@m"), "").unwrap()), vec![hop("m", 22, "u", "pw")]);

        // ProxyJump from the config, with the jump host resolved by its own Host block.
        let cfg = "Host bastion\n HostName b.example.com\n User gate\n Port 2200\n\nHost *.inner\n ProxyJump bastion\n User admin\n";
        assert_eq!(
            hops(&route_with(&conn("db.inner/srv"), cfg).unwrap()),
            vec![hop("b.example.com", 2200, "gate", ""), hop("db.inner", 22, "admin", "pw")]
        );
        assert_eq!(route_with(&conn("db.inner/srv"), cfg).unwrap().last().unwrap().target.dir, "/srv");

        // A chain, and the ProxyJump of the first jump host itself (recursive).
        let cfg = "Host target\n ProxyJump j1,joe@j2:2022\nHost j1\n ProxyJump j0\nHost j2\n ProxyJump ignored\n";
        assert_eq!(
            hops(&route_with(&conn("target"), cfg).unwrap()),
            vec![
                hop("j0", 22, &local_user(), ""),
                hop("j1", 22, &local_user(), ""),
                hop("j2", 2022, "joe", ""),
                hop("target", 22, &local_user(), "pw")
            ]
        );

        // The tunnel field replaces the config's ProxyJump (like ssh -J); `none` turns it off.
        let mut c = conn("target");
        c.tunnel_machine = "x, ann@y".into();
        c.tunnel_user = "tu".into();
        c.tunnel_password = "tp".into();
        assert_eq!(
            hops(&route_with(&c, cfg).unwrap()),
            vec![hop("x", 22, "tu", "tp"), hop("y", 22, "ann", "tp"), hop("target", 22, &local_user(), "pw")]
        );
        c.tunnel_machine = "none".into();
        assert_eq!(route_with(&c, cfg).unwrap().len(), 1);
        c.tunnel_machine = "bad/dir".into();
        assert!(route_with(&c, cfg).unwrap_err().starts_with("Tunnel:"));

        // Only the first of ProxyJump/ProxyCommand counts; tokens are expanded.
        let cfg = "Host a\n HostName a.example.com\n User ann\n ProxyCommand ssh -W %h:%p -l %r gate # %n %%\n ProxyJump j\n\
                   Host b\n ProxyJump j\n ProxyCommand nc %h %p\nHost c\n ProxyCommand none\nHost j\n ProxyCommand nc %h %p\n";
        let r = route_with(&conn("a"), cfg).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].target.proxy_command.as_deref(), Some("ssh -W a.example.com:22 -l ann gate # a %"));
        let r = route_with(&conn("b"), cfg).unwrap();
        // b goes through j; j itself is reached by its ProxyCommand.
        assert_eq!((r.len(), r[0].target.proxy_command.as_deref(), r[1].target.proxy_command.as_deref()), (2, Some("nc j 22"), None));
        assert_eq!(route_with(&conn("c"), cfg).unwrap()[0].target.proxy_command, None);
        // The tunnel field (like -J, even `none`) wins over a ProxyCommand.
        let mut c = conn("a");
        c.tunnel_machine = "none".into();
        assert_eq!(route_with(&c, cfg).unwrap()[0].target.proxy_command, None);
        c.tunnel_machine = "x,j".into();
        let r = route_with(&c, cfg).unwrap();
        assert!(r.iter().all(|h| h.target.proxy_command.is_none()), "{r:?}");

        // A loop stops with an error.
        let err = route_with(&conn("a"), "Host *\n ProxyJump bastion\n").unwrap_err();
        assert!(err.contains("loop"), "{err}");
    }

    #[test]
    fn old_files_load() {
        // Files written before tunnels existed still load.
        let f: ConfigFile = toml::from_str("[[connection]]\nmachine = \"h\"\n").unwrap();
        assert!(f.connection[0].tunnel_machine.is_empty());
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
            SavedConn { machine: "a.example.com/srv".into(), user: "bob".into(), password: "pä$$ \"w\"".into(), ..Default::default() },
            SavedConn { machine: "b".into(), ..Default::default() },
            SavedConn {
                machine: "inner/srv".into(),
                tunnel_machine: "jump.example.com:2222".into(),
                tunnel_user: "gate".into(),
                tunnel_password: "secret".into(),
                ..Default::default()
            },
        ];
        save(&p, &list).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("pä$$"));
        assert!(!text.contains("secret"));
        assert!(text.contains("tunnel_machine"));
        assert_eq!(load(&p).unwrap(), list);
        assert_eq!(load(&dir.join("missing.toml")).unwrap(), Vec::new());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

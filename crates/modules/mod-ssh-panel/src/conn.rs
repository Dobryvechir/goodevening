//! SSH connection and authentication (russh), driven synchronously from
//! the UI thread through one shared tokio runtime.

use crate::config::{Target, ssh_dir};
use russh::client::{self, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate};
use russh_sftp::client::SftpSession;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// The runtime that runs every connection (one worker thread keeps the
/// connections alive while the UI waits for keys).
pub fn rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("gm-ssh")
            .enable_all()
            .build()
            .expect("cannot start the SSH runtime")
    })
}

pub fn err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

/// What `known_hosts` says about the server's key.
#[derive(Clone, Debug)]
pub enum HostKey {
    Known,
    /// Not in any known_hosts file: the user is asked, like `ssh` does.
    Unknown(PublicKey),
    /// A different key is recorded: possible man-in-the-middle attack.
    Changed {
        file: PathBuf,
        line: usize,
        key: PublicKey,
    },
}

pub struct Client {
    host: String,
    port: u16,
    seen: Arc<Mutex<Option<HostKey>>>,
}

fn known_hosts_files() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = ssh_dir() {
        v.push(d.join("known_hosts"));
        v.push(d.join("known_hosts2"));
    }
    if cfg!(windows) {
        if let Some(pd) = std::env::var_os("ProgramData") {
            v.push(PathBuf::from(pd).join("ssh").join("ssh_known_hosts"));
        }
    } else {
        v.push(PathBuf::from("/etc/ssh/ssh_known_hosts"));
    }
    v
}

pub fn check_host_key(host: &str, port: u16, key: &PublicKey) -> HostKey {
    for f in known_hosts_files() {
        if !f.is_file() {
            continue;
        }
        match russh::keys::check_known_hosts_path(host, port, key, &f) {
            Ok(true) => return HostKey::Known,
            Ok(false) => {}
            Err(russh::keys::Error::KeyChanged { line }) => return HostKey::Changed { file: f, line, key: key.clone() },
            Err(_) => {}
        }
    }
    HostKey::Unknown(key.clone())
}

/// Adds the key to `~/.ssh/known_hosts` (the answer "yes" of `ssh`).
pub fn learn_host_key(host: &str, port: u16, key: &PublicKey) -> io::Result<()> {
    let dir = ssh_dir().ok_or_else(|| err("No home folder"))?;
    std::fs::create_dir_all(&dir)?;
    let name = if port == 22 { host.to_string() } else { format!("[{host}]:{port}") };
    let mut key = key.clone();
    key.set_comment("");
    let line = format!("{name} {}\n", key.to_openssh().map_err(err)?.trim());
    let path = dir.join("known_hosts");
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    // Start on a new line if the file does not end with one.
    let len = f.metadata()?.len();
    if len > 0 {
        let text = std::fs::read(&path)?;
        if text.last() != Some(&b'\n') {
            io::Write::write_all(&mut f, b"\n")?;
        }
    }
    io::Write::write_all(&mut f, line.as_bytes())
}

pub fn fingerprint(key: &PublicKey) -> String {
    format!("{} {}", key.algorithm().as_str(), key.fingerprint(HashAlg::Sha256))
}

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let pk = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(c) => PublicKey::from(c.public_key().clone()),
        };
        let state = check_host_key(&self.host, self.port, &pk);
        // A changed key stops here; an unknown one is decided by the user
        // before any credentials are sent.
        let ok = !matches!(state, HostKey::Changed { .. });
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(state);
        Ok(ok)
    }
}

/// The local command of `ProxyCommand`: the SSH connection runs over its
/// stdin/stdout. It is killed when the connection is dropped.
pub struct Proxy {
    child: Option<tokio::process::Child>,
    /// The end of what the command wrote to stderr (shown on errors).
    stderr: Arc<Mutex<Vec<u8>>>,
}

impl Proxy {
    /// Starts `command` through the shell, like ssh does: `$SHELL -c "exec …"`
    /// on Unix, `cmd /c` on Windows. Must run inside the runtime.
    fn spawn(command: &str) -> io::Result<(Proxy, impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static)> {
        #[cfg(windows)]
        let mut cmd = {
            let mut c = tokio::process::Command::new(std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into()));
            // /S: everything between the outer quotes is the command, as typed.
            c.raw_arg(format!("/D /S /C \"{command}\""));
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let shell = std::env::var_os("SHELL").filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into());
            let mut c = tokio::process::Command::new(shell);
            c.arg("-c").arg(format!("exec {command}"));
            c
        };
        cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| err(format!("Cannot start the ProxyCommand\n{command}\n{e}")))?;
        let (Some(stdin), Some(stdout), Some(mut stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
            return Err(err("The ProxyCommand has no stdin/stdout"));
        };
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        tokio::spawn(async move {
            let mut chunk = [0u8; 1024];
            while let Ok(n @ 1..) = tokio::io::AsyncReadExt::read(&mut stderr, &mut chunk).await {
                let mut b = sink.lock().unwrap_or_else(|e| e.into_inner());
                b.extend_from_slice(&chunk[..n]);
                let excess = b.len().saturating_sub(4096);
                b.drain(..excess);
            }
        });
        Ok((Proxy { child: Some(child), stderr: buf }, tokio::io::join(stdout, stdin)))
    }

    fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr.lock().unwrap_or_else(|e| e.into_inner())).trim().to_string()
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            // Killing and reaping the process needs the runtime.
            let _g = rt().enter();
            let _ = c.start_kill();
            drop(c);
        }
    }
}

/// An authenticated connection to a jump host (`ProxyJump`): the next
/// connection runs inside it through a `direct-tcpip` channel. A jump
/// host can itself be reached through another one (`via`).
pub struct Tunnel {
    handle: Handle<Client>,
    label: String,
    /// The tunnel this one goes through; closed after it (fields drop
    /// after `Drop::drop`).
    via: Option<Arc<Tunnel>>,
    /// The ProxyCommand this one goes through (first jump host only).
    _proxy: Option<Proxy>,
}

impl Tunnel {
    /// The jump hosts in connection order: `a, b, c`.
    pub fn label(&self) -> String {
        match &self.via {
            Some(v) => format!("{}, {}", v.label(), self.label),
            None => self.label.clone(),
        }
    }

    /// True if this connection or any one it goes through is closed.
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed() || self.via.as_ref().is_some_and(|v| v.is_closed())
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = rt().block_on(async {
            tokio::time::timeout(Duration::from_secs(2), self.handle.disconnect(russh::Disconnect::ByApplication, "", "en")).await
        });
    }
}

/// A connection that is not authenticated yet.
pub struct Pending {
    pub handle: Handle<Client>,
    pub host_key: HostKey,
    /// The tunnel this connection goes through (kept open while it lives).
    tunnel: Option<Arc<Tunnel>>,
    /// The ProxyCommand the connection runs over.
    proxy: Option<Proxy>,
}

/// The result of one key file.
pub enum KeyResult {
    Accepted,
    Rejected,
    /// The key needs a passphrase (or the given one is wrong).
    Encrypted,
    /// The file does not exist.
    Missing,
    /// The file could not be used (unsupported format…).
    Unusable,
}

/// Connects to `t` through `tunnel` when given, otherwise over its
/// ProxyCommand if it has one, otherwise directly.
pub fn connect(t: &Target, tunnel: Option<&Arc<Tunnel>>) -> io::Result<Pending> {
    let seen = Arc::new(Mutex::new(None));
    let handler = Client { host: t.host.clone(), port: t.port, seen: seen.clone() };
    let config =
        Arc::new(client::Config { keepalive_interval: Some(Duration::from_secs(30)), keepalive_max: 4, nodelay: true, ..Default::default() });
    let addr = (t.host.clone(), t.port);
    let jump = tunnel;
    let mut proxy: Option<Proxy> = None;
    let proxy_slot = &mut proxy;
    let r = rt().block_on(async move {
        let handshake = |r: Result<Result<Handle<Client>, russh::Error>, _>| {
            r.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "The server does not answer"))?.map_err(err)
        };
        match jump {
            None if t.proxy_command.is_some() => {
                let command = t.proxy_command.as_deref().unwrap_or_default();
                let (p, stream) = Proxy::spawn(command)?;
                let p = proxy_slot.insert(p);
                let r = handshake(tokio::time::timeout(Duration::from_secs(30), client::connect_stream(config, stream, handler)).await);
                if let Err(e) = r {
                    // Let the command finish writing why it failed.
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    let why = p.stderr_text();
                    let why = if why.is_empty() { String::new() } else { format!("\n{why}") };
                    return Err(err(format!("{e}\nProxyCommand: {command}{why}")));
                }
                r
            }
            None => {
                let tcp = tokio::time::timeout(Duration::from_secs(20), tokio::net::TcpStream::connect(addr))
                    .await
                    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Connection timed out"))??;
                let _ = tcp.set_nodelay(true);
                handshake(tokio::time::timeout(Duration::from_secs(30), client::connect_stream(config, tcp, handler)).await)
            }
            Some(j) => {
                if j.is_closed() {
                    return Err(io::Error::new(io::ErrorKind::NotConnected, format!("The tunnel {} is closed", j.label())));
                }
                let ch = tokio::time::timeout(
                    Duration::from_secs(20),
                    j.handle.channel_open_direct_tcpip(addr.0.clone(), u32::from(addr.1), "127.0.0.1", 0),
                )
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Connection through the tunnel timed out"))?
                .map_err(|e| err(format!("The jump host {} cannot reach {}:{}: {e}", j.label, addr.0, addr.1)))?;
                handshake(tokio::time::timeout(Duration::from_secs(30), client::connect_stream(config, ch.into_stream(), handler)).await)
            }
        }
    });
    let state = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match (r, state) {
        (_, Some(HostKey::Changed { file, line, key })) => Err(err(format!(
            "WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!\nSomeone could be eavesdropping on you right now (man-in-the-middle attack)!\nThe host key is now {}.\nOffending key in {}:{line}",
            fingerprint(&key),
            file.display()
        ))),
        (Ok(handle), Some(host_key)) => Ok(Pending { handle, host_key, tunnel: tunnel.cloned(), proxy }),
        (Ok(_), None) => Err(err("The server key was not checked")),
        (Err(e), _) => Err(e),
    }
}

impl Pending {
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    pub fn disconnect(self) {
        let _ = rt().block_on(self.handle.disconnect(russh::Disconnect::ByApplication, "", "en"));
    }

    /// Keys of a running ssh-agent (Pageant / the OpenSSH agent on Windows).
    pub fn try_agent(&mut self, user: &str) -> bool {
        let h = &mut self.handle;
        rt().block_on(async move {
            #[cfg(unix)]
            let agent = russh::keys::agent::client::AgentClient::connect_env().await.ok().map(|a| a.dynamic());
            #[cfg(windows)]
            let agent = match russh::keys::agent::client::AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
                Ok(a) => Some(a.dynamic()),
                Err(_) => russh::keys::agent::client::AgentClient::connect_pageant().await.ok().map(|a| a.dynamic()),
            };
            #[cfg(not(any(unix, windows)))]
            let agent: Option<russh::keys::agent::client::AgentClient<Box<dyn russh::keys::agent::client::AgentStream + Send + Unpin>>> = None;
            let Some(mut agent) = agent else { return false };
            let Ok(ids) = tokio::time::timeout(Duration::from_secs(5), agent.request_identities()).await.unwrap_or(Ok(Vec::new())) else {
                return false;
            };
            let hash = h.best_supported_rsa_hash().await.ok().flatten().flatten();
            for id in ids {
                let key = id.public_key().into_owned();
                match h.authenticate_publickey_with(user, key, hash, &mut agent).await {
                    Ok(r) if r.success() => return true,
                    _ => {
                        if h.is_closed() {
                            return false;
                        }
                    }
                }
            }
            false
        })
    }

    pub fn try_key_file(&mut self, user: &str, path: &Path, passphrase: Option<&str>) -> KeyResult {
        if !path.is_file() {
            return KeyResult::Missing;
        }
        let key = match russh::keys::load_secret_key(path, passphrase) {
            Ok(k) => k,
            Err(russh::keys::Error::KeyIsEncrypted) => return KeyResult::Encrypted,
            Err(e) if passphrase.is_some() => {
                // A wrong passphrase looks like a corrupt key.
                let _ = e;
                return KeyResult::Encrypted;
            }
            Err(_) => return KeyResult::Unusable,
        };
        let h = &mut self.handle;
        let r = rt().block_on(async move {
            let hash = h.best_supported_rsa_hash().await.ok().flatten().flatten();
            h.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await
        });
        match r {
            Ok(a) if a.success() => KeyResult::Accepted,
            Ok(_) => KeyResult::Rejected,
            Err(_) => KeyResult::Unusable,
        }
    }

    /// Password authentication; keyboard-interactive with the same password
    /// for servers that only offer that.
    pub fn try_password(&mut self, user: &str, password: &str) -> io::Result<bool> {
        let h = &mut self.handle;
        rt().block_on(async move {
            if h.authenticate_password(user, password).await.map_err(err)?.success() {
                return Ok(true);
            }
            let mut r = h.authenticate_keyboard_interactive_start(user, None).await.map_err(err)?;
            for _ in 0..5 {
                match r {
                    KeyboardInteractiveAuthResponse::Success => return Ok(true),
                    KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
                    KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                        let answers = prompts.iter().map(|p| if p.echo { String::new() } else { password.to_string() }).collect();
                        r = h.authenticate_keyboard_interactive_respond(answers).await.map_err(err)?;
                    }
                }
            }
            Ok(false)
        })
    }

    /// Turns the authenticated connection into a tunnel for the next one.
    pub fn into_tunnel(self, label: String) -> Arc<Tunnel> {
        Arc::new(Tunnel { handle: self.handle, label, via: self.tunnel, _proxy: self.proxy })
    }

    /// Opens the SFTP subsystem on the authenticated connection.
    pub fn finish(self, target: Target) -> io::Result<Conn> {
        let Pending { handle, tunnel, proxy, .. } = self;
        // The tunnel stays out of the async block: dropping it there would
        // call block_on inside the runtime.
        let (sftp, home) = rt().block_on(async {
            let ch = handle.channel_open_session().await.map_err(err)?;
            ch.request_subsystem(true, "sftp").await.map_err(err)?;
            let sftp = SftpSession::new(ch.into_stream()).await.map_err(err)?;
            let home = sftp.canonicalize(".").await.map_err(err)?;
            Ok::<_, io::Error>((sftp, home))
        })?;
        Ok(Conn { handle, sftp, target, home, tunnel, _proxy: proxy })
    }
}

/// An authenticated connection with its SFTP session.
pub struct Conn {
    pub handle: Handle<Client>,
    pub sftp: SftpSession,
    pub target: Target,
    /// The remote home folder (where `ssh` starts).
    pub home: String,
    /// The tunnel the connection goes through; closed after it (fields
    /// drop after `Drop::drop`).
    tunnel: Option<Arc<Tunnel>>,
    /// The ProxyCommand the connection runs over; killed after it.
    _proxy: Option<Proxy>,
}

impl Conn {
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed() || self.tunnel.as_ref().is_some_and(|t| t.is_closed())
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        // Never hang the UI on a dead network.
        let _ = rt().block_on(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                let _ = self.sftp.close().await;
                self.handle.disconnect(russh::Disconnect::ByApplication, "", "en").await
            })
            .await
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(proxy_command: &str) -> Target {
        Target {
            alias: "h".into(),
            host: "h".into(),
            port: 22,
            user: "u".into(),
            dir: String::new(),
            identity_files: Vec::new(),
            proxy_command: Some(proxy_command.into()),
        }
    }

    #[test]
    fn proxy_command_failure_is_reported() {
        // The command runs through the shell (quotes kept); what it writes
        // to stderr is shown when the handshake fails.
        let e = connect(&target("echo \"proxy said no\" 1>&2 && exit 3"), None).err().expect("no SSH server behind echo");
        let e = e.to_string();
        assert!(e.contains("proxy said no") && e.contains("ProxyCommand: echo"), "{e}");
    }

    #[test]
    fn proxy_command_that_is_not_ssh() {
        let e = connect(&target("echo not-ssh"), None).err().expect("not an SSH server").to_string();
        assert!(e.contains("ProxyCommand: echo not-ssh"), "{e}");
    }
}

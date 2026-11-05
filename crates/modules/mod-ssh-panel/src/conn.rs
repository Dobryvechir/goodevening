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

/// A connection that is not authenticated yet.
pub struct Pending {
    pub handle: Handle<Client>,
    pub host_key: HostKey,
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

pub fn connect(t: &Target) -> io::Result<Pending> {
    let seen = Arc::new(Mutex::new(None));
    let handler = Client { host: t.host.clone(), port: t.port, seen: seen.clone() };
    let config =
        Arc::new(client::Config { keepalive_interval: Some(Duration::from_secs(30)), keepalive_max: 4, nodelay: true, ..Default::default() });
    let addr = (t.host.clone(), t.port);
    let r = rt().block_on(async move {
        let tcp = tokio::time::timeout(Duration::from_secs(20), tokio::net::TcpStream::connect(addr))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Connection timed out"))??;
        let _ = tcp.set_nodelay(true);
        tokio::time::timeout(Duration::from_secs(30), client::connect_stream(config, tcp, handler))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "The server does not answer"))?
            .map_err(err)
    });
    let state = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match (r, state) {
        (_, Some(HostKey::Changed { file, line, key })) => Err(err(format!(
            "WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!\nSomeone could be eavesdropping on you right now (man-in-the-middle attack)!\nThe host key is now {}.\nOffending key in {}:{line}",
            fingerprint(&key),
            file.display()
        ))),
        (Ok(handle), Some(host_key)) => Ok(Pending { handle, host_key }),
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

    /// Opens the SFTP subsystem on the authenticated connection.
    pub fn finish(self, target: Target) -> io::Result<Conn> {
        let handle = self.handle;
        rt().block_on(async move {
            let ch = handle.channel_open_session().await.map_err(err)?;
            ch.request_subsystem(true, "sftp").await.map_err(err)?;
            let sftp = SftpSession::new(ch.into_stream()).await.map_err(err)?;
            let home = sftp.canonicalize(".").await.map_err(err)?;
            Ok(Conn { handle, sftp, target, home })
        })
    }
}

/// An authenticated connection with its SFTP session.
pub struct Conn {
    pub handle: Handle<Client>,
    pub sftp: SftpSession,
    pub target: Target,
    /// The remote home folder (where `ssh` starts).
    pub home: String,
}

impl Conn {
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
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

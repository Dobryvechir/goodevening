//! User and group names for the "Owner" column.

use std::fs::Metadata;

pub fn current_user() -> String {
    std::env::var(if cfg!(windows) { "USERNAME" } else { "USER" }).unwrap_or_default()
}

pub fn host_name() -> String {
    if cfg!(windows) {
        std::env::var("COMPUTERNAME").unwrap_or_default()
    } else {
        std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default()
    }
}

/// Owner name of a file (`uid` → name via `/etc/passwd` on Unix; empty on Windows).
pub fn owner(meta: &Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        name_for_id(meta.uid(), "/etc/passwd")
    }
    #[cfg(windows)]
    {
        let _ = meta;
        String::new()
    }
}

pub fn group(meta: &Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        name_for_id(meta.gid(), "/etc/group")
    }
    #[cfg(windows)]
    {
        let _ = meta;
        String::new()
    }
}

#[cfg(unix)]
fn name_for_id(id: u32, file: &'static str) -> String {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<(&'static str, u32), String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Ok(c) = cache.lock() {
        if let Some(n) = c.get(&(file, id)) {
            return n.clone();
        }
    }
    let mut name = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                let mut f = l.split(':');
                let n = f.next()?;
                f.next();
                (f.next()?.parse::<u32>().ok()? == id).then(|| n.to_string())
            })
        })
        .unwrap_or_default();
    if name.is_empty() {
        if file == "/etc/passwd" && rustix::process::getuid().as_raw() == id {
            name = current_user();
        } else {
            name = id.to_string();
        }
    }
    if let Ok(mut c) = cache.lock() {
        c.insert((file, id), name.clone());
    }
    name
}

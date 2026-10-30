//! Drives (Windows) and mount points (Unix) for the Alt+F1/F2 menu.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Cdrom,
    Ram,
    Home,
    Root,
    Unknown,
}

impl DriveKind {
    pub fn label(&self) -> &'static str {
        match self {
            DriveKind::Fixed => "fixed",
            DriveKind::Removable => "removable",
            DriveKind::Network => "network",
            DriveKind::Cdrom => "CD-ROM",
            DriveKind::Ram => "RAM",
            DriveKind::Home => "home",
            DriveKind::Root => "root",
            DriveKind::Unknown => "",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DriveInfo {
    pub path: PathBuf,
    /// Short name shown in the menu: `C:` on Windows, mount point elsewhere.
    pub name: String,
    pub label: String,
    pub fs: String,
    pub kind: DriveKind,
    pub total: Option<u64>,
    pub free: Option<u64>,
}

/// (total, free-for-user) bytes of the volume containing `path`.
pub fn space(path: &Path) -> Option<(u64, u64)> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        // SAFETY: valid NUL-terminated path and valid out-pointers.
        let ok = unsafe { windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
        if ok == 0 { None } else { Some((total, avail)) }
    }
    #[cfg(unix)]
    {
        let s = rustix::fs::statvfs(path).ok()?;
        let unit = if s.f_frsize > 0 { s.f_frsize } else { s.f_bsize };
        Some((s.f_blocks * unit, s.f_bavail * unit))
    }
}

pub fn list() -> Vec<DriveInfo> {
    #[cfg(windows)]
    {
        windows_drives()
    }
    #[cfg(unix)]
    {
        unix_mounts()
    }
}

#[cfg(windows)]
fn windows_drives() -> Vec<DriveInfo> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    let mut out = Vec::new();
    // SAFETY: no arguments.
    let mask = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
        // SAFETY: valid NUL-terminated root path.
        let kind = match unsafe { GetDriveTypeW(wide.as_ptr()) } {
            2 => DriveKind::Removable,
            3 => DriveKind::Fixed,
            4 => DriveKind::Network,
            5 => DriveKind::Cdrom,
            6 => DriveKind::Ram,
            _ => DriveKind::Unknown,
        };
        let mut label = [0u16; 261];
        let mut fsname = [0u16; 261];
        let (mut serial, mut maxlen, mut flags) = (0u32, 0u32, 0u32);
        // Querying removable drives without media is slow and fails; skip floppies.
        let ok = if kind == DriveKind::Removable && i < 2 {
            0
        } else {
            // SAFETY: buffers are sized as passed; out-pointers are valid.
            unsafe {
                GetVolumeInformationW(
                    wide.as_ptr(),
                    label.as_mut_ptr(),
                    label.len() as u32,
                    &mut serial,
                    &mut maxlen,
                    &mut flags,
                    fsname.as_mut_ptr(),
                    fsname.len() as u32,
                )
            }
        };
        let to_s = |b: &[u16]| String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]);
        let (label, fs) = if ok != 0 { (to_s(&label), to_s(&fsname)) } else { (String::new(), String::new()) };
        let sp = if ok != 0 { space(Path::new(&root)) } else { None };
        out.push(DriveInfo {
            path: PathBuf::from(&root),
            name: format!("{letter}:"),
            label,
            fs,
            kind,
            total: sp.map(|s| s.0),
            free: sp.map(|s| s.1),
        });
    }
    if let Some(home) = home_dir() {
        out.push(DriveInfo {
            name: "~".into(),
            label: home.display().to_string(),
            path: home,
            fs: String::new(),
            kind: DriveKind::Home,
            total: None,
            free: None,
        });
    }
    out
}

#[cfg(unix)]
fn unix_mounts() -> Vec<DriveInfo> {
    let mut out = Vec::new();
    let mut push = |path: PathBuf, fs: String, kind: DriveKind| {
        if out.iter().any(|d: &DriveInfo| d.path == path) {
            return;
        }
        let sp = space(&path);
        out.push(DriveInfo { name: path.display().to_string(), label: String::new(), path, fs, kind, total: sp.map(|s| s.0), free: sp.map(|s| s.1) });
    };
    push(PathBuf::from("/"), String::new(), DriveKind::Root);
    if let Some(h) = home_dir() {
        push(h, String::new(), DriveKind::Home);
    }
    for (point, fs) in mount_table() {
        if point == "/" {
            continue;
        }
        let kind = match fs.as_str() {
            "nfs" | "nfs4" | "cifs" | "smbfs" | "smb3" | "afpfs" | "sshfs" | "fuse.sshfs" | "webdav" => DriveKind::Network,
            "iso9660" | "udf" | "cd9660" => DriveKind::Cdrom,
            "tmpfs" | "ramfs" => DriveKind::Ram,
            "vfat" | "exfat" | "msdos" | "ntfs" | "ntfs3" | "fuseblk"
                if point.starts_with("/media") || point.starts_with("/run/media") || point.starts_with("/Volumes") =>
            {
                DriveKind::Removable
            }
            _ => DriveKind::Fixed,
        };
        push(PathBuf::from(point), fs, kind);
    }
    out
}

/// (mount point, file system type) pairs, without pseudo file systems.
#[cfg(unix)]
pub fn mount_table() -> Vec<(String, String)> {
    const PSEUDO: &[&str] = &[
        "proc",
        "sysfs",
        "devtmpfs",
        "devpts",
        "cgroup",
        "cgroup2",
        "securityfs",
        "pstore",
        "debugfs",
        "tracefs",
        "configfs",
        "fusectl",
        "mqueue",
        "hugetlbfs",
        "bpf",
        "autofs",
        "binfmt_misc",
        "overlay",
        "squashfs",
        "nsfs",
        "efivarfs",
        "devfs",
        "rpc_pipefs",
        "selinuxfs",
        "fuse.portal",
        "fuse.gvfsd-fuse",
    ];
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string("/proc/self/mounts") {
        for line in text.lines() {
            let mut f = line.split(' ');
            let (_, Some(point), Some(fs)) = (f.next(), f.next(), f.next()) else { continue };
            if PSEUDO.contains(&fs) || point.starts_with("/snap/") || point.starts_with("/sys") || point.starts_with("/proc") {
                continue;
            }
            if fs == "tmpfs" && point != "/tmp" && !point.starts_with("/dev/shm") {
                continue;
            }
            out.push((unescape_octal(point), fs.to_string()));
        }
    } else if let Ok(o) = std::process::Command::new("mount").output() {
        // macOS / BSD: "dev on /point (fstype, flags)"
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let Some((_, rest)) = line.split_once(" on ") else { continue };
            let Some((point, attrs)) = rest.rsplit_once(" (") else { continue };
            let fs = attrs.split([',', ')']).next().unwrap_or("").trim().to_string();
            if PSEUDO.contains(&fs.as_str()) || point.starts_with("/System/Volumes") || point.starts_with("/private/var/vm") {
                continue;
            }
            out.push((point.to_string(), fs));
        }
    }
    out
}

#[cfg(unix)]
fn unescape_octal(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() + 1 && b[i + 1..].len() >= 3 {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 4], 8) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from)
}

/// Root of the volume containing `path` (`C:\`, `\\server\share\` or `/`).
pub fn volume_root(path: &Path) -> PathBuf {
    if cfg!(windows) {
        let mut it = path.components();
        match it.next() {
            Some(std::path::Component::Prefix(p)) => {
                let mut r = PathBuf::from(p.as_os_str());
                r.push(std::path::MAIN_SEPARATOR_STR);
                r
            }
            _ => PathBuf::from(std::path::MAIN_SEPARATOR_STR),
        }
    } else {
        // The longest mount point that is a prefix of `path`.
        #[cfg(unix)]
        {
            let mut best = PathBuf::from("/");
            for (point, _) in mount_table() {
                let p = Path::new(&point);
                if path.starts_with(p) && p.as_os_str().len() > best.as_os_str().len() {
                    best = p.to_path_buf();
                }
            }
            best
        }
        #[cfg(not(unix))]
        PathBuf::from("/")
    }
}

/// Network resources currently connected (Windows) or network mounts (Unix):
/// (local name or mount point, remote name).
pub fn network_resources() -> Vec<(String, String)> {
    #[cfg(windows)]
    {
        windows_network()
    }
    #[cfg(unix)]
    {
        mount_table()
            .into_iter()
            .filter(|(_, fs)| matches!(fs.as_str(), "nfs" | "nfs4" | "cifs" | "smbfs" | "smb3" | "afpfs" | "sshfs" | "fuse.sshfs"))
            .map(|(p, fs)| (p, fs))
            .collect()
    }
}

#[cfg(windows)]
fn windows_network() -> Vec<(String, String)> {
    use windows_sys::Win32::NetworkManagement::WNet::{
        NETRESOURCEW, RESOURCE_CONNECTED, RESOURCETYPE_DISK, WNetCloseEnum, WNetEnumResourceW, WNetOpenEnumW,
    };
    let mut out = Vec::new();
    let mut handle: windows_sys::Win32::Foundation::HANDLE = std::ptr::null_mut();
    // SAFETY: null NETRESOURCE means "enumerate connected resources"; handle is an out-pointer.
    if unsafe { WNetOpenEnumW(RESOURCE_CONNECTED, RESOURCETYPE_DISK, 0, std::ptr::null(), &mut handle) } != 0 {
        return out;
    }
    let mut buf = vec![0u64; 2048]; // 16 KiB, 8-byte aligned for NETRESOURCEW
    loop {
        let mut count = u32::MAX;
        let mut size = (buf.len() * 8) as u32;
        // SAFETY: buffer is valid for `size` bytes; count/size are valid out-pointers.
        let r = unsafe { WNetEnumResourceW(handle, &mut count, buf.as_mut_ptr().cast(), &mut size) };
        if r != 0 {
            break;
        }
        let items = buf.as_ptr().cast::<NETRESOURCEW>();
        for i in 0..count as usize {
            // SAFETY: the API wrote `count` NETRESOURCEW records at the start of `buf`;
            // their string pointers point into the same buffer.
            let res = unsafe { &*items.add(i) };
            let local = unsafe { wide_ptr_to_string(res.lpLocalName) };
            let remote = unsafe { wide_ptr_to_string(res.lpRemoteName) };
            out.push((local, remote));
        }
    }
    // SAFETY: handle came from WNetOpenEnumW.
    unsafe { WNetCloseEnum(handle) };
    out
}

/// # Safety
/// `p` must be null or point to a NUL-terminated UTF-16 string.
#[cfg(windows)]
unsafe fn wide_ptr_to_string(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0;
    // SAFETY: caller guarantees NUL termination.
    unsafe {
        while *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lists_something() {
        let d = list();
        assert!(!d.is_empty());
        let cur = std::env::current_dir().unwrap();
        assert!(cur.starts_with(volume_root(&cur)));
    }
}

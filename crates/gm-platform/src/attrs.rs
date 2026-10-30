//! File attributes: Windows `RHSA…` flags or Unix permission bits.

use std::fs::Metadata;
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attrs {
    /// Windows `FILE_ATTRIBUTE_*` bits (0 on Unix).
    pub win: u32,
    /// Unix `st_mode` (0 on Windows).
    pub mode: u32,
}

pub const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
pub const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
pub const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
pub const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x20;
pub const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
pub const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x800;
pub const FILE_ATTRIBUTE_ENCRYPTED: u32 = 0x4000;
pub const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;
pub const FILE_ATTRIBUTE_TEMPORARY: u32 = 0x100;
pub const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
pub const FILE_ATTRIBUTE_NOT_CONTENT_INDEXED: u32 = 0x2000;

impl Attrs {
    pub fn from_metadata(meta: &Metadata) -> Attrs {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            Attrs { win: meta.file_attributes(), mode: 0 }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Attrs { win: 0, mode: meta.mode() }
        }
    }

    pub fn readonly(&self) -> bool {
        if cfg!(windows) { self.win & FILE_ATTRIBUTE_READONLY != 0 } else { self.mode & 0o222 == 0 }
    }

    /// Text for the "A" column: `RHSDACE…` on Windows, `drwxr-xr-x` on Unix.
    pub fn format(&self, is_dir: bool, is_link: bool) -> String {
        if cfg!(windows) {
            let flags = [
                (FILE_ATTRIBUTE_READONLY, 'R'),
                (FILE_ATTRIBUTE_SYSTEM, 'S'),
                (FILE_ATTRIBUTE_HIDDEN, 'H'),
                (FILE_ATTRIBUTE_ARCHIVE, 'A'),
                (FILE_ATTRIBUTE_REPARSE_POINT, 'L'),
                (FILE_ATTRIBUTE_COMPRESSED, 'C'),
                (FILE_ATTRIBUTE_ENCRYPTED, 'E'),
            ];
            flags.iter().map(|&(bit, c)| if self.win & bit != 0 { c } else { ' ' }).collect()
        } else {
            let mut s = String::with_capacity(10);
            s.push(if is_link {
                'l'
            } else if is_dir {
                'd'
            } else {
                match self.mode & 0o170000 {
                    0o020000 => 'c',
                    0o060000 => 'b',
                    0o010000 => 'p',
                    0o140000 => 's',
                    _ => '-',
                }
            });
            let m = self.mode;
            for (shift, special, sc) in [(6, 0o4000, 's'), (3, 0o2000, 's'), (0, 0o1000, 't')] {
                let bits = (m >> shift) & 7;
                s.push(if bits & 4 != 0 { 'r' } else { '-' });
                s.push(if bits & 2 != 0 { 'w' } else { '-' });
                let x = bits & 1 != 0;
                s.push(match (m & special != 0, x) {
                    (true, true) => sc,
                    (true, false) => sc.to_ascii_uppercase(),
                    (false, true) => 'x',
                    (false, false) => '-',
                });
            }
            s
        }
    }
}

/// Hidden according to platform rules (attribute on Windows, leading dot on Unix).
pub fn is_hidden(name: &str, attrs: &Attrs) -> bool {
    if cfg!(windows) { attrs.win & FILE_ATTRIBUTE_HIDDEN != 0 } else { name.starts_with('.') && name != ".." }
}

pub fn is_system(attrs: &Attrs) -> bool {
    attrs.win & FILE_ATTRIBUTE_SYSTEM != 0
}

/// Executable: extension in `PATHEXT` on Windows, any exec bit on Unix.
pub fn is_executable(name: &str, attrs: &Attrs, is_dir: bool) -> bool {
    if is_dir {
        return false;
    }
    if cfg!(windows) {
        let ext = match name.rfind('.') {
            Some(i) => name[i..].to_ascii_uppercase(),
            None => return false,
        };
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        pathext.split(';').any(|e| e.eq_ignore_ascii_case(&ext))
    } else {
        attrs.mode & 0o111 != 0
    }
}

/// Sets or clears the read-only state.
pub fn set_readonly(path: &Path, readonly: bool) -> std::io::Result<()> {
    let mut perm = std::fs::metadata(path)?.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(readonly);
    std::fs::set_permissions(path, perm)
}

/// Sets Unix permission bits (no-op on Windows).
pub fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(windows)]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// Sets Windows attribute bits (no-op on Unix).
pub fn set_win_attrs(path: &Path, attrs: u32) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call.
        let ok = unsafe { windows_sys::Win32::Storage::FileSystem::SetFileAttributesW(wide.as_ptr(), attrs) };
        if ok == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
    }
    #[cfg(unix)]
    {
        let _ = (path, attrs);
        Ok(())
    }
}

/// Creates a symbolic link; on Windows the target kind must be known.
pub fn symlink(target: &Path, link: &Path, dir: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if dir { std::os::windows::fs::symlink_dir(target, link) } else { std::os::windows::fs::symlink_file(target, link) }
    }
    #[cfg(unix)]
    {
        let _ = dir;
        std::os::unix::fs::symlink(target, link)
    }
}

/// Number of hard links of a file, if the platform reports it cheaply.
pub fn hard_links(meta: &Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(meta.nlink())
    }
    #[cfg(windows)]
    {
        let _ = meta;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unix_format() {
        if cfg!(unix) {
            let a = Attrs { win: 0, mode: 0o104755 };
            assert_eq!(a.format(false, false), "-rwsr-xr-x");
        } else {
            let a = Attrs { win: FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_ARCHIVE, mode: 0 };
            assert_eq!(a.format(false, false), "R  A   ");
        }
    }
}

//! Panel sort modes (Ctrl+F3…F12).

use gm_text::sort::{cmp_ci, cmp_natural};
use gm_vfs::Entry;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    Name,
    Ext,
    Modified,
    Size,
    Unsorted,
    Created,
    Accessed,
    Description,
    Owner,
}

impl SortMode {
    pub const ALL: [SortMode; 9] = [
        SortMode::Name,
        SortMode::Ext,
        SortMode::Modified,
        SortMode::Size,
        SortMode::Unsorted,
        SortMode::Created,
        SortMode::Accessed,
        SortMode::Description,
        SortMode::Owner,
    ];

    pub fn key(&self) -> &'static str {
        match self {
            SortMode::Name => "name",
            SortMode::Ext => "ext",
            SortMode::Modified => "modified",
            SortMode::Size => "size",
            SortMode::Unsorted => "unsorted",
            SortMode::Created => "created",
            SortMode::Accessed => "accessed",
            SortMode::Description => "description",
            SortMode::Owner => "owner",
        }
    }

    pub fn from_key(s: &str) -> SortMode {
        SortMode::ALL.iter().copied().find(|m| m.key() == s).unwrap_or(SortMode::Name)
    }

    /// The letter shown in the panel's top-left corner (uppercase = reversed).
    pub fn letter(&self) -> char {
        match self {
            SortMode::Name => 'n',
            SortMode::Ext => 'x',
            SortMode::Modified => 'm',
            SortMode::Size => 's',
            SortMode::Unsorted => 'u',
            SortMode::Created => 'c',
            SortMode::Accessed => 'a',
            SortMode::Description => 'z',
            SortMode::Owner => 'o',
        }
    }

    /// The Ctrl+F-key that selects this mode (Ctrl+F3 = Name…).
    pub fn fkey(&self) -> u8 {
        match self {
            SortMode::Name => 3,
            SortMode::Ext => 4,
            SortMode::Modified => 5,
            SortMode::Size => 6,
            SortMode::Unsorted => 7,
            SortMode::Created => 8,
            SortMode::Accessed => 9,
            SortMode::Description => 10,
            SortMode::Owner => 11,
        }
    }

    pub fn from_fkey(n: u8) -> Option<SortMode> {
        SortMode::ALL.iter().copied().find(|m| m.fkey() == n)
    }
}

/// Sorts entries in place: ".." first, folders before files, then the mode.
pub fn sort_entries(v: &mut [Entry], mode: SortMode, reverse: bool, natural: bool, selected_first: Option<&[bool]>) {
    let _ = selected_first;
    if mode == SortMode::Unsorted {
        if reverse {
            v.reverse();
        }
        v.sort_by_key(|e| (!e.is_dotdot(), !e.is_dir));
        return;
    }
    let name_cmp = |a: &Entry, b: &Entry| if natural { cmp_natural(&a.name, &b.name) } else { cmp_ci(&a.name, &b.name) };
    v.sort_by(|a, b| {
        let group = (!a.is_dotdot(), !a.is_dir).cmp(&(!b.is_dotdot(), !b.is_dir));
        if group != Ordering::Equal {
            return group;
        }
        let ord = match mode {
            SortMode::Name => name_cmp(a, b),
            SortMode::Ext => cmp_ci(a.ext(), b.ext()).then_with(|| name_cmp(a, b)),
            SortMode::Modified => b.modified.cmp(&a.modified).then_with(|| name_cmp(a, b)),
            SortMode::Size => b.size.cmp(&a.size).then_with(|| name_cmp(a, b)),
            SortMode::Created => b.created.cmp(&a.created).then_with(|| name_cmp(a, b)),
            SortMode::Accessed => b.accessed.cmp(&a.accessed).then_with(|| name_cmp(a, b)),
            SortMode::Description => {
                cmp_ci(a.description.as_deref().unwrap_or(""), b.description.as_deref().unwrap_or("")).then_with(|| name_cmp(a, b))
            }
            SortMode::Owner => cmp_ci(&a.owner, &b.owner).then_with(|| name_cmp(a, b)),
            SortMode::Unsorted => Ordering::Equal,
        };
        if reverse { ord.reverse() } else { ord }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, dir: bool, size: u64) -> Entry {
        Entry { name: name.into(), is_dir: dir, size, ..Default::default() }
    }

    #[test]
    fn folders_first_and_modes() {
        let mut v =
            vec![e("b.txt", false, 5), e("zdir", true, 0), e("..", true, 0), e("a.rs", false, 50), e("file10", false, 1), e("file2", false, 1)];
        sort_entries(&mut v, SortMode::Name, false, true, None);
        let names: Vec<&str> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["..", "zdir", "a.rs", "b.txt", "file2", "file10"]);
        sort_entries(&mut v, SortMode::Size, false, true, None);
        assert_eq!(v[2].name, "a.rs");
        sort_entries(&mut v, SortMode::Ext, false, true, None);
        assert_eq!(v[2].name, "file2");
        sort_entries(&mut v, SortMode::Name, true, true, None);
        assert_eq!(v[0].name, "..");
        assert_eq!(v[2].name, "file10");
    }
}

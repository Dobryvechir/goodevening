//! String ordering used by panels (replaces `string_sort.cpp`).

use std::cmp::Ordering;

/// Case-insensitive comparison.
pub fn cmp_ci(a: &str, b: &str) -> Ordering {
    let mut ia = a.chars().flat_map(char::to_lowercase);
    let mut ib = b.chars().flat_map(char::to_lowercase);
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x != y => return x.cmp(&y),
            _ => {}
        }
    }
}

/// "Natural" (numeric-aware) case-insensitive comparison: `file2 < file10`.
pub fn cmp_natural(a: &str, b: &str) -> Ordering {
    let a_chars: Vec<char> = a.chars().flat_map(char::to_lowercase).collect();
    let b_chars: Vec<char> = b.chars().flat_map(char::to_lowercase).collect();
    let (mut i, mut j) = (0, 0);
    while i < a_chars.len() && j < b_chars.len() {
        let (ca, cb) = (a_chars[i], b_chars[j]);
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let si = i;
            while i < a_chars.len() && a_chars[i].is_ascii_digit() {
                i += 1;
            }
            let sj = j;
            while j < b_chars.len() && b_chars[j].is_ascii_digit() {
                j += 1;
            }
            let na: String = a_chars[si..i].iter().collect();
            let nb: String = b_chars[sj..j].iter().collect();
            let ta = na.trim_start_matches('0');
            let tb = nb.trim_start_matches('0');
            let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)).then_with(|| na.len().cmp(&nb.len()));
            if ord != Ordering::Equal {
                return ord;
            }
        } else {
            if ca != cb {
                return ca.cmp(&cb);
            }
            i += 1;
            j += 1;
        }
    }
    (a_chars.len() - i).cmp(&(b_chars.len() - j)).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn natural() {
        let mut v = vec!["file10", "File2", "file1", "file02"];
        v.sort_by(|a, b| cmp_natural(a, b));
        assert_eq!(v, ["file1", "File2", "file02", "file10"]);
        assert_eq!(cmp_ci("ABC", "abd"), Ordering::Less);
    }
}

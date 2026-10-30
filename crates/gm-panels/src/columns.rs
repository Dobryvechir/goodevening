//! Panel column model, compatible with Far's column-type strings
//! (`N,S,D,T` + widths `0,10,0,5`).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnKind {
    Name,
    Size,
    Packed,
    Date,
    Time,
    Modified,
    Created,
    Accessed,
    Attributes,
    Description,
    Owner,
    Links,
    Custom(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub kind: ColumnKind,
    /// 0 = flexible.
    pub width: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnSet {
    pub columns: Vec<Column>,
    /// Status line columns.
    pub status: Vec<Column>,
    /// Name-only column sets wrap entries into several columns ("brief").
    pub multi: bool,
}

impl ColumnKind {
    pub fn parse(s: &str) -> Option<ColumnKind> {
        let s = s.trim().to_ascii_uppercase();
        // Far allows modifiers after the type letter (NM, SC, DM…); ignore unknown modifiers.
        Some(match s.as_str() {
            "DM" => ColumnKind::Modified,
            "DC" => ColumnKind::Created,
            "DA" => ColumnKind::Accessed,
            "LN" => ColumnKind::Links,
            _ => match s.chars().next()? {
                'N' => ColumnKind::Name,
                'S' => ColumnKind::Size,
                'P' => ColumnKind::Packed,
                'D' => ColumnKind::Date,
                'T' => ColumnKind::Time,
                'A' => ColumnKind::Attributes,
                'Z' => ColumnKind::Description,
                'O' => ColumnKind::Owner,
                'C' => ColumnKind::Custom(s[1..].parse().ok()?),
                _ => return None,
            },
        })
    }

    pub fn default_width(&self) -> usize {
        match self {
            ColumnKind::Name | ColumnKind::Description => 0,
            ColumnKind::Size | ColumnKind::Packed => 10,
            ColumnKind::Date => 8,
            ColumnKind::Time => 5,
            ColumnKind::Modified | ColumnKind::Created | ColumnKind::Accessed => 14,
            ColumnKind::Attributes => {
                if cfg!(windows) {
                    7
                } else {
                    10
                }
            }
            ColumnKind::Owner => 12,
            ColumnKind::Links => 3,
            ColumnKind::Custom(_) => 0,
        }
    }
}

impl ColumnSet {
    /// Parses Far's column types and widths strings.
    pub fn parse(types: &str, widths: &str) -> Option<ColumnSet> {
        let kinds: Vec<ColumnKind> = types.split(',').map(ColumnKind::parse).collect::<Option<_>>()?;
        let ws: Vec<usize> = widths.split(',').map(|w| w.trim().parse().unwrap_or(0)).collect();
        let columns: Vec<Column> = kinds
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let w = ws.get(i).copied().unwrap_or(0);
                Column { kind: *k, width: if w == 0 { k.default_width() } else { w } }
            })
            .collect();
        let multi = columns.len() > 1 && columns.iter().all(|c| c.kind == ColumnKind::Name);
        Some(ColumnSet {
            columns,
            status: vec![
                Column { kind: ColumnKind::Name, width: 0 },
                Column { kind: ColumnKind::Size, width: 10 },
                Column { kind: ColumnKind::Date, width: 8 },
                Column { kind: ColumnKind::Time, width: 5 },
            ],
            multi,
        })
    }

    /// Resolves flexible widths for a panel interior of `total` cells
    /// (separators take one cell between columns).
    pub fn layout(&self, total: usize) -> Vec<usize> {
        let n = self.columns.len();
        let seps = n.saturating_sub(1);
        let fixed: usize = self.columns.iter().map(|c| c.width).sum();
        let flex = self.columns.iter().filter(|c| c.width == 0).count();
        let avail = total.saturating_sub(seps);
        if flex == 0 {
            // Shrink the first column when fixed widths do not fit.
            let mut w: Vec<usize> = self.columns.iter().map(|c| c.width).collect();
            if fixed > avail && !w.is_empty() {
                w[0] = w[0].saturating_sub(fixed - avail);
            }
            return w;
        }
        let rest = avail.saturating_sub(fixed);
        let mut extra = rest % flex;
        self.columns
            .iter()
            .map(|c| {
                if c.width == 0 {
                    let add = usize::from(extra > 0);
                    extra = extra.saturating_sub(1);
                    rest / flex + add
                } else {
                    c.width
                }
            })
            .collect()
    }
}

/// Far's default view modes (Ctrl+0…9).
pub fn view_mode(n: u8) -> ColumnSet {
    let attrs = if cfg!(windows) { "7" } else { "10" };
    let (types, widths) = match n {
        1 => ("N,N,N", "0,0,0".to_string()),
        2 => ("N,N", "0,0".to_string()),
        3 => ("N,S,D,T", "0,10,0,0".to_string()),
        4 => ("N,S", "0,10".to_string()),
        5 => ("N,S,DC,DM,DA,A", format!("0,10,14,14,14,{attrs}")),
        6 => ("N,Z", "40,0".to_string()),
        7 => ("N,S,Z", "40,10,0".to_string()),
        8 => ("N,S,O", "0,10,15".to_string()),
        9 => ("N,S,LN", "0,10,3".to_string()),
        _ => ("N,S,DM,A", format!("0,10,14,{attrs}")),
    };
    ColumnSet::parse(types, &widths).expect("built-in view mode")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_layout() {
        let c = ColumnSet::parse("N,S,D,T", "0,10,0,5").unwrap();
        assert_eq!(c.columns[2].width, 8);
        assert!(!c.multi);
        let w = c.layout(40);
        assert_eq!(w.iter().sum::<usize>() + 3, 40);
        assert_eq!(w[1], 10);
        let b = view_mode(1);
        assert!(b.multi);
        let w = b.layout(38);
        assert_eq!(w, vec![12, 12, 12]);
        assert!(ColumnSet::parse("N,Q", "").is_none());
        assert_eq!(ColumnKind::parse("C2"), Some(ColumnKind::Custom(2)));
    }
}

//! Compare folders (replaces the `compare` plugin): selects, in both
//! panels, the items that are missing on the other side or differ by
//! size or modification time.
#![forbid(unsafe_code)]

use gm_core::gm_panels::FilePanel;
use gm_core::{Action, CommandSpec, Ctx, Module};
use std::collections::HashMap;

#[derive(Default)]
pub struct Compare;

/// Returns (left names to select, right names to select).
pub fn diff(a: &FilePanel, b: &FilePanel) -> (Vec<String>, Vec<String>) {
    let ci = gm_platform::case_insensitive_names();
    let key = |n: &str| if ci { n.to_lowercase() } else { n.to_string() };
    let bm: HashMap<String, &gm_core::gm_vfs::Entry> = b.entries.iter().filter(|e| !e.is_dotdot()).map(|e| (key(&e.name), e)).collect();
    let am: HashMap<String, &gm_core::gm_vfs::Entry> = a.entries.iter().filter(|e| !e.is_dotdot()).map(|e| (key(&e.name), e)).collect();
    let differs = |x: &gm_core::gm_vfs::Entry, y: &gm_core::gm_vfs::Entry| {
        if x.is_dir != y.is_dir {
            return true;
        }
        if x.is_dir {
            return false;
        }
        // FAT stores times with 2-second precision.
        let t = match (x.modified, y.modified) {
            (Some(p), Some(q)) => {
                let d = if p > q { p.duration_since(q) } else { q.duration_since(p) };
                d.map(|d| d.as_secs() > 2).unwrap_or(false)
            }
            _ => false,
        };
        x.size != y.size || t
    };
    let left =
        a.entries.iter().filter(|e| !e.is_dotdot()).filter(|e| bm.get(&key(&e.name)).is_none_or(|o| differs(e, o))).map(|e| e.name.clone()).collect();
    let right =
        b.entries.iter().filter(|e| !e.is_dotdot()).filter(|e| am.get(&key(&e.name)).is_none_or(|o| differs(e, o))).map(|e| e.name.clone()).collect();
    (left, right)
}

impl Module for Compare {
    fn id(&self) -> &'static str {
        "compare"
    }
    fn title(&self) -> String {
        "Compare folders".into()
    }
    fn commands(&self) -> Vec<CommandSpec> {
        vec![CommandSpec { id: "compare_folders", title: "&Compare folders".into() }]
    }
    fn run_command(&self, _id: &str, ctx: &mut Ctx) {
        let (l, r) = diff(ctx.active, ctx.passive);
        for (panel, names) in [(&mut *ctx.active, &l), (&mut *ctx.passive, &r)] {
            panel.clear_selection();
            for i in 0..panel.entries.len() {
                if names.contains(&panel.entries[i].name) {
                    panel.set_selected(i, true);
                }
            }
        }
        if l.is_empty() && r.is_empty() {
            ctx.push(Action::Message { title: "Compare folders".into(), text: "The folders are identical.".into() });
        }
    }
}

//! Panel tools (replaces the `filecase` and `samefolder` plugins).
#![forbid(unsafe_code)]

use gm_core::gm_ui::dialog::DialogBuilder;
use gm_core::{Action, CommandSpec, Ctx, Module};

#[derive(Default)]
pub struct PanelTools;

/// New name for `name` with the given case mode (0 lower, 1 upper, 2 title).
pub fn recase(name: &str, mode: u8, ext_separately: Option<u8>) -> String {
    let apply = |s: &str, m: u8| match m {
        0 => s.to_lowercase(),
        1 => s.to_uppercase(),
        _ => {
            let mut c = s.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        }
    };
    match (name.rfind('.'), ext_separately) {
        (Some(i), Some(em)) if i > 0 => format!("{}.{}", apply(&name[..i], mode), apply(&name[i + 1..], em)),
        _ => apply(name, mode),
    }
}

impl Module for PanelTools {
    fn id(&self) -> &'static str {
        "panel_tools"
    }

    fn title(&self) -> String {
        "Panel tools".into()
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![
            CommandSpec { id: "file_case", title: "Change file name &case".into() },
            CommandSpec { id: "same_folder", title: "&Same folder in the other panel".into() },
        ]
    }

    fn run_command(&self, id: &str, ctx: &mut Ctx) {
        match id {
            "same_folder" => {
                if let Some(d) = ctx.active.local_dir() {
                    let _ = ctx.passive.set_local_dir(d);
                }
            }
            "file_case" => {
                let Some(dir) = ctx.active.local_dir() else { return };
                let names = ctx.active.targets();
                if names.is_empty() {
                    return;
                }
                let mut b = DialogBuilder::new("File name case", 50);
                b.text("Name:");
                let nl = b.radio("&lower case", true, true);
                let nu = b.radio("&UPPER CASE", false, false);
                let nt = b.radio("&Title case", false, false);
                b.separator();
                let sep = b.checkbox("&Extension in lower case", true);
                b.separator();
                b.ok_cancel();
                let mut d = b.build();
                let first_btn = d.items.len() - 2;
                match d.run(ctx.ui) {
                    Some(i) if i != first_btn + 1 => {}
                    _ => return,
                }
                let _ = nl;
                let mode = if d.checked(nu) {
                    1
                } else if d.checked(nt) {
                    2
                } else {
                    0
                };
                let ext = d.checked(sep).then_some(0);
                let mut errors = Vec::new();
                for n in &names {
                    let new = recase(n, mode, ext);
                    if new != *n
                        && let Err(e) = gm_fs::rename(&dir.join(n), &new)
                    {
                        errors.push(format!("{n}: {e}"));
                    }
                }
                ctx.active.reload();
                if !errors.is_empty() {
                    ctx.push(Action::Message { title: "File name case".into(), text: errors.join("\n") });
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn recase() {
        assert_eq!(super::recase("ReadMe.TXT", 1, Some(0)), "README.txt");
        assert_eq!(super::recase("hello WORLD.md", 2, None), "Hello world.md");
    }
}

//! Shell integration (replaces the `emenu` plugin). The Explorer context
//! menu itself (IContextMenu) is not ported yet; this module offers the
//! system "Properties" dialog on Windows and "Open with the associated
//! program" on every platform.
#![forbid(unsafe_code)]

use gm_core::{Action, CommandSpec, Ctx, Module};

#[derive(Default)]
pub struct ShellMenu;

impl Module for ShellMenu {
    fn id(&self) -> &'static str {
        "shell_menu"
    }

    fn title(&self) -> String {
        "Shell".into()
    }

    fn commands(&self) -> Vec<CommandSpec> {
        let mut v = vec![CommandSpec { id: "shell_open", title: "Open with the &associated program".into() }];
        if cfg!(windows) {
            v.push(CommandSpec { id: "shell_properties", title: "Shell p&roperties".into() });
        }
        v
    }

    fn run_command(&self, id: &str, ctx: &mut Ctx) {
        let Some(p) = ctx.active.current_path().or_else(|| ctx.active.local_dir()) else { return };
        let r = match id {
            "shell_properties" => {
                if gm_platform::shell::show_properties(&p) {
                    Ok(())
                } else {
                    Err("The properties dialog is not available".to_string())
                }
            }
            _ => gm_platform::shell::open_with_system(&p).map_err(|e| e.to_string()),
        };
        if let Err(e) = r {
            ctx.push(Action::Message { title: "Shell".into(), text: e });
        }
    }
}

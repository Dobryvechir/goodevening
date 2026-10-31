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
        if id == "shell_properties" {
            if !gm_platform::shell::show_properties(&p) {
                ctx.push(Action::Message { title: "Shell".into(), text: "The properties dialog is not available".into() });
            }
        } else {
            // Through the app, so server execution mode can queue it.
            ctx.push(Action::Open(p));
        }
    }
}

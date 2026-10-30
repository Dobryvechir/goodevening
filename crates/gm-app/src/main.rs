//! `gm` — Goodevening Manager.
#![forbid(unsafe_code)]

use gm_config::{Settings, StateDb};
use gm_core::{App, DbHistory, Keymap, Module};
use gm_term::{Event, KeyCode, Term, palette};
use gm_ui::Ui;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

/// The built-in modules selected by Cargo features (the plugin replacement).
fn modules() -> Vec<Box<dyn Module>> {
    #[allow(unused_mut)]
    let mut v: Vec<Box<dyn Module>> = Vec::new();
    #[cfg(feature = "archive")]
    v.push(Box::new(mod_archive::Archive));
    #[cfg(feature = "temp_panel")]
    v.push(Box::new(mod_temp_panel::TempPanel));
    #[cfg(feature = "process_panel")]
    v.push(Box::new(mod_process_panel::ProcessPanel));
    #[cfg(feature = "compare")]
    v.push(Box::new(mod_compare::Compare));
    #[cfg(feature = "cmd_prefixes")]
    v.push(Box::new(mod_cmd_prefixes::CmdPrefixes));
    #[cfg(feature = "editor_tools")]
    v.push(Box::new(mod_editor_tools::EditorTools));
    #[cfg(feature = "panel_tools")]
    v.push(Box::new(mod_panel_tools::PanelTools));
    #[cfg(feature = "network_panel")]
    v.push(Box::new(mod_network_panel::NetworkPanel));
    #[cfg(feature = "shell_menu")]
    v.push(Box::new(mod_shell_menu::ShellMenu));
    v
}

#[derive(Default)]
struct Args {
    portable: bool,
    profile: Option<PathBuf>,
    view: Option<PathBuf>,
    edit: Option<(PathBuf, Option<usize>)>,
    keytest: bool,
    lang: Option<String>,
    colors: Option<String>,
    dirs: Vec<PathBuf>,
}

const USAGE: &str = "Goodevening Manager (gm) — a two-panel file manager

Usage: gm [options] [left_folder [right_folder]]

Options:
  -v <file>          open the file in the viewer
  -e[line] <file>    open the file in the editor (optionally at a line)
  -p, --portable     keep settings in gm-profile next to the executable
  --profile <dir>    use <dir> for settings and state
  --lang <name>      interface language (English, Russian, Ukrainian, …)
  --colors <mode>    auto, 16 or truecolor
  --keytest          show how key presses are decoded (for bug reports)
  -h, --help         this help
  --version          print the version
";

fn parse_args() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" | "-?" | "/?" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--version" => {
                println!("gm {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-p" | "--portable" => a.portable = true,
            "--profile" => a.profile = Some(PathBuf::from(it.next().ok_or("--profile needs a folder")?)),
            "--keytest" => a.keytest = true,
            "--lang" => a.lang = Some(it.next().ok_or("--lang needs a language name")?),
            "--colors" => a.colors = Some(it.next().ok_or("--colors needs a mode")?),
            "-v" => a.view = Some(PathBuf::from(it.next().ok_or("-v needs a file")?)),
            e if e.starts_with("-e") => {
                let line = e[2..].parse::<usize>().ok().map(|l| l.saturating_sub(1));
                a.edit = Some((PathBuf::from(it.next().ok_or("-e needs a file")?), line));
            }
            o if o.starts_with('-') && o.len() > 1 => return Err(format!("unknown option: {o}")),
            d => a.dirs.push(PathBuf::from(d)),
        }
    }
    Ok(a)
}

/// `gm --keytest`: prints each decoded key chord (Esc twice exits).
fn keytest(term: &mut Term) -> std::io::Result<()> {
    let mut log: Vec<String> = vec![
        "Goodevening Manager key test. Press keys; Esc twice to exit.".into(),
        format!("Keyboard enhancement (kitty protocol): {}", if term.has_enhanced_keys() { "yes" } else { "no" }),
        String::new(),
    ];
    let mut last_esc = false;
    loop {
        let lines = log.clone();
        term.draw(|buf| {
            let area = buf.area;
            let st = gm_term::style(gm_term::PaletteId::PanelText);
            gm_ui::draw::fill(buf, area, ' ', st);
            gm_ui::draw::frame(
                buf,
                area,
                true,
                gm_term::style(gm_term::PaletteId::PanelBox),
                Some(("gm --keytest", gm_term::style(gm_term::PaletteId::PanelTitle))),
            );
            let rows = area.height.saturating_sub(2) as usize;
            let start = lines.len().saturating_sub(rows);
            for (i, l) in lines[start..].iter().enumerate() {
                gm_ui::draw::put(buf, 2, 1 + i as u16, l, st, area.width.saturating_sub(4) as usize);
            }
            None
        })?;
        if let Some(ev) = term.next_event(Duration::from_secs(60))? {
            let text = match &ev {
                Event::Key(k) => {
                    if k.code == KeyCode::Esc && k.mods.0 == 0 {
                        if last_esc {
                            return Ok(());
                        }
                        last_esc = true;
                    } else {
                        last_esc = false;
                    }
                    format!("Key    {k}    ({:?}, mods {})", k.code, k.mods.0)
                }
                Event::Mouse(m) => format!("Mouse  {:?} at {},{} mods {:?}", m.kind, m.column, m.row, m.modifiers),
                Event::Resize(w, h) => format!("Resize {w}x{h}"),
                Event::Paste(s) => format!("Paste  {s:?}"),
                Event::Focus(f) => format!("Focus  {f}"),
            };
            log.push(text);
        }
    }
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("gm: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let profile = gm_config::init_profile(args.portable, args.profile.clone()).to_path_buf();
    gm_term::term::install_panic_hook(profile.clone());

    let settings_path = gm_config::settings_path();
    let (settings, settings_error) = Settings::load(&settings_path);
    let lang = args.lang.clone().unwrap_or_else(|| settings.language.clone());
    if !gm_i18n::set_language(&lang) {
        gm_i18n::set_language("English");
    }
    let bad_colors = palette::apply_overrides(&settings.colors);
    palette::set_color_mode(palette::detect_color_mode(args.colors.as_deref().unwrap_or(&settings.interface.colors)));

    let mut db = StateDb::open(&profile.join("state.db"));
    db.max_history = settings.system.history_size.max(10);
    let db = Rc::new(db);
    let keymap = Keymap::load(&gm_config::keymap_path());

    let mut term = match Term::new(settings.interface.mouse) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("gm: cannot initialize the terminal: {e}");
            return ExitCode::FAILURE;
        }
    };
    term.alt_keymap = settings.interface.alt_keymap && !cfg!(windows);
    if args.keytest {
        let r = keytest(&mut term);
        drop(term);
        return if r.is_ok() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }

    let mut ui = Ui::new(term, Box::new(DbHistory(db.clone())));
    ui.help = Some(gm_help::show);
    let start = (args.dirs.first().cloned().map(abs), args.dirs.get(1).cloned().map(abs));
    let mut app = App::new(ui, settings, settings_path, db, modules(), keymap, start);
    if let Some(e) = settings_error {
        app.pending_messages.push(("settings.toml".into(), e));
    }
    if !bad_colors.is_empty() {
        app.pending_messages.push(("settings.toml".into(), format!("Unknown colors: {}", bad_colors.join(", "))));
    }
    if let Some(v) = args.view {
        app.open_viewer(&abs(v));
    }
    if let Some((e, line)) = args.edit {
        app.open_editor(&abs(e), line);
    }
    let r = app.run();
    drop(app);
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gm: {e}");
            ExitCode::FAILURE
        }
    }
}

fn abs(p: PathBuf) -> PathBuf {
    if p.is_absolute() { p } else { std::env::current_dir().map(|c| c.join(&p)).unwrap_or(p) }
}

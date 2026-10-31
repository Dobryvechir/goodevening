# Goodevening Manager (gm)

A two-panel console file manager for **Windows, Linux and macOS**, written in
Rust. It was inspired by Midnight Commander and Far Manager 3. It has taken the best from both
Midnight Commander and Far Manager, and added new features and can run on Windows, Linux and MacOS.

```
╔n═════════════════════ C:\demo ══════════════════════╗
║           Name            │   Size   │  Date  │Time ║
║..                         │      <Up>│        │     ║
║sub                        │  <Folder>│30.10.26│19:40║
║alpha.txt                  │        26│30.10.26│19:40║
║beta.rs                    │        13│30.10.26│19:40║
╟───────────────────────────┴──────────┴────────┴─────╢
║alpha.txt                           26 30.10.26 19:40║
╚══════════ Bytes: 39, files: 2, folders: 1 ══════════╝
```

## Building

```sh
cargo build --release          # the binary is target/release/gm (gm.exe on Windows)
cargo test --workspace
```

The Rust toolchain is pinned in `rust-toolchain.toml`; `rustup` installs it
automatically. A C compiler is needed for the bundled SQLite and the
bzip2/xz decoders (MSVC on Windows, `cc` on Linux/macOS).

Step-by-step guides for installing the toolchain, building and distributing:
[Windows](docs/build/WindowsBuild.md),
[Ubuntu / WSL](docs/build/WSLUbuntuBuild.md),
[macOS](docs/build/MacOSBuild.md).

## Running

```
gm [options] [left_folder [right_folder]]

  -v <file>          open the file in the viewer
  -e[line] <file>    open the file in the editor
  -p, --portable     keep settings in gm-profile next to the executable
  --profile <dir>    use <dir> for settings and state
  --lang <name>      interface language (English, Russian, Ukrainian, …)
  --colors <mode>    auto, 16 or truecolor
  --pos <where>      move and resize the console window (see below)
  --lpath <dir>      current folder of the left panel (see below)
  --rpath <dir>      current folder of the right panel (see below)
  --keytest          show how key presses are decoded (for bug reports)
```

### Panel folders (`--lpath`, `--rpath`)

By default each panel opens the folder it showed when gm was last closed
(saved in the settings), or the current folder if that one no longer exists.
`--lpath <dir>` sets the current folder of the left panel and
`--rpath <dir>` the one of the right panel, overriding the saved settings.
Either can be given alone; the other panel then starts as usual.
They also take priority over the positional `left_folder`/`right_folder`.
Relative paths are resolved against the current folder; gm exits with an
error if the path is not an existing folder.

```
gm --lpath C:\Projects --rpath D:\Backup
gm --rpath ../other        # left panel as usual, right panel in ../other
```

### Console window position (`--pos`)

`--pos` moves and resizes the console window when gm starts:

| Value | Window |
|---|---|
| `f` | full size (maximized) |
| `n` | normal: as the system opened it (same as no `--pos`) |
| `l` | left half of the screen |
| `r` | right half of the screen |
| `t` | top half of the screen |
| `b` | bottom half of the screen |
| `lt` or `tl` | left top quarter of the screen |
| `lb` or `bl` | left bottom quarter of the screen |
| `rt` or `tr` | right top quarter of the screen |
| `rb` or `br` | right bottom quarter of the screen |

Example: `gm --pos l` in one console and `gm --pos r` in another.

Halves and quarters are taken from the work area of the screen that holds
the window (the taskbar, Dock or panels are excluded). Positioning is best
effort; where the system does not allow it, the option is silently ignored:

* **Windows**: the classic console window and Windows Terminal (the whole
  Terminal window moves, with all its tabs).
* **WSL**: the Windows console or Windows Terminal window, through
  `powershell.exe` (the window moves a second or two after gm starts).
* **macOS**: Terminal and iTerm2 through AppleScript; other terminals through
  System Events (needs the Accessibility permission), then xterm window
  sequences.
* **Linux on X11**: `wmctrl`, or `xdotool` (with `xprop`, `xwininfo` and
  `xrandr` for the work area and monitor) when installed; otherwise xterm
  window sequences (xterm, and terminals that support them). Wayland does not
  allow applications to place windows, so there only terminals that accept
  xterm window sequences react.

Settings live in `%APPDATA%\gm`, `~/.config/gm` or
`~/Library/Application Support/gm`:

| File | Content |
|---|---|
| `settings.toml` | all options (also editable with `gm:config` or F9 → Options) |
| `state.db` | command/folder/view histories, file positions, folder shortcuts (SQLite, safe for several instances) |
| `keymap.toml` | key overrides, e.g. `[panels]` `"Alt+Q" = "Quit"` |
| `GmMenu.ini` | the global user menu (F2); a `GmMenu.ini` in a folder is its local menu |

## What works

* **Panels**: file list with all ten view modes (Ctrl+0…9), Far column strings,
  sorting (Ctrl+F3…F12, natural order), selection (Ins, Shift+arrows, Gray
  `+ - *`, Ctrl+M), fast find (Alt+letters), highlighting rules, descriptions
  (`descript.ion`, Ctrl+Z), filters (Ctrl+I), info panel (Ctrl+L), tree
  panel (Ctrl+T), quick view (Ctrl+Q), hide/swap panels, drive/mount menu
  (Alt+F1/F2), automatic refresh, mouse.
* **File operations**: copy/move (F5/F6, Shift+F5/F6) with progress, speed,
  ETA, conflict dialog (overwrite / skip / rename / append / only newer /
  remember), error retry/skip; delete to the trash or permanently (F8,
  Shift+Del), wipe (Alt+Del), make folders (F7, several at once), hard and
  symbolic links (Alt+F6), attributes or Unix permissions (Ctrl+A), folder
  size (F3 on a folder).
* **Viewer** (F3): any file size (memory-mapped), text / wrap / word wrap /
  hex / dump modes, code pages with auto-detection (UTF-8, UTF-16, OEM, ANSI,
  KOI8…), search for text/hex/regex forward and backward, go to offset or
  percent, bookmarks, tail mode (Ctrl+F).
* **Editor** (F4): rope buffer, undo/redo, stream and vertical blocks,
  clipboard, search and replace (regex), go to line, code pages, line ending
  preservation and conversion (Shift+F2), read-only warning, external change
  detection, remembered positions, line numbers, F11 tools.
* **Find File** (Alt+F7): masks, text/hex/regex content in all standard code
  pages, search areas, streaming results with Go to / View / Edit / Panel.
* **Command line**: shell execution on the user screen (Ctrl+O), `cd`,
  `pushd`/`popd`, history (Alt+F8, Ctrl+E/Ctrl+X), name insertion
  (Ctrl+Enter, Ctrl+F, Ctrl+[ ]), prefixes, `gm:config`, `gm:about`.
* **Menus and options**: F9 menu bar, F2 user menu with Far metasymbols
  (`!.!`, `!\`, `!?prompt?default!`, …), file associations, folder shortcuts,
  histories, screens list (F12, Ctrl+Tab), settings dialogs, colors,
  languages (all 13 of Far's translations), help (F1, from Far's help files).

### Built-in modules (Far plugins, compiled in)

Each module is a crate behind a Cargo feature of `gm-app` (all on by default):

| Feature | Replaces | What it does |
|---|---|---|
| `archive` | arclite | zip (browse/extract/add/delete), tar, tar.gz/bz2/xz, gz, bz2, xz as panels; Shift+F1/F2 |
| `temp_panel` | tmppanel | temporary panel (`tmp:`), Find File → Panel |
| `process_panel` | proclist | process list (`proc:`), F8 terminates, F3 details |
| `network_panel` | network | network drives/mounts (`net:`, drive menu) |
| `compare` | compare | compare folders (selects differing files) |
| `cmd_prefixes` | farcmds | `view:` `edit:` `goto:` `clip:` `run:` `whereis:` `link:` |
| `editor_tools` | editcase, brackets, autowrap, align, drawline | change case, matching bracket, wrap, align, frame |
| `panel_tools` | filecase, samefolder | rename case, same folder in the other panel |
| `shell_menu` | emenu | open with the system handler; Properties on Windows |

Build without modules: `cargo build -p gm-app --no-default-features`.

## Not included (by design)

Lua macros, the plugin API and DLL loading, Far 1.x/2.x compatibility,
printing. See [docs/parity](docs/parity/README.md) for the detailed status of
every subsystem, and `RUST_PORT_PLAN.md` (in the Far source tree) for the plan.

## Terminals

On Windows every key combination works (native console input). On Linux and
macOS gm enables the kitty keyboard protocol where the terminal supports it
(kitty, WezTerm, foot, Ghostty, iTerm2, Alacritty); elsewhere some Ctrl/Alt
combinations cannot be told apart. `Esc` followed by a digit works as
F1…F10. On macOS Terminal.app enable "Use Option as Meta key". See
[docs/terminal-keys.md](docs/terminal-keys.md).

## Workspace

```
crates/gm-app        the `gm` binary, arguments, module registry
crates/gm-core       window manager, event loop, commands, menus, options
crates/gm-term       crossterm/ratatui backend, keys, palette, headless test mode
crates/gm-ui         frames, menus, key bar, line editor, dialog engine, messages
crates/gm-text       widths, code pages, EOL, Far masks, natural sort, regex
crates/gm-vfs        provider trait + local file system
crates/gm-fs         copy/move/delete/mkdir/links engine
crates/gm-platform   Windows/Unix adapters (the only crate with `unsafe`)
crates/gm-panels     file list, tree, info and quick view panels
crates/gm-viewer     F3 viewer
crates/gm-editor     F4 editor
crates/gm-search     Find File
crates/gm-config     settings.toml and state.db
crates/gm-i18n       messages (generated from Far's farlang.templ.m4)
crates/gm-help       HLF help viewer
crates/modules/*     built-in modules
tools/*              one-time converters for Far's language, help and palette files
```

`cargo test -p gm-core --test scripted_ui -- --nocapture gallery` prints the
main screens and dialogs as text.

## License

BSD 3-Clause

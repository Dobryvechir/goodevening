# Goodevening Manager (gm)

A two-panel console file manager for **Windows, Linux and macOS**, written in
Rust. It was inspired by Midnight Commander and Far Manager. It has taken the best from both
Midnight Commander and Far Manager, and added new features, and can run on Windows, Linux and MacOS.

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

## Downloading ready application for Windows, WSL, Linux.
 
   You can choose between building the application by yourself or downloading ready builds at
   our site https://movna-hatka.com/software/

## Building

Before building you must install the Rust toolchain.

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
gm [options] --client=<file> [left_folder [right_folder]]
gm [options] --client-push=<file> <command line>
gm [options] --server=<file> [folder]
gm --e|--d|--pe|--pd <key_file> <input_file> <output_file>

  -v <file>          open the file in the viewer
  -e[line] <file>    open the file in the editor
  -p, --portable     keep settings in gm-profile next to the executable
  --profile <dir>    use <dir> for settings and state
  --lang <name>      interface language (English, Russian, Ukrainian, …)
  --colors <mode>    auto, 16 or truecolor
  --pos <where>      move and resize the console window (see below)
  --lpath <dir>      current folder of the left panel (see below)
  --rpath <dir>      current folder of the right panel (see below)
  --client=<file>    send commands to the queue <file> instead of running them
  --client-push=<file>
                     add a command to the queue <file> (see below)
  --server=<file>    run commands from the queue <file> (see below)
  --keytest          show how key presses are decoded (for bug reports)
  --e, --d, --pe, --pd
                     encode / decode a file with a key file (see below)
```

### Encoding files (`--e`, `--d`, `--pe`, `--pd`)

`gm --e key_file input_file output_file` encodes `input_file` into
`output_file`, `gm --d` decodes it back. gm then only runs as this command
line utility (no panels). The result is byte for byte the same as
`python codec.py e|d key_file input_file output_file`.

The key file is scanned for `*`, `+` and `^`, each followed by hex digits;
the operations are applied in key order when encoding and undone in reverse
order when decoding:

| Op | Meaning |
|---|---|
| `^<hex>` | `data[i] ^= code[i % len]` (absolute file position) |
| `+<hex>` | `data[i] = data[i] + code[i % len]` (mod 256) |
| `*<hex>` | big-endian 2-byte numbers forming a permutation of 0..N-1; every full block of N bytes becomes `new[j] = old[code[j]]` |

`--pe` / `--pd` first ask for a password (hidden; read as one line when stdin
is redirected) and unmask the key file with it: `key[i] ^= password[i % len]`.

#### Encoded archives in the panels

**F9 → Commands → Encoded archives** (just above **Module commands**) lists
the configured encoded archives and adds (Ins or *Add new configuration…*),
changes (Enter) or deletes (Del) them. They are saved in `settings.toml`
right away:

```toml
[[encoded_archives]]
newExtension = "zipx"       # up to 7 characters
originalExtension = "zip"   # up to 7 characters
passwordProtected = true    # --pd / --pe
keyFile = "zipx.key"        # relative to the settings folder, or absolute
```

F3, F4 and Enter on a file named exactly `*.<newExtension>` decode it (as
`gm --d`, or `gm --pd` after asking for the password) into a private
temporary folder as `*.<originalExtension>` and open that copy in the viewer,
the editor, or (Enter) as an archive panel / with the system's program.
Whenever the copy is saved (F2 in the editor, files added to or deleted from
the archive panel, or another program writing it), gm encodes it back over
the original (as `gm --e` / `gm --pe`). The password is asked once per file
and kept in memory only, until the decoded copy is deleted.

A decoded copy is deleted (with its folder) as soon as gm no longer uses
it: when its viewer or editor is closed, or when its archive panel is left
(after the last change was encoded back). A copy opened with another
program is first left alone for 10 seconds, since that program starts in the
background; after that, gm tries to delete it after every key press and every
idle second. A copy that is locked (still open in a program) is silently kept
and tried again later. A copy whose encoding failed is never deleted, so the
changes are not lost. Anything left is deleted when gm exits.

### Command queue (`--client`, `--client-push`, `--server`)

One gm can run commands that other gm calls send to it, through a queue
file. `<file>` is a file name in the settings folder, the folder that holds
`settings.toml` (see the end of this section; `--profile` and `--portable`
change it). Both sides lock the file for each access, so any number of
clients and a server can use the same queue at the same time.

**`gm --client-push=<file> <command line>`** does not start the file manager and
does not run anything itself. It locks the queue file (creating it if
needed), reads it, appends the command line as a new last line, unlocks the
file and exits. Everything after `--client-push=<file>` is the command line, so
put the other options (`--profile`, `-p`) **before** it. Quote the command
when it contains shell operators (`&`, `|`, `>`, …), otherwise the calling
shell takes them. Line breaks in the command become spaces.

**`gm --client=<file>`** starts the file manager as usual, with all its
functions, but in *server execution mode*: whatever gm would run through the
shell is not run but added to the queue file, exactly like
`gm --client-push=<file> <command line>` does. This covers commands entered
on the command line at the bottom and files started from the left or right
panel (Enter or a double click); file associations and the user menu queue
their expanded command lines. Each such command is queued with a `cd` to the
folder of the active panel in front of it, so the server runs it where gm
would have: typing `cargo build` in `D:\Projects\demo` queues
`cd /d "D:\Projects\demo" && cargo build` (`cd '<folder>' && …` on Linux and
macOS). A file that is not executable is queued as the command that opens it
with the associated program (`start "" "<path>"` on Windows, `open` on macOS,
`xdg-open` elsewhere). Commands that gm handles itself still work normally:
`cd`, `pushd`/`popd`, drive changes, `exit`, module prefixes such as `proc:`,
the viewer and the editor.

**`gm --server=<file>`** does not show the panels or any window of gm; it
uses the console as a plain screen. It creates the queue file if needed and
waits until the file is not empty (it checks every 0.2 s). Then it locks the
file, reads the first line, removes it from the file and unlocks it, and runs
the line through the user's shell (`%ComSpec%` / `cmd.exe` on Windows,
`$SHELL` / `/bin/sh` elsewhere) in the folder the server was started in, or
in the folder given with `--lpath <dir>` (or as `folder`) when there is one. The
console shows the command itself, as `<folder>> <command>`, followed by all
its output; a non-zero exit code is shown as `[exit code N]`. Commands run one
after another, in the order they were queued; empty lines are skipped. The
server runs until it takes the command `exit_gm_server` from the queue (then
it deletes the queue file and exits; commands queued after it are lost with
the file), or until it is stopped with Ctrl+C or by closing the console.
`gm --client-push=<file> exit_gm_server` stops a server from any console.

```
gm --server=jobs.txt                           # in one console
gm --client-push=jobs.txt cargo build          # in any other console
gm --client-push=jobs.txt "dir /b & echo done" # quoted: & goes to the server
gm --client=jobs.txt                           # panels; Enter sends to the server
```

**F9 → Commands → Execute in other terminal** sets up a server and a
client in one step (see
[Running commands in a second terminal](#running-commands-in-a-second-terminal)).

```
C:\Projects\demo> cargo build
   Compiling demo v0.1.0 (C:\Projects\demo)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.52s
```

The queue is a plain text file with one command per line, so it can also be
filled by other programs, as long as they lock it the same way (or write it
in one go).

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

#### Moving the window while gm runs

The same positions are available at any time from the menu:
**F9 → Commands → Terminal position**. It opens a menu in the middle of the
screen with these items:

| Item | Same as |
|---|---|
| Full screen | `--pos f` |
| Left half | `--pos l` |
| Right half | `--pos r` |
| Top half | `--pos t` |
| Bottom half | `--pos b` |
| Left top | `--pos lt` |
| Left bottom | `--pos lb` |
| Right top | `--pos rt` |
| Right bottom | `--pos rb` |

Choosing an item moves and resizes the console window exactly as the
matching `--pos` value does at startup, and the panels are redrawn for the
new size. Where the system does not allow it (see the list above), nothing
happens.

#### Several gm windows side by side

**F9 → Commands → Multiple terminals** splits the screen between several
independent gm instances. It opens a menu in the middle of the screen:

| Item | This gm moves to | New gm instances |
|---|---|---|
| Left and right terminals | `--pos l` | one at `--pos r` |
| Top and bottom terminals | `--pos t` | one at `--pos b` |
| 4 terminals | `--pos lt` | three at `--pos rt`, `--pos lb`, `--pos rb` |

The running gm moves exactly as with **Terminal position**. Each new gm is
the same executable started in a new terminal window, as an independent
application, with

    gm --pos <part> --lpath <left folder> --rpath <right folder> --profile <profile> --lang <language>

so it opens with the same left and right folders, the same settings and the
same interface language. (A panel that shows no local folder, e.g. a network
panel, is not passed; the new gm then uses its saved folder.) Closing one gm
does not affect the others.

The new terminal window is opened with:

* **Windows**: a new console window, which is a new Windows Terminal window
  when Windows Terminal is the default terminal application.
* **WSL**: a new Windows console (or Windows Terminal) window running the same
  distribution (`cmd.exe /c start wsl.exe -d <distro> -e gm …`).
* **macOS**: a new iTerm2 window when gm runs in iTerm2, otherwise a new
  Terminal window.
* **Linux and other Unix desktops**: `$TERMINAL -e`, else the terminal gm runs
  in (kitty, Alacritty, WezTerm, Ghostty, Konsole, GNOME Terminal, foot), else
  the first one installed of `x-terminal-emulator`, `gnome-terminal`,
  `konsole`, `xfce4-terminal`, `mate-terminal`, `kitty`, `alacritty`,
  `wezterm`, `ghostty`, `foot`, `xterm`. Without a graphical session (no
  `DISPLAY` or `WAYLAND_DISPLAY`) no window can be opened.

If no terminal window can be opened, gm shows an error message. How well the
new windows are placed depends on the same system support as `--pos`.

#### Running commands in a second terminal

**F9 → Commands → Execute in other terminal** (just below **Multiple
terminals**) gives gm a second terminal for the programs it starts, using
the [command queue](#command-queue---client---client-push---server). It
opens a menu in the middle of the screen. What the menu shows depends on
whether gm is already in server execution mode (started with
`--client=<file>`, or connected earlier from this menu).

**Not connected.** The menu has four items:

| Item | This gm moves to | New gm (`--server`) at |
|---|---|---|
| Terminal at left | `--pos r` | `--pos l` |
| Terminal at right | `--pos l` | `--pos r` |
| Terminal at bottom | `--pos t` | `--pos b` |
| Terminal at top | `--pos b` | `--pos t` |

Choosing one of them:

1. Creates a new, empty queue file in the settings folder, with a name that
   no other file there has: `gm-queue-<process id>-<n>.txt`. The file is
   created with an exclusive "create new" call, so two gm instances never
   get the same name.
2. Moves this gm to the opposite half, exactly as **Terminal position**
   does.
3. Starts the same executable in a new terminal window (opened the same way
   as for **Multiple terminals**) with

       gm --pos <side> --lpath <folder> --profile <profile> --lang <language> --server=<queue>

   where `<folder>` is the folder of the active panel. A `--server` gm uses
   `--lpath` (or `left_folder`) as its current folder, so its prompt and the
   commands it runs start there.
4. Switches this gm to server execution mode with that queue, as if it had
   been started with `--client=<queue>`: commands typed on the command line,
   files started from the panels, file associations and the user menu are
   now run in the other terminal (each with a `cd` to the folder of the
   active panel in front of it).

If the new terminal window cannot be opened, gm shows an error message,
deletes the queue file it created and keeps running commands itself.

**Connected.** The menu has a single item, **Disconnect other terminal**.
Choosing it adds the command `exit_gm_server` to the queue file and leaves
server execution mode: gm runs commands itself again. The server first runs
the commands queued before `exit_gm_server`, then deletes the queue file and
exits (see `--server` above). This also applies when gm was started with
`--client=<file>`: the server of that queue stops. Choosing **Execute in
other terminal** again then offers the four positions and creates a new
queue.

Settings live in `%APPDATA%\gm`, `~/.config/gm` or
`~/Library/Application Support/gm`:

| File | Content |
|---|---|
| `settings.toml` | all options (also editable with `gm:config` or F9 → Options) |
| `state.db` | command/folder/view histories, file positions, folder shortcuts (SQLite, safe for several instances) |
| `keymap.toml` | key overrides, e.g. `[panels]` `"Alt+Q" = "Quit"` |
| `GmMenu.ini` | the global user menu (F2); a `GmMenu.ini` in a folder is its local menu |
| `<file>` of `--client`/`--client-push`/`--server` | the command queue, one command per line |
| `gm-queue-<pid>-<n>.txt` | queues created by F9 → Commands → Execute in other terminal (deleted by the server on Disconnect) |

Decoded copies of encoded archives live in `<temp>/gm-encoded-<pid>/` while
gm runs.

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
  Line endings: new lines follow the first line ending found in the file
  (CR+LF or LF); a new file, or one without line endings, uses the system
  default (CR+LF on Windows, LF elsewhere). **Ctrl+F8** opens the *Line
  endings* menu (`CR + LF` / `LF`): the whole file is converted to the chosen
  ending (undoable with Ctrl+Z) and new lines use it from then on.
* **Find File** (Alt+F7): masks, text/hex/regex content in all standard code
  pages, search areas, streaming results with Go to / View / Edit / Panel.
* **Command line**: shell execution on the user screen (Ctrl+O), `cd`,
  `pushd`/`popd`, history (Alt+F8, Ctrl+E/Ctrl+X), name insertion
  (Ctrl+Enter, Ctrl+F, Ctrl+[ ]), prefixes, `gm:config`, `gm:about`.
* **Menus and options**: F9 menu bar, F2 user menu with Far metasymbols
  (`!.!`, `!\`, `!?prompt?default!`, …), file associations, folder shortcuts,
  histories, screens list (F12, Ctrl+Tab), settings dialogs, colors,
  languages (all 13 of Far's translations plus Norwegian), help (F1, from Far's help files, plus a Norwegian translation).

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

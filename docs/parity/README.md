# Behavior parity with Far Manager 3

Status: ✅ done · 🟡 partial · ❌ not yet · 🚫 dropped by design.
Source of truth: the C++ modules and `FarEng.hlf.m4`.

## Panels (`filelist.cpp`, `panel.cpp`, `filepanels.cpp`)

| Feature | Status | Notes |
|---|---|---|
| Two panels, Tab, Ctrl+U swap, Ctrl+F1/F2/P hide | ✅ | |
| View modes Ctrl+0…9, Far column strings (N,S,P,D,T,DM,DC,DA,A,Z,O,LN,Cn) | ✅ | custom modes in settings: ❌ |
| Multi-column brief/medium, `}` for long names | ✅ | |
| Sort Ctrl+F3…F11, Ctrl+F12 menu, reverse, natural order | ✅ | sort groups ❌ |
| Selection: Ins, Shift+arrows, Gray + − *, Ctrl+M, Ctrl+Gray± same extension | ✅ | Shift+Gray± depends on the terminal |
| Fast find Alt+letters, Ctrl+Enter next, wildcards | ✅ | |
| Highlighting rules (masks, attributes, colors, marks) + editor | ✅ | "continue processing" ❌ |
| Descriptions `descript.ion`/`files.bbs`, Ctrl+Z | ✅ | |
| Filter Ctrl+I (masks, groups) | 🟡 | Far's full filter dialog (size/date/attributes) ❌ |
| Info panel Ctrl+L | ✅ | memory/power sections ❌ |
| Tree panel Ctrl+T, the other panel follows | ✅ | Alt+F10 tree search ❌ |
| Quick view Ctrl+Q (files, folder totals, provider items) | ✅ | |
| Status line, totals, free space, column titles, scroll bar | ✅ | |
| Drive menu Alt+F1/F2 (drives, mounts, home, modules) | ✅ | eject/disconnect 🚫 on Unix, ❌ on Windows |
| Folder shortcuts (Ctrl+Shift+0…9 set, Ctrl+Alt+0…9 go; Far uses RCtrl) | ✅ | |
| Auto refresh (`notify`), Ctrl+R | ✅ | |
| Mouse: click, double click, right click select, wheel, key bar, menu bar | ✅ | drag and drop ❌ |
| Ctrl+Up/Down height, Ctrl+Left/Right width | ✅ | |
| Ctrl+Ins / Ctrl+Shift+Ins copy names/paths | ✅ | |

## File operations (`copy.cpp`, `delete.cpp`, `mkdir.cpp`, `setattr.cpp`, `flink.cpp`)

| Feature | Status | Notes |
|---|---|---|
| Copy/move F5/F6, in place Shift+F5/F6, rename | ✅ | |
| Plan → execute, totals, progress, speed, ETA, taskbar progress | ✅ | runs in the UI thread with polling (see log) |
| Conflicts: overwrite/skip/rename/append/only newer/remember | ✅ | |
| Errors: retry/skip/skip all/cancel | ✅ | |
| Move = rename on the same volume, else copy + delete | ✅ | |
| Symlinks as links or targets, timestamps, permissions | ✅ | ACLs/streams/compression/encryption ❌ |
| Delete to trash / permanently / wipe | ✅ | |
| F7 with several names, nested paths | ✅ | link-at-create ❌ |
| Alt+F6 hard/symbolic links | ✅ | junctions ❌ |
| Ctrl+A Windows attributes, Unix permissions (3-state for several files) | ✅ | dates, owner, subfolders ❌ |
| F3 on a folder = folder size | ✅ | |
| Copy to/from providers (archives, temp panel) | ✅ | provider → provider ❌ |
| Elevation (UAC / sudo) | ❌ | Phase 9 |

## Viewer (`viewer.cpp`)

| Feature | Status | Notes |
|---|---|---|
| Any file size (memory map), special files, pipes | ✅ | |
| Wrap / word wrap / no wrap, horizontal scroll, arrows | ✅ | |
| Hex (F4), dump (Shift+F4) | ✅ | hex editing ❌ |
| Code pages: auto-detect, BOM, F8, Shift+F8 menu | ✅ | |
| Search F7 / Shift+F7 / Alt+F7: text, hex, regex, case, words, wrap around | ✅ | |
| Go to Alt+F8 (offset, hex, percent) | ✅ | |
| Bookmarks Ctrl+Shift+0…9 / Ctrl+0…9 | ✅ | |
| Tail mode (Ctrl+F), refresh on growth | ✅ | |
| F6 → editor, F12, Ctrl+Tab, Ctrl+O | ✅ | |
| Saved position and code page | ✅ | |
| Selection and copy to clipboard | ❌ | |

## Editor (`editor.cpp`, `edit.cpp`)

| Feature | Status | Notes |
|---|---|---|
| Rope buffer, large files | ✅ | |
| Undo/redo (Ctrl+Z, Alt+BS / Ctrl+Shift+Z), grouped typing | ✅ | |
| Stream blocks (Shift), vertical blocks (Alt+Shift), persistent blocks option | ✅ | |
| Clipboard (Ctrl+C/X/V, Ctrl+Ins, Shift+Ins/Del), Ctrl+P/Ctrl+M copy/move block | ✅ | |
| Search/replace (F7, Ctrl+F7, Shift+F7, Alt+F7), regex, confirm | ✅ | "find all" ❌ |
| Go to line[:col] Alt+F8, bookmarks | ✅ | |
| Code page F8 / Shift+F8 (re-decode), BOM, EOL preserve/convert, Save as | ✅ | |
| Auto indent, tabs/expand, show whitespace, line numbers (Ctrl+F3) | ✅ | |
| Read-only warning, lock, external change detection | ✅ | |
| Saved position | ✅ | |
| Cursor beyond end of line | ❌ | |
| Editing files inside archives (extract → edit → put back) | ❌ | the viewer is opened instead |
| Tools (F11): case, brackets, wrap, align, frame | ✅ | interactive line drawing ❌ |

## Search and command line

| Feature | Status | Notes |
|---|---|---|
| Find File: masks, text/hex/regex, all code pages, areas, not containing, folders | ✅ | search in archives ❌, advanced filter ❌ |
| Results: Go to, View, Edit (F4), Panel, New search, Stop | ✅ | |
| Find folder Alt+F10 | 🟡 | uses Find File with folders only |
| Command line: shell execution, user screen Ctrl+O | ✅ | embedded PTY (Phase 9) ❌ |
| cd, cd.., X:, pushd/popd, exit, env expansion | ✅ | |
| History Alt+F8, Ctrl+E/X, lock, delete, clear | ✅ | |
| Autocomplete | ❌ | |
| Prefixes (modules), gm:config, gm:about | ✅ | |

## Menus, options, help

| Feature | Status | Notes |
|---|---|---|
| F9 menu bar with all five menus | ✅ | some items open simplified dialogs |
| User menu F2, local/global GmMenu.ini, submenus, F-key hotkeys | ✅ | in-menu editing (Ins/Del) ❌ (F4 opens the file) |
| Metasymbols `!! !.! ! !~ !` !\ !=\ !: !& !@! !# !^ !?…!` | ✅ | short names 🚫 |
| File associations | ✅ | |
| Options: system, panels, interface, confirmations, viewer, editor, languages, colors, highlighting, mask groups | ✅ | tree, dialog, menu, command line, autocomplete, info panel settings ❌ |
| gm:config (every setting) | ✅ | |
| Colors (all 147 palette elements) | ✅ | 256/RGB color picker ❌ |
| 13 languages, runtime switching | ✅ | |
| Help: 8 languages, links, index, back, F1 per window | ✅ | search in help ❌; plugin/Lua topics removed |
| Screens list F12, Ctrl+Tab | ✅ | |
| Clock, terminal title, screens number | ✅ | screensaver ❌ |
| Keymap overrides (`keymap.toml`) | ✅ | panels only |

## Dropped (🚫)

Lua macros and `macro:` / `lua:` / `moon:` prefixes, the plugin API and
manager, `load:`/`unload:` (accepted with a message), ANSI plugins, printing,
Windows XP–7 code paths, Alt+F9 window resize outside Windows.

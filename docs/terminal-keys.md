# Terminal keys

Windows commanders rely on key combinations that classic Unix terminals cannot report.
gm decodes keys like this:

| Platform | Mechanism | Fidelity |
|---|---|---|
| Windows (conhost, Windows Terminal) | native console input through crossterm | all combinations |
| kitty, WezTerm, foot, Ghostty, iTerm2 3.5+, Alacritty 0.13+ | kitty keyboard protocol (enabled automatically) | nearly all |
| GNOME Terminal (VTE), xterm, tmux, screen | legacy escape sequences | see below |
| macOS Terminal.app | legacy; needs "Use Option as Meta key" | see below |

With legacy terminals:

* `Ctrl+Enter` usually arrives as `Ctrl+J`; gm binds both to "insert name".
* `Ctrl+Shift+letter` arrives as `Ctrl+letter`.
* `Shift+Gray+` / `Shift+Gray-` are indistinguishable from `+` / `-`.
* F-keys beyond F12 or with several modifiers may be missing:
  **Esc followed by a digit** acts as F1…F10 (Esc 0 = F10). This is on by
  default outside Windows (F9 → Options → Interface settings).
* Other managers `RightCtrl+0…9` (folder shortcuts) is `Ctrl+Alt+0…9` in gm.

Every binding can be changed in `keymap.toml`:

```toml
[panels]
"Alt+Q" = "Quit"
"Ctrl+F10" = ""          # unbind
"F10" = "Menu"
```

Run `gm --keytest` to see exactly what your terminal sends; please attach its
output to keyboard bug reports.

## Results to collect

None of these have been measured yet; fill the table from `gm --keytest` runs.

| Terminal | Ctrl+Enter | Ctrl+Shift+letters | Alt+Shift+F-keys | Notes |
|---|---|---|---|---|
| Windows Terminal | | | | expected: all (native input) |
| conhost | | | | expected: all (native input) |
| kitty | | | | |
| iTerm2 | | | | |
| GNOME Terminal | | | | |

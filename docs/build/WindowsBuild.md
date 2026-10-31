# Building Goodevening Manager on Windows

These steps are for Windows 10 and Windows 11 on x64. The result is one
self-contained program, `gm.exe`.

## 1. Install the Rust toolchain

gm uses the MSVC toolchain (`x86_64-pc-windows-msvc`). It needs the Microsoft
C/C++ build tools, because SQLite and the bzip2/xz decoders are compiled from C
sources during the build.

1. **Microsoft C++ Build Tools.** Install "Build Tools for Visual Studio 2022"
   (or any Visual Studio 2022 edition) with the **Desktop development with
   C++** workload. Make sure these components are selected:
   * MSVC v143 – VS 2022 C++ x64/x86 build tools
   * Windows 11 SDK (or Windows 10 SDK)

   From an administrator PowerShell:

   ```powershell
   winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   ```

2. **rustup.** Download and run `rustup-init.exe` from
   <https://rustup.rs>, or:

   ```powershell
   winget install Rustlang.Rustup
   ```

   Accept the default host triple `x86_64-pc-windows-msvc`.

3. **Git** (to get the sources): `winget install Git.Git`.

Open a **new** terminal after the installation so that `cargo` is on `PATH`.

You don't need to pick a Rust version yourself. `rust-toolchain.toml` pins
the version (currently 1.90.0, with `rustfmt` and `clippy`), and `rustup`
downloads it automatically the first time you run `cargo` in the source
folder. To check:

```powershell
cd goodevening
rustup show active-toolchain
```

## 2. Build

```powershell
git clone https://github.com/Dobryvechir/goodevening.git
cd goodevening
cargo build --release
```

The program is `target\release\gm.exe`.

Other useful commands:

| Command | Purpose |
|---|---|
| `cargo build` | debug build, `target\debug\gm.exe` |
| `cargo test --workspace` | run all tests |
| `cargo clippy --workspace --all-targets -- -D warnings` | lint, as CI does |
| `cargo fmt --all --check` | formatting check, as CI does |
| `cargo build --release -p gm-app --no-default-features` | build without the built-in modules |

### Recommended for distribution: static C runtime

By default `gm.exe` imports `VCRUNTIME140.dll`, which is not part of a clean
Windows installation. Link the C runtime statically, and `gm.exe` needs
nothing else:

```powershell
$env:RUSTFLAGS = "-C target-feature=+crt-static"
cargo build --release
Remove-Item Env:RUSTFLAGS
```

(in `cmd.exe`: `set RUSTFLAGS=-C target-feature=+crt-static`)

Changing `RUSTFLAGS` rebuilds everything; that's expected.

### Optional: Windows on ARM

```powershell
rustup target add aarch64-pc-windows-msvc
cargo build --release --target aarch64-pc-windows-msvc
```

You also need the "MSVC v143 – VS 2022 C++ ARM64 build tools" component.
The result is in `target\aarch64-pc-windows-msvc\release\gm.exe`.

## 3. Files to distribute

Distribute only **one file**:

```
target\release\gm.exe
```

Everything else is built into it:

* interface languages (`assets\lang\*.lng`), embedded at compile time;
* help files (`assets\help\*.hlf`), embedded at compile time;
* all built-in modules (archive, temporary panel, process panel, network
  panel, compare, command prefixes, editor and panel tools, shell menu);
* SQLite (for `state.db`), statically linked.

Do **not** distribute anything else from `target\release`:

| File / folder | What it is |
|---|---|
| `gm.pdb` | debug symbols; keep them yourself for analysing crash dumps |
| `gm.d` | Cargo dependency list, build only |
| `build\`, `deps\`, `examples\`, `incremental\`, `.fingerprint\`, `.cargo-lock` | intermediate build files |

You don't need the `assets` folder of the source tree at runtime either.
You may add `README.md` and `LICENSE` (BSD 3-Clause, which requires the
license text to accompany binary distributions).

A typical release archive:

```
gm-<version>-windows-x64.zip
├── gm.exe
├── LICENSE
└── README.md
```

## 4. Additional requirements for distribution

### Runtime requirements on the target computer

* Windows 10 (version 1607 or later) or Windows 11, x64.
* **Visual C++ Redistributable 2015–2022 (x64)**, **only** if `gm.exe` was
  built *without* `+crt-static`. Download it from
  <https://aka.ms/vs/17/release/vc_redist.x64.exe> or install it with
  `winget install Microsoft.VCRedist.2015+.x64`. Without it Windows reports
  "VCRUNTIME140.dll was not found".
* All other DLLs that `gm.exe` uses are standard parts of Windows (kernel32,
  user32, shell32, advapi32, ole32, dwmapi, mpr, pdh, the Universal C
  Runtime, …).
* A console: Windows Terminal (recommended, true colour is detected
  automatically), the classic console, `cmd.exe` or PowerShell.

To check which DLLs a build imports (from a "Developer PowerShell for VS"):

```powershell
dumpbin /dependents target\release\gm.exe
```

### Installation on the target computer

* Copy `gm.exe` to any folder, e.g. `%LOCALAPPDATA%\Programs\gm\` (current
  user) or `C:\Program Files\gm\` (all users, needs administrator rights).
* Optionally add that folder to `PATH` so that `gm` starts from any terminal.
* Settings are created automatically in `%APPDATA%\gm\` (`settings.toml`,
  `state.db`, optionally `keymap.toml` and `GmMenu.ini`).
* Portable use (USB stick): start `gm -p`. Settings then go to `gm-profile`
  next to `gm.exe`, so that folder must be writable (not `Program Files`).

See also `WindowsInstallation` in the repository root.

### Code signing (recommended for public downloads)

An unsigned `gm.exe` downloaded from the internet triggers Microsoft Defender
SmartScreen ("Windows protected your PC"). Users can choose *More info → Run
anyway*. To avoid the warning, sign the file with a code-signing certificate:

```powershell
signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /a target\release\gm.exe
```

`signtool` comes with the Windows SDK. Sign **before** you put the file in the
zip archive. Some antivirus products flag unsigned console tools; signing
reduces that too.

### Publishing through winget (optional)

To make gm installable with `winget install`, publish the zip (or an MSI/MSIX
package) on a stable URL, such as a GitHub release, and submit a manifest to
<https://github.com/microsoft/winget-pkgs>. The zip installer type with a
`NestedInstallerFiles` entry for `gm.exe` and the portable command alias `gm`
is enough for a single-file program.

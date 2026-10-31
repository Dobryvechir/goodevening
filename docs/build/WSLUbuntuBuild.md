# Building Goodevening Manager on Ubuntu (native or WSL)

These steps are for Ubuntu 22.04 / 24.04 on x86-64 or ARM64. They work the
same way on a native Ubuntu installation and under WSL 2 on Windows. Other
Debian-based distributions work the same; for other distributions use their
package manager for the packages in step 1.1.

The result is one program, `gm`.

## 1. Install the Rust toolchain

### 1.1 System packages

The build compiles SQLite and the bzip2/xz decoders from C sources, so a C
compiler is needed:

```sh
sudo apt update
sudo apt install -y build-essential curl git
```

`pkg-config`, `liblzma-dev` and `libbz2-dev` are **not** needed. If they are
installed, the build may link the system copies of liblzma/libbz2
dynamically instead (see section 4).

### 1.2 rustup

Install Rust with `rustup`, not with `apt install rustc cargo`, because the
distribution's packages are usually older than the version gm requires:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
```

You don't need to pick a Rust version yourself. `rust-toolchain.toml` pins
the version (currently 1.90.0, with `rustfmt` and `clippy`), and `rustup`
downloads it automatically the first time you run `cargo` in the source
folder:

```sh
cd goodevening
rustup show active-toolchain
```

### 1.3 WSL notes

* Use **WSL 2** (`wsl --set-version Ubuntu 2` in Windows PowerShell).
* Keep the sources in the Linux file system (e.g. `~/src/goodevening`), not
  under `/mnt/c/...`. Builds on `/mnt/c` are many times slower and file
  permissions there are emulated.
* Building inside WSL produces a **Linux** binary. It doesn't run on Windows
  itself; to get `gm.exe`, follow `WindowsBuild.md` on Windows.

## 2. Build

```sh
git clone https://github.com/Dobryvechir/goodevening.git
cd goodevening
cargo build --release
```

The program is `target/release/gm`.

Other useful commands:

| Command | Purpose |
|---|---|
| `cargo build` | debug build, `target/debug/gm` |
| `cargo test --workspace` | run all tests |
| `cargo clippy --workspace --all-targets -- -D warnings` | lint, as CI does |
| `cargo fmt --all --check` | formatting check, as CI does |
| `cargo build --release -p gm-app --no-default-features` | build without the built-in modules |

### Recommended for distribution: self-contained xz

If `liblzma-dev` and `pkg-config` are installed, the `xz` support links the
system `liblzma.so.5` dynamically. For a binary that doesn't depend on it:

```sh
LZMA_API_STATIC=1 cargo build --release
```

### Building for older distributions

A Linux binary works on distributions whose **glibc is the same or newer**
than the glibc of the machine where it was built. Build on the **oldest**
distribution you want to support. For example, a binary built on Ubuntu 22.04
also runs on 24.04, but not the other way round. The CI builds on
`ubuntu-latest`, so those binaries need a recent glibc.

### Optional: ARM64 (aarch64)

On an ARM64 machine (or ARM64 WSL) the commands are the same. Cross-compiling
from x86-64 needs a cross C compiler for the C sources:

```sh
sudo apt install -y gcc-aarch64-linux-gnu
rustup target add aarch64-unknown-linux-gnu
CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
cargo build --release --target aarch64-unknown-linux-gnu
```

The result is in `target/aarch64-unknown-linux-gnu/release/gm`.

## 3. Files to distribute

Distribute only **one file**:

```
target/release/gm
```

Everything else is built into it:

* interface languages (`assets/lang/*.lng`), embedded at compile time;
* help files (`assets/help/*.hlf`), embedded at compile time;
* all built-in modules (archive, temporary panel, process panel, network
  panel, compare, command prefixes, editor and panel tools, shell menu);
* SQLite (for `state.db`), statically linked;
* the X11 clipboard client (pure Rust, no libxcb needed).

Do **not** distribute anything else from `target/release` (`gm.d`, `build/`,
`deps/`, `examples/`, `incremental/`, `.fingerprint/`, `.cargo-lock` are
intermediate build files). Release builds are already stripped (`strip = true`
in `Cargo.toml`). You don't need the `assets` folder at runtime. You may add
`README.md` and `LICENSE` (BSD 3-Clause, which requires the license text to
accompany binary distributions).

A typical release archive (keep the executable bit, so use tar, not zip):

```sh
mkdir -p dist/gm-<version>-linux-x86_64
cp target/release/gm LICENSE README.md dist/gm-<version>-linux-x86_64/
tar -C dist -czf gm-<version>-linux-x86_64.tar.gz gm-<version>-linux-x86_64
```

## 4. Additional requirements for distribution

### Check the shared library dependencies

```sh
ldd target/release/gm
```

A self-contained build shows only the C library and the dynamic loader,
which every glibc-based distribution has:

```
linux-vdso.so.1
libgcc_s.so.1
libm.so.6
libc.so.6
/lib64/ld-linux-x86-64.so.2
```

If `liblzma.so.5` or `libbz2.so.1.0` show up, the build linked the system
libraries (because `pkg-config` and the `-dev` packages were installed). Both
are installed on practically every Ubuntu/Debian system, so this is usually
fine. To avoid the dependency, build with `LZMA_API_STATIC=1` (xz), or build
on a machine without `libbz2-dev` (bzip2 is then compiled in).

To check the oldest glibc the binary needs:

```sh
objdump -T target/release/gm | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1
```

### Runtime requirements on the target computer

* Linux with glibc (Ubuntu, Debian, Fedora, …) on the same CPU architecture,
  with a glibc version at least as new as the one shown above. Alpine and
  other musl-based distributions are not supported by this build.
* A terminal emulator. For full keyboard support use one with the kitty
  keyboard protocol (kitty, WezTerm, foot, Ghostty, Alacritty); see
  `docs/terminal-keys.md`. Under WSL, Windows Terminal is recommended.
* Optional runtime helpers:
  * **Clipboard**: an X11 display (`DISPLAY`), or XWayland on Wayland
    desktops. WSLg provides one in WSL. Without it gm uses an internal
    clipboard and sends OSC 52 so that terminals can copy to the local
    clipboard (also over SSH).
  * **Opening files with the system handler**: `xdg-open` (package
    `xdg-utils`).
  * **`gm --pos`** (window placement): `wmctrl` or `xdotool` (plus `xprop`,
    `xwininfo` and `xrandr` from `x11-utils` / `x11-xserver-utils`) on X11.
    Under WSL it uses `powershell.exe` from Windows; that requires Windows
    interop, which is on by default.
  * **Trash**: works with the freedesktop.org trash in
    `~/.local/share/Trash`. No extra package needed.

### Installation on the target computer

```sh
sudo install -m 755 gm /usr/local/bin/gm     # all users
# or
install -D -m 755 gm ~/.local/bin/gm         # current user (~/.local/bin must be on PATH)
```

Settings are created automatically in `~/.config/gm/` (`settings.toml`,
`state.db`, optionally `keymap.toml` and `GmMenu.ini`). Portable use: `gm -p`
keeps them in `gm-profile` next to the binary, so that folder must be
writable.

### Packaging (optional)

For a `.deb` package, `cargo install cargo-deb` and add a
`[package.metadata.deb]` section to `crates/gm-app/Cargo.toml`, then run
`cargo deb -p gm-app`. Put the runtime helpers above in `Recommends:`, not
`Depends:`, because gm works without them.

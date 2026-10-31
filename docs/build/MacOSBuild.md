# Building Goodevening Manager on macOS

These steps are for macOS 11 (Big Sur) or later on Apple Silicon (arm64) or
Intel (x86_64). The result is one program, `gm`.

## 1. Install the Rust toolchain

### 1.1 Xcode Command Line Tools

The build compiles SQLite and the bzip2/xz decoders from C sources, and needs
Apple's C compiler and linker:

```sh
xcode-select --install
```

The full Xcode is not required. To check: `clang --version`.

### 1.2 rustup

Install Rust with `rustup`. Homebrew's `rust` formula can't follow the
version that `rust-toolchain.toml` pins.

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

## 2. Build

```sh
git clone https://github.com/Dobryvechir/goodevening.git
cd goodevening
LZMA_API_STATIC=1 cargo build --release
```

The program is `target/release/gm`. It is built for the architecture of the
Mac you build on.

`LZMA_API_STATIC=1` compiles the xz decoder into the binary. Without it, if
Homebrew's `xz` and `pkg-config` are installed, `gm` links
`/opt/homebrew/lib/liblzma.5.dylib` (or `/usr/local/lib/...`), and then the
binary fails to start on Macs without Homebrew's xz. Always set it for builds
you distribute. bzip2 needs nothing, because macOS includes `libbz2`.

Other useful commands:

| Command | Purpose |
|---|---|
| `cargo build` | debug build, `target/debug/gm` |
| `cargo test --workspace` | run all tests |
| `cargo clippy --workspace --all-targets -- -D warnings` | lint, as CI does |
| `cargo fmt --all --check` | formatting check, as CI does |
| `cargo build --release -p gm-app --no-default-features` | build without the built-in modules |

### Recommended for distribution: universal binary

One file that runs natively on both Apple Silicon and Intel Macs:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
export LZMA_API_STATIC=1
export MACOSX_DEPLOYMENT_TARGET=11.0
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
mkdir -p dist
lipo -create -output dist/gm \
    target/aarch64-apple-darwin/release/gm \
    target/x86_64-apple-darwin/release/gm
lipo -info dist/gm        # Architectures in the fat file: x86_64 arm64
```

`MACOSX_DEPLOYMENT_TARGET` sets the oldest macOS version the binary runs on.
It applies to both the Rust code and the C sources. 11.0 is the oldest version
supported for Apple Silicon.

## 3. Files to distribute

Distribute only **one file**: `target/release/gm`, or `dist/gm` for the
universal binary.

Everything else is built into it:

* interface languages (`assets/lang/*.lng`), embedded at compile time;
* help files (`assets/help/*.hlf`), embedded at compile time;
* all built-in modules (archive, temporary panel, process panel, network
  panel, compare, command prefixes, editor and panel tools, shell menu);
* SQLite (for `state.db`), statically linked;
* xz, when built with `LZMA_API_STATIC=1`.

Do **not** distribute anything else from `target/` (`gm.d`, `build/`,
`deps/`, `examples/`, `incremental/`, `.fingerprint/`, `.cargo-lock` are
intermediate build files). Release builds are already stripped. You don't
need the `assets` folder at runtime. You may add `README.md` and `LICENSE`
(BSD 3-Clause, which requires the license text to accompany binary
distributions).

gm is a command-line program, not an `.app` bundle. Distribute it as a
`.tar.gz` or `.zip` (or through Homebrew, see below):

```sh
mkdir -p dist/gm-<version>-macos-universal
cp dist/gm LICENSE README.md dist/gm-<version>-macos-universal/
ditto -c -k --keepParent dist/gm-<version>-macos-universal gm-<version>-macos-universal.zip
```

## 4. Additional requirements for distribution

### Check the library dependencies

```sh
otool -L dist/gm
```

A correct build references only system libraries and frameworks under
`/usr/lib` and `/System/Library/Frameworks` (libSystem, libiconv, libbz2,
CoreFoundation, CoreServices, IOKit, AppKit, …). If you see any path under
`/opt/homebrew` or `/usr/local`, rebuild with `LZMA_API_STATIC=1`. Otherwise
the binary only works on Macs that have the same Homebrew packages.

### Code signing and notarization (required for downloads)

Files downloaded with a browser get the `com.apple.quarantine` attribute, and
Gatekeeper blocks an unsigned or non-notarized binary ("gm cannot be opened
because the developer cannot be verified" / "Apple could not verify…").

* **For yourself or a few users**: the linker already signs arm64 binaries
  ad hoc. Users can remove the quarantine attribute after downloading:

  ```sh
  xattr -d com.apple.quarantine ./gm
  ```

  `lipo` invalidates the ad-hoc signatures of the parts. Re-sign the
  universal binary ad hoc: `codesign --force --sign - dist/gm`.

* **For public distribution**: sign with a *Developer ID Application*
  certificate (Apple Developer Program) and the hardened runtime, then
  notarize:

  ```sh
  codesign --force --options runtime --timestamp \
      --sign "Developer ID Application: <Name> (<TEAMID>)" dist/gm
  ditto -c -k --keepParent dist/gm-<version>-macos-universal gm-<version>-macos-universal.zip
  xcrun notarytool submit gm-<version>-macos-universal.zip \
      --apple-id <apple-id> --team-id <TEAMID> --password <app-specific-password> --wait
  ```

  You can't staple a notarization ticket to a bare executable or a zip.
  Gatekeeper checks it online on first launch. If you package gm in a signed
  `.pkg` or `.dmg`, notarize that package and staple it with
  `xcrun stapler staple`.

Sign **after** `lipo` and **before** you create the archive. Any change to the
file afterwards breaks the signature.

### Runtime requirements on the target computer

* macOS 11 or later (or the `MACOSX_DEPLOYMENT_TARGET` you built with). The
  architecture must match, unless you ship the universal binary. Apple Silicon
  Macs can also run an x86_64 build through Rosetta 2.
* A terminal: Terminal.app, iTerm2, WezTerm, kitty, Ghostty, Alacritty, …
  For full keyboard support use one with the kitty keyboard protocol. In
  Terminal.app enable *Settings → Profiles → Keyboard → Use Option as Meta
  key*. See `docs/terminal-keys.md`.
* **`gm --pos`** (window placement) uses AppleScript. On first use macOS asks
  to let the terminal control itself ("… wants access to control
  Terminal/iTerm"); allow it under *System Settings → Privacy & Security →
  Automation*. Other terminals are moved through System Events, which needs
  the terminal to be allowed under *Privacy & Security → Accessibility*.
  Without the permission, `--pos` is silently ignored.
* Clipboard, trash and opening files (`open`) use built-in macOS services. No
  extra software is needed.

### Installation on the target computer

```sh
sudo install -m 755 gm /usr/local/bin/gm     # all users
# or
mkdir -p ~/.local/bin && install -m 755 gm ~/.local/bin/gm   # add ~/.local/bin to PATH
```

Settings are created automatically in `~/Library/Application Support/gm/`
(`settings.toml`, `state.db`, optionally `keymap.toml` and `GmMenu.ini`).
Portable use: `gm -p` keeps them in `gm-profile` next to the binary, so that
folder must be writable.

### Homebrew (optional)

To make gm installable with `brew install`, publish the release archive on a
stable URL (for example a GitHub release) and create a formula in your own
tap (`brew tap-new <user>/tap`). The formula downloads the archive, checks its
`sha256` and installs `gm` with `bin.install "gm"`. Homebrew installs don't
get the quarantine attribute, so Gatekeeper doesn't block them.

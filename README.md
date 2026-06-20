# TEKFLASH

[![CI](https://img.shields.io/github/actions/workflow/status/tekk/tekflash/ci.yml?branch=main&label=ci&logo=github)](https://github.com/tekk/tekflash/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/actions/workflow/status/tekk/tekflash/release.yml?label=release&logo=github)](https://github.com/tekk/tekflash/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/tekk/tekflash?logo=github&sort=semver)](https://github.com/tekk/tekflash/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/tekk/tekflash/total?logo=github)](https://github.com/tekk/tekflash/releases)
[![License](https://img.shields.io/github/license/tekk/tekflash)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.82%2B-orange?logo=rust)](https://www.rust-lang.org)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-blue)](https://github.com/tekk/tekflash/releases/latest)
[![Stars](https://img.shields.io/github/stars/tekk/tekflash?style=flat&logo=github)](https://github.com/tekk/tekflash/stargazers)
[![Issues](https://img.shields.io/github/issues/tekk/tekflash?logo=github)](https://github.com/tekk/tekflash/issues)
[![Last commit](https://img.shields.io/github/last-commit/tekk/tekflash?logo=github)](https://github.com/tekk/tekflash/commits/main)

A safe, fast, cross-platform TUI for flashing, backing up, and restoring block devices -
SD cards, USB sticks, and other removable media. Works on macOS, Linux, and Windows.

## Demo

[![asciicast](https://asciinema.org/a/T2lK4nTDnpQ5VNWk.svg)](https://asciinema.org/a/T2lK4nTDnpQ5VNWk)

## Install

### Via cargo (from source)

```sh
# Prereq: a recent stable Rust toolchain (rustc 1.82+). If you don't have one:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install the latest tekflash from main into ~/.cargo/bin/tekflash
cargo install --git https://github.com/tekk/tekflash --bin tekflash --locked

# Or pin to a released tag for a reproducible install
cargo install --git https://github.com/tekk/tekflash --tag v0.0.4 --bin tekflash --locked

# Verify
tekflash --version
sudo tekflash --check        # macOS / Linux: confirms elevated capability
```

`--locked` uses the committed `Cargo.lock` so transitive dependency versions match
what was tested in CI. Drop it if you want the freshest deps.

### Pre-built binaries

Static binaries are attached to each [release](https://github.com/tekk/tekflash/releases)
for `x86_64`/`aarch64` Linux musl, `x86_64`/`aarch64` macOS, and `x86_64`/`aarch64`
Windows MSVC.

## Quick start

```sh
# Launch the TUI (most users start here)
sudo tekflash

# Flash an ISO with full verify
sudo tekflash flash ~/Downloads/ubuntu-24.04.iso /dev/disk5 --verify=full

# Bit-exact backup of an SD card, zstd-19
sudo tekflash backup /dev/disk5 sd-card.img.zst --codec zstd --level 19

# File-level archive with PQ-safe password encryption
sudo tekflash archive /Volumes/MyDisk backup.tar.zst --encrypt password

# Run `sudo tekflash <subcommand> --help` for many more worked examples.
```

On Windows, run from an elevated PowerShell (or right-click -> Run as administrator)
without `sudo`. Device paths look like `\\.\PhysicalDrive2` or just `E:`.

## What's in the TUI

- **Device picker** — every removable disk is listed with vendor, model, size, bus,
  mountpoints. Internal / boot disks are hidden by default; press `a` to reveal them.
- **Action picker** — Enter on a device opens Flash / Backup / Archive with one-line
  trade-off explanations. Internal disks get a red header so destructive choices are
  obvious.
- **Codec picker** — zstd / lz4 / brotli / xz / gzip / bzip2 / none, each with a
  one-line blurb, a level (per-codec range, codec remembers its choice), and rough
  size / speed bars. Picked codec auto-appends the right extension (`.img.zst`,
  `.tar.zst`, etc.) in the file browser.
- **File browser** — type-ahead filter in Open mode, type-the-filename in Save mode
  with the auto-extension shown in grey as you type. `..` row to walk up; `.` row
  (Save mode) to commit in the current directory. Backspace pops typed characters
  before walking up.
- **Live progress view (Backup + Archive)** — runs in a background thread, doesn't
  block the TUI:
    - bytes read / total / percentage gauge
    - rate (now) and rate (avg) in **`MB/s` and `GB/min`**
    - elapsed + ETA in `HH:MM:SS`
    - estimated output file size projected from the live compression ratio
    - BLAKE3 digest when finished
    - **defrag-style block map** — each cell ≈ `total / cells` bytes, shades full /
      partial / empty as the source is read
    - for archives, a `current` row shows the file being added to the tar
- **Concurrent sessions** — start several backups / archives at once, **Tab** cycles
  through them and back to the home view, mini progress bars sit at the bottom of
  the home view showing each session's status / rate / destination. Sessions keep
  running when you Esc out of their full view; the worker thread streams in the
  background until completion.
- **macOS niceties** — backups use `/dev/rdiskN` automatically for max throughput;
  archive auto-mounts via `diskutil mountDisk` if the source volume isn't already
  mounted; `.Spotlight-V100` / `.fseventsd` / `.Trashes` etc. are skipped so SIP
  ACLs don't fail the archive.

## Building

Requires Rust 1.82+ (stable).

```sh
cargo build --release
cargo test
```

The repository ships a workspace-wide CI (`fmt + clippy + test` across macOS / Linux /
Windows) and a release workflow that builds signed-ready binaries on every `v*` tag for
six targets: `x86_64`/`aarch64` musl Linux, `x86_64`/`aarch64` Apple Darwin,
`x86_64`/`aarch64` Windows MSVC.

## License

GPL-2.0-only

## Star history

[![Star History Chart](https://api.star-history.com/svg?repos=tekk/tekflash&type=Date)](https://star-history.com/#tekk/tekflash&Date)

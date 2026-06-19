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
cargo install --git https://github.com/tekk/tekflash --tag v0.0.3 --bin tekflash --locked

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

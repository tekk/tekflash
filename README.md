# tekflash

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

A safe, fast, cross-platform TUI for flashing, backing up, and restoring block devices —
SD cards, USB sticks, and other removable media — on macOS, Linux, and Windows.

[![asciicast](https://asciinema.org/a/T2lK4nTDnpQ5VNWk.svg)](https://asciinema.org/a/T2lK4nTDnpQ5VNWk)

> Status: early development. The workspace builds, the CLI surface and TUI shell are
> in place, and the following pipelines are wired and unit-tested end-to-end on macOS,
> Linux, and Windows:
>
> - **flash** — streams from any of `.iso / .img / .bin / .raw / .img.{zst,zsd,zstd,xz,gz,bz2,lz4,br}`
>   (magic-byte detected, extension as a fallback) to a target with optional `--verify=full`.
> - **backup** — bit-for-bit read, compress through any of zstd / lz4 / brotli / xz /
>   gzip / bzip2, BLAKE3 captured during write, sidecar `.tfmanifest.json` written.
> - **archive** — file-level tar with permissions/times/ownership preservation, piped
>   through any codec.
> - **restore** — streams an archive back through extract preserving metadata.
> - **verify** — bytewise compare against a source file; reports first-mismatch offset.
> - **list** — JSON or table device enumeration (macOS `diskutil`, Linux `lsblk`,
>   Windows `Get-Disk`).
> - **PQ-safe password encryption** — Argon2id (m=256 MiB, t=3, p=4) -> ChaCha20-Poly1305
>   framed AEAD; truncation/reordering/bit-flips all fail authentication.
> - **TUI shell** — vivid dark + light palettes (truecolor / 256 / 16 / mono tiers),
>   responsive layouts down to 80×24, ASCII fallback for VT consoles, device table,
>   file browser (F2), help overlay (?).
>
> Multi-target flash, sampled/deferred verify modes, ML-KEM recipient mode, and the
> full TUI flow connecting browser -> flash/backup/restore are landing in follow-up
> commits.

## Features (planned & in-progress)

- **Single static binary** per platform — no runtime dependencies.
- **TUI for both dark and light terminals** with vivid, hand-tuned palettes; truecolor /
  256-color / 16-color / monochrome fallback; responsive layouts down to 80×24; ASCII
  glyph fallback for VT consoles.
- **Cross-platform raw-disk access**
  - macOS: opens `/dev/rdiskN` (unbuffered raw) with `/dev/diskN` fallback.
  - Linux: opens `/dev/sdX` (and on the flash path, with `O_DIRECT`).
  - Windows: opens `\\.\PhysicalDriveN` with
    `FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH`, auto-locks and dismounts child
    volumes (`FSCTL_LOCK_VOLUME` + `FSCTL_DISMOUNT_VOLUME`) before write.
- **Flash from many formats** — `.iso`, `.img`, `.bin`, `.raw`, `.img.{zst,zsd,zstd,xz,gz,bz2,lz4,br}` —
  detected by magic bytes, not extension.
- **Bit-exact backup** of a device to a streaming-compressed image file
  (`zstd`, `lz4`, `brotli`, `xz`, `gz`, `bz2`).
- **File-level `.tar.zst` archive** of a mounted device, preserving extended attributes,
  ACLs, ownership, hidden files.
- **Optional post-quantum encryption**
  - Password mode: Argon2id -> ChaCha20-Poly1305 (256-bit, PQ-safe under Grover).
  - Recipient mode: ML-KEM-768 (FIPS 203 Kyber) wraps the data key.
- **Verify after write** — opt-in re-read with BLAKE3 compare. Full / sampled / deferred
  modes. Uses per-OS cache-bypass (`fsync`+`BLKFLSBUF`+`O_DIRECT` on Linux,
  `F_FULLFSYNC`+`F_NOCACHE` on macOS, `FlushFileBuffers`+`FILE_FLAG_NO_BUFFERING` on
  Windows) so cached reads don't lie.
- **Safety features:** internal-disk filter, mount detection + auto-unmount, multi-target
  flash, dry-run, bandwidth throttle, sparse-aware backup, sidecar manifest, resume
  support.
- **Two modes:** TUI (default) and scriptable CLI with `--no-tui --json` NDJSON output.
- **Comprehensive `--help`** — every subcommand ships at least six worked examples.

## Install

### Via cargo (from source)

```sh
# Prereq: a recent stable Rust toolchain (rustc 1.82+). If you don't have one:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install the latest tekflash from main into ~/.cargo/bin/tekflash
cargo install --git https://github.com/tekk/tekflash --bin tekflash --locked

# Or pin to a released tag for a reproducible install
cargo install --git https://github.com/tekk/tekflash --tag v0.0.2 --bin tekflash --locked

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

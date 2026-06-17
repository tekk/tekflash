//! Curated EXAMPLES blocks for `--help`, injected via clap `after_help` /
//! `after_long_help`. Short `-h` shows the concise version; `--help` shows the long form
//! with every example.

pub const TOP_AFTER: &str = "Run `tekflash <SUBCOMMAND> --help` for per-command examples.";

pub const TOP_AFTER_LONG: &str = "\
USAGE (macOS / Linux):
    sudo tekflash                            launch the TUI
    sudo tekflash <SUBCOMMAND> [OPTIONS]     scriptable mode

USAGE (Windows 11+, elevated PowerShell or 'Run as administrator'):
    tekflash                                 launch the TUI
    tekflash <SUBCOMMAND> [OPTIONS]          scriptable mode

COMMON EXAMPLES:
    # Launch the TUI (most users start here)
    sudo tekflash

    # Flash an ISO, verify after write
    sudo tekflash flash ~/Downloads/ubuntu-24.04.iso /dev/disk5 --verify=full

    # Bit-exact backup of an SD card, zstd-19, with progress as JSON
    sudo tekflash backup /dev/disk5 sd.img.zst --codec zstd --level 19 --json

    # File-level archive of a mounted device, with post-quantum password encryption
    sudo tekflash archive /Volumes/MyDisk backup.tar.zst --encrypt password

    # Multi-target flash: same image to two USB sticks at once
    sudo tekflash flash raspios.img.zst /dev/sdb,/dev/sdc

For per-subcommand examples:  sudo tekflash <SUBCOMMAND> --help
Report bugs:  https://github.com/tekk/tekflash/issues
";

pub const FLASH_AFTER: &str = "Run with --help for more examples.";

pub const FLASH_AFTER_LONG: &str = "\
EXAMPLES:
    # Basic: write an ISO to /dev/disk5 (macOS uses /dev/rdisk5 internally)
    sudo tekflash flash ubuntu-24.04.iso /dev/disk5

    # Compressed source — format detected from magic bytes, not extension
    sudo tekflash flash raspios.img.zst    /dev/sdb
    sudo tekflash flash debian-arm64.img.xz /dev/sdb
    sudo tekflash flash custom.img.gz       /dev/sdb

    # Verify after write (full re-read)
    sudo tekflash flash ubuntu.iso /dev/sdb --verify=full

    # Verify only 5% of the device (fast for big SD cards)
    sudo tekflash flash ubuntu.iso /dev/sdb --verify=sampled

    # Flash with encrypted source (will prompt for password)
    sudo tekflash flash backup.img.zst.tfenc /dev/sdb --decrypt password

    # Multi-target: same image to three devices in parallel
    sudo tekflash flash raspios.img.zst /dev/sdb,/dev/sdc,/dev/sdd

    # Dry-run: full pipeline, no writes — proves throughput and ETA
    sudo tekflash flash ubuntu.iso /dev/sdb --dry-run

    # Throttle to 50 MB/s (useful on shared systems)
    sudo tekflash flash ubuntu.iso /dev/sdb --max-rate 50M

    # Headless / CI mode — NDJSON progress events
    sudo tekflash flash ubuntu.iso /dev/sdb --no-tui --json | jq -c

    # Windows 11 (elevated PowerShell) — same command, native physical-drive path
    tekflash flash ubuntu-24.04.iso \\\\.\\PhysicalDrive2 --verify=full
    # or by drive-letter syntax (resolved to the physical drive):
    tekflash flash ubuntu-24.04.iso E: --verify=full
";

pub const BACKUP_AFTER: &str = "Run with --help for more examples.";

pub const BACKUP_AFTER_LONG: &str = "\
EXAMPLES:
    # Default: zstd-3, sparse-aware, sidecar manifest written next to output
    sudo tekflash backup /dev/disk5 sd-card.img.zst

    # Maximum compression for archival
    sudo tekflash backup /dev/disk5 sd-card.img.zst --codec zstd --level 22

    # Fast compression for big spinning disks
    sudo tekflash backup /dev/sda backup.img.lz4 --codec lz4

    # Password-encrypted backup (Argon2id + ChaCha20-Poly1305)
    sudo tekflash backup /dev/disk5 sensitive.img.zst.tfenc --encrypt password

    # Post-quantum recipient mode (ML-KEM-768)
    sudo tekflash backup /dev/disk5 out.img.zst.tfenc --recipient ~/keys/laptop.pub

    # Resume an interrupted backup (reads sidecar manifest for offset)
    sudo tekflash backup /dev/disk5 sd-card.img.zst --resume

    # Skip sparse-zero detection (force every byte to be read)
    sudo tekflash backup /dev/disk5 sd-card.img.zst --no-sparse
";

pub const ARCHIVE_AFTER: &str = "Run with --help for more examples.";

pub const ARCHIVE_AFTER_LONG: &str = "\
EXAMPLES:
    # File-level archive of a mounted device — preserves all metadata
    sudo tekflash archive /mnt/source backup.tar.zst

    # Archive with maximum compression, encrypted with password
    sudo tekflash archive /mnt/source backup.tar.zst \\
        --codec zstd --level 22 --encrypt password

    # Use .tar.zsd extension (alias for .tar.zst)
    sudo tekflash archive /mnt/source backup.tar.zsd

    # Exclude patterns
    sudo tekflash archive /mnt/source backup.tar.zst \\
        --exclude '*.cache' --exclude '/mnt/source/swap'
";

pub const RESTORE_AFTER: &str = "Run with --help for more examples.";

pub const RESTORE_AFTER_LONG: &str = "\
EXAMPLES:
    # Restore a .tar.zst archive onto a mounted target
    sudo tekflash restore backup.tar.zst /mnt/target

    # Restore an encrypted archive (prompts for password)
    sudo tekflash restore backup.tar.zst.tfenc /mnt/target --decrypt password

    # Restore using your post-quantum private key
    sudo tekflash restore backup.tar.zst.tfenc /mnt/target --key ~/keys/laptop.priv
";

pub const VERIFY_AFTER: &str = "Run with --help for more examples.";

pub const VERIFY_AFTER_LONG: &str = "\
EXAMPLES:
    # Compare device against a source file (BLAKE3, full)
    sudo tekflash verify /dev/sdb against ubuntu.iso

    # Use sidecar manifest if the source isn't on hand
    sudo tekflash verify /dev/sdb --manifest sd-card.img.zst.tfmanifest.json
";

pub const LIST_AFTER: &str = "Run with --help for more examples.";

pub const LIST_AFTER_LONG: &str = "\
EXAMPLES:
    # Human-readable table (default)
    sudo tekflash list

    # Include internal/system disks
    sudo tekflash list --show-all

    # JSON for scripts
    sudo tekflash list --json
";

pub const KEYGEN_AFTER: &str = "Run with --help for more examples.";

pub const KEYGEN_AFTER_LONG: &str = "\
EXAMPLES:
    # Generate an ML-KEM-768 keypair for recipient-mode encryption
    tekflash keygen --pq --out ~/keys/laptop
    # writes laptop.pub (share) and laptop.priv (keep secret)
";

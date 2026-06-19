//! CLI parser plus subcommand entry points.

use clap::{Args as ClapArgs, Parser, Subcommand, ValueEnum};
use color_eyre::Result;
use std::path::PathBuf;
use tekflash_core::pipeline::compress::{Codec, CompressionLevel};

#[derive(Debug, Parser)]
#[command(
    name = "tekflash",
    version,
    about = "Flash, back up, and restore block devices safely (macOS / Linux / Windows)",
    after_help = crate::help::TOP_AFTER,
    after_long_help = crate::help::TOP_AFTER_LONG,
    propagate_version = true,
)]
pub struct Args {
    #[command(flatten)]
    pub global: GlobalOpts,

    #[command(subcommand)]
    pub command: Option<Command>,

    /// Preflight: print capability summary then exit 0/1 depending on elevation.
    #[arg(long, global = true)]
    pub check: bool,
}

#[derive(Debug, ClapArgs, Clone)]
pub struct GlobalOpts {
    /// Theme: dark, light, or auto-detect via OSC 11.
    #[arg(long, value_enum, default_value_t = ThemeChoice::Auto, global = true)]
    pub theme: ThemeChoice,

    /// Use plain ASCII glyphs (auto-detected on TERM=linux / TERM=dumb).
    #[arg(long, global = true)]
    pub ascii: bool,

    /// Force scriptable mode (no TUI), even with no subcommand.
    #[arg(long, global = true)]
    pub no_tui: bool,

    /// NDJSON progress events on stdout instead of TUI rendering.
    #[arg(long, global = true)]
    pub json: bool,

    /// Include internal/system disks in enumeration.
    #[arg(long, global = true)]
    pub show_all: bool,

    /// Throttle to a maximum byte rate, e.g. `50M`, `200K`, `1.5G`.
    #[arg(long, value_name = "RATE", global = true)]
    pub max_rate: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum ThemeChoice {
    Dark,
    Light,
    Auto,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Write an image to a device (raw and compressed forms supported).
    #[command(after_help = crate::help::FLASH_AFTER, after_long_help = crate::help::FLASH_AFTER_LONG)]
    Flash(FlashOpts),
    /// Bit-exact backup of a device to a compressed image file.
    #[command(after_help = crate::help::BACKUP_AFTER, after_long_help = crate::help::BACKUP_AFTER_LONG)]
    Backup(BackupOpts),
    /// File-level tar archive of a device's filesystem (preserves xattrs, ACLs, hidden files).
    #[command(after_help = crate::help::ARCHIVE_AFTER, after_long_help = crate::help::ARCHIVE_AFTER_LONG)]
    Archive(ArchiveOpts),
    /// Restore a .tar.zst archive to a device.
    #[command(after_help = crate::help::RESTORE_AFTER, after_long_help = crate::help::RESTORE_AFTER_LONG)]
    Restore(RestoreOpts),
    /// Re-read a device and compare its BLAKE3 hash against a source file or manifest.
    #[command(after_help = crate::help::VERIFY_AFTER, after_long_help = crate::help::VERIFY_AFTER_LONG)]
    Verify(VerifyOpts),
    /// Run any deferred verifications queued by previous flashes.
    VerifyQueue,
    /// Enumerate block devices (same view as the TUI, as JSON or table).
    #[command(after_help = crate::help::LIST_AFTER, after_long_help = crate::help::LIST_AFTER_LONG)]
    List(ListOpts),
    /// Generate an ML-KEM-768 (post-quantum) keypair for recipient-mode encryption.
    #[command(after_help = crate::help::KEYGEN_AFTER, after_long_help = crate::help::KEYGEN_AFTER_LONG)]
    Keygen(KeygenOpts),
}

#[derive(Debug, ClapArgs)]
pub struct FlashOpts {
    /// Source image file. If omitted, the TUI file browser opens.
    pub source: Option<PathBuf>,
    /// Target device(s), comma-separated for multi-target flash.
    #[arg(value_delimiter = ',')]
    pub targets: Vec<PathBuf>,
    /// Verify mode after write.
    #[arg(long, value_enum, default_value_t = VerifyChoice::Full)]
    pub verify: VerifyChoice,
    /// Dry run: full pipeline, no writes.
    #[arg(long)]
    pub dry_run: bool,
    /// Decryption mode if the source is encrypted.
    #[arg(long, value_enum)]
    pub decrypt: Option<EncryptionChoice>,
    /// Recipient private key for ML-KEM recipient-mode decryption.
    #[arg(long, value_name = "PATH")]
    pub key: Option<PathBuf>,
}

#[derive(Debug, ClapArgs)]
pub struct BackupOpts {
    /// Source device.
    pub source: PathBuf,
    /// Output file path (e.g. `sd-card.img.zst`). If omitted, the file browser opens in save mode.
    pub output: Option<PathBuf>,
    /// Compression codec.
    #[arg(long, value_enum, default_value_t = CodecChoice::Zstd)]
    pub codec: CodecChoice,
    /// Compression level (codec-specific range).
    #[arg(long, default_value_t = 3)]
    pub level: i32,
    /// Disable sparse-zero detection.
    #[arg(long)]
    pub no_sparse: bool,
    /// Resume from sidecar manifest.
    #[arg(long)]
    pub resume: bool,
    /// Optional encryption mode.
    #[arg(long, value_enum)]
    pub encrypt: Option<EncryptionChoice>,
    /// Recipient public key for ML-KEM recipient mode.
    #[arg(long, value_name = "PATH")]
    pub recipient: Option<PathBuf>,
}

#[derive(Debug, ClapArgs)]
pub struct ArchiveOpts {
    /// Source directory (e.g. a mounted device).
    pub source: PathBuf,
    /// Output `.tar.<codec>` path.
    pub output: PathBuf,
    /// Compression codec.
    #[arg(long, value_enum, default_value_t = CodecChoice::Zstd)]
    pub codec: CodecChoice,
    /// Compression level.
    #[arg(long, default_value_t = 3)]
    pub level: i32,
    /// Optional encryption mode.
    #[arg(long, value_enum)]
    pub encrypt: Option<EncryptionChoice>,
    /// Recipient public key for ML-KEM recipient mode.
    #[arg(long, value_name = "PATH")]
    pub recipient: Option<PathBuf>,
    /// Glob-ish exclude pattern (matches anywhere in the path). Repeatable.
    #[arg(long)]
    pub exclude: Vec<String>,
}

#[derive(Debug, ClapArgs)]
pub struct RestoreOpts {
    pub archive: PathBuf,
    pub target: PathBuf,
    #[arg(long, value_enum)]
    pub decrypt: Option<EncryptionChoice>,
    #[arg(long, value_name = "PATH")]
    pub key: Option<PathBuf>,
}

#[derive(Debug, ClapArgs)]
pub struct VerifyOpts {
    pub device: PathBuf,
    /// Source file to compare against (positional after `against`).
    #[arg(value_name = "AGAINST")]
    pub against: Option<PathBuf>,
    /// Use sidecar manifest as the source of truth instead of a file.
    #[arg(long, value_name = "PATH")]
    pub manifest: Option<PathBuf>,
}

#[derive(Debug, ClapArgs)]
pub struct ListOpts {}

#[derive(Debug, ClapArgs)]
pub struct KeygenOpts {
    /// Generate an ML-KEM (post-quantum) keypair.
    #[arg(long)]
    pub pq: bool,
    /// Output file base. Writes `<out>.pub` and `<out>.priv`.
    #[arg(long, value_name = "PATH", default_value = "tekflash-key")]
    pub out: PathBuf,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum VerifyChoice {
    Off,
    Full,
    Sampled,
    Deferred,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum EncryptionChoice {
    Password,
    Recipient,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CodecChoice {
    None,
    Zstd,
    Lz4,
    Brotli,
    Xz,
    Gzip,
    Bzip2,
}

impl From<CodecChoice> for Codec {
    fn from(c: CodecChoice) -> Self {
        match c {
            CodecChoice::None => Codec::None,
            CodecChoice::Zstd => Codec::Zstd,
            CodecChoice::Lz4 => Codec::Lz4,
            CodecChoice::Brotli => Codec::Brotli,
            CodecChoice::Xz => Codec::Xz,
            CodecChoice::Gzip => Codec::Gzip,
            CodecChoice::Bzip2 => Codec::Bzip2,
        }
    }
}

// ---------- subcommand stubs ----------

pub async fn run_list(_opts: ListOpts, global: GlobalOpts) -> Result<()> {
    let devs = tekflash_core::device::enumerate(global.show_all)?;
    if global.json {
        let out = serde_json::to_string_pretty(&devs)?;
        println!("{out}");
        return Ok(());
    }
    if devs.is_empty() {
        println!("No removable block devices found. Pass --show-all to include internal disks.");
        return Ok(());
    }
    println!(
        "{:<22}  {:<28}  {:>10}  {:<8}  {:<3}  MOUNT",
        "PATH", "MODEL", "SIZE", "BUS", "RM"
    );
    for d in &devs {
        let mount = d
            .mountpoints
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{:<22}  {:<28}  {:>10}  {:<8}  {:<3}  {}",
            d.path.display(),
            d.name(),
            d.size_human(),
            format!("{:?}", d.transport),
            if d.removable { "yes" } else { "no" },
            mount
        );
    }
    Ok(())
}

pub async fn run_flash(opts: FlashOpts, _global: GlobalOpts) -> Result<()> {
    use tekflash_core::pipeline::{
        compress::{decoder, Codec},
        format::{detect, detect_by_extension},
        hasher::{HashKind, Hasher},
        verify::{verify_full_by_hash, VerifyMode},
    };

    let Some(source) = opts.source else {
        eprintln!("flash: source is required in CLI mode (or run `tekflash` for the TUI)");
        std::process::exit(2);
    };
    if opts.targets.is_empty() {
        eprintln!("flash: at least one target device is required");
        std::process::exit(2);
    }
    if opts.targets.len() > 1 {
        eprintln!("flash: multi-target flash not yet wired in CLI mode (TUI supports it)");
        std::process::exit(2);
    }
    let target = &opts.targets[0];

    // Magic-byte detect on the source; fall back to extension if the file is shorter
    // than a magic.
    let codec: Codec = {
        let mut head = [0u8; 16];
        let mut f = std::fs::File::open(&source)?;
        use std::io::Read;
        let n = f.read(&mut head)?;
        let fmt = detect(&head[..n]);
        if matches!(fmt, tekflash_core::pipeline::format::InputFormat::Raw) {
            detect_by_extension(&source)
                .map(Codec::from)
                .unwrap_or(Codec::None)
        } else {
            Codec::from(fmt)
        }
    };

    // Open source through the appropriate decoder.
    let src = std::fs::File::open(&source)?;
    let mut reader = decoder(codec, src)?;

    let mut dst = if opts.dry_run {
        None
    } else {
        Some(std::fs::OpenOptions::new().write(true).open(target)?)
    };

    let mut hasher = Hasher::new(HashKind::Blake3);
    let mut buf = vec![0u8; 4 * 1024 * 1024];
    let mut total: u64 = 0;
    use std::io::{Read, Write};
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        if let Some(d) = dst.as_mut() {
            d.write_all(&buf[..n])?;
        }
        total += n as u64;
    }
    if let Some(mut d) = dst {
        d.flush()?;
    }

    let hash = hasher.finalize_hex();
    println!(
        "flash {}: {} -> {} ({} bytes, BLAKE3 = {hash})",
        if opts.dry_run { "(dry-run)" } else { "ok" },
        source.display(),
        target.display(),
        total
    );

    let verify_mode = match opts.verify {
        VerifyChoice::Off => VerifyMode::Off,
        VerifyChoice::Full => VerifyMode::Full,
        VerifyChoice::Sampled => VerifyMode::Sampled,
        VerifyChoice::Deferred => VerifyMode::Deferred,
    };
    if verify_mode == VerifyMode::Full && !opts.dry_run {
        eprintln!("verifying...");
        let outcome = verify_full_by_hash(target, &hash, HashKind::Blake3)?;
        if outcome.passed {
            println!("verify ok: {} bytes match", outcome.bytes_read);
        } else {
            eprintln!(
                "verify FAILED: device hash differs from source after {} bytes",
                outcome.bytes_read
            );
            std::process::exit(1);
        }
    }
    Ok(())
}

pub async fn run_backup(opts: BackupOpts, _global: GlobalOpts) -> Result<()> {
    use tekflash_core::manifest::{Manifest, SourceInfo};
    use tekflash_core::pipeline::{
        compress::encoder,
        hasher::{HashKind, Hasher},
        reader::open_for_read,
    };

    let Some(output) = opts.output else {
        eprintln!("backup: output path is required in CLI mode (or run `tekflash` for the TUI)");
        std::process::exit(2);
    };
    let src = open_for_read(&opts.source)?;
    let dst = std::fs::File::create(&output)?;
    let codec: Codec = opts.codec.into();
    let mut writer = encoder(codec, CompressionLevel(opts.level), dst)?;
    let mut hasher = Hasher::new(HashKind::Blake3);
    let mut bytes_in: u64 = 0;
    let mut buf = vec![0u8; 4 * 1024 * 1024];
    let mut src = std::io::BufReader::new(src);
    use std::io::{Read, Write};
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        writer.write_all(&buf[..n])?;
        bytes_in += n as u64;
    }
    drop(writer);

    let bytes_out = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
    let hash_hex = hasher.finalize_hex();

    // Sidecar manifest: a future restore (possibly on a different machine) has
    // everything it needs without trusting filename conventions.
    let manifest = Manifest {
        schema_version: 1,
        tekflash_version: env!("CARGO_PKG_VERSION").to_string(),
        created: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        host: hostname::get()
            .ok()
            .and_then(|s| s.to_string_lossy().into_owned().into()),
        source: SourceInfo {
            path: opts.source.clone(),
            vendor: None,
            model: None,
            serial: None,
            size_bytes: bytes_in,
        },
        bytes_in,
        bytes_out,
        hash_kind: HashKind::Blake3,
        hash_hex: hash_hex.clone(),
        codec,
        level: CompressionLevel(opts.level),
        encryption: None,
        sparse_extents: vec![],
        last_good_offset: None,
    };
    let manifest_path = output.with_extension(format!(
        "{}.tfmanifest.json",
        output.extension().and_then(|s| s.to_str()).unwrap_or("")
    ));
    if let Ok(f) = std::fs::File::create(&manifest_path) {
        let _ = serde_json::to_writer_pretty(f, &manifest);
    }

    println!(
        "backup ok: {bytes_in} bytes in, {bytes_out} bytes out ({:.1}% of source), BLAKE3 = {hash_hex}",
        if bytes_in > 0 {
            100.0 * bytes_out as f64 / bytes_in as f64
        } else {
            0.0
        }
    );
    println!("manifest:  {}", manifest_path.display());
    Ok(())
}

pub async fn run_archive(opts: ArchiveOpts, _global: GlobalOpts) -> Result<()> {
    use tekflash_core::archive::tar::archive_tree;
    use tekflash_core::pipeline::compress::encoder;

    let codec: Codec = opts.codec.into();
    let dst = std::fs::File::create(&opts.output)?;
    let writer = encoder(codec, CompressionLevel(opts.level), dst)?;

    archive_tree(&opts.source, writer, &opts.exclude)?;
    println!(
        "archive ok: {} -> {} (codec {})",
        opts.source.display(),
        opts.output.display(),
        codec.human()
    );
    Ok(())
}

pub async fn run_restore(opts: RestoreOpts, _global: GlobalOpts) -> Result<()> {
    use tekflash_core::archive::extract::extract_to;
    use tekflash_core::pipeline::{
        compress::{decoder, Codec},
        format::detect_by_extension,
    };

    // Decide which codec to use by extension only — restore needs the codec before it
    // can read the stream, so we accept the file-name hint. (Magic-byte detect would
    // require a seekable peek + rewind on the source; cheap to add later.)
    let codec = detect_by_extension(&opts.archive)
        .map(Codec::from)
        .unwrap_or(Codec::None);
    let src = std::fs::File::open(&opts.archive)?;
    let reader = decoder(codec, src)?;
    extract_to(reader, &opts.target)?;
    println!(
        "restore ok: {} -> {} (codec {})",
        opts.archive.display(),
        opts.target.display(),
        codec.human()
    );
    Ok(())
}

pub async fn run_verify(opts: VerifyOpts, _global: GlobalOpts) -> Result<()> {
    use tekflash_core::pipeline::verify::verify_full;
    let Some(against) = opts.against.or(opts.manifest) else {
        eprintln!("verify: pass either AGAINST <file> or --manifest <path>");
        std::process::exit(2);
    };
    let source = std::fs::File::open(&against)?;
    let outcome = verify_full(&opts.device, source)?;
    if outcome.passed {
        println!(
            "verify ok: {} matches {} ({} bytes)",
            opts.device.display(),
            against.display(),
            outcome.bytes_read
        );
        Ok(())
    } else {
        eprintln!(
            "verify FAILED: first mismatch at offset {:?} after {} bytes",
            outcome.first_mismatch_offset, outcome.bytes_read
        );
        std::process::exit(1);
    }
}

pub async fn run_verify_queue(_global: GlobalOpts) -> Result<()> {
    eprintln!("verify-queue: nothing pending");
    Ok(())
}

pub async fn run_keygen(_opts: KeygenOpts) -> Result<()> {
    eprintln!("keygen: not implemented in this commit");
    Ok(())
}

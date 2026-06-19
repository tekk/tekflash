//! Background worker that runs a backup on a dedicated OS thread and streams progress
//! events back to the TUI's event loop over a sync `mpsc` channel.
//!
//! Why a thread, not a tokio task? The encoder + writer chain is fully synchronous and
//! blocking; we want it off the runtime thread so the TUI keeps drawing at 60 Hz while
//! the backup grinds through gigabytes. The TUI polls the channel non-blocking on every
//! iteration of its event loop.

use color_eyre::eyre::{eyre, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tekflash_core::pipeline::compress::{encoder, Codec, CompressionLevel};
use tekflash_core::pipeline::hasher::{HashKind, Hasher};

/// Which kind of operation this session represents. The worker dispatches on this; the
/// UI uses it to pick titles, icons, and which extra fields to render (current file,
/// etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    /// Bit-exact backup of a block device into a compressed image file.
    Backup,
    /// File-level tar archive of a mounted directory.
    Archive,
}

impl OperationKind {
    pub fn human(self) -> &'static str {
        match self {
            OperationKind::Backup => "Backup",
            OperationKind::Archive => "Archive",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackupParams {
    pub kind: OperationKind,
    pub source: PathBuf,
    pub dest: PathBuf,
    pub codec: Codec,
    pub level: CompressionLevel,
    /// Total bytes of the source if known (block device size, file length, or, for
    /// archives, the sum of all file sizes after the worker walks the tree).
    pub total_bytes: Option<u64>,
}

#[derive(Debug)]
pub enum BackupEvent {
    /// Worker has finished a tree walk and can now report the true source size. Backups
    /// usually know this upfront; archives discover it.
    Started {
        total_bytes: u64,
    },
    Tick {
        bytes_in: u64,
        bytes_out: u64,
        /// Currently-being-processed file, relative to the source root. Set by the
        /// archive worker; `None` for backups.
        current_file: Option<String>,
    },
    Finished {
        bytes_in: u64,
        bytes_out: u64,
        hash_hex: String,
        elapsed: Duration,
    },
    Failed {
        message: String,
    },
}

/// Spawn the worker and return the channel + a handle to the OS thread + a live
/// `bytes_out` counter the UI can poll between ticks for smoother gauge updates. The
/// worker dispatches on `params.kind` to either the block-device backup loop or the
/// file-tree archive walk.
pub fn spawn_backup(
    params: BackupParams,
) -> (Arc<AtomicU64>, Receiver<BackupEvent>, JoinHandle<()>) {
    let bytes_out = Arc::new(AtomicU64::new(0));
    let bytes_out_worker = bytes_out.clone();
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::Builder::new()
        .name("tekflash-session".to_string())
        .spawn(move || {
            let started = Instant::now();
            let result = match params.kind {
                OperationKind::Backup => do_backup(&params, bytes_out_worker.clone(), &tx),
                OperationKind::Archive => do_archive(&params, bytes_out_worker.clone(), &tx),
            };
            match result {
                Ok((bytes_in, hash)) => {
                    let bytes_out_final = bytes_out_worker.load(Ordering::Relaxed);
                    let _ = tx.send(BackupEvent::Finished {
                        bytes_in,
                        bytes_out: bytes_out_final,
                        hash_hex: hash,
                        elapsed: started.elapsed(),
                    });
                }
                Err(e) => {
                    let _ = tx.send(BackupEvent::Failed {
                        message: e.to_string(),
                    });
                }
            }
        })
        .expect("spawn session thread");
    (bytes_out, rx, handle)
}

fn do_backup(
    params: &BackupParams,
    bytes_out: Arc<AtomicU64>,
    tx: &Sender<BackupEvent>,
) -> Result<(u64, String)> {
    // Open the source through the per-OS fast path: F_NOCACHE + F_RDAHEAD on macOS
    // (combined with /dev/rdiskN that the caller already substituted in),
    // posix_fadvise(SEQUENTIAL|NOREUSE) on Linux, FILE_FLAG_SEQUENTIAL_SCAN on Windows.
    let src = tekflash_core::device::open_fast_read(&params.source)
        .map_err(|e| eyre!("open source {}: {}", params.source.display(), e))?;
    let dst = std::fs::File::create(&params.dest)
        .map_err(|e| eyre!("create dest {}: {}", params.dest.display(), e))?;
    let counter = ByteCounter {
        inner: dst,
        bytes: bytes_out.clone(),
    };
    let mut writer =
        encoder(params.codec, params.level, counter).map_err(|e| eyre!("init encoder: {}", e))?;
    let mut hasher = Hasher::new(HashKind::Blake3);
    // Larger buffer = fewer syscalls and lets the kernel stream bigger DMA windows.
    // 8 MiB amortizes per-syscall costs and is the sweet spot for USB 3.x / NVMe.
    const BUF_BYTES: usize = 8 * 1024 * 1024;
    let mut src = std::io::BufReader::with_capacity(BUF_BYTES, src);
    let mut buf = vec![0u8; BUF_BYTES];
    let mut bytes_in: u64 = 0;
    let mut last_tick = Instant::now();
    let tick_interval = Duration::from_millis(150);

    loop {
        let n = src.read(&mut buf).map_err(|e| eyre!("read: {}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        writer
            .write_all(&buf[..n])
            .map_err(|e| eyre!("write: {}", e))?;
        bytes_in += n as u64;

        if last_tick.elapsed() >= tick_interval {
            let _ = tx.send(BackupEvent::Tick {
                bytes_in,
                bytes_out: bytes_out.load(Ordering::Relaxed),
                current_file: None,
            });
            last_tick = Instant::now();
        }
    }
    // Drop the encoder to flush trailers (zstd auto_finish, lz4 finish_on_drop, etc.).
    drop(writer);
    Ok((bytes_in, hasher.finalize_hex()))
}

/// Walk a directory tree, tar-stream its contents through the chosen codec, and emit
/// progress events including the currently-being-processed file. The hash captured is
/// BLAKE3 over the *concatenated file contents in tar order* — that's the canonical
/// "what was archived" digest and matches what restore would see on read-back.
fn do_archive(
    params: &BackupParams,
    bytes_out: Arc<AtomicU64>,
    tx: &Sender<BackupEvent>,
) -> Result<(u64, String)> {
    // Resolve the source to an actual directory we can walk. The TUI normally hands us
    // a mountpoint (e.g. /Volumes/MyDrive), but if the device has no mountpoint we get
    // the device node itself (e.g. /dev/disk5). In that case try to mount it; on
    // failure return a clear error so the user can act on it.
    let source = if params.source.is_dir() {
        params.source.clone()
    } else if params.source.to_string_lossy().starts_with("/dev/") {
        let _ = tx.send(BackupEvent::Tick {
            bytes_in: 0,
            bytes_out: 0,
            current_file: Some(format!("mounting {}…", params.source.display())),
        });
        tekflash_core::device::try_mount(&params.source).map_err(|e| {
            eyre!(
                "source {} isn't a directory and could not be mounted: {e}",
                params.source.display()
            )
        })?
    } else {
        return Err(eyre!(
            "source {} is not a directory; archive needs a mounted filesystem",
            params.source.display()
        ));
    };

    // Phase 1: walk the tree once to learn the total source size. This unlocks a
    // meaningful percentage and ETA in the UI; it usually finishes in a second or two
    // even for trees with tens of thousands of files.
    let total = precompute_total_bytes(&source);
    let _ = tx.send(BackupEvent::Started { total_bytes: total });
    // Initial Tick so the UI shows the source and "starting…" right away even when
    // the archive completes in a few milliseconds (e.g. a tiny tree).
    let _ = tx.send(BackupEvent::Tick {
        bytes_in: 0,
        bytes_out: 0,
        current_file: Some(format!("opening {}", source.display())),
    });

    let dst = std::fs::File::create(&params.dest)
        .map_err(|e| eyre!("create dest {}: {}", params.dest.display(), e))?;
    let counter = ByteCounter {
        inner: dst,
        bytes: bytes_out.clone(),
    };
    let writer =
        encoder(params.codec, params.level, counter).map_err(|e| eyre!("init encoder: {}", e))?;
    let mut builder = ::tar::Builder::new(writer);
    builder.follow_symlinks(false);

    let mut hasher = Hasher::new(HashKind::Blake3);
    let mut bytes_in: u64 = 0;
    let mut last_tick = Instant::now();
    let tick_interval = Duration::from_millis(150);

    walk_and_archive(
        &source,
        &source,
        &mut builder,
        &mut hasher,
        &mut bytes_in,
        &bytes_out,
        tx,
        &mut last_tick,
        tick_interval,
    )?;

    builder.finish().map_err(|e| eyre!("finalize tar: {}", e))?;
    Ok((bytes_in, hasher.finalize_hex()))
}

fn precompute_total_bytes(root: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        // Skip the same system metadata dirs the archive walker will skip, so the
        // total here matches what actually ends up in the tar.
        let rel = p.strip_prefix(root).unwrap_or(&p);
        if is_system_metadata(rel) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.file_type().is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                for e in rd.flatten() {
                    stack.push(e.path());
                }
            }
        } else if !meta.file_type().is_symlink() {
            total += meta.len();
        }
    }
    total
}

/// Top-level volume names we always skip in archives. These hold runtime metadata
/// indexes, trashes, and recycle bins — useless in a backup and usually behind ACLs
/// that block even sudo'd reads (Spotlight on macOS, System Volume Information on
/// FAT/exFAT, etc).
const SYSTEM_METADATA_DIRS: &[&str] = &[
    ".Spotlight-V100",
    ".fseventsd",
    ".Trashes",
    ".TemporaryItems",
    ".DocumentRevisions-V100",
    ".HFS+ Private Directory Data\r",
    ".HFS+ Private Data",
    ".vol",
    "System Volume Information",
    "$RECYCLE.BIN",
];

fn is_system_metadata(rel: &Path) -> bool {
    let Some(first) = rel.components().next() else {
        return false;
    };
    match first {
        std::path::Component::Normal(name) => SYSTEM_METADATA_DIRS
            .iter()
            .any(|s| std::ffi::OsStr::new(*s) == name),
        _ => false,
    }
}

/// Is this an I/O error we can safely skip past without aborting the whole archive?
/// PermissionDenied — typical for SIP-protected dirs on macOS, /proc/PID/mem on Linux,
/// locked files on Windows. NotFound — file was deleted while the walk was running.
fn is_skippable_io_error(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound
    )
}

fn send_skip_tick(
    tx: &Sender<BackupEvent>,
    bytes_in: u64,
    bytes_out_atom: &Arc<AtomicU64>,
    reason: &str,
    rel: &Path,
) {
    let _ = tx.send(BackupEvent::Tick {
        bytes_in,
        bytes_out: bytes_out_atom.load(Ordering::Relaxed),
        current_file: Some(format!("skipped ({reason}): {}", rel.display())),
    });
}

#[allow(clippy::too_many_arguments)]
fn walk_and_archive<W: Write>(
    base: &Path,
    path: &Path,
    builder: &mut ::tar::Builder<W>,
    hasher: &mut Hasher,
    bytes_in: &mut u64,
    bytes_out_atom: &Arc<AtomicU64>,
    tx: &Sender<BackupEvent>,
    last_tick: &mut Instant,
    tick_interval: Duration,
) -> Result<()> {
    let rel = path.strip_prefix(base).unwrap_or(path).to_path_buf();

    // Hard-skip well-known system metadata directories. They're usually unreadable even
    // under sudo (Spotlight ACLs on macOS) and aren't useful in a backup anyway.
    if is_system_metadata(&rel) {
        send_skip_tick(tx, *bytes_in, bytes_out_atom, "system metadata", &rel);
        return Ok(());
    }

    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if is_skippable_io_error(&e) => {
            send_skip_tick(
                tx,
                *bytes_in,
                bytes_out_atom,
                &format!("{}", e.kind()),
                &rel,
            );
            return Ok(());
        }
        Err(e) => return Err(eyre!("stat {}: {}", path.display(), e)),
    };

    // Tick on file boundaries (but at most every tick_interval so we don't flood the
    // channel on trees with millions of tiny files).
    if last_tick.elapsed() >= tick_interval {
        let _ = tx.send(BackupEvent::Tick {
            bytes_in: *bytes_in,
            bytes_out: bytes_out_atom.load(Ordering::Relaxed),
            current_file: Some(rel.display().to_string()),
        });
        *last_tick = Instant::now();
    }

    if meta.file_type().is_dir() {
        if !rel.as_os_str().is_empty() {
            if let Err(e) = builder.append_dir(&rel, path) {
                if let Some(io) = e.get_ref().and_then(|r| r.downcast_ref::<std::io::Error>()) {
                    if is_skippable_io_error(io) {
                        send_skip_tick(tx, *bytes_in, bytes_out_atom, "dir header", &rel);
                        return Ok(());
                    }
                }
                return Err(eyre!("append_dir {}: {}", path.display(), e));
            }
        }
        let read = match std::fs::read_dir(path) {
            Ok(r) => r,
            Err(e) if is_skippable_io_error(&e) => {
                send_skip_tick(
                    tx,
                    *bytes_in,
                    bytes_out_atom,
                    &format!("{}", e.kind()),
                    &rel,
                );
                return Ok(());
            }
            Err(e) => return Err(eyre!("read_dir {}: {}", path.display(), e)),
        };
        let mut entries: Vec<PathBuf> = Vec::new();
        for entry in read.flatten() {
            entries.push(entry.path());
        }
        // Deterministic order so archive contents are reproducible.
        entries.sort();
        for child in entries {
            walk_and_archive(
                base,
                &child,
                builder,
                hasher,
                bytes_in,
                bytes_out_atom,
                tx,
                last_tick,
                tick_interval,
            )?;
        }
    } else if meta.file_type().is_symlink() {
        if let Err(e) = builder.append_path_with_name(path, &rel) {
            if let Some(io) = e.get_ref().and_then(|r| r.downcast_ref::<std::io::Error>()) {
                if is_skippable_io_error(io) {
                    send_skip_tick(tx, *bytes_in, bytes_out_atom, "symlink", &rel);
                    return Ok(());
                }
            }
            return Err(eyre!("append_symlink {}: {}", path.display(), e));
        }
    } else {
        let f = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) if is_skippable_io_error(&e) => {
                send_skip_tick(
                    tx,
                    *bytes_in,
                    bytes_out_atom,
                    &format!("{}", e.kind()),
                    &rel,
                );
                return Ok(());
            }
            Err(e) => return Err(eyre!("open file {}: {}", path.display(), e)),
        };
        // Build a tar header from the file's metadata so append_data can stream the
        // contents through our counting/hashing reader (append_file would require
        // &mut File and bypass the wrapper).
        let mut header = ::tar::Header::new_gnu();
        header.set_metadata(&meta);
        let reader = CountingHashingReader {
            inner: f,
            bytes_in,
            hasher,
        };
        if let Err(e) = builder.append_data(&mut header, &rel, reader) {
            if let Some(io) = e.get_ref().and_then(|r| r.downcast_ref::<std::io::Error>()) {
                if is_skippable_io_error(io) {
                    send_skip_tick(tx, *bytes_in, bytes_out_atom, "read", &rel);
                    return Ok(());
                }
            }
            return Err(eyre!("append_data {}: {}", path.display(), e));
        }
    }
    Ok(())
}

/// Per-file reader that counts bytes read into `bytes_in` and updates the BLAKE3
/// hasher in lockstep. Borrows both mutably, so it's scoped to a single file's read.
struct CountingHashingReader<'a, R: Read> {
    inner: R,
    bytes_in: &'a mut u64,
    hasher: &'a mut Hasher,
}

impl<R: Read> Read for CountingHashingReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        *self.bytes_in += n as u64;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
}

/// Pass-through writer that atomically counts bytes for live UI display.
struct ByteCounter<W: Write> {
    inner: W,
    bytes: Arc<AtomicU64>,
}

impl<W: Write> Write for ByteCounter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.bytes.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// State threaded into the TUI for a running (or completed) backup.
#[derive(Debug)]
#[allow(dead_code)] // device_idx is held for future history lookups / cancel routing
pub struct BackupProgress {
    pub device_idx: usize,
    pub params: BackupParams,
    pub started_at: Instant,

    pub bytes_in: u64,
    pub bytes_out: u64,
    pub instant_rate: f64, // bytes/s based on last tick
    pub status: BackupStatus,
    /// Set by the archive worker; the home view shows this in the mini bar and the
    /// progress view shows it as its own row in the stats panel.
    pub current_file: Option<String>,

    /// Live counter that the worker writes through `ByteCounter` — the UI can poll this
    /// between Tick events to refresh the compressed-size column at 60 Hz.
    pub bytes_out_live: Arc<AtomicU64>,
    rx: Receiver<BackupEvent>,
    join_handle: Option<JoinHandle<()>>,

    last_tick_at: Instant,
    last_bytes_in_at_tick: u64,
}

#[derive(Debug)]
pub enum BackupStatus {
    Running,
    Finished { hash_hex: String, elapsed: Duration },
    Failed { message: String },
}

impl BackupProgress {
    pub fn new(device_idx: usize, params: BackupParams) -> Self {
        let (bytes_out, rx, handle) = spawn_backup(params.clone());
        let now = Instant::now();
        Self {
            device_idx,
            params,
            started_at: now,
            bytes_in: 0,
            bytes_out: 0,
            instant_rate: 0.0,
            status: BackupStatus::Running,
            current_file: None,
            bytes_out_live: bytes_out,
            rx,
            join_handle: Some(handle),
            last_tick_at: now,
            last_bytes_in_at_tick: 0,
        }
    }

    /// Drain any pending events into the local state. Returns `true` if anything
    /// changed (useful in case we want to redraw immediately).
    pub fn poll(&mut self) -> bool {
        use std::sync::mpsc::TryRecvError;
        let mut changed = false;
        loop {
            match self.rx.try_recv() {
                Ok(BackupEvent::Started { total_bytes }) => {
                    // Archive worker reports the true total after walking the tree.
                    // Backup ignores Started — total_bytes is known upfront.
                    self.params.total_bytes = Some(total_bytes);
                    changed = true;
                }
                Ok(BackupEvent::Tick {
                    bytes_in,
                    bytes_out,
                    current_file,
                }) => {
                    let dt = self.last_tick_at.elapsed().as_secs_f64().max(1e-3);
                    self.instant_rate = (bytes_in - self.last_bytes_in_at_tick) as f64 / dt;
                    self.last_bytes_in_at_tick = bytes_in;
                    self.last_tick_at = Instant::now();
                    self.bytes_in = bytes_in;
                    self.bytes_out = bytes_out;
                    if current_file.is_some() {
                        self.current_file = current_file;
                    }
                    changed = true;
                }
                Ok(BackupEvent::Finished {
                    bytes_in,
                    bytes_out,
                    hash_hex,
                    elapsed,
                }) => {
                    self.bytes_in = bytes_in;
                    self.bytes_out = bytes_out;
                    self.status = BackupStatus::Finished { hash_hex, elapsed };
                    if let Some(h) = self.join_handle.take() {
                        let _ = h.join();
                    }
                    changed = true;
                    break;
                }
                Ok(BackupEvent::Failed { message }) => {
                    self.status = BackupStatus::Failed { message };
                    if let Some(h) = self.join_handle.take() {
                        let _ = h.join();
                    }
                    changed = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        // Refresh bytes_out from the live counter so the gauge updates between ticks.
        let live = self.bytes_out_live.load(Ordering::Relaxed);
        if live != self.bytes_out {
            self.bytes_out = live;
            changed = true;
        }
        changed
    }

    pub fn elapsed(&self) -> Duration {
        match &self.status {
            BackupStatus::Finished { elapsed, .. } => *elapsed,
            _ => self.started_at.elapsed(),
        }
    }

    pub fn average_rate(&self) -> f64 {
        let elapsed = self.elapsed().as_secs_f64().max(1e-3);
        self.bytes_in as f64 / elapsed
    }

    pub fn ratio(&self) -> Option<f64> {
        if self.bytes_in == 0 {
            None
        } else {
            Some(self.bytes_out as f64 / self.bytes_in as f64)
        }
    }

    pub fn fraction(&self) -> Option<f64> {
        let total = self.params.total_bytes?;
        if total == 0 {
            return None;
        }
        Some((self.bytes_in as f64 / total as f64).clamp(0.0, 1.0))
    }

    pub fn eta(&self) -> Option<Duration> {
        let rate = self.average_rate();
        if rate <= 0.0 {
            return None;
        }
        let total = self.params.total_bytes?;
        if self.bytes_in >= total {
            return Some(Duration::ZERO);
        }
        let remaining = (total - self.bytes_in) as f64;
        let secs = remaining / rate;
        // Cap the ETA at 30 days so we don't render nonsense in pathological cases.
        Some(Duration::from_secs_f64(secs.min(86_400.0 * 30.0)))
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, BackupStatus::Running)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn tempfile(label: &str, bytes: &[u8]) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "tekflash-prog-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn backup_runner_finishes_and_reports_real_hash() {
        let payload: Vec<u8> = (0u32..400_000).map(|i| (i.wrapping_mul(7)) as u8).collect();
        let src = tempfile("src", &payload);
        let dst =
            std::env::temp_dir().join(format!("tekflash-prog-dst-{}.zst", std::process::id()));

        let params = BackupParams {
            kind: OperationKind::Backup,
            source: src.clone(),
            dest: dst.clone(),
            codec: Codec::Zstd,
            level: CompressionLevel(3),
            total_bytes: Some(payload.len() as u64),
        };
        let mut prog = BackupProgress::new(0, params);

        // Spin the poll loop until finished, with a generous safety cap.
        let started = Instant::now();
        loop {
            prog.poll();
            if !prog.is_running() {
                break;
            }
            if started.elapsed() > Duration::from_secs(10) {
                panic!("backup never finished");
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        match &prog.status {
            BackupStatus::Finished { hash_hex, .. } => {
                let mut h = Hasher::new(HashKind::Blake3);
                h.update(&payload);
                let expected = h.finalize_hex();
                assert_eq!(hash_hex, &expected);
            }
            other => panic!("expected Finished, got {other:?}"),
        }
        assert_eq!(prog.bytes_in, payload.len() as u64);
        assert!(prog.bytes_out > 0);
        // zstd should compress this deterministic payload at least a little.
        assert!(prog.bytes_out < prog.bytes_in);

        let _ = std::fs::remove_file(src);
        let _ = std::fs::remove_file(dst);
    }

    #[test]
    fn archive_runner_walks_a_small_tree_and_reports_current_file() {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("tekflash-arch-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(root.join("subdir")).unwrap();
        std::fs::write(root.join("alpha.txt"), b"alpha contents").unwrap();
        std::fs::write(root.join("subdir").join("beta.txt"), b"beta beta beta beta").unwrap();
        let dst = root.with_extension("tar.zst");

        let params = BackupParams {
            kind: OperationKind::Archive,
            source: root.clone(),
            dest: dst.clone(),
            codec: Codec::Zstd,
            level: CompressionLevel(3),
            total_bytes: None,
        };
        let mut prog = BackupProgress::new(0, params);
        let mut saw_current_file = false;

        let started = Instant::now();
        loop {
            prog.poll();
            if prog.current_file.is_some() {
                saw_current_file = true;
            }
            if !prog.is_running() {
                break;
            }
            if started.elapsed() > Duration::from_secs(10) {
                panic!("archive never finished");
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        match &prog.status {
            BackupStatus::Finished { .. } => {}
            other => panic!("expected Finished, got {other:?}"),
        }
        assert!(prog.bytes_in > 0, "expected non-zero bytes_in");
        assert!(prog.bytes_out > 0, "expected non-zero bytes_out");
        assert!(
            prog.params.total_bytes.is_some(),
            "Started should have populated total_bytes"
        );
        assert!(
            saw_current_file,
            "archive worker must report current_file at least once"
        );

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(dst);
    }

    #[test]
    fn fraction_eta_and_ratio_only_compute_when_inputs_make_sense() {
        let mut prog = BackupProgress {
            device_idx: 0,
            params: BackupParams {
                kind: OperationKind::Backup,
                source: PathBuf::from("/dev/null"),
                dest: PathBuf::from("/tmp/never"),
                codec: Codec::Zstd,
                level: CompressionLevel(3),
                total_bytes: Some(1000),
            },
            started_at: Instant::now(),
            bytes_in: 0,
            bytes_out: 0,
            instant_rate: 0.0,
            status: BackupStatus::Running,
            current_file: None,
            bytes_out_live: Arc::new(AtomicU64::new(0)),
            rx: std::sync::mpsc::channel().1,
            join_handle: None,
            last_tick_at: Instant::now(),
            last_bytes_in_at_tick: 0,
        };
        assert_eq!(prog.fraction(), Some(0.0));
        assert!(prog.ratio().is_none()); // bytes_in == 0

        prog.bytes_in = 500;
        prog.bytes_out = 100;
        assert!((prog.fraction().unwrap() - 0.5).abs() < 1e-6);
        assert!((prog.ratio().unwrap() - 0.2).abs() < 1e-6);
    }
}

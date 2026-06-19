//! Background worker that runs a backup on a dedicated OS thread and streams progress
//! events back to the TUI's event loop over a sync `mpsc` channel.
//!
//! Why a thread, not a tokio task? The encoder + writer chain is fully synchronous and
//! blocking; we want it off the runtime thread so the TUI keeps drawing at 60 Hz while
//! the backup grinds through gigabytes. The TUI polls the channel non-blocking on every
//! iteration of its event loop.

use color_eyre::eyre::{eyre, Result};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tekflash_core::pipeline::compress::{encoder, Codec, CompressionLevel};
use tekflash_core::pipeline::hasher::{HashKind, Hasher};

#[derive(Debug, Clone)]
pub struct BackupParams {
    pub source: PathBuf,
    pub dest: PathBuf,
    pub codec: Codec,
    pub level: CompressionLevel,
    /// Total bytes of the source if known (block device size or file length).
    pub total_bytes: Option<u64>,
}

#[derive(Debug)]
pub enum BackupEvent {
    Tick {
        bytes_in: u64,
        bytes_out: u64,
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

/// Spawn the backup worker and return the channel + a handle to the OS thread + a live
/// `bytes_out` counter the UI can poll between ticks for smoother gauge updates.
pub fn spawn_backup(
    params: BackupParams,
) -> (Arc<AtomicU64>, Receiver<BackupEvent>, JoinHandle<()>) {
    let bytes_out = Arc::new(AtomicU64::new(0));
    let bytes_out_worker = bytes_out.clone();
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::Builder::new()
        .name("tekflash-backup".to_string())
        .spawn(move || {
            let started = Instant::now();
            match do_backup(&params, bytes_out_worker.clone(), &tx) {
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
        .expect("spawn backup thread");
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
            });
            last_tick = Instant::now();
        }
    }
    // Drop the encoder to flush trailers (zstd auto_finish, lz4 finish_on_drop, etc.).
    drop(writer);
    Ok((bytes_in, hasher.finalize_hex()))
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
                Ok(BackupEvent::Tick {
                    bytes_in,
                    bytes_out,
                }) => {
                    let dt = self.last_tick_at.elapsed().as_secs_f64().max(1e-3);
                    self.instant_rate = (bytes_in - self.last_bytes_in_at_tick) as f64 / dt;
                    self.last_bytes_in_at_tick = bytes_in;
                    self.last_tick_at = Instant::now();
                    self.bytes_in = bytes_in;
                    self.bytes_out = bytes_out;
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
    fn fraction_eta_and_ratio_only_compute_when_inputs_make_sense() {
        let mut prog = BackupProgress {
            device_idx: 0,
            params: BackupParams {
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

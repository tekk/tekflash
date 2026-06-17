//! Block-device / file writer.
//!
//! Counterpart to `reader::pump`. Runs in a blocking task, draining a channel of buffers
//! and writing them through. Calls `flush` at EOF; the verify-after-write pass is
//! responsible for the per-OS cache invalidation (`fsync` + `BLKFLSBUF` / `F_FULLFSYNC` /
//! `FlushFileBuffers`) before re-opening for read-back.

use super::buffer::Buffer;
use color_eyre::eyre::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use tokio::sync::mpsc::Receiver;

/// Open a file path for write. `create` controls O_CREAT; existing devices are opened
/// without it (the device node already exists). Currently uses the page cache; the
/// flash-to-device path will replace this with cfg-gated unbuffered wrappers.
pub fn open_for_write(path: &Path, create: bool) -> Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true);
    if create {
        opts.create(true).truncate(true);
    }
    opts.open(path)
        .with_context(|| format!("opening {} for write", path.display()))
}

/// Drain incoming buffers into the writer until the channel closes. Returns the total
/// byte count written.
pub async fn pump<W>(writer: W, mut input: Receiver<Buffer>) -> Result<u64>
where
    W: Write + Send + 'static,
{
    let handle = tokio::task::spawn_blocking(move || -> Result<u64> {
        let mut writer = writer;
        let mut total: u64 = 0;
        while let Some(buf) = futures::executor::block_on(input.recv()) {
            let slice = buf.as_slice();
            writer.write_all(slice).context("write_all")?;
            total += slice.len() as u64;
            drop(buf); // release back to pool
        }
        writer.flush().context("final flush")?;
        Ok(total)
    });
    handle.await.context("writer join")?
}

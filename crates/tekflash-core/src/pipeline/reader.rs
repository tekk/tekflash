//! Block-device / file reader.
//!
//! Spawns a blocking task that owns the reader for its lifetime and shovels filled
//! buffers across a tokio channel. This is the correct pattern for raw block devices
//! on macOS and Windows, which don't have async file I/O anyway.

use super::buffer::{Buffer, BufferPool};
use color_eyre::eyre::{Context, Result};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tokio::sync::mpsc::Sender;

/// Open a path for reading. The flash + verify path will later use cfg-gated wrappers to
/// set `O_DIRECT` / `F_NOCACHE` / `FILE_FLAG_NO_BUFFERING` — for now we punt on that and
/// use the page cache, which is correct for backups (and fine for tests).
pub fn open_for_read(path: &Path) -> Result<File> {
    File::open(path).with_context(|| format!("opening {} for read", path.display()))
}

/// Drive a reader, send filled buffers downstream until EOF, and report the total byte
/// count. Receiver drops each buffer to recycle it.
pub async fn pump<R>(reader: R, pool: BufferPool, out: Sender<Buffer>) -> Result<u64>
where
    R: Read + Send + 'static,
{
    let handle = tokio::task::spawn_blocking(move || -> Result<u64> {
        let mut reader = reader;
        let mut total: u64 = 0;
        loop {
            // Block on the pool acquire — we're already off the runtime thread.
            let mut buf = futures::executor::block_on(pool.acquire());
            let cap = buf.capacity();
            let n = reader
                .read(&mut buf.as_mut_capacity()[..cap])
                .context("read")?;
            buf.set_len(n);
            if n == 0 {
                drop(buf);
                return Ok(total);
            }
            total += n as u64;
            if futures::executor::block_on(out.send(buf)).is_err() {
                return Ok(total);
            }
        }
    });
    handle.await.context("reader join")?
}

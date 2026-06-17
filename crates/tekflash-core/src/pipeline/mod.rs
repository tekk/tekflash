//! Streaming I/O pipeline.
//!
//! Stages are connected by tokio bounded channels so a slow downstream stage applies
//! backpressure upstream — no unbounded memory growth on a fast SSD writing to a slow USB
//! stick. All buffers come from a pooled 4 MiB pool, aligned for Linux `O_DIRECT`.

pub mod buffer;
pub mod compress;
pub mod crypto;
pub mod format;
pub mod hasher;
pub mod reader;
pub mod verify;
pub mod writer;

pub use buffer::{Buffer, BufferPool};
pub use compress::{Codec, CompressionLevel};
pub use format::{detect, InputFormat};

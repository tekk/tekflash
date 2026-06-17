//! Aligned-buffer pool.
//!
//! Linux `O_DIRECT` requires the buffer pointer, byte count, and file offset to be
//! sector-aligned (usually 4096). On macOS / Windows the alignment isn't required but
//! costs us nothing to provide.

use std::alloc::{alloc, dealloc, Layout};
use std::ptr::NonNull;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Page-size alignment that satisfies every block device we care about. 4096 is the
/// universal floor; some 4K-native NVMe drives need exactly this.
const ALIGN: usize = 4096;

/// A single owned aligned buffer. Returned to the pool on drop.
pub struct Buffer {
    ptr: NonNull<u8>,
    cap: usize,
    len: usize,
    pool: Option<Arc<Mutex<Vec<RawBuffer>>>>,
}

// Safety: we own the heap allocation exclusively and only mutate via &mut self.
unsafe impl Send for Buffer {}
unsafe impl Sync for Buffer {}

struct RawBuffer {
    ptr: NonNull<u8>,
    cap: usize,
}

unsafe impl Send for RawBuffer {}

impl Buffer {
    /// Bytes available to read from this buffer.
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// Full mutable capacity. Caller is responsible for calling `set_len` after writing.
    pub fn as_mut_capacity(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.cap) }
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Mark `n` bytes as valid (must be `<= capacity`).
    pub fn set_len(&mut self, n: usize) {
        debug_assert!(n <= self.cap);
        self.len = n;
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.take() {
            // Hand the allocation back to the pool synchronously where possible. The
            // mutex is uncontended in steady state because every stage borrows briefly.
            if let Ok(mut g) = pool.try_lock() {
                g.push(RawBuffer {
                    ptr: self.ptr,
                    cap: self.cap,
                });
                return;
            }
            // Fall through and free if the pool is locked at drop time — better than
            // blocking on a Drop.
        }
        unsafe {
            dealloc(
                self.ptr.as_ptr(),
                Layout::from_size_align_unchecked(self.cap, ALIGN),
            );
        }
    }
}

/// Pool of aligned buffers. Stages acquire one, fill it, hand it downstream, and the
/// receiver releases it back here when done.
#[derive(Clone)]
pub struct BufferPool {
    buf_size: usize,
    free: Arc<Mutex<Vec<RawBuffer>>>,
}

impl BufferPool {
    pub fn new(buf_size: usize, initial: usize) -> Self {
        let aligned_size = align_up(buf_size, ALIGN);
        let mut free = Vec::with_capacity(initial);
        for _ in 0..initial {
            free.push(allocate(aligned_size));
        }
        Self {
            buf_size: aligned_size,
            free: Arc::new(Mutex::new(free)),
        }
    }

    pub fn buf_size(&self) -> usize {
        self.buf_size
    }

    /// Acquire a buffer. Reuses a previously released one if available, otherwise
    /// allocates fresh. Always returns successfully.
    pub async fn acquire(&self) -> Buffer {
        let raw = {
            let mut g = self.free.lock().await;
            g.pop()
        };
        let raw = raw.unwrap_or_else(|| allocate(self.buf_size));
        Buffer {
            ptr: raw.ptr,
            cap: raw.cap,
            len: 0,
            pool: Some(self.free.clone()),
        }
    }
}

fn allocate(size: usize) -> RawBuffer {
    let layout = Layout::from_size_align(size, ALIGN).expect("valid aligned layout");
    let ptr = unsafe { alloc(layout) };
    let ptr = NonNull::new(ptr).expect("allocation failed");
    RawBuffer { ptr, cap: size }
}

fn align_up(n: usize, align: usize) -> usize {
    (n + align - 1) & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn buffer_round_trips_through_pool() {
        let pool = BufferPool::new(4096, 2);
        let mut b = pool.acquire().await;
        assert_eq!(b.capacity(), 4096);
        b.as_mut_capacity()[0..3].copy_from_slice(b"abc");
        b.set_len(3);
        assert_eq!(b.as_slice(), b"abc");
        drop(b);
        // Pool should now hold the recycled buffer.
        let b2 = pool.acquire().await;
        assert_eq!(b2.capacity(), 4096);
    }
}

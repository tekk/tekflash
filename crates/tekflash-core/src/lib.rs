//! # tekflash-core — **internal implementation detail of [`tekflash`]**
//!
//! This crate is the engine behind the [`tekflash`](https://crates.io/crates/tekflash)
//! TUI: device enumeration, the streaming I/O pipeline, compression, encryption, and
//! archive support. It contains no UI; the binary crate `tekflash` drives this library
//! from either the TUI or the headless CLI entry points.
//!
//! ## Stability — there is none
//!
//! `tekflash-core` is published only because [Cargo requires it](https://doc.rust-lang.org/cargo/reference/registries.html)
//! in order to publish the `tekflash` binary that depends on it. **Treat every item
//! here as private.** The API may break in any patch release; renames, signature
//! changes, and item removals will not be accompanied by a major version bump.
//!
//! If you want to *use* tekflash, install the binary:
//!
//! ```sh
//! cargo install tekflash --locked
//! ```
//!
//! If you want to embed similar functionality in your own project, fork the relevant
//! module — depending on `tekflash-core` directly will break you.

pub mod archive;
pub mod device;
pub mod manifest;
pub mod pipeline;
pub mod privilege;
pub mod progress;

pub use color_eyre::Result;

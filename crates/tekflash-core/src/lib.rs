//! tekflash-core: device enumeration, streaming I/O pipeline, compression, encryption, archive.
//!
//! No UI here. The binary crate (`tekflash`) drives this library from either the TUI or
//! the headless CLI entry points.

pub mod archive;
pub mod device;
pub mod manifest;
pub mod pipeline;
pub mod privilege;
pub mod progress;

pub use color_eyre::Result;

//! NDJSON progress output for scripting / CI.

use color_eyre::Result;
use std::io::Write;
use tekflash_core::progress::Event;

#[allow(dead_code)] // wired up when the JSON dispatch path lands
pub fn emit(event: &Event) -> Result<()> {
    let line = serde_json::to_string(event)?;
    let mut out = std::io::stdout().lock();
    out.write_all(line.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

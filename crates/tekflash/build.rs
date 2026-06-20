//! Build script.
//!
//! On Windows, embeds an application manifest into the executable so it asks for the
//! UAC elevation prompt on launch (raw access to `\\.\PhysicalDriveN` needs
//! Administrator anyway) and declares UTF-8 as the active code page so the TUI's
//! box-drawing characters render the same as on macOS / Linux.
//!
//! On every other platform this is a no-op.

fn main() {
    #[cfg(windows)]
    {
        embed_manifest::embed_manifest_file("app.manifest").expect("embed_manifest_file failed");
        println!("cargo:rerun-if-changed=app.manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}

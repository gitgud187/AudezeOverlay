//! Standalone visual test harness. Screenshot code is not linked into the app.
//!
//! It drives Windows-only APIs; on other platforms this executable only says so.
#[cfg(windows)]
mod win32;

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    win32::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("check-blur only runs on Windows.");
    std::process::exit(2);
}

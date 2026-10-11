//! Measures the real USB worker, read-only. Quit the overlay before running.
//!
//! It drives Windows-only APIs; on other platforms this executable only says so.
#[cfg(windows)]
mod win32;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    win32::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("bench-worker only runs on Windows.");
    std::process::exit(2);
}

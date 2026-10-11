//! watch-audeze diagnostic.
//!
//! It drives Windows-only APIs; on other platforms this executable only says so.
#[cfg(windows)]
mod win32;

#[cfg(windows)]
fn main() {
    win32::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("watch-audeze only runs on Windows.");
    std::process::exit(2);
}

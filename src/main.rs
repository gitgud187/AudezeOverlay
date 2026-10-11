#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
mod platform;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
fn main() {
    ui::run();
}

/// Interim: the overlay window exists only for Windows so far; see docs/LINUX.md.
#[cfg(not(windows))]
fn main() {
    eprintln!("The overlay window is not available on this platform yet. See docs/LINUX.md.");
    std::process::exit(2);
}

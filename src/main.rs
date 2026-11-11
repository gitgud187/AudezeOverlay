#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
mod platform;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod ui;

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn main() {
    ui::run();
}

/// The overlay window exists for Windows, Linux (Wayland) and macOS.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn main() {
    eprintln!("The overlay window is not available on this platform.");
    std::process::exit(2);
}

#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
mod platform;
// Until the Linux shell has the tray, shortcut and single-instance handling, some of
// the shared logic is only reached from the Windows shell.
#[cfg_attr(not(windows), allow(dead_code, unused_imports))]
mod ui;

#[cfg(any(windows, target_os = "linux"))]
fn main() {
    ui::run();
}

/// The overlay window exists for Windows and Linux (Wayland).
#[cfg(not(any(windows, target_os = "linux")))]
fn main() {
    eprintln!("The overlay window is not available on this platform.");
    std::process::exit(2);
}

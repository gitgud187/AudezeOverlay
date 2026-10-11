#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
mod platform;
// Some shared UI entry points (tray hosting, menus, size limits, pointer capture
// loss) are only called by the Windows shell and are unused in the Linux build.
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

//! Native overlay presentation and event handling.
mod app;
mod draw;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod instance;
#[cfg(target_os = "macos")]
mod macos;
mod number_roll;
mod presentation;
mod shell;
mod view;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(windows)]
mod win32;
#[cfg(target_os = "macos")]
pub use macos::main as run;
#[cfg(target_os = "linux")]
pub use wayland::main as run;
#[cfg(windows)]
pub use win32::main as run;

/// The common command line for the Unix shells. Demo runs stay independent so
/// they never disturb a real instance; another launch forwards `open`, while
/// `--toggle` flips the panel.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::ui) fn parse_args() -> Result<(bool, instance::Request), String> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(unknown) = args
        .iter()
        .skip(1)
        .find(|a| !matches!(a.as_str(), "--demo" | "--toggle"))
    {
        return Err(format!(
            "unknown option {unknown} (options: --demo, --toggle)"
        ));
    }
    let demo = args.iter().any(|a| a == "--demo");
    let request = if args.iter().any(|a| a == "--toggle") {
        instance::Request::Toggle
    } else {
        instance::Request::Open
    };
    Ok((demo, request))
}

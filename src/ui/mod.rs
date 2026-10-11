//! Native overlay presentation and event handling.
mod app;
mod draw;
mod number_roll;
mod presentation;
mod shell;
mod view;
#[cfg(windows)]
mod win32;
#[cfg(windows)]
pub use win32::main as run;

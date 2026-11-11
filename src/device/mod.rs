//! Direct device integration. UI consumers send commands and receive sound snapshots.
#[cfg(windows)]
mod bluetooth_status;
/// Bluetooth status comes from Windows' connection cache; other platforms report none.
#[cfg(not(windows))]
mod bluetooth_status {
    pub(super) fn connected() -> Option<crate::model::HeadsetFamily> {
        None
    }
}
#[cfg(target_os = "macos")]
pub mod bluetooth;
mod cable_mix;
pub mod command;
#[cfg(any(target_os = "linux", target_os = "macos", test))]
mod descriptor;
mod hid;
mod link;
pub mod profiles;
pub mod protocol;
mod selection;
mod usb;
mod worker;
pub use hid::InputStream;
pub use protocol::{consume_stream_bytes, parse_frame, Query, QueryUpdates, QUERIES};
pub use usb::Device;
pub use worker::{command_matches, delivery_time, start, trace_present, Delivery};

#[cfg(feature = "perf-stats")]
pub mod perf;

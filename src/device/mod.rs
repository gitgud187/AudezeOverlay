//! Direct device integration. UI consumers send commands and receive sound snapshots.
pub mod command;
pub mod profiles;
mod usb;
mod worker;
pub use usb::{parse_frame, Device, Query, QueryUpdates, QUERIES};
pub use worker::{command_matches, delivery_time, start, trace_present, Delivery};

#[cfg(feature = "perf-stats")]
pub mod perf;

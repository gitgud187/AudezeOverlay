//! The platform's access to one vendor-defined HID control interface.
//!
//! Each platform enumerates the interfaces it can open and hands them upward as
//! [`Interface`]s; `selection` decides which one to use and `usb::Device` speaks
//! the protocol over it. Everything above this boundary is platform-independent.
use super::profiles::Profile;
use super::selection::Candidate;
type Result<T> = std::result::Result<T, String>;

/// An opened control interface. Dropping it releases the platform handle.
pub(super) trait Hid {
    /// Sends one complete output report, report ID first, of exactly the
    /// interface's output length. A short write is an error.
    fn write_report(&self, report: &[u8]) -> Result<()>;
    /// Fetches the current input report named by `report[0]`, without waiting
    /// for the headset to volunteer one.
    fn get_input_report(&self, report: &mut [u8; 64]) -> Result<()>;
    /// Unsolicited input reports. Only diagnostics use this; sound state is polled.
    fn listen(&self) -> Result<Box<dyn InputStream>>;
}

/// A blocking stream of unsolicited input reports.
pub trait InputStream {
    /// Waits up to `timeout_ms` for one report; `None` means none arrived.
    fn read(&mut self, timeout_ms: u32) -> std::result::Result<Option<Vec<u8>>, String>;
}

pub(super) struct Interface {
    /// Platform-specific identity, used to notice that the endpoint changed.
    pub id: String,
    /// Length of the output report, report ID included.
    pub output_len: usize,
    pub hid: Box<dyn Hid>,
}

pub(super) struct Enumeration {
    pub candidates: Vec<Candidate<Interface>>,
    /// Every wired identity noticed, even if its control interface was not usable.
    pub wired_seen: Vec<Profile>,
}

#[cfg(windows)]
mod win32;
#[cfg(windows)]
pub(super) use win32::enumerate;

/// No transport exists for this platform yet.
#[cfg(not(windows))]
pub(super) fn enumerate() -> Result<Enumeration> {
    Err("Direct headset access is not available on this platform yet.".into())
}

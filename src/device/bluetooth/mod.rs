//! The headset's control channel over Classic Bluetooth (RFCOMM serial port).
//!
//! The channel carries the same `05 5A`/`05 5B`/`05 5D` frames as the USB dongle,
//! without the HID report wrapper. Protocol notes and the evidence behind each
//! frame are in `docs/BLUETOOTH_PROTOCOL.md`. The device worker uses the channel
//! when the user has turned Bluetooth control on and no USB device is connected;
//! the read-only `probe-bluetooth` diagnostic uses it too.
mod link;
mod mac;

pub(super) use link::BluetoothLink;
pub use mac::{close_open_channel, host_main_run_loop, Channel, Event};

/// Exact name the headset is paired under. Names are user-editable, so this is a
/// lookup key for the diagnostic and never a capability grant.
pub const DEVICE_NAME: &str = "Audeze Maxwell BT";

/// Serial-port service UUID `00000000-0000-0000-0099-AABBCCDDEEFF` (service name
/// `Airoha_APP`). The RFCOMM channel number is resolved from it through SDP.
pub const SERVICE_UUID: [u8; 16] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0x00, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
];

/// Why the control channel could not be opened.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The headset is not paired or not connected, so there is nothing to open.
    /// This is the ordinary "no headset" case, not a fault.
    Absent(String),
    /// A headset is there but the channel could not be opened.
    Failed(String),
}

impl OpenError {
    pub fn message(&self) -> &str {
        match self {
            Self::Absent(message) | Self::Failed(message) => message,
        }
    }
}

impl From<OpenError> for String {
    fn from(error: OpenError) -> Self {
        error.message().to_owned()
    }
}

/// Refuses any frame outside [`permits`]. Every path that writes to the channel
/// goes through this, so no caller can put other bytes on the air.
pub(super) fn check_permitted(frame: &[u8]) -> Result<(), String> {
    if permits(frame) {
        Ok(())
    } else {
        Err("This request is not allowed over Bluetooth.".into())
    }
}

/// Whether `frame` may be sent over Bluetooth: the status reads, and the three
/// writes verified with read-back and restore on a Maxwell 1 (sidetone, sidetone
/// level, noise removal; see `docs/BLUETOOTH_PROTOCOL.md`). The mix is not
/// written: it balances the dongle's two audio devices and does nothing over
/// Bluetooth, which has one. Anything
/// else is refused, however the caller got hold of it. Airoha headsets accept
/// other commands, firmware update among them, which this application never sends.
pub fn permits(frame: &[u8]) -> bool {
    ReadRequest::ALL
        .iter()
        .any(|request| request.frame() == frame)
        || matches!(
            frame,
            // Sidetone off/on, sidetone level, noise removal off/low/high. 31 is the
            // sidetone maximum this unit reports.
            [0x05, 0x5a, 0x05, 0x00, 0x82, 0x2c, 0x07, 0x00, 0..=1]
                | [0x05, 0x5a, 0x05, 0x00, 0x00, 0x09, 0x2c, 0x00, 0..=31]
                | [0x05, 0x5a, 0x05, 0x00, 0x00, 0x09, 0x24, 0x00, 0..=2]
        )
}

/// A status read the headset answers without changing anything. This is the whole
/// set the diagnostic may send: there is deliberately no write request and no way
/// to send arbitrary bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadRequest {
    Battery,
    Firmware,
    Mix,
    Sidetone,
    SidetoneLevel,
    SidetoneMax,
    NoiseRemoval,
    MicMute,
    VoicePrompt,
    AutoShutdown,
    HeadsetType,
    Connection,
}

impl ReadRequest {
    pub const ALL: [Self; 12] = [
        Self::Battery,
        Self::Firmware,
        Self::Mix,
        Self::Sidetone,
        Self::SidetoneLevel,
        Self::SidetoneMax,
        Self::NoiseRemoval,
        Self::MicMute,
        Self::VoicePrompt,
        Self::AutoShutdown,
        Self::HeadsetType,
        Self::Connection,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Battery => "battery",
            Self::Firmware => "firmware",
            Self::Mix => "mix",
            Self::Sidetone => "sidetone",
            Self::SidetoneLevel => "sidetone level",
            Self::SidetoneMax => "sidetone max",
            Self::NoiseRemoval => "noise removal",
            Self::MicMute => "mic mute",
            Self::VoicePrompt => "voice prompt",
            Self::AutoShutdown => "auto shutdown",
            Self::HeadsetType => "headset type",
            Self::Connection => "connection",
        }
    }

    /// The complete request frame: `05 5A len_lo len_hi cmd_lo cmd_hi payload…`.
    pub fn frame(self) -> &'static [u8] {
        match self {
            Self::Battery => &[0x05, 0x5a, 0x03, 0x00, 0xd6, 0x0c, 0x00],
            Self::Firmware => &[0x05, 0x5a, 0x03, 0x00, 0x07, 0x1c, 0x00],
            Self::Mix => &[0x05, 0x5a, 0x04, 0x00, 0x83, 0x2c, 0x0b, 0x00],
            Self::Sidetone => &[0x05, 0x5a, 0x04, 0x00, 0x83, 0x2c, 0x07, 0x00],
            Self::SidetoneLevel => &[0x05, 0x5a, 0x04, 0x00, 0x01, 0x09, 0x2c, 0x00],
            Self::SidetoneMax => &[0x05, 0x5a, 0x04, 0x00, 0x01, 0x09, 0x2d, 0x00],
            Self::NoiseRemoval => &[0x05, 0x5a, 0x04, 0x00, 0x01, 0x09, 0x24, 0x00],
            Self::MicMute => &[0x05, 0x5a, 0x04, 0x00, 0x01, 0x09, 0x22, 0x00],
            Self::VoicePrompt => &[0x05, 0x5a, 0x04, 0x00, 0x01, 0x09, 0x20, 0x00],
            Self::AutoShutdown => &[0x05, 0x5a, 0x04, 0x00, 0x83, 0x2c, 0x01, 0x00],
            Self::HeadsetType => &[0x05, 0x5a, 0x04, 0x00, 0x01, 0x09, 0x2b, 0x00],
            Self::Connection => &[0x05, 0x5a, 0x04, 0x00, 0x01, 0x09, 0x26, 0x00],
        }
    }

    /// Whether `frame` is the reply that carries this request's value.
    ///
    /// Most reads are answered by a `5B` frame. Battery and firmware are first
    /// acknowledged by a bare `5B` and then answered by a `5D` notification, so
    /// only the `5D` counts. The echoed bytes after the header are the request's
    /// command with a zero byte inserted for the `83 2C` family (`O` in
    /// `docs/BLUETOOTH_PROTOCOL.md`).
    pub fn answered_by(self, frame: &[u8]) -> bool {
        let (header, echo): (u8, &[u8]) = match self {
            Self::Battery => (0x5d, &[0xd6, 0x0c, 0x00]),
            Self::Firmware => (0x5d, &[0x07, 0x1c, 0x00]),
            Self::Mix => (0x5b, &[0x83, 0x2c, 0x00, 0x0b, 0x00]),
            Self::Sidetone => (0x5b, &[0x83, 0x2c, 0x00, 0x07, 0x00]),
            Self::AutoShutdown => (0x5b, &[0x83, 0x2c, 0x00, 0x01, 0x00]),
            Self::SidetoneLevel => (0x5b, &[0x01, 0x09, 0x2c, 0x00]),
            Self::SidetoneMax => (0x5b, &[0x01, 0x09, 0x2d, 0x00]),
            Self::NoiseRemoval => (0x5b, &[0x01, 0x09, 0x24, 0x00]),
            Self::MicMute => (0x5b, &[0x01, 0x09, 0x22, 0x00]),
            Self::VoicePrompt => (0x5b, &[0x01, 0x09, 0x20, 0x00]),
            Self::HeadsetType => (0x5b, &[0x01, 0x09, 0x2b, 0x00]),
            Self::Connection => (0x5b, &[0x01, 0x09, 0x26, 0x00]),
        };
        // At least one value byte must follow the echoed command.
        frame.len() > 4 + echo.len()
            && frame[0] == 5
            && frame[1] == header
            && frame[4..].starts_with(echo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_request_is_a_well_formed_read() {
        for request in ReadRequest::ALL {
            let frame = request.frame();
            assert_eq!(&frame[..2], &[0x05, 0x5a], "{request:?}");
            let len = u16::from_le_bytes([frame[2], frame[3]]) as usize;
            assert_eq!(frame.len(), 4 + len, "{request:?} length field");
            // Writes use the `82 2C` and `00 09` command prefixes. A read request
            // must never be mistaken for one.
            assert_ne!(frame[4], 0x82, "{request:?}");
            assert_ne!(frame[4..6], [0x00, 0x09], "{request:?}");
        }
    }

    #[test]
    fn only_reads_and_the_three_verified_writes_are_permitted() {
        for request in ReadRequest::ALL {
            assert!(permits(request.frame()), "{request:?}");
        }
        // The documented write frames, at their edges.
        for frame in [
            &[0x05, 0x5a, 0x05, 0x00, 0x82, 0x2c, 0x07, 0x00, 0][..],
            &[0x05, 0x5a, 0x05, 0x00, 0x82, 0x2c, 0x07, 0x00, 1],
            &[0x05, 0x5a, 0x05, 0x00, 0x00, 0x09, 0x2c, 0x00, 31],
            &[0x05, 0x5a, 0x05, 0x00, 0x00, 0x09, 0x24, 0x00, 2],
        ] {
            assert!(permits(frame), "{frame:02x?}");
        }
        // Out-of-range values, the unverified writes and arbitrary bytes.
        for frame in [
            // The mix write: valid on the dongle, meaningless over Bluetooth.
            &[0x05, 0x5a, 0x05, 0x00, 0x82, 0x2c, 0x0b, 0x00, 10][..],
            &[0x05, 0x5a, 0x05, 0x00, 0x82, 0x2c, 0x07, 0x00, 2],
            &[0x05, 0x5a, 0x05, 0x00, 0x00, 0x09, 0x2c, 0x00, 32],
            &[0x05, 0x5a, 0x05, 0x00, 0x00, 0x09, 0x24, 0x00, 3],
            // Voice prompt and auto-shutdown writes: documented, not exposed.
            &[0x05, 0x5a, 0x05, 0x00, 0x00, 0x09, 0x20, 0x00, 4],
            &[
                0x05, 0x5a, 0x0c, 0x00, 0x82, 0x2c, 0x01, 0x00, 0x01, 0x00, 0x10, 0x0e, 0x01, 0x00,
                0x10, 0x0e,
            ],
            // A valid write with an extra byte, a truncated one, and noise.
            &[0x05, 0x5a, 0x05, 0x00, 0x82, 0x2c, 0x0b, 0x00, 10, 0],
            &[0x05, 0x5a, 0x05, 0x00, 0x82, 0x2c, 0x0b, 0x00],
            &[0x05, 0x5a, 0x03, 0x00, 0x51, 0x1c, 0x00],
            &[],
        ] {
            assert!(!permits(frame), "{frame:02x?}");
        }
    }

    #[test]
    fn the_shared_check_refuses_what_is_not_permitted() {
        for request in ReadRequest::ALL {
            assert!(check_permitted(request.frame()).is_ok());
        }
        assert!(check_permitted(&[0x05, 0x5a, 0x03, 0x00, 0x51, 0x1c, 0x00]).is_err());
        assert!(check_permitted(&[]).is_err());
    }

    #[test]
    fn requests_are_distinct() {
        for (i, a) in ReadRequest::ALL.iter().enumerate() {
            for b in &ReadRequest::ALL[i + 1..] {
                assert_ne!(a.frame(), b.frame(), "{a:?} and {b:?}");
            }
        }
    }

    /// Replies copied from the macOS probe runs recorded in
    /// `docs/BLUETOOTH_PROTOCOL.md` (one Maxwell 1, firmware v1.0.1.74).
    #[test]
    fn observed_replies_answer_only_their_own_request() {
        let replies: [(ReadRequest, &[u8]); 6] = [
            (
                ReadRequest::Battery,
                &[0x05, 0x5d, 0x05, 0x00, 0xd6, 0x0c, 0x00, 0x00, 0x3b],
            ),
            (
                ReadRequest::Mix,
                &[0x05, 0x5b, 0x06, 0x00, 0x83, 0x2c, 0x00, 0x0b, 0x00, 0x0a],
            ),
            (
                ReadRequest::Sidetone,
                &[0x05, 0x5b, 0x06, 0x00, 0x83, 0x2c, 0x00, 0x07, 0x00, 0x00],
            ),
            (
                ReadRequest::NoiseRemoval,
                &[0x05, 0x5b, 0x06, 0x00, 0x01, 0x09, 0x24, 0x00, 0x00, 0x01],
            ),
            (
                ReadRequest::Connection,
                &[0x05, 0x5b, 0x06, 0x00, 0x01, 0x09, 0x26, 0x00, 0x00, 0x02],
            ),
            (
                ReadRequest::Firmware,
                &[
                    0x05, 0x5d, 0x0e, 0x00, 0x07, 0x1c, 0x00, 0x00, 0x09, 0x76, 0x31, 0x2e, 0x30,
                    0x2e, 0x31, 0x2e, 0x37, 0x34,
                ],
            ),
        ];
        for (expected, frame) in replies {
            for request in ReadRequest::ALL {
                assert_eq!(
                    request.answered_by(frame),
                    request == expected,
                    "{request:?}"
                );
            }
        }
    }

    #[test]
    fn a_bare_acknowledgement_does_not_answer_battery_or_firmware() {
        let ack = [0x05, 0x5b, 0x03, 0x00, 0xd6, 0x0c, 0x00];
        assert!(!ReadRequest::Battery.answered_by(&ack));
    }
}

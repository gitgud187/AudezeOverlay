//! The headset's sound commands over one connection.
//! Wire framing and parsing are in `protocol`; what carries the frames is a
//! `Link`, which for the dongle and USB cable is the platform's HID access in `hid`.
#[cfg(target_os = "macos")]
use super::bluetooth::{self, BluetoothLink, Channel};
use super::command::Command;
use super::hid::{InputStream, Interface};
use super::link::{HidLink, Link};
use super::profiles::Profile;
use super::protocol::{consume_stream_bytes, continuation_allowed, Query, QueryUpdates, ANC_MODES};
use super::{hid, selection};
use crate::model::SoundState;
type Result<T> = std::result::Result<T, String>;
pub struct Device {
    pub profile: Profile,
    /// Matching wired USB identity present, even if firmware exposes controls
    /// only through the dongle while both connections are attached.
    pub usb_cable: bool,
    sidetone_max: Option<u8>,
    /// Platform-specific identity of the opened interface.
    id: String,
    link: Box<dyn Link>,
    /// What the link delivered on the latest `receive`, not yet consumed.
    chunk: Vec<u8>,
    stream: Vec<u8>,
    eq_slot: u8,
    eq_read_slot: Option<u8>,
}
impl Device {
    /// Whether the latest `receive` left a response frame unfinished, so reading
    /// again is worthwhile. `delivered` is what that `receive` returned.
    pub(super) fn needs_continuation(&self, delivered: bool, extra_reads: usize) -> bool {
        continuation_allowed(delivered, &self.stream, extra_reads)
    }

    /// Opens the one control interface this application should use. Fails with an
    /// explanation when nothing usable is connected or the choice is ambiguous.
    pub fn open() -> Result<Self> {
        Self::open_with(false)
    }
    /// Like [`Device::open`], and when `bluetooth` is allowed and no USB device is
    /// connected, falls back to the headset's Bluetooth control channel. A USB
    /// device always wins: with the dongle or cable present, Bluetooth is not
    /// opened at all. Several USB candidates are an error, never a reason to fall
    /// back. Bluetooth is only attempted where it exists (macOS).
    pub fn open_with(bluetooth: bool) -> Result<Self> {
        let found = hid::enumerate()?;
        match selection::choose(found.candidates, &found.wired_seen) {
            Ok(chosen) => Ok(Self::new(chosen.profile, chosen.usb_cable, chosen.endpoint)),
            Err(selection::SelectionError::NoDevice) if bluetooth => Self::open_bluetooth()
                .map_err(|e| e.unwrap_or_else(|| selection::SelectionError::NoDevice.to_string())),
            Err(error) => Err(error.to_string()),
        }
    }
    /// Opens the Bluetooth control channel. `Err(None)` means there is no headset
    /// to open, which is the ordinary "nothing connected" case; `Err(Some(_))`
    /// explains a real failure.
    #[cfg(target_os = "macos")]
    fn open_bluetooth() -> std::result::Result<Self, Option<String>> {
        match Channel::open(bluetooth::DEVICE_NAME) {
            Ok(channel) => Ok(Self::with_link(
                Profile::bluetooth_maxwell(),
                false,
                format!("bluetooth:{}", bluetooth::DEVICE_NAME),
                Box::new(BluetoothLink::new(channel)),
            )),
            Err(bluetooth::OpenError::Absent(_)) => Err(None),
            Err(bluetooth::OpenError::Failed(message)) => Err(Some(message)),
        }
    }
    #[cfg(not(target_os = "macos"))]
    fn open_bluetooth() -> std::result::Result<Self, Option<String>> {
        Err(None)
    }
    fn new(profile: Profile, usb_cable: bool, interface: Interface) -> Self {
        let link = HidLink::new(interface.hid, interface.output_len, profile.route());
        Self::with_link(profile, usb_cable, interface.id, Box::new(link))
    }
    fn with_link(profile: Profile, usb_cable: bool, id: String, link: Box<dyn Link>) -> Self {
        Self {
            profile,
            usb_cable,
            sidetone_max: profile.verified().then_some(31),
            id,
            link,
            chunk: Vec::with_capacity(64),
            stream: Vec::with_capacity(256),
            eq_slot: 6,
            eq_read_slot: None,
        }
    }
    /// Whether this device is controlled over Bluetooth.
    pub fn is_bluetooth(&self) -> bool {
        self.profile.connection == crate::model::Connection::Bluetooth
    }
    /// Whether the connection this device was opened on is no longer the one to
    /// use, so the caller should reopen. A USB device is replaced when its
    /// interface changed. A Bluetooth device is replaced when a USB device has
    /// appeared (USB wins) or Bluetooth is no longer `allowed`.
    pub fn endpoint_changed(&self, bluetooth_allowed: bool) -> Result<bool> {
        if self.is_bluetooth() {
            return Ok(!bluetooth_allowed || Self::open_with(false).is_ok());
        }
        let current = Self::open_with(false)?;
        Ok(!self.same_endpoint(&current))
    }
    pub fn same_endpoint(&self, other: &Self) -> bool {
        self.id == other.id && self.usb_cable == other.usb_cable
    }
    pub fn initial_state(&self) -> SoundState {
        SoundState {
            usb_cable: self.usb_cable,
            family: Some(self.profile.family),
            transport: Some(self.profile.connection),
            capabilities: Some(self.profile.capabilities()),
            experimental_device: !self.profile.verified(),
            xbox: self.profile.xbox,
            sidetone_max: self.sidetone_max,
            device: Some(self.profile.family.name().into()),
            connection: Some(self.profile.connection.label().into()),
            ..Default::default()
        }
    }
    /// Unsolicited input reports, for diagnostics. Sound state is polled instead.
    pub fn listen(&self) -> Result<Box<dyn InputStream>> {
        self.link.listen()
    }
    fn packet(&mut self, body: &[u8]) -> Result<()> {
        self.link.send(body)
    }
    pub fn query(&mut self, q: Query) -> Result<()> {
        if !self.profile.supports_query(q) {
            return Err("This query is unavailable for this headset model.".into());
        }
        self.eq_read_slot = (q == Query::EqBands).then_some(self.eq_slot);
        self.packet(match q {
            Query::EqWriteAck | Query::EqSelectAck => return Ok(()),
            Query::EqIndex if !self.profile.capabilities().anc => &[5, 0x5a, 4, 0, 1, 9, 0, 0],
            Query::EqIndex => &[5, 0x5a, 3, 0, 0x30, 0x2e, 0x0d],
            Query::EqBands => {
                return self.packet(&[5, 0x5a, 5, 0, 0x30, 0x2e, 0x32, self.eq_slot, 10])
            }
            Query::Mix => &[5, 0x5a, 4, 0, 0x83, 0x2c, 0x0b, 0],
            Query::Side => &[5, 0x5a, 4, 0, 0x83, 0x2c, 7, 0],
            Query::Level => &[5, 0x5a, 4, 0, 1, 9, 0x2c, 0],
            Query::MaxSidetone => &[5, 0x5a, 4, 0, 1, 9, 0x2d, 0],
            Query::Noise => &[5, 0x5a, 4, 0, 1, 9, 0x24, 0],
            Query::Mute => &[5, 0x5a, 4, 0, 1, 9, 0x22, 0],
            Query::Anc => &[5, 0x5a, 3, 0, 0x30, 0x2e, 2],
            Query::Battery => &[5, 0x5a, 3, 0, 0xd6, 0x0c, 0],
        })
    }
    pub fn set(&mut self, c: &Command) -> Result<Query> {
        if !self.profile.supports_command(c) {
            return Err("This control is unavailable for this headset model.".into());
        }
        match c {
            Command::EqLoad(slot) if (6..=9).contains(slot) => {
                self.eq_slot = *slot;
                return Ok(Query::EqBands);
            }
            Command::EqRefresh => return Ok(Query::EqIndex),
            Command::EqGain { slot, band } => {
                self.packet(&crate::eq::gain_command(*slot, *band)?)?;
                self.eq_slot = *slot;
                return Ok(Query::EqWriteAck);
            }
            _ => (),
        }
        let (q, body) = match c {
            Command::EqSelect(slot) if *slot < 10 && !self.profile.capabilities().anc => (
                Query::EqSelectAck,
                vec![5, 0x5a, 5, 0, 0, 9, 0, 0, *slot + 1],
            ),
            Command::EqSelect(slot) if *slot < 10 => (
                Query::EqSelectAck,
                vec![5, 0x5a, 4, 0, 0x30, 0x2e, 0x0c, *slot],
            ),
            Command::Mix(p) if *p <= 100 => (
                Query::Mix,
                vec![
                    5,
                    0x5a,
                    5,
                    0,
                    0x82,
                    0x2c,
                    0x0b,
                    0,
                    ((*p as f32 / 5.).round() as u8).min(20),
                ],
            ),
            Command::Sidetone(b) => (
                Query::Side,
                vec![5, 0x5a, 5, 0, 0x82, 0x2c, 7, 0, u8::from(*b)],
            ),
            Command::SidetoneLevel(p) if *p <= 100 => (
                Query::Level,
                vec![
                    5,
                    0x5a,
                    5,
                    0,
                    0,
                    9,
                    0x2c,
                    0,
                    (*p as f32
                        * self
                            .sidetone_max
                            .ok_or("Sidetone range is not available yet")?
                            as f32
                        / 100.)
                        .round() as u8,
                ],
            ),
            Command::Anc(s) => {
                let n = ANC_MODES
                    .iter()
                    .position(|mode| *mode == s)
                    .ok_or("Unknown ANC mode")? as u8;
                (Query::Anc, vec![5, 0x5a, 4, 0, 0x30, 0x2e, 1, n])
            }
            Command::Noise(s) => {
                let n = match s.as_str() {
                    "Off" => 0,
                    "Low" => 1,
                    "High" => 2,
                    _ => return Err("Unknown FILTER mode".into()),
                };
                (Query::Noise, vec![5, 0x5a, 5, 0, 0, 9, 0x24, 0, n])
            }
            _ => return Err("Unsupported sound value".into()),
        };
        self.packet(&body)?;
        Ok(q)
    }
    /// Receives whatever the headset has sent and consumes the frames it completes.
    pub fn read(&mut self, state: &mut SoundState) -> Result<QueryUpdates> {
        self.receive()?;
        Ok(self.consume(state))
    }
    /// Takes in the bytes that have already arrived, without waiting. Returns
    /// whether the connection delivered anything, even bytes that were unusable.
    pub fn receive(&mut self) -> Result<bool> {
        self.link.receive(&mut self.chunk)
    }
    /// The bytes the latest `receive` delivered, before `consume` takes them.
    pub fn chunk(&self) -> &[u8] {
        &self.chunk
    }
    /// The same delivery as it was on the wire, wrapper included, for traces.
    pub fn wire_bytes(&self) -> &[u8] {
        self.link.wire_bytes(&self.chunk)
    }
    pub fn consume(&mut self, state: &mut SoundState) -> QueryUpdates {
        self.consume_observed(state, |_, _, _| {})
    }
    pub(super) fn consume_observed(
        &mut self,
        state: &mut SoundState,
        observed: impl FnMut(&[u8], Option<Query>, &SoundState),
    ) -> QueryUpdates {
        // Diagnostics can start with an empty snapshot too. Never decode a
        // wired frame using a default wireless route or a guessed level scale.
        state.family = Some(self.profile.family);
        state.transport = Some(self.profile.connection);
        state.capabilities = Some(self.profile.capabilities());
        state.sidetone_max = self.sidetone_max;
        let slot = self.eq_read_slot;
        let chunk = std::mem::take(&mut self.chunk);
        let updates = consume_stream_bytes(&mut self.stream, &chunk, state, observed, slot);
        // Taken: consuming twice must not feed the same bytes again.
        self.chunk = chunk;
        self.chunk.clear();
        self.sidetone_max = state.sidetone_max;
        updates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::hid::Hid;
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    type Writes = Rc<RefCell<Vec<Vec<u8>>>>;
    /// Records every output report and replays scripted input reports.
    struct RecordingHid(Writes, RefCell<VecDeque<[u8; 64]>>);
    impl Hid for RecordingHid {
        fn write_report(&self, report: &[u8]) -> Result<()> {
            self.0.borrow_mut().push(report.to_vec());
            Ok(())
        }
        fn get_input_report(&self, report: &mut [u8; 64]) -> Result<()> {
            if let Some(next) = self.1.borrow_mut().pop_front() {
                *report = next;
            }
            Ok(())
        }
        fn listen(&self) -> Result<Box<dyn InputStream>> {
            Err("not needed".into())
        }
    }
    fn device(product: u16) -> (Device, Writes) {
        device_replaying(product, &[])
    }
    fn device_replaying(product: u16, inputs: &[[u8; 64]]) -> (Device, Writes) {
        let writes = Writes::default();
        let profile = Profile::from_usb(0x3329, product).expect("known identity");
        let interface = Interface {
            id: "test".into(),
            output_len: 62,
            hid: Box::new(RecordingHid(
                writes.clone(),
                RefCell::new(inputs.iter().copied().collect()),
            )),
        };
        (Device::new(profile, false, interface), writes)
    }

    #[test]
    fn mix_is_sent_in_five_percent_steps_with_the_dongle_or_cable_route() {
        for (product, route) in [(0x4ba9, 0x80), (0x4baa, 0)] {
            let (mut device, writes) = device(product);
            device.set(&Command::Mix(52)).unwrap();
            let report = writes.borrow()[0].clone();
            assert_eq!(report.len(), 62, "a full output report");
            assert_eq!(
                report[..12],
                [6, 9, route, 5, 0x5a, 5, 0, 0x82, 0x2c, 0x0b, 0, 10],
                "report ID, length, route, then the mix body (52% rounds to step 10)"
            );
            assert!(report[12..].iter().all(|b| *b == 0));
        }
    }

    #[test]
    fn controls_a_model_lacks_are_rejected_before_anything_is_written() {
        // The original Maxwell has no ANC and no EQ editing.
        let (mut device, writes) = device(0x4b18);
        assert!(device.set(&Command::Anc("On".into())).is_err());
        assert!(device.query(Query::Anc).is_err());
        assert!(device.query(Query::EqBands).is_err());
        assert!(writes.borrow().is_empty());
        device.set(&Command::EqSelect(2)).unwrap();
        assert_eq!(writes.borrow().len(), 1, "preset selection stays available");
    }

    #[test]
    fn an_unknown_sidetone_range_is_never_guessed() {
        let (mut unverified, writes) = device(0x4b18);
        assert!(unverified.set(&Command::SidetoneLevel(50)).is_err());
        assert!(writes.borrow().is_empty());
        // Only the validated identity keeps the known 31-step fallback.
        let (mut verified, writes) = device(0x4ba9);
        verified.set(&Command::SidetoneLevel(50)).unwrap();
        assert_eq!(writes.borrow()[0][9..12], [0x2c, 0, 16]);
    }

    /// A link that records every frame sent and replays scripted chunks.
    struct FakeLink(Writes, VecDeque<Vec<u8>>);
    impl Link for FakeLink {
        fn send(&mut self, frame: &[u8]) -> Result<()> {
            self.0.borrow_mut().push(frame.to_vec());
            Ok(())
        }
        fn receive(&mut self, chunk: &mut Vec<u8>) -> Result<bool> {
            chunk.clear();
            match self.1.pop_front() {
                Some(bytes) => {
                    chunk.extend_from_slice(&bytes);
                    Ok(true)
                }
                None => Ok(false),
            }
        }
    }
    fn bluetooth_device(replies: &[&[u8]]) -> (Device, Writes) {
        let writes = Writes::default();
        let link = FakeLink(writes.clone(), replies.iter().map(|r| r.to_vec()).collect());
        let device = Device::with_link(
            Profile::bluetooth_maxwell(),
            false,
            "bluetooth".into(),
            Box::new(link),
        );
        (device, writes)
    }

    #[test]
    fn bluetooth_sends_bare_frames_for_the_verified_controls_only() {
        let (mut device, writes) = bluetooth_device(&[]);
        assert!(device.is_bluetooth());
        device.set(&Command::Mix(52)).unwrap();
        device.set(&Command::Sidetone(true)).unwrap();
        device.set(&Command::Noise("High".into())).unwrap();
        device.query(Query::Battery).unwrap();
        assert_eq!(
            *writes.borrow(),
            [
                vec![5, 0x5a, 5, 0, 0x82, 0x2c, 0x0b, 0, 10],
                vec![5, 0x5a, 5, 0, 0x82, 0x2c, 7, 0, 1],
                vec![5, 0x5a, 5, 0, 0, 9, 0x24, 0, 2],
                vec![5, 0x5a, 3, 0, 0xd6, 0x0c, 0],
            ],
            "no HID report header or route byte"
        );
        let before = writes.borrow().len();
        assert!(device.set(&Command::Anc("On".into())).is_err());
        assert!(device.set(&Command::EqSelect(2)).is_err());
        assert!(device.query(Query::Anc).is_err());
        assert!(device.query(Query::EqBands).is_err());
        assert_eq!(writes.borrow().len(), before, "nothing else was sent");
    }

    #[test]
    fn bluetooth_range_is_read_before_a_sidetone_level_is_sent() {
        let (mut device, writes) = bluetooth_device(&[&[5, 0x5b, 6, 0, 1, 9, 0x2d, 0, 0, 31]]);
        assert!(device.set(&Command::SidetoneLevel(50)).is_err());
        assert!(writes.borrow().is_empty());
        let mut state = SoundState::default();
        assert!(device
            .read(&mut state)
            .unwrap()
            .contains(&Query::MaxSidetone));
        device.set(&Command::SidetoneLevel(50)).unwrap();
        assert_eq!(writes.borrow()[0], [5, 0x5a, 5, 0, 0, 9, 0x2c, 0, 16]);
    }

    #[test]
    fn bluetooth_replies_arriving_together_or_split_are_all_parsed() {
        let mix = [5, 0x5b, 6, 0, 0x83, 0x2c, 0, 0x0b, 0, 12];
        let battery = [5, 0x5d, 5, 0, 0xd6, 0x0c, 0, 0, 59];
        let both = [mix.as_slice(), battery.as_slice()].concat();
        let (mut device, _) = bluetooth_device(&[&both[..4], &both[4..]]);
        let mut state = SoundState::default();
        assert!(device.read(&mut state).unwrap().is_empty());
        let updates: Vec<_> = device.read(&mut state).unwrap().into_iter().collect();
        assert_eq!(updates, [Query::Mix, Query::Battery]);
        assert_eq!(state.mix, Some((40, 60)));
        assert_eq!(state.battery, Some(59));
        assert_eq!(state.transport, Some(crate::model::Connection::Bluetooth));
    }

    #[test]
    fn replies_for_another_route_are_ignored() {
        // A well-formed mix reply (50/50) that arrived on the wired route, then
        // the same reply on the dongle route this headset uses.
        let mut wired = [0u8; 64];
        wired[..13].copy_from_slice(&[7, 10, 0, 5, 0x5b, 6, 0, 0x83, 0x2c, 0, 0x0b, 0, 10]);
        let mut dongle = wired;
        dongle[2] = 0x80;
        let (mut device, _) = device_replaying(0x4ba9, &[wired, dongle]);
        let mut state = SoundState::default();
        assert!(device.read(&mut state).unwrap().is_empty());
        assert_eq!(state.mix, None);
        assert!(device.read(&mut state).unwrap().contains(&Query::Mix));
        assert_eq!(state.mix, Some((50, 50)));
        assert!(
            device.consume(&mut state).is_empty(),
            "a consumed chunk is not fed twice"
        );
    }
}

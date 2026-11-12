//! The headset's sound commands over one connection.
//! Wire framing and parsing are in `protocol`; what carries the frames is a
//! `Link`, which for the dongle and USB cable is the platform's HID access in `hid`.
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
        let found = hid::enumerate()?;
        let chosen =
            selection::choose(found.candidates, &found.wired_seen).map_err(|e| e.to_string())?;
        Ok(Self::new(chosen.profile, chosen.usb_cable, chosen.endpoint))
    }
    fn new(profile: Profile, usb_cable: bool, interface: Interface) -> Self {
        Self {
            profile,
            usb_cable,
            sidetone_max: profile.verified().then_some(31),
            id: interface.id,
            link: Box::new(HidLink::new(
                interface.hid,
                interface.output_len,
                profile.route(),
            )),
            chunk: Vec::with_capacity(64),
            stream: Vec::with_capacity(256),
            eq_slot: 6,
            eq_read_slot: None,
        }
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

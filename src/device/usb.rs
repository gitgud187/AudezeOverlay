//! The headset's sound commands over one vendor-defined HID interface.
//! Wire framing and parsing are in `protocol`; the platform's HID access is in `hid`.
use super::command::Command;
use super::hid::{Hid, InputStream, Interface};
use super::profiles::Profile;
use super::protocol::{
    consume_report_bytes_with_slot, continuation_allowed, Query, QueryUpdates, ANC_MODES,
};
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
    output_len: usize,
    hid: Box<dyn Hid>,
    stream: Vec<u8>,
    eq_slot: u8,
    eq_read_slot: Option<u8>,
}
impl Device {
    pub(super) fn needs_continuation(&self, report: &[u8], extra_reads: usize) -> bool {
        continuation_allowed(report, &self.stream, extra_reads)
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
            output_len: interface.output_len,
            hid: interface.hid,
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
        self.hid.listen()
    }
    fn packet(&self, body: &[u8]) -> Result<()> {
        let mut storage = [0u8; 512];
        let bytes = &mut storage[..self.output_len];
        bytes[0] = 6;
        bytes[1] = body.len() as u8;
        bytes[2] = self.profile.route();
        bytes[3..3 + body.len()].copy_from_slice(body);
        self.hid.write_report(bytes)
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
    pub fn read(&mut self, state: &mut SoundState) -> Result<QueryUpdates> {
        let bytes = self.read_report()?;
        Ok(self.consume_report(&bytes, state))
    }
    pub fn read_report(&self) -> Result<[u8; 64]> {
        let mut bytes = [0u8; 64];
        bytes[0] = 7;
        self.hid.get_input_report(&mut bytes)?;
        Ok(bytes)
    }
    pub fn consume_report(&mut self, bytes: &[u8], state: &mut SoundState) -> QueryUpdates {
        self.consume_report_observed(bytes, state, |_, _, _| {})
    }
    pub(super) fn consume_report_observed(
        &mut self,
        bytes: &[u8],
        state: &mut SoundState,
        mut observed: impl FnMut(&[u8], Option<Query>, &SoundState),
    ) -> QueryUpdates {
        if bytes.get(2) != Some(&self.profile.route()) {
            return QueryUpdates::default();
        }
        // Diagnostics can start with an empty snapshot too. Never decode a
        // wired frame using a default wireless route or a guessed level scale.
        state.family = Some(self.profile.family);
        state.transport = Some(self.profile.connection);
        state.capabilities = Some(self.profile.capabilities());
        state.sidetone_max = self.sidetone_max;
        let slot = self.eq_read_slot;
        let updates = consume_report_bytes_with_slot(
            &mut self.stream,
            bytes,
            state,
            |frame, field, state| observed(frame, field, state),
            slot,
        );
        self.sidetone_max = state.sidetone_max;
        updates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    type Writes = Rc<RefCell<Vec<Vec<u8>>>>;
    struct RecordingHid(Writes);
    impl Hid for RecordingHid {
        fn write_report(&self, report: &[u8]) -> Result<()> {
            self.0.borrow_mut().push(report.to_vec());
            Ok(())
        }
        fn get_input_report(&self, _: &mut [u8; 64]) -> Result<()> {
            Ok(())
        }
        fn listen(&self) -> Result<Box<dyn InputStream>> {
            Err("not needed".into())
        }
    }
    fn device(product: u16) -> (Device, Writes) {
        let writes = Writes::default();
        let profile = Profile::from_usb(0x3329, product).expect("known identity");
        let interface = Interface {
            id: "test".into(),
            output_len: 62,
            hid: Box::new(RecordingHid(writes.clone())),
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
        let (mut device, _) = device(0x4ba9);
        let mut state = SoundState::default();
        // A well-formed mix reply (50/50) that arrived on the wired route.
        let reply = [7, 10, 0, 5, 0x5b, 6, 0, 0x83, 0x2c, 0, 0x0b, 0, 10];
        assert!(device.consume_report(&reply, &mut state).is_empty());
        assert_eq!(state.mix, None);
        let mut dongle = reply;
        dongle[2] = 0x80;
        assert!(device
            .consume_report(&dongle, &mut state)
            .contains(&Query::Mix));
        assert_eq!(state.mix, Some((50, 50)));
    }
}

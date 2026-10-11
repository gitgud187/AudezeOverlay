//! HID access, bounded response framing and Maxwell sound protocol mapping.
use super::command::Command;
use super::profiles::Profile;
use crate::model::SoundState;
use windows::{
    core::PCWSTR,
    Win32::{
        Devices::{DeviceAndDriverInstallation::*, HumanInterfaceDevice::*},
        Foundation::*,
        Storage::FileSystem::*,
    },
};
type Result<T> = std::result::Result<T, String>;
const ANC_MODES: [&str; 3] = ["On", "Transparency", "Off"];
pub struct Device {
    pub profile: Profile,
    /// Matching wired USB identity present, even if firmware exposes controls
    /// only through the dongle while both connections are attached.
    pub usb_cable: bool,
    sidetone_max: Option<u8>,
    path: Vec<u16>,
    input_len: usize,
    output_len: usize,
    reader: HANDLE,
    stream: Vec<u8>,
    eq_slot: u8,
    eq_read_slot: Option<u8>,
}
impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.reader);
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Query {
    Mix,
    Side,
    Level,
    Anc,
    Noise,
    Mute,
    Battery,
    EqIndex,
    EqBands,
    EqWriteAck,
    EqSelectAck,
    MaxSidetone,
}
/// An incomplete frame holds at most 191 bytes; the next report adds at most 61.
/// Even at the six-byte minimum frame size, 64 slots cover that bounded input.
/// Keeps duplicate fields in wire order for protocol diagnostics without allocating.
#[derive(Debug)]
pub struct QueryUpdates {
    fields: [Query; 64],
    len: usize,
}
impl Default for QueryUpdates {
    fn default() -> Self {
        Self {
            fields: [Query::Mix; 64],
            len: 0,
        }
    }
}
impl QueryUpdates {
    fn push(&mut self, query: Query) {
        self.fields[self.len] = query;
        self.len += 1;
    }
    pub fn contains(&self, query: &Query) -> bool {
        self.fields[..self.len].contains(query)
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}
impl IntoIterator for QueryUpdates {
    type Item = Query;
    type IntoIter = std::iter::Take<std::array::IntoIter<Query, 64>>;
    fn into_iter(self) -> Self::IntoIter {
        self.fields.into_iter().take(self.len)
    }
}
fn replace_text(target: &mut Option<String>, value: &str) {
    if target.as_deref() != Some(value) {
        if let Some(text) = target {
            text.clear();
            text.push_str(value);
        } else {
            *target = Some(value.into());
        }
    }
}
pub const QUERIES: [Query; 7] = [
    Query::Mix,
    Query::Side,
    Query::Level,
    Query::Anc,
    Query::Noise,
    Query::Mute,
    Query::Battery,
];
impl Device {
    pub(super) fn needs_continuation(&self, report: &[u8], extra_reads: usize) -> bool {
        continuation_allowed(report, &self.stream, extra_reads)
    }

    pub fn open() -> Result<Self> {
        unsafe {
            let guid = HidD_GetHidGuid();
            let list = SetupDiGetClassDevsW(
                Some(&guid),
                None,
                None,
                DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
            )
            .map_err(|e| e.to_string())?;
            let result = (|| {
                let mut selected: Option<Self> = None;
                let mut wired_profiles = Vec::new();
                for index in 0..256 {
                    let mut iface = SP_DEVICE_INTERFACE_DATA {
                        cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                        ..Default::default()
                    };
                    if SetupDiEnumDeviceInterfaces(list, None, &guid, index, &mut iface).is_err() {
                        break;
                    }
                    let mut needed = 0;
                    let _ = SetupDiGetDeviceInterfaceDetailW(
                        list,
                        &iface,
                        None,
                        0,
                        Some(&mut needed),
                        None,
                    );
                    if !(8..=32768).contains(&needed) {
                        continue;
                    }
                    let mut storage = vec![0u64; (needed as usize).div_ceil(8)];
                    let detail = storage.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
                    (*detail).cbSize =
                        std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
                    if SetupDiGetDeviceInterfaceDetailW(
                        list,
                        &iface,
                        Some(detail),
                        needed,
                        None,
                        None,
                    )
                    .is_err()
                    {
                        continue;
                    }
                    let path = PCWSTR((*detail).DevicePath.as_ptr());
                    let path_text = path.to_string().unwrap_or_default().to_ascii_lowercase();
                    if !path_text.contains("vid_3329&pid_") {
                        continue;
                    }
                    if let Some(profile) = path_text
                        .split("vid_3329&pid_")
                        .nth(1)
                        .and_then(|s| s.get(..4))
                        .and_then(|s| u16::from_str_radix(s, 16).ok())
                        .and_then(|pid| Profile::from_usb(0x3329, pid))
                        .filter(|p| p.connection == crate::model::Connection::WiredUsb)
                    {
                        if !wired_profiles.contains(&profile) {
                            wired_profiles.push(profile);
                        }
                    }
                    let handle = match CreateFileW(
                        path,
                        GENERIC_READ.0 | GENERIC_WRITE.0,
                        FILE_SHARE_READ | FILE_SHARE_WRITE,
                        None,
                        OPEN_EXISTING,
                        FILE_ATTRIBUTE_NORMAL,
                        None,
                    ) {
                        Ok(handle) => handle,
                        // Other collections (for example consumer controls) may
                        // be exclusively held by Windows. Continue to the vendor
                        // interface; never let an inaccessible sibling hide it.
                        Err(_) => continue,
                    };
                    let mut attrs = HIDD_ATTRIBUTES {
                        Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32,
                        ..Default::default()
                    };
                    let mut preparsed = PHIDP_PREPARSED_DATA::default();
                    let mut caps = HIDP_CAPS::default();
                    let profile = if HidD_GetAttributes(handle, &mut attrs) {
                        Profile::from_usb(attrs.VendorID, attrs.ProductID)
                    } else {
                        None
                    };
                    let identity = profile.is_some();
                    let usage = if identity && HidD_GetPreparsedData(handle, &mut preparsed) {
                        let status = HidP_GetCaps(preparsed, &mut caps);
                        let _ = HidD_FreePreparsedData(preparsed);
                        status.0 >= 0 && caps.UsagePage == 0xff13 && caps.Usage == 1
                    } else {
                        false
                    };
                    if !usage
                        || caps.InputReportByteLength == 0
                        || caps.InputReportByteLength > 512
                        || caps.OutputReportByteLength < 62
                        || caps.OutputReportByteLength > 512
                    {
                        let _ = CloseHandle(handle);
                        continue;
                    }
                    let Some(profile) = profile else {
                        let _ = CloseHandle(handle);
                        continue;
                    };
                    let candidate = Self {
                        profile,
                        usb_cable: false,
                        sidetone_max: profile.verified().then_some(31),
                        path: path_text.encode_utf16().chain(Some(0)).collect(),
                        input_len: caps.InputReportByteLength as usize,
                        reader: handle,
                        output_len: caps.OutputReportByteLength as usize,
                        stream: Vec::with_capacity(256),
                        eq_slot: 6,
                        eq_read_slot: None,
                    };
                    if let Some(old) = &selected {
                        if old.profile.family != profile.family || old.profile.xbox != profile.xbox
                        {
                            return Err("Multiple headset models connected. Connect one headset to choose its controls.".into());
                        }
                        if old.profile.connection == profile.connection {
                            return Err("Multiple matching headset interfaces. Connect one headset at a time.".into());
                        }
                        if profile.connection == crate::model::Connection::WirelessDongle {
                            continue;
                        }
                    }
                    selected = Some(candidate);
                }
                let mut selected = selected.ok_or_else(|| {
                    "Connect a supported Maxwell headset or USB dongle.".to_owned()
                })?;
                selected.usb_cable = wired_profiles.iter().any(|p| {
                    p.family == selected.profile.family && p.xbox == selected.profile.xbox
                });
                Ok(selected)
            })();
            let _ = SetupDiDestroyDeviceInfoList(list);
            result
        }
    }
    pub fn same_endpoint(&self, other: &Self) -> bool {
        self.path == other.path && self.usb_cable == other.usb_cable
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
    pub fn listen(&self) -> Result<crate::hid_listener::InputListener> {
        crate::hid_listener::InputListener::open(&self.path, self.input_len)
    }
    fn packet(&self, body: &[u8]) -> Result<()> {
        let mut storage = [0u8; 512];
        let bytes = &mut storage[..self.output_len];
        bytes[0] = 6;
        bytes[1] = body.len() as u8;
        bytes[2] = self.profile.route();
        bytes[3..3 + body.len()].copy_from_slice(body);
        let mut written = 0;
        unsafe { WriteFile(self.reader, Some(bytes), Some(&mut written), None) }
            .map_err(|e| e.to_string())?;
        if written as usize != bytes.len() {
            return Err("Incomplete USB sound command".into());
        }
        Ok(())
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
        if !unsafe { HidD_GetInputReport(self.reader, bytes.as_mut_ptr().cast(), 64) } {
            return Err("USB connection lost. Reconnecting…".into());
        }
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
fn consume_report_bytes_with_slot(
    stream: &mut Vec<u8>,
    bytes: &[u8],
    state: &mut SoundState,
    mut observed: impl FnMut(&[u8], Option<Query>, &SoundState),
    eq_slot: Option<u8>,
) -> QueryUpdates {
    if bytes.len() < 3 || bytes[1] as usize > bytes.len() - 3 {
        return QueryUpdates::default();
    }
    let route = if state.transport == Some(crate::model::Connection::WiredUsb) {
        0
    } else {
        0x80
    };
    if bytes[0] != 7 || bytes[1] > 61 || bytes[2] != route {
        return QueryUpdates::default();
    }
    stream.extend_from_slice(&bytes[3..3 + bytes[1] as usize]);
    let mut updated = QueryUpdates::default();
    while stream.len() >= 4 {
        if stream[0] != 5 || !matches!(stream[1], 0x5b | 0x5d) {
            stream.remove(0);
            continue;
        }
        let len = u16::from_le_bytes([stream[2], stream[3]]) as usize + 4;
        if !(6..=192).contains(&len) {
            stream.remove(0);
            continue;
        }
        if stream.len() < len {
            break;
        }
        let q = if let Some(snapshot) =
            eq_slot.and_then(|slot| crate::eq::parse_bands(slot, &stream[..len]))
        {
            state.eq_bands = Some(snapshot);
            Some(Query::EqBands)
        } else {
            parse_frame(&stream[..len], state)
        };
        observed(&stream[..len], q, state);
        if let Some(q) = q {
            updated.push(q);
        }
        stream.drain(..len);
    }
    if stream.len() > 256 {
        stream.clear();
    }
    updated
}
fn continuation_allowed(report: &[u8], stream: &[u8], already_extra: usize) -> bool {
    // Only finish a response that has actually started. An empty mailbox must
    // never cause a tight read loop. The caller allows at most two extra reads.
    already_extra < 2
        && report.len() >= 3
        && report[1] != 0
        && stream.first() == Some(&5)
        && (stream.len() < 2 || matches!(stream[1], 0x5b | 0x5d))
        && (stream.len() < 4 || {
            let expected = u16::from_le_bytes([stream[2], stream[3]]) as usize + 4;
            (6..=192).contains(&expected) && stream.len() < expected
        })
}
pub fn parse_frame(b: &[u8], s: &mut SoundState) -> Option<Query> {
    if b.len() < 7
        || b[0] != 5
        || !matches!(b[1], 0x5b | 0x5d)
        || u16::from_le_bytes([b[2], b[3]]) as usize + 4 != b.len()
    {
        return None;
    }
    if (b[4], b[5]) == (0x30, 0x2e) && s.capabilities.is_some_and(|caps| !caps.anc) {
        return None;
    }
    let q = match (b[4], b[5]) {
        (0, 9)
            if b.len() == 9
                && b[6..] == [0, 0, 0]
                && s.capabilities.is_some_and(|caps| !caps.anc) =>
        {
            Query::EqSelectAck
        }
        (0x83, 0x2c) if b.len() == 10 && b[6] == 0 && b[8] == 0 => match b[7] {
            0x0b if b[9] <= 20 => {
                let chat = b[9] * 5;
                s.mix = Some((100 - chat, chat));
                Query::Mix
            }
            7 if b[9] <= 1 => {
                s.sidetone = Some(b[9] != 0);
                Query::Side
            }
            _ => return None,
        },
        (1, 9) if b.len() == 10 && b[7] == 0 && b[8] == 0 => match b[6] {
            0 if (1..=10).contains(&b[9]) && s.capabilities.is_some_and(|caps| !caps.anc) => {
                s.eq_index = Some(b[9] - 1);
                Query::EqIndex
            }
            0x2d if b[9] > 0 => {
                s.sidetone_max = Some(b[9]);
                Query::MaxSidetone
            }
            0x2c if s.sidetone_max.is_some_and(|max| b[9] <= max) => {
                s.sidetone_level = s
                    .sidetone_max
                    .map(|max| (b[9] as f32 * 100. / max as f32).round() as u8);
                Query::Level
            }
            0x24 if b[9] <= 2 => {
                replace_text(&mut s.noise, ["Off", "Low", "High"][b[9] as usize]);
                Query::Noise
            }
            0x22 if matches!(b[9], 0 | 255) => {
                s.muted = Some(b[9] == 0);
                Query::Mute
            }
            _ => return None,
        },
        (0x30, 0x2e) if b.len() == 9 && b[6] == 0x0c && b[7] == 0 && b[8] == 0 => {
            Query::EqSelectAck
        }
        (0x30, 0x2e) if b.len() == 9 && b[6] == 0x0b && b[7] == 0 && b[8] == 0 => Query::EqWriteAck,
        (0x30, 0x2e) if b.len() == 9 && b[6] == 0x0d && b[7] == 0 && b[8] < 10 => {
            s.eq_index = Some(b[8]);
            Query::EqIndex
        }
        (0x30, 0x2e) if b.len() == 9 && b[6] == 2 && b[7] == 0 && b[8] <= 2 => {
            replace_text(&mut s.anc, ANC_MODES[b[8] as usize]);
            Query::Anc
        }
        (0xd6, 0x0c) if b.len() == 9 && b[6] == 0 && b[7] == 0 && b[8] <= 100 => {
            s.battery = Some(b[8]);
            Query::Battery
        }
        _ => return None,
    };
    Some(q)
}

#[cfg(test)]
fn consume_report_bytes(
    stream: &mut Vec<u8>,
    bytes: &[u8],
    state: &mut SoundState,
    observed: impl FnMut(&[u8], Option<Query>, &SoundState),
) -> QueryUpdates {
    consume_report_bytes_with_slot(stream, bytes, state, observed, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wired_frames_reject_wireless_routing_and_use_the_reported_sidetone_range() {
        let mut state = SoundState {
            transport: Some(crate::model::Connection::WiredUsb),
            ..Default::default()
        };
        let mut stream = vec![];
        let mut report = vec![7, 10, 0x80, 5, 0x5b, 6, 0, 1, 9, 0x2d, 0, 0, 40];
        assert!(consume_report_bytes(&mut stream, &report, &mut state, |_, _, _| {}).is_empty());
        report[2] = 0;
        assert!(
            consume_report_bytes(&mut stream, &report, &mut state, |_, _, _| {})
                .contains(&Query::MaxSidetone)
        );
        assert_eq!(
            parse_frame(&[5, 0x5b, 6, 0, 1, 9, 0x2c, 0, 0, 20], &mut state),
            Some(Query::Level)
        );
        assert_eq!(state.sidetone_level, Some(50));
        assert_eq!(
            parse_frame(&[5, 0x5b, 6, 0, 1, 9, 0x2c, 0, 0, 41], &mut state),
            None
        );
    }
    #[test]
    fn legacy_eq_selection_is_one_based_and_never_parsed_as_anc() {
        let mut state = SoundState {
            capabilities: Some(crate::model::Capabilities {
                anc: false,
                eq_edit: false,
            }),
            ..Default::default()
        };
        for raw in 1..=10 {
            assert_eq!(
                parse_frame(&[5, 0x5b, 6, 0, 1, 9, 0, 0, 0, raw], &mut state),
                Some(Query::EqIndex)
            );
            assert_eq!(state.eq_index, Some(raw - 1));
        }
        assert_eq!(
            parse_frame(&[5, 0x5b, 5, 0, 0, 9, 0, 0, 0], &mut state),
            Some(Query::EqSelectAck)
        );
        state.capabilities.as_mut().unwrap().anc = true;
        assert_eq!(
            parse_frame(&[5, 0x5b, 6, 0, 1, 9, 0, 0, 0, 7], &mut state),
            None
        );
    }
    #[test]
    fn eq_read_frames_use_q_before_gain_and_survive_fragmentation() {
        let mut expected = crate::eq::sample(8);
        expected.bands[3].q = 1.41;
        expected.bands[3].gain = -4.;
        let mut wire = vec![5, 0x5b, 143, 0, 0x30, 0x2e, 0x32];
        for band in expected.bands {
            wire.extend_from_slice(&[band.id, band.kind]);
            wire.extend_from_slice(&band.frequency.to_le_bytes());
            wire.extend_from_slice(&band.q.to_le_bytes());
            wire.extend_from_slice(&band.gain.to_le_bytes());
        }
        for chunk_size in 1..=61 {
            let mut stream = vec![];
            let mut state = SoundState::default();
            for payload in wire.chunks(chunk_size) {
                let mut report = vec![7, payload.len() as u8, 0x80];
                report.extend_from_slice(payload);
                consume_report_bytes_with_slot(
                    &mut stream,
                    &report,
                    &mut state,
                    |_, _, _| {},
                    Some(8),
                );
            }
            assert_eq!(state.eq_bands, Some(expected));
            assert!(stream.is_empty());
        }
        let mut unsolicited = SoundState::default();
        let mut stream = vec![];
        for payload in wire.chunks(59) {
            let mut report = vec![7, payload.len() as u8, 0x80];
            report.extend_from_slice(payload);
            consume_report_bytes(&mut stream, &report, &mut unsolicited, |_, _, _| {});
        }
        assert_eq!(unsolicited.eq_bands, None);
        wire[21] = 0; // Duplicate band ID.
        assert!(crate::eq::parse_bands(8, &wire).is_none());
    }
    #[test]
    fn fragments_preserve_each_mix_value_at_every_split_boundary() {
        let frames = [
            5, 0x5b, 6, 0, 0x83, 0x2c, 0, 11, 0, 9, 5, 0x5b, 6, 0, 0x83, 0x2c, 0, 11, 0, 10,
        ];
        for split in 1..frames.len() {
            let mut stream = vec![];
            let mut state = SoundState::default();
            let mut observed = vec![];
            let mut updates = vec![];
            for piece in [&frames[..split], &frames[split..]] {
                let mut report = vec![7, piece.len() as u8, 0x80];
                report.extend_from_slice(piece);
                updates.extend(consume_report_bytes(
                    &mut stream,
                    &report,
                    &mut state,
                    |_, q, s| {
                        if q == Some(Query::Mix) {
                            observed.push(s.mix.unwrap().1);
                        }
                    },
                ));
            }
            assert_eq!(observed, vec![45, 50], "split at {split}");
            assert_eq!(updates, vec![Query::Mix, Query::Mix], "split at {split}");
            assert_eq!(state.mix, Some((50, 50)));
            assert!(stream.is_empty());
        }
    }
    #[test]
    fn packed_reports_keep_all_updates_across_report_boundaries() {
        let mut wire = vec![];
        for value in 0..=20 {
            wire.extend_from_slice(&[5, 0x5b, 6, 0, 0x83, 0x2c, 0, 11, 0, value]);
        }
        let mut stream = vec![];
        let mut state = SoundState::default();
        let mut values = vec![];
        let mut updates = vec![];
        for payload in wire.chunks(61) {
            let mut report = vec![7, payload.len() as u8, 0x80];
            report.extend_from_slice(payload);
            updates.extend(consume_report_bytes(
                &mut stream,
                &report,
                &mut state,
                |_, field, state| {
                    if field == Some(Query::Mix) {
                        values.push(state.mix.unwrap().1);
                    }
                },
            ));
        }
        assert_eq!(updates, vec![Query::Mix; 21]);
        assert_eq!(values, (0..=20).map(|v| v * 5).collect::<Vec<_>>());
        assert_eq!(state.mix, Some((0, 100)));
        assert!(stream.is_empty());
    }
    #[test]
    fn continuation_reads_require_partial_data_and_have_a_fixed_budget() {
        let partial = [5, 0x5b, 6];
        let nonempty = [7, 3, 0x80, 5, 0x5b, 6];
        assert!(continuation_allowed(&nonempty, &partial, 0));
        assert!(continuation_allowed(&nonempty, &partial, 1));
        assert!(!continuation_allowed(&nonempty, &partial, 2));
        assert!(!continuation_allowed(&[7, 0, 0x80], &partial, 0));
        assert!(!continuation_allowed(&nonempty, &[], 0));
        assert!(!continuation_allowed(&nonempty, &[0, 0, 0], 0));
        assert!(!continuation_allowed(
            &nonempty,
            &[5, 0x5b, 6, 0, 0x83, 0x2c, 0, 11, 0, 9],
            0
        ));
    }
    #[test]
    fn anc_wire_modes_match_headset() {
        for (raw, name) in [(0, "On"), (1, "Transparency"), (2, "Off")] {
            let mut state = SoundState::default();
            assert_eq!(
                parse_frame(&[5, 0x5b, 5, 0, 0x30, 0x2e, 2, 0, raw], &mut state),
                Some(Query::Anc)
            );
            assert_eq!(state.anc.as_deref(), Some(name));
            assert_eq!(
                ANC_MODES.iter().position(|s| *s == name),
                Some(raw as usize)
            );
        }
    }
    #[test]
    fn validates_frames_and_bounds() {
        let mut s = SoundState::default();
        assert_eq!(
            parse_frame(&[5, 0x5b, 6, 0, 0x83, 0x2c, 0, 11, 0, 12], &mut s),
            Some(Query::Mix)
        );
        assert_eq!(s.mix, Some((40, 60)));
        assert_eq!(
            parse_frame(&[5, 0x5b, 6, 0, 0x83, 0x2c, 0, 11, 0, 255], &mut s),
            None
        );
        assert_eq!(s.mix, Some((40, 60)));
        assert_eq!(parse_frame(&[5, 0x5b, 6, 0, 0x83], &mut s), None);
    }
}

//! Fixed-size headset EQ data. Native preset slots are 0..5, custom slots 6..9.
use serde::{Deserialize, Serialize};
pub const PRESET_NAMES: [&str; 10] = [
    "Audeze",
    "Treble Boost",
    "Bass Boost",
    "Immersive",
    "Competition",
    "Footsteps",
    "Custom 1",
    "Custom 2",
    "Custom 3",
    "Custom 4",
];
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub id: u8,
    pub kind: u8,
    pub frequency: f32,
    pub gain: f32,
    pub q: f32,
}
impl Band {
    pub fn valid(self) -> bool {
        self.id < 10
            && self.kind <= 8
            && self.frequency.is_finite()
            && (10.0..=24000.0).contains(&self.frequency)
            && self.gain.is_finite()
            && (-12.0..=12.0).contains(&self.gain)
            && self.q.is_finite()
            && (0.01..=100.0).contains(&self.q)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub slot: u8,
    pub bands: [Band; 10],
}
/// Writes use frequency, gain, Q; reads use frequency, Q, gain. All floats are LE.
pub fn gain_command(slot: u8, band: Band) -> Result<[u8; 22], String> {
    if !(6..=9).contains(&slot) || !band.valid() {
        return Err("Invalid custom EQ band".into());
    }
    let mut bytes = [0; 22];
    bytes[..10].copy_from_slice(&[5, 0x5a, 18, 0, 0x30, 0x2e, 0x0b, slot, band.id, band.kind]);
    bytes[10..14].copy_from_slice(&band.frequency.to_le_bytes());
    bytes[14..18].copy_from_slice(&band.gain.to_le_bytes());
    bytes[18..22].copy_from_slice(&band.q.to_le_bytes());
    Ok(bytes)
}
pub fn parse_bands(slot: u8, frame: &[u8]) -> Option<Snapshot> {
    if !(6..=9).contains(&slot)
        || frame.len() != 147
        || frame[..7] != [5, 0x5b, 143, 0, 0x30, 0x2e, 0x32]
    {
        return None;
    }
    let mut bands = std::array::from_fn(|i| {
        let b = &frame[7 + i * 14..7 + (i + 1) * 14];
        Band {
            id: b[0],
            kind: b[1],
            frequency: f32::from_le_bytes(b[2..6].try_into().unwrap()),
            gain: f32::from_le_bytes(b[10..14].try_into().unwrap()),
            q: f32::from_le_bytes(b[6..10].try_into().unwrap()),
        }
    });
    if bands.iter().any(|b| !b.valid()) {
        return None;
    }
    let mut ids = 0u16;
    for b in bands {
        if ids & (1 << b.id) != 0 {
            return None;
        }
        ids |= 1 << b.id;
    }
    bands.sort_by_key(|band| band.id);
    Some(Snapshot { slot, bands })
}

pub fn sample(slot: u8) -> Snapshot {
    let frequencies = [
        32., 64., 125., 250., 500., 1000., 2000., 4000., 8000., 16000.,
    ];
    Snapshot {
        slot,
        bands: std::array::from_fn(|i| Band {
            id: i as u8,
            kind: 6,
            frequency: frequencies[i],
            gain: 0.,
            q: 1.,
        }),
    }
}

/// Local presentation preferences; audio curves remain in the headset's four banks.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub names: [String; 4],
    pub pinned: [bool; 10],
    pub show_factory: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            names: std::array::from_fn(|i| PRESET_NAMES[i + 6].into()),
            pinned: [true; 10],
            show_factory: true,
        }
    }
}
impl Preferences {
    pub fn name(&self, slot: u8) -> &str {
        match slot {
            0..=5 => PRESET_NAMES[slot as usize],
            6..=9 => &self.names[slot as usize - 6],
            _ => "Unknown EQ",
        }
    }
    pub fn visible(&self, slot: u8) -> bool {
        slot < 10 && self.pinned[slot as usize] && (slot >= 6 || self.show_factory)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Draft {
    pub original: Snapshot,
    pub value: Snapshot,
    pub saving: bool,
}
impl Draft {
    pub fn dirty(&self) -> bool {
        self.original != self.value
    }
    pub fn set_gain(&mut self, index: usize, gain: f32) {
        if !self.saving && index < 10 && gain.is_finite() {
            let band = &mut self.value.bands[index];
            band.gain = gain.round().clamp(-12., 12.);
            // The native graphic editor uses filter type 6 for edited bands.
            // Untouched bands retain the exact filter type, Q and frequency read.
            band.kind = 6;
        }
    }
    pub fn receive(&mut self, snapshot: Snapshot) {
        if snapshot.slot != self.value.slot {
            return;
        }
        if self.saving {
            let confirmed =
                self.value
                    .bands
                    .iter()
                    .zip(self.original.bands)
                    .all(|(target, old)| {
                        *target == old || snapshot.bands.iter().any(|actual| actual == target)
                    });
            if confirmed {
                self.original = snapshot;
                self.value = snapshot;
                self.saving = false;
            }
        } else if !self.dirty() {
            self.original = snapshot;
            self.value = snapshot;
        }
    }
}
#[derive(Default)]
pub struct Editor {
    pub selected: u8,
    pub banks: [Option<Draft>; 4],
    pub name_edit: Option<String>,
    pub name_selected: bool,
}
impl Editor {
    pub fn draft(&self) -> Option<&Draft> {
        self.selected
            .checked_sub(6)
            .and_then(|i| self.banks.get(i as usize))
            .and_then(Option::as_ref)
    }
    pub fn draft_mut(&mut self) -> Option<&mut Draft> {
        self.selected
            .checked_sub(6)
            .and_then(|i| self.banks.get_mut(i as usize))
            .and_then(Option::as_mut)
    }
    pub fn receive(&mut self, snapshot: Snapshot) {
        if !(6..=9).contains(&snapshot.slot) {
            return;
        }
        let bank = &mut self.banks[snapshot.slot as usize - 6];
        if let Some(draft) = bank {
            draft.receive(snapshot);
        } else {
            *bank = Some(Draft {
                original: snapshot,
                value: snapshot,
                saving: false,
            });
        }
    }
    pub fn failed(&mut self) {
        for draft in self.banks.iter_mut().flatten() {
            draft.saving = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_commands_preserve_q_and_frequency_and_reject_factory_writes() {
        let mut band = sample(6).bands[4];
        band.q = 1.41;
        band.gain = -7.;
        let command = gain_command(6, band).unwrap();
        assert_eq!(&command[10..14], &500f32.to_le_bytes());
        assert_eq!(&command[14..18], &(-7f32).to_le_bytes());
        assert_eq!(&command[18..22], &1.41f32.to_le_bytes());
        assert!(gain_command(0, band).is_err());
        band.gain = f32::NAN;
        assert!(gain_command(6, band).is_err());
    }
    #[test]
    fn partial_readbacks_do_not_replace_unsaved_or_saving_drafts() {
        let mut editor = Editor {
            selected: 6,
            ..Default::default()
        };
        let original = sample(6);
        editor.receive(original);
        editor.draft_mut().unwrap().set_gain(2, 5.);
        editor.receive(original);
        assert_eq!(editor.draft().unwrap().value.bands[2].gain, 5.);
        let draft = editor.draft_mut().unwrap();
        draft.saving = true;
        let target = draft.value;
        editor.receive(original);
        assert!(editor.draft().unwrap().saving);
        editor.receive(target);
        assert!(!editor.draft().unwrap().dirty());
        assert!(!editor.draft().unwrap().saving);
    }
}

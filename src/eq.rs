//! Fixed-size headset EQ data. Native preset slots are 0..5, custom slots 6..9.
use serde::{Deserialize, Serialize};
mod native_names;
pub mod profiles;
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
    /// Ignore earlier readbacks while committing or restoring a live preview.
    pub confirmation_revision: u64,
    pub previewing: bool,
    /// Includes bands returned to baseline and failed writes, so rollback can retry.
    pub touched: u16,
}
impl Draft {
    pub fn dirty(&self) -> bool {
        self.original != self.value
    }
    pub fn set_gain(&mut self, index: usize, gain: f32) {
        if !self.saving && index < 10 && gain.is_finite() {
            let before = self.value.bands[index];
            let band = &mut self.value.bands[index];
            band.gain = gain.round().clamp(-12., 12.);
            // The native graphic editor uses filter type 6 for edited bands.
            // Untouched bands retain the exact filter type, Q and frequency read.
            band.kind = 6;
            if *band != before {
                self.touched |= 1 << index;
            }
        }
    }
    pub fn receive(&mut self, snapshot: Snapshot, revision: u64) {
        if snapshot.slot != self.value.slot {
            return;
        }
        if self.saving {
            if revision < self.confirmation_revision {
                return;
            }
            // The ordered final read completed. A mismatch keeps the baseline
            // and touched bands for retry, but must not leave controls disabled.
            self.saving = false;
            let confirmed = self.value == snapshot;
            if confirmed {
                self.original = snapshot;
                self.value = snapshot;
                self.saving = false;
                self.previewing = false;
                self.touched = 0;
            }
        } else if !self.previewing && !self.dirty() {
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
    pub preview_slot: Option<u8>,
    /// Local library association for each custom bank; never a device slot ID.
    pub library_entries: [Option<usize>; 4],
    pub preview_names: [Option<String>; 4],
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
    pub fn receive(&mut self, snapshot: Snapshot, revision: u64) {
        if !(6..=9).contains(&snapshot.slot) {
            return;
        }
        let bank = &mut self.banks[snapshot.slot as usize - 6];
        if let Some(draft) = bank {
            draft.receive(snapshot, revision);
        } else {
            *bank = Some(Draft {
                original: snapshot,
                value: snapshot,
                saving: false,
                confirmation_revision: 0,
                previewing: false,
                touched: 0,
            });
        }
    }
    pub fn failed(&mut self) {
        self.preview_slot = None;
        for draft in self.banks.iter_mut().flatten() {
            draft.saving = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_restore_preserves_touched_bands_for_retry() {
        let mut editor = Editor {
            selected: 6,
            ..Default::default()
        };
        let baseline = sample(6);
        editor.receive(baseline, 1);
        let d = editor.draft_mut().unwrap();
        d.previewing = true;
        d.set_gain(0, 3.);
        let changed = d.value;
        d.value = baseline;
        d.saving = true;
        d.confirmation_revision = 3;
        editor.receive(changed, 3);
        let d = editor.draft().unwrap();
        assert!(!d.saving);
        assert_eq!(d.value, baseline);
        assert_eq!(d.touched, 1);
        assert!(d.previewing);
        editor.failed();
        assert_eq!(editor.draft().unwrap().touched, 1);
    }
    #[test]
    fn live_preview_rejects_stale_echo_even_after_returning_to_baseline() {
        let mut editor = Editor {
            selected: 6,
            ..Default::default()
        };
        let baseline = sample(6);
        editor.receive(baseline, 1);
        let d = editor.draft_mut().unwrap();
        d.previewing = true;
        d.set_gain(0, 4.);
        let stale = d.value;
        d.set_gain(0, 0.);
        editor.receive(stale, 2);
        assert_eq!(editor.draft().unwrap().value, baseline);
        let d = editor.draft_mut().unwrap();
        d.saving = true;
        d.confirmation_revision = 5;
        editor.receive(baseline, 4);
        assert!(editor.draft().unwrap().saving);
        editor.receive(baseline, 5);
        assert!(!editor.draft().unwrap().saving);
        assert!(!editor.draft().unwrap().previewing);
    }
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
        editor.receive(original, 0);
        editor.draft_mut().unwrap().set_gain(2, 5.);
        editor.receive(original, 0);
        assert_eq!(editor.draft().unwrap().value.bands[2].gain, 5.);
        let draft = editor.draft_mut().unwrap();
        draft.saving = true;
        draft.confirmation_revision = 1;
        let target = draft.value;
        editor.receive(original, 0);
        assert!(editor.draft().unwrap().saving);
        editor.receive(target, 1);
        assert!(!editor.draft().unwrap().dirty());
        assert!(!editor.draft().unwrap().saving);
    }
}

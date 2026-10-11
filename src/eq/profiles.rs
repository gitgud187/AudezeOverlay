//! Portable curves and the local preset library, independent of headset slots.
use super::{Band, Snapshot};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

const MAX_FILE_BYTES: u64 = 32 * 1024;
pub const MAX_PROFILES: usize = 128;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub version: u32,
    pub name: String,
    pub bands: [Band; 10],
}
impl Profile {
    pub fn new(name: &str, snapshot: Snapshot) -> Result<Self, String> {
        let profile = Self {
            version: 1,
            name: name.trim().to_owned(),
            bands: snapshot.bands,
        };
        profile.validate()?;
        Ok(profile)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Unsupported EQ profile version.".into());
        }
        if self.name.trim().is_empty()
            || self.name.chars().count() > 64
            || self.name.chars().any(char::is_control)
        {
            return Err("Profile names must contain 1–64 printable characters.".into());
        }
        let mut ids = 0u16;
        for band in self.bands {
            if !band.valid() || ids & (1 << band.id) != 0 {
                return Err("The profile contains invalid or duplicate EQ bands.".into());
            }
            ids |= 1 << band.id;
        }
        Ok(())
    }
    pub fn snapshot(&self, slot: u8) -> Result<Snapshot, String> {
        self.validate()?;
        if !(6..=9).contains(&slot) {
            return Err("Choose a custom headset slot first.".into());
        }
        let mut bands = self.bands;
        bands.sort_by_key(|band| band.id);
        Ok(Snapshot { slot, bands })
    }
}

/// Official .pnreq exports contain ten signed one-byte graphic gains, no name,
/// frequency or Q. Retain the destination bank's other parameters on import.
pub fn decode(
    bytes: &[u8],
    name: &str,
    legacy: bool,
    baseline: Option<Snapshot>,
) -> Result<Profile, String> {
    if bytes.len() > MAX_FILE_BYTES as usize {
        return Err("EQ file is too large.".into());
    }
    let profile = if legacy {
        if bytes.len() != 10 {
            return Err("A Maxwell .pnreq file must contain exactly ten gains.".into());
        }
        let mut snapshot =
            baseline.ok_or("Load a custom headset slot before importing a .pnreq file.")?;
        for (band, value) in snapshot.bands.iter_mut().zip(bytes) {
            let gain = *value as i8;
            if !(-12..=12).contains(&gain) {
                return Err("EQ gain is outside −12 to +12 dB.".into());
            }
            band.gain = gain as f32;
            band.kind = 6;
        }
        Profile::new(name, snapshot)?
    } else {
        serde_json::from_slice::<Profile>(bytes).map_err(|e| format!("Invalid EQ profile: {e}"))?
    };
    profile.validate()?;
    Ok(profile)
}
pub fn read(path: &Path, baseline: Option<Snapshot>) -> Result<Profile, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| format!("Could not open EQ file: {e}"))?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Could not read EQ file: {e}"))?;
    let legacy = path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("pnreq"));
    decode(
        &bytes,
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported EQ"),
        legacy,
        baseline,
    )
}
pub fn write(path: &Path, profile: &Profile) -> Result<(), String> {
    profile.validate()?;
    let bytes = serde_json::to_vec_pretty(profile).map_err(|e| e.to_string())?;
    // Do not truncate an existing export until the replacement is fully written.
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| format!("Could not create EQ export: {e}"))?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("Could not export EQ profile: {e}"))
}
/// Copies always receive a distinct name; existing library entries never change.
pub fn add(library: &mut Vec<Profile>, mut profile: Profile) -> Result<usize, String> {
    profile.validate()?;
    if library.len() >= MAX_PROFILES {
        return Err("The local library is full (128 profiles).".into());
    }
    let base = profile.name.chars().take(52).collect::<String>();
    let mut suffix = 2;
    while library
        .iter()
        .any(|p| p.name.eq_ignore_ascii_case(&profile.name))
    {
        profile.name = format!("{base} ({suffix})");
        suffix += 1;
    }
    library.push(profile);
    Ok(library.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_export_replaces_atomically_and_rejects_bad_imports() {
        let dir = std::env::temp_dir().join(format!("audeze-eq-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Portable EQ.json");
        let mut profile = Profile::new("Portable", super::super::sample(6)).unwrap();
        write(&path, &profile).unwrap();
        profile.bands[0].gain = -3.;
        write(&path, &profile).unwrap();
        assert_eq!(read(&path, None).unwrap(), profile);
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&tmp, b"belongs to another operation").unwrap();
        assert!(write(&path, &profile).is_err());
        assert_eq!(
            std::fs::read(&tmp).unwrap(),
            b"belongs to another operation"
        );
        assert_eq!(read(&path, None).unwrap(), profile);
        assert!(decode(&vec![0; MAX_FILE_BYTES as usize + 1], "Bad", false, None).is_err());
        std::fs::remove_file(tmp).unwrap();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn portable_roundtrip_preserves_parameters_and_new_copy_preserves_original() {
        let mut snapshot = super::super::sample(6);
        snapshot.bands[0].q = 1.41;
        let profile = Profile::new("My EQ", snapshot).unwrap();
        let bytes = serde_json::to_vec(&profile).unwrap();
        assert_eq!(decode(&bytes, "ignored", false, None).unwrap(), profile);
        let mut library = vec![profile.clone()];
        let mut copy = profile.clone();
        copy.bands[0].gain = 4.;
        assert_eq!(add(&mut library, copy).unwrap(), 1);
        assert_eq!(library[0], profile);
        assert_eq!(library[1].name, "My EQ (2)");
        assert_eq!(library[1].snapshot(9).unwrap().slot, 9);
        assert!(profile.snapshot(0).is_err());
    }
    #[test]
    fn legacy_gains_are_signed_bounded_and_require_a_real_baseline() {
        let original = super::super::sample(6);
        let bytes = [0xf4, 12, 0, 1, 2, 3, 4, 5, 6, 7];
        let imported = decode(&bytes, "Old EQ", true, Some(original)).unwrap();
        assert_eq!(imported.bands[0].gain, -12.);
        assert_eq!(imported.bands[0].frequency, original.bands[0].frequency);
        assert!(decode(&bytes, "Old EQ", true, None).is_err());
        assert!(decode(&[13; 10], "Bad", true, Some(original)).is_err());
        let mut invalid = imported;
        invalid.bands[1].id = 0;
        assert!(invalid.validate().is_err());
        invalid.version = 2;
        assert!(invalid.validate().is_err());
    }
}

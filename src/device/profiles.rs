//! Device identity and profile matching for supported Audeze headset variants.
//!
//! Matches USB devices by VID/PID and HID usage page/usage to a known profile.
//! Unknown or ambiguous devices are rejected rather than falling back to an
//! assumed protocol.

/// USB vendor ID for Audeze.
pub const AUDEZE_VID: u16 = 0x3329;

/// HID usage page used by Maxwell-series dongles.
pub const MAXWELL_USAGE_PAGE: u16 = 0xFF13;

/// HID usage for the sound-control interface (col02).
pub const MAXWELL_USAGE: u16 = 1;

/// Device identity from USB enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
}

/// A known device profile with its supported capabilities.
#[derive(Debug, Clone)]
pub struct DeviceProfile {
    pub identity: DeviceIdentity,
    pub display_name: &'static str,
    pub validated: bool,
    pub has_anc: bool,
    pub has_sidetone: bool,
    pub has_noise_control: bool,
    pub sidetone_max: u8,
}

impl DeviceProfile {
    pub fn capabilities(&self) -> crate::model::SoundCapabilities {
        crate::model::SoundCapabilities {
            anc: self.has_anc,
            sidetone_max: self.has_sidetone.then_some(self.sidetone_max),
            noise: self.has_noise_control,
        }
    }
    /// Returns profiles for all supported Maxwell variants.
    pub fn all_maxwell_profiles() -> &'static [DeviceProfile] {
        static PROFILES: &[DeviceProfile] = &[
            // Maxwell 2 ANC — fully validated through testing.
            DeviceProfile {
                identity: DeviceIdentity {
                    vendor_id: AUDEZE_VID,
                    product_id: 0x4BA9,
                    usage_page: MAXWELL_USAGE_PAGE,
                    usage: MAXWELL_USAGE,
                },
                display_name: "Maxwell 2 ANC",
                validated: true,
                has_anc: true,
                has_sidetone: true,
                has_noise_control: true,
                sidetone_max: 31,
            },
            // Maxwell 2 — assumed compatible; validated=false until tested.
            DeviceProfile {
                identity: DeviceIdentity {
                    vendor_id: AUDEZE_VID,
                    product_id: 0x4BA8,
                    usage_page: MAXWELL_USAGE_PAGE,
                    usage: MAXWELL_USAGE,
                },
                display_name: "Maxwell 2",
                validated: false,
                has_anc: false,
                has_sidetone: true,
                has_noise_control: true,
                sidetone_max: 31,
            },
            // Maxwell 2 variant (0x4BAA) — assumed compatible.
            DeviceProfile {
                identity: DeviceIdentity {
                    vendor_id: AUDEZE_VID,
                    product_id: 0x4BAA,
                    usage_page: MAXWELL_USAGE_PAGE,
                    usage: MAXWELL_USAGE,
                },
                display_name: "Maxwell 2",
                validated: false,
                has_anc: false,
                has_sidetone: true,
                has_noise_control: true,
                sidetone_max: 31,
            },
            // Maxwell (original) — assumed compatible.
            DeviceProfile {
                identity: DeviceIdentity {
                    vendor_id: AUDEZE_VID,
                    product_id: 0x4BA7,
                    usage_page: MAXWELL_USAGE_PAGE,
                    usage: MAXWELL_USAGE,
                },
                display_name: "Maxwell",
                validated: false,
                has_anc: false,
                has_sidetone: true,
                has_noise_control: true,
                sidetone_max: 31,
            },
        ];
        PROFILES
    }

    /// Provisional mappings are enabled only by the experimental build feature.
    /// Candidate PIDs came from official-app research; their model assignments,
    /// sound protocol and ranges remain guesses, not hardware validation.
    pub fn match_identity(id: DeviceIdentity) -> Option<&'static DeviceProfile> {
        Self::all_maxwell_profiles()
            .iter()
            .find(|p| p.identity == id && (p.validated || cfg!(feature = "experimental-devices")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_validated_maxwell_2_anc() {
        let id = DeviceIdentity {
            vendor_id: AUDEZE_VID,
            product_id: 0x4BA9,
            usage_page: MAXWELL_USAGE_PAGE,
            usage: MAXWELL_USAGE,
        };
        let profile = DeviceProfile::match_identity(id).expect("Maxwell 2 ANC should match");
        assert!(profile.validated);
        assert!(profile.has_anc);
        assert_eq!(profile.display_name, "Maxwell 2 ANC");
    }

    #[test]
    fn matches_unvalidated_maxwell_variants() {
        for pid in [0x4BA7, 0x4BA8, 0x4BAA] {
            let id = DeviceIdentity {
                vendor_id: AUDEZE_VID,
                product_id: pid,
                usage_page: MAXWELL_USAGE_PAGE,
                usage: MAXWELL_USAGE,
            };
            let profile = DeviceProfile::match_identity(id);
            assert_eq!(profile.is_some(), cfg!(feature = "experimental-devices"));
            if let Some(profile) = profile {
                assert!(!profile.validated);
                assert!(!profile.has_anc);
            }
        }
    }

    #[test]
    fn rejects_unknown_product_id() {
        let id = DeviceIdentity {
            vendor_id: AUDEZE_VID,
            product_id: 0xFFFF,
            usage_page: MAXWELL_USAGE_PAGE,
            usage: MAXWELL_USAGE,
        };
        assert!(DeviceProfile::match_identity(id).is_none());
    }

    #[test]
    fn rejects_wrong_usage_page() {
        let id = DeviceIdentity {
            vendor_id: AUDEZE_VID,
            product_id: 0x4BA9,
            usage_page: 0x0001,
            usage: 6,
        };
        assert!(DeviceProfile::match_identity(id).is_none());
    }

    #[test]
    fn capabilities_gate_commands_without_disabling_mix_or_eq() {
        use crate::device::command::Command;
        let mut profile = DeviceProfile::all_maxwell_profiles()[0].clone();
        profile.has_anc = false;
        profile.has_sidetone = false;
        profile.has_noise_control = false;
        for command in [
            Command::Anc("On".into()),
            Command::Sidetone(true),
            Command::SidetoneLevel(50),
            Command::Noise("High".into()),
        ] {
            assert!(!command.supported_by(profile.capabilities()));
        }
        assert!(Command::Mix(50).supported_by(profile.capabilities()));
        assert!(Command::EqSelect(6).supported_by(profile.capabilities()));
    }

    #[test]
    fn rejects_wrong_vendor_and_full_width_usage() {
        let id = DeviceProfile::all_maxwell_profiles()[0].identity;
        assert!(DeviceProfile::match_identity(DeviceIdentity { vendor_id: 0, ..id }).is_none());
        assert!(DeviceProfile::match_identity(DeviceIdentity { usage: 0x101, ..id }).is_none());
    }
}

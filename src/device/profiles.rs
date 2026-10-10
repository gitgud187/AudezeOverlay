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
pub const MAXWELL_USAGE: u8 = 1;

/// Device identity from USB enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u8,
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
    /// Returns profiles for all supported Maxwell variants.
    pub fn all_maxwell_profiles() -> &'static [DeviceProfile] {
        static PROFILES: &[DeviceProfile] = &[
            // Maxwell 2 ANC — fully validated through testing.
            Self {
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
            Self {
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
            Self {
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
            Self {
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

    /// Match a discovered device identity against all known profiles.
    pub fn match_identity(id: DeviceIdentity) -> Option<&'static DeviceProfile> {
        Self::all_maxwell_profiles()
            .iter()
            .find(|p| p.identity == id)
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
            let profile = DeviceProfile::match_identity(id)
                .unwrap_or_else(|| panic!("PID {:#x} should match", pid));
            assert!(!profile.validated);
            assert!(!profile.has_anc);
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
}

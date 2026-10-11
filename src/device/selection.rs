//! Which connected interface the application controls. Pure: platform code
//! enumerates what it finds, this module decides, so the rules are testable
//! without a headset and behave the same on every platform.
use super::profiles::Profile;
use crate::model::Connection;

/// A supported control interface found by the platform, with whatever the
/// platform needs to talk to it.
pub(super) struct Candidate<T> {
    pub profile: Profile,
    pub endpoint: T,
}
pub(super) struct Selected<T> {
    pub profile: Profile,
    pub endpoint: T,
    /// A matching wired USB identity is present, even when the firmware keeps
    /// control traffic on the dongle while the cable is attached.
    pub usb_cable: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SelectionError {
    NoDevice,
    MultipleModels,
    MultipleInterfaces,
}
impl std::fmt::Display for SelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoDevice => "Connect a supported Maxwell headset or USB dongle.",
            Self::MultipleModels => {
                "Multiple headset models connected. Connect one headset to choose its controls."
            }
            Self::MultipleInterfaces => {
                "Multiple matching headset interfaces. Connect one headset at a time."
            }
        })
    }
}

/// Picks the single interface to control. `wired_seen` lists every wired
/// identity the platform noticed, including ones whose control interface it
/// could not open.
///
/// A different model, or a second interface of the same kind, is ambiguous and
/// rejected rather than guessed. A wired interface wins over the same model's
/// dongle; that is a control-transport choice, not a statement about which
/// audio device Windows or Linux is playing through.
pub(super) fn choose<T>(
    candidates: Vec<Candidate<T>>,
    wired_seen: &[Profile],
) -> Result<Selected<T>, SelectionError> {
    let mut selected: Option<Candidate<T>> = None;
    for candidate in candidates {
        if let Some(old) = &selected {
            if old.profile.family != candidate.profile.family
                || old.profile.xbox != candidate.profile.xbox
            {
                return Err(SelectionError::MultipleModels);
            }
            if old.profile.connection == candidate.profile.connection {
                return Err(SelectionError::MultipleInterfaces);
            }
            if candidate.profile.connection == Connection::WirelessDongle {
                continue;
            }
        }
        selected = Some(candidate);
    }
    let selected = selected.ok_or(SelectionError::NoDevice)?;
    let usb_cable = wired_seen
        .iter()
        .any(|p| p.family == selected.profile.family && p.xbox == selected.profile.xbox);
    Ok(Selected {
        profile: selected.profile,
        endpoint: selected.endpoint,
        usb_cable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(product: u16) -> Profile {
        Profile::from_usb(0x3329, product).expect("known identity")
    }
    fn candidate(product: u16, name: &'static str) -> Candidate<&'static str> {
        Candidate {
            profile: profile(product),
            endpoint: name,
        }
    }

    #[test]
    fn a_single_dongle_is_selected_without_a_cable() {
        let chosen = choose(vec![candidate(0x4ba9, "dongle")], &[]).unwrap();
        assert_eq!(chosen.endpoint, "dongle");
        assert!(!chosen.usb_cable);
    }

    #[test]
    fn the_cable_wins_over_the_same_models_dongle_in_either_order() {
        for order in [[0x4ba9, 0x4baa], [0x4baa, 0x4ba9]] {
            let items = order
                .iter()
                .map(|p| candidate(*p, if *p == 0x4baa { "cable" } else { "dongle" }))
                .collect();
            let chosen = choose(items, &[profile(0x4baa)]).unwrap();
            assert_eq!(chosen.endpoint, "cable");
            assert!(chosen.usb_cable);
        }
    }

    #[test]
    fn an_unopenable_cable_still_marks_the_cable_as_present() {
        // The firmware can leave control traffic on the dongle while a cable is
        // attached, so the wired identity is noticed but never becomes a candidate.
        let chosen = choose(vec![candidate(0x4ba9, "dongle")], &[profile(0x4baa)]).unwrap();
        assert_eq!(chosen.endpoint, "dongle");
        assert!(chosen.usb_cable);
    }

    #[test]
    fn a_cable_for_another_model_is_not_this_headsets_cable() {
        let chosen = choose(vec![candidate(0x4ba9, "dongle")], &[profile(0x4b2a)]).unwrap();
        assert!(!chosen.usb_cable);
    }

    #[test]
    fn different_models_or_platform_variants_are_ambiguous() {
        // Maxwell 2 ANC next to Maxwell 2, and Xbox next to PlayStation.
        for other in [0x4b29, 0x4ba8] {
            let items = vec![candidate(0x4ba9, "a"), candidate(other, "b")];
            assert_eq!(
                choose(items, &[]).err(),
                Some(SelectionError::MultipleModels)
            );
        }
    }

    #[test]
    fn two_interfaces_of_the_same_kind_are_ambiguous() {
        let items = vec![candidate(0x4ba9, "a"), candidate(0x4ba9, "b")];
        assert_eq!(
            choose(items, &[]).err(),
            Some(SelectionError::MultipleInterfaces)
        );
        let cables = vec![candidate(0x4baa, "a"), candidate(0x4baa, "b")];
        assert_eq!(
            choose(cables, &[]).err(),
            Some(SelectionError::MultipleInterfaces)
        );
    }

    #[test]
    fn nothing_connected_asks_for_a_supported_device() {
        let error = choose::<()>(vec![], &[]).err().unwrap();
        assert_eq!(error, SelectionError::NoDevice);
        assert_eq!(
            error.to_string(),
            "Connect a supported Maxwell headset or USB dongle."
        );
    }
}

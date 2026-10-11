use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeadsetFamily {
    Maxwell,
    Maxwell2,
    Maxwell2Anc,
}
impl HeadsetFamily {
    pub fn name(self) -> &'static str {
        match self {
            Self::Maxwell => "Maxwell",
            Self::Maxwell2 => "Maxwell 2",
            Self::Maxwell2Anc => "Maxwell 2 ANC",
        }
    }
    pub fn settings_section(self) -> &'static str {
        match self {
            Self::Maxwell => "maxwell",
            Self::Maxwell2 => "maxwell_2",
            Self::Maxwell2Anc => "maxwell_2_anc",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Connection {
    WirelessDongle,
    WiredUsb,
    Bluetooth,
}
impl Connection {
    pub fn label(self) -> &'static str {
        match self {
            Self::WirelessDongle => "Wireless dongle",
            Self::WiredUsb => "Wired USB",
            Self::Bluetooth => "Bluetooth (Windows)",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub anc: bool,
    pub eq_edit: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SoundState {
    #[serde(default)]
    pub family: Option<HeadsetFamily>,
    #[serde(default)]
    pub transport: Option<Connection>,
    #[serde(default)]
    pub capabilities: Option<Capabilities>,
    #[serde(default)]
    pub experimental_device: bool,
    #[serde(default)]
    pub xbox: bool,
    #[serde(default)]
    pub sidetone_max: Option<u8>,
    /// Unknown until a validated charging-status source exists. USB presence
    /// alone proves power availability, not charging current or battery health.
    #[serde(default)]
    pub charging: Option<bool>,
    /// Matching data-capable cable present on this PC; not measured charging.
    #[serde(default)]
    pub usb_cable: bool,
    pub mix: Option<(u8, u8)>,
    /// Last settled local mix command; not a headset setting or preset field.
    #[serde(skip)]
    pub mix_revision: u64,
    pub sidetone: Option<bool>,
    pub sidetone_level: Option<u8>,
    pub anc: Option<String>,
    pub noise: Option<String>,
    pub battery: Option<u8>,
    pub muted: Option<bool>,
    pub device: Option<String>,
    pub connection: Option<String>,
    pub status: String,
    #[serde(default)]
    pub eq_index: Option<u8>,
    #[serde(default)]
    pub eq_bands: Option<crate::eq::Snapshot>,
    /// Forces delivery of an explicitly requested bank, even if values are unchanged.
    #[serde(skip)]
    pub eq_revision: u64,
    /// Selection acknowledgements must not confirm unrelated band drafts.
    #[serde(skip)]
    pub eq_index_revision: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct MixTarget {
    pub revision: u64,
    pub chat: u8,
}
pub fn reconcile_mix(target: &mut Option<MixTarget>, incoming: &mut SoundState) {
    if let Some(pending) = *target {
        if incoming.mix.is_none() || incoming.mix_revision >= pending.revision {
            *target = None;
        } else {
            incoming.mix = Some((100 - pending.chat, pending.chat));
        }
    }
}
impl SoundState {
    /// USB charging can bias the firmware's estimate; do not invent a correction.
    pub fn battery_label(&self) -> String {
        self.battery
            .map(|value| {
                if self.usb_cable {
                    format!("~{value}%")
                } else {
                    format!("{value}%")
                }
            })
            .unwrap_or_else(|| "—".into())
    }
    /// Bit positions match the five status cards; first readings and unavailable
    /// values do not count as headset adjustments.
    pub fn changed_cards(&self, old: &Self, enabled: [bool; 5]) -> u8 {
        fn changed<T: PartialEq>(a: &Option<T>, b: &Option<T>) -> bool {
            a.is_some() && b.is_some() && a != b
        }
        let changes = [
            changed(&self.mix, &old.mix),
            changed(&self.sidetone, &old.sidetone)
                || changed(&self.sidetone_level, &old.sidetone_level),
            changed(&self.anc, &old.anc),
            changed(&self.noise, &old.noise),
            changed(&self.muted, &old.muted),
        ];
        changes
            .into_iter()
            .zip(enabled)
            .enumerate()
            .fold(0, |mask, (i, (changed, enabled))| {
                mask | if changed && enabled { 1 << i } else { 0 }
            })
    }
    pub fn sound_changed(&self, old: &Self) -> bool {
        fn changed<T: PartialEq>(a: &Option<T>, b: &Option<T>) -> bool {
            a.is_some() && b.is_some() && a != b
        }
        changed(&self.mix, &old.mix)
            || changed(&self.sidetone, &old.sidetone)
            || changed(&self.sidetone_level, &old.sidetone_level)
            || changed(&self.anc, &old.anc)
            || changed(&self.noise, &old.noise)
            || changed(&self.muted, &old.muted)
    }
}
pub fn parse_mix(name: &str) -> Option<(u8, u8)> {
    let value = name.strip_prefix("Game/Chat Mix ")?;
    let (a, b) = value.split_once('/')?;
    let a: u8 = a.trim().parse().ok()?;
    let b: u8 = b.trim().parse().ok()?;
    (a <= 100 && b <= 100 && u16::from(a) + u16::from(b) == 100).then_some((a, b))
}
pub fn parse_percent(s: &str) -> Option<u8> {
    let value = s
        .split(|c: char| !c.is_ascii_digit())
        .find(|s| !s.is_empty())?
        .parse::<u8>()
        .ok()?;
    (value <= 100).then_some(value)
}
/// Clockwise 270-degree dial; the bottom gap snaps to the nearest endpoint.
pub fn mix_from_point(dx: f32, dy: f32) -> u8 {
    let mut angle = dy.atan2(dx).to_degrees().rem_euclid(360.);
    if (45.0..135.0).contains(&angle) {
        return if angle < 90. { 100 } else { 0 };
    }
    if angle < 135. {
        angle += 360.;
    }
    (((angle - 135.) / 270. * 20.).round().clamp(0., 20.) as u8) * 5
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn popup_mask_includes_only_changed_enabled_available_fields() {
        let old = SoundState {
            mix: Some((50, 50)),
            battery: Some(60),
            muted: Some(false),
            ..Default::default()
        };
        let mut next = old.clone();
        next.mix = Some((45, 55));
        next.muted = Some(true);
        next.anc = Some("On".into());
        next.battery = Some(59);
        assert_eq!(next.changed_cards(&old, [true; 5]), 1 | 16);
        assert_eq!(
            next.changed_cards(&old, [false, true, true, true, true]),
            16
        );
        next.mix = None;
        next.muted = None;
        assert_eq!(next.changed_cards(&old, [true; 5]), 0);
    }
    #[test]
    fn newer_wheel_target_survives_old_readbacks() {
        let mut target = Some(MixTarget {
            revision: 2,
            chat: 80,
        });
        let mut old = SoundState {
            mix: Some((70, 30)),
            mix_revision: 1,
            muted: Some(true),
            ..Default::default()
        };
        reconcile_mix(&mut target, &mut old);
        assert_eq!(old.mix, Some((20, 80)));
        assert_eq!(old.muted, Some(true));
        assert!(target.is_some());
        let mut confirmed = SoundState {
            mix: Some((20, 80)),
            mix_revision: 2,
            ..Default::default()
        };
        reconcile_mix(&mut target, &mut confirmed);
        assert!(target.is_none());
        confirmed.mix = Some((25, 75));
        reconcile_mix(&mut target, &mut confirmed);
        assert_eq!(confirmed.mix, Some((25, 75)));
    }
    #[test]
    fn failed_or_disconnected_headset_releases_pending_display_value() {
        let mut target = Some(MixTarget {
            revision: 3,
            chat: 80,
        });
        let mut failed = SoundState {
            mix: Some((50, 50)),
            mix_revision: 3,
            ..Default::default()
        };
        reconcile_mix(&mut target, &mut failed);
        assert!(target.is_none());
        assert_eq!(failed.mix, Some((50, 50)));
        target = Some(MixTarget {
            revision: 4,
            chat: 90,
        });
        reconcile_mix(&mut target, &mut SoundState::default());
        assert!(target.is_none());
    }
    #[test]
    fn dial_direction_and_gap() {
        assert_eq!(mix_from_point(0., -1.), 50);
        assert_eq!(mix_from_point(-1., 1.), 0);
        assert_eq!(mix_from_point(1., 1.), 100);
        assert_eq!(mix_from_point(-0.1, 1.), 0);
        assert_eq!(mix_from_point(0.1, 1.), 100);
    }
    #[test]
    fn mix_names_from_audeze() {
        assert_eq!(parse_mix("Game/Chat Mix 50/50"), Some((50, 50)));
        assert_eq!(parse_mix("Game/Chat Mix 100/0"), Some((100, 0)));
        for bad in [
            "50/50",
            "Game/Chat Mix 101/0",
            "Game/Chat Mix 75/75",
            "Game/Chat Mix ?/?",
        ] {
            assert_eq!(parse_mix(bad), None);
        }
    }
    #[test]
    fn unavailable_and_battery_do_not_pop() {
        let old = SoundState {
            mix: Some((50, 50)),
            battery: Some(73),
            ..Default::default()
        };
        let mut next = old.clone();
        next.battery = Some(72);
        assert!(!next.sound_changed(&old));
        next.mix = None;
        assert!(!next.sound_changed(&old));
        next.mix = Some((55, 45));
        assert!(next.sound_changed(&old));
        assert!(!next.sound_changed(&SoundState::default()));
    }
}

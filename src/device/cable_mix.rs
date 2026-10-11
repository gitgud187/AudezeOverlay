//! Short-lived recovery for the observed Maxwell 2 ANC cable-mode reset.
use super::profiles::Profile;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct CableMix {
    latest: Option<(Profile, bool, u8, Instant)>,
    before_center: Option<(u8, Instant)>,
}
impl CableMix {
    pub fn observe(&mut self, profile: Profile, usb_cable: bool, chat: u8, now: Instant) {
        if let Some((old_profile, _, old_chat, _)) = self.latest {
            if old_profile != profile {
                self.before_center = None;
            } else if old_chat != chat {
                self.before_center = (chat == 50).then_some((old_chat, now));
            }
        }
        self.latest = Some((profile, usb_cable, chat, now));
    }

    /// Never restore across a long disconnect, unchanged topology, or another model.
    /// A bounded lookback handles the reset arriving before USB enumeration.
    pub fn reconnect(&mut self, profile: Profile, usb_cable: bool, now: Instant) -> Option<u8> {
        let (old, old_cable, chat, seen) = self.latest?;
        let pair = matches!(
            (old.product_id, profile.product_id),
            (0x4ba9, 0x4baa) | (0x4baa, 0x4ba9)
        );
        let cable_changed =
            old.product_id == 0x4ba9 && profile.product_id == 0x4ba9 && old_cable != usb_cable;
        if !(pair || cable_changed) || now.saturating_duration_since(seen) > Duration::from_secs(10)
        {
            return None;
        }
        let target = if chat == 50 {
            self.before_center
                .filter(|(_, at)| now.saturating_duration_since(*at) <= Duration::from_secs(4))
                .map(|(chat, _)| chat)
                .unwrap_or(chat)
        } else {
            chat
        };
        self.latest = None;
        self.before_center = None;
        (target != 50).then_some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cable_reset_recovers_once_but_other_reconnections_do_not() {
        let wireless = Profile::from_usb(0x3329, 0x4ba9).unwrap();
        let wired = Profile::from_usb(0x3329, 0x4baa).unwrap();
        let now = Instant::now();
        let mut memory = CableMix::default();
        memory.observe(wireless, false, 80, now);
        memory.observe(wireless, false, 50, now + Duration::from_millis(50));
        assert_eq!(
            memory.reconnect(wireless, false, now + Duration::from_secs(1)),
            None
        );
        assert_eq!(
            memory.reconnect(wired, true, now + Duration::from_secs(2)),
            Some(80)
        );
        assert_eq!(
            memory.reconnect(wired, true, now + Duration::from_secs(3)),
            None
        );
        memory.observe(wireless, false, 80, now);
        assert_eq!(
            memory.reconnect(wired, true, now + Duration::from_secs(11)),
            None
        );
        let other = Profile::from_usb(0x3329, 0x4b1a).unwrap();
        assert_eq!(memory.reconnect(other, true, now), None);
        memory.observe(wireless, false, 80, now);
        memory.observe(wireless, false, 50, now + Duration::from_millis(100));
        assert_eq!(
            memory.reconnect(wireless, true, now + Duration::from_secs(1)),
            Some(80)
        );
    }
}

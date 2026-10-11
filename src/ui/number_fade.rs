//! Stationary mix text: fade out, replace once, then fade in. No queued readings.
use audeze_overlay::motion::{ease, progress};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct NumberFrame {
    pub value: Option<(u8, u8)>,
    pub opacity: f32,
    pub active: bool,
}

#[derive(Default)]
pub(super) struct NumberFade {
    shown: Option<(u8, u8)>,
    started: Option<Instant>,
    switched: bool,
}

impl NumberFade {
    pub fn frame(&mut self, target: Option<(u8, u8)>, animate: bool, now: Instant) -> NumberFrame {
        if !animate || self.shown.is_none() || target.is_none() {
            self.shown = target;
            self.started = None;
        }
        if let Some(started) = self.started {
            let p = progress(started, now, 100);
            if p >= 0.5 && !self.switched {
                // Read the latest target at the midpoint. Rapid updates neither
                // queue obsolete values nor restart the fade-out indefinitely.
                self.shown = target;
                self.switched = true;
            }
            if p >= 1. {
                self.started = None;
            }
        }
        if self.started.is_none() && self.shown != target {
            self.started = Some(now);
            self.switched = false;
        }
        let opacity = self
            .started
            .map(|started| {
                let p = progress(started, now, 100);
                if p < 0.5 {
                    1. - ease(p * 2.)
                } else {
                    ease((p - 0.5) * 2.)
                }
            })
            .unwrap_or(1.);
        NumberFrame {
            value: self.shown,
            opacity,
            active: self.started.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn bursts_use_latest_target_without_restarting_or_overlapping_numbers() {
        let mut fade = NumberFade::default();
        let t = Instant::now();
        fade.frame(Some((50, 50)), true, t);
        fade.frame(Some((60, 40)), true, t);
        let before = fade.frame(Some((70, 30)), true, t + Duration::from_millis(40));
        assert_eq!(before.value, Some((50, 50)));
        assert!(before.opacity < 0.1);
        let middle = fade.frame(Some((80, 20)), true, t + Duration::from_millis(50));
        assert_eq!(middle.value, Some((80, 20)));
        assert_eq!(middle.opacity, 0.);
        // A reversal during fade-in waits for this short cycle to finish.
        let reversed = fade.frame(Some((20, 80)), true, t + Duration::from_millis(80));
        assert_eq!(reversed.value, Some((80, 20)));
        assert!(reversed.opacity > 0.8);
        let next = fade.frame(Some((20, 80)), true, t + Duration::from_millis(100));
        assert_eq!(next.opacity, 1.);
        fade.frame(Some((20, 80)), true, t + Duration::from_millis(150));
        let done = fade.frame(Some((20, 80)), true, t + Duration::from_millis(200));
        assert_eq!(done.value, Some((20, 80)));
        assert_eq!(done.opacity, 1.);
        assert!(!done.active);
    }

    #[test]
    fn hidden_disabled_and_disconnected_values_do_not_replay_a_fade() {
        let mut fade = NumberFade::default();
        let t = Instant::now();
        fade.frame(Some((50, 50)), true, t);
        fade.frame(Some((70, 30)), true, t);
        let hidden = fade.frame(Some((100, 0)), false, t);
        assert_eq!(hidden.value, Some((100, 0)));
        assert!(!hidden.active);
        assert!(!fade.frame(Some((100, 0)), true, t).active);
        assert_eq!(fade.frame(None, true, t).value, None);
        let restored = fade.frame(Some((65, 35)), true, t);
        assert_eq!(restored.value, Some((65, 35)));
        assert_eq!(restored.opacity, 1.);
        assert!(!restored.active);
    }
}

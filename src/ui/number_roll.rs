//! Independent digit wheels. Presentation never delays or writes headset values.
use audeze_overlay::motion::Tween;
use std::time::Instant;

const ROLL_MS: u64 = 220;
const PLACES: [u8; 3] = [100, 10, 1];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DigitFrame {
    pub lower: u8,
    pub upper: u8,
    /// Fraction of one glyph pitch, increasing as the strip moves upward.
    pub fraction: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NumberFrame {
    pub digits: [DigitFrame; 3],
    pub columns: usize,
    /// Visible field width in digit cells, interpolated through carries.
    pub width: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct MixFrame {
    pub values: [NumberFrame; 2],
    pub active: bool,
}
impl MixFrame {
    pub fn still(mix: (u8, u8)) -> Self {
        Self {
            values: [mix.0, mix.1].map(|value| NumberFrame {
                digits: PLACES.map(|place| DigitFrame {
                    lower: value / place % 10,
                    upper: value / place % 10,
                    fraction: 0.,
                }),
                columns: columns(value),
                width: columns(value) as f32,
            }),
            active: false,
        }
    }
}

fn columns(value: u8) -> usize {
    if value >= 100 {
        3
    } else if value >= 10 {
        2
    } else {
        1
    }
}

#[derive(Clone, Copy)]
struct DigitRoll {
    position: Tween,
    step: u8,
}
impl DigitRoll {
    fn new(digit: u8, step: u8, now: Instant) -> Self {
        Self {
            position: Tween::new((digit / step) as f32, now),
            step,
        }
    }

    fn retarget(&mut self, digit: u8, increasing: bool, now: Instant) {
        let period = (10 / self.step) as f32;
        // Rebase to one revolution: a long wheel session cannot accumulate
        // unbounded positions or lose fractional precision. Start where drawn.
        let current = self.position.value(now).rem_euclid(period);
        let mut target = (digit / self.step) as f32;
        if increasing && target < current {
            target += period;
        }
        if !increasing && target > current {
            target -= period;
        }
        self.position = Tween::new(current, now);
        self.position.set(target, now, ROLL_MS);
    }

    fn frame(&self, now: Instant) -> DigitFrame {
        let position = if self.position.active(now) {
            self.position.value(now)
        } else {
            // Integer targets must settle exactly, without a floating-point
            // remainder leaving a sliver of the neighboring glyph visible.
            self.position.target
        };
        let lower = position.floor();
        let period = (10 / self.step) as f32;
        DigitFrame {
            lower: lower.rem_euclid(period) as u8 * self.step,
            upper: (lower + 1.).rem_euclid(period) as u8 * self.step,
            fraction: position - lower,
        }
    }
}

pub(super) struct MixRoll {
    target: Option<(u8, u8)>,
    digits: [[DigitRoll; 3]; 2],
    columns: [usize; 2],
    widths: [Tween; 2],
}
impl Default for MixRoll {
    fn default() -> Self {
        Self {
            target: None,
            digits: [[DigitRoll::new(0, 1, Instant::now()); 3]; 2],
            columns: [1; 2],
            widths: [Tween::new(1., Instant::now()); 2],
        }
    }
}
impl MixRoll {
    pub fn frame(
        &mut self,
        target: Option<(u8, u8)>,
        animate: bool,
        now: Instant,
    ) -> Option<MixFrame> {
        let Some(mix) = target else {
            self.target = None;
            return None;
        };
        let values = [mix.0, mix.1];
        let five_step = |m: (u8, u8)| m.0.is_multiple_of(5) && m.1.is_multiple_of(5);
        if !animate || !five_step(mix) || !self.target.is_some_and(five_step) {
            // Normal five-point changes need only the 0/5 marks on the units
            // wheel. Preserve arbitrary readings exactly, without inventing a
            // quantized value or switching a moving strip's glyph spacing.
            self.digits = values.map(|value| {
                PLACES.map(|place| {
                    DigitRoll::new(
                        value / place % 10,
                        if place == 1 && five_step(mix) { 5 } else { 1 },
                        now,
                    )
                })
            });
            self.columns = values.map(columns);
            self.widths = values.map(|v| Tween::new(columns(v) as f32, now));
        } else if let Some(before) = self.target.filter(|old| *old != mix) {
            let widths = values.map(|v| columns(v) as f32);
            if self.widths.map(|w| w.target) != widths {
                // Retarget both widths together. Their sum stays within the
                // maximum four visible digits, even through rapid reversals.
                for (width, target) in self.widths.iter_mut().zip(widths) {
                    *width = Tween::new(width.value(now), now);
                    width.set(target, now, ROLL_MS);
                }
            }
            for (side, old) in [before.0, before.1].into_iter().enumerate() {
                let value = values[side];
                self.columns[side] = self.columns[side].max(columns(value));
                for (column, place) in PLACES.into_iter().enumerate() {
                    let digit = value / place % 10;
                    if old / place % 10 != digit {
                        self.digits[side][column].retarget(digit, value > old, now);
                    }
                }
            }
        }
        self.target = target;
        let active = self.digits.iter().flatten().any(|d| d.position.active(now))
            || self.widths.iter().any(|w| w.active(now));
        if !active {
            self.columns = values.map(columns);
        }
        Some(MixFrame {
            values: std::array::from_fn(|side| NumberFrame {
                digits: self.digits[side].map(|digit| digit.frame(now)),
                columns: self.columns[side],
                width: self.widths[side].value(now),
            }),
            active,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn five_point_change_moves_only_units_in_opposite_directions() {
        let mut roll = MixRoll::default();
        let t = Instant::now();
        let before = roll.frame(Some((20, 80)), true, t).unwrap();
        roll.frame(Some((25, 75)), true, t);
        let frame = roll
            .frame(Some((25, 75)), true, t + Duration::from_millis(40))
            .unwrap();
        assert_eq!(frame.values[0].digits[1], before.values[0].digits[1]);
        assert_eq!(frame.values[0].digits[0], before.values[0].digits[0]);
        let up = frame.values[0].digits[2];
        let down = frame.values[1].digits[2];
        assert_eq!((up.lower, up.upper), (0, 5));
        assert_eq!((down.lower, down.upper), (5, 0));
        assert!((up.fraction + down.fraction - 1.).abs() < 0.0001);
        assert!(up.fraction > 0. && up.fraction < 1.);
        assert!(frame.active);
    }

    #[test]
    fn carry_borrow_and_unchanged_units_follow_the_whole_value_direction() {
        let mut roll = MixRoll::default();
        let t = Instant::now();
        roll.frame(Some((95, 5)), true, t);
        roll.frame(Some((100, 0)), true, t);
        let mid = roll
            .frame(Some((100, 0)), true, t + Duration::from_millis(40))
            .unwrap();
        assert_eq!(mid.values[0].columns, 3);
        assert_eq!(
            (mid.values[0].digits[1].lower, mid.values[0].digits[1].upper),
            (9, 0)
        );
        let done = roll
            .frame(Some((100, 0)), true, t + Duration::from_millis(ROLL_MS))
            .unwrap();
        assert!(!done.active);
        assert_eq!(done.values[0].digits.map(|d| d.lower), [1, 0, 0]);
        let t = t + Duration::from_millis(200);
        roll.frame(Some((90, 10)), true, t);
        let mid = roll
            .frame(Some((90, 10)), true, t + Duration::from_millis(40))
            .unwrap();
        assert_eq!(mid.values[0].digits[2], done.values[0].digits[2]);
        assert_eq!(
            (mid.values[0].digits[1].lower, mid.values[0].digits[1].upper),
            (9, 0)
        );
        let done = roll
            .frame(Some((90, 10)), true, t + Duration::from_millis(ROLL_MS))
            .unwrap();
        assert_eq!(done.values[0].columns, 2);
        assert_eq!(done.values[0].digits[1].lower, 9);
    }

    #[test]
    fn rapid_reversal_preserves_visible_positions_and_settles_on_latest() {
        let mut roll = MixRoll::default();
        let t = Instant::now();
        roll.frame(Some((20, 80)), true, t);
        roll.frame(Some((25, 75)), true, t);
        let middle = t + Duration::from_millis(40);
        let before = roll.frame(Some((25, 75)), true, middle).unwrap();
        let reverse = roll.frame(Some((20, 80)), true, middle).unwrap();
        for side in 0..2 {
            for (a, b) in before.values[side]
                .digits
                .into_iter()
                .zip(reverse.values[side].digits)
            {
                assert_eq!((a.lower, a.upper), (b.lower, b.upper));
                // Rebasing by a full revolution can round by one f32 ULP.
                assert!((a.fraction - b.fraction).abs() < 0.000001);
            }
        }
        // A sustained sequence must not accumulate queued animations or drift.
        for i in 1..10_000 {
            let value = ((i % 21) * 5) as u8;
            let frame = roll
                .frame(
                    Some((value, 100 - value)),
                    true,
                    t + Duration::from_millis(i * 20),
                )
                .unwrap();
            assert!(frame.values.iter().map(|v| v.width).sum::<f32>() <= 4.000001);
            for digit in roll.digits.iter().flatten() {
                assert!(digit.position.target.abs() <= 20.);
            }
        }
        let end = t + Duration::from_secs(201);
        roll.frame(Some((35, 65)), true, end);
        let done = roll
            .frame(Some((35, 65)), true, end + Duration::from_millis(ROLL_MS))
            .unwrap();
        assert!(!done.active);
        assert_eq!(done.values[0].digits.map(|d| d.lower), [0, 3, 5]);
        assert_eq!(done.values[1].digits.map(|d| d.lower), [0, 6, 5]);
    }

    #[test]
    fn hidden_disabled_disconnected_and_non_step_readings_snap_exactly() {
        let mut roll = MixRoll::default();
        let t = Instant::now();
        roll.frame(Some((20, 80)), true, t);
        roll.frame(Some((25, 75)), true, t);
        assert!(!roll.frame(Some((100, 0)), false, t).unwrap().active);
        assert!(!roll.frame(Some((100, 0)), true, t).unwrap().active);
        assert!(roll.frame(None, true, t).is_none());
        assert!(!roll.frame(Some((45, 55)), true, t).unwrap().active);
        let precise = roll.frame(Some((43, 57)), true, t).unwrap();
        assert_eq!(precise.values[0].digits.map(|d| d.lower), [0, 4, 3]);
        assert!(!precise.active);
    }

    #[test]
    fn every_mix_transition_settles_to_exact_digits_with_no_running_timer() {
        let t = Instant::now();
        for from in (0..=100).step_by(5) {
            for to in (0..=100).step_by(5) {
                let mut roll = MixRoll::default();
                roll.frame(Some((from, 100 - from)), true, t);
                roll.frame(Some((to, 100 - to)), true, t);
                let done = roll
                    .frame(
                        Some((to, 100 - to)),
                        true,
                        t + Duration::from_millis(ROLL_MS),
                    )
                    .unwrap();
                assert!(!done.active);
                for (frame, expected) in done.values.into_iter().zip([to, 100 - to]) {
                    assert_eq!(frame.columns, columns(expected));
                    assert_eq!(frame.width, columns(expected) as f32);
                    assert_eq!(
                        frame.digits.map(|d| d.lower),
                        PLACES.map(|p| expected / p % 10)
                    );
                    assert!(frame.digits.iter().all(|d| d.fraction == 0.));
                }
            }
        }
    }

    #[test]
    fn endpoint_carries_and_reversals_keep_width_continuous() {
        let mut roll = MixRoll::default();
        let t = Instant::now();
        roll.frame(Some((5, 95)), true, t);
        let start = roll.frame(Some((0, 100)), true, t).unwrap();
        assert_eq!(start.values[1].width, 2.);
        let now = t + Duration::from_millis(70);
        let mid = roll.frame(Some((0, 100)), true, now).unwrap();
        assert!(mid.values[1].width > 2. && mid.values[1].width < 3.);
        let reversed = roll.frame(Some((5, 95)), true, now).unwrap();
        assert_eq!(mid.values[1].width, reversed.values[1].width);
        let done = roll
            .frame(Some((5, 95)), true, now + Duration::from_millis(ROLL_MS))
            .unwrap();
        assert_eq!(done.values[1].width, 2.);
        assert!(!done.active);
    }
}

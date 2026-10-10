//! Time-based presentation only. These transitions never delay device commands.
use std::time::Instant;

pub fn ease(progress: f32) -> f32 {
    1. - (1. - progress.clamp(0., 1.)).powi(3)
}
pub fn progress(start: Instant, now: Instant, millis: u64) -> f32 {
    if millis == 0 {
        1.
    } else {
        (now.saturating_duration_since(start).as_secs_f32() * 1000. / millis as f32).min(1.)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    from: f32,
    pub target: f32,
    start: Instant,
    duration: u64,
}
impl Tween {
    pub fn new(value: f32, now: Instant) -> Self {
        Self {
            from: value,
            target: value,
            start: now,
            duration: 0,
        }
    }
    pub fn value(&self, now: Instant) -> f32 {
        self.from + (self.target - self.from) * ease(progress(self.start, now, self.duration))
    }
    pub fn set(&mut self, target: f32, now: Instant, duration: u64) {
        if self.target != target || duration == 0 {
            self.from = self.value(now);
            self.target = target;
            self.start = now;
            self.duration = duration;
        }
    }
    pub fn active(&self, now: Instant) -> bool {
        self.from != self.target && progress(self.start, now, self.duration) < 1.
    }
}
#[derive(Clone, Copy, Debug)]
pub struct SelectionFrame {
    pub weights: [f32; 3],
    pub selected: Option<usize>,
    pub rightward: bool,
}
pub struct SelectionMotion {
    weights: [Tween; 3],
    selected: Option<usize>,
    rightward: bool,
}
impl SelectionMotion {
    pub fn new(now: Instant) -> Self {
        Self {
            weights: [Tween::new(0., now); 3],
            selected: None,
            rightward: true,
        }
    }
    pub fn update(
        &mut self,
        selected: Option<usize>,
        now: Instant,
        animate: bool,
    ) -> SelectionFrame {
        let duration = if animate && self.selected.is_some() {
            280
        } else {
            0
        };
        if selected != self.selected {
            self.rightward = selected >= self.selected;
        }
        for (i, weight) in self.weights.iter_mut().enumerate() {
            weight.set(if selected == Some(i) { 1. } else { 0. }, now, duration);
        }
        self.selected = selected;
        SelectionFrame {
            weights: self.weights.map(|w| w.value(now)),
            selected,
            rightward: self.rightward,
        }
    }
    pub fn active(&self, now: Instant) -> bool {
        self.weights.iter().any(|w| w.active(now))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn retargeting_starts_at_the_visible_value_and_stops() {
        let start = Instant::now();
        let mut t = Tween::new(0., start);
        t.set(100., start, 100);
        let middle = start + Duration::from_millis(40);
        let visible = t.value(middle);
        t.set(20., middle, 100);
        assert_eq!(t.value(middle), visible);
        assert_eq!(t.value(middle + Duration::from_millis(100)), 20.);
        assert!(!t.active(middle + Duration::from_millis(100)));
    }
    #[test]
    fn selection_follows_direction_and_can_disable_animation() {
        let now = Instant::now();
        let mut s = SelectionMotion::new(now);
        s.update(Some(0), now, true);
        assert!(s.update(Some(2), now, true).rightward);
        assert!(
            !s.update(Some(1), now + Duration::from_millis(50), true)
                .rightward
        );
        assert_eq!(s.update(Some(1), now, false).weights, [0., 1., 0.]);
        assert!(!s.active(now));
    }
}

//! Presentation state bridges pure transitions to the rasterizer. No device I/O.
use super::{
    draw::Box2,
    number_roll::{MixFrame, MixRoll},
};
use audeze_overlay::{
    model::SoundState,
    motion::{ease, progress, SelectionFrame, SelectionMotion, Tween},
};
use std::time::Instant;

pub struct AnimationFrame {
    pub cards: [Box2; 6],
    pub grip: f32,
    pub numbers: Option<MixFrame>,
    pub anc: SelectionFrame,
    pub noise: SelectionFrame,
    pub active: bool,
}

pub struct Presentation {
    cards_from: [Box2; 6],
    cards_to: Option<[Box2; 6]>,
    cards_started: Instant,
    grip: Tween,
    numbers: MixRoll,
    anc: SelectionMotion,
    noise: SelectionMotion,
}
impl Default for Presentation {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            cards_from: [Box2::default(); 6],
            cards_to: None,
            cards_started: now,
            grip: Tween::new(0., now),
            numbers: MixRoll::default(),
            anc: SelectionMotion::new(now),
            noise: SelectionMotion::new(now),
        }
    }
}
impl Presentation {
    fn card_frame(&self, now: Instant, animate: bool) -> [Box2; 6] {
        let to = self.cards_to.unwrap_or(self.cards_from);
        let p = if animate {
            ease(progress(self.cards_started, now, 130))
        } else {
            1.
        };
        std::array::from_fn(|i| {
            let a = self.cards_from[i];
            let b = to[i];
            Box2::new(
                a.x + (b.x - a.x) * p,
                a.y + (b.y - a.y) * p,
                a.w + (b.w - a.w) * p,
                a.h + (b.h - a.h) * p,
            )
        })
    }
    pub fn frame(
        &mut self,
        cards: [Box2; 6],
        sound: &SoundState,
        grabbed: bool,
        animate: bool,
        now: Instant,
    ) -> AnimationFrame {
        if self.cards_to != Some(cards) {
            self.cards_from = if self.cards_to.is_some() {
                self.card_frame(now, animate)
            } else {
                cards
            };
            self.cards_to = Some(cards);
            self.cards_started = now;
        }
        self.grip.set(
            if grabbed { 1. } else { 0. },
            now,
            if animate { 110 } else { 0 },
        );
        let numbers = self.numbers.frame(sound.mix, animate, now);
        let anc = self.anc.update(
            ["On", "Off", "Transparency"]
                .iter()
                .position(|s| sound.anc.as_deref() == Some(s)),
            now,
            animate,
        );
        let noise = self.noise.update(
            ["Off", "Low", "High"]
                .iter()
                .position(|s| sound.noise.as_deref() == Some(s)),
            now,
            animate,
        );
        let active = animate
            && ((self.cards_from != cards && progress(self.cards_started, now, 130) < 1.)
                || self.grip.active(now)
                || numbers.is_some_and(|frame| frame.active)
                || self.anc.active(now)
                || self.noise.active(now));
        AnimationFrame {
            cards: self.card_frame(now, animate),
            grip: self.grip.value(now),
            numbers,
            anc,
            noise,
            active,
        }
    }
}

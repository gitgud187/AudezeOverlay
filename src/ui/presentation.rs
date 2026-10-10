//! Presentation state bridges pure transitions to the rasterizer. No device I/O.
use super::draw::Box2;
use audeze_overlay::{
    model::SoundState,
    motion::{ease, progress, SelectionFrame, SelectionMotion, Tween},
};
use std::time::Instant;

pub struct AnimationFrame {
    pub cards: [Box2; 6],
    pub grip: f32,
    pub previous_mix: Option<(u8, u8)>,
    pub number_progress: f32,
    pub anc: SelectionFrame,
    pub noise: SelectionFrame,
    pub active: bool,
}
pub struct Presentation {
    cards_from: [Box2; 6],
    cards_to: Option<[Box2; 6]>,
    cards_started: Instant,
    grip: Tween,
    current_mix: Option<(u8, u8)>,
    previous_mix: Option<(u8, u8)>,
    number_started: Instant,
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
            current_mix: None,
            previous_mix: None,
            number_started: now,
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
        if self.current_mix != sound.mix {
            self.previous_mix = self.current_mix;
            self.current_mix = sound.mix;
            self.number_started = now;
        }
        let number_progress = if animate && self.previous_mix.is_some() {
            ease(progress(self.number_started, now, 110))
        } else {
            1.
        };
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
                || number_progress < 1.
                || self.anc.active(now)
                || self.noise.active(now));
        AnimationFrame {
            cards: self.card_frame(now, animate),
            grip: self.grip.value(now),
            previous_mix: self.previous_mix,
            number_progress,
            anc,
            noise,
            active,
        }
    }
}

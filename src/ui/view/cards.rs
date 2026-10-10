//! Sound-card rendering; no persistence, window calls or device communication.
use super::*;
pub(super) fn render_card(c: &mut Canvas, hits: &mut Vec<Hit>, v: &View, i: usize, r: Box2) {
    let fs = v.config.text_scale as f32 / 100.;
    let pad = 16.;
    let inner = r.inset(pad);
    let label = 18. * fs;
    if i != 4 {
        c.text(
            layout::NAMES[i],
            Box2::new(
                inner.x + if v.edit { 28. } else { 0. },
                inner.y,
                inner.w - if v.edit { 28. } else { 0. },
                30.,
            ),
            label,
            FG,
            true,
            i == 0,
        );
    }
    match i {
        0 => {
            let mut radius = ((r.w - 52.) / 2.).min((r.h - 112.) / 2.).max(32.);
            if radius < 65. {
                // Compact cards need the space previously reserved for the
                // redundant inner caption. The dial center stays unchanged.
                radius = ((r.w - 52.) / 2.).min((r.h - 90.) / 2.).max(32.);
            }
            let compact = radius < 75.;
            let (cx, cy) = (r.x + r.w / 2., r.y + 54. + (r.h - 90.) / 2.);
            c.arc(cx, cy, radius, 135., 405., 0x696969, 1., 5.);
            c.arc(cx, cy, radius - 12., 0., 360., 0x393939, 1., 1.5);
            let grip = v.animation.map(|frame| frame.grip).unwrap_or(0.);
            let active_radius = radius + grip * 2.;
            c.arc(
                cx,
                cy,
                active_radius,
                135.,
                135. + v.mix * 2.7,
                0xd6d6d6,
                1.,
                5. + grip * 2.5,
            );
            let a = (135. + v.mix * 2.7).to_radians();
            c.circle(
                cx + active_radius * a.cos(),
                cy + active_radius * a.sin(),
                5. + grip,
                FG,
                1.,
            );
            // A small inset marker rotates with the rim dot, like a physical knob.
            c.line(
                &[
                    (cx + (radius - 19.) * a.cos(), cy + (radius - 19.) * a.sin()),
                    (cx + (radius - 26.) * a.cos(), cy + (radius - 26.) * a.sin()),
                ],
                FG,
                0.9,
                2.5,
            );
            let value = v
                .sound
                .mix
                .map(|(g, chat)| format!("{g} / {chat}"))
                .unwrap_or_else(|| "— / —".into());
            let text_width = if compact {
                (radius - 14.) * 2.
            } else {
                radius * 2.
            };
            let mut value_size = (34. * fs).min(radius * 0.55).max(22.);
            let value_width = |size: f32| {
                let px = (size * c.scale).round().max(1.);
                value
                    .chars()
                    .map(|ch| c.painter.fonts[1].metrics(ch, px).advance_width)
                    .sum::<f32>()
                    / c.scale
            };
            while value_size > 22. && value_width(value_size) > text_width - 0.5 {
                value_size = (value_size - 1.).max(22.);
            }
            let text_width = text_width.max(value_width(value_size) + 0.5);
            let value_rect = Box2::new(
                cx - text_width / 2.,
                cy - if compact { 14. } else { 26. },
                text_width,
                50.,
            );
            let transition = v.animation.map(|f| f.number_progress).unwrap_or(1.);
            if transition < 1. {
                if let Some((game, chat)) = v.animation.and_then(|f| f.previous_mix) {
                    c.text_alpha(
                        &format!("{game} / {chat}"),
                        Box2::new(
                            value_rect.x,
                            value_rect.y - transition * 5.,
                            value_rect.w,
                            value_rect.h,
                        ),
                        value_size,
                        FG,
                        true,
                        true,
                        (1. - transition) * 0.35,
                    );
                }
            }
            c.text_alpha(
                &value,
                Box2::new(
                    value_rect.x,
                    value_rect.y + (1. - transition) * 5.,
                    value_rect.w,
                    value_rect.h,
                ),
                value_size,
                FG,
                true,
                true,
                0.65 + 0.35 * transition,
            );
            if !compact {
                c.text(
                    "GAME / CHAT",
                    Box2::new(cx - radius, cy + 22., radius * 2., 24.),
                    14. * fs,
                    SUB,
                    true,
                    true,
                );
            }
            c.text(
                "Game",
                Box2::new(inner.x, r.y + r.h - 36., inner.w / 2., 26.),
                16. * fs,
                SUB,
                false,
                false,
            );
            c.text(
                "Chat",
                Box2::new(
                    r.x + r.w - 64. - 20. * (fs - 1.),
                    r.y + r.h - 36.,
                    58. + 20. * (fs - 1.),
                    26.,
                ),
                16. * fs,
                SUB,
                false,
                false,
            );
            hits.push(Hit {
                rect: Box2::new(
                    cx - radius - 10.,
                    cy - radius - 10.,
                    radius * 2. + 20.,
                    radius * 2. + 20.,
                ),
                action: Action::Mix,
            });
        }
        1 => {
            switch(
                c,
                hits,
                Box2::new(r.x + r.w - 66., r.y + 16., 48., 26.),
                v.sound.sidetone == Some(true),
                Action::Side,
            );
            let value = v
                .sound
                .sidetone_level
                .map(|n| format!("Level {n}%"))
                .unwrap_or_else(|| "Level —".into());
            c.text(
                &value,
                Box2::new(inner.x, r.y + 49., inner.w, 25.),
                16. * fs,
                SUB,
                false,
                false,
            );
            let y = r.y + r.h - 22.;
            c.line(&[(inner.x, y), (inner.x + inner.w, y)], 0x454545, 1., 4.);
            let p = v.sound.sidetone_level.unwrap_or(0) as f32 / 100.;
            c.line(
                &[(inner.x, y), (inner.x + inner.w * p, y)],
                0xbababa,
                1.,
                4.,
            );
            c.circle(
                inner.x + inner.w * p,
                y,
                5.,
                if v.sound.sidetone == Some(true) {
                    FG
                } else {
                    0x888888
                },
                1.,
            );
            hits.push(Hit {
                rect: Box2::new(inner.x - 5., y - 12., inner.w + 10., 24.),
                action: Action::Level,
            });
        }
        2 | 3 => {
            let labels = if i == 2 {
                ["On", "Off", "Transparency"]
            } else {
                ["Off", "Low", "High"]
            };
            let selected = if i == 2 {
                v.sound.anc.as_deref()
            } else {
                v.sound.noise.as_deref()
            };
            let stacked = i == 2 && inner.w < 250. && r.h >= 142.;
            let measure = |size: f32| {
                let px = (size * c.scale).round().max(1.);
                labels.map(|label| {
                    label
                        .chars()
                        .map(|ch| c.painter.fonts[1].metrics(ch, px).advance_width)
                        .sum::<f32>()
                        / c.scale
                        + 0.5
                })
            };
            let (mut font_size, mut padding) = (16., 6.);
            if i == 2 && !stacked {
                // Reserve each complete label first. Fixed proportional widths
                // clipped Transparency even when the row had enough total room.
                for (size, pad) in [(16., 6.), (16., 3.), (14., 2.), (14., 1.)] {
                    font_size = size;
                    padding = pad;
                    if measure(size).iter().sum::<f32>() + pad * 6. + 20. <= inner.w {
                        break;
                    }
                }
            }
            let measured = measure(font_size);
            let mut boxes = [Box2::default(); 3];
            if stacked {
                let y = r.y + r.h - 96.;
                let half = (inner.w - 10.) / 2.;
                boxes = [
                    Box2::new(inner.x, y, half, 36.),
                    Box2::new(inner.x + half + 10., y, half, 36.),
                    Box2::new(inner.x, y + 44., inner.w, 36.),
                ];
            } else {
                let y = r.y + r.h - 52.;
                let extra = (inner.w - 20. - measured.iter().sum::<f32>() - padding * 6.).max(0.);
                let mut x = inner.x;
                for j in 0..3 {
                    let width = if i == 2 {
                        measured[j] + padding * 2. + extra * [0.22, 0.22, 0.56][j]
                    } else {
                        (inner.w - 20.) / 3.
                    };
                    boxes[j] = Box2::new(x, y, width, 36.);
                    x += width + 10.;
                }
            }
            for (j, l) in labels.iter().enumerate() {
                let a = if i == 2 {
                    Action::Anc(j as u8)
                } else {
                    Action::Noise(j as u8)
                };
                let rect = boxes[j];
                let active = selected == Some(*l);
                let motion = v
                    .animation
                    .map(|frame| if i == 2 { frame.anc } else { frame.noise });
                let weight =
                    motion
                        .map(|frame| frame.weights[j])
                        .unwrap_or(if active { 1. } else { 0. });
                c.rect(rect, if v.hover == a { 0x424242 } else { 0x2b2b2b }, 1., 6.);
                let from_left = motion
                    .map(|frame| {
                        if frame.selected == Some(j) {
                            frame.rightward
                        } else {
                            !frame.rightward
                        }
                    })
                    .unwrap_or(true);
                let width = rect.w * weight;
                let fill = Box2::new(
                    if from_left {
                        rect.x
                    } else {
                        rect.x + rect.w - width
                    },
                    rect.y,
                    width,
                    rect.h,
                );
                if weight > 0. {
                    c.rect(fill, 0xebebeb, 1., 6.);
                }
                let label_bounds = Box2::new(
                    rect.x + padding,
                    rect.y + (rect.h - font_size - 2.) / 2.,
                    rect.w - padding * 2.,
                    24.,
                );
                c.text(l, label_bounds, font_size, FG, true, true);
                if weight > 0. {
                    c.text_clipped(l, label_bounds, font_size, 0x151515, true, true, fill);
                }
                hits.push(Hit { rect, action: a });
            }
        }
        4 => {
            let status = v.sound.device.as_deref().unwrap_or("Headset disconnected");
            let battery = v
                .sound
                .battery
                .map(|b| format!("{b}% battery"))
                .unwrap_or_else(|| "Battery —".into());
            let mute = match v.sound.muted {
                Some(true) => "Mic muted",
                Some(false) => "Mic live",
                None => "Mic —",
            };
            let wide = inner.w >= 480.;
            if wide {
                c.text(
                    status,
                    Box2::new(
                        inner.x + if v.edit { 28. } else { 0. },
                        inner.y,
                        inner.w * 0.38 - if v.edit { 28. } else { 0. },
                        28.,
                    ),
                    16. * fs,
                    FG,
                    true,
                    false,
                );
                c.text(
                    "USB Dongle",
                    Box2::new(inner.x + inner.w * 0.4, inner.y, inner.w * 0.26, 28.),
                    16. * fs,
                    SUB,
                    false,
                    false,
                );
                c.text(
                    &battery,
                    Box2::new(inner.x + inner.w * 0.7, inner.y, inner.w * 0.3, 28.),
                    16. * fs,
                    FG,
                    true,
                    false,
                );
                c.text(
                    mute,
                    Box2::new(inner.x, inner.y + 28., inner.w, 26.),
                    14. * fs,
                    SUB,
                    false,
                    false,
                );
            } else {
                c.text(
                    status,
                    Box2::new(
                        inner.x + if v.edit { 28. } else { 0. },
                        inner.y,
                        inner.w - if v.edit { 28. } else { 0. },
                        26.,
                    ),
                    16. * fs,
                    FG,
                    true,
                    false,
                );
                c.text(
                    &format!("{battery} · {mute}"),
                    Box2::new(inner.x, inner.y + 29., inner.w, 24.),
                    14. * fs,
                    SUB,
                    false,
                    false,
                );
            }
        }
        _ => (),
    }
}

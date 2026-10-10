//! Sound-card rendering; no persistence, window calls or device communication.
use super::*;
pub(super) fn render_card(c: &mut Canvas, hits: &mut Vec<Hit>, v: &View, i: usize, r: Box2) {
    let fs = v.config.text_scale as f32 / 100.;
    let short = i != 0 && r.h < 110.;
    let pad = if short { 8. } else { 16. };
    let inner = r.inset(pad);
    let mut label = if short { 16. * fs } else { 18. * fs };
    let label_width = inner.w - if v.edit { 28. } else { 0. };
    while label > 14. {
        let px = (label * c.scale).round().max(1.);
        let measured: f32 = layout::NAMES[i]
            .chars()
            .map(|ch| c.painter.fonts[1].metrics(ch, px).advance_width / c.scale)
            .sum();
        if measured <= label_width {
            break;
        }
        label -= 0.5;
    }
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
            if let Some(current) = v.sound.mix {
                let previous = v.animation.and_then(|f| f.previous_mix).unwrap_or(current);
                odometer(c, value_rect, value_size, previous, current, transition);
            } else {
                c.text(&value, value_rect, value_size, FG, true, true);
            }
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
                Box2::new(
                    r.x + r.w - 66.,
                    r.y + pad,
                    48.,
                    if short { 22. } else { 26. },
                ),
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
                Box2::new(inner.x, r.y + if short { 33. } else { 49. }, inner.w, 25.),
                16. * fs,
                SUB,
                false,
                false,
            );
            let y = r.y + r.h - if short { 12. } else { 22. };
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
                let button_height = if short { 28. } else { 36. };
                let y = r.y + r.h - button_height - if short { 8. } else { 16. };
                let extra = (inner.w - 20. - measured.iter().sum::<f32>() - padding * 6.).max(0.);
                let mut x = inner.x;
                for j in 0..3 {
                    let width = if i == 2 {
                        measured[j] + padding * 2. + extra * [0.22, 0.22, 0.56][j]
                    } else {
                        (inner.w - 20.) / 3.
                    };
                    boxes[j] = Box2::new(x, y, width, button_height);
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
                let compact_status = if inner.w < 250. {
                    format!(
                        "{} · {}",
                        v.sound
                            .battery
                            .map(|b| format!("{b}%"))
                            .unwrap_or_else(|| "—".into()),
                        match v.sound.muted {
                            Some(true) => "Muted",
                            Some(false) => "Mic live",
                            None => "Mic —",
                        }
                    )
                } else {
                    format!("{battery} · {mute}")
                };
                c.text(
                    &compact_status,
                    Box2::new(inner.x, inner.y + 29., inner.w, 24.),
                    14. * fs,
                    SUB,
                    false,
                    false,
                );
            }
        }
        5 => super::equalizer::card(c, hits, v, r),
        _ => (),
    }
}

/// Compact vector status indicators; no icon fonts or extra runtime resources.
pub(super) fn status_icons(c: &mut Canvas, v: &View) {
    let y = 63.;
    let ink = if v.sound.mix.is_some() { FG } else { SUB };
    // USB connection.
    c.line(&[(36., y + 7.), (36., y - 7.), (32., y - 3.)], ink, 1., 1.6);
    c.line(&[(36., y - 7.), (40., y - 3.)], ink, 1., 1.6);
    c.line(&[(36., y + 3.), (42., y), (42., y - 3.)], ink, 1., 1.6);
    c.circle(36., y + 7., 2., ink, 1.);
    c.text(
        if v.sound.mix.is_some() {
            "USB"
        } else {
            "Offline"
        },
        Box2::new(48., y - 8., 54., 20.),
        14.,
        ink,
        true,
        false,
    );
    // Battery outline and charge fill.
    c.line(
        &[
            (110., y - 6.),
            (130., y - 6.),
            (130., y + 6.),
            (110., y + 6.),
            (110., y - 6.),
        ],
        ink,
        1.,
        1.5,
    );
    c.rect(Box2::new(131., y - 3., 2., 6.), ink, 1., 0.);
    if let Some(b) = v.sound.battery {
        c.rect(
            Box2::new(113., y - 3., 14. * b as f32 / 100., 6.),
            ink,
            1.,
            0.,
        );
    }
    c.text(
        &v.sound
            .battery
            .map(|b| format!("{b}%"))
            .unwrap_or_else(|| "—".into()),
        Box2::new(141., y - 8., 50., 20.),
        14.,
        ink,
        true,
        false,
    );
    // Microphone plus a diagonal stroke when muted.
    c.rect(Box2::new(202., y - 7., 6., 10.), ink, 1., 3.);
    c.arc(205., y, 7., 0., 180., ink, 1., 1.4);
    c.line(
        &[
            (205., y + 7.),
            (205., y + 10.),
            (201., y + 10.),
            (209., y + 10.),
        ],
        ink,
        1.,
        1.4,
    );
    if v.sound.muted == Some(true) {
        c.line(&[(198., y - 8.), (213., y + 9.)], ink, 1., 1.6);
    }
    c.text(
        match v.sound.muted {
            Some(true) => "Muted",
            Some(false) => "Mic live",
            None => "Mic —",
        },
        Box2::new(222., y - 8., 84., 20.),
        14.,
        ink,
        true,
        false,
    );
}

fn odometer(c: &mut Canvas, r: Box2, size: f32, old: (u8, u8), new: (u8, u8), p: f32) {
    let digits = |n: u8| {
        if n == 100 {
            3
        } else if n >= 10 {
            2
        } else {
            1
        }
    };
    let game_columns = digits(new.0).max(if p < 1. { digits(old.0) } else { 0 });
    let chat_columns = digits(new.1).max(if p < 1. { digits(old.1) } else { 0 });
    let columns = game_columns + chat_columns + 1;
    let cell = (r.w / columns as f32).min(size * 0.76);
    let mut size = size;
    while size > 10. {
        let px = (size * c.scale).round().max(1.);
        let width = ('0'..='9')
            .chain(Some('/'))
            .map(|ch| c.painter.fonts[1].metrics(ch, px).advance_width / c.scale)
            .fold(0., f32::max);
        if width + 1. <= cell {
            break;
        }
        size -= 0.5;
    }
    let left = r.x + (r.w - cell * columns as f32) / 2.;
    let clip = Box2::new(left, r.y, cell * columns as f32, size + 8.);
    c.text_clipped(
        "/",
        Box2::new(left + cell * game_columns as f32, r.y, cell, r.h),
        size,
        FG,
        true,
        true,
        clip,
    );
    for (side, before, after) in [(0, old.0, new.0), (game_columns + 1, old.1, new.1)] {
        let a = if side == 0 {
            format!("{before:>game_columns$}")
        } else {
            format!("{before:<chat_columns$}")
        };
        let b = if side == 0 {
            format!("{after:>game_columns$}")
        } else {
            format!("{after:<chat_columns$}")
        };
        let direction = if after >= before { -1. } else { 1. };
        for (column, (a, b)) in a.chars().zip(b.chars()).enumerate() {
            let x = left + (side + column) as f32 * cell;
            if a == b || p >= 1. {
                c.text_clipped(
                    &b.to_string(),
                    Box2::new(x, r.y, cell, r.h),
                    size,
                    FG,
                    true,
                    true,
                    clip,
                );
            } else {
                let distance = size + 8.;
                c.text_clipped(
                    &a.to_string(),
                    Box2::new(x, r.y + direction * p * distance, cell, r.h),
                    size,
                    FG,
                    true,
                    true,
                    clip,
                );
                c.text_clipped(
                    &b.to_string(),
                    Box2::new(x, r.y - direction * (1. - p) * distance, cell, r.h),
                    size,
                    FG,
                    true,
                    true,
                    clip,
                );
            }
        }
    }
}

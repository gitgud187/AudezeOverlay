//! Sound-card rendering; no persistence, window calls or device communication.
use super::*;
pub(super) fn render_card(c: &mut Canvas, hits: &mut Vec<Hit>, v: &View, i: usize, r: Box2) {
    let fs = v.config.text_scale as f32 / 100.;
    let short = r.h < 110.;
    let pad = if short { 8. } else { 16. };
    let inner = r.inset(pad);
    let mut label = if short { 16. * fs } else { 18. * fs };
    let label_width = inner.w - if v.edit { 28. } else { 0. } - if i == 1 { 60. } else { 0. };
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
                label_width,
                30.,
            ),
            label,
            FG,
            true,
            i == 0,
        );
    }
    if i == 2 && v.sound.capabilities.is_some_and(|caps| !caps.anc) {
        c.fitted_text(
            "Not available on this model",
            Box2::new(inner.x, inner.y + 28., inner.w, 28.),
            14.,
            SUB,
            false,
            false,
        );
        return;
    }
    match i {
        0 => {
            if v.config.minimal_cards[0] || r.h < 190. {
                let y = r.y + r.h - 14.;
                let bounds = Box2::new(inner.x + 6., y - 12., inner.w - 12., 24.);
                c.line(&[(bounds.x, y), (bounds.x + bounds.w, y)], 0x555555, 1., 4.);
                if let Some((game, chat)) = v.sound.mix {
                    c.circle(bounds.x + bounds.w * chat as f32 / 100., y, 6., FG, 1.);
                    c.fitted_text(
                        &format!("Game {game} / {chat} Chat"),
                        Box2::new(inner.x, inner.y + 23., inner.w, 20.),
                        16. * fs,
                        FG,
                        true,
                        true,
                    );
                    hits.push(Hit {
                        rect: bounds,
                        action: Action::Mix,
                    });
                } else {
                    c.fitted_text(
                        "Mix unavailable",
                        Box2::new(inner.x, inner.y + 23., inner.w, 20.),
                        14.,
                        SUB,
                        false,
                        true,
                    );
                }
                return;
            }
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
            if v.sound.mix.is_some() {
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
            }
            let value = v
                .sound
                .mix
                .map(|(g, chat)| format!("{g} / {chat}"))
                .unwrap_or_else(|| "— / —".into());
            // Keep every digit inside the inner ring, including 100 / 0.
            let text_width = (radius - 18.).max(16.) * 2.;
            let value_size = (34. * fs).min(radius * 0.6).max(12.);
            let value_rect = Box2::new(
                cx - text_width / 2.,
                cy - if compact { 20. } else { 28. },
                text_width,
                40.,
            );
            let transition = v.animation.map(|f| f.number_progress).unwrap_or(1.);
            if let Some(current) = v.sound.mix {
                let previous = v.animation.and_then(|f| f.previous_mix).unwrap_or(current);
                odometer(c, value_rect, value_size, previous, current, transition);
            } else {
                c.fitted_text(&value, value_rect, value_size, FG, true, true);
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
            if v.sound.mix.is_some() {
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
            c.fitted_text(
                &value,
                Box2::new(
                    inner.x,
                    r.y + if short { 29. } else { 49. },
                    inner.w,
                    if short { 20. } else { 25. },
                ),
                (16. * fs).min(if short { 16. } else { 24. }),
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
                    c.reveal_rect(rect, fill, 0xebebeb, 6.);
                }
                let label_bounds = Box2::new(
                    rect.x + padding,
                    rect.y + (rect.h - font_size - 2.) / 2.,
                    rect.w - padding * 2.,
                    24.,
                );
                c.fitted_text(l, label_bounds, font_size, FG, true, true);
                if weight > 0. {
                    c.text_clipped(l, label_bounds, font_size, 0x151515, true, true, fill);
                }
                hits.push(Hit { rect, action: a });
            }
        }
        4 => {
            let status = v.sound.device.as_deref().unwrap_or("Headset disconnected");
            let battery = format!(
                "{} battery{}",
                v.sound.battery_label(),
                if v.sound.usb_cable {
                    " · USB power"
                } else {
                    ""
                }
            );
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
                    v.sound.connection.as_deref().unwrap_or("Disconnected"),
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
pub(super) fn status_icons(c: &mut Canvas, hits: &mut Vec<Hit>, v: &View, width: f32) {
    let ink = if v.sound.mix.is_some() { FG } else { SUB };
    let row = Box2::new(16., 72., width - 32., 54.);
    c.text(
        v.sound.device.as_deref().unwrap_or("Headset disconnected"),
        Box2::new(row.x, row.y, row.w - if v.edit { 38. } else { 0. }, 24.),
        16.,
        ink,
        true,
        false,
    );
    let step = (row.w / 3.).min(112.);
    let y = 112.;
    let x = row.x + 6.;
    if v.sound.transport == Some(audeze_overlay::model::Connection::Bluetooth) {
        c.line(
            &[
                (x - 5., y - 5.),
                (x + 5., y + 4.),
                (x, y + 9.),
                (x, y - 9.),
                (x + 5., y - 4.),
                (x - 5., y + 5.),
            ],
            ink,
            1.,
            1.5,
        );
    } else if v.sound.transport == Some(audeze_overlay::model::Connection::WirelessDongle) {
        c.arc(x + 1., y + 5., 12., 225., 315., ink, 1., 1.5);
        c.arc(x + 1., y + 5., 7., 225., 315., ink, 1., 1.5);
        c.circle(x + 1., y + 3., 2., ink, 1.);
    } else {
        c.line(&[(x, y + 6.), (x, y - 7.), (x - 4., y - 3.)], ink, 1., 1.5);
        c.line(&[(x, y - 7.), (x + 4., y - 3.)], ink, 1., 1.5);
        c.line(
            &[(x, y + 2.), (x + 6., y - 1.), (x + 6., y - 4.)],
            ink,
            1.,
            1.5,
        );
        c.circle(x, y + 6., 2., ink, 1.);
    }
    c.fitted_text(
        v.sound.connection_label(),
        Box2::new(row.x + 20., y - 8., step - 22., 20.),
        13.,
        ink,
        true,
        false,
    );
    let x = row.x + step;
    c.line(
        &[
            (x, y - 6.),
            (x + 18., y - 6.),
            (x + 18., y + 6.),
            (x, y + 6.),
            (x, y - 6.),
        ],
        ink,
        1.,
        1.5,
    );
    c.rect(Box2::new(x + 19., y - 3., 2., 6.), ink, 1., 0.);
    if v.sound.charging == Some(true) {
        c.line(
            &[
                (x + 13., y - 5.),
                (x + 8., y),
                (x + 12., y),
                (x + 7., y + 5.),
            ],
            ink,
            1.,
            1.5,
        );
    }
    if v.sound.usb_cable {
        // Plug = USB power is available. A bolt would assert measured charging.
        c.line(
            &[
                (x + 7., y - 5.),
                (x + 7., y - 2.),
                (x + 5., y - 2.),
                (x + 5., y + 2.),
                (x + 13., y + 2.),
                (x + 13., y - 2.),
                (x + 11., y - 2.),
                (x + 11., y - 5.),
            ],
            ink,
            1.,
            1.3,
        );
        c.line(&[(x + 9., y + 2.), (x + 9., y + 5.)], ink, 1., 1.3);
    } else if let Some(b) = v.sound.battery.filter(|_| v.sound.charging != Some(true)) {
        c.rect(
            Box2::new(x + 3., y - 3., 12. * b as f32 / 100., 6.),
            ink,
            1.,
            0.,
        );
    }
    c.text(
        &v.sound.battery_label(),
        Box2::new(x + 27., y - 8., step - 28., 20.),
        13.,
        ink,
        true,
        false,
    );
    let x = row.x + step * 2. + 6.;
    c.rect(Box2::new(x - 3., y - 7., 6., 10.), ink, 1., 3.);
    c.arc(x, y, 7., 0., 180., ink, 1., 1.4);
    c.line(
        &[
            (x, y + 7.),
            (x, y + 10.),
            (x - 4., y + 10.),
            (x + 4., y + 10.),
        ],
        ink,
        1.,
        1.4,
    );
    if v.sound.muted == Some(true) {
        c.line(&[(x - 7., y - 8.), (x + 8., y + 9.)], ink, 1., 1.6);
    }
    c.text(
        match v.sound.muted {
            Some(true) => "Muted",
            Some(false) => "Live",
            None => "—",
        },
        Box2::new(x + 15., y - 8., step - 21., 20.),
        13.,
        ink,
        true,
        false,
    );
    if v.edit {
        let dots = Box2::new(width - 48., 72., 32., 22.);
        button(c, hits, v, dots, "•••", Action::StatusMenu, false);
        hits.push(Hit {
            rect: row,
            action: Action::StatusMenu,
        });
    }
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
    let cell = (r.w / columns as f32).min(size * 0.68);
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
    // Center actual numeral ink, not a font's asymmetric line box. Recalculate
    // after fitting so narrower cards do not shift the digits off the dial axis.
    let px = (size * c.scale).round().max(1.);
    let metrics = c.painter.fonts[1].metrics('0', px);
    let ascent = c.painter.fonts[1]
        .horizontal_line_metrics(px)
        .map(|m| m.ascent)
        .unwrap_or(px);
    let ink_center = (ascent - metrics.height as f32 / 2. - metrics.ymin as f32) / c.scale;
    let r = Box2::new(r.x, r.y + r.h / 2. - ink_center, r.w, r.h);
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
                c.rolling_text(
                    &a.to_string(),
                    Box2::new(x, r.y + direction * p * distance, cell, r.h),
                    size,
                    clip,
                    1.4 * (std::f32::consts::PI * p).sin(),
                );
                c.rolling_text(
                    &b.to_string(),
                    Box2::new(x, r.y - direction * (1. - p) * distance, cell, r.h),
                    size,
                    clip,
                    1.4 * (std::f32::consts::PI * p).sin(),
                );
            }
        }
    }
}

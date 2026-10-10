//! Equalizer view. Tuning is a local draft until Save to headset is pressed.
use super::*;

pub(super) fn card(c: &mut Canvas, hits: &mut Vec<Hit>, v: &View, r: Box2) {
    let pad = if r.h < 110. { 8. } else { 16. };
    let name = v
        .sound
        .eq_index
        .map(|i| v.config.eq.name(i))
        .unwrap_or("Choose EQ");
    let height = if r.h < 110. { 28. } else { 36. };
    let y = r.y + r.h - pad - height;
    button(
        c,
        hits,
        v,
        Box2::new(r.x + pad, y, r.w - pad * 2. - 58., height),
        name,
        Action::EqMenu,
        false,
    );
    button(
        c,
        hits,
        v,
        Box2::new(r.x + r.w - pad - 52., y, 52., height),
        "Edit",
        Action::EqEditor,
        false,
    );
}

pub(super) fn page(c: &mut Canvas, hits: &mut Vec<Hit>, v: &View, w: f32) {
    let selected = v.eq_editor.map(|e| e.selected).unwrap_or(6);
    let draft = v.eq_editor.and_then(|e| e.draft());
    let connected = v.sound.mix.is_some();
    for slot in 0..10u8 {
        let y = 180. + slot as f32 * 37.;
        let name = v.config.eq.name(slot);
        button(
            c,
            hits,
            v,
            Box2::new(218., y, 146., 31.),
            name,
            Action::EqInspect(slot),
            selected == slot,
        );
        let pinned = v.config.eq.pinned[slot as usize];
        let r = Box2::new(370., y, 32., 31.);
        button(c, hits, v, r, "", Action::EqPin(slot), pinned);
        // A vector pin avoids font-dependent icon glyphs.
        let ink = if pinned { 0x151515 } else { FG };
        c.line(
            &[
                (r.x + 11., y + 8.),
                (r.x + 21., y + 8.),
                (r.x + 19., y + 16.),
                (r.x + 23., y + 19.),
                (r.x + 9., y + 19.),
                (r.x + 13., y + 16.),
                (r.x + 11., y + 8.),
            ],
            ink,
            1.,
            1.5,
        );
        c.line(&[(r.x + 16., y + 19.), (r.x + 16., y + 26.)], ink, 1., 1.5);
    }
    c.text(
        "Pin to show in quick switch.",
        Box2::new(218., 558., 194., 44.),
        13.,
        SUB,
        false,
        false,
    );
    switch(
        c,
        hits,
        Box2::new(218., 611., 44., 24.),
        v.config.eq.show_factory,
        Action::Setting("eq-factory", 0),
    );
    c.text(
        "Include factory EQs",
        Box2::new(274., 612., 155., 26.),
        14.,
        FG,
        false,
        false,
    );
    c.wrapped_text(
        "Turning this off hides factory presets from the quick switch menu.",
        Box2::new(218., 645., 190., 50.),
        13.,
        SUB,
    );
    let left = 426.;
    let width = w - left - 32.;
    let name = v
        .eq_editor
        .and_then(|e| e.name_edit.as_deref())
        .unwrap_or(v.config.eq.name(selected));
    let rename = Box2::new(left, 181., width, 36.);
    c.rect(rename, 0x292929, v.config.card_opacity as f32 / 100., 6.);
    c.text(
        name,
        Box2::new(left + 10., 187., width - 20., 26.),
        18.,
        FG,
        true,
        false,
    );
    if selected >= 6 {
        hits.push(Hit {
            rect: rename,
            action: Action::EqRename,
        });
    }
    c.text(
        if selected >= 6 {
            "Click the name to rename. Saved on this PC."
        } else {
            "Factory preset · read-only"
        },
        Box2::new(left, 226., width, 24.),
        13.,
        SUB,
        false,
        false,
    );
    if let Some(draft) = draft {
        let band_width = width / 10.;
        let (top, bottom) = (293., 509.);
        for value in [-12, -6, 0, 6, 12] {
            let y = top + (12 - value) as f32 / 24. * (bottom - top);
            c.line(
                &[(left, y), (left + width, y)],
                if value == 0 { 0x787878 } else { 0x424242 },
                0.7,
                1.,
            );
        }
        for (i, band) in draft.value.bands.iter().enumerate() {
            let x = left + band_width * (i as f32 + 0.5);
            let y = top + (12. - band.gain) / 24. * (bottom - top);
            c.text(
                &format!("{:+.0}", band.gain),
                Box2::new(x - band_width / 2., 258., band_width, 24.),
                16.,
                FG,
                true,
                true,
            );
            c.line(&[(x, top), (x, bottom)], 0x585858, 1., 3.);
            c.line(&[(x, (top + bottom) / 2.), (x, y)], 0xdadada, 1., 3.);
            c.circle(x, y, 7., if draft.saving { SUB } else { FG }, 1.);
            let frequency = if band.frequency >= 1000. {
                format!("{:.0}k", band.frequency / 1000.)
            } else {
                format!("{:.0}", band.frequency)
            };
            c.text(
                &frequency,
                Box2::new(x - band_width / 2., 523., band_width, 24.),
                13.,
                SUB,
                false,
                true,
            );
            if !draft.saving {
                hits.push(Hit {
                    rect: Box2::new(x - band_width / 2., top, band_width, bottom - top),
                    action: Action::EqBand(i),
                });
            }
        }
        c.text(
            "Gain: −12 to +12 dB · drag or scroll in steps of 1",
            Box2::new(left, 555., width, 25.),
            13.,
            SUB,
            false,
            false,
        );
        if !draft.saving {
            button(
                c,
                hits,
                v,
                Box2::new(left, 595., 86., 34.),
                "Flat",
                Action::EqFlat,
                false,
            );
            button(
                c,
                hits,
                v,
                Box2::new(left + 92., 595., 86., 34.),
                "Discard",
                Action::EqDiscard,
                false,
            );
        }
        if draft.dirty() && connected && !draft.saving {
            button(
                c,
                hits,
                v,
                Box2::new(left + 184., 595., width - 184., 34.),
                "Save to headset",
                Action::EqSave,
                true,
            );
        }
        c.wrapped_text(
            if draft.saving {
                "Saving and checking each changed band…"
            } else if draft.dirty() {
                "Unsaved tuning. Save writes this custom slot on your headset."
            } else {
                "Tuning loaded from your headset. Saving changes this custom slot."
            },
            Box2::new(left, 640., width, 44.),
            13.,
            SUB,
        );
    } else {
        c.wrapped_text(if selected < 6 { "Factory curves are built into the headset. Select Use EQ to listen, or choose a custom slot to create your own tuning." } else if connected { "Loading the ten bands from your headset. If they do not appear, click Reload." } else { "Connect the Maxwell 2 ANC USB dongle and turn on your headset to load its saved tuning." }, Box2::new(left, 280., width, 130.), 18., SUB);
        if selected >= 6 {
            button(
                c,
                hits,
                v,
                Box2::new(left, 420., 120., 36.),
                "Reload",
                Action::EqReload,
                false,
            );
        }
    }
    if connected {
        button(
            c,
            hits,
            v,
            Box2::new(left, 680., width, 32.),
            if v.sound.eq_index == Some(selected) {
                "Active EQ"
            } else {
                "Use EQ"
            },
            Action::EqUse(selected),
            v.sound.eq_index == Some(selected),
        );
    }
}

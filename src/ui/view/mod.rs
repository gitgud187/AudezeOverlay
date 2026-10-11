//! Pure rendering from an immutable View into pixels and hit targets.
mod cards;
mod equalizer;
mod settings;
use super::draw::{Box2, Canvas, Painter};
use audeze_overlay::{config::Config, layout, model::SoundState};
use cards::render_card;
use settings::settings_page;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    None,
    Close,
    Settings,
    Edit,
    Back,
    Tab(usize),
    Mix,
    Side,
    Level,
    Anc(u8),
    Noise(u8),
    Setting(&'static str, i32),
    Slider(&'static str),
    Card(usize),
    ResizeCard(usize),
    CardMenu(usize),
    AddCard,
    StatusMenu,
    RemoveCard(usize),
    RestoreCard(usize),
    Record,
    EndRecord,
    DiscardRecord,
    NotificationOptions,
    Number(&'static str),
    ResetApp,
    EqMenu,
    EqProfiles,
    EqSaveNew,
    EqEditor,
    EqInspect(u8),
    EqUse(u8),
    EqPin(u8),
    EqBand(usize),
    EqSave,
    EqDiscard,
    EqFlat,
    EqRename,
    EqReload,
}
#[derive(Clone, Copy)]
pub struct Hit {
    pub rect: Box2,
    pub action: Action,
}
pub struct View<'a> {
    pub config: &'a Config,
    pub eq_editor: Option<&'a audeze_overlay::eq::Editor>,
    pub sound: &'a SoundState,
    pub settings: Option<usize>,
    pub edit: bool,
    pub hover: Action,
    pub note: &'a str,
    pub recording: bool,
    pub notification_options: bool,
    pub popup_cards: Option<u8>,
    pub recorded_key: Option<(u32, u32)>,
    pub number_edit: Option<&'a audeze_overlay::input::NumberDraft>,
    pub mix: f32,
    pub demo: bool,
    pub monitor_label: &'a str,
    pub drag_preview: Option<(usize, Box2)>,
    pub animation: Option<&'a super::presentation::AnimationFrame>,
}
pub struct Scene {
    pub pix: tiny_skia::Pixmap,
    pub hits: Vec<Hit>,
    pub cards: [Box2; 6],
}
const FG: u32 = 0xf6f6f6;
const SUB: u32 = 0xbdbdbd;
pub fn slider(c: &Config, key: &str) -> (f32, f32, f32) {
    match key {
        "scale" => (c.cell_width * 1200. / 596., 85., 200.),
        "text" => (c.text_scale as f32, 100., 140.),
        "background" => (c.background_opacity as f32, 0., 100.),
        "opacity" => (c.card_opacity as f32, 10., 100.),
        "blur" => (c.blur_strength as f32, 0., 100.),
        "hold" => (c.duration_ms as f32, 500., 10000.),
        "fade" => (c.fade_ms as f32, 0., 1000.),
        "battery" => (c.low_battery as f32, 0., 50.),
        _ => (0., 0., 100.),
    }
}
fn button(
    c: &mut Canvas,
    h: &mut Vec<Hit>,
    v: &View,
    r: Box2,
    label: &str,
    a: Action,
    selected: bool,
) {
    c.rect(
        r,
        if selected {
            0xebebeb
        } else if v.hover == a {
            0x424242
        } else {
            0x2b2b2b
        },
        1.,
        6.,
    );
    c.fitted_text(
        label,
        Box2::new(r.x + 6., r.y + (r.h - 18.) / 2., r.w - 12., 24.),
        16.,
        if selected { 0x151515 } else { FG },
        true,
        true,
    );
    h.push(Hit { rect: r, action: a });
}
fn switch(c: &mut Canvas, h: &mut Vec<Hit>, r: Box2, on: bool, a: Action) {
    c.rect(r, if on { 0xd8d8d8 } else { 0x555555 }, 1., r.h / 2.);
    c.circle(
        r.x + if on { r.w - r.h / 2. } else { r.h / 2. },
        r.y + r.h / 2.,
        r.h / 2. - 4.,
        if on { 0x252525 } else { 0xe6e6e6 },
        1.,
    );
    h.push(Hit { rect: r, action: a });
}
pub fn card_boxes(config: &Config, _w: f32, _h: f32) -> [Box2; 6] {
    let (cw, ch) = (config.cell_width, config.cell_height);
    config.cards.map(|r| {
        Box2::new(
            12. + r.x as f32 * cw + 4.,
            config.card_top() + r.y as f32 * ch + 4.,
            r.w as f32 * cw - 8.,
            r.h as f32 * ch - 8.,
        )
    })
}
pub fn render(painter: &mut Painter, width: u32, height: u32, scale: f32, v: &View) -> Scene {
    render_reusing(painter, width, height, scale, v, None)
}

/// Reuse only an exactly sized raster surface; clear it so removed/transparent
/// content cannot leave pixels from the previous frame. Hit regions are rebuilt.
pub fn render_reusing(
    painter: &mut Painter,
    width: u32,
    height: u32,
    scale: f32,
    v: &View,
    previous: Option<tiny_skia::Pixmap>,
) -> Scene {
    let mut pix = previous
        .filter(|p| p.width() == width && p.height() == height)
        .unwrap_or_else(|| {
            tiny_skia::Pixmap::new(width, height).expect("Validated canvas dimensions")
        });
    pix.fill(tiny_skia::Color::TRANSPARENT);
    let mut c = Canvas {
        pix,
        scale,
        painter,
    };
    let (w, h) = (width as f32 / scale, height as f32 / scale);
    let mut hits = vec![];
    let settings = v.settings.is_some();
    let bg = if v.edit {
        (v.config.background_opacity as f32 / 100.).max(0.18)
    } else {
        v.config.background_opacity as f32 / 100.
    };
    c.rect(Box2::new(0., 0., w, h), 0x101010, bg, 8.);
    if v.popup_cards.is_some() {
        let cards = card_boxes(v.config, w, h);
        for (i, r) in cards.iter().copied().enumerate() {
            if !v.config.cards[i].visible {
                continue;
            }
            c.rect(r, 0x171717, v.config.card_opacity as f32 / 100., 10.);
            render_card(&mut c, &mut hits, v, i, r);
        }
        return Scene {
            pix: c.pix,
            hits: Vec::new(),
            cards,
        };
    }
    // Layered windows pass completely transparent pixels through before hit
    // testing. A one-alpha header gives dragging a continuous, invisible target.
    c.rect(Box2::new(16., 14., w - 190., 46.), 0x101010, 1. / 255., 0.);
    let logo_width = (w - if v.edit { 242. } else { 192. }).clamp(36., 162.);
    if !v.edit || w >= 290. {
        if logo_width < 110. {
            c.logo_mark(20., 24.);
        } else {
            c.logo(16., 36. - logo_width * 0.075, logo_width);
        }
    }
    if v.config.status_icons && !settings {
        cards::status_icons(&mut c, &mut hits, v, w);
    }
    if v.demo {
        c.text(
            "PREVIEW",
            Box2::new(220., 29., 100., 22.),
            14.,
            SUB,
            true,
            false,
        );
    }
    let close = Box2::new(w - 58., 16., 40., 40.);
    button(&mut c, &mut hits, v, close, "", Action::Close, false);
    c.line(
        &[
            (close.x + 13., close.y + 22.),
            (close.x + 27., close.y + 22.),
        ],
        FG,
        1.,
        2.,
    );
    if settings {
        button(
            &mut c,
            &mut hits,
            v,
            Box2::new(w - 190., 16., 120., 40.),
            "Back to sound",
            Action::Back,
            false,
        );
    } else {
        let gear = Box2::new(w - 108., 16., 40., 40.);
        button(&mut c, &mut hits, v, gear, "", Action::Settings, false);
        gear_icon(&mut c, gear.x + 20., gear.y + 20.);
        let edit = Box2::new(w - 158., 16., 40., 40.);
        button(&mut c, &mut hits, v, edit, "", Action::Edit, v.edit);
        grid_icon(&mut c, edit.x + 11., edit.y + 11., v.edit);
        if v.edit {
            let add = Box2::new(w - 208., 16., 40., 40.);
            button(&mut c, &mut hits, v, add, "+", Action::AddCard, false);
        }
    }
    let cards = v
        .animation
        .map(|frame| frame.cards)
        .unwrap_or_else(|| card_boxes(v.config, w, h));
    if let Some(section) = v.settings {
        settings_page(&mut c, &mut hits, v, w, h, section);
    } else {
        if v.edit {
            let (cw, ch) = (v.config.cell_width, v.config.cell_height);
            let (cols, rows) = v.config.canvas_cells(w, h);
            for x in 0..=cols {
                c.line(
                    &[
                        (12. + x as f32 * cw, v.config.card_top()),
                        (12. + x as f32 * cw, h - 16.),
                    ],
                    0xffffff,
                    0.13,
                    1.,
                );
            }
            for y in 0..=rows {
                c.line(
                    &[
                        (12., v.config.card_top() + y as f32 * ch),
                        (w - 12., v.config.card_top() + y as f32 * ch),
                    ],
                    0xffffff,
                    0.13,
                    1.,
                );
            }
        }
        let floating = v.drag_preview.map(|p| p.0);
        let order = (0..cards.len())
            .filter(|i| Some(*i) != floating)
            .chain(floating);
        for i in order {
            let r = v
                .drag_preview
                .filter(|p| p.0 == i)
                .map(|p| p.1)
                .unwrap_or(cards[i]);
            if !v.config.cards[i].visible {
                continue;
            }
            c.rect(r, 0x171717, v.config.card_opacity as f32 / 100., 10.);
            render_card(&mut c, &mut hits, v, i, r);
            if v.edit {
                hits.retain(|hit| {
                    !r.contains(hit.rect.x + hit.rect.w / 2., hit.rect.y + hit.rect.h / 2.)
                });
                hits.push(Hit {
                    rect: r,
                    action: Action::Card(i),
                });
                let corner = Box2::new(r.x + r.w - 25., r.y + r.h - 25., 25., 25.);
                hits.push(Hit {
                    rect: corner,
                    action: Action::ResizeCard(i),
                });
                c.line(
                    &[
                        (r.x + r.w - 19., r.y + r.h - 7.),
                        (r.x + r.w - 7., r.y + r.h - 19.),
                    ],
                    0xffffff,
                    0.8,
                    2.,
                );
                c.line(
                    &[
                        (r.x + r.w - 12., r.y + r.h - 7.),
                        (r.x + r.w - 7., r.y + r.h - 12.),
                    ],
                    0xffffff,
                    0.8,
                    2.,
                );
                let menu = Box2::new(r.x + 5., r.y + 16., 28., 20.);
                c.rect(
                    menu,
                    if v.hover == Action::CardMenu(i) {
                        0x555555
                    } else {
                        0x303030
                    },
                    0.95,
                    5.,
                );
                for n in 0..3 {
                    c.circle(
                        menu.x + 7. + n as f32 * 7.,
                        menu.y + 9.,
                        1.7,
                        0xffffff,
                        0.95,
                    );
                }
                hits.push(Hit {
                    rect: menu,
                    action: Action::CardMenu(i),
                });
            }
        }
        let footer = if !v.note.is_empty() {
            v.note
        } else if v.edit {
            ""
        } else if v.sound.mix.is_none() {
            v.sound.status.as_str()
        } else {
            ""
        };
        if !footer.is_empty() {
            c.rect(Box2::new(16., h - 29., w - 32., 26.), 0x111111, 0.92, 6.);
            c.text(
                footer,
                Box2::new(24., h - 25., w - 48., 22.),
                14.,
                FG,
                false,
                false,
            );
        }
    }
    Scene {
        pix: c.pix,
        hits,
        cards,
    }
}
fn gear_icon(c: &mut Canvas, x: f32, y: f32) {
    c.arc(x, y, 7., 0., 360., FG, 1., 1.7);
    c.arc(x, y, 2.5, 0., 360., FG, 1., 1.5);
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::PI / 4.;
        c.line(
            &[
                (x + 8. * a.cos(), y + 8. * a.sin()),
                (x + 11. * a.cos(), y + 11. * a.sin()),
            ],
            FG,
            1.,
            2.,
        );
    }
}
fn grid_icon(c: &mut Canvas, x: f32, y: f32, on: bool) {
    for yy in 0..2 {
        for xx in 0..2 {
            c.rect(
                Box2::new(x + xx as f32 * 11., y + yy as f32 * 11., 7., 7.),
                if on { 0x111111 } else { FG },
                1.,
                1.,
            );
        }
    }
}

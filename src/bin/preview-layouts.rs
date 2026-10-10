//! Offline rendering diagnostics. Does not create a window or connect to hardware.
#![allow(dead_code)]
#[path = "../ui/view/mod.rs"]
mod dashboard;
#[path = "../ui/draw.rs"]
mod draw;
#[path = "../ui/presentation.rs"]
mod presentation;

use audeze_overlay::{config::Config, layout, model::SoundState};
use dashboard::{Action, View};
use draw::Painter;

fn main() {
    std::fs::create_dir_all("diagnostics").expect("diagnostics directory");
    let mut painter = Painter::new();
    let sound = SoundState {
        mix_revision: 0,
        eq_index: Some(0),
        eq_bands: None,
        eq_revision: 0,
        eq_index_revision: 0,
        mix: Some((50, 50)),
        sidetone: Some(false),
        sidetone_level: Some(26),
        anc: Some("Transparency".into()),
        noise: Some("Off".into()),
        battery: Some(69),
        muted: Some(true),
        device: Some("Maxwell 2 ANC".into()),
        connection: Some("USB Dongle".into()),
        status: String::new(),
    };
    let base = Config {
        width: 520,
        height: 500,
        cell_width: 496. / 12.,
        cell_height: 394. / 15.,
        ..Config::default()
    };
    let mut tall = base.clone();
    tall.height = 723;
    tall.cell_height = 617. / 15.;
    let mut swapped = base.clone();
    let proposed = layout::Card {
        x: 6,
        ..swapped.cards[0]
    };
    swapped.cards = layout::preview_move(&base.cards, 0, proposed, (9., 2.), 15)
        .expect("mix swaps into the small card column");
    let mut narrow = base.clone();
    narrow.cards[2].w = 5;
    let mut narrow_mix = swapped.clone();
    narrow_mix.cards[0].w = 5;
    let mut minimum = Config {
        width: 440,
        height: 440,
        cell_width: 40.,
        cell_height: 26.,
        status_icons: true,
        ..Config::default()
    };
    minimum.cards[0] = layout::Card {
        x: 0,
        y: 0,
        w: 5,
        h: 8,
        visible: true,
    };
    for i in 1..6 {
        minimum.cards[i] = layout::Card {
            x: 5,
            y: (i as i32 - 1) * 3,
            w: 5,
            h: 3,
            visible: true,
        };
    }
    minimum.cards[4].x = 0;
    minimum.cards[4].y = 8;
    minimum.cards[5].y = 9;
    minimum.resize_canvas(440, 440);
    let mut column = minimum.clone();
    let mut y = 0;
    for card in &mut column.cards {
        card.x = 0;
        card.y = y;
        y += card.h;
    }
    column.trim_canvas();
    let mut large_text = minimum.clone();
    large_text.text_scale = 140;
    for (name, config) in [
        ("single-column-status", column),
        ("minimum-cards", minimum),
        ("minimum-large-text", large_text),
        ("compact", base),
        ("compact-tall", tall),
        ("swapped", swapped),
        ("compact-five-columns", narrow),
        ("swapped-five-columns", narrow_mix),
    ] {
        let view = View {
            eq_editor: None,
            config: &config,
            sound: &sound,
            settings: None,
            edit: false,
            hover: Action::None,
            note: "",
            recording: false,
            recorded_key: None,
            number_edit: None,
            mix: 50.,
            demo: false,
            monitor_label: "Automatic",
            drag_preview: None,
            animation: None,
        };
        let scene = dashboard::render(&mut painter, config.width, config.height, 1., &view);
        let buttons: Vec<_> = scene
            .hits
            .iter()
            .filter(|h| matches!(h.action, Action::Anc(_)))
            .collect();
        assert_eq!(buttons.len(), 3, "{name}: every ANC mode has a hit target");
        let card = scene.cards[2];
        let padding = if card.h < 110. { 7.9 } else { 15.9 };
        for (i, hit) in buttons.iter().enumerate() {
            let rect = hit.rect;
            assert!(
                rect.x >= card.x + padding && rect.y >= card.y + 30.,
                "{name}: button inside card"
            );
            assert!(
                rect.x + rect.w <= card.x + card.w - padding,
                "{name}: button fits width"
            );
            assert!(
                rect.y + rect.h <= card.y + card.h - padding,
                "{name}: button fits height"
            );
            for other in &buttons[i + 1..] {
                assert!(
                    rect.x + rect.w <= other.rect.x
                        || other.rect.x + other.rect.w <= rect.x
                        || rect.y + rect.h <= other.rect.y
                        || other.rect.y + other.rect.h <= rect.y,
                    "{name}: ANC buttons do not overlap"
                );
            }
        }
        let path = format!("diagnostics/layout-{name}.png");
        scene.pix.save_png(&path).expect("save preview PNG");
        println!("{path}: ANC bounds and nonoverlap verified");
    }
    let config = Config::default();
    let mut eq_editor = audeze_overlay::eq::Editor {
        selected: 6,
        ..Default::default()
    };
    eq_editor.receive(audeze_overlay::eq::sample(6), 0);
    let draft = audeze_overlay::input::NumberDraft::new("hold", 797);
    for (name, section) in [
        ("equalizer", Some(5)),
        ("cards-settings", Some(1)),
        ("editor", None),
        ("notifications", Some(2)),
        ("shortcuts", Some(3)),
        ("reset", Some(4)),
    ] {
        let view = View {
            eq_editor: Some(&eq_editor),
            config: &config,
            sound: &sound,
            settings: section,
            edit: section.is_none(),
            hover: Action::None,
            note: "",
            recording: section == Some(3),
            recorded_key: Some((2, 0x24)),
            number_edit: if section == Some(2) {
                Some(&draft)
            } else {
                None
            },
            mix: 50.,
            demo: false,
            monitor_label: "Automatic",
            drag_preview: None,
            animation: None,
        };
        let (width, height) = if section.is_some() {
            (900, 750)
        } else {
            (config.width, config.height)
        };
        dashboard::render(&mut painter, width, height, 1., &view)
            .pix
            .save_png(format!("diagnostics/layout-{name}.png"))
            .unwrap();
    }
    let mut motion = presentation::Presentation::default();
    let now = std::time::Instant::now();
    let mut before = sound.clone();
    before.anc = Some("On".into());
    let boxes = dashboard::card_boxes(&config, config.width as f32, config.height as f32);
    motion.frame(boxes, &before, false, true, now);
    let mut after = before.clone();
    after.mix = Some((20, 80));
    after.anc = Some("Transparency".into());
    for millis in [0, 45, 90, 160, 300] {
        let frame = motion.frame(
            boxes,
            &after,
            true,
            true,
            now + std::time::Duration::from_millis(millis),
        );
        let view = View {
            eq_editor: None,
            config: &config,
            sound: &after,
            settings: None,
            edit: false,
            hover: Action::Mix,
            note: "",
            recording: false,
            recorded_key: None,
            number_edit: None,
            mix: 80.,
            demo: false,
            monitor_label: "Automatic",
            drag_preview: None,
            animation: Some(&frame),
        };
        dashboard::render(&mut painter, config.width, config.height, 1., &view)
            .pix
            .save_png(format!("diagnostics/layout-motion-{millis}.png"))
            .unwrap();
        if millis == 300 {
            assert!(!frame.active, "Animations must stop when idle");
        }
    }
}

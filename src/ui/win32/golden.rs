//! Behavior checks for the logic that moves between modules, plus an optional
//! transcript. Part of `--smoke-test`; uses demo state and never writes user
//! preferences or touches the headset.
//!
//! The explicit checks here cover input paths the other smoke checks do not:
//! the wheel, keyboard drafts, Escape, window hit-testing and status changes.
//! With `--smoke-golden` the same scripted scenario also records the full
//! application state after every step, so behavior before and after a refactor
//! can be compared line by line (`diagnostics/ui-golden.txt`, with the complete
//! state text in `ui-golden-full.txt`).
use super::*;
use crate::ui::draw::Box2;

fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn drag_name(drag: &Drag) -> String {
    match drag {
        Drag::None => "none".into(),
        Drag::Mix => "mix".into(),
        Drag::Level => "level".into(),
        Drag::Setting(key) => format!("setting:{key}"),
        Drag::EqBand(index) => format!("eq-band:{index}"),
        Drag::Card { index, resize, .. } => format!("card:{index}:{resize}"),
    }
}

/// Every piece of application and window state that behavior can change.
unsafe fn state_text(app: &App) -> String {
    let round = |value: f32| (value * 100.).round() / 100.;
    let mut window = RECT::default();
    let _ = GetWindowRect(win(app).hwnd, &mut window);
    let hits: Vec<String> = app
        .hits
        .iter()
        .map(|h| {
            format!(
                "{:?}@{},{},{},{}",
                h.action,
                round(h.rect.x),
                round(h.rect.y),
                round(h.rect.w),
                round(h.rect.h)
            )
        })
        .collect();
    let cards: Vec<String> = app
        .cards
        .iter()
        .map(|b| {
            format!(
                "{},{},{},{}",
                round(b.x),
                round(b.y),
                round(b.w),
                round(b.h)
            )
        })
        .collect();
    let banks: Vec<String> = app
        .eq_editor
        .banks
        .iter()
        .map(|bank| match bank {
            Some(d) => format!(
                "{}:{}:{}:{}:{}",
                d.dirty(),
                d.saving,
                d.previewing,
                d.touched,
                serde_json::to_string(&d.value).unwrap_or_default()
            ),
            None => "-".into(),
        })
        .collect();
    format!(
        "config={}\nsound={}\nui settings={:?} edit={} interactive={} visible={} recording={} \
recorded={:?} options={} popup={:?} latched={} hotkey={} note={:?} number={:?} drag={} \
mix_to={} pending={:?} wheel={}\neq selected={} preview={:?} name={:?} name_selected={} \
banks={:?}\nwindow={},{},{},{}\nhits={:?}\ncards={:?}\n",
        serde_json::to_string(&app.config).unwrap_or_default(),
        serde_json::to_string(&app.sound).unwrap_or_default(),
        app.settings,
        app.edit,
        app.interactive,
        IsWindowVisible(win(app).hwnd).as_bool(),
        app.recording,
        app.recorded_key,
        app.notification_options,
        app.popup_cards,
        app.low_latched,
        app.hotkey_registered,
        app.note.text(),
        app.number_edit,
        drag_name(&app.drag),
        round(app.mix_to),
        app.pending_mix,
        app.wheel,
        app.eq_editor.selected,
        app.eq_editor.preview_slot,
        app.eq_editor.name_edit,
        app.eq_editor.name_selected,
        banks,
        window.left,
        window.top,
        window.right,
        window.bottom,
        hits,
        cards,
    )
}

/// Starts every scenario from the same state, whatever earlier checks left behind.
unsafe fn reset_state(app: &mut App) {
    app.config = Config::default();
    app.sound = sample();
    *app.latest.lock().unwrap() = sample();
    app.eq_editor = Default::default();
    app.settings = None;
    app.edit = false;
    app.recording = false;
    app.recorded_key = None;
    app.number_edit = None;
    app.popup_cards = None;
    app.note.clear();
    app.drag = Drag::None;
    app.mix_from = 50.;
    app.mix_to = 50.;
    app.mix_started = None;
    app.pending_mix = None;
    app.low_latched = false;
    app.wheel = 0;
    app.notification_options = false;
    app.hover = Action::None;
    app.alpha = 255;
    app.fade_in = None;
    app.fade_out = None;
    app.demo = true;
    show(app, true);
    render(app);
}

fn area(app: &App, action: Action) -> Box2 {
    app.hits
        .iter()
        .find(|h| h.action == action)
        .map(|h| h.rect)
        .unwrap_or_default()
}
fn center(b: Box2) -> (f32, f32) {
    (b.x + b.w / 2., b.y + b.h / 2.)
}

/// Window-message coordinates for a point given in logical (unscaled) units,
/// the inverse of `local`.
unsafe fn screen_point(app: &App, x: f32, y: f32) -> LPARAM {
    let mut client = RECT::default();
    let _ = GetClientRect(win(app).hwnd, &mut client);
    let scale = if app.settings.is_some() {
        (client.right as f32 / 900.).min(client.bottom as f32 / 750.)
    } else {
        GetDpiForWindow(win(app).hwnd).max(96) as f32 / 96.
    };
    let mut point = POINT {
        x: (x * scale).round() as i32,
        y: (y * scale).round() as i32,
    };
    let _ = ClientToScreen(win(app).hwnd, &mut point);
    LPARAM(((point.x as u16 as u32) | ((point.y as u16 as u32) << 16)) as isize)
}
unsafe fn client_point(app: &App, x: f32, y: f32) -> LPARAM {
    let scale = GetDpiForWindow(win(app).hwnd).max(96) as f32 / 96.;
    let (px, py) = ((x * scale).round() as i32, (y * scale).round() as i32);
    LPARAM(((px as u16 as u32) | ((py as u16 as u32) << 16)) as isize)
}
unsafe fn wheel(app: &mut App, x: f32, y: f32, notches: i32) {
    let l = screen_point(app, x, y);
    let delta = (notches * 120) as i16 as u16 as usize;
    let _ = events::dispatch(app, win(app).hwnd, WM_MOUSEWHEEL, WPARAM(delta << 16), l);
}
unsafe fn key(app: &mut App, vk: u32) {
    let _ = events::dispatch(
        app,
        win(app).hwnd,
        WM_KEYDOWN,
        WPARAM(vk as usize),
        LPARAM(0),
    );
}
unsafe fn typed(app: &mut App, text: &str) {
    for ch in text.chars() {
        let _ = events::dispatch(app, win(app).hwnd, WM_CHAR, WPARAM(ch as usize), LPARAM(0));
    }
}
unsafe fn hit_test(app: &mut App, x: f32, y: f32) -> isize {
    let l = screen_point(app, x, y);
    events::dispatch(app, win(app).hwnd, WM_NCHITTEST, WPARAM(0), l)
        .map(|r| r.0)
        .unwrap_or(-1)
}
unsafe fn status_changed(app: &mut App, next: SoundState) {
    *app.latest.lock().unwrap() = next;
    let _ = events::dispatch(app, win(app).hwnd, STATUS_MESSAGE, WPARAM(0), LPARAM(0));
}

pub(super) unsafe fn run(app: &mut App) {
    let golden = std::env::args().any(|a| a == "--smoke-golden");
    let mut log: Vec<(String, String)> = Vec::new();
    macro_rules! step {
        ($name:expr) => {
            step!($name, String::new())
        };
        ($name:expr, $extra:expr) => {{
            if golden {
                let text = format!("{}{}", state_text(app), $extra);
                log.push(($name.to_string(), text));
            }
        }};
    }
    macro_rules! check {
        ($name:expr, $ok:expr) => {{
            let ok = $ok;
            app.checks.push(($name.into(), ok));
        }};
    }

    reset_state(app);
    step!("initial");

    // ---- Pointer wheel and double click on the dashboard.
    let (mx, my) = center(area(app, Action::Mix));
    wheel(app, mx, my, 3);
    check!("wheel_moves_mix_five_points_per_notch", app.mix_to == 65.);
    step!("wheel_mix_up_3");
    wheel(app, mx, my, -1);
    check!("wheel_down_moves_mix_back", app.mix_to == 60.);
    step!("wheel_mix_down_1");
    wheel(app, mx, my, 40);
    check!("wheel_mix_stops_at_chat_limit", app.mix_to == 100.);
    step!("wheel_mix_clamped_high");
    let l = client_point(app, mx, my);
    let _ = events::dispatch(app, win(app).hwnd, WM_LBUTTONDBLCLK, WPARAM(0), l);
    check!("double_click_returns_mix_to_even", app.mix_to == 50.);
    step!("double_click_mix");

    // ---- Controls send commands (preview mode updates the displayed state).
    act(app, Action::Side, 0., 0.);
    check!(
        "sidetone_button_toggles_in_preview",
        app.sound.sidetone == Some(true)
    );
    step!("sidetone_on");
    render(app);
    let level = area(app, Action::Level);
    act(app, Action::Level, level.x + level.w * 0.5, level.y + 4.);
    check!(
        "sidetone_slider_press_sets_level",
        app.sound.sidetone_level.is_some_and(|level| level != 26)
    );
    app.drag = Drag::None;
    let _ = ReleaseCapture();
    step!("sidetone_level_drag");
    wheel(app, level.x + level.w * 0.5, level.y + 4., 2);
    step!("wheel_sidetone_level");
    act(app, Action::Anc(0), 0., 0.);
    check!(
        "anc_button_selects_mode",
        app.sound.anc.as_deref() == Some("On")
    );
    step!("anc_on");
    act(app, Action::Anc(2), 0., 0.);
    step!("anc_transparency");
    act(app, Action::Noise(2), 0., 0.);
    check!(
        "noise_button_selects_level",
        app.sound.noise.as_deref() == Some("High")
    );
    step!("noise_high");

    // ---- Mix drag across the dial and as a slider.
    act(app, Action::Mix, mx + 60., my);
    app.drag = Drag::None;
    let _ = ReleaseCapture();
    step!("mix_dial_press");

    // ---- Settings pages: every adjustable value in both directions.
    act(app, Action::Settings, 0., 0.);
    check!("settings_open_from_dashboard", app.settings.is_some());
    step!("settings_open");
    for key_name in [
        "scale",
        "text",
        "background",
        "opacity",
        "status-icons",
        "animations",
        "preserve-space",
        "changed-card-only",
        "automatic",
        "blur",
        "notify0",
        "notify1",
        "notify2",
        "notify3",
        "notify4",
        "rows",
        "snap",
        "lock",
        "hold",
        "fade",
        "battery",
        "startup",
        "monitor",
    ] {
        setting(app, key_name, 1);
        step!(format!("setting_{key_name}_up"));
        setting(app, key_name, -1);
        step!(format!("setting_{key_name}_down"));
    }
    for (key_name, value) in [
        ("scale", 700.),
        ("text", 120.),
        ("background", 33.),
        ("opacity", 88.),
        ("blur", 80.),
        ("rows", 30.),
        ("hold", 4000.),
        ("fade", 400.),
        ("battery", 25.),
    ] {
        slider_value(app, key_name, value);
        step!(format!("slider_{key_name}"));
    }
    setting(app, "reset-layout", 0);
    step!("reset_layout");

    // ---- Typed numbers: Enter commits, Escape cancels, bad input stays open.
    act(app, Action::Number("hold"), 0., 0.);
    typed(app, "1200");
    key(app, VK_RETURN.0 as u32);
    check!("typed_number_enter_commits", app.config.duration_ms == 1200);
    step!("number_enter");
    act(app, Action::Number("fade"), 0., 0.);
    typed(app, "900");
    key(app, VK_ESCAPE.0 as u32);
    check!(
        "typed_number_escape_cancels",
        app.number_edit.is_none() && app.config.fade_ms != 900
    );
    step!("number_escape");
    act(app, Action::Number("hold"), 0., 0.);
    typed(app, "5");
    key(app, VK_RETURN.0 as u32);
    check!(
        "invalid_typed_number_keeps_editing_with_a_message",
        app.number_edit.is_some() && !app.note.is_empty()
    );
    step!("number_invalid");
    let _ = events::dispatch(app, win(app).hwnd, WM_KILLFOCUS, WPARAM(0), LPARAM(0));
    step!("number_focus_lost");

    // ---- Equalizer editor: gestures, wheel, naming, quick switch.
    act(app, Action::EqEditor, 0., 0.);
    act(app, Action::EqInspect(7), 0., 0.);
    render(app);
    step!("eq_open_slot_7");
    for (band, gain) in [(0usize, -3.), (3, 4.), (9, 6.5)] {
        preview_eq_gain(app, band, gain);
    }
    step!("eq_preview_bands");
    let (bx, by) = center(area(app, Action::EqBand(3)));
    wheel(app, bx, by, -2);
    step!("eq_wheel_band");
    act(app, Action::EqFlat, 0., 0.);
    step!("eq_flat");
    act(app, Action::EqDiscard, 0., 0.);
    step!("eq_discard");
    preview_eq_gain(app, 5, -4.);
    act(app, Action::EqSave, 0., 0.);
    step!("eq_save");
    act(app, Action::EqRename, 0., 0.);
    typed(app, "Night");
    key(app, VK_RETURN.0 as u32);
    check!(
        "eq_name_typing_and_enter_commit",
        app.config.eq.name(7) == "Night"
    );
    step!("eq_rename");
    act(app, Action::EqPin(7), 0., 0.);
    act(app, Action::EqUse(8), 0., 0.);
    step!("eq_pin_and_use");

    // ---- Layout editing: move, resize, remove, restore.
    app.settings = None;
    show(app, true);
    render(app);
    act(app, Action::Edit, 0., 0.);
    render(app);
    step!("layout_edit_on");
    let (sx, sy) = center(app.cards[1]);
    let (tx, ty) = center(app.cards[2]);
    act(app, Action::Card(1), sx, sy);
    drag(app, tx, ty);
    step!("layout_drag_card");
    app.drag = Drag::None;
    let _ = ReleaseCapture();
    act(app, Action::ResizeCard(2), 0., 0.);
    drag(app, 30., 20.);
    step!("layout_resize_card");
    app.drag = Drag::None;
    let _ = ReleaseCapture();
    act(app, Action::RemoveCard(3), 0., 0.);
    step!("layout_remove_card");
    act(app, Action::RestoreCard(3), 0., 0.);
    step!("layout_restore_card");
    for (name, xf, yf) in [
        ("top_left", 2., 2.),
        ("top_right", -2., 2.),
        ("bottom_left", 2., -2.),
        ("bottom_right", -2., -2.),
        ("left", 2., 0.5),
        ("right", -2., 0.5),
        ("bottom", 0.5, -2.),
        ("header", 0.5, 30.),
        ("client", 0.5, 0.5),
    ] {
        let (w, h) = logical_size(app);
        let x = if xf < 0. {
            w + xf
        } else if xf < 1. {
            w * xf
        } else {
            xf
        };
        let y = if yf < 0. {
            h + yf
        } else if yf < 1. {
            h * yf
        } else {
            yf
        };
        let code = hit_test(app, x, y);
        step!(format!("hit_test_edit_{name}"), format!("code={code}\n"));
    }
    check!("edit_mode_corners_resize_the_window", {
        let (w, h) = logical_size(app);
        hit_test(app, 2., 2.) == HTTOPLEFT as isize
            && hit_test(app, w - 2., h - 2.) == HTBOTTOMRIGHT as isize
    });
    key(app, VK_ESCAPE.0 as u32);
    check!("escape_finishes_layout_editing", !app.edit);
    step!("escape_ends_layout");
    check!(
        "header_hit_is_caption_when_unlocked",
        hit_test(app, 210., 35.) == HTCAPTION as isize
    );
    app.config.locked = true;
    check!(
        "locked_window_has_no_caption",
        hit_test(app, 210., 35.) == HTCLIENT as isize
    );
    app.config.locked = false;
    let was_interactive = app.interactive;
    key(app, VK_ESCAPE.0 as u32);
    check!(
        "escape_closes_the_open_panel",
        was_interactive && !IsWindowVisible(win(app).hwnd).as_bool()
    );
    step!("escape_hides_panel");

    // ---- Status changes from the headset: popups, alerts, disconnects.
    reset_state(app);
    let mut next = sample();
    next.mix = Some((30, 70));
    app.interactive = false;
    status_changed(app, next);
    step!("status_mix_changed");
    let mut next = sample();
    next.mix = Some((30, 70));
    next.battery = Some(12);
    app.config.low_battery = 20;
    status_changed(app, next);
    check!(
        "low_battery_alert_appears_once",
        app.note.text().starts_with("Low headset battery") && app.low_latched
    );
    step!("status_low_battery");
    let mut next = sample();
    next.mix = Some((30, 70));
    next.battery = Some(10);
    app.note.clear();
    status_changed(app, next);
    check!("low_battery_alert_does_not_repeat", app.note.is_empty());
    step!("status_low_battery_again");
    let mut next = sample();
    next.mix = None;
    next.battery = None;
    status_changed(app, next);
    step!("status_disconnected");
    let mut next = sample();
    next.family = Some(audeze_overlay::model::HeadsetFamily::Maxwell);
    next.capabilities = Some(audeze_overlay::model::Capabilities {
        anc: false,
        eq_edit: false,
    });
    status_changed(app, next);
    step!("status_other_model");
    reset_state(app);

    // ---- Window position and shortcut drafts.
    app.config.position = Some((40, 50));
    position(app);
    step!("position_explicit");
    reset(app);
    step!("position_reset");
    act(app, Action::Record, 0., 0.);
    app.recorded_key = Some((2, 0x24));
    step!("record_draft");
    act(app, Action::DiscardRecord, 0., 0.);
    step!("record_discarded");
    factory_reset(app);
    step!("factory_reset");

    if golden {
        let _ = std::fs::create_dir_all("diagnostics");
        let summary: String = log
            .iter()
            .map(|(name, text)| format!("{name}\t{:016x}\n", fnv1a(text)))
            .collect();
        let full: String = log
            .iter()
            .map(|(name, text)| format!("=== {name}\n{text}\n"))
            .collect();
        let _ = std::fs::write("diagnostics/ui-golden.txt", summary);
        let _ = std::fs::write("diagnostics/ui-golden-full.txt", full);
    }
}

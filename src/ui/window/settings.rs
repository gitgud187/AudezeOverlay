//! Settings actions, input drafts, hotkey registration and factory reset.
use super::*;
pub(super) unsafe fn confirm_recording(app: &mut App) -> bool {
    if !app.recording {
        return true;
    }
    let choice = MessageBoxW(Some(app.hwnd), w!("Save the recorded shortcut before leaving?\nYes: save. No: discard. Cancel: keep recording."), w!("Unsaved shortcut"), MB_YESNOCANCEL | MB_ICONQUESTION);
    if choice == IDYES {
        finish_recording(app);
        !app.recording
    } else if choice == IDNO {
        app.recording = false;
        app.recorded_key = None;
        true
    } else {
        false
    }
}
pub(super) unsafe fn commit_number(app: &mut App) -> bool {
    let Some(draft) = app.number_edit.as_ref() else {
        return true;
    };
    let (_, min, max) = dashboard::slider(&app.config, draft.key);
    match draft.value(min as u32, max as u32) {
        Ok(value) => {
            let key = draft.key;
            app.number_edit = None;
            apply_numeric(app, key, value as f32);
            app.note.clear();
            save(app);
            true
        }
        Err(message) => {
            app.note = message;
            request_draw(app);
            false
        }
    }
}
pub(super) unsafe fn factory_reset(app: &mut App) {
    if !app.demo {
        if let Err(message) = startup(false) {
            app.note = message;
            return;
        }
    }
    let _ = UnregisterHotKey(Some(app.hwnd), app.hotkey_id);
    app.config = Config::default();
    app.eq_editor = audeze_overlay::eq::Editor::default();
    app.hotkey_registered = RegisterHotKey(
        Some(app.hwnd),
        app.hotkey_id,
        HOT_KEY_MODIFIERS(app.config.hotkey_mod) | MOD_NOREPEAT,
        app.config.hotkey_vk,
    )
    .is_ok();
    app.recording = false;
    app.recorded_key = None;
    app.number_edit = None;
    app.edit = false;
    app.low_latched = false;
    app.popup_cards = None;
    app.notification_options = false;
    app.settings = Some(if app.hotkey_registered { 0 } else { 3 });
    app.config.settings_page = app.settings.unwrap();
    app.note = if app.hotkey_registered {
        "App preferences reset. Headset sound is unchanged."
    } else {
        "App reset. Alt + Home is in use; record another shortcut."
    }
    .into();
    save(app);
    show(app, true);
}
pub(super) unsafe fn set_slider(app: &mut App, key: &'static str, x: f32) {
    let Some(hit) = app.hits.iter().find(|h| h.action == Action::Slider(key)) else {
        return;
    };
    let (_, min, max) = dashboard::slider(&app.config, key);
    let f = ((x - hit.rect.x - 10.) / (hit.rect.w - 20.)).clamp(0., 1.);
    let value = ((min + f * (max - min)) / 5.).round() * 5.;
    slider_value(app, key, value);
}
pub(super) unsafe fn slider_value(app: &mut App, key: &'static str, value: f32) {
    apply_numeric(app, key, (value / 5.).round() * 5.);
}
pub(super) unsafe fn apply_numeric(app: &mut App, key: &'static str, value: f32) {
    let c = &mut app.config;
    match key {
        "scale" => {
            let ratio = value / (c.cell_width * 1200. / 596.);
            c.height = (c.vertical_padding() + (c.height as f32 - c.vertical_padding()) * ratio)
                .round() as u32;
            c.width = (24. + (c.width as f32 - 24.) * ratio).round() as u32;
            c.cell_width *= ratio;
            c.cell_height *= ratio;
            c.migrate_layout();
        }
        "text" => c.text_scale = value as u32,
        "background" => c.background_opacity = value as u8,
        "opacity" => c.card_opacity = value as u8,
        "blur" => {
            c.blur_strength = value.clamp(0., 100.) as u8;
            c.background_blur = c.blur_strength > 0;
            let _ = crate::platform::blur::apply_strength(app.hwnd, c.blur_strength);
            app.applied_blur = Some(c.blur_strength);
        }
        "rows" => {
            let min = c
                .cards
                .iter()
                .filter(|r| r.visible)
                .map(|r| r.y + r.h)
                .max()
                .unwrap_or(5)
                .max(5);
            c.grid_rows = (value as i32).max(((min + 4) / 5) * 5);
            c.height = (c.vertical_padding() + c.grid_rows as f32 * c.cell_height).ceil() as u32;
        }
        "hold" => c.duration_ms = value as u32,
        "fade" => c.fade_ms = value as u32,
        "battery" => {
            c.low_battery = value as u8;
            app.low_latched = false;
        }
        _ => (),
    };
    c.fit_layout();
    request_draw(app);
}
pub(super) unsafe fn select_monitor(app: &mut App) {
    let Ok(menu) = CreatePopupMenu() else { return };
    let _ = AppendMenuW(menu, MF_STRING, 1, w!("Auto — active screen"));
    for (i, m) in app.monitors.iter().enumerate() {
        let _ = AppendMenuW(menu, MF_STRING, i + 2, PCWSTR(wide(&m.label).as_ptr()));
    }
    let mut p = POINT::default();
    let _ = GetCursorPos(&mut p);
    let id = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY,
        p.x,
        p.y,
        None,
        app.hwnd,
        None,
    )
    .0;
    if id > 0 {
        app.config.monitor = if id == 1 {
            String::new()
        } else {
            app.monitors[(id - 2) as usize].key.clone()
        };
        app.config.position = None;
    }
    let _ = DestroyMenu(menu);
}
pub(super) unsafe fn setting(app: &mut App, key: &str, n: i32) {
    if matches!(
        key,
        "card0" | "card1" | "card2" | "card3" | "card4" | "card5"
    ) {
        let i = key.as_bytes()[4] as usize - b'0' as usize;
        card_visibility(app, i, !app.config.cards[i].visible);
        return;
    }
    if key == "monitor" && n == 0 {
        select_monitor(app);
        save(app);
        return;
    }
    let c = &mut app.config;
    match key {
        "scale" => {
            let factor = if n > 0 { 1.1 } else { 1. / 1.1 };
            c.width = (c.width as f32 * factor).round().clamp(440., 1600.) as u32;
            c.height = (c.height as f32 * factor).round().clamp(400., 1400.) as u32;
            c.cell_width *= factor;
            c.cell_height *= factor;
            c.migrate_layout();
        }
        "text" => c.text_scale = (c.text_scale as i32 + n * 5).clamp(100, 140) as u32,
        "background" => {
            c.background_opacity = (c.background_opacity as i32 + n * 5).clamp(0, 100) as u8
        }
        "opacity" => c.card_opacity = (c.card_opacity as i32 + n * 5).clamp(10, 100) as u8,
        "status-icons" => {
            let old_padding = c.vertical_padding();
            c.status_icons = !c.status_icons;
            c.height = (c.height as f32 + c.vertical_padding() - old_padding) as u32;
        }
        "eq-factory" => {
            c.eq.show_factory = !c.eq.show_factory;
            if !c.eq.show_factory && app.eq_editor.selected < 6 {
                inspect_eq(app, 6);
            }
        }
        "animations" => c.animations = !c.animations,
        "preserve-space" => c.preserve_space = !c.preserve_space,
        "changed-card-only" => c.changed_card_only = !c.changed_card_only,
        "automatic" => c.auto_show = !c.auto_show,
        "blur" => {
            c.background_blur = !c.background_blur;
            let _ = crate::platform::blur::apply(app.hwnd, c.background_blur);
            c.blur_strength = if c.background_blur { 50 } else { 0 };
            app.applied_blur = Some(c.blur_strength);
        }
        "notify0" | "notify1" | "notify2" | "notify3" | "notify4" => {
            let i = key.as_bytes()[6] as usize - b'0' as usize;
            c.notify[i] = !c.notify[i];
        }
        "rows" => {
            let min = c
                .cards
                .iter()
                .filter(|r| r.visible)
                .map(|r| r.y + r.h)
                .max()
                .unwrap_or(5)
                .max(5);
            c.grid_rows = (c.grid_rows + n).clamp(min, 80);
            c.height = (c.vertical_padding() + c.grid_rows as f32 * c.cell_height).ceil() as u32;
        }
        "reset-layout" => {
            c.cards = layout::defaults();
            c.trim_canvas();
        }
        "snap" => c.snap = !c.snap,
        "lock" => c.locked = !c.locked,
        "hold" => c.duration_ms = (c.duration_ms as i32 + n * 500).clamp(500, 10000) as u32,
        "fade" => c.fade_ms = (c.fade_ms as i32 + n * 50).clamp(0, 1000) as u32,
        "battery" => {
            c.low_battery = (c.low_battery as i32 + n * 5).clamp(0, 50) as u8;
            app.low_latched = false;
        }
        "monitor" => {
            let count = app.monitors.len() as i32 + 1;
            let current = app
                .monitors
                .iter()
                .position(|m| m.key == c.monitor)
                .map(|i| i as i32 + 1)
                .unwrap_or(0);
            let next = (current + n).rem_euclid(count);
            c.monitor = if next == 0 {
                String::new()
            } else {
                app.monitors[next as usize - 1].key.clone()
            };
            c.position = None;
        }
        "startup" => {
            let enabled = !c.autostart;
            if app.demo {
                c.autostart = enabled;
            } else {
                match startup(enabled) {
                    Ok(()) => c.autostart = enabled,
                    Err(e) => app.note = e,
                }
            }
        }
        "reset-position" => reset(app),
        "preview" => {
            app.popup_cards = app.config.changed_card_only.then_some(1);
            app.settings = None;
            show(app, false)
        }
        _ => (),
    };
    app.config.fit_layout();
    save(app);
}
pub(super) unsafe fn record(app: &mut App, vk: u32) {
    if !app.recording {
        return;
    }
    if vk == VK_ESCAPE.0 as u32 {
        app.recording = false;
        return;
    }
    if matches!(vk,0x10..=0x12|0x5b..=0x5c|0xa0..=0xa5) {
        return;
    }
    let down = |k: VIRTUAL_KEY| GetKeyState(k.0 as i32) < 0;
    let mods =
        u32::from(down(VK_MENU)) + 2 * u32::from(down(VK_CONTROL)) + 4 * u32::from(down(VK_SHIFT));
    if !valid_hotkey(mods, vk) || down(VK_LWIN) || down(VK_RWIN) {
        app.note = "Choose one key, or Alt / Ctrl / Shift plus one key. F12 is reserved.".into();
        return;
    }
    app.recorded_key = Some((mods, vk));
    app.note.clear();
}
pub(super) unsafe fn finish_recording(app: &mut App) {
    if !app.recording {
        return;
    }
    let Some((mods, vk)) = app.recorded_key else {
        app.note = "Press a shortcut first, then click End recording.".into();
        return;
    };
    if (mods, vk) == (app.config.hotkey_mod, app.config.hotkey_vk) && app.hotkey_registered {
        app.recording = false;
        return;
    }
    let candidate = if app.hotkey_id >= 100 {
        app.hotkey_id - 100
    } else {
        app.hotkey_id + 100
    };
    if RegisterHotKey(
        Some(app.hwnd),
        candidate,
        HOT_KEY_MODIFIERS(mods) | MOD_NOREPEAT,
        vk,
    )
    .is_ok()
    {
        let _ = UnregisterHotKey(Some(app.hwnd), app.hotkey_id);
        app.hotkey_id = candidate;
        app.hotkey_registered = true;
        app.config.hotkey_mod = mods;
        app.config.hotkey_vk = vk;
        app.recording = false;
        app.note = "Shortcut saved.".into();
        save(app);
    } else {
        app.note = "That shortcut is already in use. Your previous shortcut still works.".into();
    }
}

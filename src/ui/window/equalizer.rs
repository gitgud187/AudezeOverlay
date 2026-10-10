//! EQ gestures and native menus; USB work remains on the device worker.
use super::*;

pub(super) unsafe fn receive_eq(app: &mut App) {
    if app.sound.mix.is_none() {
        app.eq_editor.failed();
    }
    if let Some(snapshot) = app.sound.eq_bands {
        app.eq_editor.receive(snapshot);
    }
}
pub(super) unsafe fn open_eq(app: &mut App) {
    if app.eq_editor.banks.iter().all(Option::is_none) {
        app.eq_editor.selected = app.sound.eq_index.unwrap_or(6);
    }
    send(app, Command::EqRefresh);
    inspect_eq(app, app.eq_editor.selected);
}
pub(super) unsafe fn inspect_eq(app: &mut App, slot: u8) {
    if slot >= 10 {
        return;
    }
    commit_eq_name(app);
    app.eq_editor.selected = slot;
    if slot >= 6 {
        send(app, Command::EqLoad(slot));
    }
    request_draw(app);
}
pub(super) unsafe fn set_eq_band(app: &mut App, index: usize, y: f32) {
    let Some(hit) = app.hits.iter().find(|h| h.action == Action::EqBand(index)) else {
        return;
    };
    let gain = 12. - ((y - hit.rect.y) / hit.rect.h).clamp(0., 1.) * 24.;
    if let Some(draft) = app.eq_editor.draft_mut() {
        draft.set_gain(index, gain);
    }
    request_draw(app);
}
pub(super) unsafe fn save_eq(app: &mut App) {
    if app.sound.mix.is_none() {
        app.note = "Connect the headset, then reload this EQ before saving.".into();
        return;
    }
    let Some(draft) = app.eq_editor.draft_mut() else {
        return;
    };
    if draft.saving || !draft.dirty() {
        return;
    }
    let target = draft.value;
    let original = draft.original;
    draft.saving = true;
    // Only modified bands are sent. Each has its own verified readback.
    for (band, old) in target.bands.into_iter().zip(original.bands) {
        if band != old {
            send(
                app,
                Command::EqGain {
                    slot: target.slot,
                    band,
                },
            );
        }
    }
}
pub(super) unsafe fn commit_eq_name(app: &mut App) {
    if let Some(name) = app.eq_editor.name_edit.take() {
        let slot = app.eq_editor.selected;
        if (6..=9).contains(&slot) && !name.trim().is_empty() {
            app.config.eq.names[slot as usize - 6] = name.trim().chars().take(32).collect();
            save(app);
        }
    }
}
pub(super) fn type_eq_name(app: &mut App, ch: char) {
    let Some(text) = app.eq_editor.name_edit.as_mut() else {
        return;
    };
    if ch == '\u{8}' || !ch.is_control() {
        if app.eq_editor.name_selected {
            text.clear();
            app.eq_editor.name_selected = false;
        }
        if ch == '\u{8}' {
            text.pop();
        } else if text.chars().count() < 32 {
            text.push(ch);
        }
    }
}
pub(super) unsafe fn eq_menu(app: &mut App) {
    send(app, Command::EqRefresh);
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let mut count = 0;
    for slot in 0..10u8 {
        if app.config.eq.visible(slot) {
            let flags = MF_STRING
                | if app.sound.eq_index == Some(slot) {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                };
            let _ = AppendMenuW(
                menu,
                flags,
                slot as usize + 1,
                PCWSTR(wide(app.config.eq.name(slot)).as_ptr()),
            );
            count += 1;
        }
    }
    if count == 0 {
        let _ = AppendMenuW(
            menu,
            MF_STRING | MF_GRAYED,
            0,
            w!("Pin presets in Equalizer settings"),
        );
    }
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, 20, w!("Edit equalizer…"));
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
    let _ = DestroyMenu(menu);
    if (1..=10).contains(&id) {
        send(app, Command::EqSelect((id - 1) as u8));
    } else if id == 20 {
        act(app, Action::EqEditor, 0., 0.);
    }
    request_draw(app);
}

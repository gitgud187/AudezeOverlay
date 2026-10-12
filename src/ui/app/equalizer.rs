//! EQ gestures and native menus; USB work remains on the device worker.
use super::*;

pub(in crate::ui) fn receive_eq(app: &mut App) {
    if app.sound.mix.is_none() {
        app.eq_editor.failed();
    }
    if let Some(snapshot) = app.sound.eq_bands {
        let mismatch = snapshot
            .slot
            .checked_sub(6)
            .and_then(|i| app.eq_editor.banks.get(i as usize))
            .and_then(Option::as_ref)
            .is_some_and(|d| {
                d.saving && app.sound.eq_revision >= d.confirmation_revision && d.value != snapshot
            });
        app.eq_editor.receive(snapshot, app.sound.eq_revision);
        if mismatch {
            app.note = "The headset did not confirm this EQ. Save or Discard to retry.".into();
        }
    }
}
pub(in crate::ui) fn open_eq(app: &mut App) {
    if app.eq_editor.banks.iter().all(Option::is_none) {
        app.eq_editor.selected = app.sound.eq_index.unwrap_or(6);
    }
    if !app.config.eq.show_factory && app.eq_editor.selected < 6 {
        app.eq_editor.selected = 6;
    }
    send(app, Command::EqRefresh);
    inspect_eq(app, app.eq_editor.selected);
}
pub(in crate::ui) fn inspect_eq(app: &mut App, slot: u8) {
    if slot >= 10 {
        return;
    }
    commit_eq_name(app);
    app.eq_editor.selected = slot;
    app.eq_editor.preview_slot = None;
    if slot >= 6 && !app.sound.capabilities.is_some_and(|caps| !caps.eq_edit) {
        send(app, Command::EqLoad(slot));
    }
    request_draw(app);
}
pub(in crate::ui) fn set_eq_band(app: &mut App, index: usize, y: f32) {
    let Some(hit) = app.hits.iter().find(|h| h.action == Action::EqBand(index)) else {
        return;
    };
    let gain = 12. - ((y - hit.rect.y) / hit.rect.h).clamp(0., 1.) * 24.;
    preview_eq_gain(app, index, gain);
    request_draw(app);
}
/// Coalesce repeated edits to the same band; the USB worker owns pacing/readback.
pub(in crate::ui) fn preview_eq_gain(app: &mut App, index: usize, gain: f32) {
    if app.sound.mix.is_none() {
        return;
    }
    let Some(draft) = app.eq_editor.draft_mut() else {
        return;
    };
    if draft.saving || index >= 10 {
        return;
    }
    let before = draft.value.bands[index];
    draft.set_gain(index, gain);
    let band = draft.value.bands[index];
    let slot = draft.value.slot;
    if band == before {
        return;
    }
    draft.previewing = true;
    if app.sound.eq_index != Some(slot) && app.eq_editor.preview_slot != Some(slot) {
        send(app, Command::EqSelect(slot));
    }
    app.eq_editor.preview_slot = Some(slot);
    send(app, Command::EqGain { slot, band });
}
pub(in crate::ui) fn save_eq(app: &mut App) {
    if remember_current_profile(app, false) {
        finish_eq_preview(app, false);
        if let Some(bank) = app.eq_editor.selected.checked_sub(6) {
            app.eq_editor.preview_names[bank as usize] = None;
        }
    }
}
pub(in crate::ui) fn discard_eq(app: &mut App) {
    finish_eq_preview(app, true);
    if let Some(bank) = app.eq_editor.selected.checked_sub(6) {
        if let Some(name) = app.eq_editor.preview_names[bank as usize].take() {
            app.config.eq.names[bank as usize] = name;
            app.eq_editor.library_entries[bank as usize] = None;
            save(app);
        }
    }
}
fn finish_eq_preview(app: &mut App, discard: bool) {
    if app.sound.mix.is_none() {
        app.note = Notice::UntilConnected("Reconnect the headset to save or restore this tuning.");
        return;
    }
    let Some(draft) = app.eq_editor.draft_mut() else {
        return;
    };
    if draft.saving || (!draft.dirty() && !draft.previewing) {
        return;
    }
    let edited = draft.value;
    let touched = draft.touched;
    let target = if discard { draft.original } else { edited };
    draft.value = target;
    draft.saving = true;
    // Re-send the final values to replace any older queued drag target.
    for band in target.bands {
        if touched & (1 << band.id) != 0 {
            send(
                app,
                Command::EqGain {
                    slot: target.slot,
                    band,
                },
            );
        }
    }
    // A fresh, ordered readback is the transaction boundary, not a cached value.
    let revision = app
        .controller
        .as_ref()
        .map(|c| c.send(Command::EqLoad(target.slot)))
        .unwrap_or(0);
    app.eq_editor.draft_mut().unwrap().confirmation_revision = revision;
    if app.demo {
        receive_eq(app);
    }
}
pub(in crate::ui) fn commit_eq_name(app: &mut App) {
    if let Some(name) = app.eq_editor.name_edit.take() {
        let slot = app.eq_editor.selected;
        if (6..=9).contains(&slot) && !name.trim().is_empty() {
            app.config.eq.names[slot as usize - 6] = name.trim().chars().take(32).collect();
            save(app);
        }
    }
}
pub(in crate::ui) fn type_eq_name(app: &mut App, ch: char) {
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
pub(in crate::ui) fn eq_menu(app: &mut App) {
    send(app, Command::EqRefresh);
    let mut entries = vec![];
    for slot in 0..10u8 {
        if app.config.eq.visible(slot) {
            entries.push(MenuEntry::checkable(
                slot as u32 + 1,
                app.config.eq.name(slot),
                app.sound.eq_index == Some(slot),
            ));
        }
    }
    if entries.is_empty() {
        entries.push(MenuEntry::disabled("Pin presets in Equalizer settings"));
    }
    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::item(20, "Edit equalizer…"));
    let id = app
        .shell
        .popup_menu(&entries, MenuOrigin::Panel)
        .unwrap_or(0);
    if (1..=10).contains(&id) {
        send(app, Command::EqSelect((id - 1) as u8));
    } else if id == 20 {
        act(app, Action::EqEditor, 0., 0.);
    }
    request_draw(app);
}

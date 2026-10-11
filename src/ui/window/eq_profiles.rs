//! Local EQ library and file actions. Loading explicitly previews a custom slot.
use super::*;
use audeze_overlay::eq::profiles::{self, Profile};

fn current_profile(app: &App) -> std::result::Result<Profile, String> {
    let draft = app
        .eq_editor
        .draft()
        .ok_or("Load a custom headset slot first.")?;
    if draft.saving {
        return Err("Wait for the current tuning to be confirmed.".into());
    }
    Profile::new(app.config.eq.name(draft.value.slot), draft.value)
}

unsafe fn store_profile(
    app: &mut App,
    mut profile: Profile,
    replace: Option<usize>,
) -> std::result::Result<usize, String> {
    profile.validate()?;
    let mut config = app.config.clone();
    let index = if let Some(index) = replace.filter(|i| *i < config.eq_library.len()) {
        // Save updates tuning, while Save as new keeps a distinct library name.
        profile.name = config.eq_library[index].name.clone();
        config.eq_library[index] = profile;
        index
    } else {
        profiles::add(&mut config.eq_library, profile)?
    };
    if !app.demo {
        config
            .save()
            .map_err(|e| format!("Could not save the profile library: {e}"))?;
    }
    app.config = config;
    Ok(index)
}

pub(super) unsafe fn remember_current_profile(app: &mut App, copy: bool) -> bool {
    let result = (|| {
        let profile = current_profile(app)?;
        let bank = app.eq_editor.selected as usize - 6;
        let replace = if copy {
            None
        } else {
            app.eq_editor.library_entries[bank]
        };
        let index = store_profile(app, profile, replace)?;
        app.eq_editor.library_entries[bank] = Some(index);
        Ok::<_, String>(app.config.eq_library[index].name.clone())
    })();
    match result {
        Ok(name) => {
            app.note = format!("Saved “{name}” in your profile library.");
            true
        }
        Err(error) => {
            app.note = error;
            false
        }
    }
}
pub(super) unsafe fn save_profile_copy(app: &mut App) {
    remember_current_profile(app, true);
    request_draw(app);
}

unsafe fn import_profile(app: &mut App) -> std::result::Result<(), String> {
    let Some(path) = crate::platform::profile_dialog::choose(app.hwnd, false)? else {
        return Ok(());
    };
    let baseline = app.eq_editor.draft().map(|d| d.value);
    let profile = profiles::read(&path, baseline)?;
    let index = store_profile(app, profile, None)?;
    app.note = format!(
        "Imported “{}”. Choose it in Profiles to load into a custom slot.",
        app.config.eq_library[index].name
    );
    Ok(())
}
unsafe fn export_profile(app: &mut App, profile: Profile) -> std::result::Result<(), String> {
    let Some(path) = crate::platform::profile_dialog::choose(app.hwnd, true)? else {
        return Ok(());
    };
    profiles::write(&path, &profile)?;
    app.note = "EQ profile exported. It can be imported on another PC.".into();
    Ok(())
}
unsafe fn load_profile(app: &mut App, index: usize) -> std::result::Result<(), String> {
    let profile = app
        .config
        .eq_library
        .get(index)
        .ok_or("Profile no longer exists.")?
        .clone();
    if app.sound.mix.is_none() || app.sound.capabilities.is_some_and(|c| !c.eq_edit) {
        return Err("Connect a headset with verified custom EQ editing first.".into());
    }
    let draft = app
        .eq_editor
        .draft()
        .ok_or("Choose and load a custom headset slot first.")?;
    if draft.saving {
        return Err("Wait for the current tuning to be confirmed.".into());
    }
    let target = profile.snapshot(draft.value.slot)?;
    let question = format!("Preview “{}” in {}?\n\nThis changes the sound immediately. Save keeps it on the headset; Discard restores the slot's previous curve. Your saved library profile stays separate.", profile.name, app.config.eq.name(target.slot));
    if MessageBoxW(
        Some(app.hwnd),
        PCWSTR(wide(&question).as_ptr()),
        w!("Load EQ profile"),
        MB_YESNO | MB_ICONQUESTION,
    ) != IDYES
    {
        return Ok(());
    }
    // Keep the baseline (including an existing preview) until ordered readback.
    let Some(draft) = app.eq_editor.draft_mut() else {
        return Err("The custom slot is no longer available.".into());
    };
    draft.value = target;
    draft.touched = (1 << 10) - 1;
    draft.previewing = true;
    app.eq_editor.library_entries[target.slot as usize - 6] = Some(index);
    app.eq_editor.preview_names[target.slot as usize - 6]
        .get_or_insert_with(|| app.config.eq.name(target.slot).to_owned());
    app.config.eq.names[target.slot as usize - 6] = profile.name.chars().take(32).collect();
    save(app);
    if app.sound.eq_index != Some(target.slot) {
        send(app, Command::EqSelect(target.slot));
    }
    app.eq_editor.preview_slot = Some(target.slot);
    for band in target.bands {
        send(
            app,
            Command::EqGain {
                slot: target.slot,
                band,
            },
        );
    }
    app.note = "Profile preview loaded. Save keeps it; Discard restores the previous curve.".into();
    Ok(())
}

pub(super) unsafe fn profile_menu(app: &mut App) {
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let _ = AppendMenuW(menu, MF_STRING, 1, w!("Import from file…"));
    let ready = app.eq_editor.draft().is_some_and(|d| !d.saving);
    let flags = MF_STRING | if ready { MF_ENABLED } else { MF_GRAYED };
    let _ = AppendMenuW(menu, flags, 2, w!("Export current tuning…"));
    let _ = AppendMenuW(menu, flags, 3, w!("Save current tuning as new profile"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    for (i, profile) in app.config.eq_library.iter().enumerate() {
        let Ok(child) = CreatePopupMenu() else {
            continue;
        };
        let _ = AppendMenuW(
            child,
            flags,
            1000 + i,
            w!("Load into selected custom slot…"),
        );
        let _ = AppendMenuW(child, MF_STRING, 2000 + i, w!("Export profile…"));
        let _ = AppendMenuW(child, MF_STRING, 3000 + i, w!("Remove from library…"));
        let label = profile.name.replace('&', "&&");
        if AppendMenuW(
            menu,
            MF_POPUP | MF_STRING,
            child.0 as usize,
            PCWSTR(wide(&label).as_ptr()),
        )
        .is_err()
        {
            let _ = DestroyMenu(child);
        }
    }
    if app.config.eq_library.is_empty() {
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, w!("No saved profiles yet"));
    }
    let mut point = POINT::default();
    let _ = GetCursorPos(&mut point);
    let id = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY,
        point.x,
        point.y,
        None,
        app.hwnd,
        None,
    )
    .0 as usize;
    let _ = DestroyMenu(menu);
    let result = match id {
        1 => import_profile(app),
        2 => current_profile(app).and_then(|p| export_profile(app, p)),
        3 => {
            save_profile_copy(app);
            Ok(())
        }
        1000..2000 => load_profile(app, id - 1000),
        2000..3000 => match app.config.eq_library.get(id - 2000).cloned() {
            Some(profile) => export_profile(app, profile),
            None => Ok(()),
        },
        3000..4000 => remove_profile(app, id - 3000),
        _ => Ok(()),
    };
    if let Err(error) = result {
        app.note = error;
    }
    request_draw(app);
}
unsafe fn remove_profile(app: &mut App, index: usize) -> std::result::Result<(), String> {
    let Some(profile) = app.config.eq_library.get(index) else {
        return Ok(());
    };
    let text = format!("Remove “{}” from this PC's library? Headset slots and exported files will stay as they are.", profile.name);
    if MessageBoxW(
        Some(app.hwnd),
        PCWSTR(wide(&text).as_ptr()),
        w!("Remove profile"),
        MB_YESNO | MB_ICONQUESTION,
    ) != IDYES
    {
        return Ok(());
    }
    let mut config = app.config.clone();
    config.eq_library.remove(index);
    if !app.demo {
        config
            .save()
            .map_err(|e| format!("Could not save the library: {e}"))?;
    }
    app.config = config;
    for entry in &mut app.eq_editor.library_entries {
        *entry = entry.and_then(|i| {
            if i == index {
                None
            } else {
                Some(if i > index { i - 1 } else { i })
            }
        });
    }
    app.note = "Profile removed from the library.".into();
    Ok(())
}

//! Opt-in native regression checks. Uses demo state and never writes user preferences.
use super::*;
pub(super) unsafe fn smoke(app: &mut App) {
    match app.smoke_phase {
        0 => {
            let state = &*(GetWindowLongPtrW(app.hwnd, GWLP_USERDATA) as *const WindowState);
            let _ = SendMessageW(app.hwnd, STATUS_MESSAGE, None, None);
            app.checks.push((
                "reentrant_status_is_deferred".into(),
                state.deferred.get() & 1 != 0,
            ));
            let fg = GetForegroundWindow();
            show(app, false);
            app.checks
                .push(("popup_keeps_focus".into(), GetForegroundWindow() == fg));
            app.checks.push((
                "popup_click_through".into(),
                GetWindowLongPtrW(app.hwnd, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT.0 != 0,
            ));
            app.config.duration_ms = 100;
            SetTimer(Some(app.hwnd), HOLD, 100, None);
        }
        8 => {
            app.sound = sample();
            app.checks.push((
                "popup_fades_and_hides".into(),
                !IsWindowVisible(app.hwnd).as_bool(),
            ));
            show(app, true);
            app.checks.push((
                "interactive_open".into(),
                GetWindowLongPtrW(app.hwnd, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT.0 == 0,
            ));
            app.alpha = 255;
            app.fade_in = None;
            render(app);
            let bitmap = app.bitmap.as_ref().map(|b| b.bitmap);
            let raster = app.raster.as_ref().map(|p| p.data().as_ptr());
            render(app);
            app.checks.push((
                "animation_reuses_bitmap_and_raster".into(),
                bitmap.is_some()
                    && bitmap == app.bitmap.as_ref().map(|b| b.bitmap)
                    && raster == app.raster.as_ref().map(|p| p.data().as_ptr()),
            ));
            // Multiple status/UI requests before the next animation tick keep
            // just the newest state, with no extra intermediate rasterizations.
            app.smoke = false;
            app.last_frame = Some(Instant::now());
            let rendered = app.last_frame;
            app.draw_queued = true;
            let _ = events::dispatch(app, app.hwnd, DRAW, WPARAM(0), LPARAM(0));
            let _ = events::dispatch(app, app.hwnd, DRAW, WPARAM(0), LPARAM(0));
            app.checks.push((
                "animation_coalesces_draws_until_tick".into(),
                app.last_frame == rendered && app.draw_queued && app.animation_timer,
            ));
            app.smoke = true;
            app.mix_started = None;
            let _ = events::dispatch(app, app.hwnd, WM_TIMER, WPARAM(TICK), LPARAM(0));
            app.checks.push((
                "animation_timer_stops_after_final_pending_frame".into(),
                !app.draw_queued && !app.animation_timer && app.last_frame != rendered,
            ));
            // Reconnect through the same status-message path as the worker,
            // without a mouse event or an extra device command.
            let live = app.sound.clone();
            let previous_latest = app.latest.lock().unwrap().clone();
            app.sound = SoundState::default();
            app.demo = false;
            send(app, Command::EqRefresh);
            app.demo = true;
            app.checks.push((
                "offline_command_shows_connection_notice".into(),
                !app.note.is_empty(),
            ));
            *app.latest.lock().unwrap() = live;
            let _ = events::dispatch(app, app.hwnd, STATUS_MESSAGE, WPARAM(0), LPARAM(0));
            app.checks.push((
                "reconnect_clears_notice_and_queues_draw_without_click".into(),
                app.note.is_empty() && app.sound.mix.is_some() && app.draw_queued,
            ));
            app.note = "Could not save settings: test failure".into();
            app.sound.mix = None;
            let _ = events::dispatch(app, app.hwnd, STATUS_MESSAGE, WPARAM(0), LPARAM(0));
            app.checks.push((
                "reconnect_preserves_unrelated_failure".into(),
                app.note.text() == "Could not save settings: test failure",
            ));
            app.note.clear();
            *app.latest.lock().unwrap() = previous_latest;
            render(app);
            let region = CreateRectRgn(0, 0, 0, 0);
            let shape = GetWindowRgn(app.hwnd, region);
            app.checks.push((
                "rounded_window_region_is_set".into(),
                shape != GDI_REGION_TYPE(0)
                    && !PtInRegion(region, 0, 0).as_bool()
                    && PtInRegion(region, 30, 30).as_bool(),
            ));
            let _ = DeleteObject(region.into());
            app.checks.push((
                "audeze_tray_icon_created".into(),
                !app.tray_icon.is_invalid(),
            ));
            app.checks.push((
                "tray_registered_with_shell".into(),
                app.tray_registration.registered,
            ));
            tray(app, NIM_DELETE);
            let _ = events::dispatch(app, app.hwnd, app.taskbar, WPARAM(0), LPARAM(0));
            app.checks.push((
                "tray_restored_after_taskbar_created".into(),
                app.tray_registration.registered && !app.tray_registration.can_retry(),
            ));
            let scale = GetDpiForWindow(app.hwnd).max(96) as f32 / 96.;
            let mut header = POINT {
                x: (210. * scale) as i32,
                y: (35. * scale) as i32,
            };
            let _ = ClientToScreen(app.hwnd, &mut header);
            let point =
                LPARAM(((header.x as u16 as u32) | ((header.y as u16 as u32) << 16)) as isize);
            let caption = events::dispatch(app, app.hwnd, WM_NCHITTEST, WPARAM(0), point).unwrap();
            app.checks.push((
                "transparent_header_drags_without_edit_mode".into(),
                !app.edit && WindowFromPoint(header) == app.hwnd && caption.0 == HTCAPTION as isize,
            ));
            act(app, Action::Edit, 0., 0.);
            app.checks.push(("layout_editor".into(), app.edit));
            render(app);
            let menu = app
                .hits
                .iter()
                .find(|h| h.action == Action::CardMenu(3))
                .unwrap()
                .rect;
            app.checks.push((
                "card_dots_have_priority_over_drag".into(),
                hit(app, menu.x + 8., menu.y + 8.) == Action::CardMenu(3),
            ));
            act(app, Action::RemoveCard(3), 0., 0.);
            render(app);
            app.checks.push((
                "remove_card_and_add_control_visible".into(),
                !app.config.cards[3].visible
                    && app.hits.iter().any(|h| h.action == Action::AddCard),
            ));
            act(app, Action::RestoreCard(3), 0., 0.);
            render(app);
            app.checks.push((
                "removed_card_can_be_added_back".into(),
                app.config.cards[3].visible,
            ));
            let before_resize = app.config.clone();
            let before_boxes = app.cards;
            app.config
                .resize_canvas(app.config.width + 150, app.config.height + 120);
            position(app);
            render(app);
            app.checks.push((
                "window_resize_adds_room_without_stretching".into(),
                app.cards
                    .iter()
                    .zip(before_boxes)
                    .all(|(n, o)| (n.w - o.w).abs() < 0.01 && (n.h - o.h).abs() < 0.01),
            ));
            finish_layout(app);
            app.checks.push((
                "finish_removes_empty_trailing_canvas".into(),
                app.config.width <= before_resize.width + 1
                    && app.config.height <= before_resize.height + 1,
            ));
            app.config = before_resize;
            app.edit = true;
            position(app);
            render(app);
            let original = app.config.cards;
            let source = app.cards[1];
            let target = app.cards[2];
            act(
                app,
                Action::Card(1),
                source.x + source.w / 2.,
                source.y + source.h / 2.,
            );
            drag(app, target.x + target.w / 2., target.y + target.h / 2.);
            app.checks.push((
                "passing_over_does_not_swap".into(),
                app.config.cards[2] == original[2],
            ));
            app.reorder_hover = layout::HoverIntent::default();
            app.reorder_hover.update(
                Some(2),
                (target.x + target.w / 2., target.y + target.h / 2.),
                Instant::now() - layout::HoverIntent::DELAY,
            );
            drag(app, target.x + target.w / 2., target.y + target.h / 2.);
            let swapped = app.config.cards;
            drag(app, target.x + target.w / 2., target.y + target.h / 2.);
            app.checks.push((
                "card_swap_stable_during_drag".into(),
                app.config.cards == swapped
                    && swapped[1] == original[2]
                    && swapped[2] == original[1],
            ));
            app.drag = Drag::None;
            let _ = ReleaseCapture();
            app.config.cards = original;
            render(app);
            let source = app.cards[0];
            let target = app.cards[1];
            act(
                app,
                Action::Card(0),
                source.x + source.w / 2.,
                source.y + source.h / 2.,
            );
            app.reorder_hover.update(
                Some(1),
                (target.x + target.w / 2., target.y + target.h / 2.),
                Instant::now() - layout::HoverIntent::DELAY,
            );
            drag(app, target.x + target.w / 2., target.y + target.h / 2.);
            app.checks.push((
                "large_card_adapts_to_smaller_slot".into(),
                app.config.cards[0].x == 6
                    && app.config.cards[0].h == 8
                    && app.config.cards[1].x == 0
                    && app.config.cards[2].x == 0,
            ));
            app.drag = Drag::None;
            let _ = ReleaseCapture();
            app.config.cards = original;
            slider_value(app, "rows", 25.);
            position(app);
            render(app);
            act(app, Action::Card(1), 0., 0.);
            let (width, height) = logical_size(app);
            drag(app, -6. * (width - 24.) / 12., 15. * (height - 90.) / 25.);
            app.checks.push((
                "card_drag_on_grid".into(),
                app.config.cards[1].x == 0 && app.config.cards[1].y == 15,
            ));
            app.drag = Drag::None;
            let _ = ReleaseCapture();
            act(app, Action::ResizeCard(1), 0., 0.);
            drag(app, 6. * (width - 24.) / 12., (height - 90.) / 25.);
            app.checks.push((
                "card_resize_on_grid".into(),
                app.config.cards[1].w == 12 && app.config.cards[1].h == 5,
            ));
            app.drag = Drag::None;
            let _ = ReleaseCapture();
            act(app, Action::Settings, 0., 0.);
            app.checks
                .push(("structured_settings".into(), app.settings == Some(0)));
            let card_opacity_before = app.config.card_opacity;
            let background_before = app.config.background_opacity;
            setting(app, "background", 1);
            app.checks.push((
                "independent_opacity".into(),
                app.config.background_opacity == background_before + 5
                    && app.config.card_opacity == card_opacity_before,
            ));
            render(app);
            let r = app
                .hits
                .iter()
                .find(|h| h.action == Action::Slider("opacity"))
                .unwrap()
                .rect;
            act(
                app,
                Action::Slider("opacity"),
                r.x + 10. + (r.w - 20.) * 0.48,
                r.y + 16.,
            );
            app.checks.push((
                "slider_drag_snaps_to_five".into(),
                app.config.card_opacity == 55,
            ));
            drag(app, r.x - 100., r.y);
            app.checks.push((
                "slider_clamps_minimum".into(),
                app.config.card_opacity == 10,
            ));
            drag(app, r.x + r.w + 100., r.y);
            app.checks.push((
                "slider_clamps_maximum".into(),
                app.config.card_opacity == 100,
            ));
            app.drag = Drag::None;
            let _ = ReleaseCapture();
            let restored: Config =
                serde_json::from_slice(&serde_json::to_vec(&app.config).unwrap()).unwrap();
            app.checks.push((
                "layout_and_slider_persist".into(),
                restored.cards == app.config.cards
                    && restored.card_opacity == 100
                    && restored.grid_rows == app.config.grid_rows
                    && restored.cell_width == app.config.cell_width
                    && restored.cell_height == app.config.cell_height,
            ));
            app.alpha = 30;
            app.fade_out = Some(Instant::now());
            show(app, true);
            app.checks.push((
                "reopen_cancels_partial_fade".into(),
                app.alpha == 255 && app.fade_out.is_none(),
            ));
            setting(app, "card3", 0);
            app.checks
                .push(("card_visibility".into(), !app.config.cards[3].visible));
            setting(app, "notify2", 0);
            app.checks
                .push(("selective_notifications".into(), !app.config.notify[2]));
            app.number_edit = Some(audeze_overlay::input::NumberDraft {
                key: "hold",
                text: "797".into(),
                selected: false,
            });
            let committed = commit_number(app);
            app.checks.push((
                "typed_duration_preserves_milliseconds".into(),
                committed && app.config.duration_ms == 797,
            ));
            app.config.settings_page = 2;
            act(app, Action::Settings, 0., 0.);
            app.checks
                .push(("settings_remembers_page".into(), app.settings == Some(2)));
            act(app, Action::Record, 0., 0.);
            app.recorded_key = Some((app.config.hotkey_mod, app.config.hotkey_vk));
            app.checks
                .push(("recording_waits_for_end".into(), app.recording));
            finish_recording(app);
            app.checks
                .push(("recording_ends_explicitly".into(), !app.recording));
            app.checks
                .push(("hotkey_registered".into(), app.hotkey_registered));
            app.checks.push((
                "config_round_trip".into(),
                serde_json::from_slice::<Config>(&serde_json::to_vec(&app.config).unwrap()).is_ok(),
            ));
            let size_before = app.config.cards;
            setting(app, "status-icons", 0);
            app.checks.push((
                "header_status_preserves_card_sizes".into(),
                app.config.status_icons && app.config.cards == size_before,
            ));
            app.settings = None;
            app.edit = true;
            render(app);
            app.checks.push((
                "fixed_status_row_has_remove_target".into(),
                app.hits.iter().any(|h| h.action == Action::StatusMenu),
            ));
            app.edit = false;
            render(app);
            app.checks.push((
                "add_button_only_in_layout_mode".into(),
                !app.hits.iter().any(|h| h.action == Action::AddCard),
            ));
            act(app, Action::EqEditor, 0., 0.);
            act(app, Action::EqInspect(6), 0., 0.);
            render(app);
            app.checks.push((
                "eq_editor_loads_ten_bands".into(),
                app.hits
                    .iter()
                    .filter(|h| matches!(h.action, Action::EqBand(_)))
                    .count()
                    == 10,
            ));
            let eq_before = app.sound.eq_bands;
            preview_eq_gain(app, 2, 4.);
            app.checks.push((
                "eq_drag_previews_live".into(),
                app.sound.eq_bands.unwrap().bands[2].gain == 4.
                    && app.eq_editor.draft().unwrap().dirty(),
            ));
            act(app, Action::EqDiscard, 0., 0.);
            app.checks.push((
                "eq_discard_restores_headset".into(),
                app.sound.eq_bands == eq_before && !app.eq_editor.draft().unwrap().dirty(),
            ));
            preview_eq_gain(app, 2, 4.);
            act(app, Action::EqSave, 0., 0.);
            app.checks.push((
                "eq_save_confirms_draft".into(),
                app.sound.eq_bands.unwrap().bands[2].gain == 4.
                    && !app.eq_editor.draft().unwrap().dirty(),
            ));
            act(app, Action::EqUse(6), 0., 0.);
            let original_profile = app.config.eq_library[0].clone();
            preview_eq_gain(app, 2, 6.);
            act(app, Action::EqSaveNew, 0., 0.);
            app.checks.push((
                "eq_save_as_new_preserves_existing_profile".into(),
                app.config.eq_library.len() == 2
                    && app.config.eq_library[0] == original_profile
                    && app.config.eq_library[1].bands[2].gain == 6.,
            ));
            act(app, Action::EqDiscard, 0., 0.);
            app.checks.push((
                "eq_discard_keeps_library_copy_and_restores_headset".into(),
                app.config.eq_library[1].bands[2].gain == 6.
                    && app.sound.eq_bands.unwrap().bands[2].gain == 4.,
            ));
            app.checks
                .push(("eq_quick_selection".into(), app.sound.eq_index == Some(6)));
            act(app, Action::EqPin(6), 0., 0.);
            app.checks
                .push(("eq_pin_visibility".into(), !app.config.eq.visible(6)));
            setting(app, "eq-factory", 0);
            render(app);
            app.checks.push((
                "factory_presets_hidden_in_settings".into(),
                !app.hits
                    .iter()
                    .any(|h| matches!(h.action, Action::EqInspect(slot) if slot < 6)),
            ));
            app.checks
                .push(("eq_factory_visibility".into(), !app.config.eq.visible(0)));
            act(app, Action::EqRename, 0., 0.);
            for ch in "Gaming".chars() {
                type_eq_name(app, ch);
            }
            commit_eq_name(app);
            app.checks
                .push(("eq_custom_name".into(), app.config.eq.name(6) == "Gaming"));
            let sound_before_reset = app.sound.clone();
            factory_reset(app);
            app.checks.push((
                "factory_reset_restores_defaults_without_headset_changes".into(),
                app.config.background_opacity == 70
                    && app.config.card_opacity == 70
                    && app.config.settings_page == 0
                    && app.config.cards == layout::defaults()
                    && app.sound == sound_before_reset,
            ));
            app.config.position = Some((-30000, -30000));
            app.recording = true;
            app.recorded_key = Some((2, 0x24));
            events::dispatch(app, app.hwnd, WM_KILLFOCUS, WPARAM(0), LPARAM(0));
            app.checks.push((
                "shortcut_draft_survives_focus_loss".into(),
                app.recording && app.recorded_key == Some((2, 0x24)),
            ));
            act(app, Action::DiscardRecord, 0., 0.);
            app.checks.push((
                "shortcut_discard_is_explicit".into(),
                !app.recording && app.recorded_key.is_none(),
            ));
            let full = app.config.clone();
            app.popup_cards = Some(1);
            show(app, false);
            render(app);
            app.checks.push((
                "changed_card_popup_is_temporary_and_noninteractive".into(),
                app.hits.is_empty() && app.config.cards == full.cards && app.cards[0].y < 20.,
            ));
            show(app, true);
            app.checks.push((
                "manual_open_restores_full_layout".into(),
                app.popup_cards.is_none() && app.config.cards == full.cards,
            ));
            reset(app);
            let mut r = RECT::default();
            let _ = GetWindowRect(app.hwnd, &mut r);
            app.checks.push((
                "position_recovery".into(),
                r.left > -10000 && r.top > -10000,
            ));
        }
        10 => {
            let _ = std::fs::create_dir_all("diagnostics");
            let _ = std::fs::write(
                "diagnostics/smoke-v3.json",
                serde_json::to_vec_pretty(&app.checks).unwrap(),
            );
            let _ = DestroyWindow(app.hwnd);
        }
        _ => (),
    };
    app.smoke_phase += 1;
}

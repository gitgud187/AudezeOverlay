//! Pointer gestures and menus translate into pure grid operations.
use super::*;
pub(in crate::ui) fn drag(app: &mut App, x: f32, y: f32) {
    app.drag_point = (x, y);
    match app.drag {
        Drag::EqBand(index) => set_eq_band(app, index, y),
        Drag::Setting(key) => set_slider(app, key, x),
        Drag::Mix => {
            let r = app.cards[0];
            if app.config.minimal_cards[0] || r.h < 190. {
                if let Some(hit) = app.hits.iter().find(|h| h.action == Action::Mix) {
                    set_mix(
                        app,
                        ((x - hit.rect.x) / hit.rect.w * 100.)
                            .round()
                            .clamp(0., 100.) as u8,
                    );
                }
                return;
            }
            let (cx, cy) = (r.x + r.w / 2., r.y + 54. + (r.h - 90.) / 2.);
            set_mix(app, mix_from_point(x - cx, y - cy));
        }
        Drag::Level => {
            let r = app.cards[1].inset(16.);
            send(
                app,
                Command::SidetoneLevel(((x - r.x) / r.w * 100.).round().clamp(0., 100.) as u8),
            );
        }
        Drag::Card {
            index,
            resize,
            x: sx,
            y: sy,
            original_cards,
        } => {
            let (w, h) = logical_size(app);
            let (cw, ch) = (app.config.cell_width, app.config.cell_height);
            let (cols, rows) = app.config.canvas_cells(w, h);
            let dx = ((x - sx) / cw).round() as i32;
            let dy = ((y - sy) / ch).round() as i32;
            let mut p = original_cards[index];
            if resize {
                p.w += dx;
                p.h += dy
            } else {
                p.x += dx;
                p.y += dy
            }
            if resize {
                let mut cards = original_cards;
                // Respect the work area's size, but make room without a separate window resize.
                let work = monitor_rect(app);
                let scale = app.shell.scale();
                let max_cols = ((work.width() as f32 / scale - 24.) / cw).floor() as i32;
                let max_rows = ((work.height() as f32 / scale - app.config.vertical_padding()) / ch)
                    .floor() as i32;
                if layout::resize_pushing(
                    &mut cards,
                    index,
                    p,
                    max_cols.max(cols),
                    max_rows.max(rows),
                ) {
                    app.config.cards = cards;
                    app.config.fit_layout();
                    position(app);
                }
            } else {
                let pointer = ((x - 12.) / cw, (y - app.config.card_top()) / ch);
                let target = layout::hovered_card(&original_cards, index, pointer);
                let now = Instant::now();
                let swap = app.reorder_hover.update(target, (x, y), now);
                app.shell.stop_timer(Timer::Reorder);
                if let Some(delay) = app.reorder_hover.remaining(now) {
                    app.shell
                        .start_timer(Timer::Reorder, delay.as_millis().max(1) as u32);
                }
                let preview = if swap {
                    layout::preview_move_in(&original_cards, index, p, pointer, cols, rows)
                } else {
                    layout::preview_free_move_in(&original_cards, index, p, cols, rows)
                };
                if let Some(cards) = preview {
                    app.config.cards = cards;
                }
                let source = original_cards[index];
                app.drag_preview = if swap && preview.is_some() {
                    None
                } else {
                    Some((
                        index,
                        Box2::new(
                            (12. + source.x as f32 * cw + 4. + x - sx)
                                .clamp(0., (w - source.w as f32 * cw + 8.).max(0.)),
                            (app.config.card_top() + source.y as f32 * ch + 4. + y - sy).clamp(
                                app.config.card_top(),
                                (h - source.h as f32 * ch + 8.).max(app.config.card_top()),
                            ),
                            source.w as f32 * cw - 8.,
                            source.h as f32 * ch - 8.,
                        ),
                    ))
                };
            }
            request_draw(app);
        }
        Drag::None => (),
    }
}
pub(in crate::ui) fn logical_size(app: &App) -> (f32, f32) {
    let (width, height) = app.shell.client_size().unwrap_or((0, 0));
    let s = app.shell.scale();
    (width as f32 / s, height as f32 / s)
}
pub(in crate::ui) fn finish_layout(app: &mut App) {
    app.drag_preview = None;
    app.shell.stop_timer(Timer::Reorder);
    app.edit = false;
    app.drag = Drag::None;
    app.shell.release_pointer();
    app.config.trim_canvas();
    save(app);
    if app.settings.is_none() {
        position(app);
    }
    request_draw(app);
}
pub(in crate::ui) fn card_visibility(app: &mut App, i: usize, visible: bool) {
    if i >= 6 {
        return;
    }
    if visible {
        if !app.config.restore_card(i) {
            app.note = "Make more room or reset the layout to add this card.".into();
        }
    } else {
        app.config.cards[i].visible = false;
        if !app.edit {
            app.config.trim_canvas();
        }
    }
    if i == 5 && visible {
        send(app, Command::EqRefresh);
    }
    save(app);
    if app.settings.is_none() {
        position(app);
    }
    request_draw(app);
}
pub(in crate::ui) fn card_menu(app: &mut App, index: Option<usize>) {
    let mut entries = vec![];
    if let Some(i) = index {
        entries.push(MenuEntry::item(i as u32 + 1, "Remove card"));
        if i == 0 {
            entries.push(MenuEntry::checkable(
                20,
                "Minimal mode — slider",
                app.config.minimal_cards[i],
            ));
        }
        if i == 5 {
            entries.push(MenuEntry::checkable(
                21,
                "Mini EQ editor when there is room",
                app.config.inline_eq,
            ));
        }
    } else {
        for (i, card) in app.config.cards.iter().enumerate() {
            if !card.visible {
                entries.push(MenuEntry::item(i as u32 + 1, layout::NAMES[i]));
            }
        }
        if !app.config.status_icons {
            entries.push(MenuEntry::item(7, "Status row under logo"));
        }
        if entries.is_empty() {
            entries.push(MenuEntry::disabled("All cards are already shown"));
        }
    }
    let id = app
        .shell
        .popup_menu(&entries, MenuOrigin::Panel)
        .unwrap_or(0);
    if id == 20 {
        app.config.minimal_cards[0] = !app.config.minimal_cards[0];
        let mut proposed = app.config.cards[0];
        proposed.h = if app.config.minimal_cards[0] { 3 } else { 8 };
        layout::resize_pushing(
            &mut app.config.cards,
            0,
            proposed,
            app.config.grid_columns,
            80,
        );
        app.config.trim_canvas();
        position(app);
        save(app);
        request_draw(app);
    } else if id == 21 {
        app.config.inline_eq = !app.config.inline_eq;
        if app.config.inline_eq {
            open_eq(app);
        }
        save(app);
        request_draw(app);
    } else if id == 7 {
        setting(app, "status-icons", 0);
        app.config.fit_layout();
        save(app);
        position(app);
        request_draw(app);
    } else if (1..=6).contains(&id) {
        let i = id as usize - 1;
        act(
            app,
            if index.is_some() {
                Action::RemoveCard(i)
            } else {
                Action::RestoreCard(i)
            },
            0.,
            0.,
        );
    }
}

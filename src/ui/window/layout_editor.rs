//! Pointer gestures and menus translate into pure grid operations.
use super::*;
pub(super) unsafe fn drag(app: &mut App, x: f32, y: f32) {
    app.drag_point = (x, y);
    match app.drag {
        Drag::EqBand(index) => set_eq_band(app, index, y),
        Drag::Setting(key) => set_slider(app, key, x),
        Drag::Mix => {
            let r = app.cards[0];
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
                if layout::resize_in(&mut cards, index, p, cols, rows) {
                    app.config.cards = cards;
                }
            } else {
                let pointer = ((x - 12.) / cw, (y - app.config.card_top()) / ch);
                let target = layout::hovered_card(&original_cards, index, pointer);
                let now = Instant::now();
                let swap = app.reorder_hover.update(target, (x, y), now);
                let _ = KillTimer(Some(app.hwnd), REORDER);
                if let Some(delay) = app.reorder_hover.remaining(now) {
                    SetTimer(
                        Some(app.hwnd),
                        REORDER,
                        delay.as_millis().max(1) as u32,
                        None,
                    );
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
pub(super) unsafe fn logical_size(app: &App) -> (f32, f32) {
    let mut r = RECT::default();
    let _ = GetClientRect(app.hwnd, &mut r);
    let s = GetDpiForWindow(app.hwnd).max(96) as f32 / 96.;
    (r.right as f32 / s, r.bottom as f32 / s)
}
pub(super) unsafe fn finish_layout(app: &mut App) {
    app.drag_preview = None;
    let _ = KillTimer(Some(app.hwnd), REORDER);
    app.edit = false;
    app.drag = Drag::None;
    let _ = ReleaseCapture();
    app.config.trim_canvas();
    save(app);
    if app.settings.is_none() {
        position(app);
    }
    request_draw(app);
}
pub(super) unsafe fn card_visibility(app: &mut App, i: usize, visible: bool) {
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
pub(super) unsafe fn card_menu(app: &mut App, index: Option<usize>) {
    let Ok(menu) = CreatePopupMenu() else { return };
    if let Some(i) = index {
        let _ = AppendMenuW(menu, MF_STRING, i + 1, w!("Remove card"));
    } else {
        let mut any = false;
        for (i, card) in app.config.cards.iter().enumerate() {
            if !card.visible {
                let _ = AppendMenuW(
                    menu,
                    MF_STRING,
                    i + 1,
                    PCWSTR(wide(layout::NAMES[i]).as_ptr()),
                );
                any = true;
            }
        }
        if !any {
            let _ = AppendMenuW(
                menu,
                MF_STRING | MF_GRAYED,
                99,
                w!("All cards are already shown"),
            );
        }
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
    let _ = DestroyMenu(menu);
    if (1..=6).contains(&id) {
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

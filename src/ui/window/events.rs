//! Win32 callbacks. The boxed WindowState outlives the HWND and message loop.
use super::*;

// SAFETY: lpCreateParams points to the run-owned Box<WindowState>. Windows
// dispatches this HWND on its creating thread. Clear the pointer at destruction.
pub(super) unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(l.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        (*(create.lpCreateParams as *const WindowState))
            .app
            .borrow_mut()
            .hwnd = hwnd;
    }
    // These frame responses are stateless and must also apply during SetWindowPos.
    // Falling back to the default procedure would insert a native resize border.
    if msg == WM_NCCALCSIZE {
        return LRESULT(0);
    }
    if msg == WM_NCPAINT || msg == WM_ERASEBKGND {
        return LRESULT(1);
    }
    if msg == WM_DESTROY {
        PostQuitMessage(0);
        return LRESULT(0);
    }
    if msg == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowState;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let state = &*ptr;
    let Ok(mut app) = state.app.try_borrow_mut() else {
        let flag = match msg {
            STATUS_MESSAGE => 1,
            CONTROL_MESSAGE => 2,
            DRAW | WM_PAINT => 4,
            WM_SIZE => 8,
            WM_DEVICECHANGE if w.0 == 7 => 16,
            _ => 0,
        };
        state.deferred.set(state.deferred.get() | flag);
        return DefWindowProcW(hwnd, msg, w, l);
    };
    let result = dispatch(&mut app, hwnd, msg, w, l);
    drop(app);
    flush_deferred(state, hwnd);
    // DefWindowProc can start a modal move/resize loop. Never hold a borrow here.
    result.unwrap_or_else(|| DefWindowProcW(hwnd, msg, w, l))
}

pub(super) unsafe fn flush_deferred(state: &WindowState, hwnd: HWND) {
    let flags = state.deferred.replace(0);
    for (flag, message) in [
        (1, STATUS_MESSAGE),
        (2, CONTROL_MESSAGE),
        (4, DRAW),
        (8, WM_SIZE),
        (16, WM_DEVICECHANGE),
    ] {
        if flags & flag != 0 {
            let _ = PostMessageW(
                Some(hwnd),
                message,
                WPARAM(if message == WM_DEVICECHANGE { 7 } else { 0 }),
                LPARAM(0),
            );
        }
    }
}

pub(super) unsafe fn dispatch(
    app: &mut App,
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> Option<LRESULT> {
    match msg {
        WM_DEVICECHANGE => {
            // DBT_DEVNODES_CHANGED is broadcast to top-level windows when USB
            // topology changes. Coalesce it; never enumerate on the UI thread.
            if w.0 == 7 {
                if let Some(controller) = &app.controller {
                    controller.devices_changed();
                }
            }
            Some(LRESULT(0))
        }
        WM_NCCALCSIZE => Some(LRESULT(0)),
        WM_NCPAINT | WM_ERASEBKGND => Some(LRESULT(1)),
        WM_PAINT => {
            let _ = ValidateRect(Some(hwnd), None);
            request_draw(app);
            Some(LRESULT(0))
        }
        DRAW => {
            if !app.smoke
                && !app.capture
                && IsWindowVisible(hwnd).as_bool()
                && (app.animation_timer
                    || app
                        .last_frame
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(16)))
            {
                // Keep one pending redraw. The existing animation timer will
                // render the newest state, rather than enqueueing intermediate frames.
                start_animation_timer(app);
                if !app.animation_timer {
                    // A failed native timer must not strand the pending redraw.
                    render(app);
                }
            } else {
                render(app);
            }
            Some(LRESULT(0))
        }
        m if m == WM_APP + 7 => {
            let _ = DestroyWindow(hwnd);
            Some(LRESULT(0))
        }
        WM_NCHITTEST => {
            if !app.interactive {
                return Some(LRESULT(HTTRANSPARENT as isize));
            }
            let (x, y) = local(app, l, true);
            let (w, h) = logical_size(app);
            if app.edit && !app.config.locked && app.settings.is_none() {
                let left = x < 8.;
                let right = x > w - 8.;
                let top = y < 8.;
                let bottom = y > h - 8.;
                let ht = match (left, right, top, bottom) {
                    (true, _, true, _) => HTTOPLEFT,
                    (_, true, true, _) => HTTOPRIGHT,
                    (true, _, _, true) => HTBOTTOMLEFT,
                    (_, true, _, true) => HTBOTTOMRIGHT,
                    (true, _, _, _) => HTLEFT,
                    (_, true, _, _) => HTRIGHT,
                    (_, _, true, _) => HTTOP,
                    (_, _, _, true) => HTBOTTOM,
                    _ => HTCLIENT,
                };
                if ht != HTCLIENT {
                    return Some(LRESULT(ht as isize));
                }
            }
            if y < 68. && hit(app, x, y) == Action::None && !app.config.locked {
                Some(LRESULT(HTCAPTION as isize))
            } else {
                Some(LRESULT(HTCLIENT as isize))
            }
        }
        WM_GETMINMAXINFO => {
            let m = &mut *(l.0 as *mut MINMAXINFO);
            let s = GetDpiForWindow(hwnd).max(96) as f32 / 96.;
            m.ptMinTrackSize = POINT {
                x: (app.config.minimum_width() as f32 * s) as i32,
                y: (app.config.minimum_height() as f32 * s) as i32,
            };
            m.ptMaxTrackSize = POINT {
                x: (3200. * s) as i32,
                y: (2400. * s) as i32,
            };
            Some(LRESULT(0))
        }
        WM_SIZE => {
            clip_window(app);
            request_draw(app);
            Some(LRESULT(0))
        }
        WM_EXITSIZEMOVE => {
            if !app.config.locked {
                let mut r = RECT::default();
                let _ = GetWindowRect(hwnd, &mut r);
                if app.config.snap {
                    let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                    let mut mi = MONITORINFO {
                        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    let _ = GetMonitorInfoW(mon, &mut mi);
                    let w = r.right - r.left;
                    let h = r.bottom - r.top;
                    let mut x = r.left;
                    let mut y = r.top;
                    if (x - mi.rcWork.left).abs() < 24 {
                        x = mi.rcWork.left
                    }
                    if (r.right - mi.rcWork.right).abs() < 24 {
                        x = mi.rcWork.right - w
                    }
                    if (y - mi.rcWork.top).abs() < 24 {
                        y = mi.rcWork.top
                    }
                    if (r.bottom - mi.rcWork.bottom).abs() < 24 {
                        y = mi.rcWork.bottom - h
                    }
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        x,
                        y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    r.left = x;
                    r.top = y;
                }
                app.config.position = Some((r.left, r.top));
                if app.settings.is_none() {
                    let (w, h) = logical_size(app);
                    app.config.resize_canvas(w.round() as u32, h.round() as u32);
                }
                save(app);
            }
            Some(LRESULT(0))
        }
        WM_LBUTTONDOWN => {
            let (x, y) = local(app, l, false);
            let target = hit(app, x, y);
            act(app, target, x, y);
            Some(LRESULT(0))
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = local(app, l, false);
            if hit(app, x, y) == Action::Mix {
                set_mix(app, 50);
            }
            Some(LRESULT(0))
        }
        WM_MOUSEMOVE => {
            let (x, y) = local(app, l, false);
            if !matches!(app.drag, Drag::None) {
                drag(app, x, y)
            } else {
                let target = hit(app, x, y);
                if app.hover != target {
                    app.hover = target;
                    request_draw(app);
                }
                let cursor = if matches!(target, Action::Card(_)) {
                    IDC_SIZEALL
                } else if matches!(target, Action::ResizeCard(_)) {
                    IDC_SIZENWSE
                } else if target != Action::None {
                    IDC_HAND
                } else {
                    IDC_ARROW
                };
                if let Ok(c) = LoadCursorW(None, cursor) {
                    SetCursor(Some(c));
                }
            }
            Some(LRESULT(0))
        }
        WM_LBUTTONUP => {
            if !matches!(app.drag, Drag::None) {
                app.drag_preview = None;
                let _ = KillTimer(Some(hwnd), REORDER);
                app.drag = Drag::None;
                let _ = ReleaseCapture();
                save(app);
                request_draw(app);
            }
            Some(LRESULT(0))
        }
        WM_CAPTURECHANGED => {
            if !matches!(app.drag, Drag::None) {
                save(app);
            }
            app.drag = Drag::None;
            app.drag_preview = None;
            let _ = KillTimer(Some(hwnd), REORDER);
            request_draw(app);
            Some(LRESULT(0))
        }
        WM_MOUSEWHEEL => {
            let (x, y) = local(app, l, true);
            app.wheel += (w.0 >> 16) as i16 as i32;
            let steps = app.wheel / 120;
            app.wheel %= 120;
            if steps != 0 {
                match hit(app, x, y) {
                    Action::Slider(key) => {
                        let (value, min, max) = dashboard::slider(&app.config, key);
                        slider_value(app, key, (value + steps as f32 * 5.).clamp(min, max));
                        save(app);
                    }
                    Action::EqBand(index) => {
                        if let Some(d) = app.eq_editor.draft() {
                            preview_eq_gain(app, index, d.value.bands[index].gain + steps as f32);
                        }
                        request_draw(app);
                    }
                    Action::Mix => {
                        set_mix(app, (app.mix_to as i32 + steps * 5).clamp(0, 100) as u8)
                    }
                    Action::Level if app.sound.sidetone == Some(true) => send(
                        app,
                        Command::SidetoneLevel(
                            (app.sound.sidetone_level.unwrap_or(0) as i32 + steps * 3).clamp(0, 100)
                                as u8,
                        ),
                    ),
                    _ => (),
                }
            }
            Some(LRESULT(0))
        }
        WM_KEYDOWN | WM_SYSKEYDOWN if app.recording => {
            record(app, w.0 as u32);
            request_draw(app);
            Some(LRESULT(0))
        }
        WM_KEYDOWN if app.eq_editor.name_edit.is_some() => {
            if w.0 == VK_RETURN.0 as usize {
                commit_eq_name(app);
            } else if w.0 == VK_ESCAPE.0 as usize {
                app.eq_editor.name_edit = None;
            } else if w.0 == 0x41 && GetKeyState(VK_CONTROL.0 as i32) < 0 {
                app.eq_editor.name_selected = true;
            }
            request_draw(app);
            Some(LRESULT(0))
        }
        WM_CHAR if app.eq_editor.name_edit.is_some() => {
            if let Some(ch) = char::from_u32(w.0 as u32) {
                type_eq_name(app, ch);
            }
            request_draw(app);
            Some(LRESULT(0))
        }
        WM_KEYDOWN if app.number_edit.is_some() => {
            if w.0 == VK_RETURN.0 as usize {
                commit_number(app);
            } else if w.0 == VK_ESCAPE.0 as usize {
                app.number_edit = None;
                app.note.clear();
            } else if w.0 == 0x41 && GetKeyState(VK_CONTROL.0 as i32) < 0 {
                app.number_edit.as_mut().unwrap().selected = true;
            }
            request_draw(app);
            Some(LRESULT(0))
        }
        WM_CHAR if app.number_edit.is_some() => {
            if let Some(ch) = char::from_u32(w.0 as u32) {
                app.number_edit.as_mut().unwrap().type_char(ch);
            }
            request_draw(app);
            Some(LRESULT(0))
        }
        WM_KEYDOWN if w.0 == VK_ESCAPE.0 as usize => {
            if app.edit {
                finish_layout(app);
            } else {
                hide(app)
            }
            Some(LRESULT(0))
        }
        WM_KILLFOCUS => {
            commit_eq_name(app);
            if !commit_number(app) {
                app.number_edit = None;
            }
            request_draw(app);
            Some(LRESULT(0))
        }
        WM_HOTKEY => {
            if app.recording {
                app.recorded_key = Some((app.config.hotkey_mod, app.config.hotkey_vk));
                request_draw(app);
            } else if w.0 as i32 == app.hotkey_id {
                toggle(app);
            }
            Some(LRESULT(0))
        }
        WM_TIMER if w.0 == TRAY_RETRY => {
            ensure_tray(app);
            Some(LRESULT(0))
        }
        WM_TIMER if w.0 == REORDER => {
            let _ = KillTimer(Some(hwnd), REORDER);
            if matches!(app.drag, Drag::Card { resize: false, .. }) {
                let (x, y) = app.drag_point;
                drag(app, x, y);
            }
            Some(LRESULT(0))
        }
        WM_TIMER if w.0 == HOLD => {
            let _ = KillTimer(Some(hwnd), HOLD);
            if app.config.fade_ms == 0 {
                hide(app)
            } else {
                app.fade_out = Some(Instant::now());
                start_animation_timer(app);
            }
            Some(LRESULT(0))
        }
        WM_TIMER if w.0 == TICK => {
            if let Some(t) = app.fade_out {
                let p = t.elapsed().as_secs_f32() * 1000. / app.config.fade_ms.max(1) as f32;
                if p >= 1. {
                    hide(app);
                    return Some(LRESULT(0));
                }
                app.alpha = (255. * (1. - p).powi(2)) as u8;
                present(app);
            } else if let Some(t) = app.fade_in {
                let p = (t.elapsed().as_secs_f32() / 0.12).min(1.);
                app.alpha = (255. * (1. - (1. - p).powi(3))) as u8;
                present(app);
                if app.alpha >= 230 {
                    if let Some(delivery) = app.telemetry_at.take() {
                        device::trace_present(delivery, app.alpha, app.sound.mix.map(|m| m.1));
                    }
                }
                if p >= 1. {
                    app.fade_in = None;
                }
            }
            let mix_active = app.mix_started.is_some();
            if let Some(t) = app.mix_started {
                if t.elapsed() > Duration::from_millis(MIX_ANIMATION_MS) {
                    app.mix_started = None;
                }
            }
            if mix_active || app.motion_active || app.draw_queued {
                render(app);
            }
            if app.fade_in.is_none()
                && app.fade_out.is_none()
                && app.mix_started.is_none()
                && !app.motion_active
            {
                stop_animation_timer(app);
            }
            Some(LRESULT(0))
        }
        WM_TIMER if w.0 == 3 && app.smoke => {
            smoke(app);
            Some(LRESULT(0))
        }
        STATUS_MESSAGE => {
            changed(app);
            Some(LRESULT(0))
        }
        CONTROL_MESSAGE => {
            if let Some(c) = &app.controller {
                app.note = c.last_error().unwrap_or_default().into();
                if !app.note.is_empty() {
                    app.eq_editor.failed();
                }
            }
            request_draw(app);
            Some(LRESULT(0))
        }
        TRAY => {
            match l.0 as u32 {
                WM_RBUTTONUP | WM_CONTEXTMENU => menu(app),
                WM_LBUTTONDBLCLK => {
                    if !confirm_recording(app) {
                        return Some(LRESULT(0));
                    }
                    app.settings = None;
                    show(app, true)
                }
                _ => (),
            }
            Some(LRESULT(0))
        }
        OPEN => {
            if !confirm_recording(app) {
                return Some(LRESULT(0));
            }
            app.settings = None;
            show(app, true);
            Some(LRESULT(0))
        }
        WM_DPICHANGED => {
            let r = &*(l.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            request_draw(app);
            Some(LRESULT(0))
        }
        WM_DISPLAYCHANGE => {
            app.monitors = monitors();
            position(app);
            request_draw(app);
            Some(LRESULT(0))
        }
        WM_CLOSE => {
            hide(app);
            Some(LRESULT(0))
        }
        m if m == app.taskbar && m != 0 => {
            app.tray_registration = Default::default();
            ensure_tray(app);
            Some(LRESULT(0))
        }
        _ => None,
    }
}

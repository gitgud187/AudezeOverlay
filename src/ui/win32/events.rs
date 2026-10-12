//! Win32 callbacks. The boxed WindowState outlives the HWND and message loop.
use super::*;

// SAFETY: lpCreateParams points to the run-owned Box<WindowState>. Windows
// dispatches this HWND on its creating thread. Clear the pointer at destruction.
pub(super) unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(l.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        let state = &*(create.lpCreateParams as *const WindowState);
        win_mut(&mut state.app.borrow_mut()).hwnd = hwnd;
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

/// The native key code as the logic's `Key`.
unsafe fn key_of(w: WPARAM) -> Key {
    if w.0 == VK_RETURN.0 as usize {
        Key::Enter
    } else if w.0 == VK_ESCAPE.0 as usize {
        Key::Escape
    } else if w.0 == 0x41 && GetKeyState(VK_CONTROL.0 as i32) < 0 {
        Key::SelectAll
    } else {
        Key::Other
    }
}

/// The hit-test answer Windows expects for a region: it moves and resizes the
/// window itself in response.
fn hit_test_code(region: Region) -> u32 {
    match region {
        Region::Transparent => HTTRANSPARENT as u32,
        Region::Client => HTCLIENT,
        Region::Caption => HTCAPTION,
        Region::Edge(Edge::TopLeft) => HTTOPLEFT,
        Region::Edge(Edge::TopRight) => HTTOPRIGHT,
        Region::Edge(Edge::BottomLeft) => HTBOTTOMLEFT,
        Region::Edge(Edge::BottomRight) => HTBOTTOMRIGHT,
        Region::Edge(Edge::Left) => HTLEFT,
        Region::Edge(Edge::Right) => HTRIGHT,
        Region::Edge(Edge::Top) => HTTOP,
        Region::Edge(Edge::Bottom) => HTBOTTOM,
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
                devices_changed(app);
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
            draw_requested(app);
            Some(LRESULT(0))
        }
        m if m == WM_APP + 7 => {
            let _ = DestroyWindow(hwnd);
            Some(LRESULT(0))
        }
        WM_NCHITTEST => {
            let (x, y) = local(app, l, true);
            Some(LRESULT(hit_test_code(region_at(app, x, y)) as i32 as isize))
        }
        WM_GETMINMAXINFO => {
            let m = &mut *(l.0 as *mut MINMAXINFO);
            let (min, max) = size_limits(app);
            m.ptMinTrackSize = POINT { x: min.0, y: min.1 };
            m.ptMaxTrackSize = POINT { x: max.0, y: max.1 };
            Some(LRESULT(0))
        }
        WM_SIZE => {
            resized(app);
            Some(LRESULT(0))
        }
        WM_EXITSIZEMOVE => {
            move_or_resize_finished(app);
            Some(LRESULT(0))
        }
        WM_LBUTTONDOWN => {
            let (x, y) = local(app, l, false);
            pointer_pressed(app, x, y);
            Some(LRESULT(0))
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = local(app, l, false);
            pointer_double_clicked(app, x, y);
            Some(LRESULT(0))
        }
        WM_MOUSEMOVE => {
            let (x, y) = local(app, l, false);
            pointer_moved(app, x, y);
            Some(LRESULT(0))
        }
        WM_LBUTTONUP => {
            pointer_released(app);
            Some(LRESULT(0))
        }
        WM_CAPTURECHANGED => {
            pointer_capture_lost(app);
            Some(LRESULT(0))
        }
        WM_MOUSEWHEEL => {
            let (x, y) = local(app, l, true);
            wheel(app, x, y, (w.0 >> 16) as i16 as i32);
            Some(LRESULT(0))
        }
        WM_KEYDOWN | WM_SYSKEYDOWN if app.recording => {
            record(app, w.0 as u32);
            request_draw(app);
            Some(LRESULT(0))
        }
        WM_KEYDOWN => key_pressed(app, key_of(w)).then_some(LRESULT(0)),
        WM_CHAR => {
            if app.eq_editor.name_edit.is_none() && app.number_edit.is_none() {
                return None;
            }
            match char::from_u32(w.0 as u32) {
                Some(ch) => {
                    text_typed(app, ch);
                }
                None => request_draw(app),
            }
            Some(LRESULT(0))
        }
        WM_KILLFOCUS => {
            focus_lost(app);
            Some(LRESULT(0))
        }
        WM_HOTKEY => {
            hotkey_pressed(app, w.0 as i32 == win(app).hotkey_id);
            Some(LRESULT(0))
        }
        WM_TIMER => match w.0 {
            TRAY_RETRY => {
                timer_fired(app, Timer::TrayRetry);
                Some(LRESULT(0))
            }
            REORDER => {
                timer_fired(app, Timer::Reorder);
                Some(LRESULT(0))
            }
            HOLD => {
                timer_fired(app, Timer::Hold);
                Some(LRESULT(0))
            }
            TICK => {
                timer_fired(app, Timer::Animation);
                Some(LRESULT(0))
            }
            3 if app.smoke => {
                smoke(app);
                Some(LRESULT(0))
            }
            _ => None,
        },
        STATUS_MESSAGE => {
            status_changed(app);
            Some(LRESULT(0))
        }
        CONTROL_MESSAGE => {
            command_finished(app);
            Some(LRESULT(0))
        }
        TRAY => {
            match l.0 as u32 {
                WM_RBUTTONUP | WM_CONTEXTMENU => tray_used(app, TrayEvent::Menu),
                WM_LBUTTONDBLCLK => tray_used(app, TrayEvent::Open),
                _ => (),
            }
            Some(LRESULT(0))
        }
        OPEN => {
            open_requested(app);
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
            displays_changed(app);
            Some(LRESULT(0))
        }
        WM_CLOSE => {
            close_requested(app);
            Some(LRESULT(0))
        }
        m if m == win(app).taskbar_message && m != 0 => {
            tray_host_restarted(app);
            Some(LRESULT(0))
        }
        _ => None,
    }
}

//! The Windows shell: the process entry point, the window procedure and the
//! message loop. The application logic is in `app`; this module only translates
//! between Windows messages and calls into it.
mod events;
use events::proc;
mod gdi;
#[cfg(feature = "golden-checks")]
mod golden;
mod keys;
mod shell;
mod smoke;
use super::{app::*, draw::Painter, shell::*, view::Action};
use audeze_overlay::{
    config::{valid_hotkey, Config},
    device,
    device::command::{Command, CONTROL_MESSAGE, STATUS_MESSAGE},
    layout::{self, Card},
    model::SoundState,
};
use gdi::*;
use keys::*;
use shell::{win, win_mut, WinShell};
use smoke::smoke;
use std::{
    cell::{Cell, RefCell},
    sync::{Arc, Mutex},
    time::Instant,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{LibraryLoader::*, Threading::*},
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
    },
};
const TRAY: u32 = WM_APP + 2;
const OPEN: u32 = WM_APP + 3;
const DRAW: u32 = WM_APP + 6;
const TICK: usize = 1;
const HOLD: usize = 2;
const REORDER: usize = 4;
const TRAY_RETRY: usize = 5;
// Windows may synchronously re-enter our callback during native API calls.
// RefCell prevents overlapping mutable access; queued signals are coalesced.
struct WindowState {
    app: RefCell<App>,
    deferred: Cell<u8>,
}
/// Delivers worker signals as the window messages the event handler expects.
/// Posting to a destroyed window fails harmlessly, so no lifetime is shared.
fn worker_signal(hwnd: isize) -> impl Fn(device::command::Signal) + Send + 'static {
    move |signal| {
        let message = match signal {
            device::command::Signal::Status => STATUS_MESSAGE,
            device::command::Signal::Control => CONTROL_MESSAGE,
        };
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as _)), message, WPARAM(0), LPARAM(0));
        }
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn main() {
    unsafe {
        if let Err(e) = run() {
            let _ = MessageBoxW(
                None,
                PCWSTR(wide(&format!("Could not start Audeze Overlay:\n{e}")).as_ptr()),
                w!("Audeze Overlay"),
                MB_ICONERROR | MB_OK,
            );
        }
    }
}
unsafe fn run() -> Result<()> {
    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    let mutex = CreateMutexW(None, false, w!("Local\\AudezeOverlay.Singleton"))?;
    if GetLastError() == ERROR_ALREADY_EXISTS {
        if let Ok(h) = FindWindowW(w!("AudezeOverlayWindow"), None) {
            let _ = PostMessageW(Some(h), OPEN, WPARAM(0), LPARAM(0));
        }
        let _ = CloseHandle(mutex);
        return Ok(());
    }
    let instance: HINSTANCE = GetModuleHandleW(None)?.into();
    let wc = WNDCLASSW {
        style: CS_DBLCLKS,
        lpfnWndProc: Some(proc),
        hInstance: instance,
        lpszClassName: w!("AudezeOverlayWindow"),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        ..Default::default()
    };
    if RegisterClassW(&wc) == 0 {
        return Err(Error::from_thread());
    }
    let args: Vec<_> = std::env::args().collect();
    let smoke = args.iter().any(|s| s == "--smoke-test");
    let capture = args.iter().any(|s| s == "--snapshot-preview");
    let demo = capture || smoke || args.iter().any(|s| s == "--demo");
    let latest = Arc::new(Mutex::new(SoundState::default()));
    let first = !Config::path().exists();
    let mut config = if smoke || capture {
        Config::default()
    } else {
        Config::load()
    };
    if let Some(i) = args.iter().position(|s| s == "--preview-layout") {
        if args.get(i + 1).map(String::as_str) == Some("stacked") {
            config.width = 500;
            config.height = 960;
            config.grid_rows = 30;
            config.cell_width = 476. / 12.;
            config.cell_height = 854. / 30.;
            config.cards[0] = Card {
                x: 0,
                y: 0,
                w: 12,
                h: 12,
                visible: true,
            };
            for n in 1..4 {
                config.cards[n] = Card {
                    x: 0,
                    y: 12 + (n as i32 - 1) * 5,
                    w: 12,
                    h: 5,
                    visible: true,
                };
            }
            config.cards[4] = Card {
                x: 0,
                y: 27,
                w: 12,
                h: 3,
                visible: true,
            };
        }
    }
    let remembered_page = config.settings_page;
    let startup_note = if demo {
        String::new()
    } else {
        crate::platform::startup::reconcile(config.autostart)
            .err()
            .unwrap_or_default()
    };
    let shell = WinShell::new(RegisterWindowMessageW(w!("TaskbarCreated")));
    let settings = if args.iter().any(|s| s == "--settings") {
        Some(
            args.iter()
                .position(|s| s == "--section")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(remembered_page)
                .min(5),
        )
    } else {
        None
    };
    let app = App::new(
        Box::new(shell),
        config,
        Launch {
            demo,
            capture,
            smoke,
            settings,
            note: startup_note.into(),
        },
        latest.clone(),
        Painter::new(),
    );
    let (width, height) = (app.config.width, app.config.height);
    let state = Box::new(WindowState {
        app: RefCell::new(app),
        deferred: Cell::new(0),
    });
    let raw = state.as_ref() as *const WindowState;
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
        w!("AudezeOverlayWindow"),
        w!("Audeze Overlay"),
        WS_POPUP | WS_THICKFRAME,
        0,
        0,
        width as i32,
        height as i32,
        None,
        None,
        Some(instance),
        Some(raw.cast()),
    )?;
    {
        let mut app = state.app.borrow_mut();
        win_mut(&mut app).hwnd = hwnd;
        let icon = create_tray_icon(&app.painter);
        win_mut(&mut app).tray_icon = icon;
        app.shell.hide_window();
        let (modifiers, key) = (app.config.hotkey_mod, app.config.hotkey_vk);
        app.hotkey_registered = app.shell.register_hotkey(modifiers, key);
        if !app.hotkey_registered {
            app.note = "The shortcut is already in use. Open Shortcuts to choose another.".into();
        }
        ensure_tray(&mut app);
        if !demo {
            app.controller = Some(device::start(worker_signal(hwnd.0 as isize), latest));
        }
        if smoke {
            app.shell.start_timer(Timer::SelfTest, 50);
        } else if first
            || demo
            || !app.hotkey_registered
            || args.iter().any(|s| s == "--open" || s == "--settings")
        {
            show(&mut app, true);
        }
        if first && !demo {
            save(&mut app);
        }
    }
    events::flush_deferred(&state, hwnd);
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    state.app.borrow_mut().shell.shutdown();
    let _ = CloseHandle(mutex);
    Ok(())
}
unsafe fn local(app: &App, l: LPARAM, screen: bool) -> (f32, f32) {
    let mut p = POINT {
        x: l.0 as i16 as i32,
        y: (l.0 >> 16) as i16 as i32,
    };
    if screen {
        let _ = ScreenToClient(win(app).hwnd, &mut p);
    }
    let s = ui_scale(app);
    (p.x as f32 / s, p.y as f32 / s)
}

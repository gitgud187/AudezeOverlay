//! Windows message loop and application coordination.
//! Domain rules live in the library; child modules isolate painting and editing.
mod events;
use events::proc;
mod eq_profiles;
mod equalizer;
use eq_profiles::*;
mod layout_editor;
mod notice;
mod painting;
mod settings;
mod tray_registration;
use equalizer::*;
mod smoke;
use super::{
    draw::{Box2, Painter},
    view::{self as dashboard, Action, Hit, View},
};
use crate::platform::startup::set_enabled as startup;
use audeze_overlay::{
    config::{valid_hotkey, Config},
    device,
    device::command::{Command, Controller, CONTROL_MESSAGE, STATUS_MESSAGE},
    layout::{self, Card},
    model::{mix_from_point, reconcile_mix, MixTarget, SoundState},
};
use layout_editor::*;
use notice::Notice;
use painting::*;
use settings::*;
use smoke::smoke;
use std::{
    cell::{Cell, RefCell},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
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
const MIX_ANIMATION_MS: u64 = 70;
#[derive(Clone)]
struct Monitor {
    key: String,
    label: String,
    rect: RECT,
}
#[derive(Clone, Copy)]
enum Drag {
    Setting(&'static str),
    None,
    Mix,
    Level,
    EqBand(usize),
    Card {
        index: usize,
        resize: bool,
        x: f32,
        y: f32,
        original_cards: [Card; 6],
    },
}
// Windows may synchronously re-enter our callback during native API calls.
// RefCell prevents overlapping mutable access; queued signals are coalesced.
struct WindowState {
    app: RefCell<App>,
    deferred: Cell<u8>,
}
struct App {
    hwnd: HWND,
    config: Config,
    eq_editor: audeze_overlay::eq::Editor,
    sound: SoundState,
    latest: Arc<Mutex<SoundState>>,
    controller: Option<Controller>,
    painter: Painter,
    presentation: super::presentation::Presentation,
    motion_active: bool,
    bitmap: Option<Bitmap>,
    hits: Vec<Hit>,
    cards: [Box2; 6],
    settings: Option<usize>,
    edit: bool,
    interactive: bool,
    hover: Action,
    note: Notice,
    recording: bool,
    recorded_key: Option<(u32, u32)>,
    number_edit: Option<audeze_overlay::input::NumberDraft>,
    drag: Drag,
    reorder_hover: layout::HoverIntent,
    drag_point: (f32, f32),
    drag_preview: Option<(usize, Box2)>,
    demo: bool,
    capture: bool,
    hotkey_id: i32,
    hotkey_registered: bool,
    monitors: Vec<Monitor>,
    taskbar: u32,
    mix_from: f32,
    mix_to: f32,
    mix_started: Option<Instant>,
    pending_mix: Option<MixTarget>,
    fade_in: Option<Instant>,
    fade_out: Option<Instant>,
    alpha: u8,
    draw_queued: bool,
    telemetry_at: Option<device::Delivery>,
    applied_blur: Option<u8>,
    notification_options: bool,
    popup_cards: Option<u8>,
    tray_icon: HICON,
    tray_registration: tray_registration::TrayRegistration,
    low_latched: bool,
    wheel: i32,
    smoke: bool,
    smoke_phase: u8,
    checks: Vec<(String, bool)>,
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
    let app = App {
        hwnd: HWND::default(),
        config,
        eq_editor: audeze_overlay::eq::Editor::default(),
        sound: if demo {
            sample()
        } else {
            SoundState {
                status: "Connecting directly to the USB dongle…".into(),
                ..Default::default()
            }
        },
        latest: latest.clone(),
        controller: None,
        painter: Painter::new(),
        presentation: super::presentation::Presentation::default(),
        motion_active: false,
        bitmap: None,
        hits: vec![],
        cards: [Box2::default(); 6],
        settings: if args.iter().any(|s| s == "--settings") {
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
        },
        edit: false,
        interactive: false,
        hover: Action::None,
        note: startup_note.into(),
        recording: false,
        recorded_key: None,
        number_edit: None,
        drag: Drag::None,
        reorder_hover: layout::HoverIntent::default(),
        drag_point: (0., 0.),
        drag_preview: None,
        demo,
        capture,
        hotkey_id: 1,
        hotkey_registered: false,
        monitors: monitors(),
        taskbar: RegisterWindowMessageW(w!("TaskbarCreated")),
        mix_from: 50.,
        mix_to: 50.,
        mix_started: None,
        pending_mix: None,
        fade_in: None,
        fade_out: None,
        alpha: 255,
        draw_queued: false,
        telemetry_at: None,
        applied_blur: None,
        notification_options: false,
        popup_cards: None,
        tray_icon: HICON::default(),
        tray_registration: Default::default(),
        low_latched: false,
        wheel: 0,
        smoke,
        smoke_phase: 0,
        checks: vec![],
    };
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
        app.hwnd = hwnd;
        app.tray_icon = create_tray_icon(&app.painter);
        let _ = ShowWindow(hwnd, SW_HIDE);
        app.hotkey_registered = RegisterHotKey(
            Some(hwnd),
            app.hotkey_id,
            HOT_KEY_MODIFIERS(app.config.hotkey_mod) | MOD_NOREPEAT,
            app.config.hotkey_vk,
        )
        .is_ok();
        if !app.hotkey_registered {
            app.note = "The shortcut is already in use. Open Shortcuts to choose another.".into();
        }
        ensure_tray(&mut app);
        if !demo {
            app.controller = Some(device::start(hwnd.0 as isize, latest));
        }
        if smoke {
            SetTimer(Some(hwnd), 3, 50, None);
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
    let app = state.app.borrow();
    tray(&app, NIM_DELETE);
    if !app.tray_icon.is_invalid() {
        let _ = DestroyIcon(app.tray_icon);
    }
    let _ = UnregisterHotKey(Some(hwnd), app.hotkey_id);
    let _ = CloseHandle(mutex);
    Ok(())
}
fn sample() -> SoundState {
    SoundState {
        mix_revision: 0,
        eq_index: Some(0),
        eq_bands: None,
        eq_revision: 0,
        eq_index_revision: 0,
        mix: Some((50, 50)),
        sidetone: Some(false),
        sidetone_level: Some(26),
        anc: Some("Transparency".into()),
        noise: Some("High".into()),
        battery: Some(70),
        muted: Some(true),
        device: Some("Maxwell 2 ANC".into()),
        connection: Some("USB Dongle".into()),
        status: "Preview · sample data".into(),
        ..Default::default()
    }
}
unsafe extern "system" fn enum_monitor(h: HMONITOR, _: HDC, _: *mut RECT, p: LPARAM) -> BOOL {
    let list = &mut *(p.0 as *mut Vec<Monitor>);
    let mut i = MONITORINFOEXW::default();
    i.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(h, &mut i.monitorInfo).as_bool() {
        let key = String::from_utf16_lossy(
            &i.szDevice[..i.szDevice.iter().position(|n| *n == 0).unwrap_or(32)],
        );
        list.push(Monitor {
            key,
            label: format!("Screen {}", list.len() + 1),
            rect: i.monitorInfo.rcWork,
        });
    }
    BOOL(1)
}
unsafe fn monitors() -> Vec<Monitor> {
    let mut m = vec![];
    let _ = EnumDisplayMonitors(
        None,
        None,
        Some(enum_monitor),
        LPARAM(&mut m as *mut _ as isize),
    );
    m
}
unsafe fn monitor_rect(app: &App) -> RECT {
    if let Some(m) = app.monitors.iter().find(|m| m.key == app.config.monitor) {
        return m.rect;
    }
    let m = if let Some((x, y)) = app.config.position {
        MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST)
    } else {
        MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST)
    };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let _ = GetMonitorInfoW(m, &mut info);
    info.rcWork
}
unsafe fn position(app: &App) {
    let scale = GetDpiForWindow(app.hwnd).max(96) as f32 / 96.;
    let r = monitor_rect(app);
    let (w, h) = if app.settings.is_some() {
        (900., 750.)
    } else if let Some(mask) = app.popup_cards {
        let popup = app.config.notification_layout(mask);
        (popup.width as f32, popup.height as f32)
    } else {
        (app.config.width as f32, app.config.height as f32)
    };
    let fit = if app.settings.is_some() {
        scale
            .min((r.right - r.left) as f32 / w)
            .min((r.bottom - r.top) as f32 / h)
    } else {
        scale
    };
    let w = (w * fit).round() as i32;
    let h = (h * fit).round() as i32;
    let (x, y) = app
        .config
        .position
        .unwrap_or((r.right - w - 20, r.top + 20));
    let x = x.clamp(r.left, (r.right - w).max(r.left));
    let y = y.clamp(r.top, (r.bottom - h).max(r.top));
    let mut current = RECT::default();
    let _ = GetWindowRect(app.hwnd, &mut current);
    if (
        current.left,
        current.top,
        current.right - current.left,
        current.bottom - current.top,
    ) != (x, y, w, h)
    {
        let _ = SetWindowPos(app.hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
    }
}
unsafe fn save(app: &mut App) {
    if !app.demo {
        if let Err(e) = app.config.save() {
            app.note = format!("Could not save settings: {e}").into();
        }
    }
}
unsafe fn request_draw(app: &mut App) {
    if !app.draw_queued {
        app.draw_queued = true;
        let _ = PostMessageW(Some(app.hwnd), DRAW, WPARAM(0), LPARAM(0));
    }
}
unsafe fn show(app: &mut App, interactive: bool) {
    if interactive {
        if let Some(controller) = &app.controller {
            controller.devices_changed();
        }
        app.popup_cards = None;
        if app.settings == Some(5) {
            open_eq(app);
        } else if app.config.cards[5].visible {
            send(app, Command::EqRefresh);
        }
    }
    let was_visible = IsWindowVisible(app.hwnd).as_bool();
    let _ = KillTimer(Some(app.hwnd), HOLD);
    app.interactive = interactive;
    app.fade_out = None;
    app.recording = false;
    app.number_edit = None;
    if !interactive {
        app.settings = None;
        app.edit = false;
    }
    let blur = app.config.blur_strength;
    if app.applied_blur != Some(blur) {
        let _ = crate::platform::blur::apply_strength(app.hwnd, blur);
        app.applied_blur = Some(blur);
    }
    let style = WS_EX_TOPMOST
        | WS_EX_TOOLWINDOW
        | WS_EX_LAYERED
        | if interactive {
            WINDOW_EX_STYLE(0)
        } else {
            WS_EX_NOACTIVATE | WS_EX_TRANSPARENT
        };
    if GetWindowLongPtrW(app.hwnd, GWL_EXSTYLE) as u32 != style.0 {
        SetWindowLongPtrW(app.hwnd, GWL_EXSTYLE, style.0 as isize);
    }
    if !IsWindowVisible(app.hwnd).as_bool() {
        app.alpha = if interactive && app.config.animations && !app.capture {
            0
        } else {
            255
        };
        app.fade_in = (app.alpha == 0).then(Instant::now);
    } else {
        app.alpha = 255;
        app.fade_in = None;
    }
    position(app);
    // Replacing the retained bitmap before showing prevents a one-frame flash
    // of the mix value from the previous popup.
    if !was_visible {
        render_frame(app, true);
    }
    let _ = ShowWindow(
        app.hwnd,
        if interactive {
            SW_SHOW
        } else {
            SW_SHOWNOACTIVATE
        },
    );
    // Acrylic/style transitions need the same clip even when size is unchanged.
    clip_window(app);
    request_draw(app);
    if interactive {
        let _ = SetForegroundWindow(app.hwnd);
    } else {
        SetTimer(Some(app.hwnd), HOLD, app.config.duration_ms, None);
    }
    if app.fade_in.is_some() {
        SetTimer(Some(app.hwnd), TICK, 16, None);
    }
}
unsafe fn hide(app: &mut App) {
    if !confirm_recording(app) {
        return;
    }
    commit_eq_name(app);
    if app.edit {
        finish_layout(app);
    }
    if !matches!(app.drag, Drag::None) {
        save(app);
    }
    app.interactive = false;
    app.edit = false;
    app.recording = false;
    app.number_edit = None;
    app.fade_in = None;
    app.fade_out = None;
    app.motion_active = false;
    app.drag = Drag::None;
    let _ = ReleaseCapture();
    let _ = KillTimer(Some(app.hwnd), TICK);
    let _ = KillTimer(Some(app.hwnd), HOLD);
    let _ = ShowWindow(app.hwnd, SW_HIDE);
}
unsafe fn toggle(app: &mut App) {
    if IsWindowVisible(app.hwnd).as_bool() && app.interactive {
        hide(app)
    } else {
        app.settings = None;
        show(app, true)
    }
}
unsafe fn tray(app: &App, op: NOTIFY_ICON_MESSAGE) -> bool {
    let mut n = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: app.hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: TRAY,
        hIcon: app.tray_icon,
        ..Default::default()
    };
    let tip = wide("Audeze Overlay · Direct USB");
    n.szTip[..tip.len()].copy_from_slice(&tip);
    Shell_NotifyIconW(op, &n).as_bool()
}
unsafe fn ensure_tray(app: &mut App) {
    let _ = KillTimer(Some(app.hwnd), TRAY_RETRY);
    if !app.tray_registration.can_retry() {
        return;
    }
    let added = tray(app, NIM_ADD);
    if app.tray_registration.complete(added) {
        // Explorer can be busy/unavailable at logon. This timer does no USB work
        // and stops on success or after one minute; TaskbarCreated starts anew.
        SetTimer(Some(app.hwnd), TRAY_RETRY, 2000, None);
    }
}
unsafe fn menu(app: &mut App) {
    if !confirm_recording(app) {
        return;
    }
    let Ok(m) = CreatePopupMenu() else { return };
    for (id, s) in [
        (1, "Open"),
        (2, "Settings"),
        (3, "Reset position"),
        (4, "Hide"),
        (5, "Quit"),
    ] {
        let _ = AppendMenuW(m, MF_STRING, id, PCWSTR(wide(s).as_ptr()));
    }
    let mut p = POINT::default();
    let _ = GetCursorPos(&mut p);
    let _ = SetForegroundWindow(app.hwnd);
    let id = TrackPopupMenu(
        m,
        TPM_RETURNCMD | TPM_NONOTIFY,
        p.x,
        p.y,
        None,
        app.hwnd,
        None,
    )
    .0;
    match id {
        1 => {
            app.settings = None;
            show(app, true)
        }
        2 => {
            app.settings = Some(app.config.settings_page);
            show(app, true)
        }
        3 => reset(app),
        4 => hide(app),
        5 => {
            let _ = DestroyWindow(app.hwnd);
        }
        _ => (),
    }
    let _ = DestroyMenu(m);
    let _ = PostMessageW(Some(app.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
}
unsafe fn reset(app: &mut App) {
    app.config.position = None;
    save(app);
    show(app, true);
}
unsafe fn send(app: &mut App, c: Command) {
    if app.demo {
        match c {
            Command::Mix(p) => app.sound.mix = Some((100 - p, p)),
            Command::Sidetone(b) => app.sound.sidetone = Some(b),
            Command::SidetoneLevel(n) => app.sound.sidetone_level = Some(n),
            Command::Anc(s) => app.sound.anc = Some(s),
            Command::Noise(s) => app.sound.noise = Some(s),
            Command::EqSelect(slot) => app.sound.eq_index = Some(slot),
            Command::EqRefresh => (),
            Command::EqLoad(slot) => app.sound.eq_bands = Some(audeze_overlay::eq::sample(slot)),
            Command::EqGain { slot, band } => {
                if let Some(eq) = app.sound.eq_bands.as_mut().filter(|eq| eq.slot == slot) {
                    if let Some(b) = eq.bands.iter_mut().find(|b| b.id == band.id) {
                        *b = band;
                    }
                }
            }
        }
        receive_eq(app);
        app.note = "Preview — headset unchanged".into();
    } else if app.sound.mix.is_none() {
        app.note = Notice::UntilConnected("Connect and turn on a supported headset.");
    } else if let Some(ctrl) = &app.controller {
        ctrl.clear_error();
        let mix = if let Command::Mix(value) = &c {
            Some(*value)
        } else {
            None
        };
        let revision = ctrl.send(c);
        if let Some(chat) = mix {
            app.pending_mix = Some(MixTarget { revision, chat });
            app.sound.mix = Some((100 - chat, chat));
        }
        app.note.clear();
    }
    request_draw(app);
}
unsafe fn set_mix(app: &mut App, p: u8) {
    let p = ((p as f32 / 5.).round() as u8 * 5).min(100);
    if app.sound.mix == Some((100 - p, p)) {
        return;
    }
    send(app, Command::Mix(p));
    let target = app.sound.mix.map(|mix| mix.1 as f32).unwrap_or(app.mix_to);
    if app.mix_to != target {
        app.mix_from = displayed_mix(app);
        app.mix_to = target;
        app.mix_started = (app.config.animations && !app.smoke).then(Instant::now);
        if app.mix_started.is_some() {
            SetTimer(Some(app.hwnd), TICK, 16, None);
        }
    }
}
unsafe fn act(app: &mut App, action: Action, x: f32, y: f32) {
    app.note.clear();
    if app.recording
        && matches!(
            action,
            Action::Tab(_)
                | Action::Back
                | Action::Settings
                | Action::Edit
                | Action::Close
                | Action::EqEditor
        )
        && !confirm_recording(app)
    {
        return;
    }
    if !matches!(action, Action::Number(_) | Action::None) && !commit_number(app) {
        return;
    }
    if action != Action::EqRename {
        commit_eq_name(app);
    }
    match action {
        Action::EqMenu => eq_menu(app),
        Action::EqProfiles => profile_menu(app),
        Action::EqSaveNew => save_profile_copy(app),
        Action::EqEditor => { app.settings = Some(5); app.config.settings_page = 5; save(app); show(app, true); },
        Action::EqInspect(slot) => inspect_eq(app, slot),
        Action::EqUse(slot) => { app.eq_editor.preview_slot = None; send(app, Command::EqSelect(slot)); },
        Action::EqPin(slot) => { app.config.eq.pinned[slot as usize] = !app.config.eq.pinned[slot as usize]; save(app); },
        Action::EqBand(index) => { app.drag = Drag::EqBand(index); SetCapture(app.hwnd); set_eq_band(app, index, y); },
        Action::EqSave => save_eq(app),
        Action::EqDiscard => discard_eq(app),
        Action::EqFlat => { for i in 0..10 { preview_eq_gain(app, i, 0.); } },
        Action::EqRename => { if app.eq_editor.selected >= 6 { app.eq_editor.name_edit = Some(app.config.eq.name(app.eq_editor.selected).into()); app.eq_editor.name_selected = true; } },
        Action::EqReload => inspect_eq(app, app.eq_editor.selected),
        Action::Close => hide(app),
        Action::Settings => {
            if app.edit {
                finish_layout(app);
            }
            app.settings = Some(app.config.settings_page);
            show(app, true)
        }
        Action::Back => {
            app.settings = None;
            show(app, true)
        }
        Action::Tab(i) => {
            app.settings = Some(i);
            app.config.settings_page = i;
            if i == 5 { open_eq(app); }
            save(app);
        }
        Action::Edit => {
            if app.config.locked {
                app.note = "Position is locked. Unlock it in Startup & device.".into();
            } else {
                app.settings = None;
                if app.edit {
                    finish_layout(app);
                } else {
                    app.edit = true;
                }
                show(app, true);
            }
        }
        Action::CardMenu(i) => card_menu(app, Some(i)),
        Action::AddCard => card_menu(app, None),
        Action::StatusMenu => card_menu(app, Some(6)),
        Action::RemoveCard(i) => card_visibility(app, i, false),
        Action::RestoreCard(i) => card_visibility(app, i, true),
        Action::Mix => {
            app.drag = Drag::Mix;
            SetCapture(app.hwnd);
            drag(app, x, y)
        }
        Action::Level if app.sound.sidetone == Some(true) => {
            app.drag = Drag::Level;
            SetCapture(app.hwnd);
            drag(app, x, y)
        }
        Action::Side => {
            if let Some(b) = app.sound.sidetone {
                send(app, Command::Sidetone(!b));
            }
        }
        Action::Anc(i) => send(
            app,
            Command::Anc(["On", "Off", "Transparency"][i as usize].into()),
        ),
        Action::Noise(i) => send(app, Command::Noise(["Off", "Low", "High"][i as usize].into())),
        Action::Card(i) | Action::ResizeCard(i) if app.edit && !app.config.locked => {
            app.reorder_hover = layout::HoverIntent::default();
            app.drag_point = (x,y);
            app.drag_preview = None;
            app.drag = Drag::Card {
                index: i,
                resize: matches!(action, Action::ResizeCard(_)),
                x,
                y,
                original_cards: app.config.cards,
            };
            SetCapture(app.hwnd);
        }
        Action::Setting(key, n) => setting(app, key, n),
        Action::Slider(key) => {
            app.drag = Drag::Setting(key);
            SetCapture(app.hwnd);
            set_slider(app, key, x);
        }
        Action::Record => { app.recording = true; app.recorded_key = None; }
        Action::EndRecord => finish_recording(app),
        Action::DiscardRecord => { app.recording = false; app.recorded_key = None; },
        Action::NotificationOptions => app.notification_options = !app.notification_options,
        Action::Number(key) => {
            if commit_number(app) { app.number_edit = Some(audeze_overlay::input::NumberDraft::new(key, dashboard::slider(&app.config, key).0 as u32)); }
        }
        Action::ResetApp if MessageBoxW(Some(app.hwnd), w!("Reset all overlay preferences, layout and shortcuts?\nYour headset sound settings will not change."), w!("Reset Audeze Overlay"), MB_OKCANCEL | MB_ICONQUESTION) == IDOK => { factory_reset(app); }
        _ => (),
    };
    request_draw(app);
}
unsafe fn local(app: &App, l: LPARAM, screen: bool) -> (f32, f32) {
    let mut p = POINT {
        x: l.0 as i16 as i32,
        y: (l.0 >> 16) as i16 as i32,
    };
    if screen {
        let _ = ScreenToClient(app.hwnd, &mut p);
    }
    let mut s = GetDpiForWindow(app.hwnd).max(96) as f32 / 96.;
    if app.settings.is_some() {
        let mut r = RECT::default();
        let _ = GetClientRect(app.hwnd, &mut r);
        s = (r.right as f32 / 900.).min(r.bottom as f32 / 750.);
    }
    (p.x as f32 / s, p.y as f32 / s)
}
fn hit(app: &App, x: f32, y: f32) -> Action {
    app.hits
        .iter()
        .rev()
        .find(|h| h.rect.contains(x, y))
        .map(|h| h.action)
        .unwrap_or(Action::None)
}
unsafe fn changed(app: &mut App) {
    let mut next = app.latest.lock().unwrap().clone();
    if next.family != app.sound.family
        || next.transport != app.sound.transport
        || next.xbox != app.sound.xbox
    {
        app.pending_mix = None;
        app.eq_editor = Default::default();
        if let Some(family) = next.family {
            app.config.select_eq_profile(family, next.xbox);
        }
    }
    reconcile_mix(&mut app.pending_mix, &mut next);
    let connected_now = app.sound.mix.is_none() && next.mix.is_some();
    if connected_now {
        app.note.connected();
    }
    let eq_changed = next.eq_index != app.sound.eq_index;
    let prev = &app.sound;
    let changed_cards = next.changed_cards(prev, app.config.notify);
    let popup = changed_cards != 0;
    let low = next
        .battery
        .map(|b| app.config.low_battery > 0 && b <= app.config.low_battery)
        .unwrap_or(false);
    let alert = low && !app.low_latched;
    app.low_latched = low;
    if alert {
        app.note = format!("Low headset battery: {}%", next.battery.unwrap()).into();
    }
    if next.mix != prev.mix {
        app.mix_from = displayed_mix(app);
        app.mix_to = next.mix.map(|m| m.1).unwrap_or(50) as f32;
        app.mix_started =
            if app.config.animations && prev.mix.is_some() && IsWindowVisible(app.hwnd).as_bool() {
                Some(Instant::now())
            } else {
                None
            };
    }
    app.sound = next;
    receive_eq(app);
    if connected_now && app.settings == Some(5) {
        open_eq(app);
    }
    if app.settings.is_none()
        && app.config.inline_eq
        && app.config.cards[5].visible
        && (eq_changed || connected_now)
    {
        if let Some(slot) = app.sound.eq_index {
            inspect_eq(app, slot);
        }
    }
    if (popup || alert) && app.config.auto_show && !app.interactive {
        app.popup_cards = app
            .config
            .changed_card_only
            .then_some(changed_cards | if alert { 1 << 4 } else { 0 });
        show(app, false);
    }
    if IsWindowVisible(app.hwnd).as_bool() {
        app.telemetry_at = device::delivery_time();
        request_draw(app);
        if app.mix_started.is_some() {
            SetTimer(Some(app.hwnd), TICK, 16, None);
        }
    }
}

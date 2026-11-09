//! The macOS entry point: an AppKit window around the shared application logic.
//!
//! The window is a borderless, non-activating panel at a level above ordinary
//! windows, backed by a layer whose contents are the CPU-rasterized frame. The
//! portable `App` decides what to draw; this shell translates its requests into
//! AppKit calls and translates timer and worker events back into app input.
use super::{app::*, draw::Painter, instance, shell::*};
use audeze_overlay::{
    config::Config,
    device::{self, command::Signal},
    model::SoundState,
};
use core_foundation_sys::{
    base::{kCFAllocatorDefault, CFRelease, CFTypeRef},
    date::CFAbsoluteTimeGetCurrent,
    runloop::{
        kCFRunLoopDefaultMode, CFRunLoopAddSource, CFRunLoopAddTimer, CFRunLoopGetCurrent,
        CFRunLoopGetMain, CFRunLoopSourceContext, CFRunLoopSourceCreate, CFRunLoopSourceRef,
        CFRunLoopSourceSignal, CFRunLoopTimerContext, CFRunLoopTimerCreate,
        CFRunLoopTimerInvalidate, CFRunLoopTimerRef, CFRunLoopWakeUp,
    },
};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::AnyObject, runtime::Sel, sel, MainThreadMarker,
    MainThreadOnly,
};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSApplicationActivationPolicy,
    NSAutoresizingMaskOptions, NSBackingStoreType, NSColor, NSCursor, NSEvent,
    NSEventModifierFlags, NSImage, NSMenu, NSMenuItem, NSModalResponseOK, NSOpenPanel, NSSavePanel,
    NSScreen, NSScreenSaverWindowLevel, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
    NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSVisualEffectView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CFData, CFRetained};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
};
use objc2_foundation::{
    NSArray, NSObject, NSOperatingSystemVersion, NSPoint, NSProcessInfo, NSRect, NSSize, NSString,
};
use objc2_service_management::SMAppService;
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
    rc::Rc,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    thread,
};
use tiny_skia::Pixmap;

type Result<T, E = String> = std::result::Result<T, E>;

const TIMER_COUNT: usize = 4;

thread_local! {
    /// The running application state. AppKit callbacks all run on the main
    /// thread, where this pointer is set for the lifetime of the run loop.
    static STATE: Cell<*mut State> = const { Cell::new(ptr::null_mut()) };
}

fn with_state(action: impl FnOnce(&mut State)) {
    let state = STATE.with(Cell::get);
    if !state.is_null() {
        // SAFETY: the pointer is set in `run` and cleared before `State` drops.
        action(unsafe { &mut *state });
    }
}

/// Coalesces worker notifications into two flags, as the Wayland shell does.
#[derive(Default)]
struct Pending(AtomicU8);
impl Pending {
    fn push(&self, signal: Signal) -> bool {
        let bit = match signal {
            Signal::Status => 1,
            Signal::Control => 2,
        };
        self.0.fetch_or(bit, Ordering::AcqRel) == 0
    }
    fn take(&self) -> (bool, bool) {
        let bits = self.0.swap(0, Ordering::AcqRel);
        (bits & 1 != 0, bits & 2 != 0)
    }
}

struct State {
    app: App,
    pending: Arc<Pending>,
    requests: Arc<Mutex<Vec<instance::Request>>>,
}

pub fn main() {
    if let Err(error) = run() {
        eprintln!("Could not start Audeze Overlay: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(unknown) = args
        .iter()
        .skip(1)
        .find(|a| !matches!(a.as_str(), "--demo" | "--toggle"))
    {
        return Err(format!(
            "unknown option {unknown} (options: --demo, --toggle)"
        ));
    }
    let demo = args.iter().any(|a| a == "--demo");
    let request = if args.iter().any(|a| a == "--toggle") {
        instance::Request::Toggle
    } else {
        instance::Request::Open
    };
    let socket = instance::socket_path().filter(|_| !demo);
    let instance = match &socket {
        Some(path) => match instance::acquire(path, request)
            .map_err(|error| format!("single-instance startup failed: {error}"))?
        {
            instance::Role::Forwarded => return Ok(()),
            instance::Role::Primary(instance) => Some(instance),
        },
        None => None,
    };
    let mtm = MainThreadMarker::new()
        .ok_or("The overlay must start on the macOS main thread.".to_string())?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let painter = Painter::try_new()?;
    let tray_icon = tray_icon_pixmap(&painter, 32);
    let core = Rc::new(RefCell::new(Core::new(mtm, tray_icon)?));
    let shell = MacShell { core: core.clone() };
    let config = if demo {
        Config::default()
    } else {
        Config::load()
    };
    let latest = Arc::new(Mutex::new(SoundState::default()));
    let launch = Launch {
        demo,
        capture: false,
        smoke: false,
        settings: None,
        note: "".into(),
    };
    let mut state = Box::new(State {
        app: App::new(Box::new(shell), config, launch, latest.clone(), painter),
        pending: Arc::new(Pending::default()),
        requests: Arc::new(Mutex::new(Vec::new())),
    });
    STATE.with(|cell| cell.set(&mut *state as *mut State));

    if !demo {
        let pending = state.pending.clone();
        let wake = core.borrow().redraw as usize;
        state.app.controller = Some(device::start(
            move |signal| {
                if !pending.push(signal) {
                    return;
                }
                // The source belongs to the main run loop; signalling it is
                // thread-safe and the perform callback runs on the main thread.
                unsafe {
                    CFRunLoopSourceSignal(wake as CFRunLoopSourceRef);
                    CFRunLoopWakeUp(CFRunLoopGetMain());
                }
            },
            latest,
        ));
    }

    if let Some(instance) = instance {
        instance
            .listener
            .set_nonblocking(false)
            .map_err(|e| e.to_string())?;
        let requests = state.requests.clone();
        let wake = core.borrow().redraw as usize;
        thread::Builder::new()
            .name("instance-listener".into())
            .spawn(move || {
                for stream in instance.listener.incoming() {
                    let Ok(stream) = stream else {
                        continue;
                    };
                    let Some(request) = instance::read_request(stream) else {
                        continue;
                    };
                    requests.lock().unwrap().push(request);
                    unsafe {
                        CFRunLoopSourceSignal(wake as CFRunLoopSourceRef);
                        CFRunLoopWakeUp(CFRunLoopGetMain());
                    }
                }
            })
            .map_err(|e| e.to_string())?;
    }

    app.finishLaunching();
    ensure_tray(&mut state.app);
    show(&mut state.app, true);
    app.run();
    STATE.with(|cell| cell.set(ptr::null_mut()));
    state.app.shell.shutdown();
    Ok(())
}

fn with_state_value<R>(action: impl FnOnce(&mut State) -> R) -> Option<R> {
    let state = STATE.with(Cell::get);
    if state.is_null() {
        None
    } else {
        // SAFETY: the pointer is set in `run` and cleared before `State` drops.
        Some(action(unsafe { &mut *state }))
    }
}

#[derive(Clone, Copy)]
struct ResizeDrag {
    edge: Edge,
    start: NSPoint,
    frame: NSRect,
}

thread_local! {
    static RESIZE: Cell<Option<ResizeDrag>> = const { Cell::new(None) };
}

define_class!(
    // SAFETY: NSView has no subclassing requirements that this type violates,
    // and the view is only ever used on the main thread.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "AudezeOverlayView"]
    struct OverlayView;

    impl OverlayView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: &NSEvent) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let (x, y) = self.app_point(event);
            if let Some(window) = self.window() {
                window.makeKeyWindow();
            }
            let Some(region) = with_state_value(|state| region_at(&state.app, x, y)) else {
                return;
            };
            match region {
                Region::Caption => {
                    // A native move loop handles the drag; report the result once
                    // it returns.
                    if let Some(window) = self.window() {
                        window.performWindowDragWithEvent(event);
                    }
                    with_state(|state| move_or_resize_finished(&mut state.app));
                }
                Region::Edge(edge) => {
                    let frame = self.window().map(|window| window.frame()).unwrap_or_default();
                    RESIZE.with(|slot| {
                        slot.set(Some(ResizeDrag {
                            edge,
                            start: NSEvent::mouseLocation(),
                            frame,
                        }))
                    });
                }
                Region::Client => with_state(|state| {
                    if event.clickCount() == 2 {
                        pointer_double_clicked(&mut state.app, x, y);
                    } else {
                        pointer_pressed(&mut state.app, x, y);
                    }
                }),
                Region::Transparent => {}
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if let Some(drag) = RESIZE.with(Cell::get) {
                self.resize(drag);
                return;
            }
            let (x, y) = self.app_point(event);
            with_state(|state| pointer_moved(&mut state.app, x, y));
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            if RESIZE.with(Cell::get).is_some() {
                RESIZE.with(|slot| slot.set(None));
                with_state(|state| move_or_resize_finished(&mut state.app));
            } else {
                with_state(|state| pointer_released(&mut state.app));
            }
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let (x, y) = self.app_point(event);
            with_state(|state| pointer_moved(&mut state.app, x, y));
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let (x, y) = self.app_point(event);
            let delta = if event.hasPreciseScrollingDeltas() {
                (event.scrollingDeltaY() * 8.0).round() as i32
            } else {
                (event.scrollingDeltaY() * 120.0).round() as i32
            };
            if delta != 0 {
                with_state(|state| wheel(&mut state.app, x, y, delta));
            }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let control = event
                .modifierFlags()
                .contains(NSEventModifierFlags::Control);
            let Some(text) = event.characters() else {
                with_state(|state| {
                    key_pressed(&mut state.app, Key::Other);
                });
                return;
            };
            for character in text.to_string().chars() {
                let key = match character {
                    '\r' | '\u{3}' => Some(Key::Enter),
                    '\u{1b}' => Some(Key::Escape),
                    'a' | 'A' if control => Some(Key::SelectAll),
                    _ => None,
                };
                if let Some(key) = key {
                    with_state(|state| {
                        key_pressed(&mut state.app, key);
                    });
                } else if !character.is_control() {
                    with_state(|state| {
                        text_typed(&mut state.app, character);
                    });
                }
            }
        }
    }
);

impl OverlayView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// Event location in the app's logical coordinates, matching the view and
    /// hit regions (the same conversion the Wayland shell performs).
    fn app_point(&self, event: &NSEvent) -> (f32, f32) {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let scale = self
            .window()
            .map(|window| window.backingScaleFactor())
            .unwrap_or(1.) as f32;
        let ui = with_state_value(|state| ui_scale(&state.app)).unwrap_or(scale);
        (point.x as f32 * scale / ui, point.y as f32 * scale / ui)
    }

    fn resize(&self, drag: ResizeDrag) {
        let Some(window) = self.window() else {
            return;
        };
        let now = NSEvent::mouseLocation();
        let (dx, dy) = (now.x - drag.start.x, now.y - drag.start.y);
        let left = matches!(drag.edge, Edge::Left | Edge::TopLeft | Edge::BottomLeft);
        let right = matches!(drag.edge, Edge::Right | Edge::TopRight | Edge::BottomRight);
        let top = matches!(drag.edge, Edge::Top | Edge::TopLeft | Edge::TopRight);
        let bottom = matches!(
            drag.edge,
            Edge::Bottom | Edge::BottomLeft | Edge::BottomRight
        );
        let mut frame = drag.frame;
        if left {
            frame.origin.x = drag.frame.origin.x + dx;
            frame.size.width = drag.frame.size.width - dx;
        }
        if right {
            frame.size.width = drag.frame.size.width + dx;
        }
        if top {
            frame.size.height = drag.frame.size.height + dy;
        }
        if bottom {
            frame.origin.y = drag.frame.origin.y + dy;
            frame.size.height = drag.frame.size.height - dy;
        }

        let ((min_width, min_height), (max_width, max_height)) =
            with_state_value(|state| size_limits(&state.app)).unwrap_or(((232, 160), (3200, 2400)));
        let scale = window.backingScaleFactor();
        let min_width = f64::from(min_width) / scale;
        let min_height = f64::from(min_height) / scale;
        let max_width = f64::from(max_width) / scale;
        let max_height = f64::from(max_height) / scale;
        let right_edge = drag.frame.origin.x + drag.frame.size.width;
        let top_edge = drag.frame.origin.y + drag.frame.size.height;
        if frame.size.width < min_width {
            frame.size.width = min_width;
            if left {
                frame.origin.x = right_edge - min_width;
            }
        } else if frame.size.width > max_width {
            frame.size.width = max_width;
            if left {
                frame.origin.x = right_edge - max_width;
            }
        }
        if frame.size.height < min_height {
            frame.size.height = min_height;
            if bottom {
                frame.origin.y = top_edge - min_height;
            }
        } else if frame.size.height > max_height {
            frame.size.height = max_height;
            if bottom {
                frame.origin.y = top_edge - max_height;
            }
        }
        window.setFrame_display(frame, true);
        with_state(|state| resized(&mut state.app));
    }
}

define_class!(
    // SAFETY: NSWindow has no subclassing requirements that this type violates,
    // and the window is only ever used on the main thread.
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[name = "AudezeOverlayWindow"]
    struct OverlayWindow;

    impl OverlayWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            true
        }

        #[unsafe(method(resignKeyWindow))]
        fn resign_key(&self) {
            with_state(|state| {
                focus_lost(&mut state.app);
                pointer_capture_lost(&mut state.app);
            });
        }
    }
);

impl OverlayWindow {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        unsafe {
            msg_send![
                super(this),
                initWithContentRect: frame,
                styleMask: NSWindowStyleMask::Borderless,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        }
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and the target is only
    // used from AppKit's main thread.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AudezeOverlayTrayTarget"]
    struct TrayTarget;

    impl TrayTarget {
        #[unsafe(method(open:))]
        fn open(&self, _: &AnyObject) {
            with_state(|state| tray_command(&mut state.app, TrayCommand::Open));
        }

        #[unsafe(method(settings:))]
        fn settings(&self, _: &AnyObject) {
            with_state(|state| tray_command(&mut state.app, TrayCommand::Settings));
        }

        #[unsafe(method(resetPosition:))]
        fn reset_position(&self, _: &AnyObject) {
            with_state(|state| tray_command(&mut state.app, TrayCommand::ResetPosition));
        }

        #[unsafe(method(hidePanel:))]
        fn hide_panel(&self, _: &AnyObject) {
            with_state(|state| tray_command(&mut state.app, TrayCommand::Hide));
        }

        #[unsafe(method(quitApp:))]
        fn quit_app(&self, _: &AnyObject) {
            with_state(|state| tray_command(&mut state.app, TrayCommand::Quit));
        }
    }
);

impl TrayTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    static MENU_SELECTION: Cell<Option<u32>> = const { Cell::new(None) };
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and the target is only
    // used from AppKit's main thread.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AudezeOverlayMenuTarget"]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(pick:))]
        fn pick(&self, sender: &NSMenuItem) {
            MENU_SELECTION.with(|cell| cell.set(Some(sender.tag() as u32)));
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn build_menu(
    entries: &[MenuEntry],
    target: &MenuTarget,
    mtm: MainThreadMarker,
) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    for entry in entries {
        match entry {
            MenuEntry::Item {
                id,
                label,
                checked,
                enabled,
            } => {
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        mtm.alloc(),
                        &NSString::from_str(label),
                        Some(sel!(pick:)),
                        &NSString::from_str(""),
                    )
                };
                unsafe { item.setTarget(Some(target as &AnyObject)) };
                item.setTag(*id as isize);
                item.setEnabled(*enabled);
                item.setState(if *checked { 1 } else { 0 });
                menu.addItem(&item);
            }
            MenuEntry::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            MenuEntry::Submenu { label, entries } => {
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        mtm.alloc(),
                        &NSString::from_str(label),
                        None,
                        &NSString::from_str(""),
                    )
                };
                let submenu = build_menu(entries, target, mtm);
                item.setSubmenu(Some(&submenu));
                menu.addItem(&item);
            }
        }
    }
    menu
}

struct Core {
    window: Retained<OverlayWindow>,
    view: Retained<OverlayView>,
    layer: Option<Retained<objc2_quartz_core::CALayer>>,
    visible: bool,
    interactive: bool,
    corners: i32,
    alpha: u8,
    frame: Option<CFRetained<CGImage>>,
    timers: [Option<CFRunLoopTimerRef>; TIMER_COUNT],
    redraw: CFRunLoopSourceRef,
    tray_icon: Option<Pixmap>,
    status_item: Option<Retained<NSStatusItem>>,
    tray_target: Option<Retained<TrayTarget>>,
    effect: Retained<NSVisualEffectView>,
}
impl Core {
    fn new(mtm: MainThreadMarker, tray_icon: Option<Pixmap>) -> Result<Self> {
        let frame = NSRect::new(NSPoint::new(200., 200.), NSSize::new(440., 420.));
        let window = OverlayWindow::new(mtm, frame);
        window.setLevel(NSScreenSaverWindowLevel);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(false);
        window.setIgnoresMouseEvents(true);
        window.setAcceptsMouseMovedEvents(true);

        let content = NSRect::new(NSPoint::new(0., 0.), frame.size);
        let effect = NSVisualEffectView::new(mtm);
        effect.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
        effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        effect.setState(NSVisualEffectState::Active);
        effect.setFrame(content);
        effect.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        effect.setHidden(true);

        let view = OverlayView::new(mtm, content);
        view.setWantsLayer(true);
        view.setFrame(content);
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        effect.addSubview(&view);
        window.setContentView(Some(&effect));
        let layer = view.layer();

        let mut context = CFRunLoopSourceContext {
            version: 0,
            info: ptr::null_mut(),
            retain: None,
            release: None,
            copyDescription: None,
            equal: None,
            hash: None,
            schedule: None,
            cancel: None,
            perform: redraw_performed,
        };
        let redraw = unsafe { CFRunLoopSourceCreate(kCFAllocatorDefault, 0, &mut context) };
        if redraw.is_null() {
            return Err("The macOS redraw source could not be created.".into());
        }
        unsafe { CFRunLoopAddSource(CFRunLoopGetCurrent(), redraw, kCFRunLoopDefaultMode) };

        Ok(Self {
            window,
            view,
            layer,
            visible: false,
            interactive: false,
            corners: 0,
            alpha: 255,
            frame: None,
            timers: [None; TIMER_COUNT],
            redraw,
            tray_icon,
            status_item: None,
            tray_target: None,
            effect,
        })
    }
    fn present(&mut self) {
        let Some(layer) = &self.layer else {
            return;
        };
        if let Some(frame) = &self.frame {
            let contents: &AnyObject = (**frame).as_ref();
            unsafe { layer.setContents(Some(contents)) };
            let scale = self.window.backingScaleFactor();
            layer.setContentsScale(scale);
            layer.setCornerRadius(f64::from(self.corners) / 2.0 / scale);
            layer.setMasksToBounds(self.corners > 0);
        }
        layer.setOpacity(f32::from(self.alpha) / 255.0);
    }
    fn stop_timer(&mut self, index: usize) {
        if let Some(timer) = self.timers.get_mut(index).and_then(Option::take) {
            unsafe {
                CFRunLoopTimerInvalidate(timer);
                CFRelease(timer as CFTypeRef);
            }
        }
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        for index in 0..TIMER_COUNT {
            self.stop_timer(index);
        }
        if !self.redraw.is_null() {
            unsafe { CFRelease(self.redraw as CFTypeRef) };
        }
    }
}

struct MacShell {
    core: Rc<RefCell<Core>>,
}
impl MacShell {
    fn quit_now(&self) {
        NSApplication::sharedApplication(mtm()).stop(None);
    }
}

// ---- AppKit callbacks.

extern "C" fn redraw_performed(_: *const c_void) {
    with_state(|state| {
        let requests = {
            let mut queue = state.requests.lock().unwrap();
            std::mem::take(&mut *queue)
        };
        for request in requests {
            match request {
                instance::Request::Open => open_requested(&mut state.app),
                instance::Request::Toggle => toggle(&mut state.app),
            }
        }
        let (status, control) = state.pending.take();
        if status {
            status_changed(&mut state.app);
        }
        if control {
            command_finished(&mut state.app);
        }
        draw_requested(&mut state.app);
    });
}

extern "C" fn timer_fired_callback(_: CFRunLoopTimerRef, info: *mut c_void) {
    let index = unsafe { *(info as *const usize) };
    let Some(timer) = timer_from_index(index) else {
        return;
    };
    with_state(|state| timer_fired(&mut state.app, timer));
}

extern "C" fn release_timer_context(info: *const c_void) {
    if !info.is_null() {
        drop(unsafe { Box::from_raw(info as *mut usize) });
    }
}

fn timer_index(timer: Timer) -> usize {
    match timer {
        Timer::Animation => 0,
        Timer::Hold => 1,
        Timer::Reorder => 2,
        Timer::TrayRetry => 3,
    }
}

fn timer_from_index(index: usize) -> Option<Timer> {
    match index {
        0 => Some(Timer::Animation),
        1 => Some(Timer::Hold),
        2 => Some(Timer::Reorder),
        3 => Some(Timer::TrayRetry),
        _ => None,
    }
}

fn mtm() -> MainThreadMarker {
    // Every caller is an AppKit callback or a Shell method invoked from them.
    unsafe { MainThreadMarker::new_unchecked() }
}

fn primary_height() -> f64 {
    NSScreen::screens(mtm())
        .firstObject()
        .map(|screen| screen.frame().size.height)
        .unwrap_or(0.)
}

fn screen_work(screen: &NSScreen) -> Rect {
    let frame = screen.visibleFrame();
    let scale = screen.backingScaleFactor();
    let left = frame.origin.x;
    let top = primary_height() - (frame.origin.y + frame.size.height);
    Rect {
        left: (left * scale).round() as i32,
        top: (top * scale).round() as i32,
        right: ((left + frame.size.width) * scale).round() as i32,
        bottom: ((top + frame.size.height) * scale).round() as i32,
    }
}

fn window_scale(window: &NSWindow) -> f64 {
    window
        .screen()
        .map(|screen| screen.backingScaleFactor())
        .unwrap_or_else(|| {
            NSScreen::screens(mtm())
                .firstObject()
                .map(|screen| screen.backingScaleFactor())
                .unwrap_or(1.)
        })
}

fn image_from(pixels: &Pixmap) -> Result<CFRetained<CGImage>> {
    let data = CFData::from_bytes(pixels.data());
    let provider = CGDataProvider::with_cf_data(Some(&data))
        .ok_or("The macOS frame data provider could not be created.")?;
    let space =
        CGColorSpace::new_device_rgb().ok_or("The macOS RGB color space could not be created.")?;
    let info = CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0);
    unsafe {
        CGImage::new(
            pixels.width() as usize,
            pixels.height() as usize,
            8,
            32,
            pixels.width() as usize * 4,
            Some(&space),
            info,
            Some(&provider),
            ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
    .ok_or_else(|| "The macOS frame image could not be created.".to_string())
}

// ---- Shell.

impl Shell for MacShell {
    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn start_timer(&mut self, timer: Timer, ms: u32) -> bool {
        let index = timer_index(timer);
        let mut core = self.core.borrow_mut();
        core.stop_timer(index);
        let info = Box::into_raw(Box::new(index)).cast::<c_void>();
        let mut context = CFRunLoopTimerContext {
            version: 0,
            info,
            retain: None,
            release: Some(release_timer_context),
            copyDescription: None,
        };
        let interval = f64::from(ms.max(1)) / 1000.;
        let created = unsafe {
            CFRunLoopTimerCreate(
                kCFAllocatorDefault,
                CFAbsoluteTimeGetCurrent() + interval,
                interval,
                0,
                0,
                timer_fired_callback,
                &mut context,
            )
        };
        if created.is_null() {
            unsafe { drop(Box::from_raw(info as *mut usize)) };
            return false;
        }
        unsafe { CFRunLoopAddTimer(CFRunLoopGetCurrent(), created, kCFRunLoopDefaultMode) };
        core.timers[index] = Some(created);
        true
    }
    fn stop_timer(&mut self, timer: Timer) {
        self.core.borrow_mut().stop_timer(timer_index(timer));
    }
    fn request_redraw(&self) {
        let redraw = self.core.borrow().redraw;
        if !redraw.is_null() {
            unsafe {
                CFRunLoopSourceSignal(redraw);
                CFRunLoopWakeUp(CFRunLoopGetMain());
            }
        }
    }
    fn capture_pointer(&mut self) {}
    fn release_pointer(&mut self) {}
    fn is_visible(&self) -> bool {
        self.core.borrow().visible
    }
    fn scale(&self) -> f32 {
        let core = self.core.borrow();
        window_scale(&core.window) as f32
    }
    fn client_size(&self) -> Option<(i32, i32)> {
        let core = self.core.borrow();
        let bounds = core.view.bounds();
        let scale = window_scale(&core.window);
        Some((
            (bounds.size.width * scale).round() as i32,
            (bounds.size.height * scale).round() as i32,
        ))
    }
    fn window_rect(&self) -> Rect {
        let core = self.core.borrow();
        let frame = core.window.frame();
        let scale = window_scale(&core.window);
        let left = frame.origin.x;
        let top = primary_height() - (frame.origin.y + frame.size.height);
        Rect {
            left: (left * scale).round() as i32,
            top: (top * scale).round() as i32,
            right: ((left + frame.size.width) * scale).round() as i32,
            bottom: ((top + frame.size.height) * scale).round() as i32,
        }
    }
    fn place(&mut self, target: Rect) {
        let (window, frame) = {
            let core = self.core.borrow();
            let scale = window_scale(&core.window);
            let left = f64::from(target.left) / scale;
            let top = f64::from(target.top) / scale;
            let width = f64::from(target.width()) / scale;
            let height = f64::from(target.height()) / scale;
            (
                core.window.clone(),
                NSRect::new(
                    NSPoint::new(left, primary_height() - top - height),
                    NSSize::new(width, height),
                ),
            )
        };
        window.setFrame_display(frame, false);
    }
    fn move_to(&mut self, x: i32, y: i32) {
        let (window, origin) = {
            let core = self.core.borrow();
            let scale = window_scale(&core.window);
            let frame = core.window.frame();
            (
                core.window.clone(),
                NSPoint::new(
                    f64::from(x) / scale,
                    primary_height() - f64::from(y) / scale - frame.size.height,
                ),
            )
        };
        window.setFrameOrigin(origin);
    }
    fn window_work_area(&self) -> Rect {
        let core = self.core.borrow();
        match core.window.screen() {
            Some(screen) => screen_work(&screen),
            None => monitor_by_index(0),
        }
    }
    fn set_cursor(&mut self, cursor: Cursor) {
        let cursor = match cursor {
            Cursor::Arrow => NSCursor::arrowCursor(),
            Cursor::Hand => NSCursor::pointingHandCursor(),
            Cursor::Move => NSCursor::closedHandCursor(),
            Cursor::ResizeDiagonal => NSCursor::crosshairCursor(),
        };
        cursor.set();
    }
    fn set_input_mode(&mut self, interactive: bool) {
        let (window, view) = {
            let mut core = self.core.borrow_mut();
            if core.interactive == interactive {
                return;
            }
            core.interactive = interactive;
            (core.window.clone(), core.view.clone())
        };
        window.setIgnoresMouseEvents(!interactive);
        if interactive {
            window.makeFirstResponder(Some(&view));
        } else {
            window.makeFirstResponder(None);
        }
    }
    fn show_window(&mut self, activate: bool) {
        let (window, view, interactive) = {
            let mut core = self.core.borrow_mut();
            core.visible = true;
            (core.window.clone(), core.view.clone(), core.interactive)
        };
        if interactive {
            window.makeFirstResponder(Some(&view));
        }
        if activate {
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm()).activateIgnoringOtherApps(true);
            window.makeKeyAndOrderFront(None);
        } else {
            window.orderFrontRegardless();
        }
    }
    fn hide_window(&mut self) {
        let window = {
            let mut core = self.core.borrow_mut();
            core.visible = false;
            core.window.clone()
        };
        window.orderOut(None);
    }
    fn focus(&mut self) {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm()).activateIgnoringOtherApps(true);
        let window = self.core.borrow().window.clone();
        window.makeKeyAndOrderFront(None);
    }
    fn set_blur(&mut self, strength: u8) {
        let (window, view, effect) = {
            let core = self.core.borrow();
            (core.window.clone(), core.view.clone(), core.effect.clone())
        };
        if strength > 0 {
            view.removeFromSuperview();
            window.setContentView(Some(&effect));
            effect.addSubview(&view);
            view.setFrame(effect.bounds());
            effect.setHidden(false);
        } else {
            view.removeFromSuperview();
            window.setContentView(Some(&view));
            effect.setHidden(true);
        }
    }
    fn clip_rounded(&mut self, diameter: i32) {
        self.core.borrow_mut().corners = diameter;
    }
    fn quit_later(&self) {
        self.quit_now();
    }
    fn quit(&mut self) {
        self.quit_now();
    }
    fn store_frame(&mut self, pixels: &Pixmap) -> bool {
        match image_from(pixels) {
            Ok(image) => {
                self.core.borrow_mut().frame = Some(image);
                true
            }
            Err(_) => false,
        }
    }
    fn present(&self, alpha: u8) {
        let mut core = self.core.borrow_mut();
        core.alpha = alpha;
        core.present();
    }
    fn monitors(&self) -> Vec<Monitor> {
        NSScreen::screens(mtm())
            .iter()
            .enumerate()
            .map(|(index, screen)| Monitor {
                key: monitor_key(&screen, index),
                label: screen.localizedName().to_string(),
                work: screen_work(&screen),
            })
            .collect()
    }
    fn work_area_near(&self, point: Option<(i32, i32)>) -> Rect {
        let core = self.core.borrow();
        let scale = window_scale(&core.window);
        let Some((x, y)) = point else {
            return match core.window.screen() {
                Some(screen) => screen_work(&screen),
                None => monitor_by_index(0),
            };
        };
        let px = f64::from(x) / scale;
        let py = f64::from(y) / scale;
        for screen in NSScreen::screens(mtm()).iter() {
            let frame = screen.frame();
            let top = primary_height() - (frame.origin.y + frame.size.height);
            if px >= frame.origin.x
                && px < frame.origin.x + frame.size.width
                && py >= top
                && py < top + frame.size.height
            {
                return screen_work(&screen);
            }
        }
        monitor_by_index(0)
    }
    fn popup_menu(&mut self, entries: &[MenuEntry], _: MenuOrigin) -> Option<u32> {
        if entries.is_empty() {
            return None;
        }
        let mtm = mtm();
        let target = MenuTarget::new(mtm);
        let menu = build_menu(entries, &target, mtm);
        let (window, view) = {
            let core = self.core.borrow();
            (core.window.clone(), core.view.clone())
        };
        let screen = NSEvent::mouseLocation();
        let window_point = window.convertPointFromScreen(screen);
        let location = view.convertPoint_fromView(window_point, None);
        MENU_SELECTION.with(|cell| cell.set(None));
        menu.popUpMenuPositioningItem_atLocation_inView(None, location, Some(&view));
        MENU_SELECTION.with(Cell::get)
    }
    fn confirm(&mut self, title: &str, message: &str, buttons: Buttons) -> Choice {
        let alert = NSAlert::new(mtm());
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(message));
        let (labels, choices): (&[&str], &[Choice]) = match buttons {
            Buttons::YesNoCancel => (
                &["Yes", "No", "Cancel"],
                &[Choice::Yes, Choice::No, Choice::Cancel],
            ),
            Buttons::YesNo => (&["Yes", "No"], &[Choice::Yes, Choice::No]),
            Buttons::OkCancel => (&["OK", "Cancel"], &[Choice::Ok, Choice::Cancel]),
        };
        for label in labels {
            alert.addButtonWithTitle(&NSString::from_str(label));
        }
        let index = (alert.runModal() - NSAlertFirstButtonReturn)
            .clamp(0, choices.len() as isize - 1) as usize;
        choices[index]
    }
    fn choose_file(&mut self, save: bool) -> std::result::Result<FileSelection, String> {
        let mtm = mtm();
        if save {
            let panel = NSSavePanel::savePanel(mtm);
            panel.setNameFieldStringValue(&NSString::from_str("My EQ.audeze-eq.json"));
            let types = NSArray::from_retained_slice(&[NSString::from_str("json")]);
            #[allow(deprecated)]
            panel.setAllowedFileTypes(Some(&types));
            if panel.runModal() != NSModalResponseOK {
                return Ok(FileSelection::Ready(None));
            }
            Ok(FileSelection::Ready(
                panel
                    .URL()
                    .and_then(|url| url.path())
                    .map(|path| std::path::PathBuf::from(path.to_string())),
            ))
        } else {
            let panel = NSOpenPanel::openPanel(mtm);
            panel.setAllowsMultipleSelection(false);
            let types = NSArray::from_retained_slice(&[
                NSString::from_str("json"),
                NSString::from_str("pnreq"),
            ]);
            #[allow(deprecated)]
            panel.setAllowedFileTypes(Some(&types));
            if panel.runModal() != NSModalResponseOK {
                return Ok(FileSelection::Ready(None));
            }
            Ok(FileSelection::Ready(
                panel
                    .URLs()
                    .firstObject()
                    .and_then(|url| url.path())
                    .map(|path| std::path::PathBuf::from(path.to_string())),
            ))
        }
    }
    fn register_hotkey(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn unregister_hotkey(&mut self) {}
    fn rebind_hotkey(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn tray_add(&mut self) -> bool {
        let mut core = self.core.borrow_mut();
        if core.status_item.is_some() {
            return true;
        }
        let mtm = mtm();
        let Some(icon_image) = core
            .tray_icon
            .as_ref()
            .and_then(|pixmap| image_from(pixmap).ok())
        else {
            return false;
        };
        let icon = NSImage::initWithCGImage_size(mtm.alloc(), &icon_image, NSSize::new(18., 18.));
        icon.setTemplate(true);

        let target = TrayTarget::new(mtm);
        let menu = NSMenu::new(mtm);
        {
            let add = |title: &str, action: Sel| {
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        mtm.alloc(),
                        &NSString::from_str(title),
                        Some(action),
                        &NSString::from_str(""),
                    )
                };
                unsafe { item.setTarget(Some(&*target as &AnyObject)) };
                menu.addItem(&item);
            };
            add("Open", sel!(open:));
            add("Settings", sel!(settings:));
            add("Reset position", sel!(resetPosition:));
            add("Hide", sel!(hidePanel:));
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            add("Quit", sel!(quitApp:));
        }

        let status =
            NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        #[allow(deprecated)]
        status.setImage(Some(&icon));
        status.setMenu(Some(&menu));
        core.status_item = Some(status);
        core.tray_target = Some(target);
        true
    }
    fn tray_remove(&mut self) {
        let status = {
            let mut core = self.core.borrow_mut();
            core.tray_target = None;
            core.status_item.take()
        };
        if let Some(status) = status {
            NSStatusBar::systemStatusBar().removeStatusItem(&status);
        }
    }
    fn set_autostart(&mut self, enabled: bool) -> std::result::Result<(), String> {
        let info = NSProcessInfo::processInfo();
        if !info.isOperatingSystemAtLeastVersion(NSOperatingSystemVersion {
            majorVersion: 13,
            minorVersion: 0,
            patchVersion: 0,
        }) {
            return Err("Launch at login requires macOS 13 or newer.".into());
        }
        let service = unsafe { SMAppService::mainAppService() };
        let result = if enabled {
            unsafe { service.registerAndReturnError() }
        } else {
            unsafe { service.unregisterAndReturnError() }
        };
        result.map_err(|error| error.localizedDescription().to_string())
    }
    fn shutdown(&mut self) {
        self.tray_remove();
    }
}

fn monitor_key(screen: &NSScreen, index: usize) -> String {
    let name = screen.localizedName().to_string();
    if name.is_empty() {
        format!("Screen {}", index + 1)
    } else {
        name
    }
}

fn monitor_by_index(index: usize) -> Rect {
    NSScreen::screens(mtm())
        .iter()
        .nth(index)
        .map(|screen| screen_work(&screen))
        .unwrap_or_default()
}

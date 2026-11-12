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
    base::{kCFAllocatorDefault, CFRelease, CFRetain, CFTypeRef},
    date::CFAbsoluteTimeGetCurrent,
    runloop::{
        kCFRunLoopDefaultMode, CFRunLoopAddSource, CFRunLoopAddTimer, CFRunLoopGetCurrent,
        CFRunLoopGetMain, CFRunLoopSourceContext, CFRunLoopSourceCreate, CFRunLoopSourceSignal,
        CFRunLoopTimerContext, CFRunLoopTimerCreate, CFRunLoopTimerInvalidate, CFRunLoopTimerRef,
        CFRunLoopWakeUp,
    },
};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::AnyObject, runtime::Sel, sel, MainThreadMarker,
    MainThreadOnly,
};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSApplicationActivationPolicy,
    NSAutoresizingMaskOptions, NSBackingStoreType, NSColor, NSCursor, NSEvent,
    NSEventModifierFlags, NSEventType, NSImage, NSImageResizingMode, NSMenu, NSMenuItem,
    NSModalResponseOK, NSOpenPanel, NSSavePanel, NSScreen, NSScreenSaverWindowLevel, NSStatusBar,
    NSStatusItem, NSVariableStatusItemLength, NSView, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CFData, CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGBitmapInfo, CGColorRenderingIntent,
    CGColorSpace, CGContext, CGDataProvider, CGImage, CGImageAlphaInfo, CGPath,
};
use objc2_foundation::{
    NSArray, NSEdgeInsets, NSObject, NSOperatingSystemVersion, NSPoint, NSProcessInfo, NSRect,
    NSSize, NSString,
};
use objc2_service_management::SMAppService;
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr::{self, NonNull},
    rc::Rc,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex, PoisonError,
    },
    thread,
};
use tiny_skia::Pixmap;

type Result<T, E = String> = std::result::Result<T, E>;

const TIMER_COUNT: usize = 4;

#[derive(Debug, PartialEq, Eq)]
enum KeyInput {
    Press(Key),
    Text(char),
}

/// Maps AppKit's character event to the shared input calls. macOS reports the
/// Delete key as DEL, while the drafts delete on the Backspace character the
/// other shells deliver. AppKit encodes arrows, Home/End, function keys and
/// forward-delete in the Unicode private-use range; those are commands, not
/// text, so they must not reach a draft.
fn key_input(character: char, select_all: bool, command: bool) -> Option<KeyInput> {
    match character {
        '\r' | '\u{3}' => Some(KeyInput::Press(Key::Enter)),
        '\u{1b}' => Some(KeyInput::Press(Key::Escape)),
        _ if select_all => Some(KeyInput::Press(Key::SelectAll)),
        '\u{7f}' | '\u{8}' => Some(KeyInput::Text('\u{8}')),
        _ if character.is_control()
            || ('\u{f700}'..='\u{f8ff}').contains(&character)
            || command =>
        {
            None
        }
        _ => Some(KeyInput::Text(character)),
    }
}

thread_local! {
    /// The running application state. AppKit callbacks all run on the main
    /// thread, so they reach it here instead of through a raw pointer.
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Runs `action` on the application state, or returns `None` when there is none
/// yet or it is already in use. A modal alert, panel or menu runs a nested event
/// loop while the app holds the state; a callback arriving then is skipped
/// instead of aliasing the borrow.
fn with_state<R>(action: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|cell| {
        let mut slot = cell.try_borrow_mut().ok()?;
        slot.as_mut().map(action)
    })
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
    let (demo, request) = super::parse_args()?;
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
    let pending = Arc::new(Pending::default());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut app_state = State {
        app: App::new(Box::new(shell), config, launch, latest.clone(), painter),
        pending: pending.clone(),
        requests: requests.clone(),
    };

    if !demo {
        let wake = core.borrow().redraw.clone();
        app_state.app.controller = Some(device::start(
            move |signal| {
                if pending.push(signal) {
                    wake.signal();
                }
            },
            latest,
        ));
        if let Some(controller) = &app_state.app.controller {
            controller.set_bluetooth_control(app_state.app.config.bluetooth_control);
        }
    }
    STATE.with(|cell| *cell.borrow_mut() = Some(app_state));

    if let Some(instance) = &instance {
        let listener = instance.listener.try_clone().map_err(|e| e.to_string())?;
        listener.set_nonblocking(false).map_err(|e| e.to_string())?;
        let wake = core.borrow().redraw.clone();
        thread::Builder::new()
            .name("instance-listener".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else {
                        continue;
                    };
                    let Some(request) = instance::read_request(stream) else {
                        continue;
                    };
                    requests
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(request);
                    wake.signal();
                }
            })
            .map_err(|e| e.to_string())?;
    }

    app.finishLaunching();
    with_state(|state| {
        ensure_tray(&mut state.app);
        show(&mut state.app, true);
    });
    app.run();
    // Dropping the owner unlinks the socket on a normal quit; the cloned
    // listener in the thread dies with the process.
    drop(instance);
    // Take the state out first so its drop runs outside the thread-local borrow.
    let finished = STATE.with(|cell| cell.borrow_mut().take());
    if let Some(mut state) = finished {
        state.app.shell.shutdown();
    }
    Ok(())
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
            let Some(region) = with_state(|state| region_at(&state.app, x, y)) else {
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
                Region::Client => {
                    with_state(|state| {
                    if event.clickCount() == 2 {
                        pointer_double_clicked(&mut state.app, x, y);
                    } else {
                        pointer_pressed(&mut state.app, x, y);
                    }
                    });
                }
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
            let flags = event.modifierFlags();
            let control = flags.contains(NSEventModifierFlags::Control);
            let command = flags.contains(NSEventModifierFlags::Command);
            // Control+A arrives as U+0001 in `characters`; only the unmodified
            // character still identifies the A key.
            let select_all = (control || command)
                && event
                    .charactersIgnoringModifiers()
                    .is_some_and(|text| text.to_string().eq_ignore_ascii_case("a"));
            let Some(text) = event.characters() else {
                with_state(|state| {
                    key_pressed(&mut state.app, Key::Other);
                });
                return;
            };
            for character in text.to_string().chars() {
                match key_input(character, select_all, command) {
                    Some(KeyInput::Press(key)) => {
                        with_state(|state| key_pressed(&mut state.app, key));
                    }
                    Some(KeyInput::Text(character)) => {
                        with_state(|state| text_typed(&mut state.app, character));
                    }
                    None => {}
                }
            }
        }
    }
);

impl OverlayView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        // SAFETY: `initWithFrame:` is NSView's designated initializer, and `this`
        // is a freshly allocated, unused instance of this subclass.
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
        let ui = with_state(|state| ui_scale(&state.app)).unwrap_or(scale);
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
            with_state(|state| size_limits(&state.app)).unwrap_or(((232, 160), (3200, 2400)));
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
            // NSWindow does its own key-window bookkeeping and posts the
            // notification here; skipping super leaves a half-resigned window.
            // SAFETY: calls the superclass implementation of the same selector.
            unsafe {
                let _: () = msg_send![super(self), resignKeyWindow];
            }
            // A modal alert/panel/menu can take key order while the shared app
            // holds &mut App. Calling focus_lost inline would alias that borrow,
            // so defer to the default run-loop mode, which resumes after the
            // modal loop returns.
            // SAFETY: `focusLostLater:` is defined below with the one object
            // argument `performSelector:withObject:afterDelay:` passes (nil).
            unsafe {
                let _: () = msg_send![
                    self,
                    performSelector: sel!(focusLostLater:),
                    withObject: None::<&AnyObject>,
                    afterDelay: 0.0f64
                ];
            }
        }

        #[unsafe(method(focusLostLater:))]
        fn focus_lost_later(&self, _: Option<&AnyObject>) {
            // Focus may have returned before the deferred call (for example
            // after a modal). Do not commit an edit that is still active.
            if self.isKeyWindow() {
                return;
            }
            RESIZE.with(|slot| slot.set(None));
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
        // SAFETY: NSWindow's designated initializer on a fresh, unused instance.
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

        #[unsafe(method(toggleBluetooth:))]
        fn toggle_bluetooth(&self, _: &AnyObject) {
            with_state(|state| {
                state.app.config.bluetooth_control = !state.app.config.bluetooth_control;
                save(&mut state.app);
            });
        }

        // AppKit asks before showing each item. The checkmark follows the saved
        // choice, so it stays right after a factory reset too.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            if item.action() == Some(sel!(toggleBluetooth:)) {
                let on = with_state(|state| state.app.config.bluetooth_control).unwrap_or(false);
                item.setState(if on { 1 } else { 0 });
            }
            true
        }
    }
);

impl TrayTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        // SAFETY: `init` on a freshly allocated, unused NSObject subclass.
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
        // SAFETY: `init` on a freshly allocated, unused NSObject subclass.
        unsafe { msg_send![super(this), init] }
    }
}

/// A menu item that sends `action` to `target`.
fn menu_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    target: Option<&AnyObject>,
) -> Retained<NSMenuItem> {
    // SAFETY: the title and key equivalent are valid strings, and every action
    // passed in is a selector the target implements with one object argument.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str(title),
            action,
            &NSString::from_str(""),
        )
    };
    // SAFETY: `setTarget:` is weak; callers keep the target alive as long as
    // the menu can be used.
    unsafe { item.setTarget(target) };
    item
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
                let item = menu_item(mtm, label, Some(sel!(pick:)), Some(target));
                item.setTag(*id as isize);
                item.setEnabled(*enabled);
                item.setState(if *checked { 1 } else { 0 });
                menu.addItem(&item);
            }
            MenuEntry::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            MenuEntry::Submenu { label, entries } => {
                let item = menu_item(mtm, label, None, None);
                let submenu = build_menu(entries, target, mtm);
                item.setSubmenu(Some(&submenu));
                menu.addItem(&item);
            }
        }
    }
    menu
}

/// The main run loop's wake-up source. Worker threads hold clones to ask for a
/// redraw; the source is released with its last owner.
struct RedrawSource(NonNull<c_void>);

// SAFETY: signalling a run-loop source and waking the main run loop are
// documented as thread-safe, and clones only ever do that.
unsafe impl Send for RedrawSource {}
// SAFETY: as above; every method takes `&self` and only signals and wakes.
unsafe impl Sync for RedrawSource {}

impl RedrawSource {
    /// Creates the source and adds it to the current (main) run loop.
    fn install() -> Result<Self> {
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
        // SAFETY: the context is copied by the call, and the Create rule makes
        // us the owner of the returned source.
        let source: NonNull<c_void> = NonNull::new(
            unsafe { CFRunLoopSourceCreate(kCFAllocatorDefault, 0, &mut context) }.cast(),
        )
        .ok_or("The macOS redraw source could not be created.")?;
        // SAFETY: the source is live and the current run loop is valid.
        unsafe {
            CFRunLoopAddSource(
                CFRunLoopGetCurrent(),
                source.as_ptr().cast(),
                kCFRunLoopDefaultMode,
            )
        };
        Ok(Self(source))
    }

    /// Runs the source's callback on the main thread soon.
    fn signal(&self) {
        // SAFETY: `self` owns a reference, so the source is live.
        unsafe {
            CFRunLoopSourceSignal(self.0.as_ptr().cast());
            CFRunLoopWakeUp(CFRunLoopGetMain());
        }
    }
}

impl Clone for RedrawSource {
    fn clone(&self) -> Self {
        // SAFETY: `self` owns a reference, so the source is live.
        unsafe { CFRetain(self.0.as_ptr() as CFTypeRef) };
        Self(self.0)
    }
}

impl Drop for RedrawSource {
    fn drop(&mut self) {
        // SAFETY: releases the one reference this value owns.
        unsafe { CFRelease(self.0.as_ptr() as CFTypeRef) };
    }
}

/// A repeating timer on the main run loop, identified to its callback by `Timer`
/// index. Dropping it invalidates the timer so it can no longer fire.
struct RunLoopTimer(NonNull<c_void>);

impl RunLoopTimer {
    fn start(timer: Timer, interval_ms: u32) -> Option<Self> {
        // The index travels in the context pointer itself; nothing is allocated.
        let mut context = CFRunLoopTimerContext {
            version: 0,
            info: timer_index(timer) as *mut c_void,
            retain: None,
            release: None,
            copyDescription: None,
        };
        let interval = f64::from(interval_ms.max(1)) / 1000.;
        // SAFETY: the context is copied by the call, the callback matches the
        // expected signature, and the Create rule makes us the owner.
        let created: NonNull<c_void> = NonNull::new(unsafe {
            CFRunLoopTimerCreate(
                kCFAllocatorDefault,
                CFAbsoluteTimeGetCurrent() + interval,
                interval,
                0,
                0,
                timer_fired_callback,
                &mut context,
            )
            .cast()
        })?;
        // SAFETY: the timer is live and the current run loop is valid.
        unsafe {
            CFRunLoopAddTimer(
                CFRunLoopGetCurrent(),
                created.as_ptr().cast(),
                kCFRunLoopDefaultMode,
            )
        };
        Some(Self(created))
    }
}

impl Drop for RunLoopTimer {
    fn drop(&mut self) {
        // SAFETY: we own the only reference; invalidation removes it from every run loop.
        unsafe {
            CFRunLoopTimerInvalidate(self.0.as_ptr().cast());
            CFRelease(self.0.as_ptr() as CFTypeRef);
        }
    }
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
    timers: [Option<RunLoopTimer>; TIMER_COUNT],
    redraw: RedrawSource,
    tray_icon: Option<Pixmap>,
    status_item: Option<Retained<NSStatusItem>>,
    tray_target: Option<Retained<TrayTarget>>,
    effect: Retained<NSVisualEffectView>,
    /// Corner diameter and scale the current blur mask was built for.
    mask_key: Option<(i32, f64)>,
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
        effect.setWantsLayer(true);
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

        let redraw = RedrawSource::install()?;

        Ok(Self {
            window,
            view,
            layer,
            visible: false,
            interactive: false,
            corners: 0,
            alpha: 255,
            frame: None,
            timers: [const { None }; TIMER_COUNT],
            redraw,
            tray_icon,
            status_item: None,
            tray_target: None,
            effect,
            mask_key: None,
        })
    }
    fn present(&mut self) {
        let scale = self.window.backingScaleFactor();
        let radius = f64::from(self.corners) / 2.0 / scale;
        let masked = self.corners > 0;
        if let Some(layer) = &self.layer {
            if let Some(frame) = &self.frame {
                let contents: &AnyObject = (**frame).as_ref();
                // SAFETY: a CGImage is a valid CALayer contents object.
                unsafe { layer.setContents(Some(contents)) };
                layer.setContentsScale(scale);
            }
            layer.setCornerRadius(radius);
            layer.setMasksToBounds(masked);
            layer.setOpacity(f32::from(self.alpha) / 255.0);
        }
    }
    /// `NSVisualEffectView` documents `maskImage` as the supported way to shape
    /// a behind-window material. A layer corner radius is inconsistent across
    /// macOS versions. The mask is one corner-size image with cap insets, so
    /// AppKit stretches the center and keeps all four corners sharp.
    fn update_blur_mask(&mut self) {
        if self.corners <= 0 {
            self.effect.setMaskImage(None);
            self.mask_key = None;
            return;
        }
        let scale = window_scale(&self.window);
        // The mask only depends on these two values, so it is rebuilt when one
        // of them changes, not on every layout.
        if self.mask_key == Some((self.corners, scale)) {
            return;
        }
        let logical = f64::from(self.corners) / scale;
        let Some(image) = corner_mask_image(self.corners.max(2) as usize) else {
            return;
        };
        let mask =
            NSImage::initWithCGImage_size(mtm().alloc(), &image, NSSize::new(logical, logical));
        let inset = logical / 2.;
        mask.setCapInsets(NSEdgeInsets {
            top: inset,
            left: inset,
            bottom: inset,
            right: inset,
        });
        mask.setResizingMode(IMAGE_RESIZING_STRETCH);
        self.effect.setMaskImage(Some(&mask));
        self.mask_key = Some((self.corners, scale));
    }
}

/// `NSImageResizingModeStretch`; the binding does not name the constant.
const IMAGE_RESIZING_STRETCH: NSImageResizingMode = NSImageResizingMode(1);

/// An opaque square of `pixels` per side with fully rounded (transparent) corners.
fn corner_mask_image(pixels: usize) -> Option<CFRetained<CGImage>> {
    let space = CGColorSpace::new_device_rgb()?;
    let info = CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0);
    // SAFETY: a null data pointer asks CoreGraphics to allocate and own the
    // bitmap; the width, height and row size are consistent for 8-bit RGBA.
    let context = unsafe {
        CGBitmapContextCreate(
            ptr::null_mut(),
            pixels,
            pixels,
            8,
            pixels * 4,
            Some(&space),
            info.0,
        )
    }?;
    let side = pixels as f64;
    let rect = CGRect::new(CGPoint::new(0., 0.), CGSize::new(side, side));
    CGContext::clear_rect(Some(&context), rect);
    // SAFETY: a null transform means the identity.
    let path = unsafe { CGPath::with_rounded_rect(rect, side / 2., side / 2., ptr::null()) };
    CGContext::add_path(Some(&context), Some(&path));
    CGContext::fill_path(Some(&context));
    CGBitmapContextCreateImage(Some(&context))
}

struct MacShell {
    core: Rc<RefCell<Core>>,
}
impl MacShell {
    fn quit_now(&self) {
        // Leave the headset's Bluetooth channel closed, not abandoned: a stale
        // session can keep it from accepting a new connection.
        device::bluetooth::close_open_channel();
        let app = NSApplication::sharedApplication(mtm());
        app.stop(None);
        // `stop` only takes effect after the current event. A source or timer
        // callback is not an event, so post one to wake `nextEvent`.
        if let Some(event) = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::ApplicationDefined,
            NSPoint::new(0., 0.),
            NSEventModifierFlags::empty(),
            0.,
            0,
            None,
            0,
            0,
            0,
        ) {
            app.postEvent_atStart(&event, true);
        }
    }
}

// ---- AppKit callbacks.

extern "C" fn redraw_performed(_: *const c_void) {
    with_state(|state| {
        let requests = {
            let mut queue = state
                .requests
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
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
    let Some(timer) = timer_from_index(info as usize) else {
        return;
    };
    with_state(|state| timer_fired(&mut state.app, timer));
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
    // Every caller is an AppKit callback or a Shell method invoked from one, and
    // `run` refuses to start off the main thread, so this cannot fail.
    MainThreadMarker::new().expect("AppKit callbacks run on the main thread")
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
    // SAFETY: the dimensions and row size describe the Pixmap's tightly packed
    // RGBA data, which the provider keeps alive; a null decode array is allowed.
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
        // Replacing the slot drops, and so invalidates, a timer already running.
        let started = RunLoopTimer::start(timer, ms);
        let running = started.is_some();
        self.core.borrow_mut().timers[timer_index(timer)] = started;
        running
    }
    fn stop_timer(&mut self, timer: Timer) {
        self.core.borrow_mut().timers[timer_index(timer)] = None;
    }
    fn request_redraw(&self) {
        // Clone out so the signal is sent with no borrow of the core held.
        let redraw = self.core.borrow().redraw.clone();
        redraw.signal();
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
        let mut core = self.core.borrow_mut();
        core.corners = diameter;
        core.update_blur_mask();
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
                let item = menu_item(mtm, title, Some(action), Some(&target));
                menu.addItem(&item);
            };
            add("Open", sel!(open:));
            add("Settings", sel!(settings:));
            add("Reset position", sel!(resetPosition:));
            add("Hide", sel!(hidePanel:));
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            add("Bluetooth control (experimental)", sel!(toggleBluetooth:));
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
        // SAFETY: plain ServiceManagement calls for this app's own login item;
        // failures come back as the error value.
        let result = unsafe {
            let service = SMAppService::mainAppService();
            if enabled {
                service.registerAndReturnError()
            } else {
                service.unregisterAndReturnError()
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appkit_delete_types_the_shared_backspace_character() {
        assert_eq!(
            key_input('\u{7f}', false, false),
            Some(KeyInput::Text('\u{8}'))
        );
        assert_eq!(
            key_input('\u{8}', false, false),
            Some(KeyInput::Text('\u{8}'))
        );
    }

    #[test]
    fn select_all_uses_the_unmodified_character() {
        // Control+A: characters() is U+0001, while the unmodified key is "a".
        assert_eq!(
            key_input('\u{1}', true, false),
            Some(KeyInput::Press(Key::SelectAll))
        );
        assert_eq!(
            key_input('a', true, true),
            Some(KeyInput::Press(Key::SelectAll))
        );
        assert_eq!(key_input('a', false, false), Some(KeyInput::Text('a')));
    }

    #[test]
    fn navigation_and_function_keys_are_not_text() {
        for character in ['\u{f700}', '\u{f704}', '\u{f728}', '\u{f729}'] {
            assert_eq!(key_input(character, false, false), None, "{character:?}");
        }
    }

    #[test]
    fn command_shortcuts_do_not_insert_text() {
        assert_eq!(key_input('c', false, true), None);
        assert_eq!(key_input('5', false, false), Some(KeyInput::Text('5')));
    }
}

//! The macOS entry point: an AppKit window around the shared application logic.
//!
//! The window is a borderless, non-activating panel at a level above ordinary
//! windows, backed by a layer whose contents are the CPU-rasterized frame. The
//! portable `App` decides what to draw; this shell translates its requests into
//! AppKit calls and translates timer and worker events back into app input.
use super::{app::*, draw::Painter, shell::*};
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
use objc2::{runtime::AnyObject, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSScreen,
    NSScreenSaverWindowLevel, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CFData, CFRetained};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
    rc::Rc,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
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
    let mtm = MainThreadMarker::new()
        .ok_or("The overlay must start on the macOS main thread.".to_string())?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let core = Rc::new(RefCell::new(Core::new(mtm)?));
    let shell = MacShell { core: core.clone() };
    let config = if demo {
        Config::default()
    } else {
        Config::load()
    };
    let painter = Painter::try_new()?;
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

    show(&mut state.app, true);
    app.run();
    STATE.with(|cell| cell.set(ptr::null_mut()));
    state.app.shell.shutdown();
    Ok(())
}

struct Core {
    window: objc2::rc::Retained<NSWindow>,
    view: objc2::rc::Retained<NSView>,
    layer: Option<objc2::rc::Retained<objc2_quartz_core::CALayer>>,
    visible: bool,
    interactive: bool,
    corners: i32,
    alpha: u8,
    frame: Option<CFRetained<CGImage>>,
    timers: [Option<CFRunLoopTimerRef>; TIMER_COUNT],
    redraw: CFRunLoopSourceRef,
}
impl Core {
    fn new(mtm: MainThreadMarker) -> Result<Self> {
        let frame = NSRect::new(NSPoint::new(200., 200.), NSSize::new(440., 420.));
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
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

        let view = NSView::new(mtm);
        view.setWantsLayer(true);
        window.setContentView(Some(&view));
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
        })
    }
    fn present(&mut self) {
        let Some(layer) = &self.layer else {
            return;
        };
        if let Some(frame) = &self.frame {
            let contents: &AnyObject = (&**frame).as_ref();
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
        let created = unsafe {
            CFRunLoopTimerCreate(
                kCFAllocatorDefault,
                CFAbsoluteTimeGetCurrent() + f64::from(ms) / 1000.,
                0.,
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
        let core = self.core.borrow();
        let scale = window_scale(&core.window);
        let left = f64::from(target.left) / scale;
        let top = f64::from(target.top) / scale;
        let width = f64::from(target.width()) / scale;
        let height = f64::from(target.height()) / scale;
        let frame = NSRect::new(
            NSPoint::new(left, primary_height() - top - height),
            NSSize::new(width, height),
        );
        core.window.setFrame_display(frame, false);
    }
    fn move_to(&mut self, x: i32, y: i32) {
        let core = self.core.borrow();
        let scale = window_scale(&core.window);
        let frame = core.window.frame();
        let origin = NSPoint::new(
            f64::from(x) / scale,
            primary_height() - f64::from(y) / scale - frame.size.height,
        );
        core.window.setFrameOrigin(origin);
    }
    fn window_work_area(&self) -> Rect {
        let core = self.core.borrow();
        match core.window.screen() {
            Some(screen) => screen_work(&screen),
            None => monitor_by_index(0),
        }
    }
    fn set_cursor(&mut self, _: Cursor) {}
    fn set_input_mode(&mut self, interactive: bool) {
        let mut core = self.core.borrow_mut();
        if core.interactive == interactive {
            return;
        }
        core.interactive = interactive;
        core.window.setIgnoresMouseEvents(!interactive);
    }
    fn show_window(&mut self, activate: bool) {
        let mut core = self.core.borrow_mut();
        core.visible = true;
        if activate {
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm()).activateIgnoringOtherApps(true);
            core.window.makeKeyAndOrderFront(None);
        } else {
            core.window.orderFrontRegardless();
        }
    }
    fn hide_window(&mut self) {
        let mut core = self.core.borrow_mut();
        core.visible = false;
        core.window.orderOut(None);
    }
    fn focus(&mut self) {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm()).activateIgnoringOtherApps(true);
        self.core.borrow().window.makeKeyAndOrderFront(None);
    }
    fn set_blur(&mut self, _: u8) {}
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
    fn popup_menu(&mut self, _: &[MenuEntry], _: MenuOrigin) -> Option<u32> {
        None
    }
    fn confirm(&mut self, _: &str, _: &str, _: Buttons) -> Choice {
        Choice::Cancel
    }
    fn choose_file(&mut self, _: bool) -> std::result::Result<FileSelection, String> {
        Err("File selection is not implemented on macOS yet.".into())
    }
    fn register_hotkey(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn unregister_hotkey(&mut self) {}
    fn rebind_hotkey(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn tray_add(&mut self) -> bool {
        false
    }
    fn tray_remove(&mut self) {}
    fn set_autostart(&mut self, _: bool) -> std::result::Result<(), String> {
        Err("Launch at login is not implemented on macOS yet.".into())
    }
    fn shutdown(&mut self) {}
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

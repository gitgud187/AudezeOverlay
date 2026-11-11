//! macOS HID access through IOKit's HID manager.
//!
//! macOS presents one `IOHIDDevice` per USB interface and publishes that
//! interface's report descriptor, the same bytes Linux exposes in sysfs. The
//! vendor control collection is therefore found with the shared
//! [`descriptor`](crate::device::descriptor) reader, not by matching a single
//! usage. Input reports are fetched on request with `IOHIDDeviceGetReport` and
//! commands are sent with `IOHIDDeviceSetReport`, the same on-demand mechanism
//! as `HidD_GetInputReport` on Windows and `HIDIOCGINPUT` on Linux.
//!
//! No driver, daemon or privileged helper is involved. A non-sandboxed macOS
//! application may open a vendor-defined HID collection without a TCC prompt.
use super::{
    control_report_sizes_plausible, Enumeration, Hid, InputStream, Interface, CONTROL_USAGE,
};
use crate::device::{descriptor, profiles::Profile, selection::Candidate};
use crate::model::Connection;
use core_foundation::{
    base::{CFType, TCFType},
    data::CFData,
    dictionary::CFMutableDictionary,
    number::CFNumber,
    string::CFString,
};
use core_foundation_sys::{
    base::{kCFAllocatorDefault, CFIndex, CFRelease, CFRetain, CFTypeRef},
    dictionary::CFDictionaryRef,
    runloop::{
        kCFRunLoopDefaultMode, CFRunLoopGetCurrent, CFRunLoopRef, CFRunLoopRun, CFRunLoopRunInMode,
    },
    set::{CFSetGetCount, CFSetGetValues, CFSetRef},
    string::CFStringRef,
};
use std::{
    ffi::c_void,
    ptr::{self, NonNull},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
        Arc,
    },
    thread,
    time::Duration,
};
type Result<T, E = String> = std::result::Result<T, E>;

/// The same vendor and control collection as every other transport.
const VENDOR_ID: i32 = 0x3329;
/// Reports from the device arrive in bursts; wait for a quiet moment so one
/// replug wakes the worker once.
const HOTPLUG_SETTLE_MS: u64 = 300;

const IO_RETURN_SUCCESS: i32 = 0;
const IO_RETURN_NO_DEVICE: i32 = 0xE000_02C0u32 as i32;
const IO_RETURN_OFFLINE: i32 = 0xE000_02D7u32 as i32;
const IO_RETURN_NOT_ATTACHED: i32 = 0xE000_02D9u32 as i32;
const IO_RETURN_TIMEOUT: i32 = 0xE000_02D6u32 as i32;
const IO_RETURN_NOT_READY: i32 = 0xE000_02D8u32 as i32;
const IO_RETURN_NOT_OPEN: i32 = 0xE000_02CDu32 as i32;
const IO_RETURN_NOT_PERMITTED: i32 = 0xE000_02E2u32 as i32;
const IO_RETURN_EXCLUSIVE_ACCESS: i32 = 0xE000_02C5u32 as i32;
const IO_RETURN_NOT_RESPONDING: i32 = 0xE000_02EDu32 as i32;
const IO_RETURN_DEVICE_ERROR: i32 = 0xE000_02E9u32 as i32;
const IO_RETURN_ABORTED: i32 = 0xE000_02EBu32 as i32;

/// The endpoint is gone or mid-teardown. macOS reports several codes while a
/// dongle is being unplugged; all mean the worker should reconnect.
fn disconnected(result: IOReturn) -> bool {
    matches!(
        result,
        IO_RETURN_NO_DEVICE
            | IO_RETURN_OFFLINE
            | IO_RETURN_NOT_ATTACHED
            | IO_RETURN_TIMEOUT
            | IO_RETURN_NOT_READY
            | IO_RETURN_NOT_OPEN
            | IO_RETURN_NOT_RESPONDING
            | IO_RETURN_DEVICE_ERROR
            | IO_RETURN_ABORTED
    )
}

fn denied(result: IOReturn) -> bool {
    matches!(result, IO_RETURN_NOT_PERMITTED | IO_RETURN_EXCLUSIVE_ACCESS)
}

/// Shown when a recognized interface exists but cannot be opened.
const PERMISSION_HELP: &str = "The headset's HID interface could not be opened. \
Close any application that holds it exclusively, then reconnect the dongle.";

type IOOptionBits = u32;
type IOReturn = i32;
type IOHIDReportType = u32;
type IOHIDDeviceCallback =
    extern "C" fn(context: *mut c_void, result: IOReturn, sender: *mut c_void, device: *mut c_void);
type IOHIDReportCallback = extern "C" fn(
    context: *mut c_void,
    result: IOReturn,
    sender: *mut c_void,
    report_type: IOHIDReportType,
    report_id: u32,
    report: *mut u8,
    report_length: CFIndex,
);

const REPORT_INPUT: IOHIDReportType = 0;
const REPORT_OUTPUT: IOHIDReportType = 1;

// The raw IOKit entry points. Everything below this block reaches them through
// `CfRef`, which owns the reference, or through the small wrappers that state
// their preconditions.
#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDManagerCreate(allocator: *const c_void, options: IOOptionBits) -> *mut c_void;
    fn IOHIDManagerSetDeviceMatching(manager: *mut c_void, matching: CFDictionaryRef);
    fn IOHIDManagerOpen(manager: *mut c_void, options: IOOptionBits) -> IOReturn;
    fn IOHIDManagerCopyDevices(manager: *mut c_void) -> CFSetRef;
    fn IOHIDManagerRegisterDeviceMatchingCallback(
        manager: *mut c_void,
        callback: Option<IOHIDDeviceCallback>,
        context: *mut c_void,
    );
    fn IOHIDManagerRegisterDeviceRemovalCallback(
        manager: *mut c_void,
        callback: Option<IOHIDDeviceCallback>,
        context: *mut c_void,
    );
    fn IOHIDManagerScheduleWithRunLoop(
        manager: *mut c_void,
        run_loop: CFRunLoopRef,
        mode: CFStringRef,
    );

    fn IOHIDDeviceOpen(device: *mut c_void, options: IOOptionBits) -> IOReturn;
    fn IOHIDDeviceClose(device: *mut c_void, options: IOOptionBits) -> IOReturn;
    fn IOHIDDeviceGetProperty(device: *mut c_void, key: CFStringRef) -> CFTypeRef;
    fn IOHIDDeviceGetReport(
        device: *mut c_void,
        report_type: IOHIDReportType,
        report_id: CFIndex,
        report: *mut u8,
        report_length: *mut CFIndex,
    ) -> IOReturn;
    fn IOHIDDeviceSetReport(
        device: *mut c_void,
        report_type: IOHIDReportType,
        report_id: CFIndex,
        report: *const u8,
        report_length: CFIndex,
    ) -> IOReturn;
    fn IOHIDDeviceRegisterInputReportCallback(
        device: *mut c_void,
        report: *mut u8,
        report_length: CFIndex,
        callback: Option<IOHIDReportCallback>,
        context: *mut c_void,
    );
    fn IOHIDDeviceScheduleWithRunLoop(
        device: *mut c_void,
        run_loop: CFRunLoopRef,
        mode: CFStringRef,
    );
    fn IOHIDDeviceUnscheduleFromRunLoop(
        device: *mut c_void,
        run_loop: CFRunLoopRef,
        mode: CFStringRef,
    );
}

/// One owned reference to an IOKit or Core Foundation object (a manager, a
/// device or a set). Cloning retains and dropping releases, so a handle cannot
/// leak or be released twice.
struct CfRef(NonNull<c_void>);

// SAFETY: Core Foundation retain and release are thread-safe. IOKit objects
// must not be used from two threads at once; the code here hands a clone to the
// listener thread and never touches that clone from another thread.
unsafe impl Send for CfRef {}

impl CfRef {
    /// Takes over a reference returned by a `Create`/`Copy` function.
    ///
    /// # Safety
    /// `raw` is null or a Core Foundation object the caller owns one reference to.
    unsafe fn from_create(raw: *const c_void) -> Option<Self> {
        NonNull::new(raw.cast_mut()).map(Self)
    }

    /// Retains an object borrowed from a collection (the `Get` rule).
    ///
    /// # Safety
    /// `raw` is null or a live Core Foundation object.
    unsafe fn retain(raw: *const c_void) -> Option<Self> {
        let object = NonNull::new(raw.cast_mut())?;
        // SAFETY: the caller guarantees `raw` is live, so retaining is valid.
        unsafe { CFRetain(raw as CFTypeRef) };
        Some(Self(object))
    }

    fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Clone for CfRef {
    fn clone(&self) -> Self {
        // SAFETY: `self` owns a reference, so the object is live.
        unsafe { CFRetain(self.as_ptr() as CFTypeRef) };
        Self(self.0)
    }
}

impl Drop for CfRef {
    fn drop(&mut self) {
        // SAFETY: `self` owns exactly one reference, released once here.
        unsafe { CFRelease(self.as_ptr() as CFTypeRef) };
    }
}

/// Splits the shared report buffer into the IOKit report ID and payload. A
/// numbered report keeps its ID byte; report ID 0 is unnumbered, so the byte is
/// not part of the payload IOKit receives.
fn output_report(report: &[u8]) -> Result<(CFIndex, &[u8]), &'static str> {
    let Some((&id, payload)) = report.split_first() else {
        return Err("Empty USB sound command");
    };
    if id == 0 {
        if payload.is_empty() {
            return Err("Empty USB sound command");
        }
        Ok((0, payload))
    } else {
        Ok((CFIndex::from(id), report))
    }
}

/// An open vendor-control interface; closing it is paired with opening in `Drop`.
struct Node {
    device: CfRef,
    input_len: usize,
    /// Closing the manager closes its devices, so the manager lives as long as
    /// the opened interface does. Fields drop in order: the device closes and
    /// releases first.
    _manager: CfRef,
}
impl Drop for Node {
    fn drop(&mut self) {
        // SAFETY: the device was opened by `enumerate` and is still referenced.
        unsafe { IOHIDDeviceClose(self.device.as_ptr(), 0) };
    }
}
impl Hid for Node {
    fn write_report(&self, report: &[u8]) -> Result<()> {
        // IOKit takes the report ID as an argument. For a numbered report it
        // must also remain in the buffer; report ID 0 means the buffer contains
        // only payload, as with hidapi.
        let (report_id, bytes) = output_report(report).map_err(str::to_string)?;
        // SAFETY: the device is open, and `bytes` is a live buffer of the given length.
        let result = unsafe {
            IOHIDDeviceSetReport(
                self.device.as_ptr(),
                REPORT_OUTPUT,
                report_id,
                bytes.as_ptr(),
                bytes.len() as CFIndex,
            )
        };
        match result {
            IO_RETURN_SUCCESS => Ok(()),
            _ if disconnected(result) => Err("USB connection lost. Reconnecting…".into()),
            _ if denied(result) => {
                Err("Permission denied writing to the headset's HID interface.".into())
            }
            _ => Err(format!("USB sound command failed ({result:#x}).")),
        }
    }
    fn get_input_report(&self, report: &mut [u8; 64]) -> Result<()> {
        let mut length = report.len() as CFIndex;
        // SAFETY: the device is open, and `report` is a writable buffer of `length` bytes.
        let result = unsafe {
            IOHIDDeviceGetReport(
                self.device.as_ptr(),
                REPORT_INPUT,
                CFIndex::from(report[0]),
                report.as_mut_ptr(),
                &mut length,
            )
        };
        match result {
            IO_RETURN_SUCCESS => Ok(()),
            _ if disconnected(result) => Err("USB connection lost. Reconnecting…".into()),
            _ if denied(result) => {
                Err("Permission denied reading the headset's HID reports.".into())
            }
            _ => Err(format!("USB read failed ({result:#x}).")),
        }
    }
    fn listen(&self) -> Result<Box<dyn InputStream>> {
        Listener::open(self.device.clone(), self.input_len)
    }
}

/// Lists the supported Audeze control interfaces the current user can open.
pub(in crate::device) fn enumerate() -> Result<Enumeration> {
    let manager = open_manager()?;
    let mut candidates = Vec::new();
    let mut wired_seen = Vec::new();
    let mut permission_denied = false;
    for device in attached_devices(&manager) {
        let Some(product) = number_property(&device, "ProductID") else {
            continue;
        };
        let Ok(product) = u16::try_from(product) else {
            continue;
        };
        let Some(profile) = Profile::from_usb(VENDOR_ID as u16, product) else {
            continue;
        };
        // A wired identity counts as present even if its interface is
        // unusable, exactly as on Windows and Linux.
        if profile.connection == Connection::WiredUsb && !wired_seen.contains(&profile) {
            wired_seen.push(profile);
        }
        let Some(bytes) = data_property(&device, "ReportDescriptor") else {
            continue;
        };
        let Some((input_len, output_len)) = control_lengths(&bytes) else {
            continue;
        };
        // `get_input_report` uses a fixed 64-byte buffer, so a longer
        // control report could never be read.
        if input_len > 64 {
            continue;
        }
        // SAFETY: `device` is a live reference owned by this iteration.
        match unsafe { IOHIDDeviceOpen(device.as_ptr(), 0) } {
            IO_RETURN_SUCCESS => {
                candidates.push(Candidate {
                    profile,
                    endpoint: Interface {
                        id: endpoint_id(&device, product),
                        output_len,
                        hid: Box::new(Node {
                            device,
                            input_len,
                            _manager: manager.clone(),
                        }),
                    },
                });
            }
            IO_RETURN_NOT_PERMITTED | IO_RETURN_EXCLUSIVE_ACCESS => {
                permission_denied = true;
            }
            // Gone between listing and opening, or not ours to open.
            _ => {}
        }
    }
    if candidates.is_empty() && permission_denied {
        return Err(PERMISSION_HELP.into());
    }
    candidates.sort_by(|a, b| a.endpoint.id.cmp(&b.endpoint.id));
    Ok(Enumeration {
        candidates,
        wired_seen,
    })
}

/// A manager that matches every interface of the headset's vendor. Product
/// identities, the descriptor and selection then narrow it to one collection.
fn open_manager() -> Result<CfRef> {
    // SAFETY: `IOHIDManagerCreate` follows the Create rule.
    let manager = unsafe { CfRef::from_create(IOHIDManagerCreate(kCFAllocatorDefault, 0)) }
        .ok_or("The macOS HID manager could not be created.")?;
    let mut matching = CFMutableDictionary::<CFString, CFNumber>::new();
    matching.add(&CFString::new("VendorID"), &CFNumber::from(VENDOR_ID));
    let matching = matching.to_immutable();
    // SAFETY: the manager and the matching dictionary are live; IOKit copies the dictionary.
    let opened = unsafe {
        IOHIDManagerSetDeviceMatching(manager.as_ptr(), matching.as_concrete_TypeRef());
        IOHIDManagerOpen(manager.as_ptr(), 0)
    };
    if opened != IO_RETURN_SUCCESS {
        return Err(format!(
            "The macOS HID manager could not be opened ({opened:#x})."
        ));
    }
    Ok(manager)
}

/// The HID devices the manager currently matches, each retained independently
/// of the manager's own set.
fn attached_devices(manager: &CfRef) -> Vec<CfRef> {
    // SAFETY: `IOHIDManagerCopyDevices` follows the Copy rule and returns null
    // when nothing matches.
    let Some(set) =
        (unsafe { CfRef::from_create(IOHIDManagerCopyDevices(manager.as_ptr()).cast()) })
    else {
        return Vec::new();
    };
    let set_ref = set.as_ptr() as CFSetRef;
    // SAFETY: `set` is a live CFSet.
    let count = unsafe { CFSetGetCount(set_ref) }.max(0) as usize;
    let mut values: Vec<*const c_void> = vec![ptr::null(); count];
    // SAFETY: `values` has room for `count` pointers, the set's size.
    unsafe { CFSetGetValues(set_ref, values.as_mut_ptr()) };
    values
        .into_iter()
        // SAFETY: the values are borrowed from `set`, which is alive until here.
        .filter_map(|value| unsafe { CfRef::retain(value) })
        .collect()
}

/// Calls `on_change` whenever macOS reports a HID device appearing or
/// disappearing. The manager runs on its own run loop, so this costs nothing
/// while idle. If it cannot start, the worker still retries opening the device
/// on its normal reconnect schedule.
pub(in crate::device) fn watch_hotplug(on_change: impl Fn() + Send + 'static) {
    let _ = thread::Builder::new()
        .name("hid-hotplug".into())
        .spawn(move || {
            let (tx, rx) = mpsc::channel::<()>();
            // Declared before the manager, so the manager is released first and
            // can no longer call back into a freed sender.
            let context = Box::new(tx);
            let Ok(manager) = open_manager() else {
                return;
            };
            let _ = thread::Builder::new()
                .name("hid-hotplug-events".into())
                .spawn(move || {
                    while rx.recv().is_ok() {
                        while rx
                            .recv_timeout(Duration::from_millis(HOTPLUG_SETTLE_MS))
                            .is_ok()
                        {}
                        on_change();
                    }
                });
            let context_ptr = ptr::from_ref::<mpsc::Sender<()>>(&context)
                .cast_mut()
                .cast::<c_void>();
            // SAFETY: the manager is live, and `context` outlives the run loop
            // below, which is the only place the callbacks are delivered.
            unsafe {
                IOHIDManagerRegisterDeviceMatchingCallback(
                    manager.as_ptr(),
                    Some(device_changed),
                    context_ptr,
                );
                IOHIDManagerRegisterDeviceRemovalCallback(
                    manager.as_ptr(),
                    Some(device_changed),
                    context_ptr,
                );
                IOHIDManagerScheduleWithRunLoop(
                    manager.as_ptr(),
                    CFRunLoopGetCurrent(),
                    kCFRunLoopDefaultMode,
                );
                CFRunLoopRun();
            }
        });
}

extern "C" fn device_changed(
    context: *mut c_void,
    _result: IOReturn,
    _sender: *mut c_void,
    _device: *mut c_void,
) {
    // SAFETY: the context is the `Sender<()>` that `watch_hotplug` keeps alive
    // for as long as its run loop delivers callbacks.
    let sender = unsafe { &*(context as *const mpsc::Sender<()>) };
    let _ = sender.send(());
}

/// A device property, or `None` if the device does not publish it.
fn property(device: &CfRef, key: &str) -> Option<CFType> {
    let key = CFString::new(key);
    // SAFETY: the device and key are live. The result is borrowed (Get rule), so
    // it is retained before use; null means the property is absent.
    unsafe {
        let value = IOHIDDeviceGetProperty(device.as_ptr(), key.as_concrete_TypeRef());
        (!value.is_null()).then(|| CFType::wrap_under_get_rule(value))
    }
}

fn number_property(device: &CfRef, key: &str) -> Option<i64> {
    property(device, key)?.downcast_into::<CFNumber>()?.to_i64()
}

fn data_property(device: &CfRef, key: &str) -> Option<Vec<u8>> {
    Some(property(device, key)?.downcast_into::<CFData>()?.to_vec())
}

/// A stable enough identity to notice that the endpoint changed.
fn endpoint_id(device: &CfRef, product: u16) -> String {
    let location = number_property(device, "LocationID").unwrap_or(0);
    format!("{product:04x}:{location:08x}")
}

/// Largest input and output report of the vendor control collection, if this
/// interface has one with plausible sizes.
fn control_lengths(bytes: &[u8]) -> Option<(usize, usize)> {
    descriptor::collections(&bytes[..bytes.len().min(descriptor::MAX_DESCRIPTOR_BYTES)])
        .into_iter()
        .find(|c| (c.usage_page, c.usage) == CONTROL_USAGE)
        .filter(|c| control_report_sizes_plausible(c.max_input_bytes, c.max_output_bytes))
        .map(|c| (c.max_input_bytes, c.max_output_bytes))
}

/// Unsolicited input reports, used only by diagnostics.
struct Listener {
    reports: mpsc::Receiver<Vec<u8>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
struct ListenerContext {
    reports: mpsc::Sender<Vec<u8>>,
}

/// An input-report callback scheduled on the current thread's run loop. It owns
/// the report buffer and callback context IOKit points into, so they stay valid
/// until `Drop` unregisters the callback.
struct InputRegistration {
    device: CfRef,
    _buffer: Vec<u8>,
    _context: Arc<ListenerContext>,
}
impl InputRegistration {
    fn start(device: CfRef, input_len: usize, reports: mpsc::Sender<Vec<u8>>) -> Self {
        let mut buffer = vec![0u8; input_len.max(2)];
        // An `Arc`, not a `Box`: the callback pointer stays valid when this
        // handle is moved into the struct below.
        let context = Arc::new(ListenerContext { reports });
        let context_ptr = Arc::as_ptr(&context).cast_mut().cast::<c_void>();
        // SAFETY: the device is open and live. The buffer and context are heap
        // allocations that this struct keeps until `Drop` unregisters them.
        unsafe {
            IOHIDDeviceRegisterInputReportCallback(
                device.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len() as CFIndex,
                Some(report_received),
                context_ptr,
            );
            IOHIDDeviceScheduleWithRunLoop(
                device.as_ptr(),
                CFRunLoopGetCurrent(),
                kCFRunLoopDefaultMode,
            );
        }
        Self {
            device,
            _buffer: buffer,
            _context: context,
        }
    }

    /// Delivers pending reports for up to `seconds`.
    fn run(&self, seconds: f64) {
        // SAFETY: runs the calling thread's own run loop, where `start` scheduled the device.
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, 1) };
    }
}
impl Drop for InputRegistration {
    fn drop(&mut self) {
        // SAFETY: undoes `start` on the same thread, before the buffer and
        // context are freed.
        unsafe {
            IOHIDDeviceUnscheduleFromRunLoop(
                self.device.as_ptr(),
                CFRunLoopGetCurrent(),
                kCFRunLoopDefaultMode,
            );
            IOHIDDeviceRegisterInputReportCallback(
                self.device.as_ptr(),
                ptr::null_mut(),
                0,
                None,
                ptr::null_mut(),
            );
        }
    }
}

impl Listener {
    /// `device` is the listener thread's own reference; a failed spawn drops it.
    fn open(device: CfRef, input_len: usize) -> Result<Box<dyn InputStream>> {
        let (reports, received) = mpsc::channel();
        let (ready, started) = mpsc::channel::<()>();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = thread::Builder::new()
            .name("hid-listener".into())
            .spawn(move || {
                let registration = InputRegistration::start(device, input_len, reports);
                let _ = ready.send(());
                while !stopping.load(Ordering::Acquire) {
                    registration.run(0.1);
                }
            })
            .map_err(|e| e.to_string())?;
        if started.recv_timeout(Duration::from_secs(1)).is_err() {
            stop.store(true, Ordering::Release);
            return Err("The macOS HID listener did not start.".into());
        }
        Ok(Box::new(Listener {
            reports: received,
            stop,
            thread: Some(thread),
        }))
    }
}
impl InputStream for Listener {
    fn read(&mut self, timeout_ms: u32) -> Result<Option<Vec<u8>>> {
        match self
            .reports
            .recv_timeout(Duration::from_millis(u64::from(timeout_ms)))
        {
            Ok(report) => Ok(Some(report)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err("USB connection lost. Reconnecting…".into()),
        }
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

extern "C" fn report_received(
    context: *mut c_void,
    result: IOReturn,
    _sender: *mut c_void,
    _report_type: IOHIDReportType,
    _report_id: u32,
    report: *mut u8,
    report_length: CFIndex,
) {
    if result != IO_RETURN_SUCCESS || report.is_null() || report_length <= 0 {
        return;
    }
    // SAFETY: IOKit passes `report_length` valid bytes, and the context is the
    // `ListenerContext` owned by the live `InputRegistration` that registered it.
    let (bytes, context) = unsafe {
        (
            std::slice::from_raw_parts(report, report_length as usize).to_vec(),
            &*(context as *const ListenerContext),
        )
    };
    let _ = context.reports.send(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_and_offline_devices_are_a_lost_connection() {
        for result in [
            IO_RETURN_NO_DEVICE,
            IO_RETURN_OFFLINE,
            IO_RETURN_NOT_ATTACHED,
            IO_RETURN_TIMEOUT,
            IO_RETURN_NOT_READY,
            IO_RETURN_NOT_OPEN,
            IO_RETURN_NOT_RESPONDING,
            IO_RETURN_DEVICE_ERROR,
            IO_RETURN_ABORTED,
        ] {
            assert!(disconnected(result), "{result:#x}");
        }
        assert!(!disconnected(IO_RETURN_SUCCESS));
        assert!(!disconnected(IO_RETURN_NOT_PERMITTED));
    }

    #[test]
    fn permission_and_exclusive_access_errors_are_not_disconnects() {
        assert!(denied(IO_RETURN_NOT_PERMITTED));
        assert!(denied(IO_RETURN_EXCLUSIVE_ACCESS));
        assert!(!denied(IO_RETURN_NO_DEVICE));
    }

    #[test]
    fn numbered_reports_keep_the_id_and_unnumbered_reports_strip_it() {
        let (id, bytes) = output_report(&[6, 1, 2]).unwrap();
        assert_eq!(id, 6);
        assert_eq!(bytes, &[6, 1, 2]);

        let (id, bytes) = output_report(&[0, 1, 2]).unwrap();
        assert_eq!(id, 0);
        assert_eq!(bytes, &[1, 2]);

        assert!(output_report(&[]).is_err());
        assert!(output_report(&[0]).is_err());
    }
}

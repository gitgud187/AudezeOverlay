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
    base::TCFType,
    data::{CFData, CFDataRef},
    dictionary::CFMutableDictionary,
    number::{CFNumber, CFNumberRef},
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
    ptr,
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
        IO_RETURN_TIMEOUT
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
type IOHIDManagerRef = *mut c_void;
type IOHIDDeviceRef = *mut c_void;
type IOHIDReportType = u32;
type IOHIDDeviceCallback = extern "C" fn(
    context: *mut c_void,
    result: IOReturn,
    sender: *mut c_void,
    device: IOHIDDeviceRef,
);
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

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDManagerCreate(allocator: *const c_void, options: IOOptionBits) -> IOHIDManagerRef;
    fn IOHIDManagerSetDeviceMatching(manager: IOHIDManagerRef, matching: CFDictionaryRef);
    fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: IOOptionBits) -> IOReturn;
    fn IOHIDManagerCopyDevices(manager: IOHIDManagerRef) -> CFSetRef;
    fn IOHIDManagerRegisterDeviceMatchingCallback(
        manager: IOHIDManagerRef,
        callback: Option<IOHIDDeviceCallback>,
        context: *mut c_void,
    );
    fn IOHIDManagerRegisterDeviceRemovalCallback(
        manager: IOHIDManagerRef,
        callback: Option<IOHIDDeviceCallback>,
        context: *mut c_void,
    );
    fn IOHIDManagerScheduleWithRunLoop(
        manager: IOHIDManagerRef,
        run_loop: CFRunLoopRef,
        mode: CFStringRef,
    );

    fn IOHIDDeviceOpen(device: IOHIDDeviceRef, options: IOOptionBits) -> IOReturn;
    fn IOHIDDeviceClose(device: IOHIDDeviceRef, options: IOOptionBits) -> IOReturn;
    fn IOHIDDeviceGetProperty(device: IOHIDDeviceRef, key: CFStringRef) -> CFTypeRef;
    fn IOHIDDeviceGetReport(
        device: IOHIDDeviceRef,
        report_type: IOHIDReportType,
        report_id: CFIndex,
        report: *mut u8,
        report_length: *mut CFIndex,
    ) -> IOReturn;
    fn IOHIDDeviceSetReport(
        device: IOHIDDeviceRef,
        report_type: IOHIDReportType,
        report_id: CFIndex,
        report: *const u8,
        report_length: CFIndex,
    ) -> IOReturn;
    fn IOHIDDeviceRegisterInputReportCallback(
        device: IOHIDDeviceRef,
        report: *mut u8,
        report_length: CFIndex,
        callback: Option<IOHIDReportCallback>,
        context: *mut c_void,
    );
    fn IOHIDDeviceScheduleWithRunLoop(
        device: IOHIDDeviceRef,
        run_loop: CFRunLoopRef,
        mode: CFStringRef,
    );
    fn IOHIDDeviceUnscheduleFromRunLoop(
        device: IOHIDDeviceRef,
        run_loop: CFRunLoopRef,
        mode: CFStringRef,
    );
}

/// IOKit objects may be used from one thread at a time; the listener moves the
/// reference to its own thread and returns it there.
struct SendDevice(IOHIDDeviceRef);
unsafe impl Send for SendDevice {}
impl SendDevice {
    fn into_ptr(self) -> IOHIDDeviceRef {
        self.0
    }
}

/// An open vendor-control interface; closing and releasing are paired in `Drop`.
struct Node {
    device: IOHIDDeviceRef,
    input_len: usize,
    /// Closing the manager closes its devices, so the manager lives as long as
    /// the opened interface does.
    manager: IOHIDManagerRef,
}
impl Drop for Node {
    fn drop(&mut self) {
        unsafe {
            IOHIDDeviceClose(self.device, 0);
            CFRelease(self.device as CFTypeRef);
            CFRelease(self.manager as CFTypeRef);
        }
    }
}
impl Hid for Node {
    fn write_report(&self, report: &[u8]) -> Result<()> {
        if report.is_empty() {
            return Err("Empty USB sound command".into());
        }
        // For numbered reports IOKit wants the report ID both as an argument and
        // in the first byte, and the length covers the whole buffer.
        let result = unsafe {
            IOHIDDeviceSetReport(
                self.device,
                REPORT_OUTPUT,
                report[0] as CFIndex,
                report.as_ptr(),
                report.len() as CFIndex,
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
        let result = unsafe {
            IOHIDDeviceGetReport(
                self.device,
                REPORT_INPUT,
                report[0] as CFIndex,
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
        Listener::open(self.device, self.input_len)
    }
}

/// Lists the supported Audeze control interfaces the current user can open.
pub(in crate::device) fn enumerate() -> Result<Enumeration> {
    unsafe {
        let manager = open_manager()?;
        let set = IOHIDManagerCopyDevices(manager);
        let mut candidates = Vec::new();
        let mut wired_seen = Vec::new();
        let mut permission_denied = false;
        if !set.is_null() {
            let count = CFSetGetCount(set).max(0) as usize;
            let mut values: Vec<*const c_void> = vec![ptr::null(); count];
            CFSetGetValues(set, values.as_mut_ptr());
            for value in values {
                let device = value as IOHIDDeviceRef;
                let Some(product) = number_property(device, "ProductID") else {
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
                let Some(bytes) = data_property(device, "ReportDescriptor") else {
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
                match IOHIDDeviceOpen(device, 0) {
                    IO_RETURN_SUCCESS => {
                        CFRetain(device as CFTypeRef);
                        CFRetain(manager as CFTypeRef);
                        candidates.push(Candidate {
                            profile,
                            endpoint: Interface {
                                id: endpoint_id(device, product),
                                output_len,
                                hid: Box::new(Node {
                                    device,
                                    input_len,
                                    manager,
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
            CFRelease(set as CFTypeRef);
        }
        // The opened interfaces retain the manager; this releases the
        // enumeration's own reference.
        CFRelease(manager as CFTypeRef);
        if candidates.is_empty() && permission_denied {
            return Err(PERMISSION_HELP.into());
        }
        candidates.sort_by(|a, b| a.endpoint.id.cmp(&b.endpoint.id));
        Ok(Enumeration {
            candidates,
            wired_seen,
        })
    }
}

/// A manager that matches every interface of the headset's vendor. Product
/// identities, the descriptor and selection then narrow it to one collection.
unsafe fn open_manager() -> Result<IOHIDManagerRef> {
    let manager = IOHIDManagerCreate(kCFAllocatorDefault, 0);
    if manager.is_null() {
        return Err("The macOS HID manager could not be created.".into());
    }
    let mut matching = CFMutableDictionary::<CFString, CFNumber>::new();
    matching.add(&CFString::new("VendorID"), &CFNumber::from(VENDOR_ID));
    let matching = matching.to_immutable();
    IOHIDManagerSetDeviceMatching(manager, matching.as_concrete_TypeRef());
    let opened = IOHIDManagerOpen(manager, 0);
    if opened != IO_RETURN_SUCCESS {
        CFRelease(manager as CFTypeRef);
        return Err(format!(
            "The macOS HID manager could not be opened ({opened:#x})."
        ));
    }
    Ok(manager)
}

/// Calls `on_change` whenever macOS reports a HID device appearing or
/// disappearing. The manager runs on its own run loop, so this costs nothing
/// while idle. If it cannot start, the worker still retries opening the device
/// on its normal reconnect schedule.
pub(in crate::device) fn watch_hotplug(on_change: impl Fn() + Send + 'static) {
    let _ = thread::Builder::new()
        .name("hid-hotplug".into())
        .spawn(move || unsafe {
            let Ok(manager) = open_manager() else {
                return;
            };
            let (tx, rx) = mpsc::channel::<()>();
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
            // The manager lives for the process, so the context is handed over
            // once and never reclaimed.
            let context = Box::into_raw(Box::new(tx)).cast::<c_void>();
            IOHIDManagerRegisterDeviceMatchingCallback(manager, Some(device_changed), context);
            IOHIDManagerRegisterDeviceRemovalCallback(manager, Some(device_changed), context);
            IOHIDManagerScheduleWithRunLoop(manager, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
            CFRunLoopRun();
        });
}

extern "C" fn device_changed(
    context: *mut c_void,
    _result: IOReturn,
    _sender: *mut c_void,
    _device: IOHIDDeviceRef,
) {
    // SAFETY: the context is a `Sender<()>` owned by `watch_hotplug` for the
    // process lifetime.
    let sender = unsafe { &*(context as *const mpsc::Sender<()>) };
    let _ = sender.send(());
}

unsafe fn property(device: IOHIDDeviceRef, key: &str) -> Option<CFTypeRef> {
    let key = CFString::new(key);
    let value = IOHIDDeviceGetProperty(device, key.as_concrete_TypeRef());
    (!value.is_null()).then_some(value)
}

unsafe fn number_property(device: IOHIDDeviceRef, key: &str) -> Option<i64> {
    let value = property(device, key)?;
    CFNumber::wrap_under_get_rule(value as CFNumberRef).to_i64()
}

unsafe fn data_property(device: IOHIDDeviceRef, key: &str) -> Option<Vec<u8>> {
    let value = property(device, key)?;
    Some(
        CFData::wrap_under_get_rule(value as CFDataRef)
            .bytes()
            .to_vec(),
    )
}

/// A stable enough identity to notice that the endpoint changed.
unsafe fn endpoint_id(device: IOHIDDeviceRef, product: u16) -> String {
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
impl Listener {
    fn open(device: IOHIDDeviceRef, input_len: usize) -> Result<Box<dyn InputStream>> {
        let (reports, received) = mpsc::channel();
        let (ready, started) = mpsc::channel::<Result<(), String>>();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        // The listener thread owns one reference until it exits.
        unsafe { CFRetain(device as CFTypeRef) };
        let device = SendDevice(device);
        let thread = thread::Builder::new()
            .name("hid-listener".into())
            .spawn(move || unsafe {
                let device = device.into_ptr();
                let mut buffer = vec![0u8; input_len.max(2)];
                let mut context = Box::new(ListenerContext { reports });
                let context_ptr = (&mut *context as *mut ListenerContext).cast::<c_void>();
                IOHIDDeviceRegisterInputReportCallback(
                    device,
                    buffer.as_mut_ptr(),
                    buffer.len() as CFIndex,
                    Some(report_received),
                    context_ptr,
                );
                IOHIDDeviceScheduleWithRunLoop(
                    device,
                    CFRunLoopGetCurrent(),
                    kCFRunLoopDefaultMode,
                );
                let _ = ready.send(Ok(()));
                while !stopping.load(Ordering::Acquire) {
                    CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, 1);
                }
                IOHIDDeviceUnscheduleFromRunLoop(
                    device,
                    CFRunLoopGetCurrent(),
                    kCFRunLoopDefaultMode,
                );
                IOHIDDeviceRegisterInputReportCallback(
                    device,
                    ptr::null_mut(),
                    0,
                    None,
                    ptr::null_mut(),
                );
                CFRelease(device as CFTypeRef);
                drop(context);
            })
            .map_err(|e| e.to_string())?;
        started
            .recv_timeout(Duration::from_secs(1))
            .map_err(|_| "The macOS HID listener did not start.".to_string())??;
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
    // SAFETY: the callback gets a buffer of `report_length` valid bytes, and the
    // context outlives the registration.
    let bytes = unsafe { std::slice::from_raw_parts(report, report_length as usize) }.to_vec();
    let context = unsafe { &*(context as *const ListenerContext) };
    let _ = context.reports.send(bytes);
}

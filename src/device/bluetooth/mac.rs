//! IOBluetooth RFCOMM link to the headset's control channel.
//!
//! IOBluetooth delivers channel callbacks on the **main** thread's run loop: a
//! channel opened from a secondary thread with its own run loop never reports
//! back (observed on macOS, see `docs/BLUETOOTH_PROTOCOL.md`). So the channel and
//! its delegate live on the main thread, and other threads reach them by queueing
//! blocks on the main run loop, while bytes flow back through a plain channel.
//! The application's AppKit loop already serves the main run loop; a program with
//! no UI uses [`host_main_run_loop`].
//!
//! The process must be a launched app bundle with `NSBluetoothAlwaysUsageDescription`;
//! a bare command-line process is aborted by macOS at the first IOBluetooth call.
use super::SERVICE_UUID;
use block2::RcBlock;
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject, NSObjectProtocol},
    AllocAnyThread, DefinedClass, MainThreadMarker,
};
use objc2_core_foundation::{
    kCFRunLoopDefaultMode, CFAbsoluteTimeGetCurrent, CFRunLoop, CFRunLoopTimer,
};
use objc2_foundation::NSData;
use objc2_io_bluetooth::{
    IOBluetoothDevice, IOBluetoothRFCOMMChannel, IOBluetoothRFCOMMChannelDelegate,
    IOBluetoothSDPUUID,
};
use std::{
    cell::RefCell,
    ffi::c_void,
    slice,
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex,
    },
    thread,
    time::Duration,
};

type Result<T> = std::result::Result<T, String>;

/// How long opening the link may take before it is reported as failed.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the main thread may take to accept a write.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long closing the channel on the main thread may take when a link is dropped.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
/// Seconds until the keep-alive timer would fire (about 300 years).
const KEEP_ALIVE_SECONDS: f64 = 1.0e10;

/// Something that happened on the channel.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// The open attempt finished. `status` is an `IOReturn`; zero is success.
    Opened { status: i32, mtu: u16 },
    /// Bytes from the headset. They are a stream: a frame may be split across
    /// events or share one.
    Data(Vec<u8>),
    /// The channel closed, for example because the headset powered off.
    Closed,
}

/// An open RFCOMM channel to one paired headset. Dropping it closes the channel.
///
/// Only one link exists at a time, and it can only be opened and used from a
/// thread other than the main thread, which must be running its run loop.
pub struct Link {
    events: Receiver<Event>,
    mtu: u16,
}

impl Link {
    /// Opens the control channel of the paired device named exactly `device_name`.
    ///
    /// Fails, with an explanation, when called on the main thread, the main run
    /// loop is not running, a link is already open, no or several paired devices
    /// match, the device is not connected, it has no serial service, or the
    /// channel does not open within [`OPEN_TIMEOUT`].
    pub fn open(device_name: &str) -> Result<Self> {
        if MainThreadMarker::new().is_some() {
            return Err(
                "The Bluetooth link must be opened from a thread other than the main thread."
                    .into(),
            );
        }
        let (event_tx, events) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let name = device_name.to_owned();
        on_main_run_loop(move || {
            let _ = ready_tx.send(install_session(&name, event_tx));
        })?;
        ready_rx.recv_timeout(OPEN_TIMEOUT).map_err(|_| {
            "The main run loop is not running, so the channel cannot open.".to_string()
        })??;
        let mut link = Self { events, mtu: 0 };
        match link.recv(OPEN_TIMEOUT) {
            Some(Event::Opened { status: 0, mtu }) => {
                link.mtu = mtu;
                Ok(link)
            }
            Some(Event::Opened { status, .. }) => {
                Err(format!("The channel did not open (IOReturn {status:#x})."))
            }
            Some(Event::Closed) => Err("The channel closed while opening.".into()),
            Some(Event::Data(_)) | None => Err("The channel did not open in time.".into()),
        }
    }

    /// Largest write the channel accepts in one piece, from the open handshake.
    pub fn mtu(&self) -> u16 {
        self.mtu
    }

    /// Waits up to `timeout` for the next event. `None` means nothing happened;
    /// a finished link keeps reporting [`Event::Closed`].
    pub fn recv(&self, timeout: Duration) -> Option<Event> {
        match self.events.recv_timeout(timeout) {
            Ok(event) => Some(event),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => Some(Event::Closed),
        }
    }

    /// Sends `bytes` and returns once the channel has taken them. Callers choose
    /// the bytes; this layer does not interpret them.
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        let length = u16::try_from(bytes.len())
            .ok()
            .filter(|length| *length <= self.mtu)
            .ok_or_else(|| {
                format!(
                    "A write of {} bytes exceeds the channel limit.",
                    bytes.len()
                )
            })?;
        let mut data = bytes.to_vec();
        let (done_tx, done_rx) = mpsc::channel();
        on_main_run_loop(move || {
            let status = with_session(|session| {
                // SAFETY: `data` is a live buffer of exactly `length` bytes for the
                // whole synchronous call; IOBluetooth does not retain the pointer.
                unsafe {
                    session
                        .channel
                        .writeSync_length(data.as_mut_ptr().cast::<c_void>(), length)
                }
            });
            let _ = done_tx.send(status);
        })?;
        match done_rx.recv_timeout(WRITE_TIMEOUT) {
            Ok(Some(0)) => Ok(()),
            Ok(Some(status)) => Err(format!("The write failed (IOReturn {status:#x}).")),
            Ok(None) => Err("The Bluetooth channel is closed.".into()),
            Err(_) => Err("The main run loop did not accept the write.".into()),
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        let (done_tx, done_rx) = mpsc::channel();
        let queued = on_main_run_loop(move || {
            let session = SESSION.with(|cell| cell.borrow_mut().take());
            if let Some(session) = session {
                // SAFETY: closing a channel this thread opened; the status is not
                // actionable during teardown.
                let _ = unsafe { session.channel.closeChannel() };
            }
            let _ = done_tx.send(());
        });
        if queued.is_ok() {
            let _ = done_rx.recv_timeout(CLOSE_TIMEOUT);
        }
    }
}

/// Serves the main run loop while `work` runs on another thread, then returns its
/// result. For programs without a UI of their own: the application's AppKit loop
/// does this job. Fails if called from a secondary thread.
pub fn host_main_run_loop<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T> {
    if MainThreadMarker::new().is_none() {
        return Err("The main run loop can only be served from the main thread.".into());
    }
    let run_loop = CFRunLoop::main().ok_or("The main thread has no run loop.")?;
    // A run loop with no sources returns at once. This timer never fires in
    // practice; it keeps the loop alive until `work` finishes and stops it.
    let keep_alive = RcBlock::new(|_: *mut CFRunLoopTimer| {});
    // SAFETY: the timer and block are valid for the call; the loop retains both.
    let timer = unsafe {
        CFRunLoopTimer::with_handler(
            None,
            CFAbsoluteTimeGetCurrent() + KEEP_ALIVE_SECONDS,
            0.0,
            0,
            0,
            Some(&keep_alive),
        )
    }
    .ok_or("Could not create the run-loop keep-alive timer.")?;
    // SAFETY: the mode constant is a valid, immortal CFString.
    run_loop.add_timer(Some(&timer), unsafe { kCFRunLoopDefaultMode });
    let worker = thread::Builder::new()
        .name("bluetooth-worker".into())
        .spawn(move || {
            let result = work();
            if let Some(main) = CFRunLoop::main() {
                main.stop();
            }
            result
        })
        .map_err(|e| format!("Could not start the worker thread: {e}"))?;
    CFRunLoop::run();
    worker
        .join()
        .map_err(|_| "The worker thread panicked.".to_string())
}

/// Queues `job` on the main run loop, from any thread. Fails only if the process
/// has no main run loop; if the loop is not running, the job waits.
fn on_main_run_loop(job: impl FnOnce() + Send + 'static) -> Result<()> {
    let run_loop = CFRunLoop::main().ok_or("The main thread has no run loop.")?;
    let job = Mutex::new(Some(job));
    let block = RcBlock::new(move || {
        if let Some(job) = job.lock().ok().and_then(|mut slot| slot.take()) {
            job();
        }
    });
    // SAFETY: CFRunLoopPerformBlock may be called from any thread. The loop
    // copies and retains the block, whose captures are `Send`; it only runs on
    // the main thread, which is where the session lives.
    unsafe {
        run_loop.perform_block(kCFRunLoopDefaultMode.map(|mode| &**mode), Some(&block));
    }
    run_loop.wake_up();
    Ok(())
}

/// What the main thread owns while the channel is open.
struct Session {
    channel: Retained<IOBluetoothRFCOMMChannel>,
    _delegate: Retained<ChannelDelegate>,
}

thread_local! {
    /// Only ever set on the main thread; blocks queued on its run loop read it.
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

/// Runs `action` with the open session, or returns `None` if there is none.
fn with_session<R>(action: impl FnOnce(&Session) -> R) -> Option<R> {
    SESSION.with(|cell| cell.borrow().as_ref().map(action))
}

/// Opens the channel and stores it as the main thread's session.
fn install_session(name: &str, events: Sender<Event>) -> Result<()> {
    if SESSION.with(|cell| cell.borrow().is_some()) {
        return Err("A Bluetooth link is already open.".into());
    }
    let session = open_channel(name, events)?;
    SESSION.with(|cell| *cell.borrow_mut() = Some(session));
    Ok(())
}

struct DelegateIvars {
    events: Sender<Event>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements. The delegate holds just a
    // `Sender` and is only called from the main thread's run loop.
    #[unsafe(super(NSObject))]
    #[ivars = DelegateIvars]
    #[name = "AudezeBluetoothChannelDelegate"]
    struct ChannelDelegate;

    unsafe impl NSObjectProtocol for ChannelDelegate {}

    unsafe impl IOBluetoothRFCOMMChannelDelegate for ChannelDelegate {
        #[unsafe(method(rfcommChannelData:data:length:))]
        fn data(
            &self,
            _channel: Option<&IOBluetoothRFCOMMChannel>,
            data: *mut c_void,
            length: usize,
        ) {
            if data.is_null() || length == 0 {
                return;
            }
            // SAFETY: IOBluetooth passes a buffer of `length` readable bytes that
            // stays valid for the duration of this callback; it is copied here.
            let bytes = unsafe { slice::from_raw_parts(data.cast::<u8>(), length) }.to_vec();
            let _ = self.ivars().events.send(Event::Data(bytes));
        }

        #[unsafe(method(rfcommChannelOpenComplete:status:))]
        fn open_complete(&self, channel: Option<&IOBluetoothRFCOMMChannel>, status: i32) {
            let mtu = match (status, channel) {
                // SAFETY: `channel` is the live channel this callback is about.
                (0, Some(channel)) => unsafe { channel.getMTU() },
                _ => 0,
            };
            let _ = self.ivars().events.send(Event::Opened { status, mtu });
        }

        #[unsafe(method(rfcommChannelClosed:))]
        fn closed(&self, _channel: Option<&IOBluetoothRFCOMMChannel>) {
            let _ = self.ivars().events.send(Event::Closed);
        }
    }
);

impl ChannelDelegate {
    fn new(events: Sender<Event>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(DelegateIvars { events });
        // SAFETY: `init` on a freshly allocated, unused NSObject subclass.
        unsafe { msg_send![super(this), init] }
    }
}

fn open_channel(name: &str, events: Sender<Event>) -> Result<Session> {
    let device = find_device(name)?;
    // SAFETY: plain property read on a live device.
    if !unsafe { device.isConnected() } {
        return Err(format!("{name} is paired but not connected."));
    }
    let channel_id = rfcomm_channel_id(&device)?;
    let delegate = ChannelDelegate::new(events);
    let mut channel = None;
    // SAFETY: `channel` receives the opened channel; the delegate is retained by
    // the returned session for as long as the channel can call it.
    let status = unsafe {
        device.openRFCOMMChannelAsync_withChannelID_delegate(
            Some(&mut channel),
            channel_id,
            Some(AsRef::<AnyObject>::as_ref(&*delegate)),
        )
    };
    match (status, channel) {
        (0, Some(channel)) => Ok(Session {
            channel,
            _delegate: delegate,
        }),
        (0, None) => Err("IOBluetooth returned no channel.".into()),
        (status, _) => Err(format!(
            "Opening the channel failed (IOReturn {status:#x})."
        )),
    }
}

/// The one paired device whose name is exactly `name`.
fn find_device(name: &str) -> Result<Retained<IOBluetoothDevice>> {
    // SAFETY: class method without preconditions; it returns nil when nothing is paired.
    let paired = unsafe { IOBluetoothDevice::pairedDevices() }
        .ok_or("No paired Bluetooth devices were found.")?;
    let mut matches = paired
        .iter()
        .filter_map(|object| object.downcast::<IOBluetoothDevice>().ok())
        .filter(|device| {
            // SAFETY: plain property read; `getName` may return nil, which is then no match.
            #[allow(deprecated)]
            let device_name = unsafe { device.getName() };
            device_name.is_some_and(|device_name| device_name.to_string() == name)
        });
    match (matches.next(), matches.next()) {
        (Some(device), None) => Ok(device),
        (None, _) => Err(format!("No paired device is named \"{name}\".")),
        (Some(_), Some(_)) => Err(format!("Several paired devices are named \"{name}\".")),
    }
}

/// Looks up the serial service in the device's SDP records and returns its RFCOMM
/// channel number. The number differs between devices and firmware, so it is never fixed.
fn rfcomm_channel_id(device: &IOBluetoothDevice) -> Result<u8> {
    let data = NSData::with_bytes(&SERVICE_UUID);
    // SAFETY: `data` is a live 16-byte NSData; the call returns nil if it cannot parse it.
    let uuid = unsafe { IOBluetoothSDPUUID::uuidWithData(Some(&data)) }
        .ok_or("The service UUID was rejected by IOBluetooth.")?;
    // SAFETY: plain lookup in the device's cached service records.
    let record = unsafe { device.getServiceRecordForUUID(Some(&uuid)) }
        .ok_or("The headset has no control service record.")?;
    let mut channel_id = 0u8;
    // SAFETY: `channel_id` is a valid out pointer for the duration of the call.
    let status = unsafe { record.getRFCOMMChannelID(&mut channel_id) };
    if status == 0 {
        Ok(channel_id)
    } else {
        Err(format!(
            "The control service has no RFCOMM channel (IOReturn {status:#x})."
        ))
    }
}

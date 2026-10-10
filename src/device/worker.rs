//! Maxwell 2 ANC USB-dongle sound protocol. Only the observed sound commands
//! are exposed, including bounded custom EQ edits. No firmware or arbitrary commands.
use super::usb::{Device, Query, QUERIES};
use crate::{
    device::command::{Command, Controller, QueuedCommand, CONTROL_MESSAGE, STATUS_MESSAGE},
    model::SoundState,
};
use std::{
    collections::VecDeque,
    sync::{
        mpsc::{sync_channel, SyncSender},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{Foundation::*, UI::WindowsAndMessaging::*};
type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Copy, Debug)]
pub struct Delivery {
    pub sequence: u64,
    pub received: Instant,
    pub chat: Option<u8>,
}
struct UiTrace {
    delivery: Delivery,
    presented: Instant,
    alpha: u8,
    chat: Option<u8>,
}
static UI_TRACE: OnceLock<SyncSender<UiTrace>> = OnceLock::new();
static LAST_DELIVERY: OnceLock<Mutex<Delivery>> = OnceLock::new();
pub fn delivery_time() -> Option<Delivery> {
    LAST_DELIVERY.get().map(|t| *t.lock().unwrap())
}
pub fn trace_present(delivery: Delivery, alpha: u8, chat: Option<u8>) {
    if let Some(tx) = UI_TRACE.get() {
        let _ = tx.try_send(UiTrace {
            delivery,
            presented: Instant::now(),
            alpha,
            chat,
        });
    }
}
fn post(hwnd: isize, msg: u32) {
    if hwnd != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as _)), msg, WPARAM(0), LPARAM(0));
        }
    }
}
fn next_query(due: &[Instant; 11], now: Instant, previous: Option<Query>) -> Option<usize> {
    let background = (1..7).filter(|i| due[*i] <= now).min_by_key(|i| due[*i]);
    if due[0] <= now && (previous != Some(Query::Mix) || background.is_none()) {
        Some(0)
    } else {
        background
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum ReadyWork {
    Write,
    Query(Query),
}
fn ready_work(
    due: &[Instant; 11],
    now: Instant,
    previous: Option<Query>,
    flight: bool,
    write_ready: Instant,
    readback: Option<Query>,
    commands: bool,
) -> Option<ReadyWork> {
    if flight || now < write_ready {
        None
    } else if let Some(q) = readback {
        // A continuous stream of coalesced drag commands must not postpone
        // reading the last command's result indefinitely.
        Some(ReadyWork::Query(q))
    } else if commands {
        // Saving multiple EQ bands yields to an overdue mix read between bands.
        if previous == Some(Query::EqBands) && due[0] <= now {
            Some(ReadyWork::Query(Query::Mix))
        } else {
            Some(ReadyWork::Write)
        }
    } else {
        next_query(due, now, previous).map(|i| ReadyWork::Query(QUERIES[i]))
    }
}
fn query_timeout(q: Query) -> Duration {
    Duration::from_millis(match q {
        Query::Mix => 160,
        Query::EqWriteAck | Query::EqSelectAck => 500,
        Query::EqBands => 250,
        _ => 100,
    })
}
fn query_interval(q: Query) -> Duration {
    Duration::from_millis(match q {
        Query::Mix => 50,
        Query::Battery => 30000,
        // Five 400 ms background queries consumed about half the serial
        // transport capacity: measured mix intervals were 92 ms, not 50 ms.
        // Space status reads out; explicit controls still schedule readback.
        _ => 1000,
    })
}
#[derive(Default)]
struct TraceMeta<'a> {
    request: Option<u64>,
    delivery: Option<u64>,
    alpha: Option<u8>,
    at: Option<Instant>,
    detail: &'a str,
}
struct Trace {
    file: Option<std::io::BufWriter<std::fs::File>>,
    start: Instant,
    flushed: Instant,
    rows: usize,
}
impl Trace {
    fn new() -> Self {
        use std::io::Write;
        let file = if std::env::args().any(|a| a == "--trace-usb") {
            std::env::current_exe()
                .ok()
                .and_then(|p| std::fs::File::create(p.with_file_name("usb-timing.csv")).ok())
                .map(|f| {
                    let mut w = std::io::BufWriter::new(f);
                    let _ = writeln!(w, "elapsed_ms,event,field,duration_ms,chat_percent,request_seq,delivery_seq,alpha,detail");
                    w
                })
        } else {
            None
        };
        Self {
            file,
            start: Instant::now(),
            flushed: Instant::now(),
            rows: 0,
        }
    }
    fn record(&mut self, event: &str, q: Option<Query>, duration: Duration, chat: Option<u8>) {
        self.record_with(event, q, duration, chat, TraceMeta::default());
    }
    fn record_with(
        &mut self,
        event: &str,
        q: Option<Query>,
        duration: Duration,
        chat: Option<u8>,
        meta: TraceMeta<'_>,
    ) {
        use std::io::Write;
        // Several minutes of detailed events, with an explicit end marker.
        // Disabled in normal operation; never expands without a bound.
        const LIMIT: usize = 30000;
        if self.rows >= LIMIT {
            return;
        }
        if let Some(f) = &mut self.file {
            let _ = writeln!(
                f,
                "{:.3},{},{:?},{:.3},{},{},{},{},{}",
                meta.at
                    .unwrap_or_else(Instant::now)
                    .saturating_duration_since(self.start)
                    .as_secs_f64()
                    * 1000.,
                event,
                q,
                duration.as_secs_f64() * 1000.,
                chat.map(|c| c.to_string()).unwrap_or_default(),
                meta.request.map(|n| n.to_string()).unwrap_or_default(),
                meta.delivery.map(|n| n.to_string()).unwrap_or_default(),
                meta.alpha.map(|n| n.to_string()).unwrap_or_default(),
                meta.detail,
            );
            self.rows += 1;
            if self.rows == LIMIT {
                let _ = writeln!(
                    f,
                    "{:.3},trace_stopped,None,0,,,,,row_limit",
                    self.start.elapsed().as_secs_f64() * 1000.
                );
            }
            if self.flushed.elapsed() >= Duration::from_secs(2) || self.rows == LIMIT {
                let _ = f.flush();
                self.flushed = Instant::now();
            }
        }
    }
    fn enabled(&self) -> bool {
        self.file.is_some() && self.rows < 30000
    }
    fn presentation(&mut self, sample: UiTrace) {
        self.record_with(
            if sample.alpha >= 230 {
                "ui_visible"
            } else {
                "ui_present"
            },
            None,
            sample
                .presented
                .saturating_duration_since(sample.delivery.received),
            sample.chat,
            TraceMeta {
                delivery: Some(sample.delivery.sequence),
                alpha: Some(sample.alpha),
                at: Some(sample.presented),
                detail: if sample.chat == sample.delivery.chat {
                    "value_matches_delivery"
                } else {
                    "value_differs_from_delivery"
                },
                ..Default::default()
            },
        );
    }
}
pub fn start(hwnd: isize, shared: Arc<Mutex<SoundState>>) -> Controller {
    let (tx, rx) = sync_channel(1);
    let c = Controller {
        sequence: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        pending: Arc::new(Mutex::new(VecDeque::new())),
        wake: tx,
        error: Arc::new(Mutex::new(None)),
    };
    let worker = c.clone();
    thread::spawn(move || {
        let mut trace = Trace::new();
        let (ui_tx, ui_rx) = sync_channel(64);
        if trace.file.is_some() {
            let _ = UI_TRACE.set(ui_tx);
        }
        let mut request_sequence = 0u64;
        let mut delivery_sequence = 0u64;
        loop {
            let result = (|| -> Result<()> {
                let mut device = Device::open()?;
                let mut state = SoundState {
                    device: Some("Maxwell 2 ANC".into()),
                    connection: Some("USB Dongle".into()),
                    ..SoundState::default()
                };
                let mut due = [Instant::now(); 11];
                let mut last_good = Instant::now();
                let mut waiting: Vec<(Query, Instant, QueuedCommand)> = vec![];
                let mut flight: Option<(Query, Instant, u64)> = None;
                let mut write_ready = Instant::now();
                let mut read_ready = Instant::now();
                let mut previous = None;
                let mut pending_readback = None;
                let mut eq_retry: Option<Instant> = None;
                loop {
                    #[cfg(feature = "perf-stats")]
                    super::perf::CYCLES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    while let Ok(sample) = ui_rx.try_recv() {
                        trace.presentation(sample);
                    }
                    let mut state_may_have_changed = false;
                    let work = ready_work(
                        &due,
                        Instant::now(),
                        previous,
                        flight.is_some(),
                        write_ready,
                        pending_readback.or_else(|| {
                            eq_retry
                                .filter(|at| *at <= Instant::now())
                                .map(|_| Query::EqIndex)
                        }),
                        !worker.pending.lock().unwrap().is_empty(),
                    );
                    if work == Some(ReadyWork::Write) {
                        let cmd = worker.pending.lock().unwrap().pop_front();
                        if let Some(cmd) = cmd {
                            let sent = Instant::now();
                            match device.set(&cmd.command) {
                                Ok(q) => {
                                    trace.record("write", Some(q), sent.elapsed(), None);
                                    write_ready = Instant::now()
                                        + if matches!(
                                            cmd.command,
                                            Command::EqLoad(_)
                                                | Command::EqSelect(_)
                                                | Command::EqRefresh
                                                | Command::EqGain { .. }
                                        ) {
                                            Duration::ZERO
                                        } else {
                                            Duration::from_millis(40)
                                        };
                                    due[q as usize] = write_ready;
                                    pending_readback = Some(q);
                                    waiting.retain(|(_, _, old)| {
                                        !cmd.command.supersedes(&old.command)
                                    });
                                    waiting.push((q, Instant::now(), cmd));
                                }
                                Err(e) => {
                                    if matches!(cmd.command, Command::Mix(_)) {
                                        state.mix_revision = cmd.sequence;
                                        state_may_have_changed = true;
                                    }
                                    *worker.error.lock().unwrap() = Some(e);
                                    post(hwnd, CONTROL_MESSAGE);
                                }
                            }
                        }
                    }
                    if let Some(ReadyWork::Query(q)) = work {
                        if q == Query::EqIndex {
                            eq_retry = None;
                        }
                        let i = q as usize;
                        request_sequence += 1;
                        if trace.enabled() {
                            let scheduled = Instant::now();
                            trace.record_with(
                                "query_due",
                                Some(q),
                                scheduled.saturating_duration_since(due[i]),
                                None,
                                TraceMeta {
                                    request: Some(request_sequence),
                                    at: Some(scheduled),
                                    ..Default::default()
                                },
                            );
                        }
                        let sent = trace.enabled().then(Instant::now);
                        #[cfg(feature = "perf-stats")]
                        super::perf::QUERIES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        device.query(q)?;
                        pending_readback = None;
                        let written = Instant::now();
                        trace.record_with(
                            "query",
                            Some(q),
                            sent.map(|sent| written.saturating_duration_since(sent))
                                .unwrap_or_default(),
                            None,
                            TraceMeta {
                                request: Some(request_sequence),
                                at: Some(written),
                                ..Default::default()
                            },
                        );
                        flight = Some((q, written, request_sequence));
                        previous = Some(q);
                        read_ready = written + Duration::from_millis(25);
                        due[i] = written + query_interval(q);
                    }
                    let mut updated_fields = 0u16;
                    let mut received = Instant::now();
                    if let Some((q, started, request)) = flight {
                        if Instant::now() >= read_ready {
                            for extra_reads in 0..=2 {
                                if trace.enabled() {
                                    let scheduled = Instant::now();
                                    trace.record_with(
                                        if extra_reads == 0 {
                                            "read_due"
                                        } else {
                                            "continuation_read"
                                        },
                                        Some(q),
                                        if extra_reads == 0 {
                                            scheduled.saturating_duration_since(read_ready)
                                        } else {
                                            Duration::ZERO
                                        },
                                        None,
                                        TraceMeta {
                                            request: Some(request),
                                            at: Some(scheduled),
                                            ..Default::default()
                                        },
                                    );
                                }
                                let read = trace.enabled().then(Instant::now);
                                #[cfg(feature = "perf-stats")]
                                super::perf::READS
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                let report = device.read_report()?;
                                #[cfg(feature = "perf-stats")]
                                if report[1] == 0 {
                                    super::perf::EMPTY
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                received = Instant::now();
                                let raw = if trace.enabled() {
                                    let len = (report[1] as usize + 3).min(report.len());
                                    report[..len]
                                        .iter()
                                        .map(|b| format!("{b:02x}"))
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                } else {
                                    String::new()
                                };
                                trace.record_with(
                                    "get_report",
                                    Some(q),
                                    read.map(|read| received.saturating_duration_since(read))
                                        .unwrap_or_default(),
                                    None,
                                    TraceMeta {
                                        request: Some(request),
                                        at: Some(received),
                                        detail: &raw,
                                        ..Default::default()
                                    },
                                );
                                for field in device.consume_report_observed(
                                    &report,
                                    &mut state,
                                    |bytes, field, value| {
                                        if trace.enabled() {
                                            let raw = bytes
                                                .iter()
                                                .map(|b| format!("{b:02x}"))
                                                .collect::<Vec<_>>()
                                                .join(" ");
                                            trace.record_with(
                                                if field.is_some() {
                                                    "parsed_frame"
                                                } else {
                                                    "unrecognized_frame"
                                                },
                                                field,
                                                received.saturating_duration_since(started),
                                                if field == Some(Query::Mix) {
                                                    value.mix.map(|m| m.1)
                                                } else {
                                                    None
                                                },
                                                TraceMeta {
                                                    request: Some(request),
                                                    at: Some(received),
                                                    detail: &raw,
                                                    ..Default::default()
                                                },
                                            );
                                        }
                                    },
                                ) {
                                    updated_fields |= 1 << field as u8;
                                }
                                if updated_fields & (1 << q as u8) != 0
                                    || !device.needs_continuation(&report, extra_reads)
                                {
                                    break;
                                }
                            }
                            read_ready = Instant::now()
                                + Duration::from_millis(
                                    if started.elapsed() < Duration::from_millis(50) {
                                        20
                                    } else {
                                        40
                                    },
                                );
                        }
                        if updated_fields & (1 << q as u8) != 0 {
                            trace.record_with(
                                "response",
                                Some(q),
                                received.saturating_duration_since(started),
                                if q == Query::Mix {
                                    state.mix.map(|m| m.1)
                                } else {
                                    None
                                },
                                TraceMeta {
                                    request: Some(request),
                                    at: Some(received),
                                    ..Default::default()
                                },
                            );
                            flight = None;
                            if matches!(q, Query::EqWriteAck | Query::EqSelectAck) {
                                let readback = if q == Query::EqSelectAck {
                                    Query::EqIndex
                                } else {
                                    Query::EqBands
                                };
                                // Firmware may ignore a bank read while persisting a write.
                                // Only the ACK releases the subsequent readback; never mark
                                // a band saved from its acknowledgment alone.
                                for (field, _, _) in &mut waiting {
                                    if *field == q {
                                        *field = readback;
                                    }
                                }
                                pending_readback = Some(readback);
                            }
                        } else if started.elapsed() >= query_timeout(q) {
                            trace.record_with(
                                "timeout",
                                Some(q),
                                started.elapsed(),
                                None,
                                TraceMeta {
                                    request: Some(request),
                                    ..Default::default()
                                },
                            );
                            if matches!(q, Query::EqBands | Query::EqWriteAck | Query::EqSelectAck)
                            {
                                return Err(
                                    "EQ response timed out. Reconnecting before another bank is read."
                                        .into(),
                                );
                            }
                            // A slow secondary query gets a cooldown instead of
                            // repeatedly occupying the queue ahead of the mix dial.
                            if q != Query::Mix {
                                due[q as usize] = Instant::now() + Duration::from_secs(1);
                            }
                            flight = None;
                        }
                    }
                    if updated_fields != 0 {
                        state_may_have_changed = true;
                        last_good = Instant::now();
                        if state.status.is_empty() {
                            state.status = "Direct USB · Audeze app not required".into();
                        }
                    }
                    waiting.retain(|(q, started, cmd)| {
                        let matches = command_matches(&cmd.command, &state);
                        if updated_fields & (1 << *q as u8) != 0 && matches {
                            if *q == Query::EqIndex {
                                state.eq_index_revision = cmd.sequence;
                                state_may_have_changed = true;
                            }
                            if *q == Query::EqBands {
                                state.eq_revision = cmd.sequence;
                                state_may_have_changed = true;
                            }
                            if *q == Query::Mix {
                                state.mix_revision = cmd.sequence;
                                state_may_have_changed = true;
                            }
                            false
                        } else if started.elapsed() > Duration::from_millis(900) {
                            if *q == Query::Mix {
                                state.mix_revision = cmd.sequence;
                                state_may_have_changed = true;
                            }
                            *worker.error.lock().unwrap() =
                                Some("The headset did not confirm that change. Try again.".into());
                            post(hwnd, CONTROL_MESSAGE);
                            false
                        } else {
                            true
                        }
                    });
                    // A successful selection ACK precedes the actual preset becoming active.
                    // Retry only this in-flight selection, bounded by the existing deadline.
                    let selecting = waiting
                        .iter()
                        .any(|(_, _, c)| matches!(c.command, Command::EqSelect(_)));
                    if !selecting {
                        eq_retry = None;
                    } else if updated_fields & (1 << Query::EqIndex as u8) != 0 {
                        eq_retry = Some(Instant::now() + Duration::from_millis(60));
                    }
                    if last_good.elapsed() > Duration::from_secs(3) {
                        return Err("Headset asleep or disconnected. Reconnecting…".into());
                    }
                    if state_may_have_changed {
                        let mut out = shared.lock().unwrap();
                        if *out != state {
                            *out = state.clone();
                            let delivery = if UI_TRACE.get().is_some() {
                                delivery_sequence += 1;
                                let delivery = Delivery {
                                    sequence: delivery_sequence,
                                    received,
                                    chat: state.mix.map(|m| m.1),
                                };
                                *LAST_DELIVERY
                                    .get_or_init(|| Mutex::new(delivery))
                                    .lock()
                                    .unwrap() = delivery;
                                Some(delivery)
                            } else {
                                None
                            };
                            // Publish the matching trace stamp before making this
                            // state visible to the UI. Do not write logs while
                            // holding the shared state mutex.
                            drop(out);
                            if let Some(delivery) = delivery {
                                trace.record_with(
                                    "publish",
                                    None,
                                    received.elapsed(),
                                    delivery.chat,
                                    TraceMeta {
                                        request: Some(request_sequence),
                                        delivery: Some(delivery_sequence),
                                        ..Default::default()
                                    },
                                );
                            }
                            post(hwnd, STATUS_MESSAGE);
                        } else {
                            drop(out);
                        }
                    }
                    let wake_at = if let Some((q, t, _)) = flight {
                        read_ready.min(t + query_timeout(q))
                    } else {
                        let next = if worker.pending.lock().unwrap().is_empty() {
                            *due[..7].iter().min().unwrap()
                        } else {
                            Instant::now()
                        };
                        eq_retry
                            .map(|retry| next.min(retry))
                            .unwrap_or(next)
                            .max(write_ready)
                    };
                    // Sleep until useful work is due or a user command wakes us.
                    // No fixed 10 ms USB read or wake loop when there is no reply due.
                    let sleep_for = wake_at.saturating_duration_since(Instant::now());
                    if sleep_for.is_zero() {
                        let _ = rx.try_recv();
                    } else {
                        let _ = rx.recv_timeout(sleep_for);
                    }
                }
            })();
            if let Err(e) = result {
                *shared.lock().unwrap() = SoundState {
                    status: e,
                    ..Default::default()
                };
                post(hwnd, STATUS_MESSAGE);
                worker.pending.lock().unwrap().clear();
                let _ = rx.recv_timeout(Duration::from_secs(2));
            }
        }
    });
    c
}
pub fn command_matches(c: &Command, s: &SoundState) -> bool {
    match c {
        Command::EqRefresh => s.eq_index.is_some(),
        Command::EqSelect(slot) => s.eq_index == Some(*slot),
        Command::EqLoad(slot) => s.eq_bands.is_some_and(|eq| eq.slot == *slot),
        Command::EqGain { slot, band } => s.eq_bands.is_some_and(|eq| {
            eq.slot == *slot && eq.bands.iter().any(|b| b.id == band.id && *b == *band)
        }),
        Command::Mix(p) => s.mix.map(|m| m.1) == Some((*p as f32 / 5.).round() as u8 * 5),
        Command::Sidetone(b) => s.sidetone == Some(*b),
        Command::SidetoneLevel(p) => s
            .sidetone_level
            .map(|v| (v as i16 - *p as i16).abs() <= 2)
            .unwrap_or(false),
        Command::Anc(a) => s.anc.as_ref() == Some(a),
        Command::Noise(n) => s.noise.as_ref() == Some(n),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eq_write_requires_ack_then_readback_without_changing_mix_timing() {
        let now = Instant::now();
        let due = [now; 11];
        assert_eq!(
            ready_work(
                &due,
                now,
                Some(Query::Mix),
                true,
                now,
                Some(Query::EqWriteAck),
                true
            ),
            None
        );
        assert_eq!(
            ready_work(
                &due,
                now,
                Some(Query::EqWriteAck),
                false,
                now,
                Some(Query::EqBands),
                true
            ),
            Some(ReadyWork::Query(Query::EqBands))
        );
        assert_eq!(query_interval(Query::Mix), Duration::from_millis(50));
        assert_eq!(query_timeout(Query::Mix), Duration::from_millis(160));
        assert_eq!(query_timeout(Query::EqWriteAck), Duration::from_millis(500));
        assert_eq!(query_timeout(Query::Anc), Duration::from_millis(100));
        assert_eq!(query_timeout(Query::EqBands), Duration::from_millis(250));
    }
    #[test]
    fn eq_saves_yield_to_due_mix_and_eq_is_never_polled_when_idle() {
        let now = Instant::now();
        let mut due = [now + Duration::from_secs(1); 11];
        due[7] = now;
        due[8] = now;
        assert_eq!(next_query(&due, now, None), None);
        due[0] = now;
        assert_eq!(
            ready_work(&due, now, Some(Query::EqBands), false, now, None, true),
            Some(ReadyWork::Query(Query::Mix))
        );
        assert_eq!(
            ready_work(&due, now, Some(Query::Mix), false, now, None, true),
            Some(ReadyWork::Write)
        );
    }
    #[test]
    fn continuous_commands_get_one_readback_before_the_next_write() {
        let start = Instant::now();
        let due = [start; 11];
        let mut now = start;
        let mut previous = None;
        let mut operations = vec![];
        // The queue never empties during this simulated drag. Alternate a
        // successful reply and a timeout; neither may permit a second write
        // before the first queried readback finishes.
        for index in 0..20 {
            let q = if index % 2 == 0 {
                Query::Mix
            } else {
                Query::Side
            };
            let next = ready_work(&due, now, previous, false, now, None, true);
            assert_eq!(next, Some(ReadyWork::Write));
            operations.push(next.unwrap());
            let settled = now + Duration::from_millis(40);
            assert_eq!(
                ready_work(
                    &due,
                    settled - Duration::from_millis(1),
                    previous,
                    false,
                    settled,
                    Some(q),
                    true
                ),
                None
            );
            let next = ready_work(&due, settled, previous, false, settled, Some(q), true);
            assert_eq!(next, Some(ReadyWork::Query(q)));
            operations.push(next.unwrap());
            previous = Some(q);
            let finished = settled
                + if index % 2 == 0 {
                    Duration::from_millis(30)
                } else {
                    query_timeout(q)
                };
            assert_eq!(
                ready_work(
                    &due,
                    finished - Duration::from_millis(1),
                    previous,
                    true,
                    settled,
                    None,
                    true
                ),
                None
            );
            // Once this query replies or times out, do not wait for the
            // separate 900 ms confirmation-error deadline to accept input.
            assert_eq!(
                ready_work(&due, finished, previous, false, settled, None, true),
                Some(ReadyWork::Write)
            );
            now = finished;
        }
        assert_eq!(operations.len(), 40);
        for pair in operations.as_chunks::<2>().0 {
            assert_eq!(pair[0], ReadyWork::Write);
            assert!(matches!(pair[1], ReadyWork::Query(_)));
        }
        assert_eq!(
            ready_work(&due, now, Some(Query::Anc), false, now, None, false),
            Some(ReadyWork::Query(Query::Mix))
        );
    }
    #[test]
    fn trace_preserves_value_sequence_and_distinguishes_invisible_presentation() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!(
            "audeze-overlay-trace-test-{}-{}.csv",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let start = Instant::now();
        let mut trace = Trace {
            file: Some(std::io::BufWriter::new(
                std::fs::File::create(&path).unwrap(),
            )),
            start,
            flushed: start,
            rows: 0,
        };
        let delivery = Delivery {
            sequence: 7,
            received: start + Duration::from_millis(10),
            chat: Some(55),
        };
        trace.presentation(UiTrace {
            delivery,
            presented: start + Duration::from_millis(35),
            alpha: 0,
            chat: Some(55),
        });
        trace.presentation(UiTrace {
            delivery,
            presented: start + Duration::from_millis(55),
            alpha: 255,
            chat: Some(55),
        });
        trace.file.as_mut().unwrap().flush().unwrap();
        drop(trace);
        let rows = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(rows.contains("35.000,ui_present,None,25.000,55,,7,0,value_matches_delivery"));
        assert!(rows.contains("55.000,ui_visible,None,45.000,55,,7,255,value_matches_delivery"));
    }
    #[test]
    fn overdue_background_reads_cannot_queue_ahead_of_mix() {
        let now = Instant::now();
        let due = [now - Duration::from_secs(1); 11];
        assert_eq!(next_query(&due, now, Some(Query::Anc)), Some(0));
        let background = next_query(&due, now, Some(Query::Mix)).unwrap();
        assert_ne!(background, 0);
        assert_eq!(
            next_query(
                &due,
                now + query_timeout(QUERIES[background]),
                Some(QUERIES[background])
            ),
            Some(0)
        );
        let future = [now + Duration::from_secs(1); 11];
        assert_eq!(next_query(&future, now, None), None);
    }
}

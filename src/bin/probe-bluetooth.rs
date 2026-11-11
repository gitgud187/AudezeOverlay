//! Read-only diagnostic for the headset's Bluetooth control channel (macOS).
//!
//! Sends only the status reads in `ReadRequest`; it cannot write a setting or send
//! arbitrary bytes. It measures connect time, per-read latency, a burst of reads
//! and how fast the headset keeps up with repeated reads, then listens for
//! traffic nobody asked for. Run it through `tools/run-bluetooth-probe.sh`: macOS
//! aborts a bare command-line process at its first Bluetooth call.
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("The Bluetooth diagnostic is available on macOS only.");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    use audeze_overlay::device::bluetooth::host_main_run_loop;
    // IOBluetooth reports on the main run loop, so the main thread serves it and
    // the probe itself runs on a worker thread.
    let options = probe::Options::from_args();
    match host_main_run_loop(move || probe::run(&options)).and_then(|result| result) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "macos")]
mod probe {
    use audeze_overlay::{
        device::{
            bluetooth::{Event, Link, ReadRequest, DEVICE_NAME},
            consume_stream_bytes, Query,
        },
        model::SoundState,
    };
    use std::{
        collections::VecDeque,
        time::{Duration, Instant},
    };

    pub struct Options {
        device: String,
        idle_seconds: u64,
        poll_intervals_ms: Vec<u64>,
        poll_seconds: u64,
    }

    impl Options {
        pub fn from_args() -> Self {
            let args: Vec<String> = std::env::args().collect();
            let value = |flag: &str| {
                args.iter()
                    .position(|arg| arg == flag)
                    .and_then(|i| args.get(i + 1))
            };
            Self {
                device: value("--device")
                    .cloned()
                    .unwrap_or_else(|| DEVICE_NAME.into()),
                idle_seconds: value("--idle")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10)
                    .min(300),
                poll_intervals_ms: value("--poll-ms")
                    .map(|v| v.split(',').filter_map(|part| part.parse().ok()).collect())
                    .unwrap_or_default(),
                poll_seconds: value("--poll-seconds")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(5)
                    .clamp(1, 60),
            }
        }
    }

    /// A complete response frame and when its last byte arrived.
    struct Frame {
        at: Instant,
        bytes: Vec<u8>,
        /// What the application's USB parser makes of it, if anything.
        parsed: Option<Query>,
    }

    /// Splits the channel's byte stream into frames with the same code the USB path uses.
    struct Receiver<'a> {
        link: &'a Link,
        stream: Vec<u8>,
        state: SoundState,
    }

    impl<'a> Receiver<'a> {
        fn new(link: &'a Link) -> Self {
            Self {
                link,
                stream: Vec::new(),
                state: SoundState::default(),
            }
        }

        /// Waits up to `timeout` for more data and returns the frames it completed.
        fn next(&mut self, timeout: Duration) -> Result<Vec<Frame>, String> {
            match self.link.recv(timeout) {
                Some(Event::Data(bytes)) => {
                    let at = Instant::now();
                    let mut frames = Vec::new();
                    consume_stream_bytes(
                        &mut self.stream,
                        &bytes,
                        &mut self.state,
                        |frame, parsed, _| {
                            frames.push(Frame {
                                at,
                                bytes: frame.to_vec(),
                                parsed,
                            });
                        },
                        None,
                    );
                    Ok(frames)
                }
                Some(Event::Closed) => Err("The channel closed.".into()),
                Some(Event::Opened { .. }) | None => Ok(Vec::new()),
            }
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn ms(duration: Duration) -> f64 {
        duration.as_secs_f64() * 1000.0
    }

    pub fn run(options: &Options) -> Result<(), String> {
        let opened = Instant::now();
        let link = Link::open(&options.device)?;
        println!(
            "opened in {:.0} ms, mtu {}",
            ms(opened.elapsed()),
            link.mtu()
        );
        println!("threading: probe on a worker thread; the main thread serves the run loop");
        let mut receiver = Receiver::new(&link);

        println!("\n== one read at a time");
        for request in ReadRequest::ALL {
            let sent = Instant::now();
            link.write(request.frame())?;
            let mut answered = false;
            while !answered && sent.elapsed() < Duration::from_secs(2) {
                for frame in receiver.next(Duration::from_millis(50))? {
                    if request.answered_by(&frame.bytes) {
                        answered = true;
                        println!(
                            "{:<14} {:>5.1} ms  parser: {:<12} {}",
                            request.name(),
                            ms(frame.at - sent),
                            format!("{:?}", frame.parsed),
                            hex(&frame.bytes)
                        );
                    }
                }
            }
            if !answered {
                println!("{:<14} no reply within 2 s", request.name());
            }
            std::thread::sleep(Duration::from_millis(200));
        }

        println!("\n== all reads back to back");
        let sent = Instant::now();
        for request in ReadRequest::ALL {
            link.write(request.frame())?;
        }
        let written = sent.elapsed();
        let mut pending = ReadRequest::ALL.to_vec();
        while !pending.is_empty() && sent.elapsed() < Duration::from_secs(3) {
            for frame in receiver.next(Duration::from_millis(50))? {
                if let Some(i) = pending.iter().position(|r| r.answered_by(&frame.bytes)) {
                    println!(
                        "{:<14} {:>5.1} ms",
                        pending.remove(i).name(),
                        ms(frame.at - sent)
                    );
                }
            }
        }
        println!(
            "12 writes took {:.1} ms; unanswered: {:?}",
            ms(written),
            pending.iter().map(|r| r.name()).collect::<Vec<_>>()
        );

        for interval_ms in &options.poll_intervals_ms {
            poll(&link, &mut receiver, *interval_ms, options.poll_seconds)?;
        }

        println!(
            "\n== listening {} s for unrequested traffic",
            options.idle_seconds
        );
        let listen_until = Instant::now() + Duration::from_secs(options.idle_seconds);
        let mut unrequested = 0;
        while Instant::now() < listen_until {
            for frame in receiver.next(Duration::from_millis(200))? {
                unrequested += 1;
                println!("unrequested: {}", hex(&frame.bytes));
            }
        }
        println!("{unrequested} unrequested frames");
        Ok(())
    }

    /// Reads the mix every `interval_ms` for `seconds`, without waiting for replies,
    /// and reports whether the headset kept up. Only reads; the effect on the
    /// headset's battery is not measured here.
    fn poll(
        link: &Link,
        receiver: &mut Receiver<'_>,
        interval_ms: u64,
        seconds: u64,
    ) -> Result<(), String> {
        println!("\n== mix read every {interval_ms} ms for {seconds} s");
        let interval = Duration::from_millis(interval_ms.max(1));
        let started = Instant::now();
        let mut next_send = started;
        let mut in_flight: VecDeque<Instant> = VecDeque::new();
        let mut latencies: Vec<f64> = Vec::new();
        let mut sent = 0u32;
        let end = started + Duration::from_secs(seconds);
        while Instant::now() < end + Duration::from_secs(1) {
            if Instant::now() >= next_send && Instant::now() < end {
                in_flight.push_back(Instant::now());
                link.write(ReadRequest::Mix.frame())?;
                sent += 1;
                next_send += interval;
            }
            let wait = next_send
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(5));
            for frame in receiver.next(wait)? {
                if ReadRequest::Mix.answered_by(&frame.bytes) {
                    if let Some(at) = in_flight.pop_front() {
                        latencies.push(ms(frame.at - at));
                    }
                }
            }
        }
        latencies.sort_by(f64::total_cmp);
        let pick = |fraction: f64| {
            latencies
                .get(((latencies.len() as f64 - 1.0) * fraction).round() as usize)
                .copied()
        };
        println!(
            "sent {sent}, answered {}, lost {}, latency ms: median {:.1?}, p95 {:.1?}, max {:.1?}",
            latencies.len(),
            in_flight.len(),
            pick(0.5),
            pick(0.95),
            pick(1.0)
        );
        Ok(())
    }
}

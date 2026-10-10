//! Bounded listener probe. Only documented-in-source status queries; no setters.
use audeze_overlay::device::{Device, QUERIES};
use std::time::{Duration, Instant};
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    let seconds = args
        .iter()
        .position(|s| s == "--seconds")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(10)
        .min(120);
    let mut device = Device::open()?;
    let mut listener = device.listen()?;
    if args.iter().any(|s| s == "--query") {
        for q in QUERIES {
            let _ = listener.read(0)?;
            device.query(q)?;
            let started = Instant::now();
            while started.elapsed() < Duration::from_millis(500) {
                if let Some(bytes) = listener.read(100)? {
                    println!("{q:?} {}ms {:02x?}", started.elapsed().as_millis(), bytes);
                }
            }
            let mut state = Default::default();
            let got = device.read(&mut state)?;
            println!("GET_REPORT comparison {q:?}: {got:?}, {state:?}");
        }
    }
    println!("LISTENING ONLY for {seconds}s; no status queries. Turn the physical mix wheel.");
    let start = Instant::now();
    let mut count = 0;
    while start.elapsed() < Duration::from_secs(seconds) {
        if let Some(bytes) = listener.read(1000)? {
            count += 1;
            println!("{}ms {:02x?}", start.elapsed().as_millis(), bytes);
            if args.iter().any(|s| s == "--read-notification") {
                for _ in 0..4 {
                    let report = device.read_report()?;
                    println!("NOTIFICATION PAYLOAD {:02x?}", report);
                    if report[1] == 0 {
                        break;
                    }
                }
            }
        }
    }
    println!("FINISHED: {count} unsolicited reports.");
    Ok(())
}

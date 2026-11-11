//! One bounded, read-only status capture. Close other headset controllers first.
use audeze_overlay::device::{Device, Query, QUERIES};
use std::{
    thread::sleep,
    time::{Duration, Instant},
};

fn main() -> Result<(), String> {
    let mut device = Device::open()?;
    let mut state = device.initial_state();
    println!("Profile: {:?}", device.profile);
    for query in [Query::MaxSidetone].into_iter().chain(QUERIES) {
        if !device.profile.supports_query(query) {
            continue;
        }
        device.query(query)?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(160) {
            sleep(Duration::from_millis(10));
            if device.receive()? {
                println!(
                    "{query:?} {}ms {:02x?}",
                    start.elapsed().as_millis(),
                    device.chunk()
                );
                device.consume(&mut state);
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?
    );
    Ok(())
}

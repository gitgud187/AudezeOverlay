use audeze_overlay::{legacy::accessibility as telemetry, model::SoundState};
use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
pub fn main() {
    let shared = Arc::new(Mutex::new(SoundState::default()));
    telemetry::start(0, shared.clone());
    let start = Instant::now();
    let mut previous = SoundState::default();
    while start.elapsed() < Duration::from_secs(120) {
        let now = shared.lock().unwrap().clone();
        if now != previous {
            println!(
                "{}ms {}",
                start.elapsed().as_millis(),
                serde_json::to_string(&now).unwrap()
            );
            previous = now;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

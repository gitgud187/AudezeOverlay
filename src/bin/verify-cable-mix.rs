//! Opt-in hardware test: sets Game/Chat to 20/80, then watches cable recovery.
//! Other controls and EQ are untouched. Close competing controllers first.
//! Polling device enumeration here simulates Windows notifications for this
//! console test only; the application itself uses device-change notifications.
use audeze_overlay::{
    device::{self, command::Command},
    model::SoundState,
};
use std::{
    sync::{Arc, Mutex},
    thread::sleep,
    time::{Duration, Instant},
};
fn main() -> Result<(), String> {
    if !std::env::args().any(|a| a == "--set-mix-20-80") {
        return Err("Requires --set-mix-20-80; this test changes the headset mix to 20/80.".into());
    }
    let shared = Arc::new(Mutex::new(SoundState::default()));
    let controller = device::start(0, shared.clone());
    let start = Instant::now();
    let mut initialized = false;
    let mut before = SoundState::default();
    let mut enumeration = start;
    while start.elapsed() < Duration::from_secs(180) {
        if enumeration.elapsed() >= Duration::from_millis(500) {
            controller.devices_changed();
            enumeration = Instant::now();
        }
        let state = shared.lock().map_err(|e| e.to_string())?.clone();
        if !initialized && state.mix.is_some() {
            controller.send(Command::Mix(80));
            initialized = true;
        }
        if state != before {
            println!(
                "{}ms {}",
                start.elapsed().as_millis(),
                serde_json::to_string(&state).map_err(|e| e.to_string())?
            );
            before = state;
        }
        sleep(Duration::from_millis(50));
    }
    if before.mix != Some((20, 80)) {
        return Err("Final mix was not 20/80.".into());
    }
    Ok(())
}

//! Read-only by default. --verify-same-values also rewrites existing values unchanged.
//! Quit the normal overlay before running.
use audeze_overlay::{
    device::{command::Command, Device, Query},
    model::SoundState,
};
use std::time::{Duration, Instant};
fn read(d: &mut Device, s: &mut SoundState, q: Query) -> Result<(), String> {
    d.query(q)?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(25));
        let r = d.read_report()?;
        if r[1] != 0 {
            println!(
                "report {}",
                r.iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        if d.consume_report(&r, s).contains(&q) {
            return Ok(());
        }
    }
    Err(format!("No valid {q:?} reply"))
}
fn main() -> Result<(), String> {
    if std::env::args().any(|s| s == "--worker") {
        return worker();
    }
    let mut d = Device::open()?;
    let mut s = SoundState::default();
    read(&mut d, &mut s, Query::EqIndex)?;
    println!("Current EQ: {:?}", s.eq_index);
    let verify = std::env::args().any(|s| s == "--verify-same-values");
    if verify {
        let slot = s.eq_index.ok_or("Missing active EQ")?;
        d.set(&Command::EqSelect(slot))?;
        std::thread::sleep(Duration::from_millis(40));
        read(&mut d, &mut s, Query::EqIndex)?;
        if s.eq_index != Some(slot) {
            return Err("Active EQ readback mismatch".into());
        }
    }
    for slot in 6..=9 {
        d.set(&Command::EqLoad(slot))?;
        read(&mut d, &mut s, Query::EqBands)?;
        println!("{}", serde_json::to_string(&s.eq_bands).unwrap());
        if verify {
            let original = s.eq_bands.ok_or("Missing EQ bands")?;
            d.set(&Command::EqGain {
                slot,
                band: original.bands[0],
            })?;
            read(&mut d, &mut s, Query::EqWriteAck)?;
            read(&mut d, &mut s, Query::EqBands)?;
            if s.eq_bands != Some(original) {
                return Err(format!("Slot {slot} readback mismatch"));
            }
            println!("Slot {slot}: unchanged band write and full-bank readback verified");
        }
    }
    Ok(())
}

fn worker() -> Result<(), String> {
    use std::sync::{Arc, Mutex};
    let shared = Arc::new(Mutex::new(SoundState::default()));
    let controller = audeze_overlay::device::start(0, shared.clone());
    let wait = |ready: &dyn Fn(&SoundState) -> bool| -> Result<SoundState, String> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            let snapshot = shared.lock().unwrap().clone();
            if ready(&snapshot) {
                return Ok(snapshot);
            }
            if let Some(error) = controller.last_error() {
                return Err(error);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        Err(format!(
            "Worker EQ response timed out: {:?}",
            shared.lock().unwrap()
        ))
    };
    wait(&|s| s.mix.is_some())?;
    println!("Worker connected");
    controller.send(Command::EqRefresh);
    let initial = wait(&|s| s.eq_index.is_some())?;
    println!("Worker active EQ: {:?}", initial.eq_index);
    let verify = std::env::args().any(|s| s == "--verify-same-values");
    for slot in 6..=9 {
        println!("Loading slot {slot}");
        let revision = controller.send(Command::EqLoad(slot));
        let loaded =
            wait(&|s| s.eq_revision >= revision && s.eq_bands.is_some_and(|eq| eq.slot == slot))?;
        let original = loaded.eq_bands.unwrap();
        println!("Loading slot {slot}");
        let revision = controller.send(Command::EqLoad(slot));
        wait(&|s| s.eq_revision >= revision && s.eq_bands == Some(original))?;
        if verify {
            let revision = controller.send(Command::EqGain {
                slot,
                band: original.bands[0],
            });
            wait(&|s| s.eq_revision >= revision && s.eq_bands == Some(original))?;
        }
        println!("Worker slot {slot}: repeated loading and requested readback verified");
    }
    if shared.lock().unwrap().eq_index != initial.eq_index {
        return Err("Active EQ unexpectedly changed".into());
    }
    Ok(())
}

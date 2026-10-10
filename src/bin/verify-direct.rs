use audeze_overlay::{
    device::command::Command,
    device::{command_matches, Device, QUERIES},
    model::SoundState,
};
use std::{
    thread::sleep,
    time::{Duration, Instant},
};
fn collect(d: &mut Device, s: &mut SoundState) -> Result<(), String> {
    for q in QUERIES {
        d.query(q)?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(100) {
            sleep(Duration::from_millis(10));
            d.read(s)?;
        }
    }
    Ok(())
}
fn main() -> Result<(), String> {
    if std::env::args().any(|a| a == "--worker") {
        let shared = std::sync::Arc::new(std::sync::Mutex::new(SoundState::default()));
        let _controller = audeze_overlay::device::start(0, shared.clone());
        let start = Instant::now();
        let mut first_mix = None;
        let mut complete = None;
        while start.elapsed() < Duration::from_secs(4) {
            let s = shared.lock().unwrap().clone();
            if s.mix.is_some() && first_mix.is_none() {
                first_mix = Some(start.elapsed().as_millis());
            }
            if s.mix.is_some()
                && s.sidetone.is_some()
                && s.sidetone_level.is_some()
                && s.anc.is_some()
                && s.noise.is_some()
                && s.battery.is_some()
                && s.muted.is_some()
                && complete.is_none()
            {
                complete = Some(start.elapsed().as_millis());
            }
            sleep(Duration::from_millis(10));
        }
        let s = shared.lock().unwrap().clone();
        println!(
            "Worker first mix: {first_mix:?} ms; all fields: {complete:?} ms; final: {}",
            serde_json::to_string(&s).unwrap()
        );
        return if complete.is_some() && s.mix.is_some() {
            Ok(())
        } else {
            Err("Direct worker did not obtain a complete live snapshot".into())
        };
    }
    let mut d = Device::open()?;
    let mut s = SoundState::default();
    collect(&mut d, &mut s)?;
    println!("Direct snapshot: {}", serde_json::to_string(&s).unwrap());
    if std::env::args().any(|a| a == "--restore-sidetone-off") {
        d.set(&Command::Sidetone(false))?;
        sleep(Duration::from_millis(70));
        collect(&mut d, &mut s)?;
        println!("Sidetone restored: {:?}", s.sidetone);
        return Ok(());
    }
    if std::env::args().any(|a| a == "--controls-and-restore") {
        let before = s.clone();
        let c = before.mix.ok_or("No headset response")?.1;
        let pairs = [
            (
                Command::Mix(if c < 100 { c + 5 } else { c - 5 }),
                Command::Mix(c),
            ),
            (
                Command::Anc(
                    if before.anc.as_deref() == Some("Off") {
                        "On"
                    } else {
                        "Off"
                    }
                    .into(),
                ),
                Command::Anc(before.anc.clone().ok_or("ANC missing")?),
            ),
            (
                Command::Noise(
                    if before.noise.as_deref() == Some("Low") {
                        "Off"
                    } else {
                        "Low"
                    }
                    .into(),
                ),
                Command::Noise(before.noise.clone().ok_or("FILTER missing")?),
            ),
            (
                Command::Sidetone(!before.sidetone.ok_or("Sidetone missing")?),
                Command::Sidetone(before.sidetone.unwrap()),
            ),
            (
                Command::SidetoneLevel(
                    (before.sidetone_level.ok_or("Level missing")? + 4).min(100),
                ),
                Command::SidetoneLevel(before.sidetone_level.unwrap()),
            ),
        ];
        for (change, restore) in pairs {
            let started = Instant::now();
            let q = d.set(&change)?;
            sleep(Duration::from_millis(25));
            d.query(q)?;
            while started.elapsed() < Duration::from_millis(700) {
                sleep(Duration::from_millis(10));
                d.read(&mut s)?;
                if command_matches(&change, &s) {
                    break;
                }
            }
            let passed = command_matches(&change, &s);
            println!(
                "{change:?}: confirmed={passed}, {} ms",
                started.elapsed().as_millis()
            );
            d.set(&restore)?;
            sleep(Duration::from_millis(50));
            collect(&mut d, &mut s)?;
            if !passed || !command_matches(&restore, &s) {
                return Err(format!(
                    "Control or restoration mismatch: {change:?}, {s:?}"
                ));
            }
        }
        d.set(&Command::Sidetone(before.sidetone.unwrap()))?;
        sleep(Duration::from_millis(70));
        collect(&mut d, &mut s)?;
        assert_eq!(s.sidetone, before.sidetone);
        println!("RESTORED {}", serde_json::to_string(&s).unwrap());
    }
    Ok(())
}

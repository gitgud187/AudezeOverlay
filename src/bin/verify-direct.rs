//! Direct-device diagnostic. Without flags it only reads. `--controls-and-restore`
//! changes each supported sound setting, confirms it by readback, restores it,
//! and finally checks that the headset matches its original state. Run it only
//! with the owner present: the changes are audible.
use audeze_overlay::{
    device::command::Command,
    device::{command_matches, Device, Query, QUERIES},
    model::SoundState,
};
use std::{
    thread::sleep,
    time::{Duration, Instant},
};

/// The original Maxwell answers 110-155 ms after a query, so waits here are
/// deliberately longer than the application's 100 ms status-query timeout.
const SNAPSHOT_WINDOW: Duration = Duration::from_millis(300);
const READBACK_WINDOW: Duration = Duration::from_millis(600);

fn pump(d: &mut Device, s: &mut SoundState, window: Duration) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed() < window {
        sleep(Duration::from_millis(10));
        d.read(s)?;
    }
    Ok(())
}

/// Reads every query this model supports, including the active EQ preset. The
/// sidetone range goes first: level replies are rejected until it is known.
fn collect(d: &mut Device, s: &mut SoundState) -> Result<(), String> {
    let queries = std::iter::once(Query::MaxSidetone)
        .chain(QUERIES)
        .chain(std::iter::once(Query::EqIndex));
    for q in queries {
        if d.profile.supports_query(q) {
            d.query(q)?;
            pump(d, s, SNAPSHOT_WINDOW)?;
        }
    }
    Ok(())
}

/// Repeats `query` up to three times until `matches` holds.
/// Returns the elapsed time on success, `None` when the headset never agreed.
fn confirm(
    d: &mut Device,
    s: &mut SoundState,
    query: Query,
    matches: impl Fn(&SoundState) -> bool,
) -> Result<Option<Duration>, String> {
    let start = Instant::now();
    for _ in 0..3 {
        d.query(query)?;
        let attempt = Instant::now();
        while attempt.elapsed() < READBACK_WINDOW {
            sleep(Duration::from_millis(10));
            d.read(s)?;
            if matches(s) {
                return Ok(Some(start.elapsed()));
            }
        }
    }
    Ok(None)
}

fn apply(
    d: &mut Device,
    s: &mut SoundState,
    command: &Command,
) -> Result<Option<Duration>, String> {
    let query = d.set(command)?;
    // An EQ selection acknowledgement does not prove the active preset settled.
    let query = if query == Query::EqSelectAck {
        Query::EqIndex
    } else {
        query
    };
    sleep(Duration::from_millis(25));
    confirm(d, s, query, |s| command_matches(command, s))
}

fn report(label: &str, command: &Command, outcome: Option<Duration>) {
    println!(
        "{label} {command:?}: confirmed={}, {} ms",
        outcome.is_some(),
        outcome.map_or(0, |t| t.as_millis())
    );
}

/// Commands that put every setting the test may touch back to `before`.
/// Sidetone comes last: on the original Maxwell, changing the sidetone level
/// also switches sidetone on.
fn drifted(before: &SoundState, now: &SoundState, eq_select: bool) -> Vec<Command> {
    let mut fixes = Vec::new();
    if let Some((_, chat)) = before.mix.filter(|mix| Some(mix.1) != now.mix.map(|m| m.1)) {
        fixes.push(Command::Mix(chat));
    }
    if let Some(noise) = before
        .noise
        .clone()
        .filter(|n| Some(n) != now.noise.as_ref())
    {
        fixes.push(Command::Noise(noise));
    }
    if let Some(anc) = before.anc.clone().filter(|a| Some(a) != now.anc.as_ref()) {
        fixes.push(Command::Anc(anc));
    }
    if let Some(level) = before
        .sidetone_level
        .filter(|l| Some(*l) != now.sidetone_level)
    {
        fixes.push(Command::SidetoneLevel(level));
    }
    if let Some(index) = before
        .eq_index
        .filter(|i| eq_select && Some(*i) != now.eq_index)
    {
        fixes.push(Command::EqSelect(index));
    }
    if let Some(on) = before.sidetone.filter(|on| Some(*on) != now.sidetone) {
        fixes.push(Command::Sidetone(on));
    }
    fixes
}

/// Re-reads everything and corrects any setting that moved as a side effect.
fn settle(
    d: &mut Device,
    s: &mut SoundState,
    before: &SoundState,
    eq_select: bool,
) -> Result<(), String> {
    collect(d, s)?;
    for fix in drifted(before, s, eq_select) {
        let outcome = apply(d, s, &fix)?;
        report("side effect corrected", &fix, outcome);
    }
    Ok(())
}

fn controls_and_restore(
    d: &mut Device,
    s: &mut SoundState,
    eq_select: bool,
    original: &mut Option<SoundState>,
) -> Result<(), String> {
    let before = s.clone();
    // Saved before the first write, so a failure anywhere can still be undone.
    *original = Some(before.clone());
    let chat = before.mix.ok_or("No headset response")?.1;
    let mut steps = vec![(
        Command::Mix(if chat < 100 { chat + 5 } else { chat - 5 }),
        Command::Mix(chat),
    )];
    if d.profile.capabilities().anc {
        let anc = before.anc.clone().ok_or("ANC missing")?;
        let other = if anc == "Off" { "On" } else { "Off" };
        steps.push((Command::Anc(other.into()), Command::Anc(anc)));
    }
    let noise = before.noise.clone().ok_or("FILTER missing")?;
    let other = if noise == "Low" { "Off" } else { "Low" };
    steps.push((Command::Noise(other.into()), Command::Noise(noise)));
    let sidetone = before.sidetone.ok_or("Sidetone missing")?;
    steps.push((Command::Sidetone(!sidetone), Command::Sidetone(sidetone)));
    let level = before.sidetone_level.ok_or("Level missing")?;
    let other = if level <= 96 { level + 4 } else { level - 4 };
    steps.push((Command::SidetoneLevel(other), Command::SidetoneLevel(level)));
    if eq_select {
        let index = before.eq_index.ok_or("EQ index missing")?;
        let other = if index == 0 { 1 } else { 0 };
        steps.push((Command::EqSelect(other), Command::EqSelect(index)));
    }
    for (change, restore) in steps {
        let changed = apply(d, s, &change)?;
        report("change ", &change, changed);
        let restored = apply(d, s, &restore)?;
        report("restore", &restore, restored);
        if changed.is_none() || restored.is_none() {
            return Err(format!("Control or restoration mismatch: {change:?}"));
        }
        // The next step must start from the original state, or its readback
        // could be satisfied by a side effect of this one.
        settle(d, s, &before, eq_select)?;
    }
    collect(d, s)?;
    let same = drifted(&before, s, eq_select).is_empty();
    println!(
        "RESTORED matches_before={same} {}",
        serde_json::to_string(s).unwrap()
    );
    if same {
        Ok(())
    } else {
        Err(format!("Final state differs from the original: {before:?}"))
    }
}

/// Best effort after a failure: write back whatever differs from the original.
fn emergency_restore(d: &mut Device, s: &mut SoundState, original: &SoundState, eq_select: bool) {
    eprintln!("EMERGENCY: restoring the original settings");
    if let Err(e) = settle(d, s, original, eq_select) {
        eprintln!("EMERGENCY restore failed: {e}");
    }
}

fn main() -> Result<(), String> {
    if std::env::args().any(|a| a == "--worker") {
        let shared = std::sync::Arc::new(std::sync::Mutex::new(SoundState::default()));
        let _controller = audeze_overlay::device::start(|_| {}, shared.clone());
        let start = Instant::now();
        let mut first_mix = None;
        let mut complete = None;
        while start.elapsed() < Duration::from_secs(4) {
            let s = shared.lock().unwrap().clone();
            if s.mix.is_some() && first_mix.is_none() {
                first_mix = Some(start.elapsed().as_millis());
            }
            // ANC is only read on models that have it.
            let anc_ready = s.capabilities.is_some_and(|caps| !caps.anc) || s.anc.is_some();
            if s.mix.is_some()
                && s.sidetone.is_some()
                && s.sidetone_level.is_some()
                && anc_ready
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
    println!(
        "Direct snapshot ({:?}): {}",
        d.profile,
        serde_json::to_string(&s).unwrap()
    );
    if std::env::args().any(|a| a == "--restore-sidetone-off") {
        d.set(&Command::Sidetone(false))?;
        sleep(Duration::from_millis(70));
        collect(&mut d, &mut s)?;
        println!("Sidetone restored: {:?}", s.sidetone);
        return Ok(());
    }
    if std::env::args().any(|a| a == "--controls-and-restore") {
        let eq_select = std::env::args().any(|a| a == "--eq-select");
        let mut original = None;
        let result = controls_and_restore(&mut d, &mut s, eq_select, &mut original);
        if let (Err(_), Some(original)) = (&result, &original) {
            emergency_restore(&mut d, &mut s, original, eq_select);
        }
        result?;
    }
    Ok(())
}

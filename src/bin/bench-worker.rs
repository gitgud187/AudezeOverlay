//! Measures the real USB worker, read-only. Quit the overlay before running.
//! No commands, window, priority changes, or system timer-resolution changes.
use audeze_overlay::{device, model::SoundState};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::FILETIME,
    System::Threading::{GetCurrentProcess, GetProcessTimes},
};
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
struct Counter;
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if COUNTING.load(Relaxed) {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(l.size() as u64, Relaxed);
        }
        System.alloc(l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        if COUNTING.load(Relaxed) {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(l.size() as u64, Relaxed);
        }
        System.alloc_zeroed(l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if COUNTING.load(Relaxed) {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(n as u64, Relaxed);
        }
        System.realloc(p, l, n)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l);
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;
fn cpu_ms() -> windows::core::Result<f64> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )?;
    }
    let ticks = |t: FILETIME| ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64;
    Ok((ticks(kernel) + ticks(user)) as f64 / 10000.)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let seconds = std::env::args()
        .skip(1)
        .find_map(|a| {
            a.strip_prefix("--seconds=")
                .and_then(|s| s.parse::<u64>().ok())
        })
        .unwrap_or(30)
        .clamp(5, 300);
    let state = Arc::new(Mutex::new(SoundState::default()));
    let _controller = device::start(0, state.clone());
    let warm = Instant::now();
    loop {
        let ready = {
            let s = state.lock().unwrap();
            s.mix.is_some()
                && s.anc.is_some()
                && s.noise.is_some()
                && s.battery.is_some()
                && s.muted.is_some()
                && s.sidetone_level.is_some()
                && s.sidetone.is_some()
        };
        if ready {
            break;
        }
        if warm.elapsed() > Duration::from_secs(10) {
            return Err("No complete headset state; benchmark aborted".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_secs(2));
    let before = device::perf::snapshot();
    let cpu = cpu_ms()?;
    ALLOCS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    COUNTING.store(true, Relaxed);
    let start = Instant::now();
    std::thread::sleep(Duration::from_secs(seconds));
    COUNTING.store(false, Relaxed);
    let elapsed = start.elapsed().as_secs_f64();
    let cpu_used = cpu_ms()? - cpu;
    let after = device::perf::snapshot();
    let diff: Vec<_> = after.iter().zip(before).map(|(a, b)| a - b).collect();
    let snapshot = state.lock().unwrap().clone();
    if snapshot.mix.is_none() {
        return Err("Headset disconnected during benchmark".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"seconds":elapsed,"cpu_ms":cpu_used,"allocations":ALLOCS.load(Relaxed),"allocated_bytes":BYTES.load(Relaxed),"worker_cycles":diff[0],"queries":diff[1],"report_reads":diff[2],"empty_reads":diff[3],"zero_duration_waits":diff[4],"final_state":snapshot})
        )?
    );
    Ok(())
}

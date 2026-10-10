//! Opt-in counters for the standalone read-only benchmark. Absent in normal builds.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
pub(crate) static CYCLES: AtomicU64 = AtomicU64::new(0);
pub(crate) static QUERIES: AtomicU64 = AtomicU64::new(0);
pub(crate) static READS: AtomicU64 = AtomicU64::new(0);
pub(crate) static EMPTY: AtomicU64 = AtomicU64::new(0);
pub(crate) static ZERO_WAITS: AtomicU64 = AtomicU64::new(0);
pub fn snapshot() -> [u64; 5] {
    [&CYCLES, &QUERIES, &READS, &EMPTY, &ZERO_WAITS].map(|n| n.load(Relaxed))
}

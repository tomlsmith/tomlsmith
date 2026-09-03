//! A counting global allocator that turns the complexity contracts of
//! `docs/architecture.md` into deterministic assertions.
//!
//! Wall time varies with the host; the number and size of allocations a fixed
//! workload performs does not. [`measure`] runs a closure while counting
//! allocator calls, bytes requested, and the peak number of live bytes above
//! the level at which the measurement started, so tests can assert that
//! doubling an input at most doubles the work and that peak memory stays
//! within a budget proportional to the input.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering::Relaxed},
    },
};

static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static MEASUREMENT: Mutex<()> = Mutex::new(());

/// The process-wide allocator that records every allocation.
#[derive(Debug)]
pub struct CountingAllocator;

fn record(size: usize) {
    CALLS.fetch_add(1, Relaxed);
    BYTES.fetch_add(size, Relaxed);
    let live = LIVE.fetch_add(size, Relaxed) + size;
    PEAK.fetch_max(live, Relaxed);
}

// SAFETY: every method forwards to the system allocator with the same layout
// and only updates atomic counters around the call.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if !moved.is_null() {
            LIVE.fetch_sub(layout.size(), Relaxed);
            record(new_size);
        }
        moved
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// Allocator activity observed during one [`measure`] call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Allocations {
    /// Number of allocation and reallocation calls.
    pub calls: usize,
    /// Total bytes requested by those calls.
    pub bytes: usize,
    /// Highest number of live bytes above the level at the start of the measurement.
    pub peak_live: usize,
}

/// Runs `work` while counting its allocations.
///
/// Measurements are serialized through a process-wide lock so concurrent
/// tests cannot attribute their allocations to each other.
pub fn measure<T>(work: impl FnOnce() -> T) -> (T, Allocations) {
    let _guard = MEASUREMENT.lock().unwrap_or_else(PoisonError::into_inner);
    let base = LIVE.load(Relaxed);
    CALLS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    PEAK.store(base, Relaxed);
    let result = work();
    let allocations = Allocations {
        calls: CALLS.load(Relaxed),
        bytes: BYTES.load(Relaxed),
        peak_live: PEAK.load(Relaxed).saturating_sub(base),
    };
    (result, allocations)
}

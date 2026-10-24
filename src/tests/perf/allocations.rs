//! Optional allocation accounting, enabled only with `perf-alloc`.
//!
//! Measure it separately from elapsed time: even disabled accounting adds a
//! thread-local check to allocation hooks. Counts cover the current thread's
//! successful Rust allocator requests, not allocations made by other threads
//! or by libraries that bypass Rust's global allocator. Requested bytes are
//! cumulative request sizes, not live memory, peak memory, or process RSS.
//! Deallocations may release fixtures allocated before the measured interval.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Counts {
    pub allocs: u64,
    pub reallocs: u64,
    pub deallocs: u64,
    pub requested_bytes: u64,
}

thread_local! {
    // Const initialization and a destructor-free Cell keep the allocator hook
    // from allocating while it accesses its own bookkeeping.
    static COUNTS: Cell<Option<Counts>> = const { Cell::new(None) };
}

struct CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[inline]
fn record(update: impl FnOnce(&mut Counts)) {
    // Allocations can occur while thread-local storage is being torn down.
    // Allocator hooks must neither panic nor attempt to initialize dropped TLS.
    let _ = COUNTS.try_with(|slot| {
        if let Some(mut counts) = slot.get() {
            update(&mut counts);
            slot.set(Some(counts));
        }
    });
}

#[inline]
fn record_allocation(size: usize, reallocation: bool) {
    record(|counts| {
        if reallocation {
            counts.reallocs = counts.reallocs.saturating_add(1);
        } else {
            counts.allocs = counts.allocs.saturating_add(1);
        }
        counts.requested_bytes = counts
            .requested_bytes
            .saturating_add(u64::try_from(size).unwrap_or(u64::MAX));
    });
}

// SAFETY: Every allocation operation is forwarded to System with its original
// arguments, and returned pointers are unchanged. Bookkeeping does not allocate,
// acquire locks, or unwind. System therefore supplies the GlobalAlloc contract.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size(), false);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size(), false);
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let resized = unsafe { System.realloc(pointer, layout, new_size) };
        if !resized.is_null() {
            record_allocation(new_size, true);
        }
        resized
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        record(|counts| {
            counts.deallocs = counts.deallocs.saturating_add(1);
        });
    }
}

struct StopCounting;

impl Drop for StopCounting {
    fn drop(&mut self) {
        let _ = COUNTS.try_with(|slot| slot.set(None));
    }
}

/// Count allocator activity only while `body` runs on the calling thread.
/// Fixture construction, report formatting, and result validation belong
/// outside this interval. A panic disables accounting before propagating.
pub(super) fn measure<R>(body: impl FnOnce() -> R) -> (R, Counts) {
    COUNTS.with(|slot| {
        assert!(slot.get().is_none(), "nested allocation measurement");
    });

    COUNTS.set(Some(Counts::default()));
    let stop = StopCounting;

    let result = body();
    let counts = COUNTS.with(|slot| {
        slot.get()
            .expect("allocation measurement must remain active")
    });

    drop(stop);
    (result, counts)
}

#[cfg(test)]
mod tests {
    use super::measure;
    use std::hint::black_box;

    #[test]
    fn counts_only_the_requested_window() {
        let outside = black_box(Vec::<u8>::with_capacity(256));
        let (_, empty) = measure(|| black_box(42));
        assert_eq!(empty.allocs, 0);
        let (_, counts) = measure(|| {
            let buffer = black_box(Vec::<u8>::with_capacity(128));
            drop(black_box(buffer));
        });
        assert!(counts.allocs >= 1);
        assert!(counts.requested_bytes >= 128);
        assert!(counts.deallocs >= 1);
        drop(outside);
    }

    #[test]
    fn panic_restores_disabled_accounting() {
        let panic = std::panic::catch_unwind(|| measure(|| panic!("fixture panic")));
        assert!(panic.is_err());
        let (_, counts) = measure(|| black_box(42));
        assert_eq!(counts.allocs, 0);
        assert_eq!(counts.requested_bytes, 0);
    }
}

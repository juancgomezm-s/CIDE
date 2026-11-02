//! Measurement-only allocator and Windows process counters; never linked into the editor.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
    time::Instant,
};
pub struct Meter;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
fn add(size: usize) {
    CALLS.fetch_add(1, Relaxed);
    BYTES.fetch_add(size, Relaxed);
    PEAK.fetch_max(LIVE.fetch_add(size, Relaxed) + size, Relaxed);
}
unsafe impl GlobalAlloc for Meter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            add(layout.size());
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc_zeroed(layout);
        if !ptr.is_null() {
            add(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let next = System.realloc(ptr, layout, size);
        if !next.is_null() {
            LIVE.fetch_sub(layout.size(), Relaxed);
            add(size);
        }
        next
    }
}
pub struct Sample {
    start: Instant,
    calls: usize,
    bytes: usize,
}
impl Sample {
    pub fn start() -> Self {
        PEAK.store(LIVE.load(Relaxed), Relaxed);
        Self {
            start: Instant::now(),
            calls: CALLS.load(Relaxed),
            bytes: BYTES.load(Relaxed),
        }
    }
    pub fn finish(self) -> [u128; 8] {
        let elapsed = self.start.elapsed().as_nanos();
        let (private, working, peak_commit) = process_memory();
        [
            elapsed,
            (CALLS.load(Relaxed) - self.calls) as u128,
            (BYTES.load(Relaxed) - self.bytes) as u128,
            PEAK.load(Relaxed) as u128,
            LIVE.load(Relaxed) as u128,
            private as u128,
            working as u128,
            peak_commit as u128,
        ]
    }
}
#[cfg(windows)]
fn process_memory() -> (usize, usize, usize) {
    #[repr(C)]
    struct Counters {
        cb: u32,
        faults: u32,
        peak_working: usize,
        working: usize,
        peak_paged_pool: usize,
        paged_pool: usize,
        peak_nonpaged_pool: usize,
        nonpaged_pool: usize,
        commit: usize,
        peak_commit: usize,
        private: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
    }
    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            size: u32,
        ) -> i32;
    }
    let mut counters: Counters = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<Counters>() as u32;
    assert_ne!(
        unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) },
        0
    );
    (counters.private, counters.working, counters.peak_commit)
}
#[cfg(not(windows))]
fn process_memory() -> (usize, usize, usize) {
    (0, 0, 0)
}

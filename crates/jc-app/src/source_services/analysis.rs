//! Progress belongs to one admitted generation, never to a live editor.
use crate::Wakeup;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};

pub(crate) struct AnalysisProgress {
    done: AtomicUsize,
    total: AtomicUsize,
    files: AtomicUsize,
    wakeup: Wakeup,
    last: Mutex<Instant>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisStatus {
    pub done: usize,
    pub total: usize,
    pub files: bool,
}
impl AnalysisStatus {
    pub fn percent(self) -> usize {
        if self.total == 0 {
            0
        } else {
            self.done
                .saturating_mul(100)
                .checked_div(self.total)
                .unwrap_or(0)
                .min(100)
        }
    }
}
impl AnalysisProgress {
    pub(crate) fn new(wakeup: Wakeup) -> Self {
        Self {
            done: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            files: AtomicUsize::new(0),
            wakeup,
            last: Mutex::new(Instant::now()),
        }
    }
    pub(crate) fn report(&self, done: usize, total: usize, files: bool) {
        self.done.store(done, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
        self.files.store(usize::from(files), Ordering::Relaxed);
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if last.elapsed() >= Duration::from_millis(75) || total > 0 && done == total {
            *last = Instant::now();
            self.wakeup.signal();
        }
    }
    pub(crate) fn read(&self) -> AnalysisStatus {
        AnalysisStatus {
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed) != 0,
        }
    }
}

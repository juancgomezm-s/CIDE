use super::{Workbench, WorkbenchError};
use crate::{
    text_search::{Pattern, Scanner},
    SearchLimits, SearchMatch, SearchStatus, TaskStamp,
};
use jc_core::PagedReadPlan;
use jc_files::FileError;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[derive(Debug)]
pub(super) struct Request {
    pub(super) stamp: TaskStamp,
    pub(super) generation: u64,
    pub(super) pattern: Arc<Pattern>,
}

#[derive(Debug)]
struct Delivery {
    matches: Vec<SearchMatch>,
    total: usize,
    scanned: usize,
    status: SearchStatus,
    error: Option<String>,
}

#[derive(Debug)]
struct Flight {
    stamp: TaskStamp,
    generation: u64,
    output: Arc<Mutex<Option<Delivery>>>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Flight {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

#[derive(Debug, Default)]
pub(in crate::workbench) struct PagedSearch {
    pub(super) request: Option<Request>,
    flight: Option<Flight>,
}

impl PagedSearch {
    pub(super) fn cancel(&mut self) {
        self.request = None;
        if let Some(flight) = &self.flight {
            flight.cancel.store(true, Ordering::Release);
        }
    }
}

impl Workbench {
    pub(super) fn start_paged_search(&mut self) -> Result<(), WorkbenchError> {
        if self.paged_search.flight.is_some() {
            return Ok(());
        }
        let Some(request) = self.paged_search.request.take() else {
            return Ok(());
        };
        if self.search.target != Some(request.stamp)
            || self.search.generation != request.generation
            || !self.task_stamp_is_current(request.stamp)
        {
            return Ok(());
        }
        let started = (|| -> Result<Flight, FileError> {
            let plan = self
                .document_by_id(request.stamp.document)
                .expect("consulta vigente")
                .session
                .document()
                .prepare_paged_save()?;
            let output = Arc::new(Mutex::new(None::<Delivery>));
            let cancel = Arc::new(AtomicBool::new(false));
            let delivery = output.clone();
            let canceled = cancel.clone();
            let wakeup = self.large_open.wakeup.clone();
            let retained_budget = self.search_limits.max_results;
            let stamp = request.stamp;
            std::thread::Builder::new()
                .name("cide-busqueda-paginada".into())
                .spawn(move || {
                    let publish = |mut update: Delivery| {
                        let mut slot = delivery.lock().unwrap_or_else(|error| error.into_inner());
                        if let Some(mut previous) = slot.take() {
                            previous.matches.append(&mut update.matches);
                            update.matches = previous.matches;
                        }
                        *slot = Some(update);
                        drop(slot);
                        wakeup.signal();
                    };
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        scan(
                            plan,
                            request.pattern,
                            stamp,
                            retained_budget,
                            &canceled,
                            publish,
                        );
                    }))
                    .is_err()
                    {
                        publish(Delivery {
                            matches: Vec::new(),
                            total: 0,
                            scanned: 0,
                            status: SearchStatus::Unavailable,
                            error: Some("Falló la búsqueda en el archivo paginado".into()),
                        });
                    }
                })?;
            Ok(Flight {
                stamp: request.stamp,
                generation: request.generation,
                output,
                cancel,
            })
        })();
        match started {
            Ok(flight) => {
                self.paged_search.flight = Some(flight);
                Ok(())
            }
            Err(error) => {
                self.search.status = SearchStatus::Unavailable;
                self.search.scan_error = Some(error.to_string());
                Err(error.into())
            }
        }
    }

    pub(super) fn poll_paged_search(&mut self) -> bool {
        let Some(flight) = &self.paged_search.flight else {
            return false;
        };
        let delivery = flight
            .output
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let Some(delivery) = delivery else {
            return false;
        };
        let current = !flight.cancel.load(Ordering::Acquire)
            && self.search.target == Some(flight.stamp)
            && self.search.generation == flight.generation
            && self
                .active_document()
                .is_some_and(|doc| doc.id == flight.stamp.document)
            && self.task_stamp_is_current(flight.stamp);
        if delivery.status != SearchStatus::Running {
            self.paged_search.flight = None;
            if self.start_paged_search().is_err() {
                return true;
            }
        }
        if !current {
            return false;
        }
        self.search.status = delivery.status;
        self.search.scanned_bytes = delivery.scanned;
        self.search.total_matches = delivery.total;
        self.search.scan_error = delivery.error;
        self.search.matches.extend(delivery.matches);
        self.poll_search_navigation();
        true
    }
}

fn scan(
    plan: PagedReadPlan,
    pattern: Arc<Pattern>,
    stamp: TaskStamp,
    retained_budget: usize,
    cancel: &AtomicBool,
    publish: impl Fn(Delivery),
) {
    let len = plan.range().len();
    let work = if pattern.is_regex() {
        len.saturating_mul(16).saturating_add(256 * 1024)
    } else {
        len.max(1)
    };
    let limits = SearchLimits {
        step_bytes: 64 * 1024,
        step_time: Duration::from_millis(8),
        max_scan_bytes: work,
        max_results: usize::MAX,
        total_time: Duration::MAX,
    };
    let mut scanner = Scanner::new(pattern, stamp.encoding, len);
    let mut update = Delivery {
        matches: Vec::new(),
        total: 0,
        scanned: 0,
        status: SearchStatus::Running,
        error: None,
    };
    let mut last_report = 0;
    loop {
        if cancel.load(Ordering::Acquire) {
            update.status = SearchStatus::Cancelled;
            break;
        }
        let step = match scanner.try_step(limits, |range, bytes| {
            plan.visit_range(range, |chunk| {
                bytes.extend_from_slice(chunk);
                Ok(())
            })
        }) {
            Ok(step) => step,
            Err(error) => {
                update.status = SearchStatus::Unavailable;
                update.error = Some(format!("Falló la lectura durante la búsqueda: {error}"));
                break;
            }
        };
        let available = retained_budget.saturating_sub(update.total);
        update.total += step.matches.len();
        update
            .matches
            .extend(step.matches.into_iter().take(available));
        update.scanned = update.scanned.max(step.scanned);
        update.status = step.status;
        if step.status != SearchStatus::Running {
            break;
        }
        if update.scanned.saturating_sub(last_report) >= 4 * 1024 * 1024
            || update.matches.len() >= 128
        {
            last_report = update.scanned;
            publish(Delivery {
                matches: std::mem::take(&mut update.matches),
                total: update.total,
                scanned: update.scanned,
                status: update.status,
                error: None,
            });
        }
    }
    publish(update);
}

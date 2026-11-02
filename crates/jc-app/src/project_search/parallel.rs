//! One project executor: coordinator is scanner0, plus persistent bounded helpers.
use super::{
    scan::{self, FileReport, FileTask, Source, WalkItem, Walker},
    *,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
};

const BATCH_ROWS: usize = 32;
const BATCH_BYTES: usize = 16 * 1024;
const RESULT_BYTES: usize = 2 * 1024 * 1024;

fn staging_charge(row: &ProjectSearchRow) -> usize {
    let actual = std::mem::size_of::<ProjectSearchRow>()
        + match row {
            ProjectSearchRow::Match(hit) => hit.path.capacity() + hit.snippet.capacity(),
            ProjectSearchRow::Issue { path, message } => path.capacity() + message.capacity(),
        };
    actual.max(row.charge())
}

#[derive(Default)]
struct Stop {
    stopped: AtomicBool,
    panicked: AtomicBool,
}
impl Stop {
    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
}
enum FileEvent {
    Rows(Vec<ProjectSearchRow>),
    Done(FileReport),
    Panicked,
}
struct Work {
    request: Arc<Request>,
    pattern: Arc<crate::text_search::Pattern>,
    task: FileTask,
    stop: Arc<Stop>,
    output: SyncSender<FileEvent>,
}
#[derive(Default)]
struct Slot {
    work: Option<Work>,
    busy: bool,
    shutdown: bool,
}
struct Helper {
    shared: Arc<(Mutex<Slot>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}
impl Helper {
    fn new(index: usize) -> std::io::Result<Self> {
        let shared = Arc::new((Mutex::new(Slot::default()), Condvar::new()));
        let worker = shared.clone();
        let handle = thread::Builder::new()
            .name(format!("jc-project-file-{index}"))
            .spawn(move || {
                loop {
                    let work = {
                        let (lock, changed) = &*worker;
                        let slot = lock.lock().unwrap_or_else(|e| e.into_inner());
                        let mut slot = changed
                            .wait_while(slot, |s| s.work.is_none() && !s.shutdown)
                            .unwrap_or_else(|e| e.into_inner());
                        if slot.shutdown {
                            return;
                        }
                        slot.work.take().unwrap()
                    };
                    execute(work); // consumes every Arc<Request> before entering idle wait
                    let (lock, changed) = &*worker;
                    let mut slot = lock.lock().unwrap_or_else(|e| e.into_inner());
                    slot.busy = false;
                    drop(slot);
                    changed.notify_all();
                }
            })?;
        Ok(Self {
            shared,
            handle: Some(handle),
        })
    }
    fn submit(&self, work: Work) {
        let (lock, changed) = &*self.shared;
        let mut slot = lock.lock().unwrap_or_else(|e| e.into_inner());
        assert!(!slot.busy && slot.work.is_none());
        slot.busy = true;
        slot.work = Some(work);
        drop(slot);
        changed.notify_one();
    }
    fn wait_idle(&self) {
        let (lock, changed) = &*self.shared;
        let slot = lock.lock().unwrap_or_else(|e| e.into_inner());
        drop(
            changed
                .wait_while(slot, |s| s.busy)
                .unwrap_or_else(|e| e.into_inner()),
        );
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        self.wait_idle();
        let (lock, changed) = &*self.shared;
        lock.lock().unwrap_or_else(|e| e.into_inner()).shutdown = true;
        changed.notify_all();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
#[derive(Default)]
struct Batch {
    rows: Vec<ProjectSearchRow>,
    charge: usize,
    limited: bool,
}
impl Batch {
    fn flush(&mut self, emit: &mut impl FnMut(Vec<ProjectSearchRow>) -> bool) -> bool {
        if self.rows.is_empty() {
            return true;
        }
        self.charge = 0;
        emit(std::mem::take(&mut self.rows))
    }
    fn push(
        &mut self,
        row: ProjectSearchRow,
        emit: &mut impl FnMut(Vec<ProjectSearchRow>) -> bool,
    ) -> bool {
        let charge = staging_charge(&row);
        let projected = |batch: &Self| {
            let capacity = if batch.rows.len() == batch.rows.capacity() {
                batch.rows.capacity().saturating_mul(2).clamp(4, BATCH_ROWS)
            } else {
                batch.rows.capacity()
            };
            (
                capacity,
                std::mem::size_of::<Vec<ProjectSearchRow>>()
                    + batch.charge
                    + charge
                    + (capacity - batch.rows.len() - 1) * std::mem::size_of::<ProjectSearchRow>(),
            )
        };
        if !self.rows.is_empty() && projected(self).1 > BATCH_BYTES && !self.flush(emit) {
            return false;
        }
        let (capacity, bytes) = projected(self);
        if bytes > BATCH_BYTES {
            self.limited = true;
            return false;
        }
        if capacity > self.rows.capacity() {
            self.rows.reserve_exact(capacity - self.rows.len());
        }
        let actual = std::mem::size_of::<Vec<ProjectSearchRow>>()
            + self.charge
            + charge
            + (self.rows.capacity() - self.rows.len() - 1)
                * std::mem::size_of::<ProjectSearchRow>();
        if actual > BATCH_BYTES {
            self.limited = true;
            return false;
        }
        self.charge += charge;
        self.rows.push(row);
        if self.rows.len() >= BATCH_ROWS || self.charge >= BATCH_BYTES {
            self.flush(emit)
        } else {
            true
        }
    }
}
fn execute(work: Work) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut batch = Batch::default();
        let mut send = |rows| {
            !work.stop.stopped()
                && !work.request.cancel.is_cancelled()
                && work.output.send(FileEvent::Rows(rows)).is_ok()
        };
        let mut report = scan::file(
            &work.request,
            &work.task,
            work.pattern.clone(),
            || work.stop.stopped(),
            |row| batch.push(row, &mut send),
        );
        debug_assert!(report.charged <= work.task.reservation);
        batch.flush(&mut send);
        report.limited |= batch.limited;
        report
    }));
    match result {
        Ok(report) => {
            let _ = work.output.send(FileEvent::Done(report));
        }
        Err(_) => {
            work.stop.panicked.store(true, Ordering::Release);
            work.stop.stopped.store(true, Ordering::Release);
            let _ = work.output.send(FileEvent::Panicked);
        }
    }
    // Dropping Work releases source/request/pattern/sender before busy=false.
}
struct Wave<'a> {
    helpers: &'a [Helper],
    receivers: Vec<Receiver<FileEvent>>,
    stop: Arc<Stop>,
}
impl Drop for Wave<'_> {
    fn drop(&mut self) {
        self.stop.stopped.store(true, Ordering::Release);
        self.receivers.clear(); // unblock synchronous senders before waiting
        for helper in self.helpers {
            helper.wait_idle();
        }
    }
}
struct Collector<'a, F> {
    request: &'a Request,
    publish: F,
    batch: Batch,
    rows: usize,
    charge: usize,
    limited: bool,
    stopped: bool,
}
impl<F: FnMut(Vec<ProjectSearchRow>) -> bool> Collector<'_, F> {
    fn done(&self) -> bool {
        self.limited || self.stopped || self.request.cancel.is_cancelled()
    }
    fn row(&mut self, row: ProjectSearchRow) -> bool {
        if self.done() {
            return false;
        }
        let charge = row.charge();
        if self.rows >= self.request.options.max_results
            || charge > RESULT_BYTES.saturating_sub(self.charge)
        {
            self.limited = true;
            return false;
        }
        self.rows += 1;
        self.charge += charge;
        if !self.batch.push(row, &mut self.publish) {
            if self.batch.limited {
                self.limited = true;
            } else {
                self.stopped = true;
            }
        }
        !self.done()
    }
}

pub(super) struct Executor {
    helpers: Vec<Helper>,
}
impl Executor {
    pub fn new() -> Self {
        Self {
            helpers: Vec::new(),
        }
    }
    fn workers(&mut self, requested: usize) -> usize {
        let available = thread::available_parallelism().map_or(1, |n| n.get());
        let target = requested
            .clamp(1, 4)
            .min(available.saturating_sub(4).clamp(1, 4));
        while self.helpers.len() + 1 < target {
            match Helper::new(self.helpers.len() + 1) {
                Ok(helper) => self.helpers.push(helper),
                Err(_) => break,
            }
        }
        target.min(self.helpers.len() + 1)
    }
    pub fn run(
        &mut self,
        request: Arc<Request>,
        publish: impl FnMut(Vec<ProjectSearchRow>) -> bool,
    ) -> ProjectSearchState {
        let workers = self.workers(request.options.workers);
        self.run_with_workers(request, publish, workers)
    }
    fn run_with_workers(
        &mut self,
        request: Arc<Request>,
        publish: impl FnMut(Vec<ProjectSearchRow>) -> bool,
        workers: usize,
    ) -> ProjectSearchState {
        let pattern = crate::text_search::Pattern::compile(&request.options.query, true, false)
            .expect("validated project query");
        let mut walker = Walker::new(&request);
        let mut carry = None;
        let mut finished = false;
        let mut examined = 0usize;
        let mut collector = Collector {
            request: &request,
            publish,
            batch: Batch::default(),
            rows: 0,
            charge: 0,
            limited: false,
            stopped: false,
        };
        while !collector.done() && !finished {
            let mut files = Vec::with_capacity(workers);
            let mut held = 0usize;
            while files.len() < workers && !collector.done() && !finished {
                let Some(item) = carry.take().or_else(|| walker.next()) else {
                    finished = true;
                    break;
                };
                let remaining = request
                    .options
                    .max_scan_bytes
                    .saturating_sub(examined + held);
                let task = match item {
                    WalkItem::Issue(path, message) => {
                        if !files.is_empty() {
                            carry = Some(WalkItem::Issue(path, message));
                            break;
                        }
                        collector.row(scan::issue(&path, message));
                        continue;
                    }
                    WalkItem::Limited(notice) => {
                        if !files.is_empty() {
                            carry = Some(WalkItem::Limited(notice));
                            break;
                        }
                        if let Some((path, message)) = notice {
                            collector.row(scan::issue(&path, message));
                        }
                        collector.limited = true;
                        break;
                    }
                    WalkItem::Open(path) => {
                        let len = request.open[&path].bytes.len();
                        if len > remaining {
                            if !files.is_empty() {
                                carry = Some(WalkItem::Open(path));
                                break;
                            }
                            collector.limited = true;
                            break;
                        }
                        FileTask {
                            path,
                            source: Source::Open,
                            reservation: len,
                        }
                    }
                    WalkItem::Disk(path, length) => {
                        if remaining == 0 && length != 0 {
                            if !files.is_empty() {
                                carry = Some(WalkItem::Disk(path, length));
                                break;
                            }
                            collector.limited = true;
                            break;
                        }
                        if length > request.options.max_file_bytes as u64 {
                            if !files.is_empty() {
                                carry = Some(WalkItem::Disk(path, length));
                                break;
                            }
                            collector.row(scan::issue(
                                &path,
                                "File exceeds the search byte budget".into(),
                            ));
                            continue;
                        }
                        if length > remaining as u64 {
                            if !files.is_empty() {
                                carry = Some(WalkItem::Disk(path, length));
                                break;
                            }
                            collector.row(scan::issue(
                                &path,
                                "File exceeds the search byte budget".into(),
                            ));
                            continue;
                        }
                        let limit = length as usize;
                        FileTask {
                            path,
                            source: Source::Disk { limit },
                            reservation: limit,
                        }
                    }
                };
                held += task.reservation;
                files.push(task);
            }
            if files.is_empty() {
                continue;
            }
            let helper_count = files.len() - 1;
            let mut wave = Wave {
                helpers: &self.helpers[..helper_count],
                receivers: Vec::with_capacity(helper_count),
                stop: Arc::new(Stop::default()),
            };
            let mut files = files.into_iter();
            let first = files.next().unwrap();
            for (helper, task) in wave.helpers.iter().zip(files) {
                let (output, receiver) = mpsc::sync_channel(1);
                wave.receivers.push(receiver);
                helper.submit(Work {
                    request: request.clone(),
                    pattern: pattern.clone(),
                    task,
                    stop: wave.stop.clone(),
                    output,
                });
            }
            let report = scan::file(
                &request,
                &first,
                pattern.clone(),
                || wave.stop.stopped(),
                |row| collector.row(row),
            );
            debug_assert!(report.charged <= first.reservation);
            examined += report.charged;
            collector.limited |= report.limited;
            if !collector.batch.flush(&mut collector.publish) {
                collector.stopped = true;
            }
            if wave.stop.panicked.load(Ordering::Acquire) {
                panic!("project file helper panicked");
            }
            for receiver in &wave.receivers {
                while !collector.done() {
                    if wave.stop.panicked.load(Ordering::Acquire) {
                        panic!("project file helper panicked");
                    }
                    match receiver.recv_timeout(Duration::from_millis(10)) {
                        Ok(FileEvent::Rows(rows)) => {
                            for row in rows {
                                if !collector.row(row) {
                                    break;
                                }
                            }
                        }
                        Ok(FileEvent::Done(report)) => {
                            examined += report.charged;
                            collector.limited |= report.limited;
                            if !collector.batch.flush(&mut collector.publish) {
                                collector.stopped = true;
                            }
                            break;
                        }
                        Ok(FileEvent::Panicked) => panic!("project file helper panicked"),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => {
                            panic!("project file helper lost completion")
                        }
                    }
                }
            }
            let panic_signal = wave.stop.clone();
            drop(wave);
            if panic_signal.panicked.load(Ordering::Acquire) {
                panic!("project file helper panicked");
            }
            // Match the prior first-batch behavior: flush at each file/wave end.
            if !collector.batch.flush(&mut collector.publish) {
                collector.stopped = true;
            }
        }
        if !collector.batch.flush(&mut collector.publish) {
            collector.stopped = true;
        }
        if request.cancel.is_cancelled() || collector.stopped {
            ProjectSearchState::Cancelled
        } else if collector.limited {
            ProjectSearchState::Limited
        } else {
            ProjectSearchState::Complete
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Arc<Request> {
        let root = std::env::current_dir().unwrap();
        let mut open = BTreeMap::new();
        for index in 0..4 {
            open.insert(
                root.join(format!("cpu-parallel-{index}.txt")),
                OpenSource {
                    bytes: format!("é needle\r\n{}needle\rfin\n", "x".repeat(40000 + index))
                        .into_bytes(),
                    stamp: TaskStamp {
                        document: crate::DocumentId(index as u64 + 1),
                        revision: 1,
                        encoding: TextEncoding::Utf8,
                        view: None,
                    },
                },
            );
        }
        let mut options = ProjectSearchOptions::literal("needle");
        options.exclude = vec!["**".into()];
        // These helper-level tests admit only the owned buffers directly; they
        // never invoke Walker or create a filesystem fixture.
        Arc::new(Request {
            generation: 1,
            root,
            options,
            open,
            cancel: Cancellation::new(Duration::from_secs(10)),
        })
    }
    #[test]
    fn helper_drops_request_before_idle_and_closing_receiver_releases_backpressure() {
        let request = request();
        let weak = Arc::downgrade(&request);
        let helper = Helper::new(1).unwrap();
        let pattern = crate::text_search::Pattern::compile("x", true, false).unwrap();
        let path = request.open.keys().next().unwrap().clone();
        let reservation = request.open[&path].bytes.len();
        let (output, receiver) = mpsc::sync_channel(1);
        let stop = Arc::new(Stop::default());
        helper.submit(Work {
            request: request.clone(),
            pattern,
            task: FileTask {
                path,
                source: Source::Open,
                reservation,
            },
            stop: stop.clone(),
            output,
        });
        drop(request);
        assert!(
            matches!(receiver.recv_timeout(Duration::from_secs(2)).unwrap(), FileEvent::Rows(rows) if !rows.is_empty())
        );
        stop.stopped.store(true, Ordering::Release);
        drop(receiver);
        helper.wait_idle();
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn helper_reuses_its_thread_for_owned_open_snapshots_with_exact_positions() {
        let helper = Helper::new(1).unwrap();
        for _ in 0..2 {
            let request = request();
            let path = request.open.keys().next().unwrap().clone();
            let reservation = request.open[&path].bytes.len();
            let pattern = crate::text_search::Pattern::compile("needle", true, false).unwrap();
            let (output, receiver) = mpsc::sync_channel(1);
            helper.submit(Work {
                request,
                pattern,
                task: FileTask {
                    path,
                    source: Source::Open,
                    reservation,
                },
                stop: Arc::new(Stop::default()),
                output,
            });
            let mut ranges = Vec::new();
            loop {
                match receiver.recv_timeout(Duration::from_secs(2)).unwrap() {
                    FileEvent::Rows(rows) => {
                        assert!(rows.len() <= BATCH_ROWS);
                        assert!(
                            rows.iter().map(ProjectSearchRow::charge).sum::<usize>() <= BATCH_BYTES
                        );
                        for row in rows {
                            if let ProjectSearchRow::Match(hit) = row {
                                ranges.push((hit.range, hit.position));
                            }
                        }
                    }
                    FileEvent::Done(report) => {
                        assert!(!report.limited);
                        break;
                    }
                    FileEvent::Panicked => panic!("unexpected worker panic"),
                }
            }
            helper.wait_idle();
            assert_eq!(ranges.len(), 2);
            assert_eq!(ranges[0].0, 3..9);
            assert_eq!(ranges[0].1, jc_text::TextPosition { line: 0, column: 2 });
        }
    }
    #[test]
    fn coordinator_unwind_closes_sender_and_releases_old_snapshot_before_reuse() {
        let helper = Helper::new(1).unwrap();
        let request = request();
        let weak = Arc::downgrade(&request);
        let path = request.open.keys().next().unwrap().clone();
        let reservation = request.open[&path].bytes.len();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (output, receiver) = mpsc::sync_channel(1);
            let wave = Wave {
                helpers: std::slice::from_ref(&helper),
                receivers: vec![receiver],
                stop: Arc::new(Stop::default()),
            };
            helper.submit(Work {
                request,
                pattern: crate::text_search::Pattern::compile("x", true, false).unwrap(),
                task: FileTask {
                    path,
                    source: Source::Open,
                    reservation,
                },
                stop: wave.stop.clone(),
                output,
            });
            assert!(matches!(
                wave.receivers[0]
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap(),
                FileEvent::Rows(_)
            ));
            panic!("injected coordinator publication panic");
        }));
        assert!(result.is_err());
        assert!(weak.upgrade().is_none());
        // Idle helper survived teardown and can safely retire/join.
        helper.wait_idle();
    }
}

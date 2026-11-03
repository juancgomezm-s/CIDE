//! One cooperative capture and one disk writer, with a one-chunk bounded queue.
use super::EditorRecovery;
use crate::{CaptureChunk, CaptureError, RecoveryCapture, RecoveryKey, Wakeup, Workbench};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub struct RecoveryPolicy {
    pub idle: Duration,
    pub maximum_delay: Duration,
    pub retry: Duration,
}
impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            idle: Duration::from_secs(2),
            maximum_delay: Duration::from_secs(30),
            retry: Duration::from_secs(10),
        }
    }
}
#[derive(Default, Clone, Copy, Debug)]
pub struct PeriodicStats {
    pub confirmed: u64,
    pub canceled: u64,
    pub failed: u64,
    pub unchanged: u64,
    pub chunks: u64,
    pub max_chunk_bytes: usize,
}
struct Outcome {
    checkpoint: Result<u64, String>,
    cleanup_warning: Option<String>,
    needs_cleanup: bool,
}
struct Finished {
    recovery: EditorRecovery,
    outcome: Outcome,
}
struct Maintenance {
    completion: Receiver<Finished>,
    thread: Option<JoinHandle<()>>,
}
fn cleanup(recovery: &mut EditorRecovery, checkpoint: Result<u64, String>) -> Outcome {
    if checkpoint.is_err() {
        return Outcome {
            checkpoint,
            cleanup_warning: None,
            needs_cleanup: false,
        };
    }
    let (cleanup_warning,needs_cleanup)=match recovery.store.prune_obsolete() {
        Ok(report) if report.deferred_generations+report.conflicted_generations+report.interrupted_generations>0=>
            (Some("Hay generaciones pendientes o bloqueadas; se volverá a intentar sin reescribir documentos".into()),true),
        Ok(_)=>(None,false),Err(error)=>(Some(error.to_string()),true),
    };
    Outcome {
        checkpoint,
        cleanup_warning,
        needs_cleanup,
    }
}
struct Job {
    input: Option<SyncSender<CaptureChunk>>,
    completion: Receiver<Finished>,
    thread: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    capture: RecoveryCapture,
    pending: Option<CaptureChunk>,
    key: RecoveryKey,
    aborted: bool,
    capture_error: Option<String>,
}
pub struct PeriodicRecovery {
    policy: RecoveryPolicy,
    dirty_since: Option<Instant>,
    last_activity: Instant,
    retry_at: Option<Instant>,
    last_key: Option<RecoveryKey>,
    job: Option<Job>,
    maintenance: Option<Maintenance>,
    maintenance_due: Option<Instant>,
    stats: PeriodicStats,
}
impl Default for PeriodicRecovery {
    fn default() -> Self {
        Self::new(RecoveryPolicy::default(), Instant::now()).expect("valid recovery policy")
    }
}
impl PeriodicRecovery {
    pub fn new(policy: RecoveryPolicy, now: Instant) -> Result<Self, String> {
        if policy.idle.is_zero()
            || policy.maximum_delay < policy.idle
            || policy.maximum_delay > Duration::from_secs(3600)
            || policy.retry.is_zero()
            || policy.retry > Duration::from_secs(3600)
        {
            return Err("Los plazos de recuperación periódica son inválidos".into());
        }
        Ok(Self {
            policy,
            dirty_since: Some(now),
            last_activity: now,
            retry_at: None,
            last_key: None,
            job: None,
            maintenance: None,
            maintenance_due: None,
            stats: PeriodicStats::default(),
        })
    }
    pub fn note_activity(&mut self, now: Instant) {
        self.dirty_since.get_or_insert(now);
        self.last_activity = now;
    }
    pub fn statistics(&self) -> PeriodicStats {
        self.stats
    }
    pub fn defer_failure(&mut self, now: Instant) {
        self.stats.failed += 1;
        self.retry_at = Some(now + self.policy.retry);
        self.dirty_since.get_or_insert(now);
    }
    pub fn active(&self) -> bool {
        self.job.is_some() || self.maintenance.is_some()
    }
    fn capture_deadline(&self) -> Option<Instant> {
        if self.active() {
            return None;
        }
        let first = self.dirty_since?;
        let due = (self.last_activity + self.policy.idle).min(first + self.policy.maximum_delay);
        Some(self.retry_at.map_or(due, |retry| due.max(retry)))
    }
    pub fn deadline(&self) -> Option<Instant> {
        if self.active() {
            return None;
        }
        match (self.capture_deadline(), self.maintenance_due) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
    pub fn needs_capture(&self, now: Instant) -> bool {
        self.capture_deadline()
            .is_some_and(|deadline| now >= deadline)
    }
    /// Runs on the model owner. No filesystem operation is performed here.
    /// A frontend supplies its small immutable metadata only when needs_capture.
    pub fn poll(
        &mut self,
        recovery: &mut Option<EditorRecovery>,
        bench: &Workbench,
        frontend: &[u8],
        now: Instant,
        wakeup: &Wakeup,
    ) -> Option<String> {
        if let Some(maintenance) = &self.maintenance {
            if let Ok(finished) = maintenance.completion.try_recv() {
                let mut maintenance = self.maintenance.take().unwrap();
                if let Some(thread) = maintenance.thread.take() {
                    let _ = thread.join();
                }
                *recovery = Some(finished.recovery);
                self.maintenance_due = finished
                    .outcome
                    .needs_cleanup
                    .then_some(now + self.policy.retry);
                return finished
                    .outcome
                    .cleanup_warning
                    .map(|warning| format!("Limpieza de recuperación aplazada: {warning}"));
            }
            return None;
        }
        if let Some(job) = &self.job {
            if let Ok(finished) = job.completion.try_recv() {
                let mut job = self.job.take().unwrap();
                job.input.take();
                if let Some(thread) = job.thread.take() {
                    let _ = thread.join();
                }
                *recovery = Some(finished.recovery);
                if finished.outcome.needs_cleanup {
                    self.maintenance_due = Some(now + self.policy.retry);
                } else if finished.outcome.checkpoint.is_ok() {
                    self.maintenance_due = None;
                }
                if job.aborted {
                    if let Some(error) = job.capture_error {
                        self.defer_failure(now);
                        return Some(format!(
                            "No se pudo capturar la recuperación automática: {error}"
                        ));
                    }
                    self.stats.canceled += 1;
                    self.dirty_since = Some(now);
                    self.last_activity = now;
                    self.retry_at = None;
                } else {
                    match finished.outcome.checkpoint {
                        Ok(_) => {
                            self.stats.confirmed += 1;
                            self.last_key = Some(job.key);
                            self.retry_at = None;
                            return finished.outcome.cleanup_warning.map(|warning| {
                                format!("Recuperación confirmada; limpieza aplazada: {warning}")
                            });
                        }
                        Err(error) => {
                            self.stats.failed += 1;
                            self.dirty_since.get_or_insert(now);
                            self.retry_at = Some(now + self.policy.retry);
                            return Some(format!(
                                "No se pudo actualizar la recuperación automática: {error}"
                            ));
                        }
                    }
                }
            }
        }
        if let Some(job) = &mut self.job {
            job.input.as_ref()?;
            if job.pending.is_none() {
                match job.capture.next_chunk(bench) {
                    Ok(chunk) => job.pending = chunk,
                    Err(error) => {
                        job.aborted = true;
                        if let CaptureError::Failed(message) = error {
                            job.capture_error = Some(message);
                        }
                        job.cancel.store(true, Ordering::Release);
                        job.input.take();
                        return None;
                    }
                }
            }
            if let Some(chunk) = job.pending.take() {
                if let CaptureChunk::Data { bytes, .. } = &chunk {
                    self.stats.max_chunk_bytes = self.stats.max_chunk_bytes.max(bytes.capacity());
                }
                match job.input.as_ref().unwrap().try_send(chunk) {
                    Ok(()) => {
                        self.stats.chunks += 1;
                        wakeup.signal();
                    }
                    Err(TrySendError::Full(chunk)) => {
                        job.pending = Some(chunk);
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        job.input.take();
                    }
                }
                if let Some(CaptureChunk::Data { bytes, .. }) = &job.pending {
                    self.stats.max_chunk_bytes = self.stats.max_chunk_bytes.max(bytes.capacity());
                }
            } else {
                job.input.take();
            }
            return None;
        }
        if recovery.is_none() {
            return None;
        }
        if !self.needs_capture(now) {
            if self.maintenance_due.is_some_and(|deadline| now >= deadline) {
                return self.start_maintenance(recovery, now, wakeup);
            }
            return None;
        }
        let mut capture =
            match bench.begin_recovery_capture(&recovery.as_ref().unwrap().base, frontend) {
                Ok(capture) => capture,
                Err(error) => {
                    self.stats.failed += 1;
                    self.retry_at = Some(now + self.policy.retry);
                    return Some(format!(
                        "No se pudo preparar la recuperación automática: {error}"
                    ));
                }
            };
        if self.last_key.as_ref() == Some(capture.key()) {
            self.stats.unchanged += 1;
            self.dirty_since = None;
            self.retry_at = None;
            return None;
        }
        let key = capture.key().clone();
        let metadata = std::mem::take(&mut capture.metadata);
        let documents = capture.document_count();
        let (input, receiver) = mpsc::sync_channel(1);
        let (completed, completion) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let notify = wakeup.clone();
        let handoff = Arc::new(Mutex::new(recovery.take()));
        let worker_handoff = handoff.clone();
        let thread = thread::Builder::new()
            .name("cide-recovery".into())
            .spawn(move || {
                let mut recovery = worker_handoff
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                    .unwrap();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = recovery.store.prune_obsolete();
                    let checkpoint = recovery
                        .store
                        .checkpoint(&metadata, documents, |document, writer| loop {
                            if worker_cancel.load(Ordering::Acquire) {
                                return Err(io::Error::new(
                                    io::ErrorKind::Interrupted,
                                    "Captura de recuperación cancelada",
                                ));
                            }
                            let chunk = receiver.recv().map_err(|_| {
                                io::Error::new(
                                    io::ErrorKind::UnexpectedEof,
                                    "Captura de recuperación incompleta",
                                )
                            })?;
                            notify.signal();
                            match chunk {
                                CaptureChunk::Data {
                                    document: index,
                                    bytes,
                                } if index == document => writer.write_all(&bytes)?,
                                CaptureChunk::End { document: index } if index == document => {
                                    return Ok(())
                                }
                                _ => {
                                    return Err(io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        "Secuencia de recuperación inválida",
                                    ))
                                }
                            }
                        })
                        .map_err(|e| e.to_string());
                    cleanup(&mut recovery, checkpoint)
                }));
                let outcome = result.unwrap_or_else(|_| Outcome {
                    checkpoint: Err("El trabajador de recuperación terminó inesperadamente".into()),
                    cleanup_warning: None,
                    needs_cleanup: false,
                });
                let _ = completed.send(Finished { recovery, outcome });
                notify.signal();
            });
        match thread {
            Ok(thread) => {
                self.job = Some(Job {
                    input: Some(input),
                    completion,
                    thread: Some(thread),
                    cancel,
                    capture,
                    pending: None,
                    key,
                    aborted: false,
                    capture_error: None,
                });
                self.dirty_since = None;
                self.retry_at = None;
                wakeup.signal();
                None
            }
            Err(error) => {
                *recovery = handoff.lock().unwrap_or_else(|e| e.into_inner()).take();
                self.stats.failed += 1;
                self.retry_at = Some(now + self.policy.retry);
                Some(format!(
                    "No se pudo iniciar la recuperación automática: {error}"
                ))
            }
        }
    }
    fn start_maintenance(
        &mut self,
        recovery: &mut Option<EditorRecovery>,
        now: Instant,
        wakeup: &Wakeup,
    ) -> Option<String> {
        let handoff = Arc::new(Mutex::new(recovery.take()));
        let worker = handoff.clone();
        let notify = wakeup.clone();
        let (completed, completion) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("cide-recovery-cleanup".into())
            .spawn(move || {
                let mut recovery = worker
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                    .unwrap();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    cleanup(&mut recovery, Ok(0))
                }));
                let outcome = result.unwrap_or_else(|_| Outcome {
                    checkpoint: Ok(0),
                    cleanup_warning: Some(
                        "El trabajador de limpieza terminó inesperadamente".into(),
                    ),
                    needs_cleanup: true,
                });
                let _ = completed.send(Finished { recovery, outcome });
                notify.signal();
            });
        match thread {
            Ok(thread) => {
                self.maintenance = Some(Maintenance {
                    completion,
                    thread: Some(thread),
                });
                self.maintenance_due = None;
                None
            }
            Err(error) => {
                *recovery = handoff.lock().unwrap_or_else(|e| e.into_inner()).take();
                self.maintenance_due = Some(now + self.policy.retry);
                Some(format!(
                    "No se pudo iniciar la limpieza de recuperación: {error}"
                ))
            }
        }
    }
    /// Used by normal exit before its final synchronous checkpoint. Closing the
    /// channel wakes a blocked writer; the previous committed copy remains valid.
    pub fn cancel_and_join(&mut self, recovery: &mut Option<EditorRecovery>) {
        if let Some(mut maintenance) = self.maintenance.take() {
            if let Ok(finished) = maintenance.completion.recv() {
                *recovery = Some(finished.recovery);
            }
            if let Some(thread) = maintenance.thread.take() {
                let _ = thread.join();
            }
        }
        if let Some(mut job) = self.job.take() {
            job.cancel.store(true, Ordering::Release);
            job.input.take();
            job.pending.take();
            if let Ok(finished) = job.completion.recv() {
                *recovery = Some(finished.recovery);
            }
            if let Some(thread) = job.thread.take() {
                let _ = thread.join();
            }
            self.dirty_since = Some(Instant::now());
        }
    }
}
impl Drop for PeriodicRecovery {
    fn drop(&mut self) {
        self.cancel_and_join(&mut None);
    }
}

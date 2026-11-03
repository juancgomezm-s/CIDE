use super::{ExplorerLimits, NodeId};
use crate::{Cancellation, Wakeup};
use jc_files::DirectoryListing;
use std::{
    collections::VecDeque,
    mem::size_of,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, MutexGuard,
    },
};

pub(super) struct Request {
    pub generation: u64,
    pub node: NodeId,
    pub directory_revision: u64,
    pub root: PathBuf,
    pub relative: PathBuf,
    pub limits: ExplorerLimits,
    pub cancel: Cancellation,
}
impl Request {
    pub fn charge(&self) -> usize {
        size_of::<Self>() + self.root.capacity() + self.relative.capacity()
    }
}
pub(super) struct Active {
    pub node: NodeId,
    pub directory_revision: u64,
    pub cancel: Cancellation,
}
pub(super) struct Completion {
    pub generation: u64,
    pub node: NodeId,
    pub directory_revision: u64,
    pub root: PathBuf,
    pub result: Result<DirectoryListing, &'static str>,
    pub cancelled: bool,
}
#[derive(Default)]
pub(super) struct Queue {
    pub generation: Option<u64>,
    pub pending: VecDeque<Request>,
    pub active: Option<Active>,
    pub completion: Option<Completion>,
    pub stop: bool,
}
type Provider = dyn Fn(&Request, usize) -> std::io::Result<DirectoryListing> + Send + Sync;
pub(super) struct Shared {
    queue: Mutex<Queue>,
    pub changed: Condvar,
    pub wake: Wakeup,
    pub recovery: AtomicBool,
    provider: Arc<Provider>,
}
impl Shared {
    pub fn new(wake: Wakeup) -> Self {
        Self::with_provider(
            wake,
            Arc::new(|request, budget| {
                let path = request.root.join(&request.relative);
                jc_files::list_directory_in_root(
                    &request.root,
                    &path,
                    request.limits.max_directory_entries,
                    budget.saturating_sub(size_of::<DirectoryListing>()),
                    || request.cancel.is_cancelled(),
                )
            }),
        )
    }
    pub(super) fn with_provider(wake: Wakeup, provider: Arc<Provider>) -> Self {
        Self {
            queue: Mutex::default(),
            changed: Condvar::new(),
            wake,
            recovery: AtomicBool::new(false),
            provider,
        }
    }
    pub fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }
}
fn listing_charge(listing: &DirectoryListing) -> usize {
    size_of::<DirectoryListing>()
        + listing.entries.capacity() * size_of::<jc_files::DirectoryEntry>()
        + listing
            .entries
            .iter()
            .map(|entry| entry.name.capacity())
            .sum::<usize>()
}

pub(super) fn run(shared: Arc<Shared>) {
    loop {
        let request = {
            let mut q = shared.lock();
            while !q.stop && (q.completion.is_some() || q.pending.is_empty()) {
                q = shared.changed.wait(q).unwrap_or_else(|e| e.into_inner());
            }
            if q.stop {
                return;
            }
            let request = q.pending.pop_front().unwrap();
            q.active = Some(Active {
                node: request.node,
                directory_revision: request.directory_revision,
                cancel: request.cancel.clone(),
            });
            request
        };
        shared.wake.signal();
        let overhead = size_of::<Completion>() + request.root.capacity();
        let budget = request.limits.max_directory_bytes.saturating_sub(overhead);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (shared.provider)(&request, budget)
        }));
        let cancelled = request.cancel.is_cancelled();
        let mut result = match result {
            Ok(Ok(mut listing)) if !cancelled => {
                if listing_charge(&listing) > budget
                    || listing.entries.len() > request.limits.max_directory_entries
                {
                    Err("Directory completion exceeded metadata budget")
                } else {
                    listing.entries.sort_unstable_by(|a, b| {
                        b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name))
                    });
                    Ok(listing)
                }
            }
            Ok(_) => Err(if cancelled {
                "Directory load cancelled or timed out"
            } else {
                "Directory enumeration failed"
            }),
            Err(_) => {
                shared.recovery.store(true, Ordering::Release);
                Err("Directory worker recovered from a panic")
            }
        };
        let cancelled = request.cancel.is_cancelled();
        if cancelled {
            result = Err("Directory load cancelled or timed out");
        }
        let completion = Completion {
            generation: request.generation,
            node: request.node,
            directory_revision: request.directory_revision,
            root: request.root,
            result,
            cancelled,
        };
        let mut q = shared.lock();
        q.active = None;
        if !q.stop && q.generation == Some(completion.generation) {
            // Deadline failure is terminal for the current request. Explicit
            // cancellation/refresh also advances the owner's revision, so this
            // bounded notification cannot resurrect a cancelled subtree.
            q.completion = Some(completion);
        }
        drop(q);
        shared.changed.notify_all();
        shared.wake.signal();
    }
}

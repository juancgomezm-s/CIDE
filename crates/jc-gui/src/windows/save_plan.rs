//! A bounded sequence of save intentions, with no document or OS access.
//!
//! The caller must revalidate each captured document/revision before writing.
//! A successful write is acknowledged explicitly; merely asking for the next
//! candidate never consumes it. Dialog cancellation can therefore end the plan
//! without accidentally acknowledging a file that was not saved.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SaveCandidate {
    pub document: jc_app::DocumentId,
    pub revision: u64,
    pub new_file: bool,
}

#[derive(Debug)]
pub(super) struct SavePlan {
    candidates: Box<[SaveCandidate]>,
    saved: usize,
    ended: bool,
}

impl SavePlan {
    /// Validate the bounded metadata snapshot before retaining it. No second
    /// catalog or copy of document contents, names or paths is constructed.
    pub(super) fn new(candidates: Vec<SaveCandidate>) -> Result<Self, &'static str> {
        if candidates.len() > 1024 {
            return Err("Guardar todo excede el límite de1024 documentos");
        }
        // The bound makes this allocation-free uniqueness check inexpensive;
        // preserving the input order also preserves the caller's dialog order.
        for (index, candidate) in candidates.iter().enumerate() {
            if candidates[..index]
                .iter()
                .any(|previous| previous.document == candidate.document)
            {
                return Err("Guardar todo contiene identidades de documento duplicadas");
            }
        }
        Ok(Self {
            ended: candidates.is_empty(),
            candidates: candidates.into_boxed_slice(),
            saved: 0,
        })
    }

    pub(super) fn next(&self) -> Option<SaveCandidate> {
        if self.ended {
            None
        } else {
            self.candidates.get(self.saved).copied()
        }
    }

    /// Acknowledge exactly the pending intention, including its captured
    /// revision and new-file status. Errors leave the plan unchanged.
    pub(super) fn saved(&mut self, candidate: SaveCandidate) -> Result<(), &'static str> {
        if self.next() != Some(candidate) {
            return Err("La confirmación de Guardar todo no coincide con el candidato pendiente");
        }
        self.saved += 1;
        self.ended = self.saved == self.candidates.len();
        Ok(())
    }

    pub(super) fn cancel(&mut self) {
        self.ended = true;
    }

    pub(super) fn saved_count(&self) -> usize {
        self.saved
    }

    /// Unacknowledged candidates, including those left after cancellation.
    pub(super) fn remaining(&self) -> usize {
        self.candidates.len() - self.saved
    }
}

#[cfg(test)]
mod tests {
    use super::{SaveCandidate, SavePlan};

    fn candidate(id: u64, revision: u64, new_file: bool) -> SaveCandidate {
        SaveCandidate {
            document: jc_app::DocumentId(id),
            revision,
            new_file,
        }
    }

    #[test]
    fn acknowledgements_preserve_order_and_reject_stale_or_repeated_results() {
        let first = candidate(8, 12, true);
        let second = candidate(3, 40, false);
        let mut plan = SavePlan::new(vec![first, second]).unwrap();
        assert_eq!(plan.next(), Some(first));
        assert_eq!(plan.next(), Some(first));
        for wrong in [second, candidate(8, 13, true), candidate(8, 12, false)] {
            assert!(plan.saved(wrong).is_err());
            assert_eq!(plan.next(), Some(first));
            assert_eq!((plan.saved_count(), plan.remaining()), (0, 2));
        }
        plan.saved(first).unwrap();
        assert_eq!(plan.next(), Some(second));
        assert!(plan.saved(first).is_err());
        assert_eq!((plan.saved_count(), plan.remaining()), (1, 1));
        plan.saved(second).unwrap();
        assert_eq!(plan.next(), None);
        assert_eq!((plan.saved_count(), plan.remaining()), (2, 0));
        assert!(plan.saved(second).is_err());
    }

    #[test]
    fn cancellation_keeps_unsaved_count_and_cannot_acknowledge_pending_candidate() {
        let first = candidate(1, 0, false);
        let second = candidate(2, 1, true);
        let mut plan = SavePlan::new(vec![first, second]).unwrap();
        plan.saved(first).unwrap();
        assert_eq!(plan.next(), Some(second));
        plan.cancel();
        plan.cancel();
        assert_eq!(plan.next(), None);
        assert_eq!((plan.saved_count(), plan.remaining()), (1, 1));
        assert!(plan.saved(second).is_err());
        assert_eq!((plan.saved_count(), plan.remaining()), (1, 1));
        let mut before_first = SavePlan::new(vec![first]).unwrap();
        before_first.cancel();
        assert_eq!(before_first.next(), None);
        assert_eq!(
            (before_first.saved_count(), before_first.remaining()),
            (0, 1)
        );
    }

    #[test]
    fn construction_accepts_empty_and_limit_but_rejects_duplicate_ids_and_overflow() {
        let empty = SavePlan::new(Vec::new()).unwrap();
        assert_eq!(empty.next(), None);
        assert_eq!((empty.saved_count(), empty.remaining()), (0, 0));
        // A changed revision or path-assignment status cannot make one document
        // eligible for two writes in the same plan.
        assert!(SavePlan::new(vec![candidate(4, 1, true), candidate(4, 2, false)]).is_err());
        let mut boundary =
            SavePlan::new((0..1024).map(|id| candidate(id, u64::MAX, false)).collect()).unwrap();
        for id in 0..1024 {
            let current = candidate(id, u64::MAX, false);
            assert_eq!(boundary.next(), Some(current));
            boundary.saved(current).unwrap();
        }
        assert_eq!(boundary.next(), None);
        assert_eq!((boundary.saved_count(), boundary.remaining()), (1024, 0));
        assert!(SavePlan::new((0..1025).map(|id| candidate(id, 0, false)).collect()).is_err());
    }
}

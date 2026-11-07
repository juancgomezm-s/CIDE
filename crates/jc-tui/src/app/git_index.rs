//! Whole-file index actions stay bound to the visible local comparison.
use super::{git_history::opaque, TuiApp};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use jc_app::{
    git_index::{IndexAction, IndexState},
    git_revision::{ComparisonOrigin, LocalComparisonOrigin, PairKind, RevisionComparison},
    BottomPanel, PrimaryFocus,
};

fn action(kind: PairKind) -> IndexAction {
    match kind {
        PairKind::Worktree => IndexAction::Stage,
        PairKind::Prepared => IndexAction::Unstage,
    }
}

impl TuiApp {
    fn git_index_comparison(&self) -> Option<(&RevisionComparison, &LocalComparisonOrigin)> {
        let (kind, path) = self.git_history.local.as_ref()?;
        let comparison = self.git_revision.comparison()?;
        let ComparisonOrigin::Local(local) = &comparison.origin else {
            return None;
        };
        (self.panels.bottom_visible()
            && self.panels.bottom() == BottomPanel::Diff
            && self.git_status.selected_path_ref() == Some(path.as_slice())
            && local.kind == *kind
            && comparison.path == *path
            && comparison.can_export_patch())
        .then_some((comparison, local))
    }

    fn git_index_review_current(&self) -> bool {
        let Some((comparison, _)) = self.git_index_comparison() else {
            return false;
        };
        let Some(review) = self.git_index.review() else {
            return false;
        };
        self.project_root
            .as_deref()
            .is_some_and(|root| review.matches(root, comparison, self.git_history.hunk))
    }

    pub(super) fn handle_git_index_key(&mut self, key: KeyEvent) -> bool {
        if self.focus.primary() != PrimaryFocus::BottomPanel || key.kind != KeyEventKind::Press {
            return false;
        }
        match key.code {
            KeyCode::Char('i' | 'I' | 'k' | 'K') => {
                let block = matches!(key.code, KeyCode::Char('k' | 'K'));
                match self.git_index.state() {
                    IndexState::Preparing => {
                        self.status = "Comprobando el archivo y el índice…".into()
                    }
                    IndexState::Applying => self.status = "Publicando el cambio del índice…".into(),
                    IndexState::Reviewing => {
                        self.status = "Intro confirma la revisión pendiente · Esc descarta".into();
                    }
                    _ => {
                        let target =
                            self.project_root
                                .clone()
                                .zip(self.git_index_comparison().map(|(comparison, local)| {
                                    (comparison.path.clone(), action(local.kind))
                                }));
                        self.status = match target {
                            Some((root, (path, action))) => if block {
                                self.git_index.prepare_hunk(
                                    &self.workbench,
                                    root,
                                    self.git_revision.share_comparison().unwrap(),
                                    self.git_history.hunk,
                                )
                            } else {
                                self.git_index.prepare(&self.workbench, root, path, action)
                            }
                            .map(|_| "Comprobando la selección y el índice…".into())
                            .unwrap_or_else(|error| error.to_string()),
                            None => "Selecciona el archivo y actualiza su comparación local con F5"
                                .into(),
                        };
                    }
                }
                true
            }
            KeyCode::Enter if self.git_index.review().is_some() => {
                if self.git_index_review_current() {
                    let ticket = self.git_index.review().unwrap().ticket;
                    let root = self.project_root.as_deref().unwrap();
                    self.status = self
                        .git_index
                        .apply(&self.workbench, ticket, root)
                        .map(|()| "Publicando el cambio del índice…".into())
                        .unwrap_or_else(|error| error.to_string());
                } else {
                    let _ = self.git_index.discard_review();
                    self.status = "La comparación cambió. Actualízala antes de confirmar".into();
                }
                true
            }
            KeyCode::Esc
                if matches!(
                    self.git_index.state(),
                    IndexState::Preparing | IndexState::Reviewing
                ) =>
            {
                let _ = self.git_index.discard_review();
                self.status = "Revisión del índice descartada".into();
                true
            }
            _ => false,
        }
    }

    pub(super) fn git_index_hint(&self) -> Option<&'static str> {
        match self.git_index.state() {
            IndexState::Preparing => Some("Comprobando índice… · Esc descarta"),
            IndexState::Applying => Some("Publicando cambio del índice…"),
            IndexState::Reviewing if self.git_index_review_current() => {
                let target = &self.git_index.review()?.target;
                Some(match (target.action, target.hunk.is_some()) {
                    (IndexAction::Stage, true) => "Intro confirma preparar bloque · Esc descarta",
                    (IndexAction::Unstage, true) => "Intro confirma retirar bloque · Esc descarta",
                    (IndexAction::Stage, false) => {
                        "Intro confirma preparar archivo completo · Esc descarta"
                    }
                    (IndexAction::Unstage, false) => {
                        "Intro confirma retirar archivo completo · Esc descarta"
                    }
                })
            }
            _ => {
                let (comparison, local) = self.git_index_comparison()?;
                Some(
                    match (
                        local.kind,
                        comparison.can_select_index_hunk(self.git_history.hunk),
                    ) {
                        (PairKind::Worktree, true) => "I preparar archivo · K preparar bloque",
                        (PairKind::Prepared, true) => "I retirar archivo · K retirar bloque",
                        (PairKind::Worktree, false) => "I preparar archivo completo",
                        (PairKind::Prepared, false) => "I retirar archivo completo",
                    },
                )
            }
        }
    }

    pub(super) fn git_index_publication_notice(&self) -> Option<String> {
        let publication = self.git_index.publication()?;
        if self.project_root.as_ref() != Some(&publication.target.root) {
            return None;
        }
        let mut path = opaque(&publication.target.path, 120);
        if let Some(hunk) = publication.target.hunk {
            path.push_str(&format!(" · bloque {}", hunk + 1));
        }
        Some(match &publication.result {
            Ok(()) => match publication.target.action {
                IndexAction::Stage => format!("Preparado: {path}"),
                IndexAction::Unstage => format!("Retirado: {path}"),
            },
            Err(error) => format!("Error del índice ({path}): {error}"),
        })
    }

    pub(super) fn poll_git_index(&mut self) -> bool {
        let was_applying = self.git_index.state() == IndexState::Applying;
        let changed = self.git_index.poll();
        if self.git_index.review().is_some() && !self.git_index_review_current() {
            let _ = self.git_index.discard_review();
            self.status = "La comparación cambió. Se descartó la revisión del índice".into();
            return true;
        }
        if !changed {
            return false;
        }
        if was_applying {
            if let Some(notice) = self.git_index_publication_notice() {
                self.status = notice;
            }
            if let Some(publication) = self.git_index.publication().filter(|publication| {
                publication.result.is_ok()
                    && self.project_root.as_ref() == Some(&publication.target.root)
            }) {
                // The published write belongs to its original path, even after
                // navigation. Only invalidate the old comparison of that path.
                if self
                    .git_history
                    .local
                    .as_ref()
                    .is_some_and(|(_, path)| path == &publication.target.path)
                {
                    self.git_revision.cancel();
                }
                let _ = self
                    .git_status
                    .refresh_after_index_write(&publication.target.root);
            }
        } else if let Some(failure) = self.git_index.preparation_failure() {
            if self.project_root.as_ref() == Some(&failure.target.root)
                && self
                    .git_history
                    .local
                    .as_ref()
                    .is_some_and(|(_, path)| path == &failure.target.path)
            {
                self.status = failure.error.to_string();
            }
        } else if let Some(review) = self.git_index.review() {
            self.status = match review.target.hunk {
                Some(hunk) => format!(
                    "Revisa el bloque {}. Intro confirma · Esc descarta",
                    hunk + 1
                ),
                None => "Revisa el archivo completo. Intro confirma · Esc descarta".into(),
            };
        }
        true
    }
}

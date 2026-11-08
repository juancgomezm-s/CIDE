//! Confirm whole-file index changes from the currently displayed local diff.
use super::State;
use jc_app::{
    git_index::{IndexAction, IndexState},
    git_revision::{ComparisonOrigin, LocalComparisonOrigin, PairKind, RevisionComparison},
};

impl State {
    fn index_comparison(&self) -> Option<(&RevisionComparison, &LocalComparisonOrigin)> {
        self.git_panel?;
        let comparison = self.git_revision.comparison()?;
        let ComparisonOrigin::Local(local) = &comparison.origin else {
            return None;
        };
        (self.git_workspace.local_kind == Some(local.kind)
            && self.git_workspace.selected_path.as_deref() == Some(comparison.path.as_slice()))
        .then_some((comparison, local))
    }

    pub(super) fn git_index_button(&self, block: bool) -> Option<&'static str> {
        let (comparison, local) = self.index_comparison()?;
        if block && !comparison.can_select_index_hunk(self.git_workspace.history_hunk) {
            return None;
        }
        match self.git_index.state() {
            IndexState::Preparing | IndexState::Applying => None,
            IndexState::Reviewing => {
                let review = self.git_index.review()?;
                (review.target.hunk.is_some() == block).then_some(if block {
                    "Confirmar bloque"
                } else {
                    "Confirmar índice"
                })
            }
            _ => Some(match (local.kind, block) {
                (PairKind::Worktree, false) => "Preparar archivo",
                (PairKind::Prepared, false) => "Retirar archivo",
                (PairKind::Worktree, true) => "Preparar bloque",
                (PairKind::Prepared, true) => "Retirar bloque",
            }),
        }
    }

    pub(super) fn git_index_action(&mut self, block: bool) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        let Some((comparison, local)) = self.index_comparison() else {
            return;
        };
        let action = match local.kind {
            PairKind::Worktree => IndexAction::Stage,
            PairKind::Prepared => IndexAction::Unstage,
        };
        let visible = self.git_workspace.history_hunk;
        let result = if let Some(review) = self.git_index.review() {
            if review.target.hunk.is_some() != block {
                Err("Confirma o descarta primero la revisión pendiente".into())
            } else if !review.matches(&root, comparison, visible) {
                let _ = self.git_index.discard_review();
                Err("La comparación cambió. Actualízala antes de confirmar".into())
            } else {
                let ticket = review.ticket;
                self.git_index
                    .apply(&self.workbench, ticket, &root)
                    .map(|()| "Publicando cambio del índice…")
                    .map_err(|error| error.to_string())
            }
        } else if block {
            let snapshot = self.git_revision.share_comparison().unwrap();
            self.git_index
                .prepare_hunk(&self.workbench, root, snapshot, visible)
                .map(|_| "Comprobando el bloque y el índice…")
                .map_err(|error| error.to_string())
        } else {
            let path = comparison.path.clone();
            self.git_index
                .prepare(&self.workbench, root, path, action)
                .map(|_| "Comprobando el archivo y el índice…")
                .map_err(|error| error.to_string())
        };
        match result {
            Ok(message) => self.status = message.into(),
            Err(error) => self.error(error),
        }
        self.git_workspace.invalidate();
    }

    pub(super) fn poll_git_index(&mut self) -> bool {
        let was_applying = self.git_index.state() == IndexState::Applying;
        if !self.git_index.poll() {
            return false;
        }
        if was_applying {
            if let Some(publication) = self.git_index.publication() {
                match &publication.result {
                    Ok(()) => {
                        let same_root =
                            self.project_root.as_ref() == Some(&publication.target.root);
                        self.status = match (publication.target.action, publication.target.hunk) {
                            (IndexAction::Stage, Some(h)) => {
                                format!("Bloque {} preparado en el índice Git", h + 1)
                            }
                            (IndexAction::Unstage, Some(h)) => {
                                format!("Bloque {} retirado del índice Git", h + 1)
                            }
                            (IndexAction::Stage, None) => {
                                "Archivo preparado en el índice Git".into()
                            }
                            (IndexAction::Unstage, None) => {
                                "Archivo retirado del índice Git".into()
                            }
                        };
                        if same_root {
                            self.git_revision.cancel();
                            self.schedule_git_status();
                        }
                    }
                    Err(error) => self.error(error.to_string()),
                }
            }
        } else if let Some(failure) = self.git_index.preparation_failure() {
            self.error(failure.error.to_string());
        } else if let Some(review) = self.git_index.review() {
            if self.project_root.as_ref() == Some(&review.target.root) {
                self.status = match review.target.hunk {
                    Some(h) => format!("Revisa el bloque {} y pulsa Confirmar bloque", h + 1),
                    None => "Revisa el archivo completo y pulsa Confirmar índice".into(),
                };
            } else {
                let _ = self.git_index.discard_review();
            }
        }
        self.git_workspace.invalidate();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{windows::git_panel::GitIntent, GuiOptions};
    use jc_app::{git_revision::GitRevisionState, git_status_tasks::GitStatusTaskState};
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    struct Fixture {
        base: PathBuf,
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .canonicalize()
                .unwrap();
            let base = workspace.join("work/test-temp");
            fs::create_dir_all(&base).unwrap();
            let base = base.canonicalize().unwrap();
            assert!(base.starts_with(&workspace));
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = base.join(format!("gui-git-index-{}-{stamp}", std::process::id()));
            fs::create_dir(&root).unwrap();
            fs::create_dir_all(root.join(".git/objects")).unwrap();
            fs::create_dir_all(root.join(".git/refs/heads")).unwrap();
            fs::write(root.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();
            fs::write(
                root.join(".git/config"),
                b"[core]\nrepositoryformatversion=0\nfilemode=false\nbare=false\n",
            )
            .unwrap();
            Self { root, base }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if self
                .root
                .canonicalize()
                .is_ok_and(|root| root == self.root && root.parent() == Some(self.base.as_path()))
            {
                let _ = fs::remove_dir_all(&self.root);
            }
        }
    }

    fn wait(state: &mut State, ready: impl Fn(&State) -> bool) {
        let until = Instant::now() + Duration::from_secs(5);
        while !ready(state) {
            assert!(Instant::now() < until, "{}", state.status);
            state.poll_git();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn gui_create_commit_reviews_staged_paths_before_advancing_branch() {
        let fixture = Fixture::new();
        let path = fixture.root.join("new.txt");
        fs::write(&path, b"preparado\n").unwrap();
        let config = fixture.root.join(".git/config");
        let mut content = fs::read(&config).unwrap();
        content.extend_from_slice(b"\n[user]\nname = CIDE\nemail = cide@example.test\n");
        fs::write(config, content).unwrap();
        let mut state = State::new(&GuiOptions {
            paths: vec![path],
            project_root: Some(fixture.root.clone()),
            encoding: Some(jc_text::TextEncoding::Utf8),
            ..Default::default()
        })
        .unwrap();
        state.request_git(GitIntent::Status);
        wait(&mut state, |s| {
            s.git_status.state() == GitStatusTaskState::Ready
        });
        state.request_git_local(PairKind::Worktree);
        wait(&mut state, |s| {
            s.git_revision.state() == GitRevisionState::Ready
        });
        state.git_index_action(false);
        wait(&mut state, |s| s.git_index.state() == IndexState::Reviewing);
        state.git_index_action(false);
        wait(&mut state, |s| s.git_index.state() == IndexState::Published);
        use jc_app::Modifiers;
        state.key_override = Some(Modifiers::CTRL.union(Modifiers::SHIFT));
        state.key(b'M' as usize);
        state.key_override = Some(Modifiers::NONE);
        for unit in "Commit desde GUI".encode_utf16() {
            state.character(unit);
        }
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        let plan = view.review().unwrap_or_else(|| panic!("{:?}", view.error));
        assert_eq!(plan.files()[0].path, b"new.txt");
        let id = plan.id();
        let branch = fixture.root.join(".git/refs/heads/main");
        state.key(0x0d);
        assert!(!branch.exists());
        state.key_override = Some(Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().is_none(), "{}", state.status);
        assert_eq!(fs::read(&branch).unwrap(), format!("{id}\n").as_bytes());
        wait(&mut state, |s| {
            s.git_status
                .result()
                .and_then(|r| r.info())
                .and_then(|info| info.head.as_deref())
                == Some(id.to_string().as_str())
        });
        state.key_override = Some(Modifiers::CTRL.union(Modifiers::SHIFT));
        state.key(b'A' as usize);
        state.key_override = Some(Modifiers::NONE);
        for unit in "Mensaje corregido".encode_utf16() {
            state.character(unit);
        }
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let plan = state.git_commit.view().unwrap().review().unwrap();
        assert_eq!(plan.replaced(), Some(id));
        assert!(plan.files().is_empty());
        let replacement = plan.id();
        state.key(0x0d);
        assert_eq!(fs::read(&branch).unwrap(), format!("{id}\n").as_bytes());
        state.key_override = Some(Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert_eq!(
            fs::read(branch).unwrap(),
            format!("{replacement}\n").as_bytes()
        );

        state.key_override = Some(Modifiers::NONE);
        state.key(b'G' as usize);
        wait(&mut state, |s| s.git_revision.graph().is_some());
        assert_eq!(
            state.git_revision.graph().unwrap().entries[0].commit.id,
            replacement.to_string()
        );
        state.key(0x0d);
        wait(&mut state, |s| s.git_revision.revision().is_some());
        assert_eq!(
            state.git_revision.revision().unwrap().commit,
            replacement.to_string()
        );
        state.key(0x1b);
        assert!(state.git_workspace.graph.as_ref().unwrap().showing);
        assert!(state.git_revision.graph().is_some());
    }

    #[test]
    fn gui_git_index_reviews_the_visible_comparison_then_publishes() {
        let fixture = Fixture::new();
        let path = fixture.root.join("new.txt");
        let original: String = (0..30).map(|i| format!("línea {i:02}\r\n")).collect();
        fs::write(&path, &original).unwrap();
        let mut state = State::new(&GuiOptions {
            paths: vec![path.clone()],
            project_root: Some(fixture.root.clone()),
            encoding: Some(jc_text::TextEncoding::Utf8),
            ..Default::default()
        })
        .unwrap();
        state.request_git(GitIntent::Status);
        wait(&mut state, |state| {
            state.git_status.state() == GitStatusTaskState::Ready
        });
        state.request_git_local(PairKind::Worktree);
        wait(&mut state, |state| {
            state.git_revision.state() == GitRevisionState::Ready
        });
        use jc_app::Modifiers;
        state.key_override = Some(Modifiers::CTRL.union(Modifiers::ALT));
        assert!(matches!(
            state.key(b'E' as usize).1,
            crate::windows::Action::ExportGitPatch(_)
        ));
        state.key_override = Some(Modifiers::CTRL.union(Modifiers::SHIFT));
        assert_eq!(state.git_index_button(false), Some("Preparar archivo"));
        state.key(b'I' as usize);
        wait(&mut state, |state| {
            state.git_index.state() == IndexState::Reviewing
        });
        assert_eq!(state.git_index_button(false), Some("Confirmar índice"));
        assert!(!fixture.root.join(".git/index").exists());
        state.key(b'I' as usize);
        wait(&mut state, |state| {
            state.git_index.state() == IndexState::Published
        });
        assert!(fixture.root.join(".git/index").is_file());
        assert_eq!(fs::read(&path).unwrap(), original.as_bytes());

        let first_only = original.replace("línea 03", "primer cambio");
        let worktree = first_only.replace("línea 25", "segundo cambio");
        fs::write(&path, &worktree).unwrap();
        let index = fixture.root.join(".git/index");
        let before = fs::read(&index).unwrap();
        state.request_git(GitIntent::Status);
        wait(&mut state, |state| {
            state.git_status.state() == GitStatusTaskState::Ready
        });
        state.request_git_local(PairKind::Worktree);
        wait(&mut state, |state| {
            state.git_revision.state() == GitRevisionState::Ready
        });
        assert_eq!(
            state
                .git_revision
                .comparison()
                .unwrap()
                .model
                .hunks()
                .count(),
            2
        );
        state.key_override = Some(Modifiers::CTRL.union(Modifiers::ALT));
        assert_eq!(state.git_index_button(true), Some("Preparar bloque"));
        state.key(b'B' as usize);
        wait(&mut state, |state| {
            state.git_index.state() == IndexState::Reviewing
        });
        let review = state.git_index.review().unwrap();
        assert_eq!(review.target.hunk, Some(0));
        assert_eq!(
            review.plan().partial_index_bytes(),
            Some(first_only.as_bytes())
        );
        state.key(b'B' as usize);
        assert_eq!(fs::read(&index).unwrap(), before);
        state.key_override = Some(Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |state| {
            state.git_index.state() == IndexState::Published
        });
        assert_ne!(fs::read(index).unwrap(), before);
        assert_eq!(fs::read(path).unwrap(), worktree.as_bytes());
        wait(&mut state, |state| {
            state.git_status.state() == GitStatusTaskState::Ready
        });
        state.request_git_local(PairKind::Worktree);
        wait(&mut state, |state| {
            state.git_revision.state() == GitRevisionState::Ready
        });
        let comparison = state.git_revision.comparison().unwrap();
        assert_eq!(comparison.model.hunks().count(), 1);
        assert_eq!(
            comparison
                .model
                .source_bytes(jc_app::diff_model::DiffSide::Left),
            first_only.as_bytes()
        );
    }
}

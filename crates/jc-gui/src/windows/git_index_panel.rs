//! Confirm whole-file index changes from the currently displayed local diff.
use super::State;
use jc_app::{
    git_commit::restore_label,
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

    pub(super) fn git_restore_label(&self) -> Option<&'static str> {
        let ready = self.git_revision.state() == jc_app::git_revision::GitRevisionState::Ready
            && !matches!(
                self.git_index.state(),
                IndexState::Preparing | IndexState::Reviewing | IndexState::Applying
            );
        let (comparison, _) = self.index_comparison()?;
        ready.then(|| restore_label(comparison)).flatten()
    }

    pub(super) fn open_git_restore(&mut self) {
        if self.git_restore_label().is_some() {
            if let Some((root, comparison)) = self
                .project_root
                .clone()
                .zip(self.git_revision.share_comparison())
            {
                self.git_commit.open_restore(root, comparison);
                self.git_workspace.invalidate();
            }
        }
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
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    struct Fixture {
        base: PathBuf,
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
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
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let root = base.join(format!(
                "gui-git-index-{}-{stamp}-{serial}",
                std::process::id()
            ));
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
    fn gui_graph_selects_tag_and_merge_parent_for_files_and_diff() {
        let fixture = Fixture::new();
        let git = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|directory| directory.join("git.exe"))
            .find(|path| path.is_file())
            .expect("Git is required for the owned fixture");
        let run = |args: &[&str]| {
            let output = std::process::Command::new(&git)
                .current_dir(&fixture.root)
                .env_clear()
                .envs(std::env::vars_os().filter(|(key, _)| {
                    matches!(
                        key.to_str(),
                        Some("SystemRoot" | "SYSTEMROOT" | "WINDIR" | "TEMP" | "TMP")
                    )
                }))
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "NUL")
                .env("GIT_AUTHOR_NAME", "CIDE Fixture")
                .env("GIT_AUTHOR_EMAIL", "cide@example.test")
                .env("GIT_COMMITTER_NAME", "CIDE Fixture")
                .env("GIT_COMMITTER_EMAIL", "cide@example.test")
                .args(["-c", "core.hooksPath=NUL", "-c", "core.autocrlf=false"])
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{:?}", output.stderr);
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        let source = fixture.root.join("merge.txt");
        fs::write(&source, b"primer padre\n").unwrap();
        run(&["add", "--", "merge.txt"]);
        let a = run(&["write-tree"]);
        let first = run(&["commit-tree", &a, "-m", "Primero"]);
        fs::write(&source, b"segundo padre\n").unwrap();
        run(&["add", "--", "merge.txt"]);
        let b = run(&["write-tree"]);
        let second = run(&["commit-tree", &b, "-m", "Segundo"]);
        let merge = run(&[
            "commit-tree",
            &a,
            "-p",
            &first,
            "-p",
            &second,
            "-m",
            "Merge",
        ]);
        run(&["update-ref", "refs/heads/main", &merge]);
        run(&["update-ref", "refs/heads/lateral", &second]);
        run(&["tag", "prueba", &first]);
        let index = fs::read(fixture.root.join(".git/index")).unwrap();
        let mut state = State::new(&GuiOptions {
            project_root: Some(fixture.root.clone()),
            ..Default::default()
        })
        .unwrap();
        state.request_git(GitIntent::Status);
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(b'G' as usize);
        wait(&mut state, |s| s.git_revision.graph().is_some());
        state.key(b'R' as usize);
        state.key(0x23);
        state.key(0x0d);
        wait(&mut state, |s| s.git_revision.graph().is_some());
        assert_eq!(
            state.git_revision.graph().unwrap().entries[0].commit.id,
            first
        );
        state.key(b'R' as usize);
        state.key(0x0d);
        wait(&mut state, |s| s.git_revision.graph().is_some());
        state.key(0x0d);
        wait(&mut state, |s| s.git_revision.revision().is_some());
        assert_eq!(state.git_revision.revision().unwrap().commit, merge);
        assert!(state.git_revision.revision().unwrap().files.is_empty());
        state.key(b'P' as usize);
        wait(&mut state, |s| s.git_revision.revision().is_some());
        assert_eq!(
            state.git_revision.revision().unwrap().parent.as_deref(),
            Some(second.as_str())
        );
        state.key(0x0d);
        wait(&mut state, |s| s.git_revision.comparison().is_some());
        let pair = state.git_revision.comparison().unwrap();
        assert_eq!(
            pair.model.source_bytes(jc_app::diff_model::DiffSide::Left),
            b"segundo padre\n"
        );
        assert_eq!(
            pair.model.source_bytes(jc_app::diff_model::DiffSide::Right),
            b"primer padre\n"
        );
        state.key(0x1b);
        state.key(b'B' as usize);
        for unit in "desde-grafo".encode_utf16() {
            state.character(unit);
        }
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        assert!(view.reference_review().is_some(), "{:?}", view.error);
        let new_branch = fixture.root.join(".git/refs/heads/desde-grafo");
        assert!(!new_branch.exists());
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert_eq!(fs::read_to_string(&new_branch).unwrap().trim(), merge);
        wait(&mut state, |s| s.git_revision.graph().is_some());
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(b'R' as usize);
        let graph = state.git_revision.graph().unwrap();
        let row = (1..graph.reference_count())
            .find(|&row| graph.reference_label(row) == Some("Rama desde-grafo"))
            .unwrap();
        for _ in 0..row {
            state.key(0x28);
        }
        state.key(0x71);
        for unit in "rama-renombrada".encode_utf16() {
            state.character(unit);
        }
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        assert!(view.reference_change_review().is_some(), "{:?}", view.error);
        let renamed = fixture.root.join(".git/refs/heads/rama-renombrada");
        assert!(new_branch.exists() && !renamed.exists());
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(!new_branch.exists(), "{}", state.status);
        assert_eq!(fs::read_to_string(renamed).unwrap().trim(), merge);
        assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
        let head = fixture.root.join(".git/HEAD");
        run(&["config", "--local", "user.name", "CIDE Fixture"]);
        run(&["config", "--local", "user.email", "cide@example.test"]);
        let old_head = fs::read(&head).unwrap();
        wait(&mut state, |s| s.git_revision.graph().is_some());
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(b'R' as usize);
        let graph = state.git_revision.graph().unwrap();
        let row = (1..graph.reference_count())
            .find(|&row| graph.reference_label(row) == Some("Rama rama-renombrada"))
            .unwrap();
        for _ in 0..row {
            state.key(0x28);
        }
        state.key(b'C' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        let review = view
            .branch_switch_review()
            .unwrap_or_else(|| panic!("{:?}", view.error));
        assert_eq!(review.destination(), "rama-renombrada");
        assert_eq!(fs::read(&head).unwrap(), old_head);
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert_eq!(
            fs::read(&head).unwrap(),
            b"ref: refs/heads/rama-renombrada\n",
            "{}",
            state.status
        );
        assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
        assert_eq!(fs::read(&source).unwrap(), b"segundo padre\n");

        run(&["read-tree", &merge]);
        fs::write(&source, b"primer padre\n").unwrap();
        wait(&mut state, |s| s.git_revision.graph().is_some());
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(b'R' as usize);
        let graph = state.git_revision.graph().unwrap();
        let row = (1..graph.reference_count())
            .find(|&row| graph.reference_label(row) == Some("Rama lateral"))
            .unwrap();
        for _ in 0..row {
            state.key(0x28);
        }
        state.key(b'C' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        let review = view
            .branch_switch_review()
            .unwrap_or_else(|| panic!("{:?}", view.error));
        assert_eq!(review.files()[0].path(), b"merge.txt");
        assert_eq!(fs::read(&source).unwrap(), b"primer padre\n");
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert_eq!(
            fs::read(&head).unwrap(),
            b"ref: refs/heads/lateral\n",
            "{}",
            state.status
        );
        assert_eq!(fs::read(&source).unwrap(), b"segundo padre\n");

        wait(&mut state, |s| s.git_revision.graph().is_some());
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(b'R' as usize);
        let graph = state.git_revision.graph().unwrap();
        let row = (1..graph.reference_count())
            .find(|&row| graph.reference_label(row) == Some("Rama main"))
            .unwrap();
        for _ in 0..row {
            state.key(0x28);
        }
        state.key(b'M' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        let review = view
            .merge_review()
            .unwrap_or_else(|| panic!("{:?}", view.error));
        assert_eq!(review.result().unwrap().to_string(), merge);
        assert_eq!(review.rows[0].path, b"merge.txt");
        assert_eq!(fs::read(&source).unwrap(), b"segundo padre\n");
        state.key(0x0d);
        assert!(!state.git_commit.busy(), "Intro solo no puede confirmar");
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert_eq!(
            fs::read(&source).unwrap(),
            b"primer padre\n",
            "{}",
            state.status
        );
        assert_eq!(fs::read(&head).unwrap(), b"ref: refs/heads/lateral\n");
        assert_eq!(run(&["rev-parse", "HEAD"]), merge);

        fs::write(&source, b"entrante\n").unwrap();
        run(&["add", "--", "merge.txt"]);
        let tree = run(&["write-tree"]);
        let incoming = run(&["commit-tree", &tree, "-p", &second, "-m", "Conflicto"]);
        run(&["update-ref", "refs/heads/conflicto", &incoming]);
        run(&["read-tree", &merge]);
        fs::write(&source, b"primer padre\n").unwrap();
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(0x74);
        wait(&mut state, |s| {
            s.git_revision.state() == GitRevisionState::Ready
        });
        state.key(b'R' as usize);
        let graph = state.git_revision.graph().unwrap();
        let row = (1..graph.reference_count())
            .find(|&row| graph.reference_label(row) == Some("Rama conflicto"))
            .unwrap();
        for _ in 0..row {
            state.key(0x28);
        }
        state.key(b'M' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().unwrap().merge_conflicts().is_some());
        state.key(0x0d);
        state.key(0x09);
        state.key(0x09);
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(b'U' as usize);
        state.key_override = Some(jc_app::Modifiers::NONE);
        for unit in "GUI ".encode_utf16() {
            state.character(unit);
        }
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        assert!(state.git_commit.view().unwrap().all_merge_choices_saved());
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let preview = state.git_commit.view().unwrap();
        assert!(preview.can_return_to_merge_choices(), "{:?}", preview.error);
        assert_eq!(run(&["rev-parse", "HEAD"]), merge);
        assert_eq!(fs::read(&source).unwrap(), b"primer padre\n");
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().is_none(), "{}", state.status);
        assert_eq!(fs::read(source).unwrap(), b"GUI entrante\n");
        let resolved = run(&["rev-parse", "HEAD"]);
        assert_eq!(
            run(&["rev-list", "--parents", "-n", "1", "HEAD"]),
            format!("{resolved} {merge} {incoming}")
        );

        let shape = fixture.root.join("forma");
        let child = shape.join("sub/hoja.txt");
        fs::create_dir_all(child.parent().unwrap()).unwrap();
        fs::write(&child, b"carpeta entrante\n").unwrap();
        run(&["add", "--", "forma"]);
        let tree = run(&["write-tree"]);
        let folder = run(&["commit-tree", &tree, "-p", &resolved, "-m", "Carpeta"]);
        run(&["update-ref", "refs/heads/carpeta", &folder]);
        fs::remove_file(&child).unwrap();
        fs::remove_dir(child.parent().unwrap()).unwrap();
        fs::remove_dir(&shape).unwrap();
        run(&["read-tree", &resolved]);
        fs::write(&shape, b"archivo actual\n").unwrap();
        run(&["add", "--", "forma"]);
        let tree = run(&["write-tree"]);
        let file = run(&["commit-tree", &tree, "-p", &resolved, "-m", "Archivo"]);
        run(&["update-ref", "HEAD", &file]);
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(0x74);
        wait(&mut state, |s| {
            s.git_revision.state() == GitRevisionState::Ready
        });
        state.key(b'R' as usize);
        let graph = state.git_revision.graph().unwrap();
        let row = (1..graph.reference_count())
            .find(|&row| graph.reference_label(row) == Some("Rama carpeta"))
            .unwrap();
        for _ in 0..row {
            state.key(0x28);
        }
        state.key(b'M' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        state.key(0x0d);
        state.key(0x09);
        state.key(0x09);
        let view = state.git_commit.view().unwrap();
        let text = view
            .merge_file()
            .unwrap()
            .present(view.merge_conflicts().unwrap(), 100, 20);
        assert!(text.note.starts_with("Carpeta"));
        assert_eq!(text.rows[0].1, "forma/sub/hoja.txt");
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(b'U' as usize);
        state.key(0x0d);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(
            state
                .git_commit
                .view()
                .unwrap()
                .can_return_to_merge_choices(),
            "{}",
            state.status
        );
        assert_eq!(fs::read(&shape).unwrap(), b"archivo actual\n");
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().is_none(), "{}", state.status);
        assert_eq!(fs::read(child).unwrap(), b"carpeta entrante\n");

        let previous = run(&["rev-parse", "HEAD"]);
        let actual_path = fixture.root.join("nombre-actual.txt");
        let incoming_path = fixture.root.join("nombre-entrante.txt");
        fs::write(&actual_path, b"mismo contenido\n").unwrap();
        let blob = run(&["hash-object", "-w", "--no-filters", "nombre-actual.txt"]);
        let version = |path: &str, parent: &str| {
            run(&["read-tree", &previous]);
            run(&[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{blob},{path}"),
            ]);
            let tree = run(&["write-tree"]);
            run(&["commit-tree", &tree, "-p", parent, "-m", path])
        };
        let base = version("nombre-base.txt", &previous);
        let incoming = version("nombre-entrante.txt", &base);
        let actual = version("nombre-actual.txt", &base);
        run(&["update-ref", "refs/heads/renombre", &incoming]);
        run(&["update-ref", "HEAD", &actual]);
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(0x74);
        wait(&mut state, |s| {
            s.git_revision.state() == GitRevisionState::Ready
        });
        state.key(b'R' as usize);
        let graph = state.git_revision.graph().unwrap();
        let row = (1..graph.reference_count())
            .find(|&row| graph.reference_label(row) == Some("Rama renombre"))
            .unwrap();
        for _ in 0..row {
            state.key(0x28);
        }
        state.key(b'M' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        state.key(0x0d);
        for name in [
            "nombre-base.txt",
            "nombre-actual.txt",
            "nombre-entrante.txt",
        ] {
            let view = state.git_commit.view().unwrap();
            let text = view
                .merge_file()
                .unwrap()
                .present(view.merge_conflicts().unwrap(), 100, 20);
            assert_eq!(text.rows[0].1, name);
            if name != "nombre-entrante.txt" {
                state.key(0x09);
            }
        }
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(b'U' as usize);
        state.key(0x0d);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state
            .git_commit
            .view()
            .unwrap()
            .can_return_to_merge_choices());
        assert!(!incoming_path.exists());
        assert!(actual_path.exists());
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().is_none(), "{}", state.status);
        assert_eq!(fs::read(&incoming_path).unwrap(), b"mismo contenido\n");
        assert!(!actual_path.exists());

        use std::os::windows::fs::OpenOptionsExt;
        let original = run(&["rev-parse", "HEAD"]);
        let index = fixture.root.join(".git/index");
        let added = fixture.root.join("aborto-nuevo.txt");
        let pending = fixture.root.join(".git/cide-switch.pending");
        fs::write(&added, b"archivo de la fusion\n").unwrap();
        fs::write(&incoming_path, b"cambio entrante\n").unwrap();
        run(&["add", "--", "aborto-nuevo.txt", "nombre-entrante.txt"]);
        let tree = run(&["write-tree"]);
        let incoming = run(&["commit-tree", &tree, "-p", &original, "-m", "Abortar"]);
        run(&["update-ref", "refs/heads/aborto", &incoming]);
        run(&["read-tree", &original]);
        fs::write(&incoming_path, b"mismo contenido\n").unwrap();
        fs::remove_file(&added).unwrap();
        let before = fs::read(&index).unwrap();
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(0x74);
        wait(&mut state, |s| {
            s.git_revision.state() == GitRevisionState::Ready
        });
        state.key(b'R' as usize);
        let graph = state.git_revision.graph().unwrap();
        let row = (1..graph.reference_count())
            .find(|&row| graph.reference_label(row) == Some("Rama aborto"))
            .unwrap();
        for _ in 0..row {
            state.key(0x28);
        }
        state.key(b'M' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().unwrap().merge_review().is_some());
        let block = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&incoming_path)
            .unwrap();
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        drop(block);
        assert!(pending.exists());
        assert!(added.exists(), "{}", state.status);
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(0x1b);
        state.key(b'A' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        let review = view
            .merge_abort_review()
            .unwrap_or_else(|| panic!("{:?}", view.error));
        assert_eq!(review.rows[0].path, b"aborto-nuevo.txt");
        assert!(added.exists());
        state.key(0x0d);
        assert!(!state.git_commit.busy());
        assert!(added.exists());
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().is_none(), "{}", state.status);
        assert!(!pending.exists() && !added.exists());
        assert_eq!(fs::read(&index).unwrap(), before);
        assert_eq!(fs::read(&incoming_path).unwrap(), b"mismo contenido\n");
        assert_eq!(run(&["rev-parse", "HEAD"]), original);

        let tree = run(&["rev-parse", &format!("{incoming}^{{tree}}")]);
        let merge_pick = run(&[
            "commit-tree",
            &tree,
            "-p",
            &incoming,
            "-p",
            &original,
            "-m",
            "Fusión para cherry-pick",
        ]);
        run(&["update-ref", "refs/heads/aborto", &merge_pick]);
        let pick = |state: &mut State| {
            state.key_override = Some(jc_app::Modifiers::NONE);
            state.key(0x74);
            wait(state, |s| s.git_revision.state() == GitRevisionState::Ready);
            let graph = state.git_revision.graph().unwrap();
            let row = (1..graph.reference_count())
                .find(|&row| graph.reference_label(row) == Some("Rama aborto"))
                .unwrap();
            state.key(b'R' as usize);
            for _ in 0..row {
                state.key(0x28);
            }
            state.key(0x0d);
            wait(state, |s| s.git_revision.state() == GitRevisionState::Ready);
            assert_eq!(
                state.git_revision.graph().unwrap().entries[0].commit.id,
                merge_pick
            );
            state.key(b'P' as usize);
            state.key(0x0d);
            wait(state, |s| !s.git_commit.busy());
            let view = state.git_commit.view().unwrap();
            let plan = view
                .choosing_mainline()
                .unwrap_or_else(|| panic!("{:?}", view.error));
            assert_eq!(
                plan.mainline_choices()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                [incoming.clone(), original.clone()]
            );
            state.key(0x0d);
            wait(state, |s| !s.git_commit.busy());
            let plan = state.git_commit.view().unwrap().merge_review().unwrap();
            assert_eq!(plan.mainline().unwrap().0, 1);
            assert!(!plan.can_apply() && plan.rows.is_empty());
            state.key(0x1b);
            assert!(state
                .git_commit
                .view()
                .unwrap()
                .choosing_mainline()
                .is_some());
            state.key(0x28);
            state.key(0x0d);
            wait(state, |s| !s.git_commit.busy());
        };

        fs::write(&incoming_path, b"otro cambio activo\n").unwrap();
        run(&["add", "--", "nombre-entrante.txt"]);
        let tree = run(&["write-tree"]);
        let divergent = run(&["commit-tree", &tree, "-p", &original, "-m", "Divergente"]);
        run(&["update-ref", "HEAD", &divergent]);
        let conflict_index = fs::read(&index).unwrap();
        pick(&mut state);
        let view = state.git_commit.view().unwrap();
        assert!(view.merge_conflicts().unwrap().is_cherry_pick());
        state.key(0x0d);
        state.key(0x09);
        state.key(0x09);
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(b'U' as usize);
        state.key(0x0d);
        assert!(state.git_commit.view().unwrap().all_merge_choices_saved());
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let review = state.git_commit.view().unwrap();
        assert!(review.can_return_to_merge_choices(), "{:?}", review.error);
        assert_eq!(review.merge_review().unwrap().mainline().unwrap().0, 2);
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(0x1b);
        assert!(state.git_commit.view().unwrap().merge_conflicts().is_some());
        state.key(0x1b);
        assert!(state
            .git_commit
            .view()
            .unwrap()
            .choosing_mainline()
            .is_some());
        state.key(0x1b);
        assert!(state.git_commit.view().is_none());
        assert!(!pending.exists() && !added.exists());
        assert_eq!(fs::read(&index).unwrap(), conflict_index);
        assert_eq!(run(&["rev-parse", "HEAD"]), divergent);
        assert_eq!(fs::read(&incoming_path).unwrap(), b"otro cambio activo\n");

        run(&["update-ref", "HEAD", &original]);
        run(&["read-tree", &original]);
        fs::write(&incoming_path, b"mismo contenido\n").unwrap();
        pick(&mut state);
        let view = state.git_commit.view().unwrap();
        let plan = view
            .merge_review()
            .unwrap_or_else(|| panic!("{:?}", view.error));
        assert!(plan.is_cherry_pick() && plan.can_apply());
        assert_eq!(plan.mainline().unwrap().0, 2);
        assert_eq!(plan.rows.len(), 2);
        state.key(0x0d);
        assert!(!added.exists());
        assert_eq!(run(&["rev-parse", "HEAD"]), original);
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().is_none(), "{}", state.status);
        assert!(added.exists());
        assert_eq!(fs::read(&incoming_path).unwrap(), b"cambio entrante\n");
        assert_eq!(
            run(&["rev-list", "--parents", "-n", "1", "HEAD"]),
            format!("{} {original}", run(&["rev-parse", "HEAD"]))
        );

        use jc_app::git_commit::CommitOperation;
        let applied = run(&["rev-parse", "HEAD"]);
        let revert = |state: &mut State| {
            state.key_override = Some(jc_app::Modifiers::NONE);
            state.key(0x74);
            wait(state, |s| s.git_revision.state() == GitRevisionState::Ready);
            let row = state
                .git_revision
                .graph()
                .unwrap()
                .entries
                .iter()
                .position(|entry| entry.commit.id == applied)
                .unwrap();
            for _ in 0..row {
                state.key(0x28);
            }
            state.key(b'V' as usize);
            state.key(0x0d);
            wait(state, |s| !s.git_commit.busy());
        };
        fs::write(&incoming_path, b"edicion posterior\n").unwrap();
        run(&["add", "--", "nombre-entrante.txt"]);
        let tree = run(&["write-tree"]);
        let later = run(&[
            "commit-tree",
            &tree,
            "-p",
            &applied,
            "-m",
            "Edición posterior",
        ]);
        run(&["update-ref", "HEAD", &later]);
        let before = fs::read(&index).unwrap();
        revert(&mut state);
        let view = state.git_commit.view().unwrap();
        let conflict = view
            .merge_conflicts()
            .unwrap_or_else(|| panic!("{:?}", view.error));
        assert_eq!(conflict.operation(), Some(CommitOperation::Revert));
        state.key(0x1b);
        assert!(state.git_commit.view().is_none());
        assert!(!pending.exists() && added.exists());
        assert_eq!(fs::read(&index).unwrap(), before);
        assert_eq!(fs::read(&incoming_path).unwrap(), b"edicion posterior\n");
        assert_eq!(run(&["rev-parse", "HEAD"]), later);

        run(&["update-ref", "HEAD", &applied]);
        run(&["read-tree", &applied]);
        fs::write(&incoming_path, b"cambio entrante\n").unwrap();
        revert(&mut state);
        let view = state.git_commit.view().unwrap();
        let preview = view
            .merge_review()
            .unwrap_or_else(|| panic!("{:?}", view.error));
        assert_eq!(preview.operation(), Some(CommitOperation::Revert));
        assert_eq!(preview.rows.len(), 2);
        state.key(0x0d);
        assert!(added.exists());
        assert_eq!(run(&["rev-parse", "HEAD"]), applied);
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().is_none(), "{}", state.status);
        assert!(!added.exists());
        assert_eq!(fs::read(&incoming_path).unwrap(), b"mismo contenido\n");
        assert_eq!(
            run(&["rev-list", "--parents", "-n", "1", "HEAD"]),
            format!("{} {applied}", run(&["rev-parse", "HEAD"]))
        );
        let current = run(&["rev-parse", "HEAD"]);
        let before = fs::read(&index).unwrap();
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(0x74);
        wait(&mut state, |s| {
            s.git_revision.state() == GitRevisionState::Ready
        });
        let row = state
            .git_revision
            .graph()
            .unwrap()
            .entries
            .iter()
            .position(|entry| entry.commit.id == applied)
            .unwrap();
        for _ in 0..row {
            state.key(0x28);
        }
        state.key(b'X' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        let plan = view
            .merge_review()
            .unwrap_or_else(|| panic!("{:?}", view.error));
        assert_eq!(plan.reset_mode(), Some(jc_app::git_commit::ResetMode::Soft));
        assert_eq!(plan.rows.len(), 2);
        state.key(0x0d);
        assert_eq!(run(&["rev-parse", "HEAD"]), current);
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().is_none(), "{}", state.status);
        assert_eq!(run(&["rev-parse", "HEAD"]), applied);
        assert_eq!(fs::read(&index).unwrap(), before);
        assert!(!added.exists());
        assert_eq!(fs::read(&incoming_path).unwrap(), b"mismo contenido\n");

        use jc_app::git_commit::ResetMode;
        fs::write(&incoming_path, b"cambio local para respaldar\n").unwrap();
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(0x74);
        wait(&mut state, |s| {
            s.git_revision.state() == GitRevisionState::Ready
        });
        state.key(b'X' as usize);
        state.key(0x26);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        let plan = view
            .merge_review()
            .unwrap_or_else(|| panic!("{:?}", view.error));
        assert_eq!(plan.reset_mode(), Some(ResetMode::Hard));
        assert!(plan.hard_changes_file(b"nombre-entrante.txt"));
        state.key(0x0d);
        assert_eq!(
            fs::read(&incoming_path).unwrap(),
            b"cambio local para respaldar\n"
        );
        assert!(!added.exists());
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().is_none(), "{}", state.status);
        assert_eq!(fs::read(&incoming_path).unwrap(), b"cambio entrante\n");
        assert!(added.exists());
        let backup = fs::read_dir(fixture.root.join(".git"))
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("cide-hard-"))
            })
            .unwrap();
        assert_eq!(fs::read(backup.path().join("index")).unwrap(), before);
        assert_eq!(
            fs::read(backup.path().join("archivos/1")).unwrap(),
            b"cambio local para respaldar\n"
        );

        let base_file = fixture.root.join("base-rebase.txt");
        fs::write(&base_file, b"cambio de la nueva base\n").unwrap();
        run(&["read-tree", &original]);
        run(&["add", "--", "base-rebase.txt"]);
        let tree = run(&["write-tree"]);
        let onto = run(&["commit-tree", &tree, "-p", &original, "-m", "Nueva base"]);
        run(&["update-ref", "refs/heads/rebase", &onto]);
        run(&["read-tree", &applied]);
        fs::remove_file(&base_file).unwrap();
        let before = fs::read(&index).unwrap();
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(0x74);
        wait(&mut state, |s| {
            s.git_revision.state() == GitRevisionState::Ready
        });
        let graph = state.git_revision.graph().unwrap();
        let row = (1..graph.reference_count())
            .find(|&row| graph.reference_label(row) == Some("Rama rebase"))
            .unwrap();
        state.key(b'R' as usize);
        for _ in 0..row {
            state.key(0x28);
        }
        state.key(b'I' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        assert_eq!(view.title(), "Rebase interactivo");
        assert_eq!(view.rebase_todo().unwrap()[0].original.to_string(), applied);
        state.key(b'D' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        let plan = view.merge_review().unwrap();
        assert_eq!(plan.result().unwrap().to_string(), onto);
        assert_eq!(fs::read(&index).unwrap(), before);
        assert_eq!(run(&["rev-parse", "HEAD"]), applied);
        state.key(0x1b);
        state.key(b'E' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        let plan = view
            .merge_review()
            .unwrap_or_else(|| panic!("{:?}", view.error));
        assert!(plan.is_rebase() && plan.can_apply());
        assert_eq!(plan.rebase_steps().len(), 1);
        assert_eq!(plan.rebase_steps()[0].original.to_string(), applied);
        assert_eq!(plan.rows[0].path, b"base-rebase.txt");
        let result = plan.result().unwrap().to_string();
        state.key(0x0d);
        assert!(!base_file.exists());
        assert_eq!(fs::read(&index).unwrap(), before);
        assert_eq!(run(&["rev-parse", "HEAD"]), applied);
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().is_none(), "{}", state.status);
        assert_eq!(run(&["rev-parse", "HEAD"]), result);
        assert_eq!(run(&["rev-parse", "HEAD^"]), onto);
        assert_eq!(fs::read(&base_file).unwrap(), b"cambio de la nueva base\n");
        assert_eq!(fs::read(&incoming_path).unwrap(), b"cambio entrante\n");

        run(&["read-tree", &onto]);
        fs::write(&incoming_path, b"contenido de base alterna\n").unwrap();
        run(&["add", "--", "nombre-entrante.txt"]);
        let tree = run(&["write-tree"]);
        let conflicting_base = run(&["commit-tree", &tree, "-p", &onto, "-m", "Base alterna"]);
        run(&["update-ref", "refs/heads/base-alterna", &conflicting_base]);
        run(&["read-tree", &result]);
        fs::write(&incoming_path, b"cambio entrante\n").unwrap();
        let before = fs::read(&index).unwrap();
        state.key_override = Some(jc_app::Modifiers::NONE);
        state.key(0x74);
        wait(&mut state, |s| {
            s.git_revision.state() == GitRevisionState::Ready
        });
        let graph = state.git_revision.graph().unwrap();
        let row = (1..graph.reference_count())
            .find(|&row| graph.reference_label(row) == Some("Rama base-alterna"))
            .unwrap();
        state.key(b'R' as usize);
        for _ in 0..row {
            state.key(0x28);
        }
        state.key(b'E' as usize);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        let conflict = view
            .merge_conflicts()
            .unwrap_or_else(|| panic!("{:?}", view.error));
        assert_eq!(
            conflict.rebase_conflict_commit().unwrap().to_string(),
            result
        );
        state.key(0x0d);
        state.key(0x09);
        state.key(0x09);
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(b'U' as usize);
        state.key_override = Some(jc_app::Modifiers::NONE);
        for unit in "GUI ".encode_utf16() {
            state.character(unit);
        }
        state.key_override = Some(jc_app::Modifiers::CTRL);
        state.key(0x0d);
        assert!(state.git_commit.view().unwrap().all_merge_choices_saved());
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        assert!(
            view.merge_review()
                .is_some_and(|plan| plan.is_rebase() && plan.can_apply()),
            "{:?}",
            view.error
        );
        assert_eq!(run(&["rev-parse", "HEAD"]), result);
        assert_eq!(fs::read(&index).unwrap(), before);
        assert_eq!(fs::read(&incoming_path).unwrap(), b"cambio entrante\n");
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert!(state.git_commit.view().is_none(), "{}", state.status);
        assert_eq!(run(&["rev-parse", "HEAD^"]), conflicting_base);
        assert_eq!(fs::read(&incoming_path).unwrap(), b"GUI cambio entrante\n");
    }

    #[test]
    fn gui_recovers_a_deleted_file_only_after_review() {
        gui_restore_case(None);
    }

    #[test]
    fn gui_restores_an_existing_file_and_preserves_its_local_bytes() {
        gui_restore_case(Some(b"cambios locales\r\n\xff"));
    }

    fn gui_restore_case(disk: Option<&[u8]>) {
        use jc_app::Modifiers;
        let fixture = Fixture::new();
        let path = fixture.root.join("recuperar.txt");
        let bytes = b"primera\r\nultima";
        fs::write(&path, bytes).unwrap();
        let mut state = State::new(&GuiOptions {
            project_root: Some(fixture.root.clone()),
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
        let index_path = fixture.root.join(".git/index");
        let index = fs::read(&index_path).unwrap();
        if let Some(disk) = disk {
            fs::write(&path, disk).unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        state.request_git(GitIntent::Status);
        wait(&mut state, |s| {
            s.git_status.state() == GitStatusTaskState::Ready
        });
        state.request_git_local(PairKind::Worktree);
        wait(&mut state, |s| {
            s.git_revision.state() == GitRevisionState::Ready
        });
        state.key_override = Some(Modifiers::CTRL.union(Modifiers::ALT));
        state.key(b'R' as usize);
        state.key_override = Some(Modifiers::NONE);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        let view = state.git_commit.view().unwrap();
        assert!(view.restore_review().is_some(), "{:?}", view.error);
        assert_eq!(
            view.restore_review().unwrap().pair().after.bytes(),
            disk.unwrap_or_default()
        );
        assert_eq!(fs::read(&path).ok().as_deref(), disk);
        state.key_override = Some(Modifiers::CTRL);
        state.key(0x0d);
        wait(&mut state, |s| !s.git_commit.busy());
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(fs::read(index_path).unwrap(), index);
        if let Some(disk) = disk {
            let backup = fs::read_dir(fixture.root.join(".git"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| path.extension().is_some_and(|ext| ext == "bak"))
                .expect("backed up local bytes");
            assert_eq!(fs::read(backup).unwrap(), disk);
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

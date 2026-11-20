//! Historical Git observations never borrow or replace the editable document.
use super::*;
#[cfg(test)]
use jc_app::diff_model::OwnedDiffModel;
use jc_app::Modifiers;

#[path = "git_history_browser.rs"]
mod browser;
pub(super) use browser::HistoryBrowser;

impl State {
    pub(crate) fn request_git_history(&mut self, commit: String) {
        self.request_git_history_parent(commit, 0);
    }

    pub(super) fn request_git_history_parent(&mut self, commit: String, parent: usize) {
        let Some(root) = self.git_root().map(std::path::Path::to_owned) else {
            return;
        };
        self.git_workspace.active = Column::Files;
        self.git_workspace.diff_column = 0;
        self.git_workspace.local_kind = None;
        self.git_diff.clear(&mut self.jobs);
        self.git_workspace.history_commit = Some(commit.clone());
        self.git_workspace.focus_diff = false;
        self.git_workspace.history_selected = 0;
        self.git_workspace.history_file = None;
        self.git_workspace.history_browser = HistoryBrowser::default();
        self.git_workspace.history_hunk = 0;
        self.git_workspace.diff_first = 0;
        self.git_workspace.history_encoding = self.git_workspace.history_encoding.or(self.encoding);
        self.git_workspace.notice = None;
        self.git_workspace.invalidate();
        if let Err(error) = self
            .git_revision
            .request_revision_parent(root, commit, parent)
        {
            self.git_workspace.notice = Some(error);
        }
    }
    pub(crate) fn request_git_history_file(&mut self, index: usize) {
        let Some(path) = self
            .git_revision
            .revision()
            .and_then(|revision| revision.files.get(index))
            .map(|file| file.path.clone())
        else {
            return;
        };
        self.request_git_history_path(path);
    }
    pub(super) fn cycle_git_history_parent(&mut self) {
        let (current, parents) = self.git_revision.parent_options();
        if parents.len() > 1 {
            if let Some(commit) = self.git_workspace.history_commit.clone() {
                self.request_git_history_parent(commit, (current + 1) % parents.len());
            }
        }
    }
    pub(crate) fn git_history_key(&mut self, code: usize, modifiers: Modifiers) -> Action {
        if self.git_history_browser_key(code, modifiers) {
            return Action::None;
        }
        if modifiers != Modifiers::NONE {
            return Action::None;
        }
        match code {
            0x1b => {
                self.git_revision.cancel();
                self.git_panel = None;
                self.git_workspace.history_commit = None;
            }
            0x74 => {
                if let Some(c) = self.git_workspace.history_commit.clone() {
                    self.request_git_history_parent(c, self.git_revision.parent_options().0);
                }
            }
            0x09 => self.git_workspace.focus_diff = !self.git_workspace.focus_diff,
            0x50 => self.cycle_git_history_parent(),
            0x0d if !self.git_workspace.focus_diff => {
                if self.git_workspace.history_browser.showing_tree() {
                    self.open_selected_git_history_tree_entry();
                } else {
                    self.request_git_history_file(self.git_workspace.history_selected);
                }
            }
            0x26 | 0x28 | 0x21 | 0x22 | 0x24 | 0x23 => {
                let delta = match code {
                    0x26 => -1,
                    0x28 => 1,
                    0x21 => -10,
                    0x22 => 10,
                    0x24 => isize::MIN,
                    _ => isize::MAX,
                };
                if self.git_workspace.focus_diff {
                    if matches!(code, 0x26 | 0x28) {
                        self.git_workspace.history_hunk =
                            self.git_workspace.history_hunk.saturating_add_signed(delta);
                        self.git_workspace.diff_first = 0;
                    } else {
                        self.git_workspace.diff_first = self
                            .git_workspace
                            .diff_first
                            .saturating_add_signed(delta)
                            .min(
                                self.git_workspace
                                    .diff_total
                                    .saturating_sub(self.git_workspace.diff_page),
                            );
                    }
                } else {
                    self.move_git_history_files(delta);
                }
            }
            _ => {}
        }
        self.git_workspace.invalidate();
        Action::None
    }
    pub(super) fn prepare_git_history(&mut self, mut frame: Frame) -> Frame {
        let root = self.git_root().map(std::path::Path::to_owned);
        self.git_revision.invalidate_context(root.as_deref());
        let l = Layout::new(self);
        let p = |v| self.dpi.pixels(v);
        frame.historical = true;
        frame.details_top = p(164);
        frame.rows.clear();
        frame.code.clear();
        frame.message.clear();
        frame.buttons.retain(|(r, _, _)| r.top < l.toolbar.bottom);
        frame.hits.actions.retain(|(_, a)| {
            matches!(
                a,
                Intent::Refresh | Intent::Close | Intent::Commit(_) | Intent::Local
            )
        });
        let browser = self.git_revision.revision().is_some();
        let links = [
            (0, "Cambios locales", Intent::Local),
            (1, "Resumen", Intent::HistorySummary),
        ];
        for (n, name, intent) in links.into_iter().filter(|_| !browser) {
            let r = rect(
                l.preview.left + p(12) + n * p(130),
                l.preview.top + p(2),
                l.preview.left + p(136) + n * p(130),
                l.preview.top + p(28),
            );
            if r.right < l.preview.right {
                frame.buttons.push((r, name.into(), false));
                frame.hits.actions.push((r, intent));
            }
        }
        let revision = self.git_revision.revision();
        let files = revision.map_or(&[][..], |r| r.files.as_slice());
        let capacity =
            ((l.files.bottom - l.files.top - l.head) / l.row.max(1)).clamp(0, 128) as usize;
        let selected = self
            .git_workspace
            .history_selected
            .min(files.len().saturating_sub(1));
        self.git_workspace.history_selected = selected;
        let first = selected
            .saturating_sub(capacity.saturating_sub(1))
            .min(files.len().saturating_sub(capacity));
        let showing_tree = browser && self.git_workspace.history_browser.showing_tree();
        for (index, f) in files
            .iter()
            .enumerate()
            .skip(first)
            .take(capacity)
            .filter(|_| !showing_tree)
        {
            let top = l.files.top + l.head + (index - first) as i32 * l.row;
            let r = rect(
                l.files.left,
                top,
                l.files.right,
                (top + l.row).min(l.files.bottom),
            );
            let path = opaque(&f.path, 256);
            let (tag, color) = if f.before.is_none() {
                ("A", GREEN)
            } else if f.after.is_none() {
                ("D", RED)
            } else {
                ("M", shell::ACCENT)
            };
            frame.rows.push(FileRow {
                rect: r,
                name: path.rsplit('/').next().unwrap_or(&path).into(),
                detail: path
                    .rsplit_once('/')
                    .map_or("Raíz del repositorio", |(parent, _)| parent)
                    .into(),
                tag,
                color,
                selected: index == selected,
            });
            frame.hits.actions.push((r, Intent::HistoryFile(index)));
        }
        frame.count = format!("{} archivos", files.len());
        frame.subtitle = "Archivos de la revisión".into();
        frame.footer = "Comparación histórica · solo lectura".into();
        let (parent, parents) = self.git_revision.parent_options();
        if parents.len() > 1 {
            let label = format!("Padre {}/{} ▸", parent + 1, parents.len());
            frame.footer = format!("P cambiar padre · {} · {}", label, frame.footer);
            let right = l.preview.right - p(12);
            let top = l.preview.top + p(96);
            let r = rect(right - p(117), top, right, top + p(24));
            if r.left >= l.preview.left && r.bottom <= l.preview.bottom {
                frame.buttons.push((r, label, false));
                frame.hits.actions.push((r, Intent::HistoryParent));
            }
        }
        if let Some(r) = revision {
            if frame.summary.is_empty() {
                frame.summary = vec![
                    "Revisión histórica".into(),
                    format!(
                        "{} · {} archivos modificados",
                        &r.commit[..r.commit.len().min(8)],
                        r.files.len()
                    ),
                ];
            }
            frame.details = vec![
                format!("Commit: {}", r.commit),
                r.parent
                    .as_ref()
                    .map_or("Commit raíz · árbol anterior vacío".into(), |parent| {
                        format!("Padre elegido · {parent}")
                    }),
                format!(
                    "{} archivos cambiados · {} padres",
                    r.files.len(),
                    r.parents.len()
                ),
            ];
            if r.limited {
                frame.details.push("Metadatos de padres limitados".into());
            }
            if files.is_empty() {
                frame.message = "La revisión no cambia archivos frente al padre elegido".into();
            }
        } else {
            frame.message = self.git_revision.message().into();
            frame.details.clear();
            if frame.summary.is_empty() {
                frame.summary = vec![
                    "Revisión histórica".into(),
                    self.git_workspace
                        .history_commit
                        .as_deref()
                        .map_or(String::new(), |id| id.chars().take(12).collect()),
                ];
            }
        }
        if self.git_workspace.history_file.is_some() {
            frame.details.clear();
            frame.subtitle = self.git_revision.message().into();
            if let Some(c) = self.git_revision.comparison() {
                frame.subtitle = format!("{} · revisión histórica", opaque(&c.path, 200));
                if browser {
                    frame.summary.truncate(1);
                    frame.summary.push(opaque(&c.path, 240));
                }
                let r = rect(
                    l.preview.left + p(12),
                    l.preview.top + p(128),
                    l.preview.left + p(230),
                    l.preview.top + p(152),
                );
                let name = self
                    .git_workspace
                    .history_encoding
                    .map_or("Codificación: elegir".into(), |e| {
                        format!("Codificación: {}", e.name())
                    });
                frame.buttons.push((r, name, false));
                frame.hits.actions.push((r, Intent::HistoryEncoding));
                let total = c.model.hunks().count();
                self.git_workspace.history_hunk =
                    self.git_workspace.history_hunk.min(total.saturating_sub(1));
                for (i, name, delta) in [(0, "Anterior", -1), (1, "Siguiente", 1)] {
                    let r = rect(
                        l.preview.left + p(240) + i * p(86),
                        l.preview.top + p(128),
                        l.preview.left + p(322) + i * p(86),
                        l.preview.top + p(152),
                    );
                    if r.right < l.preview.right {
                        frame.buttons.push((r, name.into(), false));
                        frame.hits.actions.push((r, Intent::Hunk(delta)));
                    }
                }
                if let Some(encoding) = self.git_workspace.history_encoding {
                    let projection = controls::project(
                        &c.model,
                        controls::ProjectionView {
                            hunk: self.git_workspace.history_hunk,
                            first: self.git_workspace.diff_first,
                            encoding,
                            geometry: (
                                rect(
                                    l.preview.left,
                                    l.preview.top,
                                    l.preview.right - p(12),
                                    l.preview.bottom - p(14),
                                ),
                                l.preview.top + p(164),
                                self.cell_height,
                            ),
                            mode: self.git_workspace.diff_mode(),
                            column: self.git_workspace.diff_column,
                            cell_width: self.cell_width,
                        },
                    );
                    let total = projection.total;
                    frame.code = self.git_workspace.set_projection(projection);
                    if total == 0 {
                        frame
                            .details
                            .push("El archivo no tiene cambios de contenido".into());
                    }
                } else {
                    frame
                        .details
                        .push("Elige la codificación para interpretar los bytes históricos".into());
                }
            } else if browser {
                frame.details.push(self.git_revision.message().into());
            }
        }
        if browser {
            if showing_tree {
                self.prepare_git_history_tree_rows(&mut frame, &l);
            }
            self.prepare_git_history_browser(&mut frame, &l);
            if let Some(notice) = &self.git_workspace.notice {
                frame.footer = format!("{notice} · {}", frame.footer);
            }
        }
        frame.hits.stamp = self.git_workspace_stamp();
        frame
    }
}

#[cfg(test)]
fn historical_rows(
    model: &OwnedDiffModel,
    hunk: usize,
    first: usize,
    page: usize,
    encoding: TextEncoding,
    geometry: (Rect, i32, i32),
) -> (Vec<CodeRow>, usize, usize) {
    let (rows, first, total, _) = compare_rows::rows(
        model,
        hunk,
        first,
        page,
        encoding,
        geometry,
        DiffView::Unified,
        0,
    );
    (rows, first, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::diff_model::DiffLimits;
    use jc_app::git_revision::GitRevisionState;
    use std::{
        fs,
        process::Command,
        sync::Arc,
        time::{Duration, Instant},
    };

    #[test]
    fn historical_addition_and_deletion_render_exact_owned_sources_without_document() {
        let bytes = b"uno\r\ndos\xff\n".to_vec();
        for added in [true, false] {
            let (a, b) = if added {
                (Vec::new(), bytes.clone())
            } else {
                (bytes.clone(), Vec::new())
            };
            let model =
                OwnedDiffModel::compute(Arc::new(a), Arc::new(b), DiffLimits::default(), || false);
            let (rows, first, total) = historical_rows(
                &model,
                0,
                usize::MAX,
                1,
                TextEncoding::Utf8,
                (rect(0, 0, 800, 600), 100, 18),
            );
            assert_eq!(rows.len(), 1);
            assert_eq!(first, total - 1);
            assert_eq!(rows[0].kind, if added { 1 } else { -1 });
            assert!(rows[0].text.contains("\\xFF"));
            assert_eq!(
                model.source_bytes(if added {
                    DiffSide::Right
                } else {
                    DiffSide::Left
                }),
                bytes
            );
        }
    }

    #[test]
    fn historical_frame_and_codec_choice_do_not_admit_work_or_use_editor_codec() {
        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.width = 1200;
        state.height = 800;
        state.git_panel = Some(GitIntent::Status);
        state.git_workspace.history_commit = Some("1".repeat(40));
        let revision = state
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision();
        let jobs = state.jobs.outstanding();
        let frame = state.prepare_git_workspace().unwrap();
        assert!(frame.historical);
        assert!(frame.code.is_empty());
        assert_eq!(frame.details_top, state.dpi.pixels(164));
        let tabs: Vec<_> = frame
            .hits
            .actions
            .iter()
            .filter(|(r, a)| {
                matches!(a, Intent::Local | Intent::HistorySummary) && r.left >= frame.preview.left
            })
            .collect();
        for (r, _) in tabs {
            assert!(r.top >= frame.preview.top);
            assert!(r.bottom <= frame.preview.top + state.dpi.pixels(32));
        }
        assert_eq!(state.git_workspace.history_encoding, None);
        assert_eq!(state.jobs.outstanding(), jobs);
        assert_eq!(
            state
                .workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision(),
            revision
        );
        state.git_history_key(0x1b, Modifiers::NONE);
        assert!(state.git_panel.is_none());
        assert_eq!(
            state
                .workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision(),
            revision
        );
    }

    #[test]
    fn history_tree_and_tabs_switch_pinned_files_without_touching_editor() {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work/test-temp");
        fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        let root = base.join(format!(
            "gui-tree-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "NUL")
                .env("GIT_AUTHOR_NAME", "CIDE fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@invalid")
                .env("GIT_COMMITTER_NAME", "CIDE fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@invalid")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        git(&["init", "--quiet", "--template=", "--initial-branch=main"]);
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/estable.p"), b"permanente\r\n").unwrap();
        fs::write(root.join("src/cambio.p"), b"anterior\r\n").unwrap();
        git(&["add", "--", "src"]);
        git(&[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "Inicial",
        ]);
        fs::write(root.join("src/cambio.p"), b"revisado\r\n").unwrap();
        git(&["add", "--", "src/cambio.p"]);
        git(&[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "Cambio",
        ]);
        let commit = git(&["rev-parse", "HEAD"]);
        fs::write(root.join("src/cambio.p"), b"solo en disco").unwrap();
        let index = fs::read(root.join(".git/index")).unwrap();

        let mut state = State::new(&crate::GuiOptions::default()).unwrap();
        state.width = 1450;
        state.height = 840;
        state.project_root = Some(root.clone());
        state.git_panel = Some(GitIntent::Status);
        state.git_workspace.history_encoding = Some(TextEncoding::Utf8);
        let revision = state
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision();
        let wait = |state: &mut State| {
            let deadline = Instant::now() + Duration::from_secs(8);
            while matches!(
                state.git_revision.state(),
                GitRevisionState::Queued | GitRevisionState::Running
            ) {
                state.poll_git();
                assert!(Instant::now() < deadline, "history delivery timeout");
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(state.git_revision.state(), GitRevisionState::Ready);
        };
        let click = |state: &mut State, intent: fn(Intent) -> bool| {
            let frame = state.prepare_git_workspace().unwrap();
            let r = frame
                .hits
                .actions
                .iter()
                .find(|(_, action)| intent(*action))
                .unwrap()
                .0;
            state.git_workspace.hits = Some(frame.hits.clone());
            state.git_workspace_click((r.left + r.right) / 2, (r.top + r.bottom) / 2);
        };

        state.request_git_history(commit.clone());
        wait(&mut state);
        assert_eq!(state.git_revision.revision().unwrap().files.len(), 1);
        click(&mut state, |a| matches!(a, Intent::HistoryFileMode(true)));
        wait(&mut state);
        click(&mut state, |a| matches!(a, Intent::HistoryTreeEntry(0)));
        wait(&mut state);
        let frame = state.prepare_git_workspace().unwrap();
        assert!(frame.rows.iter().any(|row| row.name == "estable.p"));
        assert!(frame.rows.iter().any(|row| row.name == "cambio.p"));
        click(&mut state, |a| matches!(a, Intent::HistoryTreeEntry(2)));
        wait(&mut state);
        let unchanged = state.git_revision.comparison().unwrap();
        assert_eq!(unchanged.path, b"src/estable.p");
        assert_eq!(unchanged.model.hunks().count(), 0);
        click(&mut state, |a| matches!(a, Intent::HistoryTreeEntry(1)));
        wait(&mut state);
        let changed = state.git_revision.comparison().unwrap();
        assert_eq!(changed.model.source_bytes(DiffSide::Right), b"revisado\r\n");
        click(&mut state, |a| matches!(a, Intent::HistorySummary));
        assert!(state.prepare_git_workspace().unwrap().code.is_empty());
        click(&mut state, |a| matches!(a, Intent::HistoryTab(0)));
        wait(&mut state);
        assert_eq!(
            state.git_revision.comparison().unwrap().path,
            b"src/estable.p"
        );
        click(&mut state, |a| matches!(a, Intent::HistoryTabClose));
        wait(&mut state);
        assert_eq!(
            state.git_revision.comparison().unwrap().path,
            b"src/cambio.p"
        );
        click(&mut state, |a| matches!(a, Intent::HistoryTreeEntry(0)));
        wait(&mut state);
        assert!(state.git_revision.directory().unwrap().path.is_empty());
        assert_eq!(
            state.git_revision.comparison().unwrap().path,
            b"src/cambio.p"
        );
        state.request_git_history(commit);
        wait(&mut state);
        let frame = state.prepare_git_workspace().unwrap();
        assert!(!frame
            .hits
            .actions
            .iter()
            .any(|(_, a)| matches!(a, Intent::HistoryTab(_))));
        assert_eq!(
            state
                .workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision(),
            revision
        );
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
        assert_eq!(
            fs::read(root.join("src/cambio.p")).unwrap(),
            b"solo en disco"
        );
        assert_eq!(root.parent(), Some(base.as_path()));
        assert!(root.canonicalize().unwrap().starts_with(&base));
        assert!(!root.is_symlink());
        let _ = fs::remove_dir_all(root);
    }
}

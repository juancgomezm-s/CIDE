//! Per-path history is a bounded projection of the existing revision worker.
use super::*;

impl State {
    pub(super) fn add_git_path_history_button(&self, f: &mut Frame) {
        if self.git_workspace.blame_query.is_some()
            || self.git_workspace.path_query.is_some()
            || f.preview.right - f.preview.left
                < self.dpi.pixels(if f.historical { 440 } else { 320 })
        {
            return;
        }
        let r = rect(
            f.preview.right - self.dpi.pixels(148),
            f.preview.top + self.dpi.pixels(2),
            f.preview.right - self.dpi.pixels(8),
            f.preview.top + self.dpi.pixels(28),
        );
        f.buttons.push((r, "Historial del archivo".into(), false));
        f.hits.actions.push((r, Intent::PathHistory));
    }
    pub(super) fn git_selected_history_target(&self) -> Option<(String, Vec<u8>)> {
        if self.git_workspace.path_query.is_some() && self.git_workspace.history_commit.is_none() {
            if let Some(page) = self.git_revision.path_history() {
                return Some((
                    page.entries
                        .get(self.git_workspace.path_selected)
                        .map_or_else(|| page.start.clone(), |e| e.commit.clone()),
                    page.path.clone(),
                ));
            }
        }
        let path = self
            .git_revision
            .comparison()
            .map(|c| c.path.clone())
            .or_else(|| {
                self.git_revision
                    .revision()
                    .and_then(|r| r.files.get(self.git_workspace.history_selected))
                    .map(|f| f.path.clone())
            })
            .or_else(|| {
                self.git_status
                    .result()
                    .filter(|_| self.git_status.state() == GitStatusTaskState::Ready)
                    .and_then(|r| r.status().entries().get(self.git_view.selected_status()))
                    .map(|e| e.path().to_vec())
            })
            .or_else(|| {
                let root = self.project_root.as_ref()?;
                let doc = self.workbench.active_document()?;
                let relative = doc.session.path().strip_prefix(root).ok()?;
                if !relative
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
                {
                    return None;
                }
                Some(relative.to_str()?.replace('\\', "/").into_bytes())
            });
        let start = self.git_workspace.history_commit.clone().or_else(|| {
            self.git_status
                .result()
                .filter(|_| self.git_status.state() == GitStatusTaskState::Ready)
                .and_then(|r| r.info())
                .and_then(|i| i.commits.first())
                .map(|c| c.id.clone())
        });
        start.zip(path)
    }
    pub(super) fn request_git_path_history(&mut self) {
        self.git_workspace.blame_query = None;
        if let Some((start, path)) = self.git_selected_history_target() {
            self.git_workspace.path_query = Some((start, path));
            self.refresh_git_path_history();
        } else {
            self.git_workspace.notice = Some(
                "Selecciona un archivo y actualiza el repositorio con al menos un commit".into(),
            );
            self.git_workspace.invalidate();
        }
    }
    fn reset_path_view(&mut self) {
        self.git_workspace.history_commit = None;
        self.git_workspace.history_file = None;
        self.git_workspace.local_kind = None;
        self.git_workspace.commit = None;
        self.git_workspace.path_selected = 0;
        self.git_workspace.history_first = 0;
        self.git_workspace.active = Column::History;
        self.git_workspace.focus_diff = false;
        self.git_workspace.notice = None;
        self.git_workspace.invalidate();
    }
    pub(super) fn refresh_git_path_history(&mut self) {
        let Some((start, path)) = self.git_workspace.path_query.clone() else {
            return;
        };
        let Some(root) = self.project_root.clone() else {
            return;
        };
        self.reset_path_view();
        self.git_diff.clear(&mut self.jobs);
        if let Err(e) = self.git_revision.request_path_history(root, start, path) {
            self.git_workspace.notice = Some(e);
        }
    }
    pub(super) fn next_git_path_history(&mut self) {
        let Some(root) = self.project_root.clone() else {
            return;
        };
        if let Err(e) = self.git_revision.request_path_history_next(root) {
            self.git_workspace.notice = Some(e);
            return;
        }
        self.reset_path_view();
    }
    pub(super) fn open_git_path_commit(&mut self, index: usize) {
        let Some((commit, path)) = self.git_revision.path_history().and_then(|p| {
            p.entries
                .get(index)
                .map(|e| (e.commit.clone(), p.path.clone()))
        }) else {
            return;
        };
        let Some(root) = self.project_root.clone() else {
            return;
        };
        self.git_workspace.path_selected = index;
        self.git_workspace.history_commit = Some(commit.clone());
        self.git_workspace.history_file = Some(0);
        self.git_workspace.history_hunk = 0;
        self.git_workspace.diff_first = 0;
        self.git_workspace.diff_column = 0;
        self.git_workspace.history_encoding = self.git_workspace.history_encoding.or(self.encoding);
        self.git_workspace.focus_diff = true;
        self.git_workspace.active = Column::Comparison;
        self.git_workspace.invalidate();
        if let Err(e) = self.git_revision.request_file(root, commit, path) {
            self.git_workspace.notice = Some(e);
        }
    }
    pub(super) fn back_git_path_history(&mut self) {
        self.git_workspace.history_commit = None;
        self.git_workspace.history_file = None;
        self.git_workspace.active = Column::History;
        self.git_workspace.focus_diff = false;
        self.git_workspace.invalidate();
    }
    pub(super) fn git_path_history_key(&mut self, code: usize) -> bool {
        if self.git_workspace.path_query.is_none() {
            if code == 0x41 {
                self.request_git_path_history();
                return true;
            }
            return false;
        }
        match code {
            0x1b | 0x42 if self.git_workspace.history_commit.is_some() => {
                self.back_git_path_history()
            }
            0x1b | 0x48 => {
                self.git_workspace.path_query = None;
                self.request_git(GitIntent::Status);
            }
            0x4e => self.next_git_path_history(),
            0x52 | 0x74 => self.refresh_git_path_history(),
            0x0d if !self.git_workspace.focus_diff => {
                self.open_git_path_commit(self.git_workspace.path_selected)
            }
            0x26 | 0x28 | 0x21 | 0x22 | 0x24 | 0x23 if !self.git_workspace.focus_diff => {
                let total = self
                    .git_revision
                    .path_history()
                    .map_or(0, |p| p.entries.len());
                let delta = match code {
                    0x26 => -1,
                    0x28 => 1,
                    0x21 => -10,
                    0x22 => 10,
                    0x24 => isize::MIN,
                    _ => isize::MAX,
                };
                self.git_workspace.path_selected = self
                    .git_workspace
                    .path_selected
                    .saturating_add_signed(delta)
                    .min(total.saturating_sub(1));
                let l = Layout::new(self);
                let capacity = l.history.map_or(1, |a| {
                    ((a.bottom - a.top - l.head - self.dpi.pixels(56)) / self.dpi.pixels(64)).max(1)
                }) as usize;
                let selected = self.git_workspace.path_selected;
                self.git_workspace.history_first = self
                    .git_workspace
                    .history_first
                    .clamp(selected.saturating_sub(capacity - 1), selected);
                self.git_workspace.invalidate();
            }
            _ => return false,
        }
        true
    }
    pub(super) fn prepare_git_path_history(&mut self, frame: Frame) -> Frame {
        let comparing = self.git_workspace.history_commit.is_some();
        let mut f = if comparing {
            self.prepare_git_history(frame)
        } else {
            frame
        };
        let l = Layout::new(self);
        let p = |v| self.dpi.pixels(v);
        f.commits.clear();
        f.rows.clear();
        f.message.clear();
        f.buttons.retain(|(r, _, _)| {
            r.top < l.toolbar.bottom || (comparing && r.top >= l.preview.top + p(128))
        });
        f.hits.actions.retain(|(_, a)| {
            matches!(
                a,
                Intent::Refresh | Intent::Close | Intent::Hunk(_) | Intent::HistoryEncoding
            )
        });
        let (_, path) = self.git_workspace.path_query.as_ref().expect("path view");
        let path = opaque(path, 256);
        let mut header = |x: i32, width: i32, name: &str, intent: Intent| {
            let r = rect(
                l.preview.left + p(x),
                l.preview.top + p(2),
                l.preview.left + p(x + width),
                l.preview.top + p(28),
            );
            if r.right <= l.preview.right {
                f.buttons.push((r, name.into(), false));
                f.hits.actions.push((r, intent));
            }
        };
        header(8, 122, "Volver al historial", Intent::PathBack);
        header(134, 112, "Todos los archivos", Intent::PathAll);
        let page = self.git_revision.path_history();
        if page.is_some_and(|page| page.next.is_some()) {
            header(250, 106, "Más antiguos", Intent::PathNext);
        }
        f.subtitle = String::new();
        if let Some(page) = page {
            let selected = self
                .git_workspace
                .path_selected
                .min(page.entries.len().saturating_sub(1));
            self.git_workspace.path_selected = selected;
            let meta = format!(
                "{} revisiones · {} commits recorridos{}{}",
                page.entries.len(),
                page.scanned,
                if page.shallow {
                    " · historial superficial"
                } else if page.next.is_some() {
                    " · hay más"
                } else {
                    " · fin"
                },
                if page.metadata_limited {
                    " · metadatos recortados"
                } else {
                    ""
                }
            );
            f.summary = if comparing {
                self.git_workspace
                    .history_commit
                    .as_ref()
                    .and_then(|id| page.entries.iter().find(|e| &e.commit == id))
                    .map(|e| {
                        vec![
                            opaque(&e.summary, 160),
                            format!(
                                "{} · {}",
                                opaque(&e.author, 80),
                                &e.commit[..e.commit.len().min(8)]
                            ),
                        ]
                    })
                    .unwrap_or_default()
            } else {
                vec![format!("Historial de {path}"), meta.clone()]
            };
            f.count = "1 archivo".into();
            if let Some(a) = l.history {
                let capacity = ((a.bottom - a.top - l.head - p(56)) / p(64)).clamp(0, 24) as usize;
                let first = self
                    .git_workspace
                    .history_first
                    .min(page.entries.len().saturating_sub(capacity));
                self.git_workspace.history_first = first;
                for (index, e) in page.entries.iter().enumerate().skip(first).take(capacity) {
                    let top = a.top + l.head + p(56) + (index - first) as i32 * p(64);
                    let r = rect(a.left, top, a.right, (top + p(64)).min(a.bottom));
                    f.commits.push(CommitRow {
                        rect: r,
                        title: opaque(&e.summary, 160),
                        detail: format!(
                            "{} · {}",
                            &e.commit[..e.commit.len().min(8)],
                            opaque(&e.author, 80)
                        ),
                        selected: index == selected,
                        merge: false,
                    });
                    f.hits.actions.push((r, Intent::PathCommit(index)));
                }
                f.hits.actions.push((
                    rect(
                        a.left,
                        a.top + l.head,
                        a.right,
                        (a.top + l.head + p(56)).min(a.bottom),
                    ),
                    Intent::PathAll,
                ));
            }
            if l.files.right > l.files.left {
                let r = rect(
                    l.files.left,
                    l.files.top + l.head,
                    l.files.right,
                    (l.files.top + l.head + l.row).min(l.files.bottom),
                );
                f.rows.push(FileRow {
                    rect: r,
                    name: path.rsplit('/').next().unwrap_or(&path).into(),
                    detail: path
                        .rsplit_once('/')
                        .map_or("Historial del archivo".into(), |(d, _)| d.into()),
                    tag: "H",
                    color: ACCENT,
                    selected: true,
                });
                if !page.entries.is_empty() {
                    f.hits.actions.push((r, Intent::PathCommit(selected)));
                }
            }
            if !comparing {
                f.code.clear();
                f.details = vec![
                    if page.entries.is_empty() {
                        "Sin cambios en esta página".into()
                    } else {
                        "Selecciona una revisión para comparar el archivo".into()
                    },
                    "Primer padre · sin seguimiento de renombrados".into(),
                ];
            } else if self.git_revision.comparison().is_none() {
                f.details.push(self.git_revision.message().into());
            }
            f.footer = format!(
                "Historial por archivo · {meta} · N más antiguos · R actualiza · Esc vuelve"
            );
        } else {
            f.summary = vec![
                format!("Historial de {path}"),
                self.git_revision.message().into(),
            ];
            f.code.clear();
            f.details.clear();
            f.count = "Historial".into();
            f.footer = "Historial por archivo · Solo lectura · R actualiza · Esc vuelve".into();
        }
        if let Some(notice) = &self.git_workspace.notice {
            f.details.push(label(notice, 256));
        }
        f.hits.stamp = self.git_workspace_stamp();
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        process::Command,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    #[test]
    fn path_history_header_keeps_summary_and_file_history_targets_separate() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.git_panel = Some(GitIntent::Status);
        s.git_workspace.history_commit = Some("a".repeat(40));
        s.height = 800;
        for width in [700, 800, 1000, 1280] {
            s.width = width;
            let f = s.prepare_git_workspace().unwrap();
            let buttons: Vec<_> = f
                .buttons
                .iter()
                .filter(|(r, _, _)| {
                    r.top >= f.preview.top && r.bottom <= f.preview.top + s.dpi.pixels(32)
                })
                .collect();
            for (i, (a, _, _)) in buttons.iter().enumerate() {
                for (b, _, _) in &buttons[i + 1..] {
                    assert!(a.right <= b.left || b.right <= a.left, "overlap at {width}");
                }
            }
        }
    }
    fn wait(s: &mut State) {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            s.poll_git();
            if !matches!(
                s.git_revision.state(),
                jc_app::git_revision::GitRevisionState::Queued
                    | jc_app::git_revision::GitRevisionState::Running
            ) {
                break;
            }
            assert!(Instant::now() < deadline, "path history delivery timeout");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            s.git_revision.state(),
            jc_app::git_revision::GitRevisionState::Ready,
            "{}",
            s.git_revision.message()
        );
    }
    fn click(s: &mut State, intent: impl Fn(Intent) -> bool) {
        let f = s.prepare_git_workspace().unwrap();
        let r = f.hits.actions.iter().find(|(_, a)| intent(*a)).unwrap().0;
        s.git_workspace.hits = Some(f.hits.clone());
        s.git_workspace_click((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    }
    #[test]
    fn path_history_gui_clean_file_click_page_compare_return_and_stale_hits_preserve_editor() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work/test-temp");
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap().join(format!(
            "gui-path-history-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
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
        };
        git(&["init", "--quiet", "--template=", "--initial-branch=main"]);
        let file = root.join("archivo.p");
        for n in 0..26 {
            fs::write(&file, format!("version {n}\r\n")).unwrap();
            git(&["add", "--", "archivo.p"]);
            git(&[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                &format!("Revision {n}"),
            ]);
        }
        let mut s = State::new(&crate::GuiOptions {
            project_root: Some(root.clone()),
            encoding: Some(TextEncoding::Utf8),
            ..Default::default()
        })
        .unwrap();
        s.width = 1280;
        s.height = 800;
        s.workbench
            .open_project_file(&root, file.clone(), Some(TextEncoding::Utf8), false, 1024)
            .unwrap();
        let before = s.workbench.active_bytes().unwrap();
        let panes = format!("{:?}", s.workbench.panes());
        s.request_git(GitIntent::Status);
        let deadline = Instant::now() + Duration::from_secs(8);
        while s.git_status.state() != GitStatusTaskState::Ready {
            s.poll_git();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(s.git_status.result().unwrap().status().entries().is_empty());
        click(&mut s, |i| matches!(i, Intent::PathHistory));
        wait(&mut s);
        assert_eq!(s.git_revision.path_history().unwrap().entries.len(), 24);
        let jobs = s.jobs.outstanding();
        let frame = s.prepare_git_workspace().unwrap();
        assert!(!frame.commits.is_empty());
        assert!(frame.footer.contains("hay más"));
        assert_eq!(jobs, s.jobs.outstanding());
        click(&mut s, |i| matches!(i, Intent::PathNext));
        wait(&mut s);
        assert_eq!(s.git_revision.path_history().unwrap().entries.len(), 2);
        click(&mut s, |i| matches!(i, Intent::PathCommit(1)));
        wait(&mut s);
        let pair = s.git_revision.comparison().unwrap();
        assert!(pair.before_oid.is_none());
        assert!(pair.after_oid.is_some());
        let root_blob = pair.after_oid.clone().unwrap();
        assert_eq!(pair.model.source_bytes(DiffSide::Right), b"version 0\r\n");
        assert!(!s.prepare_git_workspace().unwrap().code.is_empty());
        // Browsing another row must not relabel the still accepted root diff.
        s.git_workspace.focus_diff = false;
        assert!(s.git_path_history_key(0x26));
        assert_eq!(s.git_workspace.path_selected, 0);
        assert_eq!(s.prepare_git_workspace().unwrap().summary[0], "Revision 0");
        click(&mut s, |i| matches!(i, Intent::PathBack));
        assert!(s.git_workspace.history_commit.is_none());
        let f = s.prepare_git_workspace().unwrap();
        let r = f
            .hits
            .actions
            .iter()
            .find(|(_, i)| matches!(i, Intent::PathCommit(_)))
            .unwrap()
            .0;
        s.git_workspace.hits = Some(f.hits);
        s.width += 1;
        s.git_workspace_click(r.left + 1, r.top + 1);
        assert!(s.git_workspace.history_commit.is_none());
        let blob_path = root
            .join(".git/objects")
            .join(&root_blob[..2])
            .join(&root_blob[2..]);
        let mut permissions = fs::metadata(&blob_path).unwrap().permissions();
        // This module/test only runs on Windows: clear its readonly bit.
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&blob_path, permissions).unwrap();
        fs::remove_file(blob_path).unwrap();
        s.open_git_path_commit(1);
        let deadline = Instant::now() + Duration::from_secs(8);
        while matches!(
            s.git_revision.state(),
            jc_app::git_revision::GitRevisionState::Queued
                | jc_app::git_revision::GitRevisionState::Running
        ) {
            s.poll_git();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            s.git_revision.state(),
            jc_app::git_revision::GitRevisionState::Failed
        );
        assert!(s
            .prepare_git_workspace()
            .unwrap()
            .details
            .iter()
            .any(|line| line == s.git_revision.message()));
        assert_eq!(s.workbench.active_bytes().unwrap(), before);
        assert_eq!(format!("{:?}", s.workbench.panes()), panes);
        assert_eq!(fs::read(&file).unwrap(), before);
        drop(s);
        assert!(root
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("gui-path-history-"));
        fn writable_objects(path: &std::path::Path) {
            for entry in fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                let meta = fs::symlink_metadata(&path).unwrap();
                assert!(!meta.file_type().is_symlink());
                if meta.is_dir() {
                    writable_objects(&path);
                } else if meta.permissions().readonly() {
                    let mut permissions = meta.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    // Windows-only fixture leaves.
                    permissions.set_readonly(false);
                    fs::set_permissions(path, permissions).unwrap();
                }
            }
        }
        // Git marks loose fixture objects readonly on Windows. Make only these
        // owned leaves writable before deleting the already checked fixture.
        writable_objects(&root);
        fs::remove_dir_all(root).unwrap();
    }
}

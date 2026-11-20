//! Native, read-only line attribution over an accepted immutable Git snapshot.
use super::*;

impl State {
    pub(super) fn add_git_blame_button(&self, f: &mut Frame) {
        if self.git_workspace.commit_search.is_some() || self.git_workspace.blame_query.is_some() {
            return;
        }
        f.footer = format!("L anotaciones · {}", f.footer);
        let path_mode = self.git_workspace.path_query.is_some();
        let minimum = if path_mode {
            500
        } else if f.historical {
            550
        } else {
            440
        };
        if f.preview.right - f.preview.left < self.dpi.pixels(minimum) {
            return;
        }
        let offset = if path_mode { 120 } else { 264 };
        let r = rect(
            f.preview.right - self.dpi.pixels(offset),
            f.preview.top + self.dpi.pixels(2),
            f.preview.right - self.dpi.pixels(offset - 108),
            f.preview.top + self.dpi.pixels(28),
        );
        f.buttons.push((r, "Anotaciones".into(), false));
        f.hits.actions.push((r, Intent::Blame));
    }
    pub(super) fn request_git_blame(&mut self) {
        let Some((start, path)) = self.git_selected_history_target() else {
            self.git_workspace.notice =
                Some("Selecciona un archivo de un repositorio con commits".into());
            self.git_workspace.invalidate();
            return;
        };
        self.git_workspace.blame_query = Some((start, path, 0));
        self.refresh_git_blame();
    }
    pub(super) fn refresh_git_blame(&mut self) {
        let Some((start, path, first)) = self.git_workspace.blame_query.clone() else {
            return;
        };
        let Some(root) = self.git_root().map(std::path::Path::to_owned) else {
            return;
        };
        self.git_workspace.blame_comparing = false;
        self.git_workspace.blame_selected = 0;
        self.git_workspace.history_commit = None;
        self.git_workspace.history_file = None;
        self.git_workspace.local_kind = None;
        self.git_workspace.commit = None;
        self.git_workspace.focus_diff = true;
        self.git_workspace.active = Column::Comparison;
        self.git_workspace.diff_first = 0;
        self.git_workspace.diff_column = 0;
        self.git_workspace.notice = None;
        self.git_workspace.history_encoding = self.git_workspace.history_encoding.or(self.encoding);
        self.git_diff.clear(&mut self.jobs);
        self.git_workspace.invalidate();
        if let Err(e) = self.git_revision.request_blame(root, start, path, first) {
            self.git_workspace.notice = Some(e);
        }
    }
    pub(super) fn page_git_blame(&mut self, delta: isize) {
        let Some(page) = self.git_revision.blame() else {
            return;
        };
        let first = page.first.saturating_add_signed(delta.saturating_mul(128));
        if first >= page.total_lines && delta > 0 {
            return;
        }
        if let Some(query) = &mut self.git_workspace.blame_query {
            query.2 = first;
        }
        self.refresh_git_blame();
    }
    pub(super) fn open_git_blame_line(&mut self, index: usize) {
        let Some((id, path)) = self.git_revision.blame().and_then(|p| {
            p.rows
                .get(index)
                .and_then(|r| r.commit)
                .and_then(|i| p.commits.get(i))
                .map(|c| (c.commit.clone(), p.path.clone()))
        }) else {
            self.git_workspace.notice =
                Some("Línea sin atribuir: revisa el límite indicado en las anotaciones".into());
            self.git_workspace.invalidate();
            return;
        };
        let Some(root) = self.git_root().map(std::path::Path::to_owned) else {
            return;
        };
        self.git_workspace.blame_viewport = (
            self.git_workspace.diff_first,
            self.git_workspace.diff_column,
        );
        self.git_workspace.blame_selected = index;
        self.git_workspace.blame_comparing = true;
        self.git_workspace.active = Column::Comparison;
        self.git_workspace.focus_diff = true;
        self.git_workspace.history_commit = Some(id.clone());
        self.git_workspace.history_file = Some(0);
        self.git_workspace.history_hunk = 0;
        self.git_workspace.diff_first = 0;
        self.git_workspace.diff_column = 0;
        self.git_workspace.invalidate();
        if let Err(e) = self.git_revision.request_file(root, id, path) {
            self.git_workspace.notice = Some(e);
        }
    }
    pub(super) fn back_git_blame(&mut self) {
        self.git_workspace.blame_comparing = false;
        self.git_workspace.active = Column::Comparison;
        self.git_workspace.focus_diff = true;
        self.git_workspace.history_commit = None;
        self.git_workspace.history_file = None;
        (
            self.git_workspace.diff_first,
            self.git_workspace.diff_column,
        ) = self.git_workspace.blame_viewport;
        self.git_workspace.invalidate();
    }
    pub(super) fn git_blame_key(&mut self, code: usize) -> bool {
        if self.git_workspace.blame_query.is_none() {
            if code == 0x4c {
                self.request_git_blame();
                return true;
            }
            return false;
        }
        match code {
            0x1b | 0x42 if self.git_workspace.blame_comparing => {
                self.back_git_blame();
            }
            0x1b | 0x42 => {
                self.request_git(GitIntent::Status);
            }
            0x52 | 0x74 => self.refresh_git_blame(),
            0x4e | 0x22 => self.page_git_blame(1),
            0x50 | 0x21 => self.page_git_blame(-1),
            0x45 => {
                self.git_workspace.history_encoding =
                    Some(match self.git_workspace.history_encoding {
                        None | Some(TextEncoding::Ibm850) => TextEncoding::Utf8,
                        Some(TextEncoding::Utf8) => TextEncoding::Windows1252,
                        Some(TextEncoding::Windows1252) => TextEncoding::Iso8859_1,
                        Some(TextEncoding::Iso8859_1) => TextEncoding::Ibm850,
                    });
                self.git_workspace.invalidate();
            }
            0x0d if !self.git_workspace.blame_comparing => {
                self.open_git_blame_line(self.git_workspace.blame_selected)
            }
            0x26 | 0x28 | 0x24 | 0x23 if !self.git_workspace.blame_comparing => {
                let total = self.git_revision.blame().map_or(0, |p| p.rows.len());
                let delta = match code {
                    0x26 => -1,
                    0x28 => 1,
                    0x24 => isize::MIN,
                    _ => isize::MAX,
                };
                self.git_workspace.blame_selected = self
                    .git_workspace
                    .blame_selected
                    .saturating_add_signed(delta)
                    .min(total.saturating_sub(1));
                let selected = self.git_workspace.blame_selected;
                let capacity = self.git_workspace.diff_page.max(1);
                self.git_workspace.diff_first = self
                    .git_workspace
                    .diff_first
                    .clamp(selected.saturating_sub(capacity - 1), selected);
                self.git_workspace.invalidate();
            }
            _ => return false,
        }
        true
    }
    pub(super) fn prepare_git_blame(&mut self, frame: Frame) -> Frame {
        let comparing = self.git_workspace.blame_comparing;
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
        f.subtitle = String::new();
        f.history_heading = "AUTORÍA";
        f.buttons.retain(|(r, _, _)| {
            r.top < l.toolbar.bottom || (comparing && r.top >= l.preview.top + p(128))
        });
        f.hits.actions.retain(|(_, a)| {
            matches!(
                a,
                Intent::Refresh | Intent::Close | Intent::Hunk(_) | Intent::HistoryEncoding
            )
        });
        let mut button = |x: i32, width: i32, name: &str, intent: Intent| {
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
        if comparing {
            button(8, 154, "Volver a anotaciones", Intent::BlameBack);
        } else {
            button(8, 104, "Anterior", Intent::BlamePage(-1));
            button(116, 104, "Siguiente", Intent::BlamePage(1));
        }
        button(228, 84, "Estado", Intent::BlameExit);
        let (_, path, _) = self
            .git_workspace
            .blame_query
            .as_ref()
            .expect("annotation view");
        let path = opaque(path, 256);
        let Some(page) = self.git_revision.blame() else {
            f.summary = vec![
                format!("Anotaciones de {path}"),
                self.git_revision.message().into(),
            ];
            f.code.clear();
            f.details.clear();
            return f;
        };
        let selected = self
            .git_workspace
            .blame_selected
            .min(page.rows.len().saturating_sub(1));
        self.git_workspace.blame_selected = selected;
        let owner = page.rows.get(selected).and_then(|r| r.commit);
        f.summary = if comparing {
            self.git_workspace
                .history_commit
                .as_ref()
                .and_then(|id| page.commits.iter().find(|c| &c.commit == id))
                .map(|c| {
                    vec![
                        opaque(&c.summary, 160),
                        format!(
                            "{} · {}",
                            opaque(&c.author, 80),
                            &c.commit[..c.commit.len().min(8)]
                        ),
                    ]
                })
                .unwrap_or_default()
        } else {
            vec![
                format!("Anotaciones de {path}"),
                format!(
                    "Desde {} · primer padre · {}{}",
                    &page.start[..page.start.len().min(8)],
                    page.boundary.label(),
                    if page.metadata_limited {
                        " · metadatos recortados"
                    } else {
                        ""
                    }
                ),
            ]
        };
        f.count = "1 archivo".into();
        if let Some(area) = l.history {
            let capacity =
                ((area.bottom - area.top - l.head - p(56)) / p(64)).clamp(0, 128) as usize;
            let first = owner
                .unwrap_or(0)
                .saturating_sub(capacity.saturating_sub(1))
                .min(page.commits.len().saturating_sub(capacity));
            for (index, c) in page.commits.iter().enumerate().skip(first).take(capacity) {
                let top = area.top + l.head + p(56) + (index - first) as i32 * p(64);
                let r = rect(area.left, top, area.right, (top + p(64)).min(area.bottom));
                f.commits.push(CommitRow {
                    rect: r,
                    title: opaque(&c.author, 96),
                    detail: format!(
                        "{} · {}",
                        &c.commit[..c.commit.len().min(8)],
                        opaque(&c.summary, 100)
                    ),
                    selected: owner == Some(index),
                    merge: false,
                });
                if let Some(row) = page.rows.iter().position(|r| r.commit == Some(index)) {
                    f.hits.actions.push((r, Intent::BlameLine(row)));
                }
            }
            f.hits.actions.push((
                rect(
                    area.left,
                    area.top + l.head,
                    area.right,
                    (area.top + l.head + p(56)).min(area.bottom),
                ),
                Intent::BlameExit,
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
                detail: "Versión confirmada · solo lectura".into(),
                tag: "L",
                color: ACCENT,
                selected: true,
            });
        }
        if !comparing {
            f.code.clear();
            f.details.clear();
            let row_top = l.preview.top + p(164);
            let capacity = ((l.preview.bottom - p(14) - row_top) / self.cell_height.max(1))
                .clamp(0, 128) as usize;
            let first = self
                .git_workspace
                .diff_first
                .min(page.rows.len().saturating_sub(capacity));
            self.git_workspace.diff_first = first;
            self.git_workspace.diff_page = capacity;
            self.git_workspace.diff_total = page.rows.len();
            let gutter = p(if l.preview.right - l.preview.left >= p(600) {
                260
            } else {
                148
            });
            let columns = ((l.preview.right - l.preview.left - gutter - p(18))
                / self.cell_width.max(1))
            .max(1) as usize;
            self.git_workspace.diff_columns = columns;
            let encoding = self.git_workspace.history_encoding;
            let mut extent = 0;
            if let Some(encoding) = encoding {
                for (index, row) in page.rows.iter().enumerate().skip(first).take(capacity) {
                    let top = row_top + (index - first) as i32 * self.cell_height;
                    let (text, width) = compare_rows::text(
                        &page.source[row.range.clone()],
                        encoding,
                        self.git_workspace.diff_column,
                    );
                    extent = extent.max(width);
                    let commit = row.commit.and_then(|i| page.commits.get(i));
                    f.annotations.push(BlameRow {
                        rect: rect(
                            l.preview.left,
                            top,
                            l.preview.right - p(12),
                            top + self.cell_height,
                        ),
                        code_left: l.preview.left + gutter,
                        number: (row.line + 1).to_string(),
                        origin: commit.map_or("Pendiente".into(), |c| {
                            c.commit[..c.commit.len().min(8)].into()
                        }),
                        author: commit.map_or("Sin atribuir".into(), |c| opaque(&c.author, 96)),
                        text,
                        selected: index == selected,
                        unresolved: row.commit.is_none(),
                    });
                    f.hits.actions.push((
                        rect(
                            l.preview.left,
                            top,
                            l.preview.left + gutter,
                            top + self.cell_height,
                        ),
                        Intent::BlameLine(index),
                    ));
                }
            } else {
                f.details
                    .push("Elige la codificación con E para interpretar los bytes".into());
            }
            self.git_workspace.diff_extent = extent;
            let normalized = self
                .git_workspace
                .diff_column
                .min(extent.saturating_sub(columns));
            if normalized != self.git_workspace.diff_column {
                self.git_workspace.diff_column = normalized;
                if let Some(encoding) = encoding {
                    for (annotation, row) in
                        f.annotations.iter_mut().zip(page.rows.iter().skip(first))
                    {
                        annotation.text = compare_rows::text(
                            &page.source[row.range.clone()],
                            encoding,
                            normalized,
                        )
                        .0;
                    }
                }
            }
            if page.total_lines == 0 {
                f.details.push("Archivo confirmado vacío".into());
            }
            let codec = rect(
                l.preview.left + p(12),
                l.preview.top + p(128),
                l.preview.left + p(200),
                l.preview.top + p(152),
            );
            if codec.right < l.preview.right {
                f.buttons.push((
                    codec,
                    encoding.map_or("Codificación: elegir".into(), |e| e.name().into()),
                    false,
                ));
                f.hits.actions.push((codec, Intent::HistoryEncoding));
            }
        } else if self.git_revision.comparison().is_none() {
            f.details.push(self.git_revision.message().into());
        }
        if let Some(message) = &self.git_workspace.notice {
            f.details.push(label(message, 256));
        }
        f.footer = format!(
            "Líneas {}–{} de {} · {} · N/P páginas · E codificación · Intro commit · Esc vuelve",
            page.first + usize::from(!page.rows.is_empty()),
            page.first + page.rows.len(),
            page.total_lines,
            page.boundary.label()
        );
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
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            s.git_revision.state(),
            jc_app::git_revision::GitRevisionState::Ready,
            "{}",
            s.git_revision.message()
        );
    }
    fn click(s: &mut State, test: impl Fn(Intent) -> bool) {
        let f = s.prepare_git_workspace().unwrap();
        let r = f.hits.actions.iter().find(|(_, i)| test(*i)).unwrap().0;
        s.git_workspace.hits = Some(f.hits);
        s.git_workspace_click((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    }
    #[test]
    fn blame_gui_pages_owning_commit_back_codec_and_stale_hits_leave_dirty_editor_intact() {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work/test-temp");
        fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        let root = base.join(format!(
            "gui-blame-{}-{}",
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
            assert!(output.status.success());
        };
        git(&["init", "--quiet", "--template=", "--initial-branch=main"]);
        let file = root.join("archivo.p");
        let mut original = (0..129)
            .flat_map(|n| format!("linea {n}\r\n").into_bytes())
            .collect::<Vec<_>>();
        original.extend_from_slice(b"final\xff\r\n");
        fs::write(&file, &original).unwrap();
        git(&["add", "--", "archivo.p"]);
        git(&[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "Origen",
        ]);
        let mut latest = b"cambio\r\n".to_vec();
        latest.extend_from_slice(&original[b"linea 0\r\n".len()..]);
        fs::write(&file, &latest).unwrap();
        git(&["add", "--", "archivo.p"]);
        git(&[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "Cambio",
        ]);
        let mut s = State::new(&crate::GuiOptions {
            project_root: Some(root.clone()),
            encoding: Some(TextEncoding::Utf8),
            ..Default::default()
        })
        .unwrap();
        s.width = 1280;
        s.height = 800;
        s.workbench
            .open_project_file(&root, file.clone(), Some(TextEncoding::Utf8), false, 4096)
            .unwrap();
        s.workbench.insert_text("sin guardar\n").unwrap();
        let before = s.workbench.active_bytes().unwrap();
        let panes = format!("{:?}", s.workbench.panes());
        s.request_git(GitIntent::Status);
        let deadline = Instant::now() + Duration::from_secs(8);
        while s.git_status.state() != GitStatusTaskState::Ready {
            s.poll_git();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        click(&mut s, |i| matches!(i, Intent::Blame));
        wait(&mut s);
        let page = s.git_revision.blame().unwrap();
        assert_eq!(page.rows.len(), 128);
        assert_eq!(page.source.as_slice(), latest);
        assert_ne!(page.rows[0].commit, page.rows[1].commit);
        let jobs = s.jobs.outstanding();
        assert!(!s.prepare_git_workspace().unwrap().annotations.is_empty());
        assert_eq!(s.jobs.outstanding(), jobs);
        click(&mut s, |i| matches!(i, Intent::BlamePage(1)));
        wait(&mut s);
        assert_eq!(s.git_revision.blame().unwrap().first, 128);
        let f = s.prepare_git_workspace().unwrap();
        assert_eq!(f.annotations.len(), 2);
        assert!(f.annotations[1].text.contains("\\xFF"));
        click(&mut s, |i| matches!(i, Intent::BlameLine(0)));
        wait(&mut s);
        assert!(s.git_revision.comparison().unwrap().before_oid.is_none());
        assert_eq!(
            s.git_revision
                .comparison()
                .unwrap()
                .model
                .source_bytes(DiffSide::Right),
            original
        );
        s.git_workspace.active = Column::History;
        s.git_workspace.focus_diff = false;
        click(&mut s, |i| matches!(i, Intent::BlameBack));
        assert_eq!(s.git_workspace.active, Column::Comparison);
        assert!(s.git_workspace.focus_diff);
        assert_eq!(
            s.prepare_git_workspace().unwrap().annotations[0].number,
            "129"
        );
        assert!(s.git_blame_key(0x45));
        assert_eq!(
            s.git_workspace.history_encoding,
            Some(TextEncoding::Windows1252)
        );
        s.git_key(0x54, jc_app::Modifiers::NONE); // T must not enter a local pair behind annotations.
        assert!(s.git_workspace.local_kind.is_none());
        let f = s.prepare_git_workspace().unwrap();
        let r = f
            .hits
            .actions
            .iter()
            .find(|(_, i)| matches!(i, Intent::BlameLine(_)))
            .unwrap()
            .0;
        s.git_workspace.hits = Some(f.hits);
        s.width += 1;
        s.git_workspace_click(r.left + 1, r.top + 1);
        assert!(!s.git_workspace.blame_comparing);
        assert_eq!(s.workbench.active_bytes().unwrap(), before);
        assert_eq!(format!("{:?}", s.workbench.panes()), panes);
        assert_eq!(fs::read(&file).unwrap(), latest);
        drop(s);
        fn writable(path: &std::path::Path) {
            for e in fs::read_dir(path).unwrap() {
                let p = e.unwrap().path();
                let m = fs::symlink_metadata(&p).unwrap();
                assert!(!m.file_type().is_symlink());
                if m.is_dir() {
                    writable(&p);
                } else if m.permissions().readonly() {
                    let mut perms = m.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    // Windows-only owned Git objects.
                    perms.set_readonly(false);
                    fs::set_permissions(p, perms).unwrap();
                }
            }
        }
        assert_eq!(root.parent(), Some(base.as_path()));
        assert!(root
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("gui-blame-"));
        writable(&root);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn blame_buttons_do_not_overlap_history_controls_at_compact_widths() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.git_panel = Some(GitIntent::Status);
        s.height = 800;
        for historical in [false, true] {
            s.git_workspace.history_commit = historical.then(|| "a".repeat(40));
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
    }
}

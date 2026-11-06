//! Native pixel workspace over accepted, immutable Git observations.
use super::{ffi::*, git_panel::GitIntent, paint, shell, wide, Action, State};
use jc_app::{
    diff_model::{DiffLine, DiffSide},
    git_read::GitBlobSource,
    git_status::{GitStatusChange, GitStatusState},
    git_status_tasks::GitStatusTaskState,
};
use jc_text::{DecodedScalars, TextEncoding};
use std::path::PathBuf;

#[path = "git_history.rs"]
mod history;
#[path = "git_local.rs"]
mod local;

const ADD: u32 = 0x00364327;
const REMOVE: u32 = 0x003c304b;
const GREEN: u32 = 0x009dd8a3;
const RED: u32 = 0x009fa2ef;

#[derive(Default)]
pub(super) struct Workspace {
    pub hits: Option<Hits>,
    pub epoch: u64,
    pub commit: Option<usize>,
    pub diff_first: usize,
    pub diff_total: usize,
    pub diff_page: usize,
    pub notice: Option<String>,
    pub selected_path: Option<Vec<u8>>,
    pub focus_diff: bool,
    pub history_commit: Option<String>,
    pub local_kind: Option<jc_app::git_revision::PairKind>,
    pub history_selected: usize,
    pub history_first: usize,
    pub history_file: Option<usize>,
    pub history_hunk: usize,
    pub history_encoding: Option<TextEncoding>,
    wheel: i32,
}
impl Workspace {
    pub fn invalidate(&mut self) {
        self.epoch = self.epoch.saturating_add(1);
        self.hits = None;
    }
}
#[derive(Clone, PartialEq, Eq)]
struct HistoryStamp {
    commit: String,
    selected: usize,
    file: Option<usize>,
    hunk: usize,
    encoding: Option<TextEncoding>,
}
#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    root: Option<PathBuf>,
    epoch: u64,
    geometry: (i32, i32),
    document: Option<(jc_app::DocumentId, u64, Option<TextEncoding>)>,
    selected: usize,
    commit: Option<usize>,
    first: usize,
    history: Option<HistoryStamp>,
    local: Option<jc_app::git_revision::PairKind>,
}
#[derive(Clone, Copy)]
enum Intent {
    Refresh,
    Close,
    Folder,
    File(usize),
    Commit(usize),
    Source(GitBlobSource),
    Pair(jc_app::git_revision::PairKind),
    Hunk(isize),
    Navigate,
    Local,
    HistoryFile(usize),
    HistorySummary,
    HistoryEncoding,
}
#[derive(Clone)]
pub(super) struct Hits {
    stamp: Stamp,
    actions: Vec<(Rect, Intent)>,
}
struct FileRow {
    rect: Rect,
    name: String,
    detail: String,
    tag: &'static str,
    color: u32,
    selected: bool,
}
struct CommitRow {
    rect: Rect,
    title: String,
    detail: String,
    selected: bool,
    merge: bool,
}
struct CodeRow {
    rect: Rect,
    old: String,
    new: String,
    text: String,
    kind: i8,
}
pub(super) struct Frame {
    pub hits: Hits,
    rect: Rect,
    toolbar: Rect,
    history: Option<Rect>,
    files: Rect,
    preview: Rect,
    title: String,
    branch: String,
    count: String,
    historical: bool,
    details_top: i32,
    message: String,
    subtitle: String,
    buttons: Vec<(Rect, String, bool)>,
    commits: Vec<CommitRow>,
    rows: Vec<FileRow>,
    code: Vec<CodeRow>,
    details: Vec<String>,
    footer: String,
}
#[derive(Clone, Copy)]
struct Layout {
    rect: Rect,
    toolbar: Rect,
    history: Option<Rect>,
    files: Rect,
    preview: Rect,
    row: i32,
    head: i32,
}
fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        right: right.max(left),
        bottom: bottom.max(top),
    }
}
fn inside(r: Rect, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}
fn label(s: &str, n: usize) -> String {
    jc_app::source_services::display_ui_label(s, n.min(512))
}
fn opaque(bytes: &[u8], n: usize) -> String {
    let mut out = String::new();
    for scalar in
        DecodedScalars::new(&bytes[..bytes.len().min(1024)], TextEncoding::Utf8, 0).take(n.min(256))
    {
        if scalar.valid {
            out.push(scalar.character);
        } else {
            for b in &bytes[scalar.source_range] {
                out.push_str(&format!("\\x{b:02X}"));
            }
        }
    }
    label(&out, n)
}
impl Layout {
    fn new(s: &State) -> Self {
        let w = s.width.clamp(1, 8192);
        let h = s.height.clamp(1, 8192);
        let p = |n| s.dpi.pixels(n);
        let top = s.chrome_layout().top().min(h);
        let bottom = (h - s.cell_height - 5).max(top);
        let content_bottom = (bottom - p(24)).max(top);
        let bar = (top + p(48)).min(bottom);
        let history = (w >= p(760)).then(|| rect(0, bar, p(200).min(w / 4), content_bottom));
        let left = history.map_or(0, |r| r.right);
        let file_width = p(220).min((w - left) / 2).max(0);
        Self {
            rect: rect(0, top, w, bottom),
            toolbar: rect(0, top, w, bar),
            history,
            files: rect(left, bar, left + file_width, content_bottom),
            preview: rect(left + file_width, bar, w, content_bottom),
            row: p(46),
            head: p(44),
        }
    }
}
fn status_tag(state: GitStatusState) -> (&'static str, u32, &'static str) {
    match state {
        GitStatusState::Untracked => ("?", GREEN, "Sin seguimiento"),
        GitStatusState::Ignored => ("·", shell::MUTED, "Ignorado"),
        GitStatusState::Conflict(_) => ("!", RED, "Conflicto"),
        GitStatusState::Tracked { index, worktree } => {
            let (tag, color) = match if worktree != GitStatusChange::Unchanged {
                worktree
            } else {
                index
            } {
                GitStatusChange::Added => ("A", GREEN),
                GitStatusChange::Deleted => ("D", RED),
                GitStatusChange::Renamed | GitStatusChange::Copied => ("R", shell::ACCENT),
                _ => ("M", shell::ACCENT),
            };
            (
                tag,
                color,
                if index == GitStatusChange::Unchanged {
                    "Sin preparar"
                } else if worktree == GitStatusChange::Unchanged {
                    "Preparado"
                } else {
                    "Preparado y modificado"
                },
            )
        }
    }
}
fn source_text(bytes: &[u8], encoding: TextEncoding, start: usize) -> String {
    let mut text = String::new();
    let mut column = 0;
    for scalar in DecodedScalars::new(&bytes[..bytes.len().min(2048)], encoding, start).take(512) {
        if !scalar.valid {
            let a = scalar.source_range.start - start;
            let b = scalar.source_range.end - start;
            for byte in &bytes[a..b] {
                text.push_str(&format!("\\x{byte:02X}"));
                column += 4;
            }
        } else if scalar.character == '\t' {
            let width = 4 - column % 4;
            text.extend(std::iter::repeat_n(' ', width));
            column += width;
        } else if scalar.character.is_control() {
            text.push_str(&format!("\\u{{{:X}}}", scalar.character as u32));
        } else {
            text.push(scalar.character);
            column += 1;
        }
        if text.len() > 4096 {
            break;
        }
    }
    text
}
impl State {
    fn git_workspace_stamp(&self) -> Stamp {
        Stamp {
            local: self.git_workspace.local_kind,
            root: self.project_root.clone(),
            epoch: self.git_workspace.epoch,
            geometry: (self.width, self.height),
            document: self
                .workbench
                .active_document()
                .map(|d| (d.id, d.session.document().revision(), d.session.encoding())),
            selected: self.git_view.selected_status(),
            commit: self.git_workspace.commit,
            first: self.git_workspace.diff_first,
            history: self
                .git_workspace
                .history_commit
                .as_ref()
                .map(|c| HistoryStamp {
                    commit: c.clone(),
                    selected: self.git_workspace.history_selected,
                    file: self.git_workspace.history_file,
                    hunk: self.git_workspace.history_hunk,
                    encoding: self.git_workspace.history_encoding,
                }),
        }
    }
    pub(super) fn prepare_git_workspace(&mut self) -> Option<Frame> {
        self.validate_git_context();
        self.git_panel?;
        let historical = self.git_workspace.history_commit.is_some();
        let frame = self.prepare_git_workspace_base()?;
        Some(if historical {
            self.prepare_git_history(frame)
        } else if self.git_workspace.local_kind.is_some() {
            self.prepare_git_local(frame)
        } else {
            let mut frame = frame;
            self.add_git_sources(&mut frame);
            frame
        })
    }
    fn prepare_git_workspace_base(&mut self) -> Option<Frame> {
        self.git_panel?;
        let layout = Layout::new(self);
        let p = |n| self.dpi.pixels(n);
        let mut actions = Vec::new();
        let mut buttons = Vec::new();
        let right = layout.toolbar.right;
        for (r, text, intent, selected) in [
            (
                rect(
                    (right - p(252)).max(0),
                    layout.toolbar.top + p(8),
                    (right - p(148)).max(0),
                    layout.toolbar.bottom - p(8),
                ),
                "Actualizar",
                Intent::Refresh,
                false,
            ),
            (
                rect(
                    (right - p(140)).max(0),
                    layout.toolbar.top + p(8),
                    right - p(8),
                    layout.toolbar.bottom - p(8),
                ),
                "Volver al código",
                Intent::Close,
                false,
            ),
        ] {
            actions.push((r, intent));
            buttons.push((r, text.to_owned(), selected));
        }
        let title = self
            .project_root
            .as_ref()
            .and_then(|r| r.file_name())
            .and_then(|n| n.to_str())
            .map_or_else(|| "Repositorio Git".into(), |n| label(n, 80));
        let ready = self.git_status.state() == GitStatusTaskState::Ready;
        let snapshot = if ready {
            self.git_status.result()
        } else {
            None
        };
        let branch = snapshot
            .and_then(|s| s.info())
            .and_then(|i| i.branch.as_deref())
            .map(|b| opaque(b, 80))
            .unwrap_or_else(|| "Git local".into());
        let entries = snapshot.map_or(&[][..], |s| s.status().entries());
        let capacity = ((layout.files.bottom - layout.files.top - layout.head) / layout.row.max(1))
            .clamp(0, 128) as usize;
        self.git_view
            .prepare_status(&self.git_status, capacity, 256);
        let selected = self.git_view.selected_status();
        let first = selected
            .saturating_sub(capacity.saturating_sub(1))
            .min(entries.len().saturating_sub(capacity));
        let mut rows = Vec::new();
        for (index, e) in entries.iter().enumerate().skip(first).take(capacity) {
            let top = layout.files.top + layout.head + (index - first) as i32 * layout.row;
            let r = rect(
                layout.files.left,
                top,
                layout.files.right,
                (top + layout.row).min(layout.files.bottom),
            );
            let path = opaque(e.path(), 256);
            let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
            let (tag, color, description) = status_tag(e.state());
            rows.push(FileRow {
                rect: r,
                name,
                detail: format!("{description} · {path}"),
                tag,
                color,
                selected: index == selected,
            });
            actions.push((r, Intent::File(index)));
        }
        let mut commits = Vec::new();
        let mut details = Vec::new();
        if let Some(info) = snapshot.and_then(|s| s.info()) {
            if self
                .git_workspace
                .commit
                .is_some_and(|i| i >= info.commits.len())
            {
                self.git_workspace.commit = None;
            }
            if let Some(area) = layout.history {
                let count =
                    ((area.bottom - area.top - layout.head) / p(53).max(1)).clamp(0, 24) as usize;
                let chosen = self
                    .git_workspace
                    .commit
                    .unwrap_or(0)
                    .min(info.commits.len().saturating_sub(1));
                let _ = chosen;
                let start = self
                    .git_workspace
                    .history_first
                    .min(info.commits.len().saturating_sub(count));
                for (index, c) in info.commits.iter().enumerate().skip(start).take(count) {
                    let top = area.top + layout.head + (index - start) as i32 * p(53);
                    let r = rect(area.left, top, area.right, (top + p(53)).min(area.bottom));
                    commits.push(CommitRow {
                        rect: r,
                        title: opaque(&c.summary, 160),
                        detail: format!(
                            "{} · {}",
                            &c.id[..c.id.len().min(8)],
                            opaque(&c.author, 80)
                        ),
                        selected: self.git_workspace.commit == Some(index),
                        merge: c.parents.len() > 1,
                    });
                    actions.push((r, Intent::Commit(index)));
                }
            }
            if let Some(c) = self.git_workspace.commit.and_then(|i| info.commits.get(i)) {
                details = vec![
                    opaque(&c.summary, 256),
                    format!("Autor: {}", opaque(&c.author, 96)),
                    format!("Commit: {}", c.id),
                    format!("Padres: {}", c.parents.len()),
                    "Historial local · recorrido por el primer padre".into(),
                ];
            }
        }
        let mut message = String::new();
        if self.git_status.is_not_repository() {
            message = "Esta carpeta no es un repositorio Git".into();
        } else if matches!(
            self.git_status.state(),
            GitStatusTaskState::Queued | GitStatusTaskState::Running
        ) {
            message = "Leyendo el repositorio…".into();
        } else if !ready {
            message = label(self.git_status.message(), 256);
        } else if entries.is_empty() {
            message = "No hay cambios locales".into();
        }
        if !ready || entries.is_empty() {
            let r = rect(
                layout.files.left + p(14),
                layout.files.top + layout.head + p(62),
                layout.files.right - p(14),
                layout.files.top + layout.head + p(98),
            );
            if self.git_status.is_not_repository() {
                buttons.push((r, "Abrir carpeta".into(), false));
                actions.push((r, Intent::Folder));
            }
        }
        let mut code = Vec::new();
        let mut subtitle = "Selecciona un archivo cambiado para comparar".into();
        if let Some(notice) = &self.git_workspace.notice {
            subtitle = label(notice, 256);
        }
        let rows_visible = ((layout.preview.bottom - layout.preview.top - p(148))
            / self.cell_height.max(1))
        .clamp(0, 128) as usize;
        self.git_workspace.diff_page = rows_visible;
        self.git_workspace.diff_total = 0;
        if self.git_workspace.commit.is_none() {
            if let Some(result) = self.git_diff.result() {
                subtitle = label(&result.identity.path, 256);
                if let Some(encoding) = self
                    .workbench
                    .active_document()
                    .and_then(|d| d.session.encoding())
                {
                    if let Some(hunk) = result.model.hunks().nth(self.git_diff.selected()) {
                        let spans = result.model.spans();
                        let at = spans
                            .iter()
                            .position(|s| std::ptr::eq(s, hunk))
                            .unwrap_or(0);
                        let before = at
                            .checked_sub(1)
                            .and_then(|i| spans.get(i))
                            .map_or(0, |s| (s.left.end.0 - s.left.start.0).min(3));
                        let after = spans
                            .get(at + 1)
                            .map_or(0, |s| (s.left.end.0 - s.left.start.0).min(3));
                        let segments = [
                            (
                                0,
                                Some(hunk.left.start.0 - before),
                                Some(hunk.right.start.0 - before),
                                before,
                            ),
                            (
                                -1,
                                Some(hunk.left.start.0),
                                None,
                                hunk.left.end.0 - hunk.left.start.0,
                            ),
                            (
                                1,
                                None,
                                Some(hunk.right.start.0),
                                hunk.right.end.0 - hunk.right.start.0,
                            ),
                            (0, Some(hunk.left.end.0), Some(hunk.right.end.0), after),
                        ];
                        let total = segments.iter().map(|s| s.3).sum::<usize>();
                        self.git_workspace.diff_total = total;
                        self.git_workspace.diff_first = self
                            .git_workspace
                            .diff_first
                            .min(total.saturating_sub(rows_visible));
                        for (row, n) in (self.git_workspace.diff_first..total)
                            .take(rows_visible)
                            .enumerate()
                        {
                            let mut offset = n;
                            for &(kind, left, right, len) in &segments {
                                if offset >= len {
                                    offset -= len;
                                    continue;
                                }
                                let old = left.map(|n| n + offset);
                                let new = right.map(|n| n + offset);
                                let (side, line) = new
                                    .map_or((DiffSide::Left, old.unwrap_or(0)), |n| {
                                        (DiffSide::Right, n)
                                    });
                                if let Some(range) =
                                    result.model.line_byte_range(side, DiffLine(line))
                                {
                                    let bytes = &result.model.source_bytes(side)[range.clone()];
                                    let bytes = bytes
                                        .strip_suffix(b"\r\n")
                                        .or_else(|| bytes.strip_suffix(b"\n"))
                                        .or_else(|| bytes.strip_suffix(b"\r"))
                                        .unwrap_or(bytes);
                                    let top =
                                        layout.preview.top + p(148) + row as i32 * self.cell_height;
                                    code.push(CodeRow {
                                        rect: rect(
                                            layout.preview.left,
                                            top,
                                            layout.preview.right,
                                            (top + self.cell_height).min(layout.preview.bottom),
                                        ),
                                        old: old.map_or(String::new(), |n| (n + 1).to_string()),
                                        new: new.map_or(String::new(), |n| (n + 1).to_string()),
                                        text: source_text(bytes, encoding, range.start),
                                        kind,
                                    });
                                }
                                break;
                            }
                        }
                        let y = layout.preview.top + p(112);
                        let start = layout.preview.left + p(12);
                        for (n, text, intent) in [
                            (0, "Anterior", Intent::Hunk(-1)),
                            (1, "Siguiente", Intent::Hunk(1)),
                            (2, "Ir al código", Intent::Navigate),
                        ] {
                            let r = rect(
                                start + n * p(100),
                                y,
                                start + (n + 1) * p(100) - p(5),
                                y + p(26),
                            );
                            if r.right < layout.preview.right {
                                buttons.push((r, text.into(), false));
                                actions.push((r, intent));
                            }
                        }
                    } else {
                        subtitle = "El archivo coincide con la captura comparada".into();
                    }
                } else {
                    subtitle = "Elige la codificación del documento para comparar".into();
                }
            } else if self.git_diff_running() {
                subtitle = "Preparando comparación…".into();
            } else if self
                .git_panel
                .is_some_and(|i| matches!(i, GitIntent::Diff(_)))
                && self.git_workspace.notice.is_none()
            {
                subtitle = label(self.git_diff.message(), 256);
            }
        } else {
            subtitle = "Detalles del commit seleccionado".into();
        }
        let count = if ready {
            format!("{} archivos", entries.len())
        } else {
            "Cambios locales".into()
        };
        Some(Frame{hits:Hits{stamp:self.git_workspace_stamp(),actions},rect:layout.rect,toolbar:layout.toolbar,history:layout.history,files:layout.files,preview:layout.preview,title,branch,count,historical:false,details_top:p(148),message,subtitle,buttons,commits,rows,code,details,footer:"Solo lectura · P preparados · T disco · comparaciones con buffer explícitas · Esc vuelve".into()})
    }
    pub(super) fn git_workspace_click(&mut self, x: i32, y: i32) -> Action {
        let Some(hits) = self
            .git_workspace
            .hits
            .as_ref()
            .filter(|h| h.stamp == self.git_workspace_stamp())
        else {
            return Action::None;
        };
        let intent = hits
            .actions
            .iter()
            .rev()
            .find(|(r, _)| inside(*r, x, y))
            .map(|(_, a)| *a);
        match intent {
            Some(Intent::Refresh) => {
                if let Some(kind) = self.git_workspace.local_kind {
                    self.request_git_local(kind);
                    Action::None
                } else if let Some(commit) = self.git_workspace.history_commit.clone() {
                    self.request_git_history(commit);
                    Action::None
                } else {
                    self.request_git(self.git_panel.unwrap_or(GitIntent::Status))
                }
            }
            Some(Intent::Close) => {
                self.git_panel = None;
                self.git_revision.cancel();
                self.git_workspace.history_commit = None;
                self.git_workspace.invalidate();
                Action::None
            }
            Some(Intent::Folder) => Action::ChooseFolder,
            Some(Intent::File(index)) => {
                self.git_view.select_status(index);
                self.compare_git_status_file();
                Action::None
            }
            Some(Intent::Commit(index)) => {
                self.git_workspace.commit = Some(index);
                if let Some(commit) = self
                    .git_status
                    .result()
                    .and_then(|s| s.info())
                    .and_then(|i| i.commits.get(index))
                    .map(|c| c.id.clone())
                {
                    self.request_git_history(commit);
                }
                Action::None
            }
            Some(Intent::Pair(kind)) => {
                self.request_git_local(kind);
                Action::None
            }
            Some(Intent::Source(source)) => {
                self.git_workspace.commit = None;
                self.request_git(GitIntent::Diff(source))
            }
            Some(Intent::Hunk(delta)) => {
                if self.git_workspace.history_commit.is_some()
                    || self.git_workspace.local_kind.is_some()
                {
                    self.git_workspace.history_hunk =
                        self.git_workspace.history_hunk.saturating_add_signed(delta);
                } else {
                    self.git_diff.select_relative(delta);
                }
                self.git_workspace.diff_first = 0;
                self.git_workspace.invalidate();
                Action::None
            }
            Some(Intent::Navigate) => {
                let index = self.git_diff.selected();
                match self.git_diff.select_hunk(index, &mut self.workbench) {
                    Ok(()) => self.git_panel = None,
                    Err(e) => self.git_workspace.notice = Some(e.to_string()),
                };
                Action::None
            }
            Some(Intent::Local) => self.request_git(GitIntent::Status),
            Some(Intent::HistoryFile(index)) => {
                self.request_git_history_file(index);
                Action::None
            }
            Some(Intent::HistorySummary) => {
                self.git_workspace.history_file = None;
                self.git_workspace.invalidate();
                Action::None
            }
            Some(Intent::HistoryEncoding) => {
                self.git_workspace.history_encoding =
                    Some(match self.git_workspace.history_encoding {
                        None | Some(TextEncoding::Ibm850) => TextEncoding::Utf8,
                        Some(TextEncoding::Utf8) => TextEncoding::Windows1252,
                        Some(TextEncoding::Windows1252) => TextEncoding::Iso8859_1,
                        Some(TextEncoding::Iso8859_1) => TextEncoding::Ibm850,
                    });
                self.git_workspace.invalidate();
                Action::None
            }
            None => Action::None,
        }
    }
    pub(super) fn compare_git_status_file(&mut self) {
        if let Some(kind) = self.git_workspace.local_kind {
            self.request_git_local(kind);
            return;
        }
        let selected = self.git_view.selected_status();
        let path = if self.git_status.state() == GitStatusTaskState::Ready {
            self.git_status
                .result()
                .and_then(|r| r.status().entries().get(selected))
                .map(|e| e.path().to_vec())
        } else {
            None
        };
        let Some(path) = path else {
            return;
        };
        self.git_workspace.commit = None;
        self.git_workspace.diff_first = 0;
        self.git_workspace.focus_diff = true;
        self.git_workspace.selected_path = Some(path.clone());
        self.git_workspace.notice = None;
        self.git_workspace.invalidate();
        self.git_diff.clear(&mut self.jobs);
        let result = (|| -> Result<(), String> {
            let relative = std::str::from_utf8(&path)
                .map_err(|_| "La ruta Git no se puede abrir en Windows")?;
            if relative.contains(['\\', '\0', ':'])
                || std::path::Path::new(relative)
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err("La ruta Git no es una ruta relativa válida".into());
            }
            let root = self
                .project_root
                .clone()
                .ok_or("Abre la carpeta del repositorio")?;
            self.workbench
                .open_project_file(
                    &root,
                    root.join(relative),
                    self.encoding,
                    false,
                    16 * 1024 * 1024,
                )
                .map_err(|e| e.to_string())?;
            self.ensure_encoding_selector();
            self.request_git(GitIntent::Diff(self.git_source));
            Ok(())
        })();
        if let Err(error) = result {
            self.git_workspace.notice = Some(format!("No se pudo comparar este archivo: {error}"));
        }
    }
    pub(super) fn git_workspace_wheel(&mut self, x: i32, y: i32, delta: i32) -> bool {
        let l = Layout::new(self);
        self.git_workspace.wheel = self.git_workspace.wheel.saturating_add(delta);
        let steps = -(self.git_workspace.wheel / 120) as isize * 3;
        self.git_workspace.wheel %= 120;
        if steps == 0 {
            return false;
        }
        if inside(l.preview, x, y) {
            let old = self.git_workspace.diff_first;
            self.git_workspace.diff_first = old.saturating_add_signed(steps).min(
                self.git_workspace
                    .diff_total
                    .saturating_sub(self.git_workspace.diff_page),
            );
        } else if l.history.is_some_and(|r| inside(r, x, y)) {
            let total = self
                .git_status
                .result()
                .and_then(|r| r.info())
                .map_or(0, |i| i.commits.len());
            let page = l.history.map_or(0, |r| {
                ((r.bottom - r.top - l.head) / self.dpi.pixels(53).max(1)).max(0) as usize
            });
            self.git_workspace.history_first = self
                .git_workspace
                .history_first
                .saturating_add_signed(steps)
                .min(total.saturating_sub(page));
        } else if inside(l.files, x, y) {
            if self.git_workspace.history_commit.is_some() {
                let total = self.git_revision.revision().map_or(0, |r| r.files.len());
                self.git_workspace.history_selected = self
                    .git_workspace
                    .history_selected
                    .saturating_add_signed(steps)
                    .min(total.saturating_sub(1));
            } else {
                self.git_view.move_status(steps);
            }
            self.git_workspace.focus_diff = false;
        } else {
            return false;
        }
        self.git_workspace.invalidate();
        true
    }
}

unsafe fn text(dc: Hdc, r: Rect, value: &str, fg: u32, bg: u32) -> Result<(), String> {
    if r.right <= r.left || r.bottom <= r.top {
        return Ok(());
    }
    paint::text(dc, r, &wide(value), fg, bg)
}
pub(super) unsafe fn draw(
    dc: Hdc,
    f: &Frame,
    code_font: Handle,
    ui_font: Handle,
) -> Result<(), String> {
    paint::fill(dc, f.rect, shell::BACKGROUND)?;
    paint::fill(dc, f.toolbar, shell::SURFACE)?;
    SelectObject(dc, ui_font);
    text(
        dc,
        rect(
            14,
            f.toolbar.top + 6,
            (f.toolbar.right - 266).max(14),
            f.toolbar.bottom - 22,
        ),
        &f.title,
        shell::TEXT,
        shell::SURFACE,
    )?;
    text(
        dc,
        rect(
            14,
            f.toolbar.top + 24,
            (f.toolbar.right - 266).max(14),
            f.toolbar.bottom,
        ),
        &f.branch,
        shell::ACCENT,
        shell::SURFACE,
    )?;
    if let Some(area) = f.history {
        paint::fill(dc, area, shell::SIDEBAR)?;
        text(
            dc,
            rect(area.left + 12, area.top + 10, area.right - 8, area.top + 36),
            "HISTORIAL LOCAL",
            shell::MUTED,
            shell::SIDEBAR,
        )?;
        for c in &f.commits {
            let bg = if c.selected {
                shell::SELECTED
            } else {
                shell::SIDEBAR
            };
            paint::fill(dc, c.rect, bg)?;
            paint::fill(
                dc,
                rect(
                    c.rect.left + 11,
                    c.rect.top,
                    c.rect.left + 12,
                    c.rect.bottom,
                ),
                shell::MUTED,
            )?;
            paint::fill(
                dc,
                rect(
                    c.rect.left + 8,
                    c.rect.top + 16,
                    c.rect.left + 16,
                    c.rect.top + 24,
                ),
                if c.selected {
                    shell::ACCENT
                } else {
                    shell::MUTED
                },
            )?;
            text(
                dc,
                rect(
                    c.rect.left + 24,
                    c.rect.top + 5,
                    c.rect.right - 10,
                    c.rect.top + 28,
                ),
                &c.title,
                shell::TEXT,
                bg,
            )?;
            let detail = if c.merge {
                format!("↗ {}", c.detail)
            } else {
                c.detail.clone()
            };
            text(
                dc,
                rect(
                    c.rect.left + 24,
                    c.rect.top + 27,
                    c.rect.right - 10,
                    c.rect.bottom,
                ),
                &detail,
                shell::MUTED,
                bg,
            )?;
        }
    }
    paint::fill(dc, f.files, shell::SIDEBAR)?;
    text(
        dc,
        rect(
            f.files.left + 12,
            f.files.top + 6,
            f.files.right - 10,
            f.files.top + 25,
        ),
        if f.historical {
            "ARCHIVOS DEL COMMIT"
        } else {
            "CAMBIOS LOCALES"
        },
        shell::MUTED,
        shell::SIDEBAR,
    )?;
    text(
        dc,
        rect(
            f.files.left + 12,
            f.files.top + 25,
            f.files.right - 10,
            f.files.top + 43,
        ),
        &f.count,
        shell::MUTED,
        shell::SIDEBAR,
    )?;
    for row in &f.rows {
        let bg = if row.selected {
            shell::SELECTED
        } else {
            shell::SIDEBAR
        };
        paint::fill(dc, row.rect, bg)?;
        text(
            dc,
            rect(
                row.rect.left + 10,
                row.rect.top + 6,
                row.rect.left + 30,
                row.rect.bottom - 10,
            ),
            row.tag,
            row.color,
            bg,
        )?;
        text(
            dc,
            rect(
                row.rect.left + 35,
                row.rect.top + 4,
                row.rect.right - 8,
                row.rect.top + 25,
            ),
            &row.name,
            shell::TEXT,
            bg,
        )?;
        text(
            dc,
            rect(
                row.rect.left + 35,
                row.rect.top + 24,
                row.rect.right - 8,
                row.rect.bottom,
            ),
            &row.detail,
            shell::MUTED,
            bg,
        )?;
    }
    if !f.message.is_empty() {
        text(
            dc,
            rect(
                f.files.left + 14,
                f.files.top + 48,
                f.files.right - 12,
                f.files.top + 100,
            ),
            &f.message,
            shell::MUTED,
            shell::SIDEBAR,
        )?;
    }
    paint::fill(
        dc,
        rect(
            f.preview.left,
            f.preview.top,
            f.preview.left + 1,
            f.preview.bottom,
        ),
        shell::SURFACE,
    )?;
    text(
        dc,
        rect(
            f.preview.left + 12,
            f.preview.top + 9,
            f.preview.right - 10,
            f.preview.top + 35,
        ),
        &f.subtitle,
        shell::TEXT,
        shell::BACKGROUND,
    )?;
    if !f.details.is_empty() {
        for (i, line) in f.details.iter().enumerate() {
            text(
                dc,
                rect(
                    f.preview.left + 18,
                    f.preview.top + f.details_top + i as i32 * 34,
                    f.preview.right - 18,
                    f.preview.top + f.details_top + 30 + i as i32 * 34,
                ),
                line,
                if i == 0 { shell::ACCENT } else { shell::TEXT },
                shell::BACKGROUND,
            )?;
        }
    }
    SelectObject(dc, code_font);
    for row in &f.code {
        let bg = match row.kind {
            -1 => REMOVE,
            1 => ADD,
            _ => shell::BACKGROUND,
        };
        paint::fill(dc, row.rect, bg)?;
        text(
            dc,
            rect(
                row.rect.left + 3,
                row.rect.top,
                row.rect.left + 43,
                row.rect.bottom,
            ),
            &row.old,
            shell::MUTED,
            bg,
        )?;
        text(
            dc,
            rect(
                row.rect.left + 46,
                row.rect.top,
                row.rect.left + 86,
                row.rect.bottom,
            ),
            &row.new,
            shell::MUTED,
            bg,
        )?;
        text(
            dc,
            rect(
                row.rect.left + 90,
                row.rect.top,
                row.rect.right - 6,
                row.rect.bottom,
            ),
            &row.text,
            match row.kind {
                -1 => RED,
                1 => GREEN,
                _ => shell::TEXT,
            },
            bg,
        )?;
    }
    SelectObject(dc, ui_font);
    for (r, name, selected) in &f.buttons {
        let bg = if *selected {
            shell::SELECTED
        } else {
            shell::RAISED
        };
        paint::fill(dc, *r, bg)?;
        text(
            dc,
            rect(r.left + 7, r.top + 4, r.right - 4, r.bottom),
            name,
            shell::TEXT,
            bg,
        )?;
    }
    text(
        dc,
        rect(
            f.rect.left + 12,
            f.rect.bottom - 21,
            f.rect.right - 12,
            f.rect.bottom,
        ),
        &f.footer,
        shell::MUTED,
        shell::BACKGROUND,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_geometry_is_tiled_compact_and_bounded_without_editor_cell_layout() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        for (w, h) in [(320, 240), (800, 560), (1400, 900), (8192, 8192)] {
            s.width = w;
            s.height = h;
            let l = Layout::new(&s);
            assert_eq!(l.files.right, l.preview.left);
            assert_eq!(l.preview.right, w);
            assert!(l.preview.bottom <= h);
            assert!(l.files.left >= 0);
            assert_eq!(l.history.is_some(), w >= s.dpi.pixels(760));
        }
    }
    #[test]
    fn workspace_paint_targets_expire_after_resize_and_close_without_source_edits() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.width = 1200;
        s.height = 800;
        s.git_panel = Some(GitIntent::Status);
        let before = s
            .workbench
            .active_document()
            .unwrap()
            .session
            .document()
            .revision();
        let frame = s.prepare_git_workspace().unwrap();
        let target = frame
            .hits
            .actions
            .iter()
            .find(|(_, a)| matches!(a, Intent::Close))
            .unwrap()
            .0;
        s.git_workspace.hits = Some(frame.hits);
        s.width += 1;
        s.git_workspace_click(target.left + 1, target.top + 1);
        assert!(s.git_panel.is_some());
        let frame = s.prepare_git_workspace().unwrap();
        s.git_workspace.hits = Some(frame.hits);
        s.git_workspace_click(target.left + 1, target.top + 1);
        assert!(s.git_panel.is_none());
        assert_eq!(
            before,
            s.workbench
                .active_document()
                .unwrap()
                .session
                .document()
                .revision()
        );
    }
    #[test]
    fn valid_unicode_and_tabs_are_human_readable_and_invalid_bytes_remain_visible() {
        assert_eq!(
            source_text("niño\tX".as_bytes(), TextEncoding::Utf8, 0),
            "niño    X"
        );
        assert!(source_text(b"x\xff", TextEncoding::Utf8, 0).contains("\\xFF"));
        assert!(source_text(&vec![b'x'; 8000], TextEncoding::Utf8, 0).len() <= 4096);
    }
}

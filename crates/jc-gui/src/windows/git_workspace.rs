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

#[path = "git_compare_rows.rs"]
mod compare_rows;
#[path = "git_workspace_controls.rs"]
mod controls;
#[path = "git_workspace_smoke.rs"]
mod layout_smoke;
pub(super) use layout_smoke::run as smoke_layout;
#[path = "git_blame.rs"]
mod blame;
#[path = "git_commit_search.rs"]
mod commit_search;
#[path = "git_create_commit.rs"]
mod create_commit;
pub(super) use commit_search::paste_query as paste_commit_query;
pub(super) use create_commit::paste_message as paste_commit_message;
#[path = "git_graph.rs"]
mod graph_ui;
#[path = "git_history.rs"]
mod history;
#[path = "git_layout.rs"]
mod layout;
#[path = "git_local.rs"]
mod local;
#[path = "git_path_history.rs"]
mod path_history;
use compare_rows::DiffView;
use controls::{GitDrag, Rail};
use layout::{Arrangement, Column, Divider, Preferences};

const ADD: u32 = 0x003f4830;
const REMOVE: u32 = 0x003e354a;
const GREEN: u32 = 0x008cbea3;
const RED: u32 = 0x00968cd5;
const PANEL: u32 = 0x003c312b;
const HEADER: u32 = 0x004a3d35;
const BORDER: u32 = 0x00594b42;
const ACCENT: u32 = 0x00d0c088;
const MUTED: u32 = 0x00af9d92;
const SELECTION: u32 = 0x00624e41;

#[derive(Default)]
pub(super) struct Workspace {
    pub(super) graph: Option<jc_app::git_graph::GraphView>,
    pub(super) commit_search: Option<commit_search::CommitSearchView>,
    pub hits: Option<Hits>,
    preferences: Preferences,
    active: Column,
    menu: Option<usize>,
    drag: Option<GitDrag>,
    pub(super) diff_column: usize,
    diff_extent: usize,
    diff_columns: usize,
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
    history_browser: history::HistoryBrowser,
    pub(super) path_query: Option<(String, Vec<u8>)>,
    pub(super) blame_query: Option<(String, Vec<u8>, usize)>,
    blame_selected: usize,
    blame_comparing: bool,
    blame_viewport: (usize, usize),
    path_selected: usize,
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
    search: Option<(
        String,
        String,
        usize,
        jc_app::git_revision::SearchField,
        bool,
        bool,
        usize,
    )>,
    root: Option<PathBuf>,
    epoch: u64,
    geometry: (i32, i32),
    metrics: (i32, i32, i32),
    document: Option<(jc_app::DocumentId, u64, Option<TextEncoding>)>,
    selected: usize,
    commit: Option<usize>,
    first: usize,
    history: Option<HistoryStamp>,
    local: Option<jc_app::git_revision::PairKind>,
    preferences: Preferences,
    active: Column,
    column: usize,
    menu: Option<usize>,
    path_query: Option<(String, Vec<u8>)>,
    path_selected: usize,
    blame_query: Option<(String, Vec<u8>, usize)>,
    blame_selected: usize,
    blame_comparing: bool,
}
#[derive(Clone, Copy)]
enum Intent {
    GraphOpen,
    GraphClose,
    GraphNext,
    GraphCommit(usize),
    GraphReferences,
    GraphReference(usize),
    GraphOpenReference,
    GraphChangeReference(bool),
    GraphSwitchBranch,
    GraphMergeBranch,
    GraphRebaseBranch(bool),
    GraphCommitOperation(jc_app::git_commit::CommitOperation),
    GraphReset,
    ResetMode(jc_app::git_commit::ResetMode),
    GraphAbortMerge,
    GraphCreateReference(jc_app::git_commit::RefKind),
    HistoryParent,
    CreateCommit,
    AmendCommit,
    SubmitCommit,
    CancelCommit,
    Merge(jc_app::git_merge_resolution::MergeAction),
    SelectMainline(usize),
    RebaseTodo(jc_app::git_commit::RebaseTodoEdit),
    BackMerge,
    Index,
    IndexHunk,
    RestoreFile,
    Patch,
    SearchOpen,
    SearchRun,
    SearchScope,
    SearchFocus,
    SearchResult(usize),
    SearchNext,
    SearchBack,
    SearchExit,
    Blame,
    BlameLine(usize),
    BlamePage(isize),
    BlameBack,
    BlameExit,
    PathHistory,
    PathCommit(usize),
    PathNext,
    PathBack,
    PathAll,
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
    HistoryFileMode(bool),
    HistoryTreeEntry(usize),
    HistoryTab(usize),
    HistoryTabStep(isize),
    HistoryTabClose,
    HistorySummary,
    HistoryEncoding,
    DistributionMenu,
    Arrange(Arrangement),
    DiffView(DiffView),
    Column(Column),
    Divider(Divider),
    Rail(Rail),
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
    parallel: bool,
}
struct BlameRow {
    rect: Rect,
    code_left: i32,
    number: String,
    origin: String,
    author: String,
    text: String,
    selected: bool,
    unresolved: bool,
}
struct QueryRow {
    rect: Rect,
    text: String,
    caret: Option<i32>,
}
pub(super) struct Frame {
    query: Option<QueryRow>,
    pub hits: Hits,
    rect: Rect,
    toolbar: Rect,
    history: Option<Rect>,
    locations: Option<Rect>,
    history_heading: &'static str,
    summary: Vec<String>,
    unit: i32,
    files: Rect,
    preview: Rect,
    title: String,
    branch: String,
    count: String,
    historical: bool,
    history_browser: bool,
    details_top: i32,
    message: String,
    subtitle: String,
    buttons: Vec<(Rect, String, bool)>,
    commits: Vec<CommitRow>,
    rows: Vec<FileRow>,
    code: Vec<CodeRow>,
    merge_caret: Option<Rect>,
    annotations: Vec<BlameRow>,
    details: Vec<String>,
    footer: String,
    overlays: Vec<(Rect, String, bool)>,
    rails: Vec<Rail>,
    dividers: Vec<Rect>,
}
#[derive(Clone)]
struct Layout {
    rect: Rect,
    toolbar: Rect,
    history: Option<Rect>,
    locations: Option<Rect>,
    files: Rect,
    preview: Rect,
    row: i32,
    head: i32,
    dividers: Vec<(Rect, Divider)>,
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
        let top = s.chrome_layout().top().min(h);
        let bottom = (h - s.cell_height - 5).max(top);
        let g = layout::layout(
            w,
            h,
            top,
            bottom,
            |n| s.dpi.pixels(n),
            s.git_workspace.preferences,
            s.git_workspace.active,
        );
        let locations = g
            .history
            .filter(|r| w >= s.dpi.pixels(1200) && r.right - r.left >= s.dpi.pixels(340))
            .map(|r| {
                rect(
                    r.left,
                    r.top,
                    r.left
                        + s.dpi
                            .pixels(if w >= s.dpi.pixels(1400) { 185 } else { 140 }),
                    r.bottom,
                )
            });
        Self {
            rect: g.rect,
            toolbar: g.toolbar,
            history: g.history.map(|mut r| {
                if let Some(side) = locations {
                    r.left = side.right + 1;
                }
                r
            }),
            locations,
            files: g.files.unwrap_or_else(|| rect(0, 0, 0, 0)),
            preview: g.preview.unwrap_or_else(|| rect(0, 0, 0, 0)),
            row: s.dpi.pixels(52),
            head: s.dpi.pixels(36),
            dividers: g.dividers,
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
#[cfg(test)]
fn source_text(bytes: &[u8], encoding: TextEncoding, _start: usize) -> String {
    compare_rows::text(bytes, encoding, 0).0
}

impl State {
    fn git_workspace_stamp(&self) -> Stamp {
        Stamp {
            search: self.git_workspace.commit_search.as_ref().map(|s| {
                (
                    s.start.clone(),
                    s.query.clone(),
                    s.cursor,
                    s.field,
                    s.editing,
                    s.opened,
                    s.selected,
                )
            }),
            blame_query: self.git_workspace.blame_query.clone(),
            blame_selected: self.git_workspace.blame_selected,
            blame_comparing: self.git_workspace.blame_comparing,
            path_query: self.git_workspace.path_query.clone(),
            path_selected: self.git_workspace.path_selected,
            preferences: self.git_workspace.preferences,
            active: self.git_workspace.active,
            column: self.git_workspace.diff_column,
            menu: self.git_workspace.menu,
            local: self.git_workspace.local_kind,
            root: self.project_root.clone(),
            epoch: self.git_workspace.epoch,
            geometry: (self.width, self.height),
            metrics: (self.cell_width, self.cell_height, self.dpi.pixels(100)),
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
        if self.git_commit.view().is_some() {
            return Some(self.prepare_create_commit(frame));
        }
        if self
            .git_workspace
            .graph
            .as_ref()
            .is_some_and(|graph| graph.showing)
        {
            return Some(self.prepare_git_graph(frame));
        }
        let mut frame = if self.git_workspace.commit_search.is_some() {
            self.prepare_git_commit_search(frame)
        } else if self.git_workspace.blame_query.is_some() {
            self.prepare_git_blame(frame)
        } else if self.git_workspace.path_query.is_some() {
            self.prepare_git_path_history(frame)
        } else if historical {
            self.prepare_git_history(frame)
        } else if self.git_workspace.local_kind.is_some() {
            self.prepare_git_local(frame)
        } else {
            let mut frame = frame;
            self.add_git_sources(&mut frame);
            frame
        };
        self.add_git_path_history_button(&mut frame);
        self.add_git_blame_button(&mut frame);
        self.add_git_commit_search_button(&mut frame);
        self.prepare_git_controls(&mut frame);
        self.add_git_patch_button(&mut frame);
        let restore = self.git_restore_label();
        let restoring_existing = restore == Some("Restaurar archivo");
        if let Some(label) = restore {
            let p = |n| self.dpi.pixels(n);
            let left = frame.preview.left + p(if restoring_existing { 12 } else { 156 });
            let top = frame.preview.top + p(156);
            let r = rect(left, top, left + p(140), top + p(24));
            frame.footer = format!("Ctrl+Alt+R {label} · {}", frame.footer);
            if r.right <= frame.preview.right - p(8)
                && r.bottom <= frame.preview.bottom
                && frame.buttons.iter().all(|(b, _, _)| {
                    r.left >= b.right || r.right <= b.left || r.top >= b.bottom || r.bottom <= b.top
                })
            {
                frame.buttons.push((r, label.into(), false));
                frame.hits.actions.push((r, Intent::RestoreFile));
            }
        }
        for (block, shortcut, offset, intent) in [
            (false, "Ctrl+Mayús+I", 12, Intent::Index),
            (true, "Ctrl+Alt+B", 156, Intent::IndexHunk),
        ] {
            let Some(label) = self.git_index_button(block) else {
                continue;
            };
            let (shortcut, offset) = if block && self.git_index.review().is_some() {
                ("Ctrl+Intro", 12)
            } else {
                (shortcut, offset)
            };
            let p = |n| self.dpi.pixels(n);
            frame.footer = format!("{shortcut} {label} · {}", frame.footer);
            let left = frame.preview.left
                + p(if restoring_existing {
                    offset + 144
                } else {
                    offset
                });
            let top = frame.preview.top + p(156);
            let candidate = rect(left, top, left + p(140), top + p(24));
            if candidate.right <= frame.preview.right - p(8)
                && candidate.bottom <= frame.preview.bottom
                && frame.buttons.iter().all(|(button, _, _)| {
                    candidate.left >= button.right
                        || candidate.right <= button.left
                        || candidate.top >= button.bottom
                        || candidate.bottom <= button.top
                })
            {
                frame.buttons.push((candidate, label.into(), false));
                frame.hits.actions.push((candidate, intent));
            }
        }
        self.add_create_commit_button(&mut frame);
        self.add_graph_button(&mut frame);
        Some(frame)
    }

    fn add_git_patch_button(&self, frame: &mut Frame) {
        if let Some(notice) = &self.git_patch.notice {
            frame.footer = format!("{notice} · {}", frame.footer);
        }
        let busy = self.git_patch.busy();
        if !busy && !self.git_patch_available() {
            return;
        }
        let (name, shortcut) = if busy {
            ("Cancelar parche", "Ctrl+Alt+E cancelar exportación")
        } else {
            ("Exportar parche", "Ctrl+Alt+E exportar parche")
        };
        frame.footer = format!("{shortcut} · {}", frame.footer);
        let p = |n| self.dpi.pixels(n);
        let width = p(132);
        for (area, left, y) in [
            (frame.preview, frame.preview.right - p(140), p(128)),
            (frame.toolbar, frame.toolbar.right - p(538), p(8)),
        ] {
            let candidate = rect(left, area.top + y, left + width, area.top + y + p(24));
            if candidate.left >= area.left + p(8)
                && candidate.right <= area.right - p(8)
                && candidate.bottom <= area.bottom
                && frame.buttons.iter().all(|(button, _, _)| {
                    candidate.left >= button.right
                        || candidate.right <= button.left
                        || candidate.top >= button.bottom
                        || candidate.bottom <= button.top
                })
            {
                frame.buttons.push((candidate, name.into(), false));
                frame.hits.actions.push((candidate, Intent::Patch));
                break;
            }
        }
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
                    (right - p(204)).max(p(320)),
                    layout.toolbar.top + p(8),
                    (right - p(94)).max(p(320)),
                    layout.toolbar.top + p(34),
                ),
                "Actualizar",
                Intent::Refresh,
                false,
            ),
            (
                rect(
                    (right - p(88)).max(p(320)),
                    layout.toolbar.top + p(8),
                    right - p(8),
                    layout.toolbar.top + p(34),
                ),
                "Código",
                Intent::Close,
                false,
            ),
        ] {
            if right < p(700) {
                continue;
            }
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
                detail: path
                    .rsplit_once('/')
                    .map_or_else(|| description.into(), |(dir, _)| dir.into()),
                tag,
                color,
                selected: index == selected,
            });
            actions.push((r, Intent::File(index)));
        }
        let mut commits = Vec::new();
        let mut summary = if self.git_workspace.history_commit.is_some() {
            Vec::new()
        } else {
            vec![
                "Cambios locales".into(),
                format!("{} archivos · consulta de solo lectura", entries.len()),
            ]
        };
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
                let local = rect(
                    area.left,
                    area.top + layout.head,
                    area.right,
                    (area.top + layout.head + p(56)).min(area.bottom),
                );
                actions.push((local, Intent::Local));
                let count = ((area.bottom - area.top - layout.head - p(56)) / p(64).max(1))
                    .clamp(0, 24) as usize;
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
                    let top = area.top + layout.head + p(56) + (index - start) as i32 * p(64);
                    let r = rect(area.left, top, area.right, (top + p(64)).min(area.bottom));
                    commits.push(CommitRow {
                        rect: r,
                        title: opaque(&c.summary, 160),
                        detail: format!(
                            "{} · {}",
                            &c.id[..c.id.len().min(8)],
                            opaque(&c.author, 80)
                        ),
                        selected: self
                            .git_workspace
                            .history_commit
                            .as_ref()
                            .map_or(self.git_workspace.commit == Some(index), |id| id == &c.id),
                        merge: c.parents.len() > 1,
                    });
                    actions.push((r, Intent::Commit(index)));
                }
            }
            if let Some(c) = self
                .git_workspace
                .history_commit
                .as_ref()
                .and_then(|id| info.commits.iter().find(|c| &c.id == id))
                .or_else(|| {
                    self.git_workspace
                        .history_commit
                        .is_none()
                        .then(|| self.git_workspace.commit.and_then(|i| info.commits.get(i)))
                        .flatten()
                })
            {
                summary = vec![
                    opaque(&c.summary, 256),
                    format!("{} · {}", opaque(&c.author, 96), &c.id[..c.id.len().min(8)]),
                ];
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
        let rows_visible = ((layout.preview.bottom - layout.preview.top - p(164))
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
                    if result.model.hunks().nth(self.git_diff.selected()).is_some() {
                        let projection = controls::project(
                            &result.model,
                            controls::ProjectionView {
                                hunk: self.git_diff.selected(),
                                first: self.git_workspace.diff_first,
                                encoding,
                                geometry: (
                                    rect(
                                        layout.preview.left,
                                        layout.preview.top,
                                        layout.preview.right - p(12),
                                        layout.preview.bottom - p(14),
                                    ),
                                    layout.preview.top + p(164),
                                    self.cell_height,
                                ),
                                mode: self.git_workspace.diff_mode(),
                                column: self.git_workspace.diff_column,
                                cell_width: self.cell_width,
                            },
                        );
                        code = self.git_workspace.set_projection(projection);
                        let y = layout.preview.top + p(128);
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
        Some(Frame {
            query: None,
            locations: layout.locations,
            summary,
            unit: p(100),
            hits: Hits {
                stamp: self.git_workspace_stamp(),
                actions,
            },
            rect: layout.rect,
            toolbar: layout.toolbar,
            history: layout.history,
            history_heading: "HISTORIAL",
            files: layout.files,
            preview: layout.preview,
            title,
            branch,
            count,
            overlays: Vec::new(),
            rails: Vec::new(),
            dividers: Vec::new(),
            historical: false,
            history_browser: false,
            details_top: p(164),
            message,
            subtitle,
            buttons,
            commits,
            rows,
            code,
            merge_caret: None,
            annotations: Vec::new(),
            details,
            footer: "Solo lectura · Comparación local · Esc vuelve al editor".into(),
        })
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
        if self.git_workspace.menu.is_some()
            && !matches!(intent, Some(Intent::Arrange(_) | Intent::DistributionMenu))
        {
            self.git_workspace.menu = None;
            self.git_workspace.invalidate();
            return Action::None;
        }
        if let Some(intent) = intent {
            if let Some(action) = self.git_control_action(intent, x, y) {
                return action;
            }
        }
        match intent {
            Some(Intent::GraphOpen) => {
                self.open_git_graph();
                Action::None
            }
            Some(Intent::GraphClose) => {
                self.close_graph();
                Action::None
            }
            Some(Intent::GraphNext) => {
                self.next_graph_page();
                Action::None
            }
            Some(Intent::GraphCommit(index)) => {
                self.open_graph_commit(index);
                Action::None
            }
            Some(Intent::GraphReferences) => {
                self.toggle_graph_references();
                Action::None
            }
            Some(Intent::GraphReference(index)) => {
                self.select_graph_reference(index);
                Action::None
            }
            Some(Intent::GraphOpenReference) => {
                self.open_selected_graph_reference();
                Action::None
            }
            Some(Intent::GraphChangeReference(rename)) => {
                self.open_graph_reference_change(rename);
                Action::None
            }
            Some(Intent::GraphSwitchBranch) => {
                self.open_graph_branch_switch();
                Action::None
            }
            Some(intent @ (Intent::GraphMergeBranch | Intent::GraphRebaseBranch(_))) => {
                self.open_graph_merge(intent);
                Action::None
            }
            Some(Intent::GraphCommitOperation(operation)) => {
                self.open_graph_commit_operation(Some(operation));
                Action::None
            }
            Some(Intent::GraphReset) => {
                self.open_graph_commit_operation(None);
                Action::None
            }
            Some(Intent::ResetMode(mode)) => {
                self.git_commit.select_reset_mode(mode);
                self.git_workspace.invalidate();
                Action::None
            }
            Some(Intent::GraphAbortMerge) => {
                self.open_graph_merge_abort();
                Action::None
            }
            Some(Intent::GraphCreateReference(kind)) => {
                self.open_graph_reference_draft(kind);
                Action::None
            }
            Some(Intent::HistoryParent) => {
                self.cycle_git_history_parent();
                Action::None
            }
            Some(Intent::CreateCommit) => {
                self.open_git_create_commit(false);
                Action::None
            }
            Some(Intent::AmendCommit) => {
                self.open_git_create_commit(true);
                Action::None
            }
            Some(Intent::SubmitCommit) => {
                self.submit_git_create_commit();
                Action::None
            }
            Some(Intent::Merge(action)) => {
                self.git_commit.merge_action(action);
                self.git_workspace.invalidate();
                Action::None
            }
            Some(Intent::SelectMainline(index)) => {
                self.git_commit.select_mainline(index);
                self.git_workspace.invalidate();
                Action::None
            }
            Some(Intent::RebaseTodo(edit)) => {
                self.git_commit.edit_rebase_todo(edit);
                self.git_workspace.invalidate();
                Action::None
            }
            Some(Intent::BackMerge) => {
                self.git_commit.back_merge();
                self.git_workspace.invalidate();
                Action::None
            }
            Some(Intent::CancelCommit) => {
                self.git_commit.cancel();
                self.git_workspace.invalidate();
                Action::None
            }
            Some(Intent::Index) => {
                self.git_index_action(false);
                Action::None
            }
            Some(Intent::IndexHunk) => {
                self.git_index_action(true);
                Action::None
            }
            Some(Intent::RestoreFile) => {
                self.open_git_restore();
                Action::None
            }
            Some(Intent::Patch) => self.git_patch_action(),
            Some(Intent::SearchOpen) => {
                self.open_git_commit_search();
                Action::None
            }
            Some(Intent::SearchRun) => {
                self.run_git_commit_search();
                Action::None
            }
            Some(Intent::SearchScope) => {
                self.cycle_git_search_scope();
                Action::None
            }
            Some(Intent::SearchFocus) => {
                self.focus_git_search_input();
                Action::None
            }
            Some(Intent::SearchResult(i)) => {
                self.open_git_search_result(i);
                Action::None
            }
            Some(Intent::SearchNext) => {
                self.next_git_commit_search();
                Action::None
            }
            Some(Intent::SearchBack) => {
                self.back_git_commit_search();
                Action::None
            }
            Some(Intent::SearchExit) => self.request_git(GitIntent::Status),
            Some(Intent::Blame) => {
                self.request_git_blame();
                Action::None
            }
            Some(Intent::BlameLine(index)) => {
                self.open_git_blame_line(index);
                Action::None
            }
            Some(Intent::BlamePage(delta)) => {
                self.page_git_blame(delta);
                Action::None
            }
            Some(Intent::BlameBack) => {
                self.back_git_blame();
                Action::None
            }
            Some(Intent::BlameExit) => self.request_git(GitIntent::Status),
            Some(Intent::PathHistory) => {
                self.request_git_path_history();
                Action::None
            }
            Some(Intent::PathCommit(index)) => {
                self.open_git_path_commit(index);
                Action::None
            }
            Some(Intent::PathNext) => {
                self.next_git_path_history();
                Action::None
            }
            Some(Intent::PathBack) => {
                self.back_git_path_history();
                Action::None
            }
            Some(Intent::PathAll) => {
                self.git_workspace.path_query = None;
                self.request_git(GitIntent::Status)
            }
            Some(
                Intent::DistributionMenu
                | Intent::Arrange(_)
                | Intent::DiffView(_)
                | Intent::Column(_)
                | Intent::Divider(_)
                | Intent::Rail(_),
            ) => Action::None,
            Some(Intent::Refresh) => {
                if self.git_workspace.commit_search.is_some() {
                    self.run_git_commit_search();
                    Action::None
                } else if self.git_workspace.blame_query.is_some() {
                    self.refresh_git_blame();
                    Action::None
                } else if self.git_workspace.path_query.is_some() {
                    self.refresh_git_path_history();
                    Action::None
                } else if let Some(kind) = self.git_workspace.local_kind {
                    self.request_git_local(kind);
                    Action::None
                } else if let Some(commit) = self.git_workspace.history_commit.clone() {
                    self.request_git_history_parent(commit, self.git_revision.parent_options().0);
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
            Some(Intent::HistoryFileMode(tree)) => {
                self.set_git_history_tree(tree);
                Action::None
            }
            Some(Intent::HistoryTreeEntry(index)) => {
                self.open_git_history_tree_entry(index);
                Action::None
            }
            Some(Intent::HistoryTab(index)) => {
                self.open_git_history_tab(index);
                Action::None
            }
            Some(Intent::HistoryTabStep(delta)) => {
                self.step_git_history_tab(delta);
                Action::None
            }
            Some(Intent::HistoryTabClose) => {
                self.close_git_history_tab();
                Action::None
            }
            Some(Intent::HistorySummary) => {
                self.show_git_history_summary();
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
            None => {
                self.git_workspace.menu = None;
                self.git_workspace.invalidate();
                Action::None
            }
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
        self.git_workspace.active = Column::Comparison;
        self.git_workspace.diff_column = 0;
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
        if self.git_commit.view().is_some() {
            self.git_commit.scroll(steps);
            self.git_workspace.invalidate();
            return true;
        }
        if self
            .git_workspace
            .graph
            .as_ref()
            .is_some_and(|graph| graph.showing)
        {
            self.move_git_graph(steps);
            return true;
        }
        if inside(l.preview, x, y) {
            let old = self.git_workspace.diff_first;
            self.git_workspace.diff_first = old.saturating_add_signed(steps).min(
                self.git_workspace
                    .diff_total
                    .saturating_sub(self.git_workspace.diff_page),
            );
        } else if l.history.is_some_and(|r| inside(r, x, y)) {
            let total = if self.git_workspace.path_query.is_some() {
                self.git_revision
                    .path_history()
                    .map_or(0, |p| p.entries.len())
            } else {
                self.git_status
                    .result()
                    .and_then(|r| r.info())
                    .map_or(0, |i| i.commits.len())
            };
            let page = l.history.map_or(0, |r| {
                ((r.bottom - r.top - l.head - self.dpi.pixels(56)) / self.dpi.pixels(64).max(1))
                    .max(0) as usize
            });
            self.git_workspace.history_first = self
                .git_workspace
                .history_first
                .saturating_add_signed(steps)
                .min(total.saturating_sub(page));
        } else if inside(l.files, x, y) {
            if self.git_workspace.history_commit.is_some() {
                self.move_git_history_files(steps);
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
    let p = |v: i32| ((i64::from(v) * i64::from(f.unit)) / 100) as i32;
    paint::fill(dc, f.rect, shell::BACKGROUND)?;
    paint::fill(dc, f.toolbar, PANEL)?;
    SelectObject(dc, ui_font);
    if f.toolbar.right >= p(800) {
        let compact = f.history.is_none()
            || f.files.right == f.files.left
            || f.preview.right == f.preview.left;
        let title_left = f
            .hits
            .actions
            .iter()
            .find_map(|(r, intent)| {
                (matches!(intent, Intent::CreateCommit) && r.top < f.toolbar.bottom)
                    .then_some(r.right + p(12))
            })
            .unwrap_or(0)
            .max(p(if compact { 440 } else { 240 }));
        text(
            dc,
            rect(
                title_left,
                f.toolbar.top + p(8),
                (f.toolbar.right - p(if f.toolbar.right >= p(900) { 410 } else { 215 })).max(0),
                f.toolbar.top + p(34),
            ),
            &format!("{}   /   {}", f.title, f.branch),
            shell::TEXT,
            PANEL,
        )?;
    }
    if let Some(area) = f.locations {
        paint::fill(dc, area, shell::SIDEBAR)?;
        paint::fill(
            dc,
            rect(area.left, area.top, area.right, area.top + p(36)),
            PANEL,
        )?;
        text(
            dc,
            rect(
                area.left + p(14),
                area.top + p(9),
                area.right - p(8),
                area.top + p(32),
            ),
            "UBICACIONES",
            MUTED,
            PANEL,
        )?;
        text(
            dc,
            rect(
                area.left + p(14),
                area.top + p(49),
                area.right - p(8),
                area.top + p(74),
            ),
            "RAMA ACTUAL",
            MUTED,
            shell::SIDEBAR,
        )?;
        let branch = rect(area.left, area.top + p(82), area.right, area.top + p(111));
        paint::fill(dc, branch, SELECTION)?;
        text(
            dc,
            rect(
                branch.left + p(24),
                branch.top + p(4),
                branch.right - p(8),
                branch.bottom,
            ),
            &f.branch,
            ACCENT,
            SELECTION,
        )?;
        text(
            dc,
            rect(
                area.left + p(14),
                area.top + p(145),
                area.right - p(8),
                area.top + p(171),
            ),
            "REPOSITORIO",
            MUTED,
            shell::SIDEBAR,
        )?;
        text(
            dc,
            rect(
                area.left + p(24),
                area.top + p(177),
                area.right - p(8),
                area.top + p(201),
            ),
            &f.title,
            shell::TEXT,
            shell::SIDEBAR,
        )?;
        paint::fill(
            dc,
            rect(area.right, area.top, area.right + 1, area.bottom),
            BORDER,
        )?;
    }
    if let Some(area) = f.history {
        paint::fill(dc, area, PANEL)?;
        paint::fill(
            dc,
            rect(area.left, area.top, area.right, area.top + p(36)),
            HEADER,
        )?;
        text(
            dc,
            rect(
                area.left + p(14),
                area.top + p(9),
                area.right - p(8),
                area.top + p(32),
            ),
            f.history_heading,
            MUTED,
            HEADER,
        )?;
        let local = rect(
            area.left,
            area.top + p(36),
            area.right,
            (area.top + p(92)).min(area.bottom),
        );
        let local_bg = if !f.historical { SELECTION } else { PANEL };
        paint::fill(dc, local, local_bg)?;
        text(
            dc,
            rect(
                local.left + p(30),
                local.top + p(5),
                local.right - p(10),
                local.top + p(28),
            ),
            "Cambios locales",
            shell::TEXT,
            local_bg,
        )?;
        text(
            dc,
            rect(
                local.left + p(30),
                local.top + p(28),
                local.right - p(10),
                local.bottom,
            ),
            if f.historical {
                "Ver estado del repositorio"
            } else {
                &f.count
            },
            MUTED,
            local_bg,
        )?;
        for c in &f.commits {
            let bg = if c.selected { SELECTION } else { PANEL };
            paint::fill(dc, c.rect, bg)?;
            paint::fill(
                dc,
                rect(
                    c.rect.left + p(19),
                    c.rect.top,
                    c.rect.left + p(20),
                    c.rect.bottom,
                ),
                ACCENT,
            )?;
            paint::fill(
                dc,
                rect(
                    c.rect.left + p(16),
                    c.rect.top + p(16),
                    c.rect.left + p(23),
                    c.rect.top + p(23),
                ),
                ACCENT,
            )?;
            text(
                dc,
                rect(
                    c.rect.left + p(34),
                    c.rect.top + p(7),
                    c.rect.right - p(10),
                    c.rect.top + p(31),
                ),
                &c.title,
                shell::TEXT,
                bg,
            )?;
            text(
                dc,
                rect(
                    c.rect.left + p(34),
                    c.rect.top + p(33),
                    c.rect.right - p(10),
                    c.rect.bottom - p(5),
                ),
                &if c.merge {
                    format!("Integración · {}", c.detail)
                } else {
                    c.detail.clone()
                },
                MUTED,
                bg,
            )?;
        }
    }
    if f.files.right > f.files.left {
        paint::fill(dc, f.files, shell::SIDEBAR)?;
        paint::fill(
            dc,
            rect(
                f.files.left,
                f.files.top,
                f.files.right,
                f.files.top + p(36),
            ),
            HEADER,
        )?;
        if !f.history_browser {
            text(
                dc,
                rect(
                    f.files.left + p(12),
                    f.files.top + p(9),
                    f.files.right - p(10),
                    f.files.top + p(32),
                ),
                &format!(
                    "ARCHIVOS · {}",
                    f.count.strip_suffix(" archivos").unwrap_or(&f.count)
                ),
                MUTED,
                HEADER,
            )?;
        }
        for row in &f.rows {
            let bg = if row.selected {
                SELECTION
            } else {
                shell::SIDEBAR
            };
            paint::fill(dc, row.rect, bg)?;
            text(
                dc,
                rect(
                    row.rect.left + p(12),
                    row.rect.top + p(7),
                    row.rect.left + p(29),
                    row.rect.bottom,
                ),
                row.tag,
                row.color,
                bg,
            )?;
            text(
                dc,
                rect(
                    row.rect.left + p(34),
                    row.rect.top + p(4),
                    row.rect.right - p(8),
                    row.rect.top + p(27),
                ),
                &row.name,
                shell::TEXT,
                bg,
            )?;
            text(
                dc,
                rect(
                    row.rect.left + p(34),
                    row.rect.top + p(27),
                    row.rect.right - p(8),
                    row.rect.bottom,
                ),
                &row.detail,
                MUTED,
                bg,
            )?;
        }
        if !f.message.is_empty() {
            text(
                dc,
                rect(
                    f.files.left + p(14),
                    f.files.top + p(48),
                    f.files.right - p(12),
                    f.files.top + p(104),
                ),
                &f.message,
                MUTED,
                shell::SIDEBAR,
            )?;
        }
    }
    if f.preview.right > f.preview.left {
        let a = f.preview;
        paint::fill(dc, rect(a.left, a.top, a.right, a.top + p(32)), PANEL)?;
        if !f.history_browser {
            text(
                dc,
                rect(
                    a.left + p(if f.historical { 280 } else { 14 }),
                    a.top + p(5),
                    a.right - p(10),
                    a.top + p(29),
                ),
                &f.subtitle,
                MUTED,
                PANEL,
            )?;
        }
        for (i, s) in f.summary.iter().take(2).enumerate() {
            text(
                dc,
                rect(
                    a.left + p(18),
                    a.top + p(42 + i as i32 * 26),
                    a.right - p(16),
                    a.top + p(66 + i as i32 * 26),
                ),
                s,
                if i == 0 { shell::TEXT } else { MUTED },
                shell::BACKGROUND,
            )?;
        }
        paint::fill(
            dc,
            rect(a.left, a.top + p(94), a.right, a.top + p(123)),
            HEADER,
        )?;
        if !f.details.is_empty() {
            for (i, s) in f.details.iter().enumerate() {
                text(
                    dc,
                    rect(
                        a.left + p(18),
                        a.top + f.details_top + p(i as i32 * 28),
                        a.right - p(18),
                        a.top + f.details_top + p(26 + i as i32 * 28),
                    ),
                    s,
                    MUTED,
                    shell::BACKGROUND,
                )?;
            }
        }
    }
    SelectObject(dc, code_font);
    for row in &f.code {
        let bg = match row.kind {
            -1 => REMOVE,
            1 => ADD,
            2 => SELECTION,
            _ => shell::BACKGROUND,
        };
        paint::fill(dc, row.rect, bg)?;
        text(
            dc,
            rect(
                row.rect.left + 3,
                row.rect.top,
                (row.rect.left + 43).min(row.rect.right),
                row.rect.bottom,
            ),
            if row.parallel && row.old.is_empty() {
                &row.new
            } else {
                &row.old
            },
            MUTED,
            bg,
        )?;
        if !row.parallel {
            text(
                dc,
                rect(
                    row.rect.left + 46,
                    row.rect.top,
                    row.rect.left + 86,
                    row.rect.bottom,
                ),
                &row.new,
                MUTED,
                bg,
            )?;
        }
        text(
            dc,
            rect(
                row.rect.left + if row.parallel { 48 } else { 90 },
                row.rect.top,
                row.rect.right - 6,
                row.rect.bottom,
            ),
            &row.text,
            shell::TEXT,
            bg,
        )?;
    }
    if let Some(caret) = f.merge_caret {
        paint::fill(dc, caret, shell::TEXT)?;
    }
    for row in &f.annotations {
        let bg = if row.selected {
            SELECTION
        } else {
            shell::BACKGROUND
        };
        paint::fill(dc, row.rect, bg)?;
        SelectObject(dc, ui_font);
        text(
            dc,
            rect(
                row.rect.left + p(5),
                row.rect.top,
                row.rect.left + p(52),
                row.rect.bottom,
            ),
            &row.number,
            MUTED,
            bg,
        )?;
        text(
            dc,
            rect(
                row.rect.left + p(56),
                row.rect.top,
                (row.rect.left + p(138)).min(row.code_left - p(5)),
                row.rect.bottom,
            ),
            &row.origin,
            if row.unresolved { MUTED } else { ACCENT },
            bg,
        )?;
        if row.code_left - row.rect.left >= p(240) {
            text(
                dc,
                rect(
                    row.rect.left + p(142),
                    row.rect.top,
                    row.code_left - p(8),
                    row.rect.bottom,
                ),
                &row.author,
                MUTED,
                bg,
            )?;
        }
        paint::fill(
            dc,
            rect(
                row.code_left - p(4),
                row.rect.top,
                row.code_left - p(3),
                row.rect.bottom,
            ),
            BORDER,
        )?;
        SelectObject(dc, code_font);
        text(
            dc,
            rect(
                row.code_left,
                row.rect.top,
                row.rect.right - p(6),
                row.rect.bottom,
            ),
            &row.text,
            shell::TEXT,
            bg,
        )?;
    }
    if let Some(query) = &f.query {
        paint::fill(dc, query.rect, SELECTION)?;
        SelectObject(dc, code_font);
        text(
            dc,
            rect(
                query.rect.left + p(8),
                query.rect.top + p(3),
                query.rect.right - p(8),
                query.rect.bottom,
            ),
            &query.text,
            shell::TEXT,
            SELECTION,
        )?;
        if let Some(x) = query.caret {
            paint::fill(
                dc,
                rect(
                    query.rect.left + p(8) + x,
                    query.rect.top + p(4),
                    query.rect.left + p(9) + x,
                    query.rect.bottom - p(4),
                ),
                ACCENT,
            )?;
        }
    }
    SelectObject(dc, ui_font);
    for (r, name, selected) in &f.buttons {
        if r.right <= r.left {
            continue;
        }
        let bg = if *selected {
            SELECTION
        } else if r.top < f.toolbar.bottom {
            PANEL
        } else {
            shell::BACKGROUND
        };
        paint::fill(dc, *r, bg)?;
        text(
            dc,
            rect(r.left + p(7), r.top + p(3), r.right - p(4), r.bottom),
            name,
            if *selected { ACCENT } else { shell::TEXT },
            bg,
        )?;
        if *selected {
            paint::fill(dc, rect(r.left, r.bottom - p(2), r.right, r.bottom), ACCENT)?;
        }
    }
    for a in [f.history, Some(f.files), Some(f.preview)]
        .into_iter()
        .flatten()
    {
        if a.right > a.left {
            paint::fill(dc, rect(a.left, a.top, a.left + 1, a.bottom), BORDER)?;
        }
    }
    text(
        dc,
        rect(
            f.rect.left + p(12),
            f.rect.bottom - p(22),
            f.rect.right - p(12),
            f.rect.bottom,
        ),
        &f.footer,
        MUTED,
        shell::BACKGROUND,
    )?;
    controls::draw(dc, f, ui_font)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requested_historical_summary_never_falls_back_to_local_changes() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.git_panel = Some(GitIntent::Status);
        s.git_workspace.history_commit = Some("0123456789012345678901234567890123456789".into());
        let f = s.prepare_git_workspace().unwrap();
        assert_eq!(f.summary[0], "Revisión histórica");
        assert_eq!(f.summary[1], "012345678901");
        assert!(!f.summary.iter().any(|s| s.contains("Cambios locales")));
    }
    #[test]
    fn redesign_locations_and_toolbar_adapt_without_overlapping_actions_or_editing() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.git_panel = Some(GitIntent::Status);
        let before = s.workbench.active_bytes().unwrap();
        for dpi in [96, 144, 192] {
            s.dpi = super::super::dpi::Dpi::from_raw(dpi);
            for width in [320, 800, 1200, 1680] {
                s.width = s.dpi.pixels(width);
                s.height = s.dpi.pixels(960);
                let l = Layout::new(&s);
                assert_eq!(l.locations.is_some(), width >= 1200);
                if let Some(r) = l.locations {
                    assert!(r.right < l.history.unwrap().left);
                    assert_eq!(
                        r.right - r.left,
                        s.dpi.pixels(if width >= 1400 { 185 } else { 140 })
                    );
                }
                let f = s.prepare_git_workspace().unwrap();
                let toolbar: Vec<_> = f
                    .hits
                    .actions
                    .iter()
                    .filter(|(r, _)| {
                        r.top >= l.toolbar.top && r.bottom <= l.toolbar.bottom && r.right > r.left
                    })
                    .collect();
                for (i, (a, _)) in toolbar.iter().enumerate() {
                    for (b, _) in &toolbar[i + 1..] {
                        assert!(
                            a.right <= b.left || b.right <= a.left,
                            "overlapping toolbar at {width}/{dpi}"
                        );
                    }
                }
            }
        }
        assert_eq!(s.workbench.active_bytes().unwrap(), before);
    }
    #[test]
    fn workspace_geometry_is_tiled_compact_and_bounded_without_editor_cell_layout() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        for (w, h) in [(320, 240), (800, 560), (1400, 900), (8192, 8192)] {
            s.width = w;
            s.height = h;
            let l = Layout::new(&s);
            assert_eq!(l.rect.right, w);
            assert!(l.rect.bottom <= h);
            assert!(l.files.left >= 0 && l.files.right <= w);
            assert!(l.preview.left >= 0 && l.preview.right <= w);
            assert_eq!(s.git_workspace.preferences.arrangement, Arrangement::Three);
            assert_eq!(l.history.is_some(), w >= 612);
            if w == 320 {
                assert!(l.preview.right == l.preview.left);
                assert_eq!(l.files.right, w);
            }
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

//! Repository status view state and bounded preparation. No path-to-file action.
use jc_app::{
    git_read::GitReadError,
    git_status::{GitConflict, GitStatusChange, GitStatusError, GitStatusSnapshot, GitStatusState},
    git_status_tasks::{
        GitStatusController, GitStatusFailure, GitStatusOptions, GitStatusRequestError,
        GitStatusTaskState,
    },
    Wakeup,
};
use std::path::Path;

const MAX_ROWS: usize = 256;
const MAX_COLUMNS: usize = 256;
const PATH_LIMIT: usize = 4096;
const POLICY: &str = "sin renombrados/submódulos";

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct GitStatusPresentation {
    pub title: String,
    pub message: String,
    pub rows: Vec<GitStatusRow>,
    pub total: usize,
    pub selected: usize,
    pub scroll: usize,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct GitStatusRow {
    pub ordinal: usize,
    pub flags: String,
    pub path: String,
    pub selected: bool,
}

/// Match renderer headers: none at height1, one at height2, otherwise two.
pub(crate) fn git_status_list_height(height: u16) -> u16 {
    match height {
        0 => 0,
        1 | 2 => 1,
        _ => height - 2,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    Idle,
    Current,
    Cancelled,
    Stale,
    Rejected(&'static str),
}
#[derive(Default)]
struct ViewState {
    options: Option<GitStatusOptions>,
    mode: Mode,
    selected: usize,
    scroll: usize,
}
impl ViewState {
    fn validate_context(&mut self, root: Option<&Path>) -> bool {
        if self
            .options
            .as_ref()
            .is_some_and(|options| Some(options.root.as_path()) != root)
        {
            self.options = None;
            self.mode = Mode::Stale;
            self.reset_viewport();
            true
        } else {
            false
        }
    }
    fn reset_viewport(&mut self) {
        self.selected = 0;
        self.scroll = 0;
    }
    fn select(&mut self, target: usize, total: usize) -> bool {
        let selected = target.min(total.saturating_sub(1));
        let changed = selected != self.selected;
        self.selected = selected;
        changed
    }
    fn prepare(
        &mut self,
        snapshot: Option<&GitStatusSnapshot>,
        message: &str,
        width: u16,
        height: u16,
    ) -> GitStatusPresentation {
        let width = usize::from(width).min(MAX_COLUMNS);
        let height = usize::from(height).min(MAX_ROWS);
        let entries = if self.mode == Mode::Current {
            snapshot.map(|s| s.status().entries()).unwrap_or_default()
        } else {
            &[]
        };
        let total = entries.len();
        self.selected = self.selected.min(total.saturating_sub(1));
        if total == 0 {
            self.reset_viewport();
        } else if height == 0 {
            self.scroll = self.scroll.min(total - 1);
        } else {
            self.scroll = self.scroll.min(total.saturating_sub(height));
            if self.selected < self.scroll {
                self.scroll = self.selected;
            }
            if self.selected >= self.scroll + height {
                self.scroll = self.selected + 1 - height;
            }
        }
        let rows = entries
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(height)
            .map(|(ordinal, entry)| GitStatusRow {
                ordinal,
                flags: flags(entry.state()),
                path: escaped_prefix(entry.path(), width.saturating_sub(4)),
                selected: ordinal == self.selected,
            })
            .collect();
        GitStatusPresentation {
            title: plain_prefix("Estado Git [integrado]", width),
            message: plain_prefix(&format!("{message} | {POLICY}"), width),
            rows,
            total,
            selected: self.selected,
            scroll: self.scroll,
        }
    }
}

pub(crate) struct GitStatusView {
    controller: GitStatusController,
    view: ViewState,
}
impl GitStatusView {
    pub(crate) fn new(wake: Wakeup) -> Self {
        Self {
            controller: GitStatusController::new(wake),
            view: ViewState::default(),
        }
    }
    pub(crate) fn request(&mut self, root: &Path) -> Result<(), String> {
        self.view.reset_viewport();
        if !root.is_absolute() || root.as_os_str().len() > PATH_LIMIT {
            self.controller.cancel();
            self.view.options = None;
            let message = "Elige una raíz absoluta del repositorio";
            self.view.mode = Mode::Rejected(message);
            return Err(message.into());
        }
        let options = GitStatusOptions {
            root: root.to_path_buf(),
            limits: Default::default(),
        };
        self.view.options = Some(options.clone());
        self.view.mode = Mode::Current;
        if let Err(error) = self.controller.request(options) {
            let message = request_error(error);
            self.view.mode = Mode::Rejected(message);
            self.view.options = None;
            return Err(message.into());
        }
        Ok(())
    }
    pub(crate) fn cancel(&mut self) {
        self.controller.cancel();
        self.view.mode = Mode::Cancelled;
        self.view.reset_viewport();
    }
    pub(crate) fn clear(&mut self) {
        self.controller.cancel();
        self.view = ViewState::default();
    }
    /// Metadata-only invalidation, also safe immediately before prepare_frame.
    /// Never consumes a completion or bypasses poll_tasks panic recovery.
    pub(crate) fn validate_context(&mut self, root: Option<&Path>) -> bool {
        if self.view.validate_context(root) {
            self.controller.cancel();
            true
        } else {
            false
        }
    }
    pub(crate) fn poll(&mut self, root: Option<&Path>) -> bool {
        if self.validate_context(root) {
            return true;
        }
        let Some(options) = self.view.options.as_ref() else {
            return false;
        };
        self.controller.poll(options)
    }
    pub(crate) fn ready(&self) -> bool {
        self.controller.ready()
    }
    pub(crate) fn worker_panicked(&self) -> bool {
        matches!(
            self.controller.failure(),
            Some(GitStatusFailure::WorkerPanic)
        )
    }
    pub(crate) fn take_recovery(&self) -> bool {
        self.controller.take_recovery()
    }
    pub(crate) fn state(&self) -> GitStatusTaskState {
        match self.view.mode {
            Mode::Idle => GitStatusTaskState::Idle,
            Mode::Cancelled => GitStatusTaskState::Cancelled,
            Mode::Stale => GitStatusTaskState::Stale,
            Mode::Rejected(_) => GitStatusTaskState::Failed,
            Mode::Current => self.controller.state(),
        }
    }
    pub(crate) fn message(&self) -> &'static str {
        if let Mode::Rejected(message) = self.view.mode {
            return message;
        }
        state_message(self.state(), self.controller.failure())
    }
    fn total(&self) -> usize {
        if self.state() != GitStatusTaskState::Ready {
            return 0;
        }
        self.controller
            .result()
            .map_or(0, |snapshot| snapshot.status().entries().len())
    }
    pub(crate) fn move_selection(&mut self, delta: isize) -> bool {
        let total = self.total();
        self.view
            .select(self.view.selected.saturating_add_signed(delta), total)
    }
    pub(crate) fn first(&mut self) -> bool {
        let total = self.total();
        self.view.select(0, total)
    }
    pub(crate) fn last(&mut self) -> bool {
        let total = self.total();
        self.view.select(total.saturating_sub(1), total)
    }
    /// Only visible paths are decoded/escaped; selection never opens a raw path.
    pub(crate) fn prepare(&mut self, width: u16, height: u16) -> GitStatusPresentation {
        let message = self.message();
        let snapshot = if self.state() == GitStatusTaskState::Ready {
            self.controller.result()
        } else {
            None
        };
        self.view.prepare(snapshot, message, width, height)
    }
}

fn request_error(error: GitStatusRequestError) -> &'static str {
    match error {
        GitStatusRequestError::InvalidOptions => "Configuración de estado Git inválida",
        GitStatusRequestError::NoConfiguration => "Abre primero una carpeta de repositorio",
        GitStatusRequestError::GenerationExhausted => "Se agotó la generación del estado Git",
        GitStatusRequestError::WorkerUnavailable => "El lector de estado Git no está disponible",
    }
}
fn state_message(state: GitStatusTaskState, failure: Option<&GitStatusFailure>) -> &'static str {
    if matches!(
        failure,
        Some(GitStatusFailure::Provider(GitStatusError::Read(
            GitReadError::ConfiguredFilter
        )))
    ) {
        return "Filtros no admitidos";
    }
    match state {
        GitStatusTaskState::Idle => "Solicitar estado",
        GitStatusTaskState::Queued => "En cola",
        GitStatusTaskState::Running => "Leyendo",
        GitStatusTaskState::Ready => "Listo",
        GitStatusTaskState::Limited => "Límite alcanzado; actualiza",
        GitStatusTaskState::Cancelled => "Cancelado",
        GitStatusTaskState::Stale => "Obsoleto; actualiza",
        GitStatusTaskState::Failed => "Falló la lectura del estado",
        GitStatusTaskState::TimedOut => "Tiempo agotado; actualiza",
    }
}
fn change(change: GitStatusChange) -> char {
    match change {
        GitStatusChange::Unchanged => '.',
        GitStatusChange::Modified => 'M',
        GitStatusChange::Added => 'A',
        GitStatusChange::Deleted => 'D',
        GitStatusChange::Renamed => 'R',
        GitStatusChange::Copied => 'C',
        GitStatusChange::TypeChanged => 'T',
    }
}
fn flags(state: GitStatusState) -> String {
    match state {
        GitStatusState::Tracked { index, worktree } => {
            [change(index), change(worktree)].iter().collect()
        }
        GitStatusState::Untracked => "??".into(),
        GitStatusState::Ignored => "!!".into(),
        GitStatusState::Conflict(kind) => match kind {
            GitConflict::BothDeleted => "DD",
            GitConflict::AddedByUs => "AU",
            GitConflict::DeletedByThem => "UD",
            GitConflict::AddedByThem => "UA",
            GitConflict::DeletedByUs => "DU",
            GitConflict::BothAdded => "AA",
            GitConflict::BothModified => "UU",
        }
        .into(),
    }
}
fn plain_prefix(value: &str, width: usize) -> String {
    value.chars().take(width).collect()
}

fn escape_unicode(ch: char) -> bool {
    ch.is_control()
        || matches!(ch, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}
/// Decode at most one scalar (four bytes) at a time, never validate a whole long
/// path to display a short prefix. Escapes are indivisible at the clipping edge.
fn escaped_prefix(bytes: &[u8], width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut output = String::new();
    let mut offset = 0;
    let mut columns = 0;
    let mut previous = 0;
    while offset < bytes.len() {
        let first = bytes[offset];
        let len = match first {
            0..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 0,
        };
        let scalar = (len != 0)
            .then(|| bytes.get(offset..offset + len))
            .flatten()
            .and_then(|slice| std::str::from_utf8(slice).ok())
            .and_then(|s| s.chars().next());
        let (token, consumed) = if let Some(ch) = scalar {
            let token = match ch {
                '\\' => "\\\\".into(),
                '\n' => "\\n".into(),
                '\r' => "\\r".into(),
                '\t' => "\\t".into(),
                ch if escape_unicode(ch) => {
                    if ch.is_ascii() {
                        format!("\\x{:02X}", ch as u32)
                    } else {
                        format!("\\u{{{:X}}}", ch as u32)
                    }
                }
                ch => ch.to_string(),
            };
            (token, len)
        } else {
            (format!("\\x{first:02X}"), 1)
        };
        let count = token.chars().count();
        if columns + count > width {
            break;
        }
        previous = output.len();
        output.push_str(&token);
        columns += count;
        offset += consumed;
        if columns == width {
            break;
        }
    }
    if offset < bytes.len() {
        if columns == width {
            output.truncate(previous);
        }
        output.push('…');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::{
        git_read::GitRepositoryIdentity,
        git_status::{parse_porcelain_v1, GitStatusLimits},
    };
    use std::path::PathBuf;
    fn options(root: &str) -> GitStatusOptions {
        GitStatusOptions {
            root: PathBuf::from(root),
            limits: Default::default(),
        }
    }
    fn snapshot(input: &[u8]) -> GitStatusSnapshot {
        GitStatusSnapshot::from_parts(
            GitRepositoryIdentity {
                root: PathBuf::from("repo"),
                git_dir: PathBuf::from("repo/.git"),
            },
            parse_porcelain_v1(input, GitStatusLimits::default(), || false).unwrap(),
        )
    }
    #[test]
    fn paths_preserve_unicode_escape_bytes_controls_and_backslashes_without_partial_tokens() {
        let input = "niño/界".as_bytes();
        assert_eq!(escaped_prefix(input, 40), "niño/界");
        assert_eq!(
            escaped_prefix(b"x\xff\n\t\x1b\\", 64),
            "x\\xFF\\n\\t\\x1B\\\\"
        );
        assert_eq!(escaped_prefix("a\u{202e}b".as_bytes(), 64), "a\\u{202E}b");
        assert_eq!(escaped_prefix(b"\xffabc", 3), "…");
        assert_eq!(escaped_prefix(b"\xffabc", 4), "…");
        assert_eq!(escaped_prefix(b"a\xff", 4), "a…");
        assert_eq!(escaped_prefix(b"\xff", 4), "\\xFF");
        assert_eq!(escaped_prefix(b"abcdef", 0), "");
        assert_eq!(escaped_prefix(b"abcdef", 3), "ab…");
        for width in 0..24 {
            assert!(
                escaped_prefix(b"a\xff\nxyz\xf0\x9f\x98\x80tail", width)
                    .chars()
                    .count()
                    <= width
            );
        }
    }
    #[test]
    fn viewport_prepares_only_visible_entries_and_retains_selection_across_resize() {
        let mut input = Vec::new();
        for n in 0..2000 {
            input.extend(format!("?? file-{n}-{}\0", "x".repeat(100)).bytes());
        }
        let snapshot = snapshot(&input);
        let mut view = ViewState {
            options: Some(options("repo")),
            mode: Mode::Current,
            ..ViewState::default()
        };
        let first = view.prepare(Some(&snapshot), "Listo", 20, 4);
        assert_eq!(first.total, 2000);
        assert_eq!(first.rows.len(), 4);
        assert_eq!(first.rows[0].ordinal, 0);
        assert!(first.rows.iter().all(|r| r.path.chars().count() <= 16));
        assert!(view.select(1999, 2000));
        let last = view.prepare(Some(&snapshot), "Listo", 12, 3);
        assert_eq!(
            last.rows.iter().map(|r| r.ordinal).collect::<Vec<_>>(),
            vec![1997, 1998, 1999]
        );
        assert!(last.rows.last().unwrap().selected);
        let larger = view.prepare(Some(&snapshot), "Listo", 40, 8);
        assert_eq!(larger.rows.len(), 8);
        assert!(larger.rows.last().unwrap().selected);
        let hidden = view.prepare(Some(&snapshot), "Listo", 0, 0);
        assert!(hidden.rows.is_empty());
        assert_eq!(hidden.selected, 1999);
        let capped = view.prepare(Some(&snapshot), "Listo", u16::MAX, u16::MAX);
        assert_eq!(capped.rows.len(), MAX_ROWS);
        assert!(capped
            .rows
            .iter()
            .all(|row| row.path.chars().count() <= MAX_COLUMNS - 4));
    }
    #[test]
    fn accepted_rows_are_hidden_after_context_change_or_cancel_without_reinterpreting_paths() {
        let snapshot = snapshot(b"?? name\xff\n\0");
        let configured = options("root-a");
        let mut state = ViewState {
            options: Some(configured.clone()),
            mode: Mode::Current,
            ..ViewState::default()
        };
        assert_eq!(state.prepare(Some(&snapshot), "Listo", 40, 3).rows.len(), 1);
        assert!(!state.validate_context(Some(&configured.root)));
        assert!(state.validate_context(Some(Path::new("root-b"))));
        assert_eq!(state.mode, Mode::Stale);
        assert!(state.options.is_none());
        assert!(state
            .prepare(Some(&snapshot), "Stale", 40, 3)
            .rows
            .is_empty());
        state.options = Some(configured);
        state.mode = Mode::Cancelled;
        assert!(state
            .prepare(Some(&snapshot), "Cancelado", 40, 3)
            .rows
            .is_empty());
        assert!(state.validate_context(None));
        let configured = options("root-a");
        for root in [Some(Path::new("root-b")), None] {
            let mut state = ViewState {
                options: Some(configured.clone()),
                mode: Mode::Current,
                ..ViewState::default()
            };
            assert!(state.validate_context(root));
            assert!(state
                .prepare(Some(&snapshot), "Stale", 40, 3)
                .rows
                .is_empty());
        }
    }
    #[test]
    fn idle_clear_cancel_and_invalid_request_do_not_start_a_provider() {
        let mut view = GitStatusView::new(Wakeup::default());
        assert_eq!(view.state(), GitStatusTaskState::Idle);
        assert!(!view.ready());
        assert!(!view.poll(None));
        assert!(view.prepare(20, 5).rows.is_empty());
        view.cancel();
        assert_eq!(view.state(), GitStatusTaskState::Cancelled);
        view.clear();
        assert_eq!(view.state(), GitStatusTaskState::Idle);
        assert!(view.request(Path::new("relative-root")).is_err());
        assert_eq!(view.state(), GitStatusTaskState::Failed);
        assert!(!view.worker_panicked());
        assert!(!view.move_selection(isize::MAX));
        assert!(!view.first());
        assert!(!view.last());
    }
    #[test]
    fn headers_flags_and_filter_message_remain_bounded_and_explicit() {
        assert_eq!(
            (0..6).map(git_status_list_height).collect::<Vec<_>>(),
            vec![0, 1, 1, 1, 2, 3]
        );
        assert_eq!(
            flags(GitStatusState::Tracked {
                index: GitStatusChange::Modified,
                worktree: GitStatusChange::Unchanged
            }),
            "M."
        );
        assert_eq!(
            flags(GitStatusState::Conflict(GitConflict::DeletedByThem)),
            "UD"
        );
        assert_eq!(flags(GitStatusState::Untracked), "??");
        let failure =
            GitStatusFailure::Provider(GitStatusError::Read(GitReadError::ConfiguredFilter));
        assert_eq!(
            state_message(GitStatusTaskState::Failed, Some(&failure)),
            "Filtros no admitidos"
        );
        let mut view = ViewState::default();
        let frame = view.prepare(None, "Filtros no admitidos", 64, 4);
        assert!(frame.title.contains("integrado"));
        assert!(frame.message.contains(POLICY));
        assert!(frame.rows.is_empty());
    }
}

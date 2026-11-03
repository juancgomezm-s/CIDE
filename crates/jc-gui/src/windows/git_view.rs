//! Pure bounded projection of accepted Git read models. No request, tick,
//! admission, navigation, filesystem access or mutable document belongs here.
use jc_app::{
    diff_model::{DiffKind, DiffLine, DiffSide, OwnedDiffModel},
    git_diff::GitDiffController,
    git_read::GitBlobSource,
    git_status::{GitConflict, GitStatusChange, GitStatusEntry, GitStatusState},
    git_status_tasks::{GitStatusController, GitStatusTaskState},
};
use jc_text::{DecodedScalars, TextEncoding};

const ROWS: usize = 32;
const COLS: usize = 256;
const PREFIX: usize = 2048;

#[derive(Default)]
pub(super) struct GitView {
    status_selected: usize,
    status_total: usize,
    diff_scroll: usize,
    diff_total: usize,
    diff_page: usize,
}
pub(super) struct GitPresentation {
    pub title: String,
    pub message: String,
    pub rows: Vec<GitRow>,
    pub selected: usize,
    pub total: usize,
}
pub(super) struct GitRow {
    pub index: usize,
    pub label: String,
    pub selected: bool,
}

struct Label {
    text: String,
    limit: usize,
    clipped: bool,
}
impl Label {
    fn new(columns: usize) -> Self {
        let limit = columns.min(COLS);
        Self {
            text: String::with_capacity(limit),
            limit,
            clipped: false,
        }
    }
    fn token(&mut self, token: &str) {
        if self.clipped {
            return;
        }
        let remaining = self.limit - self.text.len();
        if token.len() > remaining {
            self.clip();
        } else {
            self.text.push_str(token);
        }
    }
    fn clip(&mut self) {
        if !self.clipped {
            for _ in 0..(self.limit - self.text.len()).min(3) {
                self.text.push('.');
            }
            self.clipped = true;
        }
    }
    fn character(&mut self, ch: char) {
        match ch {
            '\\' => self.token("\\\\"),
            ' '..='~' => self.token(&ch.to_string()),
            '\0'..='\u{1f}' | '\u{7f}' => self.token(&format!("\\x{:02X}", ch as u32)),
            _ => self.token(&format!("\\u{{{:X}}}", ch as u32)),
        }
    }
    fn plain(&mut self, text: &str) {
        for ch in text.chars() {
            if self.clipped || self.text.len() == self.limit {
                break;
            }
            self.character(ch);
        }
    }
    fn decoded(&mut self, bytes: &[u8], encoding: TextEncoding, start: usize, show_bom: bool) {
        let bom = usize::from(
            show_bom
                && start == 0
                && encoding == TextEncoding::Utf8
                && bytes.starts_with(b"\xef\xbb\xbf"),
        ) * 3;
        if bom > 0 {
            self.token("[BOM]");
        }
        let end = bytes.len().min(PREFIX + 4);
        let mut consumed = bom;
        for scalar in DecodedScalars::new(&bytes[bom..end], encoding, start.saturating_add(bom)) {
            if self.clipped || self.text.len() == self.limit {
                break;
            }
            let from = scalar.source_range.start - start;
            let to = scalar.source_range.end - start;
            if to > PREFIX {
                break;
            }
            if scalar.valid {
                self.character(scalar.character);
            } else {
                for byte in &bytes[from..to] {
                    self.token(&format!("\\x{byte:02X}"));
                }
            }
            consumed = to;
        }
        if consumed < bytes.len() {
            self.clip();
        }
    }
    fn bytes(&mut self, bytes: &[u8]) {
        // Path bytes use a nonzero origin so an initial UTF8 BOM is escaped,
        // rather than being interpreted as file encoding metadata and stripped.
        self.decoded(bytes, TextEncoding::Utf8, 1, false);
    }
}
fn bounded(text: &str, cols: usize) -> String {
    jc_app::source_services::display_ui_label(text, cols.min(COLS))
}
fn shift(current: usize, delta: isize, max: usize) -> usize {
    if delta < 0 {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as usize)
    }
    .min(max)
}
fn change(value: GitStatusChange) -> char {
    match value {
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

#[derive(Clone, Copy)]
struct Segment {
    sign: char,
    left: Option<usize>,
    right: Option<usize>,
    len: usize,
}
fn ending(bytes: &[u8]) -> (&[u8], &'static str) {
    if bytes.ends_with(b"\r\n") {
        (&bytes[..bytes.len() - 2], " [CRLF]")
    } else if bytes.ends_with(b"\n") {
        (&bytes[..bytes.len() - 1], " [LF]")
    } else if bytes.ends_with(b"\r") {
        (&bytes[..bytes.len() - 1], " [CR]")
    } else {
        (bytes, " [EOF]")
    }
}
fn source_row(
    model: &OwnedDiffModel,
    segment: Segment,
    offset: usize,
    encoding: TextEncoding,
    cols: usize,
) -> String {
    let left = segment.left.map(|line| line + offset);
    let right = segment.right.map(|line| line + offset);
    let (side, line) = right.map_or((DiffSide::Left, left.unwrap_or(0)), |line| {
        (DiffSide::Right, line)
    });
    let Some(range) = model.line_byte_range(side, DiffLine(line)) else {
        return bounded("Fila de fuente no disponible", cols);
    };
    let (content, eol) = ending(&model.source_bytes(side)[range.clone()]);
    let columns = cols.min(COLS);
    if eol.len() > columns {
        let mut out = Label::new(columns);
        out.token(eol);
        return out.text;
    }
    let mut out = Label::new(columns - eol.len());
    let prefix = match (left, right) {
        (Some(left), Some(right)) => format!("  {}:{} | ", left + 1, right + 1),
        (Some(left), None) => format!("{} {} | ", segment.sign, left + 1),
        (_, Some(right)) => format!("{} {} | ", segment.sign, right + 1),
        _ => String::new(),
    };
    out.plain(&prefix);
    out.decoded(content, encoding, range.start, true);
    out.text.push_str(eol);
    out.text
}

impl GitView {
    pub(super) fn first_status(&mut self) {
        self.status_selected = 0;
    }
    pub(super) fn last_status(&mut self) {
        self.status_selected = self.status_total.saturating_sub(1);
    }
    pub(super) fn move_status(&mut self, delta: isize) {
        self.status_selected = shift(
            self.status_selected,
            delta,
            self.status_total.saturating_sub(1),
        );
    }
    pub(super) fn first_diff(&mut self) {
        self.diff_scroll = 0;
    }
    pub(super) fn last_diff(&mut self) {
        self.diff_scroll = self.diff_total.saturating_sub(self.diff_page);
    }
    pub(super) fn scroll_diff(&mut self, delta: isize) {
        self.diff_scroll = shift(
            self.diff_scroll,
            delta,
            self.diff_total.saturating_sub(self.diff_page),
        );
    }
    pub(super) fn prepare_status(
        &mut self,
        controller: &GitStatusController,
        max_rows: usize,
        max_cols: usize,
    ) -> GitPresentation {
        let entries = if controller.state() == GitStatusTaskState::Ready {
            controller
                .result()
                .map_or(&[][..], |r| r.status().entries())
        } else {
            &[]
        };
        let mut p = self.status_rows(entries, max_rows, max_cols);
        p.message = bounded(
            &format!(
                "{}: {} | solo lectura; sin renombrados/submódulos",
                controller.state().label(),
                controller.message()
            ),
            max_cols,
        );
        p
    }
    fn status_rows(
        &mut self,
        entries: &[GitStatusEntry],
        max_rows: usize,
        max_cols: usize,
    ) -> GitPresentation {
        self.status_total = entries.len();
        self.status_selected = self
            .status_selected
            .min(self.status_total.saturating_sub(1));
        let count = max_rows.min(ROWS).min(entries.len());
        let first = if count == 0 {
            0
        } else {
            self.status_selected
                .saturating_sub(count - 1)
                .min(entries.len() - count)
        };
        let rows = entries[first..first + count]
            .iter()
            .enumerate()
            .map(|(offset, entry)| {
                let index = first + offset;
                let mut out = Label::new(max_cols);
                out.plain(&flags(entry.state()));
                out.plain(" ");
                out.bytes(entry.path());
                if let Some(original) = entry.original_path() {
                    out.plain(" <- ");
                    out.bytes(original);
                }
                GitRow {
                    index,
                    label: out.text,
                    selected: index == self.status_selected,
                }
            })
            .collect();
        GitPresentation {
            title: bounded("Estado del repositorio Git (solo lectura)", max_cols),
            message: String::new(),
            rows,
            selected: self.status_selected,
            total: self.status_total,
        }
    }
    pub(super) fn prepare_diff(
        &mut self,
        controller: &GitDiffController,
        encoding: Option<TextEncoding>,
        max_rows: usize,
        max_cols: usize,
    ) -> GitPresentation {
        let Some(result) = controller.result() else {
            self.diff_scroll = 0;
            self.diff_total = 0;
            self.diff_page = 0;
            return GitPresentation {
                title: bounded("Diferencias Git (solo lectura)", max_cols),
                message: bounded(
                    &format!("{}: {}", controller.state().label(), controller.message()),
                    max_cols,
                ),
                rows: Vec::new(),
                selected: 0,
                total: 0,
            };
        };
        let identity = &result.identity;
        let source = match identity.source {
            GitBlobSource::Head => "HEAD",
            GitBlobSource::Index => "índice",
        };
        let total = result.model.hunks().count();
        let selected = controller.selected().min(total.saturating_sub(1));
        let mut message = Label::new(max_cols);
        if let Some(commit) = &identity.commit {
            message.plain("commit ");
            message.plain(commit.as_str());
            message.plain(" | ");
        }
        message.plain(&format!(
            "{} | bloque {}/{} | ",
            controller.state().label(),
            if total == 0 { 0 } else { selected + 1 },
            total
        ));
        message.plain(&identity.path);
        let rows = if let Some(encoding) = encoding {
            self.model_rows(&result.model, selected, encoding, max_rows, max_cols)
        } else {
            self.diff_scroll = 0;
            self.diff_total = 0;
            self.diff_page = 0;
            if max_rows == 0 {
                Vec::new()
            } else {
                vec![GitRow {
                    index: selected,
                    label: bounded(
                        "Elige codificación explícita para mostrar prefijos de fuente",
                        max_cols,
                    ),
                    selected: false,
                }]
            }
        };
        GitPresentation {
            title: bounded(
                &format!("Blob Git {source} {}", identity.blob.as_str()),
                max_cols,
            ),
            message: message.text,
            rows,
            selected,
            total,
        }
    }
    fn model_rows(
        &mut self,
        model: &OwnedDiffModel,
        selected: usize,
        encoding: TextEncoding,
        max_rows: usize,
        max_cols: usize,
    ) -> Vec<GitRow> {
        let rows = max_rows.min(ROWS);
        let Some(hunk) = model.hunks().nth(selected) else {
            self.diff_scroll = 0;
            self.diff_total = 0;
            self.diff_page = 0;
            return if rows == 0 {
                Vec::new()
            } else {
                vec![GitRow {
                    index: 0,
                    label: bounded(
                        "No hay diferencias de bytes en las capturas aceptadas",
                        max_cols,
                    ),
                    selected: false,
                }]
            };
        };
        let span_index = model
            .spans()
            .iter()
            .position(|s| std::ptr::eq(s, hunk))
            .unwrap();
        let before = span_index
            .checked_sub(1)
            .and_then(|i| model.spans().get(i))
            .filter(|s| s.kind == DiffKind::Equal)
            .map_or(0, |s| (s.left.end.0 - s.left.start.0).min(3));
        let after = model
            .spans()
            .get(span_index + 1)
            .filter(|s| s.kind == DiffKind::Equal)
            .map_or(0, |s| (s.left.end.0 - s.left.start.0).min(3));
        let left_count = hunk.left.end.0 - hunk.left.start.0;
        let right_count = hunk.right.end.0 - hunk.right.start.0;
        let segments = [
            Segment {
                sign: ' ',
                left: Some(hunk.left.start.0 - before),
                right: Some(hunk.right.start.0 - before),
                len: before,
            },
            Segment {
                sign: '-',
                left: Some(hunk.left.start.0),
                right: None,
                len: left_count,
            },
            Segment {
                sign: '+',
                left: None,
                right: Some(hunk.right.start.0),
                len: right_count,
            },
            Segment {
                sign: ' ',
                left: Some(hunk.left.end.0),
                right: Some(hunk.right.end.0),
                len: after,
            },
        ];
        self.diff_total = segments.iter().map(|s| s.len).sum();
        self.diff_page = rows.saturating_sub(usize::from(rows > 1));
        self.diff_scroll = self
            .diff_scroll
            .min(self.diff_total.saturating_sub(self.diff_page));
        let mut output = Vec::with_capacity(rows);
        if rows > 1 {
            output.push(GitRow {
                index: selected,
                label: bounded(
                    &format!(
                        "@@ -{},{} +{},{} @@",
                        hunk.left.start.0 + usize::from(left_count > 0),
                        left_count,
                        hunk.right.start.0 + usize::from(right_count > 0),
                        right_count
                    ),
                    max_cols,
                ),
                selected: true,
            });
        }
        for logical in self.diff_scroll
            ..self
                .diff_scroll
                .saturating_add(self.diff_page)
                .min(self.diff_total)
        {
            let mut offset = logical;
            for segment in segments {
                if offset >= segment.len {
                    offset -= segment.len;
                    continue;
                }
                output.push(GitRow {
                    index: logical,
                    label: source_row(model, segment, offset, encoding, max_cols),
                    selected: false,
                });
                break;
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::{diff_model::DiffLimits, git_status::parse_porcelain_v1, Wakeup};
    use std::sync::Arc;
    fn model(left: &[u8], right: &[u8]) -> OwnedDiffModel {
        OwnedDiffModel::compute(
            Arc::new(left.to_vec()),
            Arc::new(right.to_vec()),
            DiffLimits::default(),
            || false,
        )
    }
    #[test]
    fn typed_status_flags_raw_paths_and_selection_are_bounded_and_read_only() {
        let input=b" M normal.txt\0MM ni\xc3\xb1o\t\xff.txt\0UU conflict.txt\0?? new.txt\0!! ignored.txt\0";
        let parsed = parse_porcelain_v1(input, Default::default(), || false).unwrap();
        let mut view = GitView::default();
        let first = view.status_rows(parsed.entries(), 32, 140);
        assert_eq!(first.rows[0].label, ".M normal.txt");
        assert_eq!(first.rows[1].label, r"MM ni\u{F1}o\x09\xFF.txt");
        assert_eq!(first.rows[2].label, "UU conflict.txt");
        assert_eq!(first.rows[3].label, "?? new.txt");
        assert_eq!(first.rows[4].label, "!! ignored.txt");
        view.last_status();
        view.move_status(-1);
        let last = view.status_rows(parsed.entries(), 1, 45);
        assert_eq!(last.selected, 3);
        assert_eq!(last.rows[0].index, 3);
        assert!(last.rows[0].selected);
        view.move_status(isize::MIN);
        view.first_status();
        assert_eq!(view.status_rows(parsed.entries(), 0, 0).selected, 0);
        assert_eq!(parsed.entries()[1].path(), b"ni\xc3\xb1o\t\xff.txt");
    }
    #[test]
    fn diff_preserves_bom_invalid_legacy_and_exact_ending_markers() {
        let left = b"\xef\xbb\xbfa\xff\r\nold\r";
        let right = "\u{feff}niño\nlast".as_bytes();
        let m = model(left, right);
        let mut view = GitView::default();
        let rows = view.model_rows(&m, 0, TextEncoding::Utf8, 32, 140);
        assert!(rows.iter().any(|r| r.label.contains("[BOM]a\\xFF [CRLF]")));
        assert!(rows.iter().any(|r| r.label.contains("old [CR]")));
        assert!(rows.iter().any(|r| r.label.contains(r"ni\u{F1}o [LF]")));
        assert!(rows.iter().any(|r| r.label.contains("last [EOF]")));
        let legacy = model(b"ni\xf1o\r\n", b"changed\r\n");
        assert!(view
            .model_rows(&legacy, 0, TextEncoding::Windows1252, 32, 140)
            .iter()
            .any(|r| r.label.contains(r"ni\u{F1}o [CRLF]")));
        assert_eq!(m.source_bytes(DiffSide::Left), left);
        assert_eq!(m.source_bytes(DiffSide::Right), right);
    }
    #[test]
    fn huge_hunk_end_then_up_resize_and_prefix_clipping_are_bounded() {
        let right = b"new\n".repeat(600);
        let m = model(b"", &right);
        let mut view = GitView::default();
        view.model_rows(&m, 0, TextEncoding::Utf8, 10, 45);
        view.last_diff();
        let end = view.model_rows(&m, 0, TextEncoding::Utf8, 10, 45);
        assert!(end.last().unwrap().label.starts_with("+ 600 |"));
        view.scroll_diff(-1);
        let up = view.model_rows(&m, 0, TextEncoding::Utf8, 10, 45);
        assert!(up.last().unwrap().label.starts_with("+ 599 |"));
        for (rows, cols) in [(0, 0), (1, 1), (32, 140), (usize::MAX, usize::MAX)] {
            let p = view.model_rows(&m, 0, TextEncoding::Utf8, rows, cols);
            assert!(p.len() <= rows.min(32));
            assert!(p.iter().all(|r| r.label.len() <= cols.min(256)
                && r.label.is_ascii()
                && !r.label.chars().any(char::is_control)));
        }
        view.first_diff();
        let huge = model(b"", &b"x".repeat(64 * 1024));
        let rows = view.model_rows(&huge, 0, TextEncoding::Utf8, 32, usize::MAX);
        assert!(rows.last().unwrap().label.ends_with(" [EOF]"));
        assert!(rows.last().unwrap().label.len() <= 256);
        let mut l = Label::new(5);
        l.plain("xé");
        assert_eq!(l.text, "x...");
    }
    #[test]
    fn context_is_three_lines_and_idle_controllers_do_not_start_work() {
        use std::fmt::Write;
        let mut left = String::new();
        for i in 0..20 {
            write!(&mut left, "line{i}\r\n").unwrap();
        }
        let right = left.replace("line10", "CHANGED");
        let m = model(left.as_bytes(), right.as_bytes());
        let mut view = GitView::default();
        let rows = view.model_rows(&m, 0, TextEncoding::Utf8, 32, 140);
        assert_eq!(rows.len(), 9);
        assert!(rows[1].label.starts_with("  8:8 |"));
        assert!(rows.last().unwrap().label.starts_with("  14:14 |"));
        let status = GitStatusController::new(Wakeup::default());
        let diff = GitDiffController::new();
        assert!(view
            .prepare_status(&status, usize::MAX, 256)
            .rows
            .is_empty());
        assert!(view.prepare_diff(&diff, None, 32, 256).rows.is_empty());
        assert_eq!(status.state(), GitStatusTaskState::Idle);
    }
}

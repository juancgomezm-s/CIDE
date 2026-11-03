//! Bounded, read-only projection of S1 results. Paths and source coordinates
//! remain authoritative in ProjectSearch; labels are never navigation inputs.
#[cfg(test)]
use jc_app::ProjectSearchState;
use jc_app::{ProjectSearch, ProjectSearchRow};
use std::path::Path;

const ROW_LIMIT: usize = 32;
const COLUMN_LIMIT: usize = 256;

#[derive(Default)]
pub(super) struct ProjectResultsView {
    selected: usize,
    total: usize,
}
pub(super) struct ProjectResultsPresentation {
    pub title: String,
    pub message: String,
    pub rows: Vec<ProjectResultRow>,
    pub selected: usize,
    pub total: usize,
}
pub(super) struct ProjectResultRow {
    pub index: usize,
    pub label: String,
    pub selected: bool,
    /// A match eligible for an attempted navigation. Stale results and issues
    /// are false; the caller must still invoke ProjectSearch::select to validate.
    pub is_match: bool,
}

fn visible(selected: usize, total: usize, max_rows: usize) -> std::ops::Range<usize> {
    let count = max_rows.min(ROW_LIMIT).min(total);
    if count == 0 {
        return 0..0;
    }
    let first = selected
        .min(total - 1)
        .saturating_sub(count - 1)
        .min(total - count);
    first..first + count
}

/// ASCII-only builder. Escape tokens are indivisible even at a narrow edge.
/// Each decoding attempt examines at most four bytes, never the complete path
/// or snippet merely to produce a short visible prefix.
struct Label {
    text: String,
    limit: usize,
    clipped: bool,
}
impl Label {
    fn new(max_cols: usize) -> Self {
        let limit = max_cols.min(COLUMN_LIMIT);
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
            for _ in 0..remaining.min(3) {
                self.text.push('.');
            }
            self.clipped = true;
        } else {
            self.text.push_str(token);
        }
    }
    fn bytes(&mut self, bytes: &[u8]) {
        let mut at = 0;
        while at < bytes.len() && !self.clipped {
            if self.text.len() == self.limit {
                self.clipped = true;
                break;
            }
            let first = bytes[at];
            let length = match first {
                0..=0x7f => 1,
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => 0,
            };
            let scalar = if length > 0 && length <= bytes.len() - at {
                std::str::from_utf8(&bytes[at..at + length])
                    .ok()
                    .and_then(|text| text.chars().next())
            } else {
                None
            };
            if let Some(ch) = scalar {
                match ch {
                    '\\' => self.token("\\\\"),
                    ' '..='~' => self.token(&ch.to_string()),
                    '\0'..='\u{1f}' | '\u{7f}' => {
                        self.token(&format!("\\x{:02X}", ch as u32));
                    }
                    _ => self.token(&format!("\\u{{{:X}}}", ch as u32)),
                }
                at += length;
            } else {
                self.token(&format!("\\x{first:02X}"));
                at += 1;
            }
        }
    }
    fn text(&mut self, text: &str) {
        self.bytes(text.as_bytes());
    }
    fn path(&mut self, path: &Path, root: Option<&Path>) {
        let path = root
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path);
        // OsStr's platform encoding is used only for display, not converted back
        // to a path. Invalid UTF-8/WTF-8 bytes are explicit hex escapes.
        self.bytes(path.as_os_str().as_encoded_bytes());
    }
}

fn label(row: &ProjectSearchRow, index: usize, root: Option<&Path>, cols: usize) -> String {
    let mut out = Label::new(cols);
    out.text(&format!("{} ", index.saturating_add(1)));
    match row {
        ProjectSearchRow::Match(hit) => {
            out.text(&format!(
                "Lin {} Col {} ",
                hit.position.line.saturating_add(1),
                hit.position.column.saturating_add(1)
            ));
            out.path(&hit.path, root);
            out.text(" | ");
            out.text(&hit.snippet);
        }
        ProjectSearchRow::Issue { path, message } => {
            out.text("! ");
            out.path(path, root);
            out.text(" | ");
            out.text(message);
        }
    }
    out.text
}

impl ProjectResultsView {
    fn normalize(&mut self, total: usize) {
        self.total = total;
        self.selected = self.selected.min(total.saturating_sub(1));
    }
    pub(super) fn move_selection(&mut self, delta: isize) {
        self.selected = if delta < 0 {
            self.selected.saturating_sub(delta.unsigned_abs())
        } else {
            self.selected.saturating_add(delta as usize)
        }
        .min(self.total.saturating_sub(1));
    }
    pub(super) fn first(&mut self) {
        self.selected = 0;
    }
    pub(super) fn last(&mut self) {
        self.selected = self.total.saturating_sub(1);
    }
    pub(super) fn selected(&self) -> usize {
        self.selected
    }
    pub(super) fn prepare(
        &mut self,
        controller: &ProjectSearch,
        max_rows: usize,
        max_cols: usize,
    ) -> ProjectResultsPresentation {
        let source = controller.rows();
        self.normalize(source.len());
        let stale = controller.is_stale();
        let rows = visible(self.selected, self.total, max_rows)
            .map(|index| ProjectResultRow {
                index,
                label: label(&source[index], index, controller.root(), max_cols),
                selected: index == self.selected,
                is_match: !stale && matches!(&source[index], ProjectSearchRow::Match(_)),
            })
            .collect();
        let state = controller.state.label();
        let mut message = Label::new(max_cols);
        message.text(&format!(
            "{state} | {} filas | {}",
            self.total,
            if stale {
                "OBSOLETO: actualiza antes de abrir"
            } else {
                "Lin/Col son posiciones escalares"
            }
        ));
        ProjectResultsPresentation {
            title: "Search Plus: proyecto".into(),
            message: message.text,
            rows,
            selected: self.selected,
            total: self.total,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::Wakeup;

    #[test]
    fn idle_and_state_presentation_are_bounded_without_starting_a_worker() {
        let mut controller = ProjectSearch::new(Wakeup::default());
        let mut view = ProjectResultsView::default();
        for state in [
            ProjectSearchState::Idle,
            ProjectSearchState::Running,
            ProjectSearchState::Complete,
            ProjectSearchState::Limited,
            ProjectSearchState::Cancelled,
            ProjectSearchState::Failed,
        ] {
            controller.state = state;
            for (rows, cols) in [(0, 0), (1, 1), (32, 45), (usize::MAX, usize::MAX)] {
                let p = view.prepare(&controller, rows, cols);
                assert!(p.rows.is_empty());
                assert_eq!((p.total, p.selected, view.selected()), (0, 0, 0));
                assert!(p.message.len() <= cols.min(COLUMN_LIMIT));
                assert!(p
                    .message
                    .chars()
                    .all(|ch| ch.is_ascii() && !ch.is_control()));
                assert_eq!(p.title, "Search Plus: proyecto");
            }
        }
    }

    #[test]
    fn selection_and_viewport_keep_end_then_up_visible_under_resizing_and_reset() {
        let mut view = ProjectResultsView::default();
        view.normalize(10_000);
        view.last();
        view.move_selection(-1);
        assert_eq!(view.selected(), 9998);
        for height in [0, 1, 4, 32, usize::MAX] {
            let range = visible(view.selected(), view.total, height);
            assert!(range.len() <= height.min(32));
            assert!(range.end <= 10_000);
            assert!(range.is_empty() || range.contains(&9998));
        }
        view.move_selection(isize::MAX);
        assert_eq!(view.selected(), 9999);
        view.move_selection(isize::MIN);
        assert_eq!(view.selected(), 0);
        view.last();
        view.normalize(2);
        assert_eq!(view.selected(), 1);
        view.first();
        assert_eq!(view.selected(), 0);
        view.normalize(0);
        view.last();
        assert_eq!(view.selected(), 0);
        assert!(visible(usize::MAX, 0, usize::MAX).is_empty());
    }

    #[test]
    fn labels_escape_invalid_controls_bidi_unicode_and_paths_without_partial_tokens() {
        let mut escaped = Label::new(256);
        escaped.bytes(b"a\\\0\r\n\t\x7f\xff\xc0");
        escaped.text("é\u{200b}\u{202e}😀");
        assert_eq!(
            escaped.text,
            r"a\\\x00\x0D\x0A\x09\x7F\xFF\xC0\u{E9}\u{200B}\u{202E}\u{1F600}"
        );
        let issue = ProjectSearchRow::Issue {
            path: Path::new("root").join("niño.txt"),
            message: "bad\r\n\t\u{202e}".into(),
        };
        let full = label(&issue, 0, Some(Path::new("root")), 256);
        assert_eq!(full, r"1 ! ni\u{F1}o.txt | bad\x0D\x0A\x09\u{202E}");
        for width in [0, 1, 2, 3, 4, 7, 45, 140, 256, usize::MAX] {
            let text = label(&issue, usize::MAX, None, width);
            assert!(text.len() <= width.min(256));
            assert!(text.chars().all(|ch| ch.is_ascii() && !ch.is_control()));
        }
        let mut clipped = Label::new(5);
        clipped.text("xé");
        assert_eq!(clipped.text, "x..."); // Never a partial \\u{...} escape.
        let mut absurd = Label::new(256);
        absurd.text(&"é".repeat(100_000));
        assert!(absurd.text.len() <= 256);
        assert!(!absurd.text.contains('é'));
    }
}

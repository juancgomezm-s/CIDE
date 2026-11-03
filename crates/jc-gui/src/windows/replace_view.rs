//! Read-only, bounded display of the core's immutable replacement preview.
//! Labels are presentation only: never parse them into paths or apply edits here.
use jc_app::ProjectReplacePreview;

const MAX_ROWS: usize = 32;
const MAX_COLUMNS: usize = 256;

#[derive(Default)]
pub(super) struct ReplaceView {
    selected: usize,
    total: usize,
}
pub(super) struct ReplacePresentation {
    pub title: String,
    pub message: String,
    pub rows: Vec<ReplaceRow>,
    pub selected: usize,
    pub total: usize,
}
pub(super) struct ReplaceRow {
    pub index: usize,
    pub label: String,
    pub selected: bool,
}

/// A bounded prefix with atomic escape tokens. Iteration stops at the visible
/// budget; it does not inspect the rest of an arbitrarily long preview line.
fn escaped(text: &str, max_cols: usize) -> String {
    let limit = max_cols.min(MAX_COLUMNS);
    let mut output = String::with_capacity(limit);
    for ch in text.chars() {
        let remaining = limit - output.len();
        if remaining == 0 {
            break;
        }
        let token = match ch {
            '\\' => "\\\\".into(),
            ' '..='~' => ch.to_string(),
            '\0'..='\u{1f}' | '\u{7f}' => format!("\\x{:02X}", ch as u32),
            _ => format!("\\u{{{:X}}}", ch as u32),
        };
        if token.len() > remaining {
            for _ in 0..remaining.min(3) {
                output.push('.');
            }
            break;
        }
        output.push_str(&token);
    }
    output
}

impl ReplaceView {
    pub(super) fn first(&mut self) {
        self.selected = 0;
    }
    pub(super) fn move_selection(&mut self, delta: isize) {
        self.selected = if delta < 0 {
            self.selected.saturating_sub(delta.unsigned_abs())
        } else {
            self.selected.saturating_add(delta as usize)
        }
        .min(self.total.saturating_sub(1));
    }
    pub(super) fn last(&mut self) {
        self.selected = self.total.saturating_sub(1);
    }
    pub(super) fn prepare(
        &mut self,
        preview: &ProjectReplacePreview,
        max_rows: usize,
        max_cols: usize,
    ) -> ReplacePresentation {
        self.prepare_lines(
            preview.lines(),
            preview.files_remaining(),
            preview.matches(),
            max_rows,
            max_cols,
        )
    }
    fn prepare_lines(
        &mut self,
        lines: &[String],
        files: usize,
        matches: usize,
        max_rows: usize,
        max_cols: usize,
    ) -> ReplacePresentation {
        self.total = lines.len();
        self.selected = self.selected.min(self.total.saturating_sub(1));
        let count = max_rows.min(MAX_ROWS).min(self.total);
        let first = if count == 0 {
            0
        } else {
            self.selected
                .saturating_sub(count - 1)
                .min(self.total - count)
        };
        let rows = lines[first..first + count]
            .iter()
            .enumerate()
            .map(|(offset, line)| {
                let index = first + offset;
                ReplaceRow {
                    index,
                    label: escaped(line, max_cols),
                    selected: index == self.selected,
                }
            })
            .collect();
        ReplacePresentation {
            title: escaped("Vista previa del reemplazo de proyecto", max_cols),
            message: escaped(
                &format!(
                    "{matches} coincidencias | {files} archivos pendientes | requiere confirmación explícita"
                ),
                max_cols,
            ),
            rows,
            selected: self.selected,
            total: self.total,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_then_up_remains_visible_through_resizing_and_preview_replacement() {
        let lines: Vec<_> = (0..100).map(|index| format!("line {index}")).collect();
        let mut view = ReplaceView::default();
        view.prepare_lines(&lines, 2, 25, 12, 80);
        view.last();
        view.move_selection(-1);
        for height in [0, 1, 8, 32, usize::MAX] {
            let p = view.prepare_lines(&lines, 2, 25, height, 45);
            assert_eq!((p.selected, p.total), (98, 100));
            assert!(p.rows.len() <= height.min(32));
            if height > 0 {
                let selected: Vec<_> = p.rows.iter().filter(|row| row.selected).collect();
                assert_eq!(selected.len(), 1);
                assert_eq!(selected[0].index, 98);
                assert_eq!(selected[0].label, "line 98");
            }
            assert!(p
                .rows
                .windows(2)
                .all(|pair| pair[1].index == pair[0].index + 1));
        }
        view.move_selection(isize::MAX);
        assert_eq!(view.prepare_lines(&lines, 2, 25, 1, 140).selected, 99);
        view.move_selection(isize::MIN);
        assert_eq!(view.prepare_lines(&lines, 2, 25, 1, 140).selected, 0);
        view.last();
        assert_eq!(view.prepare_lines(&lines[..2], 1, 1, 32, 140).selected, 1);
        view.first();
        assert_eq!(view.prepare_lines(&lines, 2, 25, 1, 45).selected, 0);
    }

    #[test]
    fn escaped_labels_bound_unicode_controls_and_never_cut_escape_tokens() {
        assert_eq!(
            escaped("\\\0\r\n\t\u{7f}é\u{200b}\u{202e}😀", 256),
            r"\\\x00\x0D\x0A\x09\x7F\u{E9}\u{200B}\u{202E}\u{1F600}"
        );
        assert_eq!(escaped("xé", 5), "x...");
        assert_eq!(escaped("\\", 1), ".");
        assert_eq!(escaped("é", 6), r"\u{E9}");
        let lines = vec!["\u{202e}".repeat(50_000), "x".repeat(50_000)];
        let mut view = ReplaceView::default();
        for columns in [0, 1, 45, 140, 256, usize::MAX] {
            let p = view.prepare_lines(&lines, usize::MAX, usize::MAX, usize::MAX, columns);
            for value in std::iter::once(&p.title)
                .chain(std::iter::once(&p.message))
                .chain(p.rows.iter().map(|row| &row.label))
            {
                assert!(value.len() <= columns.min(256));
                assert!(value.chars().all(|ch| ch.is_ascii() && !ch.is_control()));
            }
        }
    }

    #[test]
    fn empty_and_repeated_borrowed_preparation_leave_preview_lines_unchanged() {
        let mut view = ReplaceView::default();
        view.last();
        view.move_selection(isize::MIN);
        let empty = view.prepare_lines(&[], 0, 0, 32, 140);
        assert!(empty.rows.is_empty());
        assert_eq!((empty.selected, empty.total), (0, 0));
        assert!(empty.message.contains("0 archivos pendientes"));
        let lines = vec![
            "file.txt (1 coincidencias)".into(),
            "- old\r\nline".into(),
            "+ niño".into(),
        ];
        let original = lines.clone();
        let first = view.prepare_lines(&lines, 1, 1, 32, 140);
        let second = view.prepare_lines(&lines, 1, 1, 32, 140);
        assert_eq!(first.title, "Vista previa del reemplazo de proyecto");
        assert!(first
            .message
            .contains("1 coincidencias | 1 archivos pendientes"));
        assert_eq!(first.message, second.message);
        assert_eq!(
            first.rows.iter().map(|row| &row.label).collect::<Vec<_>>(),
            second.rows.iter().map(|row| &row.label).collect::<Vec<_>>()
        );
        assert_eq!(lines, original);
        assert_eq!(view.prepare_lines(&[], 0, 0, 1, 45).selected, 0);
    }
}

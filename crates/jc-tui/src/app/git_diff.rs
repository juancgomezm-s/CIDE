//! Git UI coordination and bounded preparation; rendering consumes the returned
//! read-only rows and never reaches into a live document or immutable source.
use super::{GitDiffRow, GitHunkRow, GitPresentation, GitRowKind, TuiApp};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{
    diff_model::{DiffKind, DiffLine, DiffSide, DiffSpan, OwnedDiffModel},
    git_diff::GitDiffOptions,
    git_read::GitBlobSource,
    BottomPanel, PrimaryFocus, SidebarView,
};
use jc_text::{DecodedScalars, TextEncoding};

const MAX_ROWS: usize = 256;
const PREFIX_BYTES: usize = 2048;
const CONTEXT_LINES: usize = 3;

impl TuiApp {
    pub(crate) fn request_git_diff(&mut self, source: GitBlobSource) {
        self.git_revision.cancel();
        self.git_history = Default::default();
        self.git_source = source;
        self.git_diff_scroll = 0;
        let Some(root) = self.project_root.clone() else {
            self.git_diff.clear(&mut self.tasks);
            self.status = "Abre la carpeta raíz del repositorio antes de comparar".into();
            return;
        };
        let options = GitDiffOptions {
            root,
            cwd: self.git_cwd.clone(),
            source,
        };
        self.status = match self
            .git_diff
            .request(&self.workbench, &mut self.tasks, options)
        {
            Ok(()) => self.git_diff.message().to_owned(),
            Err(error) => error,
        };
    }

    pub(crate) fn open_git_diff(&mut self) {
        self.panels.show_sidebar(SidebarView::Git);
        self.panels.show_bottom(BottomPanel::Diff);
        let _ = self.focus.focus(PrimaryFocus::Sidebar, &self.panels);
        self.request_git_diff(self.git_source);
    }

    /// The integrator ticks the versioned controller before preparation. This
    /// method only borrows accepted immutable sources and metadata and normalizes
    /// its viewport. No I/O, snapshot copy, admission or selection mutation.
    pub(crate) fn prepare_git_diff(
        &mut self,
        width: u16,
        rows: usize,
        hunk_rows: usize,
    ) -> GitPresentation {
        let limit = rows.min(MAX_ROWS);
        let hunk_limit = hunk_rows.min(MAX_ROWS);
        let mut prepared = GitPresentation {
            status: plain_prefix(
                &format!("{:?}: {}", self.git_diff.state(), self.git_diff.message()),
                width,
            ),
            ..GitPresentation::default()
        };
        let Some(result) = self.git_diff.result() else {
            let source = match self.git_source {
                GitBlobSource::Head => "HEAD",
                GitBlobSource::Index => "índice",
            };
            prepared.details.push(plain_prefix(
                &format!("{source} frente al documento activo; solo lectura"),
                width,
            ));
            if self.project_root.is_none() {
                prepared
                    .details
                    .push(plain_prefix("Abre la carpeta raíz del repositorio", width));
            }
            return prepared;
        };
        let identity = &result.identity;
        let source = match identity.source {
            GitBlobSource::Head => "Copia de HEAD",
            GitBlobSource::Index => "Copia del índice",
        };
        prepared
            .details
            .push(plain_prefix(&format!("{source}: {}", identity.path), width));
        prepared.details.push(plain_prefix(
            &format!("blob {}", identity.blob.as_str()),
            width,
        ));
        if let Some(commit) = &identity.commit {
            prepared
                .details
                .push(plain_prefix(&format!("commit {}", commit.as_str()), width));
        }
        let Some(document) = self.workbench.active_document() else {
            prepared.details.push(plain_prefix(
                "El documento comparado ya no está activo",
                width,
            ));
            return prepared;
        };
        let Some(encoding) = document.session.encoding() else {
            prepared.details.push(plain_prefix(
                "Elige una codificación para mostrar los prefijos de la fuente",
                width,
            ));
            return prepared;
        };
        prepared.details.push(plain_prefix(
            &format!(
                "Revisión de trabajo {} | {}",
                document.session.document().revision(),
                encoding.name()
            ),
            width,
        ));
        prepared.details.push(plain_prefix(
            "Identidad de la copia; actualiza para volver a leer HEAD/índice",
            width,
        ));
        prepared.total_hunks = result.model.hunks().count();
        let selected = self
            .git_diff
            .selected()
            .min(prepared.total_hunks.saturating_sub(1));
        let start = selected.saturating_sub(hunk_limit.saturating_sub(1));
        prepared.hunks = result
            .model
            .hunks()
            .enumerate()
            .skip(start)
            .take(hunk_limit)
            .map(|(index, hunk)| GitHunkRow {
                index,
                label: plain_prefix(&hunk_label(hunk), width),
                selected: index == selected,
            })
            .collect();
        let (visible, scroll) = model_rows(
            &result.model,
            selected,
            self.git_diff_scroll,
            width,
            limit,
            encoding,
        );
        prepared.rows = visible;
        self.git_diff_scroll = scroll;
        prepared
    }

    pub(crate) fn navigate_git_hunk(&mut self) {
        let selected = self.git_diff.selected();
        match self.git_diff.select_hunk(selected, &mut self.workbench) {
            Ok(()) => {
                self.git_diff_scroll = 0;
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                self.status = format!(
                    "Bloque de diferencias Git {} seleccionado",
                    selected.saturating_add(1)
                );
            }
            Err(error) => self.status = error,
        }
    }

    pub(crate) fn handle_git_sidebar_key(&mut self, key: KeyEvent) -> bool {
        if !key.modifiers.is_empty() {
            return false;
        }
        let delta = match key.code {
            KeyCode::Up => -1,
            KeyCode::Down => 1,
            KeyCode::Home => isize::MIN,
            KeyCode::End => isize::MAX,
            KeyCode::Enter => {
                self.navigate_git_hunk();
                return true;
            }
            _ => return false,
        };
        self.git_diff.select_relative(delta);
        self.git_diff_scroll = 0;
        true
    }

    pub(crate) fn handle_git_diff_key(&mut self, key: KeyEvent) -> bool {
        if self.git_history.active {
            return self.handle_git_history_key(key);
        }
        if key.modifiers != KeyModifiers::NONE {
            return false;
        }
        self.git_diff_scroll = match key.code {
            KeyCode::Up => self.git_diff_scroll.saturating_sub(1),
            KeyCode::Down => self.git_diff_scroll.saturating_add(1),
            KeyCode::PageUp => self.git_diff_scroll.saturating_sub(10),
            KeyCode::PageDown => self.git_diff_scroll.saturating_add(10),
            KeyCode::Home => 0,
            KeyCode::End => usize::MAX,
            KeyCode::Enter => {
                self.navigate_git_hunk();
                return true;
            }
            _ => return false,
        };
        true
    }
}

fn plain_prefix(text: &str, width: u16) -> String {
    let mut result = String::new();
    for ch in text.chars().take(usize::from(width).min(PREFIX_BYTES)) {
        if result.len() + ch.len_utf8() > PREFIX_BYTES {
            break;
        }
        result.push(ch);
    }
    result
}

pub(super) fn hunk_label(hunk: &DiffSpan) -> String {
    let left_count = hunk.left.end.0 - hunk.left.start.0;
    let right_count = hunk.right.end.0 - hunk.right.start.0;
    let left_start = hunk.left.start.0 + usize::from(left_count != 0);
    let right_start = hunk.right.start.0 + usize::from(right_count != 0);
    format!("@@ -{left_start},{left_count} +{right_start},{right_count} @@")
}

#[derive(Clone, Copy)]
struct Segment {
    kind: GitRowKind,
    left: Option<usize>,
    right: Option<usize>,
    len: usize,
}

pub(super) fn model_rows(
    model: &OwnedDiffModel,
    selected: usize,
    scroll: usize,
    width: u16,
    rows: usize,
    encoding: TextEncoding,
) -> (Vec<GitDiffRow>, usize) {
    let limit = rows.min(MAX_ROWS);
    if limit == 0 || width == 0 {
        return (Vec::new(), scroll);
    }
    let Some(hunk) = model.hunks().nth(selected) else {
        let message = if model.hunks().next().is_none() {
            "No hay diferencias de bytes en las copias aceptadas"
        } else {
            "El bloque de diferencias seleccionado no está disponible"
        };
        return (
            vec![GitDiffRow {
                kind: GitRowKind::Notice,
                left_line: None,
                right_line: None,
                text: plain_prefix(message, width),
                eol: "",
            }],
            0,
        );
    };
    let span_index = model
        .spans()
        .iter()
        .position(|span| std::ptr::eq(span, hunk))
        .unwrap();
    let before = span_index
        .checked_sub(1)
        .and_then(|index| model.spans().get(index))
        .filter(|span| span.kind == DiffKind::Equal);
    let after = model
        .spans()
        .get(span_index + 1)
        .filter(|span| span.kind == DiffKind::Equal);
    let before_count = before.map_or(0, |span| {
        (span.left.end.0 - span.left.start.0).min(CONTEXT_LINES)
    });
    let after_count = after.map_or(0, |span| {
        (span.left.end.0 - span.left.start.0).min(CONTEXT_LINES)
    });
    let segments = [
        Segment {
            kind: GitRowKind::Context,
            left: Some(hunk.left.start.0 - before_count),
            right: Some(hunk.right.start.0 - before_count),
            len: before_count,
        },
        Segment {
            kind: GitRowKind::Deleted,
            left: Some(hunk.left.start.0),
            right: None,
            len: hunk.left.end.0 - hunk.left.start.0,
        },
        Segment {
            kind: GitRowKind::Added,
            left: None,
            right: Some(hunk.right.start.0),
            len: hunk.right.end.0 - hunk.right.start.0,
        },
        Segment {
            kind: GitRowKind::Context,
            left: Some(hunk.left.end.0),
            right: Some(hunk.right.end.0),
            len: after_count,
        },
    ];
    let mut output = Vec::with_capacity(limit);
    if limit > 1 {
        output.push(GitDiffRow {
            kind: GitRowKind::Header,
            left_line: None,
            right_line: None,
            text: plain_prefix(&hunk_label(hunk), width),
            eol: "",
        });
    }
    let body_limit = limit - usize::from(limit > 1);
    let total = segments.iter().map(|segment| segment.len).sum::<usize>();
    let first = scroll.min(total.saturating_sub(body_limit));
    for logical in first..first.saturating_add(body_limit).min(total) {
        let mut offset = logical;
        for segment in &segments {
            if offset >= segment.len {
                offset -= segment.len;
                continue;
            }
            let left = segment.left.map(|line| line + offset);
            let right = segment.right.map(|line| line + offset);
            let (side, index) = right.map_or((DiffSide::Left, left.unwrap_or(0)), |line| {
                (DiffSide::Right, line)
            });
            let Some(range) = model.line_byte_range(side, DiffLine(index)) else {
                break;
            };
            let (text, eol) = source_prefix(
                &model.source_bytes(side)[range.clone()],
                range.start,
                encoding,
                width,
            );
            output.push(GitDiffRow {
                kind: segment.kind,
                left_line: left.map(|n| n + 1),
                right_line: right.map(|n| n + 1),
                text,
                eol,
            });
            break;
        }
    }
    (output, first)
}

/// EOL is read from the end of the borrowed line; only a bounded prefix is
/// interpreted. Invalid bytes are explicit escapes, never lossy source text.
fn source_prefix(
    bytes: &[u8],
    source_start: usize,
    encoding: TextEncoding,
    width: u16,
) -> (String, &'static str) {
    let (content, eol) = if bytes.ends_with(b"\r\n") {
        (&bytes[..bytes.len() - 2], "CRLF")
    } else if bytes.ends_with(b"\n") {
        (&bytes[..bytes.len() - 1], "LF")
    } else if bytes.ends_with(b"\r") {
        (&bytes[..bytes.len() - 1], "CR")
    } else {
        (bytes, "sin fin de línea")
    };
    let columns = usize::from(width).min(PREFIX_BYTES);
    let mut result = String::with_capacity(columns.saturating_mul(4).min(PREFIX_BYTES));
    let mut used = 0;
    let bom = usize::from(
        source_start == 0
            && encoding == TextEncoding::Utf8
            && content.starts_with(&[0xef, 0xbb, 0xbf]),
    ) * 3;
    if bom != 0 && !push_piece(&mut result, &mut used, "[BOM]", columns) {
        return (result, eol);
    }
    let prefix_end = content.len().min(PREFIX_BYTES + 4);
    let budget_end = source_start.saturating_add(PREFIX_BYTES);
    let mut consumed = bom;
    let mut truncated = false;
    for scalar in DecodedScalars::new(&content[bom..prefix_end], encoding, source_start + bom) {
        if scalar.source_range.end > budget_end {
            truncated = true;
            break;
        }
        let mut token = [0u8; 16];
        let mut token_len = 0;
        if !scalar.valid {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            for byte in &content[(scalar.source_range.start - source_start)
                ..(scalar.source_range.end - source_start)]
            {
                token[token_len..token_len + 4].copy_from_slice(&[
                    b'\\',
                    b'x',
                    HEX[usize::from(byte >> 4)],
                    HEX[usize::from(byte & 15)],
                ]);
                token_len += 4;
            }
        } else if scalar.character == '\u{feff}' {
            token[..5].copy_from_slice(b"[BOM]");
            token_len = 5;
        } else if scalar.character == '\\' {
            token[..2].copy_from_slice(b"\\\\");
            token_len = 2;
        } else if scalar.character.is_control() {
            for ch in scalar.character.escape_default() {
                token[token_len] = ch as u8;
                token_len += 1;
            }
        } else {
            token_len = scalar.character.encode_utf8(&mut token[..4]).len();
        }
        let token = std::str::from_utf8(&token[..token_len]).expect("UTF-8 scalar or ASCII escape");
        if !push_piece(&mut result, &mut used, token, columns) {
            truncated = true;
            break;
        }
        consumed = scalar.source_range.end - source_start;
    }
    if truncated || consumed < content.len() {
        let _ = push_piece(&mut result, &mut used, "…", columns);
    }
    (result, eol)
}

fn push_piece(output: &mut String, used: &mut usize, token: &str, columns: usize) -> bool {
    let count = token.chars().count();
    if output.len() + token.len() > PREFIX_BYTES || used.saturating_add(count) > columns {
        return false;
    }
    output.push_str(token);
    *used += count;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::diff_model::DiffLimits;
    use std::sync::Arc;

    #[test]
    fn prefixes_preserve_explicit_encoding_invalid_bytes_bom_and_eol_tokens() {
        let (text, eol) = source_prefix(
            b"\xef\xbb\xbfa\xff\xef\xbf\xbd\r\n",
            0,
            TextEncoding::Utf8,
            80,
        );
        assert_eq!(text, "[BOM]a\\xFF�");
        assert_eq!(eol, "CRLF");
        assert_eq!(
            source_prefix(b"ni\xf1o\r", 0, TextEncoding::Windows1252, 80),
            ("niño".into(), "CR")
        );
        assert_eq!(
            source_prefix(b"ni\xf1o\n", 0, TextEncoding::Utf8, 80),
            ("ni\\xF1o".into(), "LF")
        );
        assert_eq!(
            source_prefix("\u{feff}x".as_bytes(), 7, TextEncoding::Utf8, 80),
            ("[BOM]x".into(), "sin fin de línea")
        );
        let (text, eol) = source_prefix(&b"x".repeat(32 * 1024), 0, TextEncoding::Utf8, u16::MAX);
        assert!(text.len() <= PREFIX_BYTES);
        assert_eq!(eol, "sin fin de línea");
    }

    #[test]
    fn selected_hunk_prepares_only_three_context_lines_and_keeps_byte_sources_immutable() {
        use std::fmt::Write;
        let mut left = String::new();
        for i in 0..100 {
            write!(&mut left, "line {i}\r\n").unwrap();
        }
        let left = left.into_bytes();
        let mut right = left.clone();
        let begin = left
            .windows(b"line 40".len())
            .position(|w| w == b"line 40")
            .unwrap();
        right[begin..begin + 7].copy_from_slice(b"LINE 40");
        let model = OwnedDiffModel::compute(
            Arc::new(left.clone()),
            Arc::new(right.clone()),
            DiffLimits::default(),
            || false,
        );
        let (rows, _) = model_rows(&model, 0, 0, 140, MAX_ROWS, TextEncoding::Utf8);
        assert_eq!(rows.len(), 9); // Header, 3 equal, delete, add, 3 equal.
        assert!(matches!(rows[1].kind, GitRowKind::Context));
        assert_eq!(rows[1].left_line, Some(38));
        assert!(matches!(rows[4].kind, GitRowKind::Deleted));
        assert_eq!(rows[4].left_line, Some(41));
        assert!(matches!(rows[5].kind, GitRowKind::Added));
        assert_eq!(rows[5].right_line, Some(41));
        assert_eq!(model.source_bytes(DiffSide::Left), left);
        assert_eq!(model.source_bytes(DiffSide::Right), right);
    }

    #[test]
    fn huge_changed_hunk_scrolls_by_ranges_without_preparing_every_line() {
        let model = OwnedDiffModel::compute(
            Arc::new(Vec::new()),
            Arc::new(b"new\n".repeat(600)),
            DiffLimits::default(),
            || false,
        );
        let (first, _) = model_rows(&model, 0, 0, 45, 10, TextEncoding::Utf8);
        let (last, end_scroll) = model_rows(&model, 0, usize::MAX, 45, 10, TextEncoding::Utf8);
        assert_eq!(first.len(), 10);
        assert_eq!(last.len(), 10);
        assert_eq!(first[1].right_line, Some(1));
        assert_eq!(last[1].right_line, Some(592));
        assert_eq!(last[9].right_line, Some(600));
        let (up, up_scroll) = model_rows(
            &model,
            0,
            end_scroll.saturating_sub(1),
            45,
            10,
            TextEncoding::Utf8,
        );
        assert_eq!(up_scroll + 1, end_scroll);
        assert_eq!(up[1].right_line, Some(591));
        assert_eq!(up[9].right_line, Some(599));
        assert!(
            model_rows(&model, 0, 0, 45, usize::MAX, TextEncoding::Utf8)
                .0
                .len()
                <= MAX_ROWS
        );
        assert!(model_rows(&model, 0, 0, 0, 10, TextEncoding::Utf8)
            .0
            .is_empty());
        let (small, _) = model_rows(&model, 0, 0, 45, 1, TextEncoding::Utf8);
        let (small_end, _) = model_rows(&model, 0, usize::MAX, 45, 1, TextEncoding::Utf8);
        assert_eq!(small[0].right_line, Some(1));
        assert_eq!(small_end[0].right_line, Some(600));
    }
}

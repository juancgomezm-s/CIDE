//! Bounded read-only presentation shared independently of terminal widgets.
use crate::{
    diff_model::{DiffKind, DiffLine, DiffSide, OwnedDiffModel},
    open_diff::OpenDiffController,
};
use jc_text::{DecodedScalars, TextEncoding};
const ROWS: usize = 96;
const COLS: usize = 256;
const PREFIX: usize = 2048;
#[derive(Debug)]
pub struct OpenDiffRow {
    pub label: String,
    pub selected: bool,
    pub kind: OpenDiffRowKind,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenDiffRowKind {
    Context,
    Removed,
    Added,
    Changed,
    Header,
    Notice,
}
#[derive(Debug)]
pub struct OpenDiffPresentation {
    pub title: String,
    pub message: String,
    pub rows: Vec<OpenDiffRow>,
    pub selected: usize,
    pub total: usize,
}
#[derive(Debug, Default)]
pub struct OpenDiffView {
    scroll: usize,
    total: usize,
    page: usize,
}
fn escaped(text: &str, columns: usize) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        let token = match ch {
            '\\' => "\\\\".into(),
            ' '..='~' => ch.to_string(),
            _ => format!("\\u{{{:X}}}", ch as u32),
        };
        if out.len() + token.len() > columns.min(COLS) {
            break;
        }
        out.push_str(&token);
    }
    out
}
fn line(
    model: &OwnedDiffModel,
    side: DiffSide,
    index: usize,
    encoding: TextEncoding,
    cols: usize,
    horizontal: Option<usize>,
) -> String {
    let Some(range) = model.line_byte_range(side, DiffLine(index)) else {
        return String::new();
    };
    let bytes = &model.source_bytes(side)[range.clone()];
    let (content, eol) = if bytes.ends_with(b"\r\n") {
        (&bytes[..bytes.len() - 2], " [CRLF]")
    } else if bytes.ends_with(b"\n") {
        (&bytes[..bytes.len() - 1], " [LF]")
    } else if bytes.ends_with(b"\r") {
        (&bytes[..bytes.len() - 1], " [CR]")
    } else {
        (bytes, " [EOF]")
    };
    let cols = cols.min(COLS);
    let budget = horizontal.map_or_else(
        || cols.saturating_sub(eol.len()),
        |offset| offset.saturating_add(cols + 12).min(8 * 8192),
    );
    let mut out = escaped(&format!("{} ", index + 1), COLS);
    if horizontal.is_none() {
        out.truncate(budget);
    }
    let prefix = if horizontal.is_some() { 8192 } else { PREFIX };
    let bom = usize::from(
        range.start == 0 && encoding == TextEncoding::Utf8 && content.starts_with(b"\xef\xbb\xbf"),
    ) * 3;
    if bom > 0 && out.len() + 5 <= budget {
        out.push_str("[BOM]");
    }
    let mut consumed = bom;
    let end = content.len().min(prefix + 4);
    for scalar in DecodedScalars::new(&content[bom..end], encoding, range.start + bom) {
        let start = scalar.source_range.start - range.start;
        let end = scalar.source_range.end - range.start;
        if end > prefix {
            break;
        }
        let token = if scalar.valid {
            escaped(&scalar.character.to_string(), 32)
        } else {
            use std::fmt::Write;
            let mut escaped = String::new();
            for byte in &content[start..end] {
                write!(&mut escaped, "\\x{byte:02X}").unwrap();
            }
            escaped
        };
        if out.len() + token.len() > budget {
            break;
        }
        out.push_str(&token);
        consumed = end;
    }
    if consumed < content.len() && horizontal.is_none() {
        for _ in 0..budget.saturating_sub(out.len()).min(3) {
            out.push('.');
        }
    }
    if cols >= eol.len() && (horizontal.is_none() || consumed == content.len()) {
        out.push_str(eol);
    }
    if let Some(offset) = horizontal {
        // Labels are ASCII escaped, so display columns are byte offsets.
        let start = offset.min(out.len());
        let end = start.saturating_add(cols).min(out.len());
        return out[start..end].to_owned();
    }
    out
}
impl OpenDiffView {
    pub fn first(&mut self) {
        self.scroll = 0;
    }
    pub fn last(&mut self) {
        self.scroll = self.total.saturating_sub(self.page);
    }
    pub fn scroll(&mut self, delta: isize) {
        self.scroll = self
            .scroll
            .saturating_add_signed(delta)
            .min(self.total.saturating_sub(self.page));
    }
    pub fn scroll_relative(&mut self, delta: isize) {
        self.scroll(delta);
    }
    pub fn prepare(
        &mut self,
        controller: &OpenDiffController,
        max_rows: usize,
        max_cols: usize,
        side_by_side: bool,
    ) -> OpenDiffPresentation {
        let cols = max_cols.min(COLS);
        let mut p = OpenDiffPresentation {
            title: crate::source_services::display_ui_label(
                "Comparación de documentos abiertos (solo lectura)",
                cols,
            ),
            message: crate::source_services::display_ui_label(
                &format!("{}: {}", controller.state().label(), controller.message()),
                cols,
            ),
            rows: Vec::new(),
            selected: controller.selected(),
            total: 0,
        };
        let Some(result) = controller.result() else {
            self.total = 0;
            self.page = 0;
            self.scroll = 0;
            return p;
        };
        p.total = result.model.hunks().count();
        p.message = escaped(
            &format!(
                "I:{} [{}{}] | D:{} [{}{}] | bloque {}/{}",
                result.left.path.display(),
                result.left.stamp.encoding.name(),
                if result.left.modified { " *" } else { "" },
                result.right.path.display(),
                result.right.stamp.encoding.name(),
                if result.right.modified { " *" } else { "" },
                usize::from(p.total > 0) + p.selected,
                p.total
            ),
            cols,
        );
        self.project(
            p,
            &result.model,
            [result.left.stamp.encoding, result.right.stamp.encoding],
            (max_rows, max_cols, side_by_side, None),
        )
    }

    /// Show a comparison of immutable UTF-8 text supplied by another host.
    pub fn prepare_utf8_comparison(
        &mut self,
        model: &OwnedDiffModel,
        selected: usize,
        max_rows: usize,
        max_cols: usize,
        side_by_side: bool,
    ) -> OpenDiffPresentation {
        self.prepare_utf8_view(model, selected, (max_rows, max_cols, side_by_side, None))
    }

    /// Review bounded UTF-8 replacements, including lines wider than the panel.
    pub fn prepare_utf8_review(
        &mut self,
        model: &OwnedDiffModel,
        selected: usize,
        max_rows: usize,
        max_cols: usize,
        side_by_side: bool,
        horizontal: usize,
    ) -> OpenDiffPresentation {
        self.prepare_utf8_view(
            model,
            selected,
            (max_rows, max_cols, side_by_side, Some(horizontal)),
        )
    }

    fn prepare_utf8_view(
        &mut self,
        model: &OwnedDiffModel,
        selected: usize,
        geometry: (usize, usize, bool, Option<usize>),
    ) -> OpenDiffPresentation {
        let total = model.hunks().count();
        self.project(
            OpenDiffPresentation {
                title: String::new(),
                message: String::new(),
                rows: Vec::new(),
                selected: selected.min(total.saturating_sub(1)),
                total,
            },
            model,
            [TextEncoding::Utf8; 2],
            geometry,
        )
    }

    fn project(
        &mut self,
        mut p: OpenDiffPresentation,
        model: &OwnedDiffModel,
        [left_encoding, right_encoding]: [TextEncoding; 2],
        (max_rows, max_cols, side_by_side, horizontal): (usize, usize, bool, Option<usize>),
    ) -> OpenDiffPresentation {
        let cols = max_cols.min(COLS);
        let rows = max_rows.min(ROWS);
        let Some(hunk) = model.hunks().nth(p.selected) else {
            self.total = 0;
            self.page = 0;
            self.scroll = 0;
            if rows > 0 && cols > 0 {
                p.rows.push(OpenDiffRow {
                    label: escaped(
                        "No hay diferencias de bytes en las capturas aceptadas",
                        cols,
                    ),
                    selected: false,
                    kind: OpenDiffRowKind::Notice,
                });
            }
            return p;
        };
        if rows == 0 || cols == 0 {
            return p;
        }
        let at = model
            .spans()
            .iter()
            .position(|s| std::ptr::eq(s, hunk))
            .unwrap();
        let before = at
            .checked_sub(1)
            .and_then(|i| model.spans().get(i))
            .filter(|s| s.kind == DiffKind::Equal)
            .map_or(0, |s| (s.left.end.0 - s.left.start.0).min(3));
        let after = model
            .spans()
            .get(at + 1)
            .filter(|s| s.kind == DiffKind::Equal)
            .map_or(0, |s| (s.left.end.0 - s.left.start.0).min(3));
        let left = hunk.left.end.0 - hunk.left.start.0;
        let right = hunk.right.end.0 - hunk.right.start.0;
        let dual = side_by_side && cols >= 90;
        let changed = if dual { left.max(right) } else { left + right };
        self.total = before + changed + after;
        self.page = rows.saturating_sub(usize::from(rows > 1));
        self.scroll = self.scroll.min(self.total.saturating_sub(self.page));
        if rows > 1 {
            p.rows.push(OpenDiffRow {
                label: escaped(
                    &format!(
                        "@@ -{},{} +{},{} @@ {}",
                        hunk.left.start.0 + usize::from(left > 0),
                        left,
                        hunk.right.start.0 + usize::from(right > 0),
                        right,
                        if dual { "I | D" } else { "unificado" }
                    ),
                    cols,
                ),
                selected: true,
                kind: OpenDiffRowKind::Header,
            });
        }
        for logical in self.scroll..(self.scroll + self.page).min(self.total) {
            let (l, r, sign) = if logical < before {
                (
                    Some(hunk.left.start.0 - before + logical),
                    Some(hunk.right.start.0 - before + logical),
                    ' ',
                )
            } else if logical >= before + changed {
                let n = logical - before - changed;
                (Some(hunk.left.end.0 + n), Some(hunk.right.end.0 + n), ' ')
            } else {
                let n = logical - before;
                if dual {
                    (
                        (n < left).then_some(hunk.left.start.0 + n),
                        (n < right).then_some(hunk.right.start.0 + n),
                        '!',
                    )
                } else if n < left {
                    (Some(hunk.left.start.0 + n), None, '-')
                } else {
                    (None, Some(hunk.right.start.0 + n - left), '+')
                }
            };
            let mut label = if dual {
                let width = (cols - 3) / 2;
                let l = l.map_or_else(String::new, |n| {
                    line(model, DiffSide::Left, n, left_encoding, width, horizontal)
                });
                let r = r.map_or_else(String::new, |n| {
                    line(
                        model,
                        DiffSide::Right,
                        n,
                        right_encoding,
                        cols - width - 3,
                        horizontal,
                    )
                });
                format!("{l:width$} | {r}")
            } else {
                let (side, n, encoding) = if let Some(n) = r {
                    (DiffSide::Right, n, right_encoding)
                } else {
                    (DiffSide::Left, l.unwrap(), left_encoding)
                };
                format!(
                    "{sign} {}",
                    line(model, side, n, encoding, cols.saturating_sub(2), horizontal)
                )
            };
            label.truncate(cols); // ASCII escaped labels, including narrow prefixes.
            p.rows.push(OpenDiffRow {
                label,
                selected: false,
                kind: match sign {
                    '-' => OpenDiffRowKind::Removed,
                    '+' => OpenDiffRowKind::Added,
                    '!' => OpenDiffRowKind::Changed,
                    _ => OpenDiffRowKind::Context,
                },
            });
        }
        p
    }
}

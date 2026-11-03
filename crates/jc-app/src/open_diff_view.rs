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
    let budget = cols.saturating_sub(eol.len());
    let mut out = escaped(&format!("{} ", index + 1), budget);
    let bom = usize::from(
        range.start == 0 && encoding == TextEncoding::Utf8 && content.starts_with(b"\xef\xbb\xbf"),
    ) * 3;
    if bom > 0 && out.len() + 5 <= budget {
        out.push_str("[BOM]");
    }
    let mut consumed = bom;
    let end = content.len().min(PREFIX + 4);
    for scalar in DecodedScalars::new(&content[bom..end], encoding, range.start + bom) {
        let start = scalar.source_range.start - range.start;
        let end = scalar.source_range.end - range.start;
        if end > PREFIX {
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
    if consumed < content.len() {
        for _ in 0..budget.saturating_sub(out.len()).min(3) {
            out.push('.');
        }
    }
    if cols >= eol.len() {
        out.push_str(eol);
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
        let rows = max_rows.min(ROWS);
        let mut p = OpenDiffPresentation {
            title: escaped("Open documents comparison (read-only)", cols),
            message: escaped(
                &format!("{:?}: {}", controller.state(), controller.message()),
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
                "L:{} [{}{}] | R:{} [{}{}] | hunk {}/{}",
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
        let Some(hunk) = result.model.hunks().nth(p.selected) else {
            self.total = 0;
            self.page = 0;
            self.scroll = 0;
            if rows > 0 && cols > 0 {
                p.rows.push(OpenDiffRow {
                    label: escaped("No byte differences in accepted snapshots", cols),
                    selected: false,
                });
            }
            return p;
        };
        if rows == 0 || cols == 0 {
            return p;
        }
        let at = result
            .model
            .spans()
            .iter()
            .position(|s| std::ptr::eq(s, hunk))
            .unwrap();
        let before = at
            .checked_sub(1)
            .and_then(|i| result.model.spans().get(i))
            .filter(|s| s.kind == DiffKind::Equal)
            .map_or(0, |s| (s.left.end.0 - s.left.start.0).min(3));
        let after = result
            .model
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
                        if dual { "L | R" } else { "unified" }
                    ),
                    cols,
                ),
                selected: true,
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
                    line(
                        &result.model,
                        DiffSide::Left,
                        n,
                        result.left.stamp.encoding,
                        width,
                    )
                });
                let r = r.map_or_else(String::new, |n| {
                    line(
                        &result.model,
                        DiffSide::Right,
                        n,
                        result.right.stamp.encoding,
                        cols - width - 3,
                    )
                });
                format!("{l:width$} | {r}")
            } else {
                let (side, n, encoding) = if let Some(n) = r {
                    (DiffSide::Right, n, result.right.stamp.encoding)
                } else {
                    (DiffSide::Left, l.unwrap(), result.left.stamp.encoding)
                };
                format!(
                    "{sign} {}",
                    line(&result.model, side, n, encoding, cols.saturating_sub(2))
                )
            };
            label.truncate(cols); // ASCII escaped labels, including narrow prefixes.
            p.rows.push(OpenDiffRow {
                label,
                selected: false,
            });
        }
        p
    }
}

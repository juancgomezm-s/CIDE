//! A private merge draft. The editor and worktree remain separate from this buffer.
use crate::{edit_field, FieldEdit};
use jc_git::local_merge::{MergeConflict, MergePlan, MergeResolution, MergeSide, MERGE_EDIT_BYTES};
use jc_text::grapheme::{self, MAX_CLUSTER_BYTES};
use std::{borrow::Cow, collections::BTreeMap};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MergePane {
    Base,
    Current,
    Incoming,
    Result,
}

impl MergePane {
    pub const ALL: [Self; 4] = [Self::Base, Self::Current, Self::Incoming, Self::Result];
    pub fn label(self) -> &'static str {
        match self {
            Self::Base => "Base",
            Self::Current => "Actual",
            Self::Incoming => "Entrante",
            Self::Result => "Resultado",
        }
    }
    fn side(self) -> Option<MergeSide> {
        match self {
            Self::Base => Some(MergeSide::Base),
            Self::Current => Some(MergeSide::Current),
            Self::Incoming => Some(MergeSide::Incoming),
            Self::Result => None,
        }
    }
}

#[derive(Clone, Copy)]
pub enum MergeAction {
    Open(usize),
    Pane(MergePane),
    NextPane,
    UseVersion,
    Accept,
    Newline,
}

#[derive(Default)]
pub(crate) struct MergeDraft {
    pub choices: BTreeMap<Vec<u8>, MergeResolution>,
    pub file: Option<MergeFile>,
}

pub struct MergeFile {
    row: usize,
    pub pane: MergePane,
    choice: MergeResolution,
    cursor: usize,
    first_line: usize,
    left: usize,
}

fn editable(bytes: &[u8]) -> Option<&str> {
    if bytes.len() > MERGE_EDIT_BYTES {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    (!text
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\t' | '\r' | '\n')))
    .then_some(text)
}

impl MergeFile {
    pub(crate) fn open(row: usize, choice: Option<&MergeResolution>) -> Self {
        Self {
            row,
            pane: MergePane::Base,
            choice: choice
                .cloned()
                .unwrap_or(MergeResolution::Use(MergeSide::Current)),
            cursor: 0,
            first_line: 0,
            left: 0,
        }
    }
    pub fn path<'a>(&self, plan: &'a MergePlan) -> &'a [u8] {
        &plan.rows[self.row].path
    }
    fn result<'a>(&'a self, conflict: &'a MergeConflict) -> Option<&'a [u8]> {
        match &self.choice {
            MergeResolution::Use(side) => conflict.bytes(*side),
            MergeResolution::Edit(text) => Some(text.as_bytes()),
        }
    }
    fn source<'a>(&'a self, conflict: &'a MergeConflict) -> Option<&'a [u8]> {
        match self.pane.side() {
            Some(side) => conflict.bytes(side),
            None => self.result(conflict),
        }
    }
    pub fn can_edit(&self, plan: &MergePlan) -> bool {
        self.pane == MergePane::Result
            && plan.conflict(self.path(plan)).is_some_and(|conflict| {
                editable(self.result(conflict).unwrap_or_default()).is_some()
            })
    }
    pub(crate) fn paste_budget(&self, plan: &MergePlan) -> Option<usize> {
        self.can_edit(plan).then(|| {
            let conflict = plan.conflict(self.path(plan)).unwrap();
            MERGE_EDIT_BYTES
                .saturating_sub(self.result(conflict).map_or(0, <[u8]>::len))
                .max(1)
        })
    }
    pub(crate) fn set_pane(&mut self, pane: MergePane) {
        self.pane = pane;
    }
    pub(crate) fn next_pane(&mut self) {
        self.pane = MergePane::ALL[(self.pane as usize + 1) % MergePane::ALL.len()];
    }
    pub(crate) fn use_version(&mut self) {
        if let Some(side) = self.pane.side() {
            self.choice = MergeResolution::Use(side);
            self.pane = MergePane::Result;
            self.cursor = 0;
        }
    }
    pub(crate) fn take_choice(self) -> MergeResolution {
        self.choice
    }
    pub(crate) fn scroll(&mut self, plan: &MergePlan, delta: isize) -> Result<(), &'static str> {
        if self.can_edit(plan) {
            let edit = if delta < 0 {
                FieldEdit::Up
            } else {
                FieldEdit::Down
            };
            for _ in 0..delta.unsigned_abs().min(12) {
                let before = self.cursor;
                self.edit(plan, edit)?;
                if before == self.cursor {
                    break;
                }
            }
        } else {
            let conflict = plan.conflict(self.path(plan)).unwrap();
            let last = lines(self.source(conflict).unwrap_or_default())
                .count()
                .saturating_sub(1);
            self.first_line = self.first_line.saturating_add_signed(delta).min(last);
        }
        Ok(())
    }
    pub(crate) fn edit(
        &mut self,
        plan: &MergePlan,
        edit: FieldEdit<'_>,
    ) -> Result<(), &'static str> {
        let conflict = plan.conflict(self.path(plan)).unwrap();
        let bytes = self.result(conflict).unwrap_or_default();
        if self.pane != MergePane::Result || editable(bytes).is_none() {
            match edit {
                FieldEdit::Left => self.left = self.left.saturating_sub(8),
                FieldEdit::Right => self.left = self.left.saturating_add(8).min(16_384),
                FieldEdit::Home => self.left = 0,
                _ if self.pane == MergePane::Result => {
                    return Err("La edición requiere UTF-8 y hasta 64 KiB")
                }
                _ => {}
            }
            return Ok(());
        }
        if matches!(edit, FieldEdit::Insert(text) if text
                .chars().any(|ch| ch.is_control() && !matches!(ch, '\t' | '\r' | '\n')))
        {
            return Err("El resultado contiene caracteres de control");
        }
        // Move the owned buffer when editing. Keep a whole-side choice when
        // navigation leaves the source untouched, including a missing file.
        let mut text = match &mut self.choice {
            MergeResolution::Edit(text) => std::mem::take(text),
            MergeResolution::Use(side) => {
                std::str::from_utf8(conflict.bytes(*side).unwrap_or_default())
                    .unwrap()
                    .to_owned()
            }
        };
        let was_edit = matches!(self.choice, MergeResolution::Edit(_));
        let result = edit_field(&mut text, &mut self.cursor, edit, MERGE_EDIT_BYTES);
        if was_edit || result == Ok(true) {
            self.choice = MergeResolution::Edit(text);
        }
        // The cursor is always a boundary in the same bytes.
        debug_assert!(source_boundary(&self.choice, conflict, self.cursor));
        result.map(|_| ())
    }
    pub(crate) fn newline(&mut self, plan: &MergePlan) -> Result<(), &'static str> {
        let conflict = plan.conflict(self.path(plan)).unwrap();
        let bytes = self.result(conflict).unwrap_or_default();
        let newline = bytes
            .iter()
            .position(|b| matches!(b, b'\r' | b'\n'))
            .map_or("\n", |at| {
                if bytes[at..].starts_with(b"\r\n") {
                    "\r\n"
                } else if bytes[at] == b'\r' {
                    "\r"
                } else {
                    "\n"
                }
            });
        self.edit(plan, FieldEdit::Insert(newline))
    }
}

fn source_boundary(choice: &MergeResolution, conflict: &MergeConflict, cursor: usize) -> bool {
    let bytes = match choice {
        MergeResolution::Use(side) => conflict.bytes(*side).unwrap_or_default(),
        MergeResolution::Edit(text) => text.as_bytes(),
    };
    std::str::from_utf8(bytes).is_ok_and(|text| text.is_char_boundary(cursor))
}

pub struct MergeText {
    pub rows: Vec<(usize, String)>,
    pub caret: Option<(usize, usize)>,
    pub note: &'static str,
}

fn lines(source: &[u8]) -> impl Iterator<Item = (usize, &[u8])> {
    let mut start = 0;
    std::iter::from_fn(move || {
        let tail = source.get(start..)?;
        let end = tail
            .iter()
            .position(|b| matches!(b, b'\r' | b'\n'))
            .unwrap_or(tail.len());
        let length = if tail[end..].starts_with(b"\r\n") {
            2
        } else {
            1
        };
        let row = (start, &tail[..end]);
        start += end + length;
        Some(row)
    })
}

fn tokens(bytes: &[u8]) -> Box<dyn Iterator<Item = Cow<'_, str>> + '_> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        Box::new(grapheme::graphemes(text).map(|cluster| {
            if cluster.len() > MAX_CLUSTER_BYTES {
                Cow::Borrowed("…")
            } else if cluster == "\t" {
                Cow::Borrowed("    ")
            } else if cluster.chars().any(|ch| {
                ch.is_control() || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            }) {
                Cow::Owned(cluster.escape_default().to_string())
            } else {
                Cow::Borrowed(cluster)
            }
        }))
    } else {
        Box::new(bytes.iter().map(|&byte| {
            Cow::Owned(if byte.is_ascii_graphic() || byte == b' ' {
                char::from(byte).to_string()
            } else {
                format!("\\x{byte:02X}")
            })
        }))
    }
}

impl MergeFile {
    pub fn present(&self, plan: &MergePlan, width: usize, height: usize) -> MergeText {
        let conflict = plan.conflict(self.path(plan)).unwrap();
        let source = self.source(conflict);
        let editing = self.can_edit(plan);
        let note = match (source, self.pane, editing) {
            (None, MergePane::Result, _) => {
                "Resultado: eliminar archivo. Escribe para crear contenido."
            }
            (None, _, _) => "El archivo no existe en esta versión",
            (_, MergePane::Result, true) => "Resultado editable · se conservan los saltos de línea",
            (_, MergePane::Result, false) => {
                "Versión completa · edición disponible para UTF-8 de hasta 64 KiB"
            }
            _ => "Versión de solo lectura",
        };
        let source = source.unwrap_or_default();
        let width = width.clamp(1, 256);
        let height = height.clamp(1, 64);
        let mut first = self.first_line;
        let mut left = self.left;
        let mut caret = None;
        if editing {
            let (row, (start, bytes)) = lines(source)
                .enumerate()
                .take_while(|(_, (start, _))| *start <= self.cursor)
                .last()
                .unwrap();
            let prefix = &bytes[..self.cursor.saturating_sub(start).min(bytes.len())];
            let col: usize = tokens(prefix).map(|token| grapheme::width(&token)).sum();
            first = first.min(row).max(row.saturating_sub(height - 1));
            left = col.saturating_sub(width - 1);
            caret = Some((row - first, col - left));
        }
        let total = lines(source).count();
        first = first.min(total.saturating_sub(1));
        let rows = lines(source)
            .enumerate()
            .skip(first)
            .take(height)
            .map(|(row, (start, bytes))| {
                let mut col = 0;
                let mut text = String::new();
                let mut clipped = false;
                for token in tokens(bytes) {
                    let next = col + grapheme::width(&token);
                    if next > left + width {
                        clipped = true;
                        break;
                    }
                    if col >= left {
                        text.push_str(&token);
                    } else if next > left {
                        text.push_str(&" ".repeat(next - left));
                    }
                    col = next;
                }
                if !editing && !clipped && col >= left && col < left + width {
                    let eol = match &source[start + bytes.len()..] {
                        [b'\r', b'\n', ..] => " · CRLF",
                        [b'\n', ..] => " · LF",
                        [b'\r', ..] => " · CR",
                        _ if !bytes.is_empty() => " · sin EOL",
                        _ => "",
                    };
                    text.push_str(&grapheme::clip(eol, left + width - col));
                }
                (row + 1, text)
            })
            .collect();
        MergeText { rows, caret, note }
    }
}

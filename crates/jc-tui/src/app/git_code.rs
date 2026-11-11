//! Read-only comparison in the code area, sharing the accepted Git diff model.
use super::{git_diff::source_prefix, GitDiffRow, GitRowKind, TuiApp};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jc_app::{
    diff_model::{DiffLine, DiffSide, OwnedDiffModel},
    PrimaryFocus,
};
use jc_text::TextEncoding;

#[derive(Default)]
pub(crate) struct CodeDiffView {
    pub active: bool,
    pub choose_commit: bool,
    pub unified: bool,
    pub reveal_change: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum Control {
    History,
    Toggle,
    Change(bool),
    Close,
}
pub(crate) fn controls(width: u16, unified: bool) -> Vec<(u16, u16, String, Control)> {
    let close = "[Cerrar comparación]";
    let mode = if unified {
        "[Dividir pantalla]"
    } else {
        "[Ver en línea]"
    };
    let previous = if width < 65 {
        "[Anterior]"
    } else {
        "[Cambio anterior]"
    };
    let next = if width < 65 {
        "[Siguiente]"
    } else {
        "[Cambio siguiente]"
    };
    let mut result = vec![
        (0, 0, "[Elegir commit]".into(), Control::History),
        (
            width.saturating_sub(jc_text::grapheme::width(close) as u16),
            0,
            close.into(),
            Control::Close,
        ),
    ];
    let mut x = 0;
    for (label, control) in [
        (mode, Control::Toggle),
        (previous, Control::Change(false)),
        (next, Control::Change(true)),
    ] {
        result.push((x, 1, label.into(), control));
        x += jc_text::grapheme::width(label) as u16 + 1;
    }
    result
}

pub(crate) struct CodeDiffRow {
    pub left: Option<GitDiffRow>,
    pub right: Option<GitDiffRow>,
}

pub(crate) struct CodeDiffPresentation {
    pub left_title: String,
    pub right_title: String,
    pub message: String,
    pub unified: bool,
    pub rows: Vec<CodeDiffRow>,
}

fn source_row(
    model: &OwnedDiffModel,
    side: DiffSide,
    line: usize,
    kind: GitRowKind,
    width: u16,
    encoding: TextEncoding,
) -> Option<GitDiffRow> {
    let range = model.line_byte_range(side, DiffLine(line))?;
    let (text, eol) = source_prefix(
        &model.source_bytes(side)[range.clone()],
        range.start,
        encoding,
        width,
    );
    Some(GitDiffRow {
        kind,
        left_line: (side == DiffSide::Left).then_some(line + 1),
        right_line: (side == DiffSide::Right).then_some(line + 1),
        text,
        eol,
    })
}

pub(super) fn project(
    model: &OwnedDiffModel,
    scroll: usize,
    page: usize,
    width: u16,
    encoding: TextEncoding,
    unified: bool,
) -> (Vec<CodeDiffRow>, usize) {
    let page = jc_app::diff_rows::page(model, scroll, page, !unified);
    let rows = page
        .rows
        .into_iter()
        .map(|row| CodeDiffRow {
            left: row.left.and_then(|line| {
                source_row(
                    model,
                    DiffSide::Left,
                    line.0,
                    if row.changed {
                        GitRowKind::Deleted
                    } else {
                        GitRowKind::Context
                    },
                    width,
                    encoding,
                )
            }),
            right: row.right.and_then(|line| {
                source_row(
                    model,
                    DiffSide::Right,
                    line.0,
                    if row.changed {
                        GitRowKind::Added
                    } else {
                        GitRowKind::Context
                    },
                    width,
                    encoding,
                )
            }),
        })
        .collect();
    (rows, page.first)
}

impl TuiApp {
    pub(crate) fn git_code_control(&mut self, control: Control) {
        match control {
            Control::History => {
                self.git_code.choose_commit = true;
                self.open_git_graph();
            }
            Control::Toggle => {
                self.git_code.unified = !self.git_code.unified;
                self.git_code.reveal_change = true;
            }
            Control::Change(forward) => {
                if let Some(result) = self.git_diff.result() {
                    self.git_diff_scroll = jc_app::diff_rows::change_target(
                        &result.model,
                        self.git_diff_scroll,
                        forward,
                        !self.git_code.unified,
                    );
                }
            }
            Control::Close => {
                self.git_code.active = false;
                self.git_code.choose_commit = false;
                self.git_diff.cancel(&mut self.tasks);
                self.git_revision.cancel();
                self.git_history = Default::default();
                let _ = self.focus.focus(PrimaryFocus::Editor, &self.panels);
                self.status = "Comparación cerrada · edición del documento".into();
            }
        }
    }
    pub(super) fn prepare_git_code(&mut self, width: u16, height: u16) -> CodeDiffPresentation {
        let unified = self.git_code.unified || width < 65;
        let mut frame = CodeDiffPresentation {
            left_title: "Anterior".into(),
            right_title: "Documento abierto".into(),
            message: self.git_diff.message().into(),
            unified,
            rows: Vec::new(),
        };
        let (model, encoding) = if self.git_history.immutable() {
            frame.message = self.git_revision.message().into();
            let Some(result) = self.git_revision.comparison() else {
                return frame;
            };
            frame.left_title = result.origin.label();
            frame.right_title = super::git_history::opaque(&result.path, width);
            (&result.model, self.git_history.encoding)
        } else {
            let Some(result) = self.git_diff.result() else {
                return frame;
            };
            frame.left_title = match result.identity.source {
                jc_app::git_read::GitBlobSource::Head => "HEAD".into(),
                jc_app::git_read::GitBlobSource::Index => "Índice".into(),
                jc_app::git_read::GitBlobSource::Commit(id) => {
                    format!("Commit {}", &id.as_str()[..8])
                }
            };
            frame.right_title = format!("{} · documento abierto", result.identity.path);
            (
                &result.model,
                self.workbench
                    .active_document()
                    .and_then(|d| d.session.encoding()),
            )
        };
        let Some(encoding) = encoding else {
            frame.message = "Elige la codificación del documento para comparar".into();
            return frame;
        };
        if self.git_code.reveal_change {
            self.git_diff_scroll = jc_app::diff_rows::first_change(model, !unified).unwrap_or(0);
            self.git_code.reveal_change = false;
        }
        let columns = if unified {
            width
        } else {
            width.saturating_sub(1) / 2
        };
        let (rows, scroll) = project(
            model,
            self.git_diff_scroll,
            usize::from(height.saturating_sub(5)),
            columns.saturating_sub(15),
            encoding,
            unified,
        );
        frame.rows = rows;
        self.git_diff_scroll = scroll;
        frame
    }

    pub(super) fn git_code_key(&mut self, key: KeyEvent) {
        if key.modifiers != KeyModifiers::NONE {
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.git_code_control(Control::Close);
            }
            KeyCode::F(4) => {
                self.git_code.choose_commit = true;
                self.open_git_graph();
            }
            KeyCode::F(5) if !self.git_history.immutable() => {
                self.request_git_diff(self.git_source)
            }
            KeyCode::Char('v' | 'V') => {
                self.git_code.unified = !self.git_code.unified;
                self.git_diff_scroll = 0;
            }
            KeyCode::Up => self.git_diff_scroll = self.git_diff_scroll.saturating_sub(1),
            KeyCode::Down => self.git_diff_scroll = self.git_diff_scroll.saturating_add(1),
            KeyCode::PageUp => self.git_diff_scroll = self.git_diff_scroll.saturating_sub(10),
            KeyCode::PageDown => self.git_diff_scroll = self.git_diff_scroll.saturating_add(10),
            KeyCode::Home => self.git_diff_scroll = 0,
            KeyCode::End => self.git_diff_scroll = usize::MAX,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_app::diff_model::DiffLimits;
    use std::sync::Arc;

    fn model() -> OwnedDiffModel {
        OwnedDiffModel::compute(
            Arc::new(b"igual\r\nantes\nfin".to_vec()),
            Arc::new(b"igual\r\ndespu\xc3\xa9s\nagregada\nfin".to_vec()),
            DiffLimits::default(),
            || false,
        )
    }

    #[test]
    fn code_diff_pairs_align_replacements_and_insertions_with_one_scroll() {
        let model = model();
        let (rows, first) = project(&model, 0, 100, 80, TextEncoding::Utf8, false);
        assert_eq!(first, 0);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].left.as_ref().unwrap().text, "igual");
        assert_eq!(rows[0].right.as_ref().unwrap().eol, "CRLF");
        assert_eq!(rows[1].left.as_ref().unwrap().text, "antes");
        assert_eq!(rows[1].right.as_ref().unwrap().text, "después");
        assert!(rows[2].left.is_none());
        assert_eq!(rows[2].right.as_ref().unwrap().text, "agregada");
        assert_eq!(rows[3].left.as_ref().unwrap().text, "fin");
        for scroll in [0, 1, usize::MAX] {
            let (page, first) = project(&model, scroll, 2, 80, TextEncoding::Utf8, false);
            assert_eq!(page.len(), 2);
            assert_eq!(
                page[0].right.as_ref().unwrap().right_line,
                rows[first].right.as_ref().unwrap().right_line
            );
        }
    }

    #[test]
    fn code_diff_unified_shows_both_changed_versions_and_preserves_bytes() {
        let model = model();
        let before = model.source_bytes(DiffSide::Left).to_vec();
        let (rows, _) = project(&model, 0, 100, 80, TextEncoding::Utf8, true);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[1].left.as_ref().unwrap().text, "antes");
        assert!(rows[1].right.is_none());
        assert!(rows[2].left.is_none());
        assert_eq!(rows[2].right.as_ref().unwrap().text, "después");
        assert_eq!(model.source_bytes(DiffSide::Left), before);
        assert!(project(&model, 0, 0, 0, TextEncoding::Utf8, false)
            .0
            .is_empty());
    }
}

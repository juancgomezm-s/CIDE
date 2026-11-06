use super::{SearchHistoryEntry, Workbench, WorkbenchError};
use crate::{
    text_search::{Pattern, Scanner},
    SearchLimits, SearchStatus, TaskStamp,
};
use jc_files::FileError;
use jc_text::{decode_fragment, encode, TextPosition};
use std::ops::Range;

#[derive(Debug)]
pub(super) struct SearchWork {
    stamp: TaskStamp,
    scanner: Scanner,
}
#[derive(Debug)]
pub(super) struct CompiledQuery {
    query: String,
    case: bool,
    word: bool,
    regex: bool,
    pattern: std::sync::Arc<Pattern>,
}
#[derive(Debug, Clone, Copy)]
pub(super) struct Navigation {
    forward: bool,
    from: usize,
}
#[derive(Debug, Clone, Copy)]
pub(super) struct PendingNavigation {
    request: Navigation,
    view: Option<crate::ViewId>,
    cursor: usize,
    anchor: Option<usize>,
}
impl Workbench {
    pub fn set_search_query(&mut self, query: impl Into<String>) -> Result<(), WorkbenchError> {
        let query = query.into();
        if query.len() > 4096 {
            self.cancel_search();
            self.search.query.clear();
            self.search.matches.clear();
            self.search.status = SearchStatus::Unavailable;
            return Err(WorkbenchError::SearchInput);
        }
        if self.search.query == query
            && !matches!(
                self.search.status,
                SearchStatus::Cancelled | SearchStatus::TimedOut | SearchStatus::Unavailable
            )
        {
            return Ok(());
        }
        self.search.query = query;
        self.search.status = SearchStatus::Idle;
        self.search.target = None;
        self.search_history_index = None;
        self.rebuild_search()
    }
    pub fn toggle_search_case(&mut self) -> Result<(), WorkbenchError> {
        self.search.case_sensitive = !self.search.case_sensitive;
        self.restart_search()
    }
    pub fn toggle_search_whole_word(&mut self) -> Result<(), WorkbenchError> {
        self.search.whole_word = !self.search.whole_word;
        self.restart_search()
    }
    pub fn toggle_search_regex(&mut self) -> Result<(), WorkbenchError> {
        self.search.regex = !self.search.regex;
        self.restart_search()
    }
    fn restart_search(&mut self) -> Result<(), WorkbenchError> {
        self.search.status = SearchStatus::Idle;
        self.search.target = None;
        self.rebuild_search()
    }
    pub fn set_search_limits(&mut self, limits: SearchLimits) -> Result<(), WorkbenchError> {
        if !limits.valid() {
            return Err(WorkbenchError::SearchInput);
        }
        self.search_limits = limits;
        self.restart_search()
    }
    pub fn search_running(&self) -> bool {
        self.search_work.is_some()
    }
    pub fn cancel_search(&mut self) {
        self.search_work = None;
        self.search_navigation = None;
        if self.search.status == SearchStatus::Running {
            self.search.status = SearchStatus::Cancelled;
        }
    }
    /// Abandon a deferred Find navigation when a frontend opens another modal
    /// navigation intention, without discarding the bounded scan/results.
    pub fn cancel_search_navigation(&mut self) {
        self.search_navigation = None;
    }
    /// Schedule only. All source scanning is in poll_document_search; query input,
    /// edits and view transitions never materialize/decode the document here.
    pub(crate) fn rebuild_search(&mut self) -> Result<(), WorkbenchError> {
        let stamp = self.active_document().and_then(|d| {
            d.session.encoding().map(|encoding| TaskStamp {
                document: d.id,
                revision: d.session.document().revision(),
                encoding,
                view: None,
            })
        });
        if self.search.target == stamp && self.search.target.is_some() {
            return Ok(());
        }
        let cancelled = self.search.status == SearchStatus::Cancelled;
        self.search_work = None;
        self.search_navigation = None;
        self.search.matches.clear();
        self.search.current = None;
        self.search.scanned_bytes = 0;
        self.search.target = stamp;
        self.search.generation = self.search.generation.wrapping_add(1);
        self.search.status = SearchStatus::Idle;
        if self.search.query.is_empty() || self.active_document().is_none() {
            return Ok(());
        }
        if cancelled {
            self.search.status = SearchStatus::Cancelled;
            return Ok(());
        }
        let Some(stamp) = stamp else {
            self.search.status = SearchStatus::Unavailable;
            return Err(FileError::EncodingRequired.into());
        };
        let cached = self
            .search_pattern
            .as_ref()
            .filter(|cached| {
                cached.query == self.search.query
                    && cached.case == self.search.case_sensitive
                    && cached.word == self.search.whole_word
                    && cached.regex == self.search.regex
            })
            .map(|cached| cached.pattern.clone());
        let pattern = if let Some(cached) = cached {
            cached
        } else {
            Pattern::compile_mode(
                &self.search.query,
                self.search.case_sensitive,
                self.search.whole_word,
                self.search.regex,
            )
            .map_err(|message| {
                self.search.status = SearchStatus::Unavailable;
                WorkbenchError::SearchPattern(message)
            })?
        };
        self.search_pattern = Some(CompiledQuery {
            query: self.search.query.clone(),
            case: self.search.case_sensitive,
            word: self.search.whole_word,
            regex: self.search.regex,
            pattern: pattern.clone(),
        });
        let len = self.active_document().unwrap().session.document().len();
        self.search_work = Some(SearchWork {
            stamp,
            scanner: Scanner::new(pattern, stamp.encoding, len),
        });
        self.search.status = SearchStatus::Running;
        Ok(())
    }
    /// One chunk/slot. The UI checks input again before scheduling another slot.
    /// Returns true only when visible results/status/selection changed.
    pub fn poll_document_search(&mut self) -> bool {
        let Some(mut work) = self.search_work.take() else {
            return false;
        };
        if !self.task_stamp_is_current(work.stamp)
            || self.active_document().map(|d| d.id) != Some(work.stamp.document)
        {
            let _ = self.restart_search();
            return true;
        }
        let step = {
            let source = self
                .document_by_id(work.stamp.document)
                .unwrap()
                .session
                .document();
            work.scanner.step(self.search_limits, |range, bytes| {
                for chunk in source
                    .range_chunks(range)
                    .expect("revision-consistent search range")
                {
                    bytes.extend_from_slice(chunk);
                }
            })
        };
        let changed = !step.matches.is_empty() || step.status != self.search.status;
        self.search.matches.extend(step.matches);
        self.search.status = step.status;
        self.search.scanned_bytes = step.scanned;
        if step.status == SearchStatus::Running {
            self.search_work = Some(work);
        }
        let mut navigated = false;
        if let Some(request) = self.search_navigation {
            let view = self
                .panes
                .view(self.panes.active())
                .and_then(|v| v.view_id());
            let unchanged = self.active_editor().is_some_and(|e| {
                view == request.view && e.cursor == request.cursor && e.anchor == request.anchor
            });
            if !unchanged {
                self.search_navigation = None;
            } else if let Some(index) = self.navigation_index(request.request) {
                self.search_navigation = None;
                self.select_search_match(index).ok();
                navigated = true;
            } else if self.search.status != SearchStatus::Running {
                self.search_navigation = None;
            }
        }
        changed || navigated
    }
    fn valid_search(&self, complete: bool) -> Result<TaskStamp, WorkbenchError> {
        let stamp = self.search.target.ok_or(WorkbenchError::NoDocument)?;
        if !self.task_stamp_is_current(stamp)
            || self.active_document().map(|d| d.id) != Some(stamp.document)
            || complete && self.search.status != SearchStatus::Complete
        {
            return Err(WorkbenchError::SearchIncomplete);
        }
        Ok(stamp)
    }
    pub fn replace_current(&mut self, replacement: &str) -> Result<bool, WorkbenchError> {
        self.selection_edit(|app| app.replace_current_inner(replacement))
    }

    fn replace_current_inner(&mut self, replacement: &str) -> Result<bool, WorkbenchError> {
        let stamp = self.valid_search(true)?;
        if self.search.matches.is_empty() {
            return Ok(false);
        }
        if replacement.len() > 4096 {
            return Err(WorkbenchError::SearchInput);
        }
        let editor = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
        let index = editor
            .selection()
            .and_then(|range| {
                self.search
                    .matches
                    .iter()
                    .position(|hit| hit.range == range)
            })
            .or_else(|| {
                self.navigation_index(Navigation {
                    forward: true,
                    from: editor.cursor,
                })
            })
            .unwrap_or(0);
        let range = self.search.matches[index].range.clone();
        let length = encode(replacement, stamp.encoding)
            .map_err(FileError::from)?
            .len();
        self.remember_search();
        self.apply_text_edits(
            stamp.document,
            stamp.revision,
            vec![super::TextReplacement {
                range: range.clone(),
                text: replacement.to_owned(),
            }],
        )?;
        let mut editor = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        editor.cursor = range.start + length;
        editor.anchor = None;
        self.after_edit()?;
        Ok(true)
    }
    pub fn replace_all(&mut self, replacement: &str) -> Result<usize, WorkbenchError> {
        self.selection_edit(|app| app.replace_all_inner(replacement))
    }

    fn replace_all_inner(&mut self, replacement: &str) -> Result<usize, WorkbenchError> {
        let stamp = self.valid_search(true)?;
        let count = self.search.matches.len();
        if replacement.len() > 4096 || replacement.len().saturating_mul(count) > 2 * 1024 * 1024 {
            return Err(WorkbenchError::SearchInput);
        }
        if count == 0 {
            return Ok(0);
        }
        self.remember_search();
        let edits = self
            .search
            .matches
            .iter()
            .map(|m| super::TextReplacement {
                range: m.range.clone(),
                text: replacement.to_owned(),
            })
            .collect();
        self.apply_text_edits(stamp.document, stamp.revision, edits)?;
        self.active_editor_mut()
            .ok_or(WorkbenchError::NoDocument)?
            .anchor = None;
        self.after_edit()?;
        Ok(count)
    }
    pub fn find_next(&mut self) -> Result<Option<Range<usize>>, WorkbenchError> {
        self.find(true)
    }
    pub fn find_previous(&mut self) -> Result<Option<Range<usize>>, WorkbenchError> {
        self.find(false)
    }
    pub fn select_search_match(&mut self, index: usize) -> Result<bool, WorkbenchError> {
        self.valid_search(false)?;
        let Some(hit) = self.search.matches.get(index) else {
            return Ok(false);
        };
        let range = hit.range.clone();
        self.search.current = Some(index);
        let mut editor = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        editor.rectangle = None;
        editor.anchor = Some(range.start);
        editor.cursor = range.end;
        editor.manual_scroll = false;
        Ok(true)
    }
    pub fn search_match_position(&self, index: usize) -> Option<TextPosition> {
        self.search.matches.get(index).map(|m| m.position)
    }
    pub fn search_matches_for_tab(&self, tab: crate::TabId) -> &[crate::SearchMatch] {
        if self.search.target.is_some_and(|stamp| {
            self.task_stamp_is_current(stamp)
                && self
                    .document_by_tab(tab)
                    .is_some_and(|d| d.id == stamp.document)
        }) {
            &self.search.matches
        } else {
            &[]
        }
    }
    fn navigation_index(&self, request: Navigation) -> Option<usize> {
        let matches = &self.search.matches;
        if matches.is_empty() {
            return None;
        }
        let found = if request.forward {
            matches.iter().position(|m| m.range.start >= request.from)
        } else {
            matches.iter().rposition(|m| m.range.end <= request.from)
        };
        found.or_else(|| {
            (self.search.status == SearchStatus::Complete).then_some(if request.forward {
                0
            } else {
                matches.len() - 1
            })
        })
    }
    fn find(&mut self, forward: bool) -> Result<Option<Range<usize>>, WorkbenchError> {
        self.valid_search(false)?;
        self.remember_search();
        // Empty matches own no selection. Repeated F3 must advance instead of
        // selecting the same caret forever; only reuse a still-current caret.
        if let Some(index) = self.search.current {
            if let Some(hit) = self.search.matches.get(index) {
                if hit.range.is_empty()
                    && self.active_editor().is_some_and(|e| {
                        e.cursor == hit.range.start && e.anchor == Some(hit.range.start)
                    })
                    && self.search.status == SearchStatus::Complete
                {
                    let next = if forward {
                        (index + 1) % self.search.matches.len()
                    } else {
                        (index + self.search.matches.len() - 1) % self.search.matches.len()
                    };
                    let range = self.search.matches[next].range.clone();
                    self.select_search_match(next)?;
                    return Ok(Some(range));
                }
            }
        }
        let from = self.active_editor().map_or(0, |e| {
            e.selection()
                .map_or(e.cursor, |s| if forward { s.end } else { s.start })
        });
        let request = Navigation { forward, from };
        if let Some(index) = self.navigation_index(request) {
            let range = self.search.matches[index].range.clone();
            self.select_search_match(index)?;
            return Ok(Some(range));
        }
        if self.search.status == SearchStatus::Running {
            let editor = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
            self.search_navigation = Some(PendingNavigation {
                request,
                view: self
                    .panes
                    .view(self.panes.active())
                    .and_then(|v| v.view_id()),
                cursor: editor.cursor,
                anchor: editor.anchor,
            });
        }
        Ok(None)
    }
    pub fn find_selection(&mut self, forward: bool) -> Result<(), WorkbenchError> {
        let editor = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
        let Some(range) = editor.selection() else {
            return Ok(());
        };
        if range.len() > 4096 {
            return Err(WorkbenchError::SearchInput);
        }
        let encoding = editor
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let bytes = editor
            .session
            .document()
            .read_range(range.clone())
            .map_err(FileError::from)?;
        let decoded = decode_fragment(&bytes, encoding, range.start);
        if !decoded.issues.is_empty() || decoded.text.len() > 4096 {
            return Err(WorkbenchError::SearchInput);
        }
        self.search.regex = false;
        self.search.query = decoded.text;
        self.restart_search()?;
        let _ = self.find(forward)?;
        Ok(())
    }
    pub fn remember_search(&mut self) {
        if self.search.query.is_empty() {
            return;
        }
        let entry = SearchHistoryEntry {
            query: self.search.query.clone(),
            case_sensitive: self.search.case_sensitive,
            whole_word: self.search.whole_word,
            regex: self.search.regex,
        };
        self.search.history.retain(|old| old != &entry);
        self.search.history.push_front(entry);
        self.search.history.truncate(20);
        self.search_history_index = None;
    }
    pub fn cycle_search_history(&mut self, previous: bool) -> Result<(), WorkbenchError> {
        if self.search.history.is_empty() {
            return Ok(());
        }
        let index = match self.search_history_index {
            None => 0,
            Some(i) if previous => (i + 1).min(self.search.history.len() - 1),
            Some(i) => i.saturating_sub(1),
        };
        let entry = self.search.history[index].clone();
        self.search.case_sensitive = entry.case_sensitive;
        self.search.whole_word = entry.whole_word;
        self.search.regex = entry.regex;
        self.search.query = entry.query;
        self.restart_search()?;
        self.search_history_index = Some(index);
        Ok(())
    }
    /// One-based input is parsed at the frontend; this API takes scalar positions.
    pub fn go_to_position(&mut self, position: TextPosition) -> Result<(), WorkbenchError> {
        let offset = self.with_active_text(|text, source| text.offset(source, position))??;
        let tab = self
            .active_document()
            .ok_or(WorkbenchError::NoDocument)?
            .tab_id;
        if !self.jump_to_location(tab, offset) {
            return Err(WorkbenchError::NoDocument);
        }
        Ok(())
    }
}

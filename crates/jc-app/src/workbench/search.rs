use super::{Workbench, WorkbenchError};
use jc_files::FileError;
use jc_text::{encode, TextIndex, TextPosition};
use std::ops::Range;

impl Workbench {
    pub fn set_search_query(&mut self, query: impl Into<String>) -> Result<(), WorkbenchError> {
        self.search.query = query.into();
        self.rebuild_search()
    }

    pub fn toggle_search_case(&mut self) -> Result<(), WorkbenchError> {
        self.search.case_sensitive = !self.search.case_sensitive;
        self.rebuild_search()
    }

    pub fn toggle_search_whole_word(&mut self) -> Result<(), WorkbenchError> {
        self.search.whole_word = !self.search.whole_word;
        self.rebuild_search()
    }

    pub fn toggle_search_regex(&mut self) -> Result<(), WorkbenchError> {
        self.search.regex = !self.search.regex;
        self.rebuild_search()
    }

    pub(crate) fn rebuild_search(&mut self) -> Result<(), WorkbenchError> {
        self.search.matches.clear();
        self.search.current = None;
        if self.search.query.is_empty() || self.active_editor().is_none() {
            return Ok(());
        }
        if self.search.regex {
            return Err(WorkbenchError::RegexUnavailable);
        }
        let document = self.active_editor().ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let needle =
            encode(&self.search.query, encoding).map_err(|_| WorkbenchError::SearchEncoding)?;
        if needle.is_empty() {
            return Ok(());
        }
        let haystack = document.session.document().to_bytes();
        let decoded = jc_text::decode(&haystack, encoding);
        let index = TextIndex::new(&decoded);
        let case_sensitive = self.search.case_sensitive;
        let whole_word = self.search.whole_word;
        let mut matches = Vec::new();
        let mut offset = 0usize;
        while offset + needle.len() <= haystack.len() {
            let candidate = &haystack[offset..offset + needle.len()];
            let equal = if case_sensitive {
                candidate == needle
            } else {
                candidate
                    .iter()
                    .zip(&needle)
                    .all(|(a, b)| a.eq_ignore_ascii_case(b))
            };
            if equal
                && index.source_byte_to_position(offset).is_ok()
                && index.source_byte_to_position(offset + needle.len()).is_ok()
                && (!whole_word || is_word_match(&haystack, offset, offset + needle.len()))
            {
                matches.push(offset..offset + needle.len());
                offset += needle.len();
            } else {
                offset += 1;
            }
        }
        self.search.matches = matches;
        Ok(())
    }

    pub fn replace_current(&mut self, replacement: &str) -> Result<bool, WorkbenchError> {
        if self.search.regex {
            return Err(WorkbenchError::RegexUnavailable);
        }
        if self.search.matches.is_empty() {
            return Ok(false);
        }
        let index = self
            .search
            .current
            .unwrap_or(0)
            .min(self.search.matches.len() - 1);
        let range = self.search.matches[index].clone();
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let length = encode(replacement, encoding)
            .map_err(FileError::from)?
            .len();
        self.apply_text_edits(
            document.id,
            document.session.document().revision(),
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
        if self.search.regex {
            return Err(WorkbenchError::RegexUnavailable);
        }
        let matches = self.search.matches.clone();
        if matches.is_empty() {
            return Ok(0);
        }
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
        self.apply_text_edits(
            document.id,
            document.session.document().revision(),
            matches
                .iter()
                .map(|range| super::TextReplacement {
                    range: range.clone(),
                    text: replacement.to_owned(),
                })
                .collect(),
        )?;
        self.active_editor_mut()
            .ok_or(WorkbenchError::NoDocument)?
            .anchor = None;
        self.after_edit()?;
        Ok(matches.len())
    }

    pub fn find_next(&mut self) -> Result<Option<Range<usize>>, WorkbenchError> {
        self.find(true)
    }

    pub fn find_previous(&mut self) -> Result<Option<Range<usize>>, WorkbenchError> {
        self.find(false)
    }

    pub fn select_search_match(&mut self, index: usize) -> Result<bool, WorkbenchError> {
        let Some(range) = self.search.matches.get(index).cloned() else {
            return Ok(false);
        };
        self.search.current = Some(index);
        let mut document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        document.anchor = Some(range.start);
        document.cursor = range.end;
        Ok(true)
    }

    pub fn search_match_position(&self, index: usize) -> Option<TextPosition> {
        let range = self.search.matches.get(index)?;
        let decoded = self.active_decoded()?;
        TextIndex::new(&decoded)
            .source_byte_to_position(range.start)
            .ok()
    }

    fn find(&mut self, forward: bool) -> Result<Option<Range<usize>>, WorkbenchError> {
        if self.search.matches.is_empty() {
            return Ok(None);
        }
        let next = match self.search.current {
            None => {
                if forward {
                    0
                } else {
                    self.search.matches.len() - 1
                }
            }
            Some(current) if forward => (current + 1) % self.search.matches.len(),
            Some(0) => self.search.matches.len() - 1,
            Some(current) => current - 1,
        };
        self.search.current = Some(next);
        let range = self.search.matches[next].clone();
        let mut document = self.active_editor_mut().ok_or(WorkbenchError::NoDocument)?;
        document.anchor = Some(range.start);
        document.cursor = range.end;
        Ok(Some(range))
    }
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_word_match(bytes: &[u8], start: usize, end: usize) -> bool {
    (start == 0 || !is_word(bytes[start - 1])) && (end == bytes.len() || !is_word(bytes[end]))
}

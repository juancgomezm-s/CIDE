use super::{Workbench, WorkbenchError};
use jc_files::FileError;
use jc_text::encode;
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
        if self.search.query.is_empty() || self.active_document().is_none() {
            return Ok(());
        }
        if self.search.regex {
            return Err(WorkbenchError::RegexUnavailable);
        }
        let document = self.active_document().ok_or(WorkbenchError::NoDocument)?;
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
            if equal && (!whole_word || is_word_match(&haystack, offset, offset + needle.len())) {
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
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let bytes = encode(replacement, encoding).map_err(FileError::from)?;
        document
            .session
            .document_mut()
            .replace(range.clone(), &bytes)
            .map_err(FileError::from)?;
        document.cursor = range.start + bytes.len();
        document.anchor = None;
        self.sync_active_tab_dirty();
        self.rebuild_search()?;
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
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
        let encoding = document
            .session
            .encoding()
            .ok_or(FileError::EncodingRequired)?;
        let bytes = encode(replacement, encoding).map_err(FileError::from)?;
        for range in matches.iter().rev() {
            document
                .session
                .document_mut()
                .replace(range.clone(), &bytes)
                .map_err(FileError::from)?;
        }
        document.anchor = None;
        document.cursor = document.cursor.min(document.session.document().len());
        self.sync_active_tab_dirty();
        self.rebuild_search()?;
        Ok(matches.len())
    }

    pub fn find_next(&mut self) -> Result<Option<Range<usize>>, WorkbenchError> {
        self.find(true)
    }

    pub fn find_previous(&mut self) -> Result<Option<Range<usize>>, WorkbenchError> {
        self.find(false)
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
        let document = self
            .active_document_mut()
            .ok_or(WorkbenchError::NoDocument)?;
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

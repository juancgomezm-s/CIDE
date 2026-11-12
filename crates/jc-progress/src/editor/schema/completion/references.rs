use super::{fold, identifier, Catalog, Context, Includes, Site, Symbols, Target};
use jc_app::{Cancellation, TaskFailure, TaskItem};
use jc_text::{decode_fragment, TextEncoding};
use std::{collections::BTreeSet, ops::Range, path::Path};

const MAX_CANDIDATES: usize = 2048;
const MAX_RESULTS: usize = 48;

pub(super) struct ReferenceScan {
    key: String,
    selected: Range<usize>,
    declarations: Vec<(Site, Site)>,
    occurrences: Vec<Occurrence>,
    partial: bool,
}

struct Occurrence {
    site: Site,
    binding: Option<Site>,
    forward: Option<bool>,
    line: usize,
}

impl ReferenceScan {
    pub(super) fn key(&self) -> &str {
        &self.key
    }

    pub(super) fn mark_partial(&mut self) {
        self.partial = true;
    }

    pub(super) fn matches(&self, key: &str) -> bool {
        self.key == key
    }

    pub(super) fn matches_token(&self, context: &Context<'_>) -> bool {
        let Some(token) = context.tokens.last() else {
            return false;
        };
        if !matches!(
            token.kind,
            super::SyntaxKind::Plain | super::SyntaxKind::Keyword
        ) {
            return false;
        }
        let bytes = context.bytes(token);
        if bytes.is_ascii() {
            return bytes.eq_ignore_ascii_case(self.key.as_bytes());
        }
        context
            .word(token)
            .is_some_and(|word| self.matches(&fold(&word)))
    }

    pub(super) fn declare(&mut self, site: Site, identity: Site) -> Result<(), TaskFailure> {
        if self.declarations.len() == MAX_CANDIDATES {
            return Err(TaskFailure::LimitExceeded);
        }
        self.declarations.push((site, identity));
        Ok(())
    }

    pub(super) fn observe(
        &mut self,
        context: &mut Context<'_>,
        binding: Option<Site>,
        forward: Option<bool>,
    ) -> Result<(), TaskFailure> {
        if binding.is_none() && forward.is_none() {
            return Ok(());
        }
        if self.occurrences.len() == MAX_CANDIDATES {
            return Err(TaskFailure::LimitExceeded);
        }
        let site = context.site(context.tokens.last().expect("matched identifier"));
        let (start, line) = &mut context.reference_line;
        for (offset, byte) in context.bytes[*start..site.offset].iter().enumerate() {
            if *byte == b'\n'
                || (*byte == b'\r' && context.bytes.get(*start + offset + 1) != Some(&b'\n'))
            {
                *line += 1;
            }
        }
        *start = site.offset;
        self.occurrences.push(Occurrence {
            site,
            binding,
            forward,
            line: *line,
        });
        Ok(())
    }

    fn finish(&self, symbols: &Symbols, display: &str) -> Vec<TaskItem> {
        let resolved = |occurrence: &Occurrence| {
            occurrence.binding.clone().or_else(|| {
                symbols
                    .routine_definition(&self.key, occurrence.forward?)
                    .map(|found| found.site().clone())
            })
        };
        let selected = |site: &Site| site.file.is_none() && site.offset == self.selected.start;
        let identity = self
            .declarations
            .iter()
            .find(|(site, _)| selected(site))
            .map(|(_, identity)| identity.clone())
            .or_else(|| {
                self.occurrences
                    .iter()
                    .find(|entry| selected(&entry.site))
                    .and_then(resolved)
            });
        let Some(identity) = identity else {
            return vec![TaskItem::Message("Símbolo ABL sin resolver".into())];
        };

        let mut items = Vec::new();
        let mut locations = BTreeSet::new();
        let mut truncated = false;
        for entry in &self.occurrences {
            if resolved(entry).as_ref() != Some(&identity)
                || self
                    .declarations
                    .iter()
                    .any(|(site, _)| *site == entry.site)
            {
                continue;
            }
            let path = entry.site.file.as_ref().map(|file| file.path.as_path());
            if !locations.insert((path, entry.site.offset)) {
                continue;
            }
            if items.len() == MAX_RESULTS {
                truncated = true;
                break;
            }
            let label = format!("{display} · lín {}", entry.line);
            items.push(TaskItem::Source(
                entry.site.item(label, self.selected.clone()),
            ));
        }
        let notice = match (truncated, self.partial) {
            (true, true) => Some("Primeras 48 · se omitieron instrucciones que exceden la cuota"),
            (true, false) => Some("Se muestran las primeras 48"),
            (false, true) => Some("Se omitieron instrucciones que exceden la cuota"),
            (false, false) => None,
        };
        if let Some(notice) = notice {
            items.push(TaskItem::Message(notice.into()));
        }
        items
    }
}

pub(super) fn binding_context(
    context: &Context<'_>,
    symbols: &Symbols,
    key: &str,
) -> (Option<Site>, Option<bool>) {
    let Some(token) = context.tokens.last() else {
        return (None, None);
    };
    let before = &context.bytes[..token.range.start];
    if before.ends_with(b":") {
        // A handle or object member after ':' is not a lexical name here.
        return (None, None);
    }
    let end = token.range.end;
    let Some((target, range, _)) = context.target(end) else {
        return (None, None);
    };
    if range != token.range {
        return (None, None);
    }
    let tail = &context.bytes[end..];
    let qualified =
        tail.first() == Some(&b'.') && tail.get(1).is_some_and(|byte| identifier_byte(*byte));
    let next = tail
        .iter()
        .take(128)
        .find(|byte| !byte.is_ascii_whitespace());
    let routine = match target {
        Target::Symbols { run: true }
            if !qualified && (matches!(next, Some(b'.' | b'(')) || tail.is_empty()) =>
        {
            Some(true)
        }
        Target::Symbols { run: true } => return (None, None),
        Target::Symbols { run: false } if !qualified && next == Some(&b'(') => Some(false),
        _ => None,
    };
    let found = if let Some(run) = routine {
        symbols.routine_definition(key, run)
    } else {
        symbols.definition(&target, key, qualified)
    };
    (found.map(|found| found.site().clone()), routine)
}

fn identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') || byte >= 0x80
}

fn name_at(bytes: &[u8], encoding: TextEncoding, cursor: usize) -> Option<(Range<usize>, String)> {
    let tail = bytes.get(cursor..)?;
    let bom = if encoding == TextEncoding::Utf8 && bytes.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    let before = bytes.get(bom..cursor)?;
    let left = before
        .iter()
        .rev()
        .take(129)
        .take_while(|&&byte| identifier_byte(byte))
        .count();
    let right = tail
        .iter()
        .take(129)
        .take_while(|&&byte| identifier_byte(byte))
        .count();
    if !(1..=128).contains(&(left + right)) {
        return None;
    }
    let range = cursor - left..cursor + right;
    let decoded = decode_fragment(&bytes[range.clone()], encoding, range.start);
    (decoded.issues.is_empty() && identifier(&decoded.text)).then_some((range, decoded.text))
}

impl Catalog {
    pub(super) fn references(
        &self,
        source: &Path,
        bytes: &[u8],
        encoding: TextEncoding,
        cursor: usize,
        cancel: &Cancellation,
    ) -> Result<Vec<TaskItem>, TaskFailure> {
        let Some((selected, name)) = name_at(bytes, encoding, cursor) else {
            return Ok(Vec::new());
        };
        let mut symbols = Symbols::default();
        symbols.track_references(ReferenceScan {
            key: fold(&name),
            selected,
            declarations: Vec::new(),
            occurrences: Vec::new(),
            partial: false,
        });
        Includes::new(&self.root, source).with_locations().scan(
            &mut Context::new(bytes, encoding),
            &mut symbols,
            self,
            cancel,
        )?;
        let references = symbols.take_references().expect("active reference search");
        Ok(references.finish(&symbols, &name))
    }
}

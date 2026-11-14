use super::{identifier, Context, SourceItem};
use crate::editor::project_propath_dirs;
use jc_app::{source_services::SourceAction, Cancellation, TaskFailure};
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};
use jc_text::{decode_fragment, TextEncoding};
use std::{
    io::ErrorKind,
    ops::Range,
    path::{Path, PathBuf},
};

const MAX_IMPORTS: usize = 128;
const MAX_CLASS_BYTES: usize = 512 * 1024;
const MAX_NAME: usize = 512;

struct Name {
    text: String,
    range: Range<usize>,
    last: usize,
    wildcard: bool,
}

#[derive(Default)]
pub(super) struct Imports(Vec<Import>);

struct Import {
    name: Name,
    assembly: bool,
}

pub(super) struct TypeUse {
    name: Name,
    in_using: bool,
    assembly: bool,
    pub static_access: bool,
}

impl Imports {
    pub(super) fn add(&mut self, context: &Context<'_>) -> Result<(), TaskFailure> {
        let Some(token) = context.tokens.get(1) else {
            return Ok(());
        };
        let Some(name) = name_at(context.bytes, context.encoding, token.range.start) else {
            return Ok(());
        };
        let suffix = context
            .tokens
            .iter()
            .filter(|token| token.range.start >= name.range.end)
            .map(|token| context.bytes(token))
            .collect::<Vec<_>>();
        let assembly = match suffix.as_slice() {
            [] => false,
            [from, mode]
                if from.eq_ignore_ascii_case(b"FROM") && mode.eq_ignore_ascii_case(b"PROPATH") =>
            {
                false
            }
            [from, mode]
                if from.eq_ignore_ascii_case(b"FROM") && mode.eq_ignore_ascii_case(b"ASSEMBLY") =>
            {
                true
            }
            _ => return Ok(()),
        };
        if self.0.len() == MAX_IMPORTS {
            return Err(TaskFailure::LimitExceeded);
        }
        self.0.push(Import { name, assembly });
        Ok(())
    }

    pub(super) fn resolve(
        &self,
        root: &Path,
        source: &Path,
        use_: TypeUse,
        encoding: TextEncoding,
        cancel: &Cancellation,
    ) -> Result<Option<SourceItem>, TaskFailure> {
        if use_.assembly || use_.name.wildcard || use_.in_using && !use_.name.text.contains('.') {
            return Ok(None);
        }
        let text = &use_.name.text;
        let direct = use_.in_using || text.contains('.');
        let explicit = (!direct)
            .then(|| {
                self.0.iter().find(|import| {
                    !import.name.wildcard
                        && import
                            .name
                            .text
                            .rsplit('.')
                            .next()
                            .is_some_and(|name| name.eq_ignore_ascii_case(text))
                })
            })
            .flatten();
        if explicit.is_some_and(|import| import.assembly) {
            return Ok(None);
        }
        let wildcards = self
            .0
            .iter()
            .filter(|import| import.name.wildcard && !import.assembly);
        let names = if direct {
            vec![text.clone()]
        } else if let Some(import) = explicit {
            vec![import.name.text.clone()]
        } else {
            wildcards
                .map(|import| format!("{}.{text}", import.name.text))
                .collect()
        };
        if names.is_empty() {
            return Ok(None);
        }
        let root = root.canonicalize().map_err(|_| TaskFailure::Failed)?;
        let parent = source
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .filter(|parent| parent.starts_with(&root))
            .unwrap_or_else(|| root.clone());
        let source = parent.join(source.file_name().ok_or(TaskFailure::Failed)?);
        let dirs = project_propath_dirs(&root, &source).map_err(|_| TaskFailure::Failed)?;
        for name in names {
            let mut relative: PathBuf = name.split('.').collect();
            relative.set_extension("cls");
            for dir in &dirs {
                if cancel.is_cancelled() {
                    return Err(TaskFailure::Cancelled);
                }
                let path = dir.join(&relative);
                let (path, bytes) = match jc_files::read_in_root(&root, &path, MAX_CLASS_BYTES) {
                    Ok(result) => result,
                    Err(error) if error.kind() == ErrorKind::NotFound => {
                        if path.with_extension("r").is_file() {
                            return Ok(None);
                        }
                        continue;
                    }
                    Err(_) => return Err(TaskFailure::Failed),
                };
                let encoding = jc_text::probe_encoding(&bytes, None)
                    .encoding
                    .unwrap_or(encoding);
                let Some((kind, offset)) = declaration(&bytes, encoding, &name, cancel)? else {
                    return Ok(None);
                };
                let label: String = name.chars().take(88).collect();
                return Ok(Some(SourceItem {
                    range: use_.name.range,
                    label: format!("{kind} · {label}"),
                    action: SourceAction::ProjectLocation {
                        path,
                        offset,
                        encoding,
                        version: jc_files::ContentFingerprint::of(&bytes),
                    },
                }));
            }
        }
        Ok(None)
    }
}

pub(super) fn type_use(
    context: &Context<'_>,
    full: &[u8],
    selected: &Range<usize>,
) -> Option<TypeUse> {
    let tokens = &context.tokens;
    let mut start = tokens.len().checked_sub(1)?;
    let last = tokens.get(start)?;
    if last.range != *selected || context.word(last).is_none() {
        return None;
    }
    while start >= 2
        && context.bytes(&tokens[start - 1]) == b"."
        && tokens[start - 1].range.end == tokens[start].range.start
        && tokens[start - 2].range.end == tokens[start - 1].range.start
        && context.word(&tokens[start - 2]).is_some()
    {
        start -= 2;
    }
    let name = name_at(full, context.encoding, tokens[start].range.start)?;
    if name.range.end < selected.end {
        return None;
    }
    let previous = tokens.get(..start)?;
    let in_using =
        previous.len() == 1 && context.bytes(&previous[0]).eq_ignore_ascii_case(b"USING");
    let tail = &full[name.range.end..];
    let assembly = in_using && {
        let mut suffix = tail[..tail.len().min(128)]
            .split(|byte| byte.is_ascii_whitespace() || *byte == b'.')
            .filter(|word| !word.is_empty());
        suffix
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case(b"FROM"))
            && suffix
                .next()
                .is_some_and(|word| word.eq_ignore_ascii_case(b"ASSEMBLY"))
    };
    let type_word = |word: &[u8]| {
        [
            b"AS".as_slice(),
            b"NEW",
            b"INHERITS",
            b"IMPLEMENTS",
            b"RETURNS",
        ]
        .iter()
        .any(|keyword| word.eq_ignore_ascii_case(keyword))
    };
    let typed = previous
        .last()
        .is_some_and(|token| type_word(context.bytes(token)))
        || previous.len() >= 2
            && context
                .bytes(previous.last()?)
                .eq_ignore_ascii_case(b"CLASS")
            && type_word(context.bytes(&previous[previous.len() - 2]))
        || type_argument(context, previous)
        || previous
            .last()
            .is_some_and(|token| context.bytes(token) == b",")
            && previous
                .first()
                .is_some_and(|token| context.bytes(token).eq_ignore_ascii_case(b"CLASS"))
            && previous
                .iter()
                .any(|token| context.bytes(token).eq_ignore_ascii_case(b"IMPLEMENTS"));
    let static_access = !in_using
        && !typed
        && tail.first() == Some(&b':')
        && tail.get(1).is_some_and(|byte| component_byte(*byte));
    (in_using || typed || static_access).then_some(TypeUse {
        name,
        in_using,
        assembly,
        static_access,
    })
}

fn type_argument(context: &Context<'_>, previous: &[super::Token]) -> bool {
    let Some((last, before)) = previous.split_last() else {
        return false;
    };
    if context.bytes(last) == b"(" {
        return before
            .last()
            .is_some_and(|token| context.bytes(token).eq_ignore_ascii_case(b"GET-CLASS"));
    }
    if context.bytes(last) != b"," {
        return false;
    }
    let mut nesting = 0;
    for (index, token) in before.iter().enumerate().rev() {
        match context.bytes(token) {
            b")" => nesting += 1,
            b"(" if nesting > 0 => nesting -= 1,
            b"," if nesting == 0 => return false,
            b"(" => {
                return index.checked_sub(1).is_some_and(|index| {
                    let keyword = context.bytes(&before[index]);
                    keyword.eq_ignore_ascii_case(b"CAST")
                        || keyword.eq_ignore_ascii_case(b"TYPE-OF")
                });
            }
            _ => {}
        }
    }
    false
}

fn component_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') || byte >= 0x80
}

fn name_at(bytes: &[u8], encoding: TextEncoding, start: usize) -> Option<Name> {
    let mut end = start;
    let mut last;
    let mut wildcard = false;
    loop {
        last = end;
        while bytes.get(end).is_some_and(|byte| component_byte(*byte)) {
            end += 1;
            if end - start > MAX_NAME {
                return None;
            }
        }
        if end == last {
            return None;
        }
        if bytes.get(end) != Some(&b'.') {
            break;
        }
        match bytes.get(end + 1) {
            Some(b'*') => {
                wildcard = true;
                break;
            }
            Some(byte) if component_byte(*byte) => end += 1,
            _ => break,
        }
    }
    let decoded = decode_fragment(bytes.get(start..end)?, encoding, start);
    if !decoded.issues.is_empty() || !decoded.text.split('.').all(identifier) {
        return None;
    }
    let range = start..end + 2 * usize::from(wildcard);
    Some(Name {
        text: decoded.text,
        range,
        last,
        wildcard,
    })
}

fn declaration(
    bytes: &[u8],
    encoding: TextEncoding,
    expected: &str,
    cancel: &Cancellation,
) -> Result<Option<(&'static str, usize)>, TaskFailure> {
    let mut lexer = LexerState::default();
    let mut at_start = true;
    let mut header = None;
    let mut matched = None;
    let mut finished = false;
    let mut conditional = 0_usize;
    let bom = usize::from(encoding == TextEncoding::Utf8 && bytes.starts_with(b"\xef\xbb\xbf")) * 3;
    for (chunk, slice) in bytes[bom..].chunks(4096).enumerate() {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        let offset = bom + chunk * 4096;
        scan(
            SyntaxProfile::Abl,
            &mut lexer,
            slice,
            offset,
            offset + slice.len() == bytes.len(),
            |span| {
                if finished {
                    return;
                }
                let token = &bytes[span.source.clone()];
                if span.kind == SyntaxKind::Preprocessor {
                    if token == b"&" {
                        let directive = bytes[span.source.start..]
                            .split(|b| b.is_ascii_whitespace())
                            .next()
                            .unwrap_or_default();
                        if directive.eq_ignore_ascii_case(b"&IF") {
                            conditional += 1;
                        }
                        if directive.eq_ignore_ascii_case(b"&ENDIF") {
                            conditional = conditional.saturating_sub(1);
                        }
                    }
                    return;
                }
                if span.kind == SyntaxKind::Comment || token.iter().all(u8::is_ascii_whitespace) {
                    return;
                }
                if let Some(kind) = header.take() {
                    matched = name_at(bytes, encoding, span.source.start).and_then(|name| {
                        (!name.wildcard && name.text.eq_ignore_ascii_case(expected))
                            .then_some((kind, name.last))
                    });
                    finished = true;
                    return;
                }
                if at_start && conditional == 0 {
                    header = [
                        (b"CLASS".as_slice(), "Clase"),
                        (b"INTERFACE", "Interfaz"),
                        (b"ENUM", "Enumeración"),
                    ]
                    .into_iter()
                    .find_map(|(keyword, kind)| {
                        token.eq_ignore_ascii_case(keyword).then_some(kind)
                    });
                }
                at_start = span.kind == SyntaxKind::Operator
                    && token == b"."
                    && bytes
                        .get(span.source.end)
                        .is_none_or(u8::is_ascii_whitespace);
            },
        );
        if finished {
            break;
        }
    }
    Ok(matched)
}

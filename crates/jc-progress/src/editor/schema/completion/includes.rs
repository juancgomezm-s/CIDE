use super::{Catalog, Context, Symbols, Token};
use crate::editor::project_propath_dirs;
use jc_app::{Cancellation, TaskFailure};
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};
use jc_text::{decode_fragment, TextEncoding};
use std::{
    io::ErrorKind,
    ops::Range,
    path::{Component, Path, PathBuf},
};

const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_INCLUDE_BYTES: usize = 1024 * 1024;
const MAX_INCLUDES: usize = 32;
const MAX_DEPTH: usize = 8;

pub(super) struct Includes<'a> {
    source: &'a Path,
    root: &'a Path,
    paths: Option<(PathBuf, Vec<PathBuf>)>,
    stack: Vec<PathBuf>,
    files: usize,
    bytes: usize,
}

impl<'a> Includes<'a> {
    pub(super) fn new(root: &'a Path, source: &'a Path) -> Self {
        Self {
            source,
            root,
            paths: None,
            stack: Vec::new(),
            files: 0,
            bytes: 0,
        }
    }

    fn read(&mut self, target: &str) -> Result<Option<(PathBuf, Vec<u8>)>, TaskFailure> {
        if self.stack.len() == MAX_DEPTH || self.files == MAX_INCLUDES {
            return Err(TaskFailure::LimitExceeded);
        }
        self.files += 1;
        if self.paths.is_none() {
            let root = self.root.canonicalize().map_err(|_| TaskFailure::Failed)?;
            let parent = self
                .source
                .parent()
                .and_then(|parent| parent.canonicalize().ok())
                .filter(|parent| parent.starts_with(&root));
            let source = parent
                .map(|parent| parent.join("fuente.p"))
                .unwrap_or_else(|| root.join("fuente.p"));
            let dirs = project_propath_dirs(&root, &source).map_err(|_| TaskFailure::Failed)?;
            self.paths = Some((root, dirs));
        }
        let (root, dirs) = self.paths.as_ref().expect("initialized PROPATH");
        for dir in dirs {
            let limit = MAX_FILE_BYTES.min(MAX_INCLUDE_BYTES - self.bytes);
            match jc_files::read_in_root(root, &dir.join(target), limit) {
                Ok((path, bytes)) => {
                    self.bytes += bytes.len();
                    if self.stack.contains(&path) {
                        return Ok(None);
                    }
                    return Ok(Some((path, bytes)));
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(_) => return Err(TaskFailure::Failed),
            }
        }
        Ok(None)
    }

    fn visit(
        &mut self,
        directive: &str,
        encoding: TextEncoding,
        symbols: &mut Symbols,
        catalog: &Catalog,
        cancel: &Cancellation,
    ) -> Result<(), TaskFailure> {
        let Some(target) = static_target(directive) else {
            return Ok(());
        };
        let Some((path, bytes)) = self.read(target)? else {
            return Ok(());
        };
        let encoding = if std::str::from_utf8(&bytes).is_ok() {
            TextEncoding::Utf8
        } else {
            encoding
        };
        self.stack.push(path);
        let result = self.scan(
            &mut Context::new(&bytes, encoding),
            symbols,
            catalog,
            cancel,
        );
        self.stack.pop();
        result.map(|_| ())
    }

    pub(super) fn scan(
        &mut self,
        context: &mut Context<'_>,
        symbols: &mut Symbols,
        catalog: &Catalog,
        cancel: &Cancellation,
    ) -> Result<bool, TaskFailure> {
        let bytes = context.bytes;
        let mut lexer = LexerState::default();
        let mut protected = false;
        let mut directive: Option<Range<usize>> = None;
        let mut conditional = 0_usize;
        let mut process_directive = |range: Range<usize>,
                                     context: &mut Context<'_>,
                                     symbols: &mut Symbols| {
            let standalone = context.tokens.is_empty() && !context.overflow;
            context.tokens.clear();
            context.overflow = false;
            if range.len() <= 512 {
                let decoded = decode_fragment(&bytes[range.clone()], context.encoding, range.start);
                if decoded.issues.is_empty() {
                    let text = decoded.text.trim();
                    let keyword = text.split_ascii_whitespace().next().unwrap_or_default();
                    if keyword.eq_ignore_ascii_case("&IF") {
                        conditional += 1;
                    } else if keyword.eq_ignore_ascii_case("&ENDIF") {
                        conditional = conditional.saturating_sub(1);
                    } else if conditional == 0 && standalone {
                        self.visit(text, context.encoding, symbols, catalog, cancel)?;
                    }
                }
            }
            Ok(conditional == 0)
        };
        let mut certain = true;
        let bom = if context.encoding == TextEncoding::Utf8 && bytes.starts_with(b"\xef\xbb\xbf") {
            3
        } else {
            0
        };
        let mut failure = Ok(());
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
                    if failure.is_err() {
                        return;
                    }
                    if span.source.end == bytes.len() {
                        protected = matches!(
                            span.kind,
                            SyntaxKind::Comment | SyntaxKind::String | SyntaxKind::Preprocessor
                        );
                    }
                    if span.kind == SyntaxKind::Preprocessor {
                        match &mut directive {
                            Some(range) => range.end = span.source.end,
                            None => directive = Some(span.source),
                        }
                        return;
                    }
                    failure = (|| {
                        if let Some(range) = directive.take() {
                            certain = process_directive(range, context, symbols)?;
                        }
                        context.push(
                            Token {
                                kind: span.kind,
                                range: span.source,
                            },
                            symbols,
                            catalog,
                            certain,
                        )
                    })();
                },
            );
            failure?;
        }
        if lexer.limited() {
            return Err(TaskFailure::LimitExceeded);
        }
        if let Some(range) = directive {
            process_directive(range, context, symbols)?;
        }
        Ok(protected)
    }
}

fn static_target(directive: &str) -> Option<&str> {
    let text = directive.strip_prefix('{')?.strip_suffix('}')?.trim();
    let path = if text.starts_with('"') || text.starts_with('\'') {
        let quote = text.chars().next()?;
        text.strip_prefix(quote)?.strip_suffix(quote)?
    } else {
        if text.chars().any(char::is_whitespace) {
            return None;
        }
        text
    };
    if path.is_empty()
        || path.bytes().all(|byte| byte.is_ascii_digit())
        || path
            .chars()
            .any(|ch| ch.is_control() || "{}&~:'\"<>|?*".contains(ch))
    {
        return None;
    }
    let path_parts = Path::new(path);
    path_parts
        .components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
        .then_some(path)
}

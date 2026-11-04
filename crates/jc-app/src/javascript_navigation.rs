//! Bounded lexical navigation for JavaScript ES modules.
use crate::{
    source_services::{FileResolution, SourceAction, SourceItem, SourceProviderDescriptor},
    Cancellation, TaskFailure, TaskItem, TaskOutput, TaskSnapshot,
};
use jc_text::{decode_fragment, TextEncoding};
use std::ops::Range;

pub(crate) fn descriptor() -> SourceProviderDescriptor {
    SourceProviderDescriptor {
        id: "core.javascript",
        language: "javascript",
        symbols: Some(symbols),
        references: Some(dependencies),
        calls: None,
        dependencies: Some(dependencies),
        file_resolution: FileResolution::DocumentRelative,
        completion: None,
        snippets: &[],
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    String,
    Other,
    Punct(u8),
}

#[derive(Clone)]
struct Token {
    kind: Kind,
    range: Range<usize>,
}

#[derive(Clone)]
struct Lexer<'a> {
    bytes: &'a [u8],
    at: usize,
    previous: Option<Kind>,
}

fn word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$') || b >= 128
}

impl Lexer<'_> {
    fn next(&mut self) -> Option<Token> {
        loop {
            let tail = self.bytes.get(self.at..)?;
            match *tail.first()? {
                b if b.is_ascii_whitespace() => self.at += 1,
                b'/' if tail.starts_with(b"//") => {
                    self.at += tail
                        .iter()
                        .position(|b| matches!(b, b'\r' | b'\n'))
                        .unwrap_or(tail.len());
                }
                b'/' if tail.starts_with(b"/*") => {
                    self.at += tail[2..]
                        .windows(2)
                        .position(|pair| pair == b"*/")
                        .map_or(tail.len(), |end| end + 4);
                }
                _ => break,
            }
        }
        let start = self.at;
        let byte = self.bytes[start];
        self.at += 1;
        let kind = match byte {
            b if word_byte(b) => {
                while self.bytes.get(self.at).is_some_and(|&b| word_byte(b)) {
                    self.at += 1;
                }
                Kind::Word
            }
            b'\'' | b'"' | b'`' => {
                if !self.quoted(byte) {
                    self.at = self.bytes.len();
                    return None;
                }
                if byte == b'`' {
                    Kind::Other
                } else {
                    Kind::String
                }
            }
            b'/' => {
                // After other tokens the slash may be division or a regular expression.
                // Stop the prefix scan when its lexical goal cannot be established.
                if !matches!(
                    self.previous,
                    None | Some(Kind::Punct(b'=' | b'(' | b'[' | b',' | b':' | b'?' | b'!'))
                ) || !self.regexp()
                {
                    self.at = self.bytes.len();
                    return None;
                }
                Kind::Other
            }
            b'<' if self
                .bytes
                .get(self.at)
                .is_some_and(|&b| word_byte(b) || b == b'>') =>
            {
                // JSX text uses a different grammar and may contain apparent imports.
                self.at = self.bytes.len();
                return None;
            }
            b => Kind::Punct(b),
        };
        self.previous = Some(kind);
        Some(Token {
            kind,
            range: start..self.at,
        })
    }

    fn quoted(&mut self, quote: u8) -> bool {
        while let Some(&b) = self.bytes.get(self.at) {
            self.at += 1;
            match b {
                b if b == quote => return true,
                b'\\' => {
                    if self.bytes.get(self.at..self.at + 2) == Some(b"\r\n") {
                        self.at += 2;
                    } else {
                        self.at += 1;
                    }
                }
                b'$' if quote == b'`' && self.bytes.get(self.at) == Some(&b'{') => return false,
                b'\r' | b'\n' if quote != b'`' => return false,
                _ => {}
            }
        }
        false
    }

    fn regexp(&mut self) -> bool {
        let mut class = false;
        while let Some(&b) = self.bytes.get(self.at) {
            self.at += 1;
            match b {
                b'\\' => {
                    if self
                        .bytes
                        .get(self.at)
                        .is_some_and(|b| matches!(b, b'\r' | b'\n'))
                    {
                        return false;
                    }
                    self.at += 1;
                }
                b'[' if class => return false,
                b'[' => class = true,
                b']' => class = false,
                b'/' if !class => {
                    while self.bytes.get(self.at).is_some_and(u8::is_ascii_alphabetic) {
                        self.at += 1;
                    }
                    return true;
                }
                b'\r' | b'\n' => return false,
                _ => {}
            }
        }
        false
    }
}

fn word_is(lexer: &Lexer<'_>, token: &Token, value: &[u8]) -> bool {
    token.kind == Kind::Word && &lexer.bytes[token.range.clone()] == value
}

fn named_clause(lexer: &mut Lexer<'_>) -> Option<Token> {
    for _ in 0..128 {
        let token = lexer.next()?;
        match token.kind {
            Kind::Punct(b'}') => return lexer.next(),
            Kind::Word | Kind::String | Kind::Punct(b',') => {}
            _ => return None,
        }
    }
    None
}

fn import_target(lexer: &mut Lexer<'_>, mut first: Token, exporting: bool) -> Option<Token> {
    if !exporting && first.kind == Kind::String {
        return Some(first);
    }
    if !exporting && first.kind == Kind::Word {
        first = lexer.next()?;
        if first.kind == Kind::Punct(b',') {
            first = lexer.next()?;
            if !matches!(first.kind, Kind::Punct(b'{' | b'*')) {
                return None;
            }
        }
    } else if !matches!(first.kind, Kind::Punct(b'{' | b'*')) {
        return None;
    }
    let from = match first.kind {
        Kind::Punct(b'{') => named_clause(lexer)?,
        Kind::Punct(b'*') => {
            let next = lexer.next()?;
            if word_is(lexer, &next, b"as") {
                if lexer.next()?.kind != Kind::Word {
                    return None;
                }
                lexer.next()?
            } else if exporting {
                next
            } else {
                return None;
            }
        }
        _ => first,
    };
    if !word_is(lexer, &from, b"from") {
        return None;
    }
    lexer.next().filter(|token| token.kind == Kind::String)
}

fn dependency(bytes: &[u8], encoding: TextEncoding, token: Token) -> SourceItem {
    let content = token.range.start + 1..token.range.end - 1;
    let parsed = (content.len() <= 512)
        .then(|| decode_fragment(&bytes[content.clone()], encoding, content.start))
        .filter(|value| value.issues.is_empty() && !value.text.chars().any(char::is_control));
    let (label, action) = if let Some(value) = parsed {
        let path = value.text;
        let label = path.chars().take(100).collect::<String>();
        let action = if (path.starts_with("./") || path.starts_with("../"))
            && !path.contains(['\\', ':', '%', '#', '?', '*', '"', '<', '>', '|'])
        {
            SourceAction::RelativeFile(path)
        } else {
            SourceAction::Unresolved("Especificador externo, escapado o no admitido".into())
        };
        (format!("Módulo: {label}"), action)
    } else {
        (
            "Módulo sin resolver".into(),
            SourceAction::Unresolved("Especificador inválido o demasiado largo".into()),
        )
    };
    SourceItem {
        range: token.range,
        label,
        action,
    }
}

fn scan(
    bytes: &[u8],
    encoding: TextEncoding,
    cancel: &Cancellation,
    want_symbols: bool,
    mut emit: impl FnMut(SourceItem) -> Result<(), TaskFailure>,
) -> Result<(), TaskFailure> {
    let mut at = if encoding == TextEncoding::Utf8 && bytes.starts_with(b"\xef\xbb\xbf") {
        3
    } else {
        0
    };
    if bytes[at..].starts_with(b"#!") {
        at += bytes[at..]
            .iter()
            .position(|b| matches!(b, b'\r' | b'\n'))
            .unwrap_or(bytes.len() - at);
    }
    let mut lexer = Lexer {
        bytes,
        at,
        previous: None,
    };
    let mut depth = 0usize;
    let mut previous = None;
    while let Some(token) = lexer.next() {
        if cancel.is_cancelled() {
            return Err(TaskFailure::Cancelled);
        }
        let is_member = previous == Some(Kind::Punct(b'.'));
        previous = Some(token.kind);
        match token.kind {
            Kind::Punct(b'(' | b'[' | b'{') => depth += 1,
            Kind::Punct(b')' | b']' | b'}') => depth = depth.saturating_sub(1),
            Kind::Word if !is_member => {
                let word = &bytes[token.range.clone()];
                if want_symbols && depth == 0 && matches!(word, b"function" | b"class") {
                    let mut lookahead = lexer.clone();
                    let Some(mut name) = lookahead.next() else {
                        continue;
                    };
                    if word == b"function" && name.kind == Kind::Punct(b'*') {
                        let Some(generator) = lookahead.next() else {
                            continue;
                        };
                        name = generator;
                    }
                    if name.kind != Kind::Word
                        || name.range.len() > 384
                        || word_is(&lookahead, &name, b"extends")
                    {
                        continue;
                    }
                    let value =
                        decode_fragment(&bytes[name.range.clone()], encoding, name.range.start);
                    if value.issues.is_empty()
                        && value
                            .text
                            .chars()
                            .next()
                            .is_some_and(|ch| ch.is_alphabetic() || matches!(ch, '_' | '$'))
                    {
                        let kind = if word == b"function" {
                            "Función"
                        } else {
                            "Clase"
                        };
                        emit(SourceItem {
                            range: name.range,
                            label: format!("{kind}: {}", value.text),
                            action: SourceAction::Navigate,
                        })?;
                    }
                } else if !want_symbols && (word == b"import" || word == b"export" && depth == 0) {
                    let mut lookahead = lexer.clone();
                    let Some(first) = lookahead.next() else {
                        continue;
                    };
                    if word == b"import" && first.kind == Kind::Punct(b'(') {
                        // A property or method named import is legal. Admit the
                        // literal-argument form, which cannot be a declaration.
                        if !lookahead.next().is_some_and(|arg| arg.kind == Kind::String) {
                            continue;
                        }
                        emit(SourceItem {
                            range: token.range,
                            label: "Importación dinámica".into(),
                            action: SourceAction::Unresolved(
                                "Importación dinámica sin resolver".into(),
                            ),
                        })?;
                    } else if depth == 0 {
                        if let Some(target) =
                            import_target(&mut lookahead, first, word == b"export")
                        {
                            emit(dependency(bytes, encoding, target))?;
                            lexer = lookahead;
                            previous = lexer.previous;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn symbols(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    scan(
        snapshot.bytes(),
        snapshot.stamp.encoding,
        cancel,
        true,
        |item| output.push(TaskItem::Source(item)),
    )
}

fn dependencies(
    snapshot: &TaskSnapshot,
    cancel: &Cancellation,
    output: &mut TaskOutput,
) -> Result<(), TaskFailure> {
    scan(
        snapshot.bytes(),
        snapshot.stamp.encoding,
        cancel,
        false,
        |item| output.push(TaskItem::Source(item)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scan_js(bytes: &[u8], symbols: bool) -> Vec<SourceItem> {
        let mut items = Vec::new();
        scan(
            bytes,
            TextEncoding::Utf8,
            &Cancellation::new(Duration::from_secs(5)),
            symbols,
            |item| {
                items.push(item);
                Ok(())
            },
        )
        .unwrap();
        items
    }

    #[test]
    fn only_module_imports_and_reexports_form_relative_dependencies() {
        let source = br#"// import 'ignored.js'
const str = "import 'ignored.js'";
const sample = `import 'ignored.js'`;
const regex = /import '\.\/ignored.js'/;
import { from as value } from './local.js';
export * from '../shared.js';
import 'package';
export { value }
obj.import('./ignored.js');
function load() { import('./dynamic.js'); }
"#;
        let results = scan_js(source, false);
        assert_eq!(results.len(), 4);
        assert_eq!(
            results[0].action,
            SourceAction::RelativeFile("./local.js".into())
        );
        assert_eq!(
            results[1].action,
            SourceAction::RelativeFile("../shared.js".into())
        );
        assert!(matches!(results[2].action, SourceAction::Unresolved(_)));
        assert!(matches!(results[3].action, SourceAction::Unresolved(_)));
        assert_eq!(&source[results[0].range.clone()], b"'./local.js'");
    }

    #[test]
    fn finds_named_functions_and_classes_with_source_ranges() {
        let source =
            b"export function* read() { function nested() {} }\nexport default class Item {}";
        let results = scan_js(source, true);
        assert_eq!(results.len(), 2);
        assert_eq!(&source[results[0].range.clone()], b"read");
        assert_eq!(&source[results[1].range.clone()], b"Item");
    }
}

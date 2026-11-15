use super::{identifier, Catalog, Context, Includes, Site, SourceItem, Symbols};
use crate::editor::project_propath_dirs;
use jc_app::{source_services::SourceAction, Cancellation, TaskFailure};
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};
use jc_text::{decode_fragment, TextEncoding};
use std::{
    collections::BTreeSet,
    io::ErrorKind,
    ops::Range,
    path::{Path, PathBuf},
};

const MAX_IMPORTS: usize = 128;
const MAX_CLASS_BYTES: usize = 512 * 1024;
const MAX_NAME: usize = 512;
const MAX_BASE_CLASSES: usize = 16;
const MAX_SIGNATURES: usize = 32;
const MAX_ARGUMENT_BYTES: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Primitive {
    Character,
    Integer,
    Int64,
    Decimal,
    Logical,
    Unknown,
}

fn primitive(word: &[u8]) -> Primitive {
    [
        (b"CHARACTER".as_slice(), Primitive::Character),
        (b"INTEGER", Primitive::Integer),
        (b"INT64", Primitive::Int64),
        (b"DECIMAL", Primitive::Decimal),
        (b"LOGICAL", Primitive::Logical),
    ]
    .into_iter()
    .find_map(|(name, kind)| word.eq_ignore_ascii_case(name).then_some(kind))
    .unwrap_or(Primitive::Unknown)
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Signature {
    Property,
    Method(Vec<Primitive>),
}

pub(super) struct Hierarchy {
    name: String,
    class: Option<String>,
    parent: Option<String>,
    members: Vec<Member>,
}

struct Member {
    site: Site,
    signature: Signature,
    public: bool,
    private: bool,
    package_private: bool,
    abstract_: bool,
}

impl Hierarchy {
    pub(super) fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            class: None,
            parent: None,
            members: Vec::new(),
        }
    }

    pub(super) fn statement(
        &mut self,
        context: &Context<'_>,
        end: &[u8],
    ) -> Result<(), TaskFailure> {
        let tokens = &context.tokens;
        let is = |i: usize, word: &[u8]| {
            tokens
                .get(i)
                .is_some_and(|token| context.bytes(token).eq_ignore_ascii_case(word))
        };
        if end == b":" && is(0, b"CLASS") && self.class.is_none() && context.file.is_none() {
            let read = |i: usize| declared_type(context, tokens.get(i)?.range.start);
            self.class = read(1);
            self.parent = (2..tokens.len())
                .find(|&i| is(i, b"INHERITS"))
                .and_then(|i| read(i + 1));
        }
        if self.class.is_none() {
            return Ok(());
        }
        let (name, signature) = if is(0, b"METHOD") {
            let Some(open) = (2..tokens.len()).find(|&i| is(i, b"(")) else {
                return Ok(());
            };
            if !is(open - 1, self.name.as_bytes()) {
                return Ok(());
            }
            let Some(close) = (open + 1..tokens.len()).find(|&i| is(i, b")")) else {
                return Ok(());
            };
            let parameters = &tokens[open + 1..close];
            let types = if parameters.is_empty() {
                Vec::new()
            } else {
                parameters
                    .split(|token| context.bytes(token) == b",")
                    .map(|parameter| {
                        let mut words = parameter.iter().map(|token| context.bytes(token));
                        let Some(first) = words.next() else {
                            return Primitive::Unknown;
                        };
                        if first.eq_ignore_ascii_case(b"OUTPUT")
                            || first.eq_ignore_ascii_case(b"INPUT-OUTPUT")
                        {
                            return Primitive::Unknown;
                        }
                        let kind = words
                            .by_ref()
                            .skip_while(|word| !word.eq_ignore_ascii_case(b"AS"))
                            .nth(1)
                            .map_or(Primitive::Unknown, primitive);
                        if words.next().is_none() {
                            kind
                        } else {
                            Primitive::Unknown
                        }
                    })
                    .collect()
            };
            (open - 1, Signature::Method(types))
        } else if is(0, b"DEFINE") || is(0, b"DEF") {
            let Some(property) = (1..tokens.len()).find(|&i| is(i, b"PROPERTY")) else {
                return Ok(());
            };
            if !is(property + 1, self.name.as_bytes()) || !is(property + 2, b"AS") {
                return Ok(());
            }
            (property + 1, Signature::Property)
        } else {
            return Ok(());
        };
        let flags_end = if signature == Signature::Property {
            name - 1
        } else {
            name
        };
        let flags = |word| (1..flags_end).any(|i| is(i, word));
        if flags(b"STATIC") {
            return Ok(());
        }
        if self.members.len() == MAX_SIGNATURES {
            return Err(TaskFailure::LimitExceeded);
        }
        self.members.push(Member {
            site: context.site(&tokens[name]),
            signature,
            public: flags(b"PUBLIC"),
            private: flags(b"PRIVATE"),
            package_private: flags(b"PACKAGE-PRIVATE"),
            abstract_: flags(b"ABSTRACT"),
        });
        Ok(())
    }
}

pub(super) enum Receiver {
    Super,
    This,
    Variable(String),
}

pub(super) fn receiver(
    bytes: &[u8],
    range: &Range<usize>,
    encoding: TextEncoding,
) -> Option<Receiver> {
    let prefix = bytes.get(..range.start)?.strip_suffix(b":")?;
    let len = prefix
        .iter()
        .rev()
        .take(129)
        .take_while(|&&byte| component_byte(byte))
        .count();
    let start = prefix.len() - len;
    if !(1..=128).contains(&len) || matches!(prefix.get(start.wrapping_sub(1)), Some(b':' | b'.')) {
        return None;
    }
    let decoded = decode_fragment(&prefix[start..], encoding, start);
    let word = decoded.text;
    if !decoded.issues.is_empty() || !identifier(&word) {
        return None;
    }
    Some(if word.eq_ignore_ascii_case("SUPER") {
        Receiver::Super
    } else if word.eq_ignore_ascii_case("THIS-OBJECT") {
        Receiver::This
    } else {
        Receiver::Variable(word)
    })
}

pub(super) struct MemberUse {
    range: Range<usize>,
    name: String,
    receiver: Receiver,
    signature: Signature,
}

pub(super) fn member_use(
    context: &Context<'_>,
    range: Range<usize>,
    name: String,
    receiver: Receiver,
    symbols: &Symbols,
    cancel: &Cancellation,
) -> Result<Option<MemberUse>, TaskFailure> {
    if cancel.is_cancelled() {
        return Err(TaskFailure::Cancelled);
    }
    let suffix = &context.bytes[range.end..];
    let start = suffix
        .iter()
        .take(MAX_ARGUMENT_BYTES)
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(suffix.len());
    let signature = if suffix.get(start) == Some(&b'(') {
        let Some(arguments) = arguments(&suffix[start..], |argument| {
            literal(argument, symbols, context.encoding)
        }) else {
            return Ok(None);
        };
        Signature::Method(arguments)
    } else {
        if matches!(receiver, Receiver::Super) {
            return Ok(None);
        }
        Signature::Property
    };
    Ok(Some(MemberUse {
        range,
        name,
        receiver,
        signature,
    }))
}

fn arguments(source: &[u8], classify: impl Fn(&[u8]) -> Primitive) -> Option<Vec<Primitive>> {
    let slice = &source[..source.len().min(MAX_ARGUMENT_BYTES)];
    let mut lexer = LexerState::default();
    let mut args = Vec::new();
    let mut delimiters = Vec::new();
    let mut start = 1;
    let mut closed = false;
    let mut invalid = false;
    scan(SyntaxProfile::Abl, &mut lexer, slice, 0, true, |span| {
        if closed || invalid || span.kind != SyntaxKind::Operator {
            return;
        }
        let token = &slice[span.source.clone()];
        match token {
            b"(" | b"[" => delimiters.push(token[0]),
            b")" | b"]" => {
                let opener = if token == b")" { b'(' } else { b'[' };
                if delimiters.pop() != Some(opener) {
                    invalid = true;
                } else if delimiters.is_empty() {
                    closed = true;
                    let argument = slice[start..span.source.start].trim_ascii();
                    if !argument.is_empty() || !args.is_empty() {
                        args.push(classify(argument));
                    }
                }
            }
            b"," if delimiters.len() == 1 => {
                args.push(classify(slice[start..span.source.start].trim_ascii()));
                start = span.source.end;
            }
            _ => {}
        }
    });
    (closed && !invalid && args.len() <= 32).then_some(args)
}

fn literal(bytes: &[u8], symbols: &Symbols, encoding: TextEncoding) -> Primitive {
    let first = bytes
        .split(|byte| byte.is_ascii_whitespace())
        .next()
        .unwrap_or_default();
    let bytes = if first.eq_ignore_ascii_case(b"INPUT") {
        bytes[first.len()..].trim_ascii_start()
    } else {
        bytes
    };
    if bytes.eq_ignore_ascii_case(b"TRUE") || bytes.eq_ignore_ascii_case(b"FALSE") {
        return Primitive::Logical;
    }
    if bytes
        .first()
        .is_some_and(|byte| matches!(byte, b'"' | b'\''))
        && bytes.len() >= 2
    {
        let quote = bytes[0];
        let mut i = 1;
        while i < bytes.len() {
            if bytes[i] == b'~' || bytes[i] == quote && bytes.get(i + 1) == Some(&quote) {
                i += 2;
                continue;
            }
            if bytes[i] == quote {
                return if i + 1 == bytes.len() {
                    Primitive::Character
                } else {
                    Primitive::Unknown
                };
            }
            i += 1;
        }
        return Primitive::Unknown;
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        if text.parse::<i32>().is_ok() {
            return Primitive::Integer;
        }
        if text.parse::<i64>().is_ok() {
            return Primitive::Int64;
        }
        if text.contains('.')
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.'))
            && text.parse::<f64>().is_ok()
        {
            return Primitive::Decimal;
        }
    }
    let decoded = decode_fragment(bytes, encoding, 0);
    if decoded.issues.is_empty() && identifier(&decoded.text) {
        if let Some(ty) = symbols.object_type(&decoded.text) {
            return primitive(ty.as_bytes());
        }
    }
    Primitive::Unknown
}

fn package(class: &str) -> &str {
    class.rsplit_once('.').map_or("", |(package, _)| package)
}

impl Catalog {
    pub(super) fn inherited_member(
        &self,
        source: &Path,
        context: &Context<'_>,
        symbols: &Symbols,
        call: MemberUse,
        cancel: &Cancellation,
    ) -> Result<Option<SourceItem>, super::DefinitionFailure> {
        let inspect = |path: &Path, bytes: &[u8], encoding| {
            let mut symbols = Symbols::default();
            symbols.track_imports();
            symbols.track_hierarchy(&call.name);
            Includes::new(&self.root, path).with_locations().scan(
                &mut Context::new(bytes, encoding),
                &mut symbols,
                self,
                cancel,
            )?;
            Ok::<_, TaskFailure>(symbols.into_hierarchy())
        };
        let external = matches!(call.receiver, Receiver::Variable(_));
        let super_ = matches!(call.receiver, Receiver::Super);
        let mut loaded = if let Receiver::Variable(name) = &call.receiver {
            let Some(ty) = symbols.object_type(name) else {
                return Ok(None);
            };
            let Some(class) =
                symbols
                    .imports()
                    .load(&self.root, source, ty, false, context.encoding, cancel)?
            else {
                return Ok(None);
            };
            Some(class)
        } else {
            if !source
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("cls"))
            {
                return Ok(None);
            }
            None
        };
        let (mut hierarchy, mut imports) = if let Some(class) = &loaded {
            inspect(&class.path, &class.bytes, class.encoding)?
        } else {
            inspect(source, context.bytes, context.encoding)?
        };
        let Some(owner) = hierarchy.class.clone() else {
            return Ok(None);
        };
        let mut seen = BTreeSet::from([owner.to_lowercase()]);
        let mut signatures = BTreeSet::new();
        let mut exact = Vec::new();
        let mut uncertain = Vec::new();
        for depth in 0..=MAX_BASE_CLASSES {
            let class = hierarchy.class.as_deref().unwrap_or_default();
            if !super_ || depth != 0 {
                let mut declared_here = BTreeSet::new();
                for member in &hierarchy.members {
                    let same_package = package(class).eq_ignore_ascii_case(package(&owner));
                    let accessible = if external {
                        member.public
                    } else {
                        depth == 0 || !member.private && (!member.package_private || same_package)
                    };
                    if !accessible || signatures.contains(&member.signature) {
                        continue;
                    }
                    let rank = match (&call.signature, &member.signature) {
                        (Signature::Property, Signature::Property) => true,
                        (Signature::Method(args), Signature::Method(params))
                            if args.len() == params.len() =>
                        {
                            if !args.iter().zip(params).all(|(&arg, &param)| {
                                arg == param
                                    || matches!(arg, Primitive::Unknown)
                                    || matches!(param, Primitive::Unknown)
                                    || matches!(
                                        (arg, param),
                                        (Primitive::Integer, Primitive::Int64 | Primitive::Decimal)
                                            | (Primitive::Int64, Primitive::Decimal)
                                    )
                            }) {
                                continue;
                            }
                            args.iter()
                                .zip(params)
                                .all(|(arg, param)| arg == param && *arg != Primitive::Unknown)
                        }
                        _ => continue,
                    };
                    if !member.signature.unknown() {
                        declared_here.insert(member.signature.clone());
                    }
                    let candidate = if super_ && member.abstract_ {
                        None
                    } else {
                        let kind = if call.signature == Signature::Property {
                            "Propiedad"
                        } else {
                            "Método"
                        };
                        let class_label: String = class.chars().take(64).collect();
                        let label = format!("{kind} · {} · {class_label}", call.name);
                        Some(
                            if let Some(class) =
                                loaded.as_ref().filter(|_| member.site.file.is_none())
                            {
                                class.item(call.range.clone(), label, member.site.offset)
                            } else {
                                member.site.item(label, call.range.clone())
                            },
                        )
                    };
                    if rank {
                        exact.push(candidate);
                    } else {
                        uncertain.push((candidate, member.signature.unknown()));
                    }
                }
                signatures.extend(declared_here);
                if !exact.is_empty() && !call.signature.unknown() {
                    // A farther declaration with the same exact signature is overridden.
                    break;
                }
            }
            let Some(parent) = hierarchy.parent else {
                break;
            };
            if depth == MAX_BASE_CLASSES {
                return Err(TaskFailure::LimitExceeded.into());
            }
            let (path, encoding) = loaded.as_ref().map_or((source, context.encoding), |class| {
                (class.path.as_path(), class.encoding)
            });
            let Some(base) = imports.load(&self.root, path, &parent, false, encoding, cancel)?
            else {
                break;
            };
            if !seen.insert(base.name.to_lowercase()) {
                return Ok(None);
            }
            (hierarchy, imports) = inspect(&base.path, &base.bytes, base.encoding)?;
            loaded = Some(base);
        }
        if exact.is_empty() {
            unique(uncertain.into_iter().map(|(item, _)| item).collect())
        } else if uncertain.iter().all(|(_, unknown)| !unknown) {
            unique(exact)
        } else {
            // Incomplete signature information may hide a better overload.
            Err(super::DefinitionFailure::Ambiguous)
        }
    }
}

impl Signature {
    fn unknown(&self) -> bool {
        matches!(self, Self::Method(types) if types.contains(&Primitive::Unknown))
    }
}

fn unique(
    mut candidates: Vec<Option<SourceItem>>,
) -> Result<Option<SourceItem>, super::DefinitionFailure> {
    if candidates.len() > 1 {
        return Err(super::DefinitionFailure::Ambiguous);
    }
    Ok(candidates.pop().flatten())
}

pub(super) fn declared_type(context: &Context<'_>, start: usize) -> Option<String> {
    let name = name_at(context.bytes, context.encoding, start)?;
    (!name.wildcard).then_some(name.text)
}

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
        let Some(class) = self.load(
            root,
            source,
            &use_.name.text,
            use_.in_using,
            encoding,
            cancel,
        )?
        else {
            return Ok(None);
        };
        let label: String = class.name.chars().take(88).collect();
        Ok(Some(class.item(
            use_.name.range,
            format!("{} · {label}", class.kind),
            class.offset,
        )))
    }

    fn load(
        &self,
        root: &Path,
        source: &Path,
        text: &str,
        in_using: bool,
        encoding: TextEncoding,
        cancel: &Cancellation,
    ) -> Result<Option<ClassSource>, TaskFailure> {
        let direct = in_using || text.contains('.');
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
            vec![text.to_owned()]
        } else if let Some(import) = explicit {
            vec![import.name.text.clone()]
        } else {
            wildcards
                .map(|import| format!("{}.{text}", import.name.text))
                .chain(std::iter::once(text.to_owned()))
                .collect()
        };
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
                return Ok(Some(ClassSource {
                    name,
                    kind,
                    offset,
                    path,
                    bytes,
                    encoding,
                }));
            }
        }
        Ok(None)
    }
}

struct ClassSource {
    name: String,
    kind: &'static str,
    offset: usize,
    path: PathBuf,
    bytes: Vec<u8>,
    encoding: TextEncoding,
}

impl ClassSource {
    fn item(&self, range: Range<usize>, label: String, offset: usize) -> SourceItem {
        SourceItem {
            range,
            label,
            action: SourceAction::ProjectLocation {
                path: self.path.clone(),
                offset,
                encoding: self.encoding,
                version: jc_files::ContentFingerprint::of(&self.bytes),
            },
        }
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

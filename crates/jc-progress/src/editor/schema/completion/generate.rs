//! Reviewed source generation for ABL classes. The admitted signatures use
//! scalar ABL types, so copying one never imports a different type by name.
use super::{classes::Imports, Context, Token};
use jc_app::{source_services::ReviewedEdit, Cancellation, TextReplacement};
use jc_languages::syntax::{scan, LexerState, SyntaxKind, SyntaxProfile};
use jc_text::TextEncoding;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt::Write,
    path::{Path, PathBuf},
};

const MAX_SOURCE: usize = 512 * 1024;
const MAX_TOKENS: usize = 16_384;
const MAX_TYPES: usize = 16;
const MAX_MEMBERS: usize = 128;
const MAX_GENERATED_BYTES: usize = 4096;
const UNSUPPORTED: &str = "La clase usa sintaxis fuera del alcance de la generación ABL";
const LIMIT: &str = "Se superó el límite de análisis para generar miembros ABL";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mode {
    Input,
    Output,
    InputOutput,
}

impl Mode {
    fn word(self) -> &'static str {
        match self {
            Self::Input => "INPUT",
            Self::Output => "OUTPUT",
            Self::InputOutput => "INPUT-OUTPUT",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Public,
    Protected,
    Private,
    Package,
}

impl Access {
    fn word(self) -> &'static str {
        match self {
            Self::Public => "PUBLIC",
            Self::Protected => "PROTECTED",
            Self::Private => "PRIVATE",
            Self::Package => "PACKAGE-PROTECTED",
        }
    }

    fn supported(self) -> bool {
        matches!(self, Self::Public | Self::Protected)
    }
}

#[derive(Clone)]
struct Argument {
    mode: Mode,
    name: String,
    datatype: &'static str,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Signature {
    name: String,
    kind: MemberKind,
    arguments: Vec<(Mode, &'static str)>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MemberKind {
    Routine,
    Property,
    Event,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Accessors {
    get: bool,
    set: bool,
    no_undo: bool,
}

#[derive(Clone)]
struct Member {
    name: String,
    kind: MemberKind,
    access: Access,
    abstract_: bool,
    final_: bool,
    static_: bool,
    arguments: Option<Vec<Argument>>,
    return_type: Option<&'static str>,
    accessors: Option<Accessors>,
}

impl Member {
    fn signature(&self) -> Option<Signature> {
        let arguments = self.arguments.as_ref()?;
        Some(Signature {
            name: self.name.to_ascii_uppercase(),
            kind: self.kind,
            arguments: if self.kind == MemberKind::Routine {
                arguments
                    .iter()
                    .map(|arg| (arg.mode, arg.datatype))
                    .collect()
            } else {
                Vec::new()
            },
        })
    }

    fn supported_signature(&self) -> bool {
        self.arguments.is_some()
            && self.return_type.is_some()
            && (self.kind != MemberKind::Property || self.accessors.is_some())
    }

    fn satisfies(&self, other: &Self) -> bool {
        self.supported_signature()
            && self.return_type == other.return_type
            && self.accessors == other.accessors
            && self
                .arguments
                .iter()
                .flatten()
                .map(|arg| (arg.mode, arg.datatype))
                .eq(other
                    .arguments
                    .iter()
                    .flatten()
                    .map(|arg| (arg.mode, arg.datatype)))
    }
}

struct Unit {
    interface: bool,
    name: String,
    imports: Imports,
    parent: Option<String>,
    interfaces: Vec<String>,
    constructors: Vec<Member>,
    methods: Vec<Member>,
    end: usize,
}

fn lex(bytes: &[u8], cancel: &Cancellation) -> Result<Vec<Token>, String> {
    if bytes.len() > MAX_SOURCE {
        return Err(LIMIT.into());
    }
    let skip = usize::from(bytes.starts_with(b"\xef\xbb\xbf")) * 3;
    let mut lexer = LexerState::default();
    let mut tokens = Vec::new();
    let mut unsupported = false;
    for (chunk, slice) in bytes[skip..].chunks(4096).enumerate() {
        live(cancel)?;
        let start = skip + chunk * 4096;
        scan(
            SyntaxProfile::Abl,
            &mut lexer,
            slice,
            start,
            start + slice.len() == bytes.len(),
            |span| {
                unsupported |= span.kind == SyntaxKind::Preprocessor;
                if unsupported
                    || span.kind == SyntaxKind::Comment
                    || bytes[span.source.clone()]
                        .iter()
                        .all(u8::is_ascii_whitespace)
                {
                    return;
                }
                if tokens.len() == MAX_TOKENS {
                    unsupported = true;
                    return;
                }
                tokens.push(Token {
                    kind: span.kind,
                    range: span.source,
                });
            },
        );
    }
    if unsupported || lexer.limited() {
        return Err(UNSUPPORTED.into());
    }
    Ok(tokens)
}

fn live(cancel: &Cancellation) -> Result<(), String> {
    if cancel.is_cancelled() {
        Err("Generación ABL cancelada".into())
    } else {
        Ok(())
    }
}

fn word<'a>(bytes: &'a [u8], token: &Token) -> &'a [u8] {
    &bytes[token.range.clone()]
}

fn text(bytes: &[u8], token: &Token) -> Option<String> {
    let value = word(bytes, token);
    let identifier = value.len() <= 128
        && value.first().is_some_and(u8::is_ascii_alphabetic)
        && value
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'));
    (identifier && token.kind.is_word())
        .then(|| String::from_utf8(value.to_vec()).expect("ASCII identifier"))
}

fn at(bytes: &[u8], tokens: &[Token], pos: usize, value: &[u8]) -> bool {
    tokens
        .get(pos)
        .is_some_and(|token| word(bytes, token).eq_ignore_ascii_case(value))
}

fn dot(bytes: &[u8], token: &Token) -> bool {
    word(bytes, token) == b"."
        && bytes.get(token.range.end..).is_some_and(|tail| {
            tail.is_empty()
                || tail.first().is_some_and(u8::is_ascii_whitespace)
                || tail.starts_with(b"/*")
                || tail.starts_with(b"//")
        })
}

fn qualified(bytes: &[u8], tokens: &[Token], pos: &mut usize) -> Option<String> {
    let mut name = text(bytes, tokens.get(*pos)?)?;
    *pos += 1;
    while at(bytes, tokens, *pos, b".") {
        let before = tokens.get(*pos - 1)?;
        let period = tokens.get(*pos)?;
        let after = tokens.get(*pos + 1)?;
        if before.range.end != period.range.start || period.range.end != after.range.start {
            return None;
        }
        name.push('.');
        name.push_str(&text(bytes, after)?);
        *pos += 2;
        if name.len() > 512 {
            return None;
        }
    }
    Some(name)
}

fn datatype(word: &[u8]) -> Option<&'static str> {
    [
        "CHARACTER",
        "INTEGER",
        "INT64",
        "DECIMAL",
        "LOGICAL",
        "DATE",
        "DATETIME",
        "DATETIME-TZ",
        "HANDLE",
        "RAW",
        "LONGCHAR",
        "MEMPTR",
        "RECID",
        "ROWID",
    ]
    .into_iter()
    .find(|kind| word.eq_ignore_ascii_case(kind.as_bytes()))
}

fn arguments(bytes: &[u8], tokens: &[Token]) -> Option<Vec<Argument>> {
    if tokens.is_empty() {
        return Some(Vec::new());
    }
    let mut names = BTreeSet::new();
    tokens
        .split(|token| word(bytes, token) == b",")
        .map(|tokens| {
            let (mode, rest) = [Mode::Input, Mode::Output, Mode::InputOutput]
                .into_iter()
                .find(|mode| at(bytes, tokens, 0, mode.word().as_bytes()))
                .map_or((Mode::Input, tokens), |mode| (mode, &tokens[1..]));
            if rest.len() != 3 || !at(bytes, rest, 1, b"AS") {
                return None;
            }
            let name = text(bytes, &rest[0])?;
            if !names.insert(name.to_ascii_uppercase()) {
                return None;
            }
            Some(Argument {
                name,
                mode,
                datatype: datatype(word(bytes, &rest[2]))?,
            })
        })
        .collect()
}

fn member(bytes: &[u8], header: &[Token], ctor: bool, interface: bool) -> Option<Member> {
    let open = header.iter().position(|token| word(bytes, token) == b"(")?;
    let close = header
        .iter()
        .rposition(|token| word(bytes, token) == b")")?;
    if open < 2 || close <= open || close + 1 != header.len() {
        return None;
    }
    let name_index = open - 1;
    let name = text(bytes, &header[name_index])?;
    let mut member = Member {
        name,
        kind: MemberKind::Routine,
        access: Access::Public,
        abstract_: interface,
        static_: false,
        final_: false,
        arguments: arguments(bytes, &header[open + 1..close]),
        return_type: None,
        accessors: None,
    };
    let mut i = 1;
    while i < name_index && member.modifier(word(bytes, &header[i])) {
        i += 1;
    }
    member.return_type = if ctor {
        (i == name_index).then_some("VOID")
    } else if i + 1 == name_index {
        let ty = word(bytes, &header[i]);
        if ty.eq_ignore_ascii_case(b"VOID") {
            Some("VOID")
        } else {
            datatype(ty)
        }
    } else {
        None
    };
    Some(member)
}

impl Member {
    fn modifier(&mut self, text: &[u8]) -> bool {
        match text.to_ascii_uppercase().as_slice() {
            b"PUBLIC" => self.access = Access::Public,
            b"PROTECTED" => self.access = Access::Protected,
            b"PRIVATE" => self.access = Access::Private,
            b"PACKAGE-PRIVATE" | b"PACKAGE-PROTECTED" => self.access = Access::Package,
            b"ABSTRACT" => self.abstract_ = true,
            b"STATIC" => self.static_ = true,
            b"FINAL" => self.final_ = true,
            b"OVERRIDE" => {}
            _ => return false,
        }
        true
    }
}

fn statement<'a>(bytes: &[u8], tokens: &'a [Token], pos: &mut usize) -> Option<(&'a [Token], u8)> {
    let start = *pos;
    while let Some(token) = tokens.get(*pos) {
        let word = word(bytes, token);
        if dot(bytes, token) || matches!(word, b":" | b";") {
            let end = *pos;
            *pos += 1;
            return Some((&tokens[start..end], word[0]));
        }
        *pos += 1;
    }
    None
}

fn skip_body(
    bytes: &[u8],
    tokens: &[Token],
    pos: &mut usize,
    keyword: &[u8],
) -> Result<(), String> {
    loop {
        let (body, end) = statement(bytes, tokens, pos).ok_or(UNSUPPORTED)?;
        if end == b'.'
            && body.len() == 2
            && at(bytes, body, 0, b"END")
            && at(bytes, body, 1, keyword)
        {
            return Ok(());
        }
        if at(bytes, body, 0, b"END")
            && [
                b"METHOD".as_slice(),
                b"CONSTRUCTOR",
                b"DESTRUCTOR",
                b"CLASS",
                b"GET",
                b"SET",
            ]
            .iter()
            .any(|kind| at(bytes, body, 1, kind))
        {
            return Err(UNSUPPORTED.into());
        }
    }
}

fn data_member(
    bytes: &[u8],
    tokens: &[Token],
    pos: &mut usize,
    header: &[Token],
    end: u8,
    interface: bool,
) -> Result<Option<Member>, String> {
    let mut member = Member {
        name: String::new(),
        kind: MemberKind::Property,
        access: Access::Public,
        abstract_: interface,
        static_: false,
        final_: false,
        arguments: None,
        return_type: None,
        accessors: None,
    };
    let mut index = 1;
    while header
        .get(index)
        .is_some_and(|token| member.modifier(word(bytes, token)))
    {
        index += 1;
    }
    if at(bytes, header, index, b"EVENT") {
        member.kind = MemberKind::Event;
    } else if !at(bytes, header, index, b"PROPERTY") {
        return Ok(None);
    }
    member.name = header
        .get(index + 1)
        .and_then(|token| text(bytes, token))
        .ok_or(UNSUPPORTED)?;
    let rest = &header[index + 2..];
    if member.kind == MemberKind::Event {
        let start = usize::from(at(bytes, rest, 0, b"SIGNATURE"));
        if end == b'.'
            && at(bytes, rest, start, b"VOID")
            && at(bytes, rest, start + 1, b"(")
            && rest.len() >= start + 3
            && at(bytes, rest, rest.len() - 1, b")")
        {
            member.arguments = arguments(bytes, &rest[start + 2..rest.len() - 1]);
            member.return_type = Some("VOID");
        }
        return Ok(Some(member));
    }
    member.arguments = Some(Vec::new());
    let mut accessors = Accessors {
        get: false,
        set: false,
        no_undo: false,
    };
    let mut recognized = at(bytes, rest, 0, b"AS");
    member.return_type = rest.get(1).and_then(|token| datatype(word(bytes, token)));
    let mut i = 2;
    if at(bytes, rest, i, b"NO-UNDO") {
        accessors.no_undo = true;
        i += 1;
    }
    let mut accessor = rest.get(i..).unwrap_or_default();
    let mut delimiter = end;
    loop {
        if at(bytes, accessor, 0, b"GET") {
            accessors.get = true;
        } else if at(bytes, accessor, 0, b"SET") {
            accessors.set = true;
        } else {
            recognized &= accessor.is_empty();
            break;
        }
        recognized &= accessor.len() == 1 && delimiter == b'.';
        if delimiter == b':' {
            skip_body(bytes, tokens, pos, word(bytes, &accessor[0]))?;
        }
        if !at(bytes, tokens, *pos, b"GET") && !at(bytes, tokens, *pos, b"SET") {
            break;
        }
        (accessor, delimiter) = statement(bytes, tokens, pos).ok_or(UNSUPPORTED)?;
    }
    if recognized && (accessors.get || accessors.set) {
        member.accessors = Some(accessors);
    }
    Ok(Some(member))
}

fn header_types(
    bytes: &[u8],
    header: &[Token],
    interface: bool,
) -> Option<(String, Option<String>, Vec<String>)> {
    let mut pos = 1;
    let name = qualified(bytes, header, &mut pos)?;
    let parent = if !interface && at(bytes, header, pos, b"INHERITS") {
        pos += 1;
        Some(qualified(bytes, header, &mut pos)?)
    } else {
        None
    };
    let mut interfaces = Vec::new();
    if at(
        bytes,
        header,
        pos,
        if interface {
            b"INHERITS"
        } else {
            b"IMPLEMENTS"
        },
    ) {
        loop {
            pos += 1;
            if interfaces.len() == MAX_TYPES {
                return None;
            }
            interfaces.push(qualified(bytes, header, &mut pos)?);
            if !at(bytes, header, pos, b",") {
                break;
            }
        }
    }
    header[pos..]
        .iter()
        .all(|token| {
            [
                b"USE-WIDGET-POOL".as_slice(),
                b"ABSTRACT",
                b"FINAL",
                b"SERIALIZABLE",
            ]
            .iter()
            .any(|flag| word(bytes, token).eq_ignore_ascii_case(flag))
        })
        .then_some((name, parent, interfaces))
}

fn parse(bytes: &[u8], encoding: TextEncoding, cancel: &Cancellation) -> Result<Unit, String> {
    let tokens = lex(bytes, cancel)?;
    let mut pos = 0;
    let mut imports = Imports::default();
    let mut aliases = BTreeSet::new();
    let (header, interface) = loop {
        let (header, end) = statement(bytes, &tokens, &mut pos).ok_or(UNSUPPORTED)?;
        if at(bytes, header, 0, b"USING") && end == b'.' {
            let mut name_end = 1;
            let name = qualified(bytes, header, &mut name_end).ok_or(UNSUPPORTED)?;
            if !name.contains('.')
                || !aliases.insert(
                    name.rsplit('.')
                        .next()
                        .unwrap_or(&name)
                        .to_ascii_uppercase(),
                )
                || name_end != header.len()
                    && !(header.len() == name_end + 2
                        && at(bytes, header, name_end, b"FROM")
                        && at(bytes, header, name_end + 1, b"PROPATH"))
            {
                return Err(UNSUPPORTED.into());
            }
            let mut context = Context::new(bytes, encoding);
            context.tokens.extend(header.iter().map(|token| Token {
                kind: token.kind,
                range: token.range.clone(),
            }));
            imports.add(&context).map_err(|_| LIMIT)?;
        } else if end == b':' && at(bytes, header, 0, b"CLASS") {
            break (header, false);
        } else if end == b':' && at(bytes, header, 0, b"INTERFACE") {
            break (header, true);
        } else {
            return Err(UNSUPPORTED.into());
        }
    };
    let (name, parent, interfaces) = header_types(bytes, header, interface).ok_or(UNSUPPORTED)?;
    let mut unit = Unit {
        interface,
        name,
        imports,
        parent,
        interfaces,
        constructors: Vec::new(),
        methods: Vec::new(),
        end: 0,
    };
    let terminator = if interface {
        b"INTERFACE".as_slice()
    } else {
        b"CLASS"
    };
    while let Some((header, end)) = statement(bytes, &tokens, &mut pos) {
        if at(bytes, header, 0, b"END") && at(bytes, header, 1, terminator) {
            if end != b'.' || header.len() != 2 || pos != tokens.len() {
                return Err(UNSUPPORTED.into());
            }
            unit.end = header[0].range.start;
            return Ok(unit);
        }
        let ctor = at(bytes, header, 0, b"CONSTRUCTOR");
        let method = at(bytes, header, 0, b"METHOD");
        let destructor = at(bytes, header, 0, b"DESTRUCTOR");
        if ctor || method {
            let candidate = member(bytes, header, ctor, interface).ok_or(UNSUPPORTED)?;
            if candidate.abstract_ != (end == b'.') {
                return Err(UNSUPPORTED.into());
            }
            let members = if ctor {
                &mut unit.constructors
            } else {
                &mut unit.methods
            };
            members.push(candidate);
        }
        if ctor || method || destructor {
            if end == b':' {
                skip_body(bytes, &tokens, &mut pos, word(bytes, &header[0]))?;
            }
        } else if at(bytes, header, 0, b"DEFINE") || at(bytes, header, 0, b"DEF") {
            if let Some(member) = data_member(bytes, &tokens, &mut pos, header, end, interface)? {
                unit.methods.push(member);
            }
        }
        if unit.methods.len() + unit.constructors.len() > MAX_MEMBERS {
            return Err(LIMIT.into());
        }
    }
    Err(UNSUPPORTED.into())
}

fn load(
    root: &Path,
    source: &Path,
    encoding: TextEncoding,
    unit: &Unit,
    name: &str,
    cancel: &Cancellation,
) -> Result<(Origin, Unit), String> {
    live(cancel)?;
    let file = unit
        .imports
        .load(root, source, name, false, encoding, cancel)
        .map_err(|_| format!("No se pudo resolver {name} por PROPATH"))?
        .ok_or_else(|| format!("No se encontró {name} por PROPATH"))?;
    let parsed = parse(&file.bytes, file.encoding, cancel)?;
    if !parsed.name.eq_ignore_ascii_case(&file.name) {
        return Err(UNSUPPORTED.into());
    }
    Ok((
        Origin {
            path: file.path,
            encoding: file.encoding,
        },
        parsed,
    ))
}

struct Origin {
    path: PathBuf,
    encoding: TextEncoding,
}

enum Snippet {
    Constructor(Member),
    Override(Member),
    Implement(Member),
}

fn constructors(
    root: &Path,
    source: &Path,
    encoding: TextEncoding,
    unit: &Unit,
    cancel: &Cancellation,
) -> Result<Vec<Snippet>, String> {
    let name = unit
        .parent
        .as_deref()
        .ok_or("La clase no declara una superclase")?;
    let (_, base) = load(root, source, encoding, unit, name, cancel)?;
    if base.interface || unit.name.eq_ignore_ascii_case(&base.name) {
        return Err(UNSUPPORTED.into());
    }
    let existing: Option<BTreeSet<_>> = unit
        .constructors
        .iter()
        .filter(|member| !member.static_)
        .map(|member| member.signature().map(|sig| sig.arguments))
        .collect();
    let mut existing = existing.ok_or("Hay un constructor local con firma sin resolver")?;
    let mut snippets = Vec::new();
    for member in base.constructors {
        if member.static_ || !member.access.supported() {
            continue;
        }
        let Some(signature) = member.signature().filter(|_| member.return_type.is_some()) else {
            continue;
        };
        if existing.insert(signature.arguments) {
            snippets.push(Snippet::Constructor(member));
        }
    }
    Ok(snippets)
}

struct Contract {
    member: Member,
    emit: Option<Snippet>,
    local: bool,
}

fn known(
    unit: &Unit,
    local: bool,
    existing: &mut BTreeMap<Signature, Contract>,
    uncertain: &mut BTreeSet<String>,
) -> Result<(), String> {
    for member in &unit.methods {
        match member.signature() {
            Some(signature) => {
                let emit = !local
                    && member.access.supported()
                    && !member.final_
                    && !member.static_
                    && member.supported_signature()
                    && (member.kind == MemberKind::Routine || member.abstract_)
                    && !uncertain.contains(&signature.name);
                existing.entry(signature).or_insert_with(|| Contract {
                    member: member.clone(),
                    emit: emit.then(|| Snippet::Override(member.clone())),
                    local,
                });
            }
            None => {
                // Unknown local or nearer declarations may already occupy a
                // signature that falls in the portion this generator knows.
                let name = member.name.to_ascii_uppercase();
                if !local && member.abstract_ {
                    return Err(format!(
                        "No se pudo comprobar la firma abstracta de {}",
                        member.name
                    ));
                }
                for (signature, contract) in &mut *existing {
                    if signature.name == name {
                        contract.emit = None;
                    }
                }
                uncertain.insert(name);
            }
        }
    }
    Ok(())
}

fn interface_members(
    unit: &Unit,
    existing: &mut BTreeMap<Signature, Contract>,
    uncertain: &BTreeSet<String>,
) -> Result<(), String> {
    for member in &unit.methods {
        let signature = member
            .signature()
            .ok_or("Una interfaz declara una firma sin resolver")?;
        if uncertain.contains(&signature.name) || !member.supported_signature() {
            return Err(format!("No se pudo comprobar la firma de {}", member.name));
        }
        let contract = existing.entry(signature).or_insert_with(|| Contract {
            member: member.clone(),
            emit: Some(Snippet::Implement(member.clone())),
            local: false,
        });
        if !contract.member.satisfies(member) || contract.member.static_ {
            return Err(format!(
                "La firma de {} no satisface el contrato de {}",
                member.name, unit.name
            ));
        }
        if contract.member.access != Access::Public {
            match &mut contract.emit {
                Some(Snippet::Override(stub)) => stub.access = Access::Public,
                _ => {
                    return Err(format!(
                        "{} exige acceso público para {}",
                        unit.name, member.name
                    ))
                }
            }
        }
    }
    Ok(())
}

fn override_members(
    root: &Path,
    source: &Path,
    encoding: TextEncoding,
    unit: &Unit,
    cancel: &Cancellation,
) -> Result<Vec<Snippet>, String> {
    let mut existing = BTreeMap::new();
    let mut uncertain = BTreeSet::new();
    known(unit, true, &mut existing, &mut uncertain)?;
    let mut files = Vec::new();
    let mut seen = BTreeSet::from([unit.name.to_ascii_uppercase()]);
    let mut current = unit;
    let mut current_path = source;
    let mut current_encoding = encoding;
    // Keep a separate traversal chain while reading each base. The collected
    // units also provide the import scopes of their interface references.
    while let Some(name) = current.parent.as_deref() {
        if files.len() == MAX_TYPES {
            return Err(LIMIT.into());
        }
        let (file, parsed) = load(root, current_path, current_encoding, current, name, cancel)?;
        if parsed.interface || !seen.insert(parsed.name.to_ascii_uppercase()) {
            return Err(UNSUPPORTED.into());
        }
        known(&parsed, false, &mut existing, &mut uncertain)?;
        files.push((file, parsed));
        let (file, parsed) = files.last().expect("just inserted base");
        current = parsed;
        current_path = &file.path;
        current_encoding = file.encoding;
    }
    let mut pending = VecDeque::new();
    for (path, encoding, owner) in std::iter::once((source, encoding, unit)).chain(
        files
            .iter()
            .map(|(file, owner)| (file.path.as_path(), file.encoding, owner)),
    ) {
        for name in &owner.interfaces {
            pending.push_back(load(root, path, encoding, owner, name, cancel)?);
            if pending.len() > MAX_TYPES {
                return Err(LIMIT.into());
            }
        }
    }
    let mut inspected = 0;
    while let Some((file, parsed)) = pending.pop_front() {
        inspected += 1;
        if inspected > MAX_TYPES {
            return Err(LIMIT.into());
        }
        if !parsed.interface {
            return Err(UNSUPPORTED.into());
        }
        if !seen.insert(parsed.name.to_ascii_uppercase()) {
            continue;
        }
        interface_members(&parsed, &mut existing, &uncertain)?;
        for name in &parsed.interfaces {
            pending.push_back(load(
                root,
                &file.path,
                file.encoding,
                &parsed,
                name,
                cancel,
            )?);
            if pending.len() + inspected > MAX_TYPES {
                return Err(LIMIT.into());
            }
        }
    }
    for contract in existing.values() {
        if !contract.local && contract.member.abstract_ && contract.emit.is_none() {
            return Err(format!(
                "No se pudo generar la implementación de {}",
                contract.member.name
            ));
        }
    }
    Ok(existing
        .into_values()
        .filter_map(|contract| contract.emit)
        .collect())
}

fn append(
    output: &mut String,
    snippet: Snippet,
    class: &str,
    indent: &str,
    eol: &str,
) -> Result<(), String> {
    let (member, constructor, override_) = match &snippet {
        Snippet::Constructor(member) => (member, true, false),
        Snippet::Override(member) => (member, false, true),
        Snippet::Implement(member) => (member, false, false),
    };
    let arguments = member.arguments.as_deref().ok_or(UNSUPPORTED)?;
    let return_type = member.return_type.ok_or(UNSUPPORTED)?;
    let declaration = arguments
        .iter()
        .map(|arg| format!("{} {} AS {}", arg.mode.word(), arg.name, arg.datatype))
        .collect::<Vec<_>>()
        .join(", ");
    let forwarded = arguments
        .iter()
        .map(|arg| format!("{} {}", arg.mode.word(), arg.name))
        .collect::<Vec<_>>()
        .join(", ");
    let access = member.access.word();
    let name = &member.name;
    let flag = if override_ { " OVERRIDE" } else { "" };
    if member.kind == MemberKind::Property {
        let accessor = member.accessors.ok_or(UNSUPPORTED)?;
        let undo = if accessor.no_undo { " NO-UNDO" } else { "" };
        let get = if accessor.get { " GET." } else { "" };
        let set = if accessor.set { " SET." } else { "" };
        write!(output, "{indent}DEFINE {access}{flag} PROPERTY {name} AS {return_type}{undo}{get}{set}{eol}{eol}").unwrap();
        return Ok(());
    }
    if member.kind == MemberKind::Event {
        write!(
            output,
            "{indent}DEFINE {access}{flag} EVENT {name} SIGNATURE VOID({declaration}).{eol}{eol}"
        )
        .unwrap();
        return Ok(());
    }
    let kind = if constructor { "CONSTRUCTOR" } else { "METHOD" };
    if constructor {
        write!(
            output,
            "{indent}{kind} {access} {class}({declaration}):{eol}"
        )
        .unwrap();
        write!(output, "{indent}    SUPER({forwarded}).{eol}").unwrap();
    } else {
        write!(
            output,
            "{indent}{kind} {access}{flag} {return_type} {name}({declaration}):{eol}"
        )
        .unwrap();
        if member.abstract_ {
            write!(output, "{indent}    UNDO, THROW NEW Progress.Lang.AppError(\"Pendiente de implementar\", 0).{eol}").unwrap();
        } else {
            let return_ = if return_type == "VOID" { "" } else { "RETURN " };
            write!(
                output,
                "{indent}    {return_}SUPER:{name}({forwarded}).{eol}"
            )
            .unwrap();
        }
    }
    write!(output, "{indent}END {kind}.{eol}{eol}").unwrap();
    Ok(())
}

pub(super) fn propose(
    root: &Path,
    source: &Path,
    bytes: &[u8],
    encoding: TextEncoding,
    _cursor: usize,
    action: ReviewedEdit,
    cancel: &Cancellation,
) -> Result<Vec<TextReplacement>, String> {
    if !source
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cls"))
    {
        return Err("Abre una clase ABL del proyecto".into());
    }
    let unit = parse(bytes, encoding, cancel)?;
    if unit.interface {
        return Err("La generación requiere una clase ABL".into());
    }
    let class = unit.name.rsplit('.').next().unwrap_or(&unit.name);
    if !source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.eq_ignore_ascii_case(class))
    {
        return Err("El nombre de la clase debe coincidir con el archivo".into());
    }
    let snippets = match action {
        ReviewedEdit::GenerateConstructors => constructors(root, source, encoding, &unit, cancel)?,
        ReviewedEdit::GenerateOverrides => override_members(root, source, encoding, &unit, cancel)?,
        _ => return Err(UNSUPPORTED.into()),
    };
    if snippets.is_empty() {
        return Err("No hay miembros compatibles pendientes de generar".into());
    }
    let newline = bytes[..unit.end]
        .iter()
        .rposition(|byte| matches!(byte, b'\r' | b'\n'))
        .ok_or("END CLASS debe estar en su propia línea")?;
    let start = newline + 1;
    let indent = &bytes[start..unit.end];
    if indent.len() > 32 || !indent.iter().all(|byte| matches!(byte, b' ' | b'\t')) {
        return Err("END CLASS debe estar en su propia línea".into());
    }
    let eol = if bytes[..start].ends_with(b"\r\n") {
        "\r\n"
    } else if bytes[newline] == b'\r' {
        "\r"
    } else {
        "\n"
    };
    let indent = format!(
        "{}    ",
        std::str::from_utf8(indent).expect("ASCII indentation")
    );
    let mut text = String::new();
    for snippet in snippets {
        live(cancel)?;
        append(&mut text, snippet, class, &indent, eol)?;
        if text.len() > MAX_GENERATED_BYTES || text.matches(eol).count() > 48 {
            return Err(LIMIT.into());
        }
    }
    Ok(vec![TextReplacement {
        range: start..start,
        text,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{completion, submit, JobDir};
    use crate::Action;
    use jc_app::{JobLimits, JobSystem, Wakeup, Workbench};
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };

    const BASE: &str = r#"CLASS paquete.Base ABSTRACT:
    CONSTRUCTOR PUBLIC Base(INPUT n AS INTEGER):
    END CONSTRUCTOR.
    CONSTRUCTOR PRIVATE Base(INPUT n AS DECIMAL):
    END CONSTRUCTOR.
    METHOD PUBLIC CHARACTER Texto(INPUT n AS INTEGER):
        RETURN STRING(n).
    END METHOD.
    METHOD PUBLIC CHARACTER Texto(INPUT c AS CHARACTER):
        RETURN c.
    END METHOD.
    METHOD PROTECTED VOID Ajustar(OUTPUT n AS INTEGER):
        n = 1.
    END METHOD.
    METHOD PUBLIC ABSTRACT INTEGER Calcular(INPUT-OUTPUT n AS INTEGER).
    METHOD PUBLIC FINAL VOID Inmutable():
    END METHOD.
    METHOD PUBLIC STATIC VOID Compartido():
    END METHOD.
    DEFINE PUBLIC ABSTRACT PROPERTY Estado AS CHARACTER NO-UNDO GET. SET.
    DEFINE PUBLIC ABSTRACT EVENT Notificado SIGNATURE VOID(INPUT n AS INTEGER).
END CLASS.
"#;
    const CONTRACT: &str = r#"INTERFACE paquete.Contrato:
    METHOD PUBLIC VOID Usar(INPUT n AS INTEGER).
    DEFINE PUBLIC PROPERTY Valor AS INTEGER NO-UNDO GET. SET.
    DEFINE PUBLIC EVENT Aviso SIGNATURE VOID(INPUT n AS INTEGER).
END INTERFACE.
"#;
    const CHILD: &str = r#"USING paquete.Base FROM PROPATH.
USING paquete.Contrato FROM PROPATH.
CLASS paquete.Hijo INHERITS Base IMPLEMENTS Contrato:
    METHOD PUBLIC OVERRIDE CHARACTER Texto(INPUT n AS INTEGER):
        RETURN SUPER:Texto(INPUT n).
    END METHOD.
END CLASS.
"#;

    fn fixture() -> JobDir {
        let fixture = JobDir::create().unwrap();
        let first = fixture.path.join("primero/paquete");
        let second = fixture.path.join("segundo/paquete");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        fs::create_dir(fixture.path.join(".cide")).unwrap();
        fs::write(fixture.path.join(".cide/propath"), "primero\nsegundo\n").unwrap();
        fs::write(first.join("Base.cls"), BASE).unwrap();
        fs::write(second.join("Base.cls"), "CLASS paquete.Base:\nEND CLASS.\n").unwrap();
        fs::write(first.join("Contrato.cls"), CONTRACT).unwrap();
        fixture
    }

    fn add_members(root: &Path, child: &Path, input: &str, action: ReviewedEdit) -> String {
        let edits = propose(
            root,
            child,
            input.as_bytes(),
            TextEncoding::Utf8,
            0,
            action,
            &Cancellation::new(Duration::from_secs(3)),
        )
        .unwrap();
        let [edit] = edits.as_slice() else {
            panic!("Se esperaba una inserción")
        };
        assert!(edit.range.is_empty());
        let mut result = input.to_owned();
        result.insert_str(edit.range.start, &edit.text);
        result
    }

    #[test]
    fn oo_generation_preserves_propath_visibility_overloads_and_crlf() {
        let fixture = fixture();
        let child = fixture.path.join("primero/paquete/Hijo.cls");
        let constructor = add_members(
            &fixture.path,
            &child,
            &CHILD.replace('\n', "\r\n"),
            ReviewedEdit::GenerateConstructors,
        );
        assert!(constructor
            .contains("CONSTRUCTOR PUBLIC Hijo(INPUT n AS INTEGER):\r\n        SUPER(INPUT n)."));
        assert!(!constructor.contains("DECIMAL"));
        let code = add_members(
            &fixture.path,
            &child,
            &constructor,
            ReviewedEdit::GenerateOverrides,
        );
        assert!(!code.replace("\r\n", "").contains('\n'));
        assert_eq!(code.matches("Texto(INPUT n AS INTEGER):").count(), 1);
        assert!(
            code.contains("Texto(INPUT c AS CHARACTER):\r\n        RETURN SUPER:Texto(INPUT c).")
        );
        assert!(code.contains("Ajustar(OUTPUT n AS INTEGER):\r\n        SUPER:Ajustar(OUTPUT n)."));
        assert!(code.contains("OVERRIDE INTEGER Calcular(INPUT-OUTPUT n AS INTEGER):"));
        assert!(code.contains("OVERRIDE PROPERTY Estado AS CHARACTER NO-UNDO GET. SET."));
        assert!(code.contains("OVERRIDE EVENT Notificado SIGNATURE VOID(INPUT n AS INTEGER)."));
        assert!(code.contains("PROPERTY Valor AS INTEGER NO-UNDO GET. SET."));
        assert!(code.contains("EVENT Aviso SIGNATURE VOID(INPUT n AS INTEGER)."));
        assert!(!code.contains("Inmutable"));
        assert!(!code.contains("Compartido"));
        assert!(propose(
            &fixture.path,
            &child,
            code.as_bytes(),
            TextEncoding::Utf8,
            0,
            ReviewedEdit::GenerateOverrides,
            &Cancellation::new(Duration::from_secs(3)),
        )
        .unwrap_err()
        .contains("No hay miembros"));
    }

    #[test]
    fn oo_generation_rejects_macros_and_incompatible_interface_contracts() {
        let fixture = fixture();
        let child = fixture.path.join("primero/paquete/Hijo.cls");
        for (input, message) in [
            (
                format!("&SCOPED-DEFINE ABI INTEGER\n{CHILD}"),
                "fuera del alcance",
            ),
            (
                CHILD.replace(
                    "END CLASS.",
                    "    DEFINE PUBLIC PROPERTY Valor AS CHARACTER NO-UNDO GET. SET.\nEND CLASS.",
                ),
                "no satisface el contrato",
            ),
        ] {
            assert!(propose(
                &fixture.path,
                &child,
                input.as_bytes(),
                TextEncoding::Utf8,
                0,
                ReviewedEdit::GenerateOverrides,
                &Cancellation::new(Duration::from_secs(3)),
            )
            .unwrap_err()
            .contains(message));
        }
    }

    #[test]
    #[ignore = "requiere CIDE_TEST_PROGRESS_EXE con un AVM OO por lotes"]
    fn progress_real_oo_generation_compiles() {
        let fixture = fixture();
        let root = &fixture.path;
        let child = root.join("primero/paquete/Hijo.cls");
        let code = add_members(root, &child, CHILD, ReviewedEdit::GenerateConstructors);
        let code = add_members(root, &child, &code, ReviewedEdit::GenerateOverrides);
        fs::write(&child, code).unwrap();
        let executable = std::env::var("CIDE_TEST_PROGRESS_EXE").unwrap();
        fs::write(
            root.join(".cide/progress.local.ini"),
            format!("executable={executable}\ncpinternal=iso8859-1\ncpstream=ibm850\n"),
        )
        .unwrap();
        let mut app = Workbench::default();
        app.open_file(&child, Some(TextEncoding::Utf8)).unwrap();
        let mut jobs = JobSystem::new(JobLimits::default(), Wakeup::default()).unwrap();
        submit(Action::Check, &app, &mut jobs, Some(root)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let result = loop {
            if let Some(result) = jobs.poll(&app, 1).pop() {
                break result;
            }
            assert!(Instant::now() < deadline, "El AVM no terminó a tiempo");
            thread::sleep(Duration::from_millis(15));
        };
        let (message, failed) = completion(&result);
        assert!(!failed, "{message}: {:?}", result.output.items());
        assert!(result.output.items().iter().any(|item| {
            matches!(item, jc_app::TaskItem::Message(text) if text.contains("Sintaxis ABL comprobada"))
        }));
        assert_eq!(
            fs::read_to_string(root.join("primero/paquete/Base.cls")).unwrap(),
            BASE
        );
        assert_eq!(
            fs::read_to_string(root.join("primero/paquete/Contrato.cls")).unwrap(),
            CONTRACT
        );
    }
}

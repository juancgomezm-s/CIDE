use super::SyntaxProfile;

impl SyntaxProfile {
    pub(crate) fn modern(self) -> bool {
        self.ecmascript() || matches!(self, Self::CSharp | Self::Java | Self::Php)
    }

    pub(crate) fn ecmascript(self) -> bool {
        matches!(self, Self::JavaScript | Self::TypeScript)
    }

    pub(crate) fn dollar_identifier(self) -> bool {
        self.ecmascript() || matches!(self, Self::Php | Self::Java)
    }
}

pub(super) fn keyword(profile: SyntaxProfile, word: &[u8]) -> bool {
    use SyntaxProfile::*;
    let mut lowercase = [0; 64];
    let word = if profile == Php {
        for (dest, source) in lowercase.iter_mut().zip(word) {
            *dest = source.to_ascii_lowercase();
        }
        &lowercase[..word.len()]
    } else {
        word
    };
    let common = matches!(
        word,
        b"break"
            | b"case"
            | b"catch"
            | b"class"
            | b"const"
            | b"continue"
            | b"default"
            | b"do"
            | b"else"
            | b"false"
            | b"finally"
            | b"for"
            | b"if"
            | b"new"
            | b"null"
            | b"return"
            | b"switch"
            | b"throw"
            | b"true"
            | b"try"
            | b"while"
    );
    common
        || match profile {
            JavaScript | TypeScript => {
                matches!(
                    word,
                    b"as"
                        | b"async"
                        | b"await"
                        | b"debugger"
                        | b"delete"
                        | b"export"
                        | b"extends"
                        | b"from"
                        | b"function"
                        | b"get"
                        | b"import"
                        | b"in"
                        | b"instanceof"
                        | b"let"
                        | b"of"
                        | b"set"
                        | b"static"
                        | b"super"
                        | b"this"
                        | b"typeof"
                        | b"var"
                        | b"void"
                        | b"with"
                        | b"yield"
                ) || profile == TypeScript
                    && matches!(
                        word,
                        b"abstract"
                            | b"any"
                            | b"asserts"
                            | b"boolean"
                            | b"declare"
                            | b"enum"
                            | b"implements"
                            | b"infer"
                            | b"interface"
                            | b"is"
                            | b"keyof"
                            | b"module"
                            | b"namespace"
                            | b"never"
                            | b"number"
                            | b"override"
                            | b"private"
                            | b"protected"
                            | b"public"
                            | b"readonly"
                            | b"satisfies"
                            | b"string"
                            | b"symbol"
                            | b"type"
                            | b"undefined"
                            | b"unique"
                            | b"unknown"
                    )
            }
            CSharp => matches!(
                word,
                b"abstract"
                    | b"as"
                    | b"async"
                    | b"await"
                    | b"base"
                    | b"bool"
                    | b"byte"
                    | b"char"
                    | b"checked"
                    | b"decimal"
                    | b"delegate"
                    | b"double"
                    | b"dynamic"
                    | b"enum"
                    | b"event"
                    | b"explicit"
                    | b"extern"
                    | b"fixed"
                    | b"float"
                    | b"foreach"
                    | b"get"
                    | b"global"
                    | b"goto"
                    | b"implicit"
                    | b"in"
                    | b"init"
                    | b"int"
                    | b"interface"
                    | b"internal"
                    | b"is"
                    | b"lock"
                    | b"long"
                    | b"nameof"
                    | b"namespace"
                    | b"object"
                    | b"operator"
                    | b"out"
                    | b"override"
                    | b"params"
                    | b"partial"
                    | b"private"
                    | b"protected"
                    | b"public"
                    | b"readonly"
                    | b"record"
                    | b"ref"
                    | b"required"
                    | b"sbyte"
                    | b"sealed"
                    | b"set"
                    | b"short"
                    | b"sizeof"
                    | b"stackalloc"
                    | b"static"
                    | b"string"
                    | b"struct"
                    | b"this"
                    | b"typeof"
                    | b"uint"
                    | b"ulong"
                    | b"unchecked"
                    | b"unsafe"
                    | b"ushort"
                    | b"using"
                    | b"var"
                    | b"virtual"
                    | b"void"
                    | b"volatile"
                    | b"when"
                    | b"where"
                    | b"yield"
            ),
            Java => matches!(
                word,
                b"abstract"
                    | b"assert"
                    | b"boolean"
                    | b"byte"
                    | b"char"
                    | b"double"
                    | b"enum"
                    | b"exports"
                    | b"extends"
                    | b"final"
                    | b"float"
                    | b"implements"
                    | b"import"
                    | b"instanceof"
                    | b"int"
                    | b"interface"
                    | b"long"
                    | b"module"
                    | b"native"
                    | b"open"
                    | b"opens"
                    | b"package"
                    | b"permits"
                    | b"private"
                    | b"protected"
                    | b"provides"
                    | b"public"
                    | b"record"
                    | b"requires"
                    | b"sealed"
                    | b"short"
                    | b"static"
                    | b"super"
                    | b"synchronized"
                    | b"this"
                    | b"throws"
                    | b"to"
                    | b"transient"
                    | b"uses"
                    | b"var"
                    | b"void"
                    | b"volatile"
                    | b"with"
                    | b"yield"
            ),
            Php => matches!(
                word,
                b"abstract"
                    | b"and"
                    | b"array"
                    | b"as"
                    | b"callable"
                    | b"clone"
                    | b"declare"
                    | b"echo"
                    | b"empty"
                    | b"enddeclare"
                    | b"endfor"
                    | b"endforeach"
                    | b"endif"
                    | b"endswitch"
                    | b"endwhile"
                    | b"enum"
                    | b"eval"
                    | b"exit"
                    | b"extends"
                    | b"final"
                    | b"fn"
                    | b"foreach"
                    | b"function"
                    | b"global"
                    | b"goto"
                    | b"implements"
                    | b"include"
                    | b"include_once"
                    | b"instanceof"
                    | b"insteadof"
                    | b"interface"
                    | b"isset"
                    | b"list"
                    | b"match"
                    | b"namespace"
                    | b"or"
                    | b"print"
                    | b"private"
                    | b"protected"
                    | b"public"
                    | b"readonly"
                    | b"require"
                    | b"require_once"
                    | b"self"
                    | b"static"
                    | b"trait"
                    | b"unset"
                    | b"use"
                    | b"var"
                    | b"xor"
                    | b"yield"
            ),
            _ => false,
        }
}

pub(super) fn number(profile: SyntaxProfile, word: &[u8]) -> bool {
    let mut clean = [0; 64];
    let mut len = 0;
    for &byte in word {
        if byte != b'_' {
            clean[len] = byte;
            len += 1;
        }
    }
    let mut word = &clean[..len];
    let based = match word.get(..2) {
        Some(b"0x" | b"0X") => Some(16),
        Some(b"0b" | b"0B") => Some(2),
        Some(b"0o" | b"0O") => Some(8),
        _ => None,
    };
    if profile.ecmascript() && word.last() == Some(&b'n')
        || profile == SyntaxProfile::Java && word.last().is_some_and(|b| matches!(b, b'l' | b'L'))
    {
        word = &word[..word.len() - 1];
    } else if profile == SyntaxProfile::CSharp
        && word
            .last()
            .is_some_and(|b| matches!(b, b'u' | b'U' | b'l' | b'L'))
    {
        word = &word[..word.len() - 1];
        if word
            .last()
            .is_some_and(|b| matches!(b, b'u' | b'U' | b'l' | b'L'))
        {
            word = &word[..word.len() - 1];
        }
    }
    if let Some(base) = based {
        return word.len() > 2 && word[2..].iter().all(|&b| (b as char).is_digit(base));
    }
    if matches!(profile, SyntaxProfile::Java | SyntaxProfile::CSharp)
        && word
            .last()
            .is_some_and(|b| matches!(b, b'f' | b'F' | b'd' | b'D' | b'm' | b'M'))
    {
        word = &word[..word.len() - 1];
    }
    super::number(word, false)
}

pub(super) fn regex_prefix(word: &[u8]) -> bool {
    matches!(
        word,
        b"return"
            | b"throw"
            | b"case"
            | b"yield"
            | b"await"
            | b"delete"
            | b"void"
            | b"typeof"
            | b"in"
            | b"of"
            | b"instanceof"
            | b"else"
            | b"do"
    )
}

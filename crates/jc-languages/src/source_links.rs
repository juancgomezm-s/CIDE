//! Static source-file operands shared by hyperlink painting and navigation.
//! No filesystem access, package downloads, or system-library lookup.
use crate::syntax::{SyntaxKind, SyntaxProfile, TokenSpan};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLink {
    pub range: Range<usize>,
    pub path: String,
}

pub fn file_links(
    profile: SyntaxProfile,
    bytes: &[u8],
    spans: &[TokenSpan],
    origin: usize,
) -> Vec<FileLink> {
    // The renderer keeps colored spans only; the worker also has Plain spans.
    // Recover the same code-word boundaries in both, never inside protected text.
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < bytes.len() && tokens.len() < 1536 {
        if bytes[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let covering = spans.iter().find(|s| s.source.contains(&(origin + at)));
        if let Some(span) = covering.filter(|s| {
            matches!(
                s.kind,
                SyntaxKind::String | SyntaxKind::Comment | SyntaxKind::Preprocessor
            )
        }) {
            tokens.push(span.clone());
            at = span.source.end - origin;
            continue;
        }
        let start = at;
        at += 1;
        if bytes[start].is_ascii_alphabetic() || bytes[start] == b'_' {
            while bytes
                .get(at)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                at += 1;
            }
        }
        tokens.push(TokenSpan {
            source: origin + start..origin + at,
            kind: covering.map_or(SyntaxKind::Plain, |s| s.kind),
        });
    }
    let mut links = Vec::new();
    for span in &tokens {
        if links.len() == 32 {
            break;
        }
        let start = span.source.start.saturating_sub(origin);
        let end = span.source.end.saturating_sub(origin);
        let Some(raw) = bytes.get(start..end) else {
            continue;
        };
        let word = |value: &[u8]| span.kind.is_word() && raw.eq_ignore_ascii_case(value);
        let mut at = end;
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if profile == SyntaxProfile::Abl
            && span.kind == SyntaxKind::Preprocessor
            && raw.starts_with(b"{")
        {
            at = start + 1;
            while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            let quoted = bytes.get(at).copied().filter(|b| matches!(b, b'\'' | b'"'));
            at += usize::from(quoted.is_some());
            let first = at;
            while bytes.get(at).is_some_and(|b| {
                (quoted.is_some() || !b.is_ascii_whitespace()) && *b != b'}' && Some(*b) != quoted
            }) {
                at += 1;
            }
            if let Some(path) = local_path(&bytes[first..at], false) {
                if path.to_ascii_lowercase().ends_with(".i")
                    && (quoted.is_none() || bytes.get(at) == quoted.as_ref())
                {
                    links.push(FileLink {
                        range: origin + first..origin + at,
                        path,
                    });
                }
            }
            continue;
        }
        if profile == SyntaxProfile::Rust && word(b"mod") {
            let first = at;
            while bytes
                .get(at)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                at += 1;
            }
            let mut after = at;
            while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
                after += 1;
            }
            if at > first && bytes.get(after) == Some(&b';') {
                if let Some(name) = local_path(&bytes[first..at], false) {
                    links.push(FileLink {
                        range: origin + first..origin + at,
                        path: format!("{name}.rs"),
                    });
                }
            }
            continue;
        }
        let java = profile == SyntaxProfile::Java && word(b"import");
        let csharp = profile == SyntaxProfile::CSharp && word(b"using");
        if java || csharp {
            let static_ = bytes.get(at..at + 6) == Some(b"static")
                && bytes.get(at + 6).is_some_and(u8::is_ascii_whitespace);
            if static_ {
                at += 6;
                while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                    at += 1;
                }
            }
            let mut first = at;
            while bytes
                .get(at)
                .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
            {
                at += 1;
            }
            let mut after = at;
            while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
                after += 1;
            }
            let alias = csharp && bytes.get(after) == Some(&b'=');
            if alias {
                at = after + 1;
                while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                    at += 1;
                }
                first = at;
                while bytes
                    .get(at)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
                {
                    at += 1;
                }
                after = at;
                while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
                    after += 1;
                }
            }
            if bytes.get(after) != Some(&b';') || csharp && !(static_ || alias) {
                continue;
            }
            let Ok(name) = std::str::from_utf8(&bytes[first..at]) else {
                continue;
            };
            let system = if java {
                ["java.", "javax.", "jdk.", "sun.", "com.sun."]
                    .iter()
                    .any(|p| name.starts_with(p))
            } else {
                ["System.", "Microsoft."]
                    .iter()
                    .any(|p| name.starts_with(p))
            };
            if system || name.is_empty() || name.ends_with('.') {
                continue;
            }
            let target = if java && static_ {
                name.rsplit_once('.').map(|(class, _)| class)
            } else {
                Some(name)
            };
            if let Some(target) = target {
                links.push(FileLink {
                    range: origin + first..origin + at,
                    path: format!(
                        "{}.{}",
                        target.replace('.', "/"),
                        if java { "java" } else { "cs" }
                    ),
                });
            }
            continue;
        }
        let php = profile == SyntaxProfile::Php
            && [
                b"include".as_slice(),
                b"include_once",
                b"require",
                b"require_once",
            ]
            .iter()
            .any(|w| word(w));
        let js = matches!(
            profile,
            SyntaxProfile::JavaScript | SyntaxProfile::TypeScript
        ) && [b"from".as_slice(), b"import", b"require"]
            .iter()
            .any(|w| word(w));
        let rust =
            profile == SyntaxProfile::Rust && word(b"include") && bytes.get(at) == Some(&b'!');
        if !(php || js || rust) {
            continue;
        }
        if rust {
            at += 1;
        }
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if bytes.get(at) == Some(&b'(') {
            at += 1;
        }
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let mut anchored = false;
        if php
            && bytes
                .get(at..at + 7)
                .is_some_and(|b| b.eq_ignore_ascii_case(b"__DIR__"))
        {
            at += 7;
            while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            if bytes.get(at) != Some(&b'.') {
                continue;
            }
            at += 1;
            while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            // __DIR__ concatenation is relative to this document.
            if matches!(bytes.get(at), Some(b'\'' | b'"')) && bytes.get(at + 1) == Some(&b'/') {
                anchored = true;
            }
        }
        let Some(&quote @ (b'\'' | b'"')) = bytes.get(at) else {
            continue;
        };
        at += 1;
        if anchored {
            at += 1;
        }
        let first = at;
        while bytes
            .get(at)
            .is_some_and(|b| *b != quote && !matches!(b, b'\r' | b'\n'))
        {
            at += 1;
        }
        if bytes.get(at) != Some(&quote) || at - first > 512 {
            continue;
        }
        let mut after = at + 1;
        while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
            after += 1;
        }
        if (php || js || rust) && bytes.get(after).is_some_and(|b| !matches!(b, b';' | b')')) {
            continue;
        }
        if let Some(path) = local_path(&bytes[first..at], js) {
            if (!php
                || [".php", ".inc", ".phtml"]
                    .iter()
                    .any(|ext| path.ends_with(ext)))
                && (!rust || path.ends_with(".rs"))
            {
                links.push(FileLink {
                    range: origin + first..origin + at,
                    path,
                });
            }
        }
    }
    links
}

fn local_path(bytes: &[u8], require_relative: bool) -> Option<String> {
    let path = std::str::from_utf8(bytes).ok()?;
    if path.is_empty()
        || path.len() > 512
        || path.starts_with(['/', '\\'])
        || path.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    ':' | '\\'
                        | '$'
                        | '&'
                        | '{'
                        | '}'
                        | '~'
                        | '?'
                        | '*'
                        | '#'
                        | '%'
                        | '\''
                        | '"'
                        | '<'
                        | '>'
                        | '|'
                )
        })
        || require_relative && !path.starts_with("./") && !path.starts_with("../")
    {
        return None;
    }
    Some(path.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{scan, LexerState};
    fn links(profile: SyntaxProfile, source: &str) -> Vec<FileLink> {
        let mut spans = Vec::new();
        scan(
            profile,
            &mut LexerState::default(),
            source.as_bytes(),
            0,
            true,
            |s| spans.push(s),
        );
        file_links(profile, source.as_bytes(), &spans, 0)
    }
    #[test]
    fn local_includes_modules_and_imports_exclude_system_packages_and_comments() {
        for (profile, source, target) in [
            (
                SyntaxProfile::Abl,
                "{includes/uno.i &valor='x'}",
                "includes/uno.i",
            ),
            (
                SyntaxProfile::JavaScript,
                "import {uno} from './uno.js';",
                "./uno.js",
            ),
            (
                SyntaxProfile::TypeScript,
                "const x = require('../uno');",
                "../uno",
            ),
            (
                SyntaxProfile::Php,
                "<?php require_once('uno.php');",
                "uno.php",
            ),
            (SyntaxProfile::Rust, "include!(\"uno.rs\");", "uno.rs"),
            (SyntaxProfile::Rust, "pub mod uno;", "uno.rs"),
            (
                SyntaxProfile::Java,
                "import paquete.Uno;",
                "paquete/Uno.java",
            ),
            (
                SyntaxProfile::CSharp,
                "using Uno = Paquete.Uno;",
                "Paquete/Uno.cs",
            ),
            (
                SyntaxProfile::Php,
                "<?php require __DIR__ . '/uno.php';",
                "uno.php",
            ),
        ] {
            let result = links(profile, source);
            assert_eq!(result.len(), 1, "{source}: {result:?}");
            assert_eq!(result[0].path, target);
            assert!(!result[0].range.is_empty());
            let comment = if profile == SyntaxProfile::Php {
                format!("<?php /* {} */", source.trim_start_matches("<?php "))
            } else {
                format!("/* {source} */")
            };
            assert!(links(profile, &comment).is_empty(), "{comment}");
        }
        for (profile, source) in [
            (SyntaxProfile::Abl, "{&DINAMICO}"),
            (
                SyntaxProfile::JavaScript,
                "import fs from 'node:fs'; import x from 'react';",
            ),
            (SyntaxProfile::Rust, "use std::io; use core::fmt;"),
            (SyntaxProfile::Php, "<?php require 'uno.php' . $nombre;"),
            (SyntaxProfile::JavaScript, "require('./uno' + nombre);"),
            (SyntaxProfile::Php, "require 'https://ejemplo.com/x.php';"),
            (SyntaxProfile::CSharp, "using System.IO;"),
            (SyntaxProfile::Java, "import java.util.List;"),
        ] {
            assert!(links(profile, source).is_empty(), "{source}");
        }
    }
}

//! Streaming lexical Markdown subset. Fixed-size continuation, source-byte spans.
//! No HTML execution, source decoding, full document copy or CommonMark claim.
use super::{SyntaxKind, TokenSpan};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct MarkdownState {
    start: usize,
    indent: u8,
    prefix: u8,
    run: u8,
    body: bool,
    heading: bool,
    quote: bool,
    fence: u8,
    fence_len: u8,
    line_fence: u8,
    line_run: u8,
    closing: bool,
    tail_space: bool,
    inline: u8,
    escape: bool,
    previous: u8,
    pub(super) limited: bool,
}
pub(super) fn scan(
    s: &mut MarkdownState,
    bytes: &[u8],
    source_start: usize,
    _final_chunk: bool,
    mut emit: impl FnMut(TokenSpan),
) {
    for (n, &b) in bytes.iter().enumerate() {
        let at = source_start + n;
        if matches!(b, b'\r' | b'\n') {
            if s.line_fence != 0 && s.line_run >= 3 {
                if s.closing && s.line_fence == s.fence && s.tail_space && s.line_run >= s.fence_len
                {
                    s.fence = 0;
                    s.fence_len = 0;
                } else if !s.closing {
                    s.fence = s.line_fence;
                    s.fence_len = s.line_run;
                }
            }
            let fence = s.fence;
            let len = s.fence_len;
            let limited = s.limited;
            *s = MarkdownState {
                start: at + 1,
                fence,
                fence_len: len,
                limited,
                ..Default::default()
            };
            continue;
        }
        let mut kind = if s.fence != 0 {
            SyntaxKind::Code
        } else {
            SyntaxKind::Plain
        };
        if !s.body {
            if b == b' ' && s.indent < 3 && s.prefix == 0 {
                s.indent += 1;
                continue;
            }
            if s.prefix == 0 {
                s.prefix = b;
                s.run = 1;
                if matches!(b, b'`' | b'~') {
                    s.line_fence = b;
                    s.line_run = 1;
                    s.closing = s.fence != 0;
                    s.tail_space = true;
                }
            } else if b == s.prefix && matches!(b, b'#' | b'`' | b'~') {
                if s.run == u8::MAX {
                    s.limited = true;
                }
                s.run = s.run.saturating_add(1);
                if s.line_fence != 0 {
                    s.line_run = s.run;
                }
            } else {
                s.body = true;
                if s.prefix == b'#' && s.run <= 6 && b.is_ascii_whitespace() && s.fence == 0 {
                    s.heading = true;
                    emit(TokenSpan {
                        source: s.start + usize::from(s.indent)..at,
                        kind: SyntaxKind::Heading,
                    });
                }
                if s.line_fence != 0 && s.line_run < 3 {
                    s.line_fence = 0;
                }
                if s.line_fence != 0 && s.closing && !b.is_ascii_whitespace() {
                    s.tail_space = false;
                }
            }
            if s.line_fence != 0 {
                kind = SyntaxKind::Code;
            } else if s.fence == 0 && s.prefix == b'>' {
                s.quote = true;
            }
        } else if s.line_fence != 0 && s.closing && !b.is_ascii_whitespace() {
            s.tail_space = false;
        }
        if s.line_fence != 0 && s.line_run >= 3 {
            kind = SyntaxKind::Code;
        } else if s.fence == 0 {
            kind = if s.heading {
                SyntaxKind::Heading
            } else if s.quote {
                SyntaxKind::Comment
            } else {
                SyntaxKind::Plain
            };
            if s.escape {
                s.escape = false;
            } else if b == b'\\' {
                s.escape = true;
            } else if s.inline == b'`' {
                kind = SyntaxKind::Code;
                if b == b'`' {
                    s.inline = 0;
                }
            } else if s.inline == b'[' {
                kind = SyntaxKind::Link;
                if b == b')' {
                    s.inline = 0;
                }
            } else if b == b'`' {
                s.inline = b'`';
                kind = SyntaxKind::Code;
            } else if b == b'[' {
                s.inline = b'[';
                kind = SyntaxKind::Link;
            } else if matches!(b, b'*' | b'_') {
                if s.previous != b {
                    if s.inline == b {
                        s.inline = 0;
                    } else {
                        s.inline = b;
                    }
                }
                kind = SyntaxKind::Emphasis;
            } else if matches!(s.inline, b'*' | b'_') {
                kind = SyntaxKind::Emphasis;
            } else if !s.body && matches!(b, b'-' | b'+' | b'>' | b'|' | b'#') {
                kind = SyntaxKind::Operator;
            }
        }
        emit(TokenSpan {
            source: at..at + 1,
            kind,
        });
        s.previous = b;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn colors(bytes: &[u8], chunk: usize) -> Vec<SyntaxKind> {
        let mut s = MarkdownState::default();
        let mut out = vec![SyntaxKind::Plain; bytes.len()];
        for (n, b) in bytes.chunks(chunk).enumerate() {
            scan(
                &mut s,
                b,
                n * chunk,
                n * chunk + b.len() == bytes.len(),
                |t| out[t.source].fill(t.kind),
            );
        }
        out
    }
    #[test]
    fn markdown_headings_links_fences_and_invalid_bytes_are_fragment_independent() {
        let bytes=b"# Titulo\r\nTexto *enfasis* [sitio](url)\n````rust\nfn a() {}\n```\nretained\n````\nfin\xff";
        let expected = colors(bytes, bytes.len());
        for n in 1..=13 {
            assert_eq!(colors(bytes, n), expected);
        }
        assert_eq!(expected[2], SyntaxKind::Heading);
        let at = bytes.windows(8).position(|b| b == b"retained").unwrap();
        assert_eq!(expected[at], SyntaxKind::Code);
        let at = bytes.windows(3).position(|b| b == b"fin").unwrap();
        assert_eq!(expected[at], SyntaxKind::Plain);
    }
}

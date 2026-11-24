//! Shared palette data; frontends translate RGB into their own drawing APIs.
use jc_languages::syntax::SyntaxKind;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}
pub fn foreground(kind: SyntaxKind) -> Rgb {
    let (r, g, b) = match kind {
        SyntaxKind::Plain => (216, 215, 213),
        SyntaxKind::Keyword => (116, 181, 248),
        SyntaxKind::Type => (95, 215, 202),
        SyntaxKind::Boolean => (149, 224, 167),
        SyntaxKind::String => (241, 167, 131),
        SyntaxKind::Comment => (151, 173, 135),
        SyntaxKind::Number => (208, 170, 250),
        SyntaxKind::Preprocessor => (239, 148, 213),
        SyntaxKind::Operator => (178, 192, 210),
        SyntaxKind::Heading => (103, 212, 208),
        SyntaxKind::Emphasis => (239, 154, 112),
        SyntaxKind::Code => (166, 217, 155),
        SyntaxKind::Link => (133, 214, 245),
    };
    Rgb { r, g, b }
}

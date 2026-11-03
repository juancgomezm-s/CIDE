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
        SyntaxKind::Keyword => (249, 38, 114),
        SyntaxKind::String => (230, 219, 116),
        SyntaxKind::Comment => (138, 135, 117),
        SyntaxKind::Number => (174, 129, 255),
        SyntaxKind::Preprocessor => (102, 217, 239),
        SyntaxKind::Operator => (253, 151, 31),
        SyntaxKind::Heading => (103, 212, 208),
        SyntaxKind::Emphasis => (239, 154, 112),
        SyntaxKind::Code => (166, 217, 155),
        SyntaxKind::Link => (133, 214, 245),
    };
    Rgb { r, g, b }
}

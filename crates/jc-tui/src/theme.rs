//! Semantic terminal roles inspired by the owner's OmniCore palette.
use crate::screen::CellStyle;
use crossterm::style::Color;
pub(crate) struct Style {
    pub foreground: Color,
    pub background: Color,
    pub bold: bool,
    pub underline: bool,
    pub reverse: bool,
}
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb { r, g, b }
}
pub(crate) fn style(role: CellStyle) -> Style {
    let mut s = Style {
        foreground: rgb(244, 247, 251),
        background: rgb(6, 24, 34),
        bold: false,
        underline: false,
        reverse: false,
    };
    match role {
        CellStyle::Normal => {}
        CellStyle::Dim => s.foreground = rgb(150, 172, 192),
        CellStyle::Reverse => {
            s.background = rgb(61, 80, 104);
            s.bold = true;
            s.reverse = true;
        }
        CellStyle::Menu | CellStyle::MenuMnemonic => {
            s.background = rgb(24, 42, 56);
            s.foreground = rgb(185, 203, 223);
        }
        CellStyle::MenuActive | CellStyle::MenuMnemonicActive => {
            s.background = rgb(61, 80, 104);
            s.foreground = rgb(244, 247, 251);
            s.bold = true;
        }
        CellStyle::Panel => s.background = rgb(19, 34, 46),
        CellStyle::PanelTitle => {
            s.background = rgb(32, 44, 59);
            s.foreground = rgb(103, 212, 208);
            s.bold = true;
        }
        CellStyle::Status => {
            s.background = rgb(24, 42, 56);
            s.foreground = rgb(185, 203, 223);
        }
        CellStyle::Accent => {
            s.foreground = rgb(103, 212, 208);
            s.bold = true;
        }
        CellStyle::Border => s.foreground = rgb(64, 86, 102),
        CellStyle::Syntax(kind) => {
            let c = jc_app::syntax_theme::foreground(kind);
            s.foreground = rgb(c.r, c.g, c.b);
            s.bold = matches!(
                kind,
                jc_app::SyntaxKind::Heading | jc_app::SyntaxKind::Emphasis
            );
            s.underline = kind == jc_app::SyntaxKind::Link;
            if kind == jc_app::SyntaxKind::Code {
                s.background = rgb(19, 34, 46);
            }
        }
    }
    if matches!(
        role,
        CellStyle::MenuMnemonic | CellStyle::MenuMnemonicActive
    ) {
        s.underline = true;
        s.foreground = rgb(103, 212, 208);
        s.bold = true;
    }
    s
}

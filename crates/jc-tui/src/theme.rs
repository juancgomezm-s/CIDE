//! Semantic terminal roles inspired by the owner's OmniCore palette.
use crate::screen::CellStyle;
use crossterm::style::Color;
use jc_app::ui_theme;
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
const fn palette(rgb: jc_app::syntax_theme::Rgb) -> Color {
    Color::Rgb {
        r: rgb.r,
        g: rgb.g,
        b: rgb.b,
    }
}
pub(crate) fn style(role: CellStyle) -> Style {
    let mut s = Style {
        foreground: rgb(244, 247, 251),
        background: palette(ui_theme::BACKGROUND),
        bold: false,
        underline: false,
        reverse: false,
    };
    match role {
        CellStyle::Normal => {}
        CellStyle::DiffAdded => {
            s.background = rgb(35, 68, 49);
            s.foreground = rgb(192, 239, 200);
        }
        CellStyle::DiffDeleted => {
            s.background = rgb(80, 43, 49);
            s.foreground = rgb(255, 202, 204);
        }
        CellStyle::Dim => s.foreground = rgb(150, 172, 192),
        CellStyle::Reverse => {
            s.background = palette(ui_theme::SELECTED);
            s.bold = true;
            s.reverse = true;
        }
        CellStyle::Menu | CellStyle::MenuMnemonic => {
            s.background = palette(ui_theme::CHROME);
            s.foreground = rgb(185, 203, 223);
        }
        CellStyle::MenuActive | CellStyle::MenuMnemonicActive => {
            s.background = palette(ui_theme::SELECTED);
            s.foreground = rgb(244, 247, 251);
            s.bold = true;
        }
        CellStyle::Panel => s.background = palette(ui_theme::PANEL),
        CellStyle::PanelTitle => {
            s.background = palette(ui_theme::RAISED);
            s.foreground = rgb(103, 212, 208);
            s.bold = true;
        }
        CellStyle::Status => {
            s.background = palette(ui_theme::CHROME);
            s.foreground = rgb(185, 203, 223);
        }
        CellStyle::Accent => {
            s.foreground = rgb(103, 212, 208);
            s.bold = true;
        }
        CellStyle::Border => s.foreground = palette(ui_theme::BORDER),
        CellStyle::Relation(mark) => s.foreground = palette(mark.color()),
        CellStyle::Syntax(kind) => {
            let c = jc_app::syntax_theme::foreground(kind);
            s.foreground = rgb(c.r, c.g, c.b);
            s.bold = matches!(
                kind,
                jc_app::SyntaxKind::Heading | jc_app::SyntaxKind::Emphasis
            );
            s.underline = kind == jc_app::SyntaxKind::Link;
            if kind == jc_app::SyntaxKind::Code {
                s.background = palette(ui_theme::PANEL);
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

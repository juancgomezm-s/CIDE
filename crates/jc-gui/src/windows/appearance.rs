//! Compact defaults and independent editor zoom, in logical pixels and percent.
use super::{dpi, ffi::*, owner};

pub(super) const DEFAULT_UI: u8 = 12;
pub(super) const DEFAULT_EDITOR: u8 = 15;
pub(super) const DEFAULT_ZOOM: u8 = 100;
pub(super) const DEFAULT_MINIMAP: u16 = 160;
pub(super) const MIN_MINIMAP: u16 = 60;
pub(super) const MAX_MINIMAP: u16 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Theme(Theme),
    Ui(u8),
    Editor(u8),
    Zoom(u8),
    Minimap(bool),
    MinimapWidth(u16),
    OutlineLeft(bool),
}

pub(super) const APPEARANCE: &[(Choice, &str)] = &[
    (
        Choice::Theme(Theme::Current),
        "Tema del editor · CIDE (predeterminado)",
    ),
    (
        Choice::Theme(Theme::Neutral),
        "Tema del editor · Oscuro neutro",
    ),
    (
        Choice::Theme(Theme::Warm),
        "Tema del editor · Oscuro cálido",
    ),
    (
        Choice::Theme(Theme::Blue),
        "Tema del editor · Azul profundo",
    ),
    (Choice::Theme(Theme::Light), "Tema del editor · Claro suave"),
    (
        Choice::Theme(Theme::Contrast),
        "Tema del editor · Alto contraste",
    ),
    (Choice::Ui(11), "Interfaz · 11 px"),
    (Choice::Ui(12), "Interfaz · 12 px (compacta)"),
    (Choice::Ui(14), "Interfaz · 14 px"),
    (Choice::Ui(16), "Interfaz · 16 px"),
    (Choice::Editor(13), "Editor · 13 px"),
    (Choice::Editor(15), "Editor · 15 px (compacto)"),
    (Choice::Editor(17), "Editor · 17 px"),
    (Choice::Editor(20), "Editor · 20 px"),
    (Choice::Minimap(true), "Minimapa · Mostrar"),
    (Choice::Minimap(false), "Minimapa · Ocultar"),
    (
        Choice::MinimapWidth(MIN_MINIMAP),
        "Ancho del minimapa · 60 px",
    ),
    (Choice::MinimapWidth(96), "Ancho del minimapa · 96 px"),
    (Choice::MinimapWidth(128), "Ancho del minimapa · 128 px"),
    (
        Choice::MinimapWidth(DEFAULT_MINIMAP),
        "Ancho del minimapa · 160 px",
    ),
    (
        Choice::MinimapWidth(MAX_MINIMAP),
        "Ancho del minimapa · 200 px",
    ),
    (Choice::OutlineLeft(false), "Esquema · Derecha"),
    (Choice::OutlineLeft(true), "Esquema · Izquierda"),
];

pub(super) const ZOOMS: [u8; 9] = [60, 75, 90, 100, 110, 125, 150, 175, 200];

pub(super) fn valid(ui: u8, editor: u8, zoom: u8, visible: u8, width: u16) -> bool {
    [
        Choice::Ui(ui),
        Choice::Editor(editor),
        Choice::MinimapWidth(width),
    ]
    .into_iter()
    .all(|choice| APPEARANCE.iter().any(|(entry, _)| *entry == choice))
        && ZOOMS.contains(&zoom)
        && visible <= 1
}

pub(super) unsafe fn apply(hwnd: Hwnd, choice: Choice) {
    let Some(model) = owner(hwnd) else {
        return;
    };
    let mut state = model.state.borrow_mut();
    let (mut ui, mut editor, mut zoom) = (
        state.ui_text_size,
        state.editor_text_size,
        state.editor_zoom,
    );
    match choice {
        Choice::Theme(theme) => state.editor_theme = theme,
        Choice::Ui(value) => ui = value,
        Choice::Editor(value) => editor = value,
        Choice::Zoom(value) => zoom = value,
        Choice::Minimap(visible) => state.minimap_visible = visible,
        Choice::MinimapWidth(width) => state.minimap_width = width,
        Choice::OutlineLeft(left) => {
            state.outline.left = left;
            state.schedule_outline();
        }
    }
    model.frame.borrow_mut().take();
    model.frame_dirty.set(true);
    let old = (
        state.ui_text_size,
        state.editor_text_size,
        state.editor_zoom,
    );
    let current_dpi = state.dpi;
    if old != (ui, editor, zoom) {
        state.ui_text_size = ui;
        state.editor_text_size = editor;
        state.editor_zoom = zoom;
        if let Err(error) = state.refresh_dpi(hwnd, current_dpi) {
            (
                state.ui_text_size,
                state.editor_text_size,
                state.editor_zoom,
            ) = old;
            state.error(error);
            return;
        }
        if let Some(theme) = model.menu_theme.borrow().as_ref() {
            if let Err(error) = theme.set_text_scale(current_dpi, ui) {
                state.error(error);
                return;
            }
        }
    } else {
        state.invalidate_appearance();
    }
    state.status = match choice {
        Choice::Theme(_) => APPEARANCE
            .iter()
            .find(|(entry, _)| *entry == choice)
            .map_or("Tema", |(_, label)| *label)
            .into(),
        Choice::Ui(size) => format!("Tamaño de interfaz · {size} px"),
        Choice::Editor(size) => format!("Tamaño base del editor · {size} px"),
        Choice::Zoom(zoom) => format!("Zoom del editor · {zoom} %"),
        Choice::Minimap(true) => "Minimapa visible".into(),
        Choice::Minimap(false) => "Minimapa oculto".into(),
        Choice::MinimapWidth(width) => format!("Ancho del minimapa · {width} px"),
        Choice::OutlineLeft(true) => "Esquema a la izquierda".into(),
        Choice::OutlineLeft(false) => "Esquema a la derecha".into(),
    };
    drop(state);
    InvalidateRect(hwnd, std::ptr::null(), 0);
}

pub(super) fn editor_pixels(dpi: dpi::Dpi, size: u8, zoom: u8) -> i32 {
    // Multiply before rounding so intermediate logical rounding doesn't amplify
    // the error at high-DPI. GDI takes a negative character height.
    let size = dpi.pixels(i32::from(size) * i32::from(zoom));
    ((size + 50) / 100).max(1)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Theme {
    #[default]
    Current,
    Neutral,
    Warm,
    Blue,
    Light,
    Contrast,
}

impl Theme {
    pub(super) fn from_id(id: u8) -> Option<Self> {
        [
            Self::Current,
            Self::Neutral,
            Self::Warm,
            Self::Blue,
            Self::Light,
            Self::Contrast,
        ]
        .get(usize::from(id))
        .copied()
    }

    // RGB hex values are kept in display order for easy review.
    fn colors(self) -> [u32; 12] {
        match self {
            Self::Current => unreachable!("CIDE uses the original palette"),
            // Background, text, muted, selection, accent, map background, map viewport,
            // keyword, type, string, comment, literal.
            Self::Neutral => [
                0x202226, 0xe4e6eb, 0xa4acb9, 0x36496b, 0xaad2ff, 0x191b1f, 0x303742, 0x9ac7fa,
                0x87dacc, 0xf2bb91, 0xa0ba91, 0xd9b8fc,
            ],
            Self::Warm => [
                0x2b2622, 0xeee3d3, 0xbfb09f, 0x544137, 0xf4cd86, 0x241f1b, 0x443730, 0xf3bf81,
                0xaad8b6, 0xf1a791, 0xb9bd91, 0xdcb9ef,
            ],
            Self::Blue => [
                0x13253a, 0xe4effb, 0xa7bdd4, 0x304f70, 0x9ce5ff, 0x0f1c2c, 0x203953, 0x90caff,
                0x8be1df, 0xffc6a2, 0xa1c5a5, 0xd9c1ff,
            ],
            Self::Light => [
                0xf5f2eb, 0x26313a, 0x515e69, 0xcbdcef, 0x174675, 0xe9e5db, 0xd8e1eb, 0x204d8b,
                0x126457, 0x8a381c, 0x4b6643, 0x6c3784,
            ],
            Self::Contrast => [
                0x080808, 0xffffff, 0xd5d5d5, 0x293d64, 0xffec74, 0x000000, 0x202837, 0x92d5ff,
                0x78ffdb, 0xffbe94, 0xc4f997, 0xffafff,
            ],
        }
    }
    fn role(self, index: usize, original: u32) -> u32 {
        if self == Self::Current {
            original
        } else {
            let rgb = self.colors()[index];
            ((rgb & 0xff) << 16) | (rgb & 0xff00) | (rgb >> 16)
        }
    }
    pub(super) fn background(self) -> u32 {
        self.role(0, super::shell::BACKGROUND)
    }
    pub(super) fn text(self) -> u32 {
        self.role(1, super::shell::TEXT)
    }
    pub(super) fn muted(self) -> u32 {
        self.role(2, super::shell::MUTED)
    }
    pub(super) fn selection(self) -> u32 {
        self.role(3, super::shell::SELECTED)
    }
    pub(super) fn accent(self) -> u32 {
        self.role(4, super::shell::ACCENT)
    }
    pub(super) fn role_surface(self) -> u32 {
        self.role(5, super::shell::SURFACE)
    }
    pub(super) fn map_background(self) -> u32 {
        self.role(5, super::shell::SIDEBAR)
    }
    pub(super) fn map_viewport(self) -> u32 {
        self.role(6, super::shell::RAISED)
    }
    pub(super) fn syntax(self, kind: jc_app::SyntaxKind) -> u32 {
        use jc_app::SyntaxKind::*;
        if self == Self::Current {
            let rgb = jc_app::syntax_theme::foreground(kind);
            return u32::from(rgb.r) | (u32::from(rgb.g) << 8) | (u32::from(rgb.b) << 16);
        }
        match kind {
            Plain | Operator => self.text(),
            Keyword | Link => self.role(7, 0),
            Type | Preprocessor | Heading => self.role(8, 0),
            String | Emphasis => self.role(9, 0),
            Comment => self.role(10, 0),
            Boolean | Number | Code => self.role(11, 0),
        }
    }
}

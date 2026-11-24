//! Compact defaults and independent editor zoom, in logical pixels and percent.
use super::{dpi, ffi::*, menu_theme::Palette, owner, shell, Owner, State};

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
    FontPicker,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Settings {
    pub theme: Theme,
    pub ui: u8,
    pub editor: u8,
    pub zoom: u8,
    pub minimap: bool,
    pub minimap_width: u16,
    pub outline_left: bool,
    pub face: super::editor_fonts::Face,
}

impl Settings {
    pub(super) fn read(state: &State) -> Self {
        Self {
            theme: state.editor_theme,
            ui: state.ui_text_size,
            editor: state.editor_text_size,
            zoom: state.editor_zoom,
            minimap: state.minimap_visible,
            minimap_width: state.minimap_width,
            outline_left: state.outline.left,
            face: state.editor_face,
        }
    }

    fn with(mut self, choice: Choice) -> Self {
        match choice {
            Choice::Theme(theme) => self.theme = theme,
            Choice::Ui(ui) => self.ui = ui,
            Choice::Editor(editor) => self.editor = editor,
            Choice::Zoom(zoom) => self.zoom = zoom,
            Choice::Minimap(minimap) => self.minimap = minimap,
            Choice::MinimapWidth(width) => self.minimap_width = width,
            Choice::OutlineLeft(left) => self.outline_left = left,
            Choice::FontPicker => {}
        }
        self
    }

    pub(super) fn checked(self) -> [Choice; 6] {
        [
            Choice::Theme(self.theme),
            Choice::Ui(self.ui),
            Choice::Editor(self.editor),
            Choice::Minimap(self.minimap),
            Choice::MinimapWidth(self.minimap_width),
            Choice::OutlineLeft(self.outline_left),
        ]
    }
}

impl State {
    pub(super) fn committed_appearance(&self) -> Settings {
        self.saved_appearance
            .unwrap_or_else(|| Settings::read(self))
    }
}

pub(super) const APPEARANCE: &[(Choice, &str)] = &[
    (
        Choice::Theme(Theme::Current),
        "Tema de la ventana · CIDE (predeterminado)",
    ),
    (
        Choice::Theme(Theme::Neutral),
        "Tema de la ventana · Oscuro neutro",
    ),
    (
        Choice::Theme(Theme::Warm),
        "Tema de la ventana · Oscuro cálido",
    ),
    (
        Choice::Theme(Theme::Blue),
        "Tema de la ventana · Azul profundo",
    ),
    (
        Choice::Theme(Theme::Light),
        "Tema de la ventana · Claro suave",
    ),
    (
        Choice::Theme(Theme::Contrast),
        "Tema de la ventana · Alto contraste",
    ),
    (
        Choice::Theme(Theme::LightDay),
        "Tema de la ventana · Claro nítido",
    ),
    (
        Choice::Theme(Theme::LightPaper),
        "Tema de la ventana · Papel cálido",
    ),
    (
        Choice::Theme(Theme::ExtraDark),
        "Tema de la ventana · Grafito profundo",
    ),
    (
        Choice::Theme(Theme::Eclipse),
        "Tema de la ventana · Eclipse oscuro",
    ),
    (Choice::FontPicker, "Fuente del editor · Elegir tipografía…"),
    (Choice::Ui(11), "Interfaz · 11 px"),
    (Choice::Ui(12), "Interfaz · 12 px (compacta)"),
    (Choice::Ui(14), "Interfaz · 14 px"),
    (Choice::Ui(16), "Interfaz · 16 px"),
    (Choice::Editor(11), "Editor · 11 px"),
    (Choice::Editor(12), "Editor · 12 px"),
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

unsafe fn set(
    hwnd: Hwnd,
    model: &Owner,
    state: &mut State,
    next: Settings,
) -> Result<bool, String> {
    let old = Settings::read(state);
    if old == next {
        return Ok(false);
    }
    let current_dpi = state.dpi;
    let menu = model.menu_theme.borrow();
    if let Some(theme) = menu.as_ref().filter(|_| old.ui != next.ui) {
        theme.set_text_scale(current_dpi, next.ui)?;
    }
    if let Some(theme) = menu.as_ref().filter(|_| old.theme != next.theme) {
        if let Err(error) = theme.apply_palette(Palette::for_theme(next.theme)) {
            theme.set_text_scale(current_dpi, old.ui)?;
            return Err(error);
        }
    }
    if (old.ui, old.editor, old.zoom, old.face) != (next.ui, next.editor, next.zoom, next.face) {
        (
            state.ui_text_size,
            state.editor_text_size,
            state.editor_zoom,
        ) = (next.ui, next.editor, next.zoom);
        state.editor_face = next.face;
        if let Err(error) = state.refresh_dpi(hwnd, current_dpi) {
            (
                state.ui_text_size,
                state.editor_text_size,
                state.editor_zoom,
            ) = (old.ui, old.editor, old.zoom);
            state.editor_face = old.face;
            if let Some(theme) = menu.as_ref().filter(|_| old.ui != next.ui) {
                theme.set_text_scale(current_dpi, old.ui)?;
            }
            if let Some(theme) = menu.as_ref().filter(|_| old.theme != next.theme) {
                theme.apply_palette(Palette::for_theme(old.theme))?;
            }
            return Err(error);
        }
    } else {
        state.invalidate_appearance();
    }
    state.editor_theme = next.theme;
    if old.theme != next.theme {
        shell::set_caption_theme(hwnd, next.theme);
    }
    state.minimap_visible = next.minimap;
    state.minimap_width = next.minimap_width;
    state.outline.left = next.outline_left;
    if old.outline_left != next.outline_left {
        state.schedule_outline();
    }
    model.frame.borrow_mut().take();
    model.frame_dirty.set(true);
    Ok(true)
}

pub(super) unsafe fn synchronize_preview(hwnd: Hwnd) -> bool {
    let Some(model) = owner(hwnd) else {
        return false;
    };
    let mut state = model.state.borrow_mut();
    let Some(saved) = state.saved_appearance else {
        return false;
    };
    let next = if state.appearance_open() {
        let draft = state.appearance_draft.unwrap_or(saved);
        state
            .selector
            .as_ref()
            .and_then(|selector| selector.appearance_target())
            .map_or(draft, |choice| draft.with(choice))
    } else {
        saved
    };
    match set(hwnd, model, &mut state, next) {
        Ok(changed) => {
            if state.appearance_open() {
                state.appearance_draft = Some(Settings::read(&state));
            } else {
                state.saved_appearance = None;
                state.appearance_draft = None;
            }
            changed
        }
        Err(error) => {
            state.error(error);
            true
        }
    }
}

pub(super) unsafe fn apply(hwnd: Hwnd, choice: Choice) {
    if choice == Choice::FontPicker {
        super::editor_fonts::choose(hwnd);
        return;
    }
    let Some(model) = owner(hwnd) else {
        return;
    };
    let mut state = model.state.borrow_mut();
    let next = state
        .appearance_draft
        .filter(|_| state.appearance_open())
        .unwrap_or_else(|| state.committed_appearance())
        .with(choice);
    if let Err(error) = set(hwnd, model, &mut state, next) {
        state.error(error);
        return;
    }
    state.saved_appearance = None;
    state.appearance_draft = None;
    if state.appearance_open() {
        state.close_selector();
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
        Choice::FontPicker => unreachable!(),
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

pub(super) unsafe fn preview_font(
    hwnd: Hwnd,
    model: &Owner,
    state: &mut State,
    face: super::editor_fonts::Face,
) -> Result<(), String> {
    let next = Settings {
        face,
        ..Settings::read(state)
    };
    set(hwnd, model, state, next)?;
    state.appearance_draft = Some(Settings::read(state));
    Ok(())
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
    LightDay,
    LightPaper,
    ExtraDark,
    Eclipse,
}

impl Theme {
    pub(super) const ALL: [Self; 10] = [
        Self::Current,
        Self::Neutral,
        Self::Warm,
        Self::Blue,
        Self::Eclipse,
        Self::ExtraDark,
        Self::Contrast,
        Self::Light,
        Self::LightDay,
        Self::LightPaper,
    ];
    pub(super) fn is_light(self) -> bool {
        matches!(self, Self::Light | Self::LightDay | Self::LightPaper)
    }
    pub(super) fn from_id(id: u8) -> Option<Self> {
        [
            Self::Current,
            Self::Neutral,
            Self::Warm,
            Self::Blue,
            Self::Light,
            Self::Contrast,
            Self::LightDay,
            Self::LightPaper,
            Self::ExtraDark,
            Self::Eclipse,
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
                0xf5f2eb, 0x26313a, 0x515e69, 0xe1eafa, 0x165bc5, 0xe9e5db, 0xd8e1eb, 0x165bc5,
                0x006e78, 0xa64018, 0x486d4b, 0x8c35aa,
            ],
            Self::LightDay => [
                0xffffff, 0x252b36, 0x556273, 0xe4eeff, 0x175bce, 0xf0f3f8, 0xdce5f2, 0x175bce,
                0x007077, 0xb04412, 0x4f7251, 0x9236b5,
            ],
            Self::LightPaper => [
                0xfff8ec, 0x38322b, 0x655f55, 0xf4e5d0, 0x345ac6, 0xf5eddc, 0xe8ddc7, 0x345ac6,
                0x006e65, 0xa0470d, 0x576b3d, 0x8b409d,
            ],
            Self::ExtraDark => [
                0x0d1017, 0xdae2ef, 0x9da8bd, 0x293851, 0x89c7ff, 0x090c12, 0x1c2432, 0x89b4fa,
                0x65d4cf, 0xf6b993, 0x91b899, 0xce9bff,
            ],
            Self::Eclipse => [
                0x282828, 0xdcdcdc, 0xa8aaa9, 0x353b45, 0x75bcf5, 0x222222, 0x33383e, 0x56b6f2,
                0x40d7cb, 0xe9a06f, 0x9fbd91, 0xc7a0f9,
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

    /// Resolve the existing UI palette at the native drawing boundary. Editor
    /// syntax and source-diff cells already carry their own resolved colors.
    pub(super) fn control_color(self, color: u32) -> u32 {
        if self == Self::Current {
            return color;
        }
        match color {
            // Chrome, tabs and status use the main surface. Side panels and
            // overlay headers use their own levels of elevation.
            shell::BACKGROUND => self.background(),
            shell::SIDEBAR | 0x003c312b => self.map_background(),
            shell::RAISED | 0x004a3d35 | 0x003b3632 => self.map_viewport(),
            shell::SELECTED | 0x00624e41 => self.selection(),
            shell::TEXT | 0x00d5d7d8 => self.text(),
            shell::MUTED | 0x008d8b87 | 0x00af9d92 => self.muted(),
            shell::ACCENT | 0x00a6d9e6 | 0x00d0c088 => self.accent(),
            shell::BORDER | 0x00594b42 | 0x00625b53 => blend(self.map_viewport(), self.muted(), 48),
            0x00333130 => blend(self.map_background(), self.muted(), 12),
            // Git and standalone comparisons retain a distinct red/green signal.
            0x003f4830 | 0x00314423 | 0x00364635 => blend(self.background(), 0x0080bd83, 27),
            0x003e354a | 0x00312b50 | 0x00313549 => blend(self.background(), 0x00848ddf, 27),
            _ => color,
        }
    }

    pub(super) fn control_ink(self, color: u32) -> u32 {
        let resolved = self.control_color(color);
        if self.is_light() && resolved == color {
            blend(color, self.text(), 66)
        } else {
            resolved
        }
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
            Type | Heading => self.role(8, 0),
            Preprocessor => {
                let rgb = match self {
                    Self::Light | Self::LightDay => 0xa31b80,
                    Self::LightPaper => 0x9f207d,
                    _ => 0xef94d5,
                };
                ((rgb & 0xff) << 16) | (rgb & 0xff00) | (rgb >> 16)
            }
            String | Emphasis => self.role(9, 0),
            Comment => self.role(10, 0),
            Boolean if self == Self::Eclipse => 0x0091ce9f,
            Boolean | Number | Code => self.role(11, 0),
        }
    }
}

fn blend(base: u32, accent: u32, percent: u32) -> u32 {
    [0, 8, 16].into_iter().fold(0, |color, shift| {
        let base_channel = (base >> shift) & 0xff;
        let accent_channel = (accent >> shift) & 0xff;
        color | (((base_channel * (100 - percent) + accent_channel * percent) / 100) << shift)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn luminance(bgr: u32) -> f64 {
        [0, 8, 16]
            .into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(shift, w)| {
                let n = f64::from((bgr >> shift) & 255) / 255.;
                w * if n <= 0.04045 {
                    n / 12.92
                } else {
                    ((n + 0.055) / 1.055).powf(2.4)
                }
            })
            .sum()
    }
    #[test]
    fn syntax_is_readable_on_light_and_extra_dark_base_and_selection() {
        use jc_app::SyntaxKind::*;
        for theme in [
            Theme::Light,
            Theme::LightDay,
            Theme::LightPaper,
            Theme::ExtraDark,
            Theme::Eclipse,
        ] {
            assert_eq!(Theme::from_id(theme as u8), Some(theme));
            for bg in [theme.background(), theme.selection()] {
                for kind in [
                    Plain,
                    Keyword,
                    Type,
                    String,
                    Comment,
                    Boolean,
                    Number,
                    Operator,
                    Link,
                    Preprocessor,
                ] {
                    let (a, b) = (luminance(bg), luminance(theme.syntax(kind)));
                    assert!(
                        (a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5,
                        "{theme:?} {kind:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn eclipse_numbers_are_distinct_from_text_keywords_strings_and_booleans() {
        use jc_app::SyntaxKind::*;
        for kind in [Plain, Keyword, Type, String, Comment, Boolean, Preprocessor] {
            assert_ne!(Theme::Eclipse.syntax(Number), Theme::Eclipse.syntax(kind));
        }
    }
}

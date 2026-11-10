//! Compact defaults and independent editor zoom, in logical pixels and percent.
use super::{dpi, ffi::*, owner};

pub(super) const DEFAULT_UI: u8 = 12;
pub(super) const DEFAULT_EDITOR: u8 = 15;
pub(super) const DEFAULT_ZOOM: u8 = 100;
pub(super) const DEFAULT_MINIMAP: u16 = 76;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Ui(u8),
    Editor(u8),
    Zoom(u8),
    Minimap(bool),
    MinimapWidth(u16),
}

pub(super) const APPEARANCE: &[(Choice, &str)] = &[
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
    (Choice::MinimapWidth(60), "Ancho del minimapa · 60 px"),
    (Choice::MinimapWidth(76), "Ancho del minimapa · 76 px"),
    (Choice::MinimapWidth(96), "Ancho del minimapa · 96 px"),
];

pub(super) const ZOOMS: &[(Choice, &str)] = &[
    (Choice::Zoom(60), "60 %"),
    (Choice::Zoom(75), "75 %"),
    (Choice::Zoom(90), "90 %"),
    (Choice::Zoom(100), "100 %"),
    (Choice::Zoom(110), "110 %"),
    (Choice::Zoom(125), "125 %"),
    (Choice::Zoom(150), "150 %"),
    (Choice::Zoom(175), "175 %"),
    (Choice::Zoom(200), "200 %"),
];

pub(super) fn valid(ui: u8, editor: u8, zoom: u8, visible: u8, width: u16) -> bool {
    [
        Choice::Ui(ui),
        Choice::Editor(editor),
        Choice::Zoom(zoom),
        Choice::MinimapWidth(width),
    ]
    .into_iter()
    .all(|choice| {
        APPEARANCE
            .iter()
            .chain(ZOOMS)
            .any(|(entry, _)| *entry == choice)
    }) && visible <= 1
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
        Choice::Ui(value) => ui = value,
        Choice::Editor(value) => editor = value,
        Choice::Zoom(value) => zoom = value,
        Choice::Minimap(visible) => state.minimap_visible = visible,
        Choice::MinimapWidth(width) => state.minimap_width = width,
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
        Choice::Ui(size) => format!("Tamaño de interfaz · {size} px"),
        Choice::Editor(size) => format!("Tamaño base del editor · {size} px"),
        Choice::Zoom(zoom) => format!("Zoom del editor · {zoom} %"),
        Choice::Minimap(true) => "Minimapa visible".into(),
        Choice::Minimap(false) => "Minimapa oculto".into(),
        Choice::MinimapWidth(width) => format!("Ancho del minimapa · {width} px"),
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

//! Native IMM composition and one Workbench transaction per result.
use super::{
    ffi::{Hwnd, Point, Rect},
    line_edit::EditTarget,
    paint, State,
};
use std::ffi::c_void;

pub(super) const START: u32 = 0x010d;
pub(super) const END: u32 = 0x010e;
pub(super) const COMPOSITION: u32 = 0x010f;
pub(super) const RESULT: isize = 0x0800;
pub(super) const PREEDIT: isize = 0x01bf;
const MAX_BYTES: usize = 64 * 1024;

type Himc = *mut c_void;

#[repr(C)]
struct CompositionForm {
    style: u32,
    point: Point,
    area: Rect,
}

#[repr(C)]
struct CandidateForm {
    index: u32,
    style: u32,
    point: Point,
    area: Rect,
}

#[link(name = "imm32")]
extern "system" {
    fn ImmGetContext(hwnd: Hwnd) -> Himc;
    fn ImmReleaseContext(hwnd: Hwnd, context: Himc) -> i32;
    fn ImmGetCompositionStringW(context: Himc, index: u32, buffer: *mut c_void, bytes: u32) -> i32;
    fn ImmSetCompositionWindow(context: Himc, form: *const CompositionForm) -> i32;
    fn ImmSetCandidateWindow(context: Himc, form: *const CandidateForm) -> i32;
}

struct Context {
    hwnd: Hwnd,
    handle: Himc,
}

impl Context {
    unsafe fn get(hwnd: Hwnd) -> Option<Self> {
        let handle = ImmGetContext(hwnd);
        (!handle.is_null()).then_some(Self { hwnd, handle })
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { ImmReleaseContext(self.hwnd, self.handle) };
        }
    }
}

pub(super) unsafe fn read_result(hwnd: Hwnd) -> Result<String, &'static str> {
    let context = Context::get(hwnd).ok_or("El contexto IME ya no está disponible")?;
    let size = ImmGetCompositionStringW(context.handle, RESULT as u32, std::ptr::null_mut(), 0);
    let size = usize::try_from(size).map_err(|_| "No se pudo leer el resultado IME")?;
    if size > MAX_BYTES || size % 2 != 0 {
        return Err("El resultado IME supera el límite o no es UTF-16 válido");
    }
    if size == 0 {
        return Ok(String::new());
    }
    let mut units = vec![0u16; size / 2];
    let read = ImmGetCompositionStringW(
        context.handle,
        RESULT as u32,
        units.as_mut_ptr().cast(),
        size as u32,
    );
    if read != size as i32 {
        return Err("Cambió la longitud del resultado IME");
    }
    String::from_utf16(&units).map_err(|_| "Se rechazó un resultado IME con UTF-16 inválido")
}

pub(super) unsafe fn place(hwnd: Hwnd, caret: Rect) {
    let Some(context) = Context::get(hwnd) else {
        return;
    };
    let composition = CompositionForm {
        style: 0x0002, // CFS_POINT
        point: Point {
            x: caret.left,
            y: caret.top,
        },
        area: Rect::default(),
    };
    ImmSetCompositionWindow(context.handle, &composition);
    let candidate = CandidateForm {
        index: 0,
        style: 0x0080, // CFS_EXCLUDE
        point: Point {
            x: caret.left,
            y: caret.bottom,
        },
        area: caret,
    };
    ImmSetCandidateWindow(context.handle, &candidate);
}

impl State {
    pub(super) fn ime_editor_target(&self) -> Option<(EditTarget, u64)> {
        if !self.focused || self.selector.is_some() || !self.context().editor_input {
            return None;
        }
        Some((self.source_edit_target()?, self.interaction_epoch))
    }

    pub(super) fn ime_caret(&self) -> Option<Rect> {
        let pane = self.workbench.panes().active();
        let position = self.workbench.active_position()?;
        let area = paint::text_areas(self)
            .into_iter()
            .find(|area| area.pane == pane)?;
        let column = if self.workbench.invisibles_visible() {
            self.workbench.display_column_for_pane(pane)?
        } else {
            self.workbench.visual_column_for_pane(pane)?
        };
        let line = position.line.checked_sub(area.first)?;
        let column = column.checked_sub(area.scroll)?;
        if line >= area.rows || column >= area.columns {
            return None;
        }
        let x = area.x + column as i32 * self.cell_width;
        let y = area.y + line as i32 * self.cell_height;
        Some(Rect {
            left: x,
            top: y,
            right: x + self.cell_width,
            bottom: y + self.cell_height,
        })
    }

    pub(super) fn ime_commit(
        &mut self,
        target: (EditTarget, u64),
        result: Result<String, &'static str>,
    ) {
        self.pending_surrogate = None;
        if self.ime_editor_target() != Some(target) || self.context().rectangular_selection {
            self.error("Cambió el destino de la composición IME");
            return;
        }
        let result = match result {
            Ok(result) if result.is_empty() => return,
            Ok(result) if !result.chars().any(char::is_control) => result,
            Ok(_) => {
                self.error("La composición IME contiene caracteres de control");
                return;
            }
            Err(error) => {
                self.error(error);
                return;
            }
        };
        self.cancel_pointer();
        match self.workbench.insert_text(&result) {
            Ok(()) => {
                self.ime_target = self.ime_editor_target();
                self.status = "Listo".into();
            }
            Err(error) => self.error(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GuiOptions;
    use jc_text::TextEncoding;

    fn editor(encoding: TextEncoding) -> State {
        let mut state = State::new(&GuiOptions::default()).unwrap();
        state.focused = true;
        state.workbench.new_file("ime.txt", encoding);
        state.workbench.insert_text("texto").unwrap();
        state.workbench.select_all().unwrap();
        state
    }

    #[test]
    fn editor_ime_result_replaces_selection_in_one_undo() {
        let mut state = editor(TextEncoding::Utf8);
        let target = state.ime_editor_target().unwrap();
        state.ime_commit(target, Ok("日本語🙂".into()));
        assert_eq!(
            state.workbench.active_bytes().unwrap(),
            "日本語🙂".as_bytes()
        );
        state.workbench.undo().unwrap();
        assert_eq!(state.workbench.active_bytes().unwrap(), b"texto");
    }

    #[test]
    fn editor_ime_result_preserves_bytes_on_stale_destination_or_unrepresentable_text() {
        let mut state = editor(TextEncoding::Windows1252);
        let target = state.ime_editor_target().unwrap();
        state.ime_commit(target, Ok("é日本".into()));
        assert_eq!(state.workbench.active_bytes().unwrap(), b"texto");
        state.interaction_epoch += 1;
        state.ime_commit(target, Ok("é".into()));
        assert_eq!(state.workbench.active_bytes().unwrap(), b"texto");
        assert!(state.status.contains("destino"));
    }
}

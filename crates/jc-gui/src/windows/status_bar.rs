//! Bounded read-only status metadata and explicit panel visibility controls.
use super::{ffi::*, paint, shell, wide, State};

#[link(name = "user32")]
extern "system" {
    fn DrawTextW(dc: Hdc, value: *const u16, count: i32, rect: *mut Rect, flags: u32) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn SetBkMode(dc: Hdc, mode: i32) -> i32;
}

unsafe fn label(dc: Hdc, rect: Rect, text: &[u16]) -> Result<(), String> {
    aligned_label(dc, rect, text, 0)
}

pub(super) unsafe fn label_centered(dc: Hdc, rect: Rect, text: &[u16]) -> Result<(), String> {
    aligned_label(dc, rect, text, 1)
}

unsafe fn aligned_label(
    dc: Hdc,
    mut rect: Rect,
    text: &[u16],
    alignment: u32,
) -> Result<(), String> {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return Ok(());
    }
    SetTextColor(dc, paint::ui_ink(shell::TEXT));
    let previous = SetBkMode(dc, 1);
    let result = DrawTextW(
        dc,
        text.as_ptr(),
        -1,
        &mut rect,
        0x8000 | 0x800 | 0x20 | 4 | alignment,
    );
    SetBkMode(dc, previous);
    if result == 0 {
        Err("No se pudo dibujar la barra de estado".into())
    } else {
        Ok(())
    }
}

pub(super) struct Frame {
    rect: Rect,
    left: Rect,
    right: Rect,
    omni: Rect,
    zoom: Rect,
    zoom_label: Vec<u16>,
    explorer: bool,
    code: bool,
    omni_visible: bool,
    summary: Vec<u16>,
}
fn region(s: &State) -> Rect {
    Rect {
        left: 0,
        right: s.width.max(0),
        top: (s.height - s.cell_height - 8).max(0),
        bottom: s.height.max(0),
    }
}
fn parts(s: &State) -> (Rect, Rect, Rect, Rect, Rect) {
    let rect = region(s);
    let button = s.dpi.pixels(76).min((rect.right / 3).max(0));
    let left = Rect {
        right: button,
        ..rect
    };
    let right = Rect {
        left: rect.right - button,
        ..rect
    };
    let omni = Rect {
        right: right.left,
        left: right.left - s.dpi.pixels(86).min((rect.right / 5).max(0)),
        ..rect
    };
    let available = omni.left - left.right;
    let summary_width = ((rect.bottom - rect.top) * 10).min(available / 2).max(0);
    let zoom = Rect {
        right: omni.left - summary_width,
        left: (omni.left - summary_width - s.dpi.pixels(64)).max(left.right),
        ..rect
    };
    (rect, left, right, zoom, omni)
}
pub(super) fn prepare(s: &State) -> Frame {
    let (rect, left, right, zoom, omni) = parts(s);
    let language = s
        .workbench
        .context()
        .active_language
        .unwrap_or_else(|| "Texto plano".into());
    let git = s.git_summary();
    Frame {
        rect,
        left,
        right,
        omni,
        zoom,
        zoom_label: wide(&format!("{} % ▾", s.editor_zoom)),
        explorer: s.explorer_visible,
        code: s.outline.visible,
        omni_visible: s.omnicore.visible,
        summary: wide(&format!("{language} · {git}")),
    }
}
impl State {
    pub(super) fn status_git_click(&mut self, x: i32, y: i32) -> Option<super::Action> {
        let (rect, _, _, zoom, omni) = parts(self);
        if self.pointer_source_available()
            && y >= rect.top
            && y < rect.bottom
            && x >= zoom.right
            && x < omni.left
        {
            Some(self.request_git(super::git_panel::GitIntent::Status))
        } else {
            None
        }
    }
    pub(super) fn status_click(&mut self, x: i32, y: i32) -> bool {
        let (rect, left, right, _, omni) = parts(self);
        if y < rect.top || y >= rect.bottom || x < 0 || x >= rect.right {
            return false;
        }
        if (omni.left..omni.right).contains(&x)
            && self.pointer_overlays_clear()
            && (self.git_panel.is_none() || self.git_code.active)
        {
            self.toggle_omnicore();
            return true;
        }
        if !self.pointer_source_available() {
            return true;
        }
        if x < left.right {
            self.explorer_visible = !self.explorer_visible;
            self.explorer_focused = false;
            self.explorer_hits = None;
        } else if x >= right.left {
            self.set_outline_visible(!self.outline.visible);
        }
        self.tab_hits = None;
        true
    }
}
pub(super) fn zoom_anchor(state: &State) -> Rect {
    parts(state).3
}
pub(super) unsafe fn draw(dc: Hdc, f: &Frame, status: &[u16]) -> Result<(), String> {
    paint::fill(dc, f.rect, shell::SURFACE)?;
    for (r, label, active) in [
        (f.left, "Archivos", f.explorer),
        (f.right, "Esquema", f.code),
        (f.omni, "OmniCore", f.omni_visible),
    ] {
        let background = if active {
            shell::RAISED
        } else {
            shell::SURFACE
        };
        paint::fill(dc, r, background)?;
        paint::text(
            dc,
            Rect {
                left: r.left + 8,
                top: r.top + 3,
                ..r
            },
            &wide(label),
            shell::TEXT,
            background,
        )?;
    }
    paint::fill(dc, f.zoom, shell::RAISED)?;
    label(
        dc,
        Rect {
            left: f.zoom.left + 6,
            ..f.zoom
        },
        &f.zoom_label,
    )?;
    let split = f.zoom.left;
    if split > f.left.right {
        label(
            dc,
            Rect {
                left: f.left.right + 8,
                right: split,
                top: f.rect.top + 3,
                ..f.rect
            },
            status,
        )?;
    }
    if f.zoom.right < f.omni.left {
        label(
            dc,
            Rect {
                left: f.zoom.right + 6,
                right: f.omni.left,
                top: f.rect.top + 3,
                ..f.rect
            },
            &f.summary,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn git_indicator_requests_embedded_status_without_configuration() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.width = 1200;
        s.height = 800;
        s.project_root = Some(std::path::PathBuf::from("C:\\cide-fixture"));
        assert_eq!(s.git_summary(), "Git: consultar");
        let f = prepare(&s);
        assert!(matches!(
            s.status_git_click(f.omni.left - 20, f.rect.top + 2),
            Some(super::super::Action::None)
        ));
        assert!(s.git_status_options.is_some());
        assert!(matches!(
            s.git_panel,
            Some(super::super::git_panel::GitIntent::Status)
        ));
    }
    #[test]
    fn panel_controls_toggle_independently_without_editing_source() {
        let mut s = State::new(&crate::GuiOptions::default()).unwrap();
        s.width = 1200;
        s.height = 800;
        s.workbench
            .new_file("barra.js", jc_text::TextEncoding::Utf8);
        s.workbench.insert_text("const x = 1;").unwrap();
        let before = s.workbench.active_bytes().unwrap();
        let f = prepare(&s);
        let left = s.explorer_visible;
        let code = s.outline.visible;
        assert!(s.status_click(f.left.left + 2, f.left.top + 2));
        assert_eq!(s.explorer_visible, !left);
        assert_eq!(s.outline.visible, code);
        assert!(s.status_click(f.right.left + 2, f.right.top + 2));
        assert_eq!(s.outline.visible, !code);
        assert!(s.status_click(f.omni.left + 2, f.omni.top + 2));
        assert!(s.omnicore.visible);
        assert_eq!(s.outline.visible, !code);
        assert_eq!(s.workbench.active_bytes().unwrap(), before);
        assert!(super::super::outline_panel::region(&s).is_none());
    }
}

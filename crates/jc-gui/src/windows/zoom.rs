//! In-window zoom slider anchored to the percentage in the status bar.
use super::{
    appearance::{Choice, ZOOMS},
    ffi::*,
    paint, shell, status_bar, wide, Action, State,
};

#[derive(Default)]
pub(super) struct Zoom {
    pub open: bool,
    pub preview: Option<u8>,
}

pub(super) struct Popup {
    area: Rect,
    track_top: i32,
    track_bottom: i32,
    percent: u8,
    dpi: super::dpi::Dpi,
}

fn contains(rect: Rect, x: i32, y: i32) -> bool {
    (rect.left..rect.right).contains(&x) && (rect.top..rect.bottom).contains(&y)
}

impl Popup {
    fn new(s: &State) -> Option<Self> {
        let px = |n| s.dpi.pixels(n);
        let anchor = status_bar::zoom_anchor(s);
        let width = px(72);
        let bottom = anchor.top - px(4);
        let top = (bottom - px(240)).max(s.chrome_layout().top() + px(4));
        if anchor.right <= anchor.left || s.width < width || bottom - top < px(76) {
            return None;
        }
        let left = ((anchor.left + anchor.right - width) / 2).clamp(0, s.width - width);
        Some(Self {
            area: Rect {
                left,
                top,
                right: left + width,
                bottom,
            },
            track_top: top + px(48),
            track_bottom: bottom - px(15),
            percent: s.zoom.preview.unwrap_or(s.editor_zoom),
            dpi: s.dpi,
        })
    }

    fn thumb_y(&self, percent: u8) -> i32 {
        let min = i32::from(ZOOMS[0]);
        let max = i32::from(ZOOMS[ZOOMS.len() - 1]);
        self.track_bottom
            - (self.track_bottom - self.track_top) * (i32::from(percent) - min) / (max - min)
    }

    fn percent_at(&self, y: i32) -> u8 {
        let y = y.clamp(self.track_top, self.track_bottom);
        ZOOMS
            .into_iter()
            .min_by_key(|&percent| self.thumb_y(percent).abs_diff(y))
            .unwrap_or(100)
    }
}

pub(super) fn prepare(s: &State) -> Option<Popup> {
    if s.zoom.open {
        Popup::new(s)
    } else {
        None
    }
}

impl State {
    pub(super) fn toggle_zoom(&mut self) {
        self.zoom.open = !self.zoom.open && Popup::new(self).is_some();
        self.zoom.preview = None;
    }

    pub(super) fn zoom_down(&mut self, x: i32, y: i32) -> Option<Action> {
        if contains(status_bar::zoom_anchor(self), x, y) {
            if self.zoom.open || self.pointer_overlays_clear() {
                self.toggle_zoom();
            }
            return Some(Action::None);
        }
        if !self.zoom.open {
            return None;
        }
        if let Some(popup) = prepare(self).filter(|p| contains(p.area, x, y)) {
            if y >= popup.track_top - popup.dpi.pixels(9) {
                self.zoom.preview = Some(popup.percent_at(y));
                return Some(Action::CapturePointer);
            }
        } else {
            self.zoom.open = false;
        }
        Some(Action::None)
    }

    pub(super) fn zoom_drag(&mut self, y: i32, release: bool) -> Action {
        let Some(percent) = prepare(self).map(|popup| popup.percent_at(y)) else {
            self.zoom.preview = None;
            return Action::None;
        };
        if release {
            self.zoom.preview = None;
            self.apply_zoom(percent)
        } else {
            self.zoom.preview = Some(percent);
            Action::None
        }
    }

    fn apply_zoom(&self, percent: u8) -> Action {
        if self.editor_zoom == percent {
            Action::None
        } else {
            Action::Appearance(Choice::Zoom(percent))
        }
    }

    pub(super) fn zoom_step(&self, direction: i32) -> Action {
        let index = ZOOMS.partition_point(|&percent| percent < self.editor_zoom) as i32;
        self.apply_zoom(ZOOMS[(index + direction).clamp(0, ZOOMS.len() as i32 - 1) as usize])
    }

    pub(super) fn zoom_key(&mut self, code: usize) -> Action {
        match code {
            0x26 | 0x27 => self.zoom_step(1),
            0x25 | 0x28 => self.zoom_step(-1),
            0x24 => self.apply_zoom(ZOOMS[0]),
            0x23 => self.apply_zoom(ZOOMS[ZOOMS.len() - 1]),
            0x0d | 0x1b => {
                self.zoom.open = false;
                Action::None
            }
            _ => Action::None,
        }
    }
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateEllipticRgn(left: i32, top: i32, right: i32, bottom: i32) -> Handle;
    fn FillRgn(dc: Hdc, region: Handle, brush: Handle) -> i32;
}

unsafe fn circle(dc: Hdc, x: i32, y: i32, radius: i32, color: u32) -> Result<(), String> {
    let region = CreateEllipticRgn(x - radius, y - radius, x + radius + 1, y + radius + 1);
    let brush = CreateSolidBrush(color);
    let ok = !region.is_null() && !brush.is_null() && FillRgn(dc, region, brush) != 0;
    for object in [region, brush] {
        if !object.is_null() {
            DeleteObject(object);
        }
    }
    if ok {
        Ok(())
    } else {
        Err("No se pudo dibujar el deslizador de zoom".into())
    }
}

pub(super) unsafe fn draw(dc: Hdc, popup: &Popup) -> Result<(), String> {
    let px = |n| popup.dpi.pixels(n);
    let area = popup.area;
    let x = (area.left + area.right) / 2;
    let edge = px(1).max(1);
    paint::fill(dc, area, shell::MUTED)?;
    paint::fill(
        dc,
        Rect {
            left: area.left + edge,
            top: area.top + edge,
            right: area.right - edge,
            bottom: area.bottom - edge,
        },
        shell::RAISED,
    )?;
    status_bar::label_centered(
        dc,
        Rect {
            top: area.top + px(5),
            bottom: area.top + px(36),
            ..area
        },
        &wide(&format!("{} %", popup.percent)),
    )?;
    paint::fill(
        dc,
        Rect {
            left: area.left + px(10),
            right: area.right - px(10),
            top: area.top + px(36),
            bottom: area.top + px(36) + edge,
        },
        shell::SELECTED,
    )?;
    paint::fill(
        dc,
        Rect {
            left: x - edge,
            right: x + edge,
            top: popup.track_top,
            bottom: popup.track_bottom + 1,
        },
        shell::MUTED,
    )?;
    let y = popup.thumb_y(popup.percent);
    circle(dc, x, y, px(8), shell::TEXT)?;
    circle(dc, x, y, px(8) - 2 * edge, shell::RAISED)
}

pub(super) unsafe fn smoke(hwnd: Hwnd, bitmap: Option<&std::path::Path>) -> Result<(), String> {
    use super::{
        owner,
        project_smoke::snapshot,
        smoke::{command, key},
    };
    use jc_app::{CommandId, Modifiers};
    let model = owner(hwnd).ok_or("Falta la ventana de prueba del zoom")?;
    let before = snapshot(hwnd)?;
    let original = model.state.borrow().editor_zoom;
    let anchor = status_bar::zoom_anchor(&model.state.borrow());
    let send_mouse = |message, flags, x: i32, y: i32| {
        SendMessageW(
            hwnd,
            message,
            flags,
            (x as u16 as usize | ((y as u16 as usize) << 16)) as isize,
        );
    };
    let x = (anchor.left + anchor.right) / 2;
    send_mouse(WM_LBUTTONDOWN, 1, x, anchor.top + 2);
    send_mouse(WM_LBUTTONUP, 0, x, anchor.top + 2);
    let popup = prepare(&model.state.borrow()).ok_or("El porcentaje no abrió el deslizador")?;
    if model.state.borrow().selector.is_some()
        || popup.percent != original
        || popup.area.bottom > anchor.top
    {
        return Err("El zoom no abrió junto al porcentaje actual".into());
    }
    if let Some(path) = bitmap {
        let frame = paint::prepare(&mut model.state.borrow_mut());
        paint::bitmap(hwnd, &frame, &path.with_file_name("native-zoom-slider.bmp"))?;
    }
    let x = (popup.area.left + popup.area.right) / 2;
    send_mouse(WM_LBUTTONDOWN, 1, x, popup.thumb_y(original));
    let target = if original == 150 { 75 } else { 150 };
    let y = popup.thumb_y(target);
    send_mouse(WM_MOUSEMOVE, 1, x, y);
    if model.state.borrow().zoom.preview != Some(target)
        || model.state.borrow().editor_zoom != original
    {
        return Err("El arrastre no mostró el porcentaje antes de aplicarlo".into());
    }
    send_mouse(WM_LBUTTONUP, 0, x, y);
    if model.state.borrow().editor_zoom != target || GetCapture() == hwnd {
        return Err("Soltar el deslizador no aplicó el zoom o dejó capturado el ratón".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    command(hwnd, CommandId::ViewEditorZoom)?;
    if !model.state.borrow().zoom.open {
        return Err("El menú Ver no abrió el deslizador".into());
    }
    key(hwnd, 0x26, Modifiers::NONE)?;
    if model.state.borrow().editor_zoom <= target {
        return Err("La flecha arriba no aumentó el zoom".into());
    }
    key(hwnd, 0x0d, Modifiers::NONE)?;
    super::appearance::apply(hwnd, Choice::Zoom(original));
    if model.state.borrow().zoom.open || snapshot(hwnd)? != before {
        return Err("Cerrar el deslizador cambió la vista del documento".into());
    }
    Ok(())
}

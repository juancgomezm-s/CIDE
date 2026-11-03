//! Owned client-area title/menu chrome using documented Win32 frame messages.
//! The real HMENU stays attached for routing/popups; this module paints only
//! immutable UI metadata, never source documents or application state.
use super::{
    ffi::*,
    native_menu::{self, NativeMenuGroup},
    paint, shell, wide,
};
use std::{ffi::c_void, ptr::null_mut, sync::OnceLock};

pub(super) const TITLE_HEIGHT: i32 = 32;
pub(super) const MENU_HEIGHT: i32 = 28;
pub(super) const TOP: i32 = TITLE_HEIGHT + MENU_HEIGHT;
const BUTTON_WIDTH: i32 = 46;
const RESIZE_BORDER: i32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Minimize,
    MaxRestore,
    Exit,
    Menu(NativeMenuGroup),
}
#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
}
impl Layout {
    pub(super) fn new(width: i32, height: i32, maximized: bool) -> Self {
        Self {
            width: width.clamp(1, 8192),
            height: height.clamp(1, 8192),
            maximized,
        }
    }
    fn button(self, action: Action) -> Rect {
        let slot = match action {
            Action::Minimize => 3,
            Action::MaxRestore => 2,
            Action::Exit => 1,
            Action::Menu(_) => 0,
        };
        Rect {
            left: (self.width - slot * BUTTON_WIDTH).max(0),
            right: (self.width - (slot - 1) * BUTTON_WIDTH).clamp(0, self.width),
            top: 0,
            bottom: TITLE_HEIGHT.min(self.height),
        }
    }
}
fn groups() -> &'static [(NativeMenuGroup, &'static str)] {
    static GROUPS: OnceLock<Vec<(NativeMenuGroup, &'static str)>> = OnceLock::new();
    GROUPS.get_or_init(|| {
        native_menu::definitions()
            .into_iter()
            .map(|spec| (spec.group, spec.title))
            .collect()
    })
}
fn preferred_width(group: NativeMenuGroup) -> i32 {
    match group {
        NativeMenuGroup::File => 68,
        NativeMenuGroup::Edit => 60,
        NativeMenuGroup::Search => 64,
        NativeMenuGroup::Project => 80,
        NativeMenuGroup::Tools => 110,
        NativeMenuGroup::View => 44,
        NativeMenuGroup::Navigation => 94,
        NativeMenuGroup::Git => 40,
    }
}
/// The same stable pixel rectangles are used by draw, click and popup anchoring.
/// Compact windows compress padding/labels but retain a target for every group.
pub(super) fn menu_rect(width: i32, group: NativeMenuGroup) -> Rect {
    let width = width.clamp(1, 8192);
    let left = 8.min(width);
    let available = (width - 16).max(0);
    let total = groups()
        .iter()
        .map(|(group, _)| preferred_width(*group))
        .sum::<i32>()
        .max(1);
    let mut from = 0;
    for (candidate, _) in groups() {
        let to = from + preferred_width(*candidate);
        if *candidate == group {
            return Rect {
                left: left + from * available.min(total) / total,
                right: left + to * available.min(total) / total,
                top: TITLE_HEIGHT,
                bottom: TOP,
            };
        }
        from = to;
    }
    Rect {
        left: width,
        right: width,
        top: TITLE_HEIGHT,
        bottom: TOP,
    }
}
fn inside(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}
pub(super) fn action_at(layout: Layout, x: i32, y: i32) -> Option<Action> {
    if x < 0 || x >= layout.width || y < 0 || y >= layout.height {
        return None;
    }
    for action in [Action::Exit, Action::MaxRestore, Action::Minimize] {
        if inside(layout.button(action), x, y) {
            return Some(action);
        }
    }
    if (TITLE_HEIGHT..TOP).contains(&y) {
        return groups()
            .iter()
            .find(|(group, _)| inside(menu_rect(layout.width, *group), x, y))
            .map(|(group, _)| Action::Menu(*group));
    }
    None
}

#[repr(C)]
struct NcCalcSize {
    rectangles: [Rect; 3],
    position: *mut c_void,
}
#[repr(C)]
struct MonitorInfo {
    size: u32,
    monitor: Rect,
    work: Rect,
    flags: u32,
}
#[link(name = "user32")]
extern "system" {
    fn IsZoomed(hwnd: Hwnd) -> i32;
    fn MonitorFromRect(rect: *const Rect, flags: u32) -> Handle;
    fn GetMonitorInfoW(monitor: Handle, info: *mut MonitorInfo) -> i32;
    fn SetWindowPos(
        hwnd: Hwnd,
        after: Hwnd,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    fn DrawTextW(dc: Hdc, text: *const u16, len: i32, rect: *mut Rect, flags: u32) -> i32;
    fn DrawIconEx(
        dc: Hdc,
        x: i32,
        y: i32,
        icon: Handle,
        width: i32,
        height: i32,
        step: u32,
        brush: Handle,
        flags: u32,
    ) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn SaveDC(dc: Hdc) -> i32;
    fn RestoreDC(dc: Hdc, saved: i32) -> i32;
    fn SetBkMode(dc: Hdc, mode: i32) -> i32;
    fn CreatePen(style: i32, width: i32, color: u32) -> Handle;
    fn MoveToEx(dc: Hdc, x: i32, y: i32, previous: *mut Point) -> i32;
    fn LineTo(dc: Hdc, x: i32, y: i32) -> i32;
}

/// Root should return zero from WM_NCCALCSIZE when this returns true. This keeps
/// the client area over the system caption/menu while retaining the window's
/// ordinary style and attached HMENU. Maximized content is clipped to rcWork,
/// removing the invisible resize frame outside the monitor's usable rectangle.
pub(super) unsafe fn calc_client(hwnd: Hwnd, wparam: usize, lparam: isize) -> bool {
    if lparam == 0 {
        return false;
    }
    let rect = if wparam != 0 {
        &mut (*(lparam as *mut NcCalcSize)).rectangles[0]
    } else {
        &mut *(lparam as *mut Rect)
    };
    if IsZoomed(hwnd) != 0 {
        let monitor = MonitorFromRect(rect, 2);
        let mut info = MonitorInfo {
            size: std::mem::size_of::<MonitorInfo>() as u32,
            monitor: Rect::default(),
            work: Rect::default(),
            flags: 0,
        };
        if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 {
            let work = info.work;
            rect.left = rect.left.max(work.left);
            rect.top = rect.top.max(work.top);
            rect.right = rect.right.min(work.right).max(rect.left);
            rect.bottom = rect.bottom.min(work.bottom).max(rect.top);
        }
    }
    true
}
pub(super) unsafe fn apply_frame(hwnd: Hwnd) -> Result<(), String> {
    if SetWindowPos(hwnd, null_mut(), 0, 0, 0, 0, 0x1 | 0x2 | 0x4 | 0x10 | 0x20) == 0 {
        Err("No se pudo aplicar el marco gráfico de CIDE".into())
    } else {
        Ok(())
    }
}
fn local_hit(layout: Layout, x: i32, y: i32) -> isize {
    if x < 0 || y < 0 || x >= layout.width || y >= layout.height {
        return 0;
    }
    if !layout.maximized {
        let left = x < RESIZE_BORDER;
        let right = x >= layout.width - RESIZE_BORDER;
        let top = y < RESIZE_BORDER;
        let bottom = y >= layout.height - RESIZE_BORDER;
        match (left, right, top, bottom) {
            (true, _, true, _) => return 13,
            (_, true, true, _) => return 14,
            (true, _, _, true) => return 16,
            (_, true, _, true) => return 17,
            (true, _, _, _) => return 10,
            (_, true, _, _) => return 11,
            (_, _, true, _) => return 12,
            (_, _, _, true) => return 15,
            _ => {}
        }
    }
    if y < TITLE_HEIGHT && action_at(layout, x, y).is_none() {
        2
    } else {
        1
    }
}
/// The OS performs caption dragging, maximize-on-double-click and edge resizing;
/// no mouse capture loop or hand-written move/size behavior is introduced.
pub(super) unsafe fn hit_test(hwnd: Hwnd, lparam: isize, layout: Layout) -> isize {
    let mut point = Point {
        x: (lparam as u16) as i16 as i32,
        y: ((lparam as usize >> 16) as u16) as i16 as i32,
    };
    if ScreenToClient(hwnd, &mut point) == 0 {
        return 0;
    }
    local_hit(layout, point.x, point.y)
}

struct MenuLabel {
    rect: Rect,
    text: Vec<u16>,
    action: Action,
}
pub(super) struct Frame {
    pub layout: Layout,
    font: Handle,
    icon: Handle,
    title: Vec<u16>,
    focused: bool,
    hover: Option<Action>,
    menus: Vec<MenuLabel>,
}
impl Frame {
    /// `title` is a complete UI title, normally "CIDE · filename". It is bounded
    /// and sanitized before drawing; document/source access belongs to the owner.
    pub(super) fn new(
        layout: Layout,
        font: Handle,
        icon: Handle,
        title: &str,
        focused: bool,
        hover: Option<Action>,
    ) -> Self {
        let title = jc_app::source_services::display_ui_label(title, 128);
        Self {
            layout,
            font,
            icon,
            title: wide(if title.is_empty() { "CIDE" } else { &title }),
            focused,
            hover,
            menus: groups()
                .iter()
                .map(|(group, title)| MenuLabel {
                    rect: menu_rect(layout.width, *group),
                    text: wide(title),
                    action: Action::Menu(*group),
                })
                .collect(),
        }
    }
}
unsafe fn line(dc: Hdc, from: (i32, i32), to: (i32, i32)) -> Result<(), String> {
    if MoveToEx(dc, from.0, from.1, null_mut()) == 0 || LineTo(dc, to.0, to.1) == 0 {
        Err("No se pudo dibujar un control del marco".into())
    } else {
        Ok(())
    }
}
pub(super) unsafe fn draw(dc: Hdc, frame: &Frame) -> Result<(), String> {
    let saved = SaveDC(dc);
    if saved == 0 {
        return Err("No se pudo preservar el contexto del marco".into());
    }
    let result = (|| -> Result<(), String> {
        paint::fill(
            dc,
            Rect {
                left: 0,
                top: 0,
                right: frame.layout.width,
                bottom: TOP.min(frame.layout.height),
            },
            shell::SURFACE,
        )?;
        SelectObject(dc, frame.font);
        SetBkMode(dc, 1);
        SetTextColor(
            dc,
            if frame.focused {
                shell::TEXT
            } else {
                shell::MUTED
            },
        );
        if !frame.icon.is_null()
            && frame.layout.width > 180
            && DrawIconEx(dc, 11, 8, frame.icon, 16, 16, 0, null_mut(), 3) == 0
        {
            return Err("No se pudo dibujar el icono del marco".into());
        }
        let mut title = Rect {
            left: 36,
            top: 0,
            right: (frame.layout.width - 3 * BUTTON_WIDTH - 10).max(36),
            bottom: TITLE_HEIGHT.min(frame.layout.height),
        };
        if title.right > title.left
            && DrawTextW(
                dc,
                frame.title.as_ptr(),
                (frame.title.len() - 1) as i32,
                &mut title,
                0x20 | 4 | 0x800 | 0x8000,
            ) == 0
        {
            return Err("No se pudo dibujar el título de CIDE".into());
        }
        let pen = CreatePen(0, 1, shell::TEXT);
        if pen.is_null() {
            return Err("No se pudieron preparar los controles del marco".into());
        }
        let previous = SelectObject(dc, pen);
        let controls = (|| -> Result<(), String> {
            for action in [Action::Minimize, Action::MaxRestore, Action::Exit] {
                let rect = frame.layout.button(action);
                if rect.right <= rect.left {
                    continue;
                }
                if frame.hover == Some(action) {
                    paint::fill(
                        dc,
                        rect,
                        if action == Action::Exit {
                            0x003b3bbf
                        } else {
                            shell::SELECTED
                        },
                    )?;
                }
                let x = (rect.left + rect.right) / 2;
                let y = TITLE_HEIGHT / 2;
                match action {
                    Action::Minimize => line(dc, (x - 5, y + 3), (x + 6, y + 3))?,
                    Action::Exit => {
                        line(dc, (x - 4, y - 4), (x + 5, y + 5))?;
                        line(dc, (x - 4, y + 4), (x + 5, y - 5))?;
                    }
                    Action::MaxRestore => {
                        let (left, top, size) = if frame.layout.maximized {
                            (x - 3, y - 5, 8)
                        } else {
                            (x - 5, y - 4, 10)
                        };
                        line(dc, (left, top), (left + size, top))?;
                        line(dc, (left + size, top), (left + size, top + 8))?;
                        line(dc, (left + size, top + 8), (left, top + 8))?;
                        line(dc, (left, top + 8), (left, top))?;
                        if frame.layout.maximized {
                            line(dc, (x - 5, y - 2), (x - 5, y + 5))?;
                            line(dc, (x - 5, y + 5), (x + 2, y + 5))?;
                        }
                    }
                    Action::Menu(_) => {}
                }
            }
            Ok(())
        })();
        SelectObject(dc, previous);
        DeleteObject(pen);
        controls?;
        SetTextColor(dc, shell::TEXT);
        for menu in &frame.menus {
            if menu.rect.right <= menu.rect.left {
                continue;
            }
            if frame.hover == Some(menu.action) {
                paint::fill(dc, menu.rect, shell::SELECTED)?;
            }
            let mut rect = Rect {
                left: menu.rect.left + 8,
                right: (menu.rect.right - 6).max(menu.rect.left + 8),
                ..menu.rect
            };
            if rect.right > rect.left
                && DrawTextW(
                    dc,
                    menu.text.as_ptr(),
                    (menu.text.len() - 1) as i32,
                    &mut rect,
                    0x20 | 4 | 0x8000,
                ) == 0
            {
                return Err("No se pudo dibujar la barra de menús de CIDE".into());
            }
        }
        Ok(())
    })();
    let restored = RestoreDC(dc, saved);
    result?;
    if restored == 0 {
        Err("No se pudo restaurar el contexto del marco".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chrome_has_owned_title_controls_and_menu_targets_at_normal_and_compact_widths() {
        for width in [320, 640, 1400] {
            let layout = Layout::new(width, 600, false);
            assert_eq!(action_at(layout, width - 20, 16), Some(Action::Exit));
            assert_eq!(action_at(layout, width - 69, 16), Some(Action::MaxRestore));
            assert_eq!(action_at(layout, width - 115, 16), Some(Action::Minimize));
            for (group, _) in groups() {
                let rect = menu_rect(width, *group);
                assert!(rect.left < rect.right && rect.left >= 0 && rect.right <= width);
                assert_eq!(
                    action_at(layout, (rect.left + rect.right) / 2, 46),
                    Some(Action::Menu(*group))
                );
            }
            assert_eq!(action_at(layout, 20, TOP + 10), None);
        }
    }
    #[test]
    fn system_caption_and_resize_hits_are_distinct_from_client_buttons_and_editor() {
        let normal = Layout::new(1000, 650, false);
        assert_eq!(local_hit(normal, 1, 1), 13);
        assert_eq!(local_hit(normal, 999, 1), 14);
        assert_eq!(local_hit(normal, 1, 649), 16);
        assert_eq!(local_hit(normal, 999, 649), 17);
        assert_eq!(local_hit(normal, 500, 1), 12);
        assert_eq!(local_hit(normal, 500, 649), 15);
        assert_eq!(local_hit(normal, 100, 16), 2);
        assert_eq!(local_hit(normal, 980, 16), 1);
        assert_eq!(local_hit(normal, 100, 45), 1);
        assert_eq!(local_hit(normal, 100, 100), 1);
        let maximized = Layout::new(1000, 650, true);
        assert_eq!(local_hit(maximized, 1, 1), 2);
        assert_eq!(local_hit(maximized, 1, 649), 1);
    }
}

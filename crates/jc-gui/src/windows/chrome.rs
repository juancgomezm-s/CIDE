//! Owned client-area title/menu chrome using documented Win32 frame messages.
//! The real HMENU stays attached for routing/popups; this module paints only
//! immutable UI metadata, never source documents or application state.
use super::{
    dpi::Dpi,
    ffi::*,
    native_menu::{self, NativeMenuGroup},
    paint, shell, wide,
};
use std::{ffi::c_void, ptr::null_mut, sync::OnceLock};

pub(super) const TOP: i32 = 32;
const BUTTON_WIDTH: i32 = 46;
// Keep an accessible inside edge after extending the client over the native
// frame. Five pixels were too narrow to reliably acquire at normal scale.
const RESIZE_BORDER: i32 = 8;

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
    pub dpi: Dpi,
}
impl Layout {
    #[cfg(test)]
    pub(super) fn new(width: i32, height: i32, maximized: bool) -> Self {
        Self::at_dpi(width, height, maximized, Dpi::BASE)
    }
    pub(super) fn at_dpi(width: i32, height: i32, maximized: bool, dpi: Dpi) -> Self {
        Self {
            width: width.clamp(1, 8192),
            height: height.clamp(1, 8192),
            maximized,
            dpi,
        }
    }
    pub(super) fn top(self) -> i32 {
        self.dpi.pixels(TOP)
    }
    fn button(self, action: Action) -> Rect {
        let slot = match action {
            Action::Minimize => 3,
            Action::MaxRestore => 2,
            Action::Exit => 1,
            Action::Menu(_) => 0,
        };
        let button_width = self.dpi.pixels(BUTTON_WIDTH);
        Rect {
            left: (self.width - slot * button_width).max(0),
            right: (self.width - (slot - 1) * button_width).clamp(0, self.width),
            top: 0,
            bottom: self.top().min(self.height),
        }
    }

    fn menu_space(self) -> (i32, i32, MenuDensity) {
        let px = |value| self.dpi.pixels(value);
        let controls = self.button(Action::Minimize).left;
        let left = px(32).min(controls);
        let available = (controls - left - px(40)).max(0);
        let full = groups()
            .iter()
            .map(|(group, _)| px(preferred_width(*group)))
            .sum();
        let compact = px(54) * groups().len() as i32;
        let (width, density) = if available >= full {
            (full, MenuDensity::Full)
        } else if available >= compact {
            (compact, MenuDensity::Compact)
        } else {
            (available, MenuDensity::Initials)
        };
        (left, width, density)
    }

    pub(super) fn title_rect(self) -> Rect {
        let (menu_left, menu_width, _) = self.menu_space();
        Rect {
            left: menu_left + menu_width + self.dpi.pixels(8),
            right: self.button(Action::Minimize).left - self.dpi.pixels(8),
            top: 0,
            bottom: self.top().min(self.height),
        }
    }
}

#[derive(Clone, Copy)]
enum MenuDensity {
    Full,
    Compact,
    Initials,
}

fn menu_caption(group: NativeMenuGroup, title: &'static str, density: MenuDensity) -> &'static str {
    use NativeMenuGroup::*;
    match density {
        MenuDensity::Full => title,
        MenuDensity::Compact => match group {
            File => "&Arch.",
            Edit => "&Edit.",
            Search => "&Buscar",
            Project => "&Proy.",
            Tools => "&Herr.",
            View => "&Ver",
            Navigation => "&Nav.",
            Git => "&Git",
        },
        MenuDensity::Initials => title.get(..2).unwrap_or(title),
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
pub(super) fn menu_rect(layout: Layout, group: NativeMenuGroup) -> Rect {
    let (left, width, density) = layout.menu_space();
    let mut offset = 0;
    for (index, (candidate, _)) in groups().iter().enumerate() {
        let end = match density {
            MenuDensity::Full => offset + layout.dpi.pixels(preferred_width(*candidate)),
            _ => width * (index as i32 + 1) / groups().len() as i32,
        };
        if *candidate == group {
            return Rect {
                left: left + offset,
                right: left + end,
                top: 0,
                bottom: layout.top().min(layout.height),
            };
        }
        offset = end;
    }
    Rect::default()
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
    if y < layout.top() {
        return groups()
            .iter()
            .find(|(group, _)| inside(menu_rect(layout, *group), x, y))
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
        let border = layout.dpi.pixels(RESIZE_BORDER);
        let left = x < border;
        let right = x >= layout.width - border;
        let top = y < border;
        let bottom = y >= layout.height - border;
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
    if y < layout.top() && action_at(layout, x, y).is_none() {
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
    // Hit testing must use the current HWND, not the retained painting geometry.
    // Windows can ask for hits while size/restore messages are still being
    // processed, before the next prepared presentation catches up.
    let mut client = Rect::default();
    let layout = if GetClientRect(hwnd, &mut client) != 0 {
        Layout::at_dpi(
            client.right - client.left,
            client.bottom - client.top,
            IsZoomed(hwnd) != 0,
            layout.dpi,
        )
    } else {
        layout
    };
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
                    rect: menu_rect(layout, *group),
                    text: wide(menu_caption(*group, title, layout.menu_space().2)),
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
        let px = |value| frame.layout.dpi.pixels(value);
        paint::fill(
            dc,
            Rect {
                left: 0,
                top: 0,
                right: frame.layout.width,
                bottom: frame.layout.top().min(frame.layout.height),
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
            && frame.layout.width > px(180)
            && DrawIconEx(
                dc,
                px(11),
                px(8),
                frame.icon,
                px(16),
                px(16),
                0,
                null_mut(),
                3,
            ) == 0
        {
            return Err("No se pudo dibujar el icono del marco".into());
        }
        let mut title = frame.layout.title_rect();
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
        let pen = CreatePen(0, px(1).max(1), shell::TEXT);
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
                let y = frame.layout.top() / 2;
                match action {
                    Action::Minimize => line(dc, (x - px(5), y + px(3)), (x + px(6), y + px(3)))?,
                    Action::Exit => {
                        line(dc, (x - px(4), y - px(4)), (x + px(5), y + px(5)))?;
                        line(dc, (x - px(4), y + px(4)), (x + px(5), y - px(5)))?;
                    }
                    Action::MaxRestore => {
                        let (left, top, size) = if frame.layout.maximized {
                            (x - px(3), y - px(5), px(8))
                        } else {
                            (x - px(5), y - px(4), px(10))
                        };
                        line(dc, (left, top), (left + size, top))?;
                        line(dc, (left + size, top), (left + size, top + px(8)))?;
                        line(dc, (left + size, top + px(8)), (left, top + px(8)))?;
                        line(dc, (left, top + px(8)), (left, top))?;
                        if frame.layout.maximized {
                            line(dc, (x - px(5), y - px(2)), (x - px(5), y + px(5)))?;
                            line(dc, (x - px(5), y + px(5)), (x + px(2), y + px(5)))?;
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
            let mut rect = menu.rect;
            if rect.right > rect.left
                && DrawTextW(
                    dc,
                    menu.text.as_ptr(),
                    (menu.text.len() - 1) as i32,
                    &mut rect,
                    0x20 | 4 | 1 | 0x8000,
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
    #[link(name = "user32")]
    extern "system" {
        fn ClientToScreen(hwnd: Hwnd, point: *mut Point) -> i32;
    }

    #[test]
    fn native_resize_hit_uses_current_window_instead_of_stale_paint_size() {
        struct Window(Hwnd);
        impl Drop for Window {
            fn drop(&mut self) {
                unsafe {
                    DestroyWindow(self.0);
                }
            }
        }
        unsafe {
            // Built-in STATIC is a hidden, owned geometry fixture. No editor,
            // filesystem, source document or desktop input is involved.
            let window = Window(CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("CIDE resize fixture").as_ptr(),
                0x00cf0000,
                0,
                0,
                800,
                600,
                null_mut(),
                null_mut(),
                GetModuleHandleW(std::ptr::null()),
                null_mut(),
            ));
            assert!(!window.0.is_null());
            let stale = Layout::new(20, 20, true);
            for (width, height) in [(800, 600), (1000, 750)] {
                assert_ne!(
                    SetWindowPos(window.0, null_mut(), 0, 0, width, height, 0x16),
                    0
                );
                let mut client = Rect::default();
                assert_ne!(GetClientRect(window.0, &mut client), 0);
                for (x, y, expected) in [
                    (client.right - 1, client.bottom / 2, 11),
                    (client.right / 2, client.bottom - 1, 15),
                    (client.right - 1, client.bottom - 1, 17),
                ] {
                    let mut point = Point { x, y };
                    assert_ne!(ClientToScreen(window.0, &mut point), 0);
                    let packed =
                        (((point.y as u16 as usize) << 16) | point.x as u16 as usize) as isize;
                    assert_eq!(hit_test(window.0, packed, stale), expected);
                }
            }
        }
    }

    #[test]
    fn all_resize_edges_remain_acquirable_at_each_dpi_and_stop_when_maximized() {
        for raw in [96, 144, 192] {
            let dpi = Dpi::from_raw(raw);
            let layout = Layout::at_dpi(1600, 1000, false, dpi);
            // Test the inner edge of each resize target, not only outer pixels.
            let edge = dpi.pixels(RESIZE_BORDER) - 1;
            let points = [
                (edge, 500, 10),
                (1599 - edge, 500, 11),
                (800, edge, 12),
                (800, 999 - edge, 15),
                (edge, edge, 13),
                (1599 - edge, edge, 14),
                (edge, 999 - edge, 16),
                (1599 - edge, 999 - edge, 17),
            ];
            for (x, y, expected) in points {
                assert_eq!(local_hit(layout, x, y), expected, "DPI {raw} at {x},{y}");
                let maximized = Layout {
                    maximized: true,
                    ..layout
                };
                assert!(!matches!(local_hit(maximized, x, y), 10..=17));
            }
            let outside = dpi.pixels(RESIZE_BORDER);
            assert_eq!(local_hit(layout, outside, 500), 1);
            assert_eq!(local_hit(layout, 1599 - outside, 500), 1);
            assert_eq!(local_hit(layout, 800, 999 - outside), 1);
            let title = layout.title_rect();
            assert_eq!(
                local_hit(layout, (title.left + title.right) / 2, outside),
                2
            );
            assert_eq!(local_hit(layout, -1, 500), 0);
            assert_eq!(local_hit(layout, 1600, 500), 0);
        }
    }
    #[test]
    fn scaled_chrome_keeps_draw_hit_and_popup_origins_aligned() {
        for dpi in [96, 144, 192] {
            let layout = Layout::at_dpi(1400, 900, false, Dpi::from_raw(dpi));
            assert_eq!(layout.top(), 32 * dpi as i32 / 96);
            let button = layout.button(Action::Exit);
            let (x, y) = ((button.left + button.right) / 2, button.bottom / 2);
            assert_eq!(action_at(layout, x, y), Some(Action::Exit));
            assert_eq!(local_hit(layout, x, y), 1);
            assert_eq!(local_hit(layout, 1, 1), 13);
            for (group, _) in groups() {
                let rect = menu_rect(layout, *group);
                assert_eq!(rect.top, 0);
                assert_eq!(rect.bottom, layout.top());
                assert!(rect.right <= layout.title_rect().left);
                let x = (rect.left + rect.right) / 2;
                let y = (rect.top + rect.bottom) / 2;
                assert_eq!(action_at(layout, x, y), Some(Action::Menu(*group)));
                assert_eq!(local_hit(layout, x, y), 1);
            }
        }
    }
    #[test]
    fn chrome_has_owned_title_controls_and_menu_targets_at_normal_and_compact_widths() {
        for width in [320, 640, 1400] {
            let layout = Layout::new(width, 600, false);
            assert_eq!(action_at(layout, width - 20, 16), Some(Action::Exit));
            assert_eq!(action_at(layout, width - 69, 16), Some(Action::MaxRestore));
            assert_eq!(action_at(layout, width - 115, 16), Some(Action::Minimize));
            for (group, _) in groups() {
                let rect = menu_rect(layout, *group);
                assert!(rect.left < rect.right && rect.left >= 0 && rect.right <= width);
                assert_eq!(
                    action_at(layout, (rect.left + rect.right) / 2, 16),
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
        assert_eq!(local_hit(normal, 800, 16), 2);
        assert_eq!(local_hit(normal, 100, 16), 1);
        assert_eq!(local_hit(normal, 980, 16), 1);
        assert_eq!(local_hit(normal, 100, 45), 1);
        assert_eq!(local_hit(normal, 100, 100), 1);
        let maximized = Layout::new(1000, 650, true);
        assert_eq!(local_hit(maximized, 1, 1), 2);
        assert_eq!(local_hit(maximized, 1, 649), 1);
    }
}

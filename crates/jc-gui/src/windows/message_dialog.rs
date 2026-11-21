//! Application-owned modal confirmation, painted with the current CIDE palette.
//! The native dialog manager owns keyboard traversal and parent-window modality.
use super::{dpi, ffi::*, paint, shell, wide};
use std::cell::Cell;

pub(super) mod form;
pub(super) mod records;
pub(super) mod sql_records;

#[repr(C)]
struct DrawItem {
    control_type: u32,
    control_id: u32,
    item_id: u32,
    action: u32,
    state: u32,
    window: Hwnd,
    dc: Hdc,
    rect: Rect,
    data: usize,
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
    fn DialogBoxIndirectParamW(
        instance: Handle,
        template: *const u32,
        parent: Hwnd,
        procedure: Option<unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize>,
        parameter: isize,
    ) -> isize;
    fn EndDialog(hwnd: Hwnd, result: isize) -> i32;
    fn GetDlgItem(hwnd: Hwnd, id: i32) -> Hwnd;
    fn GetFocus() -> Hwnd;
    fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn MonitorFromWindow(hwnd: Hwnd, flags: u32) -> Handle;
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
    fn DrawTextW(dc: Hdc, text: *const u16, count: i32, rect: *mut Rect, flags: u32) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn SetBkMode(dc: Hdc, mode: i32) -> i32;
}

struct Dialog<'a> {
    parent: Hwnd,
    title: &'a str,
    message: &'a str,
    accept: &'a str,
    scrollable: bool,
    information: bool,
    ui_size: u8,
    theme: super::appearance::Theme,
    font: Handle,
    brush: Handle,
    failed: Cell<bool>,
    #[cfg(test)]
    answer: Option<bool>,
    #[cfg(test)]
    initial_cancel: Cell<bool>,
    #[cfg(test)]
    themed_controls: Cell<bool>,
}

fn centered(parent: Rect, work: Rect, dpi: dpi::Dpi, size: [i32; 2]) -> Rect {
    let width = dpi.pixels(size[0]).min((work.right - work.left).max(1));
    let height = dpi.pixels(size[1]).min((work.bottom - work.top).max(1));
    let left = (parent.left + (parent.right - parent.left - width) / 2)
        .clamp(work.left, work.right - width);
    let top = (parent.top + (parent.bottom - parent.top - height) / 2)
        .clamp(work.top, work.bottom - height);
    Rect {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

unsafe fn position(hwnd: Hwnd, owner: Hwnd, size: [i32; 2]) -> Option<(dpi::Dpi, i32, i32)> {
    let mut parent = Rect::default();
    let reference = if owner.is_null() { hwnd } else { owner };
    let mut info = MonitorInfo {
        size: std::mem::size_of::<MonitorInfo>() as u32,
        monitor: Rect::default(),
        work: Rect::default(),
        flags: 0,
    };
    if GetWindowRect(reference, &mut parent) == 0
        || GetMonitorInfoW(MonitorFromWindow(reference, 2), &mut info) == 0
    {
        return None;
    }
    let scale = dpi::for_window(reference);
    let bounds = centered(parent, info.work, scale, size);
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    if SetWindowPos(
        hwnd,
        std::ptr::null_mut(),
        bounds.left,
        bounds.top,
        width,
        height,
        0x14,
    ) == 0
    {
        return None;
    }
    Some((scale, width, height))
}

unsafe fn arrange(hwnd: Hwnd, dialog: &Dialog<'_>) -> bool {
    let size = if dialog.information {
        [500, 330]
    } else if dialog.scrollable {
        [620, 350]
    } else {
        [420, 176]
    };
    let Some((scale, width, height)) = position(hwnd, dialog.parent, size) else {
        return false;
    };
    if dialog.scrollable && (width < scale.pixels(300) || height < scale.pixels(216)) {
        return false;
    }
    let margin = scale.pixels(16);
    let gap = scale.pixels(12);
    let button = scale.pixels(if dialog.scrollable { 154 } else { 104 });
    let button_height = scale.pixels(32);
    let bottom = height - margin - button_height;
    for (id, x, y, w, h) in [
        (
            100,
            margin,
            scale.pixels(48),
            width - margin * 2,
            bottom - scale.pixels(60),
        ),
        (
            1,
            width - margin - button * 2 - gap,
            bottom,
            button,
            button_height,
        ),
        (2, width - margin - button, bottom, button, button_height),
    ] {
        if SetWindowPos(GetDlgItem(hwnd, id), std::ptr::null_mut(), x, y, w, h, 0x14) == 0 {
            return false;
        }
    }
    if dialog.information {
        ShowWindow(GetDlgItem(hwnd, 2), 0);
        ShowWindow(GetDlgItem(hwnd, 100), 0);
        SetWindowPos(
            GetDlgItem(hwnd, 1),
            std::ptr::null_mut(),
            width - margin - button,
            bottom,
            button,
            button_height,
            0x14,
        );
    }
    true
}
impl Drop for Dialog<'_> {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.font);
            DeleteObject(self.brush);
        }
    }
}

fn control(words: &mut Vec<u16>, id: u16, bounds: [u16; 4], class: u16, text: &str) {
    let style = if class == 0x80 { 0x1000b } else { 0 };
    styled_control(words, id, bounds, class, text, style);
}

fn styled_control(
    words: &mut Vec<u16>,
    id: u16,
    bounds: [u16; 4],
    class: u16,
    text: &str,
    style: u32,
) {
    if words.len() % 2 != 0 {
        words.push(0);
    }
    let style = 0x5000_0000 | style;
    words.extend([style as u16, (style >> 16) as u16, 0, 0]);
    words.extend(bounds);
    words.extend([id, 0xffff, class]);
    words.extend(wide(text));
    words.push(0);
}

fn template(dialog: &Dialog<'_>) -> Vec<u32> {
    let mut words = template_header(dialog.title, 3);
    if dialog.scrollable {
        // Read-only multiline edit with native text selection and vertical scrolling.
        styled_control(
            &mut words,
            100,
            [12, 32, 296, 60],
            0x81,
            dialog.message,
            0xa1_2804,
        );
    } else {
        control(&mut words, 100, [12, 32, 296, 60], 0x82, dialog.message);
    }
    control(&mut words, 1, [132, 106, 80, 22], 0x80, dialog.accept);
    control(&mut words, 2, [220, 106, 80, 22], 0x80, "Cancelar");
    pack_template(words)
}

fn template_header(title: &str, controls: u16) -> Vec<u16> {
    // Standard DLGTEMPLATE followed by DWORD-aligned DLGITEMTEMPLATE records.
    // A Vec<u32> guarantees the alignment required by the dialog manager.
    let style = 0x8008_0800u32; // POPUP | SYSMENU | CENTER; all chrome uses CIDE paint.
    let mut words = vec![
        style as u16,
        (style >> 16) as u16,
        0,
        0,
        controls,
        0,
        0,
        320,
        140,
        0,
        0,
    ];
    words.extend(wide(title));
    words
}

fn pack_template(mut words: Vec<u16>) -> Vec<u32> {
    if words.len() % 2 != 0 {
        words.push(0);
    }
    words
        .chunks_exact(2)
        .map(|pair| u32::from(pair[0]) | (u32::from(pair[1]) << 16))
        .collect()
}

unsafe fn paint_button(item: &DrawItem, dialog: &Dialog<'_>) -> Result<(), String> {
    paint_button_label(
        item,
        dialog,
        if item.control_id == 1 {
            dialog.accept
        } else {
            "Cancelar"
        },
    )
}

unsafe fn paint_button_label(
    item: &DrawItem,
    dialog: &Dialog<'_>,
    label: &str,
) -> Result<(), String> {
    let background = if item.state & (1 | 16) != 0 {
        shell::SELECTED
    } else {
        shell::BACKGROUND
    };
    paint::fill(item.dc, item.rect, background)?;
    let previous = SelectObject(item.dc, dialog.font);
    let mode = SetBkMode(item.dc, 1);
    SetTextColor(
        item.dc,
        paint::ui_ink(if item.state & 4 != 0 {
            shell::MUTED
        } else {
            shell::TEXT
        }),
    );
    let label = wide(label);
    let mut rect = item.rect;
    let drawn = DrawTextW(item.dc, label.as_ptr(), -1, &mut rect, 1 | 4 | 0x20 | 0x800);
    SetBkMode(item.dc, mode);
    SelectObject(item.dc, previous);
    if drawn == 0 {
        Err("No se pudo dibujar el botón del mensaje".into())
    } else {
        Ok(())
    }
}

unsafe fn paint_frame(hwnd: Hwnd, dialog: &Dialog<'_>) {
    let mut ps: PaintStruct = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    if !dc.is_null() {
        let mut client = Rect::default();
        GetClientRect(hwnd, &mut client);
        let height = dpi::for_window(hwnd).pixels(32).min(client.bottom);
        let previous = SelectObject(dc, dialog.font);
        let result = paint::fill(dc, client, shell::RAISED).and_then(|_| {
            paint::fill(
                dc,
                Rect {
                    bottom: height,
                    ..client
                },
                shell::SELECTED,
            )?;
            // A light outline separates the raised modal from the editor.
            for edge in [
                Rect {
                    bottom: 1,
                    ..client
                },
                Rect {
                    top: client.bottom - 1,
                    ..client
                },
                Rect { right: 1, ..client },
                Rect {
                    left: client.right - 1,
                    ..client
                },
            ] {
                paint::fill(dc, edge, shell::MUTED)?;
            }
            paint::text(
                dc,
                Rect {
                    left: 12,
                    right: client.right - 12,
                    top: 8,
                    bottom: height,
                },
                &wide(dialog.title),
                shell::TEXT,
                shell::SELECTED,
            )?;
            if dialog.information {
                paint_about(dc, client, dialog)?;
            }
            Ok(())
        });
        if result.is_err() {
            dialog.failed.set(true);
        }
        SelectObject(dc, previous);
        EndPaint(hwnd, &ps);
    } else {
        dialog.failed.set(true);
    }
}

unsafe fn paint_about(dc: Hdc, bounds: Rect, dialog: &Dialog<'_>) -> Result<(), String> {
    let scale = dpi::for_window(dialog.parent);
    let px = |n| scale.pixels(n);
    let icon = LoadIconW(GetModuleHandleW(std::ptr::null()), 101usize as *const u16);
    if !icon.is_null() {
        DrawIconEx(
            dc,
            px(28),
            px(58),
            icon,
            px(48),
            px(48),
            0,
            std::ptr::null_mut(),
            3,
        );
    }
    let heading = dpi::ui_font_size(scale, 28);
    let previous = SelectObject(dc, heading);
    paint::text(
        dc,
        Rect {
            left: px(94),
            top: px(52),
            right: bounds.right - px(24),
            bottom: px(91),
        },
        &wide("CIDE"),
        shell::TEXT,
        shell::RAISED,
    )?;
    SelectObject(dc, dialog.font);
    paint::text(
        dc,
        Rect {
            left: px(95),
            top: px(94),
            right: bounds.right - px(24),
            bottom: px(115),
        },
        &wide(&format!("Versión {}", jc_app::about::VERSION)),
        shell::ACCENT,
        shell::RAISED,
    )?;
    paint::text(
        dc,
        Rect {
            left: px(28),
            top: px(140),
            right: bounds.right - px(24),
            bottom: px(163),
        },
        &wide(jc_app::about::OWNER),
        shell::TEXT,
        shell::RAISED,
    )?;
    paint::text(
        dc,
        Rect {
            left: px(28),
            top: px(165),
            right: bounds.right - px(24),
            bottom: px(185),
        },
        &wide("Autor y titular de CIDE"),
        shell::MUTED,
        shell::RAISED,
    )?;
    paint::fill(
        dc,
        Rect {
            left: px(28),
            top: px(203),
            right: bounds.right - px(28),
            bottom: px(204),
        },
        shell::SURFACE,
    )?;
    for (i, line) in [
        format!("Compilación · {}", jc_app::about::REVISION),
        format!(
            "{} · {}  ·  Licencia MIT",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    ]
    .iter()
    .enumerate()
    {
        let top = px(216 + i as i32 * 23);
        paint::text(
            dc,
            Rect {
                left: px(28),
                top,
                right: bounds.right - px(28),
                bottom: top + px(21),
            },
            &wide(line),
            shell::MUTED,
            shell::RAISED,
        )?;
    }
    SelectObject(dc, previous);
    DeleteObject(heading);
    Ok(())
}

unsafe extern "system" fn procedure(hwnd: Hwnd, message: u32, wp: usize, lp: isize) -> isize {
    if message == 0x110 {
        SetWindowLongPtrW(hwnd, -21, lp);
        let dialog = &*(lp as *const Dialog<'_>);
        shell::set_caption_theme(hwnd, dialog.theme);
        if !arrange(hwnd, dialog) {
            dialog.failed.set(true);
            EndDialog(hwnd, 0);
            return 0;
        }
        for id in [100, 1, 2] {
            SendMessageW(GetDlgItem(hwnd, id), 0x30, dialog.font as usize, 0);
        }
        SetFocus(GetDlgItem(hwnd, if dialog.information { 1 } else { 2 }));
        if dialog.information {
            SendMessageW(hwnd, 0x401, 1, 0);
        }
        #[cfg(test)]
        {
            let default = SendMessageW(hwnd, 0x400, 0, 0) as usize & 0xffff;
            dialog.initial_cancel.set(default == 2);
            if let Some(answer) = dialog.answer {
                let details = GetDlgItem(hwnd, 100);
                let controls_match = !dialog.scrollable || {
                    let style = GetWindowLongPtrW(details, -16);
                    let mut text = vec![0; dialog.message.encode_utf16().count() + 1];
                    let len = GetWindowTextW(details, text.as_mut_ptr(), text.len() as i32);
                    style & 0x20_0804 == 0x20_0804
                        && String::from_utf16_lossy(&text[..len as usize]) == dialog.message
                };
                dialog.themed_controls.set(
                    controls_match
                        && [1, 2]
                            .into_iter()
                            .all(|id| GetWindowLongPtrW(GetDlgItem(hwnd, id), -16) & 0xf == 0xb)
                        && [100, 1, 2].into_iter().all(|id| {
                            SendMessageW(GetDlgItem(hwnd, id), 0x31, 0, 0) == dialog.font as isize
                        }),
                );
                ShowWindow(hwnd, 0);
                PostMessageW(hwnd, 0x111, if answer { 1 } else { default }, 0);
            }
        }
        return 0;
    }
    let data = GetWindowLongPtrW(hwnd, -21);
    if data == 0 {
        return 0;
    }
    let dialog = &*(data as *const Dialog<'_>);
    let _colors = paint::UiColors::enter(dialog.theme);
    match message {
        0x111 if matches!(wp & 0xffff, 1 | 2) => {
            EndDialog(hwnd, isize::from(wp & 0xffff == 1));
            1
        }
        0x10 => {
            EndDialog(hwnd, 0);
            1
        }
        0x400 => {
            // DM_GETDEFID follows explicit keyboard focus. Cancel remains the
            // initial default and Enter cannot implicitly accept a discard.
            let id = if GetFocus() == GetDlgItem(hwnd, 1) {
                1
            } else {
                2
            };
            SetWindowLongPtrW(hwnd, 0, id | (0x534b << 16));
            1
        }
        0x401 => 1, // DM_SETDEFID must not replace the owner-drawn button style.
        0x136 | 0x138 => {
            SetTextColor(wp as Hdc, paint::ui_ink(shell::TEXT));
            SetBkColor(wp as Hdc, paint::ui_color(shell::RAISED));
            dialog.brush as isize
        }
        0x2b => {
            let item = &*(lp as *const DrawItem);
            if item.control_type != 4 || !matches!(item.control_id, 1 | 2) {
                return 0;
            }
            if paint_button(item, dialog).is_err() {
                dialog.failed.set(true);
            }
            1
        }
        0xf => {
            paint_frame(hwnd, dialog);
            1
        }
        0x84 => {
            let mut point = Point {
                x: (lp as u16) as i16 as i32,
                y: ((lp as usize >> 16) as u16) as i16 as i32,
            };
            if ScreenToClient(hwnd, &mut point) != 0
                && point.y >= 0
                && point.y < dpi::for_window(hwnd).pixels(32)
            {
                SetWindowLongPtrW(hwnd, 0, 2); // DWLP_MSGRESULT = HTCAPTION.
                1
            } else {
                0
            }
        }
        _ => 0,
    }
}

pub(super) unsafe fn confirm(
    parent: Hwnd,
    title: &str,
    message: &str,
    accept: &str,
) -> Result<bool, String> {
    let mut dialog = create(parent, title, message, accept)?;
    show(parent, &mut dialog)
}

pub(super) unsafe fn information(parent: Hwnd, message: &str) -> Result<bool, String> {
    let mut dialog = create(parent, "Acerca de CIDE", message, "Cerrar")?;
    dialog.information = true;
    show(parent, &mut dialog)
}

pub(super) unsafe fn confirm_review(
    parent: Hwnd,
    title: &str,
    message: &str,
    accept: &str,
) -> Result<bool, String> {
    let mut dialog = create(parent, title, message, accept)?;
    dialog.scrollable = true;
    show(parent, &mut dialog)
}

impl Dialog<'_> {
    unsafe fn child<'a>(
        &self,
        parent: Hwnd,
        title: &'a str,
        message: &'a str,
        accept: &'a str,
    ) -> Result<Dialog<'a>, String> {
        // Modal HWND user data belongs to that dialog, not the editor window.
        create_at_size(parent, title, message, accept, self.ui_size, self.theme)
    }
}

unsafe fn create<'a>(
    parent: Hwnd,
    title: &'a str,
    message: &'a str,
    accept: &'a str,
) -> Result<Dialog<'a>, String> {
    let theme = super::owner(parent)
        .and_then(|owner| {
            owner
                .state
                .try_borrow()
                .ok()
                .map(|state| state.editor_theme)
        })
        .unwrap_or_default();
    create_at_size(
        parent,
        title,
        message,
        accept,
        dpi::window_ui_size(parent),
        theme,
    )
}

unsafe fn create_at_size<'a>(
    parent: Hwnd,
    title: &'a str,
    message: &'a str,
    accept: &'a str,
    ui_size: u8,
    theme: super::appearance::Theme,
) -> Result<Dialog<'a>, String> {
    let font = dpi::ui_font_size(dpi::for_window(parent), ui_size);
    let brush = CreateSolidBrush(theme.control_color(shell::RAISED));
    if font.is_null() || brush.is_null() {
        if !font.is_null() {
            DeleteObject(font);
        }
        if !brush.is_null() {
            DeleteObject(brush);
        }
        return Err("No se pudo preparar el mensaje de CIDE".into());
    }
    Ok(Dialog {
        parent,
        title,
        message,
        accept,
        scrollable: false,
        information: false,
        ui_size,
        theme,
        font,
        brush,
        failed: Cell::new(false),
        #[cfg(test)]
        answer: None,
        #[cfg(test)]
        initial_cancel: Cell::new(false),
        #[cfg(test)]
        themed_controls: Cell::new(false),
    })
}

unsafe fn show(parent: Hwnd, dialog: &mut Dialog<'_>) -> Result<bool, String> {
    let data = template(dialog);
    let result = DialogBoxIndirectParamW(
        GetModuleHandleW(std::ptr::null()),
        data.as_ptr(),
        parent,
        Some(procedure),
        (dialog as *mut Dialog<'_>) as isize,
    );
    if result == -1 || dialog.failed.get() {
        return Err("No se pudo mostrar el mensaje de CIDE; operación cancelada".into());
    }
    Ok(result == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::null_mut;

    #[test]
    fn dialog_stays_centered_on_owner_and_inside_monitor_work_area() {
        for raw in [96, 144, 192] {
            let dpi = dpi::Dpi::from_raw(raw);
            let work = Rect {
                left: -1920,
                top: 40,
                right: 0,
                bottom: 1080,
            };
            let parent = Rect {
                left: -1800,
                top: 100,
                right: -200,
                bottom: 1000,
            };
            let r = centered(parent, work, dpi, [420, 176]);
            assert_eq!(r.left + r.right, parent.left + parent.right);
            assert_eq!(r.top + r.bottom, parent.top + parent.bottom);
            let edge = centered(
                Rect {
                    left: -100,
                    top: 900,
                    right: 700,
                    bottom: 1500,
                },
                work,
                dpi,
                [420, 176],
            );
            assert!(edge.left >= work.left && edge.right <= work.right);
            assert!(edge.top >= work.top && edge.bottom <= work.bottom);
        }
    }

    #[test]
    fn themed_confirmation_defaults_to_cancel_and_returns_explicit_answer() {
        unsafe {
            for answer in [false, true] {
                let mut dialog =
                    create(null_mut(), "CIDE", "Mensaje sintético", "Descartar").unwrap();
                dialog.answer = Some(answer);
                assert_eq!(show(null_mut(), &mut dialog).unwrap(), answer);
                assert!(dialog.initial_cancel.get());
                assert!(dialog.themed_controls.get());
            }
        }
    }

    #[test]
    fn scrollable_record_confirmation_keeps_both_values_and_defaults_to_cancel() {
        unsafe {
            let message = format!(
                "sports.Customer · CustNum = 1\r\nValor actual\r\n{}\r\nValor propuesto\r\n{}",
                "Café ".repeat(200),
                "México ".repeat(200)
            );
            for answer in [false, true] {
                let mut dialog =
                    create(null_mut(), "Vista previa", &message, "Confirmar edición").unwrap();
                dialog.scrollable = true;
                dialog.answer = Some(answer);
                assert_eq!(show(null_mut(), &mut dialog).unwrap(), answer);
                assert!(dialog.initial_cancel.get());
                assert!(dialog.themed_controls.get());
            }
        }
    }
}

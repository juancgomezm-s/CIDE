//! Themed native controls for selection and project configuration. The dialog owns its
//! temporary values and invokes the caller only for an explicit acceptance.
use super::*;
use std::cell::RefCell;

const ACCEPT: u16 = 1;
const CANCEL: u16 = 2;
const DETAILS: u16 = 100;
const LIST: u16 = 101;
const ERROR: u16 = 102;
const LABEL_START: u16 = 110;
const EDIT_START: u16 = 130;

const WM_INITDIALOG: u32 = 0x110;
const WM_DRAWITEM: u32 = 0x2b;
const WM_CTLCOLOREDIT: u32 = 0x133;
const WM_CTLCOLORLISTBOX: u32 = 0x134;
const WM_CTLCOLORDLG: u32 = 0x136;
const WM_CTLCOLORSTATIC: u32 = 0x138;
const LB_ADDSTRING: u32 = 0x180;
const LB_SETCURSEL: u32 = 0x186;
const LB_GETCURSEL: u32 = 0x188;
const EM_SETLIMITTEXT: u32 = 0xc5;
const DM_GETDEFID: u32 = 0x400;
const DM_SETDEFID: u32 = 0x401;

#[link(name = "user32")]
extern "system" {
    fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> i32;
}

pub(in crate::windows) struct Field<'a> {
    pub label: &'a str,
    pub value: &'a str,
    pub limit: usize,
}

type Submit<'a> = dyn FnMut(&[String]) -> Result<(), String> + 'a;

enum Mode<'a> {
    Choose(&'a [String]),
    Edit(&'a [Field<'a>], RefCell<&'a mut Submit<'a>>),
}

struct Form<'a> {
    dialog: Dialog<'a>,
    mode: Mode<'a>,
    selection: Cell<Option<usize>>,
    #[cfg(test)]
    probe: Option<&'a tests::Probe>,
}

impl Form<'_> {
    fn template(&self) -> Vec<u32> {
        let extra = match self.mode {
            Mode::Choose(_) => 1,
            Mode::Edit(fields, _) => 1 + 2 * fields.len(),
        };
        let mut words = template_header(self.dialog.title, (3 + extra) as u16);
        // Positioning moves these controls using the owner's effective DPI.
        let bounds = [0, 0, 1, 1];
        styled_control(&mut words, DETAILS, bounds, 0x82, self.dialog.message, 0x80);
        match self.mode {
            Mode::Choose(_) => {
                // Listbox with notifications, scrolling and partial rows allowed.
                styled_control(&mut words, LIST, bounds, 0x83, "", 0xa1_0101);
            }
            Mode::Edit(fields, _) => {
                for (index, field) in fields.iter().enumerate() {
                    let index = index as u16;
                    styled_control(
                        &mut words,
                        LABEL_START + index,
                        bounds,
                        0x82,
                        field.label,
                        0x80,
                    );
                    // A single-line edit with a border and horizontal scrolling.
                    styled_control(&mut words, EDIT_START + index, bounds, 0x81, "", 0x81_0080);
                }
                styled_control(&mut words, ERROR, bounds, 0x82, "", 0x80);
            }
        }
        control(&mut words, ACCEPT, bounds, 0x80, self.dialog.accept);
        control(&mut words, CANCEL, bounds, 0x80, "Cancelar");
        pack_template(words)
    }

    unsafe fn arrange(&self, hwnd: Hwnd) -> bool {
        let height = match self.mode {
            Mode::Choose(_) => 324,
            Mode::Edit(fields, _) => 180 + 32 * fields.len() as i32,
        };
        let Some((dpi, width, actual_height)) = position(hwnd, self.dialog.parent, [560, height])
        else {
            return false;
        };
        // A work area shorter than the fields cannot present an actionable
        // dialog. The list uses native scrolling even on a small work area.
        if width < dpi.pixels(390)
            || actual_height
                < dpi.pixels(if matches!(self.mode, Mode::Choose(_)) {
                    212
                } else {
                    height
                })
        {
            return false;
        }
        let margin = dpi.pixels(16);
        let body_width = width - margin * 2;
        let footer = actual_height - dpi.pixels(48);
        let button = dpi.pixels(108);
        let place = |id: u16, x, y, w, h| {
            let item = GetDlgItem(hwnd, i32::from(id));
            SendMessageW(item, 0x30, self.dialog.font as usize, 0);
            SetWindowPos(item, std::ptr::null_mut(), x, y, w, h, 0x14) != 0
        };
        if !place(DETAILS, margin, dpi.pixels(44), body_width, dpi.pixels(48))
            || !place(
                ACCEPT,
                width - margin - 2 * button - dpi.pixels(12),
                footer,
                button,
                dpi.pixels(32),
            )
            || !place(
                CANCEL,
                width - margin - button,
                footer,
                button,
                dpi.pixels(32),
            )
        {
            return false;
        }
        match self.mode {
            Mode::Choose(items) => {
                if !place(
                    LIST,
                    margin,
                    dpi.pixels(94),
                    body_width,
                    footer - dpi.pixels(106),
                ) {
                    return false;
                }
                let list = GetDlgItem(hwnd, i32::from(LIST));
                for item in items {
                    if SendMessageW(list, LB_ADDSTRING, 0, wide(item).as_ptr() as isize) < 0 {
                        return false;
                    }
                }
                SendMessageW(list, LB_SETCURSEL, 0, 0);
                SetFocus(list);
            }
            Mode::Edit(fields, _) => {
                let label_width = dpi.pixels(232);
                for (index, field) in fields.iter().enumerate() {
                    let top = dpi.pixels(94 + 32 * index as i32);
                    let label = LABEL_START + index as u16;
                    let edit = EDIT_START + index as u16;
                    if !place(
                        label,
                        margin,
                        top + dpi.pixels(4),
                        label_width,
                        dpi.pixels(25),
                    ) || !place(
                        edit,
                        margin + label_width,
                        top,
                        body_width - label_width,
                        dpi.pixels(26),
                    ) {
                        return false;
                    }
                    let edit = GetDlgItem(hwnd, i32::from(edit));
                    SendMessageW(edit, EM_SETLIMITTEXT, field.limit, 0);
                    if SetWindowTextW(edit, wide(field.value).as_ptr()) == 0 {
                        return false;
                    }
                }
                if !place(
                    ERROR,
                    margin,
                    footer - dpi.pixels(38),
                    body_width,
                    dpi.pixels(36),
                ) {
                    return false;
                }
                SetFocus(GetDlgItem(hwnd, i32::from(EDIT_START)));
            }
        }
        true
    }

    unsafe fn accept(&self, hwnd: Hwnd) {
        match self.mode {
            Mode::Choose(items) => {
                let list = GetDlgItem(hwnd, i32::from(LIST));
                let selected = SendMessageW(list, LB_GETCURSEL, 0, 0);
                if (0..items.len() as isize).contains(&selected) {
                    self.selection.set(Some(selected as usize));
                    EndDialog(hwnd, 1);
                }
            }
            Mode::Edit(fields, ref submit) => {
                let result =
                    read_fields(hwnd, fields).and_then(|values| submit.borrow_mut()(&values));
                match result {
                    Ok(()) => {
                        EndDialog(hwnd, 1);
                    }
                    Err(error) => {
                        let label = GetDlgItem(hwnd, i32::from(ERROR));
                        SetWindowTextW(label, wide(&error).as_ptr());
                    }
                }
            }
        }
    }
}

unsafe fn read_fields(hwnd: Hwnd, fields: &[Field<'_>]) -> Result<Vec<String>, String> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let edit = GetDlgItem(hwnd, i32::from(EDIT_START + index as u16));
            let mut units = vec![0; field.limit + 1];
            let len = GetWindowTextW(edit, units.as_mut_ptr(), units.len() as i32);
            String::from_utf16(&units[..len as usize])
                .map_err(|_| format!("El campo «{}» contiene texto inválido", field.label))
        })
        .collect()
}

unsafe extern "system" fn procedure(hwnd: Hwnd, message: u32, wp: usize, lp: isize) -> isize {
    if message == WM_INITDIALOG {
        SetWindowLongPtrW(hwnd, -21, lp);
        let form = &*(lp as *const Form<'_>);
        if !form.arrange(hwnd) {
            form.dialog.failed.set(true);
            EndDialog(hwnd, 0);
            return 0;
        }
        #[cfg(test)]
        if let Some(probe) = form.probe {
            probe.run(hwnd, form);
        }
        #[cfg(test)]
        drive_scripted_form(hwnd, form);
        return 0;
    }
    let data = GetWindowLongPtrW(hwnd, -21);
    if data == 0 {
        return 0;
    }
    let form = &*(data as *const Form<'_>);
    let dialog = &form.dialog;
    match message {
        WM_COMMAND => match (wp as u16, (wp >> 16) as u16) {
            (ACCEPT, 0) | (LIST, 2) => {
                form.accept(hwnd);
                1
            }
            (CANCEL, 0) => {
                EndDialog(hwnd, 0);
                1
            }
            _ => 0,
        },
        WM_CLOSE => {
            EndDialog(hwnd, 0);
            1
        }
        DM_GETDEFID => {
            let id = if GetFocus() == GetDlgItem(hwnd, i32::from(CANCEL)) {
                CANCEL
            } else {
                ACCEPT
            };
            SetWindowLongPtrW(hwnd, 0, id as isize | (0x534b << 16));
            1
        }
        DM_SETDEFID => 1,
        WM_CTLCOLORDLG | WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            SetTextColor(wp as Hdc, shell::TEXT);
            SetBkColor(wp as Hdc, shell::RAISED);
            dialog.brush as isize
        }
        WM_DRAWITEM => {
            let item = &*(lp as *const DrawItem);
            if item.control_type != 4 || !matches!(item.control_id as u16, ACCEPT | CANCEL) {
                return 0;
            }
            if paint_button(item, dialog).is_err() {
                dialog.failed.set(true);
            }
            1
        }
        WM_PAINT => {
            paint_frame(hwnd, dialog);
            1
        }
        _ => 0,
    }
}

unsafe fn show(form: &Form<'_>) -> Result<bool, String> {
    let template = form.template();
    let result = DialogBoxIndirectParamW(
        GetModuleHandleW(std::ptr::null()),
        template.as_ptr(),
        form.dialog.parent,
        Some(procedure),
        (form as *const Form<'_>) as isize,
    );
    if result == -1 || (result == 0 && form.dialog.failed.get()) {
        return Err("No se pudo mostrar el formulario de CIDE".into());
    }
    Ok(result == 1)
}

pub(in crate::windows) unsafe fn choose(
    parent: Hwnd,
    title: &str,
    message: &str,
    items: &[String],
) -> Result<Option<usize>, String> {
    choose_dialog(create(parent, title, message, "Continuar")?, items)
}

pub(super) unsafe fn choose_dialog(
    dialog: Dialog<'_>,
    items: &[String],
) -> Result<Option<usize>, String> {
    let form = Form {
        dialog,
        mode: Mode::Choose(items),
        selection: Cell::new(None),
        #[cfg(test)]
        probe: None,
    };
    show(&form)?;
    Ok(form.selection.get())
}

pub(in crate::windows) unsafe fn edit<'a>(
    parent: Hwnd,
    title: &'a str,
    message: &'a str,
    fields: &'a [Field<'a>],
    submit: &'a mut Submit<'a>,
) -> Result<bool, String> {
    edit_dialog(create(parent, title, message, "Guardar")?, fields, submit)
}

pub(super) unsafe fn edit_dialog<'a>(
    dialog: Dialog<'a>,
    fields: &'a [Field<'a>],
    submit: &'a mut Submit<'a>,
) -> Result<bool, String> {
    show(&Form {
        dialog,
        mode: Mode::Edit(fields, RefCell::new(submit)),
        selection: Cell::new(None),
        #[cfg(test)]
        probe: None,
    })
}

#[cfg(test)]
thread_local! {
    static SCRIPTED_VALUE: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn script_record_edit(column: &str, value: &str) {
    SCRIPTED_VALUE.set(Some((column.into(), value.into())));
}

#[cfg(test)]
unsafe fn drive_scripted_form(hwnd: Hwnd, form: &Form<'_>) {
    let instructed = SCRIPTED_VALUE.with(|script| {
        let mut script = script.borrow_mut();
        match (&form.mode, form.dialog.title, script.as_ref()) {
            (Mode::Choose(items), "Editar columna", Some((column, _))) => {
                let index = items
                    .iter()
                    .position(|item| item == column)
                    .unwrap_or(usize::MAX);
                let list = GetDlgItem(hwnd, i32::from(LIST));
                SendMessageW(list, LB_SETCURSEL, index, 0);
                Some(())
            }
            (Mode::Edit(_, _), "Editar registro", Some(_)) => {
                let (_, value) = script.take()?;
                SetWindowTextW(
                    GetDlgItem(hwnd, i32::from(EDIT_START)),
                    wide(&value).as_ptr(),
                );
                Some(())
            }
            _ => None,
        }
    });
    if instructed.is_some() {
        ShowWindow(hwnd, 0);
        // If admission failed, leave the test with its assertion rather than an open modal.
        PostMessageW(hwnd, WM_COMMAND, usize::from(CANCEL), 0);
        SendMessageW(hwnd, WM_COMMAND, usize::from(ACCEPT), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::null_mut;

    pub(super) struct Probe {
        command: u16,
        controls_match: Cell<bool>,
    }

    impl Probe {
        pub(super) unsafe fn run(&self, hwnd: Hwnd, form: &Form<'_>) {
            let text_at = |id: u16| {
                let mut text = [0u16; 256];
                let control = GetDlgItem(hwnd, i32::from(id));
                let len = GetWindowTextW(control, text.as_mut_ptr(), text.len() as i32);
                String::from_utf16_lossy(&text[..len as usize])
            };
            let Mode::Edit(fields, _) = form.mode else {
                EndDialog(hwnd, 0);
                return;
            };
            self.controls_match.set(
                text_at(ACCEPT) == "Guardar"
                    && text_at(CANCEL) == "Cancelar"
                    && text_at(DETAILS) == "Configuración local"
                    && text_at(LABEL_START) == fields[0].label
                    && text_at(EDIT_START) == fields[0].value,
            );
            ShowWindow(hwnd, 0);
            SendMessageW(hwnd, WM_COMMAND, usize::from(self.command), 0);
        }
    }

    #[test]
    fn project_connections_native_form_uses_its_controls_and_cancels_without_submitting() {
        unsafe {
            let owner = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("CIDE").as_ptr(),
                0x8000_0000,
                0,
                0,
                800,
                600,
                null_mut(),
                null_mut(),
                GetModuleHandleW(std::ptr::null()),
                null_mut(),
            );
            assert!(!owner.is_null());
            for command in [CANCEL, ACCEPT] {
                let probe = Probe {
                    command,
                    controls_match: Cell::new(false),
                };
                let fields = [Field {
                    label: "Nombre",
                    value: "Inventário",
                    limit: 256,
                }];
                let submitted = RefCell::new(Vec::new());
                let mut submit = |values: &[String]| {
                    submitted.borrow_mut().extend_from_slice(values);
                    Ok(())
                };
                let form = Form {
                    dialog: create(
                        owner,
                        "Conexiones de datos",
                        "Configuración local",
                        "Guardar",
                    )
                    .unwrap(),
                    mode: Mode::Edit(&fields, RefCell::new(&mut submit)),
                    selection: Cell::new(None),
                    probe: Some(&probe),
                };
                assert_eq!(show(&form).unwrap(), command == ACCEPT);
                assert!(probe.controls_match.get());
                if command == ACCEPT {
                    assert_eq!(*submitted.borrow(), ["Inventário"]);
                } else {
                    assert!(submitted.borrow().is_empty());
                }
            }
            assert_ne!(DestroyWindow(owner), 0);
        }
    }
}

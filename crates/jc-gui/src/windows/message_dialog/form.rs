//! Themed native controls for selection and project configuration. The dialog owns its
//! temporary values and invokes the caller only for an explicit acceptance.
use super::*;
use jc_app::data_connections::ConnectionTest;
use std::cell::RefCell;
use std::path::Path;

const ACCEPT: u16 = 1;
const CANCEL: u16 = 2;
const DETAILS: u16 = 100;
const LIST: u16 = 101;
const ERROR: u16 = 102;
const TEST: u16 = 103;
const TEST_TIMER: usize = 1;
const WM_TIMER: u32 = 0x113;
const EN_CHANGE: u16 = 0x300;
const LABEL_START: u16 = 110;
const EDIT_START: u16 = 130;
const PICK_START: u16 = 150;

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
    fn SetTimer(hwnd: Hwnd, id: usize, milliseconds: u32, callback: usize) -> usize;
    fn KillTimer(hwnd: Hwnd, id: usize) -> i32;
    fn EnableWindow(hwnd: Hwnd, enabled: i32) -> i32;
}

pub(in crate::windows) struct Field<'a> {
    pub label: &'a str,
    pub value: &'a str,
    pub limit: usize,
}

type Submit<'a> = dyn FnMut(&[String]) -> Result<(), String> + 'a;
type StartTest<'a> = dyn Fn(&[String]) -> Result<ConnectionTest, String> + 'a;

pub(in crate::windows) struct ConfigTest<'a> {
    pub paths: Option<(&'a Path, &'a [usize])>,
    pub start: &'a StartTest<'a>,
}

struct TestControls<'a> {
    start: &'a StartTest<'a>,
    pending: RefCell<Option<ConnectionTest>>,
}

enum Mode<'a> {
    Choose(&'a [String]),
    Edit(&'a [Field<'a>], RefCell<&'a mut Submit<'a>>),
}

struct Form<'a> {
    dialog: Dialog<'a>,
    mode: Mode<'a>,
    selection: Cell<Option<usize>>,
    paths: Option<(&'a Path, &'a [usize])>,
    connection_test: Option<TestControls<'a>>,
    #[cfg(test)]
    probe: Option<&'a tests::Probe>,
}

impl Form<'_> {
    fn template(&self) -> Vec<u32> {
        let extra = match self.mode {
            Mode::Choose(_) => 1,
            Mode::Edit(fields, _) => {
                1 + 2 * fields.len() + self.paths.map_or(0, |(_, fields)| fields.len())
            }
        };
        let mut words = template_header(
            self.dialog.title,
            (3 + extra + usize::from(self.connection_test.is_some())) as u16,
        );
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
                    if self
                        .paths
                        .is_some_and(|(_, fields)| fields.contains(&(index as usize)))
                    {
                        control(&mut words, PICK_START + index, bounds, 0x80, "Elegir…");
                    }
                }
                styled_control(&mut words, ERROR, bounds, 0x82, "", 0x80);
            }
        }
        control(&mut words, ACCEPT, bounds, 0x80, self.dialog.accept);
        control(&mut words, CANCEL, bounds, 0x80, "Cancelar");
        if self.connection_test.is_some() {
            control(&mut words, TEST, bounds, 0x80, "Probar conexión");
        }
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
        if self.connection_test.is_some()
            && !place(TEST, margin, footer, dpi.pixels(126), dpi.pixels(32))
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
                    let picker = self
                        .paths
                        .is_some_and(|(_, fields)| fields.contains(&index));
                    let picker_width = if picker { dpi.pixels(78) } else { 0 };
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
                        body_width - label_width - picker_width,
                        dpi.pixels(26),
                    ) {
                        return false;
                    }
                    if picker
                        && !place(
                            PICK_START + index as u16,
                            width - margin - picker_width + dpi.pixels(4),
                            top,
                            picker_width - dpi.pixels(4),
                            dpi.pixels(26),
                        )
                    {
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

    unsafe fn pick_path(&self, hwnd: Hwnd, index: usize) -> bool {
        let Some((root, indices)) = self.paths else {
            return false;
        };
        if !indices.contains(&index) {
            return false;
        }
        let edit = GetDlgItem(hwnd, i32::from(EDIT_START + index as u16));
        let mut units = [0; 1025];
        let count = GetWindowTextW(edit, units.as_mut_ptr(), units.len() as i32);
        let current = root.join(String::from_utf16_lossy(&units[..count as usize]));
        let initial = current
            .parent()
            .filter(|path| path.is_dir())
            .unwrap_or(root);
        let result = crate::windows::folder::choose_file(hwnd, Some(initial));
        match result {
            Ok(Some(path)) => {
                let path = path.strip_prefix(root).unwrap_or(&path);
                if let Some(value) = path.to_str().filter(|value| value.len() <= 1024) {
                    SetWindowTextW(edit, wide(value).as_ptr());
                } else {
                    SetWindowTextW(
                        GetDlgItem(hwnd, i32::from(ERROR)),
                        wide("La ruta elegida es demasiado larga o no está en Unicode").as_ptr(),
                    );
                }
                SetFocus(edit);
            }
            Err(error) => {
                SetWindowTextW(GetDlgItem(hwnd, i32::from(ERROR)), wide(&error).as_ptr());
            }
            Ok(None) => {}
        }
        true
    }

    unsafe fn accept(&self, hwnd: Hwnd) {
        if self
            .connection_test
            .as_ref()
            .is_some_and(|test| test.pending.borrow().is_some())
        {
            return;
        }
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

    unsafe fn test_controls(&self, hwnd: Hwnd, fields: &[Field<'_>], enabled: bool) {
        let enabled = i32::from(enabled);
        for id in [ACCEPT, TEST]
            .into_iter()
            .chain((0..fields.len() as u16).map(|i| EDIT_START + i))
            .chain(
                self.paths
                    .into_iter()
                    .flat_map(|(_, indices)| indices.iter().map(|&i| PICK_START + i as u16)),
            )
        {
            EnableWindow(GetDlgItem(hwnd, i32::from(id)), enabled);
        }
    }

    unsafe fn test_message(&self, hwnd: Hwnd, message: &str) {
        SetWindowTextW(GetDlgItem(hwnd, i32::from(ERROR)), wide(message).as_ptr());
    }

    unsafe fn start_test(&self, hwnd: Hwnd) {
        let (Some(test), Mode::Edit(fields, _)) = (&self.connection_test, &self.mode) else {
            return;
        };
        if test.pending.borrow().is_some() {
            return;
        }
        if SetTimer(hwnd, TEST_TIMER, 60, 0) == 0 {
            self.test_message(hwnd, "No se pudo programar la prueba de conexión.");
            return;
        }
        match read_fields(hwnd, fields).and_then(|values| (test.start)(&values)) {
            Ok(pending) => {
                *test.pending.borrow_mut() = Some(pending);
                self.test_controls(hwnd, fields, false);
                self.test_message(hwnd, "Probando conexión…");
            }
            Err(error) => {
                KillTimer(hwnd, TEST_TIMER);
                self.test_message(hwnd, &format!("No se pudo probar la conexión. {error}"));
            }
        }
    }

    unsafe fn poll_test(&self, hwnd: Hwnd) {
        let (Some(test), Mode::Edit(fields, _)) = (&self.connection_test, &self.mode) else {
            return;
        };
        let result = test
            .pending
            .borrow()
            .as_ref()
            .and_then(ConnectionTest::poll);
        if let Some(result) = result {
            test.pending.borrow_mut().take();
            KillTimer(hwnd, TEST_TIMER);
            self.test_controls(hwnd, fields, true);
            self.test_message(
                hwnd,
                &result.unwrap_or_else(|error| format!("Falló la conexión. {error}")),
            );
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
            (TEST, 0) => {
                form.start_test(hwnd);
                1
            }
            (id, EN_CHANGE)
                if (EDIT_START..PICK_START).contains(&id) && form.connection_test.is_some() =>
            {
                form.test_message(hwnd, "");
                1
            }
            (ACCEPT, 0) | (LIST, 2) => {
                form.accept(hwnd);
                1
            }
            (CANCEL, 0) => {
                EndDialog(hwnd, 0);
                1
            }
            (id, 0) if id >= PICK_START => {
                i32::from(form.pick_path(hwnd, (id - PICK_START) as usize)) as isize
            }
            _ => 0,
        },
        WM_TIMER if wp == TEST_TIMER && form.connection_test.is_some() => {
            form.poll_test(hwnd);
            1
        }
        WM_CLOSE => {
            EndDialog(hwnd, 0);
            1
        }
        DM_GETDEFID => {
            let focus = GetFocus();
            let id = [CANCEL, TEST]
                .into_iter()
                .chain(
                    form.paths
                        .into_iter()
                        .flat_map(|(_, indices)| indices.iter().map(|&i| PICK_START + i as u16)),
                )
                .find(|&id| !focus.is_null() && focus == GetDlgItem(hwnd, i32::from(id)))
                .unwrap_or(ACCEPT);
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
            if item.control_type != 4
                || (!matches!(item.control_id as u16, ACCEPT | CANCEL | TEST)
                    && item.control_id < u32::from(PICK_START))
            {
                return 0;
            }
            let result = if item.control_id == u32::from(TEST) {
                paint_button_label(item, dialog, "Probar conexión")
            } else if item.control_id >= u32::from(PICK_START) {
                paint_button_label(item, dialog, "Elegir…")
            } else {
                paint_button(item, dialog)
            };
            if result.is_err() {
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
        paths: None,
        connection_test: None,
        #[cfg(test)]
        probe: None,
    };
    show(&form)?;
    Ok(form.selection.get())
}

pub(in crate::windows) unsafe fn edit_fields<'a>(
    parent: Hwnd,
    title: &'a str,
    message: &'a str,
    accept: &'a str,
    fields: &'a [Field<'a>],
    submit: &'a mut Submit<'a>,
) -> Result<bool, String> {
    edit_dialog(create(parent, title, message, accept)?, fields, submit)
}

pub(super) unsafe fn edit_dialog<'a>(
    dialog: Dialog<'a>,
    fields: &'a [Field<'a>],
    submit: &'a mut Submit<'a>,
) -> Result<bool, String> {
    edit_with_paths(dialog, fields, submit, None)
}

pub(in crate::windows) unsafe fn edit_config<'a>(
    parent: Hwnd,
    title: &'a str,
    message: &'a str,
    accept: &'a str,
    fields: &'a [Field<'a>],
    submit: &'a mut Submit<'a>,
    test: ConfigTest<'a>,
) -> Result<bool, String> {
    edit_with_paths(
        create(parent, title, message, accept)?,
        fields,
        submit,
        Some(test),
    )
}

unsafe fn edit_with_paths<'a>(
    dialog: Dialog<'a>,
    fields: &'a [Field<'a>],
    submit: &'a mut Submit<'a>,
    test: Option<ConfigTest<'a>>,
) -> Result<bool, String> {
    show(&Form {
        dialog,
        mode: Mode::Edit(fields, RefCell::new(submit)),
        selection: Cell::new(None),
        paths: test.as_ref().and_then(|test| test.paths),
        connection_test: test.map(|test| TestControls {
            start: test.start,
            pending: RefCell::new(None),
        }),
        #[cfg(test)]
        probe: None,
    })
}

#[cfg(test)]
type ScriptedRowAction = (usize, Vec<(String, String)>);

#[cfg(test)]
thread_local! {
    static SCRIPTED_VALUE: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static SCRIPTED_HISTORY: Cell<Option<usize>> = const { Cell::new(None) };
    static SCRIPTED_ROW_ACTION: RefCell<Option<ScriptedRowAction>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn script_record_edit(column: &str, value: &str) {
    SCRIPTED_VALUE.set(Some((column.into(), value.into())));
}

#[cfg(test)]
pub(super) fn script_record_history(index: usize) {
    SCRIPTED_HISTORY.set(Some(index));
}

#[cfg(test)]
pub(super) fn script_row_action(action: usize, fields: &[(&str, &str)]) {
    let fields = fields
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    SCRIPTED_ROW_ACTION.set(Some((action, fields)));
}

#[cfg(test)]
unsafe fn drive_scripted_form(hwnd: Hwnd, form: &Form<'_>) {
    let history = if form.dialog.title == "Historial de respaldos" {
        SCRIPTED_HISTORY.take().map(|index| {
            SendMessageW(GetDlgItem(hwnd, i32::from(LIST)), LB_SETCURSEL, index, 0);
        })
    } else {
        None
    };
    let row_action = SCRIPTED_ROW_ACTION.with(|script| {
        let mut script = script.borrow_mut();
        match (&form.mode, form.dialog.title) {
            (Mode::Choose(_), "Operaciones de fila") => {
                let index = script.as_ref()?.0;
                SendMessageW(GetDlgItem(hwnd, i32::from(LIST)), LB_SETCURSEL, index, 0);
                if index == 2 {
                    script.take();
                }
                Some(())
            }
            (Mode::Edit(fields, _), "Alta de registro" | "Restaurar alta o borrado") => {
                let (_, values) = script.take()?;
                for (name, value) in values {
                    let index = fields.iter().position(|field| field.label == name).unwrap();
                    SetWindowTextW(
                        GetDlgItem(hwnd, i32::from(EDIT_START) + index as i32),
                        wide(&value).as_ptr(),
                    );
                }
                Some(())
            }
            _ => None,
        }
    });
    let instructed = history.or(row_action).or_else(|| {
        SCRIPTED_VALUE.with(|script| {
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
        })
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
        tested: Cell<bool>,
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
                    && text_at(EDIT_START) == fields[0].value
                    && text_at(PICK_START) == "Elegir…",
            );
            ShowWindow(hwnd, 0);
            if form.connection_test.is_some() {
                SendMessageW(hwnd, WM_COMMAND, usize::from(TEST), 0);
                let started =
                    text_at(TEST) == "Probar conexión" && text_at(ERROR) == "Probando conexión…";
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                while text_at(ERROR) == "Probando conexión…" && std::time::Instant::now() < deadline
                {
                    SendMessageW(hwnd, WM_TIMER, TEST_TIMER, 0);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                let finished = text_at(ERROR) == "Conexión correcta de prueba";
                SetWindowTextW(
                    GetDlgItem(hwnd, i32::from(EDIT_START)),
                    wide(fields[0].value).as_ptr(),
                );
                self.tested
                    .set(started && finished && text_at(ERROR).is_empty());
            }
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
                    tested: Cell::new(false),
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
                    paths: Some((Path::new(env!("CARGO_MANIFEST_DIR")), &[0])),
                    connection_test: Some(TestControls {
                        start: &|values| {
                            if values != ["Inventário"] {
                                return Err("Borrador inesperado".into());
                            }
                            ConnectionTest::start(
                                |_| Ok("Conexión correcta de prueba".into()),
                                || {},
                            )
                        },
                        pending: RefCell::new(None),
                    }),
                    probe: Some(&probe),
                };
                assert_eq!(show(&form).unwrap(), command == ACCEPT);
                assert!(probe.controls_match.get());
                assert!(probe.tested.get());
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

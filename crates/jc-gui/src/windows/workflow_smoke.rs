//! Owned-HWND acceptance routes for the new shared GUI surfaces. Source fixtures
//! are synthetic; no AVM, system clipboard, Git write or external link executes.
use super::{
    ffi::*,
    owner, paint,
    project_smoke::snapshot,
    smoke::{command, expected, key, paste, position},
    State,
};
use crate::SmokeOptions;
use jc_app::{
    open_diff::OpenDiffState, source_services::SourceState, CommandId, Modifiers, PaneId,
};
use jc_text::TextEncoding;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

const SOURCE: &[u8] = b"alpha\r\nbeta\r\n";

unsafe fn pump(hwnd: Hwnd) {
    for _ in 0..64 {
        let mut message: Msg = std::mem::zeroed();
        if PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) == 0 {
            break;
        }
        // Actual worker/self-posted notification only. No direct poll/tick or
        // synthetic keyboard event completes an asynchronous request.
        DispatchMessageW(&message);
    }
}
unsafe fn wait_for(
    hwnd: Hwnd,
    description: &str,
    ready: impl Fn(&State) -> bool,
) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        pump(hwnd);
        {
            let state = owner(hwnd)
                .ok_or("La prueba perdió su HWND")?
                .state
                .borrow();
            if ready(&state) {
                return Ok(());
            }
            if Instant::now() >= until {
                return Err(format!(
                    "Se agotó el plazo de {description}: {}",
                    state.status
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
unsafe fn wait_diff(hwnd: Hwnd) -> Result<(), String> {
    wait_for(hwnd, "entrega de comparación", |state| {
        state.open_diff.state() == OpenDiffState::Ready
    })
}
unsafe fn wait_source(hwnd: Hwnd) -> Result<(), String> {
    wait_for(hwnd, "entrega de código", |state| {
        state.source_gui.controller.state() == SourceState::Ready
    })
}
unsafe fn palette(hwnd: Hwnd, target: CommandId) -> Result<(), String> {
    let label = jc_app::default_registry()
        .get(target)
        .ok_or("Comando de prueba no registrado")?
        .label;
    command(hwnd, CommandId::PaletteOpen)?;
    paste(hwnd, label)?;
    key(hwnd, 0x0d, Modifiers::NONE)
}
unsafe fn memory(
    hwnd: Hwnd,
    options: &SmokeOptions,
    name: &str,
    text: &str,
) -> Result<jc_app::DocumentId, String> {
    reset_surfaces(hwnd)?;
    let path = options
        .fixture
        .parent()
        .ok_or("El fixture no tiene directorio padre")?
        .join(name);
    {
        let mut state = owner(hwnd)
            .ok_or("La prueba perdió su HWND")?
            .state
            .borrow_mut();
        state.explorer_visible = false;
        state.workbench.new_file(path, TextEncoding::Utf8);
    }
    paste(hwnd, text)?;
    position(hwnd, 0, 0)?;
    expected(hwnd, text.as_bytes())?;
    Ok(owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_document()
        .unwrap()
        .id)
}
unsafe fn reset_surfaces(hwnd: Hwnd) -> Result<(), String> {
    let mut borrowed = owner(hwnd)
        .ok_or("La prueba perdió su HWND")?
        .state
        .borrow_mut();
    let state: &mut State = &mut borrowed;
    state.close_selector();
    state.open_diff.cancel(&mut state.jobs);
    state.open_diff_visible = false;
    state.source_gui.controller.cancel(&mut state.jobs);
    state.close_source_ui();
    state.snippet_gui = None;
    state.markdown_gui = None;
    state.bottom.focused = false;
    state.explorer_focused = false;
    Ok(())
}
unsafe fn restore(hwnd: Hwnd, initial: (jc_app::DocumentId, u64, usize)) -> Result<(), String> {
    reset_surfaces(hwnd)?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        for pane in [PaneId::Primary, PaneId::Secondary] {
            if state.workbench.focus_editor_pane(pane)
                && state
                    .workbench
                    .panes()
                    .view(pane)
                    .is_some_and(|view| view.is_pinned())
            {
                state.workbench.toggle_reference_pin();
            }
        }
        state.workbench.close_split_editor();
        while state.workbench.tabs().tabs().len() > initial.2 {
            let last = state.workbench.tabs().tabs().len();
            if !state.workbench.select_tab(last) {
                return Err("No se pudo seleccionar pestaña temporal".into());
            }
            state
                .workbench
                .close_active(true)
                .map_err(|error| error.to_string())?;
        }
        if !state.workbench.select_tab(1) {
            return Err("No se pudo restaurar el fixture original".into());
        }
        state.smoke_clipboard = None;
        let document = state
            .workbench
            .active_document()
            .ok_or("Se cerró el fixture original")?;
        if document.id != initial.0
            || document.session.document().revision() != initial.1
            || state.workbench.tabs().tabs().len() != initial.2
            || state.workbench.panes().is_split()
        {
            return Err("Cambió la identidad/revisión/catálogo del fixture original".into());
        }
    }
    wait_for(hwnd, "liberación de tareas propias", |state| {
        state.jobs.outstanding() == 0
    })?;
    expected(hwnd, SOURCE)
}
unsafe fn bitmap(hwnd: Hwnd, options: &SmokeOptions, name: &str) -> Result<(), String> {
    let Some(base) = &options.bitmap else {
        return Ok(());
    };
    let path = base.with_file_name(name);
    if path.exists() || path == *base || path == options.fixture || path == options.report {
        return Err("La captura debe ser un nuevo artefacto hermano propio".into());
    }
    let frame = {
        let mut state = owner(hwnd)
            .ok_or("La prueba perdió su HWND")?
            .state
            .borrow_mut();
        paint::prepare(&mut state)
    };
    paint::bitmap(hwnd, &frame, &path)
}

struct IncludeFixture {
    path: PathBuf,
    parent: PathBuf,
}
const INCLUDE: &[u8] = b"DISPLAY \"owned include\".\r\n";
impl IncludeFixture {
    fn create(options: &SmokeOptions) -> Result<Self, String> {
        let parent = options
            .fixture
            .parent()
            .ok_or("El fixture no tiene directorio padre")?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let path = parent.join("workflow-include.i");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        file.write_all(INCLUDE).map_err(|e| e.to_string())?;
        Ok(Self { path, parent })
    }
}
impl Drop for IncludeFixture {
    fn drop(&mut self) {
        use std::os::windows::fs::MetadataExt;
        if self.path.parent() == Some(self.parent.as_path())
            && fs::symlink_metadata(&self.path)
                .is_ok_and(|m| m.is_file() && m.file_attributes() & 0x400 == 0)
            && fs::read(&self.path).is_ok_and(|bytes| bytes == INCLUDE)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    let first = snapshot(hwnd)?;
    if first.3 != 1 {
        return Err("El recorrido requiere solo el fixture original abierto".into());
    }
    let initial = (first.0, first.1, first.3);
    let mut checks = Vec::new();

    let left = memory(hwnd, options, "workflow-left.txt", "same\r\nleft\r\n")?;
    command(hwnd, CommandId::EditorSplitRight)?;
    command(hwnd, CommandId::EditorFocusSecondary)?;
    let right = memory(hwnd, options, "workflow-right.txt", "same\r\nright\r\n")?;
    command(hwnd, CommandId::EditorFocusPrimary)?;
    command(hwnd, CommandId::EditorTogglePin)?;
    palette(hwnd, CommandId::DiffCompareOpen)?;
    wait_diff(hwnd)?;
    let before = snapshot(hwnd)?;
    {
        let mut state = owner(hwnd).unwrap().state.borrow_mut();
        let presentation = state
            .open_diff_presentation()
            .ok_or("No se mostró la comparación")?;
        if !presentation
            .presentation
            .rows
            .iter()
            .any(|row| row.label.starts_with("@@"))
            || state.git_context.is_some()
            || state.git_panel.is_some()
        {
            return Err("La comparación no tiene filas visibles o activó una ruta Git".into());
        }
    }
    SendMessageW(hwnd, WM_CHAR, 'X' as usize, 0);
    key(hwnd, 0x2e, Modifiers::NONE)?;
    paste(hwnd, "no source edits")?;
    if snapshot(hwnd)? != before {
        return Err("La entrada del visor editó fuente o caret ocultos".into());
    }
    bitmap(hwnd, options, "native-open-diff.bmp")?;
    if snapshot(hwnd)? != before {
        return Err("La composición de diferencias mutó el modelo".into());
    }
    checks.push("workflow_open_diff_notified_readonly_without_git");

    key(hwnd, 0x0d, Modifiers::SHIFT)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.workbench.panes().active() != PaneId::Primary
            || state.workbench.active_document().unwrap().id != left
            || !state
                .workbench
                .panes()
                .view(PaneId::Primary)
                .unwrap()
                .is_pinned()
        {
            return Err("El bloque izquierdo no volvió a su grupo anclado original".into());
        }
    }
    command(hwnd, CommandId::DiffCompareOpen)?;
    wait_diff(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_document()
        .unwrap()
        .id
        != right
    {
        return Err("El bloque derecho no volvió a su documento original".into());
    }
    command(hwnd, CommandId::DiffCompareOpen)?;
    wait_diff(hwnd)?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .insert_text("changed")
        .map_err(|e| e.to_string())?;
    key(hwnd, 0x28, Modifiers::NONE)?;
    if owner(hwnd).unwrap().state.borrow().open_diff.state() != OpenDiffState::Stale {
        return Err("La comparación no invalidó una revisión cambiada".into());
    }
    let stale = snapshot(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if snapshot(hwnd)? != stale {
        return Err("Se navegó una entrega obsoleta".into());
    }
    key(hwnd, 0x74, Modifiers::NONE)?;
    wait_diff(hwnd)?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    if owner(hwnd).unwrap().state.borrow().open_diff_visible {
        return Err("Escape no cerró comparación".into());
    }
    checks.push("workflow_open_diff_pin_origins_stale_refresh_cancel");
    restore(hwnd, initial)?;

    let symbols = "PROCEDURE alpha:\r\nEND.\r\nPROCEDURE beta:\r\nEND.\r\n";
    memory(hwnd, options, "workflow-symbols.p", symbols)?;
    command(hwnd, CommandId::ViewSymbols)?;
    wait_source(hwnd)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .source_gui
        .controller
        .items()
        .len()
        != 2
    {
        return Err("El proveedor léxico no entregó los dos símbolos propios".into());
    }
    let before = snapshot(hwnd)?;
    SendMessageW(hwnd, WM_CHAR, 'X' as usize, 0);
    key(hwnd, 0x2e, Modifiers::NONE)?;
    paste(hwnd, "no source edits")?;
    if snapshot(hwnd)? != before {
        return Err("La lista de símbolos editó fuente oculta".into());
    }
    key(hwnd, 0x28, Modifiers::NONE)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_editor()
        .unwrap()
        .cursor
        != symbols.find("beta:").unwrap()
    {
        return Err("La selección de símbolo no navegó su rango fuente".into());
    }
    expected(hwnd, symbols.as_bytes())?;
    checks.push("workflow_source_symbols_notification_selection_readonly");
    restore(hwnd, initial)?;

    let words = "customer custom\r\ncus";
    memory(hwnd, options, "workflow-words.rs", words)?;
    position(hwnd, 1, 3)?;
    command(hwnd, CommandId::SourceComplete)?;
    wait_source(hwnd)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    expected(hwnd, b"customer custom\r\ncustomer")?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, words.as_bytes())?;
    position(hwnd, 1, 3)?;
    command(hwnd, CommandId::SourceComplete)?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .move_left(false)
        .map_err(|e| e.to_string())?;
    wait_for(hwnd, "rechazo de completado obsoleto", |state| {
        state.source_gui.controller.state() == SourceState::Stale && state.jobs.outstanding() == 0
    })?;
    command(hwnd, CommandId::SourceAccept)?;
    expected(hwnd, words.as_bytes())?;
    key(hwnd, 0x1b, Modifiers::NONE)?;
    checks.push("workflow_source_completion_one_undo_and_stale_rejection");
    restore(hwnd, initial)?;

    {
        let include = IncludeFixture::create(options)?;
        let origin = memory(
            hwnd,
            options,
            "workflow-reference.p",
            "{workflow-include.i}\r\n",
        )?;
        command(hwnd, CommandId::SourceReferences)?;
        wait_source(hwnd)?;
        command(hwnd, CommandId::SourceOpenToSide)?;
        expected(hwnd, INCLUDE)?;
        let state = owner(hwnd).unwrap().state.borrow();
        if state.workbench.active_document().unwrap().id == origin
            || !state.workbench.panes().is_split()
            || fs::read(&include.path).map_err(|e| e.to_string())? != INCLUDE
        {
            return Err("La referencia no abrió el include confinado sin cambiarlo".into());
        }
        drop(state);
        checks.push("workflow_source_include_confined_side_without_avm");
        restore(hwnd, initial)?;
    }

    memory(hwnd, options, "workflow-template.p", "base\r\n")?;
    command(hwnd, CommandId::EditInsertSnippet)?;
    paste(hwnd, "Procedimiento")?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    key(hwnd, 0x55, Modifiers::CTRL)?;
    paste(hwnd, "owned_proc")?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if !state.context().snippet_ready {
            return Err("No se preparó la vista previa de plantilla".into());
        }
    }
    expected(hwnd, b"base\r\n")?;
    SendMessageW(hwnd, WM_CHAR, 'X' as usize, 0);
    key(hwnd, 0x2e, Modifiers::NONE)?;
    paste(hwnd, "no source edits")?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    expected(hwnd, b"base\r\n")?;
    key(hwnd, 0x78, Modifiers::NONE)?;
    expected(
        hwnd,
        b"PROCEDURE owned_proc:\r\n    \r\nEND PROCEDURE.\r\nbase\r\n",
    )?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, b"base\r\n")?;
    checks.push("workflow_snippet_review_before_one_transaction_and_undo");

    position(hwnd, 0, 0)?;
    command(hwnd, CommandId::EditInsertSnippet)?;
    paste(hwnd, "Procedimiento")?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    key(hwnd, 0x0d, Modifiers::NONE)?;
    owner(hwnd)
        .unwrap()
        .state
        .borrow_mut()
        .workbench
        .insert_text("changed")
        .map_err(|e| e.to_string())?;
    let changed = snapshot(hwnd)?;
    key(hwnd, 0x78, Modifiers::NONE)?;
    if snapshot(hwnd)? != changed {
        return Err("Una plantilla obsoleta editó el fuente".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    key(hwnd, 0x5a, Modifiers::CTRL)?;
    expected(hwnd, b"base\r\n")?;
    checks.push("workflow_snippet_stale_revision_rejects_acceptance");
    restore(hwnd, initial)?;

    let markdown = "# Título\r\n\r\n**Fuerte** y *énfasis* [sitio](https://example.invalid)\n```rust\nlet x = 1;\n```\n";
    memory(hwnd, options, "workflow-reading.md", markdown)?;
    command(hwnd, CommandId::ViewMarkdownPreview)?;
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        pump(hwnd);
        let ready = {
            let mut state = owner(hwnd).unwrap().state.borrow_mut();
            let frame = state
                .markdown_presentation()
                .ok_or("No se mostró la lectura Markdown")?;
            !frame.presentation.pending
                && frame
                    .presentation
                    .rows
                    .iter()
                    .flat_map(|r| &r.spans)
                    .any(|s| s.kind == jc_app::SyntaxKind::Heading)
                && frame
                    .presentation
                    .rows
                    .iter()
                    .flat_map(|r| &r.spans)
                    .any(|s| s.kind == jc_app::SyntaxKind::Emphasis)
                && frame
                    .presentation
                    .rows
                    .iter()
                    .flat_map(|r| &r.spans)
                    .any(|s| s.kind == jc_app::SyntaxKind::Code)
        };
        if ready {
            break;
        }
        if Instant::now() >= until {
            return Err("Markdown no entregó la presentación tipada esperada".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let before = snapshot(hwnd)?;
    SendMessageW(hwnd, WM_CHAR, 'X' as usize, 0);
    key(hwnd, 0x2e, Modifiers::NONE)?;
    paste(hwnd, "no source edits")?;
    key(hwnd, 0x22, Modifiers::NONE)?;
    key(hwnd, 0x24, Modifiers::NONE)?;
    if snapshot(hwnd)? != before {
        return Err("La lectura Markdown editó la fuente o su caret".into());
    }
    bitmap(hwnd, options, "native-markdown.bmp")?;
    command(hwnd, CommandId::PaletteOpen)?;
    paste(hwnd, "Guardar")?;
    {
        let state = owner(hwnd).unwrap().state.borrow();
        if state.selector.is_none() || state.markdown_gui.is_none() {
            return Err("La paleta superior no conservó la lectura Markdown".into());
        }
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    if owner(hwnd).unwrap().state.borrow().markdown_gui.is_none() {
        return Err("Cancelar la paleta perdió la lectura Markdown subyacente".into());
    }
    key(hwnd, 0x1b, Modifiers::NONE)?;
    if !owner(hwnd).unwrap().state.borrow().context().editor_input {
        return Err("Cerrar Markdown no devolvió la entrada al editor".into());
    }
    expected(hwnd, markdown.as_bytes())?;
    checks.push("workflow_markdown_readonly_typed_render_palette_and_focus");
    restore(hwnd, initial)?;
    if fs::read(&options.fixture).map_err(|e| e.to_string())? != SOURCE {
        return Err("El recorrido cambió el archivo original del fixture".into());
    }
    Ok(checks)
}

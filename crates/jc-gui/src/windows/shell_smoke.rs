//! Native shell acceptance and raster measurement on our own fixture/HWND only.
use super::{
    ffi::*,
    owner, paint,
    smoke::{expected, key, position},
};
use crate::SmokeOptions;
use jc_app::{Modifiers, PaneId};
use jc_text::TextEncoding;
use std::{
    fs,
    io::Write,
    time::{Duration, Instant},
};

struct ProjectFixture {
    root: std::path::PathBuf,
    files: Vec<(std::path::PathBuf, Vec<u8>)>,
    directories: Vec<std::path::PathBuf>,
}
impl ProjectFixture {
    fn create(parent: &std::path::Path) -> Result<Self, String> {
        let root = parent.join("proyecto-cide");
        fs::create_dir(&root).map_err(|e| e.to_string())?;
        let mut value = Self {
            root: root.clone(),
            files: Vec::new(),
            directories: vec![root],
        };
        for name in ["src", "docs", "recursos"] {
            let dir = value.root.join(name);
            fs::create_dir(&dir).map_err(|e| e.to_string())?;
            value.directories.push(dir);
        }
        for (name, bytes) in [
            ("README.md", b"# Proyecto de prueba\n".as_slice()),
            ("Cargo.toml", b"[package]\nname = \"clientes\"\n".as_slice()),
            (
                "src/clientes.rs",
                b"pub fn calcular_total() {}\n".as_slice(),
            ),
            ("src/aplicacion.rs", b"fn main() {}\n".as_slice()),
            ("docs/arquitectura.md", b"# Arquitectura\n".as_slice()),
        ] {
            let path = value.root.join(name);
            fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .map_err(|e| e.to_string())?
                .write_all(bytes)
                .map_err(|e| e.to_string())?;
            value.files.push((path, bytes.to_vec()));
        }
        Ok(value)
    }
}
impl Drop for ProjectFixture {
    fn drop(&mut self) {
        use std::os::windows::fs::MetadataExt;
        for (path, bytes) in &self.files {
            if path.starts_with(&self.root)
                && fs::symlink_metadata(path)
                    .is_ok_and(|m| m.is_file() && m.file_attributes() & 0x400 == 0)
                && fs::read(path).is_ok_and(|b| b == *bytes)
            {
                let _ = fs::remove_file(path);
            }
        }
        for path in self.directories.iter().rev() {
            if path.starts_with(&self.root)
                && fs::symlink_metadata(path)
                    .is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0)
            {
                let _ = fs::remove_dir(path);
            }
        }
    }
}

unsafe fn prepared(hwnd: Hwnd) -> paint::Frame {
    let mut state = owner(hwnd).unwrap().state.borrow_mut();
    let frame = paint::prepare(&mut state);
    state.tab_hits = Some(frame.tab_hits());
    frame
}
unsafe fn click(hwnd: Hwnd, point: (i32, i32)) {
    SendMessageW(
        hwnd,
        WM_LBUTTONDOWN,
        1,
        ((point.1 as isize) << 16) | point.0 as isize,
    );
    SendMessageW(
        hwnd,
        WM_LBUTTONUP,
        0,
        ((point.1 as isize) << 16) | point.0 as isize,
    );
}
pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<Vec<&'static str>, String> {
    let mut checks = Vec::new();
    let resize = super::interaction_smoke::Resize::new(hwnd)?;
    resize.set(1500, 960)?;
    super::zoom::smoke(hwnd, options.bitmap.as_deref())?;
    checks.push("shell_zoom_slider_mouse_keyboard_and_menu_preserve_document");
    let initial = {
        let s = owner(hwnd).unwrap().state.borrow();
        let d = s.workbench.active_document().unwrap();
        (d.id, d.session.document().revision(), s.width, s.height)
    };
    let root = options
        .fixture
        .parent()
        .ok_or("Falta directorio de prueba")?;
    let caption = super::shell::caption_color(hwnd);
    if caption.is_some_and(|color| color != super::shell::SURFACE) {
        return Err("El título no recibió el fondo del tema elegido".into());
    }
    let menu_theme = owner(hwnd)
        .unwrap()
        .menu_theme
        .borrow()
        .clone()
        .ok_or("Falta el tema del menú")?;
    if menu_theme.palette().surface != super::shell::SURFACE || menu_theme.statistics().labels < 100
    {
        return Err("La barra superior no comparte la paleta del editor".into());
    }
    {
        let s = owner(hwnd).unwrap().state.borrow();
        super::native_menu::refresh(hwnd, &s)?;
        let before = menu_theme.statistics();
        super::native_menu::refresh(hwnd, &s)?;
        let after = menu_theme.statistics();
        if before.style_updates != after.style_updates
            || before.width_calculations != after.width_calculations
        {
            return Err(
                "Actualizar un menú idéntico repitió las medidas o el estilo nativo".into(),
            );
        }
    }
    let stats = menu_theme.statistics();
    let theme_report=format!("{{\"background_rgb\":\"#2E3440\",\"client_caption\":true,\"caption_color_supported\":{},\"caption_colorref\":{},\"menu_surface_colorref\":{},\"menu_measures\":{},\"menu_draws\":{},\"physical_display_verified\":false}}",
        caption.is_some(),caption.map_or("null".to_owned(),|c|c.to_string()),menu_theme.palette().surface,stats.measures,stats.draws);
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(options.report.with_file_name("window-theme.json"))
        .map_err(|e| e.to_string())?
        .write_all(theme_report.as_bytes())
        .map_err(|e| e.to_string())?;
    checks.push("shell_title_and_native_menu_follow_selected_palette");
    let project = ProjectFixture::create(root)?;
    let mut sample = String::new();
    for n in 0..48 {
        use std::fmt::Write as _;
        write!(
            &mut sample,
            "    let cliente_{n:02} = calcular_total(42, \"México\"); // valor\r\n"
        )
        .map_err(|e| e.to_string())?;
    }
    let source=format!("// CIDE · interfaz gráfica independiente\r\nuse std::collections::HashMap;\r\n// Unicode: e\u{301} 中 👩‍💻\r\npub fn calcular_total(base: i64, ciudad: &str) -> i64 {{\r\n{sample}    base + 10\r\n}}\r\n");
    let tab = {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.set_project_root(project.root.clone())?;
        s.workbench
            .new_file(root.join("clientes.rs"), TextEncoding::Utf8);
        s.workbench
            .insert_text(&source)
            .map_err(|e| e.to_string())?;
        let tab = s.workbench.tabs().active_id().unwrap();
        for n in 0..36 {
            s.workbench
                .new_file(root.join(format!("modulo-{n:02}.rs")), TextEncoding::Utf8);
        }
        s.workbench.select_tab(2);
        s.explorer_visible = true;
        tab
    };
    position(hwnd, 0, 0)?;
    SendMessageW(hwnd, WM_PAINT, 0, 0);
    let before = {
        let o = owner(hwnd).unwrap();
        let s = o.state.borrow();
        (
            o.frame_preparations.get(),
            s.workbench.text_statistics(tab),
            s.source_edit_target(),
        )
    };
    SendMessageW(hwnd, WM_PAINT, 0, 0);
    SendMessageW(hwnd, WM_PAINT, 0, 0);
    {
        let o = owner(hwnd).unwrap();
        let s = o.state.borrow();
        if before
            != (
                o.frame_preparations.get(),
                s.workbench.text_statistics(tab),
                s.source_edit_target(),
            )
        {
            return Err("Exponer la GUI volvió a preparar fuente o cambió la vista".into());
        }
    }
    checks.push("shell_exposure_reuses_owned_frame_without_source_work");
    // Real catalog delivery, no polling in rasterization or a synthetic key.
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let mut message: Msg = std::mem::zeroed();
        while PeekMessageW(&mut message, hwnd, TASKS_MESSAGE, TASKS_MESSAGE, 1) != 0 {
            DispatchMessageW(&message);
        }
        if !owner(hwnd).unwrap().state.borrow().explorer.ready() {
            let state = owner(hwnd).unwrap().state.borrow();
            if state.explorer.nodes().len() > 1 {
                break;
            }
        }
        if Instant::now() >= until {
            return Err("El catálogo de prueba no entregó sus filas".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let frame = prepared(hwnd);
    let counts = frame.source_draw_counts();
    if counts.0 < 1000 || counts.1 >= counts.0 {
        return Err(format!("No se agruparon los tramos fuente: {counts:?}"));
    }
    let measure = super::render_probe::measure(hwnd, &frame)?;
    let path = options.report.with_file_name("render-probe.json");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?
        .write_all(measure.as_bytes())
        .map_err(|e| e.to_string())?;
    if let Some(base) = &options.bitmap {
        paint::bitmap(hwnd, &frame, &base.with_file_name("native-shell.bmp"))?;
    }
    checks.push("shell_proportional_chrome_pixel_tree_and_rounded_tabs");
    checks.push("shell_grouped_runs_exact_raster_and_retained_surface_probe");
    let point = frame
        .tab_hits()
        .list_pixel(PaneId::Primary)
        .ok_or("Falta lista de pestañas")?;
    click(hwnd, point);
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        if !s
            .selector
            .as_ref()
            .is_some_and(|s| s.kind() == super::selector::SelectorKind::OpenTabs)
        {
            return Err("La flecha no abrió documentos abiertos".into());
        }
        s.edit_selector_input(jc_app::FieldEdit::Insert("modulo-33"), false);
        let p = s
            .selector_presentation(12)
            .ok_or("No hay lista de documentos")?;
        if p.total != 1 || p.rows.len() != 1 {
            return Err("El filtro no localizó el documento oculto".into());
        }
    }
    key(hwnd, 0x0d, Modifiers::NONE)?;
    if !owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .active_document()
        .unwrap()
        .session
        .path()
        .ends_with("modulo-33.rs")
    {
        return Err("Se eligió otro documento en la lista de desbordamiento".into());
    }
    checks.push("shell_many_open_documents_filtered_identity_selection");
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.workbench.select_tab(2);
        s.workbench.split_editor_right();
        s.workbench.focus_editor_pane(PaneId::Secondary);
        s.workbench.toggle_reference_pin();
        s.workbench.focus_editor_pane(PaneId::Primary);
    }
    let frame = prepared(hwnd);
    let point = frame
        .tab_hits()
        .tab_pixel(PaneId::Secondary, tab, false)
        .ok_or("Falta pestaña del segundo grupo")?;
    click(hwnd, point);
    {
        let s = owner(hwnd).unwrap().state.borrow();
        if s.workbench.panes().active() != PaneId::Secondary
            || !s
                .workbench
                .panes()
                .view(PaneId::Secondary)
                .unwrap()
                .is_pinned()
        {
            return Err("Seleccionar pestaña perdió grupo o pin".into());
        }
    }
    let frame = prepared(hwnd);
    if let Some(base) = &options.bitmap {
        paint::bitmap(hwnd, &frame, &base.with_file_name("native-shell-split.bmp"))?;
    }
    checks.push("shell_group_tab_pointer_keeps_pin_and_shared_document");
    // Closing one of two views keeps dirty source. The final dirty view still
    // uses the production confirmation path, with private injected decisions.
    let point = frame
        .tab_hits()
        .tab_pixel(PaneId::Secondary, tab, true)
        .ok_or("Falta botón cerrar")?;
    click(hwnd, point);
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        s.workbench.close_split_editor();
        s.workbench.select_tab(2);
        s.smoke_close_decisions.push_back(false);
    }
    let frame = prepared(hwnd);
    let point = frame
        .tab_hits()
        .tab_pixel(PaneId::Primary, tab, true)
        .ok_or("Falta cierre de pestaña sucia")?;
    click(hwnd, point);
    if owner(hwnd)
        .unwrap()
        .state
        .borrow()
        .workbench
        .document_by_tab(tab)
        .is_none()
    {
        return Err("Cancelar el cierre descartó el documento sucio".into());
    }
    checks.push("shell_close_shared_view_and_cancel_dirty_final_view");
    resize.set(400, 500)?;
    let frame = prepared(hwnd);
    if frame
        .tab_hits()
        .tab_pixel(PaneId::Secondary, tab, false)
        .is_some()
    {
        return Err("La vista compacta conservó una barra invisible".into());
    }
    if let Some(base) = &options.bitmap {
        paint::bitmap(
            hwnd,
            &frame,
            &base.with_file_name("native-shell-compact.bmp"),
        )?;
    }
    checks.push("shell_compact_geometry_keeps_active_document_reachable");
    {
        let mut s = owner(hwnd).unwrap().state.borrow_mut();
        while s.workbench.tabs().tabs().len() > 1 {
            let n = s.workbench.tabs().tabs().len();
            s.workbench.select_tab(n);
            s.workbench.close_active(true).map_err(|e| e.to_string())?;
        }
        s.workbench.select_tab(1);
        s.set_project_root(root.to_owned())?;
        s.explorer_visible = false;
        s.tab_scroll = Default::default();
        s.tab_hits = None;
        let d = s.workbench.active_document().ok_or("Se cerró el fixture")?;
        if d.id != initial.0 || d.session.document().revision() != initial.1 {
            return Err("Cambió el fixture original".into());
        }
    }
    expected(hwnd, b"alpha\r\nbeta\r\n")?;
    drop(resize);
    Ok(checks)
}

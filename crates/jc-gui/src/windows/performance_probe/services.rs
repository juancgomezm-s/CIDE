//! Combined services on the owned hidden HWND, with a fresh recovery depot.
use super::{cpu, memory, samples, Surface};
use crate::windows::{ffi::*, markdown_images::ImageSource, owner, project_smoke, smoke};
use crate::SmokeOptions;
use jc_app::{
    git_status::{GitStatusChange, GitStatusState},
    git_status_tasks::GitStatusTaskState,
    session::EditorRecovery,
    CommandId, ProjectSearchRow, ProjectSearchState, Workbench,
};
use std::{
    fs,
    io::Write,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SOURCE_BYTES: usize = 256 * 1024;
const IMAGE_BYTES: usize = 1024 * 1024 * 4;
const QUERY: &str = "CIDE_SERVICE_NEEDLE";
const NEEDLE_EDIT: u8 = b'Q';

fn private_depot(fixture: &Path) -> Result<(PathBuf, PathBuf), String> {
    let fixture = fixture.canonicalize().map_err(|e| e.to_string())?;
    let project = fixture.parent().ok_or("Falta la raíz de prueba")?;
    let run = project.parent().ok_or("Falta la carpeta de ejecución")?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .ok_or("Falta el espacio de trabajo")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let work = workspace.join("work");
    if run.parent() != Some(work.as_path())
        || !run.file_name().is_some_and(|name| {
            name.to_str()
                .is_some_and(|name| name.starts_with("m2-services-"))
        })
        || project.file_name().is_none_or(|name| name != "project")
    {
        return Err("La recuperación de prueba exige work/m2-services-*/project".into());
    }
    for directory in [&work, run, project] {
        let info = fs::symlink_metadata(directory).map_err(|e| e.to_string())?;
        if !info.is_dir() || info.file_attributes() & 0x400 != 0 {
            return Err("Directorio de prueba no ordinario".into());
        }
    }
    let depot = run.join("private-state");
    fs::create_dir(&depot).map_err(|e| format!("Crear depósito privado nuevo: {e}"))?;
    Ok((project.to_owned(), depot))
}

unsafe fn dispatch(hwnd: Hwnd) -> bool {
    let mut message: Msg = std::mem::zeroed();
    if PeekMessageW(&mut message, hwnd, 0, 0, 1) == 0 {
        return false;
    }
    DispatchMessageW(&message);
    true
}

unsafe fn wait_queue(milliseconds: u32) -> Result<(), String> {
    if super::MsgWaitForMultipleObjects(0, std::ptr::null(), 0, milliseconds, 0x04ff) == u32::MAX {
        return Err("No se pudo esperar la cola propia de mensajes".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Activity {
    search: ProjectSearchState,
    git: GitStatusTaskState,
    recovery_active: bool,
    confirmed: u64,
    image_bytes: usize,
}

impl Activity {
    fn has_pending_git(self) -> bool {
        matches!(
            self.git,
            GitStatusTaskState::Queued | GitStatusTaskState::Running
        )
    }

    fn overlaps(self) -> bool {
        self.recovery_active
            && self.search == ProjectSearchState::Running
            && self.has_pending_git()
            && self.image_bytes == IMAGE_BYTES
    }
}

unsafe fn observe(hwnd: Hwnd, source: &ImageSource) -> Result<Activity, String> {
    let state = owner(hwnd)
        .ok_or("Se perdió el HWND propio")?
        .state
        .borrow();
    let stats = state.periodic_recovery.statistics();
    if stats.failed != 0 {
        return Err(format!("Falló la captura periódica: {}", state.status));
    }
    let image_bytes = match state.markdown_images.get(source).map(|entry| &entry.result) {
        Some(Ok(image)) => {
            if (image.width, image.height) != (1024, 1024) {
                return Err("Dimensiones inesperadas de la imagen Markdown".into());
            }
            image.bgra.len()
        }
        Some(Err(error)) => return Err(format!("Imagen Markdown: {error}")),
        None => 0,
    };
    let git = state.git_status.state();
    if matches!(
        git,
        GitStatusTaskState::Failed | GitStatusTaskState::TimedOut | GitStatusTaskState::Limited
    ) {
        return Err(format!(
            "Consulta Git {git:?}: {:?}",
            state.git_status.failure()
        ));
    }
    let search = state.project_search.state;
    if !matches!(
        search,
        ProjectSearchState::Running | ProjectSearchState::Complete
    ) {
        return Err(format!("Consulta de proyecto {search:?}: {}", state.status));
    }
    Ok(Activity {
        search,
        git,
        recovery_active: state.periodic_recovery.active(),
        confirmed: stats.confirmed,
        image_bytes,
    })
}

fn activity_json(state: Activity, elapsed: Duration) -> String {
    format!(
        "{{\"elapsed_ms\":{:.3},\"search\":\"{:?}\",\"git\":\"{:?}\",\"recovery_active\":{},\"checkpoints\":{},\"image_bytes\":{}}}",
        elapsed.as_secs_f64() * 1000.0,
        state.search,
        state.git,
        state.recovery_active,
        state.confirmed,
        state.image_bytes,
    )
}

unsafe fn verify_services(hwnd: Hwnd) -> Result<(), String> {
    let state = owner(hwnd)
        .ok_or("Se perdió el HWND propio")?
        .state
        .borrow();
    let mut files = state
        .project_search
        .rows()
        .iter()
        .map(|row| match row {
            ProjectSearchRow::Match(hit) => hit.path.file_name().ok_or("Coincidencia sin archivo"),
            ProjectSearchRow::Issue { .. } => Err("La búsqueda encontró una incidencia"),
        })
        .collect::<Result<Vec<_>, _>>()?;
    files.sort_unstable();
    if files.len() != 64
        || !files.iter().enumerate().all(|(i, name)| {
            name.to_str()
                .is_some_and(|name| name == format!("f{i:03}.rs"))
        })
        || state.project_search.is_stale()
    {
        return Err(format!(
            "Resultados de búsqueda inesperados: {} filas",
            files.len()
        ));
    }
    let entries = state
        .git_status
        .result()
        .ok_or("Falta el estado Git")?
        .status()
        .entries();
    if entries.len() != 3 {
        return Err(format!(
            "Estado Git esperaba 3 cambios y recibió {}",
            entries.len()
        ));
    }
    for (path, index, worktree) in [
        (
            b"f001.rs",
            GitStatusChange::Unchanged,
            GitStatusChange::Modified,
        ),
        (
            b"f002.rs",
            GitStatusChange::Modified,
            GitStatusChange::Unchanged,
        ),
        (
            b"f003.rs",
            GitStatusChange::Unchanged,
            GitStatusChange::Modified,
        ),
    ] {
        if !entries.iter().any(|entry| {
            entry.path() == path && entry.state() == GitStatusState::Tracked { index, worktree }
        }) {
            return Err(format!(
                "Cambio Git inesperado para {}",
                String::from_utf8_lossy(path)
            ));
        }
    }
    Ok(())
}

pub(super) unsafe fn run(hwnd: Hwnd, options: &SmokeOptions) -> Result<(), String> {
    let (project, depot) = private_depot(&options.fixture)?;
    let image_source = ImageSource::Local(project.join("image.bmp"));
    let source = project.join("f000.rs");
    let markdown = project.join("README.md");
    let owner = owner(hwnd).ok_or("Se perdió el HWND propio")?;
    let started = Instant::now();
    let mut phases = vec![memory("sources_open")?];
    let (width, height) = {
        let state = owner.state.borrow();
        if state.workbench.tabs().tabs().len() != 5 || state.recovery.is_some() {
            return Err("Se esperaban cinco documentos y ningún depósito activo".into());
        }
        (state.width, state.height)
    };
    let surface = Surface::new(width, height)?;
    surface.paint(hwnd)?;
    phases.push(memory("editor_frame_with_probe_dib")?);
    {
        let mut state = owner.state.borrow_mut();
        state.restore_from(EditorRecovery::open_in(&depot)?)?;
        if !state.workbench.select_tab(1)
            || state.workbench.active_document().is_none_or(|doc| {
                doc.session.path().canonicalize().ok().as_deref() != Some(source.as_path())
                    || doc.session.document().len() != SOURCE_BYTES
            })
        {
            return Err("La primera pestaña debe ser f000.rs de 256 KiB".into());
        }
    }
    smoke::position(hwnd, 0, 0)?;
    SendMessageW(hwnd, WM_UNICHAR, NEEDLE_EDIT as usize, 0);
    let (dirty_revision, preview_revision) = {
        let mut state = owner.state.borrow_mut();
        let dirty = state
            .workbench
            .active_document()
            .ok_or("Falta f000.rs tras editar")?
            .session
            .document();
        if !dirty.is_modified() || dirty.len() != SOURCE_BYTES + 1 {
            return Err("No se aplicó la edición destinada a recuperación".into());
        }
        let dirty_revision = dirty.revision();
        if !state.workbench.select_tab(5) {
            return Err("No se pudo activar README.md".into());
        }
        let doc = state.workbench.active_document().ok_or("Falta README.md")?;
        if doc.session.path().canonicalize().ok().as_deref() != Some(markdown.as_path()) {
            return Err("La última pestaña debe ser README.md".into());
        }
        (dirty_revision, doc.session.document().revision())
    };
    smoke::position(hwnd, 0, 0)?;
    project_smoke::start(hwnd, QUERY, "*.rs").map_err(|e| format!("Iniciar Search Plus: {e}"))?;
    smoke::command(hwnd, CommandId::GitRepositoryStatus)?;
    smoke::command(hwnd, CommandId::ViewMarkdownPreview)?;
    surface.paint(hwnd)?;
    phases.push(memory("services_requested")?);

    let until = Instant::now() + Duration::from_secs(18);
    let mut previous = None;
    let mut timeline = Vec::new();
    let mut refreshed = false;
    let mut overlapping_states = 0;
    let mut initial_accepted = false;
    let mut redraw = Vec::with_capacity(120);
    let mut redraw_during_recovery = 0;
    let mut redraw_during_overlap = 0;
    while Instant::now() < until {
        let dispatched = dispatch(hwnd);
        let activity = observe(hwnd, &image_source)?;
        if previous != Some(activity) && timeline.len() < 128 {
            timeline.push(activity_json(activity, started.elapsed()));
            overlapping_states += usize::from(activity.overlaps());
            previous = Some(activity);
        }
        let accepted = activity.search == ProjectSearchState::Complete
            && activity.git == GitStatusTaskState::Ready
            && activity.image_bytes == IMAGE_BYTES;
        if accepted && !initial_accepted {
            verify_services(hwnd)?;
            phases.push(memory("search_git_image_accepted")?);
            initial_accepted = true;
        }
        if initial_accepted && activity.recovery_active && !refreshed {
            let mut state = owner.state.borrow_mut();
            state.start_project_search(true);
            state.schedule_git_status();
            refreshed = true;
            phases.push(memory("refresh_during_recovery")?);
        }
        if initial_accepted && (refreshed || activity.confirmed != 0) && redraw.len() < 120 {
            let before = owner.buffer.borrow().statistics().compositions;
            let start = Instant::now();
            owner.frame_dirty.set(true);
            surface.paint(hwnd)?;
            redraw.push(start.elapsed().as_secs_f64() * 1000.0);
            redraw_during_recovery += usize::from(activity.recovery_active);
            redraw_during_overlap += usize::from(activity.overlaps());
            if owner.buffer.borrow().statistics().compositions != before + 1 {
                return Err("La muestra no compuso un frame nativo".into());
            }
        } else if !dispatched {
            wait_queue(25)?;
        }
        if accepted && redraw.len() == 120 && activity.confirmed != 0 && !activity.recovery_active {
            break;
        }
    }
    let settled = observe(hwnd, &image_source)?;
    if !initial_accepted
        || settled.search != ProjectSearchState::Complete
        || settled.git != GitStatusTaskState::Ready
        || settled.image_bytes != IMAGE_BYTES
        || settled.confirmed == 0
        || settled.recovery_active
        || redraw.len() != 120
    {
        return Err(format!(
            "Servicios no concluyeron: {settled:?}, {} muestras",
            redraw.len()
        ));
    }
    verify_services(hwnd)?;
    phases.push(memory("all_results_retained")?);
    let recovery = owner
        .state
        .borrow_mut()
        .recovery
        .take()
        .ok_or("Falta el depósito al confirmar la captura")?;
    drop(recovery);
    let pixels = {
        let state = owner.state.borrow();
        let doc = state
            .workbench
            .active_document()
            .ok_or("Se cerró README.md")?;
        if doc.session.document().revision() != preview_revision || state.paint_errors != 0 {
            return Err("Cambió README.md o falló la composición".into());
        }
        let image = state
            .markdown_images
            .get(&image_source)
            .and_then(|entry| entry.result.as_ref().ok())
            .ok_or("No se retuvo la imagen aceptada")?;
        std::sync::Arc::downgrade(image)
    };
    // Replace the retained frame before dispatching another task completion.
    smoke::command(hwnd, CommandId::ViewToggleBottomPanel)?;
    surface.paint(hwnd)?;
    if pixels.upgrade().is_some() || owner.state.borrow().markdown_gui.is_some() {
        return Err("La escena de fuente conservó los píxeles de Markdown".into());
    }
    phases.push(memory("markdown_pixels_released")?);
    let stats = {
        let mut state = owner.state.borrow_mut();
        state.project_search.clear();
        state.project_panel = None;
        state.clear_git();
        state.periodic_recovery.statistics()
    };
    owner.frame_dirty.set(true);
    surface.paint(hwnd)?;
    drop(surface);
    phases.push(memory("services_cleared_probe_dib_released")?);

    let idle_start = Instant::now();
    let cpu_start = cpu()?;
    let idle_duration = Duration::from_secs(2);
    while idle_start.elapsed() < idle_duration {
        if !dispatch(hwnd) {
            let remaining = idle_duration.saturating_sub(idle_start.elapsed());
            wait_queue(remaining.as_millis().max(1) as u32)?;
        }
    }
    let idle_ms = idle_start.elapsed().as_secs_f64() * 1000.0;
    let idle_cpu_ms = cpu()?.saturating_sub(cpu_start) as f64 / 10_000.0;
    phases.push(memory("idle_after_release")?);

    // Reopen the published generation after sampling, before the smoke window
    // exits. No exit checkpoint can mask a failed periodic publication.
    let mut recovery = EditorRecovery::open_in(&depot)?;
    let (mut restored, _, used_backup) = Workbench::restore_recovery(&mut recovery.store)?
        .ok_or("No se publicó ninguna generación periódica")?;
    if used_backup || restored.tabs().tabs().len() != 5 || !restored.select_tab(1) {
        return Err("Generación periódica inesperada".into());
    }
    let doc = restored
        .active_document()
        .ok_or("Falta f000.rs restaurado")?;
    let document = doc.session.document();
    let on_disk = fs::read(&source).map_err(|e| e.to_string())?;
    let recovered = document
        .read_range(0..document.len())
        .map_err(|e| e.to_string())?;
    if doc.session.path().canonicalize().ok().as_deref() != Some(source.as_path())
        || document.revision() != dirty_revision
        || !document.is_modified()
        || on_disk.len() != SOURCE_BYTES
        || recovered.first() != Some(&NEEDLE_EDIT)
        || recovered.get(1..) != Some(on_disk.as_slice())
        || stats.chunks == 0
        || stats.max_chunk_bytes > 64 * 1024
    {
        return Err("La recuperación periódica no conservó la edición y sus límites".into());
    }
    let mut report = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&options.report)
        .map_err(|e| format!("Crear informe propio: {e}"))?;
    writeln!(report,
        "{{\"ok\":true,\"desktop_used\":false,\"personal_recovery_used\":false,\"physical_display_verified\":false,\"client_pixels\":[{width},{height}],\"probe_dib_bytes\":{},\"memory\":[{}],\"redraw_samples_ms\":[{}],\"redraws_during_recovery\":{redraw_during_recovery},\"redraws_during_overlap\":{redraw_during_overlap},\"observations\":[{}],\"refresh_during_recovery\":{refreshed},\"overlapping_controller_observations\":{overlapping_states},\"search_matches\":64,\"git_changes\":3,\"markdown_image_bytes\":{IMAGE_BYTES},\"markdown_pixels_released\":true,\"recovery_verified\":true,\"recovery_checkpoints\":{},\"recovery_chunks\":{},\"max_recovery_chunk_bytes\":{},\"idle_ms\":{idle_ms:.3},\"idle_cpu_ms\":{idle_cpu_ms:.3}}}",
        (width as usize) * (height as usize) * 4,
        phases.join(","), samples(&redraw), timeline.join(","),
        stats.confirmed, stats.chunks, stats.max_chunk_bytes,
    ).map_err(|e| e.to_string())?;
    report.sync_all().map_err(|e| e.to_string())
}
